//! O estado do app: o orquestrador, os lotes delegados, a fila e os executores. Não conhece o
//! Tauri: os eventos saem por `Emit`, que os testes trocam por um canal.
use crate::cli::{self, Exes};
use crate::config::{self, Choice, Options};
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
const EXECUTOR_SIZE: (u16, u16) = (110, 30);
/// Acima disto o prompt não vai na linha de comando (o limite do Windows é 32767 caracteres).
const PROMPT_ARG_MAX: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeState {
    AwaitingChoice,
    Queued,
    Running,
    Done,
    Stopped,
    Failed,
}

impl NodeState {
    fn is_final(self) -> bool {
        matches!(self, NodeState::Done | NodeState::Stopped | NodeState::Failed)
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

/// A saída do comando de delegação (`delegate.rs`) para um lote terminado e o código de saída
/// dele: 0 se todas as tarefas concluíram.
pub fn format_result(items: &[(String, Outcome)], git: &Result<String, String>) -> (i32, String) {
    let mut out = String::new();
    for (i, (title, outcome)) in items.iter().enumerate() {
        out += &format!("--- EXECUTOR {}/{} ---\n{title}\n{}\n", i + 1, items.len(), outcome.text.trim_end());
        out += &format!("executor_exit: {}{}\n", outcome.exit, if outcome.ok { "" } else { " (FALHOU)" });
    }
    // O que o Git diz que está alterado agora. Não é o diff do lote: o que já estava pendente
    // antes aparece junto.
    out += "--- GIT ---\ngit status (tudo o que está alterado agora, inclusive o que já estava pendente antes desta delegação):\n";
    out += &match git {
        Err(why) => format!("(git status falhou: {why})\n"),
        Ok(status) if status.trim().is_empty() => "(nada)\n".to_string(),
        Ok(status) => format!("{}\n", status.trim_end()),
    };
    out += "--- FIM ---\n";
    (if items.iter().all(|(_, o)| o.ok) { 0 } else { 1 }, out)
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
    max_parallel: usize,
    slots: HashMap<String, Slot>,
    nodes: Vec<Node>,
    batches: Vec<Batch>,
    seen: HashSet<String>,
    counter: u64,
    epoch: u64,
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
            max_parallel: 4,
            slots: HashMap::new(),
            nodes: Vec::new(),
            batches: Vec::new(),
            seen: HashSet::new(),
            counter: 0,
            epoch: 0,
        };
        Core { inner: Arc::new(Mutex::new(inner)), emit }
    }

    /// A raiz Git de `folder` passa a ser o projeto. Erro se não for um repositório.
    pub fn set_project(&self, folder: &Path) -> Result<String, String> {
        let root = proc::git_root(folder)
            .ok_or_else(|| format!("{} não está dentro de um repositório Git. Escolha outra pasta.", folder.display()))?;
        let mut inner = self.inner.lock().unwrap();
        if inner.session.is_some() {
            return Err("Feche o orquestrador antes de trocar de projeto.".into());
        }
        inner.project = Some(root.clone());
        Ok(root)
    }

    pub fn options(&self) -> Result<Options, String> {
        let (path, codex) = {
            let inner = self.inner.lock().unwrap();
            (inner.cfg_path.clone(), inner.exes.codex.clone())
        };
        let cfg = config::read(&path)?;
        self.inner.lock().unwrap().max_parallel = cfg.max_parallel;
        Ok(config::options(&cfg, &codex))
    }

    pub fn start_orchestrator(&self, choice: Choice, cols: u16, rows: u16) -> Result<String, String> {
        config::check_choice("orchestrator", &choice)?;
        let mut inner = self.inner.lock().unwrap();
        let project = inner.project.clone().ok_or("Escolha a pasta do projeto primeiro.")?;
        if inner.session.is_some() {
            return Err("Já há um orquestrador aberto neste canvas.".into());
        }
        let mut cfg = config::read(&inner.cfg_path)?;
        cfg.orchestrator = choice.clone();
        config::save(&inner.cfg_path, &cfg)?;
        inner.max_parallel = cfg.max_parallel;

        session::recover_orphans(&inner.places);
        let (session, created) = Session::create(&inner.places, &inner.exes.delegate, &project, &cfg);
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
        Ok(ORCHESTRATOR.into())
    }

    pub fn set_executor_choice(&self, choice: Choice) -> Result<(), String> {
        config::check_choice("executor", &choice)?;
        let mut inner = self.inner.lock().unwrap();
        let mut cfg = config::read(&inner.cfg_path)?;
        cfg.executor = choice.clone();
        config::save(&inner.cfg_path, &cfg)?;
        inner.choice = Some(choice.clone());
        (self.emit)("executor-choice", json!(choice));
        self.schedule(&mut inner);
        Ok(())
    }

    pub fn set_max_parallel(&self, value: i64) -> Result<(), String> {
        let value = config::check_max_parallel(value)?;
        let mut inner = self.inner.lock().unwrap();
        let mut cfg = config::read(&inner.cfg_path)?;
        cfg.max_parallel = value;
        config::save(&inner.cfg_path, &cfg)?;
        inner.max_parallel = value;
        self.schedule(&mut inner);
        Ok(())
    }

