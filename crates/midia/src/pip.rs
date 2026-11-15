//! Janela de picture-in-picture do vídeo, como a do Chromium: pequena, sem moldura, sempre no topo, redimensionável pelas
//! bordas e arrastável pelo quadro. Não tem player próprio: desenha o quadro que a `VisaoMidia` já decodificou e manda os
//! comandos para ela. Com o ponteiro em cima, escurece e mostra "voltar para a guia" (alto à esquerda), fechar (alto à
//! direita) e tocar/pausar (centro).
use crate::visao::VisaoMidia;
use gpui_kit::{
    canvas, div, img, point, prelude::*, px, rgba, size, App, Bounds, Context, ObjectFit, PathBuilder, Pixels, Subscription,
    WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowKind, WindowOptions,
};
use orquestrador_ui::tema::cor;
use orquestrador_ui::widgets::icone;
use std::cell::Cell;
use std::rc::Rc;

pub(crate) type AreaPip = Rc<Cell<(Bounds<Pixels>, f32)>>;

pub struct JanelaPip {
    visao: WeakEntity<VisaoMidia>,
    area: AreaPip,
    sobre: bool,
    _assinaturas: Vec<Subscription>,
    _vigiar: gpui_kit::Task<()>,
}

/// Tamanho inicial: como o Chromium, cerca de um quinto da largura da tela com a proporção do vídeo, no canto inferior
/// direito da área útil.
pub(crate) fn opcoes(video: (u32, u32), cx: &App) -> WindowOptions {
    let tela = cx.primary_display().map(|d| d.visible_bounds()).unwrap_or(Bounds {
        origin: point(px(0.), px(0.)),
        size: size(px(1280.), px(720.)),
    });
    let proporcao = if video.0 > 0 && video.1 > 0 { video.1 as f32 / video.0 as f32 } else { 9. / 16. };
    let largura = (f32::from(tela.size.width) / 5.).clamp(300., 640.);
    let altura = (largura * proporcao).max(120.);
    let margem = px(16.);
    let origem = point(
        tela.origin.x + tela.size.width - px(largura) - margem,
        tela.origin.y + tela.size.height - px(altura) - margem,
    );
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: origem, size: size(px(largura), px(altura)) })),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::Normal,
        is_movable: true,
        is_resizable: true,
        is_minimizable: false,
        window_min_size: Some(size(px(160.), px(90.))),
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    }
}

impl JanelaPip {
    pub(crate) fn new(visao: WeakEntity<VisaoMidia>, area: AreaPip, window: &mut Window, cx: &mut Context<Self>) -> Self {
        sempre_no_topo(window);
        let mut assinaturas = Vec::new();
        if let Some(v) = visao.upgrade() {
            assinaturas.push(cx.observe(&v, |_, _, cx| cx.notify()));
        }
        // Fechar pelo sistema (Alt+F4) passa pela View, que pausa como o Chromium.
        let v = visao.clone();
        assinaturas.push(cx.on_release(move |_, cx| {
            let _ = v.update(cx, |v, cx| v.pip_fechada(cx));
        }));
        let vigiar = cx.spawn_in(window, async move |p, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
            let fora = p.update_in(cx, |p, window, cx| {
                if p.sobre && !window.is_window_hovered() {
                    p.sobre = false;
                    cx.notify();
                }
            });
            if fora.is_err() {
                break;
            }
        });
        JanelaPip { visao, area, sobre: false, _assinaturas: assinaturas, _vigiar: vigiar }
    }
}

