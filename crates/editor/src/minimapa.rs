//! Minimapa à direita do editor: o texto em caracteres, a sobreposição ao passar o mouse e uma faixa de git. Desenha
//! num canvas com 2 pixels por pixel e o
//! conteúdo reduzido a 1/4: em pixels da tela, a linha tem 1/8 da do editor (19 → 2,375 px), e o texto é escrito na
//! fonte do editor com esse tamanho, na cor do texto. A largura é 1/6 da do editor, até 120 px.
use crate::realce;
use gpui::{
    canvas, div, fill, point, prelude::*, px, size, App, Bounds, ContentMask, DispatchPhase, Entity, Hsla, MouseButton,
    MouseMoveEvent, MouseUpEvent, Pixels, SharedString, TextAlign, TextRun, Window,
};
use gpui_kit::component::input::{EditorState, RopeExt};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// `Scale.PixelMultiplier * Scale.SizeRatio`: pixels do editor por pixel do minimapa.
const ESCALA: f32 = 8.;
const LARGURA_MAXIMA: f32 = 120.;
/// Faixa de git: 2 px de folga do canvas (1 px na tela), `GUTTER_WIDTH` de 4 (2 px) e mais 2 (1 px) antes do texto.
const GIT_X: f32 = 1.;
const GIT_LARGURA: f32 = 2.;
const TEXTO_X: f32 = 4.;
/// Passadas do texto (ver `pintar`).
const PASSADAS: usize = 2;
/// Transição de opacidade da sobreposição (`transition: opacity 300ms`).
const TRANSICAO: Duration = Duration::from_millis(300);

#[derive(IntoElement)]
pub struct Minimapa {
    editor: Entity<EditorState>,
    /// `None` sem `gutters` (a tela de conflito): o texto começa na borda.
    git: Option<Vec<(usize, Hsla)>>,
}

impl Minimapa {
    pub fn new(editor: Entity<EditorState>) -> Self {
        Minimapa { editor, git: None }
    }

    /// Faixa de git (`gutters` do minimapa): linha (base 0) → cor. Mesmo vazia, recua o texto em 4 px.
    pub fn com_git(mut self, git: Vec<(usize, Hsla)>) -> Self {
        self.git = Some(git);
        self
    }
}

/// Rolagem do editor nas grandezas do `scrollDOM`: `scrollTop`, `clientHeight`, `scrollHeight` e a linha.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rolagem {
    topo: f32,
    cliente: f32,
    total: f32,
    lh: f32,
}

impl Rolagem {
    fn de(s: &EditorState) -> Option<Rolagem> {
        let lh = f32::from(s.line_height()?);
        let cliente = f32::from(s.input_bounds().size.height);
        let mapa = s.display_map();
        let conteudo = f32::from(mapa.display_row_top(mapa.display_row_count(), px(lh)));
        // Altura rolável do editor de código do Kit: o conteúdo mais meia tela (ou 3 linhas) em branco no fim.
        let total = (conteudo + (cliente / 2.).max(3. * lh)).max(cliente);
        Some(Rolagem { topo: -f32::from(s.scroll_offset().y), cliente, total, lh })
    }

    fn maximo(&self) -> f32 {
        (self.total - self.cliente).max(0.)
    }

    fn fracao(&self) -> f32 {
        let m = self.maximo();
        if m > 0. { self.topo / m } else { 0. }
    }

    /// Sem rolagem, a sobreposição some (`cm-minimap-overlay-off`) e o minimapa não responde ao ponteiro.
    fn rola(&self) -> bool {
        self.maximo() > 0.
    }

    /// Altura da sobreposição e o topo dela (`computeHeight` e `computeTop`).
    fn sobreposicao(&self) -> (f32, f32) {
        let h = self.cliente / ESCALA;
        let max_topo = self.cliente - h;
        (h, (max_topo * self.fracao()).min(self.topo / ESCALA))
    }

    /// `scrollTop` depois de um clique fora da sobreposição, `dy` px abaixo do topo do minimapa (`onMouseDown`).
    fn clique(&self, dy: f32) -> f32 {
        let faixa_visivel = self.cliente * ESCALA - self.cliente;
        let topo_visivel = faixa_visivel * self.fracao();
        (self.topo - topo_visivel).max(0.) + dy * ESCALA - self.cliente / 2.
    }

