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
pub mod issues;
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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

/// A pasta do projeto para um comando, depois de checar que o uso está liberado.
fn repo(core: &Core, gate: &Gate) -> Result<PathBuf, String> {
    gate.ensure_open()?;
    core.repo()
}

/// Roda `run` na pasta do projeto, depois de checar que o uso está liberado e que nenhum executor está trabalhando.
fn idle<T>(core: &Core, gate: &Gate, run: impl FnOnce(&Path) -> Result<T, String>) -> Result<T, String> {
    gate.ensure_open()?;
    core.git_idle(run)
}

// Todos os comandos são `async` para rodar fora da thread da janela: vários esperam processo
// ou disco, e o diálogo de pasta trava quem o chama.
// Exceto `update_status` e `retry_update`, todo comando passa por `Gate::ensure_open` antes de tocar
// no trabalho: com a versão fora de dia, nada do projeto, dos terminais ou da configuração é acessível.

#[tauri::command(async)]
fn choose_project(app: tauri::AppHandle, core: State<Core>, gate: State<Gate>) -> Result<Option<Opened>, String> {
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
    files::list(&repo(&core, &gate)?, &path)
}

/// O conteúdo de um arquivo do projeto para a tela. `path` é relativo ao projeto.
#[tauri::command(async)]
fn read_file(core: State<Core>, gate: State<Gate>, path: String) -> Result<files::Content, String> {
    files::read(&repo(&core, &gate)?, &path)
}

/// Bytes de uma imagem ou áudio do projeto para a aba de mídia. `path` é relativo ao projeto. Vai como bytes crus (o
/// frontend recebe `ArrayBuffer`), sem JSON nem base64; o tipo é o da extensão, decidido na tela.
#[tauri::command(async)]
fn read_media(core: State<Core>, gate: State<Gate>, path: String) -> Result<tauri::ipc::Response, String> {
    files::read_media(&repo(&core, &gate)?, &path).map(tauri::ipc::Response::new)
}

/// Grava `text` só se o arquivo ainda é `base` (o texto lido, sem BOM) e ainda tem o BOM `bom`. Recusa sem gravar
/// se mudou por fora.
#[tauri::command(async)]
fn write_file_checked(core: State<Core>, gate: State<Gate>, path: String, text: String, base: String, bom: bool) -> Result<(), String> {
    files::write_checked(&repo(&core, &gate)?, &path, &text, &base, bom)
}

/// Busca `query.text` nos arquivos do projeto (rastreados e não ignorados; sem binário, não UTF-8 nem link). Consulta
/// vazia não busca nada; padrão inválido volta como erro em português.
#[tauri::command(async)]
fn search(core: State<Core>, gate: State<Gate>, query: search::Query) -> Result<search::Results, String> {
    search::search(&repo(&core, &gate)?, &query)
}

/// Abertura rápida: os arquivos cujo caminho casa com `term` (termos separados por espaço).
#[tauri::command(async)]
fn quick_open(core: State<Core>, gate: State<Gate>, term: String) -> Result<search::Quick, String> {
    search::quick_open(&repo(&core, &gate)?, &term)
}

/// A prévia da substituição de `query` por `replacement`. Não grava nada.
#[tauri::command(async)]
fn replace_preview(core: State<Core>, gate: State<Gate>, query: search::Query, replacement: String) -> Result<search::Preview, String> {
    search::preview(&repo(&core, &gate)?, &query, &replacement)
}

/// Grava a substituição só nos arquivos escolhidos da prévia, com a revisão que ela mostrou. Erros são por arquivo.
#[tauri::command(async)]
fn replace_apply(core: State<Core>, gate: State<Gate>, request: search::ApplyRequest) -> Result<search::Applied, String> {
    search::apply(&repo(&core, &gate)?, &request)
}

/// O estado do Git do projeto para o painel: com repositório, a branch, ahead/behind e os arquivos alterados; sem
/// ele, só o motivo (sem repositório, sem Git, ou o repositório fica acima do projeto).
#[tauri::command(async)]
fn git_status(core: State<Core>, gate: State<Gate>) -> Result<git::Estado, String> {
    git::estado(&repo(&core, &gate)?)
}

/// Cria o repositório na pasta do projeto, só quando ela não está em nenhum. Sem commit e sem stage.
#[tauri::command(async)]
fn git_init(core: State<Core>, gate: State<Gate>) -> Result<(), String> {
    git::init(&repo(&core, &gate)?)
}

