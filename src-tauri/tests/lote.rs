//! O ciclo inteiro sem modelo: o app de verdade (Core), o programa `orquestrador-delegar` de verdade, pelo
//! adaptador `.ps1` que o app escreve na sessão, digitado como o orquestrador o digitaria no Windows
//! PowerShell 5.1 e no pwsh, e CLIs falsos (examples/fake_cli.rs) no lugar do claude e do codex.
use base64::Engine;
use orquestrador_lib::cli::Exes;
use orquestrador_lib::config::Choice;
use orquestrador_lib::core::{title, Core, Emit, ORCHESTRATOR};
use orquestrador_lib::session::Places;
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// O que o orquestrador escreve numa tarefa e precisa chegar igual ao executor: aspa dupla pura, barra antes
/// da aspa, JSON com escapes, tarefa com espaço terminada em barra, acentos, apóstrofos e várias linhas.
const TASKS: [&str; 8] = [
    "Diga \"oi\" e \"tchau\" puros",
    "Diga \\\"oi\\\" com barra antes da aspa",
    r#"{"a": "b\"c", "d": "e\\f", "g": "\u00e7"}"#,
    "Copie C:\\Users\\Maria Silva\\",
    "Ação: d'água, coração, ''duplo''",
    "Linha 1 \"x\"\nLinha 2 sem fim \\",
    "\"",
    "'começa e termina com apóstrofo'",
];

struct Bench {
    core: Core,
    events: Receiver<(String, Value)>,
    root: PathBuf,
}

fn bench(tag: &str) -> Bench {
    bench_in(tag, "temp")
}

