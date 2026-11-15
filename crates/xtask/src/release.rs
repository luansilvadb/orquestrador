//! Regras do fluxo de release: versões iguais nos manifestos, notas completas, o `latest.json` que o app lê e a
//! conferência do instalador. Uso: `cargo xtask release <comando> ...` (detalhes em docs/releases/README.md).
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

pub const REPO: &str = "luansilvadb/orquestrador";
pub const REQUIRED_SECTIONS: [&str; 7] = [
    "Resumo",
    "Novidades",
    "Melhorias e correções",
    "Impacto da atualização",
    "Requisitos e limitações",
    "Instalação",
    "Comparação com a versão anterior",
];
// Texto de modelo ou de release automática: não pode chegar à release.
static PLACEHOLDER: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?-u:\b)(TODO|TBD|XXX)(?-u:\b)|\{\{|<!--|Instalador para Windows, por usuário\. O app se atualiza sozinho ao abrir\.").unwrap()
});
static SEMVER: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$").unwrap());
const MIN_NOTES_CHARS: usize = 1500;
const MIN_SECTION_CHARS: usize = 60;
pub const CI_WAIT_MS: f64 = 30. * 60. * 1000.;
const CI_POLL_MS: f64 = 15_000.;
const CI_RUN_LIMIT: usize = 1000;
const CI_RUN_FIELDS: &str =
    "attempt,conclusion,createdAt,databaseId,event,headBranch,headSha,status,workflowDatabaseId,workflowName,startedAt,updatedAt";
const PENDENTES: [&str; 5] = ["queued", "in_progress", "requested", "waiting", "pending"];

pub type R<T> = Result<T, String>;

/// Tamanho como o `length` do JavaScript (unidades UTF-16), para os mínimos valerem igual.
fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

// ---------------------------------------------------------------- CI

/// `Date.parse` de ISO 8601 (`AAAA-MM-DDTHH:MM:SS[.fff](Z|±HH:MM)`) em ms desde a época.
fn parse_iso(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let num = |a: usize, n: usize| -> Option<i64> {
        let t = s.get(a..a + n)?;
        t.bytes().all(|c| c.is_ascii_digit()).then(|| t.parse().ok())?
    };
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d, h, mi, se) = (num(0, 4)?, num(5, 2)?, num(8, 2)?, num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 59 {
        return None;
    }
    let mut i = 19;
    let mut frac = 0.;
    if b.get(i) == Some(&b'.') {
        let ini = i + 1;
        i = ini;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == ini {
            return None;
        }
        frac = format!("0.{}", &s[ini..i]).parse::<f64>().ok()? * 1000.;
    }
    let off = match &s[i..] {
        "Z" => 0,
        r if r.len() == 6 && (r.starts_with('+') || r.starts_with('-')) && &r[3..4] == ":" => {
            let v = num(i + 1, 2)? * 60 + num(i + 4, 2)?;
            if r.starts_with('-') {
                -v
            } else {
                v
            }
        }
        _ => return None,
    };
    // Dias desde a época (algoritmo de Howard Hinnant).
    let (yy, m) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let dias = era * 146097 + doe - 719468;
    let seg = dias * 86400 + h * 3600 + mi * 60 + se - off * 60;
    Some(seg as f64 * 1000. + frac)
}

fn ci_run_time(run: &Value) -> R<f64> {
    let parse = |field: &str, optional: bool| -> R<Option<f64>> {
        let v = &run[field];
        if optional && (v.is_null() || v.as_str() == Some("0001-01-01T00:00:00Z")) {
            return Ok(None);
        }
        match v.as_str().and_then(parse_iso) {
            Some(t) if t > 0. => Ok(Some(t)),
            _ => Err(format!("CI retornou {field} inválido.")),
        }
    };
    let created = parse("createdAt", false)?.unwrap();
    let updated = parse("updatedAt", false)?.unwrap();
    let started = parse("startedAt", true)?;
    let status = run["status"].as_str().unwrap_or("");
    let pending = PENDENTES.contains(&status);
    if (status == "in_progress" || status == "completed") && started.is_none() {
        return Err("CI retornou startedAt ausente para uma execução iniciada.".into());
    }
    Ok(if pending && status != "in_progress" { updated } else { started.unwrap_or(created) })
}

