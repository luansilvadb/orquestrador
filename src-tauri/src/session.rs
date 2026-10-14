//! A pasta temporária de uma sessão do orquestrador (`strategist-<guid>`): o `mcp.json` que aponta o servidor MCP
//! de delegação (Claude), o Git que o orquestrador pode rodar, as regras do Codex e as permissões do Claude, a
//! limpeza e a recuperação das sobras de sessões mortas.
use crate::cli::toml;
use crate::config::Config;
use crate::proc::{owner_state, start_time, Owner};
use crate::roles;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A justificativa da regra do comando de delegação da forma antiga (`orq-<id>.ps1`, até a v0.1.9). Só
/// `recover_orphans` a usa, para limpar a sessão de quem atualizou com um Codex aberto.
const JUSTIFICATION: &str = "Delegacao autorizada ao executor";

/// O Git do orquestrador, com qualquer subcomando. `git.exe` vai junto porque o orquestrador pode digitar
/// o nome com a extensão, e a regra casa o token exato.
const GIT_PROGRAMS: [&str; 2] = ["git", "git.exe"];
const GIT_JUSTIFICATION: &str = "Git do orquestrador, autorizado pelo usuario";

/// Onde a sessão vive e o que ela consulta fora de si. Os testes trocam tudo por pastas próprias.
#[derive(Clone, Debug)]
pub struct Places {
    pub temp: PathBuf,
    pub codex_rules: PathBuf,
    /// Pastas globais do usuário que têm `skills` (`.agents`, `.claude`), nessa ordem.
    pub global_dirs: Vec<PathBuf>,
}

impl Places {
    pub fn real() -> Places {
        let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default());
        let codex_home = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
        Places {
            temp: std::env::temp_dir(),
            codex_rules: codex_home.unwrap_or_else(|| home.join(".codex")).join("rules"),
            global_dirs: [".agents", ".claude"].iter().map(|d| home.join(d)).filter(|d| d.join("skills").is_dir()).collect(),
        }
    }
}

pub struct Session {
    pub dir: PathBuf,
    pub rule: PathBuf,
    pub global_dirs: Vec<PathBuf>,
}

/// Uma regra `prefix_rule` do Codex para um único token, que é o programa liberado, com a justificativa que quem
/// a pede escolhe (`GIT_JUSTIFICATION` para o Git, `JUSTIFICATION` na forma antiga).
fn rule(token: &str, justification: &str) -> String {
    format!("prefix_rule(pattern=[{}], decision=\"allow\", justification=\"{justification}\")\n", toml(token))
}

/// O nome do comando de delegação da forma antiga: `orq-<id>.ps1`, com o id que dá nome à pasta `strategist-<id>`.
fn command_for(name: &str) -> String {
    format!("orq-{}.ps1", name.strip_prefix("strategist-").unwrap_or(name))
}

/// A primeira linha da regra do Codex de uma sessão: é por ela que `recover_orphans` a reconhece.
fn marker(name: &str) -> String {
    format!("# orquestrador {name}")
}

/// Os programas das skills globais, por interpretador, nas duas grafias de barra (o orquestrador
/// pode escrever qualquer uma). Medido no Claude Code 2.1.296: regra com curinga dentro do
/// caminho não passa em dontAsk; regra que nomeia o arquivo passa. Por isso um a um.
pub fn skill_programs(global_dirs: &[PathBuf], runner: &str) -> Vec<String> {
    let exts: &[&str] = if runner == "node" { &["mjs", "cjs", "js"] } else { &["py"] };
    fn walk(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                // A profundidade só existe para um link em círculo não prender o início.
                if depth < 24 && path.file_name().is_some_and(|n| n != "node_modules") {
                    walk(&path, depth + 1, out);
                }
            } else {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    for dir in global_dirs {
        walk(&dir.join("skills"), 0, &mut files);
    }
    files
        .iter()
        .filter(|f| f.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e)))
        .flat_map(|f| {
            let s = f.to_string_lossy().to_string();
            [s.clone(), s.replace('\\', "/")]
        })
        .collect()
}

const RUNNERS: [&str; 2] = ["node", "python"];

