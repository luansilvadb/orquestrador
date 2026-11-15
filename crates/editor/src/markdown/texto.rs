//! Texto com estilos por trecho que entra na seleção da janela (`TextSelectionLayer` do Kit), como o `SelectableText`
//! do Kit, que só aceita um estilo para o texto todo. Usado nos blocos de código e nas células de tabela do Preview.
use gpui::{
    App, Bounds, Element, ElementId, FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, SharedString, StrikethroughStyle, StyledText, TextRun, Window,
};
use gpui_kit::base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use std::ops::Range;

/// Estilo de um trecho; `None`/`false` herda o do texto.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Marca {
    pub cor: Option<Hsla>,
    pub fundo: Option<Hsla>,
    pub negrito: bool,
    pub italico: bool,
    pub tachado: bool,
    pub mono: bool,
}

pub struct TextoRico {
    id: ElementId,
    texto: SharedString,
    marcas: Vec<(Range<usize>, Marca)>,
    mono: SharedString,
    texto_el: Option<StyledText>,
}

impl TextoRico {
    /// `marcas` em ordem e sem sobreposição; `mono` é a fonte dos trechos com `Marca::mono`.
    pub fn new(id: impl Into<ElementId>, texto: impl Into<SharedString>, marcas: Vec<(Range<usize>, Marca)>, mono: impl Into<SharedString>) -> Self {
        TextoRico { id: id.into(), texto: texto.into(), marcas, mono: mono.into(), texto_el: None }
    }
}

/// Corridas que cobrem o texto inteiro, com os trechos marcados por cima do estilo base.
pub fn corridas(texto: &str, marcas: &[(Range<usize>, Marca)], base: &gpui::TextStyle, mono: &SharedString) -> Vec<TextRun> {
    let corrida = |len: usize, m: Marca| {
        let mut font = base.font();
        if m.mono {
            font.family = mono.clone();
        }
        if m.negrito {
            font.weight = FontWeight::BOLD;
        }
        if m.italico {
            font.style = FontStyle::Italic;
        }
        let cor = m.cor.unwrap_or(base.color);
        TextRun {
            len,
            font,
            color: cor,
            background_color: m.fundo,
            underline: None,
            strikethrough: m.tachado.then(|| StrikethroughStyle { thickness: gpui::px(1.), color: Some(cor) }),
        }
    };
    let mut out = Vec::new();
    let mut pos = 0;
    for (r, m) in marcas {
        let (a, b) = (r.start.clamp(pos, texto.len()), r.end.min(texto.len()));
        if a > pos {
            out.push(corrida(a - pos, Marca::default()));
        }
        if b > a {
            out.push(corrida(b - a, *m));
            pos = b;
        } else {
            pos = a;
        }
    }
    if pos < texto.len() || out.is_empty() {
        out.push(corrida(texto.len() - pos, Marca::default()));
    }
    out
}

impl IntoElement for TextoRico {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextoRico {
    type RequestLayoutState = TextSelectionHandle;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self, global_id: Option<&GlobalElementId>, inspector_id: Option<&InspectorElementId>, window: &mut Window, cx: &mut App,
    ) -> (LayoutId, TextSelectionHandle) {
        let texto = self.texto.clone();
        let handle = window.with_element_state(global_id.expect("id estável"), |guardado: Option<TextSelectionHandle>, _| {
            let h = guardado.unwrap_or_else(|| TextSelectionHandle::new(texto.clone(), cx));
            (h.clone(), h)
        });
        let runs = corridas(&self.texto, &self.marcas, &window.text_style(), &self.mono);
        let el = self.texto_el.insert(StyledText::new(self.texto.clone()).with_runs(runs));
        let (id, ()) = el.request_layout(global_id, inspector_id, window, cx);
        (id, handle)
    }

    fn prepaint(
        &mut self, global_id: Option<&GlobalElementId>, inspector_id: Option<&InspectorElementId>, bounds: Bounds<Pixels>,
        handle: &mut TextSelectionHandle, window: &mut Window, cx: &mut App,
    ) -> Hitbox {
        if let Some(el) = self.texto_el.as_mut() {
            el.prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        }
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let registro = TextSelectionRegistration::new(hitbox.clone(), bounds).with_text_bounds(vec![bounds]).with_rendered_element(handle, window, cx);
        handle.register(registro, window, cx);
        hitbox
    }

    fn paint(
        &mut self, global_id: Option<&GlobalElementId>, inspector_id: Option<&InspectorElementId>, bounds: Bounds<Pixels>,
        handle: &mut TextSelectionHandle, _: &mut Hitbox, window: &mut Window, cx: &mut App,
    ) {
        let Some(el) = self.texto_el.as_mut() else { return };
        let layout = el.layout().clone();
        let projecao = handle.update_runs(&[TextSelectionRun::new(self.texto.clone(), layout.clone(), bounds)], cx);
        let cor = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
        for r in projecao.ranges().iter().flatten() {
            pintar_selecao(&layout, r.clone(), cor, window);
        }
        el.paint(global_id, inspector_id, bounds, &mut (), &mut (), window, cx);
    }
}

fn pintar_selecao(layout: &gpui::TextLayout, r: Range<usize>, cor: Hsla, window: &mut Window) {
    let (Some(a), Some(b)) = (layout.position_for_index(r.start), layout.position_for_index(r.end)) else { return };
    let caixa = layout.bounds();
    let lh = layout.line_height();
    let mut quads = Vec::new();
    if a.y == b.y {
        quads.push(Bounds::from_corners(a, gpui::point(b.x, b.y + lh)));
    } else {
        quads.push(Bounds::from_corners(a, gpui::point(caixa.right(), a.y + lh)));
        if b.y > a.y + lh {
            quads.push(Bounds::from_corners(gpui::point(caixa.left(), a.y + lh), gpui::point(caixa.right(), b.y)));
        }
        quads.push(Bounds::from_corners(gpui::point(caixa.left(), b.y), gpui::point(b.x, b.y + lh)));
    }
    for q in quads {
        window.paint_quad(gpui::fill(q, cor));
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn corridas_cobrem_o_texto() {
        let base = gpui::TextStyle::default();
        let m = Marca { negrito: true, ..Default::default() };
        let r = corridas("abcdef", &[(1..3, m)], &base, &"Mono".into());
        assert_eq!(r.iter().map(|r| r.len).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(r[1].font.weight, FontWeight::BOLD);
        assert_eq!(corridas("", &[], &base, &"Mono".into()).len(), 1);
    }
}
