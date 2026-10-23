//! O ciclo inteiro sem modelo: o app de verdade (Core), o servidor MCP dele (`orquestrador --mcp <pasta da sessão>`)
//! falado por um cliente de teste em JSON-RPC pela entrada e saída padrão, e CLIs falsos (examples/fake_cli.rs) no
//! lugar do claude e do codex.
use base64::Engine;
use orquestrador_lib::capacity::{Memory, Resources};
use orquestrador_lib::cli::Exes;
use orquestrador_lib::config::Choice;
use orquestrador_lib::core::{title, Core, Emit, ORCHESTRATOR};
use orquestrador_lib::session::Places;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

const GIB: u64 = 1 << 30;

/// O que o orquestrador escreve numa tarefa e precisa chegar igual ao executor: aspa dupla pura, barra antes
/// da aspa, JSON com escapes, tarefa com espaço terminada em barra, acentos, apóstrofos, várias linhas e caracteres
/// que um shell comeria (`$`, `;`, crase). As tarefas viajam em JSON pelo MCP, sem passar pelo shell.
const TASKS: [&str; 11] = [
    "Diga \"oi\" e \"tchau\" puros",
    "Diga \\\"oi\\\" com barra antes da aspa",
    r#"{"a": "b\"c", "d": "e\\f", "g": "\u00e7"}"#,
    "Copie C:\\Users\\Maria Silva\\",
    "Ação: d'água, coração, ''duplo''",
    "Linha 1 \"x\"\nLinha 2 sem fim \\",
    "\"",
    "'começa e termina com apóstrofo'",
    "Ação: d'água, it's ''ok''",
    "Linha 1; (x) <y> *z* @ $HOME\nLinha 2 `crase` \"aspas\" e barra no fim\\",
    "C:\\Users\\Maria Silva\\ ; $(calc) && não",
];

/// Um cliente do servidor MCP do orquestrador: sobe `orquestrador --mcp <pasta>` e fala com ele por pipe, uma
/// mensagem JSON por linha. As respostas que chegam fora de ordem ficam guardadas até alguém pedir o id delas.
/// Ao sair de escopo, o servidor morre: uma chamada que ainda espera o lote deixa de ser respondida, e o lote segue.
struct Mcp {
    child: Child,
    input: ChildStdin,
    lines: Receiver<Value>,
    waiting: Vec<Value>,
    next: u64,
}

