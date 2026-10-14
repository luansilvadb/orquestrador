//! Backend do app Orquestrador: terminais em que um CLI orquestra e outros executam.
pub mod capacity;
pub mod cli;
pub mod clipboard;
pub mod config;
pub mod core;
pub mod delegate;
pub mod files;
pub mod git;
pub mod hook;
pub mod proc;
pub mod pty;
pub mod roles;
pub mod search;
pub mod session;
pub mod update;

use crate::config::{Choice, Options};
use crate::core::{Core, Snapshot};
use crate::update::Gate;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

// Todos os comandos são `async` para rodar fora da thread da janela: vários esperam processo
// ou disco, e o diálogo de pasta trava quem o chama.
// Exceto `update_status` e `retry_update`, todo comando passa por `Gate::ensure_open` antes de tocar
// no trabalho: com a versão fora de dia, nada do projeto, dos terminais ou da configuração é acessível.

#[tauri::command(async)]
fn choose_project(app: tauri::AppHandle, core: State<Core>, gate: State<Gate>) -> Result<Option<String>, String> {
    gate.ensure_open()?;
    match app.dialog().file().set_title("Pasta do projeto").blocking_pick_folder() {
        None => Ok(None),
        Some(folder) => core.set_project(&folder.into_path().map_err(|e| e.to_string())?).map(Some),
    }
}

#[tauri::command(async)]
fn get_options(core: State<Core>, gate: State<Gate>) -> Result<Options, String> {
    gate.ensure_open()?;
    core.options()
}

/// Um nível de uma pasta do projeto para o explorador. `path` é relativo ao projeto ("" é a raiz).
#[tauri::command(async)]
fn list_dir(core: State<Core>, gate: State<Gate>, path: String) -> Result<Vec<files::Entry>, String> {
    gate.ensure_open()?;
    files::list(&core.repo()?, &path)
}

/// O conteúdo de um arquivo do projeto para a tela. `path` é relativo ao projeto.
#[tauri::command(async)]
fn read_file(core: State<Core>, gate: State<Gate>, path: String) -> Result<files::Content, String> {
    gate.ensure_open()?;
    files::read(&core.repo()?, &path)
}

/// Grava o conteúdo de um arquivo do projeto. `path` é relativo ao projeto.
#[tauri::command(async)]
fn write_file(core: State<Core>, gate: State<Gate>, path: String, text: String) -> Result<(), String> {
    gate.ensure_open()?;
    files::write(&core.repo()?, &path, &text)
}

/// Busca `query` nos arquivos do projeto (rastreados e não rastreados, sem o ignorado e sem binário). Consulta
/// vazia não busca nada.
#[tauri::command(async)]
fn search(core: State<Core>, gate: State<Gate>, query: String, case_sensitive: bool, whole_word: bool, regex: bool) -> Result<search::Results, String> {
    gate.ensure_open()?;
    search::search(&core.repo()?, &query, case_sensitive, whole_word, regex)
}

/// O estado do Git do projeto para o painel: branch, ahead/behind e os arquivos alterados.
#[tauri::command(async)]
fn git_status(core: State<Core>, gate: State<Gate>) -> Result<git::Status, String> {
    gate.ensure_open()?;
    git::status(&core.repo()?)
}

/// O diff de um arquivo. `staged` compara o índice com o HEAD; senão, a pasta de trabalho com o índice.
#[tauri::command(async)]
fn git_diff(core: State<Core>, gate: State<Gate>, path: String, staged: bool) -> Result<git::Diff, String> {
    gate.ensure_open()?;
    git::diff(&core.repo()?, &path, staged)
}

/// Prepara os caminhos. Lista vazia prepara tudo, inclusive o não rastreado e o apagado.
#[tauri::command(async)]
fn git_stage(core: State<Core>, gate: State<Gate>, paths: Vec<String>) -> Result<(), String> {
    gate.ensure_open()?;
    git::stage(&core.repo()?, &paths)
}

/// Tira os caminhos do índice. Lista vazia tira tudo.
#[tauri::command(async)]
fn git_unstage(core: State<Core>, gate: State<Gate>, paths: Vec<String>) -> Result<(), String> {
    gate.ensure_open()?;
    git::unstage(&core.repo()?, &paths)
}

#[tauri::command(async)]
fn git_commit(core: State<Core>, gate: State<Gate>, message: String) -> Result<(), String> {
    gate.ensure_open()?;
    git::commit(&core.repo()?, &message)
}

/// A saída do `git fetch`, para a tela mostrar.
#[tauri::command(async)]
fn git_fetch(core: State<Core>, gate: State<Gate>) -> Result<String, String> {
    gate.ensure_open()?;
    git::fetch(&core.repo()?)
}

/// Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_pull(core: State<Core>, gate: State<Gate>) -> Result<String, String> {
    gate.ensure_open()?;
    core.git_idle(git::pull)
}

