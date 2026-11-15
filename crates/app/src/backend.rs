//! Liga o `orquestrador_core` à interface: o registro de projetos, o `Emit` num canal lido na thread principal e as abas
//! salvas em disco (`estado`).
use orquestrador_core::agents::{cli, session};
use orquestrador_core::config;
use orquestrador_core::core::projects::{Projects, TermSink};
use orquestrador_core::core::Emit;
use crate::estado::Estado;
pub use crate::estado::RECENTES_MAX;
use std::cell::RefCell;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// O que chega de fora da thread da janela.
#[derive(Debug)]
pub enum Evento {
    /// Um evento do core, com o `project` que o `scoped_emit` põe no payload.
    Core { nome: String, payload: Value },
    /// Caminhos repassados por outra instância (`open_paths`).
    AbrirCaminhos(Vec<PathBuf>),
}

pub type Remetente = async_channel::Sender<Evento>;
pub type Receptor = async_channel::Receiver<Evento>;

pub struct Backend {
    pub projetos: Arc<Projects>,
    /// O estado de todos os terminais, alimentado pela saída do core.
    pub terminais: Arc<crate::terminal::motor::Registro>,
    /// Avisos de saída nova por terminal, lidos pela janela.
    pub avisos: crate::terminal::motor::Avisos,
    /// O `interface.json`; `RefCell` porque o backend é compartilhado por `Rc`.
    pub estado: orquestrador_ui::estado::Compartilhado,
    /// Liga o poll do Git; os testes desligam para o executor ficar ocioso.
    pub poll_git: bool,
}

impl Backend {
    /// O backend do app instalado: os mesmos caminhos e executáveis que o Tauri usa.
    pub fn real() -> (Backend, Remetente, Receptor) {
        let cfg_path = config::config_path().unwrap_or_default();
        let interface = cfg_path.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.join("interface.json"));
        Self::com(cli::Exes::real(), session::Places::real(), cfg_path, interface, true)
    }

    pub fn com(
        exes: cli::Exes, places: session::Places, cfg_path: PathBuf, arquivo_interface: Option<PathBuf>, poll_git: bool,
    ) -> (Backend, Remetente, Receptor) {
        let (tx, rx) = async_channel::unbounded();
        let emissor = tx.clone();
        let emit: Emit = Arc::new(move |nome, payload| {
            let _ = emissor.try_send(Evento::Core { nome: nome.to_string(), payload });
        });
        let (terminais, avisos) = crate::terminal::motor::Registro::novo();
        let destino = terminais.clone();
        let sink: TermSink = Arc::new(move |projeto, id, _seq, bytes| destino.receber(projeto, id, bytes));
        let projetos = Arc::new(Projects::new(exes, places, cfg_path, emit).with_term_sink(sink));
        terminais.ligar(&projetos);
        (Backend { projetos, terminais, avisos, estado: std::rc::Rc::new(RefCell::new(Estado::abrir(arquivo_interface))), poll_git }, tx, rx)
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
