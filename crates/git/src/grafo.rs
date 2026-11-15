//! Modelo puro do grafo de commits (porte de `src/git/git-graph.ts`): faixas e segmentos de cada linha, sem desenho.
//! Cada linha descreve um commit: a faixa onde ele fica, a cor dela e os segmentos que ligam a borda de cima ao meio e o
//! meio à borda de baixo.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metade {
    Cima,
    Baixo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segmento {
    pub de: usize,
    pub para: usize,
    pub metade: Metade,
    pub cor: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinhaGrafo {
    pub coluna: usize,
    pub cor: usize,
    pub segmentos: Vec<Segmento>,
    pub largura: usize,
}

/// Commits do mais novo para o mais antigo, filhos antes dos pais. Pais fora da lista seguem até a borda de baixo.
pub fn montar_grafo<'a>(commits: impl IntoIterator<Item = (&'a str, &'a [String])>) -> Vec<LinhaGrafo> {
    // Faixa ocupada: o hash que ela espera e a cor que carrega desde que nasceu.
    let mut faixas: Vec<Option<(String, usize)>> = Vec::new();
    let mut proxima_cor = 0;
    let livre = |faixas: &Vec<Option<(String, usize)>>| faixas.iter().position(Option::is_none).unwrap_or(faixas.len());
    let mut saida = Vec::new();
    for (hash, pais) in commits {
        let esperando: Vec<usize> =
            faixas.iter().enumerate().filter(|(_, f)| f.as_ref().is_some_and(|f| f.0 == hash)).map(|(k, _)| k).collect();
        let coluna = esperando.first().copied().unwrap_or_else(|| livre(&faixas));
        let cor = match esperando.first() {
            Some(&k) => faixas[k].as_ref().map(|f| f.1).unwrap_or(0),
            None => {
                proxima_cor += 1;
                proxima_cor - 1
            }
        };
        let mut segmentos = Vec::new();
        for &k in &esperando {
            let c = faixas[k].as_ref().map(|f| f.1).unwrap_or(0);
            segmentos.push(Segmento { de: k, para: coluna, metade: Metade::Cima, cor: c });
        }
        for (k, faixa) in faixas.iter().enumerate() {
            let Some((h, c)) = faixa else { continue };
            if h == hash {
                continue;
            }
            segmentos.push(Segmento { de: k, para: k, metade: Metade::Cima, cor: *c });
            segmentos.push(Segmento { de: k, para: k, metade: Metade::Baixo, cor: *c });
        }
        for &k in &esperando {
            faixas[k] = None;
        }
        for (i, pai) in pais.iter().enumerate() {
            if i == 0 {
                if coluna >= faixas.len() {
                    faixas.resize(coluna + 1, None);
                }
                faixas[coluna] = Some((pai.clone(), cor));
                segmentos.push(Segmento { de: coluna, para: coluna, metade: Metade::Baixo, cor });
                continue;
            }
            if let Some(existente) = faixas.iter().position(|f| f.as_ref().is_some_and(|f| &f.0 == pai)) {
                segmentos.push(Segmento { de: coluna, para: existente, metade: Metade::Baixo, cor });
                continue;
            }
            let j = livre(&faixas);
            let nova = proxima_cor;
            proxima_cor += 1;
            if j >= faixas.len() {
                faixas.resize(j + 1, None);
            }
            faixas[j] = Some((pai.clone(), nova));
            segmentos.push(Segmento { de: coluna, para: j, metade: Metade::Baixo, cor: nova });
        }
        while faixas.last().is_some_and(Option::is_none) {
            faixas.pop();
        }
        let largura = 1 + segmentos.iter().flat_map(|s| [s.de, s.para]).chain([coluna]).max().unwrap_or(0);
        saida.push(LinhaGrafo { coluna, cor, segmentos, largura });
    }
    saida
}

#[cfg(test)]
mod testes {
    //! Porte de `tests/check-git-graph.mjs`.
    use super::*;

    fn c(hash: &str, pais: &[&str]) -> (String, Vec<String>) {
        (hash.to_string(), pais.iter().map(|p| p.to_string()).collect())
    }
    fn grafo(commits: &[(String, Vec<String>)]) -> Vec<LinhaGrafo> {
        montar_grafo(commits.iter().map(|(h, p)| (h.as_str(), p.as_slice())))
    }
    fn segs(l: &LinhaGrafo) -> Vec<String> {
        let mut v: Vec<String> = l
            .segmentos
            .iter()
            .map(|s| format!("{} {}>{} c{}", if s.metade == Metade::Cima { "top" } else { "bottom" }, s.de, s.para, s.cor))
            .collect();
        v.sort();
        v
    }
    fn ord(v: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = v.iter().map(|s| s.to_string()).collect();
        v.sort();
        v
    }

