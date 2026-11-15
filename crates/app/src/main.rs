#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// A saída dos macros do GPUI aponta para `gpui::`; o alias resolve tudo pelo Kit.
extern crate gpui_kit as gpui;

mod backend;
mod bancada;
mod estado;
mod instancia;
mod moldura;
mod paineis;
mod recursos;
mod tema;
mod terminal;
#[cfg(test)]
mod testes;

use bancada::{AlternarTerminal, Bancada};
use gpui_kit::{px, size, AppContext, Bounds, KeyBinding, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions};
use std::rc::Rc;

fn main() {
    // Chamado pelos CLIs dos executores, o executável só faz o serviço do hook e sai, sem janela (o mesmo despacho do Tauri).
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--hook") => std::process::exit(orquestrador_core::agents::hook::run(&args[1..])),
        Some("--allow") => {
            // O pedido vem por stdin e é lido inteiro, para o CLI não escrever num pipe já fechado.
            let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
            println!("{}", orquestrador_core::agents::cli::ALLOW);
        }
        Some("--mcp") => std::process::exit(orquestrador_core::agents::mcp::serve(&args[1..])),
        _ => {
            let cwd = std::env::current_dir().unwrap_or_default();
            // Como o Tauri, a instância única vale só no build de release.
            let _guarda = if cfg!(debug_assertions) {
                None
            } else {
                match instancia::reivindicar(&args, &cwd) {
                    instancia::Instancia::Repassada => return,
                    instancia::Instancia::Principal(guarda) => Some(guarda),
                }
            };
            let paths = orquestrador_core::open_paths::extract_paths(&args, &cwd);
            abrir_janela(paths);
        }
    }
}

fn abrir_janela(iniciais: Vec<std::path::PathBuf>) {
    gpui_kit::application().with_assets(recursos::Recursos).run(move |cx| {
        gpui_kit::init(cx);
        tema::aplicar(cx);
        cx.bind_keys([KeyBinding::new("ctrl-`", AlternarTerminal, Some("Bancada"))]);
        let (backend, remetente, eventos) = backend::Backend::real();
        if !cfg!(debug_assertions) {
            instancia::escutar(remetente);
        }
        let backend = Rc::new(backend);
        let bounds = Bounds::centered(None, size(px(tema::JANELA_LARGURA), px(tema::JANELA_ALTURA)), cx);
        let opcoes = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("Orquestrador".into()), appears_transparent: true, traffic_light_position: None }),
            window_background: WindowBackgroundAppearance::Opaque,
            window_min_size: Some(size(px(640.), px(400.))),
            app_id: Some("orquestrador".into()),
            ..Default::default()
        };
        let aberta = gpui_kit::open_window(opcoes, cx, move |window, cx| {
            let view = cx.new(|cx| Bancada::new(backend, eventos, iniciais, cx));
            window.focus(&view.read(cx).foco.clone(), cx);
            view
        });
        if let Err(e) = aberta {
            eprintln!("A janela não abriu: {e}");
            cx.quit();
        }
        // Fechar a única janela encerra o app e os terminais do core (o `Drop` de `Projects`).
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