/// Um canvas cujo temp fica em `root\<temp>`: o nome dessa pasta é o que o teste quer provar. O programa de
/// delegação é uma cópia numa pasta com espaço, acento e apóstrofo, porque o caminho dele não pode importar.
fn bench_in(tag: &str, temp: &str) -> Bench {
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
    let app_dir = root.join("app ação it's");
    std::fs::create_dir_all(&app_dir).unwrap();
    let delegate = app_dir.join("orquestrador-delegar.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_orquestrador-delegar"), &delegate).unwrap();
    let exes = Exes {
        claude: fake.clone(),
        codex: fake,
        app: env!("CARGO_BIN_EXE_orquestrador").to_string(),
        delegate: delegate.to_string_lossy().to_string(),
    };
    let places = Places { temp: root.join(temp), codex_rules: root.join("rules"), global_dirs: vec![] };
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

    /// Espera todos os pares (nó, estado) acontecerem, em qualquer ordem: dois executores podem mudar de estado
    /// na ordem que o relógio der.
    fn states(&self, wants: &[(&str, &str)]) {
        let mut left: Vec<(&str, &str)> = wants.to_vec();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !left.is_empty() {
            let Ok((event, payload)) = self.events.recv_timeout(deadline.saturating_duration_since(Instant::now())) else {
                panic!("os estados esperados não chegaram: {left:?}");
            };
            if event == "node-state" {
                left.retain(|(id, state)| !(payload["id"] == *id && payload["state"] == *state));
            }
        }
    }

    fn added(&self, n: usize) -> Vec<Value> {
        (0..n).map(|_| self.wait("node-added", |e, _| e == "node-added")).collect()
    }

    /// A linha de delegação como o orquestrador a digitaria: o nome da sessão, seguido das tarefas entre aspas
    /// simples, com o apóstrofo dobrado.
    fn line(&self, tasks: &[&str]) -> String {
        let mut line = self.core.delegate_command().expect("a sessão do orquestrador está aberta");
        for task in tasks {
            line += &format!(" '{}'", task.replace('\'', "''"));
        }
        line
    }

    /// Roda o shell com a pasta da sessão na frente do PATH, como o orquestrador roda (ver `which` no teste de PATH).
    fn spawn_shell(&self, shell: &str, args: &[&str]) -> Child {
        let path = format!("{};{}", self.core.session_dir().unwrap().display(), std::env::var("PATH").unwrap_or_default());
        Command::new(shell)
            .args(["-NoProfile", "-NonInteractive"])
            .args(args)
            .env("PATH", path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    /// Com `-EncodedCommand` a linha vai em base64, então o quoting do teste não mexe nela. Nada é acrescentado
    /// depois dela: o código de saída é o que o próprio adaptador deixa no processo.
    fn delegate(&self, shell: &str, tasks: &[&str]) -> Child {
        let utf16: Vec<u8> = self.line(tasks).encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
        self.spawn_shell(shell, &["-EncodedCommand", &encoded])
    }

    /// O caminho real: `-Command` com a linha, como o shell do orquestrador a roda, sem `exit` acrescentado.
    fn delegate_cmd(&self, shell: &str, tasks: &[&str]) -> Child {
        let line = self.line(tasks);
        self.spawn_shell(shell, &["-Command", &line])
    }

    /// Para lote que não cabe numa linha de comando (o próprio shell tem o limite de 32767 caracteres): a linha
    /// vai num script temporário, UTF-8 com BOM, invocado por `-File`.
    fn delegate_file(&self, shell: &str, tasks: &[&str]) -> Child {
        let script = self.root.join(format!("lote-{shell}.ps1"));
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(format!("{}\r\n", self.line(tasks)).as_bytes());
        std::fs::write(&script, bytes).unwrap();
        self.spawn_shell(shell, &["-File", script.to_str().unwrap()])
    }

    /// Quantas pastas de lote há na sessão: `--aguardar` não pode acrescentar nenhuma.
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

    fn screen(&self, id: &str) -> String {
        let snap = self.core.term_snapshot(id);
        String::from_utf8_lossy(&base64::engine::general_purpose::STANDARD.decode(snap.data).unwrap()).to_string()
    }
}

fn finish(child: Child) -> (i32, String) {
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).expect("a saída do comando de delegação tem de ser UTF-8"))
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

    // O comando morto por tempo de espera: o lote segue, e '--aguardar' volta a esperar por ele,
    // sem publicar lote novo.
    let mut run = b.delegate("powershell", &["DORMIR D"]);
    let d = id(&b.added(1)[0]);
    let before = b.requests();
    run.kill().unwrap();
    run.wait().unwrap();
    let waiting = b.delegate_cmd("powershell", &["--aguardar"]);
    std::thread::sleep(Duration::from_millis(1500));
    b.core.stop_executor(&d);
    let (code, out) = finish(waiting);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("--- EXECUTOR 1/1 ---\nDORMIR D\ninterrompido pelo usuário\n") && out.ends_with("--- FIM ---\n"), "{out}");
    assert_eq!(b.requests(), before, "--aguardar não publica lote novo");

    // Entregue: não há mais o que aguardar. Erros do comando fecham com a linha final e código 4.
    for (shell, args, why) in [
        ("powershell", vec!["--aguardar"], "Nenhum lote aguardando entrega."),
        ("pwsh", vec!["ok", "  "], "Tarefa vazia."),
        ("powershell", vec![], "Nenhuma tarefa."),
    ] {
        let (code, out) = finish(b.delegate(shell, &args));
        assert_eq!(code, 4, "{shell}: {out}");
        assert!(out.starts_with(&format!("wrapper_error: {why}")) && out.ends_with("\n--- FIM ---\n"), "{shell}: {out}");
    }
    assert_eq!(b.requests(), before, "tarefa vazia não chega a publicar lote");

    let mut cleared = b.core.clear_done();
    cleared.sort();
    let mut all = vec![a.clone(), bb, c, id(node), d];
    all.sort();
    assert_eq!(cleared, all);
    assert_eq!(b.core.term_snapshot(&a).seq, 0);

    assert!(b.core.has_running());
    let session = b.core.session_dir().unwrap();
    b.core.shutdown();
    assert!(!b.core.has_running());
    assert!(!session.exists(), "a pasta da sessão tem de sair no shutdown");
    let _ = std::fs::remove_dir_all(&b.root);
}