/// Inteiro seguro e positivo, como `Number.isSafeInteger(x) && x >= 1`.
fn inteiro(v: &Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    let f = v.as_f64()?;
    (f.fract() == 0. && f.abs() <= 9007199254740991.).then_some(f as i64)
}

struct Execucao {
    attempt: i64,
    conclusion: String,
    database_id: i64,
    status: String,
    current_at: f64,
}

fn validate_ci_run(run: &Value, sha: &str) -> R<Execucao> {
    const FIELDS: [&str; 12] = [
        "attempt", "conclusion", "createdAt", "databaseId", "event", "headBranch", "headSha", "status",
        "workflowDatabaseId", "workflowName", "startedAt", "updatedAt",
    ];
    let Some(obj) = run.as_object() else { return Err("CI retornou uma execução incompleta ou inválida.".into()) };
    if FIELDS.iter().any(|f| !obj.contains_key(*f)) {
        return Err("CI retornou uma execução incompleta ou inválida.".into());
    }
    let pos = |f: &str| inteiro(&run[f]).filter(|&i| i >= 1);
    let (Some(attempt), Some(database_id), Some(_)) = (pos("attempt"), pos("databaseId"), pos("workflowDatabaseId")) else {
        return Err("CI retornou identificadores ou tentativa inválidos.".into());
    };
    if run["workflowName"] != "ci"
        || run["event"] != "push"
        || run["headBranch"] != "main"
        || !run["headSha"].as_str().is_some_and(|s| s.to_lowercase() == sha.to_lowercase())
    {
        return Err("CI retornou workflow, evento, branch ou SHA diferente do esperado.".into());
    }
    let conclusion = &run["conclusion"];
    if !conclusion.is_null() && !conclusion.is_string() {
        return Err("CI retornou conclusion inválida.".into());
    }
    let current_at = ci_run_time(run)?;
    let status = run["status"].as_str().unwrap_or("").to_string();
    let conclusion = conclusion.as_str().unwrap_or("").to_string();
    if PENDENTES.contains(&status.as_str()) {
        if !conclusion.is_empty() {
            return Err("CI pendente retornou uma conclusão inesperada.".into());
        }
    } else if status != "completed" {
        return Err(format!("CI retornou status desconhecido: {}.", run["status"].as_str().map_or(run["status"].to_string(), String::from)));
    }
    Ok(Execucao { attempt, conclusion, database_id, status, current_at })
}

/// O `gh` de `wait_for_ci`: argumentos e prazo em ms.
pub type Exec<'a> = Box<dyn FnMut(&[String], u64) -> R<String> + 'a>;

/// O que `wait_for_ci` usa do mundo: o `gh` (argumentos e prazo em ms), o relógio em ms e a espera.
pub struct Ci<'a> {
    pub exec: Exec<'a>,
    pub now: Box<dyn FnMut() -> f64 + 'a>,
    pub sleep: Box<dyn FnMut(f64) + 'a>,
    pub timeout_ms: f64,
    pub poll_ms: f64,
}

impl Ci<'static> {
    pub fn real() -> Self {
        let inicio = Instant::now();
        Ci {
            exec: Box::new(gh),
            now: Box::new(move || inicio.elapsed().as_secs_f64() * 1000.),
            sleep: Box::new(|ms| std::thread::sleep(Duration::from_secs_f64(ms / 1000.))),
            timeout_ms: CI_WAIT_MS,
            poll_ms: CI_POLL_MS,
        }
    }
}