impl Mcp {
    /// Sobe o servidor da pasta, sem falar com ele ainda.
    fn spawn(session: &Path) -> Mcp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_orquestrador")).arg("--mcp").arg(session).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
            }
        });
        Mcp { child, input, lines, waiting: Vec::new(), next: 1 }
    }

    /// Sobe o servidor e faz o `initialize` e a notificação que um cliente manda antes de qualquer chamada.
    fn open(session: &Path) -> Mcp {
        let mut mcp = Mcp::spawn(session);
        let id = mcp.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "teste", "version": "0" } }));
        assert_eq!(mcp.reply(id)["result"]["protocolVersion"], "2025-06-18");
        mcp.write(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        mcp
    }

    fn write(&mut self, msg: Value) {
        writeln!(self.input, "{msg}").unwrap();
        self.input.flush().unwrap();
    }

    /// Manda uma requisição e devolve o id dela, sem esperar a resposta.
    fn request(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next;
        self.next += 1;
        self.write(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    /// Espera a resposta do `id` por até 60 s, para um teste nunca travar. As outras que chegarem ficam guardadas.
    fn reply(&mut self, id: u64) -> Value {
        if let Some(i) = self.waiting.iter().position(|m| m["id"] == id) {
            return self.waiting.remove(i);
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let msg = self.lines.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap_or_else(|_| panic!("o servidor não respondeu o id {id}"));
            if msg["id"] == id {
                return msg;
            }
            self.waiting.push(msg);
        }
    }

    /// Manda a chamada de uma ferramenta e não espera: para um lote que não vai terminar, ou para um cliente que morre.
    fn start(&mut self, name: &str, args: Value) -> u64 {
        self.request("tools/call", json!({ "name": name, "arguments": args }))
    }

    /// Espera a chamada `id` e devolve o texto dela e se veio com `isError`.
    fn done(&mut self, id: u64) -> (String, bool) {
        let reply = self.reply(id);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("resposta sem texto: {reply}"));
        (text.to_string(), reply["result"]["isError"].as_bool().unwrap())
    }

    /// Chama uma ferramenta e espera a resposta: (texto, isError).
    fn call(&mut self, name: &str, args: Value) -> (String, bool) {
        let id = self.start(name, args);
        self.done(id)
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Bench {
    core: Core,
    events: Receiver<(String, Value)>,
    root: PathBuf,
    /// A memória física que o teto enxerga; o teste muda os dois números no meio do caminho.
    total: Arc<AtomicU64>,
    free: Arc<AtomicU64>,
    /// Tudo que `wait`, `states` e `drain` já leram dos eventos, para perguntar depois se um nó passou por um estado.
    seen: RefCell<Vec<(String, Value)>>,
}

/// Uma sessão de teste cujo temp fica em `root\<temp>`: o nome dessa pasta é o que o teste quer provar.
/// `cpus` são as CPUs que o teste escolhe: `Some(8)` dá teto 4 (com 64 GiB livres, sem apertar a fila),
/// `Some(4)` dá teto 2 e `Some(2)` dá teto 1.
fn bench(tag: &str, temp: &str, cpus: Option<usize>) -> Bench {
    let root = std::env::temp_dir().join(format!("orq-lote-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(root.join(temp)).unwrap();
    assert!(Command::new("git").arg("init").arg("-q").arg(&repo).status().unwrap().success());
    std::fs::write(repo.join("novo.txt"), "x").unwrap();
    // target\debug\deps\lote-*.exe -> target\debug\examples\fake_cli.exe
    let fake = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("examples").join("fake_cli.exe");
    assert!(fake.is_file(), "falta {} (rode `cargo test`, que compila os exemplos)", fake.display());
    let fake = fake.to_string_lossy().to_string();
    let exes = Exes { claude: fake.clone(), codex: fake, app: env!("CARGO_BIN_EXE_orquestrador").to_string() };
    let places = Places { temp: root.join(temp), codex_rules: root.join("rules") };
    let (tx, events) = channel();
    let emit: Emit = Arc::new(move |event, payload| drop(tx.send((event.to_string(), payload))));
    let total = Arc::new(AtomicU64::new(64 * GIB));
    let free = Arc::new(AtomicU64::new(64 * GIB));
    let (t, f) = (total.clone(), free.clone());
    let memory = move || Some(Memory { total: t.load(Ordering::SeqCst), available: f.load(Ordering::SeqCst) });
    let resources = Resources { cpus, memory: Arc::new(memory) };
    let core = Core::new(exes, places, root.join("config.json"), emit).with_resources(resources);
    core.set_project(&repo).unwrap();
    Bench { core, events, root, total, free, seen: RefCell::new(Vec::new()) }
}

impl Bench {
    /// Põe a memória que o teto enxerga a partir do próximo passe da fila.
    fn memoria(&self, total: u64, livre: u64) {
        self.total.store(total, Ordering::SeqCst);
        self.free.store(livre, Ordering::SeqCst);
    }

    /// Guarda o evento lido, para `passou` poder perguntar depois.
    fn lembra(&self, event: &str, payload: &Value) {
        if event == "node-state" {
            self.seen.borrow_mut().push((event.to_string(), payload.clone()));
        }
    }

    /// Lê os eventos que já chegaram, sem esperar.
    fn drain(&self) {
        while let Ok((event, payload)) = self.events.try_recv() {
            if event != "term-data" {
                self.lembra(&event, &payload);
            }
        }
    }

    /// Se algum evento de estado já lido diz que o nó `id` esteve em um destes `states`.
    fn passou(&self, id: &str, states: &[&str]) -> bool {
        self.seen.borrow().iter().any(|(_, p)| p["id"] == id && states.contains(&p["state"].as_str().unwrap_or("")))
    }

    /// O próximo evento que não seja `term-data` e que satisfaça `want`.
    fn wait(&self, what: &str, want: impl Fn(&str, &Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        while let Ok((event, payload)) = self.events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            if event != "term-data" {
                self.lembra(&event, &payload);
                if want(&event, &payload) {
                    return payload;
                }
            }
        }
        panic!("o evento esperado não chegou: {what}");
    }

    fn state(&self, id: &str, state: &str) -> Value {
        if let Some((_, payload)) = self.seen.borrow().iter().rev().find(|(_, p)| p["id"] == id && p["state"] == state) {
            return payload.clone();
        }
        self.wait(&format!("{id} -> {state}"), |e, p| e == "node-state" && p["id"] == id && p["state"] == state)
    }

    /// Espera todos os pares (nó, estado) acontecerem, em qualquer ordem: dois executores podem mudar de estado
    /// na ordem que o relógio der.
    fn states(&self, wants: &[(&str, &str)]) {
        let mut left: Vec<(&str, &str)> = wants.to_vec();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !left.is_empty() {
            let Ok((event, payload)) = self.events.recv_timeout(deadline.saturating_duration_since(Instant::now())) else {
                panic!("os estados esperados não chegaram: {left:?}");
            };
            self.lembra(&event, &payload);
            if event == "node-state" {
                left.retain(|(id, state)| !(payload["id"] == *id && payload["state"] == *state));
            }
        }
    }

    fn added(&self, n: usize) -> Vec<Value> {
        (0..n).map(|_| {
            let node = self.wait("node-added", |e, _| e == "node-added");
            self.seen.borrow_mut().push(("node-added".into(), node.clone()));
            node
        }).collect()
    }

    /// Um cliente MCP ligado à sessão do orquestrador aberta.
    fn mcp(&self) -> Mcp {
        Mcp::open(&self.core.session_dir().expect("a sessão do orquestrador está aberta"))
    }

    /// Quantas pastas de lote há na sessão: `aguardar` não pode acrescentar nenhuma.
    fn requests(&self) -> usize {
        std::fs::read_dir(self.core.session_dir().unwrap().join("requests")).unwrap().flatten().count()
    }

    /// A pasta do lote publicado por último (o nome é cronológico).
    fn last_batch(&self) -> PathBuf {
        let dir = self.core.session_dir().unwrap().join("requests");
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.ends_with(".tmp")).collect();
        names.sort();
        dir.join(names.pop().expect("um lote publicado"))
    }

    /// Espera todo lote publicado ter a marca de entregue, que o servidor grava depois de a resposta sair.
    fn entregues(&self) {
        let dir = self.core.session_dir().unwrap();
        until("os lotes entregues", || {
            std::fs::read_dir(dir.join("requests")).unwrap().flatten().all(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.ends_with(".tmp") || dir.join("results").join(format!("{name}.delivered")).exists()
            })
        });
    }

    /// Os passos do começo da maioria dos testes: abre o orquestrador claude, escolhe claude de executor e
    /// espera o orquestrador falso desenhar.
    fn pronto(&self) {
        self.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
        self.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
        until("o orquestrador falso desenhar", || self.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));
    }

    fn screen(&self, id: &str) -> String {
        let snap = self.core.term_snapshot(id);
        String::from_utf8_lossy(&base64::engine::general_purpose::STANDARD.decode(snap.unwrap().data).unwrap()).to_string()
    }
}

/// Todo teste acaba com a sessão encerrada e a pasta apagada, passe ou falhe.
impl Drop for Bench {
    fn drop(&mut self) {
        self.core.shutdown();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_string()
}

fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "não aconteceu: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn lote_com_escolha_fila_parada_falha_e_aguardar() {
    // 4 CPUs lógicas dão teto 2; a memória de sobra não aperta.
    let b = bench("lote", "temp", Some(4));
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));
    let mut m = b.mcp();

    // Lote de três, ainda sem escolha de executor: todos esperam.
    let run = m.start("delegar", json!({ "tarefas": ["Tarefa A: ação\nlinha 2 d'água", "DORMIR B", "FALHAR C"] }));
    let nodes = b.added(3);
    let (a, bb, c) = (id(&nodes[0]), id(&nodes[1]), id(&nodes[2]));
    for (i, n) in nodes.iter().enumerate() {
        assert_eq!((n["state"].as_str(), n["index"].as_u64(), n["choice"].is_null()), (Some("awaiting-choice"), Some(i as u64), true));
        assert_eq!(n["batch"], nodes[0]["batch"]);
    }
    assert_eq!(nodes[0]["title"], "Tarefa A: ação");
    // Nó ainda sem PTY: retrato vazio com seq 0, e o tamanho pedido fica guardado.
    let empty = b.core.term_snapshot(&a);
    let empty = empty.unwrap();
    assert_eq!((empty.seq, empty.data.as_str()), (0, ""));
    b.core.term_resize(&a, 90, 25).unwrap();
    assert_eq!(b.core.term_snapshot("não existe").unwrap().seq, 0);

    // Teto 2 (das CPUs): A e B abrem, C fica na fila; quando A termina, C abre e falha sem sinal de fim.
    assert!(b.core.set_executor_choice(Choice::new("codex", "", "enorme")).is_err());
    b.core.set_executor_choice(Choice::new("codex", "gpt-x", "low")).unwrap();
    assert_eq!(b.state(&a, "running")["choice"]["model"], "gpt-x");
    b.state(&bb, "running");
    b.state(&c, "queued");
    b.state(&a, "done");
    b.state(&c, "running");
    b.state(&c, "failed");
    until("B desenhar", || b.screen(&bb).contains("dormindo"));
    assert!(b.core.term_snapshot(&bb).unwrap().seq >= 1);

    // Teclado só chega ao orquestrador.
    b.core.term_write(&bb, "sair\r").unwrap();
    b.core.term_write(ORCHESTRATOR, "oi\r").unwrap();
    until("o eco do orquestrador", || b.screen(ORCHESTRATOR).contains("eco:oi"));

    b.core.stop_executor(&bb).unwrap();
    b.state(&bb, "stopped");
    let (out, is_error) = m.done(run);
    assert!(!is_error, "{out}");
    let expected = "--- EXECUTOR 1/3 ---\nTarefa A: ação\nfeito: linha 2 d'água\nexecutor_exit: 0\n\
        --- EXECUTOR 2/3 ---\nDORMIR B\ninterrompido pelo usuário\nexecutor_exit: interrompido (FALHOU)\n\
        --- EXECUTOR 3/3 ---\nFALHAR C\nO executor terminou sem sinal de fim de rodada (código de saída 3).\nexecutor_exit: 3 (FALHOU)\n\
        --- GIT ---\ngit status (tudo o que está alterado agora, inclusive o que já estava pendente antes desta delegação):\n\
        ?? novo.txt\n--- FIM ---\n";
    assert_eq!(out, expected);
    // O executor concluído já saiu: o slot foi tirado junto com o estado final, e a tela não fica guardada.
    assert_eq!(b.core.term_snapshot(&a).unwrap().seq, 0, "o slot do executor concluído saiu");

    // Com a escolha feita, o próximo lote abre sozinho; agora com executor Claude.
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    let run = m.start("delegar", json!({ "tarefas": ["Só uma: coração"] }));
    let node = &b.added(1)[0];
    assert_eq!(b.state(&id(node), "running")["choice"]["tool"], "claude");
    let (out, is_error) = m.done(run);
    assert!(!is_error && out.starts_with("--- EXECUTOR 1/1 ---\nSó uma: coração\nfeito: Só uma: coração\nexecutor_exit: 0\n--- GIT ---\n"), "{out}");

    // O cliente morre no meio da espera: o lote segue, e um `aguardar` de outro cliente o devolve, sem publicar lote novo.
    let mut dying = b.mcp();
    dying.start("delegar", json!({ "tarefas": ["DORMIR D"] }));
    let d = id(&b.added(1)[0]);
    let before = b.requests();
    drop(dying);
    let mut m = b.mcp();
    let waiting = m.start("aguardar", json!({}));
    std::thread::sleep(Duration::from_millis(1500));
    b.core.stop_executor(&d).unwrap();
    let (out, is_error) = m.done(waiting);
    assert!(!is_error && out.starts_with("--- EXECUTOR 1/1 ---\nDORMIR D\ninterrompido pelo usuário\n") && out.ends_with("--- FIM ---\n"), "{out}");
    assert_eq!(b.requests(), before, "aguardar não publica lote novo");

    // Entregue: não há mais o que aguardar. Os erros voltam com isError e o texto do motivo, sem --- FIM ---.
    b.entregues();
    let (text, is_error) = m.call("aguardar", json!({}));
    assert!(is_error && text == "Nenhum lote aguardando entrega.", "{text}");
    let (text, is_error) = m.call("delegar", json!({ "tarefas": ["ok", "  "] }));
    assert!(is_error && text == "Tarefa vazia.", "{text}");
    let (text, is_error) = m.call("delegar", json!({ "tarefas": [] }));
    assert!(is_error && text.starts_with("Nenhuma tarefa"), "{text}");
    assert_eq!(b.requests(), before, "tarefa vazia não chega a publicar lote");

    assert!(b.core.has_running().unwrap());
    let session = b.core.session_dir().unwrap();
    b.core.shutdown();
    assert!(!b.core.has_running().unwrap());
    assert!(!session.exists(), "a pasta da sessão tem de sair no shutdown");
}

