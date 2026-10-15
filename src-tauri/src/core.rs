//! O estado do app: o orquestrador, os lotes delegados, a fila e os executores. Não conhece o
//! Tauri: os eventos saem por `Emit`, que os testes trocam por um canal.
use crate::capacity::{self, Resources};
use crate::cli::{self, Exes};
use crate::config::{self, Choice, Options};
use crate::files;
use crate::git;
use crate::hook;
use crate::proc;
use crate::pty::{Buffer, Shared, Term};
use crate::session::{self, Places, Session};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub const ORCHESTRATOR: &str = "orchestrator";
/// Os ids de shell são `shell-<n>`, gerados pelo frontend. Não colidem com `exec-<n>` nem com `orchestrator`.
pub const SHELL_PREFIX: &str = "shell-";
const EXECUTOR_SIZE: (u16, u16) = (110, 30);
const SEM_PROJETO: &str = "Escolha a pasta do projeto primeiro.";
/// Acima disto o prompt não vai na linha de comando (o limite do Windows é 32767 caracteres).
const PROMPT_ARG_MAX: usize = 20_000;

/// Se `id` é de um shell do usuário.
pub fn is_shell(id: &str) -> bool {
    id.starts_with(SHELL_PREFIX)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeState {
    AwaitingChoice,
    Queued,
    Running,
    /// O CLI parou num pedido de confirmação e espera a resposta do orquestrador. O processo segue vivo e
    /// ocupa o teto de simultâneos, como um nó rodando.
    AwaitingOrchestrator,
    Done,
    Stopped,
    Failed,
}

impl NodeState {
    fn is_final(self) -> bool {
        matches!(self, NodeState::Done | NodeState::Stopped | NodeState::Failed)
    }

    /// O processo do nó está vivo e pode terminar a rodada ou pedir confirmação.
    fn is_live(self) -> bool {
        matches!(self, NodeState::Running | NodeState::AwaitingOrchestrator)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecutorNode {
    pub id: String,
    pub batch: String,
    pub index: usize,
    pub title: String,
    pub state: NodeState,
    pub choice: Option<Choice>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub seq: u64,
    pub data: String,
}

/// Como uma tarefa terminou, do jeito que volta ao orquestrador.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub text: String,
    pub exit: String,
    pub ok: bool,
}

/// `title` = primeira linha da tarefa, cortada em 80 caracteres.
pub fn title(task: &str) -> String {
    task.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(80).collect()
}

/// Quantas linhas do `git status` o resultado do lote mostra; o resto vira uma linha de contagem.
const MAX_LINHAS_GIT: usize = 200;

/// O Git do projeto no fim do lote: a saída do `git status --porcelain` (no modo com repositório), ou o modo sem Git.
pub enum GitLote {
    Status(String),
    NoRepo,
    NoGit,
    RepoAbove(String),
}

/// A saída de um lote terminado (`delegate.rs`, entregue pelo `mcp.rs`) e o código de saída dele: 0 se todas as
/// tarefas concluíram.
pub fn format_result(items: &[(String, Outcome)], git: &Result<GitLote, String>) -> (i32, String) {
    let mut out = String::new();
    for (i, (title, outcome)) in items.iter().enumerate() {
        out += &format!("--- EXECUTOR {}/{} ---\n{title}\n{}\n", i + 1, items.len(), outcome.text.trim_end());
        out += &format!("executor_exit: {}{}\n", outcome.exit, if outcome.ok { "" } else { " (FALHOU)" });
    }
    // O que o Git diz que está alterado agora. Não é o diff do lote: o que já estava pendente
    // antes aparece junto. Sem Git, a seção diz só o motivo.
    const CABECALHO: &str = "git status (tudo o que está alterado agora, inclusive o que já estava pendente antes desta delegação):\n";
    out += "--- GIT ---\n";
    out += &match git {
        Err(why) => format!("{CABECALHO}(git status falhou: {why})\n"),
        Ok(GitLote::Status(status)) => format!("{CABECALHO}{}", lista(status)),
        Ok(GitLote::NoRepo) => "(sem Git: a pasta não é um repositório Git)\n".to_string(),
        Ok(GitLote::NoGit) => "(sem Git: o Git não foi encontrado neste computador)\n".to_string(),
        Ok(GitLote::RepoAbove(root)) => format!("(sem Git: a pasta está dentro do repositório {root}, que não é a raiz do projeto)\n"),
    };
    out += "--- FIM ---\n";
    (if items.iter().all(|(_, o)| o.ok) { 0 } else { 1 }, out)
}

/// As linhas do `git status` para o resultado: até `MAX_LINHAS_GIT`, depois a contagem do que ficou de fora.
fn lista(status: &str) -> String {
    if status.trim().is_empty() {
        return "(nada)\n".to_string();
    }
    let linhas: Vec<&str> = status.trim_end().lines().collect();
    let mut out: String = linhas.iter().take(MAX_LINHAS_GIT).map(|l| format!("{l}\n")).collect();
    if linhas.len() > MAX_LINHAS_GIT {
        out += &format!("(… e mais {} arquivos; rode git status para ver todos)\n", linhas.len() - MAX_LINHAS_GIT);
    }
    out
}

/// A última resposta do executor, se `payload` é o sinal de fim da rodada dele.
/// Claude Code: o JSON do hook `Stop`. Codex: o JSON do `notify`, que (medido no 0.162) também
/// chega para a rodada interna que gera o título da conversa, cuja entrada é outra: só vale a
/// rodada cuja entrada começa pelo prompt que o app mandou.
pub fn turn_end(payload: &str, prompt: &str) -> Option<String> {
    let v: Value = serde_json::from_str(payload).ok()?;
    let text = |key: &str| v[key].as_str().filter(|s| !s.trim().is_empty()).unwrap_or("(o executor terminou sem texto de resposta)").to_string();
    if v["hook_event_name"] == "Stop" {
        return Some(text("last_assistant_message"));
    }
    if v["type"] == "agent-turn-complete" {
        let head = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect::<String>();
        let ours = head(prompt);
        let inputs = v["input-messages"].as_array()?;
        return inputs.iter().any(|m| head(m.as_str().unwrap_or("")) == ours).then(|| text("last-assistant-message"));
    }
    None
}

struct Slot {
    buffer: Shared,
    term: Option<Term>,
    size: (u16, u16),
    gen: u64,
}

impl Slot {
    fn new(gen: u64) -> Slot {
        Slot { buffer: Shared::new(Mutex::new(Buffer::default())), term: None, size: EXECUTOR_SIZE, gen }
    }
}

struct Node {
    info: ExecutorNode,
    dir: PathBuf,
    prompt: String,
}

struct Batch {
    id: String,
    titles: Vec<String>,
    outcomes: Vec<Option<Outcome>>,
}

struct Inner {
    exes: Exes,
    places: Places,
    cfg_path: PathBuf,
    project: Option<String>,
    session: Option<Session>,
    choice: Option<Choice>,
    /// CPU e memória de onde sai o teto de executores vivos (ver `capacity`). Não há ajuste manual.
    resources: Resources,
    slots: HashMap<String, Slot>,
    nodes: Vec<Node>,
    batches: Vec<Batch>,
    seen: HashSet<String>,
    counter: u64,
    epoch: u64,
    /// Uma operação do Git (`git_idle`) roda sem o lock: enquanto esta marca estiver ligada, nenhum lote novo é aceito.
    git_exclusive: bool,
}

impl Inner {
    /// Recusa enquanto algum executor não terminou (esperando escolha, na fila, rodando ou esperando o
    /// orquestrador): a branch não muda e os commits não entram no meio de um lote.
    fn ensure_executors_done(&self) -> Result<(), String> {
        if self.nodes.iter().any(|n| !n.info.state.is_final()) {
            return Err("Há executores trabalhando. Espere o lote terminar.".into());
        }
        Ok(())
    }
}

/// Desmarca `git_exclusive` ao sair de escopo. Vale também quando o comando do Git falha ou entra em pânico.
struct GitExclusive(Arc<Mutex<Inner>>);

impl Drop for GitExclusive {
    fn drop(&mut self) {
        // Um pânico em outra thread pode ter envenenado o lock, mas a marca tem que ser solta do mesmo jeito.
        self.0.lock().unwrap_or_else(|e| e.into_inner()).git_exclusive = false;
    }
}

/// O projeto aberto. `subfolder` vale quando a pasta escolhida é uma subpasta da raiz do repositório.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub project: String,
    pub subfolder: bool,
}

#[derive(Clone)]
pub struct Core {
    inner: Arc<Mutex<Inner>>,
    emit: Emit,
}

impl Core {
    pub fn new(exes: Exes, places: Places, cfg_path: PathBuf, emit: Emit) -> Core {
        let inner = Inner {
            exes,
            places,
            cfg_path,
            project: None,
            session: None,
            choice: None,
            resources: Resources::real(),
            slots: HashMap::new(),
            nodes: Vec::new(),
            batches: Vec::new(),
            seen: HashSet::new(),
            counter: 0,
            epoch: 0,
            git_exclusive: false,
        };
        Core { inner: Arc::new(Mutex::new(inner)), emit }
    }

    /// Troca de onde vêm CPU e memória para o teto. O app usa `Resources::real()`, que `new` já põe; os testes
    /// põem recursos fixos, para o resultado não depender da máquina.
    pub fn with_resources(self, resources: Resources) -> Core {
        self.inner.lock().unwrap().resources = resources;
        self
    }

    /// `folder` passa a ser o projeto. Dentro de um repositório, o projeto é a raiz dele; fora (ou se o Git não abre a
    /// pasta), é a própria pasta, no caminho em que veio. Erro se a pasta não existe ou não é pasta.
    pub fn set_project(&self, folder: &Path) -> Result<Opened, String> {
        if !folder.exists() {
            return Err(format!("A pasta {} não existe.", folder.display()));
        }
        if !folder.is_dir() {
            return Err(format!("{} não é uma pasta.", folder.display()));
        }
        let (project, subfolder) = match proc::git_root(folder) {
            Some(root) => {
                let subfolder = !proc::mesma_pasta(folder, Path::new(&root));
                (root, subfolder)
            }
            None => (folder.display().to_string(), false),
        };
        let mut inner = self.inner.lock().unwrap();
        if inner.session.is_some() {
            return Err("Feche o orquestrador antes de trocar de projeto.".into());
        }
        inner.project = Some(project.clone());
        Ok(Opened { project, subfolder })
    }

    /// A pasta do projeto como caminho, para os comandos que a recebem.
    pub fn repo(&self) -> Result<PathBuf, String> {
        self.inner.lock().unwrap().project.clone().map(PathBuf::from).ok_or_else(|| SEM_PROJETO.into())
    }

    /// Roda `run` com a pasta do projeto, mas só se nenhum executor estiver pendente e nenhuma outra operação
    /// do Git estiver em curso. Vale para o pull, a troca e a criação de branch: puxar commits só sem executor
    /// pendente. Sob o lock, marca `git_exclusive` e solta o lock antes de rodar `run`: o comando pode demorar
    /// (rede, janela de credencial) sem travar o app, e nenhum executor começa até ele terminar. Enquanto a
    /// marca estiver ligada, `accept_batches` não aceita lote novo, então nenhum executor começa no meio do
    /// comando.
    pub fn git_idle<T>(&self, run: impl FnOnce(&Path) -> Result<T, String>) -> Result<T, String> {
        let project = {
            let mut inner = self.inner.lock().unwrap();
            inner.ensure_executors_done()?;
            if inner.git_exclusive {
                return Err("Outra operação do Git está em andamento.".into());
            }
            let project = inner.project.clone().ok_or(SEM_PROJETO)?;
            inner.git_exclusive = true;
            project
        };
        let _exclusive = GitExclusive(self.inner.clone());
        run(Path::new(&project))
    }

    pub fn options(&self) -> Result<Options, String> {
        let (path, codex) = {
            let inner = self.inner.lock().unwrap();
            (inner.cfg_path.clone(), inner.exes.codex.clone())
        };
        let cfg = config::read(&path)?;
        Ok(config::options(&cfg, &codex))
    }

    pub fn start_orchestrator(&self, choice: Choice, cols: u16, rows: u16) -> Result<(), String> {
        config::check_choice("orchestrator", &choice)?;
        let mut inner = self.inner.lock().unwrap();
        let project = inner.project.clone().ok_or(SEM_PROJETO)?;
        if inner.session.is_some() {
            return Err("Já há um orquestrador aberto.".into());
        }
        let mut cfg = config::read(&inner.cfg_path)?;
        cfg.orchestrator = choice.clone();
        config::save(&inner.cfg_path, &cfg)?;

        session::recover_orphans(&inner.places);
        let (session, created) = Session::create(&inner.places, &inner.exes.app, &project, &cfg);
        let started = created.and_then(|_| {
            let spawn = cli::orchestrator(&inner.exes, &choice, &session, &project);
            inner.slots.remove(ORCHESTRATOR);
            self.spawn(&mut inner, ORCHESTRATOR, &spawn, Some((cols, rows)))
        });
        if let Err(e) = started {
            session.remove();
            return Err(e);
        }
        inner.session = Some(session);
        inner.choice = None;
        inner.epoch += 1;
        let (core, epoch) = (self.clone(), inner.epoch);
        std::thread::spawn(move || core.pump(epoch));
        Ok(())
    }

    /// Abre o PowerShell do usuário no slot `id`, na pasta `cwd` do projeto (relativa, com "/"; `Some("")` é a raiz;
    /// `None` é a própria raiz do projeto), e devolve o programa que abriu. Recusado se esse id já tiver um vivo;
    /// outros shells não contam. Cada início tem buffer novo. Pasta que sai do projeto, não existe ou é `.git` é
    /// recusada antes de o shell abrir.
    pub fn start_shell_in(&self, id: &str, cols: u16, rows: u16, cwd: Option<&str>) -> Result<String, String> {
        if !is_shell(id) {
            return Err("Id de shell inválido.".into());
        }
        let mut inner = self.inner.lock().unwrap();
        let project = inner.project.clone().ok_or(SEM_PROJETO)?;
        if inner.slots.get(id).is_some_and(|s| s.term.is_some()) {
            return Err("Já há um shell aberto com este id.".into());
        }
        let dir = match cwd {
            Some(rel) => files::plain(Path::new(&project), rel, true)?.to_string_lossy().into_owned(),
            None => project.clone(),
        };
        inner.slots.remove(id);
        let mut started = Err(String::new());
        for program in cli::SHELLS {
            started = self.spawn(&mut inner, id, &cli::shell(program, &dir), Some((cols, rows))).map(|_| program);
            if started.is_ok() {
                break;
            }
        }
        if started.is_err() {
            inner.slots.remove(id);
        }
        started.map(str::to_string)
    }

    /// Fecha o shell `id`: tira o slot, e o processo morre com ele. Não emite evento. Id que não é de shell ou
    /// que não existe não faz nada.
    pub fn close_shell(&self, id: &str) {
        if is_shell(id) {
            self.inner.lock().unwrap().slots.remove(id);
        }
    }

    pub fn set_executor_choice(&self, choice: Choice) -> Result<(), String> {
        config::check_choice("executor", &choice)?;
        let mut inner = self.inner.lock().unwrap();
        let mut cfg = config::read(&inner.cfg_path)?;
        cfg.executor = choice.clone();
        config::save(&inner.cfg_path, &cfg)?;
        inner.choice = Some(choice.clone());
        self.schedule(&mut inner);
        Ok(())
    }

    /// Só o orquestrador e os shells recebem teclado; os executores são somente leitura e qualquer outro id é ignorado.
    pub fn term_write(&self, id: &str, data: &str) {
        if id != ORCHESTRATOR && !is_shell(id) {
            return;
        }
        if let Some(term) = self.inner.lock().unwrap().slots.get_mut(id).and_then(|s| s.term.as_mut()) {
            term.write(data.as_bytes());
        }
    }

    /// Vale também para um nó ainda sem PTY: o tamanho fica guardado para quando ele abrir.
    pub fn term_resize(&self, id: &str, cols: u16, rows: u16) {
        if let Some(slot) = self.inner.lock().unwrap().slots.get_mut(id) {
            slot.size = (cols, rows);
            if let Some(term) = &slot.term {
                term.resize(cols, rows);
            }
        }
    }

    pub fn term_snapshot(&self, id: &str) -> Snapshot {
        let buffer = self.inner.lock().unwrap().slots.get(id).map(|s| s.buffer.clone());
        match buffer {
            Some(buffer) => {
                let buf = buffer.lock().unwrap();
                Snapshot { seq: buf.seq, data: B64.encode(&buf.data) }
            }
            None => Snapshot { seq: 0, data: String::new() },
        }
    }

    pub fn stop_executor(&self, id: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(i) = inner.nodes.iter().position(|n| n.info.id == id && !n.info.state.is_final()) {
            let outcome = Outcome { text: "interrompido pelo usuário".into(), exit: "interrompido".into(), ok: false };
            self.finish(&mut inner, i, NodeState::Stopped, outcome);
        }
    }

    pub fn has_running(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.session.is_some() || inner.slots.iter().any(|(id, s)| !is_shell(id) && s.term.is_some())
    }

    /// Mata tudo, os shells inclusive, e limpa a sessão e a regra.
    pub fn shutdown(&self) {
        let mut inner = self.inner.lock().unwrap();
        self.teardown(&mut inner);
        inner.slots.clear();
    }

    /// A pasta da sessão viva, para os testes e para diagnóstico.
    pub fn session_dir(&self) -> Option<PathBuf> {
        self.inner.lock().unwrap().session.as_ref().map(|s| s.dir.clone())
    }

    fn teardown(&self, inner: &mut Inner) {
        inner.epoch += 1;
        // Os shells não pertencem à sessão: saem daqui e só `shutdown` os mata.
        inner.slots.retain(|id, _| is_shell(id));
        inner.nodes.clear();
        inner.batches.clear();
        inner.seen.clear();
        inner.choice = None;
        if let Some(warning) = inner.session.take().and_then(|s| s.remove()) {
            eprintln!("{warning}");
        }
    }

    fn spawn(&self, inner: &mut Inner, id: &str, spawn: &cli::Spawn, size: Option<(u16, u16)>) -> Result<(), String> {
        inner.counter += 1;
        let gen = inner.counter;
        let slot = inner.slots.entry(id.to_string()).or_insert_with(|| Slot::new(gen));
        slot.gen = gen;
        if let Some(size) = size {
            slot.size = size;
        }
        let (emit, data_id) = (self.emit.clone(), id.to_string());
        let (core, exit_id) = (self.clone(), id.to_string());
        let term = Term::spawn(
            spawn,
            slot.size,
            slot.buffer.clone(),
            move |seq, bytes| emit("term-data", json!({ "id": data_id, "seq": seq, "data": B64.encode(bytes) })),
            move |code| core.on_exit(&exit_id, gen, code),
        )?;
        slot.term = Some(term);
        Ok(())
    }

    fn on_exit(&self, id: &str, gen: u64, code: Option<u32>) {
        let mut inner = self.inner.lock().unwrap();
        // Um processo de uma sessão anterior terminando agora não mexe na atual.
        match inner.slots.get_mut(id) {
            Some(slot) if slot.gen == gen => slot.term = None,
            _ => return,
        }
        if id == ORCHESTRATOR {
            self.teardown(&mut inner);
            (self.emit)("orchestrator-exit", json!({ "code": code }));
        } else if is_shell(id) {
            inner.slots.remove(id);
            (self.emit)("shell-exit", json!({ "id": id, "code": code }));
        } else if let Some(i) = inner.nodes.iter().position(|n| n.info.id == id && n.info.state.is_live()) {
            // O sinal de fim pode ter chegado junto com a saída do processo.
            match self.signal(&inner.nodes[i]) {
                Some(text) => self.finish(&mut inner, i, NodeState::Done, Outcome { text, exit: "0".into(), ok: true }),
                None => {
                    let code = code.map_or("desconhecido".to_string(), |c| c.to_string());
                    let text = format!("O executor terminou sem sinal de fim de rodada (código de saída {code}).");
                    self.finish(&mut inner, i, NodeState::Failed, Outcome { text, exit: code, ok: false });
                }
            }
        }
    }

    /// Laço da sessão: recebe os lotes pedidos pelo comando de delegação e os sinais de fim de rodada.
    fn pump(&self, epoch: u64) {
        // A memória livre muda sozinha, então a fila é reavaliada a cada 1 s (cinco voltas), e só quando há nó na fila.
        let mut turns = 0u32;
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let mut inner = self.inner.lock().unwrap();
            if inner.epoch != epoch {
                return;
            }
            self.accept_batches(&mut inner);
            for i in 0..inner.nodes.len() {
                if inner.nodes[i].info.state.is_live() {
                    if let Some(text) = self.signal(&inner.nodes[i]) {
                        self.finish(&mut inner, i, NodeState::Done, Outcome { text, exit: "0".into(), ok: true });
                    } else {
                        self.watch_asks(&mut inner, i);
                    }
                }
            }
            // Só aqui, fora do laço por índice: o resultado do nó final já está no lote, então ele pode sair da lista.
            inner.nodes.retain(|n| !n.info.state.is_final());
            turns = turns.wrapping_add(1);
            let queued = inner.nodes.iter().any(|n| n.info.state == NodeState::Queued);
            if turns % 5 == 0 && queued && inner.choice.is_some() && !inner.git_exclusive {
                self.schedule(&mut inner);
            }
        }
    }

    /// Acompanha os pedidos de confirmação de um nó vivo: entrega as respostas do orquestrador e põe o nó em
    /// `AwaitingOrchestrator` enquanto houver pedido sem resposta, de volta a `Running` quando não houver.
    fn watch_asks(&self, inner: &mut Inner, i: usize) {
        let dir = inner.nodes[i].dir.join("ask");
        let waiting = !hook::pending_asks(&dir).is_empty();
        let state = match (inner.nodes[i].info.state, waiting) {
            (NodeState::Running, true) => NodeState::AwaitingOrchestrator,
            (NodeState::AwaitingOrchestrator, false) => NodeState::Running,
            _ => return,
        };
        inner.nodes[i].info.state = state;
        self.announce(&inner.nodes[i].info, false);
    }

    fn signal(&self, node: &Node) -> Option<String> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(node.dir.join("hook")).ok()?.flatten().map(|e| e.path()).collect();
        files.retain(|f| f.extension().is_some_and(|e| e == "json"));
        files.sort();
        files.iter().find_map(|file| {
            let found = turn_end(&std::fs::read_to_string(file).unwrap_or_default(), &node.prompt);
            if found.is_none() {
                let _ = std::fs::remove_file(file);
            }
            found
        })
    }

    fn accept_batches(&self, inner: &mut Inner) {
        // Com uma operação do Git em curso, o pedido fica na pasta `requests`: `seen` só marca o que foi aceito,
        // então a volta seguinte do laço o lê, sem perder nem repetir o lote.
        if inner.git_exclusive {
            return;
        }
        let Some(session) = &inner.session else { return };
        let (requests, exec) = (session.dir.join("requests"), session.dir.join("exec"));
        let mut ids: Vec<String> = std::fs::read_dir(&requests)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| !name.ends_with(".tmp") && !inner.seen.contains(name))
            .collect();
        ids.sort();
        for id in ids {
            inner.seen.insert(id.clone());
            let tasks: Vec<String> = (1..).map_while(|n| std::fs::read_to_string(requests.join(&id).join(format!("{n}.txt"))).ok()).collect();
            let titles: Vec<String> = tasks.iter().map(|t| title(t)).collect();
            inner.batches.push(Batch { id: id.clone(), titles: titles.clone(), outcomes: vec![None; tasks.len()] });
            for (index, task) in tasks.iter().enumerate() {
                inner.counter += 1;
                let node_id = format!("exec-{}", inner.counter);
                let info = ExecutorNode { id: node_id.clone(), batch: id.clone(), index, title: titles[index].clone(), state: NodeState::AwaitingChoice, choice: None };
                inner.slots.insert(node_id.clone(), Slot::new(0));
                inner.nodes.push(Node { info, dir: exec.join(&node_id), prompt: cli::executor_prompt(task) });
            }
            // Cada nó entra na sessão já no estado em que vai ficar: rodando, na fila ou esperando a escolha.
            let first = inner.nodes.len() - tasks.len();
            self.schedule_from(inner, first, true);
            if tasks.is_empty() {
                self.deliver(inner, &id);
            }
        }
    }

    fn schedule(&self, inner: &mut Inner) {
        self.schedule_from(inner, 0, false);
    }

    /// Abre os executores que cabem no teto, na ordem em que chegaram; o resto fica na fila.
    /// Sem escolha de executor ninguém abre. Os nós a partir de `new_from` são novos (quando
    /// `announce`): saem como `node-added` em vez de `node-state`.
    /// O teto (`capacity`) é lido uma vez por passe e cada início consome uma vaga dele: os vivos nunca são
    /// interrompidos, só deixa de abrir novo enquanto não houver vaga.
    fn schedule_from(&self, inner: &mut Inner, new_from: usize, announce: bool) {
        let mut running = inner.nodes.iter().filter(|n| n.info.state.is_live()).count();
        let admission = capacity::admission(inner.resources.cpus, (inner.resources.memory)(), running);
        for i in 0..inner.nodes.len() {
            let is_new = announce && i >= new_from;
            let before = inner.nodes[i].info.state;
            if matches!(before, NodeState::AwaitingChoice | NodeState::Queued) {
                if let Some(choice) = inner.choice.clone() {
                    if running < admission {
                        running += 1;
                        inner.nodes[i].info.state = NodeState::Running;
                        inner.nodes[i].info.choice = Some(choice.clone());
                        self.announce(&inner.nodes[i].info, is_new);
                        if let Err(why) = self.start(inner, i, &choice) {
                            running -= 1;
                            self.finish_quiet(inner, i, NodeState::Failed, Outcome { text: why, exit: "não iniciou".into(), ok: false });
                        }
                        continue;
                    }
                    inner.nodes[i].info.state = NodeState::Queued;
                }
            }
            if is_new || inner.nodes[i].info.state != before {
                self.announce(&inner.nodes[i].info, is_new);
            }
        }
    }

    fn announce(&self, node: &ExecutorNode, is_new: bool) {
        if is_new {
            (self.emit)("node-added", json!(node));
        } else {
            (self.emit)("node-state", json!({ "id": node.id, "state": node.state, "choice": node.choice }));
        }
    }

    fn start(&self, inner: &mut Inner, i: usize, choice: &Choice) -> Result<(), String> {
        let (id, dir, mut prompt) = (inner.nodes[i].info.id.clone(), inner.nodes[i].dir.clone(), inner.nodes[i].prompt.clone());
        let project = inner.project.clone().unwrap_or_default();
        let io = |e: std::io::Error| format!("Não foi possível preparar o executor em {}: {e}", dir.display());
        std::fs::create_dir_all(dir.join("hook")).map_err(io)?;
        std::fs::create_dir_all(dir.join("ask")).map_err(io)?;
        // Quem é o executor na delegação: a espera de um lote lê isto para mostrar a pergunta com "(i/n)".
        let size = inner.batches.iter().find(|b| b.id == inner.nodes[i].info.batch).map_or(0, |b| b.titles.len());
        let meta = json!({ "batch": inner.nodes[i].info.batch, "index": inner.nodes[i].info.index, "size": size });
        std::fs::write(dir.join("batch.json"), meta.to_string()).map_err(io)?;
        if prompt.len() > PROMPT_ARG_MAX {
            let file = dir.join("tarefa.md");
            std::fs::write(&file, &prompt).map_err(io)?;
            prompt = format!("Leia o arquivo {} por inteiro e siga as instruções dele.", file.display());
            inner.nodes[i].prompt = prompt.clone();
        }
        let (spawn, settings) = cli::executor(&inner.exes, choice, &project, &dir, &prompt);
        if let Some(settings) = settings {
            std::fs::write(dir.join("settings.json"), settings).map_err(io)?;
        }
        self.spawn(inner, &id, &spawn, None)
    }

    fn finish(&self, inner: &mut Inner, i: usize, state: NodeState, outcome: Outcome) {
        self.finish_quiet(inner, i, state, outcome);
        self.schedule(inner);
    }

    /// Estado final de um nó: avisa o frontend, tira o slot (o processo morre com ele e a tela some), guarda
    /// o resultado no lote e, se era o último, entrega o lote. Não mexe na fila (quem chama decide), nem tira o
    /// nó de `inner.nodes`: quem percorre os nós por índice é o laço do `pump`, que faz essa limpeza depois.
    fn finish_quiet(&self, inner: &mut Inner, i: usize, state: NodeState, outcome: Outcome) {
        let node = &mut inner.nodes[i].info;
        node.state = state;
        let (id, batch, index) = (node.id.clone(), node.batch.clone(), node.index);
        self.announce(&inner.nodes[i].info, false);
        // O pedido sem resposta morre com o nó: quem lista as perguntas para o orquestrador deixa de vê-lo.
        hook::clear_asks(&inner.nodes[i].dir.join("ask"));
        inner.slots.remove(&id);
        if let Some(b) = inner.batches.iter_mut().find(|b| b.id == batch) {
            b.outcomes[index] = Some(outcome);
        }
        self.deliver(inner, &batch);
    }

    /// Com todas as tarefas do lote em estado final, escreve o resultado que o comando de delegação
    /// espera: primeira linha o código de saída, depois a saída. Publicado por renomeação.
    fn deliver(&self, inner: &mut Inner, batch: &str) {
        let Some(pos) = inner.batches.iter().position(|b| b.id == batch && b.outcomes.iter().all(Option::is_some)) else { return };
        let done = inner.batches.remove(pos);
        let items: Vec<(String, Outcome)> = done.titles.into_iter().zip(done.outcomes.into_iter().flatten()).collect();
        let project = inner.project.clone().unwrap_or_default();
        let dir = Path::new(&project);
        let git = match git::modo(dir) {
            Ok(git::Modo::Repo) => proc::git_saida(dir, &["status", "--porcelain", "--untracked-files=all"]).map(GitLote::Status),
            Ok(git::Modo::NoRepo) => Ok(GitLote::NoRepo),
            Ok(git::Modo::NoGit) => Ok(GitLote::NoGit),
            Ok(git::Modo::RepoAbove(root)) => Ok(GitLote::RepoAbove(root)),
            Err(why) => Err(why),
        };
        let (code, text) = if items.is_empty() {
            (4, "wrapper_error: o lote chegou sem tarefas.\n--- FIM ---\n".to_string())
        } else {
            format_result(&items, &git)
        };
        if let Some(session) = &inner.session {
            let results = session.dir.join("results");
            let tmp = results.join(format!("{batch}.tmp"));
            if let Err(e) = std::fs::write(&tmp, format!("{code}\n{text}")).and_then(|_| std::fs::rename(&tmp, results.join(format!("{batch}.txt")))) {
                eprintln!("Aviso: o resultado do lote {batch} não pôde ser gravado: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;
    use crate::testutil::fresh;

    fn ok(text: &str) -> Outcome {
        Outcome { text: text.into(), exit: "0".into(), ok: true }
    }

    #[test]
    fn titulo_e_a_primeira_linha_cortada_em_80() {
        assert_eq!(title("\n  Remova X.\nEscopo: Y."), "Remova X.");
        assert_eq!(title(&"ã".repeat(200)).chars().count(), 80);
        assert_eq!(title(""), "");
    }

    #[test]
    fn resultado_de_um_lote_concluido() {
        let items = [("Tarefa A".to_string(), ok("Fiz A.\ncódigo 0\n")), ("Tarefa B".to_string(), ok("Fiz B."))];
        let (code, text) = format_result(&items, &Ok(GitLote::Status(" M a.txt\n?? b.txt\n".into())));
        assert_eq!(code, 0);
        assert_eq!(
            text,
            "--- EXECUTOR 1/2 ---\nTarefa A\nFiz A.\ncódigo 0\nexecutor_exit: 0\n\
             --- EXECUTOR 2/2 ---\nTarefa B\nFiz B.\nexecutor_exit: 0\n\
             --- GIT ---\ngit status (tudo o que está alterado agora, inclusive o que já estava pendente antes desta delegação):\n M a.txt\n?? b.txt\n\
             --- FIM ---\n"
        );
    }

    #[test]
    fn resultado_com_interrompido_falha_e_git_vazio_ou_quebrado() {
        let items = [
            ("A".to_string(), ok("ok")),
            ("B".to_string(), Outcome { text: "interrompido pelo usuário".into(), exit: "interrompido".into(), ok: false }),
            ("C".to_string(), Outcome { text: "O executor terminou sem sinal de fim de rodada (código de saída 3).".into(), exit: "3".into(), ok: false }),
        ];
        let (code, text) = format_result(&items, &Ok(GitLote::Status("\n".into())));
        assert_ne!(code, 0);
        assert!(text.contains("--- EXECUTOR 2/3 ---\nB\ninterrompido pelo usuário\nexecutor_exit: interrompido (FALHOU)\n"));
        assert!(text.contains("--- EXECUTOR 3/3 ---\nC\nO executor terminou sem sinal de fim de rodada (código de saída 3).\nexecutor_exit: 3 (FALHOU)\n"));
        assert!(text.ends_with("delegação):\n(nada)\n--- FIM ---\n"));
        let (_, text) = format_result(&items[..1], &Err("código 128".into()));
        assert!(text.ends_with("(git status falhou: código 128)\n--- FIM ---\n"));
    }

    #[test]
    fn resultado_sem_git_diz_so_o_motivo_em_uma_linha() {
        let items = [("A".to_string(), ok("feito"))];
        let casos = [
            (GitLote::NoRepo, "--- GIT ---\n(sem Git: a pasta não é um repositório Git)\n--- FIM ---\n"),
            (GitLote::NoGit, "--- GIT ---\n(sem Git: o Git não foi encontrado neste computador)\n--- FIM ---\n"),
            (
                GitLote::RepoAbove("D:\\raiz".into()),
                "--- GIT ---\n(sem Git: a pasta está dentro do repositório D:\\raiz, que não é a raiz do projeto)\n--- FIM ---\n",
            ),
        ];
        for (git, secao) in casos {
            let (code, text) = format_result(&items, &Ok(git));
            assert_eq!(code, 0);
            assert!(text.ends_with(secao), "{text}");
            assert!(!text.contains("git status"), "sem Git não há a linha do git status: {text}");
        }
    }

    #[test]
    fn resultado_corta_o_git_status_em_200_linhas_e_conta_o_resto() {
        let items = [("A".to_string(), ok("feito"))];
        let status: String = (1..=203).map(|n| format!(" M arq{n}.txt\n")).collect();
        let (_, text) = format_result(&items, &Ok(GitLote::Status(status)));
        assert!(text.contains(" M arq200.txt\n(… e mais 3 arquivos; rode git status para ver todos)\n--- FIM ---\n"), "{text}");
        assert!(!text.contains("arq201.txt"));
        let (_, text) = format_result(&items, &Ok(GitLote::Status((1..=200).map(|n| format!(" M a{n}\n")).collect())));
        assert!(!text.contains("e mais"), "exatamente 200 linhas não tem contagem: {text}");
    }

    // Os dois JSON abaixo são os que os CLIs reais entregaram na medição (encurtados).
    #[test]
    fn fim_de_rodada_do_claude() {
        let payload = r#"{"session_id":"db7a","permission_mode":"bypassPermissions","hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"OK","background_tasks":[]}"#;
        assert_eq!(turn_end(payload, "qualquer").as_deref(), Some("OK"));
        assert_eq!(turn_end(r#"{"hook_event_name":"PreToolUse"}"#, "x"), None);
        assert_eq!(turn_end("", "x"), None);
    }

    #[test]
    fn fim_de_rodada_do_codex_ignora_a_rodada_do_titulo() {
        let prompt = "Execute só a tarefa abaixo.\n\nTAREFA:\nresponda apenas OK";
        let real = json!({ "type": "agent-turn-complete", "client": "codex-tui", "input-messages": [prompt.replace('\n', "\r\n")], "last-assistant-message": "OK" });
        let title = json!({ "type": "agent-turn-complete", "client": "codex-tui",
            "input-messages": [format!("Generate a concise, single-line task title of at most 36 characters.\n\nUser prompt:\n{prompt}")],
            "last-assistant-message": "{\"title\":\"Responda OK\"}" });
        assert_eq!(turn_end(&real.to_string(), prompt).as_deref(), Some("OK"));
        assert_eq!(turn_end(&title.to_string(), prompt), None);
    }

    /// Um Core com um repositório de teste (branch `main` e `outra`), sem CLI real nem configuração.
    fn git_core(tag: &str) -> (Core, std::path::PathBuf) {
        git_core_with(tag, Arc::new(|_: &str, _: Value| {}))
    }

    fn git_core_with(tag: &str, emit: Emit) -> (Core, std::path::PathBuf) {
        let root = fresh("orq-core", tag);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| proc::git_saida(&repo, args).unwrap();
        git(&["init", "-q", "-b", "main"]);
        for (key, value) in [("user.name", "teste"), ("user.email", "teste@teste"), ("core.autocrlf", "false")] {
            git(&["config", key, value]);
        }
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "a"]);
        git(&["branch", "outra"]);
        let core = core_vazio(&root, emit);
        core.set_project(&repo).unwrap();
        (core, root)
    }

    /// Um Core sem projeto, com CLIs falsas e a configuração e as sessões na pasta `root`.
    fn core_vazio(root: &Path, emit: Emit) -> Core {
        let fake = String::from("fake");
        let exes = Exes { claude: fake.clone(), codex: fake.clone(), app: fake };
        let places = session::Places { temp: root.join("temp"), codex_rules: root.join("rules"), global_dirs: vec![] };
        Core::new(exes, places, root.join("config.json"), emit)
    }

    #[test]
    fn pasta_fora_de_repositorio_abre_com_o_proprio_caminho() {
        let root = fresh("orq-core", "pasta-solta");
        let pasta = root.join("solta");
        std::fs::create_dir_all(&pasta).unwrap();
        let core = core_vazio(&root, Arc::new(|_: &str, _: Value| {}));
        let opened = core.set_project(&pasta).unwrap();
        assert_eq!(opened, Opened { project: pasta.display().to_string(), subfolder: false });
        assert_eq!(core.repo().unwrap(), pasta);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pasta_inexistente_ou_que_nao_e_pasta_e_recusada() {
        let root = fresh("orq-core", "inexistente");
        let arquivo = root.join("arquivo.txt");
        std::fs::write(&arquivo, "x").unwrap();
        let core = core_vazio(&root, Arc::new(|_: &str, _: Value| {}));
        let sumida = root.join("nao-existe");
        assert_eq!(core.set_project(&sumida).unwrap_err(), format!("A pasta {} não existe.", sumida.display()));
        assert!(core.set_project(&arquivo).unwrap_err().ends_with("não é uma pasta."));
        assert!(core.repo().is_err(), "nada foi aberto");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn subpasta_de_repositorio_abre_a_raiz_com_subfolder() {
        let (core, root) = git_core("subpasta");
        let repo = root.join("repo");
        let sub = repo.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let opened = core.set_project(&sub).unwrap();
        assert!(opened.subfolder);
        assert!(proc::mesma_pasta(Path::new(&opened.project), &repo), "raiz: {}", opened.project);
        let opened = core.set_project(&repo).unwrap();
        assert!(!opened.subfolder);
        assert!(proc::mesma_pasta(Path::new(&opened.project), &repo), "raiz: {}", opened.project);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shell_em_pasta_fora_do_projeto_e_recusado_antes_de_abrir() {
        let (core, root) = git_core("shell-cwd");
        for rel in ["..", "a.txt", "nao-existe", ".git"] {
            assert!(core.start_shell_in("shell-1", 80, 24, Some(rel)).is_err(), "{rel} deveria ser recusado");
            assert!(!vivo(&core, "shell-1"), "{rel} não pode deixar shell aberto");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    fn set_executor_state(core: &Core, state: NodeState) {
        let mut inner = core.inner.lock().unwrap();
        inner.nodes.clear();
        let info = ExecutorNode { id: "exec-1".into(), batch: "lote".into(), index: 0, title: "t".into(), state, choice: None };
        inner.nodes.push(Node { info, dir: PathBuf::new(), prompt: String::new() });
    }

    #[test]
    fn git_que_muda_o_repositorio_espera_os_executores_e_o_resto_nao() {
        const TRAVA: &str = "Há executores trabalhando. Espere o lote terminar.";
        let (core, root) = git_core("trava");
        for state in [NodeState::AwaitingChoice, NodeState::Queued, NodeState::Running, NodeState::AwaitingOrchestrator] {
            set_executor_state(&core, state);
            assert_eq!(core.git_idle(git::pull).unwrap_err(), TRAVA, "{state:?}");
            assert_eq!(core.git_idle(|dir| git::switch(dir, "outra")).unwrap_err(), TRAVA, "{state:?}");
            assert_eq!(core.git_idle(|dir| git::create_branch(dir, "nova")).unwrap_err(), TRAVA, "{state:?}");
            assert!(git::status(&core.repo().unwrap()).is_ok(), "o status nunca é travado ({state:?})");
            assert!(git::branches(&core.repo().unwrap()).is_ok(), "a lista de branches nunca é travada ({state:?})");
        }
        // Recusados não mudaram nada: continua em `main`, sem a branch nova.
        let b = git::branches(&core.repo().unwrap()).unwrap();
        assert_eq!((b.current.as_str(), b.local), ("main", vec!["main".to_string(), "outra".to_string()]));

        for state in [NodeState::Done, NodeState::Stopped, NodeState::Failed] {
            set_executor_state(&core, state);
            assert_eq!(core.git_idle(|dir| git::switch(dir, "outra")), Ok(()), "{state:?} já terminou");
            assert_eq!(core.git_idle(|dir| git::switch(dir, "main")), Ok(()));
            // Sem remoto o pull falha pelo remoto, não pela trava.
            assert_ne!(core.git_idle(git::pull).unwrap_err(), TRAVA, "{state:?}");
        }
        assert_eq!(core.git_idle(|dir| git::create_branch(dir, "nova")), Ok(()));
        assert_eq!(git::branches(&core.repo().unwrap()).unwrap().current, "nova");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn git_exclusivo(core: &Core) -> bool {
        core.inner.lock().unwrap().git_exclusive
    }

    #[test]
    fn com_operacao_do_git_em_curso_outra_recusa_e_a_leitura_responde() {
        const EM_CURSO: &str = "Outra operação do Git está em andamento.";
        let (core, root) = git_core("em-curso");
        core.inner.lock().unwrap().git_exclusive = true;
        assert_eq!(core.git_idle(|dir| git::switch(dir, "outra")).unwrap_err(), EM_CURSO);
        assert_eq!(core.git_idle(|dir| git::create_branch(dir, "nova")).unwrap_err(), EM_CURSO);
        assert_eq!(core.git_idle(git::pull).unwrap_err(), EM_CURSO);
        assert!(git::status(&core.repo().unwrap()).is_ok());
        assert!(git::branches(&core.repo().unwrap()).is_ok());
        // A recusa não solta a marca de quem está rodando.
        assert!(git_exclusivo(&core));
        core.inner.lock().unwrap().git_exclusive = false;
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_marca_some_depois_de_sucesso_e_de_falha() {
        let (core, root) = git_core("marca");
        assert_eq!(core.git_idle(|dir| git::switch(dir, "outra")), Ok(()));
        assert!(!git_exclusivo(&core), "depois de sucesso");
        assert!(core.git_idle(|dir| git::switch(dir, "inexistente")).is_err());
        assert!(!git_exclusivo(&core), "depois de falha");
        assert_eq!(core.git_idle(|dir| git::switch(dir, "main")), Ok(()), "a operação seguinte passa");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Espera `ok` ficar verdadeiro, até 15 segundos.
    fn espera(mut ok: impl FnMut() -> bool) -> bool {
        let fim = std::time::Instant::now() + Duration::from_secs(15);
        while !ok() {
            if std::time::Instant::now() > fim {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        true
    }

    /// Um emissor que guarda os eventos, e o acesso a eles.
    fn gravador() -> (Emit, Arc<Mutex<Vec<(String, Value)>>>) {
        let eventos = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
        let sink = eventos.clone();
        let emit: Emit = Arc::new(move |ev: &str, p: Value| sink.lock().unwrap().push((ev.to_string(), p)));
        (emit, eventos)
    }

    fn vivo(core: &Core, id: &str) -> bool {
        core.inner.lock().unwrap().slots.get(id).is_some_and(|s| s.term.is_some())
    }

    #[test]
    fn shell_abre_recusa_o_mesmo_id_sai_com_exit_e_volta_a_abrir() {
        let (emit, eventos) = gravador();
        let (core, root) = git_core_with("shell", emit);
        let programa = core.start_shell_in("shell-1", 80, 24, None).unwrap();
        assert!(cli::SHELLS.contains(&programa.as_str()), "{programa} não está em cli::SHELLS");
        assert_eq!(core.start_shell_in("shell-1", 80, 24, None).unwrap_err(), "Já há um shell aberto com este id.");
        assert!(!core.has_running(), "o shell vivo não faz o app pedir confirmação");
        core.term_write("shell-1", "exit\r");
        assert!(espera(|| eventos.lock().unwrap().iter().any(|(ev, _)| ev == "shell-exit")), "shell-exit não chegou");
        let saida = eventos.lock().unwrap().iter().find(|(ev, _)| ev == "shell-exit").unwrap().1.clone();
        assert_eq!(saida["id"], "shell-1", "o payload traz o id do shell: {saida}");
        assert!(saida.get("code").is_some(), "o payload traz o código: {saida}");
        assert_eq!(core.term_snapshot("shell-1").seq, 0, "o slot saiu com o processo");
        assert!(core.start_shell_in("shell-1", 80, 24, None).is_ok(), "depois da saída, o mesmo id volta a abrir");
        core.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dois_shells_abertos_ao_mesmo_tempo() {
        let (core, root) = git_core("dois-shells");
        core.start_shell_in("shell-1", 80, 24, None).unwrap();
        core.start_shell_in("shell-2", 80, 24, None).unwrap();
        assert!(vivo(&core, "shell-1") && vivo(&core, "shell-2"), "cada id tem o seu processo");
        assert!(!core.has_running(), "shells não fazem o app pedir confirmação");
        core.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn id_sem_prefixo_de_shell_e_recusado() {
        let (core, root) = git_core("id-invalido");
        assert_eq!(core.start_shell_in("exec-1", 80, 24, None).unwrap_err(), "Id de shell inválido.");
        assert_eq!(core.start_shell_in(ORCHESTRATOR, 80, 24, None).unwrap_err(), "Id de shell inválido.");
        assert!(core.inner.lock().unwrap().slots.is_empty(), "a recusa não cria slot");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn exit_de_um_shell_emite_o_id_dele_e_o_outro_segue_vivo() {
        let (emit, eventos) = gravador();
        let (core, root) = git_core_with("shell-exit-um", emit);
        core.start_shell_in("shell-1", 80, 24, None).unwrap();
        core.start_shell_in("shell-2", 80, 24, None).unwrap();
        core.term_write("shell-1", "exit\r");
        assert!(espera(|| eventos.lock().unwrap().iter().any(|(ev, _)| ev == "shell-exit")), "shell-exit não chegou");
        let saidas: Vec<Value> = eventos.lock().unwrap().iter().filter(|(ev, _)| ev == "shell-exit").map(|(_, p)| p.clone()).collect();
        assert_eq!(saidas.len(), 1, "um shell saiu, um evento: {saidas:?}");
        assert_eq!(saidas[0]["id"], "shell-1");
        assert!(!core.inner.lock().unwrap().slots.contains_key("shell-1"), "o slot do que saiu foi removido");
        assert!(vivo(&core, "shell-2"), "o outro shell segue vivo");
        core.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn close_shell_tira_so_aquele_shell_e_nao_emite_exit() {
        let (emit, eventos) = gravador();
        let (core, root) = git_core_with("close-shell", emit);
        core.start_shell_in("shell-1", 80, 24, None).unwrap();
        core.start_shell_in("shell-2", 80, 24, None).unwrap();
        core.inner.lock().unwrap().slots.insert("exec-1".into(), Slot::new(0));
        // Id que não é de shell, e id que não existe: nada acontece.
        core.close_shell("exec-1");
        core.close_shell("shell-9");
        assert!(core.inner.lock().unwrap().slots.contains_key("exec-1"), "close_shell não mexe em executor");
        core.close_shell("shell-1");
        assert!(!core.inner.lock().unwrap().slots.contains_key("shell-1"), "o slot saiu");
        assert!(vivo(&core, "shell-2"), "o outro shell segue vivo");
        // Dá tempo para uma saída tardia do processo fechado chegar, se fosse emitir.
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!eventos.lock().unwrap().iter().any(|(ev, _)| ev == "shell-exit"), "close_shell não emite shell-exit");
        core.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn teardown_do_orquestrador_deixa_os_shells_e_o_shutdown_os_leva() {
        let (core, root) = git_core("shell-teardown");
        core.start_shell_in("shell-1", 80, 24, None).unwrap();
        core.start_shell_in("shell-2", 80, 24, None).unwrap();
        {
            let mut inner = core.inner.lock().unwrap();
            inner.slots.insert(ORCHESTRATOR.into(), Slot::new(99));
            inner.slots.insert("exec-1".into(), Slot::new(0));
        }
        // A saída do orquestrador (gen 99) passa por `on_exit`, que chama o teardown da sessão.
        core.on_exit(ORCHESTRATOR, 99, Some(0));
        {
            let inner = core.inner.lock().unwrap();
            assert!(!inner.slots.contains_key(ORCHESTRATOR) && !inner.slots.contains_key("exec-1"));
        }
        assert!(vivo(&core, "shell-1") && vivo(&core, "shell-2"), "os shells sobrevivem à saída do orquestrador");
        core.shutdown();
        assert!(core.inner.lock().unwrap().slots.is_empty(), "o shutdown leva todos os shells");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_marca_some_mesmo_com_panico_no_comando() {
        let (core, root) = git_core("panico");
        let saiu = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| core.git_idle::<()>(|_| panic!("comando git travou"))));
        assert!(saiu.is_err());
        assert!(!git_exclusivo(&core));
        assert_eq!(core.git_idle(|dir| git::switch(dir, "outra")), Ok(()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn o_lock_do_core_fica_livre_enquanto_o_git_roda() {
        // Com o lock segurado durante o comando, `core.repo()` (que pega o lock) travaria este teste.
        let (core, root) = git_core("livre");
        let branch = core.git_idle(|_| {
            assert!(git_exclusivo(&core), "marcado enquanto o comando roda");
            git::branches(&core.repo()?).map(|b| b.current)
        });
        assert_eq!(branch, Ok("main".to_string()));
        assert!(!git_exclusivo(&core));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lote_pedido_durante_a_operacao_do_git_espera_e_entra_uma_vez() {
        let (core, root) = git_core("lote");
        let session = root.join("sessao");
        let pedido = session.join("requests").join("lote1");
        std::fs::create_dir_all(&pedido).unwrap();
        std::fs::write(pedido.join("1.txt"), "Tarefa X\n").unwrap();
        let mut inner = core.inner.lock().unwrap();
        inner.session = Some(Session { dir: session.clone(), rule: PathBuf::new(), global_dirs: vec![] });
        inner.git_exclusive = true;
        core.accept_batches(&mut inner);
        assert!(inner.nodes.is_empty(), "com o git em curso, o lote não é aceito");
        inner.git_exclusive = false;
        core.accept_batches(&mut inner);
        assert_eq!(inner.nodes.len(), 1);
        assert_eq!(inner.nodes[0].info.batch, "lote1");
        core.accept_batches(&mut inner);
        assert_eq!(inner.nodes.len(), 1, "o lote não entra duas vezes");
        drop(inner);
        let _ = std::fs::remove_dir_all(&root);
    }
}
