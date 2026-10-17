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
pub mod mcp;
pub mod proc;
pub mod pty;
pub mod roles;
pub mod search;
pub mod session;
pub mod update;
#[cfg(test)]
mod testutil;

use crate::config::{Choice, Options};
use crate::core::{Core, Opened, Snapshot};
use crate::update::Gate;
use std::sync::Arc;
use tauri::ipc::Invoke;
use tauri::{Emitter, Manager, State, Wry};
use tauri_plugin_dialog::DialogExt;

// Todos os comandos são `async` para rodar fora da thread da janela: vários esperam processo
// ou disco, e o diálogo de pasta trava quem o chama.

/// Exceto `update_status` e `retry_update`, todo comando passa por `Gate::ensure_open` aqui, antes de rodar: com a versão
/// fora de dia, nada do projeto, dos terminais ou da configuração é acessível. Precisa ser `fn`: num `let`, o closure do
/// `generate_handler!` não tem o tipo inferido.
fn gated(commands: impl Fn(Invoke<Wry>) -> bool + Send + Sync + 'static) -> impl Fn(Invoke<Wry>) -> bool + Send + Sync + 'static {
    move |invoke| {
        if !matches!(invoke.message.command(), "update_status" | "retry_update") {
            if let Err(e) = invoke.message.webview().state::<Gate>().ensure_open() {
                invoke.resolver.reject(e);
                return true;
            }
        }
        commands(invoke)
    }
}

#[tauri::command(async)]
fn choose_project(app: tauri::AppHandle, core: State<Core>) -> Result<Option<Opened>, String> {
    match app.dialog().file().set_title("Pasta do projeto").blocking_pick_folder() {
        None => Ok(None),
        Some(folder) => core.set_project(&folder.into_path().map_err(|e| e.to_string())?).map(Some),
    }
}

#[tauri::command(async)]
fn get_options(core: State<Core>) -> Result<Options, String> {
    core.options()
}

/// Um nível de uma pasta do projeto para o explorador. `path` é relativo ao projeto ("" é a raiz).
#[tauri::command(async)]
fn list_dir(core: State<Core>, path: String) -> Result<Vec<files::Entry>, String> {
    files::list(&core.repo()?, &path)
}

/// O conteúdo de um arquivo do projeto para a tela. `path` é relativo ao projeto.
#[tauri::command(async)]
fn read_file(core: State<Core>, path: String) -> Result<files::Content, String> {
    files::read(&core.repo()?, &path)
}

/// Bytes de uma imagem ou áudio do projeto para a aba de mídia. `path` é relativo ao projeto. Vai como bytes crus (o
/// frontend recebe `ArrayBuffer`), sem JSON nem base64; o tipo é o da extensão, decidido na tela.
#[tauri::command(async)]
fn read_media(core: State<Core>, path: String) -> Result<tauri::ipc::Response, String> {
    files::read_media(&core.repo()?, &path).map(tauri::ipc::Response::new)
}

/// Grava `text` só se o arquivo ainda é `base` (o texto lido, sem BOM) e ainda tem o BOM `bom`. Recusa sem gravar
/// se mudou por fora.
#[tauri::command(async)]
fn write_file_checked(core: State<Core>, path: String, text: String, base: String, bom: bool) -> Result<(), String> {
    files::write_checked(&core.repo()?, &path, &text, &base, bom)
}

/// Busca `query.text` nos arquivos do projeto (rastreados e não ignorados; sem binário, não UTF-8 nem link). Consulta
/// vazia não busca nada; padrão inválido volta como erro em português.
#[tauri::command(async)]
fn search(core: State<Core>, query: search::Query) -> Result<search::Results, String> {
    search::search(&core.repo()?, &query)
}

/// Abertura rápida: os arquivos cujo caminho casa com `term` (termos separados por espaço).
#[tauri::command(async)]
fn quick_open(core: State<Core>, term: String) -> Result<search::Quick, String> {
    search::quick_open(&core.repo()?, &term)
}

/// A prévia da substituição de `query` por `replacement`. Não grava nada.
#[tauri::command(async)]
fn replace_preview(core: State<Core>, query: search::Query, replacement: String) -> Result<search::Preview, String> {
    search::preview(&core.repo()?, &query, &replacement)
}

/// Grava a substituição só nos arquivos escolhidos da prévia, com a revisão que ela mostrou. Erros são por arquivo.
#[tauri::command(async)]
fn replace_apply(core: State<Core>, request: search::ApplyRequest) -> Result<search::Applied, String> {
    search::apply(&core.repo()?, &request)
}

/// O estado do Git do projeto para o painel: com repositório, a branch, ahead/behind e os arquivos alterados; sem
/// ele, só o motivo (sem repositório, sem Git, ou o repositório fica acima do projeto).
#[tauri::command(async)]
fn git_status(core: State<Core>) -> Result<git::Estado, String> {
    git::estado(&core.repo()?)
}

/// Cria o repositório na pasta do projeto, só quando ela não está em nenhum. Sem commit e sem stage.
#[tauri::command(async)]
fn git_init(core: State<Core>) -> Result<(), String> {
    git::init(&core.repo()?)
}

/// Prepara os caminhos. Lista vazia prepara tudo, inclusive o não rastreado e o apagado.
#[tauri::command(async)]
fn git_stage(core: State<Core>, paths: Vec<String>) -> Result<(), String> {
    git::stage(&core.repo()?, &paths)
}

/// Tira os caminhos do índice. Lista vazia tira tudo.
#[tauri::command(async)]
fn git_unstage(core: State<Core>, paths: Vec<String>) -> Result<(), String> {
    git::unstage(&core.repo()?, &paths)
}

#[tauri::command(async)]
fn git_commit(core: State<Core>, message: String) -> Result<(), String> {
    git::commit(&core.repo()?, &message)
}

/// A saída do `git fetch`, para a tela mostrar.
#[tauri::command(async)]
fn git_fetch(core: State<Core>) -> Result<String, String> {
    git::fetch(&core.repo()?)
}

/// Diff de um arquivo com a revisão e os blocos (`hunks`). A tela devolve a revisão e o bloco nas ações por bloco.
#[tauri::command(async)]
fn git_review_diff(core: State<Core>, path: String, staged: bool) -> Result<git::ReviewDiff, String> {
    git::review_diff(&core.repo()?, &path, staged)
}

/// Prepara um bloco da pasta de trabalho. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_stage_hunk(core: State<Core>, hunk: git::HunkRef) -> Result<(), String> {
    core.git_idle(|dir| git::stage_hunk(dir, &hunk))
}