/// Prepara os caminhos. Lista vazia prepara tudo, inclusive o não rastreado e o apagado.
#[tauri::command(async)]
fn git_stage(core: State<Core>, gate: State<Gate>, paths: Vec<String>) -> Result<(), String> {
    git::stage(&repo(&core, &gate)?, &paths)
}

/// Tira os caminhos do índice. Lista vazia tira tudo.
#[tauri::command(async)]
fn git_unstage(core: State<Core>, gate: State<Gate>, paths: Vec<String>) -> Result<(), String> {
    git::unstage(&repo(&core, &gate)?, &paths)
}

#[tauri::command(async)]
fn git_commit(core: State<Core>, gate: State<Gate>, message: String) -> Result<(), String> {
    git::commit(&repo(&core, &gate)?, &message)
}

/// A saída do `git fetch`, para a tela mostrar.
#[tauri::command(async)]
fn git_fetch(core: State<Core>, gate: State<Gate>) -> Result<String, String> {
    git::fetch(&repo(&core, &gate)?)
}

/// Diff de um arquivo com a revisão e os blocos (`hunks`). A tela devolve a revisão e o bloco nas ações por bloco.
#[tauri::command(async)]
fn git_review_diff(core: State<Core>, gate: State<Gate>, path: String, staged: bool) -> Result<git::ReviewDiff, String> {
    git::review_diff(&repo(&core, &gate)?, &path, staged)
}

/// Prepara um bloco da pasta de trabalho. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_stage_hunk(core: State<Core>, gate: State<Gate>, hunk: git::HunkRef) -> Result<(), String> {
    idle(&core, &gate, |dir| git::stage_hunk(dir, &hunk))
}

/// Tira um bloco do índice. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_unstage_hunk(core: State<Core>, gate: State<Gate>, hunk: git::HunkRef) -> Result<(), String> {
    idle(&core, &gate, |dir| git::unstage_hunk(dir, &hunk))
}

/// Descarta um bloco da alteração não preparada. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_discard_hunk(core: State<Core>, gate: State<Gate>, hunk: git::HunkRef) -> Result<(), String> {
    idle(&core, &gate, |dir| git::discard_hunk(dir, &hunk))
}

/// Descarta a alteração não preparada do arquivo, se ele ainda está na `revision` mostrada. Recusado com executor ativo.
#[tauri::command(async)]
fn git_discard_file(core: State<Core>, gate: State<Gate>, path: String, revision: String) -> Result<(), String> {
    idle(&core, &gate, |dir| git::discard_file(dir, &path, &revision))
}

/// Uma página do histórico (`limit` até 200, `skip` os já mostrados); com `path`, só os commits que tocaram o arquivo.
#[tauri::command(async)]
fn git_history(core: State<Core>, gate: State<Gate>, limit: usize, skip: usize, path: Option<String>) -> Result<Vec<git::Commit>, String> {
    git::history(&repo(&core, &gate)?, limit, skip, path.as_deref())
}

/// Uma página do gráfico: os commits de HEAD e do upstream da branch atual (`limit` até 200, `skip` os já mostrados).
#[tauri::command(async)]
fn git_graph(core: State<Core>, gate: State<Gate>, limit: usize, skip: usize) -> Result<Vec<git::GraphCommit>, String> {
    git::graph(&repo(&core, &gate)?, limit, skip)
}

/// Cabeçalho do commit e os arquivos que ele mudou.
#[tauri::command(async)]
fn git_commit_detail(core: State<Core>, gate: State<Gate>, id: String) -> Result<git::CommitDetail, String> {
    git::commit_detail(&repo(&core, &gate)?, &id)
}

/// O diff de um commit, ou só de um caminho dele.
#[tauri::command(async)]
fn git_commit_diff(core: State<Core>, gate: State<Gate>, id: String, path: Option<String>) -> Result<git::Diff, String> {
    git::commit_diff(&repo(&core, &gate)?, &id, path.as_deref())
}

/// Os lados de um arquivo em conflito, o texto atual da pasta e a revisão.
#[tauri::command(async)]
fn git_conflict(core: State<Core>, gate: State<Gate>, path: String) -> Result<git::Conflict, String> {
    git::conflict(&repo(&core, &gate)?, &path)
}