/// O pedido publicado é o texto original, byte a byte, nos dois shells; e o executor recebe o mesmo texto.
#[test]
fn texto_original_chega_igual_ao_pedido_publicado_nos_dois_shells() {
    let b = bench("texto");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));
    for shell in ["powershell", "pwsh"] {
        let run = b.delegate(shell, &TASKS);
        let nodes = b.added(TASKS.len());
        let (code, out) = finish(run);
        assert_eq!(code, 0, "{shell}: {out}");
        let batch = b.last_batch();
        for (n, task) in TASKS.iter().enumerate() {
            let published = std::fs::read_to_string(batch.join(format!("{}.txt", n + 1))).unwrap();
            assert_eq!(published, *task, "{shell}: a tarefa {} chegou diferente do pedido", n + 1);
            assert_eq!(nodes[n]["title"], title(task), "{shell}: título da tarefa {}", n + 1);
        }
        assert!(out.contains(&format!("--- EXECUTOR {}/{} ---\n", TASKS.len(), TASKS.len())), "{shell}: {out}");
    }
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}

/// Com a pasta da sessão e o programa em caminhos com espaço, acento e apóstrofo, o comando segue sendo um
/// nome só, e os dois shells delegam.
#[test]
fn caminhos_com_espaco_acento_e_apostrofo_funcionam_nos_dois_shells() {
    let b = bench_in("caminhos", "temp com espaço, açaí e it's");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));
    let head = b.core.delegate_command().unwrap();
    assert!(!head.contains(' ') && !head.contains('\'') && !head.contains('\\'), "o comando é um nome só: {head}");
    for shell in ["powershell", "pwsh"] {
        let run = b.delegate(shell, &["Só uma: coração"]);
        b.added(1);
        let (code, out) = finish(run);
        assert!(code == 0 && out.starts_with("--- EXECUTOR 1/1 ---\nSó uma: coração\nfeito: Só uma: coração\nexecutor_exit: 0\n--- GIT ---\n"), "{shell}: {code} {out}");
    }
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}

/// O orquestrador acha o comando de delegação na primeira pasta do PATH dele, que é a da sessão.
#[test]
fn orquestrador_acha_o_comando_de_delegacao_na_frente_do_path() {
    let b = bench("path");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));
    let head = b.core.delegate_command().unwrap();
    b.core.term_write(ORCHESTRATOR, &format!("which:{head}\r"));
    until("o PATH achar o comando na primeira pasta", || b.screen(ORCHESTRATOR).contains("achou:0"));
    b.core.term_write(ORCHESTRATOR, "which:nao-existe.ps1\r");
    until("um nome que não existe não é achado", || b.screen(ORCHESTRATOR).contains("achou:nenhum"));
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}

#[test]
fn tarefa_invalida_e_falta_de_tarefa_fecham_com_erro_e_nao_publicam() {
    let b = bench("invalida");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));
    let cases: [&[&str]; 4] = [&[""], &["  "], &["ok", " \n"], &[]];
    for shell in ["powershell", "pwsh"] {
        for tasks in cases {
            let (code, out) = finish(b.delegate(shell, tasks));
            assert_eq!(code, 4, "{shell} {tasks:?}: {out}");
            assert!(out.starts_with("wrapper_error: ") && out.ends_with("\n--- FIM ---\n"), "{shell} {tasks:?}: {out}");
        }
    }
    assert_eq!(b.requests(), 0, "nenhuma tarefa inválida chega a publicar lote");
    b.core.shutdown();
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
    let head = b.core.delegate_command().unwrap();
    assert_eq!(std::fs::read_to_string(&rule).unwrap().lines().next().unwrap(), format!("prefix_rule(pattern=[\"{head}\"], decision=\"allow\", justification=\"Delegacao autorizada ao executor\")"));
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
    // O comando que esperava percebe que a sessão acabou em vez de esperar para sempre.
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

/// Tarefa longa, com Unicode, aspas e barra no fim. Cada uma tem menos de 20000 bytes, o limite a partir do qual
/// o app passa a tarefa por arquivo; as três juntas passam de 32767 caracteres em base64.
fn tarefa_longa(n: usize) -> String {
    let mut text = format!("Tarefa longa {n}: ação, coração, \"aspas\" e C:\\Users\\Maria Silva\\\n");
    while text.len() < 9000 {
        text += "Linha de preenchimento: acentuação é ç ã, \"aspas\", barra \\ e apóstrofo d'água.\n";
    }
    text + &format!("Fim da tarefa {n}: C:\\pasta\\")
}

