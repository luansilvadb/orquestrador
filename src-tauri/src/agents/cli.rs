//! As linhas de comando dos dois CLIs nos dois papéis, e o único lugar das permissões deles: flags, settings e env.
//! Nenhum agente tem comando barrado pelo app (sem sandbox, lista de ferramentas, `deny` ou hook `PreToolUse`); o
//! que cada papel faz vem só dos textos de `roles.rs`.
use crate::agents::roles::{self, EXECUTOR};
use crate::agents::session::Session;
use crate::config::Choice;
use serde_json::{json, Value};
use std::path::Path;

/// Os executáveis: os dois CLIs e o próprio app, que serve de servidor MCP do orquestrador (`--mcp`) e de
/// manipulador dos hooks.
#[derive(Clone, Debug)]
pub struct Exes {
    pub claude: String,
    pub codex: String,
    pub app: String,
}

impl Exes {
    /// Os binários nativos: o atalho do npm (`codex` sem extensão, `codex.cmd`) não abre num PTY, e o `.cmd` passaria o
    /// prompt pelo `cmd.exe`.
    pub fn real() -> Exes {
        let me = std::env::current_exe().unwrap_or_default();
        Exes { claude: "claude.exe".into(), codex: "codex.exe".into(), app: me.to_string_lossy().to_string() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spawn {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, String)>,
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// Texto como valor TOML de `-c`: string básica, com as mesmas fugas do JSON.
pub(crate) fn toml(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// O Codex nos dois papéis: sem sandbox e sem aprovações, com a pasta confiada. `--no-daemon`: o agente roda neste
/// processo, então matar o processo mata o agente. A confiança, medida no Codex 0.162: sem ela o CLI interativo
/// abre "Trust this folder?" e fica parado; a forma `projects.'caminho'.trust_level=...` não é aceita por `-c`, a
/// tabela inteira é, vale só para este processo e nada é gravado no config.toml do usuário. Medido no Codex 0.163:
/// sem `--no-alt-screen`, o Codex ativa a tela alternativa e a captura do mouse, impedindo a roda de rolar o histórico.
fn codex_base(dir: &str) -> Vec<String> {
    let mut args = strs(&["--no-daemon", "--no-alt-screen", "-C", dir,
        "--dangerously-bypass-approvals-and-sandbox", "--dangerously-bypass-hook-trust"]);
    args.extend(["-c".to_string(), format!("projects={{{}={{trust_level=\"trusted\"}}}}", toml(dir))]);
    args
}

/// O Claude nos dois papéis: todas as ferramentas, sem pedir permissão.
const CLAUDE_BYPASS: [&str; 2] = ["--permission-mode", "bypassPermissions"];

/// O env do Claude nos dois papéis, que tira o diálogo de confiança na pasta (detalhe medido no doc de `executor()`).
fn claude_env() -> Vec<(String, String)> {
    vec![("CLAUDE_CODE_SANDBOXED".into(), "1".into())]
}

/// O que o app em modo `--allow` imprime: a resposta do hook `PermissionRequest`, que aceita qualquer pedido. Medido
/// no Claude Code 2.1.302: mesmo em bypassPermissions, um `rm` na raiz do disco pede confirmação ("cannot be
/// auto-allowed by permission rules") e, sem resposta, é negado; com este `allow` na hora, roda.
pub const ALLOW: &str = r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#;

/// Um hook de settings do Claude que roda o app com `args`. Na forma `args`, roda sem shell, então caminho com
/// espaço passa.
fn app_hook(app: &str, args: &[&str]) -> Value {
    json!({ "hooks": [{ "type": "command", "command": app, "args": args }] })
}

/// O settings do Claude nos dois papéis, passado com `--settings`: `extra` mais `skipDangerousModePermissionPrompt`,
/// que tira o aviso do modo de bypass, e o hook `PermissionRequest` que aceita o que o bypass ainda pergunta.
fn claude_settings(app: &str, mut extra: Value) -> String {
    extra["skipDangerousModePermissionPrompt"] = true.into();
    extra["hooks"]["PermissionRequest"] = json!([app_hook(app, &["--allow"])]);
    serde_json::to_string_pretty(&extra).unwrap()
}

/// O settings do orquestrador Claude. Sem `CLAUDE_CODE_USE_POWERSHELL_TOOL`, o Claude Code 2.1.296 não expõe o
/// PowerShell no Windows com Git Bash (medido com um stub que mostra a lista de ferramentas enviada ao modelo), e o
/// orquestrador ficaria no Git Bash. `MCP_TOOL_TIMEOUT` dá às chamadas ao servidor do orquestrador os 15 minutos
/// da espera de um lote. As duas entram só por este arquivo: o ambiente do usuário e o dos executores não mudam.
pub fn orchestrator_settings(app: &str) -> String {
    claude_settings(app, json!({ "env": { "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1", "MCP_TOOL_TIMEOUT": "900000" } }))
}

/// O servidor MCP do orquestrador pela configuração do Codex, só para este processo: o app em modo `--mcp`, com a
/// pasta da sessão. `approve` libera as três ferramentas: medido no Codex 0.162.1, sem ele toda chamada MCP é
/// negada com `approval_policy` `never`, e os outros valores não liberam; se o bypass de `codex_base` o dispensa
/// não foi medido, então fica. O prazo cobre a espera de 5 minutos de uma chamada, com folga.
fn codex_mcp(app: &str, dir: &str) -> Vec<String> {
    let server = "mcp_servers.orquestrador";
    [
        format!("{server}.command={}", toml(app)),
        format!("{server}.args=[{},{}]", toml("--mcp"), toml(dir)),
        format!("{server}.default_tools_approval_mode={}", toml("approve")),
        format!("{server}.tool_timeout_sec=900"),
    ]
    .into_iter()
    .flat_map(|setting| ["-c".to_string(), setting])
    .collect()
}

/// O orquestrador: o CLI interativo com a mesma permissão dos executores, mais o servidor MCP do próprio app
/// (`delegar` e `aguardar`). Em qualquer CLI, inicia na pasta do projeto, como os executores, e carrega o contexto
/// dele pelo próprio CLI (instruções, skills, settings, MCPs e hooks); a pasta da sessão guarda só o funcionamento
/// interno, e o papel vai pela linha de comando.
pub fn orchestrator(exes: &Exes, choice: &Choice, project: &str, session: &Session) -> Spawn {
    let dir = session.dir.to_string_lossy().to_string();
    let (program, mut args, env) = if choice.tool == "codex" {
        let mut args = codex_base(project);
        args.extend(codex_mcp(&exes.app, &dir));
        // O AGENTS.md lido é o do projeto; o papel entra como instrução de desenvolvedor, chave de topo do config.
        args.extend(["-c".to_string(), format!("developer_instructions={}", toml(&roles::orchestrator(project)))]);
        (exes.codex.clone(), args, vec![])
    } else {
        // Sem --restricted e sem --strict-mcp-config, o Claude mantém os MCPs e os hooks do usuário e do projeto; o
        // --mcp-config soma o servidor do orquestrador. (Medido no 2.1.300: com --restricted, nenhum MCP de usuário ou
        // de projeto conecta, e com --setting-sources também não.)
        let mut args = strs(&["--mcp-config"]);
        args.push(session.dir.join("mcp.json").to_string_lossy().to_string());
        args.push("--settings".to_string());
        args.push(session.dir.join("claude-settings.json").to_string_lossy().to_string());
        args.extend(strs(&CLAUDE_BYPASS));
        args.push("--append-system-prompt-file".to_string());
        args.push(session.dir.join("AGENTS.md").to_string_lossy().to_string());
        (exes.claude.clone(), args, claude_env())
    };
    args.extend(choice.cli_args());
    Spawn { program, args, cwd: project.to_string(), env }
}

/// O shell do usuário: um PowerShell interativo na pasta do projeto, sem env.
pub fn shell(program: &str, project: &str) -> Spawn {
    Spawn { program: program.to_string(), args: strs(&["-NoLogo"]), cwd: project.to_string(), env: vec![] }
}

/// O prompt inicial de um executor: o texto de `roles::EXECUTOR` seguido da tarefa.
pub fn executor_prompt(task: &str) -> String {
    format!("{}\n{}", EXECUTOR.trim_end(), task.trim())
}

/// O executor: o CLI interativo com permissão total, sem diálogo de início e com o sinal de fim de rodada
/// entregue em `node_dir\hook`. Devolve também o conteúdo do arquivo de settings que o Claude lê
/// (`node_dir\settings.json`), quando há.
///
/// Medido no Claude Code 2.1.296: o diálogo de confiança na pasta aparece mesmo em bypassPermissions e só some
/// com CLAUDE_CODE_SANDBOXED, que o app passa aos dois papéis Claude (não documentada, pode mudar entre versões; não
/// há flag nem chave de settings); o hook Stop traz `last_assistant_message`.
pub fn executor(exes: &Exes, choice: &Choice, project: &str, node_dir: &Path, prompt: &str) -> (Spawn, Option<String>) {
    let hook_dir = node_dir.join("hook").to_string_lossy().to_string();
    let mut spawn = Spawn { program: String::new(), args: vec![], cwd: project.to_string(), env: vec![] };
    let settings = if choice.tool == "codex" {
        spawn.program = exes.codex.clone();
        spawn.args = codex_base(project);
        let notify = [exes.app.as_str(), "--hook", &hook_dir].map(toml).join(",");
        spawn.args.extend(["-c".to_string(), format!("notify=[{notify}]")]);
        // Sem hook `PermissionRequest`: medido no 0.162.1, com o bypass um `Remove-Item` fora do projeto roda sem
        // confirmação e o evento não dispara.
        None
    } else {
        spawn.program = exes.claude.clone();
        spawn.args = strs(&CLAUDE_BYPASS);
        spawn.args.extend(["--settings".to_string(), node_dir.join("settings.json").to_string_lossy().to_string()]);
        spawn.env.extend(claude_env());
        Some(claude_settings(&exes.app, json!({ "hooks": { "Stop": [app_hook(&exes.app, &["--hook", &hook_dir])] } })))
    };
    spawn.args.extend(choice.cli_args());
    spawn.args.push(prompt.to_string());
    (spawn, settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::session::tests::{places, APP};
    use crate::config::Config;
    use crate::testutil::Tmp;

    fn exes() -> Exes {
        Exes { claude: "claude".into(), codex: "codex".into(), app: r"C:\Program Files\Orq\o'rq.exe".into() }
    }

    /// Medido com o portable-pty 0.9: `codex` acha antes o atalho do npm sem extensão e falha com `os error 193`.
    #[test]
    fn clis_reais_sao_os_binarios_nativos() {
        let real = Exes::real();
        assert_eq!((real.claude.as_str(), real.codex.as_str()), ("claude.exe", "codex.exe"));
    }

    fn session(tag: &str, tool: &str) -> (Session, Tmp) {
        let cfg = Config { orchestrator: Choice::new(tool, "", ""), ..Config::default() };
        let (tmp, p) = places(tag);
        let (s, r) = Session::create(&p, APP, r"C:\proj", &cfg);
        r.unwrap();
        (s, tmp)
    }

    #[test]
    fn orquestrador_codex() {
        let (s, _tmp) = session("cli-codex", "codex");
        let dir = s.dir.to_string_lossy().to_string();
        let got = orchestrator(&exes(), &Choice::new("codex", "gpt-5.5", "high"), r"C:\proj", &s);
        assert_eq!(got.program, "codex");
        assert_eq!(got.cwd, r"C:\proj");
        let mcp_args = format!("mcp_servers.orquestrador.args=[\"--mcp\",{}]", serde_json::to_string(&dir).unwrap());
        let role = std::fs::read_to_string(s.dir.join("AGENTS.md")).unwrap();
        let instructions = format!("developer_instructions={}", serde_json::to_string(&role).unwrap());
        assert_eq!(
            got.args,
            [
                "--no-daemon", "--no-alt-screen", "-C", r"C:\proj",
                "--dangerously-bypass-approvals-and-sandbox", "--dangerously-bypass-hook-trust",
                "-c", r#"projects={"C:\\proj"={trust_level="trusted"}}"#,
                "-c", r#"mcp_servers.orquestrador.command="C:\\Program Files\\Orq\\o'rq.exe""#,
                "-c", &mcp_args,
                "-c", r#"mcp_servers.orquestrador.default_tools_approval_mode="approve""#,
                "-c", "mcp_servers.orquestrador.tool_timeout_sec=900",
                "-c", &instructions,
                "-m", "gpt-5.5", "-c", "model_reasoning_effort=high",
            ]
        );
        assert!(role.contains(r"C:\proj") && role.contains("`delegar`"), "o papel é o mesmo texto do AGENTS.md da sessão");
    }

    #[test]
    fn orquestrador_claude() {
        let (s, _tmp) = session("cli-claude", "claude");
        let got = orchestrator(&exes(), &Choice::new("claude", "opus", ""), r"C:\proj", &s);
        assert_eq!(got.program, "claude");
        assert_eq!(got.cwd, r"C:\proj");
        assert_eq!(got.env, [("CLAUDE_CODE_SANDBOXED".to_string(), "1".to_string())]);
        assert_eq!(
            got.args,
            [
                "--mcp-config", &s.dir.join("mcp.json").to_string_lossy(),
                "--settings", &s.dir.join("claude-settings.json").to_string_lossy(),
                "--permission-mode", "bypassPermissions",
                "--append-system-prompt-file", &s.dir.join("AGENTS.md").to_string_lossy(),
                "--model", "opus",
            ]
        );
        assert!(!got.args.iter().any(|a| a == "--plugin-dir" || a == "--add-dir"), "as skills vêm do carregamento nativo");
    }

    /// Caminho do app, do projeto e da sessão com espaço, acento e apóstrofo: cada um entra como um argumento só, e
    /// no `-c` do Codex vai como texto TOML.
    #[test]
    fn caminhos_com_espaco_acento_e_apostrofo_passam_inteiros() {
        let (_tmp, mut p) = places("cli-odd");
        p.temp = p.temp.parent().unwrap().join("temp com espaço, açaí e it's");
        std::fs::create_dir_all(&p.temp).unwrap();
        let app = r"C:\Users\Maria Silva\it's\ação\orquestrador.exe";
        let project = r"C:\Users\Maria Silva\projeto d'água";
        let cfg = Config { orchestrator: Choice::new("codex", "", ""), ..Config::default() };
        let (s, r) = Session::create(&p, app, project, &cfg);
        r.unwrap();
        let dir = s.dir.to_string_lossy().to_string();
        let exes = Exes { claude: "claude".into(), codex: "codex".into(), app: app.into() };
        let got = orchestrator(&exes, &Choice::new("codex", "", ""), project, &s);
        assert_eq!(got.cwd, project);
        assert_eq!(got.args[3], project);
        assert!(got.args.contains(&format!("projects={{{}={{trust_level=\"trusted\"}}}}", toml(project))));
        assert!(got.args.contains(&format!("mcp_servers.orquestrador.command={}", toml(app))));
        assert!(got.args.contains(&format!("mcp_servers.orquestrador.args=[{},{}]", toml("--mcp"), toml(&dir))));
        assert!(got.args.contains(&format!("developer_instructions={}", toml(&roles::orchestrator(project)))));
        assert!(dir.contains("temp com espaço, açaí e it's"));
        let claude = orchestrator(&exes, &Choice::new("claude", "", ""), project, &s);
        assert_eq!(claude.cwd, project);
        assert_eq!(claude.args[0], "--mcp-config");
        assert_eq!(claude.args[1], s.dir.join("mcp.json").to_string_lossy());
        assert!(claude.args[1].contains("it's"));
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
        let v: serde_json::Value = serde_json::from_str(&settings.unwrap()).unwrap();
        assert_eq!(v["skipDangerousModePermissionPrompt"], true);
        assert!(v.get("permissions").is_none() && v["hooks"].get("PreToolUse").is_none(), "nenhum comando barrado pelo app");
        let stop = &v["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(stop["command"], r"C:\Program Files\Orq\o'rq.exe");
        assert_eq!(stop["args"], serde_json::json!(["--hook", r"C:\tmp\s\exec\exec-1\hook"]));
        let allow = &v["hooks"]["PermissionRequest"][0];
        assert_eq!(allow["hooks"][0]["command"], r"C:\Program Files\Orq\o'rq.exe");
        assert_eq!(allow["hooks"][0]["args"], serde_json::json!(["--allow"]));
        assert!(allow.get("matcher").is_none(), "o pedido de qualquer ferramenta é aceito");
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
                "--no-daemon", "--no-alt-screen", "-C", r"C:\proj",
                "--dangerously-bypass-approvals-and-sandbox", "--dangerously-bypass-hook-trust",
                "-c", r#"projects={"C:\\proj"={trust_level="trusted"}}"#,
                "-c", r#"notify=["C:\\Program Files\\Orq\\o'rq.exe","--hook","C:\\tmp\\s\\exec\\exec-2\\hook"]"#,
                "-c", "model_reasoning_effort=xhigh",
                "PROMPT",
            ]
        );
        assert!(!got.args.iter().any(|a| a.contains("PermissionRequest")), "o Codex não recebe o hook PermissionRequest");
        assert!(!got.args.iter().any(|a| a.contains("mcp_servers")), "o executor não recebe o servidor do orquestrador");
    }

    #[test]
    fn prompt_do_executor_seguido_da_tarefa() {
        let p = executor_prompt("  Faça X.\nPronto quando: Y.\n");
        assert!(p.starts_with(EXECUTOR.trim_end()));
        assert!(p.ends_with("\nFaça X.\nPronto quando: Y."));
    }
}
