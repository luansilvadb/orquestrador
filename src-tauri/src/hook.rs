//! Os dois modos sem janela do executável, chamados pelos CLIs dos executores.
use std::io::Read;

/// `orquestrador.exe --hook <pasta> [json]`: o sinal de fim de rodada. O Claude Code entrega o
/// JSON do hook `Stop` por stdin; o Codex, como último argumento do `notify`. Grava o que recebeu,
/// sem interpretar, num arquivo novo da pasta (publicado por renomeação); quem lê é o app.
/// Um arquivo por chamada: medido no Codex 0.162, o `notify` também dispara para a rodada interna
/// que gera o título da conversa, e uma chamada não pode apagar a outra.
pub fn run(args: &[String]) -> i32 {
    let Some(dir) = args.first() else { return 2 };
    let payload = match args.get(1) {
        Some(json) => json.clone().into_bytes(),
        None => {
            let mut buf = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut buf);
            buf
        }
    };
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let name = format!("{dir}\\{nanos:032}-{}", std::process::id());
    let written = std::fs::write(format!("{name}.tmp"), payload).and_then(|_| std::fs::rename(format!("{name}.tmp"), format!("{name}.json")));
    if written.is_err() { 1 } else { 0 }
}

/// O que não tem volta e fica barrado aos executores (item 8 do contrato).
pub const BARRED: [&str; 5] = ["git push", "git reset --hard", "git clean", "npm publish", "pnpm publish"];

/// O comando barrado que aparece em `command`, em qualquer ponto dele (depois de `cd x &&`,
/// dentro de `pwsh -Command "..."`). Compara texto: é uma rede, não uma parede.
pub fn barred(command: &str) -> Option<&'static str> {
    let flat = command.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    BARRED.into_iter().find(|b| flat.contains(b))
}

/// `orquestrador.exe --guard`: hook PreToolUse (Claude Code e Codex usam o mesmo formato). Recebe
/// a chamada de ferramenta por stdin e nega a que traz um comando barrado; o resto passa em silêncio.
pub fn guard() -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let call: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
    let command = match &call["tool_input"]["command"] {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if let Some(found) = barred(&command) {
        println!("{}", serde_json::json!({ "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": format!("`{found}` é barrado nesta sessão; pare e relate a quem delegou"),
        }}));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barrados_em_qualquer_ponto_do_comando() {
        for cmd in ["git push", "git  push origin main", "cd x && git push --force", "pwsh -Command \"git clean -fd\"", "GIT RESET --hard HEAD~1", "npm publish", "pnpm publish --no-git-checks"] {
            assert!(barred(cmd).is_some(), "{cmd}");
        }
        for cmd in ["git status", "git reset --soft HEAD~1", "git commit -m x", "npm run build", "git pull", "pnpm install"] {
            assert!(barred(cmd).is_none(), "{cmd}");
        }
    }

    #[test]
    fn hook_grava_um_arquivo_por_chamada() {
        let dir = std::env::temp_dir().join(format!("orq-hook-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().to_string();
        assert_eq!(run(&[d.clone(), r#"{"a":"ação"}"#.into()]), 0);
        assert_eq!(run(&[d.clone(), r#"{"b":2}"#.into()]), 0);
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        assert_eq!(files.len(), 2);
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), r#"{"a":"ação"}"#);
        assert_eq!(run(&[]), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
