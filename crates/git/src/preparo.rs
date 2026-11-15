//! Preparar e tirar do preparo um bloco do editor, calculando o texto novo do índice (porte de `src/editor/git-stage.ts` e
//! da parte pura de `git-hunk.ts`), para o git gutter usar: o backend só grava o texto inteiro (`git::write_index`).
//! Textos com `\n`. Posições em bytes; `ate` de um trecho é o começo da linha seguinte, ou o tamanho + 1 quando o trecho
//! vai até o fim, como os `Chunk` do `@codemirror/merge`.
use similar::{capture_diff_slices, Algorithm, DiffOp};

/// Um trecho A × B: as posições em A e em B.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trecho {
    pub de_a: usize,
    pub ate_a: usize,
    pub de_b: usize,
    pub ate_b: usize,
}

/// A faixa de um bloco no buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Faixa {
    pub de: usize,
    pub ate: usize,
}

impl From<Trecho> for Faixa {
    fn from(t: Trecho) -> Self {
        Faixa { de: t.de_b, ate: t.ate_b }
    }
}

/// Linhas com a quebra; um texto vazio ou terminado em `\n` ganha a linha vazia do fim, como o `Text` do CodeMirror.
fn pedacos(texto: &str) -> Vec<&str> {
    let mut v: Vec<&str> = texto.split_inclusive('\n').collect();
    if texto.is_empty() || texto.ends_with('\n') {
        v.push("");
    }
    v
}

fn inicios(pedacos: &[&str], tamanho: usize) -> Vec<usize> {
    let mut v = Vec::with_capacity(pedacos.len() + 1);
    let mut p = 0;
    for s in pedacos {
        v.push(p);
        p += s.len();
    }
    // O fim de um trecho que inclui a última linha é o tamanho + 1.
    v.push(tamanho + 1);
    v
}

/// Os trechos A × B, linha a linha. Linhas iguais com quebra diferente contam como mudança, como no diff por caractere.
pub fn trechos(a: &str, b: &str) -> Vec<Trecho> {
    let (pa, pb) = (pedacos(a), pedacos(b));
    let (ia, ib) = (inicios(&pa, a.len()), inicios(&pb, b.len()));
    let mut saida: Vec<Trecho> = Vec::new();
    let mut aberto: Option<(usize, usize, usize, usize)> = None;
    let fechar = |aberto: &mut Option<(usize, usize, usize, usize)>, saida: &mut Vec<Trecho>| {
        if let Some((a0, a1, b0, b1)) = aberto.take() {
            let pos = |ini: &Vec<usize>, i: usize, j: usize| if i == j { (ini[i].min(ini[ini.len() - 1]), ini[i]) } else { (ini[i], ini[j]) };
            let (de_a, ate_a) = pos(&ia, a0, a1);
            let (de_b, ate_b) = pos(&ib, b0, b1);
            saida.push(Trecho { de_a, ate_a, de_b, ate_b });
        }
    };
    for op in capture_diff_slices(Algorithm::Myers, &pa, &pb) {
        match op {
            DiffOp::Equal { .. } => fechar(&mut aberto, &mut saida),
            _ => {
                let (o, n) = (op.old_range(), op.new_range());
                aberto = Some(match aberto {
                    Some((a0, _, b0, _)) => (a0, o.end, b0, n.end),
                    None => (o.start, o.end, n.start, n.end),
                });
            }
        }
    }
    fechar(&mut aberto, &mut saida);
    saida
}

/// Os dois trechos do buffer se tocam? Um vazio toca o vizinho encostado; dois com linhas, só se dividem alguma.
pub fn tocam(a: Faixa, b: Faixa) -> bool {
    if a.de == a.ate || b.de == b.ate {
        a.de <= b.ate && b.de <= a.ate
    } else {
        a.de < b.ate && b.de < a.ate
    }
}

/// Troca, em `a`, o trecho A de cada trecho pelo trecho B de `b`, do último para o primeiro.
pub fn aplicar(a: &str, b: &str, trechos: &[Trecho]) -> String {
    let mut saida = a.to_string();
    for t in trechos.iter().rev() {
        let fim_a = t.ate_a.min(saida.len());
        let meio = &b[t.de_b.min(b.len())..t.ate_b.min(b.len())];
        saida = format!("{}{}{}", &saida[..t.de_a.min(saida.len())], meio, &saida[fim_a..]);
    }
    saida
}