    /// Só o orquestrador recebe teclado; qualquer outro id é ignorado.
    pub fn term_write(&self, id: &str, data: &str) {
        if id != ORCHESTRATOR {
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

    pub fn clear_done(&self) -> Vec<String> {
        let mut inner = self.inner.lock().unwrap();
        let ids: Vec<String> = inner.nodes.iter().filter(|n| n.info.state.is_final()).map(|n| n.info.id.clone()).collect();
        inner.nodes.retain(|n| !n.info.state.is_final());
        for id in &ids {
            inner.slots.remove(id);
        }
        ids
    }

    pub fn has_running(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.session.is_some() || inner.slots.values().any(|s| s.term.is_some())
    }

    /// Mata tudo e limpa a sessão e a regra.
    pub fn shutdown(&self) {
        self.teardown(&mut self.inner.lock().unwrap());
    }

    /// A pasta da sessão viva, para os testes e para diagnóstico.
    pub fn session_dir(&self) -> Option<PathBuf> {
        self.inner.lock().unwrap().session.as_ref().map(|s| s.dir.clone())
    }

    /// O comando de delegação da sessão viva, como o orquestrador o digita.
    pub fn delegate_command(&self) -> Option<String> {
        self.inner.lock().unwrap().session.as_ref().map(|s| s.command.clone())
    }

    fn teardown(&self, inner: &mut Inner) {
        inner.epoch += 1;
        inner.slots.clear();
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
        let slot = inner.slots.entry(id.to_string()).or_insert_with(|| Slot { buffer: Shared::new(Mutex::new(Buffer::default())), term: None, size: EXECUTOR_SIZE, gen });
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
        } else if let Some(i) = inner.nodes.iter().position(|n| n.info.id == id && n.info.state == NodeState::Running) {
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
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let mut inner = self.inner.lock().unwrap();
            if inner.epoch != epoch {
                return;
            }
            self.accept_batches(&mut inner);
            for i in 0..inner.nodes.len() {
                if inner.nodes[i].info.state == NodeState::Running {
                    if let Some(text) = self.signal(&inner.nodes[i]) {
                        self.finish(&mut inner, i, NodeState::Done, Outcome { text, exit: "0".into(), ok: true });
                    }
                }
            }
        }
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
                inner.slots.insert(node_id.clone(), Slot { buffer: Shared::new(Mutex::new(Buffer::default())), term: None, size: EXECUTOR_SIZE, gen: 0 });
                inner.nodes.push(Node { info, dir: exec.join(&node_id), prompt: cli::executor_prompt(task) });
            }
            // Cada nó entra no canvas já no estado em que vai ficar: rodando, na fila ou esperando a escolha.
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
    fn schedule_from(&self, inner: &mut Inner, new_from: usize, announce: bool) {
        let mut running = inner.nodes.iter().filter(|n| n.info.state == NodeState::Running).count();
        for i in 0..inner.nodes.len() {
            let is_new = announce && i >= new_from;
            let before = inner.nodes[i].info.state;
            if matches!(before, NodeState::AwaitingChoice | NodeState::Queued) {
                if let Some(choice) = inner.choice.clone() {
                    if running < inner.max_parallel {
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

    /// Estado final de um nó: avisa o frontend, encerra o processo, guarda o resultado no lote
    /// e, se era o último, entrega o lote. Não mexe na fila (quem chama decide).
    fn finish_quiet(&self, inner: &mut Inner, i: usize, state: NodeState, outcome: Outcome) {
        let node = &mut inner.nodes[i].info;
        node.state = state;
        let (id, batch, index) = (node.id.clone(), node.batch.clone(), node.index);
        self.announce(&inner.nodes[i].info, false);
        if state == NodeState::Done {
            // O sinal de fim chega antes do último quadro da tela: o processo ganha um instante
            // para desenhá-lo, e o que fica guardado no bloco é a tela final.
            let (core, gen) = (self.clone(), inner.slots.get(&id).map_or(0, |s| s.gen));
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1500));
                if let Some(slot) = core.inner.lock().unwrap().slots.get_mut(&id) {
                    if slot.gen == gen {
                        slot.term = None;
                    }
                }
            });
        } else if let Some(slot) = inner.slots.get_mut(&id) {
            slot.term = None;
        }
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
        let git = proc::git(Path::new(&project), &["status", "--porcelain", "--untracked-files=all"]);
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
        let (code, text) = format_result(&items, &Ok(" M a.txt\n?? b.txt\n".into()));
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
        let (code, text) = format_result(&items, &Ok("\n".into()));
        assert_ne!(code, 0);
        assert!(text.contains("--- EXECUTOR 2/3 ---\nB\ninterrompido pelo usuário\nexecutor_exit: interrompido (FALHOU)\n"));
        assert!(text.contains("--- EXECUTOR 3/3 ---\nC\nO executor terminou sem sinal de fim de rodada (código de saída 3).\nexecutor_exit: 3 (FALHOU)\n"));
        assert!(text.ends_with("delegação):\n(nada)\n--- FIM ---\n"));
        let (_, text) = format_result(&items[..1], &Err("código 128".into()));
        assert!(text.ends_with("(git status falhou: código 128)\n--- FIM ---\n"));
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
}
