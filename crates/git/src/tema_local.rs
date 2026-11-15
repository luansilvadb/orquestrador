//! Tema do Kit trocado só dentro de um elemento: o `Diff` do Kit lê as cores e a fonte mono do tema global ao montar e ao
//! desenhar as linhas, e o comparador usa as do editor (fundo #1F1F1F, Consolas 14 px, vermelho e verde do VS Code). O
//! elemento aplica a troca em cada fase (layout, prepaint e paint) do filho e devolve o tema anterior em seguida.
use gpui_kit::component::Theme;
use gpui_kit::{AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Window};
use std::rc::Rc;

pub struct TemaLocal {
    filho: AnyElement,
    trocar: Rc<dyn Fn(&mut Theme)>,
}

pub fn tema_local(filho: impl IntoElement, trocar: impl Fn(&mut Theme) + 'static) -> TemaLocal {
    TemaLocal { filho: filho.into_any_element(), trocar: Rc::new(trocar) }
}

impl TemaLocal {
    fn com<R>(&mut self, cx: &mut App, f: impl FnOnce(&mut AnyElement, &mut App) -> R) -> R {
        let antes = Theme::global(cx).clone();
        (self.trocar)(Theme::global_mut(cx));
        let r = f(&mut self.filho, cx);
        *Theme::global_mut(cx) = antes;
        r
    }
}

impl IntoElement for TemaLocal {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TemaLocal {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.com(cx, |f, cx| f.request_layout(window, cx)), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.com(cx, |f, cx| f.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.com(cx, |f, cx| f.paint(window, cx));
    }
}
