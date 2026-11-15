//! Tema do xterm de `src/terminal/term.ts` e a paleta de 256 cores.
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use gpui_kit::{rgb, Hsla};

pub const FUNDO: u32 = 0x0b0c0d;
pub const TEXTO: u32 = 0xd4d7dc;
pub const CURSOR: u32 = 0xd4d7dc;
pub const CURSOR_TEXTO: u32 = 0x0b0c0d;
pub const SELECAO: u32 = 0x3a4180;
pub const SELECAO_INATIVA: u32 = 0x2a2d3a;

const ANSI: [u32; 16] = [
    0x1b1d21, 0xe5706b, 0x6cc28a, 0xd9b35c, 0x6f9fe8, 0xb48be0, 0x5bbdc4, 0xc5c8ce, 0x6b7079, 0xf08d88, 0x8bd6a4,
    0xe8c97c, 0x8fb6f2, 0xc9a6ee, 0x7dd3d9, 0xf2f3f5,
];

/// A cor do índice 0..=255: as 16 do tema, o cubo 6x6x6 e os cinzas, como o xterm.
pub fn indice(i: u8) -> u32 {
    match i {
        0..=15 => ANSI[i as usize],
        16..=231 => {
            let n = i - 16;
            let nivel = |v: u8| if v == 0 { 0u32 } else { 55 + v as u32 * 40 };
            (nivel(n / 36) << 16) | (nivel((n / 6) % 6) << 8) | nivel(n % 6)
        }
        _ => {
            let v = 8 + (i as u32 - 232) * 10;
            (v << 16) | (v << 8) | v
        }
    }
}

fn de_rgb(c: Rgb) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

/// Resolve a cor da célula; as trocadas pelo programa (OSC 4/10/11) valem antes do tema.
pub fn resolver(cor: Color, trocadas: &Colors, negrito: bool) -> u32 {
    match cor {
        Color::Spec(c) => de_rgb(c),
        Color::Indexed(i) => {
            // drawBoldTextInBrightColors do xterm: negrito nas 8 primeiras usa a clara.
            let i = if negrito && i < 8 { i + 8 } else { i };
            trocadas[i as usize].map(de_rgb).unwrap_or_else(|| indice(i))
        }
        Color::Named(n) => {
            let n = if negrito && (n as usize) < 8 { n.to_bright() } else { n };
            if let Some(c) = trocadas[n] {
                return de_rgb(c);
            }
            match n {
                NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => TEXTO,
                NamedColor::Background => FUNDO,
                NamedColor::Cursor => CURSOR,
                outro => {
                    let i = outro as usize;
                    if i < 16 {
                        ANSI[i]
                    } else {
                        // Dim*: a base; a transparência sai de `DIM`.
                        ANSI[(i - NamedColor::DimBlack as usize).min(7)]
                    }
                }
            }
        }
    }
}

pub fn hsla(c: u32) -> Hsla {
    rgb(c).into()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn paleta_de_256_como_o_xterm() {
        assert_eq!(indice(1), 0xe5706b);
        assert_eq!(indice(16), 0x000000);
        assert_eq!(indice(21), 0x0000ff);
        assert_eq!(indice(196), 0xff0000);
        assert_eq!(indice(232), 0x080808);
        assert_eq!(indice(255), 0xeeeeee);
    }
}
