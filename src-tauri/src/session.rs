//! A pasta temporária de uma sessão do orquestrador (`strategist-<guid>`), o adaptador de delegação que
//! ela libera (`orq-<guid>.ps1`), o Git que o orquestrador pode rodar, as regras do Codex e as permissões
//! do Claude, a limpeza e a recuperação das sobras de sessões mortas.
use crate::cli::toml;
use crate::config::Config;
use crate::delegate;
use crate::proc::{owner_state, start_time, Owner};
use crate::roles;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A justificativa que a regra do Codex leva; é o que o Codex mostra ao aprovar a regra.
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
    /// O comando de delegação como o orquestrador o digita: só o nome do adaptador. Ele fica na pasta da
    /// sessão, que é a primeira entrada do PATH do orquestrador, então não precisa de caminho nem aspas.
    pub command: String,
    pub rule: PathBuf,
    pub global_dirs: Vec<PathBuf>,
}

fn clean(path: &Path) -> String {
    path.to_string_lossy().trim_end_matches('\\').to_string()
}

/// Uma regra `prefix_rule` do Codex para um único token, que é o programa liberado.
fn rule(token: &str) -> String {
    rule_with(token, JUSTIFICATION)
}

/// A mesma regra, com a justificativa que quem a pede escolhe (o Git do orquestrador, por exemplo).
fn rule_with(token: &str, justification: &str) -> String {
    format!("prefix_rule(pattern=[{}], decision=\"allow\", justification=\"{justification}\")\n", toml(token))
}