/// `gh` com prazo: passou dele, o processo morre e volta erro.
fn gh(args: &[String], timeout: u64) -> R<String> {
    use std::io::Read;
    let mut filho = std::process::Command::new("gh")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("gh indisponível: {e}"))?;
    let mut saida = filho.stdout.take().unwrap();
    let mut erro = filho.stderr.take().unwrap();
    let leitor = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = saida.read_to_end(&mut s);
        s
    });
    let leitor_erro = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = erro.read_to_end(&mut s);
        s
    });
    let fim = Instant::now() + Duration::from_millis(timeout);
    let status = loop {
        if let Some(s) = filho.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if Instant::now() >= fim {
            let _ = filho.kill();
            return Err(format!("gh {} excedeu o prazo de {timeout} ms.", args.join(" ")));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let s = leitor.join().unwrap_or_default();
    let e = leitor_erro.join().unwrap_or_default();
    if !status.success() {
        return Err(format!("gh falhou ({status}): {}", String::from_utf8_lossy(&e).trim()));
    }
    if s.len() > 1024 * 1024 {
        return Err("gh run list retornou mais de 1 MiB.".into());
    }
    String::from_utf8(s).map_err(|_| "gh run list retornou uma resposta inválida.".into())
}

pub fn wait_for_ci(sha: &str, mut ci: Ci) -> R<String> {
    let sha_ok = sha.len() == 40 && sha.bytes().all(|c| c.is_ascii_hexdigit());
    if !sha_ok {
        return Err("wait-ci exige o SHA de commit completo retornado por git rev-parse HEAD.".into());
    }
    if !ci.timeout_ms.is_finite() || ci.timeout_ms <= 0. || !ci.poll_ms.is_finite() || ci.poll_ms <= 0. {
        return Err("Prazo ou intervalo de espera inválido.".into());
    }
    let expirou = || format!("CI de {sha} não concluiu com sucesso em até 30 minutos.");
    let deadline = (ci.now)() + ci.timeout_ms;
    let args: Vec<String> = [
        "run", "list", "--repo", REPO, "--workflow", "ci.yml", "--commit", sha, "--branch", "main", "--event", "push",
        "--limit", &CI_RUN_LIMIT.to_string(), "--json", CI_RUN_FIELDS,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    loop {
        let remaining = deadline - (ci.now)();
        if remaining <= 0. {
            return Err(expirou());
        }
        let output = (ci.exec)(&args, remaining.floor().max(1.) as u64)?;
        if deadline - (ci.now)() <= 0. {
            return Err(expirou());
        }
        let runs: Value = serde_json::from_str(&output).map_err(|_| "gh run list retornou JSON inválido.".to_string())?;
        if deadline - (ci.now)() <= 0. {
            return Err(expirou());
        }
        let Some(runs) = runs.as_array() else { return Err("gh run list não retornou uma lista de execuções.".into()) };
        if runs.len() >= CI_RUN_LIMIT {
            return Err("CI retornou execuções demais para determinar a tentativa mais recente.".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut validated = Vec::new();
        for run in runs {
            let r = validate_ci_run(run, sha)?;
            if !ids.insert(r.database_id) {
                return Err("CI retornou a mesma execução mais de uma vez.".into());
            }
            validated.push(r);
        }
        validated.sort_by(|a, b| b.current_at.total_cmp(&a.current_at));
        if let Some(latest) = validated.first() {
            if validated.get(1).is_some_and(|s| s.current_at == latest.current_at) {
                return Err("CI tem execuções mais recentes com horário ambíguo.".into());
            }
            if latest.status == "completed" {
                if latest.conclusion != "success" {
                    let c = if latest.conclusion.is_empty() { "conclusão ausente" } else { &latest.conclusion };
                    return Err(format!("CI da tentativa {} terminou com {c}.", latest.attempt));
                }
                if deadline - (ci.now)() <= 0. {
                    return Err(expirou());
                }
                return Ok(format!("CI aprovada para {sha} (push em main, tentativa {}).", latest.attempt));
            }
        }
        let wait = deadline - (ci.now)();
        if wait <= 0. {
            return Err(expirou());
        }
        (ci.sleep)(ci.poll_ms.min(wait));
    }
}

// ---------------------------------------------------------------- versões

pub fn check_semver(version: &str) -> R<&str> {
    if SEMVER.is_match(version) {
        Ok(version)
    } else {
        Err(format!("\"{version}\" não é uma versão X.Y.Z."))
    }
}

/// `version` da seção pedida do Cargo.toml (padrão `[package]`), não a de dependências.
pub fn cargo_package_version(text: &str, section: &str) -> Option<String> {
    let mut linhas = text.lines();
    linhas.by_ref().find(|l| *l == section)?;
    static V: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r#"^version\s*=\s*"([^"]+)""#).unwrap());
    for line in linhas {
        if line.starts_with('[') {
            break;
        }
        if let Some(m) = V.captures(line) {
            return Some(m[1].to_string());
        }
    }
    None
}

/// Todas as versões do pacote `name` no Cargo.lock (deve haver uma só).
pub fn cargo_lock_versions(text: &str, name: &str) -> Vec<String> {
    static P: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new("^\\s*name = \"([^\"]+)\"\r?\nversion = \"([^\"]+)\"").unwrap());
    text.split("[[package]]")
        .filter_map(|b| P.captures(b))
        .filter(|m| &m[1] == name)
        .map(|m| m[2].to_string())
        .collect()
}

/// Versões por arquivo. O `[workspace.package]` e o Cargo.lock valem sempre; `package.json`, `package-lock.json` e
/// `tauri.conf.json` entram enquanto existirem.
pub fn manifest_versions(root: &Path) -> R<BTreeMap<String, Option<String>>> {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    let json = |p: &str| -> R<Value> { serde_json::from_str(&read(p)?).map_err(|e| format!("{p}: {e}")) };
    let s = |v: &Value| v.as_str().map(String::from);
    let mut out = BTreeMap::new();
    if root.join("package.json").exists() {
        out.insert("package.json".into(), s(&json("package.json")?["version"]));
    }
    if root.join("package-lock.json").exists() {
        let lock = json("package-lock.json")?;
        out.insert("package-lock.json".into(), s(&lock["version"]));
        out.insert("package-lock.json (packages[\"\"])".into(), s(&lock["packages"][""]["version"]));
    }
    if root.join("src-tauri/tauri.conf.json").exists() {
        out.insert("src-tauri/tauri.conf.json".into(), s(&json("src-tauri/tauri.conf.json")?["version"]));
    }
    out.insert("Cargo.toml ([workspace.package])".into(), cargo_package_version(&read("Cargo.toml")?, "[workspace.package]"));
    let lock = read("Cargo.lock")?;
    let one = |e: Vec<String>| if e.len() == 1 { e[0].clone() } else { format!("{} entradas", e.len()) };
    let mut pacotes = vec!["orquestrador-core", "orquestrador-app"];
    if root.join("src-tauri/Cargo.toml").exists() {
        pacotes.insert(0, "orquestrador");
    }
    for p in pacotes {
        out.insert(format!("Cargo.lock ({p})"), Some(one(cargo_lock_versions(&lock, p))));
    }
    Ok(out)
}

pub fn check_versions<'a>(versions: &BTreeMap<String, Option<String>>, expected: &'a str) -> R<&'a str> {
    check_semver(expected)?;
    let wrong: Vec<String> = versions
        .iter()
        .filter(|(_, f)| f.as_deref() != Some(expected))
        .map(|(file, f)| format!("  {file}: {}", f.as_deref().unwrap_or("ausente")))
        .collect();
    if wrong.is_empty() {
        Ok(expected)
    } else {
        Err(format!("A versão {expected} não bate com:\n{}", wrong.join("\n")))
    }
}

// ---------------------------------------------------------------- notas

/// Partes depois de cada `## ` no começo de linha, como `split(/^## /m).slice(1)`.
fn secoes(body: &str) -> Vec<(&str, &str)> {
    let mut inicios: Vec<usize> = Vec::new();
    if body.starts_with("## ") {
        inicios.push(0);
    }
    inicios.extend(body.match_indices("\n## ").map(|(i, _)| i + 1));
    inicios
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let fim = inicios.get(k + 1).map_or(body.len(), |&f| f);
            let part = &body[i + 3..fim];
            match part.find('\n') {
                Some(e) => (part[..e].trim(), part[e + 1..].trim()),
                None => ("", part.trim()),
            }
        })
        .collect()
}

