//! Configuração: `%LOCALAPPDATA%\orquestrador\config.json`, as validações e a lista de modelos.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const TOOLS: [&str; 2] = ["claude", "codex"];

/// Esforços aceitos por ferramenta, conferidos nos CLIs instalados (`claude --help`, união de
/// `codex debug models`). Vazio mantém o padrão do CLI.
pub fn efforts(tool: &str) -> Option<&'static [&'static str]> {
    match tool {
        "claude" => Some(&["low", "medium", "high", "xhigh", "max"]),
        "codex" => Some(&["low", "medium", "high", "xhigh", "max", "ultra"]),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub tool: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: String,
}

impl Choice {
    pub fn new(tool: &str, model: &str, effort: &str) -> Self {
        Self { tool: tool.into(), model: model.into(), effort: effort.into() }
    }

    /// Modelo e esforço como argumentos literais do CLI, só para o que está definido.
    pub fn cli_args(&self) -> Vec<String> {
        let (model_flag, effort) = match self.tool.as_str() {
            "codex" => ("-m", ["-c".to_string(), format!("model_reasoning_effort={}", self.effort)]),
            _ => ("--model", ["--effort".to_string(), self.effort.clone()]),
        };
        let mut args = Vec::new();
        if !self.model.is_empty() {
            args.extend([model_flag.to_string(), self.model.clone()]);
        }
        if !self.effort.is_empty() {
            args.extend(effort);
        }
        args
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub orchestrator: Choice,
    pub executor: Choice,
    pub max_parallel: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { orchestrator: Choice::new("codex", "", ""), executor: Choice::new("claude", "", ""), max_parallel: 4 }
    }
}

// Começar com hífen seria lido como outra opção do CLI; espaço e aspas quebram o argumento.
fn model_ok(model: &str) -> bool {
    let mut chars = model.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "._:/@+[]-".contains(c))
}

/// Erros de um papel (vazio = válido).
pub fn check_role(role: &str, c: &Choice) -> Vec<String> {
    let Some(valid) = efforts(&c.tool) else {
        return vec![format!("{role}: ferramenta '{}' desconhecida (válidas: {})", c.tool, TOOLS.join(", "))];
    };
    let mut errors = Vec::new();
    if !c.model.is_empty() && !model_ok(&c.model) {
        errors.push(format!(
            "{role}: modelo '{}' inválido (use letras, dígitos e . _ : / @ + [ ] -, sem espaço, aspas nem iniciar com '-')",
            c.model
        ));
    }
    if !c.effort.is_empty() && !valid.contains(&c.effort.as_str()) {
        errors.push(format!(
            "{role}: esforço '{}' inválido para {} (válidos: {}; alguns modelos aceitam só parte; vazio mantém o padrão do CLI)",
            c.effort,
            c.tool,
            valid.join(", ")
        ));
    }
    errors
}

pub fn check_choice(role: &str, c: &Choice) -> Result<(), String> {
    let errors = check_role(role, c);
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

pub fn check_max_parallel(value: i64) -> Result<usize, String> {
    if (1..=16).contains(&value) { Ok(value as usize) } else { Err(format!("maxParallel: {value} fora do intervalo de 1 a 16")) }
}

pub fn config_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .ok_or("LOCALAPPDATA não está definido; não há onde guardar a configuração.")?;
    Ok(PathBuf::from(base).join("orquestrador").join("config.json"))
}

