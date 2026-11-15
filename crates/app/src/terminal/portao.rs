//! O portão de abertura: no instalado, antes de liberar o uso, os dois CLIs são garantidos (`clis::garantir_todos`), com o
//! andamento de cada um na tela (porte da fase `clis` de update.rs e da tela `Screen` do App.tsx). O build de
//! desenvolvimento libera direto.
use super::ui::{self, Variante};
use crate::tema::{self, cor};
use gpui_kit::TestSupportExt;
use gpui_kit::{
    anchored, canvas, deferred, div, point, prelude::*, px, size, App, Bounds, FontWeight, Hsla, MouseButton, PathBuilder,
    Pixels, Window,
};
use orquestrador_core::clis::{Andamento, Passo};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub enum Portao {
    Clis { claude: Andamento, codex: Andamento },
    Pronto,
    Falhou(String),
}

impl Portao {
    pub fn inicial() -> Portao {
        if cfg!(debug_assertions) {
            Portao::Pronto
        } else {
            let inicio = Andamento { step: Passo::Checking, version: None };
            Portao::Clis { claude: inicio.clone(), codex: inicio }
        }
    }
}

fn passo(p: Passo) -> &'static str {
    match p {
        Passo::Checking => "conferindo…",
        Passo::Installing => "instalando…",
        Passo::Updating => "atualizando…",
        Passo::Ready => "pronto",
    }
}

pub fn linha_do_cli(nome: &str, a: &Andamento) -> String {
    match &a.version {
        Some(v) => format!("{nome} {v}: {}", passo(a.step)),
        None => format!("{nome}: {}", passo(a.step)),
    }
}

/// O texto de status e as linhas dos CLIs.
pub fn textos(p: &Portao) -> (&'static str, Vec<String>) {
    match p {
        Portao::Clis { claude, codex } => {
            ("Preparando os CLIs…", vec![linha_do_cli("Claude Code", claude), linha_do_cli("Codex", codex)])
        }
        Portao::Falhou(_) => ("Não foi possível atualizar. O uso fica bloqueado até a atualização dar certo.", vec![]),
        Portao::Pronto => ("", vec![]),
    }
}

/// A rodada dos CLIs numa thread: o andamento chega por `avisar`, e o fim traz os caminhos ou o erro.
pub fn garantir(avisar: async_channel::Sender<Portao>, exes: orquestrador_core::agents::cli::Exes) {
    std::thread::spawn(move || {
        let progresso = avisar.clone();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            orquestrador_core::clis::garantir_todos(&orquestrador_core::clis::Real, |claude, codex| {
                let _ = progresso.try_send(Portao::Clis { claude, codex });
            })
        }));
        let fim = match r {
            Ok(Ok(caminhos)) => {
                exes.set(caminhos);
                Portao::Pronto
            }
            Ok(Err(msg)) => Portao::Falhou(msg),
            Err(_) => Portao::Falhou("A verificação dos CLIs parou inesperadamente.".into()),
        };
        let _ = avisar.try_send(fim);
    });
}

