//! Gatilho de dropdown: o `on_mouse_down_out` do menu ignora o clique que cai no próprio gatilho, para o `on_click`
//! dele alternar (fechar) sem o menu fechar no mouse down e reabrir no clique.
use gpui_kit::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

/// Guarde um por gatilho na entidade dona.
#[derive(Clone, Default)]
pub struct Gatilho(Rc<Cell<bool>>);

impl Gatilho {
    /// Marca o elemento como gatilho, acompanhando o hover dele.
    pub fn marcar<E: StatefulInteractiveElement>(&self, el: E) -> E {
        let sobre = self.0.clone();
        el.on_hover(move |h, _, _| sobre.set(*h))
    }

    /// O ponteiro está sobre o gatilho; use no `on_mouse_down_out` do menu.
    pub fn sobre(&self) -> bool {
        self.0.get()
    }
}
