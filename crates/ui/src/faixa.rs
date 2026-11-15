//! Faixa de abas com rolagem horizontal (porte de `src/shared/tab-strip.ts`), comum às abas de projeto, de shell, de
//! executores e do editor. Quem chama desenha cada aba; a faixa cuida do arrasto (começa depois de 4 px, as vizinhas abrem
//! espaço, rola sozinha na borda, Esc ou a janela perdendo o foco cancelam), da roda do mouse e de revelar a aba ativa.
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::{
    canvas, deferred, div, point, prelude::*, px, AnyElement, App, Context, Div, DispatchPhase, ElementId, EntityId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ScrollHandle, SharedString, StyleRefinement, Subscription, Window,
};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// Arrasto só começa depois deste deslocamento, para o clique seguir só selecionando (`DRAG_START`).
pub const ARRASTO_INICIO: f32 = 4.;
/// A menos disto da borda da faixa, ela rola sozinha 8 px por quadro (`EDGE`).
pub const BORDA: f32 = 24.;

/// Aba segurada, com as posições de antes do arrasto (coordenadas do conteúdo da faixa), como `trackTabDrag`.
/// `lo..=hi` limita onde ela pode entrar (as fixadas andam só entre as fixadas).
#[derive(Clone, Debug)]
pub struct ArrastoAba {
    pub de: usize,
    pub slots: Vec<(f32, f32)>,
    pub inicio_x: f32,
    pub x: f32,
    pub rolagem_inicio: f32,
    pub movendo: bool,
    pub dx: f32,
    pub alvo: usize,
    pub lo: usize,
    pub hi: usize,
}

impl ArrastoAba {
    pub fn new(de: usize, slots: Vec<(f32, f32)>, x: f32, rolagem: f32) -> Self {
        let hi = slots.len().saturating_sub(1);
        ArrastoAba { de, slots, inicio_x: x, x, rolagem_inicio: rolagem, movendo: false, dx: 0., alvo: de, lo: 0, hi }
    }

    pub fn com_intervalo(mut self, lo: usize, hi: usize) -> Self {
        self.lo = lo.min(self.de);
        self.hi = hi.max(self.de).min(self.slots.len() - 1);
        self
    }

    /// A rolagem conta no deslocamento; a vizinha é passada quando a borda da frente cruza o meio dela.
    pub fn atualizar(&mut self, rolagem: f32) {
        let (esq, larg) = self.slots[self.de];
        let (ult_esq, ult_larg) = self.slots[self.hi];
        let min = self.slots[self.lo].0 - esq;
        let max = (ult_esq + ult_larg - esq - larg).max(min);
        self.dx = (self.x - self.inicio_x + rolagem - self.rolagem_inicio).clamp(min, max);
        let esq = esq + self.dx;
        let (de, lo, hi) = (self.de, self.lo, self.hi);
        self.alvo = lo
            + self
                .slots
                .iter()
                .enumerate()
                .filter(|&(i, &(l, w))| i != de && i >= lo && i <= hi && l + w / 2. < if i < de { esq } else { esq + larg })
                .count();
    }

    /// `dragShift`: a arrastada segue `dx`; as vizinhas entre a origem e o destino abrem o espaço dela.
    pub fn deslocamento(&self, i: usize) -> f32 {
        let larg = self.slots[self.de].1;
        if i == self.de {
            self.dx
        } else if self.de < i && i <= self.alvo {
            -larg
        } else if self.alvo <= i && i < self.de {
            larg
        } else {
            0.
        }
    }
}

/// A aba solta noutro lugar: `para` é o índice final dela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoverAba {
    pub de: usize,
    pub para: usize,
}

/// Resultado de um passo do ponteiro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passo {
    Nada,
    /// Passou dos 4 px: o arrasto começou agora.
    Iniciou,
    Moveu,
}

#[derive(Default)]
struct Interno {
    rolagem: ScrollHandle,
    arrasto: RefCell<Option<ArrastoAba>>,
    /// Aba ativa e quantidade da última revelação; mudou, rola até a ativa (`revealTab`).
    revelada: RefCell<Option<(SharedString, usize)>>,
    esc: RefCell<Option<Subscription>>,
}