    /// Arrastar a sobreposição `dy` px a partir do topo `topo`: o novo `scrollTop` e o novo topo (`onMouseMove`).
    fn arrastar(&self, topo: f32, dy: f32) -> (f32, f32) {
        let max_sem_sobra = self.maximo() / ESCALA;
        let max_com_sobra = self.cliente - self.cliente / ESCALA;
        let mudanca = topo + dy;
        let rolagem = (self.maximo() * mudanca / max_com_sobra).max(mudanca * ESCALA);
        (rolagem, mudanca.max(0.).min(max_com_sobra.min(max_sem_sobra)))
    }
}

/// Primeira linha desenhada e quantas cabem (`canvasStartAndEndIndex`, sem padding no documento): as linhas ficam
/// presas à grade do canvas, sem deslocamento fracionário.
fn faixa_linhas(linhas: usize, linha: f32, altura: f32, fracao: f32) -> (usize, usize) {
    let total = linhas as f32 * linha;
    let topo = (fracao * (total - altura)).max(0.);
    let inicio = (topo / linha).round() as usize;
    (inicio, (altura / linha).round() as usize)
}

/// Opacidade animada (0 a 1) com a transição do CSS.
#[derive(Clone, Copy)]
struct Fade {
    de: f32,
    para: f32,
    desde: Instant,
}

impl Fade {
    fn valor(&self, agora: Instant) -> f32 {
        let t = (agora.saturating_duration_since(self.desde).as_secs_f32() / TRANSICAO.as_secs_f32()).min(1.);
        self.de + (self.para - self.de) * t
    }

    fn mirar(&mut self, para: f32, agora: Instant) {
        if self.para != para {
            *self = Fade { de: self.valor(agora), para, desde: agora };
        }
    }
}

struct Estado {
    fade: Fade,
    sobre_janela: bool,
    /// Arraste da sobreposição: o último y do ponteiro e o topo que ela tem enquanto isso.
    arrasto: Option<(f32, f32)>,
    caixa: Bounds<Pixels>,
}

fn rolar(editor: &Entity<EditorState>, topo: f32, cx: &mut App) {
    editor.update(cx, |s, cx| {
        let o = s.scroll_offset();
        s.set_scroll_offset(point(o.x, px(-topo.max(0.))), cx);
    });
}

impl RenderOnce for Minimapa {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let s = self.editor.read(cx);
        let n = s.text().len();
        if n > realce::LIMITE_COR && crate::armazem::tamanho_js(&s.text().to_string()) > realce::LIMITE_COR {
            return div().into_any_element();
        }
        let id = self.editor.entity_id();
        let estado = window
            .use_keyed_state(("minimapa", id), cx, |_, _| {
                Rc::new(RefCell::new(Estado {
                    fade: Fade { de: 0., para: 0., desde: Instant::now() },
                    sobre_janela: false,
                    arrasto: None,
                    caixa: Bounds::default(),
                }))
            })
            .read(cx)
            .clone();
        let (e1, e2) = (estado.clone(), estado.clone());
        let (ed1, ed2) = (self.editor.clone(), self.editor.clone());
        let git = self.git;
        div()
            .id(("minimapa", id))
            .flex_none()
            .w(gpui::relative(1. / 6.))
            .max_w(px(LARGURA_MAXIMA))
            // Opaco: o editor passa por baixo (o fundo das linhas novas do bloco do Git não aparece aqui).
            .bg(crate::documento::cor::fundo())
            .h_full()
            .on_hover(move |dentro, window, _| {
                let mut e = e1.borrow_mut();
                e.sobre_janela = *dentro;
                window.refresh();
            })
            .on_any_mouse_down(move |ev, window, cx| {
                if ev.button == MouseButton::Right {
                    return;
                }
                let Some(r) = Rolagem::de(ed1.read(cx)).filter(Rolagem::rola) else { return };
                let mut e = e2.borrow_mut();
                let dy = f32::from(ev.position.y - e.caixa.origin.y);
                let (h, topo) = r.sobreposicao();
                if dy >= topo && dy < topo + h {
                    e.arrasto = Some((f32::from(ev.position.y), topo));
                    window.refresh();
                    return;
                }
                drop(e);
                rolar(&ed1, r.clique(dy), cx);
            })
            .child(
                canvas(
                    move |b, _, _| {
                        estado.borrow_mut().caixa = b;
                        estado
                    },
                    move |b, estado, window, cx| pintar(&ed2, b, &estado, git.as_deref(), window, cx),
                )
                .size_full(),
            )
            .into_any_element()
    }
}

