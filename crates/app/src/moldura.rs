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

// Itens do menu Arquivo que vão ao editor do projeto ativo.
actions!(orquestrador, [NovoArquivo, AbrirArquivo, Salvar, SalvarComo]);

pub use orquestrador_ui::widgets::icone;
use orquestrador_ui::widgets::{item_menu, TooltipTexto as _};
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
        crate::medir::quadro(window, cx);
        self.garantir_telas(window, cx);
        self.drenar_pendentes(window, cx);
        self.aplicar_telas(window, cx);
        let arrastando = self.arrasto.map(|a| a.divisor);
        let dialogo = self.dialogo_fechamento(window, cx);
        let dica = self.dica_el(window, cx);
        div()
            .id("bancada")
            .key_context("Bancada")
            .track_focus(&self.foco)
            .capture_key_down(cx.listener(Self::tecla_global))
            .on_action(cx.listener(|this, _: &AbrirPasta, _, cx| this.escolher_pasta(cx)))
            .on_action(cx.listener(|this, _: &FecharPasta, window, cx| this.fechar_ativa(window, cx)))
            .on_action(cx.listener(|this, _: &AlternarTerminal, _, cx| this.alternar_terminal(cx)))
            .on_action(cx.listener(|this, _: &NovoArquivo, window, cx| this.no_editor(window, cx, |e, w, cx| e.novo_arquivo(w, cx))))
            .on_action(cx.listener(|this, _: &AbrirArquivo, _, cx| this.escolher_arquivo(cx)))
            .on_action(cx.listener(|this, _: &Salvar, window, cx| this.no_editor(window, cx, |e, w, cx| e.salvar_ativa(w, cx))))
            .on_action(cx.listener(|this, _: &SalvarComo, window, cx| this.no_editor(window, cx, |e, w, cx| e.salvar_ativa_como(w, cx))))
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
                    .when(self.ativa.is_some(), |d| d.child(self.painel_orquestrador(window, cx))),
            )
            .child(self.status_bar(cx))
            .children(dialogo)
            .children(dica)
            .child(orquestrador_ui::avisos::camada(window, cx))
    }
}

impl Bancada {
    /// Uma operação no editor à vista (o do projeto ativo ou o sem projeto).
    fn no_editor(
        &mut self, window: &mut Window, cx: &mut Context<Self>,
        f: impl FnOnce(&mut orquestrador_editor::AreaEditor, &mut Window, &mut Context<orquestrador_editor::AreaEditor>),
    ) {
        if let Some(ed) = self.editor_ativo() {
            ed.update(cx, |e, cx| f(e, window, cx));
        }
    }

