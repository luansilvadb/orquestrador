//! Aba de mídia do editor central: imagem, áudio e vídeo. Contrato em `README.md`.
// A saída dos macros do GPUI aponta para `gpui::`; o alias resolve tudo pelo Kit.
extern crate gpui_kit as gpui;

pub mod imagem;
mod mf;
pub mod pip;
pub mod player;
pub mod tipos;
mod visao;

pub use mf::simular_sem_saida_de_audio;
pub use tipos::{formatar_tempo, iniciar, midia_de, TipoMidia};
pub use visao::{registrar_atalhos, EventoMidia, VisaoMidia, VELOCIDADES};
