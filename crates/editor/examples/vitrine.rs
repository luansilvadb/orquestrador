//! Janela 1400×900 com a coluna do editor na posição das capturas de referência (x 325..828, y 36..878), sobre um
//! projeto temporário com os textos do mock (`examples/dados`, ver o README).
//! Uso: cargo run -p orquestrador-editor --example vitrine-editor -- arquivos|varias|menu|lista|dividido|rapido|md-preview|md-codigo|binario|vazio|git-bloco
use gpui_kit::{div, prelude::*, px, size, App, Bounds, Context, Entity, Window, WindowBounds, WindowOptions};
use orquestrador_editor::armazem::Chaves;
use orquestrador_editor::{Abertura, AreaEditor};
use orquestrador_ui::tema::{self, cor};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Base do Git de `src/editor-workspace.tsx` como a do mock do React: a pasta muda as linhas 28, 32 e 96.
struct GitVitrine(String);
impl orquestrador_editor::gutter_git::FonteGutter for GitVitrine {
    fn base(&self, caminho: &str) -> Result<Option<orquestrador_editor::gutter_git::BaseGit>, String> {
        if caminho != "src/editor-workspace.tsx" {
            return Ok(None);
        }
        let head = self
            .0
            .replace("useLayoutEffect, useMemo, useRef, useState,", "useLayoutEffect, useRef, useState,")
            .replace("import { ChevronDown, Columns2, Ellipsis, Pin, X }", "import { Columns2, Ellipsis, Pin, X }")
            .replace("readOnly: boolean; size: number };", "readOnly: boolean };");
        Ok(Some(orquestrador_editor::gutter_git::BaseGit { index: Some(head.clone()), head: Some(head) }))
    }
    fn gravar_indice(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

struct Memoria(BTreeMap<String, String>);
impl Chaves for Memoria {
    fn ler(&self, c: &str) -> Option<String> {
        self.0.get(c).cloned()
    }
    fn gravar(&mut self, c: &str, t: &str) -> Result<(), String> {
        self.0.insert(c.into(), t.into());
        Ok(())
    }
    fn apagar(&mut self, c: &str) {
        self.0.remove(c);
    }
    fn chaves(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

struct Vitrine {
    area: Entity<AreaEditor>,
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
            .child(div().absolute().left(px(325.)).top(px(36.)).w(px(503.)).h(px(842.)).flex().child(self.area.clone()))
    }
}

fn preparar() -> PathBuf {
    let raiz = std::env::temp_dir().join("orq-editor-vitrine");
    let dados = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/dados");
    for p in ["src", "docs", "dados"] {
        let _ = std::fs::create_dir_all(raiz.join(p));
    }
    let copiar = |de: &str, para: &str| std::fs::copy(dados.join(de), raiz.join(para)).expect("cópia");
    copiar("README.md", "README.md");
    copiar("App.tsx", "src/App.tsx");
    copiar("main.tsx", "src/main.tsx");
    copiar("longo.ts", "src/longo.ts");
    copiar("editor-workspace.tsx", "src/editor-workspace.tsx");
    copiar("guia.md", "docs/guia.md");
    // Maior que o limite de leitura do core (1 MiB): "grande demais".
    std::fs::write(raiz.join("dados/exportacao.json"), vec![b' '; 1_100_000]).expect("json");
    raiz
}

fn main() {
    let modo = std::env::args().nth(1).unwrap_or_else(|| "arquivos".into());
    let raiz = preparar();
    gpui_kit::application().with_assets(orquestrador_ui::recursos::Recursos).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        tema::aplicar(cx);
        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
        let opcoes = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        let raiz = raiz.clone();
        gpui_kit::open_window(opcoes, cx, move |window, cx| {
            let projeto = raiz.to_string_lossy().to_string();
            let area = cx.new(|cx| AreaEditor::com_armazenamento(projeto, Rc::new(RefCell::new(Memoria(BTreeMap::new()))), window, cx));
            let pasta = std::fs::read_to_string(raiz.join("src/editor-workspace.tsx")).unwrap_or_default().replace("
", "
");
            area.update(cx, |a, cx| a.definir_fonte_git(Arc::new(GitVitrine(pasta)), cx));
            let p = |permanente| Abertura { permanente, ..Default::default() };
            area.update(cx, |a, cx| match modo.as_str() {
                "varias" => {
                    for c in ["src/App.tsx", "src/main.tsx", "src/longo.ts", "README.md"] {
                        a.abrir_arquivo(c, p(true), window, cx);
                    }
                }
                "menu" => {
                    for c in ["src/App.tsx", "src/main.tsx"] {
                        a.abrir_arquivo(c, p(true), window, cx);
                    }
                }
                "lista" => {
                    for c in ["src/App.tsx", "src/main.tsx", "src/longo.ts"] {
                        a.abrir_arquivo(c, p(true), window, cx);
                    }
                }
                "dividido" => {
                    a.abrir_arquivo("src/App.tsx", p(true), window, cx);
                    let id = a.workspace().aba_focada().map(|t| t.id.clone()).unwrap();
                    a.dividir(&id, window, cx);
                }
                "rapido" => a.abrir_rapido(window, cx),
                "md-preview" | "md-codigo" => a.abrir_arquivo("docs/guia.md", p(true), window, cx),
                "binario" => a.abrir_arquivo("dados/exportacao.json", p(true), window, cx),
                "vazio" => {}
                "git-bloco" => {
                    a.abrir_arquivo("src/editor-workspace.tsx", Abertura { permanente: true, linha: Some(26), coluna: Some(1), ..Default::default() }, window, cx)
                }
                _ => a.abrir_arquivo("src/editor-workspace.tsx", Abertura { permanente: true, linha: Some(26), coluna: Some(1), ..Default::default() }, window, cx),
            });
            let v = cx.new(|_| Vitrine { area: area.clone() });
            let a2 = area.clone();
            let modo2 = modo.clone();
            // Passos que dependem da leitura em segundo plano terminar.
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(600)).await;
                    let _ = cx.update(|window, cx| {
                        a2.update(cx, |a, cx| a.passo_vitrine(&modo2, window, cx));
                        if modo2 == "menu" {
                            // Botão direito na aba App.tsx.
                            use gpui_kit::{point, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
                            let position = point(px(370.), px(50.));
                            let base = MouseDownEvent { button: MouseButton::Right, position, modifiers: Modifiers::default(), click_count: 1, first_mouse: false };
                            window.dispatch_event(PlatformInput::MouseDown(base), cx);
                            let solta = MouseUpEvent { button: MouseButton::Right, position, modifiers: Modifiers::default(), click_count: 1 };
                            window.dispatch_event(PlatformInput::MouseUp(solta), cx);
                        }
                    });
                })
                .detach();
            v
        })
        .expect("janela");
    });
}
