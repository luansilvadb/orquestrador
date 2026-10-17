//! Colagem de imagem com o CLI real num PTY: o gesto de colar tem que fazer o CLI anexar a imagem do
//! clipboard do Windows, sem enviar mensagem (nenhum modelo é chamado). Codex: o app manda `\x16` (Ctrl+V).
//! Claude Code no Windows: a imagem sai com Alt+V (`ESC v`), então é esse o gesto medido aqui.
//! Antes, ponha uma imagem no clipboard (o driver faz isso). Uso, num PowerShell:
//!   $env:ORQ_PASTE = 'codex'   # ou 'claude'
//!   cargo test --test colagem -- --ignored --nocapture
use orquestrador_lib::cli::Spawn;
use orquestrador_lib::pty::{Buffer, Shared, Term};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A saída crua. O Claude desenha alguns espaços como "cursor uma coluna à frente": viram espaço, para o marcador inteiro aparecer.
fn screen(buffer: &Shared) -> String {
    String::from_utf8_lossy(&buffer.lock().unwrap().data).replace("\x1b[1C", " ")
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

/// Sobe o CLI real na pasta dada e espera o diálogo de confiança, respondendo com Enter se ele aparecer.
fn abrir_cli(cli: &str, dir: &Path, size: (u16, u16)) -> (Term, Shared) {
    let dir_s = dir.to_string_lossy().to_string();
    let args: Vec<String> = if cli == "codex" {
        // Os argumentos que o orquestrador usa para o Codex (cli.rs), sem o servidor MCP: sem sandbox e sem
        // aprovação, como o executor, e a pasta já confiada.
        let trust = format!("projects={{{}={{trust_level=\"trusted\"}}}}", serde_json::to_string(&dir_s).unwrap());
        ["--no-daemon", "-C", &dir_s, "--dangerously-bypass-approvals-and-sandbox", "-c", &trust]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };
    // O diálogo de confiança do Claude pode demorar mais que a espera abaixo; com esta variável ele não aparece (cli.rs).
    let env = if cli == "claude" { vec![("CLAUDE_CODE_SANDBOXED".to_string(), "1".to_string())] } else { Vec::new() };
    let spawn = Spawn { program: cli.to_string(), args, cwd: dir_s, env };
    let buffer: Shared = Arc::new(Mutex::new(Buffer::default()));
    let mut term = Term::spawn(&spawn, size, buffer.clone(), |_, _| {}, |_| {}).expect("não iniciou o CLI");

    // Diálogo de confiança (Claude pergunta; o Codex já recebe a confiança por -c): Enter aceita a opção padrão.
    std::thread::sleep(Duration::from_secs(8));
    if screen(&buffer).to_lowercase().contains("trust") {
        println!("diálogo de confiança: respondido com Enter");
        term.write(b"\r");
    }
    (term, buffer)
}

#[test]
#[ignore]
fn colar_imagem_no_cli_real() {
    let cli = std::env::var("ORQ_PASTE").unwrap_or("codex".into());
    let dir = std::env::temp_dir().join(format!("orq-paste-{}-{cli}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (mut term, buffer) = abrir_cli(&cli, &dir, (120, 40));
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

/// Arquivo copiado no Explorer: o app cola o caminho (o xterm o embrulha em paste entre colchetes, que os
/// dois CLIs ligam). Mede o que o CLI faz com ele: caminho de imagem vira anexo, com e sem aspas, e o de
/// outro arquivo fica no texto. Uso: o mesmo `ORQ_PASTE`, com `colar_caminho` no filtro do teste.
#[test]
#[ignore]
fn colar_caminho_de_arquivo_no_cli_real() {
    use base64::Engine;
    let cli = std::env::var("ORQ_PASTE").unwrap_or("codex".into());
    let dir = std::env::temp_dir().join(format!("orq-paste-path-{}-{cli}", std::process::id()));
    let spaced = dir.join("pasta com espaço");
    std::fs::create_dir_all(&spaced).unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==")
        .unwrap();
    let (plain, quoted, note) = (dir.join("um.png"), spaced.join("dois.png"), dir.join("nota.txt"));
    std::fs::write(&plain, &png).unwrap();
    std::fs::write(&quoted, &png).unwrap();
    std::fs::write(&note, "x").unwrap();
    let (mut term, buffer) = abrir_cli(&cli, &dir, (160, 40));
    std::thread::sleep(Duration::from_secs(10));
    println!("paste entre colchetes ligado pelo CLI: {}", screen(&buffer).contains("\x1b[?2004h"));

    // Como `src/paste.ts` escreve: aspas só quando o caminho tem espaço. O Claude recebe um paste só, com um caminho (e um espaço) por
    // linha; o Codex, um paste por caminho (com um espaço no fim), separados por F12, que fecha o paste
    // anterior sem intervalo de tempo. A nota vai no meio: no Codex ela fica entre as duas imagens, na ordem.
    let paths = [plain.to_string_lossy().to_string(), note.to_string_lossy().to_string(), format!("\"{}\"", quoted.to_string_lossy())];
    let bracketed = |text: &str| format!("\x1b[200~{text}\x1b[201~");
    if cli == "codex" {
        let pastes: Vec<String> = paths.iter().map(|p| bracketed(&format!("{p} "))).collect();
        term.write(pastes.join("\x1b[24~").as_bytes());
    } else {
        term.write(bracketed(&paths.map(|p| format!("{p} ")).join("\n")).as_bytes());
    }
    let first = wait_for(&buffer, "Image #1", 15);
    let second = wait_for(&buffer, "Image #2", 15);
    let text = wait_for(&buffer, "nota.txt", 15);
    std::thread::sleep(Duration::from_secs(2));
    let after = screen(&buffer);
    let out = std::env::temp_dir().join(format!("orq-paste-path-{cli}-screen.txt"));
    std::fs::write(&out, &after).unwrap();
    println!("tela salva em {}", out.display());
    println!("imagem sem aspas anexada: {first}; imagem com aspas e espaço anexada: {second}; caminho do .txt no texto: {text}");
    let model_work = ["Working", "Thinking", "Esperando"].iter().any(|m| after.contains(m));

    drop(term);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(first && second && text, "o CLI não tratou os caminhos colados como esperado");
    assert!(!model_work, "a tela mostra trabalho do modelo");
}

/// O CLI real aberto num PowerShell, como num shell avulso do app: o terminal diz que é ele enquanto o processo vive (não
/// pelo que foi digitado) e volta a ser shell comum quando ele sai. Sai pelo comando do próprio CLI, sem mandar mensagem.
/// Uso: o mesmo `ORQ_PASTE`, com `cli_real` no filtro do teste.
#[test]
#[ignore]
fn cli_real_no_shell_abre_e_sai() {
    use orquestrador_lib::cli::shell;
    let cli = std::env::var("ORQ_PASTE").unwrap_or("codex".into());
    let dir = std::env::temp_dir().join(format!("orq-paste-shell-{}-{cli}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir_s = dir.to_string_lossy().to_string();
    let buffer: Shared = Arc::new(Mutex::new(Buffer::default()));
    let mut term = Term::spawn(&shell("pwsh.exe", &dir_s), (160, 40), buffer.clone(), |_, _| {}, |_| {}).expect("não iniciou o PowerShell");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(term.cli(), None, "o PowerShell sozinho não é CLI");
    // O Codex recebe a confiança na pasta por -c; o Claude pula o diálogo com a variável, como em `abrir_cli`.
    let (start, quit) = if cli == "codex" {
        (format!("codex --no-daemon -c \"projects={{'{dir_s}'={{trust_level='trusted'}}}}\"\r"), "/quit")
    } else {
        ("$env:CLAUDE_CODE_SANDBOXED = '1'; claude\r".to_string(), "/exit")
    };
    term.write(start.as_bytes());
    let deadline = Instant::now() + Duration::from_secs(30);
    while term.cli() != Some(cli.as_str()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    let seen = term.cli();
    // O comando de sair, e o Enter à parte: no Codex, a linha digitada de uma vez seria tomada por paste.
    std::thread::sleep(Duration::from_secs(8));
    term.write(quit.as_bytes());
    std::thread::sleep(Duration::from_millis(500));
    term.write(b"\r");
    let deadline = Instant::now() + Duration::from_secs(20);
    while term.cli().is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    let after = term.cli();
    println!("CLI visto com ele aberto: {seen:?}; depois de {quit}: {after:?}");
    drop(term);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(seen, Some(cli.as_str()), "o terminal não viu o {cli} rodando");
    assert_eq!(after, None, "o {cli} saiu e o terminal continuou como CLI");
}
