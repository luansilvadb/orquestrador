// CLI falso para os testes (tests/lote.rs): faz o papel do claude e do codex sem chamar modelo.
// Como executor, entrega o fim de rodada pelo mesmo caminho dos CLIs reais (o manipulador
// `orquestrador.exe --hook`) e fica aberto, como um CLI interativo. O comportamento vem do texto
// da tarefa: FALHAR sai com 3 sem sinal; DORMIR nunca termina; PERGUNTA pede confirmação de um comando
// ao hook `PermissionRequest` da configuração, como o Claude Code faz, e termina a rodada com a decisão.
// Para scripts/medir-desempenho.mjs, opcionais e desligados sem as variáveis: ORQ_FAKE_DELEGAR=<n> faz o orquestrador delegar n
// tarefas pelo servidor MCP do `--mcp-config`; ORQ_FAKE_SEGUNDOS=<s> faz o executor desenhar uma TUI por s segundos antes do fim de rodada.
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let prompt = args.last().cloned().unwrap_or_default();
    let settings = args.iter().position(|a| a == "--settings").map(|i| std::fs::read_to_string(&args[i + 1]).unwrap());
    let settings: Option<serde_json::Value> = settings.and_then(|s| serde_json::from_str(&s).ok());
    let stop = settings.as_ref().map(|v| v["hooks"]["Stop"][0]["hooks"][0].clone());
    let notify: Option<Vec<String>> = args.iter().find_map(|a| a.strip_prefix("notify=")).map(|v| serde_json::from_str(v).unwrap());

    // Só o orquestrador recebe o servidor MCP do app: `--mcp-config` no Claude, `mcp_servers.` no Codex.
    if args.iter().any(|a| a == "--mcp-config" || a.starts_with("mcp_servers.")) {
        orchestrator();
    }
    if prompt.contains("FALHAR") {
        std::process::exit(3);
    }
    if let Some(secs) = std::env::var("ORQ_FAKE_SEGUNDOS").ok().and_then(|v| v.parse::<u64>().ok()) {
        tui(secs, &prompt);
    }
    let answer = if prompt.contains("PERGUNTA") { permission(settings.as_ref()) } else { format!("feito: {}", prompt.lines().last().unwrap_or("")) };
    if prompt.contains("DORMIR") {
        println!("dormindo");
    } else if let Some(n) = notify {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let send = |input: String, last: &str| {
            let json = serde_json::json!({ "type": "agent-turn-complete", "input-messages": [input], "last-assistant-message": last });
            Command::new(&n[0]).args(&n[1..]).arg(json.to_string()).status().unwrap();
        };
        send(format!("Generate a concise, single-line task title.\n\nUser prompt:\n{prompt}"), "{\"title\":\"x\"}");
        send(prompt.clone(), &answer);
        println!("{answer}");
    } else if let Some(hook) = stop.filter(|h| h["command"].is_string()) {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let hook_args: Vec<String> = serde_json::from_value(hook["args"].clone()).unwrap();
        let mut child = Command::new(hook["command"].as_str().unwrap()).args(hook_args).stdin(Stdio::piped()).spawn().unwrap();
        let json = serde_json::json!({ "hook_event_name": "Stop", "last_assistant_message": answer });
        child.stdin.take().unwrap().write_all(json.to_string().as_bytes()).unwrap();
        child.wait().unwrap();
        println!("{answer}");
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// O orquestrador: ecoa o que for digitado; "sair" termina com código 7.
fn orchestrator() -> ! {
    println!("orquestrador falso pronto");
    if let Some(n) = std::env::var("ORQ_FAKE_DELEGAR").ok().and_then(|v| v.parse::<usize>().ok()) {
        let args: Vec<String> = std::env::args().collect();
        let config = args.iter().position(|a| a == "--mcp-config").map(|i| args[i + 1].clone()).expect("--mcp-config");
        std::thread::spawn(move || delegate(&config, n));
    }
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap_or_default();
        if line.trim() == "sair" {
            std::process::exit(7);
        }
        println!("eco:{}", line.trim());
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// O que um CLI real faz num pedido de confirmação: entrega o pedido ao hook `PermissionRequest` da
/// configuração (o mesmo comando e os mesmos argumentos), espera a decisão na saída dele e devolve o texto
/// final da rodada: `aceito`, ou `recusado:` com a mensagem da recusa.
fn permission(settings: Option<&serde_json::Value>) -> String {
    let Some(hook) = settings.map(|v| v["hooks"]["PermissionRequest"][0]["hooks"][0].clone()).filter(|h| h["command"].is_string()) else {
        return "sem hook de confirmação".into();
    };
    let hook_args: Vec<String> = serde_json::from_value(hook["args"].clone()).unwrap();
    let call = serde_json::json!({
        "session_id": "falso",
        "permission_mode": "bypassPermissions",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": { "command": "rm /d/arquivo.log", "description": "Remove o log" },
        "permission_suggestions": [],
    });
    let mut child = Command::new(hook["command"].as_str().unwrap()).args(hook_args).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(call.to_string().as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let decision = &reply["hookSpecificOutput"]["decision"];
    match decision["behavior"].as_str() {
        Some("allow") => "aceito".into(),
        Some("deny") => format!("recusado: {}", decision["message"].as_str().unwrap_or("")),
        other => format!("decisão inesperada: {other:?}"),
    }
}

/// O que o orquestrador faz pelo servidor MCP: sobe o comando do `mcp.json`, inicia a conversa e delega `n` tarefas num lote,
/// em JSON-RPC por linha como `tests/lote.rs`. A resposta só chega quando o lote acaba; o servidor fica vivo até lá.
fn delegate(config: &str, n: usize) {
    let mcp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(config).unwrap()).unwrap();
    let server = &mcp["mcpServers"]["orquestrador"];
    let args: Vec<String> = serde_json::from_value(server["args"].clone()).unwrap();
    let mut child = Command::new(server["command"].as_str().unwrap()).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let mut send = |msg: serde_json::Value| {
        writeln!(input, "{msg}").unwrap();
        input.flush().unwrap();
    };
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "fake_cli", "version": "0" } } }));
    let _ = lines.next();
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let tasks: Vec<String> = (1..=n).map(|i| format!("Tarefa de medição {i}")).collect();
    send(serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "delegar", "arguments": { "tarefas": tasks } } }));
    println!("delegou {n} tarefas");
    for line in lines.map_while(Result::ok) {
        if line.contains("\"id\":2") {
            println!("lote entregue");
        }
    }
    let _ = child.wait();
}

