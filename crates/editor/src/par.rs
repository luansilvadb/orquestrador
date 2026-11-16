//! O par de colchetes sob o cursor: caixa nos dois colchetes do
//! par e vermelho no colchete sem par (ou nos dois de tipos diferentes).
use gpui::Hsla;
use gpui_kit::component::input::{RangeDecoration, RangeDecorationStyle, Rope};
use std::ops::Range;

/// Distância máxima da busca do par.
const DISTANCIA: usize = 10_000;
const COLCHETES: &[u8; 6] = b"()[]{}";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Casamento {
    pub inicio: Range<usize>,
    pub fim: Option<Range<usize>>,
    pub casado: bool,
}

/// Fundo do `editorBracketMatch` (#0064001A) e caixa #888.
pub fn cor_fundo() -> Hsla {
    gpui::rgba(0x0064001a).into()
}
pub fn cor_caixa() -> Hsla {
    gpui::rgb(0x888888).into()
}

fn byte(t: &Rope, i: usize) -> Option<u8> {
    (i < t.len()).then(|| t.byte(i))
}

/// `matchBrackets(state, pos, dir)`: `dir < 0` olha o fechamento antes de `pos`, `dir > 0` a abertura em `pos`. Só
/// conta colchetes da mesma classe do inicial (`contado`: fora de string e comentário).
fn casar_em(t: &Rope, pos: usize, dir: i8, contado: &dyn Fn(usize) -> bool) -> Option<Casamento> {
    let ini = if dir < 0 { pos.checked_sub(1)? } else { pos };
    let c = byte(t, ini)?;
    let k = COLCHETES.iter().position(|b| *b == c)?;
    if (k % 2 == 0) != (dir > 0) {
        return None;
    }
    let classe = contado(ini);
    let mut nivel = 0usize;
    let mut i = ini as isize;
    let fim_doc = t.len() as isize;
    let mut andado = 0usize;
    while i >= 0 && i < fim_doc {
        if andado > DISTANCIA {
            return None;
        }
        let b = t.byte(i as usize);
        if let Some(f) = COLCHETES.iter().position(|x| *x == b) {
            if contado(i as usize) == classe {
                if (f % 2 == 0) == (dir > 0) {
                    nivel += 1;
                } else if nivel == 1 {
                    let p = i as usize;
                    return Some(Casamento { inicio: ini..ini + 1, fim: Some(p..p + 1), casado: f / 2 == k / 2 });
                } else {
                    nivel = nivel.saturating_sub(1);
                }
            }
        }
        i += dir as isize;
        andado += 1;
    }
    Some(Casamento { inicio: ini..ini + 1, fim: None, casado: false })
}

/// A ordem do `bracketMatching` (com `afterCursor`): fechamento antes do cursor, abertura antes do cursor, abertura
/// depois e fechamento depois.
pub fn casar(t: &Rope, cursor: usize, contado: &dyn Fn(usize) -> bool) -> Option<Casamento> {
    casar_em(t, cursor, -1, contado)
        .or_else(|| cursor.checked_sub(1).and_then(|c| casar_em(t, c, 1, contado)))
        .or_else(|| casar_em(t, cursor, 1, contado))
        .or_else(|| (cursor < t.len()).then(|| casar_em(t, cursor + 1, -1, contado)).flatten())
}

/// As caixas do par casado; o par sem casamento só muda de cor (as faixas de `sem_par`).
pub fn decoracoes(c: &Option<Casamento>) -> (Vec<RangeDecoration>, Vec<Range<usize>>) {
    let Some(c) = c else { return (Vec::new(), Vec::new()) };
    let faixas: Vec<Range<usize>> = std::iter::once(c.inicio.clone()).chain(c.fim.clone()).collect();
    if c.casado {
        let caixas = faixas
            .into_iter()
            .map(|r| RangeDecoration::new(r).with_style(RangeDecorationStyle::InlineBox).with_color(cor_fundo()).with_ring(cor_caixa()))
            .collect();
        (caixas, Vec::new())
    } else {
        (Vec::new(), faixas)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn em(t: &str, cursor: usize) -> Option<Casamento> {
        casar(&Rope::from(t), cursor, &|_| true)
    }

    #[test]
    fn ordem_da_busca() {
        // `f(a)|`: fechamento antes do cursor.
        assert_eq!(em("f(a)", 4), Some(Casamento { inicio: 3..4, fim: Some(1..2), casado: true }));
        // `f(|a)`: abertura antes do cursor.
        assert_eq!(em("f(a)", 2), Some(Casamento { inicio: 1..2, fim: Some(3..4), casado: true }));
        // `f|(a)`: abertura depois do cursor.
        assert_eq!(em("f(a)", 1), Some(Casamento { inicio: 1..2, fim: Some(3..4), casado: true }));
        // `(a|)`: fechamento depois do cursor.
        assert_eq!(em("(a)", 2), Some(Casamento { inicio: 2..3, fim: Some(0..1), casado: true }));
        // `)(|`: o fechamento antes vence a abertura antes.
        assert_eq!(em("()", 2).unwrap().inicio, 1..2);
        assert_eq!(em("abc", 1), None);
    }

    #[test]
    fn aninhado_tipos_diferentes_e_sem_par() {
        assert_eq!(em("{ [a] ( b ) }", 0).unwrap().fim, Some(12..13));
        assert_eq!(em("(]", 0), Some(Casamento { inicio: 0..1, fim: Some(1..2), casado: false }));
        assert_eq!(em("(a", 0), Some(Casamento { inicio: 0..1, fim: None, casado: false }));
        let (caixas, sem) = decoracoes(&em("(a", 0));
        assert!(caixas.is_empty());
        assert_eq!(sem, vec![0..1]);
        let (caixas, sem) = decoracoes(&em("(a)", 0));
        assert_eq!(caixas.len(), 2);
        assert!(sem.is_empty());
        assert_eq!(caixas[0].style(), RangeDecorationStyle::InlineBox);
        assert_eq!(caixas[0].color(), Some(gpui::rgba(0x0064001a).into()));
        assert_eq!(caixas[0].ring(), Some(gpui::rgb(0x888888).into()));
    }

    #[test]
    fn colchete_em_string_so_casa_com_os_da_string() {
        // `f("(", x)`: o `(` da string não conta para o par de fora.
        let t = "f(\"(\", x)";
        let fora = |i: usize| !(2..5).contains(&i);
        assert_eq!(casar(&Rope::from(t), 1, &fora).unwrap().fim, Some(8..9));
    }

    #[test]
    fn alem_da_distancia_nao_marca() {
        let t = format!("({}", "a".repeat(DISTANCIA + 10));
        assert_eq!(em(&t, 0), None);
        let t = format!("({})", "a".repeat(100));
        assert!(em(&t, 0).unwrap().casado);
    }
}
