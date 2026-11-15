//! Trava das operações Git que a UI dispara (porte de `src/git/git-guard.ts`): uma por vez, mesmo entre a lateral e as abas
//! do editor. A bancada cria uma só e passa para todas as telas. Não cobre os executores vivos: isso é regra do backend.
use gpui_kit::{App, Window};
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Default)]
pub struct GuardaGit(Rc<Cell<bool>>);

impl GuardaGit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Começa uma operação. `false` (e avisa) se outra já está rodando.
    pub fn entrar(&self, window: &mut Window, cx: &mut App) -> bool {
        if self.0.get() {
            crate::comum::aviso("Espere a operação do Git em andamento terminar.", window, cx);
            return false;
        }
        self.0.set(true);
        true
    }

    /// Termina a operação que `entrar` começou.
    pub fn sair(&self) {
        self.0.set(false);
    }

    pub fn ocupada(&self) -> bool {
        self.0.get()
    }
}