/// Tira um bloco do índice. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_unstage_hunk(core: State<Core>, hunk: git::HunkRef) -> Result<(), String> {
    core.git_idle(|dir| git::unstage_hunk(dir, &hunk))
}

/// Descarta um bloco da alteração não preparada. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_discard_hunk(core: State<Core>, hunk: git::HunkRef) -> Result<(), String> {
    core.git_idle(|dir| git::discard_hunk(dir, &hunk))
}

/// Descarta a alteração não preparada do arquivo, se ele ainda está na `revision` mostrada. Recusado com executor ativo.
#[tauri::command(async)]
fn git_discard_file(core: State<Core>, path: String, revision: String) -> Result<(), String> {
    core.git_idle(|dir| git::discard_file(dir, &path, &revision))
}

/// Uma página do histórico (`limit` até 200, `skip` os já mostrados); com `path`, só os commits que tocaram o arquivo.
#[tauri::command(async)]
fn git_history(core: State<Core>, limit: usize, skip: usize, path: Option<String>) -> Result<Vec<git::Commit>, String> {
    git::history(&core.repo()?, limit, skip, path.as_deref())
}

/// Uma página do gráfico: os commits de HEAD e do upstream da branch atual (`limit` até 200, `skip` os já mostrados).
#[tauri::command(async)]
fn git_graph(core: State<Core>, limit: usize, skip: usize) -> Result<Vec<git::GraphCommit>, String> {
    git::graph(&core.repo()?, limit, skip)
}

/// Cabeçalho do commit e os arquivos que ele mudou.
#[tauri::command(async)]
fn git_commit_detail(core: State<Core>, id: String) -> Result<git::CommitDetail, String> {
    git::commit_detail(&core.repo()?, &id)
}

/// O diff de um commit, ou só de um caminho dele.
#[tauri::command(async)]
fn git_commit_diff(core: State<Core>, id: String, path: Option<String>) -> Result<git::Diff, String> {
    git::commit_diff(&core.repo()?, &id, path.as_deref())
}

