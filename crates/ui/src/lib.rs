//! Peças de interface comuns à bancada e aos crates de tela: tema, recursos embutidos, estado salvo, widgets e a faixa
//! de abas. Contrato dos crates de tela em `README.md`.
// A saída dos macros do GPUI aponta para `gpui::`; o alias resolve tudo pelo Kit.
extern crate gpui_kit as gpui;

pub mod avisos;
pub mod estado;
pub mod faixa;
pub mod icones_arquivo;
pub mod lista;
pub mod recursos;
pub mod tema;
pub mod widgets;

pub use icones_arquivo::icone_arquivo;
pub use faixa::{Aba, ArrastoAba, EstadoFaixa, FaixaAbas, MoverAba};
pub use lista::{EstadoLista, ItemLista, ListaAbas};
