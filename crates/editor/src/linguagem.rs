//! Linguagem de um arquivo pelo nome, igual ao `shikiLanguage` de `src/editor/shiki-colors.ts`: metadados do Linguist
//! ligados às gramáticas do Shiki, `.env` como dotenv e, no fim, as descrições do `@codemirror/language-data`. A tabela
//! vem de `assets/linguagens.json`, gerada por `gerar-dados.mjs` a partir dos mesmos pacotes.
use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Codemirror {
    name: String,
    alias: Vec<String>,
    extensions: Vec<String>,
    filename: Option<String>,
    #[serde(default)]
    flags: String,
}

#[derive(Deserialize)]
pub(crate) struct Shiki {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
}

#[derive(Deserialize)]
struct Dados {
    names: HashMap<String, String>,
    filenames: HashMap<String, Option<String>>,
    extensions: HashMap<String, Option<String>>,
    codemirror: Vec<Codemirror>,
    shiki: Vec<Shiki>,
}

struct Tabela {
    dados: Dados,
    nomes_cm: Vec<Option<Regex>>,
}

static TABELA: LazyLock<Tabela> = LazyLock::new(|| {
    let dados: Dados = serde_json::from_str(include_str!("../assets/linguagens.json")).expect("linguagens.json válido");
    let nomes_cm = dados
        .codemirror
        .iter()
        .map(|d| d.filename.as_ref().and_then(|f| RegexBuilder::new(f).case_insensitive(d.flags.contains('i')).build().ok()))
        .collect();
    Tabela { dados, nomes_cm }
});

static DOTENV: LazyLock<Regex> = LazyLock::new(|| RegexBuilder::new(r"^\.env(?:\..+)?$|\.env$").case_insensitive(true).build().unwrap());

fn nome(caminho: &str) -> &str {
    let i = caminho.rfind(['/', '\\']).map_or(0, |i| i + 1);
    &caminho[i..]
}

/// Id da gramática Shiki do arquivo (`typescript`, `rust`…), ou `None` sem gramática.
pub fn linguagem(caminho: &str) -> Option<String> {
    let nome = nome(caminho);
    if DOTENV.is_match(nome) {
        return Some("dotenv".into());
    }
    let t = &*TABELA;
    let minusculo = nome.to_lowercase();
    if let Some(Some(l)) = t.dados.filenames.get(&minusculo) {
        return Some(l.clone());
    }
    for (i, _) in minusculo.match_indices('.') {
        if let Some(Some(l)) = t.dados.extensions.get(&minusculo[i..]) {
            return Some(l.clone());
        }
    }
    // `LanguageDescription.matchFilename`: primeiro o nome, depois a última extensão.
    let desc = t
        .dados
        .codemirror
        .iter()
        .zip(&t.nomes_cm)
        .find(|(_, r)| r.as_ref().is_some_and(|r| r.is_match(nome)))
        .map(|(d, _)| d)
        .or_else(|| {
            let ext = nome.rsplit_once('.')?.1;
            (!ext.is_empty()).then_some(())?;
            t.dados.codemirror.iter().find(|d| d.extensions.iter().any(|e| e == ext))
        })?;
    std::iter::once(&desc.name).chain(&desc.alias).find_map(|c| t.dados.names.get(&c.to_lowercase()).cloned())
}

/// Nome e apelidos da gramática Shiki, para achar a do `syntect`.
pub(crate) fn info_shiki(id: &str) -> Option<&'static Shiki> {
    TABELA.dados.shiki.iter().find(|s| s.id == id)
}

/// Arquivo Markdown (`.md`, `.markdown`, `.mdx`): ganha o seletor Preview | Markdown.
pub fn eh_markdown(caminho: &str) -> bool {
    let n = nome(caminho).to_lowercase();
    n.ends_with(".md") || n.ends_with(".markdown") || n.ends_with(".mdx")
}

#[cfg(test)]
mod testes {
    //! Porte de `tests/check-shiki-language.mjs`.
    use super::*;

    #[test]
    fn identifica_por_linguist_e_shiki() {
        let casos: &[(&str, Option<&str>)] = &[
            ("player.gd", Some("gdscript")),
            ("level.tscn", Some("gdresource")),
            ("item.tres", Some("gdresource")),
            ("project.godot", Some("gdresource")),
            ("code.exs", Some("elixir")),
            ("index.cjs", Some("javascript")),
            ("index.mjs", Some("javascript")),
            ("script.cmd", Some("bat")),
            (".env.example", Some("dotenv")),
            ("settings.env", Some("dotenv")),
            ("Dockerfile", Some("docker")),
            ("Cargo.toml", Some("toml")),
            ("code.ts", Some("typescript")),
            ("code.tsx", Some("tsx")),
            ("code.js", Some("javascript")),
            ("code.jsx", Some("javascript")),
            ("code.json", Some("json")),
            ("code.html", Some("html")),
            ("code.css", Some("css")),
            ("code.scss", Some("scss")),
            ("code.yaml", Some("yaml")),
            ("code.md", Some("markdown")),
            ("code.c", Some("c")),
            ("code.cpp", Some("cpp")),
            ("code.h", Some("c")),
            ("code.cs", Some("csharp")),
            ("code.fs", Some("fsharp")),
            ("code.rs", Some("rust")),
            ("code.py", Some("python")),
            ("code.astro", Some("astro")),
            ("code.ts.unknown", None),
            ("typescript", None),
            ("unknown.xyzdesconhecida", None),
        ];
        for (arquivo, esperado) in casos {
            assert_eq!(linguagem(arquivo).as_deref(), *esperado, "{arquivo}");
        }
    }

    #[test]
    fn markdown() {
        assert!(eh_markdown("docs/guia.md"));
        assert!(eh_markdown("A.MDX"));
        assert!(!eh_markdown("a.mdl"));
    }
}
