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
    actions, deferred, div, img, point, prelude::*, px, svg, Action, AnyElement, App, Context, Hsla, MouseButton, ScrollHandle,
    SharedString, Window, WindowControlArea,
};

// Itens do menu Arquivo que dependem do editor (outra fase); por ora não têm quem os trate.
actions!(orquestrador, [NovoArquivo, AbrirArquivo, Salvar, SalvarComo]);

/// Arrasto só começa depois deste deslocamento, para o clique seguir só selecionando (`DRAG_START` de tab-strip.ts).
const ARRASTO_INICIO: f32 = 4.;
/// A menos disto da borda da faixa, ela rola sozinha 8 px por quadro (`EDGE`).
const BORDA: f32 = 24.;

/// Estado de tela da faixa de abas de projeto.
#[derive(Default)]
pub struct AbasUi {
    pub rolagem: ScrollHandle,
    pub arrasto: Option<ArrastoAba>,
    /// Aba ativa e quantidade da última revelação; mudou, rola até a ativa (`revealTab`).
    revelada: Option<(String, usize)>,
}

/// Aba segurada, com as posições de antes do arrasto (coordenadas do conteúdo da faixa), como `trackTabDrag`.
#[derive(Clone, Debug)]
pub struct ArrastoAba {
    pub de: usize,
    pub slots: Vec<(f32, f32)>,
    pub inicio_x: f32,
    pub x: f32,
    pub rolagem_inicio: f32,
    pub movendo: bool,
    pub dx: f32,
    pub alvo: usize,
}

impl ArrastoAba {
    pub fn new(de: usize, slots: Vec<(f32, f32)>, x: f32, rolagem: f32) -> Self {
        ArrastoAba { de, slots, inicio_x: x, x, rolagem_inicio: rolagem, movendo: false, dx: 0., alvo: de }
    }

    /// A rolagem conta no deslocamento; a vizinha é passada quando a borda da frente cruza o meio dela.
    pub fn atualizar(&mut self, rolagem: f32) {
        let (esq, larg) = self.slots[self.de];
        let (ult_esq, ult_larg) = self.slots[self.slots.len() - 1];
        let min = self.slots[0].0 - esq;
        let max = (ult_esq + ult_larg - esq - larg).max(min);
        self.dx = (self.x - self.inicio_x + rolagem - self.rolagem_inicio).clamp(min, max);
        let esq = esq + self.dx;
        let de = self.de;
        self.alvo = self
            .slots
            .iter()
            .enumerate()
            .filter(|&(i, &(l, w))| i != de && l + w / 2. < if i < de { esq } else { esq + larg })
            .count();
    }

    /// `dragShift`: a arrastada segue `dx`; as vizinhas entre a origem e o destino abrem o espaço dela.
    pub fn deslocamento(&self, i: usize) -> f32 {
        let larg = self.slots[self.de].1;
        if i == self.de {
            self.dx
        } else if self.de < i && i <= self.alvo {
            -larg
        } else if self.alvo <= i && i < self.de {
            larg
        } else {
            0.
        }
    }
}

/// Item do menu Arquivo como no lobehub: rótulo e atalho em cinza à direita.
fn item_menu(rotulo: &'static str, atalho: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyElement {
    move |_, _| {
        div()
            .flex()
            .flex_1()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .pl(px(8.))
            .pr(px(19.))
            .text_size(tema::TEXTO_MENU)
            .child(rotulo)
            .child(div().text_color(cor::text_tertiary()).child(atalho))
            .into_any_element()
    }
}

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

/// O botão da janela com a dica do React (`Minimizar`, `Maximizar`/`Restaurar`, `Fechar`).
fn com_dica(id: &'static str, dica: &'static str, botao: impl IntoElement) -> impl IntoElement {
    div().id(id).h_full().tooltip(move |window, cx| Tooltip::new(dica).build(window, cx)).child(botao)
}

pub(crate) fn icone(caminho: &'static str, tamanho: f32, cor: Hsla) -> gpui_kit::Svg {
    svg().path(caminho).size(px(tamanho)).flex_none().text_color(cor)
}

