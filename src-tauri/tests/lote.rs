//! O ciclo inteiro sem modelo: o app de verdade (Core), o `delegate.ps1` de verdade rodando no
//! Windows PowerShell 5.1 e no pwsh, o manipulador `orquestrador.exe --hook` de verdade, e CLIs
//! falsos (examples/fake_cli.rs) no lugar do claude e do codex.
use base64::Engine;
use orquestrador_lib::cli::Exes;
use orquestrador_lib::config::Choice;
use orquestrador_lib::core::{Core, Emit, ORCHESTRATOR};
use orquestrador_lib::session::Places;
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Bench {
    core: Core,
    events: Receiver<(String, Value)>,
    root: PathBuf,
}

fn bench(tag: &str) -> Bench {
    let root = std::env::temp_dir().join(format!("orq-lote-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(root.join("temp")).unwrap();
    assert!(Command::new("git").arg("init").arg("-q").arg(&repo).status().unwrap().success());
    std::fs::write(repo.join("novo.txt"), "x").unwrap();
    // target\debug\deps\lote-*.exe -> target\debug\examples\fake_cli.exe
    let fake = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("examples").join("fake_cli.exe");
    assert!(fake.is_file(), "falta {} (rode `cargo test`, que compila os exemplos)", fake.display());
    let fake = fake.to_string_lossy().to_string();
    let exes = Exes { claude: fake.clone(), codex: fake, app: env!("CARGO_BIN_EXE_orquestrador").to_string() };
    let places = Places { temp: root.join("temp"), codex_rules: root.join("rules"), global_dirs: vec![] };
    let (tx, events) = channel();
    let emit: Emit = Arc::new(move |event, payload| drop(tx.send((event.to_string(), payload))));
    let core = Core::new(exes, places, root.join("config.json"), emit);
    core.set_project(&repo).unwrap();
    Bench { core, events, root }
}

impl Bench {
    /// O próximo evento que não seja `term-data` e que satisfaça `want`.
    fn wait(&self, what: &str, want: impl Fn(&str, &Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        while let Ok((event, payload)) = self.events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            if event != "term-data" && want(&event, &payload) {
                return payload;
            }
        }
        panic!("o evento esperado não chegou: {what}");
    }

    fn state(&self, id: &str, state: &str) -> Value {
        self.wait(&format!("{id} -> {state}"), |e, p| e == "node-state" && p["id"] == id && p["state"] == state)
    }

    fn added(&self, n: usize) -> Vec<Value> {
        (0..n).map(|_| self.wait("node-added", |e, _| e == "node-added")).collect()
    }

    fn delegate(&self, shell: &str, tasks: &[&str]) -> Child {
        let launcher = self.core.session_dir().unwrap().join("delegate.ps1");
        Command::new(shell)
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(launcher)
            .args(tasks)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn screen(&self, id: &str) -> String {
        let snap = self.core.term_snapshot(id);
        String::from_utf8_lossy(&base64::engine::general_purpose::STANDARD.decode(snap.data).unwrap()).to_string()
    }
}

fn finish(child: Child) -> (i32, String) {
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).expect("a saída do delegate.ps1 tem de ser UTF-8"))
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
    let b = bench("lote");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    let session = b.core.session_dir().unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));

    // Lote de três, ainda sem escolha de executor: todos esperam.
    let run = b.delegate("powershell", &["Tarefa A: ação\nlinha 2 d'água", "DORMIR B", "FALHAR C"]);
    let nodes = b.added(3);
    let (a, bb, c) = (id(&nodes[0]), id(&nodes[1]), id(&nodes[2]));
    for (i, n) in nodes.iter().enumerate() {
        assert_eq!((n["state"].as_str(), n["index"].as_u64(), n["choice"].is_null()), (Some("awaiting-choice"), Some(i as u64), true));
        assert_eq!(n["batch"], nodes[0]["batch"]);
    }
    assert_eq!(nodes[0]["title"], "Tarefa A: ação");
    // Nó ainda sem PTY: retrato vazio com seq 0, e o tamanho pedido fica guardado.
    let empty = b.core.term_snapshot(&a);
    assert_eq!((empty.seq, empty.data.as_str()), (0, ""));
    b.core.term_resize(&a, 90, 25);
    assert_eq!(b.core.term_snapshot("não existe").seq, 0);

    // Teto 2: A e B abrem, C fica na fila; quando A termina, C abre e falha sem sinal de fim.
    b.core.set_max_parallel(2).unwrap();
    assert!(b.core.set_max_parallel(17).is_err());
    assert!(b.core.set_executor_choice(Choice::new("codex", "", "enorme")).is_err());
    b.core.set_executor_choice(Choice::new("codex", "gpt-x", "low")).unwrap();
    assert_eq!(b.wait("executor-choice", |e, _| e == "executor-choice")["tool"], "codex");
    assert_eq!(b.state(&a, "running")["choice"]["model"], "gpt-x");
    b.state(&bb, "running");
    b.state(&c, "queued");
    b.state(&a, "done");
    b.state(&c, "running");
    b.state(&c, "failed");
    until("B desenhar", || b.screen(&bb).contains("dormindo"));
    assert!(b.core.term_snapshot(&bb).seq >= 1);

    // Teclado só chega ao orquestrador.
    b.core.term_write(&bb, "sair\r");
    b.core.term_write(ORCHESTRATOR, "oi\r");
    until("o eco do orquestrador", || b.screen(ORCHESTRATOR).contains("eco:oi"));

    b.core.stop_executor(&bb);
    b.state(&bb, "stopped");
    let (code, out) = finish(run);
    assert_eq!(code, 1, "{out}");
    let expected = "--- EXECUTOR 1/3 ---\nTarefa A: ação\nfeito: linha 2 d'água\nexecutor_exit: 0\n\
        --- EXECUTOR 2/3 ---\nDORMIR B\ninterrompido pelo usuário\nexecutor_exit: interrompido (FALHOU)\n\
        --- EXECUTOR 3/3 ---\nFALHAR C\nO executor terminou sem sinal de fim de rodada (código de saída 3).\nexecutor_exit: 3 (FALHOU)\n\
        --- GIT ---\ngit status (tudo o que está alterado agora, inclusive o que já estava pendente antes desta delegação):\n\
        ?? novo.txt\n--- FIM ---\n";
    assert_eq!(out, expected);
    // A tela do executor concluído continua guardada depois de o processo ser encerrado.
    assert!(b.screen(&a).contains("feito: linha 2"));

    // Com a escolha feita, o próximo lote abre sozinho; agora pelo pwsh e com executor Claude.
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    let run = b.delegate("pwsh", &["Só uma: coração"]);
    let node = &b.added(1)[0];
    assert_eq!((node["state"].as_str(), node["choice"]["tool"].as_str()), (Some("running"), Some("claude")));
    let (code, out) = finish(run);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("--- EXECUTOR 1/1 ---\nSó uma: coração\nfeito: Só uma: coração\nexecutor_exit: 0\n--- GIT ---\n"), "{out}");

    // O comando morto por tempo de espera: o lote segue, e '--aguardar' volta a esperar por ele.
    let mut run = b.delegate("powershell", &["DORMIR D"]);
    let d = id(&b.added(1)[0]);
    run.kill().unwrap();
    run.wait().unwrap();
    let waiting = b.delegate("powershell", &["--aguardar"]);
    std::thread::sleep(Duration::from_millis(1500));
    b.core.stop_executor(&d);
    let (code, out) = finish(waiting);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("--- EXECUTOR 1/1 ---\nDORMIR D\ninterrompido pelo usuário\n") && out.ends_with("--- FIM ---\n"), "{out}");
    // Entregue: não há mais o que aguardar. Erros do wrapper fecham com a linha final e código 4.
    for (shell, args, why) in [("powershell", vec!["--aguardar"], "Nenhum lote aguardando entrega."), ("pwsh", vec!["ok", "  "], "Tarefa vazia."), ("powershell", vec![], "Nenhuma tarefa.")] {
        let (code, out) = finish(b.delegate(shell, &args));
        assert_eq!(code, 4, "{out}");
        assert!(out.starts_with(&format!("wrapper_error: {why}")) && out.ends_with("\n--- FIM ---\n"), "{out}");
    }

    let mut cleared = b.core.clear_done();
    cleared.sort();
    let mut all = vec![a.clone(), bb, c, id(node), d];
    all.sort();
    assert_eq!(cleared, all);
    assert_eq!(b.core.term_snapshot(&a).seq, 0);

    assert!(b.core.has_running());
    b.core.shutdown();
    assert!(!b.core.has_running());
    assert!(!session.exists(), "a pasta da sessão tem de sair no shutdown");
    let _ = std::fs::remove_dir_all(&b.root);
}