/// Lista de problemas; vazia quer dizer que as notas servem.
pub fn note_problems(text: &str, version: &str, previous: Option<&str>) -> Vec<String> {
    let body = text.trim();
    let mut problems = Vec::new();
    if body.split('\n').next().unwrap_or("").trim() != format!("# Orquestrador v{version}") {
        problems.push(format!("a primeira linha deve ser \"# Orquestrador v{version}\""));
    }
    if len16(body) < MIN_NOTES_CHARS {
        problems.push(format!("notas curtas demais ({} caracteres; mínimo {MIN_NOTES_CHARS})", len16(body)));
    }
    if let Some(m) = PLACEHOLDER.find(body) {
        problems.push(format!("texto de modelo ou genérico: \"{}\"", m.as_str()));
    }
    let sections = secoes(body);
    for required in REQUIRED_SECTIONS {
        match sections.iter().find(|(t, _)| t.starts_with(required)) {
            None => problems.push(format!("falta a seção \"## {required}\"")),
            Some((_, t)) if len16(t) < MIN_SECTION_CHARS => {
                problems.push(format!("a seção \"## {required}\" está vazia ou curta demais"))
            }
            Some((_, t)) => {
                if let Some(p) = previous.filter(|p| !p.is_empty() && required.starts_with("Comparação")) {
                    if !t.contains(&format!("v{p}")) {
                        problems.push(format!("a comparação não cita v{p}"));
                    }
                }
            }
        }
    }
    problems
}

