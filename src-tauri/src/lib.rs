//! Backend do app Orquestrador: terminais em que um CLI orquestra e outros executam.
pub mod agents;
pub mod config;
pub mod core;
pub mod ide;
pub mod open_paths;
pub mod platform;
pub mod proc;
pub mod update;
#[cfg(test)]
mod testutil;

use crate::agents::{cli, session};
use crate::config::{Choice, Options};
use crate::core::projects::{Opened, ProjectSnapshot, Projects};
use crate::core::Snapshot;
use crate::ide::{files, format, git, search};
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
fn choose_project(app: tauri::AppHandle, projects: State<Projects>) -> Result<Option<Opened>, String> {
    match app.dialog().file().set_title("Pasta do projeto").blocking_pick_folder() {
        None => Ok(None),
        Some(folder) => projects.open_project(&folder.into_path().map_err(|e| e.to_string())?).map(Some),
    }
}

#[tauri::command(async)]
fn open_project(projects: State<Projects>, path: String) -> Result<Opened, String> {
    projects.open_project(std::path::Path::new(&path))
}

#[tauri::command(async)]
fn close_project(projects: State<Projects>, project: String) -> Result<(), String> {
    projects.close_project(&project)
}

#[tauri::command(async)]
fn project_snapshot(projects: State<Projects>, project: String) -> Result<ProjectSnapshot, String> {
    projects.snapshot(&project)
}

#[tauri::command(async)]
fn pick_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    match app.dialog().file().set_title("Abrir arquivo").blocking_pick_file() {
        None => Ok(None),
        Some(file) => Ok(Some(file.into_path().map_err(|e| e.to_string())?.display().to_string())),
    }
}

#[tauri::command(async)]
fn pick_save_path(app: tauri::AppHandle, directory: Option<String>, file_name: String) -> Result<Option<String>, String> {
    let mut dialog = app.dialog().file().set_title("Salvar como").set_file_name(file_name);
    if let Some(directory) = directory {
        dialog = dialog.set_directory(directory);
    }
    match dialog.blocking_save_file() {
        None => Ok(None),
        Some(file) => Ok(Some(file.into_path().map_err(|e| e.to_string())?.display().to_string())),
    }
}

#[tauri::command(async)]
fn get_options(projects: State<Projects>) -> Result<Options, String> {
    projects.options()
}

/// Um nível de uma pasta do projeto para o explorador. `path` é relativo ao projeto ("" é a raiz).
#[tauri::command(async)]
fn list_dir(projects: State<Projects>, project: String, path: String) -> Result<Vec<files::Entry>, String> {
    let core = projects.core(&project)?;
    files::list(&core.repo()?, &path)
}

/// O conteúdo de um arquivo do projeto para a tela. `path` é relativo ao projeto.
#[tauri::command(async)]
fn read_file(projects: State<Projects>, project: String, path: String) -> Result<files::Content, String> {
    let core = projects.core(&project)?;
    files::read(&core.repo()?, &path)
}

/// Conteúdo de um arquivo absoluto que o usuário abriu ou escolheu para o editor.
#[tauri::command(async)]
fn read_file_at(path: String) -> Result<files::Content, String> {
    files::read_at(std::path::Path::new(&path))
}

/// Bytes de uma imagem do projeto para a aba de mídia. `path` é relativo ao projeto. Vai como bytes crus (o
/// frontend recebe `ArrayBuffer`), sem JSON nem base64; o tipo é o da extensão, decidido na tela.
#[tauri::command(async)]
fn read_media(projects: State<Projects>, project: String, path: String) -> Result<tauri::ipc::Response, String> {
    let core = projects.core(&project)?;
    files::read_media(&core.repo()?, &path).map(tauri::ipc::Response::new)
}

/// Bytes de uma imagem aberta pelo caminho absoluto escolhido pelo usuário.
#[tauri::command(async)]
fn read_media_at(path: String) -> Result<tauri::ipc::Response, String> {
    files::read_media_at(std::path::Path::new(&path)).map(tauri::ipc::Response::new)
}

