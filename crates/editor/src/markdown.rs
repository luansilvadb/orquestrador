//! Markdown: o seletor "Preview | Markdown" na linha do breadcrumb (`markdown-switch.tsx`) e o Preview renderizado
//! (`markdown-preview.tsx`) no visual do `Markdown` do Lobe UI 5.58: coluna de até 800 px, 32 px de margem lateral,
//! 24 px em cima, texto de 14 px. O fluxo do texto (parágrafos, títulos, listas, citações, seleção) é o `TextView` do
//! Kit; blocos de código e tabelas são desenhados em [`blocos`]. Link http(s) abre no navegador; relativo abre no
//! editor; âncora rola até o título; imagem relativa vem do disco.
mod blocos;
mod texto;

use crate::area::AreaEditor;
use crate::caminhos::{normalizar_segmentos, separar};
use blocos::lobe;
use gpui::{div, prelude::*, px, AnyElement, App, ImageSource, ScrollHandle, SharedString, WeakEntity, Window};
use gpui_kit::base::text::{
    markdown_ast, InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, TextView, TextViewState,
    TextViewStyle,
};
use gpui_kit::TestSupportExt as _;
use orquestrador_ui::tema::{self, cor};

/// O seletor: dois botões de 18 px; o ligado com fundo.
pub fn seletor(previa: bool, ao_mudar: impl Fn(bool, &mut Window, &mut App) + 'static) -> impl IntoElement {
    let ao_mudar = std::rc::Rc::new(ao_mudar);
    let botao = |id: &'static str, rotulo: &'static str, ligado: bool, valor: bool| {
        let f = ao_mudar.clone();
        div()
            .id(id)
            .flex()
            .items_center()
            .h_full()
            .px(px(6.))
            .rounded(px(2.))
            .text_size(tema::TEXTO_LABEL)
            .line_height(px(12.))
            .when(ligado, |d| d.bg(cor::fill_hover()).text_color(cor::text_primary()))
            .cursor_pointer()
            .when(!ligado, |d| d.text_color(cor::text_secondary()).hover(|s| s.text_color(cor::text_primary())))
            .on_click(move |_, window, cx| f(valor, window, cx))
            .child(rotulo)
            .test_support()
    };
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .h(px(18.))
        .child(botao("md-preview", "Preview", previa, true))
        .child(botao("md-codigo", "Markdown", !previa, false))
}

/// Caminho relativo ao projeto a partir da pasta do `.md`; `None` se sair da raiz.
pub fn resolver(base: &str, alvo: &str) -> Option<String> {
    let pasta: Vec<&str> = if alvo.starts_with('/') { Vec::new() } else { separar(base).1.split('/').collect() };
    let sem_query = alvo.split(['?', '#']).next().unwrap_or("");
    let decodificado = decodificar(sem_query);
    let partes: Vec<&str> = pasta.into_iter().chain(decodificado.split('/')).collect();
    normalizar_segmentos(partes).map(|v| v.join("/"))
}

fn decodificar(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn eh_web(url: &str) -> bool {
    let l = url.to_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:")
}
/// Links relativos clicados no Preview, à espera da área (o clique do `TextView` exige `Send`).
static PEDIDOS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
/// Âncoras (`#titulo`) clicadas, com o id do Preview.
static ANCORAS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn pedidos() -> Vec<String> {
    PEDIDOS.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
}

/// O `slug` de markdown-preview.tsx.
pub fn slug(texto: &str) -> String {
    let limpo: String = texto.trim().to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-').collect();
    limpo.split_whitespace().collect::<Vec<_>>().join("-")
}

/// Faixa da fonte (bytes) do título ATX cujo texto ou slug é `alvo`, fora de blocos cercados.
pub fn titulo(fonte: &str, alvo: &str) -> Option<std::ops::Range<usize>> {
    let mut cerca: Option<&str> = None;
    let mut pos = 0;
    for linha in fonte.split_inclusive('\n') {
        let ini = pos;
        pos += linha.len();
        let t = linha.trim_start();
        if let Some(c) = cerca {
            if t.starts_with(c) {
                cerca = None;
            }
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            cerca = Some(&t[..3]);
            continue;
        }
        let n = t.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&n) && t[n..].starts_with([' ', '\t']) {
            let texto = t[n..].trim().trim_end_matches('#').trim();
            if texto == alvo || slug(texto) == alvo {
                return Some(ini..ini + linha.trim_end_matches(['\n', '\r']).len());
            }
        }
    }
    None
}