/// Estado de tela de uma faixa; guarde um por faixa na entidade dona. Clonar compartilha o mesmo estado.
#[derive(Clone, Default)]
pub struct EstadoFaixa(Rc<Interno>);

impl EstadoFaixa {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rolagem(&self) -> &ScrollHandle {
        &self.0.rolagem
    }

    pub fn arrasto(&self) -> Option<ArrastoAba> {
        self.0.arrasto.borrow().clone()
    }

    /// Rolagem atual, em px a partir do começo (o `scrollLeft`).
    pub fn rolagem_x(&self) -> f32 {
        -f32::from(self.0.rolagem.offset().x)
    }

    /// Segura a aba `indice` de `n`: guarda as posições de todas como estavam no último quadro. As `fixadas` primeiras só
    /// trocam entre si, e as demais não entram entre elas.
    pub fn pressionar(&self, indice: usize, n: usize, fixadas: usize, x: f32) {
        let r = &self.0.rolagem;
        let origem = f32::from(r.bounds().origin.x);
        let rolagem = self.rolagem_x();
        let slots: Option<Vec<(f32, f32)>> = (0..n)
            .map(|i| r.bounds_for_item(i).map(|b| (f32::from(b.origin.x) - origem + rolagem, f32::from(b.size.width))))
            .collect();
        let (lo, hi) = if indice < fixadas { (0, fixadas.saturating_sub(1)) } else { (fixadas, n.saturating_sub(1)) };
        *self.0.arrasto.borrow_mut() =
            slots.filter(|s| indice < s.len()).map(|s| ArrastoAba::new(indice, s, x, rolagem).com_intervalo(lo, hi));
    }

    pub fn mover(&self, x: f32) -> Passo {
        let rolagem = self.rolagem_x();
        let mut arrasto = self.0.arrasto.borrow_mut();
        let Some(a) = arrasto.as_mut() else { return Passo::Nada };
        a.x = x;
        let mut passo = Passo::Moveu;
        if !a.movendo {
            if (x - a.inicio_x).abs() < ARRASTO_INICIO {
                return Passo::Nada;
            }
            a.movendo = true;
            passo = Passo::Iniciou;
        }
        a.atualizar(rolagem);
        passo
    }

    /// Solta a aba: `confirmar` falso (Esc, janela sem foco) devolve à posição original. Devolve se havia arrasto em
    /// movimento (a faixa redesenha) e a troca de lugar, se houve.
    pub fn soltar(&self, confirmar: bool) -> (bool, Option<MoverAba>) {
        let arrasto = self.0.arrasto.borrow_mut().take();
        // A assinatura do Esc pode estar rodando agora; some depois.
        let esc = self.0.esc.borrow_mut().take();
        drop(esc);
        match arrasto {
            Some(a) if a.movendo => (true, (confirmar && a.alvo != a.de).then_some(MoverAba { de: a.de, para: a.alvo })),
            _ => (false, None),
        }
    }

    /// Rola até a aba `indice` quando a ativa (`chave`) ou a quantidade de abas mudou desde a última vez.
    pub fn revelar(&self, chave: Option<(SharedString, usize)>, indice: Option<usize>) {
        if *self.0.revelada.borrow() != chave {
            if let Some(i) = indice {
                self.0.rolagem.scroll_to_item(i);
            }
            *self.0.revelada.borrow_mut() = chave;
        }
    }

    /// Durante o arrasto, perto da borda, rola 8 px e pede outro quadro.
    fn rolar_na_borda(&self, window: &mut Window) {
        let Some(x) = self.arrasto().filter(|a| a.movendo).map(|a| a.x) else { return };
        let r = &self.0.rolagem;
        let caixa = r.bounds();
        let passo = if x < f32::from(caixa.left()) + BORDA {
            -8.
        } else if x > f32::from(caixa.right()) - BORDA {
            8.
        } else {
            return;
        };
        let max = f32::from(r.max_offset().x);
        let antes = self.rolagem_x();
        let nova = (antes + passo).clamp(0., max.max(0.));
        if nova != antes {
            r.set_offset(point(px(-nova), r.offset().y));
            if let Some(a) = self.0.arrasto.borrow_mut().as_mut() {
                a.atualizar(nova);
            }
        }
        window.request_animation_frame();
    }

