//! Regras puras de caminho e nome (porte de `src/shared/file-tab-actions.ts` e `src/shared/paths.ts`): texto copiado,
//! caminho relativo ao projeto, pasta que contém e nomes que o Windows recusa.

/// Qual texto a ação de copiar produz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoCopia {
    Relativo,
    Absoluto,
    Navegacao,
}

/// Linha e coluna (1-based) do cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cursor {
    pub line: u64,
    pub col: u64,
}

/// Caminho absoluto: unidade (`C:\` ou `C:/`), UNC (`\\`) ou raiz (`/`).
pub fn eh_absoluto(caminho: &str) -> bool {
    let b = caminho.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || caminho.starts_with("\\\\")
        || caminho.starts_with('/')
}

/// Nome do arquivo (depois da última `/`).
pub fn nome_arquivo(caminho: &str) -> &str {
    caminho.rsplit('/').next().unwrap_or(caminho)
}

/// Nome e pasta de um caminho com `/` (pasta vazia é a raiz).
pub fn separar(caminho: &str) -> (&str, &str) {
    match caminho.rfind('/') {
        Some(i) => (&caminho[i + 1..], &caminho[..i]),
        None => (caminho, ""),
    }
}

/// Junta pasta e nome num caminho relativo ao projeto (pasta vazia é a raiz).
pub fn juntar(pasta: &str, nome: &str) -> String {
    if pasta.is_empty() {
        nome.to_string()
    } else {
        format!("{pasta}/{nome}")
    }
}

fn sem_barras_finais(s: &str) -> &str {
    s.trim_end_matches(['\\', '/'])
}

/// Texto copiado por cada ação, com o separador do projeto. Navegação traz linha e coluna; sem cursor, `None`.
pub fn texto_caminho(tipo: TipoCopia, caminho: &str, projeto: &str, cursor: Option<Cursor>) -> Option<String> {
    let absoluto = eh_absoluto(caminho);
    let sep = if projeto.contains('\\') || (absoluto && caminho.contains('\\')) { '\\' } else { '/' };
    let trocar = |s: &str| s.chars().map(|c| if c == '\\' || c == '/' { sep } else { c }).collect::<String>();
    if absoluto {
        let abs = trocar(caminho);
        let raiz = sem_barras_finais(projeto);
        let comparavel = barras_unicas(&abs).to_lowercase();
        let raiz_comparavel = barras_unicas(raiz).to_lowercase();
        let relativo = (!raiz.is_empty() && comparavel.starts_with(&format!("{raiz_comparavel}/")))
            .then(|| abs[raiz.len() + 1..].to_string());
        return match tipo {
            TipoCopia::Relativo => relativo.map(|r| trocar(&r)),
            TipoCopia::Absoluto => Some(abs),
            TipoCopia::Navegacao => cursor.map(|c| format!("{abs}:{}:{}", c.line, c.col)),
        };
    }
    if caminho.is_empty() {
        return match tipo {
            TipoCopia::Relativo => Some(".".into()),
            TipoCopia::Absoluto => {
                let b = projeto.as_bytes();
                // Raiz de unidade (`D:\`) mantém a barra: sem ela, `D:` vira o diretório atual da unidade.
                let raiz_unidade = b.len() >= 3
                    && b[0].is_ascii_alphabetic()
                    && b[1] == b':'
                    && projeto[2..].chars().all(|c| c == '\\' || c == '/');
                if raiz_unidade {
                    Some(projeto.to_string())
                } else {
                    let s = sem_barras_finais(projeto);
                    Some(if s.is_empty() { projeto.to_string() } else { s.to_string() })
                }
            }
            TipoCopia::Navegacao => None,
        };
    }
    let relativo = caminho.split('/').collect::<Vec<_>>().join(&sep.to_string());
    if tipo == TipoCopia::Relativo {
        return Some(relativo);
    }
    let abs = format!("{}{sep}{relativo}", sem_barras_finais(projeto));
    match tipo {
        TipoCopia::Absoluto => Some(abs),
        _ => cursor.map(|c| format!("{abs}:{}:{}", c.line, c.col)),
    }
}

fn barras_unicas(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut anterior = false;
    for c in s.chars() {
        let barra = c == '\\' || c == '/';
        if barra {
            if !anterior {
                out.push('/');
            }
        } else {
            out.push(c);
        }
        anterior = barra;
    }
    out
}

