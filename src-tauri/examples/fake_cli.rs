// CLI falso para os testes (tests/lote.rs): faz o papel do claude e do codex sem chamar modelo.
// Como executor, entrega o fim de rodada pelo mesmo caminho dos CLIs reais (o manipulador
// `orquestrador.exe --hook`) e fica aberto, como um CLI interativo. O comportamento vem do texto
// da tarefa: FALHAR sai com 3 sem sinal; DORMIR nunca termina.
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let prompt = args.last().cloned().unwrap_or_default();
    let settings = args.iter().position(|a| a == "--settings").map(|i| std::fs::read_to_string(&args[i + 1]).unwrap());
    let stop = settings.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).map(|v| v["hooks"]["Stop"][0]["hooks"][0].clone());
    let notify: Option<Vec<String>> = args.iter().find_map(|a| a.strip_prefix("notify=")).map(|v| serde_json::from_str(v).unwrap());
    let answer = format!("feito: {}", prompt.lines().last().unwrap_or(""));

    if prompt.contains("FALHAR") {
        std::process::exit(3);
    }
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
        // Orquestrador: ecoa o que for digitado; "sair" termina com código 7. "which:<nome>" responde em
        // que pasta do PATH deste processo o programa é achado (o índice), para o teste ver o ambiente real.
        println!("orquestrador falso pronto");
        for line in std::io::stdin().lock().lines() {
            let line = line.unwrap_or_default();
            if line.trim() == "sair" {
                std::process::exit(7);
            }
            if let Some(name) = line.trim().strip_prefix("which:") {
                let path = std::env::var("PATH").unwrap_or_default();
                let found = path.split(';').position(|dir| std::path::Path::new(dir).join(name).is_file());
                println!("achou:{}", found.map_or("nenhum".to_string(), |i| i.to_string()));
                continue;
            }
            println!("eco:{}", line.trim());
        }
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