    /// Esc em captura, antes do terminal ou do editor, cancela o arrasto em movimento.
    fn escutar_esc(&self, vista: EntityId, cx: &mut App) {
        let fraco: Weak<Interno> = Rc::downgrade(&self.0);
        let sub = cx.intercept_keystrokes(move |e, _, cx| {
            if e.keystroke.key != "escape" {
                return;
            }
            let Some(interno) = fraco.upgrade() else { return };
            let estado = EstadoFaixa(interno);
            if estado.arrasto().is_some_and(|a| a.movendo) {
                cx.stop_propagation();
                // Soltar derruba esta assinatura, então fica para depois do despacho.
                cx.defer(move |cx| {
                    estado.soltar(false);
                    cx.notify(vista);
                });
            }
        });
        *self.0.esc.borrow_mut() = Some(sub);
    }
}

type Pressionar = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;
type Menu = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// O que a aba precisa ser para a faixa ajustá-la: um `div` com id, inclusive o observado dos testes.
pub trait ElementoAba: IntoElement + Styled + InteractiveElement + ParentElement + 'static {}
impl<T: IntoElement + Styled + InteractiveElement + ParentElement + 'static> ElementoAba for T {}

/// Ajustes da faixa sobre o elemento da aba, aplicados no desenho.
struct Ajuste {
    deslocamento: Option<f32>,
    estilo: Option<StyleRefinement>,
    italico: bool,
    pressionar: Option<Pressionar>,
    menu: Option<Menu>,
}

/// Uma aba: o elemento desenhado por quem chama (com o clique, o X e a dica) e o que a faixa precisa saber dele.
pub struct Aba {
    chave: SharedString,
    montar: Box<dyn FnOnce(Ajuste) -> AnyElement>,
    fixada: bool,
    previa: bool,
    menu: Option<Menu>,
}

impl Aba {
    pub fn new<E: ElementoAba>(chave: impl Into<SharedString>, elemento: E) -> Self {
        let montar = Box::new(move |a: Ajuste| {
            let mut el = elemento.relative();
            if let Some(dx) = a.deslocamento {
                el = el.left(px(dx));
            }
            if let Some(estilo) = &a.estilo {
                el.style().refine(estilo);
            }
            if a.italico {
                el = el.italic();
            }
            if let Some(f) = a.pressionar {
                el = el.on_mouse_down(MouseButton::Left, f);
            }
            match a.menu {
                Some(f) => el.context_menu(move |m, w, cx| f(m, w, cx)).into_any_element(),
                None => el.into_any_element(),
            }
        });
        Aba { chave: chave.into(), montar, fixada: false, previa: false, menu: None }
    }

    /// Fixada: fica no começo e só troca de lugar com as outras fixadas.
    pub fn fixada(mut self, fixada: bool) -> Self {
        self.fixada = fixada;
        self
    }

    /// Prévia (a aba que a próxima abertura substitui): o texto em itálico.
    pub fn previa(mut self, previa: bool) -> Self {
        self.previa = previa;
        self
    }

    /// Menu de contexto no clique direito.
    pub fn menu_contexto(mut self, f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static) -> Self {
        self.menu = Some(Rc::new(f));
        self
    }
}

type Ouvinte<E> = Rc<dyn Fn(&E, &mut Window, &mut App)>;

