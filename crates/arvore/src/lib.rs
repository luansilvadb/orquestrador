//! A tela Arquivos sem o editor: a árvore de arquivos do projeto. Contrato em `README.md`.
// A saída dos macros do GPUI aponta para `gpui::`; o alias resolve tudo pelo Kit.
extern crate gpui_kit as gpui;

pub mod modelo;
mod vista;

pub use vista::{registrar_atalhos, Arrasto, EventoArvore, Explorador, FocarAnterior, FocarProximo, ALTURA_LINHA};
#[cfg(test)]
mod testes;