/// A tela do portão sobre tudo, menos os controles da janela; `tentar` refaz a rodada depois de uma falha.
pub fn tela(p: &Portao, inicio: Instant, window: &mut Window, tentar: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    let janela = window.viewport_size();
    let (status, linhas) = textos(p);
    let falha = match p {
        Portao::Falhou(m) => Some(m.clone()),
        _ => None,
    };
    let parada = falha.is_some();
    let marca = canvas(|_, _, _| (), move |b, _, window, _| pintar_marca(b, inicio, parada, window)).size(px(64.)).mx_auto().mb(px(4.));
    let topo = div()
        .id("portao-topo")
        .window_control_area(gpui_kit::WindowControlArea::Drag)
        .flex()
        .items_center()
        .h(tema::TITLEBAR)
        .w(janela.width - tema::BOTAO_JANELA * 3.)
        .px(px(10.))
        .bg(cor::panel_black())
        .border_b_1()
        .border_color(cor::hairline())
        .child(gpui_kit::img(crate::recursos::icone::MARCA).size(px(16.)).flex_none());
    let coluna = div()
        .id("portao")
        .flex()
        .flex_col()
        .gap(px(16.))
        .w_full()
        .max_w(px(448.))
        .text_center()
        .child(marca)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(div().text_size(tema::TEXTO_TITLE).font_weight(FontWeight::SEMIBOLD).line_height(px(16.)).child("Orquestrador"))
                .child(
                    div()
                        .id("portao-status")
                        .flex()
                        .flex_col()
                        .text_size(tema::TEXTO_BODY)
                        .text_color(cor::text_secondary())
                        .child(status)
                        .children(linhas.into_iter().enumerate().map(|(i, l)| div().id(("portao-cli", i)).child(l)))
                        .test_support(),
                ),
        )
        .when_some(falha, |d, m| {
            d.child(
                div()
                    .id("portao-erro")
                    .text_left()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(tema::RAIO_SM)
                    .bg(gpui_kit::rgba(0xf4416c1f))
                    .border_1()
                    .border_color(gpui_kit::rgba(0xf4416c66))
                    .text_size(tema::TEXTO_BODY)
                    .child(m),
            )
            .child(
                div().flex().justify_center().child(
                    ui::botao("portao-tentar", "Tentar novamente", Variante::Primario, false)
                        .on_click(move |_, window, cx| tentar(window, cx))
                        .test_support(),
                ),
            )
        });
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("portao-tela")
                .relative()
                .w(janela.width)
                .h(janela.height)
                .text_color(cor::text_primary())
                .font_family(tema::FONTE)
                // Os controles da janela, que a tela não cobre, seguem funcionando.
                .child(topo.occlude())
                .child(
                    div()
                        .id("portao-corpo")
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .bg(cor::pitch_black())
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(tema::TITLEBAR)
                        .bottom_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(24.))
                        .child(coluna),
                ),
        ),
    )
    .with_priority(3)
}

// ---------- a marca animada do index.html ----------

