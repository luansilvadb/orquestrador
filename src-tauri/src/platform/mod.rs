//! Tudo o que é específico do sistema operacional: processos e Job, consulta de processo, COM, Lixeira, Explorer,
//! clipboard, memória, pastas do usuário e os nomes dos shells. `windows.rs` implementa; `other.rs` (macOS, Linux)
//! responde "Ainda não suportado neste sistema.", ou `None`/vazio quando o chamador já trata a ausência. Fora daqui
//! nenhum módulo chama a API do sistema diretamente.
use std::path::PathBuf;

#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
mod other;

#[cfg(windows)]
pub use windows::{
    abrir_para_gravar, abrir_pasta_no_explorador, clear_if_same, cli_do_processo, copiar_ou_mover, em_uso,
    ja_existe, mandar_para_lixeira, memoria, mover_sem_substituir, open_url,
    pasta_local_do_usuario, pasta_pessoal, revelar_no_explorador,
    sem_janela, set_files, situacao_do_processo, snapshot, start_time, symlink_arquivo, Job, SHELLS,
};
#[cfg(not(windows))]
pub use other::{
    abrir_para_gravar, abrir_pasta_no_explorador, clear_if_same, cli_do_processo, copiar_ou_mover, em_uso,
    ja_existe, mandar_para_lixeira, memoria, mover_sem_substituir, open_url,
    pasta_local_do_usuario, pasta_pessoal, revelar_no_explorador,
    sem_janela, set_files, situacao_do_processo, snapshot, start_time, symlink_arquivo, Job, SHELLS,
};

/// Os arquivos no clipboard: os caminhos, na ordem, e se foram recortados (`cut`) ou copiados. `paths` vem vazio quando
/// o clipboard não tem arquivos.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

/// O que `situacao_do_processo` acha: não existe, existe mas não dá para ler, ou foi lido (a hora de início e se ainda roda).
#[derive(Debug, PartialEq)]
pub enum Situacao {
    Inexistente,
    Ilegivel,
    Lido { criada: u64, rodando: bool },
}

/// Por que uma operação de shell (Lixeira ou colagem) não foi feita. A mensagem para o usuário fica em `files.rs`.
#[derive(Debug, PartialEq)]
pub enum ErroShell {
    /// O COM não iniciou na thread.
    SemCom,
    /// O Windows não criou a operação.
    SemOperacao,
    /// O caminho do item não foi achado pelo Shell.
    ItemAusente,
    /// O Windows recusou, abortou ou não executou a operação.
    Recusada,
    /// Este sistema ainda não faz a operação.
    NaoSuportado,
}