pub fn check_notes(text: &str, version: &str, previous: Option<&str>) -> R<String> {
    let p = note_problems(text, version, previous);
    if p.is_empty() {
        Ok(text.trim().to_string())
    } else {
        Err(format!("Notas da {version} recusadas:\n  - {}", p.join("\n  - ")))
    }
}

// ---------------------------------------------------------------- manifesto

pub fn installer_name(version: &str) -> String {
    format!("Orquestrador_{version}_x64-setup.exe")
}

pub fn installer_url(version: &str, repo: &str) -> String {
    format!("https://github.com/{repo}/releases/download/v{version}/{}", installer_name(version))
}

fn json_texto(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), String::from)
}

/// Troca a URL da API do asset (rascunho) pela pública e põe as notas completas no campo `notes`.
pub fn prepare_manifest(text: &str, version: &str, notes: &str, repo: &str) -> R<String> {
    let mut m: Value = serde_json::from_str(text).map_err(|e| format!("latest.json inválido: {e}"))?;
    if m["version"] != version {
        return Err(format!("latest.json anuncia {}, não {version}.", json_texto(&m["version"])));
    }
    let keys: Vec<String> = m["platforms"]
        .as_object()
        .map(|o| o.keys().filter(|k| k.starts_with("windows-x86_64")).cloned().collect())
        .unwrap_or_default();
    if keys.is_empty() {
        return Err("latest.json sem plataforma windows-x86_64.".into());
    }
    for k in keys {
        if let Some(p) = m["platforms"][&k].as_object_mut() {
            p.insert("url".into(), installer_url(version, repo).into());
        }
    }
    m["notes"] = canonical_text(notes.trim()).into();
    Ok(format!("{}\n", serde_json::to_string_pretty(&m).unwrap()))
}