/// Tamanho do título no `Markdown` do Lobe (`headerMultiple` 1): 2,5/2/1,5/1,25/1 × a fonte.
fn tamanho_titulo(nivel: u8) -> f32 {
    blocos::FONTE
        * match nivel {
            1 => 2.5,
            2 => 2.,
            3 => 1.5,
            4 => 1.25,
            _ => 1.,
        }
}

/// Margem do título: `max(0,8 em, fonte)`, em cima e embaixo.
fn margem_titulo(nivel: u8) -> f32 {
    (tamanho_titulo(nivel) * 0.8).max(blocos::FONTE)
}

fn estilo(cx: &App) -> TextViewStyle {
    TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx))
        .with_foreground(lobe::texto())
        .with_muted_foreground(lobe::texto_secundario())
        .with_link(lobe::info())
        .with_border(lobe::borda())
        // 1 em de 14 px entre os blocos (rem de 16 px).
        .with_paragraph_gap(gpui::rems(blocos::FONTE / 16.))
        .with_heading(|nivel| {
            let m = margem_titulo(nivel);
            // O bloco de cima já deixa 1 em; as margens colapsam como no CSS.
            gpui::StyleRefinement::default()
                .text_size(px(tamanho_titulo(nivel)))
                .line_height(gpui::relative(1.25))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(lobe::texto())
                .pt(px(m - blocos::MARGEM))
                .pb(px(m))
        })
        .with_inline_code_style(
            gpui::StyleRefinement::default()
                .font_family(tema::FONTE_MONO)
                // 0,875 em no Lobe, mas o Chromium arredonda o avanço da Cascadia para 8 px: 13,5 px dá a mesma largura.
                .text_size(px(13.5))
                .bg(lobe::fill_secundario())
                .border_1()
                .border_color(lobe::fill_quaternario())
                .rounded(px(blocos::FONTE * 0.875 * 0.25))
                .px(px(blocos::FONTE * 0.875 * 0.4))
                .py(px(blocos::FONTE * 0.875 * 0.1))
                // `margin-inline: 0,25em` do Lobe.
                .mx(px(blocos::FONTE * 0.875 * 0.25)),
        )
        .with_blockquote_style(
            gpui::StyleRefinement::default().border_l(px(4.)).border_color(lobe::borda()).pl(px(blocos::FONTE)).text_color(lobe::texto_secundario()),
        )
        .with_list_style(gpui::StyleRefinement::default().text_color(lobe::texto().opacity(0.5)))
}

/// Realce dos blocos de código do `TextView` padrão do app com o `syntect`/dark-plus do editor. O Preview desenha os
/// próprios blocos com o lobe-theme ([`blocos`]).
pub fn instalar_realce_de_codigo(cx: &mut App) {
    use gpui_kit::base::text::TextViewDefaults;
    let base = cx.try_global::<TextViewDefaults>().cloned().unwrap_or_default();
    base.with_code_block_highlighter(|bloco| {
        let lang = bloco.lang().map(|l| l.to_string()).unwrap_or_default();
        let falso = format!("bloco.{lang}");
        let lingua = crate::linguagem::linguagem(&falso).or_else(|| (!lang.is_empty()).then(|| lang.clone()));
        let sintaxe = lingua.as_deref().and_then(|l| crate::realce::sintaxe(l, &falso));
        if sintaxe.is_none() {
            return Vec::new();
        }
        crate::realce::colorir(sintaxe, &bloco.code())
    })
    .install(cx);
}

/// Imagem relativa ao `.md` (sem sair do projeto) lida do disco; http(s), `data:` e
/// `blob:` pela URL.
pub fn fonte_imagem(projeto: &str, base: &str, url: &str) -> Option<ImageSource> {
    let l = url.to_lowercase();
    if eh_web(url) || l.starts_with("data:") || l.starts_with("blob:") {
        return Some(ImageSource::from(url.to_string()));
    }
    let rel = resolver(base, url)?;
    // Um `rel` absoluto (buffer fora do projeto) substitui a raiz no `join`.
    Some(ImageSource::from(std::path::PathBuf::from(projeto).join(rel)))
}

