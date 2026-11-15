//! Descoberta e execução dos formatadores do editor, sem depender do projeto aberto pelo app.
use serde::Serialize;
use serde_json::{Map, Value};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Engine {
    Prettier,
    Python,
    Rust,
    Go,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Info {
    Prettier {
        config: Option<String>,
        #[serde(rename = "configKind")]
        config_kind: &'static str,
        options: Map<String, Value>,
    },
    Cli { formatter: &'static str },
    Unavailable { reason: String },
}

fn engine(path: &Path) -> Option<Engine> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "css" | "scss" | "less" | "html" | "htm" | "vue"
        | "json" | "jsonc" | "json5" | "md" | "markdown" | "yaml" | "yml" => Some(Engine::Prettier),
        "py" => Some(Engine::Python),
        "rs" => Some(Engine::Rust),
        "go" => Some(Engine::Go),
        _ => None,
    }
}

fn validate_path(path: &Path) -> Result<&Path, String> {
    if !path.is_absolute() {
        return Err("O caminho do arquivo precisa ser absoluto.".into());
    }
    let parent = path.parent().filter(|dir| dir.is_dir()).ok_or_else(|| "A pasta do arquivo não existe.".to_string())?;
    if !path.is_file() {
        return Err(format!("Arquivo não encontrado: {}", path.display()));
    }
    Ok(parent)
}

fn executable_in_path(name: &str) -> bool {
    let Some(path) = env::var_os("PATH") else { return false };
    env::split_paths(&path).any(|dir| {
        if dir.join(name).is_file() {
            return true;
        }
        #[cfg(windows)]
        return [".exe", ".com"].iter().any(|ext| dir.join(format!("{name}{ext}")).is_file());
        #[cfg(not(windows))]
        false
    })
}

#[derive(Debug, PartialEq)]
enum Config {
    /// Texto, tipo e pasta onde o arquivo de configuração foi achado (vazia sem configuração).
    Found(Option<String>, &'static str, PathBuf),
    /// Configuração em JS, TS, JSON5 ou TOML: o Prettier do editor não a lê, e formatar sem ela mudaria o estilo.
    Unsupported(PathBuf),
}

const UNSUPPORTED_CONFIGS: [&str; 14] = [
    ".prettierrc.js", ".prettierrc.cjs", ".prettierrc.mjs", ".prettierrc.ts", ".prettierrc.cts", ".prettierrc.mts",
    ".prettierrc.json5", ".prettierrc.toml",
    "prettier.config.js", "prettier.config.cjs", "prettier.config.mjs", "prettier.config.ts", "prettier.config.cts",
    "prettier.config.mts",
];

/// Mesma ordem do Prettier 3 em cada pasta: `package.json` com a chave `prettier` vence os `.prettierrc*`.
fn config_file(path: &Path) -> Result<Config, String> {
    for dir in path.ancestors() {
        let package = dir.join("package.json");
        if package.is_file() {
            let text = std::fs::read_to_string(&package).map_err(|e| format!("Não foi possível ler {}: {e}", package.display()))?;
            let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .map_err(|e| format!("{}: {e}", package.display()))?;
            if let Some(config) = value.get("prettier").filter(|config| !config.is_null()) {
                return Ok(Config::Found(Some(config.to_string()), "json", dir.to_path_buf()));
            }
        }
        for (name, kind) in [
            (".prettierrc", "auto"),
            (".prettierrc.json", "json"),
            (".prettierrc.yml", "yaml"),
            (".prettierrc.yaml", "yaml"),
        ] {
            let file = dir.join(name);
            if file.is_file() {
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| format!("Não foi possível ler {}: {e}", file.display()))?;
                return Ok(Config::Found(Some(text), kind, dir.to_path_buf()));
            }
        }
        if let Some(file) = UNSUPPORTED_CONFIGS.iter().map(|name| dir.join(name)).find(|file| file.is_file()) {
            return Ok(Config::Unsupported(file));
        }
    }
    Ok(Config::Found(None, "json", PathBuf::new()))
}

