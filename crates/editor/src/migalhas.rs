//! O caminho do arquivo embaixo das abas (porte de `src/editor/breadcrumbs.tsx`): 22 px, pastas e arquivo separados por
//! `›`. Clicar num item abre o seletor com o conteúdo da pasta dele; pasta desce um nível, arquivo abre como prévia.
use crate::area::AreaEditor;
use crate::modelo::Grupo;
use gpui::{anchored, deferred, div, prelude::*, px, AnyElement, MouseButton, SharedString, WeakEntity};
use orquestrador_core::ide::files::Entry;
use orquestrador_ui::icone_arquivo;
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::{icone, TooltipTexto};

/// Seletor aberto: o item marcado e a pasta que ele mostra agora.
pub struct Seletor {
    pub grupo: Grupo,
    pub indice: usize,
    pub pasta: String,
    pub marcado: String,
    pub entradas: Option<Vec<Entry>>,
    pub erro: Option<String>,
    pub ancora: Option<gpui::Point<gpui::Pixels>>,
}

impl Seletor {
    pub fn new(grupo: Grupo, indice: usize, pasta: String, marcado: String) -> Self {
        Seletor { grupo, indice, pasta, marcado, entradas: None, erro: None, ancora: None }
    }
}

/// A barra; `extra` fica à direita (o seletor Preview | Markdown).
pub fn barra(caminho: &str, g: Grupo, aberto: Option<&Seletor>, extra: Option<AnyElement>, ent: WeakEntity<AreaEditor>) -> impl IntoElement {
    let partes: Vec<String> = caminho.split(['/', '\\']).filter(|s| !s.is_empty()).map(str::to_string).collect();
    let n = partes.len();
    let mut nav = div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(22.))
        .overflow_hidden()
        .bg(cor::panel_black())
        .pl(px(4.))
        .text_size(tema::TEXTO_MENU)
        .text_color(cor::text_secondary());
    for (i, nome) in partes.iter().enumerate() {
        let ultimo = i + 1 == n;
        let cheio = partes[..=i].join("/");
        let aberto_aqui = aberto.is_some_and(|s| s.grupo == g && s.indice == i);
        let (ent, caminho) = (ent.clone(), caminho.to_string());
        nav = nav.child(
            div()
                .flex()
                .h_full()
                .items_center()
                .when(ultimo, |d| d.min_w_0())
                .when(!ultimo, |d| d.flex_none())
                .child(
                    div()
                        .id(SharedString::from(format!("migalha-{g:?}-{i}")))
                        .flex()
                        .h_full()
                        .min_w_0()
                        .items_center()
                        .gap(px(6.))
                        .px(px(4.))
                        .rounded(px(2.))
                        .cursor_pointer()
                        .hover(|s| s.text_color(cor::text_primary()))
                        .when(aberto_aqui, |d| d.text_color(cor::text_primary()))
                        .tooltip_texto(cheio.clone())
                        .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                            let pos = ev.position;
                            let _ = ent.update(cx, |a, cx| {
                                if aberto_aqui {
                                    a.fechar_migalha(cx);
                                } else {
                                    a.abrir_seletor_migalha(g, i, &caminho, window, cx);
                                    a.ancorar_migalha(pos);
                                }
                            });
                            cx.stop_propagation();
                        })
                        .when(ultimo, |d| d.child(gpui::img(icone_arquivo(&cheio, false, false)).size(px(16.)).flex_none()))
                        .child(div().truncate().child(nome.clone())),
                )
                .when(!ultimo, |d| d.child(icone("icons/chevron-right.svg", 16., cor::text_secondary()))),
        );
    }
    nav.children(extra.map(|e| div().ml_auto().flex().flex_none().h_full().items_center().px(px(8.)).child(e)))
}