/// Responde ao protocolo `orq-media`, que serve vídeo e áudio em pedaços. Só a query importa: `path`, relativo ao `project` quando
/// ele vem, ou absoluto quando não vem.
fn media_protocol(app: &tauri::AppHandle, request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let (mut path, mut project) = (None, None);
    if let Ok(url) = tauri::Url::parse(&request.uri().to_string()) {
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "path" => path = Some(value.into_owned()),
                "project" => project = Some(value.into_owned()),
                _ => {}
            }
        }
    }
    let file = match (path, project) {
        (None, _) => Err((403, "Falta o caminho do arquivo.".to_string())),
        (Some(path), Some(project)) => app
            .state::<Projects>()
            .core(&project)
            .and_then(|core| core.repo())
            .map_err(|e| (403, e))
            .and_then(|repo| files::stream_path(&repo, &path)),
        (Some(path), None) => files::stream_path_at(std::path::Path::new(&path)),
    };
    let range = request.headers().get(tauri::http::header::RANGE).and_then(|v| v.to_str().ok());
    match file {
        Ok(file) => files::serve_stream(&file, range),
        Err((status, message)) => files::text_response(status, message),
    }
}

/// Diz qual formatador serve ao caminho absoluto e devolve as opções locais do Prettier.
#[tauri::command(async)]
fn formatter_info(path: String) -> Result<format::Info, String> {
    format::info(std::path::Path::new(&path))
}

/// Formata o texto recebido pela entrada padrão do CLI, no diretório do arquivo.
#[tauri::command(async)]
fn format_cli(path: String, text: String) -> Result<String, String> {
    format::format_cli(std::path::Path::new(&path), &text)
}

/// Grava `text` só se o arquivo ainda é `base` (o texto lido, sem BOM) e ainda tem o BOM `bom`. Recusa sem gravar
/// se mudou por fora.
#[tauri::command(async)]
fn write_file_checked(
    projects: State<Projects>, project: String, path: String, text: String, base: String, bom: bool,
) -> Result<(), String> {
    let core = projects.core(&project)?;
    files::write_checked(&core.repo()?, &path, &text, &base, bom)
}

/// Grava um arquivo absoluto que o usuário abriu ou escolheu, só se não mudou desde a leitura.
#[tauri::command(async)]
fn write_file_at_checked(path: String, text: String, base: String, bom: bool) -> Result<(), String> {
    files::write_checked_at(std::path::Path::new(&path), &text, &base, bom)
}

/// Busca `query.text` nos arquivos do projeto (rastreados e não ignorados; sem binário, não UTF-8 nem link). Consulta
/// vazia não busca nada; padrão inválido volta como erro em português.
#[tauri::command(async)]
fn search(projects: State<Projects>, project: String, query: search::Query) -> Result<search::Results, String> {
    let core = projects.core(&project)?;
    search::search(&core.repo()?, &query)
}

/// Abertura rápida: os arquivos cujo caminho casa com `term` (termos separados por espaço).
#[tauri::command(async)]
fn quick_open(projects: State<Projects>, project: String, term: String) -> Result<search::Quick, String> {
    let core = projects.core(&project)?;
    search::quick_open(&core.repo()?, &term)
}

/// A prévia da substituição de `query` por `replacement`. Não grava nada.
#[tauri::command(async)]
fn replace_preview(
    projects: State<Projects>, project: String, query: search::Query, replacement: String,
) -> Result<search::Preview, String> {
    let core = projects.core(&project)?;
    search::preview(&core.repo()?, &query, &replacement)
}

/// Grava a substituição só nos arquivos escolhidos da prévia, com a revisão que ela mostrou. Erros são por arquivo.
#[tauri::command(async)]
fn replace_apply(
    projects: State<Projects>, project: String, request: search::ApplyRequest,
) -> Result<search::Applied, String> {
    let core = projects.core(&project)?;
    search::apply(&core.repo()?, &request)
}

/// O estado do Git do projeto para o painel: com repositório, a branch, ahead/behind e os arquivos alterados; sem
/// ele, só o motivo (sem repositório, sem Git, ou o repositório fica acima do projeto).
#[tauri::command(async)]
fn git_status(projects: State<Projects>, project: String) -> Result<git::Estado, String> {
    let core = projects.core(&project)?;
    git::estado(&core.repo()?)
}