/// Grava a resolução de um conflito se a pasta ainda está na `revision` lida. Com `markResolved`, prepara o arquivo.
#[tauri::command(async)]
fn git_write_resolution(core: State<Core>, gate: State<Gate>, path: String, content: String, revision: String, mark_resolved: bool) -> Result<(), String> {
    idle(&core, &gate, |dir| git::write_resolution(dir, &path, &content, &revision, mark_resolved))
}

/// Aceita a remoção de um arquivo em conflito que um lado apagou, se ainda está na `revision` lida.
#[tauri::command(async)]
fn git_accept_deletion(core: State<Core>, gate: State<Gate>, path: String, revision: String) -> Result<(), String> {
    idle(&core, &gate, |dir| git::accept_deletion(dir, &path, &revision))
}

/// Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_pull(core: State<Core>, gate: State<Gate>) -> Result<String, String> {
    idle(&core, &gate, git::pull)
}

/// A saída do `git push`, para a tela mostrar. Sem upstream, publica em origin.
#[tauri::command(async)]
fn git_push(core: State<Core>, gate: State<Gate>) -> Result<String, String> {
    git::push(&repo(&core, &gate)?)
}

#[tauri::command(async)]
fn git_branches(core: State<Core>, gate: State<Gate>) -> Result<git::Branches, String> {
    git::branches(&repo(&core, &gate)?)
}

/// Troca para uma branch local existente. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_switch(core: State<Core>, gate: State<Gate>, name: String) -> Result<(), String> {
    idle(&core, &gate, |dir| git::switch(dir, &name))
}

/// Cria a branch a partir do HEAD e troca para ela. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_create_branch(core: State<Core>, gate: State<Gate>, name: String) -> Result<(), String> {
    idle(&core, &gate, |dir| git::create_branch(dir, &name))
}

/// A pasta dos arquivos de issues e o projeto, depois de checar que o uso está liberado.
fn issues_at(core: &Core, gate: &Gate) -> Result<(PathBuf, PathBuf), String> {
    let project = repo(core, gate)?;
    Ok((issues::dir()?, project))
}

/// As issues do projeto (só neste computador), a em andamento e a próxima. Sem arquivo, nenhuma.
#[tauri::command(async)]
fn issues_load(core: State<Core>, gate: State<Gate>) -> Result<issues::Dados, String> {
    let (dir, project) = issues_at(&core, &gate)?;
    issues::load(&dir, &project)
}

/// Cria a issue aberta, com o número seguinte, e a devolve.
#[tauri::command(async)]
fn issues_create(core: State<Core>, gate: State<Gate>, title: String, body: String, labels: Vec<String>) -> Result<issues::Issue, String> {
    let (dir, project) = issues_at(&core, &gate)?;
    issues::create(&dir, &project, &title, &body, &labels)
}

/// Troca o título, a descrição e as labels da issue `number` e a devolve.
#[tauri::command(async)]
fn issues_update(core: State<Core>, gate: State<Gate>, number: u64, title: String, body: String, labels: Vec<String>) -> Result<issues::Issue, String> {
    let (dir, project) = issues_at(&core, &gate)?;
    issues::update(&dir, &project, number, &title, &body, &labels)
}

/// Reabre (`open` true) ou fecha a issue `number` e a devolve.
#[tauri::command(async)]
fn issues_set_state(core: State<Core>, gate: State<Gate>, number: u64, open: bool) -> Result<issues::Issue, String> {
    let (dir, project) = issues_at(&core, &gate)?;
    issues::set_state(&dir, &project, number, open)
}

/// Apaga a issue `number` de vez; a em andamento e a próxima que eram ela perdem a marca.
#[tauri::command(async)]
fn issues_delete(core: State<Core>, gate: State<Gate>, number: u64) -> Result<(), String> {
    let (dir, project) = issues_at(&core, &gate)?;
    issues::delete(&dir, &project, number)
}

/// Guarda a issue em andamento e a próxima no arquivo do projeto.
#[tauri::command(async)]
fn issues_set_tracked(core: State<Core>, gate: State<Gate>, active: Option<issues::Active>, next: Option<u64>) -> Result<(), String> {
    let (dir, project) = issues_at(&core, &gate)?;
    issues::set_tracked(&dir, &project, active, next)
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
    files::rename(&repo(&core, &gate)?, &path, &name)
}

/// Manda os itens `paths` para a Lixeira. Devolve os feitos e o erro do que parou (ver `Batch`).
#[tauri::command(async)]
fn delete_files(core: State<Core>, gate: State<Gate>, paths: Vec<String>) -> Result<files::Batch<String>, String> {
    Ok(files::recycle_many(&repo(&core, &gate)?, &paths))
}