const RESERVADOS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Motivo para um nome de arquivo não servir no Windows; `None` se serve. Mesmas mensagens de `check_name` do core.
pub fn erro_nome(nome: &str) -> Option<String> {
    if nome.trim().is_empty() {
        return Some("Digite um nome para o arquivo.".into());
    }
    if nome == "." || nome == ".." || nome.len() > 255 {
        return Some("Esse nome não é permitido.".into());
    }
    if nome.chars().any(|c| (c as u32) < 0x20 || "<>:\"/\\|?*".contains(c)) {
        return Some("O nome tem caractere que o Windows não aceita: < > : \" / \\ | ? *".into());
    }
    if nome.starts_with(' ') || nome.ends_with(' ') || nome.ends_with('.') {
        return Some("O nome não pode começar nem terminar com espaço, nem terminar com ponto.".into());
    }
    if nome.eq_ignore_ascii_case(".git") {
        return Some("A pasta .git não pode ser alterada aqui.".into());
    }
    let base = nome.split('.').next().unwrap_or("").to_uppercase();
    if RESERVADOS.contains(&base.as_str()) {
        return Some(format!("“{nome}” é um nome reservado pelo Windows."));
    }
    None
}

/// Segmentos sem vazios e `.`, com `..` aplicado; `None` se `..` sair acima do primeiro.
pub fn normalizar_segmentos<'a>(segmentos: impl IntoIterator<Item = &'a str>) -> Option<Vec<&'a str>> {
    let mut out = Vec::new();
    for s in segmentos {
        if s.is_empty() || s == "." {
            continue;
        }
        if s == ".." {
            out.pop()?;
        } else {
            out.push(s);
        }
    }
    Some(out)
}

/// Caminho relativo (`/`) de `abs` dentro de `projeto`, sem diferença de caixa nem de barra; `None` fora dele, no
/// próprio projeto ou sem projeto.
pub fn relativo_a(projeto: &str, abs: &str) -> Option<String> {
    if projeto.is_empty() {
        return None;
    }
    let raiz = sem_barras_finais(projeto).replace('\\', "/");
    let cheio = abs.replace('\\', "/");
    let cabeca = cheio.get(..raiz.len())?;
    if !cabeca.eq_ignore_ascii_case(&raiz) || cheio.as_bytes().get(raiz.len()) != Some(&b'/') {
        return None;
    }
    Some(cheio[raiz.len() + 1..].to_string())
}

/// `caminho` é `base` ou está dentro dele (relativos, com `/`), sem diferença de caixa.
pub fn dentro_de(caminho: &str, base: &str) -> bool {
    let p = caminho.to_lowercase();
    let b = base.to_lowercase();
    p == b || p.starts_with(&format!("{b}/"))
}

/// Caminho absoluto com `\` do arquivo aberto (relativo ao projeto ou já absoluto).
pub fn absoluto_no_projeto(projeto: &str, caminho: &str) -> String {
    if caminho.is_empty() {
        return String::new();
    }
    if eh_absoluto(caminho) {
        return caminho.replace('/', "\\");
    }
    format!("{}\\{}", sem_barras_finais(projeto), caminho.replace('/', "\\"))
}

#[cfg(test)]
mod testes {
    //! Porte de `tests/check-file-tabs.mjs`.
    use super::*;
    use TipoCopia::*;

    fn c(line: u64, col: u64) -> Option<Cursor> {
        Some(Cursor { line, col })
    }

    #[test]
    fn relativo_usa_separador_do_windows() {
        assert_eq!(texto_caminho(Relativo, "src/App.tsx", "D:\\orquestrador", None).as_deref(), Some("src\\App.tsx"));
    }

    #[test]
    fn absoluto_junta_sem_barra_duplicada() {
        assert_eq!(
            texto_caminho(Absoluto, "src/App.tsx", "D:\\orquestrador\\", None).as_deref(),
            Some("D:\\orquestrador\\src\\App.tsx")
        );
        assert_eq!(texto_caminho(Absoluto, "src/App.tsx", "/home/u/proj", None).as_deref(), Some("/home/u/proj/src/App.tsx"));
    }

    #[test]
    fn arquivo_solto_mantem_absoluto() {
        assert_eq!(texto_caminho(Absoluto, "C:\\arquivos\\solto.ts", "", None).as_deref(), Some("C:\\arquivos\\solto.ts"));
        assert_eq!(texto_caminho(Navegacao, "C:\\arquivos\\solto.ts", "", c(3, 2)).as_deref(), Some("C:\\arquivos\\solto.ts:3:2"));
        assert_eq!(texto_caminho(Relativo, "C:\\arquivos\\solto.ts", "", None), None);
        assert_eq!(texto_caminho(Relativo, "D:/proj/src/a.ts", "D:\\proj", None).as_deref(), Some("src\\a.ts"));
    }

