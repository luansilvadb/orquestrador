//! Acha URLs e caminhos de arquivo numa linha lógica do terminal (porte de `src/terminal/term-link-parse.ts`). As
//! posições são índices de `char`, que é como a linha do terminal é montada.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Url { inicio: usize, fim: usize, url: String },
    Caminho { inicio: usize, fim: usize, caminho: String, linha: Option<u32>, coluna: Option<u32> },
}

impl Link {
    pub fn intervalo(&self) -> (usize, usize) {
        match self {
            Link::Url { inicio, fim, .. } | Link::Caminho { inicio, fim, .. } => (*inicio, *fim),
        }
    }
}

/// `\w` do JavaScript sem a flag `u`: só ASCII.
fn palavra(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `[\w.\-@+~$%#]`, um caractere de segmento de caminho.
fn de_segmento(c: char) -> bool {
    palavra(c) || ".-@+~$%#".contains(c)
}

fn separador(c: char) -> bool {
    c == '\\' || c == '/'
}

/// O que não pode vir logo antes do começo de um caminho: impede começar no meio de outro token.
fn bloqueia_inicio(c: char) -> bool {
    de_segmento(c) || separador(c) || c == ':'
}

fn fim_de_url(c: char) -> bool {
    c.is_whitespace() || "<>\"'`".contains(c)
}

/// URL sem a pontuação final; `)` fica se fecha um `(` da própria URL.
fn aparar_url(url: &[char]) -> usize {
    let mut fim = url.len();
    while fim > 0 {
        let ultimo = url[fim - 1];
        if !".,;:!?'\"`])}>".contains(ultimo) {
            break;
        }
        if ultimo == ')' {
            let abre = url[..fim].iter().filter(|&&c| c == '(').count();
            let fecha = url[..fim].iter().filter(|&&c| c == ')').count();
            if abre >= fecha {
                break;
            }
        }
        fim -= 1;
    }
    fim
}

fn digitos(t: &[char], i: usize) -> usize {
    let mut j = i;
    while j < t.len() && t[j].is_ascii_digit() {
        j += 1;
    }
    j
}

fn numero(t: &[char], a: usize, b: usize) -> Option<u32> {
    t[a..b].iter().collect::<String>().parse().ok()
}

/// O corpo do caminho a partir de `i` (sem prefixo): segmentos separados por uma barra, terminando num segmento.
fn corpo(t: &[char], mut i: usize) -> Option<usize> {
    let mut fim = None;
    loop {
        let comeco = i;
        while i < t.len() && de_segmento(t[i]) {
            i += 1;
        }
        if i == comeco {
            return fim;
        }
        fim = Some(i);
        if i + 1 < t.len() && separador(t[i]) && de_segmento(t[i + 1]) {
            i += 1;
        } else {
            return fim;
        }
    }
}

/// Casamento do caminho em `i`: (fim do caminho, fim com sufixo, linha, coluna).
fn casar_caminho(t: &[char], i: usize) -> Option<(usize, usize, Option<u32>, Option<u32>)> {
    let mut prefixos = Vec::new();
    if i + 2 < t.len() && t[i].is_ascii_alphabetic() && t[i + 1] == ':' && separador(t[i + 2]) {
        prefixos.push(i + 3);
    }
    let pontos = t[i..].iter().take(2).take_while(|&&c| c == '.').count();
    for n in (1..=pontos).rev() {
        if i + n < t.len() && separador(t[i + n]) {
            prefixos.push(i + n + 1);
        }
    }
    prefixos.push(i);
    let fim = prefixos.into_iter().find_map(|p| corpo(t, p))?;
    // Sufixo: `:l`, `:l:c`, `:l-l2` ou `(l,c)`.
    let mut sufixo = (fim, None, None);
    if fim < t.len() && t[fim] == ':' {
        let d = digitos(t, fim + 1);
        if d > fim + 1 {
            sufixo = (d, numero(t, fim + 1, d), None);
            if d < t.len() && (t[d] == ':' || t[d] == '-') {
                let e = digitos(t, d + 1);
                if e > d + 1 {
                    let col = if t[d] == ':' { numero(t, d + 1, e) } else { None };
                    sufixo = (e, sufixo.1, col);
                }
            }
        }
    } else if fim < t.len() && t[fim] == '(' {
        let d = digitos(t, fim + 1);
        if d > fim + 1 {
            let linha = numero(t, fim + 1, d);
            if d < t.len() && t[d] == ')' {
                sufixo = (d + 1, linha, None);
            } else if d < t.len() && t[d] == ',' {
                let mut k = d + 1;
                if k < t.len() && t[k].is_whitespace() {
                    k += 1;
                }
                let e = digitos(t, k);
                if e > k && e < t.len() && t[e] == ')' {
                    sufixo = (e + 1, linha, numero(t, k, e));
                }
            }
        }
    }
    Some((fim, sufixo.0, sufixo.1, sufixo.2))
}

fn tem_extensao(p: &[char]) -> bool {
    // `\.[A-Za-z]\w{0,9}$`
    let Some(ponto) = p.iter().rposition(|&c| c == '.') else { return false };
    let ext = &p[ponto + 1..];
    !ext.is_empty() && ext.len() <= 10 && ext[0].is_ascii_alphabetic() && ext.iter().all(|&c| palavra(c))
}

pub fn achar_links(texto: &str) -> Vec<Link> {
    let t: Vec<char> = texto.chars().collect();
    let mut saida = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let inicia = (i == 0 || !palavra(t[i - 1]))
            && (t[i..].starts_with(&['h', 't', 't', 'p', ':', '/', '/']) || t[i..].starts_with(&['h', 't', 't', 'p', 's', ':', '/', '/']));
        if !inicia {
            i += 1;
            continue;
        }
        let corpo_inicio = i + if t[i + 4] == 's' { 8 } else { 7 };
        let mut j = corpo_inicio;
        while j < t.len() && !fim_de_url(t[j]) {
            j += 1;
        }
        if j == corpo_inicio {
            i += 1;
            continue;
        }
        let fim = i + aparar_url(&t[i..j]);
        if fim - i > "https://".len() {
            saida.push(Link::Url { inicio: i, fim, url: t[i..fim].iter().collect() });
        }
        i = j;
    }
    let em_url = |p: usize, s: &[Link]| s.iter().any(|l| matches!(l, Link::Url { inicio, fim, .. } if p >= *inicio && p < *fim));
    let mut caminhos = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if i > 0 && bloqueia_inicio(t[i - 1]) {
            i += 1;
            continue;
        }
        let Some((fim_caminho, fim, linha, coluna)) = casar_caminho(&t, i) else {
            i += 1;
            continue;
        };
        let proximo = fim;
        if !em_url(i, &saida) {
            let mut caminho = &t[i..fim_caminho];
            let com_sufixo = fim > fim_caminho;
            if !com_sufixo {
                while caminho.last().is_some_and(|&c| c == '.' || c == '-') {
                    caminho = &caminho[..caminho.len() - 1];
                }
            }
            let tem_sep = caminho.iter().any(|&c| separador(c));
            let so_pontos = {
                let sem = if caminho.last().is_some_and(|&c| separador(c)) { &caminho[..caminho.len() - 1] } else { caminho };
                !sem.is_empty() && sem.len() <= 2 && sem.iter().all(|&c| c == '.')
            };
            if (tem_sep || tem_extensao(caminho)) && !so_pontos && !caminho.is_empty() {
                let fim_link = if com_sufixo { fim } else { i + caminho.len() };
                caminhos.push(Link::Caminho {
                    inicio: i,
                    fim: fim_link,
                    caminho: caminho.iter().collect(),
                    linha: if com_sufixo { linha } else { None },
                    coluna: if com_sufixo { coluna } else { None },
                });
            }
        }
        i = proximo.max(i + 1);
    }
    saida.extend(caminhos);
    saida.sort_by_key(|l| l.intervalo().0);
    saida
}