/// Os lados de um arquivo em conflito, o texto atual da pasta e a revisão.
#[tauri::command(async)]
fn git_conflict(core: State<Core>, path: String) -> Result<git::Conflict, String> {
    git::conflict(&core.repo()?, &path)
}

/// Grava a resolução de um conflito se a pasta ainda está na `revision` lida. Com `markResolved`, prepara o arquivo.
#[tauri::command(async)]
fn git_write_resolution(core: State<Core>, path: String, content: String, revision: String, mark_resolved: bool) -> Result<(), String> {
    core.git_idle(|dir| git::write_resolution(dir, &path, &content, &revision, mark_resolved))
}

/// Aceita a remoção de um arquivo em conflito que um lado apagou, se ainda está na `revision` lida.
#[tauri::command(async)]
fn git_accept_deletion(core: State<Core>, path: String, revision: String) -> Result<(), String> {
    core.git_idle(|dir| git::accept_deletion(dir, &path, &revision))
}

/// Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_pull(core: State<Core>) -> Result<String, String> {
    core.git_idle(git::pull)
}

/// A saída do `git push`, para a tela mostrar. Sem upstream, publica em origin.
#[tauri::command(async)]
fn git_push(core: State<Core>) -> Result<String, String> {
    git::push(&core.repo()?)
}

#[tauri::command(async)]
fn git_branches(core: State<Core>) -> Result<git::Branches, String> {
    git::branches(&core.repo()?)
}

/// Troca para uma branch local existente. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_switch(core: State<Core>, name: String) -> Result<(), String> {
    core.git_idle(|dir| git::switch(dir, &name))
}

/// Cria a branch a partir do HEAD e troca para ela. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_create_branch(core: State<Core>, name: String) -> Result<(), String> {
    core.git_idle(|dir| git::create_branch(dir, &name))
}

#[tauri::command(async)]
fn start_orchestrator(core: State<Core>, choice: Choice, cols: u16, rows: u16) -> Result<(), String> {
    core.start_orchestrator(choice, cols, rows)
}

/// Abre um shell do usuário, no slot `id` (`shell-<n>`), na pasta do projeto ou, com `cwd` (relativo ao projeto), na
/// pasta dele. Recusado se esse id já tiver um aberto. Devolve o programa que abriu.
#[tauri::command(async)]
fn start_shell(core: State<Core>, id: String, cols: u16, rows: u16, cwd: Option<String>) -> Result<String, String> {
    core.start_shell_in(&id, cols, rows, cwd.as_deref())
}

/// Renomeia o arquivo `path` para `name`, na mesma pasta. Devolve o caminho novo. Nunca sobrescreve.
#[tauri::command(async)]
fn rename_file(core: State<Core>, path: String, name: String) -> Result<String, String> {
    files::rename(&core.repo()?, &path, &name)
}

/// Manda os itens `paths` para a Lixeira. Devolve os feitos e o erro do que parou (ver `Batch`).
#[tauri::command(async)]
fn delete_files(core: State<Core>, paths: Vec<String>) -> Result<files::Batch<String>, String> {
    Ok(files::recycle_many(&core.repo()?, &paths))
}

/// Move os itens `paths` para a pasta `dest` (vazio é a raiz). Devolve os movidos e o erro do que parou (ver `Batch`).
#[tauri::command(async)]
fn move_files(core: State<Core>, paths: Vec<String>, dest: String) -> Result<files::Batch<files::Moved>, String> {
    Ok(files::move_items(&core.repo()?, &paths, &dest))
}

/// Põe os itens `paths` no clipboard como copiados (`cut` false) ou recortados. Nada se move aqui.
#[tauri::command(async)]
fn copy_files(core: State<Core>, paths: Vec<String>, cut: bool) -> Result<(), String> {
    files::copy_items(&core.repo()?, &paths, cut)
}

/// Mostra o arquivo `path` selecionado no Explorer do Windows.
#[tauri::command(async)]
fn reveal_file(core: State<Core>, path: String) -> Result<(), String> {
    files::reveal(&core.repo()?, &path)
}