/// Um lote que não cabe na linha de comando do Windows, com tarefas longas, Unicode, aspas e barra no fim: nos
/// dois shells, cada texto publicado é o original, byte a byte, e o lote termina com código 0.
#[test]
fn lote_que_nao_cabe_na_linha_de_comando_chega_inteiro_nos_dois_shells() {
    let b = bench("longo");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));
    let mut tasks: Vec<String> = (1..=3).map(tarefa_longa).collect();
    tasks.push("Só uma: coração".into());
    let encoded: usize = tasks.iter().map(|t| base64::engine::general_purpose::STANDARD.encode(t).len()).sum();
    assert!(encoded > 32767, "o lote tem de passar do limite da linha de comando: {encoded}");
    let refs: Vec<&str> = tasks.iter().map(String::as_str).collect();
    for shell in ["powershell", "pwsh"] {
        let run = b.delegate_file(shell, &refs);
        let nodes = b.added(tasks.len());
        let (code, out) = finish(run);
        assert_eq!(code, 0, "{shell}: {out}");
        let batch = b.last_batch();
        for (n, task) in tasks.iter().enumerate() {
            let published = std::fs::read_to_string(batch.join(format!("{}.txt", n + 1))).unwrap();
            assert!(published == *task, "{shell}: a tarefa {} chegou diferente do pedido", n + 1);
            assert_eq!(nodes[n]["title"], title(task), "{shell}: título da tarefa {}", n + 1);
        }
        assert!(out.contains(&format!("--- EXECUTOR {0}/{0} ---\n", tasks.len())), "{shell}: {out}");
    }
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}

/// O caminho real, `-Command` sem nenhum `exit` acrescentado: sucesso sai com 0, executor que falha sai com 1 e o
/// próprio comando que falha sai com 4 (inclusive `--aguardar` sem lote pendente). Nos dois shells.
#[test]
fn chamada_real_com_command_sai_com_0_1_e_4_nos_dois_shells() {
    let b = bench("real");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));
    for shell in ["powershell", "pwsh"] {
        let run = b.delegate_cmd(shell, &["Só uma: coração"]);
        b.added(1);
        let (code, out) = finish(run);
        assert!(code == 0 && out.starts_with("--- EXECUTOR 1/1 ---\nSó uma: coração\nfeito: Só uma: coração\nexecutor_exit: 0\n--- GIT ---\n"), "{shell}: {code} {out}");

        let run = b.delegate_cmd(shell, &["FALHAR C"]);
        b.added(1);
        let (code, out) = finish(run);
        assert!(code == 1 && out.contains("executor_exit: 3 (FALHOU)") && out.ends_with("\n--- FIM ---\n"), "{shell}: {code} {out}");

        let (code, out) = finish(b.delegate_cmd(shell, &[""]));
        assert!(code == 4 && out.starts_with("wrapper_error: Tarefa vazia.") && out.ends_with("\n--- FIM ---\n"), "{shell}: {code} {out}");

        let (code, out) = finish(b.delegate_cmd(shell, &["--aguardar"]));
        assert!(code == 4 && out.starts_with("wrapper_error: Nenhum lote aguardando entrega.") && out.ends_with("\n--- FIM ---\n"), "{shell}: {code} {out}");
    }
    // Perfil do pwsh 7.4 em diante com a preferência de erro de programa nativo ligada: o código continua o do programa.
    let line = format!("$PSNativeCommandUseErrorActionPreference = $true; {}", b.line(&["--aguardar"]));
    let (code, out) = finish(b.spawn_shell("pwsh", &["-Command", &line]));
    assert!(code == 4 && out.starts_with("wrapper_error: Nenhum lote aguardando entrega."), "pwsh com preferência nativa: {code} {out}");
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}