impl Render for JanelaPip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (quadro, tocando) = self
            .visao
            .upgrade()
            .map(|v| {
                let v = v.read(cx);
                (v.quadro_atual(), v.tocando())
            })
            .unwrap_or((None, false));
        let area = self.area.clone();
        let botao = |id: &'static str| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(|s| s.bg(rgba(0xffffff26)))
        };
        div()
            .id("pip")
            .debug_selector(|| "pip".into())
            .relative()
            .size_full()
            .bg(cor::pitch_black())
            .window_control_area(WindowControlArea::Drag)
            // `on_hover` oscilaria: os botões ocluem a raiz. O movimento acende; a saída do ponteiro é vista em `vigiar`.
            .on_mouse_move(cx.listener(|p, _, _, cx| {
                if !p.sobre {
                    p.sobre = true;
                    cx.notify();
                }
            }))
            .child(canvas(move |b, window, _| area.set((b, window.scale_factor())), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .when_some(quadro, |d, q| d.child(img(q).size_full().object_fit(ObjectFit::Contain).debug_selector(|| "pip-video".into())))
            .when(self.sobre, |d| {
                d.child(div().absolute().top_0().left_0().size_full().bg(rgba(0x00000066)))
                    .child(
                        botao("pip-voltar")
                            .absolute()
                            .top(px(8.))
                            .left(px(8.))
                            .size(px(32.))
                            .child(canvas(|_, _, _| {}, |b, _, window, _| icone_voltar(b, window)).size(px(20.)))
                            .on_click(cx.listener(|p, _, _, cx| {
                                cx.stop_propagation();
                                let _ = p.visao.update(cx, |v, cx| v.voltar_da_pip(cx));
                            })),
                    )
                    .child(
                        botao("pip-fechar")
                            .absolute()
                            .top(px(8.))
                            .right(px(8.))
                            .size(px(32.))
                            .child(icone("icons/x.svg", 20., cor::text_primary()))
                            .on_click(cx.listener(|p, _, _, cx| {
                                cx.stop_propagation();
                                let _ = p.visao.update(cx, |v, cx| v.fechar_pip(cx));
                            })),
                    )
                    .child(
                        div().absolute().top_0().left_0().size_full().flex().items_center().justify_center().child(
                            botao("pip-tocar")
                                .size(px(56.))
                                .child(icone(
                                    if tocando { "icons/midia-pausar.svg" } else { "icons/midia-tocar.svg" },
                                    36.,
                                    cor::text_primary(),
                                ))
                                .on_click(cx.listener(|p, _, _, cx| {
                                    cx.stop_propagation();
                                    let _ = p.visao.update(cx, |v, cx| v.alternar_reproducao(cx));
                                })),
                        ),
                    )
            })
    }
}

/// O ícone "voltar para a guia" do Chromium: a moldura da guia com a seta entrando pelo canto inferior esquerdo.
fn icone_voltar(b: Bounds<Pixels>, window: &mut Window) {
    let o = b.origin;
    let u = f32::from(b.size.width) / 20.;
    let p = |x: f32, y: f32| point(o.x + px(x * u), o.y + px(y * u));
    let mut moldura = PathBuilder::stroke(px(1.6 * u));
    moldura.move_to(p(8., 3.));
    moldura.line_to(p(17., 3.));
    moldura.line_to(p(17., 15.));
    moldura.line_to(p(10., 15.));
    moldura.move_to(p(3., 10.));
    moldura.line_to(p(3., 3.));
    moldura.line_to(p(5., 3.));
    let mut seta = PathBuilder::stroke(px(1.6 * u));
    seta.move_to(p(3., 17.));
    seta.line_to(p(10., 10.));
    seta.move_to(p(5., 10.));
    seta.line_to(p(10., 10.));
    seta.line_to(p(10., 15.));
    for caminho in [moldura, seta] {
        if let Ok(c) = caminho.build() {
            window.paint_path(c, cor::text_primary());
        }
    }
}

/// O GPUI não tem janela "sempre no topo" redimensionável: no Windows, `HWND_TOPMOST` e sem o botão de maximizar (que o
/// duplo clique no quadro arrastável acionaria).
#[cfg(windows)]
fn sempre_no_topo(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, WS_MAXIMIZEBOX,
    };
    if let Ok(RawWindowHandle::Win32(h)) = HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        let hwnd = h.hwnd.get() as _;
        // SAFETY: o HWND é da janela viva que o GPUI acabou de criar.
        unsafe {
            let estilo = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(hwnd, GWL_STYLE, estilo & !(WS_MAXIMIZEBOX as isize));
            SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
    }
}

#[cfg(not(windows))]
fn sempre_no_topo(_: &Window) {}