enum Prepared {
    Ready(Option<String>, &'static str),
    /// `overrides` em YAML: sem parser YAML no backend, o casamento não é possível.
    YamlOverrides,
}

fn yaml_has_overrides(text: &str) -> bool {
    text.lines().any(|line| {
        let key = line.split(':').next().unwrap_or("");
        line.contains(':') && !key.starts_with(char::is_whitespace) && key.trim_matches(['"', '\'']) == "overrides"
    })
}

fn patterns(value: Option<&Value>) -> Vec<&str> {
    match value {
        Some(Value::String(pattern)) => vec![pattern.as_str()],
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// Aplica, em ordem, os `overrides` que casam com o arquivo (glob relativo à pasta da configuração) e remove a chave.
fn apply_overrides(text: String, kind: &'static str, dir: &Path, file: &Path) -> Prepared {
    let Ok(Value::Object(mut config)) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
        return if kind != "json" && yaml_has_overrides(&text) { Prepared::YamlOverrides } else { Prepared::Ready(Some(text), kind) };
    };
    let Some(overrides) = config.remove("overrides") else { return Prepared::Ready(Some(text), kind) };
    let relative = file.strip_prefix(dir).unwrap_or(file).to_string_lossy().replace('\\', "/");
    for item in overrides.as_array().into_iter().flatten().filter_map(Value::as_object) {
        let matches = |key: &str| patterns(item.get(key)).into_iter().any(|pattern| override_matches(pattern, &relative));
        if matches("files") && !matches("excludeFiles") {
            if let Some(options) = item.get("options").and_then(Value::as_object) {
                config.extend(options.clone());
            }
        }
    }
    Prepared::Ready(Some(Value::Object(config).to_string()), "json")
}

/// Primeiro `.prettierignore` acima do arquivo; devolve o caminho dele se casa o arquivo.
fn prettier_ignore(file: &Path) -> Result<Option<PathBuf>, String> {
    for dir in file.ancestors().skip(1) {
        let ignore_file = dir.join(".prettierignore");
        if !ignore_file.is_file() { continue; }
        let mut builder = ignore::gitignore::GitignoreBuilder::new(dir);
        // Linhas inválidas são descartadas, como faz o Prettier.
        let _ = builder.add(&ignore_file);
        let matcher = builder.build().map_err(|e| format!("{}: {e}", ignore_file.display()))?;
        return Ok(matcher.matched_path_or_any_parents(file, false).is_ignore().then_some(ignore_file));
    }
    Ok(None)
}

/// Troca cada `{a,b}` pelas alternativas; chaves sem vírgula ficam literais.
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else { return vec![pattern.to_string()] };
    let mut depth = 0;
    let mut commas = Vec::new();
    let mut close = None;
    for (i, c) in pattern.char_indices().skip_while(|(i, _)| *i < open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 { close = Some(i); break; }
            }
            ',' if depth == 1 => commas.push(i),
            _ => {}
        }
    }
    let Some(close) = close else { return vec![pattern.to_string()] };
    let suffix = &pattern[close + 1..];
    if commas.is_empty() {
        return expand_braces(suffix).into_iter().map(|tail| format!("{}{tail}", &pattern[..=close])).collect();
    }
    let mut bounds = vec![open];
    bounds.extend(commas);
    bounds.push(close);
    bounds.windows(2)
        .flat_map(|w| expand_braces(&format!("{}{}{suffix}", &pattern[..open], &pattern[w[0] + 1..w[1]])))
        .collect()
}

/// `*`, `**` e `?` sobre um nome; `fold` ignora maiúsculas (.editorconfig), o Prettier não ignora.
fn glob_matches(pattern: &[char], name: &[char], fold: bool) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|skip| glob_matches(rest, &name[skip..], fold)),
        Some(('?', rest)) => !name.is_empty() && glob_matches(rest, &name[1..], fold),
        Some((c, rest)) => {
            name.first().is_some_and(|n| if fold { n.eq_ignore_ascii_case(c) } else { n == c }) && glob_matches(rest, &name[1..], fold)
        }
    }
}

fn segment_matches(pattern: &str, name: &str) -> bool {
    let mut chars: Vec<char> = pattern.chars().collect();
    chars.dedup_by(|a, b| *a == '*' && *b == '*');
    glob_matches(&chars, &name.chars().collect::<Vec<_>>(), false)
}

fn segments_match(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| segments_match(rest, &path[skip..])),
        Some((segment, rest)) => path
            .split_first()
            .is_some_and(|(name, tail)| segment_matches(segment, name) && segments_match(rest, tail)),
    }
}

/// Glob de `overrides` como no Prettier (micromatch, `dot: true`): sem barra casa o nome base, com barra o caminho relativo à configuração.
fn override_matches(pattern: &str, relative: &str) -> bool {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    expand_braces(pattern).iter().any(|pattern| {
        if !pattern.contains('/') {
            return segment_matches(pattern, relative.rsplit('/').next().unwrap_or(relative));
        }
        segments_match(&pattern.split('/').collect::<Vec<_>>(), &relative.split('/').collect::<Vec<_>>())
    })
}

