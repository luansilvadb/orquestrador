//! Tokens de `DESIGN.md` e medidas da interface, aplicados ao tema do GPUI Kit.
// Tokens ainda sem uso na casca ficam para as próximas fases.
#![allow(dead_code)]
use gpui_kit::component::Theme;
use gpui_kit::{px, rgb, rgba, App, Hsla, Pixels};
use std::borrow::Cow;

/// Cores: `colors` de `DESIGN.md`.
pub mod cor {
    use super::*;

    pub fn ink_white() -> Hsla { rgb(0xeeeeee).into() }
    pub fn ink_white_hover() -> Hsla { rgb(0xffffff).into() }
    pub fn ink_white_active() -> Hsla { rgb(0xcccccc).into() }
    pub fn pitch_black() -> Hsla { rgb(0x000000).into() }
    pub fn panel_black() -> Hsla { rgb(0x0d0d0d).into() }
    pub fn raised_black() -> Hsla { rgb(0x1a1a1a).into() }
    pub fn terminal_black() -> Hsla { rgb(0x0b0c0d).into() }
    pub fn text_primary() -> Hsla { rgb(0xffffff).into() }
    pub fn text_secondary() -> Hsla { rgb(0xaaaaaa).into() }
    pub fn text_tertiary() -> Hsla { rgb(0x6f6f6f).into() }
    pub fn hairline() -> Hsla { rgb(0x202020).into() }
    pub fn hairline_soft() -> Hsla { rgb(0x1a1a1a).into() }
    /// Branco a 6%.
    pub fn fill_muted() -> Hsla { rgba(0xffffff0f).into() }
    /// Branco a 10%.
    pub fn fill_hover() -> Hsla { rgba(0xffffff1a).into() }
    pub fn signal_error() -> Hsla { rgb(0xf4416c).into() }
    pub fn signal_success() -> Hsla { rgb(0xc4f042).into() }
    pub fn signal_warning() -> Hsla { rgb(0xffb224).into() }
    pub fn signal_info() -> Hsla { rgb(0x60b1ff).into() }
    pub fn focus_blue() -> Hsla { rgb(0x007fd4).into() }
    /// `close-red` a 90%, como no hover do Fechar da titlebar.
    pub fn close_red() -> Hsla { rgba(0xe81123e6).into() }

    // Exceções fixas da titlebar e da navegação.
    pub fn titlebar_text() -> Hsla { rgb(0xcccccc).into() }
    pub fn aba_ativa_texto() -> Hsla { rgb(0xececec).into() }
    pub fn aba_inativa_texto() -> Hsla { rgb(0x999999).into() }
    pub fn badge_git() -> Hsla { rgb(0x307e9f).into() }
    /// Fundo dos `DraggablePanel` (Orquestrador e Terminal), medido nas capturas de referência.
    pub fn painel_arrastavel() -> Hsla { rgb(0x0b0b0b).into() }
    /// Máscara do `Modal` do Lobe UI: `colorBgContainer` (o painel) a 60%, que clareia o preto em vez de escurecer.
    pub fn mascara() -> Hsla { rgba(0x0d0d0d99).into() }
    /// `::selection` do Lobe UI (`yellow9`), que vale também nos campos de texto.
    pub fn selecao() -> Hsla { rgb(0xffef5c).into() }
    /// Marca d'água do editor vazio: `#ECEAE4` a 6%.
    pub fn marca_dagua() -> Hsla { rgba(0xeceae40f).into() }
}

/// Tipografia.
/// A Inter 4.1 variável embutida em `assets/fonts/`. O DirectWrite agrupa a família pelo STAT, que soma o nome da
/// instância `opsz` padrão ("Text") ao `name` "Inter Variable".
pub const FONTE: &str = "Inter Variable Text";
/// Fonte do sistema, como em `src/terminal/term.ts`; o app não a empacota.
pub const FONTE_MONO: &str = "Cascadia Mono";
pub const TEXTO_HEADLINE: Pixels = px(20.);
pub const TEXTO_TITLE: Pixels = px(16.);
pub const TEXTO_BODY: Pixels = px(14.);
pub const TEXTO_MENU: Pixels = px(13.);
pub const TEXTO_LABEL: Pixels = px(12.);
pub const TEXTO_MONO: Pixels = px(13.);
/// Código (editor, comparador e conflito). A referência mede ~8,45 px por caractere em linhas de
/// 19 px; no DirectWrite isso é a Consolas a 15,5 px.
pub const FONTE_CODIGO: &str = "Consolas";
pub const TAMANHO_CODIGO: f32 = 15.5;
pub const LINHA_CODIGO: f32 = 19.;

