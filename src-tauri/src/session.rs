//! A pasta temporária de uma sessão do orquestrador (`strategist-<guid>`), o launcher de
//! delegação, a regra do Codex, a limpeza e a recuperação das sobras de sessões mortas.
use crate::config::Config;
use crate::proc::{owner_state, start_time, Owner};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub const STRATEGIST_MD: &str = include_str!("../../strategist.md");
pub const EXECUTOR_MD: &str = include_str!("../../executor.md");
const DELEGATE_PS1: &str = include_str!("delegate.ps1");

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
    pub launcher: PathBuf,
    pub rule: PathBuf,
    pub global_dirs: Vec<PathBuf>,
}

fn clean(path: &Path) -> String {
    path.to_string_lossy().trim_end_matches('\\').to_string()
}

fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn rule_text(launcher: &str) -> String {
    format!("prefix_rule(pattern=[{}], decision=\"allow\", justification=\"Delegacao autorizada ao executor\")\n", toml_str(launcher))
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
    /// Cria a pasta e tudo o que a sessão usa. O que falhar no meio é desfeito por quem chamou,
    /// com `remove`: por isso `dir` e `rule` são devolvidos mesmo no erro.
    pub fn create(places: &Places, project: &str, cfg: &Config) -> (Session, Result<(), String>) {
        let name = format!("strategist-{}", uuid::Uuid::new_v4().simple());
        let dir = PathBuf::from(clean(&places.temp)).join(&name);
        let session = Session {
            launcher: dir.join("delegate.ps1"),
            rule: places.codex_rules.join(format!("{name}.rules")),
            global_dirs: places.global_dirs.clone(),
            dir,
        };
        let result = session.fill(&name, project, cfg).map_err(|e| format!("Não foi possível criar a sessão em {}: {e}", session.dir.display()));
        (session, result)
    }

    fn fill(&self, name: &str, project: &str, cfg: &Config) -> std::io::Result<()> {
        let d = &self.dir;
        std::fs::create_dir(d)?;
        for sub in ["requests", "results", "exec"] {
            std::fs::create_dir(d.join(sub))?;
        }
        let pid = std::process::id();
        std::fs::write(d.join("owner"), format!("{pid} {} {name}", start_time(pid).unwrap_or(0)))?;
        std::fs::write(&self.launcher, DELEGATE_PS1.replace('\n', "\r\n").replace("\r\r\n", "\r\n"))?;
        // Cópia congelada da configuração desta sessão.
        std::fs::write(d.join("session-config.json"), serde_json::to_string_pretty(cfg).unwrap())?;
        let agents = STRATEGIST_MD.replace("{{DELEGATE}}", &self.launcher.to_string_lossy()).replace("{{PROJECT}}", project);
        std::fs::write(d.join("AGENTS.md"), agents)?;
        if cfg.orchestrator.tool == "codex" {
            std::fs::create_dir_all(self.rule.parent().unwrap())?;
            std::fs::write(&self.rule, self.codex_rules())?;
        } else {
            std::fs::write(d.join("claude-settings.json"), self.claude_settings())?;
        }
        Ok(())
    }

    /// A regra do launcher e, depois dela, uma regra por interpretador com os programas das skills.
    fn codex_rules(&self) -> String {
        let mut text = rule_text(&self.launcher.to_string_lossy());
        for runner in RUNNERS {
            let files: Vec<String> = skill_programs(&self.global_dirs, runner).iter().map(|f| toml_str(f)).collect();
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
        let mut allow: Vec<String> = ["Read", "Glob", "Grep", "Skill"].iter().map(|s| s.to_string()).collect();
        allow.push(format!("PowerShell({} *)", self.launcher.display()));
        for runner in RUNNERS {
            allow.extend(skill_programs(&self.global_dirs, runner).iter().map(|f| format!("PowerShell({runner} {f} *)")));
        }
        serde_json::to_string_pretty(&serde_json::json!({ "permissions": { "allow": allow } })).unwrap()
    }

    /// Tira a regra e a pasta. Nunca falha: o que não saiu fica e é dito no texto devolvido.
    pub fn remove(&self) -> Option<String> {
        let _ = std::fs::remove_file(&self.rule);
        remove_dir(&self.dir)
    }
}

/// Apaga uma pasta de sessão sem seguir junção nem link: o que for um fica no lugar, e a pasta
/// que o contém também. Tenta de novo por um instante, porque um processo recém-morto ainda
/// pode segurar um arquivo.
pub fn remove_dir(dir: &Path) -> Option<String> {
    const REPARSE: u32 = 0x400;
    fn sweep(dir: &Path) -> std::io::Result<()> {
        if std::fs::symlink_metadata(dir)?.file_attributes() & REPARSE != 0 {
            return Err(std::io::Error::other("é uma junção ou um link, que não é seguido"));
        }
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let meta = std::fs::symlink_metadata(&path)?;
            if meta.file_attributes() & REPARSE != 0 {
                continue;
            }
            if meta.is_dir() { sweep(&path)? } else { std::fs::remove_file(&path)? }
        }
        std::fs::remove_dir(dir)
    }
    let mut last = None;
    for _ in 0..10 {
        match sweep(dir) {
            Ok(()) => return None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !dir.exists() => return None,
            Err(e) => last = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Some(format!("Aviso: a pasta de sessão {} não foi removida por inteiro e ficou no lugar: {}", dir.display(), last.unwrap()))
}

fn is_session_name(name: &str) -> bool {
    name.strip_prefix("strategist-").is_some_and(|id| id.len() == 32 && id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')))
}

/// A pasta se declara aquela sessão e o dono está comprovadamente morto.
fn owner_dead(dir: &Path, name: &str) -> bool {
    let owner = std::fs::read_to_string(dir.join("owner")).unwrap_or_default();
    let parts: Vec<&str> = owner.trim().split(' ').collect();
    let [pid, started, session] = parts[..] else { return false };
    let (Ok(pid), Ok(started)) = (pid.parse::<u32>(), started.parse::<u64>()) else { return false };
    session == name && owner_state(pid, started) == Owner::Dead
}

/// Uma sessão morta à força deixa a regra do Codex valendo enquanto o launcher dela existir, e a
/// pasta para trás. As duas saem aqui, mas só com prova: a regra tem o nome e a primeira linha
/// exatos que este app escreve para aquela pasta, a pasta se declara aquela sessão (`pid início
/// sessão` em `owner`) e o dono está comprovadamente morto. Menos que isso fica intocado.
pub fn recover_orphans(places: &Places) {
    let temp = PathBuf::from(clean(&places.temp));
    for entry in std::fs::read_dir(&places.codex_rules).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".rules")) else { continue };
        if !is_session_name(name) || !path.is_file() {
            continue;
        }
        let old = temp.join(name);
        let expected = rule_text(&old.join("delegate.ps1").to_string_lossy());
        let first = std::fs::read_to_string(&path).ok().and_then(|t| t.lines().next().map(String::from));
        if first.as_deref() != Some(expected.trim()) {
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
    for entry in std::fs::read_dir(&temp).into_iter().flatten().flatten() {
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

    pub fn places(tag: &str) -> Places {
        let root = std::env::temp_dir().join(format!("orq-ses-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let skills = root.join("home").join(".agents").join("skills");
        std::fs::create_dir_all(skills.join("grilling").join("node_modules").join("dep")).unwrap();
        std::fs::create_dir_all(root.join("temp")).unwrap();
        std::fs::write(skills.join("grilling").join("canvas.mjs"), "").unwrap();
        std::fs::write(skills.join("grilling").join("tool.py"), "").unwrap();
        std::fs::write(skills.join("grilling").join("SKILL.md"), "").unwrap();
        std::fs::write(skills.join("grilling").join("node_modules").join("dep").join("index.js"), "").unwrap();
        Places { temp: root.join("temp"), codex_rules: root.join("codex").join("rules"), global_dirs: vec![root.join("home").join(".agents")] }
    }

    pub fn discard(p: &Places) {
        let _ = std::fs::remove_dir_all(p.temp.parent().unwrap());
    }

    fn cfg(orchestrator: &str) -> Config {
        Config { orchestrator: Choice::new(orchestrator, "", ""), ..Config::default() }
    }

    #[test]
    fn sessao_codex_escreve_regra_agents_e_owner_e_limpa_tudo() {
        let p = places("codex");
        let (s, r) = Session::create(&p, r"C:\proj", &cfg("codex"));
        r.unwrap();
        let name = s.dir.file_name().unwrap().to_string_lossy().to_string();
        assert!(is_session_name(&name));
        let agents = std::fs::read_to_string(s.dir.join("AGENTS.md")).unwrap();
        assert!(agents.contains(r"C:\proj") && agents.contains(&*s.launcher.to_string_lossy()));
        assert!(!agents.contains("{{"));
        let owner = std::fs::read_to_string(s.dir.join("owner")).unwrap();
        assert_eq!(owner, format!("{} {} {name}", std::process::id(), start_time(std::process::id()).unwrap()));
        let frozen: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(s.dir.join("session-config.json")).unwrap()).unwrap();
        assert_eq!(frozen["orchestrator"]["tool"], "codex");

        let rule = std::fs::read_to_string(&s.rule).unwrap();
        let lines: Vec<&str> = rule.lines().collect();
        let escaped = s.launcher.to_string_lossy().replace('\\', "\\\\");
        assert_eq!(lines[0], format!("prefix_rule(pattern=[\"{escaped}\"], decision=\"allow\", justification=\"Delegacao autorizada ao executor\")"));
        assert!(lines[1].starts_with("prefix_rule(pattern=[\"node\", [") && lines[1].contains("canvas.mjs") && lines[1].contains("grilling/canvas.mjs"));
        assert!(lines[2].starts_with("prefix_rule(pattern=[\"python\", [") && lines[2].contains("tool.py"));
        assert!(!rule.contains("node_modules") && !rule.contains("SKILL.md"));
        assert!(!s.dir.join("claude-settings.json").exists());

        // O que o app deixa na pasta durante o uso também sai.
        std::fs::create_dir_all(s.dir.join("requests").join("x")).unwrap();
        std::fs::write(s.dir.join("requests").join("x").join("1.txt"), "t").unwrap();
        assert_eq!(s.remove(), None);
        assert!(!s.dir.exists() && !s.rule.exists());
        discard(&p);
    }

    #[test]
    fn sessao_claude_escreve_settings_e_nenhuma_regra() {
        let p = places("claude");
        let (s, r) = Session::create(&p, r"C:\proj", &cfg("claude"));
        r.unwrap();
        let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(s.dir.join("claude-settings.json")).unwrap()).unwrap();
        let allow: Vec<&str> = settings["permissions"]["allow"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(&allow[..5], ["Read", "Glob", "Grep", "Skill", &format!("PowerShell({} *)", s.launcher.display())]);
        assert!(allow.iter().any(|a| a.starts_with("PowerShell(node ") && a.ends_with("canvas.mjs *)")));
        assert!(allow.iter().any(|a| a.starts_with("PowerShell(python ") && a.ends_with("tool.py *)")));
        assert!(!s.rule.exists());
        assert_eq!(s.remove(), None);
        discard(&p);
    }

    #[test]
    fn regra_orfa_so_sai_com_prova() {
        let p = places("orphans");
        std::fs::create_dir_all(&p.codex_rules).unwrap();
        let temp = PathBuf::from(clean(&p.temp));
        let make = |id: char, owner: Option<String>, first_line_ok: bool| {
            let name = format!("strategist-{}", id.to_string().repeat(32));
            let dir = temp.join(&name);
            if let Some(owner) = owner {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("delegate.ps1"), "").unwrap();
                std::fs::write(dir.join("owner"), owner.replace("{name}", &name)).unwrap();
            }
            let text = if first_line_ok { rule_text(&dir.join("delegate.ps1").to_string_lossy()) } else { "prefix_rule(pattern=[\"outra\"])\n".into() };
            let rule = p.codex_rules.join(format!("{name}.rules"));
            std::fs::write(&rule, text).unwrap();
            (dir, rule)
        };
        let me = std::process::id();
        let alive = make('a', Some(format!("{me} {} {{name}}", start_time(me).unwrap())), true);
        let dead = make('b', Some(format!("{me} {} {{name}}", start_time(me).unwrap() + 1)), true);
        let gone = make('c', None, true);
        let foreign = make('d', Some(format!("{me} 1 {{name}}")), false);
        let misnamed = make('e', Some(format!("{me} 1 strategist-outra")), true);
        std::fs::write(p.codex_rules.join("default.rules"), "x").unwrap();

        recover_orphans(&p);

        assert!(alive.0.exists() && alive.1.exists(), "dono vivo fica");
        assert!(!dead.0.exists() && !dead.1.exists(), "dono morto: regra e pasta saem");
        assert!(!gone.1.exists(), "pasta sumida: a regra sai");
        assert!(foreign.1.exists() && !foreign.0.exists(), "regra de primeira linha diferente fica; a pasta de dono morto sai");
        assert!(misnamed.0.exists() && misnamed.1.exists(), "owner de outra sessão fica");
        assert!(p.codex_rules.join("default.rules").exists());
        discard(&p);
    }

    #[test]
    fn pasta_de_sessao_claude_morta_sai_pelo_dono() {
        let p = places("claude-orphan");
        let name = format!("strategist-{}", "f".repeat(32));
        let dir = PathBuf::from(clean(&p.temp)).join(&name);
        std::fs::create_dir_all(dir.join("exec")).unwrap();
        std::fs::write(dir.join("owner"), format!("{} 1 {name}", std::process::id())).unwrap();
        recover_orphans(&p);
        assert!(!dir.exists());
        discard(&p);
    }
}