    /// Atalhos de janela num listener só, em captura (antes dos terminais), como o `windowKey` do App.tsx.
    fn tecla_global(&mut self, e: &gpui_kit::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.fechamento.is_some() {
            return;
        }
        let k = &e.keystroke;
        let m = k.modifiers;
        let tecla = k.key.as_str();
        let ctrl = m.control && !m.alt && !m.platform;
        let so_ctrl = ctrl && !m.shift;
        let no_terminal = self.terminais.read(cx).foco_em_terminal(window, cx);
        let pronto = self.terminais.read(cx).pronto(cx);
        let projeto = self.ativa.is_some() && pronto;
        let consumir = |cx: &mut Context<Self>| cx.stop_propagation();
        if pronto && !no_terminal {
            if let Some(t) = self.acorde_k.take() {
                if !matches!(tecla, "control" | "shift" | "alt" | "platform")
                    && t.elapsed() < std::time::Duration::from_millis(1500)
                    && so_ctrl
                    && tecla == "o"
                {
                    consumir(cx);
                    self.escolher_pasta(cx);
                    return;
                }
            } else if so_ctrl && tecla == "k" {
                self.acorde_k = Some(std::time::Instant::now());
            }
            match (so_ctrl, ctrl && m.shift, tecla) {
                (true, _, "n") => {
                    consumir(cx);
                    self.no_editor(window, cx, |e, w, cx| e.novo_arquivo(w, cx));
                    return;
                }
                (true, _, "o") => {
                    consumir(cx);
                    self.escolher_arquivo(cx);
                    return;
                }
                (true, _, "s") => {
                    consumir(cx);
                    self.no_editor(window, cx, |e, w, cx| e.salvar_ativa(w, cx));
                    return;
                }
                (_, true, "s") => {
                    consumir(cx);
                    self.no_editor(window, cx, |e, w, cx| e.salvar_ativa_como(w, cx));
                    return;
                }
                _ => {}
            }
        }
        // Ctrl+R e F5 recarregariam a webview no React; aqui só são consumidos, como lá.
        if (ctrl && tecla == "r") || (tecla == "f5" && !m.alt && !m.platform) {
            consumir(cx);
            return;
        }
        if ctrl && tecla_crase(k) {
            consumir(cx);
            self.alternar_terminal(cx);
            return;
        }
        if ctrl && m.shift && tecla == "f" && projeto {
            consumir(cx);
            self.abrir_pesquisa(window, cx);
            return;
        }
        if tecla == "f4" && !m.control && !m.alt && !m.platform && projeto && !no_terminal {
            consumir(cx);
            self.passo_pesquisa(if m.shift { -1 } else { 1 }, cx);
            return;
        }
        // Como no React (`input, textarea`, o que inclui a área de texto do Monaco): num campo de texto a tecla segue adiante.
        if so_ctrl && tecla == "p" && projeto && !no_terminal && !gpui_kit::component::WindowExt::has_focused_input(window, cx) {
            consumir(cx);
            self.no_editor(window, cx, |e, w, cx| e.abrir_rapido(w, cx));
        }
    }

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
            // A faixa inteira arrasta a janela (o `data-tauri-drag-region` do React): a área de arrasto fica atrás das
            // pílulas, que bloqueiam o mouse para não virarem barra de título.
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .id("titlebar-arrasto")
                            .window_control_area(WindowControlArea::Drag)
                            .absolute()
                            .inset_0(),
                    )
                    .child(self.abas_de_projeto(cx)),
            )
            .child(
                div()
                    .flex()
                    .h_full()
                    .flex_none()
                    .child(self.botao_com_dica(
                        2,
                        "Minimizar",
                        orquestrador_ui::widgets::botao_janela("janela-minimizar", icone::MINIMIZAR, WindowControlArea::Min, false),
                        cx,
                    ))
                    .child(self.botao_com_dica(
                        1,
                        if maximizada { "Restaurar" } else { "Maximizar" },
                        orquestrador_ui::widgets::botao_janela(
                            "janela-maximizar",
                            if maximizada { icone::RESTAURAR } else { icone::MAXIMIZAR },
                            WindowControlArea::Max,
                            false,
                        ),
                        cx,
                    ))
                    .child(self.botao_com_dica(
                        0,
                        "Fechar",
                        orquestrador_ui::widgets::botao_janela("janela-fechar", icone::FECHAR_JANELA, WindowControlArea::Close, true),
                        cx,
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
            let ocupacao = self.ocupacao(&aba.id, cx);
            let rolagem = self.abas_ui.rolagem().clone();
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
                .occlude()
                .on_scroll(move |e, window, cx| rolar_faixa(&rolagem, e, window, cx))
                .tooltip_texto(caminho)
                .on_click(cx.listener(move |this, _, _, cx| this.selecionar(&id_sel, cx)))
                .child(div().flex_none().opacity(0.7).child(icone(icone::PASTA, 14., texto)))
                .child(div().flex_1().min_w_0().truncate().child(nome.clone()))
                .when(ocupacao > 0, |d| {
                    let dica: SharedString = format!("{ocupacao} executores ativos").into();
                    d.child(
                        div()
                            .id(SharedString::from(format!("ocupado-{}", aba.id)))
                            .flex_none()
                            .size(px(6.))
                            .rounded_full()
                            .bg(cor::signal_warning())
                            .tooltip_texto(dica)
                            .test_support(),
                    )
                })
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
                        .tooltip_texto("Fechar pasta")
                        .when(!selecionada, |d| d.opacity(0.).group_hover(grupo.clone(), |s| s.opacity(1.)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.fechar(&id_fechar, window, cx);
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
            .max_w_full()
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
                    .occlude()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .size(px(26.))
                    .rounded(tema::RAIO_SM)
                    .hover(|s| s.bg(cor::fill_hover()))
                    .tooltip_texto("Abrir pasta")
                    .on_click(cx.listener(|this, _, _, cx| this.escolher_pasta(cx)))
                    .child(icone(icone::MAIS, 16., cor::titlebar_text()))
                    .test_support(),
            )
    }

    /// Botão da janela com a dica alinhada à direita, embaixo (o `placement="bottomRight"` do React). `coluna` conta da
    /// borda direita (0 é o Fechar).
    fn botao_com_dica(
        &self, coluna: u8, dica: &'static str, botao: impl IntoElement, cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("dica-janela-{coluna}")))
            .h_full()
            .on_hover(cx.listener(move |this, dentro: &bool, window, cx| {
                this.dica_janela = None;
                this.dica_tarefa = None;
                if *dentro {
                    this.dica_tarefa = Some(cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                        let _ = this.update(cx, |this, cx| {
                            this.dica_janela = Some((dica, coluna));
                            cx.notify();
                        });
                    }));
                }
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.dica_janela = None;
                this.dica_tarefa = None;
                cx.notify();
            }))
            .child(botao)
    }

    /// A dica dos botões da janela: 4 px abaixo da titlebar e 4 px para dentro da borda direita do botão, como no React.
    pub(crate) fn dica_el(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let (texto, coluna) = self.dica_janela?;
        let direita = window.viewport_size().width - tema::BOTAO_JANELA * coluna as f32 - px(4.);
        // Sem a margem de 12 px que o Kit põe em volta da dica (ela deslocaria a caixa).
        let dica = Tooltip::new(texto).m_0().build(window, cx);
        Some(
            gpui_kit::deferred(
                gpui_kit::anchored()
                    .anchor(gpui_kit::Anchor::TopRight)
                    .position(gpui_kit::point(direita, tema::TITLEBAR + px(4.)))
                    .child(div().id("dica-janela").child(dica).test_support()),
            )
            .with_priority(3)
            .into_any_element(),
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

/// A tecla física à esquerda do 1 (o `e.code === 'Backquote'` do React): no ABNT2 ela produz `'`, então o atalho
/// confere o scancode 0x29 em vez do caractere.
fn tecla_crase(k: &gpui_kit::Keystroke) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, MapVirtualKeyW, MAPVK_VSC_TO_VK_EX};
        // SAFETY: chamadas sem ponteiros; o estado vem da fila de mensagens da thread da janela.
        let vk = unsafe { MapVirtualKeyW(0x29, MAPVK_VSC_TO_VK_EX) };
        if vk != 0 && unsafe { GetKeyState(vk as i32) } < 0 {
            return true;
        }
    }
    k.key == "`"
}

/// Roda do mouse sobre a pílula rola a faixa na horizontal (o `wheelToScroll` do React).
fn rolar_faixa(rolagem: &gpui_kit::ScrollHandle, e: &gpui_kit::ScrollEvent, window: &mut Window, cx: &mut gpui_kit::App) {
    let d = e.delta.pixel_delta(window.line_height());
    let passo = if d.x.abs() > d.y.abs() { d.x } else { d.y };
    let max = rolagem.max_offset().x;
    let mut o = rolagem.offset();
    o.x = (o.x + passo).clamp(-max, px(0.));
    rolagem.set_offset(o);
    cx.stop_propagation();
    window.refresh();
}
