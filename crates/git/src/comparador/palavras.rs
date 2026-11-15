//! Diferença por palavra do comparador, como o `lineDiffType: 'word-alt'` do `@pierre/diffs` 1.2.12: o
//! `diffWordsWithSpace` do jsdiff 9 (Myers sobre palavras, espaços e pontuação) e a junção do `pushOrJoinSpan`, que cola
//! num só realce as mudanças separadas por um caractere igual. Devolve as faixas (em bytes) a realçar em cada linha.
use std::ops::Range;

/// Linhas maiores que isto (unidades UTF-16, como o `length` do JavaScript) não ganham realce por palavra.
pub const MAX_LINHA: usize = 1000;

fn letra(c: char) -> bool {
    matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '\u{AD}' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2C6}'
        | '\u{2C8}'..='\u{2D7}' | '\u{2DE}'..='\u{2FF}' | '\u{1E00}'..='\u{1EFF}')
}

/// Espaço que não é quebra de linha (`[^\S\n\r]`).
fn espaco(c: char) -> bool {
    c.is_whitespace() && c != '\n' && c != '\r'
}

/// O `tokenize` do `WordsWithSpaceDiff`: `(\r?\n)|[letras]+|[^\S\n\r]+|[^letras]`.
pub fn fichas(texto: &str) -> Vec<&str> {
    let mut v = Vec::new();
    let mut it = texto.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let mut fim = i + c.len_utf8();
        if c == '\r' && matches!(it.peek(), Some((_, '\n'))) {
            fim += 1;
            it.next();
        } else if letra(c) {
            while let Some(&(j, d)) = it.peek() {
                if !letra(d) {
                    break;
                }
                fim = j + d.len_utf8();
                it.next();
            }
        } else if espaco(c) {
            while let Some(&(j, d)) = it.peek() {
                if !espaco(d) {
                    break;
                }
                fim = j + d.len_utf8();
                it.next();
            }
        }
        v.push(&texto[i..fim]);
    }
    v
}

/// Um componente do resultado: `n` fichas seguidas iguais, incluídas ou removidas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Componente {
    pub n: usize,
    pub incluida: bool,
    pub removida: bool,
}

#[derive(Clone, Copy)]
struct No {
    comp: Componente,
    anterior: Option<usize>,
}

#[derive(Clone, Copy)]
struct Caminho {
    antigo: isize,
    ultimo: Option<usize>,
}

