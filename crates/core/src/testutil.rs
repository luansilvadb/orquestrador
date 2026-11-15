//! Fixtures dos testes unitários que mais de um módulo usa: pastas temporárias com nome por processo, o Git de
//! teste com identidade fixa, a junction do Windows e as sessões de delegação. Só existe em `cargo test`.
use crate::platform::start_time;
use crate::proc::git_saida;
use std::path::{Path, PathBuf};

/// Pasta vazia `<temporária>\<prefix>-<pid>-<tag>`. Quem chama a apaga quando acaba; `Tmp` faz isso sozinho.
pub fn fresh(prefix: &str, tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Pasta temporária do teste. Some quando o teste acaba, mesmo se ele falhar.
pub struct Tmp(pub PathBuf);

impl Tmp {
    pub fn new(prefix: &str, tag: &str) -> Tmp {
        Tmp(fresh(prefix, tag))
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// O Git pelo mesmo caminho do app (`proc::git_saida`). Falha o teste se o Git falhar.
pub fn git(dir: &Path, args: &[&str]) -> String {
    git_saida(dir, args).unwrap()
}

/// Identidade e fim de linha fixos na configuração local: nada do usuário nem do Windows entra no teste.
pub fn identity(dir: &Path) {
    for (key, value) in [("user.name", "teste"), ("user.email", "teste@teste"), ("core.autocrlf", "false"), ("core.safecrlf", "false"), ("commit.gpgsign", "false")] {
        git(dir, &["config", key, value]);
    }
}

pub fn write(dir: &Path, rel: &str, data: impl AsRef<[u8]>) {
    let file = dir.join(rel);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, data).unwrap();
}

/// Adiciona tudo e commita, para que os arquivos sejam rastreados.
pub fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

/// Junction do Windows (`mklink /J`): não pede privilégio. Diz se foi criada.
pub fn junction(link: &Path, target: &Path) -> bool {
    std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link).arg(target).output().is_ok_and(|o| o.status.success())
}

/// Uma pasta de sessão de delegação cujo dono é este processo (vivo) ou um processo que já não existe.
pub fn session(prefix: &str, tag: &str, alive: bool) -> Tmp {
    let tmp = Tmp::new(prefix, tag);
    let dir = &tmp.0;
    for sub in ["requests", "results", "exec"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    let me = std::process::id();
    let started = start_time(me).unwrap() + u64::from(!alive);
    std::fs::write(dir.join("owner"), format!("{me} {started} strategist-{tag}")).unwrap();
    tmp
}
