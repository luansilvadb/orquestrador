//! Descoberta e execução dos formatadores do editor, sem depender do projeto aberto pelo app.
use serde::Serialize;
use serde_json::{Map, Value};
use std::env;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

fn config_file(path: &Path) -> Result<(Option<String>, &'static str), String> {
    for dir in path.ancestors() {
        for (name, kind) in [
            (".prettierrc", "auto"),
            (".prettierrc.json", "json"),
            (".prettierrc.yaml", "yaml"),
            (".prettierrc.yml", "yaml"),
        ] {
            let file = dir.join(name);
            if file.is_file() {
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| format!("Não foi possível ler {}: {e}", file.display()))?;
                return Ok((Some(text), kind));
            }
        }
        let package = dir.join("package.json");
        if package.is_file() {
            let text = std::fs::read_to_string(&package).map_err(|e| format!("Não foi possível ler {}: {e}", package.display()))?;
            let value: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", package.display()))?;
            if let Some(config) = value.get("prettier").filter(|config| !config.is_null()) {
                return Ok((Some(config.to_string()), "json"));
            }
        }
    }
    Ok((None, "json"))
}

fn section_matches(section: &str, name: &str) -> bool {
    if section == "*" {
        return true;
    }
    let Some((_, ext)) = name.rsplit_once('.') else { return false };
    if let Some(exts) = section.strip_prefix("*.{").and_then(|s| s.strip_suffix('}')) {
        return exts.split(',').any(|candidate| candidate.trim().eq_ignore_ascii_case(ext));
    }
    section.strip_prefix("*.").is_some_and(|candidate| candidate.eq_ignore_ascii_case(ext))
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
            let (config, config_kind) = config_file(directory)?;
            Ok(Info::Prettier { config, config_kind, options: editorconfig_options(directory, path)? })
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

fn cargo_edition(directory: &Path) -> String {
    for dir in directory.ancestors() {
        let manifest = dir.join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(manifest) else { continue };
        let mut package = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') && line.ends_with(']') {
                package = line == "[package]";
            } else if package {
                if let Some((key, value)) = line.split_once('=') {
                    if key.trim() == "edition" {
                        let edition = value.trim().trim_matches('"').trim_matches('\'');
                        if !edition.is_empty() { return edition.to_string(); }
                    }
                }
            }
        }
        break;
    }
    "2021".into()
}

fn run(mut command: Command, input: &str) -> Result<String, String> {
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("O formatador não pôde iniciar: {e}"))?;
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let input = input.as_bytes().to_vec();
    let writer = thread::spawn(move || stdin_pipe(stdin, &input));
    let out_reader = thread::spawn(move || read_pipe(stdout));
    let err_reader = thread::spawn(move || read_pipe(stderr));
    let started = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status, false),
            Ok(None) if started.elapsed() < TIMEOUT => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                break (child.wait().map_err(|e| format!("Não foi possível encerrar o formatador: {e}"))?, true);
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Não foi possível aguardar o formatador: {e}"));
            }
        }
    };
    let _ = writer.join();
    let output = out_reader.join().map_err(|_| "Falha ao ler a saída do formatador.".to_string())?
        .map_err(|e| format!("Falha ao ler a saída do formatador: {e}"))?;
    let error = err_reader.join().map_err(|_| "Falha ao ler o erro do formatador.".to_string())?
        .map_err(|e| format!("Falha ao ler o erro do formatador: {e}"))?;
    let stderr = String::from_utf8_lossy(&error).trim().to_string();
    if timed_out {
        let detail = if stderr.is_empty() { String::new() } else { format!(" {stderr}") };
        return Err(format!("O formatador excedeu o limite de 15 s.{detail}"));
    }
    if !status.success() {
        return Err(if stderr.is_empty() { format!("O formatador falhou (código {}).", status.code().unwrap_or(-1)) }
            else { format!("O formatador falhou: {stderr}") });
    }
    String::from_utf8(output).map_err(|e| format!("O formatador devolveu texto que não está em UTF-8: {e}"))
}

fn stdin_pipe(mut stdin: impl Write, input: &[u8]) {
    let _ = stdin.write_all(input);
}

fn read_pipe(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    pipe.read_to_end(&mut data)?;
    Ok(data)
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
    run(command, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{write, Tmp};
    use serde_json::json;

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
        assert_eq!(found, (Some("singleQuote: false\n".into()), "yaml"));
        std::fs::remove_file(nested.join(".prettierrc.yaml")).unwrap();
        assert_eq!(config_file(&nested).unwrap(), (Some("{\"singleQuote\":true}".into()), "json"));
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