/// O `Diff.diffWithOptionsObj` do jsdiff, com os mesmos desempates, sobre fichas já separadas.
pub fn componentes(antigas: &[&str], novas: &[&str]) -> Vec<Componente> {
    let (n_novo, n_antigo) = (novas.len() as isize, antigas.len() as isize);
    let mut nos: Vec<No> = Vec::new();
    let max = (n_novo + n_antigo) as usize;
    // `bestPath[k]` com k de -max a max.
    let mut melhor: Vec<Option<Caminho>> = vec![None; 2 * max + 3];
    let ix = |k: isize| (k + max as isize + 1) as usize;
    let extrair = |c: &mut Caminho, diagonal: isize, nos: &mut Vec<No>| -> isize {
        let mut antigo = c.antigo;
        let mut novo = antigo - diagonal;
        let mut comuns = 0;
        while novo + 1 < n_novo && antigo + 1 < n_antigo && novas[(novo + 1) as usize] == antigas[(antigo + 1) as usize] {
            novo += 1;
            antigo += 1;
            comuns += 1;
        }
        if comuns > 0 {
            nos.push(No { comp: Componente { n: comuns, incluida: false, removida: false }, anterior: c.ultimo });
            c.ultimo = Some(nos.len() - 1);
        }
        c.antigo = antigo;
        novo
    };
    let construir = |ultimo: Option<usize>, nos: &[No]| -> Vec<Componente> {
        let mut v = Vec::new();
        let mut a = ultimo;
        while let Some(i) = a {
            v.push(nos[i].comp);
            a = nos[i].anterior;
        }
        v.reverse();
        v
    };
    let mut inicio = Caminho { antigo: -1, ultimo: None };
    let novo = extrair(&mut inicio, 0, &mut nos);
    if inicio.antigo + 1 >= n_antigo && novo + 1 >= n_novo {
        return construir(inicio.ultimo, &nos);
    }
    melhor[ix(0)] = Some(inicio);
    let somar = |c: Caminho, incluida: bool, removida: bool, passo: isize, nos: &mut Vec<No>| -> Caminho {
        match c.ultimo.map(|i| nos[i]) {
            Some(u) if u.comp.incluida == incluida && u.comp.removida == removida => {
                nos.push(No { comp: Componente { n: u.comp.n + 1, incluida, removida }, anterior: u.anterior });
            }
            _ => nos.push(No { comp: Componente { n: 1, incluida, removida }, anterior: c.ultimo }),
        }
        Caminho { antigo: c.antigo + passo, ultimo: Some(nos.len() - 1) }
    };
    let (mut min_d, mut max_d) = (isize::MIN, isize::MAX);
    let mut edicao: isize = 1;
    while edicao as usize <= max {
        let mut d = min_d.max(-edicao);
        while d <= max_d.min(edicao) {
            let remover = melhor[ix(d - 1)];
            if remover.is_some() {
                melhor[ix(d - 1)] = None;
            }
            let incluir = melhor[ix(d + 1)];
            let pode_incluir = incluir.is_some_and(|c| {
                let novo = c.antigo - d;
                0 <= novo && novo < n_novo
            });
            let pode_remover = remover.is_some_and(|c| c.antigo + 1 < n_antigo);
            if !pode_incluir && !pode_remover {
                melhor[ix(d)] = None;
                d += 2;
                continue;
            }
            let mut base = if !pode_remover || (pode_incluir && remover.unwrap().antigo < incluir.unwrap().antigo) {
                somar(incluir.unwrap(), true, false, 0, &mut nos)
            } else {
                somar(remover.unwrap(), false, true, 1, &mut nos)
            };
            let novo = extrair(&mut base, d, &mut nos);
            if base.antigo + 1 >= n_antigo && novo + 1 >= n_novo {
                return construir(base.ultimo, &nos);
            }
            melhor[ix(d)] = Some(base);
            if base.antigo + 1 >= n_antigo {
                max_d = max_d.min(d - 1);
            }
            if novo + 1 >= n_novo {
                min_d = min_d.max(d + 1);
            }
            d += 2;
        }
        edicao += 1;
    }
    Vec::new()
}

/// Um pedaço do `lineDiff`: o texto e se é igual, incluído ou removido.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parte<'a> {
    pub valor: &'a str,
    pub incluida: bool,
    pub removida: bool,
}

/// `diffWordsWithSpace(antiga, nova)`: as partes com o texto, como o `buildValues` do jsdiff.
pub fn diferenca<'a>(antiga: &'a str, nova: &'a str) -> Vec<Parte<'a>> {
    let (fa, fn_) = (fichas(antiga), fichas(nova));
    let mut v = Vec::new();
    let (mut pa, mut pn) = (0usize, 0usize);
    // Posições em bytes: as fichas são fatias contíguas da linha.
    let fim = |f: &[&str], ate: usize, base: &str| -> usize { if ate == 0 { 0 } else { f[ate - 1].as_ptr() as usize - base.as_ptr() as usize + f[ate - 1].len() } };
    for c in componentes(&fa, &fn_) {
        if c.removida {
            let (a, b) = (fim(&fa, pa, antiga), fim(&fa, pa + c.n, antiga));
            v.push(Parte { valor: &antiga[a..b], incluida: false, removida: true });
            pa += c.n;
        } else {
            let (a, b) = (fim(&fn_, pn, nova), fim(&fn_, pn + c.n, nova));
            v.push(Parte { valor: &nova[a..b], incluida: c.incluida, removida: false });
            pn += c.n;
            if !c.incluida {
                pa += c.n;
            }
        }
    }
    v
}

