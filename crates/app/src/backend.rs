//! Liga o `orquestrador_core` à interface: o registro de projetos, o `Emit` num canal lido na thread principal e as abas
//! salvas em disco.
use orquestrador_core::agents::{cli, session};
use orquestrador_core::config;
use orquestrador_core::core::projects::Projects;
use orquestrador_core::core::Emit;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// O que chega de fora da thread da janela.
#[derive(Debug)]
pub enum Evento {
    /// Um evento do core, com o `project` que o `scoped_emit` põe no payload.
    Core { payload: Value },
    /// Caminhos repassados por outra instância (`open_paths`).
    AbrirCaminhos(Vec<PathBuf>),
}

pub type Remetente = async_channel::Sender<Evento>;
pub type Receptor = async_channel::Receiver<Evento>;

/// Abas abertas e projetos recentes. O React guarda isto no localStorage da webview; aqui vai num JSON ao lado da configuração.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AbasSalvas {
    pub abertas: Vec<String>,
    pub ativa: Option<String>,
    pub recentes: Vec<String>,
}

pub const RECENTES_MAX: usize = 10;

pub struct Backend {
    pub projetos: Arc<Projects>,
    arquivo_abas: Option<PathBuf>,
    /// Liga o poll do Git; os testes desligam para o executor ficar ocioso.
    pub poll_git: bool,
}

impl Backend {
    /// O backend do app instalado: os mesmos caminhos e executáveis que o Tauri usa.
    pub fn real() -> (Backend, Remetente, Receptor) {
        let cfg_path = config::config_path().unwrap_or_default();
        let abas = cfg_path.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.join("gpui-abas.json"));
        Self::com(cli::Exes::real(), session::Places::real(), cfg_path, abas, true)
    }

    pub fn com(
        exes: cli::Exes, places: session::Places, cfg_path: PathBuf, arquivo_abas: Option<PathBuf>, poll_git: bool,
    ) -> (Backend, Remetente, Receptor) {
        let (tx, rx) = async_channel::unbounded();
        let emissor = tx.clone();
        let emit: Emit = Arc::new(move |_nome, payload| {
            let _ = emissor.try_send(Evento::Core { payload });
        });
        let projetos = Arc::new(Projects::new(exes, places, cfg_path, emit));
        (Backend { projetos, arquivo_abas, poll_git }, tx, rx)
    }

    pub fn ler_abas(&self) -> AbasSalvas {
        self.arquivo_abas
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|texto| serde_json::from_str(&texto).ok())
            .unwrap_or_default()
    }

    /// Falha de gravação só perde a preferência; o app segue.
    pub fn gravar_abas(&self, abas: &AbasSalvas) {
        let Some(arquivo) = &self.arquivo_abas else { return };
        if let Some(pasta) = arquivo.parent() {
            let _ = std::fs::create_dir_all(pasta);
        }
        if let Ok(texto) = serde_json::to_string_pretty(abas) {
            let _ = std::fs::write(arquivo, texto);
        }
    }
}

/// O resumo do Git que a moldura mostra: branch, contadores de sync e alterados (badge).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResumoGit {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub alterados: usize,
}

/// `None` quando o projeto não é a raiz de um repositório (mesma regra de `isRepo` no React).
pub fn resumo_git(projetos: &Projects, id: &str) -> Option<ResumoGit> {
    use orquestrador_core::ide::git;
    let repo = projetos.core(id).ok()?.repo().ok()?;
    match git::estado(&repo).ok()? {
        git::Estado::Repo { status } => Some(ResumoGit {
            branch: status.branch,
            ahead: status.ahead,
            behind: status.behind,
            alterados: status.files.len(),
        }),
        _ => None,
    }
}

/// Último segmento do caminho, com `\` ou `/`.
pub fn nome_da_pasta(caminho: &str) -> String {
    caminho.split(['\\', '/']).filter(|s| !s.is_empty()).next_back().unwrap_or(caminho).to_string()
}

pub fn eh_pasta(caminho: &Path) -> bool {
    caminho.is_dir()
}