impl Session {
    /// Cria a pasta e tudo o que a sessão usa. `app` é o caminho do próprio app, que a sessão põe como servidor
    /// MCP do orquestrador. O que falhar no meio é desfeito por quem chamou, com `remove`: por isso `dir` e `rule`
    /// são devolvidos mesmo no erro.
    pub fn create(places: &Places, app: &str, project: &str, cfg: &Config) -> (Session, Result<(), String>) {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let name = format!("strategist-{nanos:032x}");
        let session = Session {
            rule: places.codex_rules.join(format!("{name}.rules")),
            global_dirs: places.global_dirs.clone(),
            dir: places.temp.join(&name),
        };
        let result = session
            .fill(&name, app, project, cfg)
            .map_err(|e| format!("Não foi possível criar a sessão em {}: {e}", session.dir.display()));
        (session, result)
    }

    fn fill(&self, name: &str, app: &str, project: &str, cfg: &Config) -> std::io::Result<()> {
        let d = &self.dir;
        std::fs::create_dir(d)?;
        for sub in ["requests", "results", "exec"] {
            std::fs::create_dir(d.join(sub))?;
        }
        let pid = std::process::id();
        std::fs::write(d.join("owner"), format!("{pid} {} {name}", start_time(pid).unwrap_or(0)))?;
        std::fs::write(d.join("AGENTS.md"), roles::ORCHESTRATOR.replace("{{PROJECT}}", project))?;
        if cfg.orchestrator.tool == "codex" {
            // O servidor do Codex vai na linha de comando, em `cli.rs`; a regra só leva o marcador e o Git.
            std::fs::create_dir_all(self.rule.parent().unwrap())?;
            std::fs::write(&self.rule, self.codex_rules(name))?;
        } else {
            let mcp = json!({ "mcpServers": { "orquestrador": { "command": app, "args": ["--mcp", d.to_string_lossy()] } } });
            std::fs::write(d.join("mcp.json"), serde_json::to_string_pretty(&mcp).unwrap())?;
            std::fs::write(d.join("claude-settings.json"), self.claude_settings())?;
        }
        Ok(())
    }

    /// A primeira linha é o marcador desta sessão, que `recover_orphans` reconhece. Depois dela, uma regra para cada
    /// programa do Git e uma por interpretador com os programas das skills. O servidor de delegação não tem regra:
    /// o Codex o chama por MCP, e a aprovação das ferramentas dele vai na linha de comando.
    fn codex_rules(&self, name: &str) -> String {
        let mut text = format!("{}\n", marker(name));
        for program in GIT_PROGRAMS {
            text += &rule(program, GIT_JUSTIFICATION);
        }
        for runner in RUNNERS {
            let files: Vec<String> = skill_programs(&self.global_dirs, runner).iter().map(|f| toml(f)).collect();
            if !files.is_empty() {
                text += &format!(
                    "prefix_rule(pattern=[\"{runner}\", [{}]], decision=\"allow\", justification=\"Programa de skill global do usuario\")\n",
                    files.join(", ")
                );
            }
        }
        text
    }