/// Move os itens `paths` para a pasta `dest` (vazio é a raiz). Devolve os movidos e o erro do que parou (ver `Batch`).
#[tauri::command(async)]
fn move_files(core: State<Core>, gate: State<Gate>, paths: Vec<String>, dest: String) -> Result<files::Batch<files::Moved>, String> {
    Ok(files::move_items(&repo(&core, &gate)?, &paths, &dest))
}

/// Põe os itens `paths` no clipboard como copiados (`cut` false) ou recortados. Nada se move aqui.
#[tauri::command(async)]
fn copy_files(core: State<Core>, gate: State<Gate>, paths: Vec<String>, cut: bool) -> Result<(), String> {
    files::copy_items(&repo(&core, &gate)?, &paths, cut)
}

/// Mostra o arquivo `path` selecionado no Explorer do Windows.
#[tauri::command(async)]
fn reveal_file(core: State<Core>, gate: State<Gate>, path: String) -> Result<(), String> {
    files::reveal(&repo(&core, &gate)?, &path)
}

/// Abre o diálogo "Abrir com..." do Windows para o arquivo `path`.
#[tauri::command(async)]
fn open_with(core: State<Core>, gate: State<Gate>, path: String) -> Result<(), String> {
    files::open_with(&repo(&core, &gate)?, &path)
}

/// Cria o arquivo `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo. Nunca sobrescreve.
#[tauri::command(async)]
fn create_file(core: State<Core>, gate: State<Gate>, dir: String, name: String) -> Result<String, String> {
    files::create_file(&repo(&core, &gate)?, &dir, &name)
}

/// Cria a pasta `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo. Nunca sobrescreve.
#[tauri::command(async)]
fn create_dir(core: State<Core>, gate: State<Gate>, dir: String, name: String) -> Result<String, String> {
    files::create_dir(&repo(&core, &gate)?, &dir, &name)
}

/// Abre no Explorer do Windows a pasta `path` do projeto (vazio é a raiz).
#[tauri::command(async)]
fn reveal_dir(core: State<Core>, gate: State<Gate>, path: String) -> Result<(), String> {
    files::reveal_dir(&repo(&core, &gate)?, &path)
}

/// Os arquivos e o efeito (copiar ou recortar) do clipboard agora. Lido na hora de cada ação; sem arquivo, `paths` vem vazio.
#[tauri::command(async)]
fn clipboard_snapshot(gate: State<Gate>) -> Result<clipboard::Snapshot, String> {
    gate.ensure_open()?;
    Ok(clipboard::snapshot())
}

/// Cola `snap` e, se um recorte moveu algo, esvazia o clipboard se ele ainda for `snap`. Um clipboard trocado no meio
/// fica como está.
fn paste_and_clear(project: &Path, dest: &str, snap: &clipboard::Snapshot) -> files::Batch<files::Pasted> {
    let result = files::paste_detailed(project, dest, snap);
    if snap.cut && !result.done.is_empty() {
        clipboard::clear_if_same(snap);
    }
    result
}

/// A colagem com o detalhe: cada item colado (`from` absoluto como estava no clipboard, `to` relativo ao projeto), e
/// o erro do que parou, se parou. Ver `Batch`.
#[tauri::command(async)]
fn paste_files_detailed(core: State<Core>, gate: State<Gate>, dest: String, snap: clipboard::Snapshot) -> Result<files::Batch<files::Pasted>, String> {
    Ok(paste_and_clear(&repo(&core, &gate)?, &dest, &snap))
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

#[tauri::command(async)]
fn stop_executor(core: State<Core>, gate: State<Gate>, id: String) -> Result<(), String> {
    gate.ensure_open()?;
    core.stop_executor(&id);
    Ok(())
}

/// Se o orquestrador está numa rodada agora: a tela lê isto ao montar e depois acompanha `orchestrator-turn`.
#[tauri::command(async)]
fn orchestrator_busy(core: State<Core>, gate: State<Gate>) -> Result<bool, String> {
    gate.ensure_open()?;
    Ok(core.orchestrator_busy())
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
            issues_load,
            issues_create,
            issues_update,
            issues_set_state,
            issues_delete,
            issues_set_tracked,
            start_orchestrator,
            start_shell,
            close_shell,
            set_executor_choice,
            term_write,
            term_resize,
            term_snapshot,
            stop_executor,
            orchestrator_busy,
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