pub fn verify_manifest(text: &str, version: &str, sig_text: &str, notes: Option<&str>, repo: &str) -> R<String> {
    let m: Value = serde_json::from_str(text).map_err(|e| format!("latest.json inválido: {e}"))?;
    let mut problems = Vec::new();
    if m["version"] != version {
        problems.push(format!("anuncia {}, não {version}", json_texto(&m["version"])));
    }
    let p = &m["platforms"]["windows-x86_64"];
    if p.is_null() {
        problems.push("sem windows-x86_64".into());
    } else {
        let url = installer_url(version, repo);
        if p["url"] != url.as_str() {
            problems.push(format!("URL {}, esperada {url}", json_texto(&p["url"])));
        }
        if p["signature"] != sig_text.trim() {
            problems.push("a assinatura não é a do .sig enviado".into());
        }
    }
    if let Some(n) = notes {
        if canonical_text(m["notes"].as_str().unwrap_or("")) != canonical_text(n) {
            problems.push("notes do latest.json não são o texto completo de docs/releases".into());
        }
    }
    if problems.is_empty() {
        Ok(format!("latest.json de {version}: URL, assinatura e notes conferidos."))
    } else {
        Err(format!("latest.json recusado:\n  - {}", problems.join("\n  - ")))
    }
}

// ---------------------------------------------------------------- Minisign
// Assinatura Minisign, como o Tauri grava (https://jedisct1.github.io/minisign/). Só verificação.
// Chave pública (pubkey do updater) = base64 de "untrusted comment: …\n" + base64(alg "Ed" + key id 8 + chave 32).
// .sig = base64 de: "untrusted comment: …", base64(alg + key id + assinatura 64), "trusted comment: …" e base64(global 64).
// Alg "ED" assina o BLAKE2b-512 do arquivo (o modo do Tauri); "Ed" assina o arquivo direto. A global cobre a
// assinatura do arquivo seguida do texto do comentário confiável, com a mesma chave.

/// Base64 tolerante como o `Buffer.from(…, 'base64')`: ignora padding e espaços.
fn b64(s: &str) -> Vec<u8> {
    const E: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent).with_decode_allow_trailing_bits(true),
    );
    let limpo: String = s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/').collect();
    E.decode(limpo).unwrap_or_default()
}

fn linhas(t: &str) -> Vec<&str> {
    t.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

pub struct ChaveMinisign {
    pub key_id: [u8; 8],
    pub public_key: [u8; 32],
}

pub fn parse_minisign_key(key_b64: &str) -> R<ChaveMinisign> {
    let text = String::from_utf8_lossy(&b64(key_b64.trim())).into_owned();
    let raw = b64(linhas(&text).get(1).copied().unwrap_or(""));
    if raw.len() != 42 || &raw[..2] != b"Ed" {
        return Err("chave pública do updater não é Minisign Ed25519.".into());
    }
    Ok(ChaveMinisign { key_id: raw[2..10].try_into().unwrap(), public_key: raw[10..42].try_into().unwrap() })
}

pub struct AssinaturaMinisign {
    pub algorithm: [u8; 2],
    pub key_id: [u8; 8],
    pub signature: [u8; 64],
    pub global: [u8; 64],
    pub trusted: String,
}

pub fn parse_minisign_sig(sig_text: &str) -> R<AssinaturaMinisign> {
    let text = String::from_utf8_lossy(&b64(sig_text.trim())).into_owned();
    let l = linhas(&text);
    let (comment, trusted) = (l.first().copied().unwrap_or(""), l.get(2).copied().unwrap_or(""));
    if !comment.starts_with("untrusted comment: ") || !trusted.starts_with("trusted comment: ") {
        return Err("assinatura sem os comentários do formato Minisign.".into());
    }
    let sig = b64(l.get(1).copied().unwrap_or(""));
    let global = b64(l.get(3).copied().unwrap_or(""));
    if sig.len() != 74 || global.len() != 64 {
        return Err("assinatura Minisign com tamanho inválido.".into());
    }
    Ok(AssinaturaMinisign {
        algorithm: sig[..2].try_into().unwrap(),
        key_id: sig[2..10].try_into().unwrap(),
        signature: sig[10..74].try_into().unwrap(),
        global: global.try_into().unwrap(),
        trusted: trusted["trusted comment: ".len()..].to_string(),
    })
}

/// Confere o arquivo contra a chave: key id, assinatura do payload e assinatura global do comentário confiável.
/// Devolve os campos do comentário.
pub fn verify_minisign(key_b64: &str, sig_text: &str, file: &[u8]) -> R<BTreeMap<String, String>> {
    use blake2::Digest;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let key = parse_minisign_key(key_b64)?;
    let sig = parse_minisign_sig(sig_text)?;
    if sig.key_id != key.key_id {
        return Err("assinatura de outra chave: o key id não é o da pubkey do updater.".into());
    }
    let message: Vec<u8> = match &sig.algorithm {
        b"ED" => blake2::Blake2b512::digest(file).to_vec(),
        b"Ed" => file.to_vec(),
        a => return Err(format!("algoritmo de assinatura desconhecido: {}.", String::from_utf8_lossy(a))),
    };
    let nao_confere = || "assinatura do instalador não confere com a chave pública.".to_string();
    let pk = VerifyingKey::from_bytes(&key.public_key).map_err(|_| nao_confere())?;
    pk.verify(&message, &Signature::from_bytes(&sig.signature)).map_err(|_| nao_confere())?;
    let mut global_msg = sig.signature.to_vec();
    global_msg.extend_from_slice(sig.trusted.as_bytes());
    pk.verify(&global_msg, &Signature::from_bytes(&sig.global))
        .map_err(|_| "comentário confiável alterado: a assinatura global não confere.".to_string())?;
    Ok(sig
        .trusted
        .split('\t')
        .map(|f| match f.split_once(':') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (f.to_string(), String::new()),
        })
        .collect())
}