fn pintar(editor: &Entity<EditorState>, b: Bounds<Pixels>, estado: &Rc<RefCell<Estado>>, git: Option<&[(usize, Hsla)]>, window: &mut Window, cx: &mut App) {
    // Arraste e soltura valem fora do minimapa, como os ouvintes do pacote na `window`.
    {
        let (est, ed) = (estado.clone(), editor.clone());
        window.on_mouse_event(move |ev: &MouseMoveEvent, fase, window, cx| {
            if fase != DispatchPhase::Bubble {
                return;
            }
            let mut e = est.borrow_mut();
            let y = f32::from(ev.position.y);
            let Some((ultimo, topo)) = e.arrasto else {
                // A sobreposição fica mais clara com o ponteiro sobre ela.
                if e.sobre_janela {
                    window.refresh();
                }
                return;
            };
            let Some(r) = Rolagem::de(ed.read(cx)) else { return };
            let dy = y - ultimo;
            e.arrasto = Some((y, topo));
            let (h, _) = r.sobreposicao();
            let abs_topo = f32::from(e.caixa.origin.y) + topo;
            let no_topo = r.topo <= 0.;
            let no_fim = r.topo.round() >= r.maximo().round();
            if (no_topo && (dy < 0. || y < abs_topo)) || (no_fim && (dy > 0. || y > abs_topo + h)) {
                return;
            }
            let (rolagem, novo) = r.arrastar(topo, dy);
            e.arrasto = Some((y, novo));
            drop(e);
            rolar(&ed, rolagem, cx);
            window.refresh();
        });
        let est = estado.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, fase, window, _| {
            if fase == DispatchPhase::Bubble && est.borrow_mut().arrasto.take().is_some() {
                window.refresh();
            }
        });
    }
    let s = editor.read(cx);
    let Some(r) = Rolagem::de(s) else { return };
    let linha = r.lh / ESCALA;
    let alt = f32::from(b.size.height);
    let texto = s.text().clone();
    let linhas = texto.lines_len();
    let (inicio, cabem) = faixa_linhas(linhas, linha, alt, r.fracao());
    let fim = (inicio + cabem).min(linhas);
    let cor_texto = crate::documento::cor::texto();
    let selecao_cor: Hsla = gpui::rgb(0x3a3d41).into();
    let sel = s.selected_range();
    let x0 = b.origin.x + px(if git.is_some() { TEXTO_X } else { 0. });
    let fonte = gpui::font(crate::documento::FONTE_EDITOR);
    // O pacote usa a altura da linha reduzida como tamanho da fonte.
    let tamanho = px(linha);
    let ts = window.text_system().clone();
    // `charWidth` do pacote: a largura de "_".
    let ch = ts.advance(ts.resolve_font(&fonte), tamanho, '_').map(|s| f32::from(s.width)).unwrap_or(linha * 0.55);
    let agora = Instant::now();
    let (sobre_sobreposicao, arrastando, opacidade) = {
        let mut e = estado.borrow_mut();
        let (h, topo) = r.sobreposicao();
        let topo = e.arrasto.map_or(topo, |(_, t)| t);
        let m = window.mouse_position();
        let sobre = e.sobre_janela && m.y >= b.origin.y + px(topo) && m.y < b.origin.y + px(topo + h);
        let alvo = if e.sobre_janela || e.arrasto.is_some() { 1. } else { 0. };
        e.fade.mirar(alvo, agora);
        (sobre, e.arrasto.is_some(), e.fade.valor(agora))
    };
    window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
        for l in inicio..fim {
            let y = b.origin.y + px((l - inicio) as f32 * linha);
            // `fillText` troca tabulação e quebras por espaço.
            let t: String = texto.slice_line(l).to_string().chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
            let t = t.trim_end().to_string();
            if !t.is_empty() {
                let run = TextRun { len: t.len(), font: fonte.clone(), color: cor_texto, background_color: None, underline: None, strikethrough: None };
                let moldada = window.text_system().shape_line(SharedString::from(t), tamanho, &[run], None);
                // O canvas desenha em 2× e o navegador reduz: o traço fica mais encorpado que o da fonte rasterizada
                // direto em 2,4 px. Duas passadas igualam a tinta média medida nas capturas.
                for _ in 0..PASSADAS {
                    let _ = moldada.paint(point(x0, y), px(linha), TextAlign::Left, None, window, cx);
                }
            }
            // Seleção por cima do texto (`SelectionState.drawLine`): a parte selecionada e, se continua na linha de
            // baixo, o resto da largura a 65%.
            let (ini, fim_l) = (texto.line_start_offset(l), texto.line_end_offset(l));
            if sel.start < sel.end && sel.start <= fim_l && sel.end > ini {
                let col = |o: usize| texto.slice(ini..o.clamp(ini, fim_l)).chars().count() as f32 * ch;
                let (a, z) = (col(sel.start), col(sel.end));
                if sel.end > fim_l {
                    let r = Bounds::new(point(x0 + px(a), y), size(b.size.width, px(linha)));
                    window.paint_quad(fill(r, selecao_cor.opacity(0.65)));
                }
                window.paint_quad(fill(Bounds::new(point(x0 + px(a), y), size(px(z - a), px(linha))), selecao_cor));
            }
        }
        for &(l, cor) in git.unwrap_or_default() {
            if l < inicio || l >= fim {
                continue;
            }
            let y = b.origin.y + px((l - inicio) as f32 * linha);
            window.paint_quad(fill(Bounds::new(point(b.origin.x + px(GIT_X), y), size(px(GIT_LARGURA), px(linha))), cor));
        }
        if r.rola() && opacidade > 0. {
            let (h, topo) = r.sobreposicao();
            let topo = estado.borrow().arrasto.map_or(topo, |(_, t)| t);
            let base = if arrastando { 0.4 } else if sobre_sobreposicao { 0.3 } else { 0.2 };
            let cor: Hsla = gpui::rgb(0x797979).into();
            let caixa = Bounds::new(point(b.origin.x, b.origin.y + px(topo)), size(b.size.width, px(h)));
            window.paint_quad(fill(caixa, cor.opacity(base * opacidade)));
        }
    });
    if opacidade > 0. && opacidade < 1. || estado.borrow().fade.valor(agora) != estado.borrow().fade.para {
        window.request_animation_frame();
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn rolagem(topo: f32) -> Rolagem {
        // 19 px por linha, 760 px de área e 3800 de rolagem.
        Rolagem { topo, cliente: 760., total: 4560., lh: 19. }
    }

    #[test]
    fn linhas_presas_a_grade() {
        // 100 linhas de 2,375 px numa área de 150: sobra 87,5 px para rolar.
        assert_eq!(faixa_linhas(100, 2.375, 150., 0.), (0, 63));
        assert_eq!(faixa_linhas(100, 2.375, 150., 1.), (37, 63));
        assert_eq!(faixa_linhas(10, 2.375, 150., 0.5), (0, 63));
    }

    #[test]
    fn sobreposicao_acompanha_a_rolagem() {
        let r = rolagem(0.);
        assert_eq!(r.sobreposicao(), (95., 0.));
        // No meio da rolagem, o topo é o menor entre a proporção da área e scrollTop / 8.
        let r = rolagem(1900.);
        assert_eq!(r.sobreposicao().1, (665. * 0.5f32).min(1900. / 8.));
        let r = rolagem(3800.);
        assert_eq!(r.sobreposicao().1, 475.);
    }

    #[test]
    fn clique_e_arraste_como_o_pacote() {
        let r = rolagem(0.);
        // Clique 50 px abaixo do topo, sem rolagem: 50 * 8 - 760 / 2.
        assert_eq!(r.clique(50.), 20.);
        // Arrastar 10 px a sobreposição no topo: o maior entre a proporção e o deslocamento × 8.
        let (rol, topo) = r.arrastar(0., 10.);
        assert_eq!(topo, 10.);
        assert!((rol - (3800f32 * 10. / 665.).max(80.)).abs() < 1e-3);
        // Passar do fim prende o topo no máximo.
        assert_eq!(r.arrastar(470., 100.).1, 475.);
    }

    #[test]
    fn transicao_de_opacidade() {
        let t0 = Instant::now();
        let mut f = Fade { de: 0., para: 0., desde: t0 };
        f.mirar(1., t0);
        assert!((f.valor(t0 + Duration::from_millis(150)) - 0.5).abs() < 1e-3);
        assert_eq!(f.valor(t0 + Duration::from_millis(400)), 1.);
    }
}