/// A faixa: um contêiner rolável (estilize como um `div`) com as abas e, depois delas, os `extra`.
#[derive(IntoElement)]
pub struct FaixaAbas {
    id: ElementId,
    estado: EstadoFaixa,
    base: Div,
    abas: Vec<Aba>,
    ativa: Option<SharedString>,
    extras: Vec<AnyElement>,
    ao_mover: Option<Ouvinte<MoverAba>>,
    ao_iniciar: Option<Ouvinte<usize>>,
    estilo_arrastada: Option<StyleRefinement>,
}

impl FaixaAbas {
    pub fn new(id: impl Into<ElementId>, estado: &EstadoFaixa) -> Self {
        FaixaAbas {
            id: id.into(),
            estado: estado.clone(),
            base: div(),
            abas: Vec::new(),
            ativa: None,
            extras: Vec::new(),
            ao_mover: None,
            ao_iniciar: None,
            estilo_arrastada: None,
        }
    }

    pub fn aba(mut self, aba: Aba) -> Self {
        self.abas.push(aba);
        self
    }

    pub fn abas(mut self, abas: impl IntoIterator<Item = Aba>) -> Self {
        self.abas.extend(abas);
        self
    }

    /// A aba ativa, revelada quando muda.
    pub fn ativa(mut self, chave: Option<impl Into<SharedString>>) -> Self {
        self.ativa = chave.map(Into::into);
        self
    }

    /// Elemento depois das abas, dentro da rolagem (o "+" da titlebar).
    pub fn extra(mut self, el: impl IntoElement) -> Self {
        self.extras.push(el.into_any_element());
        self
    }

    /// Liga o arrasto; sem isto as abas não se movem.
    pub fn ao_mover(mut self, f: impl Fn(&MoverAba, &mut Window, &mut App) + 'static) -> Self {
        self.ao_mover = Some(Rc::new(f));
        self
    }

    /// Chamado com o índice quando o arrasto passa dos 4 px (o `onStart`).
    pub fn ao_iniciar(mut self, f: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.ao_iniciar = Some(Rc::new(f));
        self
    }

    /// Estilo extra da aba em arrasto (ela já pinta por cima das vizinhas), como `|s| s.opacity(0.8)`.
    pub fn estilo_arrastada(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.estilo_arrastada = Some(f(StyleRefinement::default()));
        self
    }
}

impl Styled for FaixaAbas {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for FaixaAbas {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let estado = self.estado;
        let vista = window.current_view();
        if estado.arrasto().is_some() && !window.is_window_active() {
            estado.soltar(false);
        }
        estado.rolar_na_borda(window);
        let n = self.abas.len();
        let indice_ativa = self.ativa.as_ref().and_then(|c| self.abas.iter().position(|a| &a.chave == c));
        estado.revelar(self.ativa.clone().map(|c| (c, n)), indice_ativa);

