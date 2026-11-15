//! Realce de sintaxe com `syntect` e o tema dark-plus do VS Code (as cores de `shiki-colors.ts`), mais as cores dos
//! pares de colchetes de `bracket-colors.ts` e o marcador de lista do Markdown. O tema vem de `assets/dark-plus.json`, o
//! mesmo do Shiki; as gramáticas são as do `two-face` (as do bat), escolhidas pela linguagem de [`crate::linguagem`].
use crate::linguagem;
use gpui::{FontStyle, FontWeight, HighlightStyle, Hsla, Rgba, SharedString, Task, UnderlineStyle};
use gpui_kit::component::input::{EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, InputHighlighterFactory, Rope};
use serde::Deserialize;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::LazyLock;
use std::time::Duration;
use syntect::highlighting::{
    Color, FontStyle as Estilo, HighlightIterator, HighlightState, Highlighter, ScopeSelectors, StyleModifier, Theme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

/// Acima disto (caracteres), o arquivo fica sem cor, como o `LIMIT` do Shiki e o `COLOR_LIMIT` do editor.
pub const LIMITE_COR: usize = 200_000;
/// Documentos até este tamanho são recoloridos na hora da tecla; maiores, em segundo plano.
const SINCRONO_ATE: usize = 8 * 1024;
const ESPERA: Duration = Duration::from_millis(30);

static SINTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

#[derive(Deserialize)]
struct TemaJson {
    #[serde(rename = "tokenColors")]
    token_colors: Vec<Regra>,
}

#[derive(Deserialize)]
struct Regra {
    scope: Option<Escopos>,
    settings: Ajustes,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Escopos {
    Um(String),
    Varios(Vec<String>),
}

#[derive(Deserialize)]
struct Ajustes {
    foreground: Option<String>,
    #[serde(rename = "fontStyle")]
    font_style: Option<String>,
}

fn cor_hex(h: &str) -> Option<Color> {
    let h = h.trim_start_matches('#');
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some(Color { r: v(0)?, g: v(2)?, b: v(4)?, a: 255 }),
        8 => Some(Color { r: v(0)?, g: v(2)?, b: v(4)?, a: v(6)? }),
        _ => None,
    }
}

/// O dark-plus do Shiki como tema do `syntect`.
static TEMA: LazyLock<Theme> = LazyLock::new(|| {
    let json: TemaJson = serde_json::from_str(include_str!("../assets/dark-plus.json")).expect("dark-plus.json válido");
    let mut tema = Theme { settings: ThemeSettings { foreground: cor_hex("#D4D4D4"), ..Default::default() }, ..Default::default() };
    for r in json.token_colors {
        let estilo = r.settings.font_style.as_deref().map(|f| {
            let mut e = Estilo::empty();
            for p in f.split_whitespace() {
                match p {
                    "italic" => e |= Estilo::ITALIC,
                    "bold" => e |= Estilo::BOLD,
                    "underline" => e |= Estilo::UNDERLINE,
                    _ => {}
                }
            }
            e
        });
        let modificador = StyleModifier { foreground: r.settings.foreground.as_deref().and_then(cor_hex), background: None, font_style: estilo };
        let escopos = match r.scope {
            None => {
                if let Some(c) = modificador.foreground {
                    tema.settings.foreground = Some(c);
                }
                continue;
            }
            Some(Escopos::Um(s)) => vec![s],
            Some(Escopos::Varios(v)) => v,
        };
        for e in escopos {
            if let Ok(scope) = ScopeSelectors::from_str(&e) {
                tema.scopes.push(ThemeItem { scope, style: modificador });
            }
        }
    }
    // No VS Code, entre regras de mesma especificidade vence a última; no `syntect`, a primeira.
    tema.scopes.reverse();
    tema
});

/// Cor padrão do texto colorido (o `editor.foreground` do dark-plus) e do texto sem linguagem (`#CCCCCC` do editor).
pub fn cor_texto() -> Hsla {
    gpui::rgb(0xd4d4d4).into()
}

const NIVEIS: [u32; 3] = [0xFFD700, 0xDA70D6, 0x179FFF];
fn cor_inesperado() -> Hsla {
    gpui::rgba(0xff1212cc).into()
}
fn cor_lista() -> Hsla {
    gpui::rgb(0x6796e6).into()
}

fn hsla(c: Color) -> Hsla {
    Rgba { r: c.r as f32 / 255., g: c.g as f32 / 255., b: c.b as f32 / 255., a: c.a as f32 / 255. }.into()
}

/// A gramática do `syntect` para a linguagem Shiki.
pub(crate) fn sintaxe(lang: &str, caminho: &str) -> Option<&'static SyntaxReference> {
    let s = &*SINTAXES;
    let especial = match lang {
        "tsx" => Some("TypeScriptReact"),
        "typescript" => Some("TypeScript"),
        "javascript" | "jsx" => Some("JavaScript (Babel)"),
        "docker" => Some("Dockerfile"),
        "bat" => Some("Batch File"),
        "dotenv" => Some("DotENV"),
        "shellscript" => Some("Bourne Again Shell (bash)"),
        "csharp" => Some("C#"),
        "cpp" => Some("C++"),
        _ => None,
    };
    let ext = caminho.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let info = linguagem::info_shiki(lang);
    especial
        .and_then(|n| s.find_syntax_by_name(n))
        .or_else(|| s.find_syntax_by_token(lang))
        .or_else(|| info.and_then(|i| s.find_syntax_by_name(&i.name)))
        .or_else(|| info.and_then(|i| i.aliases.iter().find_map(|a| s.find_syntax_by_token(a))))
        .or_else(|| (!ext.is_empty()).then(|| s.find_syntax_by_extension(ext)).flatten())
        .filter(|x| x.name != "Plain Text")
}