/// O comando de delegação de uma sessão: `orq-<id>.ps1`, com o id que dá nome à pasta `strategist-<id>`.
pub fn command_for(name: &str) -> String {
    format!("orq-{}.ps1", name.strip_prefix("strategist-").unwrap_or(name))
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
    pub fn create(places: &Places, delegate: &str, project: &str, cfg: &Config) -> (Session, Result<(), String>) {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let name = format!("strategist-{nanos:032x}");
        let session = Session {
            command: command_for(&name),
            rule: places.codex_rules.join(format!("{name}.rules")),
            global_dirs: places.global_dirs.clone(),
            dir: PathBuf::from(clean(&places.temp)).join(&name),
        };
        let result = if Path::new(delegate).is_file() {
            delegate::adapter(Path::new(delegate), &session.dir).and_then(|script| {
                session
                    .fill(&name, project, cfg, &script)
                    .map_err(|e| format!("Não foi possível criar a sessão em {}: {e}", session.dir.display()))
            })
        } else {
            Err(format!(
                "O programa de delegação não está em {delegate}. Ele vai junto com o app: em desenvolvimento, ele é compilado por `npm run dev:tauri`."
            ))
        };
        (session, result)
    }

    fn fill(&self, name: &str, project: &str, cfg: &Config, script: &str) -> std::io::Result<()> {
        let d = &self.dir;
        std::fs::create_dir(d)?;
        for sub in ["requests", "results", "exec"] {
            std::fs::create_dir(d.join(sub))?;
        }
        let pid = std::process::id();
        std::fs::write(d.join("owner"), format!("{pid} {} {name}", start_time(pid).unwrap_or(0)))?;
        // Com BOM: o Windows PowerShell 5.1 lê um .ps1 sem BOM como ANSI, e o caminho dele sairia errado.
        std::fs::write(d.join(&self.command), format!("\u{feff}{script}"))?;
        let agents = roles::ORCHESTRATOR.replace("{{DELEGATE}}", &self.command).replace("{{PROJECT}}", project);
        std::fs::write(d.join("AGENTS.md"), agents)?;
        if cfg.orchestrator.tool == "codex" {
            std::fs::create_dir_all(self.rule.parent().unwrap())?;
            std::fs::write(&self.rule, self.codex_rules())?;
        } else {
            std::fs::write(d.join("claude-settings.json"), self.claude_settings())?;
        }
        Ok(())
    }

    /// A regra do comando de delegação, que tem de ficar na primeira linha (é por ela que `recover_orphans`
    /// reconhece a sessão). Depois dela, uma regra para cada programa do Git e uma por interpretador com os
    /// programas das skills.
    fn codex_rules(&self) -> String {
        let mut text = rule(&self.command);
        for program in GIT_PROGRAMS {
            text += &rule_with(program, GIT_JUSTIFICATION);
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
        let mut allow: Vec<String> = ["Read", "Glob", "Grep", "Skill"].iter().map(|s| s.to_string()).collect();
        allow.push(format!("PowerShell({} *)", self.command));
        allow.extend(GIT_PROGRAMS.iter().map(|g| format!("PowerShell({g} *)")));
        // Medido no Claude Code 2.1.296: a regra casa o texto do comando, com as aspas. O orquestrador costuma
        // pôr o caminho entre aspas (`node "C:\...\x.mjs" start`), e a regra sem aspas nega esse comando. Por
        // isso cada programa entra nas três grafias: sem aspas, com aspas duplas e com aspas simples.
        for runner in RUNNERS {
            for file in skill_programs(&self.global_dirs, runner) {
                allow.extend(["", "\"", "'"].iter().map(|q| format!("PowerShell({runner} {q}{file}{q} *)")));
            }
        }
        // Sem esta variável, o Claude Code 2.1.296 não expõe o PowerShell no Windows com Git Bash. Ela entra só
        // por este arquivo, passado com --settings: o ambiente do usuário não muda.
        serde_json::to_string_pretty(&serde_json::json!({
            "env": { "CLAUDE_CODE_USE_POWERSHELL_TOOL": "1" },
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

/// Se a primeira linha é a regra que este app escreveu para a pasta `dir`, a do adaptador da sessão.
/// Compara com o texto exato que `rule` escreve, sem interpretar o TOML: uma linha parecida, com outro
/// token ou com mais tokens, não é nossa e fica.
fn is_delegate_rule(line: &str, dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else { return false };
    rule(&command_for(name)).trim_end() == line
}

/// Uma sessão morta à força deixa a regra do Codex valendo enquanto a pasta dela existir, e a
/// pasta para trás. As duas saem aqui, mas só com prova: a regra é a que este app escreveu para
/// aquela pasta (`is_delegate_rule`), a pasta se declara aquela sessão (`pid início sessão` em `owner`)
/// e o dono está comprovadamente morto. Menos que isso fica intocado.
pub fn recover_orphans(places: &Places) {
    let temp = PathBuf::from(clean(&places.temp));
    for entry in std::fs::read_dir(&places.codex_rules).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".rules")) else { continue };
        if !is_session_name(name) || !path.is_file() {
            continue;
        }
        let old = temp.join(name);
        let first = std::fs::read_to_string(&path).ok().and_then(|t| t.lines().next().map(String::from));
        if !first.is_some_and(|line| is_delegate_rule(&line, &old)) {
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
        std::fs::create_dir_all(skills.join("exemplo").join("node_modules").join("dep")).unwrap();
        std::fs::create_dir_all(root.join("temp")).unwrap();
        std::fs::write(root.join("orquestrador-delegar.exe"), "").unwrap();
        std::fs::write(skills.join("exemplo").join("canvas.mjs"), "").unwrap();
        std::fs::write(skills.join("exemplo").join("tool.py"), "").unwrap();
        std::fs::write(skills.join("exemplo").join("SKILL.md"), "").unwrap();
        std::fs::write(skills.join("exemplo").join("node_modules").join("dep").join("index.js"), "").unwrap();
        Places { temp: root.join("temp"), codex_rules: root.join("codex").join("rules"), global_dirs: vec![root.join("home").join(".agents")] }
    }

    /// O programa de delegação de mentira que `places` deixa na raiz do teste.
    pub fn delegate_of(p: &Places) -> String {
        p.temp.parent().unwrap().join("orquestrador-delegar.exe").to_string_lossy().into_owned()
    }

    pub fn discard(p: &Places) {
        let _ = std::fs::remove_dir_all(p.temp.parent().unwrap());
    }

    fn cfg(orchestrator: &str) -> Config {
        Config { orchestrator: Choice::new(orchestrator, "", ""), ..Config::default() }
    }

    #[test]
    fn sessao_codex_escreve_regra_adaptador_agents_e_owner_e_limpa_tudo() {
        let p = places("codex");
        let delegate = delegate_of(&p);
        let (s, r) = Session::create(&p, &delegate, r"C:\proj", &cfg("codex"));
        r.unwrap();
        let name = s.dir.file_name().unwrap().to_string_lossy().to_string();
        assert!(is_session_name(&name));
        assert_eq!(s.command, format!("orq-{}.ps1", &name["strategist-".len()..]));
        assert!(!s.command.contains(['\\', '/', ' ', '\'']), "o comando é um nome só: {}", s.command);
        let adapter = std::fs::read_to_string(s.dir.join(&s.command)).unwrap();
        assert!(adapter.starts_with('\u{feff}') && adapter.contains("$program = '"), "{adapter}");
        let agents = std::fs::read_to_string(s.dir.join("AGENTS.md")).unwrap();
        assert!(agents.contains(r"C:\proj") && agents.contains(&s.command));
        assert!(!agents.contains("{{"));
        let owner = std::fs::read_to_string(s.dir.join("owner")).unwrap();
        assert_eq!(owner, format!("{} {} {name}", std::process::id(), start_time(std::process::id()).unwrap()));

        let rule = std::fs::read_to_string(&s.rule).unwrap();
        let lines: Vec<&str> = rule.lines().collect();
        assert_eq!(lines[0], format!("prefix_rule(pattern=[\"{}\"], decision=\"allow\", justification=\"Delegacao autorizada ao executor\")", s.command));
        assert!(is_delegate_rule(lines[0], &s.dir));
        assert_eq!(lines[1], "prefix_rule(pattern=[\"git\"], decision=\"allow\", justification=\"Git do orquestrador, autorizado pelo usuario\")");
        assert_eq!(lines[2], "prefix_rule(pattern=[\"git.exe\"], decision=\"allow\", justification=\"Git do orquestrador, autorizado pelo usuario\")");
        assert!(lines[3].starts_with("prefix_rule(pattern=[\"node\", [") && lines[3].contains("canvas.mjs") && lines[3].contains("exemplo/canvas.mjs"));
        assert!(lines[4].starts_with("prefix_rule(pattern=[\"python\", [") && lines[4].contains("tool.py"));
        assert_eq!(lines.len(), 5, "delegação, git, git.exe, node e python, e nada além");
        assert!(!rule.contains("node_modules") && !rule.contains("SKILL.md") && !rule.contains("orquestrador-delegar"));
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
        let (s, r) = Session::create(&p, &delegate_of(&p), r"C:\proj", &cfg("claude"));
        r.unwrap();
        let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(s.dir.join("claude-settings.json")).unwrap()).unwrap();
        assert_eq!(settings["env"]["CLAUDE_CODE_USE_POWERSHELL_TOOL"], "1", "sem a variável o Claude não expõe o PowerShell");
        let allow: Vec<&str> = settings["permissions"]["allow"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(&allow[..7], ["Read", "Glob", "Grep", "Skill", &format!("PowerShell({} *)", s.command), "PowerShell(git *)", "PowerShell(git.exe *)"]);
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

    #[test]
    fn caminho_com_espaco_acento_e_apostrofo_nao_precisa_de_nome_curto() {
        // Nenhum nome 8.3 nem ativação de volume: a pasta e o programa têm caminhos comuns de usuário.
        let mut p = places("caminho");
        let odd = p.temp.parent().unwrap().join("pasta com ação").join("it's");
        std::fs::create_dir_all(&odd).unwrap();
        std::fs::write(odd.join("orquestrador-delegar.exe"), "").unwrap();
        p.temp = p.temp.parent().unwrap().join("temp com espaço e açaí it's");
        std::fs::create_dir_all(&p.temp).unwrap();
        let delegate = odd.join("orquestrador-delegar.exe").to_string_lossy().into_owned();
        let (s, r) = Session::create(&p, &delegate, r"C:\proj", &cfg("claude"));
        r.unwrap();
        assert!(s.dir.starts_with(&p.temp) && s.dir.join(&s.command).is_file());
        let adapter = std::fs::read_to_string(s.dir.join(&s.command)).unwrap();
        assert!(adapter.contains("$program = 'C:\\") && adapter.contains("it''s"), "{adapter}");
        assert_eq!(s.remove(), None);
        discard(&p);
    }

    #[test]
    fn programa_inexistente_nao_cria_a_sessao() {
        let p = places("sem-programa");
        let (s, r) = Session::create(&p, r"C:\não existe\orquestrador-delegar.exe", r"C:\proj", &cfg("claude"));
        assert!(r.unwrap_err().contains("não está em"));
        assert!(!s.dir.exists());
        discard(&p);
    }

    #[test]
    fn regra_orfa_so_sai_com_prova() {
        let p = places("orphans");
        std::fs::create_dir_all(&p.codex_rules).unwrap();
        let temp = PathBuf::from(clean(&p.temp));
        // `first` escolhe a primeira linha: a regra atual ou uma estrangeira.
        let make = |id: char, owner: Option<String>, first: &dyn Fn(&Path) -> String| {
            let name = format!("strategist-{}", id.to_string().repeat(32));
            let dir = temp.join(&name);
            if let Some(owner) = owner {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("owner"), owner.replace("{name}", &name)).unwrap();
            }
            let rule_file = p.codex_rules.join(format!("{name}.rules"));
            std::fs::write(&rule_file, first(&dir)).unwrap();
            (dir, rule_file)
        };
        let me = std::process::id();
        let current = |dir: &Path| rule(&command_for(&dir.file_name().unwrap().to_string_lossy()));
        let foreign = |_: &Path| "prefix_rule(pattern=[\"outra\"])\n".to_string();
        // Quase iguais à nossa: um token a mais, e um token com espaço no fim.
        let two_tokens = |dir: &Path| format!("prefix_rule(pattern=[{}, \"x\"], decision=\"allow\", justification=\"{JUSTIFICATION}\")\n", toml(&command_for(&dir.file_name().unwrap().to_string_lossy())));
        let wrong_token = |dir: &Path| rule(&format!("{} ", command_for(&dir.file_name().unwrap().to_string_lossy())));
        // Ids só com hexadecimal, como o nome de sessão exige.
        let alive = make('a', Some(format!("{me} {} {{name}}", start_time(me).unwrap())), &current);
        let dead = make('b', Some(format!("{me} {} {{name}}", start_time(me).unwrap() + 1)), &current);
        let gone = make('c', None, &current);
        let foreign_dead = make('d', Some(format!("{me} {} {{name}}", start_time(me).unwrap() + 1)), &foreign);
        let misnamed = make('e', Some(format!("{me} 1 strategist-outra")), &current);
        let two_dead = make('2', Some(format!("{me} {} {{name}}", start_time(me).unwrap() + 1)), &two_tokens);
        let token_dead = make('3', Some(format!("{me} {} {{name}}", start_time(me).unwrap() + 1)), &wrong_token);
        std::fs::write(p.codex_rules.join("default.rules"), "x").unwrap();

        recover_orphans(&p);

        assert!(alive.0.exists() && alive.1.exists(), "dono vivo fica");
        assert!(!dead.0.exists() && !dead.1.exists(), "dono morto: regra e pasta saem");
        assert!(!gone.1.exists(), "pasta sumida: a regra sai");
        assert!(foreign_dead.1.exists() && !foreign_dead.0.exists(), "regra de primeira linha diferente fica; a pasta de dono morto sai");
        assert!(misnamed.0.exists() && misnamed.1.exists(), "owner de outra sessão fica");
        assert!(two_dead.1.exists() && !two_dead.0.exists(), "regra com dois tokens não é nossa: fica");
        assert!(token_dead.1.exists() && !token_dead.0.exists(), "token com espaço a mais não é nosso: fica");
        assert!(p.codex_rules.join("default.rules").exists());
        discard(&p);
    }

    #[test]
    fn regra_com_git_e_skills_sai_como_orfa_pela_primeira_linha() {
        // As regras do Git e das skills vêm depois da de delegação: a sessão morta sai inteira, como antes.
        let p = places("orfa-git");
        let (s, r) = Session::create(&p, &delegate_of(&p), r"C:\proj", &cfg("codex"));
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
        let dir = PathBuf::from(clean(&p.temp)).join(&name);
        std::fs::create_dir_all(dir.join("exec")).unwrap();
        std::fs::write(dir.join("owner"), format!("{} 1 {name}", std::process::id())).unwrap();
        recover_orphans(&p);
        assert!(!dir.exists());
        discard(&p);
    }
}