fn section_matches(section: &str, name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    expand_braces(section).iter().any(|pattern| {
        let pattern = pattern.strip_prefix("**/").unwrap_or(pattern);
        // Padrões com pasta dependem do caminho relativo ao .editorconfig, que não é comparado aqui.
        if pattern.contains('/') { return false; }
        let mut chars: Vec<char> = pattern.chars().collect();
        chars.dedup_by(|a, b| *a == '*' && *b == '*');
        glob_matches(&chars, &name, true)
    })
}

fn set_editor_option(options: &mut Map<String, Value>, key: &str, value: &str) {
    let value = value.trim().to_ascii_lowercase();
    match key {
        "indent_style" => match value.as_str() {
            "tab" => { options.insert("useTabs".into(), Value::Bool(true)); }
            "space" => { options.insert("useTabs".into(), Value::Bool(false)); }
            "unset" => { options.remove("useTabs"); }
            _ => {}
        },
        "indent_size" if value == "unset" => { options.remove("tabWidth"); }
        "indent_size" if value == "tab" => {
            if !options.contains_key("tabWidth") { options.insert("tabWidth".into(), Value::from(4)); }
        }
        "indent_size" if value != "tab" => {
            if let Ok(width) = value.parse::<u64>() { options.insert("tabWidth".into(), Value::from(width)); }
        }
        "tab_width" => {
            if value == "unset" { options.remove("tabWidth"); }
            else if let Ok(width) = value.parse::<u64>() { options.insert("tabWidth".into(), Value::from(width)); }
        }
        "end_of_line" if ["lf", "crlf", "cr"].contains(&value.as_str()) => {
            options.insert("endOfLine".into(), Value::String(value));
        }
        "end_of_line" if value == "unset" => { options.remove("endOfLine"); }
        "max_line_length" => {
            if value == "unset" { options.remove("printWidth"); }
            else if let Ok(width) = value.parse::<u64>() { options.insert("printWidth".into(), Value::from(width)); }
        }
        _ => {}
    }
}

fn editorconfig_file(text: &str, file: &Path, options: &mut Map<String, Value>) {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let mut section: Option<String> = None;
    let mut properties = Vec::new();
    let apply = |properties: &[(String, String)], options: &mut Map<String, Value>| {
        for key in ["indent_style", "end_of_line", "max_line_length"] {
            if let Some((_, value)) = properties.iter().rev().find(|(name, _)| name == key) {
                set_editor_option(options, key, value);
            }
        }
        let size = properties.iter().rev().find(|(key, _)| key == "indent_size").map(|(_, value)| value.as_str());
        let width = properties.iter().rev().find(|(key, _)| key == "tab_width").map(|(_, value)| value.as_str());
        match size {
            Some(size) if size.eq_ignore_ascii_case("tab") => {
                if let Some(width) = width { set_editor_option(options, "tab_width", width); }
                else if !options.contains_key("tabWidth") { options.insert("tabWidth".into(), Value::from(4)); }
            }
            Some(size) if size.eq_ignore_ascii_case("unset") => {
                if let Some(width) = width { set_editor_option(options, "tab_width", width); }
                else { options.remove("tabWidth"); }
            }
            Some(size) => {
                if size.parse::<u64>().is_ok() { set_editor_option(options, "indent_size", size); }
                else if let Some(width) = width { set_editor_option(options, "tab_width", width); }
            }
            None => {
                if let Some(width) = width { set_editor_option(options, "tab_width", width); }
            }
        }
    };
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(pattern) = section.take() {
                if section_matches(&pattern, name) { apply(&properties, options); }
                properties.clear();
            }
            section = Some(line[1..line.len() - 1].trim().to_string());
        } else if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if section.is_some() {
                properties.push((key, value));
            }
        }
    }
    if let Some(pattern) = section {
        if section_matches(&pattern, name) { apply(&properties, options); }
    }
}

fn editorconfig_root(text: &str) -> bool {
    let mut preamble = true;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') { continue; }
        if line.starts_with('[') && line.ends_with(']') { preamble = false; }
        else if preamble {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim().eq_ignore_ascii_case("root") && value.trim().eq_ignore_ascii_case("true") { return true; }
            }
        }
    }
    false
}

