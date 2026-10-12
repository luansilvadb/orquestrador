//! Medição com os CLIs reais (chama modelo, por isso `#[ignore]`): o orquestrador delega um lote
//! de duas tarefas e o resultado volta sozinho. Uso, num repositório Git descartável:
//!   $env:ORQ_REAL_REPO = 'C:\caminho\repo'; $env:ORQ_REAL = 'codex,claude'   # orquestrador,executor
//!   cargo test --test real -- --ignored --nocapture
use base64::Engine;
use orquestrador_lib::cli::Exes;
use orquestrador_lib::config::Choice;
use orquestrador_lib::core::{Core, Emit, ORCHESTRATOR};
use orquestrador_lib::session::Places;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn screen(core: &Core, id: &str) -> String {
    let raw = base64::engine::general_purpose::STANDARD.decode(core.term_snapshot(id).data).unwrap();
    String::from_utf8_lossy(&raw).to_string()
}

#[test]
#[ignore]
fn lote_real() {
    let repo = std::env::var("ORQ_REAL_REPO").expect("defina ORQ_REAL_REPO");
    let pair = std::env::var("ORQ_REAL").unwrap_or("codex,claude".into());
    let (orchestrator, executor) = pair.split_once(',').unwrap();
    let cheap = |tool: &str| if tool == "claude" { Choice::new("claude", "haiku", "") } else { Choice::new("codex", "", "low") };
    let exes = Exes { app: env!("CARGO_BIN_EXE_orquestrador").to_string(), ..Exes::real() };
    let (tx, events) = channel();
    let emit: Emit = Arc::new(move |event, payload| drop(tx.send((event.to_string(), payload))));
    let core = Core::new(exes, Places::real(), std::env::temp_dir().join("orq-real-config.json"), emit);
    core.set_project(std::path::Path::new(&repo)).unwrap();
    core.start_orchestrator(cheap(orchestrator), 120, 40).unwrap();
    core.set_executor_choice(cheap(executor)).unwrap();
    let session = core.session_dir().unwrap();
    println!("sessão: {}", session.display());

    // O orquestrador é o único terminal em que alguém digita: o diálogo de confiança na pasta,
    // quando aparece, é respondido aqui como o usuário responderia.
    std::thread::sleep(Duration::from_secs(8));
    if screen(&core, ORCHESTRATOR).contains("trust") {
        println!("diálogo de confiança no orquestrador: respondido");
        core.term_write(ORCHESTRATOR, "\x1b[B");
        std::thread::sleep(Duration::from_millis(300));
        core.term_write(ORCHESTRATOR, "\r");
        std::thread::sleep(Duration::from_secs(6));
    }
    core.term_write(ORCHESTRATOR, "Sem perguntas e sem skills: delegue agora, num unico comando, um lote com duas tarefas: (1) Crie o arquivo e2e-a.txt contendo a palavra maçã. (2) Rode exatamente git push --dry-run e relate o que aconteceu, sem tentar alternativas. Quando o comando voltar, responda so RECEBIDO e o executor_exit de cada uma.");
    std::thread::sleep(Duration::from_millis(800));
    core.term_write(ORCHESTRATOR, "\r");

    let deadline = Instant::now() + Duration::from_secs(420);
    let (mut done, mut total, mut ids) = (0, 0, Vec::new());
    while done < 2 {
        let Ok((event, payload)) = events.recv_timeout(deadline.saturating_duration_since(Instant::now())) else { break };
        match event.as_str() {
            "term-data" => continue,
            "node-added" => {
                total += 1;
                ids.push(payload["id"].as_str().unwrap().to_string());
            }
            "orchestrator-exit" => panic!("o orquestrador saiu: {payload}"),
            "node-state" if ["done", "failed", "stopped"].contains(&payload["state"].as_str().unwrap()) => done += 1,
            _ => {}
        }
        println!("{event}: {payload}");
    }
    // Um instante para o orquestrador receber a saída e responder.
    std::thread::sleep(Duration::from_secs(25));
    std::fs::write(std::env::temp_dir().join("orq-real-orchestrator.log"), screen(&core, ORCHESTRATOR)).unwrap();
    for id in &ids {
        std::fs::write(std::env::temp_dir().join(format!("orq-real-{id}.log")), screen(&core, id)).unwrap();
    }
    for entry in std::fs::read_dir(session.join("results")).unwrap().flatten() {
        println!("== {}\n{}", entry.path().display(), std::fs::read_to_string(entry.path()).unwrap_or_default());
    }
    core.shutdown();
    assert_eq!((total, done), (2, 2), "o lote não fechou");
    assert!(!session.exists());
}