/// Cria o repositório na pasta do projeto, só quando ela não está em nenhum. Sem commit e sem stage.
#[tauri::command(async)]
fn git_init(projects: State<Projects>, project: String) -> Result<(), String> {
    let core = projects.core(&project)?;
    git::init(&core.repo()?)
}

/// Prepara os caminhos. Lista vazia prepara tudo, inclusive o não rastreado e o apagado.
#[tauri::command(async)]
fn git_stage(projects: State<Projects>, project: String, paths: Vec<String>) -> Result<(), String> {
    let core = projects.core(&project)?;
    git::stage(&core.repo()?, &paths)
}

/// Tira os caminhos do índice. Lista vazia tira tudo.
#[tauri::command(async)]
fn git_unstage(projects: State<Projects>, project: String, paths: Vec<String>) -> Result<(), String> {
    let core = projects.core(&project)?;
    git::unstage(&core.repo()?, &paths)
}

#[tauri::command(async)]
fn git_commit(projects: State<Projects>, project: String, message: String) -> Result<(), String> {
    let core = projects.core(&project)?;
    git::commit(&core.repo()?, &message)
}

/// A saída do `git fetch`, para a tela mostrar.
#[tauri::command(async)]
fn git_fetch(projects: State<Projects>, project: String) -> Result<String, String> {
    let core = projects.core(&project)?;
    git::fetch(&core.repo()?)
}

/// Diff de um arquivo com a revisão e os blocos (`hunks`). A tela devolve a revisão e o bloco nas ações por bloco.
#[tauri::command(async)]
fn git_review_diff(
    projects: State<Projects>, project: String, path: String, staged: bool,
) -> Result<git::ReviewDiff, String> {
    let core = projects.core(&project)?;
    git::review_diff(&core.repo()?, &path, staged)
}

/// O texto de um arquivo no HEAD e no índice, para os marcadores de mudança do editor. `path` é relativo ao projeto.
/// `None` quando não há marcadores a mostrar (sem repositório, não rastreado, binário ou grande).
#[tauri::command(async)]
fn git_file_base(projects: State<Projects>, project: String, path: String) -> Result<Option<git::FileBase>, String> {
    let core = projects.core(&project)?;
    git::file_base(&core.repo()?, &path)
}

/// Grava no índice o texto de um arquivo, para preparar ou tirar do preparo um trecho pelo editor. `expected` é o texto
/// do índice que o editor viu. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_write_index(
    projects: State<Projects>, project: String, path: String, text: String, expected: String,
) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.git_idle(|dir| git::write_index(dir, &path, &text, &expected))
}

/// Prepara um bloco da pasta de trabalho. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_stage_hunk(projects: State<Projects>, project: String, hunk: git::HunkRef) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.git_idle(|dir| git::stage_hunk(dir, &hunk))
}

/// Tira um bloco do índice. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_unstage_hunk(projects: State<Projects>, project: String, hunk: git::HunkRef) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.git_idle(|dir| git::unstage_hunk(dir, &hunk))
}

/// Descarta um bloco da alteração não preparada. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_discard_hunk(projects: State<Projects>, project: String, hunk: git::HunkRef) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.git_idle(|dir| git::discard_hunk(dir, &hunk))
}

/// Descarta a alteração não preparada do arquivo, se ele ainda está na `revision` mostrada. Recusado com executor ativo.
#[tauri::command(async)]
fn git_discard_file(projects: State<Projects>, project: String, path: String, revision: String) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.git_idle(|dir| git::discard_file(dir, &path, &revision))
}

/// Uma página do histórico (`limit` até 200, `skip` os já mostrados); com `path`, só os commits que tocaram o arquivo.
#[tauri::command(async)]
fn git_history(
    projects: State<Projects>, project: String, limit: usize, skip: usize, path: Option<String>,
) -> Result<Vec<git::GraphCommit>, String> {
    let core = projects.core(&project)?;
    git::history(&core.repo()?, limit, skip, path.as_deref())
}

/// Uma página do gráfico: os commits de HEAD e do upstream da branch atual (`limit` até 200, `skip` os já mostrados).
#[tauri::command(async)]
fn git_graph(
    projects: State<Projects>, project: String, limit: usize, skip: usize,
) -> Result<Vec<git::GraphCommit>, String> {
    let core = projects.core(&project)?;
    git::graph(&core.repo()?, limit, skip)
}