/// Instalador de `version` (nome `file_name`, conteúdo `file`) assinado pela chave pública, com o comentário confiável
/// dizendo o mesmo arquivo e a mesma versão.
pub fn verify_installer(version: &str, file_name: &str, file: &[u8], sig_text: &str, key_b64: &str) -> R<String> {
    check_semver(version)?;
    if file_name != installer_name(version) {
        return Err(format!("o instalador se chama {file_name}, não {}.", installer_name(version)));
    }
    let f = verify_minisign(key_b64, sig_text, file)?;
    let campo = |k: &str| f.get(k).cloned();
    if campo("file").as_deref() != Some(file_name) {
        return Err(format!("o comentário confiável diz o arquivo {}, não {file_name}.", campo("file").unwrap_or("undefined".into())));
    }
    if campo("version").as_deref() != Some(version) {
        return Err(format!("a versão assinada é {}, não {version}.", campo("version").unwrap_or("ausente".into())));
    }
    Ok(format!("{file_name}: assinatura Minisign, key id e comentário confiável conferidos (versão {version})."))
}

// ---------------------------------------------------------------- corpo

/// Só diferença de representação é ignorada: CRLF x LF e espaço final.
pub fn canonical_text(s: &str) -> String {
    s.replace("\r\n", "\n").trim_end().to_string()
}

/// Corpo da release: o GitHub já mostra o nome, então o H1 da fonte não entra. Começa em `## Resumo`.
pub fn release_body(notes: &str) -> String {
    let c = canonical_text(notes);
    let t = c.trim_start();
    let sem_h1 = match (t.starts_with("# "), t.find('\n')) {
        (true, Some(i)) => &t[i + 1..],
        _ => t,
    };
    sem_h1.trim_start().to_string()
}

pub fn verify_body(body: &str, notes: &str) -> R<String> {
    if canonical_text(body) != release_body(notes) {
        return Err("o corpo da release não é o texto completo de docs/releases, sem o título.".into());
    }
    Ok("corpo da release igual às notas, sem o título.".into())
}

// ---------------------------------------------------------------- comandos

const USO: &str = "comandos: wait-ci <SHA>, check-versions [X.Y.Z], check-notes X.Y.Z [anterior], prepare-manifest X.Y.Z latest.json notas.md, verify-manifest X.Y.Z latest.json arquivo.sig [notas.md], verify-installer X.Y.Z instalador.exe arquivo.sig [pubkey], release-body X.Y.Z corpo.md, verify-body corpo.md notas.md";

