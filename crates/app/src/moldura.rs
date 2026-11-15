//! Desenho da moldura: titlebar (titlebar.tsx), navegação (nav.tsx), painéis (App.tsx, terminal-screen.tsx,
//! shell-panel.tsx) e barra de status (status-bar.tsx).
use crate::backend::nome_da_pasta;
use crate::bancada::{
    AbrirPasta, AbrirRecente, AlternarTerminal, Bancada, Divisor, FecharPasta, Sair,
};
use crate::recursos::icone;
use crate::tema::{self, cor};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::TestSupportExt;
use gpui_kit::{
    actions, div, img, prelude::*, px, Action, Context, MouseButton,
    SharedString, Window, WindowControlArea,
};

// Itens do menu Arquivo que dependem do editor (outra fase); por ora não têm quem os trate.
actions!(orquestrador, [NovoArquivo, AbrirArquivo, Salvar, SalvarComo]);

pub use orquestrador_ui::widgets::icone;
use orquestrador_ui::widgets::{com_dica, item_menu};
use orquestrador_ui::{Aba, FaixaAbas, MoverAba};

fn menu_arquivo(
    menu: PopupMenu, recentes: Vec<String>, projeto_aberto: bool, window: &mut Window, cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let item = |m: PopupMenu, rotulo, atalho, acao: Box<dyn Action>| m.menu_element(acao, item_menu(rotulo, atalho));
    let m = menu.min_w(px(250.));
    let m = item(m, "Novo arquivo", "Ctrl+N", Box::new(NovoArquivo));
    let m = item(m, "Abrir arquivo…", "Ctrl+O", Box::new(AbrirArquivo));
    let m = item(m, "Abrir pasta…", "Ctrl+K Ctrl+O", Box::new(AbrirPasta));
    // O ícone vazio abre a coluna de ícone em todos os itens, o recuo do menu do lobehub.
    let m = m.submenu_with_icon(Some(Icon::empty()), "Projetos recentes", window, cx, move |sub, _, _| {
        if recentes.is_empty() {
            return sub.menu_with_disabled("Nenhum projeto recente", Box::new(AbrirPasta), true);
        }
        recentes.iter().fold(sub, |sub, caminho| {
            let nome = nome_da_pasta(caminho);
            let caminho_txt = caminho.clone();
            sub.menu_element(Box::new(AbrirRecente(caminho.clone())), move |_, _| {
                div()
                    .flex()
                    .flex_col()
                    .py(px(4.))
                    .text_size(tema::TEXTO_MENU)
                    .child(nome.clone())
                    .child(div().text_size(tema::TEXTO_LABEL).text_color(cor::text_tertiary()).child(caminho_txt.clone()))
            })
        })
    });
    let m = m.separator();
    let m = item(m, "Salvar", "Ctrl+S", Box::new(Salvar));
    let m = item(m, "Salvar como…", "Ctrl+Shift+S", Box::new(SalvarComo));
    let m = m.menu_element_with_disabled(Box::new(FecharPasta), !projeto_aberto, item_menu("Fechar pasta", ""));
    m.separator().menu_element(Box::new(Sair), item_menu("Sair", ""))
}

impl Render for Bancada {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.titulo());
        let arrastando = self.arrasto.map(|a| a.divisor);
        div()
            .id("bancada")
            .key_context("Bancada")
            .track_focus(&self.foco)
            .on_action(cx.listener(|this, _: &AbrirPasta, _, cx| this.escolher_pasta(cx)))
            .on_action(cx.listener(|this, _: &FecharPasta, _, cx| this.fechar_ativa(cx)))
            .on_action(cx.listener(|this, _: &AlternarTerminal, _, cx| this.alternar_terminal(cx)))
            .on_action(cx.listener(|this, a: &AbrirRecente, _, cx| this.abrir_recente(&a.0, cx)))
            .on_action(cx.listener(|this, _: &Sair, window, cx| this.sair(window, cx)))
            .on_mouse_move(cx.listener(|this, e: &gpui_kit::MouseMoveEvent, window, cx| {
                if this.arrasto.is_some() {
                    this.arrastar(e.position, window.viewport_size(), cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.soltar(cx);
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.soltar(cx);
            }))
            .when(arrastando == Some(Divisor::Terminal), |d| d.cursor_row_resize())
            .when(matches!(arrastando, Some(Divisor::Lateral | Divisor::Painel)), |d| d.cursor_col_resize())
            .size_full()
            .flex()
            .flex_col()
            .bg(cor::pitch_black())
            .text_color(cor::text_primary())
            .font_family(tema::FONTE)
            .text_size(tema::TEXTO_BODY)
            .child(self.titlebar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.nav(cx))
                    .when(self.lateral_visivel(), |d| d.child(self.lateral(cx)))
                    .child(self.centro(cx))
                    .when(self.ativa.is_some(), |d| d.child(self.painel_orquestrador(cx))),
            )
            .child(self.status_bar(cx))
    }
}

impl Bancada {
    // ---------- titlebar ----------

