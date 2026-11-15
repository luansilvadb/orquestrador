//! Ícones de arquivo e pasta do Material Icon Theme 5.40.0 (MIT, `assets/file-icons/LICENSE`), com a resolução de
//! `src/shared/file-icons.ts`: nome do arquivo, extensão (da mais longa para a mais curta, "d.ts" antes de "ts"),
//! linguagem pelo nome, linguagem pela extensão e o padrão; pasta pelo nome (aberta ou fechada) ou a padrão. Sem
//! diferença de caixa. `assets/file-icons/theme.json` é o mapa gerado por `scripts/sync-file-icons.mjs`.
//!
//! Os SVGs são coloridos: desenhe-os com `img()`, porque `svg()` pinta uma cor só.
use gpui_kit::SharedString;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

#[path = "icones_arquivo_svgs.rs"]
mod svgs;

/// Prefixo dos caminhos de asset dos ícones.
pub const PREFIXO: &str = "file-icons/";

type Mapa = HashMap<String, String>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tema {
    file: String,
    folder: String,
    folder_expanded: String,
    icons: Mapa,
    file_names: Mapa,
    file_extensions: Mapa,
    lang_names: Mapa,
    lang_extensions: Mapa,
    folder_names: Mapa,
    folder_names_expanded: Mapa,
}

fn tema() -> &'static Tema {
    static TEMA: OnceLock<Tema> = OnceLock::new();
    TEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/file-icons/theme.json")).expect("theme.json dos ícones é válido")
    })
}

/// "a.b.c" -> ["b.c", "c"]. Arquivo que começa com ponto (".gitignore") não tem o nome como extensão.
fn extensoes(nome: &str) -> Vec<String> {
    let partes: Vec<&str> = nome.split('.').collect();
    let inicio = if nome.starts_with('.') { 2 } else { 1 };
    (inicio..partes.len()).map(|i| partes[i..].join(".")).collect()
}

/// A chave do ícone (ex.: "typescript", "folder-src") do último segmento do caminho.
pub fn chave_icone(caminho: &str, pasta: bool, aberta: bool) -> &'static str {
    let t = tema();
    let nome = caminho.rsplit(['/', '\\']).next().unwrap_or(caminho).to_lowercase();
    if pasta {
        let (mapa, padrao) = if aberta { (&t.folder_names_expanded, &t.folder_expanded) } else { (&t.folder_names, &t.folder) };
        return mapa.get(&nome).unwrap_or(padrao);
    }
    let exts = extensoes(&nome);
    t.file_names
        .get(&nome)
        .or_else(|| exts.iter().find_map(|e| t.file_extensions.get(e)))
        .or_else(|| t.lang_names.get(&nome))
        .or_else(|| exts.iter().find_map(|e| t.lang_extensions.get(e)))
        .unwrap_or(&t.file)
}

/// Nome do SVG (em `assets/file-icons/icons/`) da chave.
pub fn arquivo_da_chave(chave: &str) -> Option<&'static str> {
    tema().icons.get(chave).map(String::as_str)
}

/// Caminho do asset do ícone, para `img()`.
pub fn icone_arquivo(caminho: &str, pasta: bool, aberta: bool) -> SharedString {
    let chave = chave_icone(caminho, pasta, aberta);
    let arquivo = arquivo_da_chave(chave).unwrap_or("file.svg");
    format!("{PREFIXO}{arquivo}").into()
}

/// Bytes do SVG de um caminho `file-icons/<nome>.svg`. O SVG sem `width` ganha 32 px: o GPUI rasteriza no tamanho
/// intrínseco, e um `viewBox` de 4 unidades sairia borrado; o de 3000, pesado.
pub(crate) fn carregar(caminho: &str) -> Option<Cow<'static, [u8]>> {
    let nome = caminho.strip_prefix(PREFIXO)?;
    let i = svgs::SVGS.binary_search_by(|(n, _)| (*n).cmp(nome)).ok()?;
    let bytes = svgs::SVGS[i].1;
    Some(com_tamanho(bytes))
}

