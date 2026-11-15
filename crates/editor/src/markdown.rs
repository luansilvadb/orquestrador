//! Markdown: o seletor "Preview | Markdown" na linha do breadcrumb (`markdown-switch.tsx`) e o Preview renderizado
//! (`markdown-preview.tsx`) com o `TextView::markdown` do Kit no estilo do `Markdown` do Lobe UI: coluna de até 800 px,
//! 32 px de margem lateral, 24 px em cima, texto de 14 px. Link http(s) abre no navegador; relativo abre no editor;
//! âncora rola até o título.
use crate::area::AreaEditor;
use crate::caminhos::{normalizar_segmentos, separar};
use gpui::{div, prelude::*, px, AnyElement, App, SharedString, WeakEntity, Window};
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::text::{TextView, TextViewStyle};
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
        if b[i] == b'%' && i + 2 < b.len() {
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

fn estilo() -> TextViewStyle {
    let mut cabeca = gpui::StyleRefinement::default();
    cabeca = cabeca.bg(gpui::rgb(0x121212)).text_color(cor::text_primary()).font_weight(gpui::FontWeight::SEMIBOLD);
    let codigo = gpui::StyleRefinement::default().bg(gpui::rgb(0x1b1b1b)).rounded(tema::RAIO_MD).p(px(12.));
    TextViewStyle::default()
        .paragraph_gap(gpui::rems(1.))
        .heading_font_size(|nivel, base| match nivel {
            1 => base * 2.25,
            2 => base * 1.75,
            3 => base * 1.5,
            4 => base * 1.25,
            _ => base,
        })
        .table_head(cabeca)
        .code_block(codigo)
}

/// Cores do `Markdown` do Lobe UI escuro, só durante o desenho do Preview.
fn tema_lobe(t: &mut gpui_kit::component::Theme) {
    t.link = gpui::rgb(0x4e8cc9).into();
    t.muted = gpui::rgb(0x262626).into();
    t.border = gpui::rgb(0x202020).into();
    t.table_head = gpui::rgb(0x121212).into();
    t.table_head_foreground = cor::text_primary();
    t.foreground = cor::text_primary();
}

/// Realce dos blocos de código do Preview com o mesmo `syntect`/dark-plus do editor.
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

/// Links relativos clicados no Preview, à espera da área (o clique do `TextView` exige `Send`).
static PEDIDOS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn pedidos() -> Vec<String> {
    PEDIDOS.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
}

/// O Preview do texto atual do buffer.
pub fn previa(id: &str, texto: SharedString, caminho: String, _ent: WeakEntity<AreaEditor>, _window: &mut Window, _cx: &mut App) -> AnyElement {
    let base = caminho.clone();
    div()
        .id(SharedString::from(format!("md-{id}")))
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .bg(cor::panel_black())
        .child(
            div().mx_auto().max_w(px(800.)).px(px(32.)).py(px(24.)).text_size(tema::TEXTO_BODY).child(crate::documento::ComTema(Some(
                TextView::markdown(SharedString::from(format!("md-texto-{id}")), texto)
                    .style(estilo())
                    .selectable(true)
                    .on_link_click(move |url: &SharedString, _, window: &mut Window, _: &mut App| {
                        if url.starts_with('#') {
                            return;
                        }
                        if eh_web(url) {
                            if let Err(e) = orquestrador_core::platform::open_url(url) {
                                eprintln!("{e}");
                            }
                        } else if let Some(alvo) = resolver(&base, url) {
                            if let Ok(mut v) = PEDIDOS.lock() {
                                v.push(alvo);
                            }
                            window.refresh();
                        }
                    })
                    .into_any_element()), tema_lobe),
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
    }
}