/// Os padrões com o que o arquivo define por cima, validado. Sem arquivo, os padrões. Arquivo
/// ilegível ou resultado inválido é erro, com o motivo: não há fallback.
pub fn read(path: &Path) -> Result<Config, String> {
    let mut cfg = Config::default();
    if path.is_file() {
        let unreadable = |e: String| format!("Configuração inválida (JSON ilegível) em {}: {e}", path.display());
        let text = std::fs::read_to_string(path).map_err(|e| unreadable(e.to_string()))?;
        let file: serde_json::Value =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| unreadable(e.to_string()))?;
        for (role, choice) in [("orchestrator", &mut cfg.orchestrator), ("executor", &mut cfg.executor)] {
            for (key, slot) in [("tool", &mut choice.tool), ("model", &mut choice.model), ("effort", &mut choice.effort)] {
                match &file[role][key] {
                    serde_json::Value::String(s) if !s.is_empty() => *slot = s.clone(),
                    serde_json::Value::Null | serde_json::Value::String(_) => {}
                    other => *slot = other.to_string(),
                }
            }
        }
        match &file["maxParallel"] {
            serde_json::Value::Null => {}
            v => cfg.max_parallel = check_max_parallel(v.as_i64().unwrap_or(-1)).map_err(|e| invalid(path, &e))?,
        }
    }
    let errors: Vec<String> =
        [check_role("orchestrator", &cfg.orchestrator), check_role("executor", &cfg.executor)].concat();
    if errors.is_empty() { Ok(cfg) } else { Err(invalid(path, &errors.join("; "))) }
}

fn invalid(path: &Path, why: &str) -> String {
    format!("Configuração inválida em {}: {why}", path.display())
}