/// A saída do `git push`, para a tela mostrar. Sem upstream, publica em origin.
#[tauri::command(async)]
fn git_push(core: State<Core>, gate: State<Gate>) -> Result<String, String> {
    gate.ensure_open()?;
    git::push(&core.repo()?)
}

#[tauri::command(async)]
fn git_branches(core: State<Core>, gate: State<Gate>) -> Result<git::Branches, String> {
    gate.ensure_open()?;
    git::branches(&core.repo()?)
}

/// Troca para uma branch local existente. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_switch(core: State<Core>, gate: State<Gate>, name: String) -> Result<(), String> {
    gate.ensure_open()?;
    core.git_idle(|dir| git::switch(dir, &name))
}

/// Cria a branch a partir do HEAD e troca para ela. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_create_branch(core: State<Core>, gate: State<Gate>, name: String) -> Result<(), String> {
    gate.ensure_open()?;
    core.git_idle(|dir| git::create_branch(dir, &name))
}

#[tauri::command(async)]
fn start_orchestrator(core: State<Core>, gate: State<Gate>, choice: Choice, cols: u16, rows: u16) -> Result<(), String> {
    gate.ensure_open()?;
    core.start_orchestrator(choice, cols, rows)
}

/// Abre um shell do usuário, no slot `id` (`shell-<n>`), na pasta do projeto ou, com `cwd` (relativo ao projeto), na
/// pasta dele. Recusado se esse id já tiver um aberto. Devolve o programa que abriu.
#[tauri::command(async)]
fn start_shell(core: State<Core>, gate: State<Gate>, id: String, cols: u16, rows: u16, cwd: Option<String>) -> Result<String, String> {
    gate.ensure_open()?;
    core.start_shell_in(&id, cols, rows, cwd.as_deref())
}

/// Renomeia o arquivo `path` para `name`, na mesma pasta. Devolve o caminho novo. Nunca sobrescreve.
#[tauri::command(async)]
fn rename_file(core: State<Core>, gate: State<Gate>, path: String, name: String) -> Result<String, String> {
    gate.ensure_open()?;
    files::rename(&core.repo()?, &path, &name)
}

/// Manda o arquivo `path` para a Lixeira do Windows. Sem fallback para apagar de vez.
#[tauri::command(async)]
fn delete_file(core: State<Core>, gate: State<Gate>, path: String) -> Result<(), String> {
    gate.ensure_open()?;
    files::recycle(&core.repo()?, &path)
}

/// Mostra o arquivo `path` selecionado no Explorer do Windows.
#[tauri::command(async)]
fn reveal_file(core: State<Core>, gate: State<Gate>, path: String) -> Result<(), String> {
    gate.ensure_open()?;
    files::reveal(&core.repo()?, &path)
}

/// Abre o diálogo "Abrir com..." do Windows para o arquivo `path`.
#[tauri::command(async)]
fn open_with(core: State<Core>, gate: State<Gate>, path: String) -> Result<(), String> {
    gate.ensure_open()?;
    files::open_with(&core.repo()?, &path)
}

/// Põe o arquivo `path` no clipboard como arquivo copiado ou recortado no Explorer. Recortar só marca o movimento.
#[tauri::command(async)]
fn copy_file(core: State<Core>, gate: State<Gate>, path: String, cut: bool) -> Result<(), String> {
    gate.ensure_open()?;
    let file = files::plain_file(&core.repo()?, &path)?;
    clipboard::set_files(&[file], cut)
}

/// Cria o arquivo `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo. Nunca sobrescreve.
#[tauri::command(async)]
fn create_file(core: State<Core>, gate: State<Gate>, dir: String, name: String) -> Result<String, String> {
    gate.ensure_open()?;
    files::create_file(&core.repo()?, &dir, &name)
}

/// Cria a pasta `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo. Nunca sobrescreve.
#[tauri::command(async)]
fn create_dir(core: State<Core>, gate: State<Gate>, dir: String, name: String) -> Result<String, String> {
    gate.ensure_open()?;
    files::create_dir(&core.repo()?, &dir, &name)
}

/// Abre no Explorer do Windows a pasta `path` do projeto (vazio é a raiz).
#[tauri::command(async)]
fn reveal_dir(core: State<Core>, gate: State<Gate>, path: String) -> Result<(), String> {
    gate.ensure_open()?;
    files::reveal_dir(&core.repo()?, &path)
}

/// Os arquivos e o efeito (copiar ou recortar) do clipboard agora. Lido na hora de cada ação; sem arquivo, `paths` vem vazio.
#[tauri::command(async)]
fn clipboard_snapshot(gate: State<Gate>) -> Result<clipboard::Snapshot, String> {
    gate.ensure_open()?;
    Ok(clipboard::snapshot())
}