/// Trechos coloridos, em ordem e sem sobreposição.
pub type Trechos = Vec<(Range<usize>, HighlightStyle)>;

fn estilo(st: syntect::highlighting::Style) -> HighlightStyle {
    HighlightStyle {
        color: Some(hsla(st.foreground)),
        font_style: st.font_style.contains(Estilo::ITALIC).then_some(FontStyle::Italic),
        font_weight: st.font_style.contains(Estilo::BOLD).then_some(FontWeight::BOLD),
        underline: st.font_style.contains(Estilo::UNDERLINE).then(|| UnderlineStyle { thickness: gpui::px(1.), ..Default::default() }),
        ..Default::default()
    }
}

/// Colore o texto inteiro: sintaxe pelo tema e, por cima, os pares de colchetes fora de string e comentário.
pub fn colorir(sintaxe: Option<&SyntaxReference>, texto: &str) -> Trechos {
    if texto.len() > LIMITE_COR * 4 || crate::armazem::tamanho_js(texto) > LIMITE_COR {
        return Vec::new();
    }
    let hl = Highlighter::new(&TEMA);
    let (fora_str, fora_com, lista) = &*ESCOPOS;
    let mut trechos: Trechos = Vec::new();
    // Pilha de colchetes abertos: caractere e nível.
    let mut abertos: Vec<(u8, usize)> = Vec::new();
    let mut offset = 0;
    let mut parse = sintaxe.map(ParseState::new);
    let mut estado = HighlightState::new(&hl, ScopeStack::new());
    // Trechos da linha atual: as sobreposições dos colchetes partem só este vetor pequeno.
    let mut da_linha: Trechos = Vec::new();
    for linha in syntect::util::LinesWithEndings::from(texto) {
        da_linha.clear();
        let ops = match parse.as_mut().map(|p| p.parse_line(linha, &SINTAXES)) {
            Some(Ok(ops)) => Some(ops),
            Some(Err(_)) => {
                parse = None;
                None
            }
            None => None,
        };
        // Faixas de string/comentário da linha, onde os colchetes não contam.
        let mut fora: Vec<Range<usize>> = Vec::new();
        let mut listas: Vec<Range<usize>> = Vec::new();
        if let Some(ops) = &ops {
            let mut pilha = estado.path.clone();
            let mut cursor = 0;
            let mut marcar = |a: usize, b: usize, pilha: &ScopeStack| {
                if a >= b {
                    return;
                }
                let escopos = pilha.as_slice();
                if escopos.iter().any(|e| fora_str.is_prefix_of(*e) || fora_com.is_prefix_of(*e)) {
                    fora.push(a..b);
                }
                if escopos.iter().any(|e| lista.is_prefix_of(*e)) {
                    listas.push(a..b);
                }
            };
            for (i, op) in ops {
                marcar(cursor, *i, &pilha);
                let _ = pilha.apply(op);
                cursor = *i;
            }
            marcar(cursor, linha.len(), &pilha);
            let mut pos = 0;
            let util = linha.trim_end_matches(['\n', '\r']).len();
            for (st, pedaco) in HighlightIterator::new(&mut estado, ops, linha, &hl) {
                let fim = pos + pedaco.len();
                let alcance = pos..fim.min(util).max(pos);
                if !alcance.is_empty() {
                    let e = estilo(st);
                    match da_linha.last_mut() {
                        Some((r, s)) if r.end == alcance.start && *s == e => r.end = alcance.end,
                        _ => da_linha.push((alcance, e)),
                    }
                }
                pos = fim;
            }
        }
        // Colchetes, como o caminho "pares lidos do texto" de bracket-colors.ts.
        for (i, b) in linha.bytes().enumerate() {
            if !b"()[]{}".contains(&b) || fora.iter().any(|r| r.contains(&i)) {
                continue;
            }
            let cor = match b {
                b'(' | b'[' | b'{' => {
                    let n = abertos.len();
                    abertos.push((b, n));
                    gpui::rgb(NIVEIS[n % 3]).into()
                }
                _ => {
                    let abre = match b {
                        b')' => b'(',
                        b']' => b'[',
                        _ => b'{',
                    };
                    match abertos.iter().rposition(|(c, _)| *c == abre) {
                        Some(topo) => {
                            let n = abertos[topo].1;
                            abertos.truncate(topo);
                            gpui::rgb(NIVEIS[n % 3]).into()
                        }
                        None => cor_inesperado(),
                    }
                }
            };
            sobrepor(&mut da_linha, i..i + 1, cor);
        }
        for r in listas {
            sobrepor(&mut da_linha, r, cor_lista());
        }
        trechos.extend(da_linha.drain(..).map(|(r, s)| (offset + r.start..offset + r.end, s)));
        offset += linha.len();
    }
    trechos
}