/// A imagem relativa resolvida só ao carregar (fora do render da área, que não pode ser lida no meio do próprio
/// render): já aparece no primeiro quadro.
fn fonte_imagem_viva(ent: WeakEntity<AreaEditor>, base: String, url: String) -> ImageSource {
    let l = url.to_lowercase();
    if eh_web(&url) || l.starts_with("data:") || l.starts_with("blob:") {
        return ImageSource::from(url);
    }
    ImageSource::Custom(std::sync::Arc::new(move |window: &mut Window, cx: &mut App| {
        let projeto = ent.upgrade().map(|a| a.read(cx).projeto().to_string()).unwrap_or_default();
        match fonte_imagem(&projeto, &base, &url)? {
            ImageSource::Resource(r) => window.use_asset::<gpui::ImgResourceLoader>(&r, cx),
            _ => None,
        }
    }))
}

/// Abre um link do Preview: âncora rola até o título, http(s)/mailto no navegador, relativo no editor.
fn abrir_link(id_previa: &str, base: &str, url: &str, window: &mut Window) {
    if let Some(a) = url.strip_prefix('#') {
        if let Ok(mut v) = ANCORAS.lock() {
            v.push((id_previa.to_string(), decodificar(a)));
        }
        window.refresh();
    } else if eh_web(url) {
        if let Err(e) = orquestrador_core::platform::open_url(url) {
            eprintln!("{e}");
        }
    } else if let Some(alvo) = resolver(base, url) {
        if let Ok(mut v) = PEDIDOS.lock() {
            v.push(alvo);
        }
        window.refresh();
    }
}

/// Links desenhados pelo Preview: o `TextView` sublinha todo link, e o `a` do Lobe não tem sublinhado
/// (`colorInfoText`, `colorInfoHover` no hover).
struct Links {
    id: String,
    base: String,
}

impl MarkdownPlugin for Links {
    fn name(&self) -> &str {
        "orq-md-link"
    }

    fn parse(&self, no: &markdown_ast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(l) = no else { return None };
        let c = blocos::celula(&l.children);
        let texto = c.texto.clone();
        Some(MarkdownNode::new("orq-md-link", (l.url.clone(), c)).text(texto))
    }

    fn render_inline(&self, no: &MarkdownNode, ctx: &InlineRenderContext, _: &mut Window, _: &mut App) -> Option<InlineElement> {
        let (url, c) = no.data::<(String, blocos::Celula)>()?.clone();
        let chave = no.source_range().map_or(0, |r| r.start);
        let (id, base) = (self.id.clone(), self.base.clone());
        let el = div()
            .id(SharedString::from(format!("md-link-{}-{chave}", self.id)))
            .cursor_pointer()
            .line_height(ctx.line_height())
            .text_color(lobe::info())
            .hover(|s| s.text_color(gpui::rgb(0xa7d3ff)))
            .on_click(move |_, window, _| abrir_link(&id, &base, &url, window))
            .child(texto::TextoRico::new(SharedString::from(format!("md-link-texto-{}-{chave}", self.id)), c.texto, c.marcas, tema::FONTE_MONO));
        Some(InlineElement::new(el))
    }
}