/// Cola `snap` na pasta `dest` do projeto (vazio é a raiz). `snap` é a lista lida na ação, a mesma que o frontend
/// usou para checar alterações não salvas. Recorte que já tirou algo das origens esvazia o clipboard, se ele ainda
/// for `snap`; um clipboard trocado no meio fica como está.
#[tauri::command(async)]
fn paste_files(core: State<Core>, gate: State<Gate>, dest: String, snap: clipboard::Snapshot) -> Result<Vec<String>, String> {
    gate.ensure_open()?;
    let created = files::paste(&core.repo()?, &dest, &snap);
    if snap.cut && snap.paths.iter().any(|p| !p.exists()) {
        clipboard::clear_if_same(&snap);
    }
    created
}

/// Fecha o shell `id` e mata o processo dele.
#[tauri::command(async)]
fn close_shell(core: State<Core>, gate: State<Gate>, id: String) -> Result<(), String> {
    gate.ensure_open()?;
    core.close_shell(&id);
    Ok(())
}

#[tauri::command(async)]
fn set_executor_choice(core: State<Core>, gate: State<Gate>, choice: Choice) -> Result<(), String> {
    gate.ensure_open()?;
    core.set_executor_choice(choice)
}

#[tauri::command(async)]
fn term_write(core: State<Core>, gate: State<Gate>, id: String, data: String) -> Result<(), String> {
    gate.ensure_open()?;
    core.term_write(&id, &data);
    Ok(())
}

#[tauri::command(async)]
fn term_resize(core: State<Core>, gate: State<Gate>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    gate.ensure_open()?;
    core.term_resize(&id, cols, rows);
    Ok(())
}

#[tauri::command(async)]
fn term_snapshot(core: State<Core>, gate: State<Gate>, id: String) -> Result<Snapshot, String> {
    gate.ensure_open()?;
    Ok(core.term_snapshot(&id))
}

/// Os arquivos copiados no Explorer que estão no clipboard, para o colar do terminal do orquestrador.
#[tauri::command(async)]
fn clipboard_files(gate: State<Gate>) -> Result<Vec<String>, String> {
    gate.ensure_open()?;
    Ok(clipboard::files())
}

#[tauri::command(async)]
fn stop_executor(core: State<Core>, gate: State<Gate>, id: String) -> Result<(), String> {
    gate.ensure_open()?;
    core.stop_executor(&id);
    Ok(())
}

#[tauri::command(async)]
fn clear_done(core: State<Core>, gate: State<Gate>) -> Result<Vec<String>, String> {
    gate.ensure_open()?;
    Ok(core.clear_done())
}

#[tauri::command(async)]
fn has_running(core: State<Core>, gate: State<Gate>) -> Result<bool, String> {
    gate.ensure_open()?;
    Ok(core.has_running())
}

#[tauri::command(async)]
fn shutdown(core: State<Core>, gate: State<Gate>) -> Result<(), String> {
    gate.ensure_open()?;
    core.shutdown();
    Ok(())
}

/// Estado da atualização da abertura: a tela lê isto ao carregar e depois acompanha o evento.
#[tauri::command(async)]
fn update_status(gate: State<Gate>) -> Result<update::Status, String> {
    Ok(gate.status())
}

/// Nova tentativa depois de uma falha. Enquanto uma verificação roda, não faz nada.
#[tauri::command(async)]
fn retry_update(app: tauri::AppHandle, gate: State<Gate>) -> Result<update::Status, String> {
    update::check(&app, &gate);
    Ok(gate.status())
}

pub fn run(context: tauri::Context) {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let emit: core::Emit = Arc::new(move |event, payload| drop(handle.emit(event, payload)));
            // Sem LOCALAPPDATA o app abre mesmo assim; o erro aparece em `get_options`.
            let cfg_path = config::config_path().unwrap_or_default();
            app.manage(Core::new(cli::Exes::real(), session::Places::real(), cfg_path, emit.clone()));
            // Build de desenvolvimento libera o uso direto. O instalado confere a versão antes de liberar.
            let gate = Gate::new(cfg!(debug_assertions), emit);
            app.manage(gate.clone());
            update::check(app.handle(), &gate);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            choose_project,
            get_options,
            list_dir,
            read_file,
            write_file,
            rename_file,
            delete_file,
            reveal_file,
            reveal_dir,
            open_with,
            copy_file,
            create_file,
            create_dir,
            clipboard_snapshot,
            paste_files,
            search,
            git_status,
            git_diff,
            git_stage,
            git_unstage,
            git_commit,
            git_fetch,
            git_pull,
            git_push,
            git_branches,
            git_switch,
            git_create_branch,
            start_orchestrator,
            start_shell,
            close_shell,
            set_executor_choice,
            term_write,
            term_resize,
            term_snapshot,
            clipboard_files,
            stop_executor,
            clear_done,
            has_running,
            shutdown,
            update_status,
            retry_update
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