fn editorconfig_options(directory: &Path, file: &Path) -> Result<Map<String, Value>, String> {
    let mut configs = Vec::new();
    for dir in directory.ancestors() {
        let config = dir.join(".editorconfig");
        if config.is_file() {
            let content = std::fs::read_to_string(&config)
                .map_err(|e| format!("Não foi possível ler {}: {e}", config.display()))?;
            let root = editorconfig_root(&content);
            configs.push(content);
            if root { break; }
        }
    }
    let mut options = Map::new();
    for content in configs.iter().rev() {
        editorconfig_file(content, file, &mut options);
    }
    Ok(options)
}

pub fn info(path: &Path) -> Result<Info, String> {
    let directory = validate_path(path)?;
    match engine(path) {
        Some(Engine::Prettier) => {
            if let Some(ignore) = prettier_ignore(path)? {
                return Ok(Info::Unavailable { reason: format!("Arquivo ignorado por {}", ignore.display()) });
            }
            match config_file(directory)? {
                Config::Found(config, config_kind, dir) => {
                    let (config, config_kind) = match config.map(|text| apply_overrides(text, config_kind, &dir, path)) {
                        Some(Prepared::Ready(config, kind)) => (config, kind),
                        Some(Prepared::YamlOverrides) => {
                            return Ok(Info::Unavailable { reason: "`overrides` em configuração YAML do Prettier não é suportado".into() });
                        }
                        None => (None, config_kind),
                    };
                    Ok(Info::Prettier { config, config_kind, options: editorconfig_options(directory, path)? })
                }
                Config::Unsupported(file) => Ok(Info::Unavailable {
                    reason: format!("Configuração do Prettier em JS, TS, JSON5 ou TOML não é suportada: {}", file.display()),
                }),
            }
        }
        Some(Engine::Python) if executable_in_path("ruff") => Ok(Info::Cli { formatter: "ruff" }),
        Some(Engine::Python) if executable_in_path("black") => Ok(Info::Cli { formatter: "black" }),
        Some(Engine::Python) => Ok(Info::Unavailable { reason: "ruff ou black não encontrado no PATH".into() }),
        Some(Engine::Rust) if executable_in_path("rustfmt") => Ok(Info::Cli { formatter: "rustfmt" }),
        Some(Engine::Rust) => Ok(Info::Unavailable { reason: "rustfmt não encontrado no PATH".into() }),
        Some(Engine::Go) if executable_in_path("gofmt") => Ok(Info::Cli { formatter: "gofmt" }),
        Some(Engine::Go) => Ok(Info::Unavailable { reason: "gofmt não encontrado no PATH".into() }),
        None => {
            let ext = path.extension().and_then(|e| e.to_str()).map(|e| format!(".{e}")).unwrap_or_else(|| "sem extensão".into());
            Ok(Info::Unavailable { reason: format!("Nenhum formatador para {ext}") })
        }
    }
}

/// Pares chave e valor de uma seção do TOML, com a chave sem espaços (`edition . workspace` vira `edition.workspace`).
fn toml_entries<'a>(text: &'a str, section: &str) -> Vec<(String, &'a str)> {
    let mut inside = false;
    let mut entries = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            inside = line == section;
        } else if inside {
            if let Some((key, value)) = line.split_once('=') {
                entries.push((key.split_whitespace().collect(), value.trim()));
            }
        }
    }
    entries
}

fn edition_value(value: &str) -> Option<String> {
    let edition = value.trim_matches('"').trim_matches('\'');
    (!edition.is_empty()).then(|| edition.to_string())
}

fn workspace_edition(directory: &Path) -> Option<String> {
    for dir in directory.ancestors() {
        let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) else { continue };
        if !text.lines().map(str::trim).any(|line| line == "[workspace]" || line.starts_with("[workspace.")) { continue; }
        return toml_entries(&text, "[workspace.package]").into_iter()
            .find(|(key, _)| key == "edition")
            .and_then(|(_, value)| edition_value(value));
    }
    None
}

fn cargo_edition(directory: &Path) -> String {
    for dir in directory.ancestors() {
        let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) else { continue };
        for (key, value) in toml_entries(&text, "[package]") {
            let edition = match key.as_str() {
                "edition.workspace" => workspace_edition(dir),
                "edition" if value.starts_with('{') => value.contains("workspace").then(|| workspace_edition(dir)).flatten(),
                "edition" => edition_value(value),
                _ => continue,
            };
            if let Some(edition) = edition { return edition; }
        }
        break;
    }
    "2021".into()
}