/// O Preview do texto atual do buffer.
pub fn previa(id: &str, texto: SharedString, caminho: String, ent: WeakEntity<AreaEditor>, window: &mut Window, cx: &mut App) -> AnyElement {
    let estado = window.use_keyed_state(SharedString::from(format!("md-estado-{id}")), cx, |_, cx| TextViewState::markdown(&texto, cx));
    let ultimo = window
        .use_keyed_state(SharedString::from(format!("md-ultimo-{id}")), cx, |_, _| std::rc::Rc::new(std::cell::RefCell::new(texto.clone())))
        .read(cx)
        .clone();
    if *ultimo.borrow() != texto {
        *ultimo.borrow_mut() = texto.clone();
        estado.update(cx, |s, cx| s.set_text(&texto, cx));
    }
    let rolagem = window.use_keyed_state(SharedString::from(format!("md-rolagem-{id}")), cx, |_, _| ScrollHandle::new()).read(cx).clone();
    let pedidas: Vec<String> = ANCORAS
        .lock()
        .map(|mut v| {
            let (minhas, outras): (Vec<_>, Vec<_>) = std::mem::take(&mut *v).into_iter().partition(|(i, _)| i == id);
            *v = outras;
            minhas.into_iter().map(|(_, a)| a).collect()
        })
        .unwrap_or_default();
    for alvo in pedidas {
        if let Some(r) = titulo(&texto, &alvo) {
            estado.update(cx, |s, cx| {
                if let Some(r) = s.rendered_text().range_for_source(r) {
                    let _ = s.reveal_range(r.start..r.start, cx);
                }
            });
        }
    }
    let base = caminho.clone();
    let id_previa = id.to_string();
    let (ent_img, base_img) = (ent.clone(), caminho.clone());
    let (ent_bloco, base_bloco) = (ent, caminho.clone());
    let comeca_com_titulo = texto.trim_start().starts_with('#');
    let r2 = rolagem.clone();
    let extensoes = MarkdownExtensions::default()
        .parser_revision(1)
        .block_parser(|no, _| blocos::analisar(no))
        .block_renderer(blocos::NOME_CODIGO, blocos::desenhar_codigo)
        .block_renderer(blocos::NOME_TABELA, blocos::desenhar_tabela)
        .block_renderer(blocos::NOME_TAREFAS, blocos::desenhar_tarefas)
        .block_renderer(blocos::NOME_IMAGENS, move |no, _, _| {
            blocos::desenhar_imagens(no, &|url| Some(fonte_imagem_viva(ent_bloco.clone(), base_bloco.clone(), url.to_string())))
        })
        .plugin(Links { id: id.to_string(), base: caminho });
    div()
        .id(SharedString::from(format!("md-{id}")))
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&rolagem)
        .bg(cor::panel_black())
        .child(
            div()
                .mx_auto()
                .max_w(px(800.))
                .px(px(32.))
                .py(px(24.))
                // O `TextView` zera a margem de cima do primeiro bloco; o título do Lobe a mantém.
                .when(comeca_com_titulo, |d| d.pt(px(24. + blocos::MARGEM)))
                .text_size(px(blocos::FONTE))
                .line_height(gpui::relative(1.8))
                .text_color(lobe::texto())
                .child(
                    TextView::new(&estado)
                        .style(estilo(cx))
                        .selectable(true)
                        .markdown_extensions(extensoes)
                        .image_source(move |uri| fonte_imagem_viva(ent_img.clone(), base_img.clone(), uri.to_string()))
                        .on_reveal(move |b, _, _| {
                            let o = r2.offset();
                            r2.set_offset(gpui::point(o.x, o.y - (b.origin.y - r2.bounds().origin.y)));
                        })
                        .on_link_click(move |url: &SharedString, _, window: &mut Window, _: &mut App| abrir_link(&id_previa, &base, url, window)),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn resolve_links_relativos() {
        assert_eq!(resolver("docs/guia.md", "outro.md").as_deref(), Some("docs/outro.md"));
        assert_eq!(resolver("docs/guia.md", "../README.md#x").as_deref(), Some("README.md"));
        assert_eq!(resolver("docs/guia.md", "/src/a%20b.ts").as_deref(), Some("src/a b.ts"));
        assert_eq!(resolver("guia.md", "../../fora.md"), None);
        assert_eq!(resolver("guia.md", "a%20b.md").as_deref(), Some("a b.md"));
    }

    #[test]
    fn ancoras_pelos_titulos() {
        let f = "# Guia do App

```
# não é título
```
## Instalação rápida ##
texto
";
        assert_eq!(slug("Instalação rápida!"), "instalação-rápida");
        assert_eq!(titulo(f, "guia-do-app"), Some(0..13));
        let r = titulo(f, "instalação-rápida").unwrap();
        assert_eq!(&f[r], "## Instalação rápida ##");
        assert_eq!(titulo(f, "não-é-título"), None);
    }

    #[test]
    fn imagens_relativas_ao_arquivo() {
        let caminho = |s: Option<ImageSource>| match s {
            Some(ImageSource::Resource(gpui::Resource::Path(p))) => Some(p.to_string_lossy().replace('\\', "/")),
            Some(ImageSource::Resource(gpui::Resource::Uri(u))) => Some(u.to_string()),
            _ => None,
        };
        assert_eq!(caminho(fonte_imagem("D:/p", "docs/guia.md", "../assets/logo.png")).as_deref(), Some("D:/p/assets/logo.png"));
        assert_eq!(caminho(fonte_imagem("D:/p", "docs/guia.md", "https://x.dev/a.png")).as_deref(), Some("https://x.dev/a.png"));
        assert!(fonte_imagem("D:/p", "guia.md", "../../fora.png").is_none());
        assert_eq!(tamanho_titulo(1), 35.);
        assert_eq!(margem_titulo(1), 28.);
        assert_eq!(margem_titulo(5), 14.);
    }
}
