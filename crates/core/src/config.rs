//! Configuração: `%LOCALAPPDATA%\orquestrador\config.json`, as validações e a lista de modelos.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub orchestrator: Choice,
    pub executor: Choice,
}

impl Default for Config {
    fn default() -> Self {
        Self { orchestrator: Choice::new("codex", "", ""), executor: Choice::new("claude", "", "") }
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

pub fn config_path() -> Option<PathBuf> {
    crate::platform::pasta_local_do_usuario().map(|b| b.join("orquestrador").join("config.json"))
}

/// O arquivo é lido como `Config` (ver o derive): chave que falta vale o padrão do que ela cobre. Forma errada é
/// erro, sem cair no padrão: papel sem `tool`, valor `null` ou arquivo que não é objeto. Sem arquivo, os padrões.
/// Arquivo ilegível ou resultado inválido é erro, com o motivo: não há fallback. Uma chave `maxParallel` de versões
/// antigas é ignorada: o teto de executores é automático (ver `capacity`).
pub fn read(path: &Path) -> Result<Config, String> {
    let cfg = if path.is_file() {
        let unreadable = |e: String| format!("Configuração inválida (JSON ilegível) em {}: {e}", path.display());
        let text = std::fs::read_to_string(path).map_err(|e| unreadable(e.to_string()))?;
        serde_json::from_str::<Config>(text.trim_start_matches('\u{feff}')).map_err(|e| unreadable(e.to_string()))?
    } else {
        Config::default()
    };
    let errors: Vec<String> =
        [check_role("orchestrator", &cfg.orchestrator), check_role("executor", &cfg.executor)].concat();
    if errors.is_empty() { Ok(cfg) } else { Err(invalid(path, &errors.join("; "))) }
}

fn invalid(path: &Path, why: &str) -> String {
    format!("Configuração inválida em {}: {why}", path.display())
}

pub fn save(path: &Path, cfg: &Config) -> Result<(), String> {
    // Publicada por renomeação: uma queda no meio deixa a configuração anterior, não um JSON cortado.
    let tmp = path.with_extension("json.tmp");
    let write = || {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(&tmp, serde_json::to_string_pretty(cfg).unwrap())?;
        std::fs::rename(&tmp, path)
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
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub tools: Vec<ToolOption>,
    pub last: Config,
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

struct ModelosEmConsulta {
    resultado: Mutex<Option<Vec<ModelOption>>>,
    pronto: Condvar,
}

impl ModelosEmConsulta {
    fn aguardar(&self) -> Vec<ModelOption> {
        let mut resultado = self.resultado.lock().unwrap();
        let limite = Instant::now() + PRAZO_MODELOS;
        while resultado.is_none() {
            let restante = limite.saturating_duration_since(Instant::now());
            if restante.is_zero() {
                return Vec::new();
            }
            let (novo, tempo) = self.pronto.wait_timeout(resultado, restante).unwrap();
            resultado = novo;
            if tempo.timed_out() && resultado.is_none() {
                return Vec::new();
            }
        }
        resultado.as_ref().unwrap().clone()
    }
}

struct EstadoModelos {
    cache: Vec<ModelOption>,
    em_consulta: Option<Arc<ModelosEmConsulta>>,
}

struct CacheModelosCodex(Mutex<EstadoModelos>);

impl CacheModelosCodex {
    const fn new() -> Self {
        Self(Mutex::new(EstadoModelos { cache: Vec::new(), em_consulta: None }))
    }

    fn get_or_query(&self, query: impl FnOnce() -> Vec<ModelOption>) -> Vec<ModelOption> {
        let (consulta, dono) = {
            let mut estado = self.0.lock().unwrap();
            if !estado.cache.is_empty() {
                return estado.cache.clone();
            }
            if let Some(consulta) = &estado.em_consulta {
                (consulta.clone(), false)
            } else {
                let consulta = Arc::new(ModelosEmConsulta { resultado: Mutex::new(None), pronto: Condvar::new() });
                estado.em_consulta = Some(consulta.clone());
                (consulta, true)
            }
        };

        if !dono {
            return consulta.aguardar();
        }

        let resposta = std::panic::catch_unwind(std::panic::AssertUnwindSafe(query));
        let (resultado, panico) = match resposta {
            Ok(resultado) => (resultado, None),
            Err(panico) => (Vec::new(), Some(panico)),
        };
        let mut estado = self.0.lock().unwrap();
        if !resultado.is_empty() {
            estado.cache = resultado.clone();
        }
        *consulta.resultado.lock().unwrap() = Some(resultado.clone());
        estado.em_consulta = None;
        drop(estado);
        consulta.pronto.notify_all();
        if let Some(panico) = panico {
            std::panic::resume_unwind(panico);
        }
        resultado
    }
}

// O Claude não tem comando que liste modelos: os apelidos resolvem para o mais novo de cada
// família. O Codex é perguntado com prazo; se não responder a tempo, a lista fica vazia (o nome
// ainda pode ser digitado) e a pergunta é refeita na próxima vez.
static CODEX_MODELS: CacheModelosCodex = CacheModelosCodex::new();

/// Prazo da consulta dos modelos do Codex: o normal é abaixo de 300 ms, e uma instalação travada não pode segurar o
/// diálogo de escolha.
const PRAZO_MODELOS: Duration = Duration::from_secs(5);

pub fn models(tool: &str, codex_exe: &str) -> Vec<ModelOption> {
    let all = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if tool == "claude" {
        let efforts = all(efforts("claude").unwrap());
        return all(&["fable", "opus", "sonnet", "haiku"]).into_iter().map(|name| ModelOption { name, efforts: efforts.clone() }).collect();
    }
    CODEX_MODELS.get_or_query(|| {
        let mut cmd = crate::proc::quiet(codex_exe);
        cmd.args(["debug", "models"]);
        // Estourou o prazo, saiu com erro ou não devolveu uma lista: fica vazia, e a próxima pergunta tenta de novo.
        if let Ok(saida) = crate::proc::saida_com_prazo(cmd, b"", PRAZO_MODELOS) {
            if !saida.estourou && saida.status.success() {
                return parse_codex_models(&String::from_utf8_lossy(&saida.out));
            }
        }
        Vec::new()
    })
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
        last: cfg.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Tmp;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread::{self, JoinHandle};

    const CLIENTES_CONCORRENTES: usize = 8;
    const PRAZO_TESTE: Duration = Duration::from_secs(10);

    fn lista_teste() -> Vec<ModelOption> {
        vec![ModelOption { name: "gpt-test".into(), efforts: vec!["low".into(), "high".into()] }]
    }

    fn vista(modelos: &[ModelOption]) -> Vec<(String, Vec<String>)> {
        modelos.iter().map(|modelo| (modelo.name.clone(), modelo.efforts.clone())).collect()
    }

    fn consultas_simultaneas(
        cache: Arc<CacheModelosCodex>,
        consulta: Arc<dyn Fn() -> Vec<ModelOption> + Send + Sync>,
    ) -> Vec<JoinHandle<Vec<ModelOption>>> {
        let threads: Vec<_> = (0..CLIENTES_CONCORRENTES)
            .map(|_| {
                let cache = cache.clone();
                let consulta = consulta.clone();
                thread::spawn(move || cache.get_or_query(|| consulta()))
            })
            .collect();
        aguardar_clientes(&cache, CLIENTES_CONCORRENTES);
        threads
    }

    fn aguardar_clientes(cache: &CacheModelosCodex, quantidade: usize) {
        let limite = Instant::now() + PRAZO_TESTE;
        loop {
            let ligados = {
                let estado = cache.0.lock().unwrap();
                estado.em_consulta.as_ref().map(Arc::strong_count).unwrap_or(0)
            };
            // Uma referência fica no estado, outra no dono, e cada aguardante mantém a geração.
            if ligados >= quantidade + 2 {
                return;
            }
            assert!(Instant::now() < limite, "clientes não aderiram à mesma geração (referências: {ligados})");
            thread::yield_now();
        }
    }

    fn aguardar_consulta_finalizada(cache: &CacheModelosCodex) {
        let limite = Instant::now() + PRAZO_TESTE;
        loop {
            if cache.0.lock().unwrap().em_consulta.is_none() {
                return;
            }
            assert!(Instant::now() < limite, "consulta não terminou no prazo do teste");
            thread::yield_now();
        }
    }

    /// O `config.json` de uma pasta que some com o `Tmp` devolvido.
    fn temp(name: &str) -> (Tmp, PathBuf) {
        let tmp = Tmp::new("orq-cfg", name);
        let path = tmp.0.join("config.json");
        (tmp, path)
    }

    #[test]
    fn sem_arquivo_valem_os_padroes() {
        let (_tmp, path) = temp("none");
        let cfg = read(&path).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!((cfg.orchestrator.tool.as_str(), cfg.executor.tool.as_str()), ("codex", "claude"));
    }

    #[test]
    fn arquivo_sobrepoe_os_padroes_e_volta_igual_depois_de_salvo() {
        let (_tmp, path) = temp("over");
        std::fs::write(&path, r#"{"orchestrator":{"tool":"claude","model":"opus","effort":"high"}}"#).unwrap();
        let cfg = read(&path).unwrap();
        assert_eq!(cfg.orchestrator, Choice::new("claude", "opus", "high"));
        assert_eq!(cfg.executor, Choice::new("claude", "", ""));
        save(&path, &cfg).unwrap();
        assert_eq!(read(&path).unwrap(), cfg);
        assert!(!path.with_extension("json.tmp").exists(), "o temporário da renomeação não fica");
    }

    /// Uma chave `maxParallel` de versões antigas não dita teto, seja qual for o valor, e some ao salvar.
    #[test]
    fn maxparallel_antigo_e_ignorado_e_nao_e_gravado() {
        let (_tmp, path) = temp("legado");
        for legacy in ["0", "17", "\"muitos\"", "null", "7"] {
            let json = format!(r#"{{"executor":{{"tool":"codex","effort":"low"}},"maxParallel":{legacy}}}"#);
            std::fs::write(&path, json).unwrap();
            let cfg = read(&path).unwrap_or_else(|e| panic!("maxParallel {legacy}: {e}"));
            assert_eq!(cfg.executor, Choice::new("codex", "", "low"), "maxParallel {legacy}");
            save(&path, &cfg).unwrap();
            assert!(!std::fs::read_to_string(&path).unwrap().contains("maxParallel"), "maxParallel {legacy} saiu do arquivo");
        }
    }

    #[test]
    fn arquivo_ilegivel_e_erro_sem_fallback() {
        let (_tmp, path) = temp("bad");
        std::fs::write(&path, "{ isto nao e json").unwrap();
        assert!(read(&path).unwrap_err().contains("JSON ilegível"));
    }

    /// A forma errada não cai no padrão: papel sem `tool`, valor `null` e arquivo que não é objeto são erro.
    #[test]
    fn forma_errada_e_erro_e_nao_cai_no_padrao() {
        let (_tmp, path) = temp("forma");
        for json in [r#"{"executor":{"model":"opus"}}"#, r#"{"executor":{"tool":"claude","model":null}}"#, r#"["claude"]"#] {
            std::fs::write(&path, json).unwrap();
            assert!(read(&path).unwrap_err().contains("JSON ilegível"), "{json}");
        }
    }

    #[test]
    fn valores_invalidos_sao_recusados_com_o_motivo() {
        let (_tmp, path) = temp("invalid");
        for (json, why) in [
            (r#"{"executor":{"tool":"gemini"}}"#, "ferramenta 'gemini' desconhecida"),
            (r#"{"executor":{"tool":"claude","effort":"ultra"}}"#, "esforço 'ultra' inválido para claude"),
            (r#"{"executor":{"tool":"claude","effort":"High"}}"#, "esforço 'High' inválido"),
            (r#"{"orchestrator":{"tool":"codex","model":"-m x"}}"#, "modelo '-m x' inválido"),
        ] {
            std::fs::write(&path, json).unwrap();
            let err = read(&path).unwrap_err();
            assert!(err.contains(why), "{json}: {err}");
        }
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

    #[test]
    fn clientes_concorrentes_compartilham_sucesso_e_uma_consulta() {
        let cache = Arc::new(CacheModelosCodex::new());
        let chamadas = Arc::new(AtomicUsize::new(0));
        let consultas_inesperadas = Arc::new(AtomicUsize::new(0));
        let (iniciou, esperar_inicio) = mpsc::channel();
        let (liberar, espera) = mpsc::channel();
        let dono = {
            let cache = cache.clone();
            let chamadas = chamadas.clone();
            thread::spawn(move || {
                cache.get_or_query(|| {
                    chamadas.fetch_add(1, Ordering::SeqCst);
                    iniciou.send(()).unwrap();
                    espera.recv_timeout(PRAZO_TESTE).expect("liberar consulta de sucesso");
                    lista_teste()
                })
            })
        };
        esperar_inicio.recv_timeout(PRAZO_TESTE).expect("dono iniciou a consulta");
        let consulta_inesperada = {
            let consultas_inesperadas = consultas_inesperadas.clone();
            Arc::new(move || {
                consultas_inesperadas.fetch_add(1, Ordering::SeqCst);
                Vec::new()
            })
        };

        let clientes = consultas_simultaneas(cache, consulta_inesperada);
        liberar.send(()).unwrap();
        let resultado_dono = dono.join().unwrap();
        assert_eq!(vista(&resultado_dono), vista(&lista_teste()));
        let resultados: Vec<_> = clientes.into_iter().map(|cliente| cliente.join().unwrap()).collect();
        assert!(resultados.iter().all(|resultado| vista(resultado) == vista(&lista_teste())));
        assert_eq!(chamadas.load(Ordering::SeqCst), 1);
        assert_eq!(consultas_inesperadas.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn clientes_concorrentes_compartilham_falha_e_retry_imediato_preserva_a_geracao_antiga() {
        let cache = Arc::new(CacheModelosCodex::new());
        let chamadas = Arc::new(AtomicUsize::new(0));
        let consultas_inesperadas = Arc::new(AtomicUsize::new(0));
        let (iniciou, esperar_inicio) = mpsc::channel();
        let (liberar, espera) = mpsc::channel();
        let dono = {
            let cache = cache.clone();
            let chamadas = chamadas.clone();
            thread::spawn(move || {
                cache.get_or_query(|| {
                    chamadas.fetch_add(1, Ordering::SeqCst);
                    iniciou.send(()).unwrap();
                    espera.recv_timeout(PRAZO_TESTE).expect("liberar consulta com falha");
                    Vec::new()
                })
            })
        };
        esperar_inicio.recv_timeout(PRAZO_TESTE).expect("dono iniciou a consulta");
        let consulta_inesperada = {
            let consultas_inesperadas = consultas_inesperadas.clone();
            Arc::new(move || {
                consultas_inesperadas.fetch_add(1, Ordering::SeqCst);
                Vec::new()
            })
        };

        let clientes = consultas_simultaneas(cache.clone(), consulta_inesperada);
        liberar.send(()).unwrap();
        assert!(dono.join().unwrap().is_empty());
        aguardar_consulta_finalizada(&cache);
        let resultado = cache.get_or_query(|| {
            chamadas.fetch_add(1, Ordering::SeqCst);
            lista_teste()
        });
        assert_eq!(vista(&resultado), vista(&lista_teste()));

        let resultados: Vec<_> = clientes.into_iter().map(|cliente| cliente.join().unwrap()).collect();
        assert!(resultados.iter().all(Vec::is_empty));
        assert_eq!(chamadas.load(Ordering::SeqCst), 2);
        assert_eq!(consultas_inesperadas.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn panico_do_dono_libera_aguardantes_e_permite_nova_consulta() {
        let cache = Arc::new(CacheModelosCodex::new());
        let chamadas = Arc::new(AtomicUsize::new(0));
        let (iniciou, esperar_inicio) = mpsc::channel();
        let (liberar, espera) = mpsc::channel();
        let dono = {
            let cache = cache.clone();
            let chamadas = chamadas.clone();
            thread::spawn(move || {
                cache.get_or_query(|| {
                    chamadas.fetch_add(1, Ordering::SeqCst);
                    iniciou.send(()).unwrap();
                    espera.recv_timeout(PRAZO_TESTE).expect("liberar consulta que entra em pânico");
                    std::panic::panic_any("falha planejada da consulta");
                })
            })
        };
        esperar_inicio.recv_timeout(PRAZO_TESTE).expect("dono iniciou a consulta");

        let consultas_inesperadas = Arc::new(AtomicUsize::new(0));
        let consulta_inesperada = {
            let consultas_inesperadas = consultas_inesperadas.clone();
            Arc::new(move || {
                consultas_inesperadas.fetch_add(1, Ordering::SeqCst);
                Vec::new()
            })
        };
        let aguardantes = consultas_simultaneas(cache.clone(), consulta_inesperada);

        liberar.send(()).unwrap();
        let panico = dono.join().expect_err("o pânico original deve chegar ao dono");
        assert_eq!(panico.downcast_ref::<&str>(), Some(&"falha planejada da consulta"));

        let resultado = cache.get_or_query(|| {
            chamadas.fetch_add(1, Ordering::SeqCst);
            lista_teste()
        });
        assert_eq!(vista(&resultado), vista(&lista_teste()));
        let resultados: Vec<_> = aguardantes.into_iter().map(|cliente| cliente.join().unwrap()).collect();
        assert!(resultados.iter().all(Vec::is_empty));
        assert_eq!(chamadas.load(Ordering::SeqCst), 2);
        assert_eq!(consultas_inesperadas.load(Ordering::SeqCst), 0);
    }
}