fn run(command: Command, input: &str, timeout: Duration) -> Result<String, String> {
    let saida = crate::proc::saida_com_prazo(command, input.as_bytes(), timeout).map_err(|falha| {
        use crate::proc::FalhaPrazo;
        match falha {
            FalhaPrazo::Iniciar(e) => format!("O formatador não pôde iniciar: {e}"),
            FalhaPrazo::Aguardar(e) => format!("Não foi possível aguardar o formatador: {e}"),
            FalhaPrazo::Ler(e) => format!("Não foi possível ler a saída do formatador: {e}"),
        }
    })?;
    let stderr = String::from_utf8_lossy(&saida.err).trim().to_string();
    if saida.estourou {
        let detail = if stderr.is_empty() { String::new() } else { format!(" {stderr}") };
        return Err(format!("O formatador excedeu o limite de {} s.{detail}", timeout.as_secs()));
    }
    if !saida.status.success() {
        return Err(if stderr.is_empty() { format!("O formatador falhou (código {}).", saida.status.code().unwrap_or(-1)) }
            else { format!("O formatador falhou: {stderr}") });
    }
    String::from_utf8(saida.out).map_err(|e| format!("O formatador devolveu texto que não está em UTF-8: {e}"))
}

pub fn format_cli(path: &Path, text: &str) -> Result<String, String> {
    let directory = validate_path(path)?;
    let mut command = match engine(path) {
        Some(Engine::Python) if executable_in_path("ruff") => {
            let mut cmd = crate::proc::quiet("ruff");
            cmd.args(["format", "--stdin-filename"]).arg(path).arg("-");
            cmd
        }
        Some(Engine::Python) if executable_in_path("black") => {
            let mut cmd = crate::proc::quiet("black");
            cmd.arg("-q").arg("--stdin-filename").arg(path).arg("-");
            cmd
        }
        Some(Engine::Rust) if executable_in_path("rustfmt") => {
            let mut cmd = crate::proc::quiet("rustfmt");
            cmd.args(["--emit", "stdout", "--edition"]).arg(cargo_edition(directory));
            cmd
        }
        Some(Engine::Go) if executable_in_path("gofmt") => crate::proc::quiet("gofmt"),
        Some(Engine::Python) => return Err("ruff ou black não encontrado no PATH".into()),
        Some(Engine::Rust) => return Err("rustfmt não encontrado no PATH".into()),
        Some(Engine::Go) => return Err("gofmt não encontrado no PATH".into()),
        _ => return Err("Nenhum formatador de linha de comando para esta extensão.".into()),
    };
    command.current_dir(directory);
    run(command, text, TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{write, Tmp};
    use serde_json::json;
    use std::time::Instant;

    #[test]
    fn escolhe_motor_pela_extensao() {
        for ext in ["js", "tsx", "vue", "jsonc", "scss", "yaml", "md"] {
            assert_eq!(engine(Path::new(&format!("arquivo.{ext}"))), Some(Engine::Prettier), ".{ext}");
        }
        assert_eq!(engine(Path::new("script.PY")), Some(Engine::Python));
        assert_eq!(engine(Path::new("lib.rs")), Some(Engine::Rust));
        assert_eq!(engine(Path::new("main.go")), Some(Engine::Go));
        assert_eq!(engine(Path::new("settings.toml")), None);
    }

    #[test]
    fn le_editorconfig_e_aplica_secoes_e_valores() {
        let tmp = Tmp::new("orq-format", "editorconfig");
        write(&tmp.0, ".editorconfig", "root = true\n[*]\nindent_style = space\nindent_size = 4\nend_of_line = crlf\nmax_line_length = 100\n[*.{md,markdown}]\nindent_size = 2\n");
        let options = editorconfig_options(&tmp.0, &tmp.0.join("README.md")).unwrap();
        assert_eq!(options.get("useTabs"), Some(&json!(false)));
        assert_eq!(options.get("tabWidth"), Some(&json!(2)));
        assert_eq!(options.get("endOfLine"), Some(&json!("crlf")));
        assert_eq!(options.get("printWidth"), Some(&json!(100)));
        let json_options = editorconfig_options(&tmp.0, &tmp.0.join("data.json")).unwrap();
        assert_eq!(json_options.get("tabWidth"), Some(&json!(4)));
    }

    #[test]
    fn root_editorconfig_so_interrompe_no_preambulo() {
        let tmp = Tmp::new("orq-format", "editorconfig-root");
        let nested = tmp.0.join("src");
        std::fs::create_dir_all(&nested).unwrap();
        write(&tmp.0, ".editorconfig", "[*]\nindent_size = 4\nmax_line_length = 80\n");
        write(&nested, ".editorconfig", "[*]\nroot = true\nindent_size = 2\n");
        let options = editorconfig_options(&nested, &nested.join("file.js")).unwrap();
        assert_eq!(options.get("tabWidth"), Some(&json!(2)));
        assert_eq!(options.get("printWidth"), Some(&json!(80)));

        write(&nested, ".editorconfig", "root = true\n[*]\nindent_size = 2\n");
        let options = editorconfig_options(&nested, &nested.join("file.js")).unwrap();
        assert_eq!(options.get("tabWidth"), Some(&json!(2)));
        assert_eq!(options.get("printWidth"), None);
    }

    #[test]
    fn procura_o_prettierrc_mais_proximo_e_depois_o_ancestral() {
        let tmp = Tmp::new("orq-format", "prettierrc");
        let nested = tmp.0.join("src").join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        write(&tmp.0, ".prettierrc.json", "{\"singleQuote\":true}");
        write(&nested, ".prettierrc.yaml", "singleQuote: false\n");
        let found = config_file(&nested).unwrap();
        assert_eq!(found, Config::Found(Some("singleQuote: false\n".into()), "yaml", nested.clone()));
        std::fs::remove_file(nested.join(".prettierrc.yaml")).unwrap();
        assert_eq!(config_file(&nested).unwrap(), Config::Found(Some("{\"singleQuote\":true}".into()), "json", tmp.0.clone()));
    }

    #[test]
    fn configuracao_em_js_ts_json5_ou_toml_deixa_o_prettier_indisponivel() {
        let tmp = Tmp::new("orq-format", "prettier-js");
        let nested = tmp.0.join("src");
        write(&tmp.0, ".prettierrc", "{\"singleQuote\":true}");
        write(&nested, "a.ts", "let a = 1\n");
        for name in UNSUPPORTED_CONFIGS {
            write(&nested, name, "");
            assert_eq!(config_file(&nested).unwrap(), Config::Unsupported(nested.join(name)), "{name}");
            std::fs::remove_file(nested.join(name)).unwrap();
        }
        write(&nested, "prettier.config.mjs", "export default {};\n");
        let Info::Unavailable { reason } = info(&nested.join("a.ts")).unwrap() else { panic!("deveria estar indisponível") };
        assert!(reason.contains("não é suportada") && reason.contains("prettier.config.mjs"), "{reason}");
        assert_eq!(std::fs::read_to_string(nested.join("a.ts")).unwrap(), "let a = 1\n");
    }

    #[test]
    fn package_json_com_bom_e_lido() {
        let tmp = Tmp::new("orq-format", "package-bom");
        write(&tmp.0, "package.json", "\u{feff}{\"prettier\":{\"semi\":false}}");
        assert_eq!(config_file(&tmp.0).unwrap(), Config::Found(Some("{\"semi\":false}".into()), "json", tmp.0.clone()));
    }

    #[test]
    fn secoes_do_editorconfig_aceitam_listas_nomes_e_dois_asteriscos() {
        assert!(section_matches("{package.json,.travis.yml}", "package.json"));
        assert!(section_matches("{package.json,.travis.yml}", ".travis.yml"));
        assert!(!section_matches("{package.json,.travis.yml}", "other.json"));
        assert!(section_matches("package.json", "package.json"));
        assert!(!section_matches("package.json", "a.json"));
        assert!(section_matches("**.js", "a.js"));
        assert!(!section_matches("**.js", "a.ts"));
        assert!(section_matches("*", "Makefile"));
        assert!(section_matches("*.{md,markdown}", "README.MD"));
        assert!(section_matches("*.{js,{ts,tsx}}", "a.tsx"));
        assert!(!section_matches("*.js", "a.json"));
        assert!(section_matches("**/*.rs", "lib.rs"));

        let tmp = Tmp::new("orq-format", "editorconfig-secoes");
        write(&tmp.0, ".editorconfig", "root = true\n[*]\nindent_size = 4\n[{package.json,.travis.yml}]\nindent_size = 2\n");
        let options = editorconfig_options(&tmp.0, &tmp.0.join("package.json")).unwrap();
        assert_eq!(options.get("tabWidth"), Some(&json!(2)));
    }

    fn prettier_options(dir: &Path, name: &str) -> Value {
        let Info::Prettier { config, .. } = info(&dir.join(name)).unwrap() else { panic!("deveria ser Prettier") };
        serde_json::from_str(&config.unwrap()).unwrap()
    }

    #[test]
    fn override_que_casa_entra_e_a_chave_overrides_sai() {
        let tmp = Tmp::new("orq-format", "override-casa");
        write(&tmp.0, ".prettierrc", r#"{"semi":false,"overrides":[{"files":"*.md","options":{"proseWrap":"always"}},{"files":["*.md","docs/*.md"],"options":{"semi":true}}]}"#);
        write(&tmp.0, "README.md", "# a\n");
        assert_eq!(prettier_options(&tmp.0, "README.md"), json!({"semi": true, "proseWrap": "always"}));
    }

    #[test]
    fn override_que_nao_casa_nao_entra() {
        let tmp = Tmp::new("orq-format", "override-nao-casa");
        write(&tmp.0, "package.json", r#"{"prettier":{"semi":false,"overrides":[{"files":"*.md","options":{"proseWrap":"always"}},{"files":"src/*.ts","options":{"tabWidth":8}},{"files":"*.MD","options":{"useTabs":true}}]}}"#);
        write(&tmp.0, "a.ts", "let a = 1\n");
        write(&tmp.0, "README.md", "# a\n");
        assert_eq!(prettier_options(&tmp.0, "a.ts"), json!({"semi": false}));
        // Maiúsculas contam e o glob com barra é relativo à pasta da configuração.
        assert_eq!(prettier_options(&tmp.0, "README.md"), json!({"semi": false, "proseWrap": "always"}));
    }

    #[test]
    fn override_com_barra_e_relativo_a_pasta_da_configuracao() {
        let tmp = Tmp::new("orq-format", "override-barra");
        let src = tmp.0.join("src").join("sub");
        write(&tmp.0, ".prettierrc.json", r#"{"overrides":[{"files":"src/**/*.{ts,tsx}","options":{"tabWidth":8}},{"files":"./lib/*.ts","options":{"semi":false}}]}"#);
        write(&src, "a.tsx", "let a = 1\n");
        write(&src, "b.js", "let a = 1\n");
        assert_eq!(prettier_options(&src, "a.tsx"), json!({"tabWidth": 8}));
        assert_eq!(prettier_options(&src, "b.js"), json!({}));
    }

    #[test]
    fn exclude_files_tira_o_arquivo_do_override() {
        let tmp = Tmp::new("orq-format", "override-exclude");
        let docs = tmp.0.join("docs");
        write(&tmp.0, ".prettierrc", r#"{"overrides":[{"files":"*.md","excludeFiles":["CHANGELOG.md","docs/*.md"],"options":{"proseWrap":"always"}}]}"#);
        write(&tmp.0, "README.md", "# a\n");
        write(&tmp.0, "CHANGELOG.md", "# a\n");
        write(&docs, "guia.md", "# a\n");
        assert_eq!(prettier_options(&tmp.0, "README.md"), json!({"proseWrap": "always"}));
        assert_eq!(prettier_options(&tmp.0, "CHANGELOG.md"), json!({}));
        assert_eq!(prettier_options(&docs, "guia.md"), json!({}));
    }

    #[test]
    fn overrides_em_yaml_deixam_o_prettier_indisponivel() {
        let tmp = Tmp::new("orq-format", "override-yaml");
        write(&tmp.0, ".prettierrc.yaml", "semi: false\noverrides:\n  - files: '*.md'\n    options:\n      proseWrap: always\n");
        write(&tmp.0, "README.md", "# a\n");
        let Info::Unavailable { reason } = info(&tmp.0.join("README.md")).unwrap() else { panic!("deveria estar indisponível") };
        assert!(reason.contains("overrides"), "{reason}");
        write(&tmp.0, ".prettierrc.yaml", "semi: false\n");
        assert!(matches!(info(&tmp.0.join("README.md")).unwrap(), Info::Prettier { .. }));
    }

    #[test]
    fn prettierignore_impede_a_formatacao() {
        let tmp = Tmp::new("orq-format", "prettierignore");
        let dist = tmp.0.join("dist");
        write(&tmp.0, ".prettierignore", "# gerado\ndist/\n*.min.js\n!keep.min.js\n");
        write(&dist, "a.js", "let a = 1\n");
        write(&tmp.0, "app.min.js", "let a = 1\n");
        write(&tmp.0, "keep.min.js", "let a = 1\n");
        write(&tmp.0, "app.js", "let a = 1\n");
        for name in ["dist/a.js", "app.min.js"] {
            let Info::Unavailable { reason } = info(&tmp.0.join(name)).unwrap() else { panic!("{name} deveria estar ignorado") };
            assert!(reason.contains(".prettierignore"), "{reason}");
        }
        for name in ["keep.min.js", "app.js"] {
            assert!(matches!(info(&tmp.0.join(name)).unwrap(), Info::Prettier { .. }), "{name}");
        }
    }

    #[test]
    fn package_json_com_chave_prettier_vence_o_prettierrc_da_mesma_pasta() {
        let tmp = Tmp::new("orq-format", "package-vs-rc");
        write(&tmp.0, "package.json", r#"{"prettier":{"semi":false}}"#);
        write(&tmp.0, ".prettierrc", r#"{"semi":true}"#);
        assert_eq!(config_file(&tmp.0).unwrap(), Config::Found(Some(r#"{"semi":false}"#.into()), "json", tmp.0.clone()));
        write(&tmp.0, "package.json", r#"{"name":"x"}"#);
        assert_eq!(config_file(&tmp.0).unwrap(), Config::Found(Some(r#"{"semi":true}"#.into()), "auto", tmp.0.clone()));
    }

    #[test]
    fn edicao_herdada_do_workspace() {
        let tmp = Tmp::new("orq-format", "cargo-workspace");
        let member = tmp.0.join("crates").join("app");
        let src = member.join("src");
        std::fs::create_dir_all(&src).unwrap();
        write(&tmp.0, "Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nedition = \"2024\"\n");
        write(&member, "Cargo.toml", "[package]\nname = \"app\"\nedition.workspace = true\n");
        assert_eq!(cargo_edition(&src), "2024");
        write(&member, "Cargo.toml", "[package]\nname = \"app\"\nedition = { workspace = true }\n");
        assert_eq!(cargo_edition(&src), "2024");
        write(&member, "Cargo.toml", "[package]\nname = \"app\"\nedition = \"2018\"\n");
        assert_eq!(cargo_edition(&src), "2018");
        write(&member, "Cargo.toml", "[package]\nname = \"app\"\n");
        assert_eq!(cargo_edition(&src), "2021");
    }

    #[cfg(windows)]
    fn print_file(file: &Path) -> Command {
        let mut cmd = crate::proc::quiet("cmd");
        cmd.arg("/C").arg("type").arg(file);
        cmd
    }

    #[cfg(not(windows))]
    fn print_file(file: &Path) -> Command {
        let mut cmd = crate::proc::quiet("cat");
        cmd.arg(file);
        cmd
    }

    #[test]
    fn run_encerra_o_processo_que_passa_do_prazo() {
        #[cfg(windows)]
        let cmd = {
            let mut cmd = crate::proc::quiet("ping");
            cmd.args(["-n", "30", "127.0.0.1"]);
            cmd
        };
        #[cfg(not(windows))]
        let cmd = {
            let mut cmd = crate::proc::quiet("sleep");
            cmd.arg("30");
            cmd
        };
        let started = Instant::now();
        let error = run(cmd, "", Duration::from_millis(300)).unwrap_err();
        assert!(error.contains("excedeu o limite"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    }

    #[test]
    fn run_le_a_saida_grande_inteira() {
        let tmp = Tmp::new("orq-format", "saida-grande");
        let line = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\n";
        let text = line.repeat(1024 * 1024 / line.len() + 1);
        write(&tmp.0, "grande.txt", &text);
        assert_eq!(run(print_file(&tmp.0.join("grande.txt")), "", TIMEOUT).unwrap(), text);
    }

    #[test]
    fn run_recusa_saida_fora_do_utf8() {
        let tmp = Tmp::new("orq-format", "saida-binaria");
        write(&tmp.0, "binario.txt", b"abc\xff\x80def");
        let error = run(print_file(&tmp.0.join("binario.txt")), "", TIMEOUT).unwrap_err();
        assert!(error.contains("UTF-8"), "{error}");
    }

    #[test]
    fn exige_caminho_absoluto_arquivo_e_pasta_existente() {
        let tmp = Tmp::new("orq-format", "path");
        write(&tmp.0, "a.json", "{}");
        assert!(validate_path(Path::new("a.json")).is_err());
        assert!(validate_path(&tmp.0.join("sumiu.json")).is_err());
        assert!(validate_path(&tmp.0.join("a.json")).is_ok());
    }
}
