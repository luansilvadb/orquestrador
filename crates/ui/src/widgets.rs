//! Widgets genéricos no estilo do app: ícone, botões do Lobe UI, ícone de estado, botões da janela, dicas, item de menu e alça de divisor.
use crate::tema::{self, cor};
use gpui_kit::component::tooltip::{registrar_clique, Tooltip};
use gpui_kit::{
    canvas, div, prelude::*, px, svg, AnyElement, App, Div, ElementId, FontWeight, Hsla, Pixels, SharedString, Stateful, Svg, Window,
    WindowControlArea,
};

pub fn icone(caminho: &'static str, tamanho: f32, cor: Hsla) -> Svg {
    svg().path(caminho).size(px(tamanho)).flex_none().text_color(cor)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variante {
    Primario,
    Padrao,
}

/// Botão de 32 px do Lobe UI. `ocupado` desabilita e mostra o giro.
pub fn botao(id: &'static str, rotulo: impl Into<SharedString>, variante: Variante, ocupado: bool) -> Stateful<Div> {
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .flex_none()
        .h(tema::CAMPO)
        .px(px(12.))
        .rounded(tema::RAIO_SM)
        .text_size(tema::TEXTO_BODY)
        .when(ocupado, |d| d.opacity(0.6).child(icone("icons/loader-circle.svg", 14., cor::text_secondary())));
    let base = match variante {
        Variante::Primario => base
            .bg(cor::ink_white())
            .text_color(cor::pitch_black())
            .when(!ocupado, |d| d.hover(|s| s.bg(cor::ink_white_hover())).active(|s| s.bg(cor::ink_white_active()))),
        Variante::Padrao => base
            .bg(cor::pitch_black())
            .border_1()
            .border_color(gpui_kit::rgba(0xffffff26))
            .text_color(cor::text_primary())
            .when(!ocupado, |d| d.hover(|s| s.border_color(gpui_kit::rgba(0xffffff4d)))),
    };
    base.child(rotulo.into())
}

/// Botão pequeno (24 px) com borda, como o `size="small"` padrão.
pub fn botao_pequeno(id: impl Into<gpui_kit::ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .flex_none()
        .h(px(24.))
        .px(px(7.))
        .rounded(tema::RAIO_XS)
        .bg(cor::pitch_black())
        .border_1()
        .border_color(gpui_kit::rgba(0xffffff26))
        .text_size(tema::TEXTO_LABEL)
        .hover(|s| s.border_color(gpui_kit::rgba(0xffffff4d)))
}

/// Passo do giro: 30 passos por volta de 1,5 s (o `--animate-spin` do src/index.css), 20 quadros/s.
pub const PASSO_GIRO: std::time::Duration = std::time::Duration::from_millis(50);

/// Ícone do estado do executor; o de "executando" gira em passos, pelo relógio comum (quem redesenha é o painel, a 20 Hz).
pub fn icone_estado(estado: &str, caminho: &'static str, tamanho: f32, cor: Hsla) -> Svg {
    static INICIO: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let svg = icone(caminho, tamanho, cor);
    if estado != "running" {
        return svg;
    }
    let passo = (INICIO.get_or_init(std::time::Instant::now).elapsed().as_millis() / PASSO_GIRO.as_millis()) % 30;
    svg.with_transformation(gpui_kit::Transformation::rotate(gpui_kit::percentage(passo as f32 / 30.)))
}

/// Ícone sem borda de 24 px (`ActionIcon size="small" variant="borderless"`).
pub fn acao(id: impl Into<gpui_kit::ElementId>, caminho: &'static str, habilitada: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .size(px(24.))
        .rounded(tema::RAIO_SM)
        .when(habilitada, |d| d.hover(|s| s.bg(cor::fill_hover())))
        .when(!habilitada, |d| d.opacity(0.35))
        .child(icone(caminho, 14., cor::text_secondary()))
}

/// Controle da janela: 46 px, hover branco a 10% (Fechar vermelho com o ícone branco), sem estado ativo. A área nativa
/// (`WindowControlArea`) dá ao Windows o clique, o snap do Maximizar e o duplo clique.
pub fn botao_janela(id: &'static str, caminho: &'static str, area: WindowControlArea, perigo: bool) -> impl IntoElement {
    use gpui_kit::TestSupportExt;
    div()
        .id(id)
        .group(id)
        .window_control_area(area)
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .w(tema::BOTAO_JANELA)
        .h_full()
        .when(perigo, |d| d.hover(|s| s.bg(cor::close_red())))
        .when(!perigo, |d| d.hover(|s| s.bg(cor::fill_hover())))
        .child(
            icone(caminho, 16., cor::titlebar_text())
                .when(perigo, |s| s.group_hover(id, |s| s.text_color(cor::text_primary()))),
        )
        .test_support()
}

/// Envolve o elemento numa caixa de altura total com a dica.
pub fn com_dica(id: &'static str, dica: &'static str, elemento: impl IntoElement) -> impl IntoElement {
    div().id(id).h_full().tooltip_texto(dica).child(elemento)
}

/// Item de menu como no lobehub: rótulo e atalho em cinza a 70% à direita. Para `PopupMenu::menu_element`.
pub fn item_menu(rotulo: &'static str, atalho: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyElement {
    move |_, _| {
        div()
            .flex()
            .flex_1()
            .items_center()
            .justify_between()
            .gap(px(26.))
            .text_size(tema::TEXTO_MENU)
            .child(rotulo)
            .child(div().text_color(cor::text_tertiary()).opacity(0.7).child(atalho))
            .into_any_element()
    }
}

/// Avisa que um menu ou modal próprio está sendo desenhado: a dica visível some, como no React. O `PopupMenu` do Kit e
/// a [`mascara`] já chamam.
pub use gpui_kit::component::tooltip::marcar_sobreposicao;

/// Máscara dos modais (a do `Modal` do Lobe UI) cobrindo a janela; o diálogo vai como filho. Chamar durante o render.
pub fn mascara(id: impl Into<gpui_kit::ElementId>, window: &Window) -> Stateful<Div> {
    marcar_sobreposicao();
    let janela = window.viewport_size();
    div().id(id).occlude().w(janela.width).h(janela.height).bg(cor::mascara())
}

/// Tooltip de texto simples.
pub trait TooltipTexto {
    fn tooltip_texto(self, texto: impl Into<SharedString>) -> Self;
}

impl<E: StatefulInteractiveElement> TooltipTexto for E {
    fn tooltip_texto(self, texto: impl Into<SharedString>) -> Self {
        let texto: SharedString = texto.into();
        // Como no Base UI: depois do clique no dono, a dica só volta num novo hover.
        self.capture_any_mouse_down(|ev, _, _| registrar_clique(ev.position))
            .tooltip(move |window, cx| Tooltip::new(texto.clone()).build(window, cx))
    }
}

/// `texto` quebrado com o `text-wrap: pretty` do Lobe na largura `largura`, medido na fonte do app: as linhas vão
/// separadas por `\n` (os `\n` do texto ficam como parágrafos).
pub fn linhas_pretty(texto: &str, largura: f32, tamanho: Pixels, peso: FontWeight, window: &Window) -> String {
    let sistema = window.text_system().clone();
    let fonte = gpui_kit::Font { weight: peso, ..gpui_kit::font(tema::FONTE) };
    let medir = |t: &str| {
        let run = gpui_kit::TextRun { len: t.len(), font: fonte.clone(), color: Default::default(), background_color: None, underline: None, strikethrough: None };
        f32::from(sistema.shape_line(t.to_string().into(), tamanho, &[run], None).width)
    };
    texto.split('\n').map(|p| crate::quebra::linhas(p, largura, medir).join("\n")).collect::<Vec<_>>().join("\n")
}

/// Bloco de texto corrido com o `text-wrap: pretty` do Lobe, na largura do próprio bloco (a do pai, medida no quadro
/// anterior). `tamanho` e `peso` são os do texto desenhado; o resto do estilo vem do pai ou de quem chama.
pub fn texto_pretty(
    id: impl Into<ElementId>, texto: impl Into<SharedString>, tamanho: Pixels, peso: FontWeight, window: &mut Window, cx: &mut App,
) -> Div {
    let texto: SharedString = texto.into();
    let largura = window.use_keyed_state(id, cx, |_, _| 0f32);
    let l = *largura.read(cx);
    let quebrado: SharedString = if l > 0. { linhas_pretty(&texto, l, tamanho, peso, window).into() } else { texto };
    let medidor = canvas(
        move |b, window, cx| {
            let n = f32::from(b.size.width);
            if *largura.read(cx) != n {
                // Notificar no meio do desenho se perde: a largura nova vale no quadro seguinte.
                window.on_next_frame(move |_, cx| largura.update(cx, |v, cx| {
                    *v = n;
                    cx.notify();
                }));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();
    div().relative().w_full().min_w_0().text_size(tamanho).font_weight(peso).child(quebrado).child(medidor)
}

/// Borda do painel onde fica a alça de redimensionar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Borda {
    Direita,
    Esquerda,
    Topo,
}

/// Alça de arrasto de 4 px sobre a borda do painel (o pai é `relative`). Quem chama liga o `on_mouse_down` e acompanha o
/// ponteiro; o cursor de redimensionar fica na alça.
pub fn alca(id: impl Into<gpui_kit::ElementId>, borda: Borda) -> Stateful<Div> {
    let base = div().id(id).absolute().occlude();
    match borda {
        Borda::Direita => base.top_0().bottom_0().right(px(-2.)).w(px(4.)).cursor_col_resize(),
        Borda::Esquerda => base.top_0().bottom_0().left(px(-2.)).w(px(4.)).cursor_col_resize(),
        Borda::Topo => base.left_0().right_0().top(px(-2.)).h(px(4.)).cursor_row_resize(),
    }
}
