//! De onde a tela tira o Git: cada comando é a função do core correspondente. A bancada entrega um [`FonteCore`] com o `Core` do projeto; os testes, o simulado.
use orquestrador_core::core::Core;
use orquestrador_core::ide::git;
use std::sync::Arc;

pub use git::{
    Branches, Commit, CommitDetail, CommitFile, CommitHoverDetail, Conflict, Diff, Estado, GitFile, GitRef, GraphCommit,
    HunkRef, Kind, RefKind, ReviewDiff, Status,
};

/// Os comandos Git da tela. Bloqueiam: a tela os chama no executor de fundo.
pub trait FonteGit: Send + Sync + 'static {
    fn estado(&self) -> Result<Estado, String>;
    fn init(&self) -> Result<(), String>;
    fn stage(&self, caminhos: &[String]) -> Result<(), String>;
    fn unstage(&self, caminhos: &[String]) -> Result<(), String>;
    fn commit(&self, mensagem: &str) -> Result<(), String>;
    fn fetch(&self) -> Result<String, String>;
    fn pull(&self) -> Result<String, String>;
    fn push(&self) -> Result<String, String>;
    fn branches(&self) -> Result<Branches, String>;
    fn switch(&self, nome: &str) -> Result<(), String>;
    fn create_branch(&self, nome: &str) -> Result<(), String>;
    fn review_diff(&self, caminho: &str, staged: bool) -> Result<ReviewDiff, String>;
    fn stage_hunk(&self, bloco: &HunkRef) -> Result<(), String>;
    fn unstage_hunk(&self, bloco: &HunkRef) -> Result<(), String>;
    fn discard_hunk(&self, bloco: &HunkRef) -> Result<(), String>;
    fn discard_file(&self, caminho: &str, revisao: &str) -> Result<(), String>;
    fn history(&self, limite: usize, pular: usize, caminho: Option<&str>) -> Result<Vec<GraphCommit>, String>;
    fn graph(&self, limite: usize, pular: usize) -> Result<Vec<GraphCommit>, String>;
    fn commit_detail(&self, id: &str) -> Result<CommitDetail, String>;
    fn commit_hover(&self, hash: &str) -> Result<CommitHoverDetail, String>;
    fn commit_diff(&self, id: &str, caminho: &str) -> Result<Diff, String>;
    fn conflict(&self, caminho: &str) -> Result<Conflict, String>;
    fn write_resolution(&self, caminho: &str, texto: &str, revisao: &str, resolvido: bool) -> Result<(), String>;
    fn accept_deletion(&self, caminho: &str, revisao: &str) -> Result<(), String>;
    /// Abre a URL no navegador (no cartão do commit).
    fn abrir_url(&self, url: &str) -> Result<(), String>;
}

pub type Fonte = Arc<dyn FonteGit>;

/// Os comandos pelo `Core` do projeto: mutações que mexem em branch ou índice de forma exclusiva passam por
/// `git_idle`.
pub struct FonteCore(pub Core);

impl FonteGit for FonteCore {
    fn estado(&self) -> Result<Estado, String> {
        git::estado(&self.0.repo()?)
    }
    fn init(&self) -> Result<(), String> {
        git::init(&self.0.repo()?)
    }
    fn stage(&self, caminhos: &[String]) -> Result<(), String> {
        git::stage(&self.0.repo()?, caminhos)
    }
    fn unstage(&self, caminhos: &[String]) -> Result<(), String> {
        git::unstage(&self.0.repo()?, caminhos)
    }
    fn commit(&self, mensagem: &str) -> Result<(), String> {
        git::commit(&self.0.repo()?, mensagem)
    }
    fn fetch(&self) -> Result<String, String> {
        git::fetch(&self.0.repo()?)
    }
    fn pull(&self) -> Result<String, String> {
        self.0.git_idle(git::pull)
    }
    fn push(&self) -> Result<String, String> {
        git::push(&self.0.repo()?)
    }
    fn branches(&self) -> Result<Branches, String> {
        git::branches(&self.0.repo()?)
    }
    fn switch(&self, nome: &str) -> Result<(), String> {
        self.0.git_idle(|dir| git::switch(dir, nome))
    }
    fn create_branch(&self, nome: &str) -> Result<(), String> {
        self.0.git_idle(|dir| git::create_branch(dir, nome))
    }
    fn review_diff(&self, caminho: &str, staged: bool) -> Result<ReviewDiff, String> {
        git::review_diff(&self.0.repo()?, caminho, staged)
    }
    fn stage_hunk(&self, bloco: &HunkRef) -> Result<(), String> {
        self.0.git_idle(|dir| git::stage_hunk(dir, bloco))
    }
    fn unstage_hunk(&self, bloco: &HunkRef) -> Result<(), String> {
        self.0.git_idle(|dir| git::unstage_hunk(dir, bloco))
    }
    fn discard_hunk(&self, bloco: &HunkRef) -> Result<(), String> {
        self.0.git_idle(|dir| git::discard_hunk(dir, bloco))
    }
    fn discard_file(&self, caminho: &str, revisao: &str) -> Result<(), String> {
        self.0.git_idle(|dir| git::discard_file(dir, caminho, revisao))
    }
    fn history(&self, limite: usize, pular: usize, caminho: Option<&str>) -> Result<Vec<GraphCommit>, String> {
        git::history(&self.0.repo()?, limite, pular, caminho)
    }
    fn graph(&self, limite: usize, pular: usize) -> Result<Vec<GraphCommit>, String> {
        git::graph(&self.0.repo()?, limite, pular)
    }
    fn commit_detail(&self, id: &str) -> Result<CommitDetail, String> {
        git::commit_detail(&self.0.repo()?, id)
    }
    fn commit_hover(&self, hash: &str) -> Result<CommitHoverDetail, String> {
        git::commit_hover(&self.0.repo()?, hash)
    }
    fn commit_diff(&self, id: &str, caminho: &str) -> Result<Diff, String> {
        git::commit_diff(&self.0.repo()?, id, caminho)
    }
    fn conflict(&self, caminho: &str) -> Result<Conflict, String> {
        git::conflict(&self.0.repo()?, caminho)
    }
    fn write_resolution(&self, caminho: &str, texto: &str, revisao: &str, resolvido: bool) -> Result<(), String> {
        self.0.git_idle(|dir| git::write_resolution(dir, caminho, texto, revisao, resolvido))
    }
    fn accept_deletion(&self, caminho: &str, revisao: &str) -> Result<(), String> {
        self.0.git_idle(|dir| git::accept_deletion(dir, caminho, revisao))
    }
    fn abrir_url(&self, url: &str) -> Result<(), String> {
        orquestrador_core::platform::open_url(url).map_err(|e| e.to_string())
    }
}