#[test]
fn saida_do_orquestrador_mata_os_executores_e_limpa_sessao_e_regra() {
    let b = bench("saida");
    assert!(b.core.start_orchestrator(Choice::new("gemini", "", ""), 100, 30).is_err());
    b.core.start_orchestrator(Choice::new("codex", "", ""), 100, 30).unwrap();
    assert!(b.core.start_orchestrator(Choice::new("codex", "", ""), 100, 30).unwrap_err().contains("Já há um orquestrador"));
    let session = b.core.session_dir().unwrap();
    let rule = b.root.join("rules").join(format!("{}.rules", session.file_name().unwrap().to_string_lossy()));
    assert!(rule.is_file());
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));

    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    let run = b.delegate("powershell", &["DORMIR"]);
    let node = id(&b.added(1)[0]);
    until("o executor desenhar", || b.screen(&node).contains("dormindo"));

    b.core.term_write(ORCHESTRATOR, "sair\r");
    assert_eq!(b.wait("orchestrator-exit", |e, _| e == "orchestrator-exit")["code"], 7);
    assert!(!b.core.has_running());
    assert!(!session.exists() && !rule.exists());
    assert!(b.core.clear_done().is_empty());
    // O delegate.ps1 que esperava percebe que a sessão acabou em vez de esperar para sempre.
    let (code, out) = finish(run);
    assert_eq!(code, 4, "{out}");
    assert!(out.starts_with("wrapper_error: ") && out.ends_with("--- FIM ---\n"), "{out}");

    // Reabre vazio, e a última escolha do orquestrador ficou salva.
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(b.root.join("config.json")).unwrap()).unwrap();
    assert_eq!((saved["orchestrator"]["tool"].as_str(), saved["executor"]["tool"].as_str()), (Some("claude"), Some("claude")));
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}

#[test]
fn pasta_fora_de_repositorio_git_e_recusada() {
    let b = bench("fora");
    let outside = PathBuf::from(std::env::var("SystemRoot").unwrap());
    assert!(b.core.set_project(&outside).unwrap_err().contains("não está dentro de um repositório Git"));
    let _ = std::fs::remove_dir_all(&b.root);
}
