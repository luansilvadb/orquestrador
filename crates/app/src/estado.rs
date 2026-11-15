//! Estado de interface que o React guarda no localStorage, num único `interface.json` em `%LOCALAPPDATA%\orquestrador\`.
//! Cada seção tem nome e versão próprios: uma seção de versão desconhecida, ilegível ou ausente volta ao padrão sem
//! derrubar as outras, e seções que este binário não conhece (editor, pesquisa de outras fases) são preservadas.
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Uma seção do arquivo. `validar` corrige valores fora de faixa depois de ler.
pub trait Secao: Default + Serialize + DeserializeOwned {
    const NOME: &'static str;
    const VERSAO: u32;
    fn validar(&mut self) {}
}

pub const RECENTES_MAX: usize = 10;

/// Abas de projeto (`orquestrador.project.tabs`, `.active`, `.recent` no App.tsx).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Projetos {
    pub abertas: Vec<String>,
    pub ativa: Option<String>,
    pub recentes: Vec<String>,
}

impl Secao for Projetos {
    const NOME: &'static str = "projetos";
    const VERSAO: u32 = 1;
    fn validar(&mut self) {
        self.abertas.retain(|c| !c.is_empty());
        self.recentes.retain(|c| !c.is_empty());
        self.recentes.truncate(RECENTES_MAX);
    }
}

/// Medidas dos painéis e o Terminal aberto. `None` usa o padrão do tema.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Paineis {
    pub largura_lateral: Option<f32>,
    pub largura_painel: Option<f32>,
    pub altura_terminal: Option<f32>,
    pub terminal_aberto: bool,
}

impl Secao for Paineis {
    const NOME: &'static str = "paineis";
    const VERSAO: u32 = 1;
    fn validar(&mut self) {
        use crate::tema;
        let limpa = |v: &mut Option<f32>, min: f32| {
            *v = v.filter(|x| x.is_finite() && *x >= min && *x <= 10_000.);
        };
        limpa(&mut self.largura_lateral, tema::LATERAL_MIN);
        limpa(&mut self.largura_painel, tema::PAINEL_MIN);
        limpa(&mut self.altura_terminal, tema::TERMINAL_MIN);
    }
}

/// O arquivo em memória: seções cruas, decodificadas sob demanda.
pub struct Estado {
    arquivo: Option<PathBuf>,
    secoes: Map<String, Value>,
}

impl Estado {
    /// Sem arquivo (`None`), só guarda em memória. Arquivo ausente ou corrompido começa vazio.
    pub fn abrir(arquivo: Option<PathBuf>) -> Estado {
        let secoes = arquivo
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default();
        Estado { arquivo, secoes }
    }

    pub fn ler<S: Secao>(&self) -> S {
        let Some(Value::Object(sec)) = self.secoes.get(S::NOME) else { return S::default() };
        if sec.get("versao").and_then(Value::as_u64) != Some(u64::from(S::VERSAO)) {
            return S::default();
        }
        let mut valor = sec.get("dados").cloned().and_then(|d| serde_json::from_value::<S>(d).ok()).unwrap_or_default();
        valor.validar();
        valor
    }

    /// Troca a seção e grava o arquivo. Falha de gravação só perde a preferência; o app segue.
    pub fn gravar<S: Secao>(&mut self, valor: &S) {
        let Ok(dados) = serde_json::to_value(valor) else { return };
        let novo = serde_json::json!({ "versao": S::VERSAO, "dados": dados });
        if self.secoes.get(S::NOME) == Some(&novo) {
            return;
        }
        self.secoes.insert(S::NOME.to_string(), novo);
        if let Err(e) = self.gravar_arquivo() {
            eprintln!("interface.json não gravou: {e}");
        }
    }

    /// Grava num temporário ao lado e renomeia por cima: uma queda no meio não deixa o arquivo pela metade.
    fn gravar_arquivo(&self) -> std::io::Result<()> {
        let Some(arquivo) = &self.arquivo else { return Ok(()) };
        if let Some(pasta) = arquivo.parent() {
            std::fs::create_dir_all(pasta)?;
        }
        let texto = serde_json::to_string_pretty(&self.secoes).map_err(std::io::Error::other)?;
        let tmp = arquivo.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(texto.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, arquivo).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }
}
