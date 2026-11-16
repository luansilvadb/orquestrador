//! Lista "Todas as abas": uma linha de 26 px por aba, ícone, rótulo cortado,
//! estado e ● de alteração à direita e o check da ativa no canto. Com `ao_fechar`, o X toma o lugar do check na linha
//! destacada (ponteiro ou teclado). Setas, Home e End destacam, Enter escolhe, Delete fecha a destacada e Esc fecha o menu.
use crate::tema::{self, cor};
use crate::widgets::icone;
use gpui_kit::TestSupportExt;
use gpui_kit::{
    anchored, deferred, div, prelude::*, px, AnyElement, App, Div, ElementId, EntityId, FocusHandle, FontWeight,
    KeyDownEvent, SharedString, Stateful, StyleRefinement, Window,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

struct Interno {
    aberta: Cell<bool>,
    destacada: Cell<Option<usize>>,
    foco: FocusHandle,
    anterior: RefCell<Option<FocusHandle>>,
    gatilho: crate::Gatilho,
}

/// Estado de tela de uma lista; guarde um por lista na entidade dona.
#[derive(Clone)]
pub struct EstadoLista(Rc<Interno>);

impl EstadoLista {
    pub fn new(cx: &mut App) -> Self {
        EstadoLista(Rc::new(Interno {
            aberta: Cell::new(false),
            destacada: Cell::new(None),
            foco: cx.focus_handle(),
            anterior: RefCell::new(None),
            gatilho: crate::Gatilho::default(),
        }))
    }

    pub fn aberta(&self) -> bool {
        self.0.aberta.get()
    }

    /// Abre com o foco no menu, para o teclado; o foco de antes volta no Esc.
    pub fn abrir(&self, window: &mut Window, cx: &mut App) {
        self.0.aberta.set(true);
        self.0.destacada.set(None);
        *self.0.anterior.borrow_mut() = window.focused(cx);
        window.focus(&self.0.foco, cx);
    }

    pub fn fechar(&self) {
        self.0.aberta.set(false);
        self.0.destacada.set(None);
    }

    fn fechar_devolvendo_foco(&self, window: &mut Window, cx: &mut App) {
        self.fechar();
        if let Some(f) = self.0.anterior.borrow_mut().take() {
            window.focus(&f, cx);
        }
    }
}

type Acao = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct ItemLista {
    chave: SharedString,
    icone: AnyElement,
    rotulo: AnyElement,
    estado: Option<SharedString>,
    modificado: bool,
    ao_escolher: Acao,
    ao_fechar: Option<Acao>,
}

impl ItemLista {
    pub fn new(
        chave: impl Into<SharedString>, icone: impl IntoElement, rotulo: impl IntoElement,
        ao_escolher: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        ItemLista {
            chave: chave.into(),
            icone: icone.into_any_element(),
            rotulo: rotulo.into_any_element(),
            estado: None,
            modificado: false,
            ao_escolher: Rc::new(ao_escolher),
            ao_fechar: None,
        }
    }

    /// Texto à direita (o estado do executor).
    pub fn estado(mut self, estado: impl Into<SharedString>) -> Self {
        self.estado = Some(estado.into());
        self
    }

    /// Alteração não salva: mostra o ● e, ao fechar, o menu fecha antes (fechar pede confirmação).
    pub fn modificado(mut self, modificado: bool) -> Self {
        self.modificado = modificado;
        self
    }

    pub fn ao_fechar(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.ao_fechar = Some(Rc::new(f));
        self
    }
}

/// O gatilho padrão: o ícone de lista com a contagem.
pub fn botao_lista(id: impl Into<ElementId>, n: usize) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .flex_none()
        .self_center()
        .h(px(24.))
        .px(px(7.))
        .rounded(tema::RAIO_SM)
        .text_size(tema::TEXTO_BODY)
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|s| s.bg(cor::fill_hover()))
        .child(icone("icons/list.svg", 14., cor::text_primary()))
        .child(n.to_string())
}

/// O gatilho e o menu. O contêiner (estilize como um `div`) é `relative flex flex-none`.
#[derive(IntoElement)]
pub struct ListaAbas {
    id: ElementId,
    estado: EstadoLista,
    base: Div,
    itens: Vec<ItemLista>,
    ativa: Option<SharedString>,
    direita: bool,
    gatilho: Option<Stateful<Div>>,
}

