//! Macos e Linux: as mesmas primitivas do `windows.rs`, sem o recurso. O que é só ausência (processo ilegível, pasta
//! do usuário, memória) devolve `None`; o que é recurso do Windows devolve "Ainda não suportado neste sistema.".
use super::{ErroShell, Situacao, Snapshot};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

const AINDA_NAO: &str = "Ainda não suportado neste sistema.";

/// Sem o recurso de Windows, o comando não tem janela para esconder.
pub fn sem_janela(_cmd: &mut Command) {}

/// `kill` encerra o grupo usado por comandos com prazo; adoção genérica e consulta de PIDs não são suportadas.
pub struct Job(Option<i32>);

impl Job {
    pub fn adopt(_pid: u32) -> Option<Job> {
        None
    }

    pub fn kill(&self) {
        #[cfg(unix)]
        if let Some(group) = self.0 {
            unsafe { kill(-group, 9) };
        }
    }

    pub fn pids(&self) -> Option<Vec<u32>> {
        None
    }
}

pub fn spawn_com_prazo(cmd: &mut Command) -> io::Result<(Child, Job)> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        let child = cmd.spawn()?;
        Ok((child, Job(Some(child.id() as i32))))
    }
    #[cfg(not(unix))]
    {
        Ok((cmd.spawn()?, Job(None)))
    }
}

#[cfg(unix)]
#[link(name = "c")]
extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

/// Sem a consulta de executável do Windows, nenhum processo é CLI.
pub fn cli_do_processo(_pid: u32) -> Option<(&'static str, u64)> {
    None
}

pub fn start_time(_pid: u32) -> Option<u64> {
    None
}

/// Sem a consulta de processo do Windows, o dono nunca é provado vivo nem morto: fica `Ilegivel`.
pub fn situacao_do_processo(_pid: u32) -> Situacao {
    Situacao::Ilegivel
}

pub fn memoria() -> Option<(u64, u64)> {
    None
}

pub fn pasta_local_do_usuario() -> Option<PathBuf> {
    None
}

/// `$HOME`, o equivalente do `%USERPROFILE%`; vazio se não estiver definido.
pub fn pasta_pessoal() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Sem PowerShell: a lista vazia faz o shell do usuário responder "Ainda não suportado neste sistema.".
pub const SHELLS: [&str; 0] = [];

pub fn mover_sem_substituir(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, AINDA_NAO))
}

pub fn ja_existe(_erro: &io::Error) -> bool {
    false
}

pub fn em_uso(_erro: &io::Error) -> bool {
    false
}

/// Sem links simbólicos de arquivo pelo Windows: só os testes usam.
pub fn symlink_arquivo(_alvo: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, AINDA_NAO))
}

/// Sem o compartilhamento do Windows, o arquivo abre só para ler e gravar.
pub fn abrir_para_gravar(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(path)
}

pub fn revelar_no_explorador(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, AINDA_NAO))
}

pub fn abrir_pasta_no_explorador(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, AINDA_NAO))
}

pub fn open_url(_url: &str) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, AINDA_NAO))
}

pub fn mandar_para_lixeira(_file: &Path) -> Result<(), ErroShell> {
    Err(ErroShell::NaoSuportado)
}

pub fn copiar_ou_mover(_src: &Path, _parent: &Path, _name: &str, _cut: bool) -> Result<(), ErroShell> {
    Err(ErroShell::NaoSuportado)
}

/// Sem clipboard de arquivos, nada está copiado.
pub fn snapshot() -> Snapshot {
    Snapshot::default()
}

pub fn clear_if_same(_expected: &Snapshot) {}

pub fn set_files(_paths: &[PathBuf], _cut: bool) -> Result<(), String> {
    Err(AINDA_NAO.into())
}