/// Pubkey do updater: o arquivo dado ou a do `tauri.conf.json` enquanto ele existir.
fn pubkey(root: &Path, arquivo: Option<&String>) -> R<String> {
    if let Some(a) = arquivo {
        return std::fs::read_to_string(a).map(|s| s.trim().to_string()).map_err(|e| format!("{a}: {e}"));
    }
    let conf = root.join("src-tauri/tauri.conf.json");
    let texto = std::fs::read_to_string(&conf).map_err(|_| "sem src-tauri/tauri.conf.json: passe o arquivo da pubkey.".to_string())?;
    let v: Value = serde_json::from_str(&texto).map_err(|e| e.to_string())?;
    v["plugins"]["updater"]["pubkey"].as_str().map(String::from).ok_or("tauri.conf.json sem plugins.updater.pubkey.".into())
}

pub fn run(root: &Path, args: &[String]) -> R<String> {
    let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
    let arg = |i: usize| args.get(i).map(String::as_str);
    let precisa = |n: usize| if args.len() > n { Ok(()) } else { Err(USO.to_string()) };
    match arg(0) {
        Some("wait-ci") => {
            if args.len() != 2 {
                return Err("uso: wait-ci <SHA de git rev-parse HEAD>".into());
            }
            wait_for_ci(&args[1], Ci::real())
        }
        Some("check-versions") => {
            let v = manifest_versions(root)?;
            let expected = match arg(1) {
                Some(e) => e.to_string(),
                None => v
                    .get("package.json")
                    .or_else(|| v.get("Cargo.toml ([workspace.package])"))
                    .cloned()
                    .flatten()
                    .unwrap_or_default(),
            };
            check_versions(&v, &expected)?;
            Ok(format!("Versões {expected} sincronizadas em todos os manifests."))
        }
        Some("check-notes") => {
            precisa(1)?;
            let version = &args[1];
            let path = root.join(format!("docs/releases/v{version}.md"));
            let text = std::fs::read_to_string(&path)
                .map_err(|_| format!("Notas ausentes: {} não existe. A release não sai sem elas.", path.display()))?;
            check_notes(&text, version, arg(2))?;
            Ok(format!("Notas da v{version} completas."))
        }
        Some("prepare-manifest") => {
            precisa(3)?;
            let out = prepare_manifest(&read(&args[2])?, &args[1], &read(&args[3])?, REPO)?;
            std::fs::write(&args[2], out).map_err(|e| e.to_string())?;
            Ok(format!("latest.json de {} apontando para a URL pública, com as notas.", args[1]))
        }
        Some("verify-manifest") => {
            precisa(3)?;
            let notes = args.get(4).map(|p| read(p)).transpose()?;
            verify_manifest(&read(&args[2])?, &args[1], &read(&args[3])?, notes.as_deref(), REPO)
        }
        Some("verify-installer") => {
            precisa(3)?;
            let exe = Path::new(&args[2]);
            let nome = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let file = std::fs::read(exe).map_err(|e| format!("{}: {e}", args[2]))?;
            verify_installer(&args[1], &nome, &file, &read(&args[3])?, &pubkey(root, args.get(4))?)
        }
        Some("release-body") => {
            precisa(2)?;
            check_semver(&args[1])?;
            let notas = std::fs::read_to_string(root.join(format!("docs/releases/v{}.md", args[1]))).map_err(|e| e.to_string())?;
            std::fs::write(&args[2], release_body(&notas)).map_err(|e| e.to_string())?;
            Ok(format!("Corpo da release {} gravado em {}, sem o título.", args[1], args[2]))
        }
        Some("verify-body") => {
            precisa(2)?;
            verify_body(&read(&args[1])?, &read(&args[2])?)
        }
        _ => Err(USO.into()),
    }
}

#[cfg(test)]
#[path = "release_testes.rs"]
mod testes;