/// Publica `tasks` pelo `delegar` e confere, byte a byte, o texto publicado de cada tarefa e o título de cada nó.
/// O lote termina sem erro, com um EXECUTOR por tarefa.
fn publica_e_confere(b: &Bench, tasks: &[String]) {
    b.pronto();
    let mut m = b.mcp();
    let run = m.start("delegar", json!({ "tarefas": tasks }));
    let nodes = b.added(tasks.len());
    let (out, is_error) = m.done(run);
    assert!(!is_error, "{out}");
    let batch = b.last_batch();
    for (n, task) in tasks.iter().enumerate() {
        let published = std::fs::read_to_string(batch.join(format!("{}.txt", n + 1))).unwrap();
        assert_eq!(published, *task, "a tarefa {} chegou diferente do pedido", n + 1);
        assert_eq!(nodes[n]["title"], title(task), "título da tarefa {}", n + 1);
    }
    assert!(out.contains(&format!("--- EXECUTOR {0}/{0} ---\n", tasks.len())), "{out}");
}

/// O texto publicado é o pedido, byte a byte, e o título de cada nó é o título dele; o lote termina sem erro.
#[test]
fn textos_com_aspas_barra_e_acentos_chegam_iguais_ao_pedido() {
    let b = bench("texto", "temp", Some(8));
    let tasks: Vec<String> = TASKS.iter().map(|t| t.to_string()).collect();
    publica_e_confere(&b, &tasks);
}