/// Cabeçalho do commit e os arquivos que ele mudou.
#[tauri::command(async)]
fn git_commit_detail(projects: State<Projects>, project: String, id: String) -> Result<git::CommitDetail, String> {
    let core = projects.core(&project)?;
    git::commit_detail(&core.repo()?, &id)
}

/// Detalhes do hover de um commit; só a chamada da interface ao abrir o hover executa esta leitura.
#[tauri::command(async)]
fn git_commit_hover(
    projects: State<Projects>, project: String, hash: String,
) -> Result<git::CommitHoverDetail, String> {
    let core = projects.core(&project)?;
    git::commit_hover(&core.repo()?, &hash)
}

/// O diff de um caminho de um commit.
#[tauri::command(async)]
fn git_commit_diff(
    projects: State<Projects>, project: String, id: String, path: String,
) -> Result<git::Diff, String> {
    let core = projects.core(&project)?;
    git::commit_diff(&core.repo()?, &id, &path)
}

/// Os lados de um arquivo em conflito, o texto atual da pasta e a revisão.
#[tauri::command(async)]
fn git_conflict(projects: State<Projects>, project: String, path: String) -> Result<git::Conflict, String> {
    let core = projects.core(&project)?;
    git::conflict(&core.repo()?, &path)
}

/// Grava a resolução de um conflito se a pasta ainda está na `revision` lida. Com `markResolved`, prepara o arquivo.
#[tauri::command(async)]
fn git_write_resolution(
    projects: State<Projects>, project: String, path: String, content: String, revision: String, mark_resolved: bool,
) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.git_idle(|dir| git::write_resolution(dir, &path, &content, &revision, mark_resolved))
}

/// Aceita a remoção de um arquivo em conflito que um lado apagou, se ainda está na `revision` lida.
#[tauri::command(async)]
fn git_accept_deletion(
    projects: State<Projects>, project: String, path: String, revision: String,
) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.git_idle(|dir| git::accept_deletion(dir, &path, &revision))
}

/// Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_pull(projects: State<Projects>, project: String) -> Result<String, String> {
    let core = projects.core(&project)?;
    core.git_idle(git::pull)
}

/// A saída do `git push`, para a tela mostrar. Sem upstream, publica em origin.
#[tauri::command(async)]
fn git_push(projects: State<Projects>, project: String) -> Result<String, String> {
    let core = projects.core(&project)?;
    git::push(&core.repo()?)
}

#[tauri::command(async)]
fn git_branches(projects: State<Projects>, project: String) -> Result<git::Branches, String> {
    let core = projects.core(&project)?;
    git::branches(&core.repo()?)
}

/// Troca para uma branch local existente. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_switch(projects: State<Projects>, project: String, name: String) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.git_idle(|dir| git::switch(dir, &name))
}

/// Cria a branch a partir do HEAD e troca para ela. Recusado enquanto houver executor trabalhando.
#[tauri::command(async)]
fn git_create_branch(projects: State<Projects>, project: String, name: String) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.git_idle(|dir| git::create_branch(dir, &name))
}

#[tauri::command(async)]
fn start_orchestrator(
    projects: State<Projects>, project: String, choice: Choice, cols: u16, rows: u16,
) -> Result<(), String> {
    let core = projects.core(&project)?;
    core.start_orchestrator(choice, cols, rows)
}

/// Abre um shell do usuário, no slot `id` (`shell-<n>`), na pasta do projeto ou, com `cwd` (relativo ao projeto), na
/// pasta dele. Recusado se esse id já tiver um aberto. Devolve o programa que abriu.
#[tauri::command(async)]
fn start_shell(
    projects: State<Projects>, project: Option<String>, id: String, cols: u16, rows: u16, cwd: Option<String>,
) -> Result<String, String> {
    projects.shell_core(project.as_deref())?.start_shell_in(&id, cols, rows, cwd.as_deref())
}

/// Renomeia o arquivo `path` para `name`, na mesma pasta. Devolve o caminho novo. Nunca sobrescreve.
#[tauri::command(async)]
fn rename_file(projects: State<Projects>, project: String, path: String, name: String) -> Result<String, String> {
    let core = projects.core(&project)?;
    files::rename(&core.repo()?, &path, &name)
}

