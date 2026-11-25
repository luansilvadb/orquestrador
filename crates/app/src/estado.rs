//! Seções do `interface.json` da bancada; o arquivo e o registro de seções ficam em `orquestrador_ui::estado`.
pub use orquestrador_ui::estado::{Estado, Secao};
use serde::{Deserialize, Serialize};

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
        use orquestrador_ui::tema;
        let limpa = |v: &mut Option<f32>, min: f32| {
            *v = v.filter(|x| x.is_finite() && *x >= min && *x <= 10_000.);
        };
        limpa(&mut self.largura_lateral, tema::LATERAL_MIN);
        limpa(&mut self.largura_painel, tema::PAINEL_MIN);
        limpa(&mut self.altura_terminal, tema::TERMINAL_MIN);
    }
}

/// Os avisos que o usuário já dispensou no portão com "Continuar": por origem (`claude.exe`, `codex.exe`), a chave do
/// último dispensado (`Aviso::dispensa`); o aviso com a mesma chave não volta.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AvisosDispensados {
    pub chaves: std::collections::BTreeMap<String, String>,
}

impl Secao for AvisosDispensados {
    const NOME: &'static str = "avisosDispensados";
    const VERSAO: u32 = 2;
}