/// Com a pasta da sessão em caminho com espaço, acento e apóstrofo, o servidor sobe e o lote roda.
#[test]
fn caminhos_com_espaco_acento_e_apostrofo_funcionam() {
    let b = bench("caminhos", "temp com espaço, açaí e it's", Some(8));
    b.pronto();
    let session = b.core.session_dir().unwrap();
    let text = session.to_string_lossy();
    assert!(text.contains(' ') && text.contains('ç') && text.contains('\''), "a pasta da sessão tem esses caracteres: {text}");
    let mut m = b.mcp();
    let run = m.start("delegar", json!({ "tarefas": ["Só uma: coração"] }));
    b.added(1);
    let (out, is_error) = m.done(run);
    assert!(!is_error && out.starts_with("--- EXECUTOR 1/1 ---\nSó uma: coração\nfeito: Só uma: coração\nexecutor_exit: 0\n--- GIT ---\n"), "{out}");
}

#[test]
fn saida_do_orquestrador_mata_os_executores_e_limpa_a_sessao() {
    let b = bench("saida", "temp", Some(8));
    assert!(b.core.start_orchestrator(Choice::new("gemini", "", ""), 100, 30).is_err());
    b.core.start_orchestrator(Choice::new("codex", "", ""), 100, 30).unwrap();
    assert!(b.core.start_orchestrator(Choice::new("codex", "", ""), 100, 30).unwrap_err().contains("Já há um orquestrador"));
    let session = b.core.session_dir().unwrap();
    let name = session.file_name().unwrap().to_string_lossy().into_owned();
    // O orquestrador Codex não leva regra em CODEX_HOME: a sessão não escreve nada lá.
    let rule = b.root.join("rules").join(format!("{name}.rules"));
    assert!(!rule.exists() && !b.root.join("rules").exists());
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));

    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    let mut m = b.mcp();
    let run = m.start("delegar", json!({ "tarefas": ["DORMIR"] }));
    let node = id(&b.added(1)[0]);
    until("o executor desenhar", || b.screen(&node).contains("dormindo"));

    b.core.term_write(ORCHESTRATOR, "sair\r").unwrap();
    assert_eq!(b.wait("orchestrator-exit", |e, _| e == "orchestrator-exit")["code"], 7);
    assert!(!b.core.has_running().unwrap());
    assert!(!session.exists());
    // A chamada que esperava percebe que a sessão acabou em vez de esperar para sempre.
    let (text, is_error) = m.done(run);
    assert!(is_error && text == "A sessão do orquestrador terminou antes do fim do lote.", "{text}");

    // Reabre vazio, e a última escolha do orquestrador ficou salva.
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(b.root.join("config.json")).unwrap()).unwrap();
    assert_eq!((saved["orchestrator"]["tool"].as_str(), saved["executor"]["tool"].as_str()), (Some("claude"), Some("claude")));
}

