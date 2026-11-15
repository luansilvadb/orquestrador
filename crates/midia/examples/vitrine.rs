//! Janela 1400×900 com a aba de mídia na posição do editor central das capturas de referência (x 325..828, y 88..878),
//! com os dados do mock: `assets/logo.png` (PNG 32×32), `midia/tom.wav` (2 s em silêncio), `midia/clipe.webm` (VP8 64×48
//! de 2 s) e `midia/quebrado.png` (leitura que falha).
//! Uso: cargo run -p orquestrador-midia --example vitrine-midia -- imagem|audio|video|tocar|pip|erro [arquivo]
//! `tocar` toca na aba; `pip` toca e abre o picture-in-picture; `arquivo` troca a mídia do cenário (medir memória com um vídeo longo).
use gpui_kit::{div, prelude::*, px, size, App, Bounds, Context, Entity, Window, WindowBounds, WindowOptions};
use orquestrador_midia::VisaoMidia;
use orquestrador_ui::tema::{self, cor};
use std::path::PathBuf;

struct Vitrine {
    midia: Entity<VisaoMidia>,
}

impl Render for Vitrine {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .bg(cor::pitch_black())
            .font_family(tema::FONTE)
            .child(div().absolute().left_0().top_0().w_full().h(px(36.)).bg(cor::panel_black()))
            .child(div().absolute().left_0().top(px(36.)).w(px(324.)).h(px(842.)).bg(cor::panel_black()))
            .child(div().absolute().left(px(324.)).top(px(36.)).w(px(1.)).h(px(842.)).bg(cor::hairline()))
            .child(div().absolute().left(px(828.)).top(px(36.)).w(px(1.)).h(px(842.)).bg(cor::hairline()))
            .child(div().absolute().left(px(829.)).top(px(36.)).w(px(571.)).h(px(842.)).bg(cor::painel_arrastavel()))
            .child(div().absolute().left_0().top(px(878.)).w_full().h(px(22.)).bg(cor::panel_black()))
            .child(div().absolute().left(px(325.)).top(px(88.)).w(px(503.)).h(px(790.)).flex().child(self.midia.clone()))
    }
}

/// O `wavSilence(2)` do mock: PCM 8 bits mono a 8 kHz.
fn wav_silencio(segundos: u32) -> Vec<u8> {
    let n = segundos * 8000;
    let mut b = Vec::with_capacity(44 + n as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + n).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&n.to_le_bytes());
    b.resize(44 + n as usize, 128);
    b
}

fn preparar(modo: &str) -> PathBuf {
    let raiz = std::env::temp_dir().join("orq-midia-vitrine");
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let _ = std::fs::create_dir_all(raiz.join("assets"));
    let _ = std::fs::create_dir_all(raiz.join("midia"));
    match modo {
        "audio" => {
            let c = raiz.join("midia/tom.wav");
            std::fs::write(&c, wav_silencio(2)).expect("wav");
            c
        }
        "video" | "tocar" | "pip" => {
            let c = raiz.join("midia/clipe.webm");
            std::fs::copy(fixtures.join("clipe.webm"), &c).expect("webm");
            c
        }
        "erro" => raiz.join("midia/quebrado.png"),
        _ => {
            let c = raiz.join("assets/logo.png");
            std::fs::copy(fixtures.join("logo.png"), &c).expect("png");
            c
        }
    }
}

fn main() {
    let modo = std::env::args().nth(1).unwrap_or_else(|| "imagem".into());
    let caminho = std::env::args().nth(2).map(PathBuf::from).unwrap_or_else(|| preparar(&modo));
    let pip = modo == "pip";
    let tocar = pip || modo == "tocar";
    gpui_kit::application().with_assets(orquestrador_ui::recursos::Recursos).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        tema::aplicar(cx);
        orquestrador_midia::registrar_atalhos(cx);
        let bounds = Bounds::centered(None, size(px(tema::JANELA_LARGURA), px(tema::JANELA_ALTURA)), cx);
        let opcoes = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        gpui_kit::open_window(opcoes, cx, move |window, cx| {
            let midia = cx.new(|cx| VisaoMidia::new(caminho, window, cx));
            if tocar {
                let m = midia.downgrade();
                cx.spawn(async move |cx| {
                    for _ in 0..100 {
                        cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
                        let pronto = m.update(cx, |v, _| v.estado_reproducao().is_some_and(|e| e.info.pronto)).unwrap_or(true);
                        if pronto {
                            break;
                        }
                    }
                    let _ = m.update(cx, |v, cx| {
                        v.tocar(cx);
                        if pip {
                            v.abrir_pip(cx);
                        }
                    });
                })
                .detach();
            }
            cx.new(|_| Vitrine { midia })
        })
        .expect("janela");
    });
}