/// Abre o diálogo "Abrir com..." do Windows para o arquivo `path`.
#[tauri::command(async)]
fn open_with(core: State<Core>, path: String) -> Result<(), String> {
    files::open_with(&core.repo()?, &path)
}

/// Cria o arquivo `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo. Nunca sobrescreve.
#[tauri::command(async)]
fn create_file(core: State<Core>, dir: String, name: String) -> Result<String, String> {
    files::create_file(&core.repo()?, &dir, &name)
}

/// Cria a pasta `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo. Nunca sobrescreve.
#[tauri::command(async)]
fn create_dir(core: State<Core>, dir: String, name: String) -> Result<String, String> {
    files::create_dir(&core.repo()?, &dir, &name)
}

/// Abre no Explorer do Windows a pasta `path` do projeto (vazio é a raiz).
#[tauri::command(async)]
fn reveal_dir(core: State<Core>, path: String) -> Result<(), String> {
    files::reveal_dir(&core.repo()?, &path)
}

/// Os arquivos e o efeito (copiar ou recortar) do clipboard agora. Lido na hora de cada ação; sem arquivo, `paths` vem vazio.
#[tauri::command(async)]
fn clipboard_snapshot() -> Result<clipboard::Snapshot, String> {
    Ok(clipboard::snapshot())
}

/// A colagem com o detalhe: cada item colado (`from` absoluto como estava no clipboard, `to` relativo ao projeto), e
/// o erro do que parou, se parou. Ver `Batch`. Se um recorte moveu algo, esvazia o clipboard se ele ainda for `snap`; um
/// clipboard trocado no meio fica como está.
#[tauri::command(async)]
fn paste_files_detailed(core: State<Core>, dest: String, snap: clipboard::Snapshot) -> Result<files::Batch<files::Moved>, String> {
    let result = files::paste_detailed(&core.repo()?, &dest, &snap);
    if snap.cut && !result.done.is_empty() {
        clipboard::clear_if_same(&snap);
    }
    Ok(result)
}

/// Fecha o shell `id` e mata o processo dele.
#[tauri::command(async)]
fn close_shell(core: State<Core>, id: String) -> Result<(), String> {
    core.close_shell(&id);
    Ok(())
}

#[tauri::command(async)]
fn set_executor_choice(core: State<Core>, choice: Choice) -> Result<(), String> {
    core.set_executor_choice(choice)
}

#[tauri::command(async)]
fn term_write(core: State<Core>, id: String, data: String) -> Result<(), String> {
    core.term_write(&id, &data);
    Ok(())
}

/// O CLI que roda agora no terminal `id` (`claude`, `codex` ou nenhum), para a colagem de arquivo e imagem.
#[tauri::command(async)]
fn term_cli(core: State<Core>, id: String) -> Result<Option<&'static str>, String> {
    Ok(core.term_cli(&id))
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
fn has_running(core: State<Core>) -> Result<bool, String> {
    Ok(core.has_running())
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
        .invoke_handler(gated(tauri::generate_handler![
            choose_project,
            get_options,
            list_dir,
            read_file,
            read_media,
            write_file_checked,
            rename_file,
            delete_files,
            move_files,
            copy_files,
            reveal_file,
            reveal_dir,
            open_with,
            create_file,
            create_dir,
            clipboard_snapshot,
            paste_files_detailed,
            search,
            quick_open,
            replace_preview,
            replace_apply,
            git_status,
            git_init,
            git_stage,
            git_unstage,
            git_commit,
            git_fetch,
            git_pull,
            git_push,
            git_branches,
            git_switch,
            git_create_branch,
            git_review_diff,
            git_stage_hunk,
            git_unstage_hunk,
            git_discard_hunk,
            git_discard_file,
            git_history,
            git_graph,
            git_commit_detail,
            git_commit_diff,
            git_conflict,
            git_write_resolution,
            git_accept_deletion,
            start_orchestrator,
            start_shell,
            close_shell,
            set_executor_choice,
            term_write,
            term_cli,
            term_resize,
            term_snapshot,
            stop_executor,
            has_running,
            update_status,
            retry_update
        ]))
        .build(context)
        .expect("o app não pôde ser iniciado")
        .run(|app, event| {
            // Fechar o app mata tudo e limpa a sessão.
            if let tauri::RunEvent::Exit = event {
                app.state::<Core>().shutdown();
            }
        });
}
