//! Realce de sintaxe do comparador: `syntect` com as gramáticas do `two-face` e o tema Dark+ do VS Code.
//! Roda no executor de fundo do `DiffState`, uma vez por lado de cada bloco.
use gpui_kit::{FontStyle, FontWeight, HighlightStyle, Hsla, Rgba};
use std::ops::Range;
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle as Estilo, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};

static SINTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
#[allow(deprecated)]
static TEMA: LazyLock<Theme> =
    LazyLock::new(|| two_face::theme::extra().get(two_face::theme::EmbeddedThemeName::VisualStudioDarkPlus).clone());

/// Carrega as gramáticas e o tema num thread à parte, para o primeiro diff não esperar por eles.
pub fn aquecer() {
    std::thread::spawn(|| {
        LazyLock::force(&SINTAXES);
        LazyLock::force(&TEMA);
    });
}

/// Acima disto o texto não é realçado.
const LIMITE: usize = 2_000_000;

fn sintaxe(caminho: &str) -> Option<&'static SyntaxReference> {
    let nome = caminho.rsplit(['/', '\\']).next().unwrap_or(caminho);
    let ext = nome.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let s = &*SINTAXES;
    let por_ext = |e: &str| s.find_syntax_by_extension(e);
    match ext.as_deref() {
        // `two-face` traz TypeScript e TSX; `.mts`/`.cts` usam o TypeScript, `.jsx` o JavaScript (Babel).
        Some("mts" | "cts") => por_ext("ts"),
        Some(e) => por_ext(e).or_else(|| s.find_syntax_by_token(e)),
        None => s.find_syntax_by_extension(nome),
    }
}

fn cor(c: syntect::highlighting::Color) -> Hsla {
    Rgba { r: c.r as f32 / 255., g: c.g as f32 / 255., b: c.b as f32 / 255., a: c.a as f32 / 255. }.into()
}

/// Cor padrão do texto no Dark+ (`editor.foreground`).
pub fn cor_texto() -> Hsla {
    TEMA.settings.foreground.map(cor).unwrap_or_else(|| gpui_kit::rgb(0xd4d4d4).into())
}

/// Faixas de bytes de `texto` com cor, itálico e negrito. `caminho` escolhe a gramática pela extensão.
pub fn realcar(caminho: &str, texto: &str) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(sintaxe) = sintaxe(caminho) else { return Vec::new() };
    if texto.len() > LIMITE {
        return Vec::new();
    }
    let mut h = HighlightLines::new(sintaxe, &TEMA);
    let mut saida = Vec::new();
    let mut base = 0;
    for linha in syntect::util::LinesWithEndings::from(texto) {
        let Ok(partes) = h.highlight_line(linha, &SINTAXES) else { break };
        let mut pos = base;
        for (estilo, pedaco) in partes {
            let fim = pos + pedaco.len();
            let conteudo = pedaco.trim_end_matches(['\n', '\r']);
            if !conteudo.is_empty() {
                saida.push((
                    pos..pos + conteudo.len(),
                    HighlightStyle {
                        color: Some(cor(estilo.foreground)),
                        font_style: estilo.font_style.contains(Estilo::ITALIC).then_some(FontStyle::Italic),
                        font_weight: estilo.font_style.contains(Estilo::BOLD).then_some(FontWeight::BOLD),
                        ..Default::default()
                    },
                ));
            }
            pos = fim;
        }
        base += linha.len();
    }
    modificador_type(texto, &mut saida);
    saida
}

/// A gramática TypeScript do `two-face` não conhece o modificador `type` de `import { type X }`/`export { type X }` e
/// o pinta como identificador; o Shiki o marca `keyword.control.type`, #C586C0 no Dark+.
fn modificador_type(texto: &str, saida: &mut [(Range<usize>, HighlightStyle)]) {
    let cor_chave: Hsla = gpui_kit::rgb(0xc586c0).into();
    for (r, estilo) in saida.iter_mut() {
        if &texto[r.clone()] != "type" {
            continue;
        }
        let antes = texto[..r.start].trim_end().chars().last();
        let depois = &texto[r.end..];
        let segue_nome = depois.starts_with([' ', '\t']) && depois.trim_start().starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$');
        if matches!(antes, Some('{' | ',')) && segue_nome {
            estilo.color = Some(cor_chave);
        }
    }
}



#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn typescript_com_cores_do_dark_plus() {
        let r = realcar("src/a.ts", "import { x } from 'y';\nconst a = 1;\n");
        assert!(!r.is_empty());
        // `import` é palavra-chave de controle no Dark+: #C586C0.
        let import = r.iter().find(|(f, _)| f.start == 0).expect("realce do import");
        let c: Rgba = import.1.color.unwrap().into();
        assert_eq!(((c.r * 255.).round() as u8, (c.g * 255.).round() as u8, (c.b * 255.).round() as u8), (0xc5, 0x86, 0xc0));
        assert!(realcar("sem-extensao-conhecida.xyzq", "a").is_empty());
    }

    #[test]
    fn modificador_type_como_no_shiki() {
        let t = "import {\n  a, type B, type C as D,\n} from 'x';\nimport { type E } from 'y';\nconst type = 1;\n";
        let cor = |k: usize| -> (u8, u8, u8) {
            let (_, s) = realcar("a.tsx", t).into_iter().find(|(r, _)| r.start == k).unwrap();
            let c: Rgba = s.color.unwrap().into();
            ((c.r * 255.).round() as u8, (c.g * 255.).round() as u8, (c.b * 255.).round() as u8)
        };
        let todos: Vec<usize> = t.match_indices("type").map(|(i, _)| i).collect();
        for &k in &todos[..3] {
            assert_eq!(cor(k), (0xc5, 0x86, 0xc0), "type em {k}");
        }
        assert_ne!(cor(todos[3]), (0xc5, 0x86, 0xc0));
    }
}