/// O programa sozinho, sem sessão: entrada vazia, cortada, ilegível ou com argumento fecha com `wrapper_error`,
/// código 4 e `--- FIM ---`, sem publicar nada.
#[test]
fn entrada_do_adaptador_vazia_cortada_ou_ilegivel_fecha_com_wrapper_error() {
    let exe = env!("CARGO_BIN_EXE_orquestrador-delegar");
    let cases: [(&[u8], &str); 4] = [(b"", "veio vazia"), (b"cHJpbWVpcmE=;c2VndW5kYQ==", "incompleta"), (b"!!!\r\n", "não está em base64"), (b"\xFF\xFE\r\n", "não é UTF-8")];
    for (input, why) in cases {
        let mut child = Command::new(exe).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let (code, out) = finish(child);
        assert!(code == 4 && out.starts_with("wrapper_error: ") && out.contains(why) && out.ends_with("\n--- FIM ---\n"), "{why}: {code} {out}");
    }
    let child = Command::new(exe).arg("x").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let (code, out) = finish(child);
    assert!(code == 4 && out.contains("não recebe argumentos") && out.ends_with("\n--- FIM ---\n"), "{code} {out}");
}

/// Um executor para num pedido de confirmação: a delegação devolve a pergunta com código 3 sem esperar o lote, o
/// outro executor do lote termina enquanto isso, `--aguardar` devolve a pergunta de novo, e `--responder aceitar`
/// faz o executor seguir até o fim do lote, que sai com a resposta dele.
#[test]
fn pergunta_do_executor_passa_pelo_orquestrador_e_aceita_sem_parar_o_outro() {
    let b = bench("pergunta");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));

    let run = b.delegate("powershell", &["PERGUNTA: remover o log", "Tarefa B: coração"]);
    let nodes = b.added(2);
    let (p, q) = (id(&nodes[0]), id(&nodes[1]));
    b.states(&[(p.as_str(), "awaiting-orchestrator"), (q.as_str(), "done")]);
    let (code, out) = finish(run);
    assert_eq!(code, 3, "{out}");
    let asked = format!("--- PERGUNTA {p} (1/2) ---\nPERGUNTA: remover o log\nO executor parou num pedido de confirmação e espera a sua resposta.\nBash: rm /d/arquivo.log\n");
    assert!(out.starts_with(&asked) && out.ends_with("--- AGUARDANDO RESPOSTA ---\n"), "{out}");
    assert!(!out.contains("--- FIM ---") && !out.contains("Tarefa B"), "{out}");

    // `--aguardar` devolve a mesma pergunta, e não publica lote nenhum.
    let requests = b.requests();
    let (code, again) = finish(b.delegate("powershell", &["--aguardar"]));
    assert!(code == 3 && again.starts_with(&asked), "{code} {again}");
    assert_eq!(b.requests(), requests);

    // Aceitar: o nó volta a rodar, termina a rodada, e o lote sai com a resposta dele.
    let run = b.delegate("powershell", &["--responder", p.as_str(), "aceitar"]);
    b.states(&[(p.as_str(), "running"), (p.as_str(), "done")]);
    let (code, out) = finish(run);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.starts_with("--- EXECUTOR 1/2 ---\nPERGUNTA: remover o log\naceito\nexecutor_exit: 0\n--- EXECUTOR 2/2 ---\nTarefa B: coração\nfeito: Tarefa B: coração\nexecutor_exit: 0\n--- GIT ---\n"),
        "{out}"
    );
    assert_eq!(b.requests(), requests, "--responder não publica lote novo");
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}

