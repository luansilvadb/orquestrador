//! Formatar Documento (Shift+Alt+F), porte de `src/editor/format.ts`. O core descobre o formatador do arquivo
//! (`ide::format::info`); no caso Prettier, que o React roda em JavaScript, aqui os crates Rust do ecossistema dprint
//! fazem o trabalho com as opções da config do Prettier do projeto mapeadas para a saída mais próxima possível; os outros
//! motores (Python, Rust, Go) seguem pelo CLI do core.
use crate::armazem::FimDeLinha;
use orquestrador_core::ide::format::{self, Info};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formatado {
    pub texto: String,
    /// Fim de linha pedido pela config (`endOfLine`), quando não é `auto`.
    pub quebra: Option<FimDeLinha>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Falha {
    /// Sem formatador para o arquivo (vira aviso informativo).
    Indisponivel(String),
    Erro(String),
}

/// Formata `texto` (com `\n`) do arquivo absoluto `caminho`.
pub fn formatar(caminho: &Path, texto: &str) -> Result<Formatado, Falha> {
    match format::info(caminho).map_err(Falha::Erro)? {
        Info::Unavailable { reason } => Err(Falha::Indisponivel(reason)),
        Info::Cli { .. } => {
            let t = format::format_cli(caminho, texto).map_err(Falha::Erro)?;
            let q = crate::armazem::quebra_de(&t);
            Ok(Formatado { texto: t, quebra: Some(q) })
        }
        Info::Prettier { config, config_kind, options } => {
            let mut opcoes = options;
            if let Some(c) = config {
                for (k, v) in ler_config(&c, config_kind)? {
                    opcoes.insert(k, v);
                }
            }
            let o = Opcoes::de(&opcoes);
            let saida = prettier(caminho, texto, &o).map_err(Falha::Erro)?;
            let quebra = match o.fim_de_linha.as_str() {
                "crlf" => Some(FimDeLinha::CrLf),
                "cr" => Some(FimDeLinha::Cr),
                "auto" => None,
                _ => Some(FimDeLinha::Lf),
            };
            Ok(Formatado { texto: saida, quebra })
        }
    }
}

fn ler_config(texto: &str, tipo: &str) -> Result<Map<String, Value>, Falha> {
    let valor: Value = match tipo {
        "json" => json5::from_str(texto).map_err(|e| Falha::Erro(e.to_string()))?,
        "yaml" => serde_yaml::from_str(texto).map_err(|e| Falha::Erro(e.to_string()))?,
        _ => json5::from_str(texto).or_else(|_| serde_yaml::from_str(texto)).map_err(|e: serde_yaml::Error| Falha::Erro(e.to_string()))?,
    };
    match valor {
        Value::Object(m) => Ok(m),
        _ => Err(Falha::Erro("A configuração do Prettier precisa ser um objeto.".into())),
    }
}

/// As opções do Prettier que importam, com os padrões do Prettier 3.
#[derive(Clone, Debug, PartialEq)]
pub struct Opcoes {
    pub largura: u32,
    pub tab: u8,
    pub tabs: bool,
    pub ponto_e_virgula: bool,
    pub aspas_simples: bool,
    pub aspas_simples_jsx: bool,
    pub virgula_final: String,
    pub espaco_chaves: bool,
    pub parenteses_seta: String,
    pub aspas_props: String,
    pub prosa: String,
    pub fim_de_linha: String,
    pub mesma_linha_jsx: bool,
}

impl Opcoes {
    pub fn de(m: &Map<String, Value>) -> Opcoes {
        let n = |k: &str, p: u64| m.get(k).and_then(Value::as_u64).unwrap_or(p);
        let b = |k: &str, p: bool| m.get(k).and_then(Value::as_bool).unwrap_or(p);
        let s = |k: &str, p: &str| m.get(k).and_then(Value::as_str).unwrap_or(p).to_string();
        Opcoes {
            largura: n("printWidth", 80) as u32,
            tab: n("tabWidth", 2) as u8,
            tabs: b("useTabs", false),
            ponto_e_virgula: b("semi", true),
            aspas_simples: b("singleQuote", false),
            aspas_simples_jsx: b("jsxSingleQuote", false),
            virgula_final: s("trailingComma", "all"),
            espaco_chaves: b("bracketSpacing", true),
            parenteses_seta: s("arrowParens", "always"),
            aspas_props: s("quoteProps", "as-needed"),
            prosa: s("proseWrap", "preserve"),
            fim_de_linha: s("endOfLine", "lf"),
            mesma_linha_jsx: b("bracketSameLine", false),
        }
    }
}

fn extensao(caminho: &Path) -> String {
    caminho.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Escolhe o motor pela extensão, como o `parserFor` do React.
pub fn prettier(caminho: &Path, texto: &str, o: &Opcoes) -> Result<String, String> {
    let ext = extensao(caminho);
    match ext.as_str() {
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" => typescript(caminho, texto, o),
        "json" | "jsonc" | "json5" => json(caminho, texto, o),
        "css" | "scss" | "less" => css(texto, &ext, o),
        "html" | "htm" | "vue" => html(texto, &ext, o),
        "md" | "markdown" => markdown(texto, o),
        _ => yaml(texto, o),
    }
}

fn typescript(caminho: &Path, texto: &str, o: &Opcoes) -> Result<String, String> {
    use dprint_plugin_typescript::configuration::*;
    let mut b = ConfigurationBuilder::new();
    b.line_width(o.largura)
        .indent_width(o.tab)
        .use_tabs(o.tabs)
        .quote_style(if o.aspas_simples { QuoteStyle::AlwaysSingle } else { QuoteStyle::AlwaysDouble })
        .jsx_quote_style(if o.aspas_simples_jsx { JsxQuoteStyle::PreferSingle } else { JsxQuoteStyle::PreferDouble })
        .semicolons(if o.ponto_e_virgula { Semicolons::Always } else { Semicolons::Asi })
        .trailing_commas(if o.virgula_final == "none" { TrailingCommas::Never } else { TrailingCommas::OnlyMultiLine })
        .quote_props(match o.aspas_props.as_str() {
            "preserve" => QuoteProps::Preserve,
            "consistent" => QuoteProps::Consistent,
            _ => QuoteProps::AsNeeded,
        })
        .arrow_function_use_parentheses(if o.parenteses_seta == "avoid" { UseParentheses::PreferNone } else { UseParentheses::Force })
        .object_expression_space_surrounding_properties(o.espaco_chaves)
        .object_pattern_space_surrounding_properties(o.espaco_chaves)
        .import_declaration_space_surrounding_named_imports(o.espaco_chaves)
        .export_declaration_space_surrounding_named_exports(o.espaco_chaves)
        .type_literal_space_surrounding_properties(o.espaco_chaves)
        .module_sort_import_declarations(SortOrder::Maintain)
        .module_sort_export_declarations(SortOrder::Maintain)
        .import_declaration_sort_named_imports(SortOrder::Maintain)
        .export_declaration_sort_named_exports(SortOrder::Maintain)
        .operator_position(OperatorPosition::SameLine)
        .brace_position(BracePosition::SameLine)
        .next_control_flow_position(NextControlFlowPosition::SameLine)
        .jsx_multi_line_parens(JsxMultiLineParens::Always)
        .jsx_bracket_position(if o.mesma_linha_jsx { SameOrNextLinePosition::SameLine } else { SameOrNextLinePosition::NextLine })
        .prefer_hanging(false)
        .ignore_node_comment_text("prettier-ignore");
    let config = b.build();
    let r = dprint_plugin_typescript::format_text(dprint_plugin_typescript::FormatTextOptions {
        path: caminho,
        extension: None,
        text: texto.into(),
        config: &config,
        external_formatter: None,
        arena_pool: None,
    })
    .map_err(|e| e.to_string())?;
    Ok(r.unwrap_or_else(|| texto.to_string()))
}

fn json(caminho: &Path, texto: &str, o: &Opcoes) -> Result<String, String> {
    use dprint_plugin_json::configuration::*;
    let mut b = ConfigurationBuilder::new();
    b.line_width(o.largura).indent_width(o.tab).use_tabs(o.tabs).prefer_single_line(false).ignore_node_comment_text("prettier-ignore");
    let r = dprint_plugin_json::format_text(caminho, texto, &b.build()).map_err(|e| e.to_string())?;
    Ok(r.unwrap_or_else(|| texto.to_string()))
}

fn css(texto: &str, ext: &str, o: &Opcoes) -> Result<String, String> {
    let sintaxe = match ext {
        "scss" => malva::Syntax::Scss,
        "less" => malva::Syntax::Less,
        _ => malva::Syntax::Css,
    };
    let mut opcoes = malva::config::FormatOptions::default();
    opcoes.layout.print_width = o.largura as usize;
    opcoes.layout.indent_width = o.tab as usize;
    opcoes.layout.use_tabs = o.tabs;
    opcoes.language.quotes = if o.aspas_simples { malva::config::Quotes::AlwaysSingle } else { malva::config::Quotes::AlwaysDouble };
    opcoes.language.prefer_single_line = false;
    malva::format_text(texto, sintaxe, &opcoes).map_err(|e| format!("{e:?}"))
}

fn html(texto: &str, ext: &str, o: &Opcoes) -> Result<String, String> {
    let lingua = if ext == "vue" { markup_fmt::Language::Vue } else { markup_fmt::Language::Html };
    let mut opcoes = markup_fmt::config::FormatOptions::default();
    opcoes.layout.print_width = o.largura as usize;
    opcoes.layout.indent_width = o.tab as usize;
    opcoes.layout.use_tabs = o.tabs;
    let o2 = o.clone();
    markup_fmt::format_text(texto, lingua, &opcoes, move |codigo: &str, dicas: markup_fmt::Hints| -> Result<Cow<'_, str>, anyhow::Error> {
        let mut oi = o2.clone();
        oi.largura = dicas.print_width as u32;
        let falso = Path::new("embutido").with_extension(dicas.ext);
        let r = match dicas.ext {
            "css" | "scss" | "less" => css(codigo, dicas.ext, &oi),
            "json" | "jsonc" => json(&falso, codigo, &oi),
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts" => typescript(&falso, codigo, &oi),
            _ => Ok(codigo.to_string()),
        };
        r.map(Cow::Owned).map_err(anyhow::Error::msg)
    })
    .map_err(|e| format!("{e:?}"))
}

fn markdown(texto: &str, o: &Opcoes) -> Result<String, String> {
    use dprint_plugin_markdown::configuration::*;
    let mut b = ConfigurationBuilder::new();
    b.line_width(o.largura).text_wrap(match o.prosa.as_str() {
        "always" => TextWrap::Always,
        "never" => TextWrap::Never,
        _ => TextWrap::Maintain,
    });
    let config = b.build();
    let o2 = o.clone();
    let r = dprint_plugin_markdown::format_text(texto, &config, move |tag: &str, codigo: &str, largura: u32| {
        let mut oi = o2.clone();
        oi.largura = largura;
        let ext = match tag {
            "ts" | "typescript" => "ts",
            "tsx" => "tsx",
            "js" | "javascript" => "js",
            "jsx" => "jsx",
            "json" | "jsonc" => "json",
            "css" | "scss" | "less" => tag,
            "yaml" | "yml" => "yaml",
            _ => return Ok(None),
        };
        let falso = Path::new("embutido").with_extension(ext);
        Ok(prettier(&falso, codigo, &oi).ok())
    })
    .map_err(|e| e.to_string())?;
    Ok(r.unwrap_or_else(|| texto.to_string()))
}

fn yaml(texto: &str, o: &Opcoes) -> Result<String, String> {
    let mut opcoes = pretty_yaml::config::FormatOptions::default();
    opcoes.layout.print_width = o.largura as usize;
    opcoes.layout.indent_width = o.tab as usize;
    opcoes.language.quotes = if o.aspas_simples { pretty_yaml::config::Quotes::PreferSingle } else { pretty_yaml::config::Quotes::PreferDouble };
    opcoes.language.brace_spacing = o.espaco_chaves;
    pretty_yaml::format_text(texto, &opcoes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod testes {
    use super::*;

    fn padrao() -> Opcoes {
        Opcoes::de(&Map::new())
    }

    #[test]
    fn typescript_como_o_prettier() {
        let t = prettier(Path::new("a.ts"), "const  x={a:1,b:'c'}\nfunction f(a,b){return a+b}\n", &padrao()).unwrap();
        assert_eq!(t, "const x = { a: 1, b: \"c\" };\nfunction f(a, b) {\n  return a + b;\n}\n");
        let mut o = padrao();
        o.aspas_simples = true;
        o.ponto_e_virgula = false;
        o.espaco_chaves = false;
        let t = prettier(Path::new("a.ts"), "const x = {a: \"b\"};\n", &o).unwrap();
        assert_eq!(t, "const x = {a: 'b'}\n");
    }

    #[test]
    fn imports_nao_sao_reordenados() {
        let t = prettier(Path::new("a.ts"), "import { b, a } from \"z\";\nimport c from \"a\";\n", &padrao()).unwrap();
        assert_eq!(t, "import { b, a } from \"z\";\nimport c from \"a\";\n");
    }

    #[test]
    fn json_css_yaml_markdown() {
        assert_eq!(prettier(Path::new("a.json"), "{\"a\":[1,2]}", &padrao()).unwrap(), "{ \"a\": [1, 2] }\n");
        assert_eq!(prettier(Path::new("a.css"), "a{color:red}", &padrao()).unwrap(), "a {\n  color: red;\n}\n");
        assert_eq!(prettier(Path::new("a.yaml"), "a:   1\n", &padrao()).unwrap(), "a: 1\n");
        assert_eq!(prettier(Path::new("a.md"), "# Título\n\n\n\ntexto\n", &padrao()).unwrap(), "# Título\n\ntexto\n");
    }

    #[test]
    fn opcoes_da_config() {
        let m: Map<String, Value> = serde_json::from_str(r#"{"printWidth":100,"tabWidth":4,"semi":false,"endOfLine":"crlf"}"#).unwrap();
        let o = Opcoes::de(&m);
        assert_eq!((o.largura, o.tab, o.ponto_e_virgula, o.fim_de_linha.as_str()), (100, 4, false, "crlf"));
        assert!(matches!(ler_config("semi: false\n", "yaml"), Ok(m) if m["semi"] == Value::Bool(false)));
        assert!(matches!(ler_config("[1]", "json"), Err(Falha::Erro(_))));
    }
}
