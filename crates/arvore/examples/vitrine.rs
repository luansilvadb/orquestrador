//! Vitrine da árvore: janela de 1400x900 com a tela Arquivos preenchida como o app simulado (`src/backend/mock.ts`), no
//! lugar que ela ocupa na bancada (x 44..324, abaixo da titlebar de 36 px). O projeto é criado em
//! `%TEMP%\orq-arvore-vitrine\exemplo`, com `git init` e o `.gitignore` do mock (dist e *.log).
//!
//! `cargo run -p orquestrador-arvore --example vitrine-arvore -- <cenário>`, com o cenário:
//! `inicial` (padrão), `editor` (src e docs abertos, App.tsx aberto), `selecao` (App.tsx e main.tsx), `menu-arquivo`,
//! `menu-pasta`, `renomear`, `toast`, `toast-info`, `excluir`, `criar`, `grande` (5000 arquivos).
extern crate gpui_kit as gpui;

use gpui_kit::{div, point, prelude::*, px, size, App, Bounds, Context, Entity, Window, WindowBounds, WindowOptions};
use orquestrador_arvore::Explorador;
use orquestrador_core::ide::git::{GitFile, Kind};
use orquestrador_ui::avisos::{self, Aviso};
use orquestrador_ui::{recursos, tema};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn arquivo(raiz: &Path, rel: &str) {
    let p = raiz.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, format!("{rel}\n")).unwrap();
}

fn projeto(grande: bool) -> PathBuf {
    let base = std::env::temp_dir().join("orq-arvore-vitrine");
    let _ = std::fs::remove_dir_all(&base);
    let raiz = base.join("exemplo");
    for rel in [
        ".gitignore", "README.md", "debug.log", "package.json", "assets/logo.png", "dados/exportacao.json",
        "docs/design-system.md", "docs/rascunho-painel.md", "docs/apagado.md", "docs/visual-comparador.md",
        "src/App.tsx", "src/editor-workspace.tsx", "src/index.css", "src/longo.ts", "src/main.tsx", "src/conflito.ts",
        "src/sem-quebra.ts", "src/marcadores.ts", "src/components/ui/button.tsx", "src/components/ui/card.tsx",
        "src/lib/utils.ts", "dist/index.js",
    ] {
        arquivo(&raiz, rel);
    }
    std::fs::create_dir_all(raiz.join("tests")).unwrap();
    std::fs::write(raiz.join(".gitignore"), "dist\n*.log\n").unwrap();
    if grande {
        for i in 0..5000 {
            arquivo(&raiz, &format!("grande/item-{i:04}.md"));
        }
    }
    let _ = std::process::Command::new("git").arg("init").arg("-q").current_dir(&raiz).status();
    raiz
}

fn git() -> Vec<GitFile> {
    let e = |p: &str, s: Option<Kind>, u: Option<Kind>| GitFile { path: p.into(), orig_path: None, staged: s, unstaged: u };
    use Kind::*;
    vec![
        e("README.md", None, Some(Modified)),
        e("docs/visual-comparador.md", None, Some(Modified)),
        e("docs/design-system.md", Some(Modified), None),
        e("docs/rascunho-painel.md", None, Some(Untracked)),
        e("src/App.tsx", Some(Modified), Some(Modified)),
        e("src/editor-workspace.tsx", Some(Modified), Some(Modified)),
        e("src/sem-quebra.ts", None, Some(Modified)),
        e("src/marcadores.ts", Some(Modified), Some(Modified)),
        e("dados/exportacao.json", None, Some(Modified)),
        e("assets/logo.png", None, Some(Modified)),
        e("src/conflito.ts", None, Some(Conflicted)),
        e("docs/apagado.md", None, Some(Conflicted)),
    ]
}

struct Vitrine {
    arvore: Entity<Explorador>,
}

impl Render for Vitrine {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let borda = orquestrador_ui::tema::cor::hairline();
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(tema::cor::pitch_black())
            .font_family(tema::FONTE)
            .child(div().h(tema::TITLEBAR).flex_none().bg(tema::cor::panel_black()).border_b_1().border_color(borda))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(div().w(px(44.)).flex_none().border_r_1().border_color(borda))
                    .child(div().w(px(281.)).flex_none().border_r_1().border_color(borda).child(self.arvore.clone()))
                    .child(div().flex_1()),
            )
            .child(div().h(tema::STATUS_BAR).flex_none().border_t_1().border_color(borda))
            .child(avisos::camada(window, cx))
    }
}

fn main() {
    let cenario = std::env::args().nth(1).unwrap_or_else(|| "inicial".into());
    let raiz = projeto(cenario == "grande");
    gpui_kit::application().with_assets(recursos::Recursos).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        tema::aplicar(cx);
        let bounds = Bounds::centered(None, size(px(tema::JANELA_LARGURA), px(tema::JANELA_ALTURA)), cx);
        let opcoes = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        gpui_kit::open_window(opcoes, cx, move |window, cx| {
            let arvore = cx.new(|cx| Explorador::new(raiz.clone(), "exemplo", window, cx));
            arvore.update(cx, |a, cx| {
                a.definir_git(Some(&git()), cx);
                a.definir_historico(true);
            });
            let c = cenario.clone();
            let alvo = arvore.clone();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(Duration::from_millis(2500)).await;
                    let _ = cx.update(|window, cx| {
                        alvo.update(cx, |a, cx| match c.as_str() {
                            "editor" => {
                                a.expandir(vec!["src".into(), "docs".into()], window, cx);
                                a.definir_aberto(Some("src/editor-workspace.tsx".into()), cx);
                            }
                            "selecao" => {
                                a.expandir(vec!["src".into()], window, cx);
                                a.selecionar(vec!["src/App.tsx".into(), "src/main.tsx".into()], cx);
                            }
                            "menu-arquivo" => a.expandir(vec!["docs".into()], window, cx),
                            "menu-pasta" => a.abrir_menu_de_contexto(Some("src".into()), point(px(185.), px(205.)), window, cx),
                            "renomear" => {
                                a.selecionar(vec!["README.md".into()], cx);
                                a.renomear("README.md".into(), window, cx)
                            }
                            "toast" => avisos::mostrar(Aviso::sucesso("Alterações sincronizadas", 600_000), cx),
                            "toast-info" => avisos::mostrar(
                                Aviso::info(
                                    "Esta pasta está dentro do repositório C:\\projetos\\exemplo. O projeto aberto é a raiz dele.",
                                    600_000,
                                ),
                                cx,
                            ),
                            "excluir" => a.excluir(vec!["src".into(), "README.md".into()], window, cx),
                            "criar" => a.criar(false, String::new(), window, cx),
                            "grande" => a.expandir(vec!["grande".into()], window, cx),
                            _ => {}
                        });
                    });
                    if c == "menu-arquivo" {
                        cx.background_executor().timer(Duration::from_millis(400)).await;
                        let _ = cx.update(|window, cx| {
                            alvo.update(cx, |a, cx| {
                                a.abrir_menu_de_contexto(Some("docs/design-system.md".into()), point(px(185.), px(226.)), window, cx)
                            })
                        });
                    }
                })
                .detach();
            cx.new(|_| Vitrine { arvore })
        })
        .unwrap();
    });
}
