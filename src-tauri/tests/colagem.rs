//! Colagem de imagem com o CLI real num PTY: o gesto de colar tem que fazer o CLI anexar a imagem do
//! clipboard do Windows, sem enviar mensagem (nenhum modelo é chamado). Codex: o app manda `\x16` (Ctrl+V).
//! Claude Code no Windows: a imagem sai com Alt+V (`ESC v`), então é esse o gesto medido aqui.
//! Antes, ponha uma imagem no clipboard (o driver faz isso). Uso, num PowerShell:
//!   $env:ORQ_PASTE = 'codex'   # ou 'claude'
//!   cargo test --test colagem -- --ignored --nocapture
use orquestrador_lib::cli::Spawn;
use orquestrador_lib::pty::{Buffer, Shared, Term};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn screen(buffer: &Shared) -> String {
    String::from_utf8_lossy(&buffer.lock().unwrap().data).to_string()
}

fn wait_for(buffer: &Shared, marker: &str, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if screen(buffer).contains(marker) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

#[test]
#[ignore]
fn colar_imagem_no_cli_real() {
    let cli = std::env::var("ORQ_PASTE").unwrap_or("codex".into());
    let dir = std::env::temp_dir().join(format!("orq-paste-{}-{cli}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir_s = dir.to_string_lossy().to_string();
    let args: Vec<String> = if cli == "codex" {
        // Mesmos argumentos que o orquestrador usa para o Codex (cli.rs), sem sandbox de aprovação nem regras.
        let trust = format!("projects={{{}={{trust_level=\"trusted\"}}}}", serde_json::to_string(&dir_s).unwrap());
        ["--no-daemon", "-C", &dir_s, "--sandbox", "read-only", "--ask-for-approval", "never", "-c", &trust]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };
    let spawn = Spawn { program: cli.clone(), args, cwd: dir_s.clone(), env: Vec::new(), path_first: Vec::new() };
    let buffer: Shared = Arc::new(Mutex::new(Buffer::default()));
    let mut term = Term::spawn(&spawn, (120, 40), buffer.clone(), |_, _| {}, |_| {}).expect("não iniciou o CLI");

    // Diálogo de confiança (Claude pergunta; o Codex já recebe a confiança por -c): Enter aceita a opção padrão.
    std::thread::sleep(Duration::from_secs(8));
    if screen(&buffer).to_lowercase().contains("trust") {
        println!("diálogo de confiança: respondido com Enter");
        term.write(b"\r");
    }
    std::thread::sleep(Duration::from_secs(10));
    let before = screen(&buffer);
    println!("antes do Ctrl+V: {} bytes, marcador já presente: {}", before.len(), before.contains("Image #1"));

    // O gesto de colar imagem de cada CLI no Windows: Codex, Ctrl+V (o app manda `\x16`); Claude, Alt+V.
    let (gesto, nome) = if cli == "codex" { (&b"\x16"[..], "\\x16") } else { (&b"\x1bv"[..], "Alt+V") };
    term.write(gesto);
    let attached = wait_for(&buffer, "Image #1", 15);
    std::thread::sleep(Duration::from_secs(2));
    let after = screen(&buffer);
    let out = std::env::temp_dir().join(format!("orq-paste-{cli}-screen.txt"));
    std::fs::write(&out, &after).unwrap();
    println!("tela salva em {}", out.display());
    println!("imagem anexada (marcador 'Image #1' depois de {nome}): {attached}");
    let model_work = ["Working", "Thinking", "Esperando"].iter().any(|m| after.contains(m));
    println!("sinal de trabalho do modelo na tela: {model_work}");

    drop(term);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(attached, "o CLI não anexou a imagem com {nome}");
    assert!(!model_work, "a tela mostra trabalho do modelo");
}
