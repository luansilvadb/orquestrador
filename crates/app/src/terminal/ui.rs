//! Peças pequenas dos painéis de terminal: ícone, botões do Lobe UI (primário, padrão, texto) e etiqueta de estado.
use crate::tema::{self, cor};
use gpui_kit::{div, prelude::*, px, svg, Div, Hsla, SharedString, Stateful, Svg};

pub fn icone(caminho: &'static str, tamanho: f32, cor: Hsla) -> Svg {
    svg().path(caminho).size(px(tamanho)).flex_none().text_color(cor)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variante {
    Primario,
    Padrao,
}

/// Botão de 32 px do Lobe UI. `ocupado` desabilita e mostra o giro.
pub fn botao(id: &'static str, rotulo: impl Into<SharedString>, variante: Variante, ocupado: bool) -> Stateful<Div> {
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .flex_none()
        .h(tema::CAMPO)
        .px(px(12.))
        .rounded(tema::RAIO_SM)
        .text_size(tema::TEXTO_BODY)
        .when(ocupado, |d| d.opacity(0.6).child(icone("icons/loader-circle.svg", 14., cor::text_secondary())));
    let base = match variante {
        Variante::Primario => base
            .bg(cor::ink_white())
            .text_color(cor::pitch_black())
            .when(!ocupado, |d| d.hover(|s| s.bg(cor::ink_white_hover())).active(|s| s.bg(cor::ink_white_active()))),
        Variante::Padrao => base
            .bg(cor::pitch_black())
            .border_1()
            .border_color(gpui_kit::rgba(0xffffff26))
            .text_color(cor::text_primary())
            .when(!ocupado, |d| d.hover(|s| s.border_color(gpui_kit::rgba(0xffffff4d)))),
    };
    base.child(rotulo.into())
}

/// Botão pequeno (24 px) com borda, como o `size="small"` padrão.
pub fn botao_pequeno(id: impl Into<gpui_kit::ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .flex_none()
        .h(px(24.))
        .px(px(7.))
        .rounded(tema::RAIO_XS)
        .bg(cor::pitch_black())
        .border_1()
        .border_color(gpui_kit::rgba(0xffffff26))
        .text_size(tema::TEXTO_LABEL)
        .hover(|s| s.border_color(gpui_kit::rgba(0xffffff4d)))
}

/// Passo do giro: 30 passos por volta de 1,5 s (o `--animate-spin` do src/index.css), 20 quadros/s.
pub const PASSO_GIRO: std::time::Duration = std::time::Duration::from_millis(50);

/// Ícone do estado do executor; o de "executando" gira em passos, pelo relógio comum (quem redesenha é o painel, a 20 Hz).
pub fn icone_estado(estado: &str, caminho: &'static str, tamanho: f32, cor: Hsla) -> Svg {
    static INICIO: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let svg = icone(caminho, tamanho, cor);
    if estado != "running" {
        return svg;
    }
    let passo = (INICIO.get_or_init(std::time::Instant::now).elapsed().as_millis() / PASSO_GIRO.as_millis()) % 30;
    svg.with_transformation(gpui_kit::Transformation::rotate(gpui_kit::percentage(passo as f32 / 30.)))
}

/// Ícone sem borda de 24 px (`ActionIcon size="small" variant="borderless"`).
pub fn acao(id: impl Into<gpui_kit::ElementId>, caminho: &'static str, habilitada: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .size(px(24.))
        .rounded(tema::RAIO_SM)
        .when(habilitada, |d| d.hover(|s| s.bg(cor::fill_hover())))
        .when(!habilitada, |d| d.opacity(0.35))
        .child(icone(caminho, 14., cor::text_secondary()))
}
