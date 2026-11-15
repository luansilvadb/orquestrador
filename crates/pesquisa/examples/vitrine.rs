//! Abre a tela Pesquisar numa janela 1400x900, na posição da coluna lateral da bancada, com os arquivos do mock
//! (`src/termo-a.ts`, `src/termo-b.ts`, uma imagem e um JSON grande que ficam de fora). `VITRINE=substituir` abre a
//! substituição com "novo"; `VITRINE=confirmar` também abre a confirmação do Substituir Tudo.
use gpui_kit::{div, prelude::*, px, size, App, Bounds, Context, Entity, Window, WindowBounds, WindowOptions};
use orquestrador_pesquisa::TelaPesquisa;
use orquestrador_ui::{recursos, tema};
use std::time::Duration;

struct Vitrine {
    tela: Entity<TelaPesquisa>,
}

impl Render for Vitrine {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(tema::cor::pitch_black())
            .flex()
            .flex_col()
            .child(div().h(tema::TITLEBAR).bg(tema::cor::panel_black()).border_b_1().border_color(tema::cor::hairline()))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .min_h_0()
                    .child(div().w(px(44.)).bg(tema::cor::panel_black()).border_r_1().border_color(tema::cor::hairline()))
                    .child(
                        div()
                            .w(px(281.))
                            .h_full()
                            .bg(tema::cor::panel_black())
                            .border_r_1()
                            .border_color(tema::cor::hairline())
                            .child(self.tela.clone()),
                    ),
            )
    }
}

fn pasta_mock() -> std::path::PathBuf {
    let raiz = std::env::temp_dir().join("orq-vitrine-pesquisa");
    let _ = std::fs::remove_dir_all(&raiz);
    std::fs::create_dir_all(raiz.join("src")).unwrap();
    std::fs::create_dir_all(raiz.join("dados")).unwrap();
    std::fs::write(raiz.join("src/termo-a.ts"), "const termo = 1;\n").unwrap();
    std::fs::write(raiz.join("src/termo-b.ts"), "let x = termo + termo;\n").unwrap();
    std::fs::write(raiz.join("logo.png"), b"\x89PNG\0\0termo").unwrap();
    std::fs::write(raiz.join("dados/exportacao.json"), "termo ".repeat(200_000)).unwrap();
    raiz
}

fn main() {
    let modo = std::env::var("VITRINE").unwrap_or_default();
    let raiz = pasta_mock();
    gpui_kit::application().with_assets(recursos::Recursos).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        tema::aplicar(cx);
        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
        let opcoes = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        let modo = modo.clone();
        let raiz = raiz.clone();
        gpui_kit::open_window(opcoes, cx, move |window, cx| {
            let tela = cx.new(|cx| TelaPesquisa::new(Some(raiz), window, cx));
            tela.update(cx, |t, cx| {
                t.semear("termo", window, cx);
                if modo == "substituir" || modo == "confirmar" {
                    t.definir_substituicao("novo", window, cx);
                }
            });
            if modo == "confirmar" {
                let t = tela.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(Duration::from_millis(1500)).await;
                    t.update(cx, |t, cx| t.substituir_tudo(cx));
                })
                .detach();
            }
            cx.new(|_| Vitrine { tela })
        })
        .unwrap();
    });
}
