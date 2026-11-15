//! A tela Pesquisar: busca e substituição de texto nos arquivos do projeto, com o motor de `orquestrador_core::ide::search`.
//! API e eventos em `README.md`.
// A saída dos macros do GPUI aponta para `gpui::`; o alias resolve tudo pelo Kit.
extern crate gpui_kit as gpui;

pub mod logica;
mod tela;

pub use logica::Visao;
pub use tela::{EventoPesquisa, PreferenciaPesquisa, TelaPesquisa};