pub fn save(path: &Path, cfg: &Config) -> Result<(), String> {
    let write = || {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(path, serde_json::to_string_pretty(cfg).unwrap())
    };
    write().map_err(|e| format!("Não foi possível gravar a configuração em {}: {e}", path.display()))
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelOption {
    pub name: String,
    pub efforts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolOption {
    pub name: String,
    pub efforts: Vec<String>,
    pub models: Vec<ModelOption>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Last {
    pub orchestrator: Choice,
    pub executor: Choice,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub tools: Vec<ToolOption>,
    pub last: Last,
    pub max_parallel: usize,
}

/// A saída de `codex debug models` -> modelos visíveis, cada um com os esforços que aceita.
pub fn parse_codex_models(json: &str) -> Vec<ModelOption> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let models = v["models"].as_array().cloned().unwrap_or_default();
    models
        .iter()
        .filter(|m| m["visibility"] == "list")
        .filter_map(|m| {
            let levels = m["supported_reasoning_levels"].as_array().cloned().unwrap_or_default();
            Some(ModelOption {
                name: m["slug"].as_str()?.to_string(),
                efforts: levels.iter().filter_map(|l| l["effort"].as_str().map(String::from)).collect(),
            })
        })
        .collect()
}

// O Claude não tem comando que liste modelos: os apelidos resolvem para o mais novo de cada
// família. O Codex é perguntado; se não responder, a lista fica vazia (o nome ainda pode ser
// digitado) e a pergunta é refeita na próxima vez.
static CODEX_MODELS: Mutex<Vec<ModelOption>> = Mutex::new(Vec::new());

pub fn models(tool: &str, codex_exe: &str) -> Vec<ModelOption> {
    let all = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if tool == "claude" {
        let efforts = all(efforts("claude").unwrap());
        return all(&["fable", "opus", "sonnet", "haiku"]).into_iter().map(|name| ModelOption { name, efforts: efforts.clone() }).collect();
    }
    let mut cache = CODEX_MODELS.lock().unwrap();
    if cache.is_empty() {
        if let Ok(out) = crate::proc::quiet(codex_exe).args(["debug", "models"]).output() {
            *cache = parse_codex_models(&String::from_utf8_lossy(&out.stdout));
        }
    }
    cache.clone()
}

pub fn options(cfg: &Config, codex_exe: &str) -> Options {
    Options {
        tools: TOOLS
            .iter()
            .map(|t| ToolOption {
                name: t.to_string(),
                efforts: efforts(t).unwrap().iter().map(|s| s.to_string()).collect(),
                models: models(t, codex_exe),
            })
            .collect(),
        last: Last { orchestrator: cfg.orchestrator.clone(), executor: cfg.executor.clone() },
        max_parallel: cfg.max_parallel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orq-cfg-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

    fn drop_dir(config: &Path) {
        let _ = std::fs::remove_dir_all(config.parent().unwrap());
    }

    #[test]
    fn sem_arquivo_valem_os_padroes() {
        let path = temp("none");
        let cfg = read(&path).unwrap();
        drop_dir(&path);
        assert_eq!(cfg, Config::default());
        assert_eq!((cfg.orchestrator.tool.as_str(), cfg.executor.tool.as_str(), cfg.max_parallel), ("codex", "claude", 4));
    }

    #[test]
    fn arquivo_sobrepoe_os_padroes_e_volta_igual_depois_de_salvo() {
        let path = temp("over");
        std::fs::write(&path, r#"{"orchestrator":{"tool":"claude","model":"opus","effort":"high"},"maxParallel":7}"#).unwrap();
        let cfg = read(&path).unwrap();
        assert_eq!(cfg.orchestrator, Choice::new("claude", "opus", "high"));
        assert_eq!(cfg.executor, Choice::new("claude", "", ""));
        assert_eq!(cfg.max_parallel, 7);
        save(&path, &cfg).unwrap();
        assert_eq!(read(&path).unwrap(), cfg);
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"maxParallel\": 7"));
        drop_dir(&path);
    }

    #[test]
    fn arquivo_ilegivel_e_erro_sem_fallback() {
        let path = temp("bad");
        std::fs::write(&path, "{ isto nao e json").unwrap();
        assert!(read(&path).unwrap_err().contains("JSON ilegível"));
        drop_dir(&path);
    }

    #[test]
    fn valores_invalidos_sao_recusados_com_o_motivo() {
        let path = temp("invalid");
        for (json, why) in [
            (r#"{"executor":{"tool":"gemini"}}"#, "ferramenta 'gemini' desconhecida"),
            (r#"{"executor":{"tool":"claude","effort":"ultra"}}"#, "esforço 'ultra' inválido para claude"),
            (r#"{"executor":{"tool":"claude","effort":"High"}}"#, "esforço 'High' inválido"),
            (r#"{"orchestrator":{"tool":"codex","model":"-m x"}}"#, "modelo '-m x' inválido"),
            (r#"{"maxParallel":0}"#, "fora do intervalo"),
            (r#"{"maxParallel":"muitos"}"#, "fora do intervalo"),
        ] {
            std::fs::write(&path, json).unwrap();
            let err = read(&path).unwrap_err();
            assert!(err.contains(why), "{json}: {err}");
        }
        drop_dir(&path);
    }

    #[test]
    fn padrao_do_nome_do_modelo() {
        for ok in ["opus", "gpt-5.5", "claude-opus-4-1[1m]", "org/model:tag@v1+x", "a"] {
            assert!(model_ok(ok), "{ok}");
        }
        for bad in ["", "-opus", "o pus", "o\"pus", "o'pus", "ópus", ".x"] {
            assert!(!model_ok(bad), "{bad}");
        }
    }

    #[test]
    fn argumentos_de_modelo_e_esforco_por_ferramenta() {
        assert_eq!(Choice::new("claude", "opus", "high").cli_args(), ["--model", "opus", "--effort", "high"]);
        assert_eq!(Choice::new("codex", "gpt-5.5", "xhigh").cli_args(), ["-m", "gpt-5.5", "-c", "model_reasoning_effort=xhigh"]);
        assert_eq!(Choice::new("codex", "", "low").cli_args(), ["-c", "model_reasoning_effort=low"]);
        assert!(Choice::new("claude", "", "").cli_args().is_empty());
    }

    #[test]
    fn modelos_do_codex_so_os_visiveis() {
        let json = r#"{"models":[
            {"slug":"gpt-a","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]},
            {"slug":"interno","visibility":"hide","supported_reasoning_levels":[]}]}"#;
        let models = parse_codex_models(json);
        assert_eq!(models.len(), 1);
        assert_eq!((models[0].name.as_str(), models[0].efforts.join(",")), ("gpt-a", "low,high".to_string()));
        assert!(parse_codex_models("nao e json").is_empty());
    }
}
