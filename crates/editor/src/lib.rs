//! Coluna central do editor: grupos de abas, editor de texto, Markdown, abrir rápido e recuperação. API em `README.md`.
// A saída dos macros do GPUI aponta para `gpui::`; o alias resolve tudo pelo Kit.
extern crate gpui_kit as gpui;

pub mod abrir_rapido;
pub mod area;
pub mod armazem;
pub mod caminhos;
pub mod comparar;
pub mod documento;
pub mod formatar;
pub mod gutter_git;
pub mod linguagem;
pub mod markdown;
pub mod migalhas;
pub mod minimapa;
pub mod modelo;
pub mod realce;

pub use area::{Abertura, AoAbrirDocumento, AoDescartarConflito, AreaEditor, EventoEditor, FabricaConteudo, InfoDocumento};
pub use modelo::{Alvo, Grupo, Lado};