    #[test]
    fn linear() {
        let r = grafo(&[c("a", &["b"]), c("b", &["c"]), c("c", &[])]);
        assert_eq!(r.iter().map(|l| l.coluna).collect::<Vec<_>>(), [0, 0, 0]);
        assert_eq!(r.iter().map(|l| l.cor).collect::<Vec<_>>(), [0, 0, 0]);
        assert_eq!(segs(&r[0]), ord(&["bottom 0>0 c0"]));
        assert_eq!(segs(&r[1]), ord(&["bottom 0>0 c0", "top 0>0 c0"]));
        assert_eq!(segs(&r[2]), ord(&["top 0>0 c0"]));
        assert_eq!(r.iter().map(|l| l.largura).collect::<Vec<_>>(), [1, 1, 1]);
    }

    #[test]
    fn branch_e_merge() {
        let r = grafo(&[c("M", &["A", "B"]), c("B", &["R"]), c("A", &["R"]), c("R", &[])]);
        assert_eq!(r.iter().map(|l| l.coluna).collect::<Vec<_>>(), [0, 1, 0, 0]);
        assert_eq!(segs(&r[0]), ord(&["bottom 0>0 c0", "bottom 0>1 c1"]));
        assert_eq!(r[0].largura, 2);
        assert_eq!(segs(&r[1]), ord(&["bottom 0>0 c0", "bottom 1>1 c1", "top 0>0 c0", "top 1>1 c1"]));
        assert_eq!(segs(&r[2]), ord(&["bottom 0>0 c0", "bottom 1>1 c1", "top 0>0 c0", "top 1>1 c1"]));
        assert_eq!(r[1].cor, 1);
        assert_eq!(r[2].cor, 0);
        assert_eq!(segs(&r[3]), ord(&["top 0>0 c0", "top 1>0 c1"]));
        assert_eq!(r[3].largura, 2);
    }

    #[test]
    fn merge_com_pai_ja_esperado() {
        let r = grafo(&[c("X", &["Q"]), c("M", &["A", "Q"])]);
        assert_eq!(r[1].coluna, 1);
        assert!(segs(&r[1]).contains(&"bottom 1>0 c1".to_string()));
        assert!(segs(&r[1]).contains(&"bottom 1>1 c1".to_string()));
    }

    #[test]
    fn dois_filhos_do_mesmo_pai() {
        let r = grafo(&[c("A", &["P"]), c("B", &["P"]), c("P", &[])]);
        assert_eq!(r[1].coluna, 1);
        assert_eq!(r[1].cor, 1);
        assert_eq!(segs(&r[2]), ord(&["top 0>0 c0", "top 1>0 c1"]));
        assert_eq!(r[2].coluna, 0);
        assert_eq!(r[2].largura, 2);
    }

    #[test]
    fn merge_com_tres_pais() {
        let r = grafo(&[c("M", &["A", "B", "C"]), c("A", &["R"]), c("B", &["R"]), c("C", &["R"]), c("R", &[])]);
        assert_eq!(segs(&r[0]), ord(&["bottom 0>0 c0", "bottom 0>1 c1", "bottom 0>2 c2"]));
        assert_eq!(r[0].largura, 3);
        assert_eq!(segs(&r[4]), ord(&["top 0>0 c0", "top 1>0 c1", "top 2>0 c2"]));
        assert_eq!(r[4].largura, 3);
    }

    #[test]
    fn commit_raiz() {
        let r = grafo(&[c("r", &[])]);
        assert_eq!(r, vec![LinhaGrafo { coluna: 0, cor: 0, segmentos: vec![], largura: 1 }]);
        let cadeia = grafo(&[c("a", &["r"]), c("r", &[])]);
        assert_eq!(segs(&cadeia[1]), ord(&["top 0>0 c0"]));
    }

    #[test]
    fn pai_nao_carregado() {
        let r = grafo(&[c("a", &["Z"])]);
        assert_eq!(segs(&r[0]), ord(&["bottom 0>0 c0"]));
        assert_eq!(r[0].largura, 1);
    }

    #[test]
    fn faixa_livre_reaproveitada() {
        let r = grafo(&[c("a", &["c"]), c("b", &["d"]), c("c", &[]), c("e", &["f"])]);
        assert_eq!(r.iter().map(|l| l.coluna).collect::<Vec<_>>(), [0, 1, 0, 0]);
        assert_eq!(r.iter().map(|l| l.cor).collect::<Vec<_>>(), [0, 1, 0, 2]);
        assert_eq!(segs(&r[3]), ord(&["bottom 0>0 c2", "bottom 1>1 c1", "top 1>1 c1"]));
        assert_eq!(r[3].largura, 2);
    }

    #[test]
    fn faixas_vazias_cortadas() {
        let r = grafo(&[c("a", &["b"]), c("b", &[]), c("z", &[])]);
        assert_eq!(r[2].coluna, 0);
        assert_eq!(r[2].largura, 1);
    }

    #[test]
    fn cor_estavel() {
        let r = grafo(&[c("M", &["A", "B"]), c("B", &["R"]), c("A", &["R"]), c("R", &[])]);
        assert_eq!(r[1].cor, 1);
        assert!(r[2].segmentos.iter().any(|s| s.de == 1 && s.metade == Metade::Cima && s.cor == 1));
        assert_eq!((r[0].cor, r[2].cor, r[3].cor), (0, 0, 0));
    }

    #[test]
    fn lista_vazia() {
        assert!(grafo(&[]).is_empty());
    }
}