/// Escopos que contam nos colchetes e no marcador de lista.
static ESCOPOS: LazyLock<(Scope, Scope, Scope)> = LazyLock::new(|| {
    (Scope::new("string").unwrap(), Scope::new("comment").unwrap(), Scope::new("punctuation.definition.list").unwrap())
});

/// Troca a cor de `alcance` mantendo o resto do estilo, partindo o trecho que o contém.
fn sobrepor(trechos: &mut Trechos, alcance: Range<usize>, cor: Hsla) {
    let i = trechos.partition_point(|(r, _)| r.end <= alcance.start);
    let novo = |base: Option<&HighlightStyle>| HighlightStyle { color: Some(cor), ..base.cloned().unwrap_or_default() };
    match trechos.get(i).cloned() {
        Some((r, s)) if r.start <= alcance.start && r.end >= alcance.end => {
            let mut partes = Vec::with_capacity(3);
            if r.start < alcance.start {
                partes.push((r.start..alcance.start, s));
            }
            partes.push((alcance.clone(), novo(Some(&s))));
            if alcance.end < r.end {
                partes.push((alcance.end..r.end, s));
            }
            trechos.splice(i..=i, partes);
        }
        _ => trechos.insert(i, (alcance, novo(None))),
    }
}

/// Estilos de `alcance`, sem buracos.
fn estilos_em(trechos: &[(Range<usize>, HighlightStyle)], alcance: &Range<usize>) -> Trechos {
    let primeiro = trechos.partition_point(|(r, _)| r.end <= alcance.start);
    let mut out = Vec::new();
    let mut cursor = alcance.start;
    for (r, s) in &trechos[primeiro..] {
        if r.start >= alcance.end {
            break;
        }
        let (a, b) = (r.start.max(alcance.start), r.end.min(alcance.end));
        if a >= b || b <= cursor {
            continue;
        }
        if cursor < a {
            out.push((cursor..a, HighlightStyle::default()));
        }
        out.push((a.max(cursor)..b, *s));
        cursor = b;
    }
    if cursor < alcance.end {
        out.push((cursor..alcance.end, HighlightStyle::default()));
    }
    out
}