        let fixadas = self.abas.iter().filter(|a| a.fixada).count();
        let arrasto = estado.arrasto().filter(|a| a.movendo && a.slots.len() == n);
        let arrastavel = self.ao_mover.is_some();
        let mut faixa = self.base.id(self.id).overflow_x_scroll().track_scroll(estado.rolagem());
        for (i, aba) in self.abas.into_iter().enumerate() {
            let segurada = arrasto.as_ref().is_some_and(|a| a.de == i);
            let pressionar = arrastavel.then(|| {
                let e = estado.clone();
                Box::new(move |ev: &MouseDownEvent, _: &mut Window, _: &mut App| {
                    e.pressionar(i, n, fixadas, f32::from(ev.position.x))
                }) as Pressionar
            });
            let el = (aba.montar)(Ajuste {
                deslocamento: arrasto.as_ref().map(|a| a.deslocamento(i)),
                estilo: self.estilo_arrastada.clone().filter(|_| segurada),
                italico: aba.previa,
                pressionar,
                menu: aba.menu,
            });
            // A arrastada pinta por cima das vizinhas (o `z-10` do React).
            faixa = if segurada { faixa.child(deferred(el)) } else { faixa.child(el) };
        }
        faixa = faixa.children(self.extras);
        if estado.arrasto().is_none() {
            return faixa;
        }
        // Enquanto há aba segurada, o ponteiro é acompanhado na janela inteira.
        let (ao_mover, ao_iniciar) = (self.ao_mover, self.ao_iniciar);
        faixa.child(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    let e = estado.clone();
                    let iniciar = ao_iniciar.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, fase, window, cx| {
                        if fase != DispatchPhase::Bubble {
                            return;
                        }
                        match e.mover(f32::from(ev.position.x)) {
                            Passo::Nada => {}
                            Passo::Iniciou => {
                                e.escutar_esc(vista, cx);
                                if let (Some(f), Some(a)) = (&iniciar, e.arrasto()) {
                                    f(&a.de, window, cx);
                                }
                                cx.notify(vista);
                            }
                            Passo::Moveu => cx.notify(vista),
                        }
                    });
                    let e = estado.clone();
                    let mover = ao_mover.clone();
                    window.on_mouse_event(move |ev: &MouseUpEvent, fase, window, cx| {
                        if fase != DispatchPhase::Bubble || ev.button != MouseButton::Left {
                            return;
                        }
                        let (movia, troca) = e.soltar(true);
                        if let (Some(f), Some(t)) = (&mover, troca) {
                            f(&t, window, cx);
                        }
                        if movia {
                            cx.notify(vista);
                        }
                    });
                },
            )
            .absolute()
            .size_0(),
        )
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn arrasto_de_aba_calcula_destino_e_deslocamentos() {
        // Três abas de 100 px com 4 px de vão.
        let slots = vec![(4., 100.), (108., 100.), (212., 100.)];
        let mut a = ArrastoAba::new(0, slots.clone(), 50., 0.);
        a.x = 50. + 60.;
        a.atualizar(0.);
        assert_eq!(a.alvo, 1, "a borda da frente passou do meio da segunda");
        assert_eq!(a.deslocamento(1), -100.);
        assert_eq!(a.deslocamento(2), 0.);
        a.x = 50. + 1000.;
        a.atualizar(0.);
        assert_eq!(a.dx, 208., "travada no fim da faixa");
        assert_eq!(a.alvo, 2);

        let mut b = ArrastoAba::new(2, slots.clone(), 300., 0.);
        b.x = 300. - 1000.;
        b.atualizar(0.);
        assert_eq!(b.alvo, 0);
        assert_eq!(b.deslocamento(0), 100.);
        // A rolagem da faixa conta como deslocamento.
        let mut c = ArrastoAba::new(0, vec![(4., 100.), (108., 100.)], 50., 0.);
        c.atualizar(60.);
        assert_eq!(c.alvo, 1);

        // Com a primeira fixada, a terceira não passa da segunda.
        let mut d = ArrastoAba::new(2, slots, 300., 0.).com_intervalo(1, 2);
        d.x = 300. - 1000.;
        d.atualizar(0.);
        assert_eq!(d.dx, -104., "travada no começo das não fixadas");
        assert_eq!(d.alvo, 1);
        assert_eq!(d.deslocamento(0), 0.);
    }

    #[test]
    fn soltar_sem_mover_e_clique() {
        let e = EstadoFaixa::new();
        *e.0.arrasto.borrow_mut() = Some(ArrastoAba::new(0, vec![(0., 100.), (104., 100.)], 10., 0.));
        assert_eq!(e.mover(12.), Passo::Nada, "abaixo de 4 px ainda é clique");
        assert_eq!(e.soltar(true), (false, None));
        *e.0.arrasto.borrow_mut() = Some(ArrastoAba::new(0, vec![(0., 100.), (104., 100.)], 10., 0.));
        assert_eq!(e.mover(200.), Passo::Iniciou);
        assert_eq!(e.mover(210.), Passo::Moveu);
        assert_eq!(e.soltar(true), (true, Some(MoverAba { de: 0, para: 1 })));
        *e.0.arrasto.borrow_mut() = Some(ArrastoAba::new(0, vec![(0., 100.), (104., 100.)], 10., 0.));
        e.mover(200.);
        assert_eq!(e.soltar(false), (true, None), "Esc devolve");
    }
}
