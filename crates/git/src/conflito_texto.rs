//! Marcadores de conflito (normais e diff3), porte da parte de conflitos de `src/editor/workspace-store.ts`. Trabalha com
//! linhas `\n`, como o texto que o backend devolve.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlocoConflito {
    pub nossas: Vec<String>,
    pub base: Option<Vec<String>>,
    pub deles: Vec<String>,
    // Linhas de marcador como estavam, para devolver um bloco pendente sem perder o texto.
    pub cabeca: String,
    pub cabeca_base: Option<String>,
    pub meio: String,
    pub cauda: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Peca {
    Linha(String),
    Bloco(BlocoConflito),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escolha {
    Atual,
    Recebida,
    Ambas,
    Pendente,
}

impl Escolha {
    pub fn rotulo(self) -> &'static str {
        match self {
            Escolha::Atual => "Atual",
            Escolha::Recebida => "Recebida",
            Escolha::Ambas => "Ambas",
            Escolha::Pendente => "Pendente",
        }
    }
}

/// Linha de marcador: exatamente sete caracteres iguais (`<`, `|`, `=` ou `>`) seguidos de espaço ou fim. `=========` é texto.
fn marcador(linha: &str) -> Option<char> {
    let c = linha.chars().next()?;
    if !matches!(c, '<' | '|' | '=' | '>') {
        return None;
    }
    let b = linha.as_bytes();
    if b.len() < 7 || b[..7].iter().any(|&x| x != c as u8) {
        return None;
    }
    match linha[7..].chars().next() {
        None => Some(c),
        Some(x) if x.is_whitespace() => Some(c),
        _ => None,
    }
}

fn e(linha: &str, c: char) -> bool {
    marcador(linha) == Some(c)
}

/// Quebra o texto em linhas e blocos; `malformado` quando um bloco não fecha (sem ação por trecho).
pub fn ler_conflito(texto: &str) -> (Vec<Peca>, bool) {
    let linhas: Vec<&str> = texto.split('\n').collect();
    let mut pecas = Vec::new();
    let mut i = 0;
    while i < linhas.len() {
        let linha = linhas[i];
        if !e(linha, '<') {
            pecas.push(Peca::Linha(linha.to_string()));
            i += 1;
            continue;
        }
        let mut nossas = Vec::new();
        let mut j = i + 1;
        while j < linhas.len() && !e(linhas[j], '=') && !e(linhas[j], '|') {
            nossas.push(linhas[j].to_string());
            j += 1;
        }
        let (mut cabeca_base, mut base) = (None, None);
        if j < linhas.len() && e(linhas[j], '|') {
            cabeca_base = Some(linhas[j].to_string());
            j += 1;
            let mut b = Vec::new();
            while j < linhas.len() && !e(linhas[j], '=') {
                b.push(linhas[j].to_string());
                j += 1;
            }
            base = Some(b);
        }
        if j >= linhas.len() {
            return (pecas, true);
        }
        let meio = linhas[j].to_string();
        j += 1;
        let mut deles = Vec::new();
        while j < linhas.len() && !e(linhas[j], '>') {
            deles.push(linhas[j].to_string());
            j += 1;
        }
        if j >= linhas.len() {
            return (pecas, true);
        }
        pecas.push(Peca::Bloco(BlocoConflito {
            nossas,
            base,
            deles,
            cabeca: linha.to_string(),
            cabeca_base,
            meio,
            cauda: linhas[j].to_string(),
        }));
        i = j + 1;
    }
    (pecas, false)
}

/// Texto final: cada bloco vira o lado escolhido; `Pendente` devolve o bloco com os marcadores originais.
pub fn montar_conflito(pecas: &[Peca], escolhas: &[Escolha]) -> String {
    let mut saida: Vec<&str> = Vec::new();
    let mut n = 0;
    for p in pecas {
        match p {
            Peca::Linha(l) => saida.push(l),
            Peca::Bloco(b) => {
                let escolha = escolhas.get(n).copied().unwrap_or(Escolha::Pendente);
                n += 1;
                match escolha {
                    Escolha::Atual => saida.extend(b.nossas.iter().map(String::as_str)),
                    Escolha::Recebida => saida.extend(b.deles.iter().map(String::as_str)),
                    Escolha::Ambas => saida.extend(b.nossas.iter().chain(&b.deles).map(String::as_str)),
                    Escolha::Pendente => {
                        saida.push(&b.cabeca);
                        saida.extend(b.nossas.iter().map(String::as_str));
                        if let Some(cb) = &b.cabeca_base {
                            saida.push(cb);
                            saida.extend(b.base.iter().flatten().map(String::as_str));
                        }
                        saida.push(&b.meio);
                        saida.extend(b.deles.iter().map(String::as_str));
                        saida.push(&b.cauda);
                    }
                }
            }
        }
    }
    saida.join("\n")
}

pub fn contar_blocos(pecas: &[Peca]) -> usize {
    pecas.iter().filter(|p| matches!(p, Peca::Bloco(_))).count()
}

/// Ainda há linha de marcador no texto.
pub fn tem_marcadores(texto: &str) -> bool {
    texto.split('\n').any(|l| marcador(l).is_some())
}

#[cfg(test)]
mod testes {
    use super::*;

    const DOIS: &str = "topo\n<<<<<<< ours\natual-1\n=======\nrecebida-1\n>>>>>>> theirs\nmeio\n<<<<<<< ours\natual-2\n=======\nrecebida-2\n>>>>>>> theirs\nfim\n";

    #[test]
    fn escolhe_por_trecho_e_preserva_o_resto() {
        let (p, mal) = ler_conflito(DOIS);
        assert!(!mal);
        assert_eq!(contar_blocos(&p), 2);
        let t = montar_conflito(&p, &[Escolha::Atual, Escolha::Pendente]);
        assert_eq!(t, "topo\natual-1\nmeio\n<<<<<<< ours\natual-2\n=======\nrecebida-2\n>>>>>>> theirs\nfim\n");
        let (p2, _) = ler_conflito(&t);
        assert_eq!(contar_blocos(&p2), 1);
        let t2 = montar_conflito(&p2, &[Escolha::Recebida]);
        assert_eq!(t2, "topo\natual-1\nmeio\nrecebida-2\nfim\n");
        assert!(!tem_marcadores(&t2));
        assert_eq!(montar_conflito(&p, &[Escolha::Ambas, Escolha::Ambas]), "topo\natual-1\nrecebida-1\nmeio\natual-2\nrecebida-2\nfim\n");
    }

    #[test]
    fn diff3_e_malformado() {
        let t = "<<<<<<< a\nx\n||||||| base\nb\n=======\ny\n>>>>>>> c";
        let (p, mal) = ler_conflito(t);
        assert!(!mal);
        assert_eq!(montar_conflito(&p, &[Escolha::Pendente]), t);
        assert!(ler_conflito("<<<<<<< a\nx\n=======\ny").1);
        // Oito sinais é texto (sublinhado de Markdown).
        assert!(!tem_marcadores("titulo\n========"));
        assert!(tem_marcadores("=======\r"));
    }
}