fn com_tamanho(bytes: &'static [u8]) -> Cow<'static, [u8]> {
    let Ok(texto) = std::str::from_utf8(bytes) else { return Cow::Borrowed(bytes) };
    let Some(inicio) = texto.find("<svg") else { return Cow::Borrowed(bytes) };
    let Some(fim) = texto[inicio..].find('>').map(|f| inicio + f) else { return Cow::Borrowed(bytes) };
    let tag = &texto[inicio..fim];
    if tag.contains(" width=") {
        return Cow::Borrowed(bytes);
    }
    let mut novo = String::with_capacity(texto.len() + 32);
    novo.push_str(&texto[..inicio + 4]);
    novo.push_str(r#" width="32" height="32""#);
    novo.push_str(&texto[inicio + 4..]);
    Cow::Owned(novo.into_bytes())
}

/// Todos os caminhos de asset dos ícones.
pub(crate) fn listar(prefixo: &str) -> Vec<SharedString> {
    svgs::SVGS
        .iter()
        .map(|(n, _)| format!("{PREFIXO}{n}"))
        .filter(|c| c.starts_with(prefixo))
        .map(SharedString::from)
        .collect()
}

#[cfg(test)]
mod testes {
    //! Porte de `tests/check-explorer.mjs` (parte dos ícones) e `tests/check-file-icons-ui.mjs` (resolução e SVG).
    use super::*;

    fn mapa(m: &Mapa, k: &str) -> String {
        m.get(k).cloned().unwrap_or_else(|| panic!("{k} sem mapa"))
    }

    #[test]
    fn pastas_fechadas_e_abertas() {
        let t = tema();
        assert_eq!(chave_icone("a/b/SRC", true, false), mapa(&t.folder_names, "src"));
        assert_eq!(chave_icone("C:\\a\\SRC", true, true), mapa(&t.folder_names_expanded, "src"));
        assert_eq!(chave_icone("desconhecida", true, false), t.folder);
        assert_eq!(chave_icone("desconhecida", true, true), t.folder_expanded);
    }

    #[test]
    fn nomes_extensoes_caminhos_e_desconhecidos() {
        let t = tema();
        for nome in ["package.json", "vite.config.ts", ".gitignore", "Dockerfile"] {
            let caminho = format!("C:\\repo\\{}", nome.to_uppercase());
            assert_eq!(chave_icone(&caminho, false, false), mapa(&t.file_names, &nome.to_lowercase()), "{nome}");
        }
        for (nome, ext) in [
            ("app.tsx", "tsx"),
            ("main.ts", "ts"),
            ("index.js", "js"),
            ("data.json", "json"),
            ("main.rs", "rs"),
            ("Program.cs", "cs"),
            ("script.gd", "gd"),
            ("scene.tscn", "tscn"),
            ("resource.tres", "tres"),
            ("project.godot", "godot"),
        ] {
            assert_eq!(chave_icone(&format!("a/b/{nome}"), false, false), mapa(&t.file_extensions, ext), "{nome}");
        }
        assert_eq!(chave_icone(".env", false, false), t.file_names.get(".env").unwrap_or(&t.file));
        assert_eq!(chave_icone("sem-extensao", false, false), t.file);
        assert_eq!(chave_icone(".inventado", false, false), t.file);
    }

    #[test]
    fn ordem_de_resolucao() {
        let t = tema();
        assert_eq!(chave_icone("src/types.d.ts", false, false), mapa(&t.file_extensions, "d.ts"));
        assert_eq!(chave_icone("a/b/VITE.CONFIG.TS", false, false), mapa(&t.file_names, "vite.config.ts"));
    }

    #[test]
    fn todo_icone_mapeado_tem_svg() {
        let t = tema();
        let mapas = [&t.file_names, &t.file_extensions, &t.lang_names, &t.lang_extensions, &t.folder_names, &t.folder_names_expanded];
        let mut chaves: Vec<&String> = vec![&t.file, &t.folder, &t.folder_expanded];
        chaves.extend(mapas.iter().flat_map(|m| m.values()));
        let quebradas: Vec<&String> = chaves
            .into_iter()
            .filter(|k| arquivo_da_chave(k).and_then(|a| carregar(&format!("{PREFIXO}{a}"))).is_none())
            .collect();
        assert!(quebradas.is_empty(), "{quebradas:?}");
    }

    /// `check-file-icons-ui.mjs`: árvore e abas usam o mesmo SVG, válido, para os nomes do Godot e o desconhecido.
    #[test]
    fn svg_dos_nomes_da_vitrine() {
        let t = tema();
        for nome in ["script.gd", "scene.tscn", "resource.tres", "project.godot", "arquivo.inventado"] {
            let ext = nome.rsplit('.').next().unwrap();
            let chave = t.file_names.get(nome).or_else(|| t.file_extensions.get(ext)).unwrap_or(&t.file);
            assert_eq!(chave_icone(nome, false, false), chave, "{nome}");
            let caminho = icone_arquivo(nome, false, false);
            let bytes = carregar(&caminho).expect("SVG embutido");
            let texto = String::from_utf8(bytes.into_owned()).unwrap();
            assert!(texto.contains("<svg") && texto.contains("</svg>"), "{nome}");
            assert!(texto.contains(r#"width="#), "{nome}: com tamanho");
        }
        assert_eq!(icone_arquivo("src", true, false), icone_arquivo("SRC", true, false));
        assert_ne!(icone_arquivo("src", true, false), icone_arquivo("src", true, true));
    }

    #[test]
    fn extensoes_compostas() {
        assert_eq!(extensoes("a.b.c"), ["b.c", "c"]);
        assert!(extensoes(".gitignore").is_empty());
        assert_eq!(extensoes(".eslintrc.json"), ["json"]);
    }
}
