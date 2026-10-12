//! Backend do app Orquestrador: um canvas de terminais em que um CLI orquestra e outros executam.
pub mod cli;
pub mod config;
pub mod core;
pub mod hook;
pub mod proc;
pub mod pty;
pub mod session;

use crate::config::{Choice, Options};
use crate::core::{Core, Snapshot};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

// Todos os comandos são `async` para rodar fora da thread da janela: vários esperam processo
// ou disco, e o diálogo de pasta trava quem o chama.

#[tauri::command(async)]
fn choose_project(app: tauri::AppHandle, core: State<Core>) -> Result<Option<String>, String> {
    match app.dialog().file().set_title("Pasta do projeto").blocking_pick_folder() {
        None => Ok(None),
        Some(folder) => core.set_project(&folder.into_path().map_err(|e| e.to_string())?).map(Some),
    }
}

#[tauri::command(async)]
fn get_options(core: State<Core>) -> Result<Options, String> {
    core.options()
}

#[tauri::command(async)]
fn start_orchestrator(core: State<Core>, choice: Choice, cols: u16, rows: u16) -> Result<String, String> {
    core.start_orchestrator(choice, cols, rows)
}

#[tauri::command(async)]
fn set_executor_choice(core: State<Core>, choice: Choice) -> Result<(), String> {
    core.set_executor_choice(choice)
}

#[tauri::command(async)]
fn set_max_parallel(core: State<Core>, value: i64) -> Result<(), String> {
    core.set_max_parallel(value)
}

#[tauri::command(async)]
fn term_write(core: State<Core>, id: String, data: String) -> Result<(), String> {
    core.term_write(&id, &data);
    Ok(())
}

#[tauri::command(async)]
fn term_resize(core: State<Core>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    core.term_resize(&id, cols, rows);
    Ok(())
}

#[tauri::command(async)]
fn term_snapshot(core: State<Core>, id: String) -> Result<Snapshot, String> {
    Ok(core.term_snapshot(&id))
}

#[tauri::command(async)]
fn stop_executor(core: State<Core>, id: String) -> Result<(), String> {
    core.stop_executor(&id);
    Ok(())
}

#[tauri::command(async)]
fn clear_done(core: State<Core>) -> Result<Vec<String>, String> {
    Ok(core.clear_done())
}

#[tauri::command(async)]
fn has_running(core: State<Core>) -> Result<bool, String> {
    Ok(core.has_running())
}

#[tauri::command(async)]
fn shutdown(core: State<Core>) -> Result<(), String> {
    core.shutdown();
    Ok(())
}

pub fn run(context: tauri::Context) {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let emit: core::Emit = Arc::new(move |event, payload| drop(handle.emit(event, payload)));
            // Sem LOCALAPPDATA o app abre mesmo assim; o erro aparece em `get_options`.
            let cfg_path = config::config_path().unwrap_or_default();
            app.manage(Core::new(cli::Exes::real(), session::Places::real(), cfg_path, emit));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            choose_project,
            get_options,
            start_orchestrator,
            set_executor_choice,
            set_max_parallel,
            term_write,
            term_resize,
            term_snapshot,
            stop_executor,
            clear_done,
            has_running,
            shutdown
        ])
        .build(context)
        .expect("o app não pôde ser iniciado")
        .run(|app, event| {
            // Rede de segurança: fechar sem passar por `shutdown` também mata tudo e limpa a sessão.
            if let tauri::RunEvent::Exit = event {
                app.state::<Core>().shutdown();
            }
        });
}
