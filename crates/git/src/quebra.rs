//! `text-wrap: pretty` do Chromium, que o tema global do lobe-ui liga em todo o app: quando a quebra gulosa deixa uma
//! palavra sozinha na última linha, as últimas (até 4) linhas são redistribuídas para minimizar a soma dos quadrados
//! das sobras; a última linha não conta, mas precisa de duas palavras.

const LINHAS_OTIMIZADAS: usize = 4;

/// As linhas de `texto` na largura `largura`, com `medir` dando a largura de um trecho.
pub fn linhas(texto: &str, largura: f32, medir: impl Fn(&str) -> f32) -> Vec<String> {
    let palavras: Vec<&str> = texto.split(' ').filter(|p| !p.is_empty()).collect();
    if palavras.is_empty() {
        return Vec::new();
    }
    let trecho = |i: usize, j: usize| palavras[i..j].join(" ");
    let cabe = |i: usize, j: usize| j == i + 1 || medir(&trecho(i, j)) <= largura;
    // Gulosa, como o navegador.
    let mut inicios = vec![0];
    let mut i = 0;
    while i < palavras.len() {
        let mut j = i + 1;
        while j < palavras.len() && cabe(i, j + 1) {
            j += 1;
        }
        if j < palavras.len() {
            inicios.push(j);
        }
        i = j;
    }
    let n = inicios.len();
    if n >= 2 && palavras.len() - inicios[n - 1] == 1 {
        let k = n.min(LINHAS_OTIMIZADAS);
        let base = inicios[n - k];
        // custo[i] = menor soma dos quadrados das sobras de palavras[i..] em linhas que cabem.
        let fim = palavras.len();
        let mut custo = vec![f32::INFINITY; fim + 1];
        let mut prox = vec![fim; fim + 1];
        custo[fim] = 0.;
        for i in (base..fim).rev() {
            for j in i + 1..=fim {
                if !cabe(i, j) {
                    break;
                }
                let c = if j == fim {
                    if j - i < 2 { continue } else { 0. }
                } else {
                    let sobra = largura - medir(&trecho(i, j));
                    sobra * sobra + custo[j]
                };
                if c < custo[i] {
                    custo[i] = c;
                    prox[i] = j;
                }
            }
        }
        // Sem distribuição possível (palavra mais larga que a linha), fica a gulosa.
        if custo[base].is_finite() {
            inicios.truncate(n - k);
            let mut i = base;
            while i < fim {
                inicios.push(i);
                i = prox[i];
            }
        }
    }
    inicios.iter().enumerate().map(|(k, &i)| trecho(i, inicios.get(k + 1).copied().unwrap_or(palavras.len()))).collect()
}

#[cfg(test)]
mod testes {
    use super::linhas;

    fn medir(t: &str) -> f32 {
        t.chars().count() as f32
    }

    #[test]
    fn sem_orfa_fica_a_gulosa() {
        assert_eq!(linhas("aa bb cc dd", 5., medir), ["aa bb", "cc dd"]);
    }

    #[test]
    fn orfa_puxa_palavra_da_linha_anterior() {
        assert_eq!(linhas("aaa bbb ccc d", 11., medir), ["aaa bbb", "ccc d"]);
        // Com 5 linhas, a primeira fica como a gulosa deixou.
        assert_eq!(linhas("aaaaaaaaaaa aaa bbb ccc ddd eee fff g", 11., medir)[0], "aaaaaaaaaaa");
    }

    #[test]
    fn palavra_maior_que_a_largura_ocupa_a_linha() {
        assert_eq!(linhas("abcdefgh x", 4., medir), ["abcdefgh", "x"]);
    }
}
