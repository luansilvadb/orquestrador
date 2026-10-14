// CLI falso para os testes (tests/lote.rs): faz o papel do claude e do codex sem chamar modelo.
// Como executor, entrega o fim de rodada pelo mesmo caminho dos CLIs reais (o manipulador
// `orquestrador.exe --hook`) e fica aberto, como um CLI interativo. O comportamento vem do texto
// da tarefa: FALHAR sai com 3 sem sinal; DORMIR nunca termina; PERGUNTA pede confirmação de um comando
// ao hook `PermissionRequest` da configuração, como o Claude Code faz, e termina a rodada com a decisão.
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let prompt = args.last().cloned().unwrap_or_default();
    let settings = args.iter().position(|a| a == "--settings").map(|i| std::fs::read_to_string(&args[i + 1]).unwrap());
    let settings: Option<serde_json::Value> = settings.and_then(|s| serde_json::from_str(&s).ok());
    let stop = settings.as_ref().map(|v| v["hooks"]["Stop"][0]["hooks"][0].clone());
    let notify: Option<Vec<String>> = args.iter().find_map(|a| a.strip_prefix("notify=")).map(|v| serde_json::from_str(v).unwrap());

    if prompt.contains("FALHAR") {
        std::process::exit(3);
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
    } else {
        // Orquestrador: ecoa o que for digitado; "sair" termina com código 7.
        println!("orquestrador falso pronto");
        for line in std::io::stdin().lock().lines() {
            let line = line.unwrap_or_default();
            if line.trim() == "sair" {
                std::process::exit(7);
            }
            println!("eco:{}", line.trim());
        }
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