/// Uma TUI como a de um CLI de IA: a linha de status (spinner, cronômetro, contador) e o rodapé redesenhados a cada ~100 ms com
/// movimentos de cursor e cores, e blocos de texto acima deles algumas vezes por segundo. Conteúdo determinístico.
fn tui(secs: u64, prompt: &str) {
    const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    const WORDS: [&str; 16] = ["lendo", "arquivo", "função", "teste", "módulo", "erro", "tipo", "valor", "src/app.ts", "config", "retorno", "chamada", "índice", "lista", "estado", "fila"];
    let start = std::time::Instant::now();
    let mut seed: u32 = prompt.bytes().fold(12345u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let mut rnd = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed
    };
    let mut out = std::io::stdout().lock();
    // Duas linhas de rodapé (status e dica) ficam no fim; tudo é escrito de uma vez por quadro, como uma TUI que monta o quadro antes.
    let _ = write!(out, "\x1b[?25l\r\n\r\n");
    let mut frame = 0u64;
    let mut tokens = 0u64;
    while start.elapsed().as_secs() < secs {
        let mut buf = String::new();
        buf.push_str("\x1b[2A\r");
        if frame % 3 == 0 {
            let lines = 2 + rnd() % 5;
            for _ in 0..lines {
                buf.push_str("\x1b[2K\x1b[38;5;245m● \x1b[0m");
                let mut len = 0;
                while len < 70 {
                    let w = WORDS[(rnd() % WORDS.len() as u32) as usize];
                    if rnd() % 5 == 0 {
                        buf.push_str(&format!("\x1b[1;36m{w}\x1b[0m "));
                    } else {
                        buf.push_str(w);
                        buf.push(' ');
                    }
                    len += w.chars().count() + 1;
                }
                buf.push_str("\r\n");
            }
            tokens += 40 + (rnd() % 200) as u64;
        }
        let t = start.elapsed().as_secs();
        buf.push_str(&format!(
            "\x1b[2K\x1b[38;5;208m{}\x1b[0m \x1b[1mTrabalhando…\x1b[0m \x1b[2m({}m {:02}s · ↓ {:.1}k tokens · esc para interromper)\x1b[0m\r\n",
            SPIN[(frame % 10) as usize], t / 60, t % 60, tokens as f64 / 1000.0
        ));
        buf.push_str("\x1b[2K\x1b[2m  ⏵⏵ bypass permissions on (shift+tab para alternar)\x1b[0m\r\n");
        let _ = out.write_all(buf.as_bytes());
        let _ = out.flush();
        frame += 1;
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = write!(out, "\x1b[?25h");
    let _ = out.flush();
}
