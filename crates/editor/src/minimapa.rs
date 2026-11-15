//! Minimapa à direita do editor, como o `@replit/codemirror-minimap` com `displayText: 'characters'` e
//! `showOverlay: 'mouse-over'`: cada caractere vira um traço na cor da sintaxe, a janela visível só aparece com o
//! ponteiro em cima, e clicar ou arrastar rola o editor. A largura acompanha a do editor, como nas capturas.
use crate::realce;
use gpui::{canvas, div, fill, point, prelude::*, px, size, App, Bounds, Entity, Hsla, MouseButton, Pixels, Window};
use gpui_kit::component::input::{EditorState, RopeExt};
use std::cell::Cell;
use std::rc::Rc;

/// Altura de uma linha no minimapa e largura de um caractere.
const LINHA: f32 = 3.;
const CARACTERE: f32 = 1.;

#[derive(IntoElement)]
pub struct Minimapa {
    editor: Entity<EditorState>,
}

impl Minimapa {
    pub fn new(editor: Entity<EditorState>) -> Self {
        Minimapa { editor }
    }
}

/// Deslocamento (px) do conteúdo do minimapa quando o documento é mais alto que a área.
fn deslocamento(linhas: usize, altura: f32, fracao: f32) -> f32 {
    let total = linhas as f32 * LINHA;
    if total <= altura { 0. } else { (total - altura) * fracao.clamp(0., 1.) }
}

/// Medidas da rolagem do editor: linhas visíveis, máximo de linhas roladas e a linha do topo.
fn medidas(s: &EditorState) -> Option<(f32, f32, f32, usize)> {
    let lh = f32::from(s.line_height()?);
    let linhas = s.text().lines_len();
    let visiveis = (f32::from(s.input_bounds().size.height) / lh).max(1.);
    let max = (linhas as f32 - visiveis).max(0.);
    Some((lh, visiveis, max, linhas))
}

fn rolar_para(editor: &Entity<EditorState>, y: f32, b: Bounds<Pixels>, cx: &mut App) {
    editor.update(cx, |s, cx| {
        let Some((lh, visiveis, max, linhas)) = medidas(s) else { return };
        let topo = -f32::from(s.scroll_offset().y) / lh;
        let fracao = if max > 0. { topo / max } else { 0. };
        let desl = deslocamento(linhas, f32::from(b.size.height), fracao);
        let linha = ((y - f32::from(b.origin.y) + desl) / LINHA - visiveis / 2.).clamp(0., max);
        let o = s.scroll_offset();
        s.set_scroll_offset(point(o.x, px(-(lh * linha))), cx);
    });
}

impl RenderOnce for Minimapa {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let s = self.editor.read(cx);
        let n = s.text().len();
        if n > realce::LIMITE_COR && crate::armazem::tamanho_js(&s.text().to_string()) > realce::LIMITE_COR {
            return div().into_any_element();
        }
        let largura = (f32::from(s.input_bounds().size.width) / 5.).clamp(0., 120.);
        let id = self.editor.entity_id();
        let sobre = window.use_keyed_state(("minimapa-sobre", id), cx, |_, _| Rc::new(Cell::new(false))).read(cx).clone();
        let arrasto = window.use_keyed_state(("minimapa-arrasto", id), cx, |_, _| Rc::new(Cell::new(false))).read(cx).clone();
        let caixa: Rc<Cell<Bounds<Pixels>>> = Rc::default();
        let (c1, c2, c3) = (caixa.clone(), caixa.clone(), caixa);
        let (e1, e2, e3) = (self.editor.clone(), self.editor.clone(), self.editor.clone());
        let (a1, a2, a3) = (arrasto.clone(), arrasto.clone(), arrasto);
        let s1 = sobre.clone();
        div()
            .id(("minimapa", id))
            .flex_none()
            .w(px(largura))
            .h_full()
            .on_hover(move |dentro, window, _| {
                s1.set(*dentro);
                window.refresh();
            })
            .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
                a1.set(true);
                rolar_para(&e1, f32::from(ev.position.y), c1.get(), cx);
            })
            .on_mouse_move(move |ev, _, cx| {
                if a2.get() && ev.pressed_button == Some(MouseButton::Left) {
                    rolar_para(&e2, f32::from(ev.position.y), c2.get(), cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| a3.set(false))
            .child(canvas(move |b, _, _| c3.set(b), move |b, _, window, cx| pintar(&e3, b, sobre.get(), window, cx)).size_full())
            .into_any_element()
    }
}

fn pintar(editor: &Entity<EditorState>, b: Bounds<Pixels>, sobre: bool, window: &mut Window, cx: &mut App) {
    let s = editor.read(cx);
    let Some((lh, visiveis, max, linhas)) = medidas(s) else { return };
    let texto = s.text().clone();
    let alt = f32::from(b.size.height);
    let topo = -f32::from(s.scroll_offset().y) / lh;
    let fracao = if max > 0. { topo / max } else { 0. };
    let desl = deslocamento(linhas, alt, fracao);
    let primeira = (desl / LINHA).floor() as usize;
    let ultima = (((desl + alt) / LINHA).ceil() as usize).min(linhas);
    let padrao: Hsla = gpui::rgb(0xcccccc).into();
    let maximo = (f32::from(b.size.width) / CARACTERE) as usize;
    for l in primeira..ultima {
        let linha = texto.slice_line(l).to_string();
        let y = f32::from(b.origin.y) + l as f32 * LINHA - desl;
        let mut col = 0usize;
        let mut corrida: Option<(usize, usize, Hsla)> = None;
        let fechar = |c: Option<(usize, usize, Hsla)>, window: &mut Window| {
            if let Some((x0, n, cor)) = c {
                let r = Bounds::new(point(b.origin.x + px(x0 as f32 * CARACTERE), px(y)), size(px(n as f32 * CARACTERE), px(LINHA - 1.)));
                window.paint_quad(fill(r, cor.opacity(0.6)));
            }
        };
        for ch in linha.chars() {
            if col >= maximo || ch == '\n' || ch == '\r' {
                break;
            }
            let passo = if ch == '\t' { 4 } else { 1 };
            if ch.is_whitespace() {
                fechar(corrida.take(), window);
                col += passo;
                continue;
            }
            // O minimapa do React lê só a árvore do CodeMirror, sem as cores do Shiki: desenha tudo na cor do texto.
            let cor = padrao;
            match &mut corrida {
                Some((_, n, c)) if *c == cor => *n += 1,
                _ => {
                    fechar(corrida.take(), window);
                    corrida = Some((col, 1, cor));
                }
            }
            col += passo;
        }
        fechar(corrida.take(), window);
    }
    if sobre {
        let y = f32::from(b.origin.y) + topo * LINHA - desl;
        let r = Bounds::new(point(b.origin.x, px(y)), size(b.size.width, px(visiveis * LINHA)));
        window.paint_quad(fill(r, gpui::rgba(0x79797933)));
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn deslocamento_do_minimapa() {
        assert_eq!(deslocamento(10, 100., 0.5), 0.);
        assert_eq!(deslocamento(100, 150., 1.), 150.);
        assert_eq!(deslocamento(100, 150., 0.5), 75.);
    }
}