/// Manda os itens `paths` para a Lixeira. Devolve os feitos e o erro do que parou (ver `Batch`).
#[tauri::command(async)]
fn delete_files(
    projects: State<Projects>, project: String, paths: Vec<String>,
) -> Result<files::Batch<String>, String> {
    let core = projects.core(&project)?;
    Ok(files::recycle_many(&core.repo()?, &paths))
}

/// Move os itens `paths` para a pasta `dest` (vazio é a raiz). Devolve os movidos e o erro do que parou (ver `Batch`).
#[tauri::command(async)]
fn move_files(
    projects: State<Projects>, project: String, paths: Vec<String>, dest: String,
) -> Result<files::Batch<files::Moved>, String> {
    let core = projects.core(&project)?;
    Ok(files::move_items(&core.repo()?, &paths, &dest))
}

/// Põe os itens `paths` no clipboard como copiados (`cut` false) ou recortados. Nada se move aqui.
#[tauri::command(async)]
fn copy_files(projects: State<Projects>, project: String, paths: Vec<String>, cut: bool) -> Result<(), String> {
    let core = projects.core(&project)?;
    files::copy_items(&core.repo()?, &paths, cut)
}

/// Mostra o arquivo `path` selecionado no Explorer do Windows.
#[tauri::command(async)]
fn reveal_file(projects: State<Projects>, project: String, path: String) -> Result<(), String> {
    let core = projects.core(&project)?;
    files::reveal(&core.repo()?, &path)
}

/// Abre o diálogo "Abrir com..." do Windows para o arquivo `path`.
#[tauri::command(async)]
fn open_with(projects: State<Projects>, project: String, path: String) -> Result<(), String> {
    let core = projects.core(&project)?;
    files::open_with(&core.repo()?, &path)
}

/// Abre no navegador somente URLs HTTPS do GitHub.
#[tauri::command(async)]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://github.com/") {
        return Err("Só é permitido abrir links do GitHub.".into());
    }
    platform::open_url(&url).map_err(|e| e.to_string())
}

/// Cria o arquivo `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo. Nunca sobrescreve.
#[tauri::command(async)]
fn create_file(projects: State<Projects>, project: String, dir: String, name: String) -> Result<String, String> {
    let core = projects.core(&project)?;
    files::create_file(&core.repo()?, &dir, &name)
}

/// Cria a pasta `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo. Nunca sobrescreve.
#[tauri::command(async)]
fn create_dir(projects: State<Projects>, project: String, dir: String, name: String) -> Result<String, String> {
    let core = projects.core(&project)?;
    files::create_dir(&core.repo()?, &dir, &name)
}

/// Abre no Explorer do Windows a pasta `path` do projeto (vazio é a raiz).
#[tauri::command(async)]
fn reveal_dir(projects: State<Projects>, project: String, path: String) -> Result<(), String> {
    let core = projects.core(&project)?;
    files::reveal_dir(&core.repo()?, &path)
}

/// Os arquivos e o efeito (copiar ou recortar) do clipboard agora. Lido na hora de cada ação; sem arquivo, `paths` vem vazio.
#[tauri::command(async)]
fn clipboard_snapshot() -> Result<platform::Snapshot, String> {
    Ok(platform::snapshot())
}

/// A colagem com o detalhe: cada item colado (`from` absoluto como estava no clipboard, `to` relativo ao projeto), e
/// o erro do que parou, se parou. Ver `Batch`. Se um recorte moveu algo, esvazia o clipboard se ele ainda for `snap`; um
/// clipboard trocado no meio fica como está.
#[tauri::command(async)]
fn paste_files_detailed(
    projects: State<Projects>, project: String, dest: String, snap: platform::Snapshot,
) -> Result<files::Batch<files::Moved>, String> {
    let core = projects.core(&project)?;
    let result = files::paste_detailed(&core.repo()?, &dest, &snap);
    if snap.cut && !result.done.is_empty() {
        platform::clear_if_same(&snap);
    }
    Ok(result)
}

/// Fecha o shell `id` e mata o processo dele.
#[tauri::command(async)]
fn close_shell(projects: State<Projects>, project: Option<String>, id: String) -> Result<(), String> {
    projects.shell_core(project.as_deref())?.close_shell(&id)
}