/// `.` e `..` resolvidos; `None` se sobe acima do começo.
fn normalizar(segmentos: &[&str]) -> Option<Vec<String>> {
    let mut saida: Vec<String> = Vec::new();
    for s in segmentos {
        match *s {
            "" | "." => {}
            ".." => {
                saida.pop()?;
            }
            outro => saida.push(outro.to_string()),
        }
    }
    Some(saida)
}

/// Absoluto com `/`, ou relativo a `raiz` normalizado. `None` se relativo sem raiz ou se sai acima da unidade.
pub fn resolver_caminho(caminho: &str, raiz: Option<&str>) -> Option<String> {
    let p = caminho.replace('\\', "/");
    let b = p.as_bytes();
    let absoluto = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/';
    let abs = if absoluto {
        p
    } else {
        let raiz = raiz?.replace('\\', "/");
        format!("{}/{}", raiz.trim_end_matches('/'), p)
    };
    let mut partes = abs.split('/');
    let unidade = partes.next()?.to_string();
    let resto: Vec<&str> = partes.collect();
    let mut tudo = vec![unidade];
    tudo.extend(normalizar(&resto)?);
    Some(tudo.join("/"))
}

/// Caminho relativo (com `/`) de `abs` dentro de `projeto`, ou `None` se fora. Sem diferença de caixa nem de barra.
pub fn relativo_a(projeto: &str, abs: &str) -> Option<String> {
    if projeto.is_empty() {
        return None;
    }
    let raiz = projeto.trim_end_matches(['\\', '/']).replace('\\', "/");
    let cheio = abs.replace('\\', "/");
    let cabeca = cheio.get(..raiz.len())?;
    if !cabeca.eq_ignore_ascii_case(&raiz) || cheio.as_bytes().get(raiz.len()) != Some(&b'/') {
        return None;
    }
    Some(cheio[raiz.len() + 1..].to_string())
}