    fn titlebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let maximizada = window.is_maximized();
        let projeto_aberto = self.ativa.is_some();
        let recentes = self.recentes.clone();
        let foco = self.foco.clone();
        div()
            .id("titlebar")
            .flex()
            .items_center()
            .flex_none()
            .h(tema::TITLEBAR)
            .bg(cor::panel_black())
            .border_b_1()
            .border_color(cor::hairline())
            .whitespace_nowrap()
            .child(
                div()
                    .id("titlebar-marca")
                    .window_control_area(WindowControlArea::Drag)
                    .flex()
                    .items_center()
                    .h_full()
                    .px(px(10.))
                    .child(img(icone::MARCA).size(px(16.)).flex_none()),
            )
            .child(
                Button::new("menu-arquivo")
                    .ghost()
                    .xsmall()
                    .h(px(22.))
                    .px(px(8.))
                    .mr(px(4.))
                    .rounded(tema::RAIO_XS)
                    .child(div().text_size(tema::TEXTO_MENU).text_color(cor::titlebar_text()).child("Arquivo"))
                    .dropdown_menu(move |menu, window, cx| {
                        menu_arquivo(menu.action_context(foco.clone()), recentes.clone(), projeto_aberto, window, cx)
                    }),
            )
            .child(self.abas_de_projeto(cx))
            .child(
                div()
                    .id("titlebar-arrasto")
                    .window_control_area(WindowControlArea::Drag)
                    .flex_1()
                    .h_full()
                    .min_w(px(24.)),
            )
            .child(
                div()
                    .flex()
                    .h_full()
                    .flex_none()
                    .child(com_dica(
                        "dica-minimizar",
                        "Minimizar",
                        orquestrador_ui::widgets::botao_janela("janela-minimizar", icone::MINIMIZAR, WindowControlArea::Min, false),
                    ))
                    .child(com_dica(
                        "dica-maximizar",
                        if maximizada { "Restaurar" } else { "Maximizar" },
                        orquestrador_ui::widgets::botao_janela(
                            "janela-maximizar",
                            if maximizada { icone::RESTAURAR } else { icone::MAXIMIZAR },
                            WindowControlArea::Max,
                            false,
                        ),
                    ))
                    .child(com_dica(
                        "dica-fechar",
                        "Fechar",
                        orquestrador_ui::widgets::botao_janela("janela-fechar", icone::FECHAR_JANELA, WindowControlArea::Close, true),
                    )),
            )
    }

    /// Pílulas nas medidas do botão Arquivo: 26 px, raio de 6 px; a ativa clara, o X na ativa e no hover.
    fn abas_de_projeto(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let abas = self.abas.iter().map(|aba| {
            let selecionada = self.ativa.as_deref() == Some(aba.id.as_str());
            let grupo: SharedString = format!("aba-{}", aba.id).into();
            let id_sel = aba.id.clone();
            let id_fechar = aba.id.clone();
            let nome = nome_da_pasta(&aba.caminho);
            let caminho: SharedString = aba.caminho.clone().into();
            let texto = if selecionada { cor::aba_ativa_texto() } else { cor::aba_inativa_texto() };
            let elemento = div()
                .id(grupo.clone())
                .group(grupo.clone())
                .flex()
                .items_center()
                .flex_none()
                .gap(px(6.))
                .h(px(26.))
                .min_w(px(96.))
                .max_w(px(208.))
                .pl(px(8.))
                .pr(px(4.))
                .rounded(tema::RAIO_SM)
                .text_size(tema::TEXTO_LABEL)
                .when(selecionada, |d| d.bg(cor::fill_hover()).text_color(cor::aba_ativa_texto()))
                .when(!selecionada, |d| {
                    d.text_color(cor::aba_inativa_texto())
                        .hover(|s| s.bg(cor::fill_muted()).text_color(cor::titlebar_text()))
                })
                .tooltip(move |window, cx| Tooltip::new(caminho.clone()).build(window, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.selecionar(&id_sel, cx)))
                .child(div().flex_none().opacity(0.7).child(icone(icone::PASTA, 14., texto)))
                .child(div().flex_1().min_w_0().truncate().child(nome.clone()))
                .child(
                    div()
                        .id(SharedString::from(format!("fechar-aba-{}", aba.id)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_none()
                        .size(px(18.))
                        .rounded(tema::RAIO_XS)
                        .hover(|s| s.bg(cor::fill_hover()))
                        .tooltip(|window, cx| Tooltip::new("Fechar pasta").build(window, cx))
                        .when(!selecionada, |d| d.opacity(0.).group_hover(grupo.clone(), |s| s.opacity(1.)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.fechar(&id_fechar, cx);
                        }))
                        .child(icone(icone::FECHAR, 12., cor::titlebar_text()))
                        .test_support(),
                )
                .test_support();
            Aba::new(aba.id.clone(), elemento)
        });
        FaixaAbas::new("abas-projeto", &self.abas_ui)
            .flex()
            .items_center()
            .flex_shrink(1.)
            .gap(px(4.))
            .px(px(4.))
            .h_full()
            .min_w_0()
            .abas(abas.collect::<Vec<_>>())
            .ativa(self.ativa.clone())
            .estilo_arrastada(|s| s.opacity(0.8))
            .ao_mover(cx.listener(|this, m: &MoverAba, _, _| this.mover_aba(m.de, m.para)))
            .extra(
                div()
                    .id("abrir-pasta")
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .size(px(26.))
                    .rounded(tema::RAIO_SM)
                    .hover(|s| s.bg(cor::fill_hover()))
                    .tooltip(|window, cx| Tooltip::new("Abrir pasta").build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.escolher_pasta(cx)))
                    .child(icone(icone::MAIS, 16., cor::titlebar_text()))
                    .test_support(),
            )
    }

    /// Reordena as abas e grava a ordem (o `onMove` do App.tsx).
    pub fn mover_aba(&mut self, de: usize, para: usize) {
        if de >= self.abas.len() || para >= self.abas.len() || de == para {
            return;
        }
        let aba = self.abas.remove(de);
        self.abas.insert(para, aba);
        self.salvar();
    }
}