/// `cubic-bezier(.2, .8, .2, 1)` aproximada: sai rápido e assenta.
fn suave(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// Interpola nos quadros-chave `(fração, valor)`, com a curva entre eles.
fn quadros(t: f32, chaves: &[(f32, f32)]) -> f32 {
    for par in chaves.windows(2) {
        let ((a, va), (b, vb)) = (par[0], par[1]);
        if t <= b {
            let f = if b > a { suave((t - a) / (b - a)) } else { 1. };
            return va + (vb - va) * f;
        }
    }
    chaves.last().map_or(0., |c| c.1)
}

const CICLO: f32 = 2400.;

fn pintar_marca(b: Bounds<Pixels>, inicio: Instant, parada: bool, window: &mut Window) {
    let ms = if parada { 0. } else { inicio.elapsed().as_secs_f32() * 1000. };
    let escala = f32::from(b.size.width) / 200.;
    let p = |x: f32, y: f32| point(b.origin.x + px((x - 28.) * escala), b.origin.y + px((y - 28.) * escala));
    let tinta: Hsla = gpui_kit::rgb(0xeceae4).into();
    let hub = (73.9, 182.1);
    let raizes = [((73.9, 59.1), 200.), ((156.3, 91.1), 360.), ((196.3, 169.2), 520.)];
    let circulo = |window: &mut Window, (x, y): (f32, f32), r: f32, cor: Hsla| {
        let c = p(x, y);
        let r = px(r * escala);
        window.paint_quad(gpui_kit::quad(
            Bounds::new(point(c.x - r, c.y - r), size(r * 2., r * 2.)),
            r,
            cor,
            px(0.),
            gpui_kit::transparent_black(),
            Default::default(),
        ));
    };
    for ((x, y), atraso) in raizes {
        let t = if parada { 0.5 } else { (((ms - atraso).max(0.)) % CICLO) / CICLO };
        let antes = !parada && ms < atraso;
        let visivel = if antes { 0. } else { quadros(t, &[(0., 0.), (0.22, 1.), (0.70, 1.), (0.90, 0.), (1., 0.)]) };
        if visivel > 0.01 {
            let fim = (hub.0 + (x - hub.0) * visivel, hub.1 + (y - hub.1) * visivel);
            let mut caminho = PathBuilder::stroke(px(19. * escala));
            caminho.move_to(p(hub.0, hub.1));
            caminho.line_to(p(fim.0, fim.1));
            if let Ok(c) = caminho.build() {
                window.paint_path(c, tinta);
            }
            circulo(window, fim, 9.5, tinta);
        }
        let (op, esc) = if antes {
            (0., 0.3)
        } else {
            (
                quadros(t, &[(0., 0.), (0.16, 0.), (0.26, 1.), (0.60, 1.), (0.70, 0.), (1., 0.)]),
                quadros(t, &[(0., 0.3), (0.16, 0.3), (0.26, 1.15), (0.32, 1.), (0.60, 1.), (0.70, 0.3), (1., 0.3)]),
            )
        };
        if op > 0.01 {
            circulo(window, (x, y), 23. * esc, tinta.opacity(op));
        }
    }
    let pop = (ms / 360.).min(1.);
    let batida = if parada || ms < 360. { 1. } else { quadros(((ms - 360.) % CICLO) / CICLO, &[(0., 1.), (0.08, 1.1), (0.20, 1.), (1., 1.)]) };
    let laranja: Hsla = gpui_kit::rgb(0xf5a524).into();
    circulo(window, hub, 36.5 * (0.4 + 0.6 * suave(pop)) * batida, laranja.opacity(suave(pop)));
    if !parada {
        window.request_animation_frame();
    }
}

#[cfg(test)]
mod testes {
    // Porte de tests/check-update-clis-ui.mjs (o texto) e da forma da fase `clis`.
    use super::*;

    #[test]
    fn fase_clis_uma_linha_por_cli_com_versao_e_passo() {
        let p = Portao::Clis {
            claude: Andamento { step: Passo::Updating, version: Some("2.1.325".into()) },
            codex: Andamento { step: Passo::Installing, version: None },
        };
        let (status, linhas) = textos(&p);
        assert_eq!(status, "Preparando os CLIs…");
        assert_eq!(linhas, vec!["Claude Code 2.1.325: atualizando…", "Codex: instalando…"]);
    }

    #[test]
    fn falha_bloqueia_com_o_motivo_e_desenvolvimento_libera() {
        assert!(textos(&Portao::Falhou("x".into())).0.starts_with("Não foi possível atualizar"));
        assert_eq!(Portao::inicial(), Portao::Pronto, "o teste roda no build de desenvolvimento");
    }

    #[test]
    fn quadros_chave_interpolam_e_seguram() {
        let k = [(0., 0.), (0.22, 1.), (0.70, 1.), (0.90, 0.), (1., 0.)];
        assert_eq!(quadros(0., &k), 0.);
        assert_eq!(quadros(0.5, &k), 1.);
        assert_eq!(quadros(0.95, &k), 0.);
    }
}

/// O portão como vista: segue o andamento da rodada e desenha a tela enquanto o uso não foi liberado.
pub struct Abertura {
    pub estado: Portao,
    inicio: Instant,
    exes: orquestrador_core::agents::cli::Exes,
    _tarefa: Option<gpui_kit::Task<()>>,
}

impl Abertura {
    pub fn new(exes: orquestrador_core::agents::cli::Exes, cx: &mut gpui_kit::Context<Self>) -> Self {
        let mut a = Abertura { estado: Portao::inicial(), inicio: Instant::now(), exes, _tarefa: None };
        // Só no debug, como o `?update=clis` do app simulado: a fase dos CLIs parada, para conferir a tela.
        if cfg!(debug_assertions) && std::env::var("ORQ_PORTAO").as_deref() == Ok("clis") {
            a.estado = Portao::Clis {
                claude: Andamento { step: Passo::Updating, version: Some("2.1.325".into()) },
                codex: Andamento { step: Passo::Installing, version: None },
            };
            return a;
        }
        if a.estado != Portao::Pronto {
            a.rodar(cx);
        }
        a
    }

    pub fn pronto(&self) -> bool {
        self.estado == Portao::Pronto
    }

    fn rodar(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let (tx, rx) = async_channel::unbounded();
        garantir(tx, self.exes.clone());
        self._tarefa = Some(cx.spawn(async move |this: gpui_kit::WeakEntity<Self>, cx| {
            while let Ok(estado) = rx.recv().await {
                if this.update(cx, |a, cx| {
                    a.estado = estado;
                    cx.notify();
                })
                .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// "Tentar novamente": só depois de uma falha.
    pub fn tentar(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if matches!(self.estado, Portao::Falhou(_)) {
            let inicio = Andamento { step: Passo::Checking, version: None };
            self.estado = Portao::Clis { claude: inicio.clone(), codex: inicio };
            self.rodar(cx);
            cx.notify();
        }
    }
}

impl Render for Abertura {
    fn render(&mut self, window: &mut Window, cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        if self.pronto() {
            return div().into_any_element();
        }
        let eu = cx.entity().downgrade();
        tela(&self.estado, self.inicio, window, move |_, cx| {
            let _ = eu.update(cx, |a, cx| a.tentar(cx));
        })
        .into_any_element()
    }
}
