//! Navegação (nav.tsx), faixa lateral, coluna central, painéis Terminal e Orquestrador e barra de status (status-bar.tsx).
use crate::bancada::{Bancada, Divisor, Rota};
use crate::moldura::icone;
use orquestrador_ui::widgets::{Borda, TooltipTexto as _};
use crate::recursos::icone as ic;
use crate::tema::{self, cor};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::Sizable;
use gpui_kit::TestSupportExt;
use gpui_kit::{div, prelude::*, px, Context, FontWeight, MouseButton, Window};

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
    let borda = match divisor {
        Divisor::Lateral => Borda::Direita,
        Divisor::Painel => Borda::Esquerda,
        Divisor::Terminal => Borda::Topo,
    };
    orquestrador_ui::widgets::alca(id, borda).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.comecar_arrasto(divisor, e.position);
        }),
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
        let alterados = self.telas_ativas().and_then(|t| t.resumo()).map_or(0, |g| g.alterados);
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
        let telas = self.telas_ativas();
        let conteudo: Option<gpui_kit::AnyElement> = telas.map(|t| match self.rota {
            Rota::Arquivos => t.arvore.clone().into_any_element(),
            Rota::Pesquisa => t.pesquisa.clone().into_any_element(),
            Rota::Git => t.git.clone().into_any_element(),
        });
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
                    .when_some(conteudo, |d, c| d.child(div().flex_1().min_h_0().child(c)))
                    .when(self.ativa.is_some() && telas.is_none(), |d| d.child(cabecalho(titulo)))
                    .when(self.ativa.is_none(), |d| d.child(convite_abrir_pasta(convite, cx)))
                    .test_support(),
            )
            .child(alca("alca-lateral", Divisor::Lateral, cx))
    }

    pub(crate) fn centro(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // O editor do projeto ativo (ou o sem projeto): abas, grupos, diffs, conflitos, mídia e a marca d'água vazia.
        let editor = self.editor_ativo();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                div()
                    .id("area-editor")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .when_some(editor, |d, e| d.child(e))
                    .test_support(),
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
            .child(self.conteudo_terminal(cx))
            .child(alca("alca-terminal", Divisor::Terminal, cx))
            .test_support()
    }

    /// Visibilidade dos terminais e, com projeto aberto, o fim dos shells sem projeto.
    fn ajustar_terminais(&self, cx: &mut Context<Self>) {
        let (ativa, aberto) = (self.ativa.clone(), self.terminal_aberto);
        self.terminais.update(cx, |t, cx| {
            if ativa.is_some() {
                t.soltar_sem_projeto(cx);
            }
            t.marcar_visiveis(&ativa, aberto, cx);
        });
    }

    fn conteudo_terminal(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let (ativa, raiz) = (self.ativa.clone(), self.aba_ativa().map(|a| a.caminho.clone()));
        let el = self.terminais.update(cx, |t, cx| {
            // Painel aberto sem shell (reaberto da sessão anterior): nasce o primeiro.
            if t.projeto(&ativa).shells.is_empty() {
                t.adicionar_shell(ativa.clone(), raiz.clone(), cx);
            }
            t.painel_terminal(ativa, raiz, cx)
        });
        self.ajustar_terminais(cx);
        el
    }

    fn conteudo_orquestrador(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let (ativa, raiz) = (self.ativa.clone(), self.aba_ativa().map(|a| a.caminho.clone()));
        let el = self.terminais.update(cx, |t, cx| t.painel_orquestrador(ativa, raiz, window, cx));
        self.ajustar_terminais(cx);
        el
    }

    pub(crate) fn painel_orquestrador(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(self.conteudo_orquestrador(window, cx))
            .child(alca("alca-painel", Divisor::Painel, cx))
            .test_support()
    }

    pub(crate) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let git = self.telas_ativas().and_then(|t| t.resumo());
        let sincronizando = self.telas_ativas().is_some_and(|t| t.sincronizando);
        // O portão de abertura (terminal/portao.rs) cobre a janela enquanto o uso não foi liberado.
        let abertura = self.terminais.read(cx).abertura.clone();
        let bloqueada = !abertura.read(cx).pronto();
        div()
            .when(bloqueada, |d| d.child(abertura))
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
                        .tooltip_texto(format!("{}, branch atual", git.branch))
                        .child(div().truncate().child(git.branch.clone())),
                )
                .child({
                    let dica: gpui_kit::SharedString = if sincronizando {
                        "Sincronizando…".into()
                    } else {
                        match &contagem {
                            Some(c) => format!("Sincronizar as alterações ({c})").into(),
                            None => "Sincronizar as alterações".into(),
                        }
                    };
                    item_status("status-sync")
                        .when(sincronizando, |d| d.hover(|s| s.bg(gpui_kit::transparent_black())))
                        .tooltip_texto(dica)
                        .when(!sincronizando, |d| d.on_click(cx.listener(|this, _, window, cx| this.sincronizar(window, cx))))
                        .child(icone(ic::SINCRONIZAR, 14., cor::text_primary()))
                        .when_some(contagem, |d, c| d.child(c))
                        .test_support()
                })
            })
            .child(
                item_status("status-terminal")
                    .ml_auto()
                    .when(self.terminal_aberto, |d| d.bg(cor::fill_hover()))
                    .on_click(cx.listener(|this, _, _, cx| this.alternar_terminal(cx)))
                    .tooltip_texto("Painel Terminal (Ctrl+`)")
                    .child(icone(ic::TERMINAL, 14., cor::text_primary()))
                    .child("Terminal")
                    .test_support(),
            )
    }
}
