//! As linhas de comando dos dois CLIs nos dois papéis.
use crate::config::Choice;
use crate::hook::BARRED;
use crate::roles::EXECUTOR;
use crate::session::Session;
use std::path::Path;

/// Os executáveis: os dois CLIs, o próprio app (que serve de manipulador dos hooks) e o programa de
/// delegação, que vai junto com o app.
#[derive(Clone, Debug)]
pub struct Exes {
    pub claude: String,
    pub codex: String,
    pub app: String,
    pub delegate: String,
}

impl Exes {
    pub fn real() -> Exes {
        let me = std::env::current_exe().unwrap_or_default();
        Exes {
            claude: "claude".into(),
            codex: "codex".into(),
            app: me.to_string_lossy().to_string(),
            delegate: me.with_file_name(crate::delegate::PROGRAM).to_string_lossy().to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spawn {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
    /// Pastas que vão à frente do PATH de base do processo. O orquestrador leva a pasta da sessão, onde
    /// fica o comando de delegação dele.
    pub path_first: Vec<String>,
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// Texto como valor TOML de `-c`: string básica, com as mesmas fugas do JSON.
pub(crate) fn toml(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// Medido no Codex 0.162: sem isto o CLI interativo abre "Trust this folder?" e fica parado. A
/// forma `projects.'caminho'.trust_level=...` não é aceita por `-c`; a tabela inteira é, vale só
/// para este processo e nada é gravado no config.toml do usuário.
fn codex_trust(dir: &str) -> Vec<String> {
    vec!["-c".into(), format!("projects={{{}={{trust_level=\"trusted\"}}}}", toml(dir))]
}

/// O orquestrador: lê o projeto, delega, usa as skills globais e o git (stage, commit e push quando o usuário pede).
pub fn orchestrator(exes: &Exes, choice: &Choice, session: &Session, project: &str) -> Spawn {
    let dir = session.dir.to_string_lossy().to_string();
    let (program, mut args) = if choice.tool == "codex" {
        // Sandbox só de leitura: o Codex não escreve em lugar nenhum, nem nesta pasta nem na de
        // regras, a não ser pelo que a regra da sessão libera. Ninguém aprova nada: o que a regra não
        // libera é recusado pela política; o que ela libera (delegação, git, skills) roda fora do sandbox
        // (medido no Codex 0.162.1).
        // --no-daemon: o agente roda neste processo, então matar o processo mata o agente.
        let mut args = strs(&["--no-daemon", "-C", &dir, "--sandbox", "read-only", "--ask-for-approval", "never"]);
        args.extend(codex_trust(&dir));
        (exes.codex.clone(), args)
    } else {
        // Sem sandbox, as ferramentas são a parede: --restricted deixa só as nomeadas, ignora os
        // settings do usuário e confina Read ao projeto e às pastas globais; dontAsk nega todo
        // comando que não seja o de delegação, o git, um programa de skill ou o que o Claude Code toma
        // por somente leitura. O modo restrito descarta as skills do usuário, então .agents volta
        // como plugin (medido: as skills carregam como ".agents:nome").
        let mut args = strs(&["--restricted", "--strict-mcp-config", "--tools", "Read,Glob,Grep,PowerShell,Skill", "--settings"]);
        args.push(session.dir.join("claude-settings.json").to_string_lossy().to_string());
        args.extend(strs(&["--permission-mode", "dontAsk", "--append-system-prompt-file"]));
        args.push(session.dir.join("AGENTS.md").to_string_lossy().to_string());
        if let Some(first) = session.global_dirs.first() {
            args.extend(["--plugin-dir".to_string(), first.to_string_lossy().to_string(), "--add-dir".to_string()]);
            args.extend(session.global_dirs.iter().map(|d| d.to_string_lossy().to_string()));
        }
        (exes.claude.clone(), args)
    };
    args.extend(choice.cli_args());
    Spawn { program, args, cwd: project.to_string(), env: vec![], path_first: vec![dir] }
}

/// Os PowerShells que o shell do usuário tenta, nesta ordem: o primeiro que iniciar vale.
pub const SHELLS: [&str; 2] = ["pwsh.exe", "powershell.exe"];

/// O shell do usuário: um PowerShell interativo na pasta do projeto, sem env nem pasta à frente do PATH.
pub fn shell(program: &str, project: &str) -> Spawn {
    Spawn { program: program.to_string(), args: strs(&["-NoLogo"]), cwd: project.to_string(), env: vec![], path_first: vec![] }
}

/// O prompt inicial de um executor: o texto de `roles::EXECUTOR` seguido da tarefa.
pub fn executor_prompt(task: &str) -> String {
    format!("{}\n{}", EXECUTOR.trim_end(), task.trim())
}

/// O executor: o CLI interativo com permissão total menos o barrado, sem diálogo de início e com
/// o sinal de fim de rodada entregue em `node_dir\hook`. Devolve também o conteúdo do arquivo de
/// settings que o Claude lê (`node_dir\settings.json`), quando há.
///
/// Medido no Claude Code 2.1.296: o diálogo de confiança na pasta aparece mesmo em
/// bypassPermissions e só some com CLAUDE_CODE_SANDBOXED (não há flag nem chave de settings); o
/// aviso do modo de bypass some com skipDangerousModePermissionPrompt; as regras de negação valem
/// em bypassPermissions; o hook Stop traz `last_assistant_message`. Hooks na forma `args` rodam
/// sem shell, então caminho com espaço passa.
/// Medido no Codex 0.162: regras (`prefix_rule`) são globais, mas um hook PreToolUse dado por `-c`
/// vale só para o processo e nega o comando; o comando do hook roda no PowerShell.
pub fn executor(exes: &Exes, choice: &Choice, project: &str, node_dir: &Path, prompt: &str) -> (Spawn, Option<String>) {
    let hook_dir = node_dir.join("hook").to_string_lossy().to_string();
    let ask_dir = node_dir.join("ask").to_string_lossy().to_string();
    let mut spawn = Spawn { program: String::new(), args: vec![], cwd: project.to_string(), env: vec![], path_first: vec![] };
    let settings = if choice.tool == "codex" {
        spawn.program = exes.codex.clone();
        spawn.args = strs(&["--no-daemon", "-C", project, "--dangerously-bypass-approvals-and-sandbox", "--dangerously-bypass-hook-trust"]);
        spawn.args.extend(codex_trust(project));
        let notify = [exes.app.as_str(), "--hook", &hook_dir].map(toml).join(",");
        let guard = toml(&format!("& '{}' --guard", exes.app.replace('\'', "''")));
        spawn.args.extend(["-c".to_string(), format!("notify=[{notify}]")]);
        spawn.args.extend(["-c".to_string(), format!("hooks.PreToolUse=[{{hooks=[{{type=\"command\",command={guard}}}]}}]")]);
        // Sem hook de confirmação: medido no 0.162.1, com o bypass nenhuma confirmação chega ao CLI e o evento
        // `PermissionRequest` não dispara.
        None
    } else {
        spawn.program = exes.claude.clone();
        spawn.args = strs(&["--permission-mode", "bypassPermissions", "--settings", &node_dir.join("settings.json").to_string_lossy()]);
        spawn.env.push(("CLAUDE_CODE_SANDBOXED".into(), "1".into()));
        let deny: Vec<String> = BARRED.iter().flat_map(|b| [format!("Bash({b}*)"), format!("PowerShell({b}*)")]).collect();
        let run = |args: &[&str]| serde_json::json!({ "hooks": [{ "type": "command", "command": exes.app, "args": args }] });
        let mut guard = run(&["--guard"]);
        guard["matcher"] = "Bash|PowerShell".into();
        // O pedido de confirmação espera a resposta do orquestrador. Quem limita essa espera é o `--ask`, que
        // desiste antes de o Claude Code negar sozinho (`ASK_DEADLINE`, em hook.rs); o prazo do hook só vai a
        // um dia para não cortar antes disso.
        let mut ask = run(&["--ask", &ask_dir]);
        ask["hooks"][0]["timeout"] = 86400.into();
        Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "skipDangerousModePermissionPrompt": true,
                "permissions": { "deny": deny },
                "hooks": { "Stop": [run(&["--hook", &hook_dir])], "PreToolUse": [guard], "PermissionRequest": [ask] },
            }))
            .unwrap(),
        )
    };
    spawn.args.extend(choice.cli_args());
    spawn.args.push(prompt.to_string());
    (spawn, settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::session::tests::{delegate_of, discard, places};

    fn exes() -> Exes {
        Exes {
            claude: "claude".into(),
            codex: "codex".into(),
            app: r"C:\Program Files\Orq\o'rq.exe".into(),
            delegate: r"C:\Program Files\Orq\orquestrador-delegar.exe".into(),
        }
    }

    fn session(tag: &str, tool: &str) -> (Session, crate::session::Places) {
        let cfg = Config { orchestrator: Choice::new(tool, "", ""), ..Config::default() };
        let p = places(tag);
        let (s, r) = Session::create(&p, &delegate_of(&p), r"C:\proj", &cfg);
        r.unwrap();
        (s, p)
    }

    #[test]
    fn orquestrador_codex() {
        let (s, p) = session("cli-codex", "codex");
        let dir = s.dir.to_string_lossy().to_string();
        let got = orchestrator(&exes(), &Choice::new("codex", "gpt-5.5", "high"), &s, r"C:\proj");
        assert_eq!(got.program, "codex");
        assert_eq!(got.cwd, r"C:\proj");
        assert_eq!(got.path_first, [dir.clone()], "o comando de delegação da sessão é achado primeiro no PATH");
        let trust = format!("projects={{{}={{trust_level=\"trusted\"}}}}", serde_json::to_string(&dir).unwrap());
        assert_eq!(
            got.args,
            ["--no-daemon", "-C", &dir, "--sandbox", "read-only", "--ask-for-approval", "never", "-c", &trust, "-m", "gpt-5.5", "-c", "model_reasoning_effort=high"]
        );
        s.remove();
        discard(&p);
    }

    #[test]
    fn orquestrador_claude() {
        let (s, p) = session("cli-claude", "claude");
        let got = orchestrator(&exes(), &Choice::new("claude", "opus", ""), &s, r"C:\proj");
        let agents = s.global_dirs[0].to_string_lossy().to_string();
        assert_eq!(got.program, "claude");
        assert_eq!(got.path_first, [s.dir.to_string_lossy().to_string()]);
        assert_eq!(
            got.args,
            [
                "--restricted", "--strict-mcp-config", "--tools", "Read,Glob,Grep,PowerShell,Skill",
                "--settings", &s.dir.join("claude-settings.json").to_string_lossy(),
                "--permission-mode", "dontAsk",
                "--append-system-prompt-file", &s.dir.join("AGENTS.md").to_string_lossy(),
                "--plugin-dir", &agents, "--add-dir", &agents,
                "--model", "opus",
            ]
        );
        s.remove();
        discard(&p);
    }

    #[test]
    fn executor_claude() {
        let node = Path::new(r"C:\tmp\s\exec\exec-1");
        let (got, settings) = executor(&exes(), &Choice::new("claude", "sonnet", "low"), r"C:\proj", node, "PROMPT\nlinha 2");
        assert_eq!(got.program, "claude");
        assert_eq!(got.cwd, r"C:\proj");
        assert_eq!(
            got.args,
            ["--permission-mode", "bypassPermissions", "--settings", r"C:\tmp\s\exec\exec-1\settings.json", "--model", "sonnet", "--effort", "low", "PROMPT\nlinha 2"]
        );
        assert_eq!(got.env, [("CLAUDE_CODE_SANDBOXED".to_string(), "1".to_string())]);
        assert!(got.path_first.is_empty(), "o executor não recebe o comando de delegação");
        let v: serde_json::Value = serde_json::from_str(&settings.unwrap()).unwrap();
        assert_eq!(v["skipDangerousModePermissionPrompt"], true);
        let deny: Vec<&str> = v["permissions"]["deny"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
        assert_eq!(deny.len(), 10);
        for rule in ["Bash(git push*)", "PowerShell(git push*)", "Bash(git reset --hard*)", "PowerShell(git clean*)", "Bash(npm publish*)", "PowerShell(pnpm publish*)"] {
            assert!(deny.contains(&rule), "{rule}");
        }
        let stop = &v["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(stop["command"], r"C:\Program Files\Orq\o'rq.exe");
        assert_eq!(stop["args"], serde_json::json!(["--hook", r"C:\tmp\s\exec\exec-1\hook"]));
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "Bash|PowerShell");
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["args"], serde_json::json!(["--guard"]));
        let ask = &v["hooks"]["PermissionRequest"][0]["hooks"][0];
        assert_eq!(ask["command"], r"C:\Program Files\Orq\o'rq.exe");
        assert_eq!(ask["args"], serde_json::json!(["--ask", r"C:\tmp\s\exec\exec-1\ask"]));
        assert_eq!(ask["timeout"], 86400);
        assert!(v["hooks"]["PermissionRequest"][0].get("matcher").is_none(), "o pedido de qualquer ferramenta passa pelo --ask");
    }

    #[test]
    fn executor_codex() {
        let node = Path::new(r"C:\tmp\s\exec\exec-2");
        let (got, settings) = executor(&exes(), &Choice::new("codex", "", "xhigh"), r"C:\proj", node, "PROMPT");
        assert!(settings.is_none());
        assert!(got.env.is_empty());
        assert_eq!(got.program, "codex");
        assert_eq!(
            got.args,
            [
                "--no-daemon", "-C", r"C:\proj", "--dangerously-bypass-approvals-and-sandbox", "--dangerously-bypass-hook-trust",
                "-c", r#"projects={"C:\\proj"={trust_level="trusted"}}"#,
                "-c", r#"notify=["C:\\Program Files\\Orq\\o'rq.exe","--hook","C:\\tmp\\s\\exec\\exec-2\\hook"]"#,
                "-c", r#"hooks.PreToolUse=[{hooks=[{type="command",command="& 'C:\\Program Files\\Orq\\o''rq.exe' --guard"}]}]"#,
                "-c", "model_reasoning_effort=xhigh",
                "PROMPT",
            ]
        );
        assert!(!got.args.iter().any(|a| a.contains("PermissionRequest")), "o Codex não recebe o hook de confirmação");
    }

    #[test]
    fn programa_de_delegacao_fica_ao_lado_do_app() {
        let real = Exes::real();
        let me = std::env::current_exe().unwrap();
        assert_eq!(Path::new(&real.app), me);
        assert_eq!(Path::new(&real.delegate), me.with_file_name(crate::delegate::PROGRAM));
    }

    #[test]
    fn prompt_do_executor_seguido_da_tarefa() {
        let p = executor_prompt("  Faça X.\nPronto quando: Y.\n");
        assert!(p.starts_with(EXECUTOR.trim_end()));
        assert!(p.ends_with("\nFaça X.\nPronto quando: Y."));
    }
}