/// `pushOrJoinSpan` com `enableJoin`: (realçado, comprimento em bytes).
fn juntar(v: &mut Vec<(bool, usize, usize)>, valor: &str, neutro: bool, ultimo: bool) {
    match v.last_mut() {
        Some(u) if !ultimo => {
            let u_neutro = !u.0;
            // Igual depois de igual, mudança depois de mudança, ou um caractere igual entre mudanças: cola no anterior.
            if neutro == u_neutro || (neutro && valor.encode_utf16().count() == 1 && !u_neutro) {
                u.1 += valor.len();
                u.2 += 1;
                return;
            }
            v.push((!neutro, valor.len(), 1));
        }
        _ => v.push((!neutro, valor.len(), 1)),
    }
}

/// Faixas realçadas da linha removida e da incluída, pareadas (o `computeLineDiffDecorations` com `word-alt`).
pub fn realces(antiga: &str, nova: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    if antiga.encode_utf16().count() > MAX_LINHA || nova.encode_utf16().count() > MAX_LINHA {
        return (Vec::new(), Vec::new());
    }
    let partes = diferenca(antiga, nova);
    let (mut d, mut a) = (Vec::new(), Vec::new());
    let n = partes.len();
    for (i, p) in partes.iter().enumerate() {
        let ultimo = i + 1 == n;
        if !p.incluida && !p.removida {
            juntar(&mut d, p.valor, true, ultimo);
            juntar(&mut a, p.valor, true, ultimo);
        } else if p.removida {
            juntar(&mut d, p.valor, false, ultimo);
        } else {
            juntar(&mut a, p.valor, false, ultimo);
        }
    }
    let faixas = |v: Vec<(bool, usize, usize)>| {
        let mut pos = 0;
        let mut out = Vec::new();
        for (realce, n, _) in v {
            if realce {
                out.push(pos..pos + n);
            }
            pos += n;
        }
        out
    };
    (faixas(d), faixas(a))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn fichas_como_o_jsdiff() {
        assert_eq!(fichas("let  x=élan_1;"), ["let", "  ", "x", "=", "élan_1", ";"]);
        assert_eq!(fichas("a\r\nb"), ["a", "\r\n", "b"]);
    }

    // Esperados gerados com `diffWordsWithSpace` do jsdiff 9 e `pushOrJoinSpan` do `@pierre/diffs` 1.2.12.
    #[test]
    fn partes_do_diff_words_with_space() {
        let p = diferenca("import { a, b } from 'x';", "import { a, c, b } from 'x';");
        let t: Vec<_> = p.iter().map(|p| (p.valor, p.incluida, p.removida)).collect();
        assert_eq!(t, [("import { a, ", false, false), ("c, ", true, false), ("b } from 'x';", false, false)]);
        let p = diferenca("foo.bar", "baz.qux");
        let t: Vec<_> = p.iter().map(|p| (p.valor, p.incluida, p.removida)).collect();
        assert_eq!(t, [("foo", false, true), ("baz", true, false), (".", false, false), ("bar", false, true), ("qux", true, false)]);
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init, clippy::type_complexity)]
    fn word_alt_como_o_pierre() {
        let casos: [(&str, &str, &[Range<usize>], &[Range<usize>]); 6] = [
            ("foo.bar", "baz.qux", &[0..7], &[0..4, 4..7]),
            ("  antes", "  depois", &[2..7], &[2..8]),
            ("a = 1; b = 2;", "a = 3; b = 4;", &[4..5, 11..12], &[4..5, 11..12]),
            ("useLayoutEffect, useRef,", "useLayoutEffect, useMemo, useRef,", &[], &[17..26]),
            ("import { Columns2, Ellipsis, Pin, X } from 'lucide-react';", "import { ChevronDown, Columns2, Ellipsis, Pin, X } from 'lucide-react';", &[], &[9..22]),
            ("const total = soma(a, b) * 2;", "let total = soma(a, c) / 2; // x", &[0..5, 22..23, 25..26], &[0..3, 20..21, 23..24, 27..32]),
        ];
        for (antiga, nova, d, a) in casos {
            assert_eq!(realces(antiga, nova), (d.to_vec(), a.to_vec()), "{antiga} → {nova}");
        }
    }

    #[test]
    fn linha_longa_sem_realce() {
        let longa = "x".repeat(MAX_LINHA + 1);
        assert_eq!(realces(&longa, "y"), (Vec::new(), Vec::new()));
    }
}