/// Tarefa longa, com Unicode, aspas e barra no fim, de uns 9000 bytes.
fn tarefa_longa(n: usize) -> String {
    let mut text = format!("Tarefa longa {n}: ação, coração, \"aspas\" e C:\\Users\\Maria Silva\\\n");
    while text.len() < 9000 {
        text += "Linha de preenchimento: acentuação é ç ã, \"aspas\", barra \\ e apóstrofo d'água.\n";
    }
    text + &format!("Fim da tarefa {n}: C:\\pasta\\")
}

/// Um lote com mais de 40 mil caracteres, com tarefas longas, Unicode, aspas e barra no fim: cada texto publicado
/// é o original, byte a byte, e o lote termina sem erro.
#[test]
fn lote_com_mais_de_40_mil_caracteres_chega_inteiro() {
    let b = bench("longo", "temp", Some(8));
    let tasks: Vec<String> = (1..=5).map(tarefa_longa).collect();
    let chars: usize = tasks.iter().map(|t| t.chars().count()).sum();
    assert!(chars > 40_000, "o lote tem de passar de 40 mil caracteres: {chars}");
    publica_e_confere(&b, &tasks);
}

/// O pedido de confirmação do CLI passa pelo hook `PermissionRequest`, que roda o binário de verdade em `--allow`:
/// aceito na hora, sem parar o executor nem o lote.
#[test]
fn pergunta_do_executor_e_aceita_na_hora() {
    let b = bench("pergunta", "temp", Some(8));
    b.pronto();
    let mut m = b.mcp();

    let run = m.start("delegar", json!({ "tarefas": ["PERGUNTA: remover o log", "Tarefa B: coração"] }));
    let nodes = b.added(2);
    b.states(&[(id(&nodes[0]).as_str(), "done"), (id(&nodes[1]).as_str(), "done")]);
    let (out, is_error) = m.done(run);
    assert!(
        !is_error && out.starts_with("--- EXECUTOR 1/2 ---\nPERGUNTA: remover o log\naceito\nexecutor_exit: 0\n--- EXECUTOR 2/2 ---\nTarefa B: coração\nfeito: Tarefa B: coração\nexecutor_exit: 0\n--- GIT ---\n"),
        "{out}"
    );
}