    #[test]
    fn navegacao_com_e_sem_cursor() {
        assert_eq!(
            texto_caminho(Navegacao, "src/App.tsx", "D:\\orquestrador", c(12, 3)).as_deref(),
            Some("D:\\orquestrador\\src\\App.tsx:12:3")
        );
        assert_eq!(texto_caminho(Navegacao, "src/App.tsx", "D:\\orquestrador", None), None);
    }

    #[test]
    fn raiz_do_projeto() {
        assert_eq!(texto_caminho(Relativo, "", "D:\\orquestrador", None).as_deref(), Some("."));
        assert_eq!(texto_caminho(Absoluto, "", "D:\\orquestrador\\", None).as_deref(), Some("D:\\orquestrador"));
        assert_eq!(texto_caminho(Absoluto, "", "/home/u/proj", None).as_deref(), Some("/home/u/proj"));
        assert_eq!(texto_caminho(Navegacao, "", "D:\\orquestrador", None), None);
        assert_eq!(texto_caminho(Absoluto, "", "D:\\", None).as_deref(), Some("D:\\"));
        assert_eq!(texto_caminho(Absoluto, "", "D:/", None).as_deref(), Some("D:/"));
        assert_eq!(texto_caminho(Relativo, "", "D:\\", None).as_deref(), Some("."));
        assert_eq!(texto_caminho(Absoluto, "", "/", None).as_deref(), Some("/"));
    }

    #[test]
    fn relativo_a_partir_do_absoluto() {
        assert_eq!(relativo_a("D:\\orquestrador", "D:\\orquestrador\\src\\App.tsx").as_deref(), Some("src/App.tsx"));
        assert_eq!(relativo_a("D:\\Orquestrador\\", "d:/orquestrador/src/App.tsx").as_deref(), Some("src/App.tsx"));
        assert_eq!(relativo_a("D:\\orquestrador", "D:\\orquestrador-antigo\\x.ts"), None);
        assert_eq!(relativo_a("D:\\orquestrador", "C:\\outra\\x.ts"), None);
        assert_eq!(relativo_a("D:\\orquestrador", "D:\\orquestrador"), None);
        assert_eq!(relativo_a("", "/x/y.ts"), None);
    }

    #[test]
    fn caminho_absoluto() {
        for p in ["C:\\x", "c:/x", "\\\\srv\\x", "/x"] {
            assert!(eh_absoluto(p), "{p}");
        }
        for p in ["", "x/y.ts", "C:x", "C:"] {
            assert!(!eh_absoluto(p), "{p}");
        }
    }

    #[test]
    fn dentro_de_pasta() {
        assert!(dentro_de("src", "src"));
        assert!(dentro_de("src/App.tsx", "src"));
        assert!(!dentro_de("srcx/App.tsx", "src"));
        assert!(!dentro_de("src2/a.ts", "src"));
        assert!(!dentro_de("src2", "src"));
        assert!(dentro_de("readme.md", "README.md"));
        assert!(dentro_de("Pasta/Sub/a.ts", "pasta"));
        assert!(!dentro_de("Pasta2/a.ts", "pasta"));
        assert!(!dentro_de("SRC2/a.ts", "src"));
        assert!(dentro_de("Src/App.tsx", "SRC"));
    }

    #[test]
    fn nomes() {
        assert_eq!(erro_nome(".git").as_deref(), Some("A pasta .git não pode ser alterada aqui."));
        assert_eq!(erro_nome(".GIT").as_deref(), Some("A pasta .git não pode ser alterada aqui."));
        assert_eq!(erro_nome(".gitignore"), None);
        for n in ["novo.txt", "Arquivo com espaço.md", "ação.ts", "conta.con", ".env"] {
            assert_eq!(erro_nome(n), None, "{n}");
        }
        for n in [
            "", "   ", ".", "..", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a|b", "a\u{1}b", " x", "x ", "x.", "CON",
            "nul.txt", "com1.md", "LPT9.log",
        ] {
            assert!(erro_nome(n).is_some(), "{n:?} deveria ser recusado");
        }
        assert_eq!(erro_nome("a\\b").as_deref(), Some("O nome tem caractere que o Windows não aceita: < > : \" / \\ | ? *"));
        assert_eq!(erro_nome("CON.txt").as_deref(), Some("“CON.txt” é um nome reservado pelo Windows."));
    }

    #[test]
    fn segmentos() {
        assert_eq!(normalizar_segmentos(["a", "", ".", "b", "..", "c"]), Some(vec!["a", "c"]));
        assert_eq!(normalizar_segmentos([".."]), None);
    }
}