/// Recusa com motivo: o motivo chega ao executor como a mensagem de erro, e ele segue. Os erros de `--responder`
/// (sem motivo, resposta desconhecida, executor sem pergunta) fecham com `wrapper_error` e código 4, sem gravar nada.
#[test]
fn recusa_leva_o_motivo_ao_executor_e_erros_do_responder_nao_gravam_nada() {
    let b = bench("recusa");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));

    let run = b.delegate("powershell", &["PERGUNTA: apagar o banco"]);
    let p = id(&b.added(1)[0]);
    let (code, out) = finish(run);
    assert!(code == 3 && out.starts_with(&format!("--- PERGUNTA {p} (1/1) ---\n")), "{code} {out}");
    b.states(&[(p.as_str(), "awaiting-orchestrator")]);

    let requests = b.requests();
    for (args, why) in [
        (vec!["--responder", p.as_str(), "recusar"], "recusar pede um motivo"),
        (vec!["--responder", p.as_str(), "talvez"], "resposta desconhecida"),
        (vec!["--responder", "exec-999", "aceitar"], "não tem pergunta pendente"),
    ] {
        let (code, out) = finish(b.delegate("powershell", &args));
        assert!(code == 4 && out.starts_with("wrapper_error: ") && out.contains(why) && out.ends_with("\n--- FIM ---\n"), "{why}: {code} {out}");
    }
    assert_eq!(b.requests(), requests);

    let run = b.delegate("powershell", &["--responder", p.as_str(), "recusar", "não apague o banco: use o arquivo de teste que você criou"]);
    b.states(&[(p.as_str(), "running"), (p.as_str(), "done")]);
    let (code, out) = finish(run);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.starts_with("--- EXECUTOR 1/1 ---\nPERGUNTA: apagar o banco\nrecusado: não apague o banco: use o arquivo de teste que você criou\nexecutor_exit: 0\n--- GIT ---\n"),
        "{out}"
    );
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}

/// `parar` num executor que espera resposta: ele termina como interrompido, a pergunta não volta, e o lote fecha
/// sem ela. Responder depois disso é erro.
#[test]
fn parar_num_executor_que_espera_resposta_fecha_o_lote_sem_a_pergunta() {
    let b = bench("parar");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));

    let run = b.delegate("powershell", &["PERGUNTA: parar aqui"]);
    let p = id(&b.added(1)[0]);
    let (code, out) = finish(run);
    assert_eq!(code, 3, "{out}");
    b.states(&[(p.as_str(), "awaiting-orchestrator")]);

    b.core.stop_executor(&p);
    b.state(&p, "stopped");
    let (code, out) = finish(b.delegate("powershell", &["--aguardar"]));
    assert_eq!(code, 1, "{out}");
    assert!(
        out.starts_with("--- EXECUTOR 1/1 ---\nPERGUNTA: parar aqui\ninterrompido pelo usuário\nexecutor_exit: interrompido (FALHOU)\n--- GIT ---\n")
            && out.ends_with("--- FIM ---\n")
            && !out.contains("AGUARDANDO"),
        "{out}"
    );
    let (code, out) = finish(b.delegate("powershell", &["--responder", p.as_str(), "aceitar"]));
    assert!(code == 4 && out.contains("não tem pergunta pendente"), "{code} {out}");
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}

/// O pedido que sai da pasta `ask` devolve o nó de `awaiting-orchestrator` a `running`, e a rodada segue até o fim.
/// O `--ask` retira o próprio pedido ao expirar; o prazo de 100 s não é esperado aqui: a retirada é simulada na pasta.
#[test]
fn pedido_que_sai_da_pasta_devolve_o_no_a_rodando() {
    let b = bench("expira");
    b.core.start_orchestrator(Choice::new("claude", "", ""), 100, 30).unwrap();
    b.core.set_executor_choice(Choice::new("claude", "", "")).unwrap();
    until("o orquestrador falso desenhar", || b.screen(ORCHESTRATOR).contains("orquestrador falso pronto"));

    let run = b.delegate("powershell", &["PERGUNTA: remover o log"]);
    let p = id(&b.added(1)[0]);
    let (code, out) = finish(run);
    assert_eq!(code, 3, "{out}");
    b.states(&[(p.as_str(), "awaiting-orchestrator")]);

    let ask = b.core.session_dir().unwrap().join("exec").join(&p).join("ask");
    for entry in std::fs::read_dir(&ask).unwrap().flatten() {
        std::fs::remove_file(entry.path()).unwrap();
    }
    b.state(&p, "running");
    // Sem pedido na pasta, o `--ask` do CLI simulado nega com o motivo de encerramento e a rodada termina.
    b.state(&p, "done");
    b.core.shutdown();
    let _ = std::fs::remove_dir_all(&b.root);
}