fn deslocar(trechos: &mut Trechos, e: &InputEdit) {
    let delta = e.new_end_byte as isize - e.old_end_byte as isize;
    trechos.retain_mut(|(r, _)| {
        if r.end <= e.start_byte {
            true
        } else if r.start >= e.old_end_byte {
            r.start = r.start.saturating_add_signed(delta);
            r.end = r.end.saturating_add_signed(delta);
            true
        } else {
            false
        }
    });
}

/// Dobras por chaves fora de strings e comentários de linha.
fn dobras(texto: &str) -> Vec<FoldRange> {
    let mut inicios = Vec::new();
    let mut out = Vec::new();
    for (n, linha) in texto.lines().enumerate() {
        let mut cs = linha.chars().peekable();
        let mut aspas = None;
        let mut escape = false;
        while let Some(c) = cs.next() {
            match aspas {
                Some(a) => {
                    if escape {
                        escape = false;
                    } else if c == '\\' {
                        escape = true;
                    } else if c == a {
                        aspas = None;
                    }
                }
                None => match c {
                    '/' if cs.peek() == Some(&'/') => break,
                    '"' | '\'' | '`' => aspas = Some(c),
                    '{' | '[' | '(' => inicios.push(n),
                    '}' | ']' | ')' => {
                        if let Some(i) = inicios.pop() {
                            if n > i {
                                out.push(FoldRange::new(i, n));
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
    }
    out.sort_by_key(|r| r.start_line);
    out.dedup_by_key(|r| r.start_line);
    out
}

struct Realce {
    lang: SharedString,
    sintaxe: Option<&'static SyntaxReference>,
    trechos: Rc<RefCell<Trechos>>,
    dobras: Rc<RefCell<Vec<FoldRange>>>,
    tarefa: Option<Task<()>>,
}

impl InputHighlighter for Realce {
    fn language(&self) -> SharedString {
        self.lang.clone()
    }

    fn update(&mut self, edit: Option<InputEdit>, texto: &Rope, dobrar: bool, window: &mut gpui::Window, cx: &mut gpui::Context<EditorState>) {
        if let Some(e) = edit {
            deslocar(&mut self.trechos.borrow_mut(), &e);
        }
        if texto.len() <= SINCRONO_ATE {
            self.tarefa = None;
            let t = texto.to_string();
            *self.trechos.borrow_mut() = colorir(self.sintaxe, &t);
            *self.dobras.borrow_mut() = if dobrar { dobras(&t) } else { Vec::new() };
            return;
        }
        let (sintaxe, trechos, dobras_rc, texto) = (self.sintaxe, self.trechos.clone(), self.dobras.clone(), texto.clone());
        let primeira = edit.is_none();
        self.tarefa = Some(cx.spawn_in(window, async move |ent, cx| {
            if !primeira {
                cx.background_executor().timer(ESPERA).await;
            }
            let (t, d) = cx
                .background_executor()
                .spawn(async move {
                    let t = texto.to_string();
                    (colorir(sintaxe, &t), if dobrar { dobras(&t) } else { Vec::new() })
                })
                .await;
            *trechos.borrow_mut() = t;
            *dobras_rc.borrow_mut() = d.clone();
            let _ = ent.update(cx, |s, cx| s.apply_highlighter_fold_candidates(d, cx));
        }));
    }

    fn styles(&self, alcance: &Range<usize>, _: &dyn HighlightStyleResolver) -> Vec<(Range<usize>, HighlightStyle)> {
        estilos_em(&self.trechos.borrow(), alcance)
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.dobras.borrow().clone()
    }
}

thread_local! {
    /// Cores atuais por nome de linguagem do editor (o caminho), para o minimapa desenhar com as mesmas cores.
    static REGISTRO: RefCell<std::collections::HashMap<String, std::rc::Weak<RefCell<Trechos>>>> = RefCell::default();
}

/// Trechos coloridos do editor cujo nome de linguagem é `caminho`.
pub fn trechos_de(caminho: &str) -> Option<Rc<RefCell<Trechos>>> {
    REGISTRO.with(|r| r.borrow().get(caminho).and_then(std::rc::Weak::upgrade))
}

/// Fábrica para o `EditorState`: o "nome de linguagem" que o editor recebe é o caminho do arquivo, e daí sai a
/// linguagem Shiki e a gramática. Sem gramática, ainda colore os colchetes, como o editor do React.
pub fn fabrica() -> InputHighlighterFactory {
    Rc::new(|caminho: &str| {
        let lang = linguagem::linguagem(caminho);
        let sintaxe = lang.as_deref().and_then(|l| sintaxe(l, caminho));
        let trechos: Rc<RefCell<Trechos>> = Rc::default();
        REGISTRO.with(|r| {
            let mut r = r.borrow_mut();
            r.retain(|_, w| w.strong_count() > 0);
            r.insert(caminho.to_string(), Rc::downgrade(&trechos));
        });
        Some(Box::new(Realce {
            lang: caminho.to_string().into(),
            sintaxe,
            trechos,
            dobras: Rc::default(),
            tarefa: None,
        }) as Box<dyn InputHighlighter>)
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    fn cor_de(trechos: &Trechos, t: &str, trecho: &str) -> Option<Hsla> {
        let i = t.find(trecho).unwrap();
        trechos.iter().find(|(r, _)| r.contains(&i)).and_then(|(_, s)| s.color)
    }

    #[test]
    fn tema_dark_plus_e_colchetes() {
        let t = "const x: number = f(a[1]);\n// (comentário)\n";
        let s = sintaxe("typescript", "a.ts");
        assert!(s.is_some(), "gramática TypeScript");
        let tr = colorir(s, t);
        // `const` é storage.type: #569CD6 no dark-plus.
        assert_eq!(cor_de(&tr, t, "const"), Some(gpui::rgb(0x569cd6).into()));
        assert_eq!(cor_de(&tr, t, "("), Some(gpui::rgb(0xFFD700).into()), "primeiro nível dourado");
        assert_eq!(cor_de(&tr, t, "["), Some(gpui::rgb(0xDA70D6).into()), "segundo nível orquídea");
        // Dentro do comentário o parêntese fica com a cor do comentário (#6A9955).
        let i = t.find("(com").unwrap();
        let c = tr.iter().find(|(r, _)| r.contains(&i)).and_then(|(_, s)| s.color);
        assert_eq!(c, Some(gpui::rgb(0x6a9955).into()));
    }

    #[test]
    fn fechamento_sem_abertura_vermelho() {
        let t = "a)";
        let tr = colorir(None, t);
        assert_eq!(cor_de(&tr, t, ")"), Some(cor_inesperado()));
    }

    #[test]
    fn sintaxes_das_linguagens_comuns() {
        for (l, c) in [
            ("typescript", "a.ts"),
            ("tsx", "a.tsx"),
            ("javascript", "a.js"),
            ("json", "a.json"),
            ("rust", "a.rs"),
            ("python", "a.py"),
            ("css", "a.css"),
            ("scss", "a.scss"),
            ("html", "a.html"),
            ("markdown", "a.md"),
            ("yaml", "a.yaml"),
            ("toml", "Cargo.toml"),
            ("bat", "a.cmd"),
            ("dotenv", ".env"),
            ("docker", "Dockerfile"),
            ("csharp", "a.cs"),
            ("cpp", "a.cpp"),
            ("c", "a.c"),
            ("go", "a.go"),
        ] {
            assert!(sintaxe(l, c).is_some(), "{l}");
        }
    }

    #[test]
    fn colore_3000_linhas_em_tempo_util() {
        // 3000 linhas abaixo do limite de cor (200 mil caracteres), como um arquivo grande comum.
        let linha = "export const x = f(a, 'b', [1, 2]); // comentário\n";
        let t = linha.repeat(3000);
        let ini = std::time::Instant::now();
        let tr = colorir(sintaxe("typescript", "a.ts"), &t);
        let gasto = ini.elapsed();
        eprintln!("3000 linhas coloridas em {gasto:?}");
        assert!(!tr.is_empty());
        // Em segundo plano; mesmo sem otimização tem de caber em poucos segundos.
        assert!(gasto < std::time::Duration::from_secs(if cfg!(debug_assertions) { 30 } else { 3 }));
    }

    #[test]
    fn estilos_cobrem_o_alcance() {
        let tr: Trechos = vec![(2..4, HighlightStyle::default())];
        let e = estilos_em(&tr, &(0..6));
        assert_eq!(e.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>(), [0..2, 2..4, 4..6]);
    }
}

