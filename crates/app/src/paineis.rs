//! Navegação (nav.tsx), faixa lateral, coluna central, painéis Terminal e Orquestrador e barra de status (status-bar.tsx).
use crate::bancada::{Bancada, Divisor, Rota};
use crate::moldura::icone;
use crate::recursos::icone as ic;
use crate::tema::{self, cor};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::Sizable;
use gpui_kit::TestSupportExt;
use gpui_kit::{div, prelude::*, px, Context, FontWeight, MouseButton, WindowControlArea};

/// Controle da janela: 46 px, hover branco a 10% (Fechar vermelho com o ícone branco), sem estado ativo. A área nativa
/// (`WindowControlArea`) dá ao Windows o clique, o snap do Maximizar e o duplo clique.
pub fn botao_janela(id: &'static str, caminho: &'static str, area: WindowControlArea, perigo: bool) -> impl IntoElement {
    div()
        .id(id)
        .group(id)
        .window_control_area(area)
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .w(tema::BOTAO_JANELA)
        .h_full()
        .when(perigo, |d| d.hover(|s| s.bg(cor::close_red())))
        .when(!perigo, |d| d.hover(|s| s.bg(cor::fill_hover())))
        .child(
            icone(caminho, 16., cor::titlebar_text())
                .when(perigo, |s| s.group_hover(id, |s| s.text_color(cor::text_primary()))),
        )
        .test_support()
}

/// Título de seção da faixa lateral: 36 px, 12 px a 500 em texto secundário (explorer.tsx).
fn cabecalho(titulo: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .flex_none()
        .h(tema::FAIXA)
        .px(px(12.))
        .border_b_1()
        .border_color(cor::hairline())
        .text_size(tema::TEXTO_LABEL)
        .font_weight(FontWeight::MEDIUM)
        .text_color(cor::text_secondary())
        .child(titulo)
}

/// Alça de arrasto de 4 px sobre a borda do painel.
fn alca(id: &'static str, divisor: Divisor, cx: &mut Context<Bancada>) -> impl IntoElement {
    let base = div().id(id).absolute().occlude();
    let base = match divisor {
        Divisor::Lateral => base.top_0().bottom_0().right(px(-2.)).w(px(4.)).cursor_col_resize(),
        Divisor::Painel => base.top_0().bottom_0().left(px(-2.)).w(px(4.)).cursor_col_resize(),
        Divisor::Terminal => base.left_0().right_0().top(px(-2.)).h(px(4.)).cursor_row_resize(),
    };
    base.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.comecar_arrasto(divisor, e.position);
        }),
    )
}

/// Faixa de abas de 36 px no estilo `variant="square"`: linha de 2 px sob a ativa e divisor de 1 px em toda a largura.
fn faixa_de_abas(rotulo: &'static str) -> impl IntoElement {
    div()
        .relative()
        .flex()
        .flex_none()
        .h(tema::FAIXA)
        .px(px(4.))
        .child(div().absolute().left_0().right_0().bottom_0().h(px(1.)).bg(cor::hairline()))
        .child(
            div()
                .relative()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .h_full()
                .text_size(tema::TEXTO_MENU)
                .font_weight(FontWeight::MEDIUM)
                .child(icone(ic::TERMINAL, 14., cor::text_primary()))
                .child(rotulo)
                .child(div().absolute().left_0().right_0().bottom_0().h(px(2.)).bg(cor::ink_white())),
        )
}

fn convite_abrir_pasta(descricao: &'static str, cx: &mut Context<Bancada>) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .p(px(16.))
        .child(div().text_size(tema::TEXTO_MENU).text_color(cor::text_secondary()).text_center().child(descricao))
        .child(
            Button::new("abrir-pasta-convite")
                .primary()
                .small()
                .label("Abrir pasta")
                .on_click(cx.listener(|this, _, _, cx| this.escolher_pasta(cx))),
        )
}

/// Item da barra de status: 22 px, 5 px dos lados, 12 px de texto, hover com o preenchimento.
fn item_status(id: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .flex_none()
        .gap(px(4.))
        .px(px(5.))
        .h_full()
        .hover(|s| s.bg(cor::fill_muted()))
}

