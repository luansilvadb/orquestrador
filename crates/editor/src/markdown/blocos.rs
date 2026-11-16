//! Blocos de código e tabelas do Preview desenhados como no `Markdown` do Lobe UI 5.58: o `Snippet` (código de uma
//! linha até 32 caracteres) e o `Highlighter` com o botão copiar e o `lobe-theme` do realce; a tabela com a largura do
//! conteúdo (`width: max-content`), células de no mínimo 120 px e cabeçalho em `colorFillQuaternary`.
use super::texto::{Marca, TextoRico};
use gpui::{div, prelude::*, px, AnyElement, App, ClipboardItem, FontWeight, Hsla, SharedString, Window};
use gpui_kit::component::text::{markdown_ast as md, MarkdownNode};
use orquestrador_ui::tema;
use std::ops::Range;
use std::str::FromStr;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use syntect::highlighting::{Color, FontStyle as Estilo, Highlighter, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

pub const NOME_CODIGO: &str = "orq-md-codigo";
pub const NOME_TABELA: &str = "orq-md-tabela";
pub const NOME_IMAGENS: &str = "orq-md-imagens";

/// Parágrafo só de imagens (`> p > img` do Lobe): url e texto alternativo.
#[derive(Debug, Clone, PartialEq)]
pub struct Imagens(pub Vec<(String, String)>);

pub const NOME_TAREFAS: &str = "orq-md-tarefas";

/// Lista de tarefas simples (cada item um parágrafo): marcado e texto.
#[derive(Debug, Clone, PartialEq)]
pub struct Tarefas(pub Vec<(bool, Celula)>);

/// Tokens do tema escuro do Lobe UI (`colorText...`, `colorFill...`, `colorBorderSecondary`).
pub mod lobe {
    use gpui::{rgb, rgba, Hsla};
    pub fn texto() -> Hsla {
        rgb(0xffffff).into()
    }
    pub fn texto_secundario() -> Hsla {
        rgb(0xaaaaaa).into()
    }
    pub fn texto_terciario() -> Hsla {
        rgb(0x6f6f6f).into()
    }
    pub fn info() -> Hsla {
        rgb(0x60b1ff).into()
    }
    pub fn fill_quaternario() -> Hsla {
        rgba(0xffffff05).into()
    }
    pub fn fill_terciario() -> Hsla {
        rgba(0xffffff0f).into()
    }
    pub fn fill_secundario() -> Hsla {
        rgba(0xffffff1a).into()
    }
    pub fn borda() -> Hsla {
        rgb(0x202020).into()
    }
    pub fn borda_secundaria() -> Hsla {
        rgb(0x1a1a1a).into()
    }
}

/// Fonte de 14 px do Preview (`fontSize={14}`) e o `--lobe-markdown-margin-multiple` (2) × 0,5 em.
pub const FONTE: f32 = 14.;
pub const MARGEM: f32 = FONTE;

#[derive(Debug, Clone, PartialEq)]
pub struct Codigo {
    pub lang: String,
    pub codigo: String,
}

impl Codigo {
    /// O `CodeBlock` do Lobe: uma linha (o `\n` final do hast conta uma) e até 32 unidades UTF-16 vira `Snippet`.
    pub fn linha_unica(&self) -> bool {
        !self.codigo.contains('\n') && self.codigo.encode_utf16().count() < 32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alinhamento {
    Inicio,
    Centro,
    Fim,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Celula {
    pub texto: String,
    pub marcas: Vec<(Range<usize>, Marca)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tabela {
    pub alinhamentos: Vec<Alinhamento>,
    /// A primeira é o cabeçalho.
    pub linhas: Vec<Vec<Celula>>,
}

/// Converte os nós de bloco que o Preview desenha por conta própria.
pub fn analisar(no: &md::Node) -> Option<MarkdownNode> {
    match no {
        md::Node::Code(c) => {
            let d = Codigo { lang: c.lang.clone().unwrap_or_default(), codigo: c.value.clone() };
            let fonte = format!("```{}\n{}\n```", d.lang, d.codigo);
            Some(MarkdownNode::new(NOME_CODIGO, d.clone()).text(d.codigo).markdown(fonte))
        }
        md::Node::List(l)
            if !l.children.is_empty()
                && l.children.iter().all(|i| match i {
                    md::Node::ListItem(i) => i.checked.is_some() && i.children.len() <= 1 && i.children.iter().all(|c| matches!(c, md::Node::Paragraph(_))),
                    _ => false,
                }) =>
        {
            let itens: Vec<(bool, Celula)> = l
                .children
                .iter()
                .filter_map(|i| match i {
                    md::Node::ListItem(i) => Some((i.checked == Some(true), i.children.first().and_then(|p| p.children()).map(|c| celula(c)).unwrap_or_default())),
                    _ => None,
                })
                .collect();
            let texto = itens.iter().map(|(m, c)| format!("- [{}] {}", if *m { 'x' } else { ' ' }, c.texto)).collect::<Vec<_>>().join("
");
            Some(MarkdownNode::new(NOME_TAREFAS, Tarefas(itens)).text(texto.clone()).markdown(texto))
        }
        md::Node::Paragraph(p)
            if p.children.iter().any(|n| matches!(n, md::Node::Image(_)))
                && p.children.iter().all(|n| match n {
                    md::Node::Image(_) => true,
                    md::Node::Text(t) => t.value.trim().is_empty(),
                    _ => false,
                }) =>
        {
            let itens: Vec<(String, String)> =
                p.children.iter().filter_map(|n| if let md::Node::Image(i) = n { Some((i.url.clone(), i.alt.clone())) } else { None }).collect();
            let texto = itens.iter().map(|(_, a)| a.as_str()).collect::<Vec<_>>().join(" ");
            Some(MarkdownNode::new(NOME_IMAGENS, Imagens(itens)).text(texto))
        }
        md::Node::Table(t) => {
            let alinhamentos = t
                .align
                .iter()
                .map(|a| match a {
                    md::AlignKind::Center => Alinhamento::Centro,
                    md::AlignKind::Right => Alinhamento::Fim,
                    _ => Alinhamento::Inicio,
                })
                .collect();
            let linhas: Vec<Vec<Celula>> = t
                .children
                .iter()
                .filter_map(|l| match l {
                    md::Node::TableRow(r) => Some(r.children.iter().map(|c| celula(c.children().map(|v| v.as_slice()).unwrap_or(&[]))).collect()),
                    _ => None,
                })
                .collect();
            let texto = linhas.iter().map(|l: &Vec<Celula>| l.iter().map(|c| c.texto.as_str()).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n");
            Some(MarkdownNode::new(NOME_TABELA, Tabela { alinhamentos, linhas }).text(texto))
        }
        _ => None,
    }
}

/// Texto e estilos de uma célula a partir dos nós em linha.
pub fn celula(nos: &[md::Node]) -> Celula {
    let mut c = Celula::default();
    inline(nos, Marca::default(), &mut c);
    c
}

fn inline(nos: &[md::Node], m: Marca, c: &mut Celula) {
    let push = |c: &mut Celula, s: &str, m: Marca| {
        let a = c.texto.len();
        c.texto.push_str(s);
        if m != Marca::default() && !s.is_empty() {
            c.marcas.push((a..c.texto.len(), m));
        }
    };
    for n in nos {
        match n {
            md::Node::Text(t) => push(c, &t.value, m),
            md::Node::InlineCode(t) => push(c, &t.value, Marca { mono: true, fundo: Some(lobe::fill_secundario()), ..m }),
            md::Node::Html(t) => push(c, &t.value, m),
            md::Node::Break(_) => push(c, " ", m),
            md::Node::Image(i) => push(c, &i.alt, m),
            md::Node::Strong(s) => inline(&s.children, Marca { negrito: true, ..m }, c),
            md::Node::Emphasis(s) => inline(&s.children, Marca { italico: true, ..m }, c),
            md::Node::Delete(s) => inline(&s.children, Marca { tachado: true, cor: Some(lobe::texto_terciario()), ..m }, c),
            md::Node::Link(l) => inline(&l.children, Marca { cor: Some(lobe::info()), ..m }, c),
            outro => {
                if let Some(filhos) = outro.children() {
                    inline(filhos, m, c);
                }
            }
        }
    }
}

// ---- realce com o lobe-theme ----

/// As mesmas gramáticas do editor (`two-face` e, por cima, as do Shiki convertidas), que são as do realce do Lobe.
static SINTAXES: LazyLock<SyntaxSet> = LazyLock::new(|| {
    let mut b = two_face::syntax::extra_newlines().into_builder();
    let lista: &[(&str, &str)] = include!("../../assets/gramaticas/lista.rs");
    for (_, texto) in lista {
        if let Ok(d) = syntect::parsing::SyntaxDefinition::load_from_str(texto, true, None) {
            b.add(d);
        }
    }
    b.build()
});

#[derive(serde::Deserialize)]
struct TemaJson {
    #[serde(rename = "tokenColors")]
    token_colors: Vec<Regra>,
}

#[derive(serde::Deserialize)]
struct Regra {
    scope: Option<Escopos>,
    settings: Ajustes,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Escopos {
    Um(String),
    Varios(Vec<String>),
}

#[derive(serde::Deserialize)]
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

/// O `lobe-theme` do `Highlighter` do Lobe UI com os tokens escuros resolvidos (`assets/lobe-theme.json`).
static TEMA: LazyLock<Theme> = LazyLock::new(|| {
    let json: TemaJson = serde_json::from_str(include_str!("../../assets/lobe-theme.json")).expect("lobe-theme.json válido");
    let mut tema = Theme { settings: ThemeSettings { foreground: cor_hex("#ffffff"), ..Default::default() }, ..Default::default() };
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
    // No VS Code/Shiki vence a última regra de mesma especificidade; no `syntect`, a primeira.
    tema.scopes.reverse();
    tema
});

fn hsla(c: Color) -> Hsla {
    gpui::Rgba { r: c.r as f32 / 255., g: c.g as f32 / 255., b: c.b as f32 / 255., a: c.a as f32 / 255. }.into()
}

/// Trechos coloridos do código com o lobe-theme; sem gramática, texto na cor padrão.
pub fn realcar(lang: &str, codigo: &str) -> Vec<(Range<usize>, Marca)> {
    let falso = format!("bloco.{lang}");
    let lingua = crate::linguagem::linguagem(&falso).or_else(|| (!lang.is_empty()).then(|| lang.to_lowercase()));
    let Some(nome) = lingua.as_deref().and_then(|l| crate::realce::sintaxe(l, &falso)).map(|s| s.name.clone()) else { return Vec::new() };
    let Some(sintaxe) = SINTAXES.find_syntax_by_name(&nome) else { return Vec::new() };
    let hl = Highlighter::new(&TEMA);
    let mut parse = ParseState::new(sintaxe);
    let mut estado = syntect::highlighting::HighlightState::new(&hl, ScopeStack::new());
    let mut out: Vec<(Range<usize>, Marca)> = Vec::new();
    let mut offset = 0;
    for bruta in syntect::util::LinesWithEndings::from(codigo) {
        // CRLF vira LF para a gramática, como no realce do editor.
        let lf;
        let linha = match bruta.strip_suffix("\r\n") {
            Some(l) => {
                lf = format!("{l}\n");
                lf.as_str()
            }
            None => bruta,
        };
        let Ok(ops) = parse.parse_line(linha, &SINTAXES) else { break };
        let mut pos = 0;
        for (st, pedaco) in syntect::highlighting::HighlightIterator::new(&mut estado, &ops, linha, &hl) {
            let r = offset + pos..offset + pos + pedaco.trim_end_matches(['\n', '\r']).len();
            pos += pedaco.len();
            if r.is_empty() {
                continue;
            }
            let m = Marca {
                cor: Some(hsla(st.foreground)),
                italico: st.font_style.contains(Estilo::ITALIC),
                negrito: st.font_style.contains(Estilo::BOLD),
                ..Default::default()
            };
            match out.last_mut() {
                Some((a, b)) if a.end == r.start && *b == m => a.end = r.end,
                _ => out.push((r, m)),
            }
        }
        offset += bruta.len();
    }
    out
}

// ---- desenho ----

/// O `CopyButton` do Lobe: ícone de copiar, e o de confirmação por 2 s depois do clique (`useCopied`).
fn botao_copiar(id: SharedString, conteudo: String, bloco: f32, icone: f32, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let quando = window.use_keyed_state(SharedString::from(format!("{id}-copiado")), cx, |_, _| std::rc::Rc::new(std::cell::Cell::new(None::<Instant>))).read(cx).clone();
    let copiado = quando.get().is_some_and(|t| t.elapsed() < Duration::from_secs(2));
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .size(px(bloco))
        .rounded(px((bloco / 6.).floor()))
        .cursor_pointer()
        .text_color(if copiado { lobe::texto() } else { lobe::texto_terciario() })
        .when(copiado, |d| d.bg(lobe::fill_secundario()))
        .hover(|s| s.text_color(lobe::texto_secundario()).bg(lobe::fill_terciario()))
        .child(
            gpui::svg()
                .path(if copiado { "icons/check.svg" } else { "icons/copy.svg" })
                .size(px(icone))
                .text_color(if copiado { lobe::texto() } else { lobe::texto_terciario() }),
        )
        .on_click(move |_, window, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(conteudo.clone()));
            quando.set(Some(Instant::now()));
            window.refresh();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(Duration::from_millis(2010)).await;
                    let _ = cx.update(|window, _| window.refresh());
                })
                .detach();
        })
}

/// Nome da linguagem no rótulo do `Highlighter` (`getCodeLanguageDisplayName`, o `name` do Shiki).
fn nome_linguagem(lang: &str) -> String {
    if lang.is_empty() {
        return "Plaintext".into();
    }
    let falso = format!("bloco.{lang}");
    crate::linguagem::linguagem(&falso)
        .or_else(|| Some(lang.to_lowercase()))
        .and_then(|l| crate::linguagem::info_shiki(&l).map(|i| i.name.clone()))
        .unwrap_or_else(|| "Plaintext".into())
}

pub fn desenhar_codigo(no: &MarkdownNode, window: &mut Window, cx: &mut App) -> AnyElement {
    let Some(c) = no.data::<Codigo>() else { return div().into_any_element() };
    let chave = no.source_range().map_or(0, |r| r.start);
    let id = SharedString::from(format!("md-codigo-{chave}"));
    let mono: SharedString = tema::FONTE_MONO.into();
    let texto = c.codigo.trim().to_string();
    let marcas = realcar(&c.lang, &texto);
    if c.linha_unica() {
        // `Snippet`: 38 px de altura, 12/8 px nas laterais, copiar sempre visível.
        return div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(38.))
            .pl(px(12.))
            .pr(px(8.))
            .rounded(px(8.))
            .bg(lobe::fill_terciario())
            .overflow_hidden()
            .child(
                div()
                    .id(SharedString::from(format!("{id}-rolagem")))
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .whitespace_nowrap()
                    .font_family(mono.clone())
                    // 0,85 em no Lobe; medido no React, a Cascadia sai com 12,6 px.
                    .text_size(px(12.6))
                    .text_color(lobe::texto())
                    .child(TextoRico::new(SharedString::from(format!("{id}-texto")), texto.clone(), marcas, mono)),
            )
            .child(botao_copiar(SharedString::from(format!("{id}-copiar")), texto, 24., 14., window, cx))
            .into_any_element();
    }
    // `Highlighter` preenchido: 16 px de respiro, 12 px de texto e 4 px entre as linhas; ações e linguagem no hover.
    let grupo = SharedString::from(format!("{id}-grupo"));
    div()
        .id(id.clone())
        .group(grupo.clone())
        .relative()
        .w_full()
        .rounded(px(8.))
        .bg(lobe::fill_quaternario())
        .overflow_hidden()
        .child(
            div()
                .id(SharedString::from(format!("{id}-rolagem")))
                .w_full()
                .overflow_x_scroll()
                .p(px(16.))
                .whitespace_nowrap()
                .font_family(mono.clone())
                .text_size(px(12.))
                .line_height(px(12. * 1.8 + 4.))
                .text_color(lobe::texto())
                .child(TextoRico::new(SharedString::from(format!("{id}-texto")), texto.clone(), marcas, mono.clone())),
        )
        .child(
            div()
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .opacity(0.)
                .group_hover(grupo.clone(), |s| s.opacity(1.))
                .rounded(px(4.))
                .bg(lobe::fill_terciario())
                .child(botao_copiar(SharedString::from(format!("{id}-copiar")), texto, 28., 16., window, cx)),
        )
        .child(
            div()
                .absolute()
                .bottom(px(8.))
                .right(px(8.))
                .opacity(0.)
                .group_hover(grupo, |s| s.opacity(1.))
                .h(px(22.))
                .px(px(7.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .bg(lobe::fill_quaternario())
                .font_family(mono)
                .text_size(px(12.))
                .text_color(lobe::texto_secundario())
                .child(nome_linguagem(&c.lang)),
        )
        .into_any_element()
}

/// Imagens com `max-width: 100%`, raio de 8 px e o contorno `--lobe-markdown-border-color`; a que não carrega fica
/// como a caixa vazia de 32 px do navegador.
pub fn desenhar_imagens(no: &MarkdownNode, fonte: &dyn Fn(&str) -> Option<gpui::ImageSource>) -> AnyElement {
    let Some(Imagens(itens)) = no.data::<Imagens>() else { return div().into_any_element() };
    // Caixa de 32 px com o contorno de `box-shadow` (1 px por fora): 34 px visíveis.
    let caixa = || div().size(px(34.)).my(px(1.)).rounded(px(9.)).border_1().border_color(lobe::fill_secundario()).into_any_element();
    div()
        .w_full()
        .flex()
        .flex_wrap()
        .items_end()
        .children(itens.iter().map(|(url, _)| match fonte(url) {
            Some(f) => gpui::img(f).max_w_full().rounded(px(8.)).with_fallback(caixa).with_loading(caixa).into_any_element(),
            None => caixa(),
        }))
        .into_any_element()
}

/// Lista de tarefas do Lobe: texto a 2 em da margem (`ul` e `li` com 1 em), itens a `0,33 em × 2` um do outro e o
/// checkbox desabilitado do Chromium escuro (13 px, cinza; marcado em #757575 com o visto em #3b3b3b).
pub fn desenhar_tarefas(no: &MarkdownNode, _window: &mut Window, _cx: &mut App) -> AnyElement {
    let Some(Tarefas(itens)) = no.data::<Tarefas>() else { return div().into_any_element() };
    let chave = no.source_range().map_or(0, |r| r.start);
    let mono: SharedString = tema::FONTE_MONO.into();
    let linha = FONTE * 1.8;
    div()
        .flex()
        .flex_col()
        .gap(px(FONTE * 0.33 * 2.))
        .line_height(px(linha))
        .children(itens.iter().enumerate().map(|(i, (marcado, c))| {
            let caixa = div()
                .absolute()
                .left(px(6.))
                .top(px(((linha - 13.) / 2.).round()))
                .size(px(13.))
                .rounded(px(2.))
                .flex()
                .items_center()
                .justify_center()
                .when(*marcado, |d| d.bg(gpui::rgb(0x757575)).child(gpui::svg().path("icons/check.svg").size(px(11.)).text_color(gpui::rgb(0x3b3b3b))))
                .when(!marcado, |d| d.bg(gpui::rgb(0x3b3b3b)).border_1().border_color(gpui::rgb(0x626262)));
            div()
                .relative()
                .pl(px(26.))
                .child(caixa)
                .child(TextoRico::new(SharedString::from(format!("md-tarefa-{chave}-{i}")), c.texto.clone(), c.marcas.clone(), mono.clone()))
        }))
        .into_any_element()
}

pub fn desenhar_tabela(no: &MarkdownNode, _window: &mut Window, _cx: &mut App) -> AnyElement {
    let Some(t) = no.data::<Tabela>() else { return div().into_any_element() };
    let chave = no.source_range().map_or(0, |r| r.start);
    let colunas = t.linhas.iter().map(Vec::len).max().unwrap_or(0);
    let mono: SharedString = tema::FONTE_MONO.into();
    // Células sem quebra: a altura de cada linha é a mesma em todas as colunas (0,75 em × 2 + 1,8 em).
    let altura = FONTE * 0.75 * 2. + FONTE * 1.8;
    let coluna = |j: usize| {
        let alinha = t.alinhamentos.get(j).copied().unwrap_or(Alinhamento::Inicio);
        div().flex().flex_col().flex_none().min_w(px(120.)).children(t.linhas.iter().enumerate().map(|(i, l)| {
            let c = l.get(j).cloned().unwrap_or_default();
            div()
                .flex()
                .items_center()
                .h(px(altura))
                .px(px(FONTE))
                .when(alinha == Alinhamento::Centro, |d| d.justify_center())
                .when(alinha == Alinhamento::Fim, |d| d.justify_end())
                .when(i == 0, |d| d.bg(lobe::fill_quaternario()).font_weight(FontWeight::BOLD))
                .relative()
                // O `box-shadow: 0 1px 0` do `tr`: uma linha de 1 px logo abaixo, sem ocupar espaço.
                .child(div().absolute().left_0().right_0().bottom(px(-1.)).h(px(1.)).bg(lobe::borda_secundaria()))
                .child(TextoRico::new(SharedString::from(format!("md-tabela-{chave}-{i}-{j}")), c.texto, c.marcas, mono.clone()))
        }))
    };
    // O contorno do React é `box-shadow` (fora da caixa): 1 px a mais acima e abaixo, e a primeira linha 1 px mais baixa.
    div()
        .flex()
        .my(px(1.))
        .child(
            div()
                .id(SharedString::from(format!("md-tabela-{chave}")))
                .pt(px(1.))
                .flex()
                .flex_none()
                .max_w_full()
                .overflow_x_scroll()
                .rounded(px(8.))
                .border_1()
                .border_color(lobe::borda_secundaria())
                .whitespace_nowrap()
                .line_height(px(FONTE * 1.8))
                .children((0..colunas).map(coluna)),
        )
        .into_any_element()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn arvore(fonte: &str) -> md::Node {
        markdown::to_mdast(fonte, &markdown::ParseOptions::gfm()).unwrap()
    }

    fn primeiro(no: &md::Node, f: &dyn Fn(&md::Node) -> bool) -> Option<md::Node> {
        if f(no) {
            return Some(no.clone());
        }
        no.children()?.iter().find_map(|c| primeiro(c, f))
    }

    #[test]
    fn codigo_de_uma_linha_vira_snippet() {
        let n = arvore("```ts\nconst x: number = 1;\n```");
        let c = primeiro(&n, &|n| matches!(n, md::Node::Code(_))).unwrap();
        let no = analisar(&c).unwrap();
        let d = no.data::<Codigo>().unwrap();
        assert_eq!(d.lang, "ts");
        assert!(d.linha_unica());
        assert!(!Codigo { lang: String::new(), codigo: "a\nb".into() }.linha_unica());
        assert!(!Codigo { lang: String::new(), codigo: "x".repeat(32) }.linha_unica());
    }

    #[test]
    fn tabela_com_estilos_em_linha() {
        let n = arvore("| A | **B** |\n| :-: | --: |\n| `c` | [d](x) ~~e~~ |\n");
        let t = primeiro(&n, &|n| matches!(n, md::Node::Table(_))).unwrap();
        let no = analisar(&t).unwrap();
        let t = no.data::<Tabela>().unwrap();
        assert_eq!(t.alinhamentos, [Alinhamento::Centro, Alinhamento::Fim]);
        assert_eq!(t.linhas.len(), 2);
        assert_eq!(t.linhas[0][1].texto, "B");
        assert!(t.linhas[0][1].marcas[0].1.negrito);
        assert!(t.linhas[1][0].marcas[0].1.mono);
        assert_eq!(t.linhas[1][1].texto, "d e");
        assert_eq!(t.linhas[1][1].marcas[0].1.cor, Some(lobe::info()));
        assert!(t.linhas[1][1].marcas[1].1.tachado);
        assert_eq!(no.as_text(), "A\tB\nc\td e");
    }

    #[test]
    fn listas_de_tarefas() {
        let n = arvore("- [x] feito
- [ ] **pendente**
");
        let l = primeiro(&n, &|n| matches!(n, md::Node::List(_))).unwrap();
        let no = analisar(&l).unwrap();
        let t = &no.data::<Tarefas>().unwrap().0;
        assert_eq!((t[0].0, t[0].1.texto.as_str()), (true, "feito"));
        assert!(!t[1].0 && t[1].1.marcas[0].1.negrito);
        let comum = arvore("- a
- [ ] b
");
        assert!(analisar(&primeiro(&comum, &|n| matches!(n, md::Node::List(_))).unwrap()).is_none());
    }

    #[test]
    fn paragrafo_so_de_imagens() {
        let n = arvore("![logo](../a.png) ![b](https://x/b.png)\n\ntexto ![c](c.png)\n");
        let p = primeiro(&n, &|n| matches!(n, md::Node::Paragraph(_))).unwrap();
        let no = analisar(&p).unwrap();
        assert_eq!(no.data::<Imagens>().unwrap().0[0], ("../a.png".into(), "logo".into()));
        let segundo = match &n {
            md::Node::Root(r) => r.children[1].clone(),
            _ => unreachable!(),
        };
        assert!(analisar(&segundo).is_none());
    }

    #[test]
    fn realce_com_o_lobe_theme() {
        let c = "const x: number = 1;";
        let m = realcar("ts", c);
        let cor = |s: &str| {
            let i = c.find(s).unwrap();
            m.iter().find(|(r, _)| r.contains(&i)).and_then(|(_, m)| m.cor)
        };
        // storage.type: purple10; support.type: colorWarning; constant.numeric: volcano10 (como na captura do React).
        assert_eq!(cor("const"), Some(gpui::rgb(0xd590da).into()));
        assert_eq!(cor("number"), Some(gpui::rgb(0xffb224).into()));
        assert_eq!(cor("1"), Some(gpui::rgb(0xff9480).into()));
        assert!(realcar("linguagem-que-nao-existe", "x").is_empty());
    }
}
