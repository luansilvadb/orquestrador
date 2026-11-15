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
}

impl Backend {
    /// O backend do app instalado: os mesmos caminhos e executáveis da versão 0.1.48.
    pub fn real() -> (Backend, Remetente, Receptor) {
        let cfg_path = config::config_path().unwrap_or_default();
        let interface = cfg_path.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.join("interface.json"));
        Self::com(cli::Exes::real(), session::Places::real(), cfg_path, interface)
    }

    pub fn com(
        exes: cli::Exes, places: session::Places, cfg_path: PathBuf, arquivo_interface: Option<PathBuf>,
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
        (Backend { projetos, terminais, avisos, estado: std::rc::Rc::new(RefCell::new(Estado::abrir(arquivo_interface))) }, tx, rx)
    }
}

/// Último segmento do caminho, com `\` ou `/`.
pub fn nome_da_pasta(caminho: &str) -> String {
    caminho.split(['\\', '/']).rfind(|s| !s.is_empty()).unwrap_or(caminho).to_string()
}

pub fn eh_pasta(caminho: &Path) -> bool {
    caminho.is_dir()
}