/// Preparar: cada trecho buffer × índice que toca o bloco passa a ter, no índice, as linhas do buffer.
pub fn preparar_bloco(indice: &str, buffer: &str, bloco: Faixa) -> String {
    let ts: Vec<Trecho> = trechos(indice, buffer).into_iter().filter(|t| tocam((*t).into(), bloco)).collect();
    aplicar(indice, buffer, &ts)
}

/// Tirar do preparo: o bloco vai para posições do índice pelos trechos buffer × índice de antes dele; cada trecho
/// índice × HEAD que toca essa faixa volta às linhas do HEAD.
pub fn tirar_bloco(head: &str, indice: &str, buffer: &str, bloco: Faixa) -> String {
    let contra_buffer = trechos(indice, buffer);
    let para_indice = |pos: usize| -> usize {
        let mut delta: isize = 0;
        for t in &contra_buffer {
            if t.ate_b <= pos && t.de_b < pos {
                delta = t.ate_a as isize - t.ate_b as isize;
            }
        }
        (pos as isize + delta).max(0) as usize
    };
    let faixa = Faixa { de: para_indice(bloco.de), ate: para_indice(bloco.ate) };
    let ts: Vec<Trecho> = trechos(head, indice)
        .into_iter()
        .filter(|t| tocam(Faixa { de: t.de_b, ate: t.ate_b }, faixa))
        .map(|t| Trecho { de_a: t.de_b, ate_a: t.ate_b, de_b: t.de_a, ate_b: t.ate_a })
        .collect();
    aplicar(indice, head, &ts)
}

/// Linhas antigas de um bloco (o trecho A, sem a quebra que o fecha), como as que o bloco aberto mostra em vermelho.
pub fn linhas_antigas(head: &str, t: &Trecho) -> Vec<String> {
    if t.de_a == t.ate_a {
        return Vec::new();
    }
    let texto = &head[t.de_a.min(head.len())..t.ate_a.min(head.len())];
    let texto = if t.ate_a <= head.len() { texto.strip_suffix('\n').unwrap_or(texto) } else { texto };
    texto.split('\n').map(str::to_string).collect()
}

/// O bloco seguinte (`1`) ou anterior (`-1`), dando a volta: com um aberto, o vizinho dele; sem, o primeiro depois (ou o
/// último antes) da linha do cursor. `inicios` são as primeiras linhas (base 1) de cada bloco, em ordem.
pub fn ir_para_bloco(inicios: &[usize], aberto: Option<usize>, linha_cursor: usize, direcao: i32) -> Option<usize> {
    let n = inicios.len();
    if n == 0 {
        return None;
    }
    Some(match aberto {
        Some(i) => ((i as i64 + direcao as i64).rem_euclid(n as i64)) as usize,
        None => {
            let achado = if direcao > 0 {
                inicios.iter().position(|&l| l > linha_cursor)
            } else {
                inicios.iter().rposition(|&l| l < linha_cursor)
            };
            achado.unwrap_or(if direcao > 0 { 0 } else { n - 1 })
        }
    })
}

#[cfg(test)]
mod testes {
    //! Porte de `tests/check-git-stage.mjs`.
    use super::*;

    fn linhas(ls: &[&str]) -> String {
        ls.iter().map(|l| format!("{l}\n")).collect()
    }
    fn blocos(head: &str, buffer: &str) -> Vec<Faixa> {
        trechos(head, buffer).into_iter().map(Faixa::from).collect()
    }

    #[test]
    fn aplicar_todos_os_trechos_da_b() {
        let amostras = ["", "a", "a\n", "b", "a\nb", "a\nb\n", "b\na", "a\nb\nc", "x\na\nb\nc\n", "a\n\nb", "\n", "\n\n", "a\nc\n", "c\nd"];
        for a in amostras {
            for b in amostras {
                assert_eq!(aplicar(a, b, &trechos(a, b)), b, "{a:?} → {b:?}");
            }
        }
    }

    #[test]
    fn preparar_o_bloco_do_meio() {
        let head = linhas(&["a", "b", "c", "d", "e", "f", "g", "h", "i"]);
        let buffer = linhas(&["A", "b", "c", "d", "E", "f", "g", "h", "I"]);
        let hs = blocos(&head, &buffer);
        assert_eq!(hs.len(), 3);
        assert_eq!(preparar_bloco(&head, &buffer, hs[1]), linhas(&["a", "b", "c", "d", "E", "f", "g", "h", "i"]));
        assert_eq!(preparar_bloco(&head, &buffer, hs[0]), linhas(&["A", "b", "c", "d", "e", "f", "g", "h", "i"]));
        assert_eq!(preparar_bloco(&head, &buffer, hs[2]), linhas(&["a", "b", "c", "d", "e", "f", "g", "h", "I"]));
    }