/// Pouca memória livre deixa um executor só. O resto espera, e a fila só anda quando a memória volta; quem já
/// rodava segue vivo.
#[test]
fn memoria_baixa_segura_a_fila_e_a_memoria_de_volta_abre_sem_matar_quem_roda() {
    let b = bench("memoria", "temp", Some(16)); // teto de CPU 8
    b.memoria(64 * GIB, GIB * 5 / 2); // 2,5 GiB livres: a reserva de 2 GiB deixa 0,5 GiB, nenhuma vaga
    b.pronto();
    let mut m = b.mcp();

    let _run = m.start("delegar", json!({ "tarefas": ["DORMIR A", "DORMIR B"] }));
    let nodes = b.added(2);
    let (a, q) = (id(&nodes[0]), id(&nodes[1]));
    assert!(nodes.iter().all(|n| n["state"] == "queued"));
    b.state(&a, "running");

    // Passam-se várias reavaliações da fila e nada abre: sem vaga de memória, B fica parado.
    std::thread::sleep(Duration::from_millis(2500));
    b.drain();
    assert!(!b.passou(&q, &["running"]), "sem memória, B não abre");
    assert_eq!(b.core.term_snapshot(&q).unwrap().seq, 0, "B não tem processo");

    // A memória voltou: B abre no próximo passe, e A continua vivo.
    b.memoria(64 * GIB, 64 * GIB);
    b.state(&q, "running");
    b.drain();
    assert!(!b.passou(&a, &["done", "failed", "stopped"]), "A não terminou nem foi parado");
}