impl ListaAbas {
    /// `id` é o do gatilho padrão.
    pub fn new(id: impl Into<ElementId>, estado: &EstadoLista, itens: Vec<ItemLista>) -> Self {
        ListaAbas {
            id: id.into(),
            estado: estado.clone(),
            base: div().relative().flex().flex_none(),
            itens,
            ativa: None,
            direita: true,
            gatilho: None,
        }
    }

    pub fn ativa(mut self, chave: impl Into<SharedString>) -> Self {
        self.ativa = Some(chave.into());
        self
    }

    /// Alinha o menu à esquerda do gatilho (`bottomLeft`); o padrão é à direita (`bottomRight`).
    pub fn a_esquerda(mut self) -> Self {
        self.direita = false;
        self
    }

    /// Gatilho próprio no lugar do botão com a contagem; a lista liga o clique.
    pub fn gatilho(mut self, gatilho: Stateful<Div>) -> Self {
        self.gatilho = Some(gatilho);
        self
    }
}

impl Styled for ListaAbas {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

fn escolher(estado: &EstadoLista, f: &Acao, window: &mut Window, cx: &mut App) {
    estado.fechar();
    f(window, cx);
}

fn fechar_item(estado: &EstadoLista, item: (&Acao, bool), vista: EntityId, window: &mut Window, cx: &mut App) {
    if item.1 {
        estado.fechar();
    }
    (item.0)(window, cx);
    cx.notify(vista);
}

impl RenderOnce for ListaAbas {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let vista = window.current_view();
        let estado = self.estado;
        let n = self.itens.len();
        // A lista esvaziou (fechou o último terminal): o menu some na hora.
        if n == 0 {
            estado.fechar();
        }
        let e = estado.clone();
        let gatilho = self.gatilho.unwrap_or_else(|| botao_lista(self.id, n)).on_click(move |_, window, cx| {
            if e.aberta() {
                e.fechar();
            } else if n > 0 {
                e.abrir(window, cx);
            }
            cx.notify(vista);
        });
        let gatilho = estado.0.gatilho.marcar(gatilho).relative().test_support();
        if !estado.aberta() {
            return self.base.child(gatilho);
        }