    #[test]
    fn preparar_com_indice_ja_diferente() {
        let head = linhas(&["a", "b", "c", "d", "e", "f"]);
        let indice = linhas(&["a", "B", "c", "d", "e", "f"]);
        let buffer = linhas(&["a", "B", "c", "d", "e", "F"]);
        let hs = blocos(&head, &buffer);
        assert_eq!(preparar_bloco(&indice, &buffer, hs[1]), buffer);
    }

    #[test]
    fn preparar_apagadas_e_acrescentadas() {
        let head = linhas(&["a", "b", "c", "d", "e", "f", "g"]);
        let buffer = linhas(&["a", "c", "d", "e", "x", "y", "f", "g"]);
        let ts = trechos(&head, &buffer);
        let (apagado, acrescentado) = (ts[0], ts[1]);
        assert_eq!(apagado.de_b, apagado.ate_b);
        assert_eq!(acrescentado.de_a, acrescentado.ate_a);
        assert_eq!(preparar_bloco(&head, &buffer, apagado.into()), linhas(&["a", "c", "d", "e", "f", "g"]));
        assert_eq!(preparar_bloco(&head, &buffer, acrescentado.into()), linhas(&["a", "b", "c", "d", "e", "x", "y", "f", "g"]));
        let uma = preparar_bloco(&head, &buffer, apagado.into());
        assert_eq!(preparar_bloco(&uma, &buffer, blocos(&head, &buffer)[1]), buffer);
    }

    #[test]
    fn tirar_do_preparo() {
        let head = linhas(&["a", "1", "2", "b", "c", "d", "e", "f", "g", "h"]);
        let indice = linhas(&["a", "1", "2", "B", "c", "d", "e", "f", "G", "h"]);
        let buffer = linhas(&["z", "a", "1", "2", "B", "c", "d", "e", "f", "G", "h"]);
        let hs = blocos(&head, &buffer);
        assert_eq!(hs.len(), 3);
        assert_eq!(tirar_bloco(&head, &indice, &buffer, hs[1]), linhas(&["a", "1", "2", "b", "c", "d", "e", "f", "G", "h"]));
        assert_eq!(tirar_bloco(&head, &indice, &buffer, hs[2]), linhas(&["a", "1", "2", "B", "c", "d", "e", "f", "g", "h"]));
        let preparado = preparar_bloco(&head, &buffer, hs[0]);
        assert_eq!(tirar_bloco(&head, &preparado, &buffer, hs[0]), head);
    }

    #[test]
    fn tirar_apagadas() {
        let head = linhas(&["a", "b", "c", "d"]);
        let indice = linhas(&["a", "c", "d"]);
        let hs = blocos(&head, &indice);
        assert_eq!(tirar_bloco(&head, &indice, &indice, hs[0]), head);
    }

    #[test]
    fn fim_sem_quebra() {
        let (head, buffer) = ("a\nb\nc", "a\nb\nc\nd");
        let hs = blocos(head, buffer);
        assert_eq!(preparar_bloco(head, buffer, hs[0]), buffer);
        assert_eq!(tirar_bloco(head, buffer, buffer, hs[0]), head);
        assert_eq!(preparar_bloco("a\nb", "a\nb\n", blocos("a\nb", "a\nb\n")[0]), "a\nb\n");
        assert_eq!(preparar_bloco("a\nb\n", "a\nb", blocos("a\nb\n", "a\nb")[0]), "a\nb");
        let (head2, buffer2) = ("a\nb\nc\nd", "A\nb\nc\nD");
        let hs = blocos(head2, buffer2);
        assert_eq!(preparar_bloco(head2, buffer2, hs[1]), "a\nb\nc\nD");
        assert_eq!(tirar_bloco(head2, "a\nb\nc\nD", buffer2, blocos(head2, buffer2)[1]), head2);
    }

    #[test]
    fn navegacao_e_linhas_antigas() {
        assert_eq!(ir_para_bloco(&[3, 10, 20], None, 12, 1), Some(2));
        assert_eq!(ir_para_bloco(&[3, 10, 20], None, 12, -1), Some(1));
        assert_eq!(ir_para_bloco(&[3, 10, 20], None, 25, 1), Some(0));
        assert_eq!(ir_para_bloco(&[3, 10, 20], Some(0), 1, -1), Some(2));
        assert_eq!(ir_para_bloco(&[], None, 1, 1), None);
        let head = "a\nb\nc\n";
        let t = trechos(head, "a\nc\n")[0];
        assert_eq!(linhas_antigas(head, &t), ["b"]);
    }
}