/// A RAM total cai no meio do lote: os vivos seguem até o fim (nenhum é parado ou reiniciado), e só o que chega
/// depois espera. Quando a RAM volta, o que esperava abre.
#[test]
fn reducao_de_memoria_nao_mata_vivos_e_a_retomada_abre_a_fila() {
    let b = bench("reducao", "temp", Some(16)); // teto de CPU 8
    b.pronto();
    let mut m = b.mcp();

    let _run = m.start("delegar", json!({ "tarefas": ["DORMIR A", "DORMIR B", "DORMIR C"] }));
    let nodes = b.added(3);
    let vivos: Vec<String> = nodes.iter().map(id).collect();
    assert!(nodes.iter().all(|n| n["state"] == "queued"), "os novos entram primeiro na fila global: {nodes:?}");
    for vivo in &vivos { b.state(vivo, "running"); }

    // RAM total de 4 GiB: teto 2 com três vivos. O quarto espera e nenhum vivo cai.
    b.memoria(4 * GIB, 4 * GIB);
    let _run2 = m.start("delegar", json!({ "tarefas": ["DORMIR D"] }));
    let quarto = b.added(1).remove(0);
    let d = id(&quarto);
    assert_eq!(quarto["state"], "queued", "com teto 2 e três vivos, o quarto espera");
    std::thread::sleep(Duration::from_millis(2500));
    b.drain();
    assert!(!b.passou(&d, &["running"]), "com a RAM menor, D espera");
    for v in &vivos {
        assert!(!b.passou(v, &["done", "failed", "stopped"]), "{v} seguiu vivo");
    }

    // A RAM voltou: D abre no próximo passe, e os três seguem vivos.
    b.memoria(64 * GIB, 64 * GIB);
    b.state(&d, "running");
    b.drain();
    for v in &vivos {
        assert!(!b.passou(v, &["done", "failed", "stopped"]), "{v} seguiu vivo depois da retomada");
    }
}

/// Com um executor por vez (pouca memória), a fila sai na ordem de chegada: cada um abre quando o anterior termina.
#[test]
fn fila_sai_na_ordem_de_chegada_com_um_executor_por_vez() {
    let b = bench("fifo", "temp", Some(16));
    b.memoria(64 * GIB, GIB * 5 / 2); // teto 1
    b.pronto();
    let mut m = b.mcp();

    let run = m.start("delegar", json!({ "tarefas": ["Tarefa A", "Tarefa B", "Tarefa C"] }));
    let nodes = b.added(3);
    let (a, bb, c) = (id(&nodes[0]), id(&nodes[1]), id(&nodes[2]));
    assert!(nodes.iter().all(|n| n["state"] == "queued"));
    b.state(&a, "running");

    b.state(&a, "done");
    b.state(&bb, "running");
    b.drain();
    assert!(!b.passou(&c, &["running"]), "C espera B, que é o próximo da fila");
    b.state(&bb, "done");
    b.state(&c, "running");
    b.state(&c, "done");
    let (out, is_error) = m.done(run);
    assert!(!is_error, "{out}");
    assert!(out.contains("--- EXECUTOR 1/3 ---\nTarefa A\nfeito: Tarefa A\nexecutor_exit: 0\n--- EXECUTOR 2/3 ---\nTarefa B\n"), "{out}");
}
