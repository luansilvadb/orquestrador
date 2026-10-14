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
use serde::Serialize;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

/// Resultado de uma ação em lote: o que foi feito (`done`) e o erro do que parou (`error`, `null` se tudo deu certo).
/// Vem sempre como resposta, nunca como erro: a tela precisa do `done` também quando a operação falhou no meio.
#[derive(Serialize)]
struct Batch<T> {
    done: Vec<T>,
    error: Option<String>,
}

fn batch<T>(result: Result<Vec<T>, files::Partial<T>>) -> Batch<T> {
    match result {
        Ok(done) => Batch { done, error: None },
        Err(partial) => Batch { done: partial.done, error: Some(partial.error) },
    }
}

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

/// Grava `text` só se o arquivo ainda é `base` (o texto lido, sem BOM) e ainda tem o BOM `bom`. Recusa sem gravar
/// se mudou por fora. Use no lugar de `write_file` quando há uma leitura anterior.
#[tauri::command(async)]
fn write_file_checked(core: State<Core>, gate: State<Gate>, path: String, text: String, base: String, bom: bool) -> Result<(), String> {
    gate.ensure_open()?;
    files::write_checked(&core.repo()?, &path, &text, &base, bom)
}

/// Busca `query.text` nos arquivos do projeto (rastreados e não ignorados; sem binário, não UTF-8 nem link). Consulta
/// vazia não busca nada; padrão inválido volta como erro em português.
#[tauri::command(async)]
fn search(core: State<Core>, gate: State<Gate>, query: search::Query) -> Result<search::Results, String> {
    gate.ensure_open()?;
    search::search(&core.repo()?, &query)
}

/// Abertura rápida: os arquivos cujo caminho casa com `term` (termos separados por espaço).
#[tauri::command(async)]
fn quick_open(core: State<Core>, gate: State<Gate>, term: String) -> Result<search::Quick, String> {
    gate.ensure_open()?;
    search::quick_open(&core.repo()?, &term)
}

/// A prévia da substituição de `query` por `replacement`. Não grava nada.
#[tauri::command(async)]
fn replace_preview(core: State<Core>, gate: State<Gate>, query: search::Query, replacement: String) -> Result<search::Preview, String> {
    gate.ensure_open()?;
    search::preview(&core.repo()?, &query, &replacement)
}

/// Grava a substituição só nos arquivos escolhidos da prévia, com a revisão que ela mostrou. Erros são por arquivo.
#[tauri::command(async)]
fn replace_apply(core: State<Core>, gate: State<Gate>, request: search::ApplyRequest) -> Result<search::Applied, String> {
    gate.ensure_open()?;
    search::apply(&core.repo()?, &request)
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

/// Diff de um arquivo com a revisão e os blocos (`hunks`). A tela devolve a revisão e o bloco nas ações por bloco.
#[tauri::command(async)]
fn git_review_diff(core: State<Core>, gate: State<Gate>, path: String, staged: bool) -> Result<git::ReviewDiff, String> {
    gate.ensure_open()?;
    git::review_diff(&core.repo()?, &path, staged)
}

/// Prepara um bloco da pasta de trabalho. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_stage_hunk(core: State<Core>, gate: State<Gate>, hunk: git::HunkRef) -> Result<(), String> {
    gate.ensure_open()?;
    core.git_idle(|dir| git::stage_hunk(dir, &hunk))
}

/// Tira um bloco do índice. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_unstage_hunk(core: State<Core>, gate: State<Gate>, hunk: git::HunkRef) -> Result<(), String> {
    gate.ensure_open()?;
    core.git_idle(|dir| git::unstage_hunk(dir, &hunk))
}

/// Descarta um bloco da alteração não preparada. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_discard_hunk(core: State<Core>, gate: State<Gate>, hunk: git::HunkRef) -> Result<(), String> {
    gate.ensure_open()?;
    core.git_idle(|dir| git::discard_hunk(dir, &hunk))
}

/// Descarta a alteração não preparada do arquivo, se ele ainda está na `revision` mostrada. Recusado com executor ativo.
#[tauri::command(async)]
fn git_discard_file(core: State<Core>, gate: State<Gate>, path: String, revision: String) -> Result<(), String> {
    gate.ensure_open()?;
    core.git_idle(|dir| git::discard_file(dir, &path, &revision))
}

/// Uma página do histórico (`limit` até 200, `skip` os já mostrados); com `path`, só os commits que tocaram o arquivo.
#[tauri::command(async)]
fn git_history(core: State<Core>, gate: State<Gate>, limit: usize, skip: usize, path: Option<String>) -> Result<Vec<git::Commit>, String> {
    gate.ensure_open()?;
    git::history(&core.repo()?, limit, skip, path.as_deref())
}

/// Uma página do gráfico: os commits de HEAD e do upstream da branch atual (`limit` até 200, `skip` os já mostrados).
#[tauri::command(async)]
fn git_graph(core: State<Core>, gate: State<Gate>, limit: usize, skip: usize) -> Result<Vec<git::GraphCommit>, String> {
    gate.ensure_open()?;
    git::graph(&core.repo()?, limit, skip)
}