/// O seletor da pasta, ancorado embaixo do item clicado.
pub fn seletor(s: &Seletor, ent: WeakEntity<AreaEditor>) -> AnyElement {
    let mut caixa = div()
        .id("migalha-seletor")
        .occlude()
        .w(px(256.))
        .p(px(4.))
        .rounded(tema::RAIO_MD)
        .border_1()
        .border_color(gpui::rgba(0xffffff26))
        .bg(cor::raised_black())
        .font_family(tema::FONTE)
        .text_color(cor::text_primary())
        .on_mouse_down_out({
            let ent = ent.clone();
            move |_, _, cx| {
                let _ = ent.update(cx, |a, cx| a.fechar_migalha(cx));
            }
        });
    if !s.pasta.is_empty() {
        let (ent, acima) = (ent.clone(), crate::caminhos::separar(&s.pasta).1.to_string());
        caixa = caixa.child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .h(px(22.))
                .px(px(6.))
                .border_b_1()
                .border_color(cor::hairline())
                .text_size(tema::TEXTO_LABEL)
                .text_color(cor::text_secondary())
                .child(
                    div()
                        .id("migalha-acima")
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(16.))
                        .rounded(px(2.))
                        .cursor_pointer()
                        .hover(|s| s.bg(cor::fill_muted()))
                        .tooltip_texto("Pasta de cima")
                        .on_click(move |_, window, cx| {
                            let _ = ent.update(cx, |a, cx| a.ler_pasta_migalha(acima.clone(), window, cx));
                        })
                        .child(icone("icons/chevron-left.svg", 14., cor::text_secondary())),
                )
                .child(div().truncate().child(s.pasta.clone())),
        );
    }
    let mut lista = div().id("migalha-lista").flex().flex_col().py(px(2.)).max_h(px(300.)).overflow_y_scroll();
    if let Some(e) = &s.erro {
        lista = lista.child(div().px(px(8.)).py(px(4.)).text_size(tema::TEXTO_LABEL).text_color(cor::signal_error()).child(e.clone()));
    } else if s.entradas.is_none() {
        lista = lista.child(div().px(px(8.)).py(px(4.)).text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child("Lendo a pasta…"));
    }
    for (i, e) in s.entradas.iter().flatten().enumerate() {
        let cheio = crate::caminhos::juntar(&s.pasta, &e.name);
        let marcado = cheio == s.marcado;
        let (ent, c, pasta) = (ent.clone(), cheio.clone(), e.dir);
        lista = lista.child(
            div()
                .id(SharedString::from(format!("migalha-item-{i}")))
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(22.))
                .px(px(8.))
                .text_size(tema::TEXTO_MENU)
                .when(e.ignored, |d| d.text_color(cor::text_tertiary()))
                .when(marcado, |d| d.bg(cor::fill_hover()))
                .cursor_pointer()
                .hover(|s| s.bg(cor::fill_muted()))
                .tooltip_texto(cheio.clone())
                .on_click(move |_, window, cx| {
                    let _ = ent.update(cx, |a, cx| {
                        if pasta {
                            a.ler_pasta_migalha(c.clone(), window, cx);
                        } else {
                            a.fechar_migalha(cx);
                            a.abrir_do_markdown(c.clone(), window, cx);
                        }
                    });
                })
                .child(gpui::img(icone_arquivo(&cheio, e.dir, false)).size(px(16.)).flex_none())
                .child(div().flex_1().min_w_0().truncate().child(e.name.clone()))
                .when(e.dir, |d| d.child(icone("icons/chevron-right.svg", 14., cor::text_secondary()))),
        );
    }
    caixa = caixa.child(lista);
    let pos = s.ancora.unwrap_or_default();
    deferred(anchored().position(pos).snap_to_window_with_margin(px(8.)).child(caixa)).with_priority(1).into_any_element()
}

impl AreaEditor {
    pub(crate) fn ancorar_migalha(&mut self, p: gpui::Point<gpui::Pixels>) {
        if let Some(s) = self.migalha_mut() {
            // Abaixo do item (a barra tem 22 px): o ponteiro está no meio dela.
            s.ancora = Some(gpui::point(p.x - px(8.), p.y + px(12.)));
        }
    }
}