impl Bancada {
    pub(crate) fn nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let alterados = self.git_ativo().map_or(0, |g| g.alterados);
        div()
            .id("nav")
            .flex()
            .flex_col()
            .items_center()
            .flex_none()
            .w(tema::NAV)
            .gap(px(4.))
            .py(px(8.))
            .bg(cor::panel_black())
            .border_r_1()
            .border_color(cor::hairline())
            .child(self.botao_nav("nav-arquivos", Rota::Arquivos, ic::ARQUIVOS, 0, cx))
            .child(self.botao_nav("nav-pesquisa", Rota::Pesquisa, ic::PESQUISAR, 0, cx))
            .child(self.botao_nav("nav-git", Rota::Git, ic::GIT, alterados, cx))
    }

    /// Botão de 36 px com ícone de 20 px; o ativo tem fundo e filete de 2 px à esquerda, encostado na borda.
    fn botao_nav(
        &self, id: &'static str, rota: Rota, caminho: &'static str, badge: usize, cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ativo = self.lateral_visivel() && self.rota == rota;
        div()
            .relative()
            .flex()
            .justify_center()
            .w_full()
            .when(ativo, |d| {
                d.child(div().absolute().left_0().top(px(8.)).bottom(px(8.)).w(px(2.)).rounded_full().bg(cor::ink_white()))
            })
            .child(
                div()
                    .id(id)
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(tema::BOTAO)
                    .rounded(tema::RAIO_SM)
                    .when(ativo, |d| d.bg(cor::fill_hover()))
                    .when(!ativo, |d| d.hover(|s| s.bg(cor::fill_muted())))
                    .on_click(cx.listener(move |this, _, _, cx| this.ir(rota, cx)))
                    .child(icone(caminho, 20., cor::text_tertiary()))
                    .when(badge > 0, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(19.))
                                .right(px(5.))
                                .min_w(px(12.))
                                .h(px(12.))
                                .px(px(3.))
                                .rounded(tema::RAIO_XS)
                                .bg(cor::badge_git())
                                .text_color(cor::text_primary())
                                .text_size(px(10.))
                                .line_height(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .flex()
                                .justify_center()
                                .child(badge.to_string()),
                        )
                    })
                    .test_support(),
            )
    }

    pub(crate) fn lateral(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, titulo, convite) = match self.rota {
            Rota::Arquivos => ("tela-arquivos", "Arquivos", "Abra uma pasta para explorar os arquivos."),
            Rota::Pesquisa => ("tela-pesquisa", "Pesquisar", "Abra uma pasta para pesquisar nos arquivos."),
            Rota::Git => ("tela-git", "Git", "Abra uma pasta para usar o Git."),
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            // A linha do DraggablePanel fica fora dos 280 px.
            .w(px(self.largura_lateral + 1.))
            .h_full()
            .bg(cor::pitch_black())
            .border_r_1()
            .border_color(cor::hairline_soft())
            .child(
                div()
                    .id(id)
                    .flex()
                    .flex_col()
                    .size_full()
                    .when(self.ativa.is_some(), |d| d.child(cabecalho(titulo)))
                    .when(self.ativa.is_none(), |d| d.child(convite_abrir_pasta(convite, cx)))
                    .test_support(),
            )
            .child(alca("alca-lateral", Divisor::Lateral, cx))
    }

    pub(crate) fn centro(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let projeto = self.ativa.is_some();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    // Faixa das abas do editor (30 px, medidas do Antigravity), vazia nesta fase.
                    .when(projeto, |d| d.child(div().flex_none().h(px(30.)).border_b_1().border_color(cor::hairline())))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .child(icone(ic::MARCA_MONO, 240., cor::marca_dagua())),
                    ),
            )
            .when(self.terminal_aberto, |d| d.child(self.painel_terminal(cx)))
    }

    fn painel_terminal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("painel-terminal")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h(px(self.altura_terminal))
            .bg(cor::painel_arrastavel())
            .border_t_1()
            .border_color(cor::hairline_soft())
            .child(faixa_de_abas("Terminal"))
            .child(div().flex_1())
            .child(alca("alca-terminal", Divisor::Terminal, cx))
            .test_support()
    }

    pub(crate) fn painel_orquestrador(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("painel-orquestrador")
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(self.largura_painel))
            .h_full()
            .bg(cor::painel_arrastavel())
            .border_l_1()
            .border_color(cor::hairline_soft())
            .child(faixa_de_abas("Orquestrador"))
            .child(
                div().flex_1().min_h_0().flex().items_center().justify_center().child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .p(px(16.))
                        .w(px(380.))
                        .max_w_full()
                        .child(
                            Button::new("abrir-terminal")
                                .primary()
                                .h(tema::CAMPO)
                                .px(px(12.))
                                .child(div().text_size(tema::TEXTO_BODY).child("Abrir terminal")),
                        )
                        .child(
                            div()
                                .text_size(tema::TEXTO_LABEL)
                                .text_color(cor::text_secondary())
                                .text_center()
                                .child("Abre o orquestrador, com quem você conversa. Os executores aparecem em abas quando ele delega."),
                        ),
                ),
            )
            .child(alca("alca-painel", Divisor::Painel, cx))
            .test_support()
    }

    pub(crate) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let git = self.git_ativo().cloned();
        div()
            .id("status-bar")
            .flex()
            .flex_none()
            .h(tema::STATUS_BAR)
            .px(px(3.))
            .bg(cor::panel_black())
            .border_t_1()
            .border_color(cor::hairline())
            .text_size(tema::TEXTO_LABEL)
            .when_some(git, |d, git| {
                let contagem = (git.ahead > 0 || git.behind > 0).then(|| format!("{}↓ {}↑", git.behind, git.ahead));
                d.child(
                    item_status("status-branch")
                        .flex_shrink(1.)
                        .min_w_0()
                        .on_click(cx.listener(|this, _, _, cx| this.ir(Rota::Git, cx)))
                        .child(div().truncate().child(git.branch.clone())),
                )
                .child(
                    item_status("status-sync")
                        .child(icone(ic::SINCRONIZAR, 14., cor::text_primary()))
                        .when_some(contagem, |d, c| d.child(c)),
                )
            })
            .child(
                item_status("status-terminal")
                    .ml_auto()
                    .when(self.terminal_aberto, |d| d.bg(cor::fill_hover()))
                    .on_click(cx.listener(|this, _, _, cx| this.alternar_terminal(cx)))
                    .child(icone(ic::TERMINAL, 14., cor::text_primary()))
                    .child("Terminal")
                    .test_support(),
            )
    }
}
