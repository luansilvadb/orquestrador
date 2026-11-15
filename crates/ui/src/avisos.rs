//! Avisos curtos (o `toast` do Lobe UI em `src/main.tsx` e `.app-toasts` de `src/index.css`): no alto e no meio da
//! janela, com o topo a 43 px (o deslocamento de 44 px do CSS, como sai na captura), largura do texto até 560 px, fundo
//! `raised_black`, borda de 1 px, raio de 8 px, 8 px por 12 px de respiro e o ícone do tipo a 8 px do título de 13 px.
//! Os mais novos ficam embaixo, 8 px entre eles. Qualquer tela chama [`mostrar`]; a bancada desenha [`camada`] uma vez,
//! por cima de tudo.
use crate::tema::{self, cor};
use crate::widgets::icone;
use gpui_kit::{div, prelude::*, px, AnyElement, App, FontWeight, Global, Hsla, SharedString, Window};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoAviso {
    Sucesso,
    Info,
    Alerta,
    Erro,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Aviso {
    pub tipo: TipoAviso,
    pub titulo: SharedString,
    pub duracao: Duration,
}

impl Aviso {
    pub fn new(tipo: TipoAviso, titulo: impl Into<SharedString>, ms: u64) -> Self {
        Aviso { tipo, titulo: titulo.into(), duracao: Duration::from_millis(ms) }
    }
    pub fn sucesso(titulo: impl Into<SharedString>, ms: u64) -> Self {
        Self::new(TipoAviso::Sucesso, titulo, ms)
    }
    pub fn info(titulo: impl Into<SharedString>, ms: u64) -> Self {
        Self::new(TipoAviso::Info, titulo, ms)
    }
    pub fn alerta(titulo: impl Into<SharedString>, ms: u64) -> Self {
        Self::new(TipoAviso::Alerta, titulo, ms)
    }
    pub fn erro(titulo: impl Into<SharedString>, ms: u64) -> Self {
        Self::new(TipoAviso::Erro, titulo, ms)
    }
}

#[derive(Default)]
struct Fila {
    proximo: u64,
    itens: Vec<(u64, Aviso)>,
}

impl Global for Fila {}

/// Mostra o aviso pelo tempo dele, em todas as janelas que desenham a [`camada`].
pub fn mostrar(aviso: Aviso, cx: &mut App) {
    let fila = cx.default_global::<Fila>();
    let id = fila.proximo;
    fila.proximo += 1;
    let duracao = aviso.duracao;
    fila.itens.push((id, aviso));
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(duracao).await;
        cx.update(|cx| {
            cx.default_global::<Fila>().itens.retain(|(i, _)| *i != id);
            cx.refresh_windows();
        });
    })
    .detach();
}

/// Os avisos à vista, do mais antigo ao mais novo (para testes e para quem quiser conferir).
pub fn visiveis(cx: &App) -> Vec<Aviso> {
    cx.try_global::<Fila>().map(|f| f.itens.iter().map(|(_, a)| a.clone()).collect()).unwrap_or_default()
}

fn icone_e_cor(tipo: TipoAviso) -> (&'static str, Hsla) {
    match tipo {
        TipoAviso::Sucesso => ("icons/circle-check-big.svg", cor::signal_success()),
        TipoAviso::Info => ("icons/info.svg", cor::signal_info()),
        TipoAviso::Alerta => ("icons/triangle-alert.svg", cor::signal_warning()),
        TipoAviso::Erro => ("icons/circle-x.svg", cor::signal_error()),
    }
}

/// A camada dos avisos: posição absoluta sobre a janela inteira, sem pegar o ponteiro fora dos avisos.
pub fn camada(window: &mut Window, cx: &mut App) -> AnyElement {
    let itens = visiveis(cx);
    if itens.is_empty() {
        return div().into_any_element();
    }
    let mut coluna = div().flex().flex_col().items_center().gap(px(8.)).w_full().max_w(px(560.));
    // A largura que sobra ao título: a coluna menos o respiro, a borda e o ícone com o vão.
    let largura = (560f32).min(f32::from(window.viewport_size().width) - 32.) - 24. - 2. - 16. - 8.;
    for aviso in itens {
        let (caminho, tom) = icone_e_cor(aviso.tipo);
        coluna = coluna.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .max_w_full()
                .px(px(12.))
                .py(px(8.))
                .rounded(tema::RAIO_MD)
                .bg(cor::raised_black())
                .border_1()
                .border_color(gpui_kit::rgba(0xffffff14))
                .text_size(tema::TEXTO_MENU)
                .line_height(px(20.))
                .text_color(cor::text_primary())
                .child(icone(caminho, 16., tom))
                .child({
                    let linhas = crate::widgets::linhas_pretty(&aviso.titulo, largura, tema::TEXTO_MENU, FontWeight::MEDIUM, window);
                    // Quebrado, o aviso ocupa a largura toda, como o `fit-content` do navegador.
                    let quebrou = linhas.contains('\n');
                    div().min_w_0().when(quebrou, |d| d.w(px(largura))).font_weight(FontWeight::MEDIUM).child(linhas)
                }),
        );
    }
    div()
        .absolute()
        .top(px(43.))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .px(px(16.))
        .child(coluna)
        .into_any_element()
}