/// Cabeçalho do commit e os arquivos que ele mudou.
#[tauri::command(async)]
fn git_commit_detail(core: State<Core>, gate: State<Gate>, id: String) -> Result<git::CommitDetail, String> {
    gate.ensure_open()?;
    git::commit_detail(&core.repo()?, &id)
}

/// O diff de um commit, ou só de um caminho dele.
#[tauri::command(async)]
fn git_commit_diff(core: State<Core>, gate: State<Gate>, id: String, path: Option<String>) -> Result<git::Diff, String> {
    gate.ensure_open()?;
    git::commit_diff(&core.repo()?, &id, path.as_deref())
}

/// Os lados de um arquivo em conflito, o texto atual da pasta e a revisão.
#[tauri::command(async)]
fn git_conflict(core: State<Core>, gate: State<Gate>, path: String) -> Result<git::Conflict, String> {
    gate.ensure_open()?;
    git::conflict(&core.repo()?, &path)
}

/// Grava a resolução de um conflito se a pasta ainda está na `revision` lida. Com `markResolved`, prepara o arquivo.
#[tauri::command(async)]
fn git_write_resolution(core: State<Core>, gate: State<Gate>, path: String, content: String, revision: String, mark_resolved: bool) -> Result<(), String> {
    gate.ensure_open()?;
    core.git_idle(|dir| git::write_resolution(dir, &path, &content, &revision, mark_resolved))
}

/// Aceita a remoção de um arquivo em conflito que um lado apagou, se ainda está na `revision` lida.
#[tauri::command(async)]
fn git_accept_deletion(core: State<Core>, gate: State<Gate>, path: String, revision: String) -> Result<(), String> {
    gate.ensure_open()?;
    core.git_idle(|dir| git::accept_deletion(dir, &path, &revision))
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

/// Manda os itens `paths` para a Lixeira. Devolve os feitos e o erro do que parou (ver `Batch`).
#[tauri::command(async)]
fn delete_files(core: State<Core>, gate: State<Gate>, paths: Vec<String>) -> Result<Batch<String>, String> {
    gate.ensure_open()?;
    Ok(batch(files::recycle_many(&core.repo()?, &paths)))
}

/// Move os itens `paths` para a pasta `dest` (vazio é a raiz). Devolve os movidos e o erro do que parou (ver `Batch`).
#[tauri::command(async)]
fn move_files(core: State<Core>, gate: State<Gate>, paths: Vec<String>, dest: String) -> Result<Batch<files::Moved>, String> {
    gate.ensure_open()?;
    Ok(batch(files::move_items(&core.repo()?, &paths, &dest)))
}

/// Põe os itens `paths` no clipboard como copiados (`cut` false) ou recortados. Nada se move aqui.
#[tauri::command(async)]
fn copy_files(core: State<Core>, gate: State<Gate>, paths: Vec<String>, cut: bool) -> Result<(), String> {
    gate.ensure_open()?;
    files::copy_items(&core.repo()?, &paths, cut)
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

/// Cola `snap` e, se um recorte moveu algo, esvazia o clipboard se ele ainda for `snap`. Um clipboard trocado no meio
/// fica como está. Vale para as duas colagens (com e sem o detalhe do que foi feito).
fn paste_and_clear(project: &std::path::Path, dest: &str, snap: &clipboard::Snapshot) -> Result<Vec<files::Pasted>, files::Partial<files::Pasted>> {
    let result = files::paste_detailed(project, dest, snap);
    let moved = match &result {
        Ok(done) => !done.is_empty(),
        Err(partial) => !partial.done.is_empty(),
    };
    if snap.cut && moved {
        clipboard::clear_if_same(snap);
    }
    result
}

/// Cola `snap` na pasta `dest` do projeto (vazio é a raiz). `snap` é a lista lida na ação, a mesma que o frontend
/// usou para checar alterações não salvas. Mantém o contrato antigo: devolve os caminhos criados ou o erro.
#[tauri::command(async)]
fn paste_files(core: State<Core>, gate: State<Gate>, dest: String, snap: clipboard::Snapshot) -> Result<Vec<String>, String> {
    gate.ensure_open()?;
    match paste_and_clear(&core.repo()?, &dest, &snap) {
        Ok(done) => Ok(done.into_iter().map(|p| p.to).collect()),
        Err(partial) => Err(partial.error),
    }
}

/// A colagem com o detalhe: cada item colado (`from` absoluto como estava no clipboard, `to` relativo ao projeto), e
/// o erro do que parou, se parou. Ver `Batch`.
#[tauri::command(async)]
fn paste_files_detailed(core: State<Core>, gate: State<Gate>, dest: String, snap: clipboard::Snapshot) -> Result<Batch<files::Pasted>, String> {
    gate.ensure_open()?;
    Ok(batch(paste_and_clear(&core.repo()?, &dest, &snap)))
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
            write_file_checked,
            rename_file,
            delete_file,
            delete_files,
            move_files,
            copy_files,
            reveal_file,
            reveal_dir,
            open_with,
            copy_file,
            create_file,
            create_dir,
            clipboard_snapshot,
            paste_files,
            paste_files_detailed,
            search,
            quick_open,
            replace_preview,
            replace_apply,
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