impl Render for Bancada {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.titulo());
        self.preparar_faixa(window);
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
                if this.abas_ui.arrasto.is_some() {
                    this.mover_ponteiro_aba(f32::from(e.position.x), cx);
                }
            }))
            .on_key_down(cx.listener(|this, e: &gpui_kit::KeyDownEvent, _, cx| {
                // Esc cancela o arrasto de aba sem chegar ao terminal.
                if e.keystroke.key == "escape" && this.abas_ui.arrasto.as_ref().is_some_and(|a| a.movendo) {
                    cx.stop_propagation();
                    this.soltar_aba(false, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.soltar(cx);
                this.soltar_aba(true, cx);
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.soltar(cx);
                this.soltar_aba(true, cx);
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
                        crate::paineis::botao_janela("janela-minimizar", icone::MINIMIZAR, WindowControlArea::Min, false),
                    ))
                    .child(com_dica(
                        "dica-maximizar",
                        if maximizada { "Restaurar" } else { "Maximizar" },
                        crate::paineis::botao_janela(
                            "janela-maximizar",
                            if maximizada { icone::RESTAURAR } else { icone::MAXIMIZAR },
                            WindowControlArea::Max,
                            false,
                        ),
                    ))
                    .child(com_dica(
                        "dica-fechar",
                        "Fechar",
                        crate::paineis::botao_janela("janela-fechar", icone::FECHAR_JANELA, WindowControlArea::Close, true),
                    )),
            )
    }

    /// Pílulas nas medidas do botão Arquivo: 26 px, raio de 6 px; a ativa clara, o X na ativa e no hover.
    fn abas_de_projeto(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut faixa = div()
            .id("abas-projeto")
            .flex()
            .items_center()
            .flex_shrink(1.)
            .gap(px(4.))
            .px(px(4.))
            .h_full()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.abas_ui.rolagem);
        let arrasto = self.abas_ui.arrasto.as_ref().filter(|a| a.movendo);
        for (indice, aba) in self.abas.iter().enumerate() {
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
                .relative()
                .when_some(arrasto, |d, a| d.left(px(a.deslocamento(indice))))
                .when(arrasto.is_some_and(|a| a.de == indice), |d| d.opacity(0.8))
                .tooltip(move |window, cx| Tooltip::new(caminho.clone()).build(window, cx))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, _| {
                    this.pressionar_aba(indice, f32::from(e.position.x));
                }))
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
            // A arrastada pinta por cima das vizinhas (o `z-10` do React).
            faixa = if arrasto.is_some_and(|a| a.de == indice) {
                faixa.child(deferred(elemento))
            } else {
                faixa.child(elemento)
            };
        }
        faixa.child(
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

    /// Rolagem atual da faixa, em px a partir do começo (o `scrollLeft`).
    fn rolagem_abas(&self) -> f32 {
        -f32::from(self.abas_ui.rolagem.offset().x)
    }

    /// Segura a aba: guarda as posições de todas como estavam no último quadro.
    pub fn pressionar_aba(&mut self, indice: usize, x: f32) {
        let r = &self.abas_ui.rolagem;
        let origem = f32::from(r.bounds().origin.x);
        let rolagem = self.rolagem_abas();
        let slots: Option<Vec<(f32, f32)>> = (0..self.abas.len())
            .map(|i| r.bounds_for_item(i).map(|b| (f32::from(b.origin.x) - origem + rolagem, f32::from(b.size.width))))
            .collect();
        self.abas_ui.arrasto = slots.filter(|s| indice < s.len()).map(|s| ArrastoAba::new(indice, s, x, rolagem));
    }

    pub fn mover_ponteiro_aba(&mut self, x: f32, cx: &mut Context<Self>) {
        let rolagem = self.rolagem_abas();
        let Some(a) = self.abas_ui.arrasto.as_mut() else { return };
        a.x = x;
        if !a.movendo {
            if (x - a.inicio_x).abs() < ARRASTO_INICIO {
                return;
            }
            a.movendo = true;
        }
        a.atualizar(rolagem);
        cx.notify();
    }

    /// Solta a aba: `confirmar` falso (Esc, janela perdeu o foco) devolve à posição original.
    pub fn soltar_aba(&mut self, confirmar: bool, cx: &mut Context<Self>) {
        let Some(a) = self.abas_ui.arrasto.take() else { return };
        if a.movendo {
            if confirmar && a.alvo != a.de {
                self.mover_aba(a.de, a.alvo);
            }
            cx.notify();
        }
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

    /// Antes de desenhar: cancela arrasto com a janela inativa, rola na borda durante o arrasto e revela a aba ativa.
    fn preparar_faixa(&mut self, window: &mut Window) {
        if self.abas_ui.arrasto.is_some() && !window.is_window_active() {
            self.abas_ui.arrasto = None;
        }
        if let Some(x) = self.abas_ui.arrasto.as_ref().filter(|a| a.movendo).map(|a| a.x) {
            let r = &self.abas_ui.rolagem;
            let caixa = r.bounds();
            let passo = if x < f32::from(caixa.left()) + BORDA {
                -8.
            } else if x > f32::from(caixa.right()) - BORDA {
                8.
            } else {
                0.
            };
            if passo != 0. {
                let max = f32::from(r.max_offset().x);
                let antes = self.rolagem_abas();
                let nova = (antes + passo).clamp(0., max.max(0.));
                if nova != antes {
                    r.set_offset(point(px(-nova), r.offset().y));
                    if let Some(a) = self.abas_ui.arrasto.as_mut() {
                        a.atualizar(nova);
                    }
                }
                window.request_animation_frame();
            }
        }
        let chave = self.ativa.clone().map(|id| (id, self.abas.len()));
        if chave != self.abas_ui.revelada {
            if let Some(i) = self.ativa.as_ref().and_then(|id| self.abas.iter().position(|a| &a.id == id)) {
                self.abas_ui.rolagem.scroll_to_item(i);
            }
            self.abas_ui.revelada = chave;
        }
    }
}