    fn claude_settings(&self) -> String {
        let mut allow: Vec<String> = ["Read", "Glob", "Grep", "Skill", "mcp__orquestrador__delegar", "mcp__orquestrador__aguardar", "mcp__orquestrador__responder"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        allow.extend(GIT_PROGRAMS.iter().map(|g| format!("PowerShell({g} *)")));
        // Medido no Claude Code 2.1.296: a regra casa o texto do comando, com as aspas. O orquestrador costuma pôr o
        // caminho entre aspas (`node "C:\...\x.mjs" start`), e a regra sem aspas nega esse comando. Por isso cada
        // programa entra nas três grafias: sem aspas, com aspas duplas e com aspas simples.
        for runner in RUNNERS {
            for file in skill_programs(&self.global_dirs, runner) {
                allow.extend(["", "\"", "'"].iter().map(|q| format!("PowerShell({runner} {q}{file}{q} *)")));
            }
        }
        // Sem a primeira variável, o Claude Code 2.1.296 não expõe o PowerShell no Windows com Git Bash. A segunda
        // dá às chamadas ao servidor do orquestrador os 15 minutos da espera de um lote. Elas entram só por este
        // arquivo, passado com --settings: o ambiente do usuário não muda.
        serde_json::to_string_pretty(&json!({
            "env": { "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1", "MCP_TOOL_TIMEOUT": "900000" },
            "permissions": { "allow": allow },
        }))
        .unwrap()
    }

    /// Tira a regra e a pasta. Nunca falha: o que não saiu fica e é dito no texto devolvido.
    pub fn remove(&self) -> Option<String> {
        let _ = std::fs::remove_file(&self.rule);
        remove_dir(&self.dir)
    }
}

/// Apaga uma pasta de sessão. Link e junção são desfeitos sem tocar o alvo, inclusive quando a própria pasta
/// é um. Tenta de novo por um instante, porque um processo recém-morto ainda pode segurar um arquivo.
pub fn remove_dir(dir: &Path) -> Option<String> {
    let mut last = None;
    for _ in 0..10 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => last = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Some(format!("Aviso: a pasta de sessão {} não foi removida por inteiro e ficou no lugar: {}", dir.display(), last.unwrap()))
}

fn is_session_name(name: &str) -> bool {
    name.strip_prefix("strategist-").is_some_and(|id| id.len() == 32 && id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')))
}

/// O dono que a pasta declara em `owner`: pid, hora de início e o nome da sessão.
fn owner_of(dir: &Path) -> Option<(u32, u64, String)> {
    let owner = std::fs::read_to_string(dir.join("owner")).unwrap_or_default();
    let parts: Vec<&str> = owner.trim().split(' ').collect();
    let [pid, started, session] = parts[..] else { return None };
    Some((pid.parse().ok()?, started.parse().ok()?, session.to_string()))
}

/// A sessão desta pasta está viva: o dono que ela declara ainda está rodando.
pub fn owner_alive(dir: &Path) -> bool {
    owner_of(dir).is_some_and(|(pid, started, _)| owner_state(pid, started) == Owner::Alive)
}

/// A pasta se declara aquela sessão e o dono está comprovadamente morto.
fn owner_dead(dir: &Path, name: &str) -> bool {
    owner_of(dir).is_some_and(|(pid, started, session)| session == name && owner_state(pid, started) == Owner::Dead)
}

/// A primeira linha é o marcador que este app escreveu para a pasta `dir`, exato: outro nome ou espaço a mais não é
/// nosso.
fn is_marker(line: &str, dir: &Path) -> bool {
    dir.file_name().and_then(|n| n.to_str()).is_some_and(|name| marker(name) == line)
}

/// A primeira linha é a regra do comando de delegação da forma antiga, a da v0.1.9, para a pasta `dir`. Compara com
/// o texto exato que `rule` escreve, sem interpretar o TOML: uma linha parecida, com outro token ou com mais tokens,
/// não é nossa e fica.
///
/// Sai a partir de 2027-02-01. Ela existe só para limpar arquivos órfãos de quem atualizou com um orquestrador Codex
/// aberto: esses arquivos liberam o git, fora do sandbox, para qualquer codex.
fn is_legacy_rule(line: &str, dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else { return false };
    rule(&command_for(name), JUSTIFICATION).trim_end() == line
}

/// Uma sessão morta à força deixa a regra do Codex valendo enquanto a pasta dela existir, e a
/// pasta para trás. As duas saem aqui, mas só com prova: a regra é a que este app escreveu para
/// aquela pasta (`is_marker`, ou `is_legacy_rule` na forma antiga), a pasta se declara aquela sessão
/// (`pid início sessão` em `owner`) e o dono está comprovadamente morto. Menos que isso fica intocado.
pub fn recover_orphans(places: &Places) {
    let temp = &places.temp;
    for entry in std::fs::read_dir(&places.codex_rules).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".rules")) else { continue };
        if !is_session_name(name) || !path.is_file() {
            continue;
        }
        let old = temp.join(name);
        let first = std::fs::read_to_string(&path).ok().and_then(|t| t.lines().next().map(String::from));
        if !first.is_some_and(|line| is_marker(&line, &old) || is_legacy_rule(&line, &old)) {
            continue;
        }
        if !old.exists() {
            let _ = std::fs::remove_file(&path);
        } else if owner_dead(&old, name) {
            let _ = std::fs::remove_file(&path);
            remove_dir(&old);
        }
    }
    // Sessões sem regra (orquestrador Claude) só deixam a pasta: mesma prova pelo dono.
    for entry in std::fs::read_dir(temp).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_session_name(&name) && owner_dead(&entry.path(), &name) {
            remove_dir(&entry.path());
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::Choice;

    /// O caminho do app que as sessões de teste põem no servidor MCP.
    pub const APP: &str = r"C:\Program Files\Orq\orquestrador.exe";

    pub fn places(tag: &str) -> Places {
        let root = std::env::temp_dir().join(format!("orq-ses-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let skills = root.join("home").join(".agents").join("skills");
        std::fs::create_dir_all(skills.join("exemplo").join("node_modules").join("dep")).unwrap();
        std::fs::create_dir_all(root.join("temp")).unwrap();
        std::fs::write(skills.join("exemplo").join("canvas.mjs"), "").unwrap();
        std::fs::write(skills.join("exemplo").join("tool.py"), "").unwrap();
        std::fs::write(skills.join("exemplo").join("SKILL.md"), "").unwrap();
        std::fs::write(skills.join("exemplo").join("node_modules").join("dep").join("index.js"), "").unwrap();
        Places { temp: root.join("temp"), codex_rules: root.join("codex").join("rules"), global_dirs: vec![root.join("home").join(".agents")] }
    }

    pub fn discard(p: &Places) {
        let _ = std::fs::remove_dir_all(p.temp.parent().unwrap());
    }

    fn cfg(orchestrator: &str) -> Config {
        Config { orchestrator: Choice::new(orchestrator, "", ""), ..Config::default() }
    }

    #[test]
    fn sessao_codex_escreve_marcador_regras_agents_e_owner_e_limpa_tudo() {
        let p = places("codex");
        let (s, r) = Session::create(&p, APP, r"C:\proj", &cfg("codex"));
        r.unwrap();
        let name = s.dir.file_name().unwrap().to_string_lossy().to_string();
        assert!(is_session_name(&name));
        let agents = std::fs::read_to_string(s.dir.join("AGENTS.md")).unwrap();
        assert!(agents.contains(r"C:\proj") && !agents.contains("{{"));
        for tool in ["`delegar`", "`aguardar`", "`responder`"] {
            assert!(agents.contains(tool), "{tool}");
        }
        assert!(!agents.contains("cada apóstrofo é dobrado") && !agents.contains("Não use here-string"));
        let owner = std::fs::read_to_string(s.dir.join("owner")).unwrap();
        assert_eq!(owner, format!("{} {} {name}", std::process::id(), start_time(std::process::id()).unwrap()));

        let rule = std::fs::read_to_string(&s.rule).unwrap();
        let lines: Vec<&str> = rule.lines().collect();
        assert_eq!(lines[0], format!("# orquestrador {name}"));
        assert!(is_marker(lines[0], &s.dir));
        assert_eq!(lines[1], "prefix_rule(pattern=[\"git\"], decision=\"allow\", justification=\"Git do orquestrador, autorizado pelo usuario\")");
        assert_eq!(lines[2], "prefix_rule(pattern=[\"git.exe\"], decision=\"allow\", justification=\"Git do orquestrador, autorizado pelo usuario\")");
        assert!(lines[3].starts_with("prefix_rule(pattern=[\"node\", [") && lines[3].contains("canvas.mjs") && lines[3].contains("exemplo/canvas.mjs"));
        assert!(lines[4].starts_with("prefix_rule(pattern=[\"python\", [") && lines[4].contains("tool.py"));
        assert_eq!(lines.len(), 5, "marcador, git, git.exe, node e python, e nada além");
        // O caminho das skills de teste tem `orq-ses-`, então o comando antigo se reconhece pelo `.ps1`.
        assert!(!rule.contains(".ps1") && !rule.contains("Delegacao") && !rule.contains("node_modules") && !rule.contains("SKILL.md"));
        assert!(!s.dir.join("claude-settings.json").exists() && !s.dir.join("mcp.json").exists());

        // O que o app deixa na pasta durante o uso também sai.
        std::fs::create_dir_all(s.dir.join("requests").join("x")).unwrap();
        std::fs::write(s.dir.join("requests").join("x").join("1.txt"), "t").unwrap();
        assert_eq!(s.remove(), None);
        assert!(!s.dir.exists() && !s.rule.exists());
        discard(&p);
    }

    #[test]
    fn sessao_claude_escreve_mcp_settings_e_nenhuma_regra() {
        let p = places("claude");
        let (s, r) = Session::create(&p, APP, r"C:\proj", &cfg("claude"));
        r.unwrap();
        let agents = std::fs::read_to_string(s.dir.join("AGENTS.md")).unwrap();
        assert!(!agents.contains("{{") && agents.contains("`delegar`"), "{agents}");
        let mcp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(s.dir.join("mcp.json")).unwrap()).unwrap();
        assert_eq!(mcp["mcpServers"]["orquestrador"]["command"], APP);
        assert_eq!(mcp["mcpServers"]["orquestrador"]["args"], json!(["--mcp", s.dir.to_string_lossy()]));
        assert_eq!(mcp["mcpServers"].as_object().unwrap().len(), 1, "só o orquestrador");
        let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(s.dir.join("claude-settings.json")).unwrap()).unwrap();
        assert_eq!(settings["env"]["CLAUDE_CODE_USE_POWERSHELL_TOOL"], "1", "sem a variável o Claude não expõe o PowerShell");
        assert_eq!(settings["env"]["MCP_TOOL_TIMEOUT"], "900000", "a espera de um lote cabe no prazo");
        let allow: Vec<&str> = settings["permissions"]["allow"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(
            &allow[..9],
            [
                "Read", "Glob", "Grep", "Skill",
                "mcp__orquestrador__delegar", "mcp__orquestrador__aguardar", "mcp__orquestrador__responder",
                "PowerShell(git *)", "PowerShell(git.exe *)",
            ]
        );
        assert!(allow.iter().all(|a| !a.starts_with("PowerShell(orq-")), "o comando antigo saiu da permissão");
        assert!(allow.iter().any(|a| a.starts_with("PowerShell(node ") && a.ends_with("canvas.mjs *)")));
        assert!(allow.iter().any(|a| a.starts_with("PowerShell(python ") && a.ends_with("tool.py *)")));
        // O caminho entre aspas, como o orquestrador costuma escrever, também passa.
        assert!(allow.iter().any(|a| a.starts_with("PowerShell(node \"") && a.ends_with("canvas.mjs\" *)")));
        assert!(allow.iter().any(|a| a.starts_with("PowerShell(node '") && a.ends_with("canvas.mjs' *)")));
        assert!(allow.iter().all(|a| !a.contains("--hook") && !a.contains("--guard")), "os modos dos hooks não são liberados ao orquestrador");
        assert!(!s.rule.exists());
        assert_eq!(s.remove(), None);
        discard(&p);
    }

    /// Pasta e app com espaço, acento e apóstrofo: o `mcp.json` leva o caminho inteiro, sem nome curto.
    #[test]
    fn caminho_com_espaco_acento_e_apostrofo_vai_inteiro_ao_mcp_json() {
        let mut p = places("caminho");
        p.temp = p.temp.parent().unwrap().join("temp com espaço e açaí it's");
        std::fs::create_dir_all(&p.temp).unwrap();
        let app = r"C:\Users\Maria Silva\it's\ação\orquestrador.exe";
        let (s, r) = Session::create(&p, app, r"C:\proj", &cfg("claude"));
        r.unwrap();
        let mcp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(s.dir.join("mcp.json")).unwrap()).unwrap();
        assert_eq!(mcp["mcpServers"]["orquestrador"]["command"], app);
        assert_eq!(mcp["mcpServers"]["orquestrador"]["args"][1], *s.dir.to_string_lossy());
        assert_eq!(s.remove(), None);
        discard(&p);
    }

    #[test]
    fn regra_orfa_so_sai_com_prova() {
        let p = places("orphans");
        std::fs::create_dir_all(&p.codex_rules).unwrap();
        let temp = p.temp.clone();
        // `first` escolhe a primeira linha da regra: o marcador atual, a forma antiga ou uma estrangeira.
        let make = |id: char, owner: Option<String>, first: &dyn Fn(&str) -> String| {
            let name = format!("strategist-{}", id.to_string().repeat(32));
            let dir = temp.join(&name);
            if let Some(owner) = owner {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("owner"), owner.replace("{name}", &name)).unwrap();
            }
            let rule_file = p.codex_rules.join(format!("{name}.rules"));
            std::fs::write(&rule_file, format!("{}\n", first(&name))).unwrap();
            (dir, rule_file)
        };
        let me = std::process::id();
        let dead_owner = format!("{me} {} {{name}}", start_time(me).unwrap() + 1);
        let live_owner = format!("{me} {} {{name}}", start_time(me).unwrap());
        let marker_line = |name: &str| marker(name);
        let legacy = |name: &str| rule(&command_for(name), JUSTIFICATION).trim_end().to_string();
        let foreign = |_: &str| "prefix_rule(pattern=[\"outra\"])".to_string();
        // Quase iguais ao marcador: dois espaços, um token a mais, e um nome de sessão a mais.
        let double_space = |name: &str| format!("# orquestrador  {name}");
        let extra_word = |name: &str| format!("{} extra", marker(name));
        // Quase iguais à forma antiga: um token a mais, e um token com espaço no fim.
        let two_tokens = |name: &str| format!("prefix_rule(pattern=[{}, \"x\"], decision=\"allow\", justification=\"{JUSTIFICATION}\")", toml(&command_for(name)));
        let wrong_token = |name: &str| rule(&format!("{} ", command_for(name)), JUSTIFICATION).trim_end().to_string();
        // Ids só com hexadecimal, como o nome de sessão exige.
        let alive = make('a', Some(live_owner.clone()), &marker_line);
        let dead = make('b', Some(dead_owner.clone()), &marker_line);
        let gone = make('c', None, &marker_line);
        let legacy_dead = make('f', Some(dead_owner.clone()), &legacy);
        let foreign_dead = make('d', Some(dead_owner.clone()), &foreign);
        let misnamed = make('e', Some(format!("{me} 1 strategist-outra")), &marker_line);
        let spaced = make('4', Some(dead_owner.clone()), &double_space);
        let extra = make('5', Some(dead_owner.clone()), &extra_word);
        let two_dead = make('2', Some(dead_owner.clone()), &two_tokens);
        let token_dead = make('3', Some(dead_owner.clone()), &wrong_token);
        std::fs::write(p.codex_rules.join("default.rules"), "x").unwrap();

        recover_orphans(&p);

        assert!(alive.0.exists() && alive.1.exists(), "dono vivo fica");
        assert!(!dead.0.exists() && !dead.1.exists(), "marcador com dono morto: regra e pasta saem");
        assert!(!gone.1.exists(), "pasta sumida: a regra sai");
        assert!(!legacy_dead.0.exists() && !legacy_dead.1.exists(), "forma antiga com dono morto: regra e pasta saem");
        assert!(foreign_dead.1.exists() && !foreign_dead.0.exists(), "regra de primeira linha diferente fica; a pasta de dono morto sai");
        assert!(misnamed.0.exists() && misnamed.1.exists(), "owner de outra sessão fica");
        assert!(spaced.1.exists() && !spaced.0.exists(), "marcador com espaço a mais não é nosso: a regra fica");
        assert!(extra.1.exists() && !extra.0.exists(), "marcador com palavra a mais não é nosso: a regra fica");
        assert!(two_dead.1.exists() && !two_dead.0.exists(), "regra antiga com dois tokens não é nossa: fica");
        assert!(token_dead.1.exists() && !token_dead.0.exists(), "token com espaço a mais não é nosso: fica");
        assert!(p.codex_rules.join("default.rules").exists());
        discard(&p);
    }

    #[test]
    fn regra_com_git_e_skills_sai_como_orfa_pela_primeira_linha() {
        // As regras do Git e das skills vêm depois do marcador: a sessão morta sai inteira.
        let p = places("orfa-git");
        let (s, r) = Session::create(&p, APP, r"C:\proj", &cfg("codex"));
        r.unwrap();
        let me = std::process::id();
        let name = s.dir.file_name().unwrap().to_string_lossy().to_string();
        std::fs::write(s.dir.join("owner"), format!("{me} {} {name}", start_time(me).unwrap() + 1)).unwrap();
        recover_orphans(&p);
        assert!(!s.rule.exists() && !s.dir.exists());
        discard(&p);
    }

    #[test]
    fn pasta_de_sessao_claude_morta_sai_pelo_dono() {
        let p = places("claude-orphan");
        let name = format!("strategist-{}", "f".repeat(32));
        let dir = p.temp.join(&name);
        std::fs::create_dir_all(dir.join("exec")).unwrap();
        std::fs::write(dir.join("owner"), format!("{} 1 {name}", std::process::id())).unwrap();
        recover_orphans(&p);
        assert!(!dir.exists());
        discard(&p);
    }
}