pub fn eh_url_web(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

#[cfg(test)]
mod testes {
    // Porte de tests/check-term-links.mjs.
    use super::*;

    #[derive(Debug, PartialEq)]
    enum Um {
        Caminho { caminho: String, linha: Option<u32>, coluna: Option<u32>, texto: String },
        Url { url: String, texto: String },
    }

    fn um(texto: &str) -> Um {
        let links = achar_links(texto);
        assert_eq!(links.len(), 1, "{texto}: {links:?}");
        let chars: Vec<char> = texto.chars().collect();
        let fatia = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
        match links.into_iter().next().unwrap() {
            Link::Url { inicio, fim, url } => Um::Url { url, texto: fatia(inicio, fim) },
            Link::Caminho { inicio, fim, caminho, linha, coluna } => Um::Caminho { caminho, linha, coluna, texto: fatia(inicio, fim) },
        }
    }

    fn cam(caminho: &str, linha: Option<u32>, coluna: Option<u32>, texto: &str) -> Um {
        Um::Caminho { caminho: caminho.into(), linha, coluna, texto: texto.into() }
    }

    fn texto(u: Um) -> String {
        match u {
            Um::Caminho { texto, .. } | Um::Url { texto, .. } => texto,
        }
    }

    #[test]
    fn relativos() {
        assert_eq!(um("veja src/app.tsx agora"), cam("src/app.tsx", None, None, "src/app.tsx"));
        assert_eq!(um("./src/app.tsx"), cam("./src/app.tsx", None, None, "./src/app.tsx"));
        assert_eq!(um("em ..\\x\\y.ts"), cam("..\\x\\y.ts", None, None, "..\\x\\y.ts"));
        assert_eq!(um("README.md"), cam("README.md", None, None, "README.md"));
    }

    #[test]
    fn absolutos_do_windows() {
        assert!(matches!(um("D:\\orquestrador\\src\\app.tsx"), Um::Caminho { caminho, .. } if caminho == "D:\\orquestrador\\src\\app.tsx"));
        assert!(matches!(um("at (D:/orquestrador/src/app.tsx:3:4)"), Um::Caminho { caminho, .. } if caminho == "D:/orquestrador/src/app.tsx"));
    }

    #[test]
    fn sufixos_de_posicao() {
        assert_eq!(um("src/a.ts:12"), cam("src/a.ts", Some(12), None, "src/a.ts:12"));
        assert_eq!(um("src/a.ts:12:5"), cam("src/a.ts", Some(12), Some(5), "src/a.ts:12:5"));
        assert_eq!(um("src/a.ts(12,5)"), cam("src/a.ts", Some(12), Some(5), "src/a.ts(12,5)"));
        assert_eq!(um("src/a.ts:12-20"), cam("src/a.ts", Some(12), None, "src/a.ts:12-20"));
    }

    #[test]
    fn pontuacao_final() {
        for t in ["src/a.ts.", "src/a.ts,", "(src/a.ts)", "'src/a.ts'", "\"src/a.ts\"", "`src/a.ts`"] {
            assert_eq!(texto(um(t)), "src/a.ts", "{t}");
        }
        assert_eq!(texto(um("src/a.ts:3.")), "src/a.ts:3");
    }

    #[test]
    fn urls() {
        assert_eq!(
            um("abra https://example.com/a?b=1."),
            Um::Url { url: "https://example.com/a?b=1".into(), texto: "https://example.com/a?b=1".into() }
        );
        assert!(matches!(um("(veja https://example.com/x)"), Um::Url { url, .. } if url == "https://example.com/x"));
        assert!(matches!(um("https://en.wikipedia.org/wiki/A_(b)"), Um::Url { url, .. } if url == "https://en.wikipedia.org/wiki/A_(b)"));
        assert!(matches!(um("http://localhost:5173/src/a.ts,"), Um::Url { url, .. } if url == "http://localhost:5173/src/a.ts"));
    }

    #[test]
    fn texto_sem_link() {
        for t in ["", "olá mundo", "versão 1.5 e 2.0", "a: b", "..."] {
            assert_eq!(achar_links(t), vec![], "{t}");
        }
    }

    #[test]
    fn resolucao() {
        assert_eq!(resolver_caminho("./src/../a.ts", Some("D:\\proj")).as_deref(), Some("D:/proj/a.ts"));
        assert_eq!(resolver_caminho("D:\\x\\y.ts", Some("C:\\p")).as_deref(), Some("D:/x/y.ts"));
        assert_eq!(resolver_caminho("a.ts", None), None);
        assert_eq!(resolver_caminho("../../a", Some("C:/p")), None);
    }

    #[test]
    fn relativo_ao_projeto() {
        assert_eq!(relativo_a("D:\\Proj\\", "d:/proj/src/a.ts").as_deref(), Some("src/a.ts"));
        assert_eq!(relativo_a("D:\\proj", "D:/projeto/a.ts"), None);
        assert_eq!(relativo_a("", "D:/a"), None);
    }
}
