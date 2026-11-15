//! A tela Git (lateral) e as vistas de comparação e de conflito do editor, porte de `src/git/` e de
//! `src/editor/{diff-view,compare-rows,conflict-view}.tsx`. API pública em `README.md`.
// A saída dos macros do GPUI aponta para `gpui::`; o alias resolve tudo pelo Kit.
extern crate gpui_kit as gpui;

pub mod colagem;
pub mod comparar;
pub mod comparador;
pub mod comum;
pub mod conflito_texto;
pub mod fonte;
pub mod grafo;
pub mod guarda;
pub mod pre_leitura;
pub mod quebra;
pub use orquestrador_hunks as preparo;
pub mod realce;
pub mod simulado;
pub mod sondagem;
pub mod tela;
mod tela_grafo;
pub mod conflito;
pub mod diff;
#[cfg(test)]
mod testes;

pub use comum::{Alvo, Lado};
pub use fonte::{Fonte, FonteCore, FonteGit};
pub use guarda::GuardaGit;
pub use pre_leitura::PreLeiturasGit;
pub use tela::{EventoGit, TelaGit};
pub use diff::{EventoDiff, VisaoDiff};
pub use conflito::{EventoConflito, RascunhoConflito, VisaoConflito};

/// Atalhos das vistas (F8/Shift+F8 no comparador, Ctrl+S no conflito, Shift+F10 e tecla de menu na tela Git). Chame uma vez, depois do `gpui_kit::init`.
pub fn init(cx: &mut gpui_kit::App) {
    realce::aquecer();
    diff::init(cx);
    tela::init(cx);
    conflito::init(cx);
}