#[tauri::command(async)]
fn set_executor_choice(projects: State<Projects>, choice: Choice) -> Result<(), String> {
    projects.set_executor_choice(choice)
}

#[tauri::command(async)]
fn term_write(projects: State<Projects>, project: Option<String>, id: String, data: String) -> Result<(), String> {
    projects.shell_core(project.as_deref())?.term_write(&id, &data)
}

/// O CLI que roda agora no terminal `id` (`claude`, `codex` ou nenhum), para a colagem de arquivo e imagem.
#[tauri::command(async)]
fn term_cli(projects: State<Projects>, project: Option<String>, id: String) -> Result<Option<&'static str>, String> {
    projects.shell_core(project.as_deref())?.term_cli(&id)
}

#[tauri::command(async)]
fn term_resize(
    projects: State<Projects>, project: Option<String>, id: String, cols: u16, rows: u16,
) -> Result<(), String> {
    projects.shell_core(project.as_deref())?.term_resize(&id, cols, rows)
}

#[tauri::command(async)]
fn term_snapshot(projects: State<Projects>, project: Option<String>, id: String) -> Result<Snapshot, String> {
    projects.shell_core(project.as_deref())?.term_snapshot(&id)
}

#[tauri::command(async)]
fn stop_executor(projects: State<Projects>, project: String, id: String) -> Result<(), String> {
    projects.core(&project)?.stop_executor(&id)
}

#[tauri::command(async)]
fn has_running(projects: State<Projects>) -> Result<bool, String> {
    Ok(projects.has_running())
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

#[tauri::command(async)]
fn take_open_paths(paths: State<open_paths::OpenPaths>) -> Vec<String> {
    paths.take()
}

pub fn run(context: tauri::Context, initial_paths: Vec<std::path::PathBuf>) {
    let builder = tauri::Builder::default();
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        let args = args.into_iter().skip(1).collect::<Vec<_>>();
        let paths = open_paths::extract_paths(&args, std::path::Path::new(&cwd));
        if !paths.is_empty() {
            app.state::<open_paths::OpenPaths>().push(paths);
            let _ = app.emit("open-paths", ());
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // A leitura do disco vai para o pool de bloqueio, fora da thread do webview.
        .register_asynchronous_uri_scheme_protocol("orq-media", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || responder.respond(media_protocol(&app, &request)));
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let emit: core::Emit = Arc::new(move |event, payload| drop(handle.emit(event, payload)));
            app.manage(open_paths::OpenPaths::new(initial_paths));
            // Sem LOCALAPPDATA o app abre mesmo assim; o erro aparece em `get_options`.
            let cfg_path = config::config_path().unwrap_or_default();
            app.manage(Projects::new(cli::Exes::real(), session::Places::real(), cfg_path, emit.clone()));
            // Build de desenvolvimento libera o uso direto. O instalado confere a versão antes de liberar.
            let gate = Gate::new(cfg!(debug_assertions), emit);
            app.manage(gate.clone());
            update::check(app.handle(), &gate);
            // A janela nasce oculta e o index.html a mostra; se o JS falhar, ela aparece mesmo assim.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(3));
                if let Some(window) = handle.get_webview_window("main") {
                    if !window.is_visible().unwrap_or(false) {
                        let _ = window.show();
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(gated(tauri::generate_handler![
            choose_project,
            open_project,
            close_project,
            project_snapshot,
            pick_file,
            pick_save_path,
            get_options,
            list_dir,
            read_file,
            read_file_at,
            read_media,
            read_media_at,
            formatter_info,
            format_cli,
            write_file_checked,
            write_file_at_checked,
            rename_file,
            delete_files,
            move_files,
            copy_files,
            reveal_file,
            reveal_dir,
            open_with,
            open_url,
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
            git_file_base,
            git_write_index,
            git_stage_hunk,
            git_unstage_hunk,
            git_discard_hunk,
            git_discard_file,
            git_history,
            git_graph,
            git_commit_detail,
            git_commit_hover,
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
            retry_update,
            take_open_paths
        ]))
        .build(context)
        .expect("o app não pôde ser iniciado")
        .run(|app, event| {
            // Fechar o app mata tudo e limpa a sessão.
            if let tauri::RunEvent::Exit = event {
                app.state::<Projects>().shutdown();
            }
        });
}