/// Raios.
pub const RAIO_XS: Pixels = px(4.);
pub const RAIO_MENU: Pixels = px(5.);
pub const RAIO_SM: Pixels = px(6.);
pub const RAIO_MD: Pixels = px(8.);
pub const RAIO_LG: Pixels = px(12.);

/// Espaçamentos e alturas.
pub const LINHA: Pixels = px(24.);
pub const ITEM_MENU: Pixels = px(26.);
pub const FAIXA: Pixels = px(36.);
pub const NAV: Pixels = px(44.);
pub const BOTAO: Pixels = px(36.);
pub const CAMPO: Pixels = px(32.);

/// Moldura.
pub const TITLEBAR: Pixels = px(36.);
pub const BOTAO_JANELA: Pixels = px(46.);
pub const STATUS_BAR: Pixels = px(22.);
pub const LATERAL: f32 = 280.;
pub const LATERAL_MIN: f32 = 200.;
pub const PAINEL: f32 = 572.;
pub const PAINEL_MIN: f32 = 320.;
pub const CENTRO_MIN: f32 = 240.;
pub const TERMINAL: f32 = 280.;
pub const TERMINAL_MIN: f32 = 120.;
pub const ARQUIVO_MIN: f32 = 160.;
pub const JANELA_LARGURA: f32 = 1400.;
pub const JANELA_ALTURA: f32 = 900.;

const INTER: &[u8] = include_bytes!("../assets/fonts/InterVariable.ttf");
const INTER_ITALICO: &[u8] = include_bytes!("../assets/fonts/InterVariable-Italic.ttf");

/// Registra a Inter embutida e liga o tema escuro do Kit aos tokens.
pub fn aplicar(cx: &mut App) {
    if let Err(e) = cx.text_system().add_fonts(vec![Cow::Borrowed(INTER), Cow::Borrowed(INTER_ITALICO)]) {
        eprintln!("Inter não carregou: {e}");
    }
    Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
    let t = Theme::global_mut(cx);
    t.font_family = FONTE.into();
    t.font_size = TEXTO_BODY;
    t.mono_font_family = FONTE_MONO.into();
    t.mono_font_size = TEXTO_MONO;
    t.radius = RAIO_SM;
    t.radius_lg = RAIO_MD;
    t.shadow = false;
    t.focus_ring = false;

    t.background = cor::pitch_black();
    t.foreground = cor::text_primary();
    t.border = cor::hairline();
    t.muted = cor::fill_muted();
    t.muted_foreground = cor::text_secondary();
    t.accent = cor::fill_hover();
    t.accent_foreground = cor::text_primary();
    t.popover = cor::raised_black();
    t.popover_foreground = cor::text_primary();
    t.selection = cor::selecao();
    // `::selection` do Lobe: texto preto sobre o amarelo.
    t.selection_foreground = Some(rgb(0x000000).into());
    t.primary = cor::ink_white();
    t.primary_hover = cor::ink_white_hover();
    t.primary_active = cor::ink_white_active();
    t.primary_foreground = cor::pitch_black();
    t.button_primary = cor::ink_white();
    t.button_primary_hover = cor::ink_white_hover();
    t.button_primary_active = cor::ink_white_active();
    t.button_primary_foreground = cor::pitch_black();
    t.secondary = cor::fill_muted();
    t.secondary_hover = cor::fill_hover();
    t.secondary_foreground = cor::text_primary();
    t.input = cor::fill_muted();
    t.ring = cor::text_tertiary();
    t.danger = cor::signal_error();
    t.success = cor::signal_success();
    t.warning = cor::signal_warning();
    t.info = cor::signal_info();
    t.title_bar = cor::panel_black();
    t.title_bar_border = cor::hairline();
    t.status_bar = cor::panel_black();
    t.status_bar_border = cor::hairline();
    t.sidebar = cor::panel_black();
    t.sidebar_border = cor::hairline();
    t.tab_bar = cor::panel_black();
    t.colors.list = cor::pitch_black();
    t.list_active = cor::fill_muted();
    t.list_hover = cor::fill_muted();
    t.scrollbar = cor::pitch_black();
    t.window_border = cor::hairline();
}