        let destacada = estado.0.destacada.get();
        let acoes: Rc<Vec<(Acao, Option<Acao>, bool)>> =
            Rc::new(self.itens.iter().map(|i| (i.ao_escolher.clone(), i.ao_fechar.clone(), i.modificado)).collect());
        let (e, a) = (estado.clone(), acoes.clone());
        let mut menu = div()
            .id("lista-de-abas")
            .track_focus(&estado.0.foco)
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(220.))
            // O `sideOffset` de 6 px do DropdownMenu do Lobe UI, a borda de 1 px e o anel claro por fora.
            .mt(px(6.))
            .p(px(4.))
            .rounded(tema::RAIO_MD)
            .bg(cor::raised_black())
            .border_1()
            .border_color(cor::hairline())
.shadow(vec![gpui_kit::BoxShadow::new(px(0.), px(0.), gpui_kit::hsla(0., 0., 1., 0.18)).spread_radius(px(1.))])
            .on_key_down(move |ev: &KeyDownEvent, window, cx| {
                let atual = e.0.destacada.get();
                let novo = match ev.keystroke.key.as_str() {
                    "down" => Some(atual.map_or(0, |i| (i + 1) % n)),
                    "up" => Some(atual.map_or(n - 1, |i| (i + n - 1) % n)),
                    "home" => Some(0),
                    "end" => Some(n - 1),
                    "enter" | "space" => {
                        if let Some(i) = atual {
                            escolher(&e, &a[i].0, window, cx);
                        }
                        None
                    }
                    "delete" => {
                        if let Some((Some(f), m)) = atual.map(|i| (a[i].1.clone(), a[i].2)) {
                            fechar_item(&e, (&f, m), vista, window, cx);
                        }
                        None
                    }
                    "escape" => {
                        e.fechar_devolvendo_foco(window, cx);
                        None
                    }
                    _ => return,
                };
                cx.stop_propagation();
                if novo.is_some() {
                    e.0.destacada.set(novo);
                }
                cx.notify(vista);
            })
            .on_mouse_down_out({
                let e = estado.clone();
                move |_, _, cx| {
                    if !e.0.gatilho.sobre() {
                        e.fechar();
                        cx.notify(vista);
                    }
                }
            });
        for (i, item) in self.itens.into_iter().enumerate() {
            let marcado = self.ativa.as_ref() == Some(&item.chave);
            let realce = destacada == Some(i);
            let fechavel = item.ao_fechar.is_some();
            let (e1, e2, e3) = (estado.clone(), estado.clone(), estado.clone());
            let escolha = item.ao_escolher.clone();
            let fechar = item.ao_fechar.clone();
            let modificado = item.modificado;
            menu = menu.child(
                div()
                    .id(SharedString::from(format!("lista-{}", item.chave)))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(tema::ITEM_MENU)
                    .pl(px(8.))
                    .pr(px(30.))
                    .rounded(tema::RAIO_MENU)
                    .text_size(tema::TEXTO_MENU)
                    .cursor_pointer()
                    .when(realce, |d| d.bg(cor::fill_hover()))
                    .on_hover(move |dentro, _, cx| {
                        if *dentro && e1.0.destacada.get() != Some(i) {
                            e1.0.destacada.set(Some(i));
                            cx.notify(vista);
                        }
                    })
                    .on_click(move |_, window, cx| escolher(&e2, &escolha, window, cx))
                    .child(item.icone)
                    .child(div().max_w(px(288.)).truncate().child(item.rotulo))
                    .when(item.estado.is_some() || modificado, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .ml_auto()
                                .pl(px(12.))
                                .text_color(cor::text_secondary())
                                .children(item.estado)
                                .when(modificado, |d| d.child("●")),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .right(px(5.))
                            .top(px(5.))
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(marcado && !(fechavel && realce), |d| {
                                d.child(icone("icons/check.svg", 14., cor::text_secondary()))
                            })
                            .when_some(fechar.filter(|_| realce), |d, f| {
                                d.child(
                                    div()
                                        .id(SharedString::from(format!("lista-fechar-{}", item.chave)))
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(2.))
                                        .hover(|s| s.bg(cor::fill_hover()))
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            fechar_item(&e3, (&f, modificado), vista, window, cx);
                                        })
                                        .child(icone("icons/x.svg", 14., cor::text_secondary())),
                                )
                            }),
                    ),
            );
        }
        // Abre logo abaixo do gatilho, alinhado à direita dele (`bottomRight`) ou à esquerda (`bottomLeft`).
        let canto = if self.direita { gpui_kit::Anchor::TopRight } else { gpui_kit::Anchor::TopLeft };
        self.base.child(gatilho.child(
            div()
                .absolute()
                .bottom_0()
                .when(self.direita, |d| d.right_0())
                .when(!self.direita, |d| d.left_0())
                .size_0()
                .child(deferred(anchored().anchor(canto).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(1)),
        ))
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use gpui_kit::{Context, Modifiers, Render, TestAppContext};

    struct Vista(EstadoLista);

    impl Render for Vista {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let item = ItemLista::new("a", div(), "a", |_, _| {});
            div().size_full().child(
                ListaAbas::new("lista", &self.0, vec![item]).gatilho(botao_lista("g", 1).debug_selector(|| "gatilho".into())),
            )
        }
    }

    #[gpui_kit::test]
    fn clicar_no_gatilho_alterna_o_menu(cx: &mut TestAppContext) {
        let (vista, cx) = cx.add_window_view(|_, cx| Vista(EstadoLista::new(cx)));
        cx.run_until_parked();
        let centro = cx.debug_bounds("gatilho").expect("gatilho").center();
        let aberta = |cx: &mut gpui_kit::VisualTestContext| vista.read_with(cx, |v, _| v.0.aberta());
        cx.simulate_mouse_move(centro, None, Modifiers::none());
        cx.simulate_click(centro, Modifiers::none());
        assert!(aberta(cx), "o clique no gatilho abre");
        cx.simulate_click(centro, Modifiers::none());
        assert!(!aberta(cx), "o segundo clique no gatilho fecha e o menu segue fechado");
        cx.simulate_click(centro, Modifiers::none());
        assert!(aberta(cx));
        let fora = centro + gpui_kit::point(px(0.), px(300.));
        cx.simulate_mouse_move(fora, None, Modifiers::none());
        cx.simulate_click(fora, Modifiers::none());
        assert!(!aberta(cx), "o clique fora fecha");
    }
}
