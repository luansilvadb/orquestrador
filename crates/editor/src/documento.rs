//! Um documento aberto: o buffer do disco e o `EditorState` do Kit que guarda o texto, a seleção e o histórico de
//! desfazer. As vistas do mesmo `doc_id` em grupos diferentes compartilham este estado. Aqui ficam também os comandos de
//! `@codemirror/commands` que o app usa e o Kit não traz, e as cores e medidas do editor (as do Antigravity, Dark Modern).
use crate::armazem::{difere_do_disco, Buffer};
use crate::realce;
use gpui::{
    actions, div, prelude::*, px, AnyElement, App, Context, Entity, Hsla, KeyBinding, Pixels, SharedString, Window,
};
use gpui_kit::component::input::{
    EditorState, GutterDecorationCollection, OverviewRulerDecorationCollection, RangeDecorationCollection, Rope, RopeExt, WrappingIndent,
};
use gpui_kit::component::{ActiveTheme as _, Theme};
use std::ops::Range;

/// Contexto de teclado do editor de arquivo (por cima do do Kit).
pub const CONTEXTO: &str = "EditorArquivo";

actions!(
    editor_arquivo,
    [
        Salvar,
        Formatar,
        IrParaLinha,
        Comentar,
        MoverLinhaAcima,
        MoverLinhaAbaixo,
        CopiarLinhaAbaixo,
        CopiarLinhaAcima,
        ExcluirLinha,
        SelecionarProxima,
        SelecionarLinha,
        InserirLinhaAbaixo,
        IrParaPar,
        ProximaAlteracao,
        AlteracaoAnterior,
    ]
);

/// Atalhos de `@codemirror/commands` (o `standardKeymap`/`defaultKeymap` do `basicSetup`) que o editor do app usa.
pub fn registrar_atalhos(cx: &mut App) {
    let c = Some(CONTEXTO);
    cx.bind_keys([
        KeyBinding::new("ctrl-s", Salvar, c),
        KeyBinding::new("shift-alt-f", Formatar, c),
        KeyBinding::new("ctrl-g", IrParaLinha, c),
        KeyBinding::new("ctrl-/", Comentar, c),
        KeyBinding::new("alt-up", MoverLinhaAcima, c),
        KeyBinding::new("alt-down", MoverLinhaAbaixo, c),
        KeyBinding::new("shift-alt-down", CopiarLinhaAbaixo, c),
        KeyBinding::new("shift-alt-up", CopiarLinhaAcima, c),
        KeyBinding::new("ctrl-shift-k", ExcluirLinha, c),
        KeyBinding::new("ctrl-d", SelecionarProxima, c),
        KeyBinding::new("ctrl-l", SelecionarLinha, c),
        KeyBinding::new("ctrl-enter", InserirLinhaAbaixo, c),
        KeyBinding::new("ctrl-shift-\\", IrParaPar, c),
        KeyBinding::new("alt-f5", ProximaAlteracao, c),
        KeyBinding::new("shift-alt-f5", AlteracaoAnterior, c),
    ]);
}

/// Medidas do editor do Antigravity: texto de 14 px em linhas de 19 px, Consolas.
pub const FONTE_EDITOR: &str = "Consolas";
pub const TAMANHO_EDITOR: Pixels = px(14.);
pub const LINHA_EDITOR: Pixels = px(19.);

pub mod cor {
    use super::*;
    pub fn fundo() -> Hsla {
        gpui::rgb(0x1f1f1f).into()
    }
    pub fn texto() -> Hsla {
        gpui::rgb(0xcccccc).into()
    }
    pub fn numero() -> Hsla {
        gpui::rgb(0x6e7681).into()
    }
    pub fn selecao() -> Hsla {
        gpui::rgb(0x264f78).into()
    }
    pub fn cursor() -> Hsla {
        gpui::rgb(0xaeafad).into()
    }
    pub fn guia() -> Hsla {
        gpui::rgb(0x404040).into()
    }
}

/// Documento aberto.
pub struct Documento {
    pub buffer: Buffer,
    pub editor: Entity<EditorState>,
    /// Decorações do gutter (marcadores do Git), da régua de visão geral e de trechos, para a fase do Git ligar.
    pub gutter: GutterDecorationCollection,
    pub regua: OverviewRulerDecorationCollection,
    pub trechos: RangeDecorationCollection,
    /// `draft` do buffer está atrás do editor (só sincroniza quando alguém lê).
    sujo_cache: bool,
}

impl Documento {
    pub fn novo(buffer: Buffer, window: &mut Window, cx: &mut App) -> Documento {
        let texto = buffer.draft.clone();
        let linguagem = linguagem_do_editor(&buffer.path);
        let somente_leitura = buffer.read_only;
        let editor = cx.new(|cx| {
            let mut s = EditorState::new(window, cx)
                .language(linguagem)
                .line_number(true)
                .folding(true)
                .indent_guides(true)
                .soft_wrap(true)
                .wrapping_indent(WrappingIndent::Same)
                .searchable(true)
                .gutter_decorations_width(px(16.))
                .context_menu(false)
                .default_value(texto);
            s.set_highlighter_factory(realce::fabrica(), cx);
            s.set_readonly(somente_leitura, cx);
            s
        });
        let (gutter, regua, trechos) = editor.update(cx, |s, cx| {
            (s.create_gutter_decorations_collection(Vec::new(), cx), s.create_overview_ruler_decorations_collection(Vec::new(), cx), s.create_range_decorations_collection(Vec::new(), cx))
        });
        Documento { sujo_cache: buffer.sujo(), buffer, editor, gutter, regua, trechos }
    }

    /// Texto atual do editor, com `\n`.
    pub fn texto(&self, cx: &App) -> String {
        self.editor.read(cx).text().to_string()
    }

    /// Copia o texto do editor para o `draft` do buffer e recalcula a sujeira. Chamado a cada mudança.
    pub fn sincronizar(&mut self, cx: &App) -> bool {
        let t = self.texto(cx);
        self.sujo_cache = difere_do_disco(&self.buffer.raw, &t);
        self.buffer.draft = t;
        if !self.sujo_cache {
            // Igual ao disco: volta ao fim de linha do arquivo.
            self.buffer.newline = crate::armazem::quebra_de(&self.buffer.raw);
        }
        self.sujo_cache
    }

    pub fn sujo(&self) -> bool {
        self.sujo_cache
    }

    /// Troca o texto vindo de fora (disco, recuperação) mantendo o que for igual no começo e no fim.
    pub fn trocar_texto(&mut self, novo: &str, window: &mut Window, cx: &mut App) {
        let atual = self.texto(cx);
        if atual != novo {
            let (ini, fim_velho, fim_novo) = diferenca(&atual, novo);
            let trecho = novo[ini..fim_novo].to_string();
            self.editor.update(cx, |s, cx| {
                let cursor = s.cursor();
                s.set_selected_range(ini..fim_velho, cx);
                s.replace(trecho, window, cx);
                let c = mapear(&atual, novo, cursor);
                s.set_selected_range(c..c, cx);
            });
        }
        self.buffer.draft = novo.to_string();
        self.sujo_cache = self.buffer.sujo();
    }

    pub fn definir_somente_leitura(&mut self, r: bool, cx: &mut App) {
        self.buffer.read_only = r;
        self.editor.update(cx, |s, cx| s.set_readonly(r, cx));
    }

    /// Caminho mudou (renomear, salvar como): troca a linguagem.
    pub fn definir_caminho(&mut self, caminho: &str, cx: &mut App) {
        self.buffer.path = caminho.to_string();
        let l = linguagem_do_editor(caminho);
        self.editor.update(cx, |s, cx| s.set_highlighter(l, cx));
    }

    /// Linha e coluna (1-based) do cursor.
    pub fn cursor(&self, cx: &App) -> (u64, u64) {
        let p = self.editor.read(cx).cursor_position();
        (p.line as u64 + 1, p.character as u64 + 1)
    }

    /// Põe o cursor em linha:coluna (1-based, limitados ao documento), rola até ele e foca.
    pub fn revelar(&self, linha: u64, coluna: u64, window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |s, cx| {
            let t = s.text().clone();
            let off = deslocamento(&t, linha, coluna);
            s.set_selected_range(off..off, cx);
            let p = t.offset_to_position(off);
            s.set_cursor_position(p, window, cx);
            s.focus(window, cx);
        });
    }
}

/// O nome de linguagem que vai para o `EditorState`: o próprio caminho (a fábrica de realce resolve a gramática). Sem
/// caminho, um nome neutro.
fn linguagem_do_editor(caminho: &str) -> SharedString {
    if caminho.is_empty() {
        "sem-titulo.txt".into()
    } else {
        caminho.to_string().into()
    }
}

/// Deslocamento em bytes de linha:coluna 1-based, limitando a linha ao documento e a coluna à linha (coluna em UTF-16).
pub fn deslocamento(t: &Rope, linha: u64, coluna: u64) -> usize {
    let n = t.lines_len().max(1);
    let l = (linha.max(1) as usize - 1).min(n - 1);
    let ini = t.line_start_offset(l);
    let texto_linha = t.slice_line(l).to_string();
    let texto_linha = texto_linha.trim_end_matches(['\n', '\r']);
    let mut u16s = 0;
    let alvo = coluna.max(1) as usize - 1;
    for (i, ch) in texto_linha.char_indices() {
        if u16s >= alvo {
            return ini + i;
        }
        u16s += ch.len_utf16();
    }
    ini + texto_linha.len()
}

/// Prefixo e sufixo comuns: devolve (início, fim no velho, fim no novo), em fronteira de caractere.
pub fn diferenca(velho: &str, novo: &str) -> (usize, usize, usize) {
    let mut ini = velho.bytes().zip(novo.bytes()).take_while(|(a, b)| a == b).count();
    while !velho.is_char_boundary(ini) || !novo.is_char_boundary(ini) {
        ini -= 1;
    }
    let mut suf = velho.as_bytes()[ini..].iter().rev().zip(novo.as_bytes()[ini..].iter().rev()).take_while(|(a, b)| a == b).count();
    while !velho.is_char_boundary(velho.len() - suf) || !novo.is_char_boundary(novo.len() - suf) {
        suf -= 1;
    }
    (ini, velho.len() - suf, novo.len() - suf)
}

/// `mapPosition` de file-view.tsx: leva uma posição do texto velho para o novo pela diferença.
pub fn mapear(antes: &str, depois: &str, pos: usize) -> usize {
    let (ini, fim_velho, fim_novo) = diferenca(antes, depois);
    if pos <= ini {
        pos
    } else if pos >= fim_velho {
        depois.len() - (antes.len() - pos)
    } else {
        pos.min(fim_novo)
    }
}

/// Ir para linha:coluna (`parseGoto`): linha dentro do documento e coluna até o fim da linha + 1.
pub fn ler_ir_para(valor: &str, linhas: usize, tamanho_linha: impl Fn(usize) -> usize) -> Result<(u64, u64), String> {
    let re = regex::Regex::new(r"^\s*(\d+)(?:\s*:\s*(\d+))?\s*$").unwrap();
    let Some(m) = re.captures(valor) else { return Err("Digite linha ou linha:coluna, só números.".into()) };
    let linha: usize = m[1].parse().unwrap_or(0);
    if linha < 1 || linha > linhas {
        return Err(format!("A linha vai de 1 a {linhas}."));
    }
    let coluna: usize = m.get(2).map_or(1, |c| c.as_str().parse().unwrap_or(0));
    let max = tamanho_linha(linha) + 1;
    if coluna < 1 || coluna > max {
        return Err(format!("A coluna vai de 1 a {max} nesta linha."));
    }
    Ok((linha as u64, coluna as u64))
}

/// Comentário de linha da linguagem (o `commentTokens.line` do CodeMirror), ou bloco quando não há de linha.
pub fn comentario(caminho: &str) -> (&'static str, Option<&'static str>) {
    let l = crate::linguagem::linguagem(caminho).unwrap_or_default();
    match l.as_str() {
        "python" | "shellscript" | "yaml" | "toml" | "dotenv" | "docker" | "ruby" | "perl" | "r" | "powershell" | "make"
        | "makefile" | "elixir" | "gdscript" | "gdresource" | "nix" | "coffee" | "julia" | "ini" | "properties" | "cmake" => ("#", None),
        "bat" => ("REM ", None),
        "sql" | "lua" | "haskell" | "elm" => ("--", None),
        "html" | "xml" | "markdown" | "vue" | "svelte" | "astro" | "mdx" => ("<!--", Some("-->")),
        "css" => ("/*", Some("*/")),
        "clojure" | "lisp" | "scheme" | "ini2" => (";", None),
        "vim" | "viml" => ("\"", None),
        "tex" | "latex" | "erlang" | "matlab" => ("%", None),
        _ => ("//", None),
    }
}

/// Linhas (0-based, inclusivas) cobertas por uma seleção.
fn linhas_de(t: &Rope, r: &Range<usize>) -> (usize, usize) {
    let a = t.offset_to_position(r.start).line as usize;
    let mut b = t.offset_to_position(r.end).line as usize;
    if b > a && r.end == t.line_start_offset(b) {
        b -= 1;
    }
    (a, b)
}

fn fim_da_linha(t: &Rope, l: usize) -> usize {
    t.line_end_offset(l)
}

/// Comandos de edição sobre o estado do Kit. Cada um vira uma edição só no histórico.
pub fn executar(s: &mut EditorState, cmd: Comando, caminho: &str, window: &mut Window, cx: &mut Context<EditorState>) {
    if !s.is_editable() && !matches!(cmd, Comando::SelecionarProxima | Comando::SelecionarLinha | Comando::IrParaPar) {
        return;
    }
    let t = s.text().clone();
    let sel = s.selected_range();
    let total = t.lines_len();
    let substituir = |s: &mut EditorState, r: Range<usize>, novo: String, depois: Range<usize>, window: &mut Window, cx: &mut Context<EditorState>| {
        s.set_selected_range(r, cx);
        s.replace(novo, window, cx);
        s.set_selected_range(depois, cx);
    };
    match cmd {
        Comando::Comentar => {
            let (a, b) = linhas_de(&t, &sel);
            let (abre, fecha) = comentario(caminho);
            let ini = t.line_start_offset(a);
            let fim = fim_da_linha(&t, b);
            let bloco = t.slice(ini..fim).to_string();
            let linhas: Vec<&str> = bloco.split('\n').collect();
            let novo = match fecha {
                None => {
                    let todas = linhas.iter().filter(|l| !l.trim().is_empty()).all(|l| l.trim_start().starts_with(abre.trim_end()));
                    let recuo = linhas.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
                    linhas
                        .iter()
                        .map(|l| {
                            if l.trim().is_empty() {
                                l.to_string()
                            } else if todas {
                                let i = l.find(abre.trim_end()).unwrap();
                                let resto = &l[i + abre.trim_end().len()..];
                                format!("{}{}", &l[..i], resto.strip_prefix(' ').unwrap_or(resto))
                            } else {
                                format!("{}{} {}", &l[..recuo], abre.trim_end(), &l[recuo..])
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                Some(fecha) => {
                    let corpo = bloco.trim();
                    if corpo.starts_with(abre) && corpo.ends_with(fecha) {
                        let i = bloco.find(abre).unwrap();
                        let j = bloco.rfind(fecha).unwrap();
                        let dentro = &bloco[i + abre.len()..j];
                        format!("{}{}{}", &bloco[..i], dentro.strip_prefix(' ').unwrap_or(dentro).strip_suffix(' ').unwrap_or(dentro), &bloco[j + fecha.len()..])
                    } else {
                        let recuo = bloco.len() - bloco.trim_start().len();
                        format!("{}{abre} {} {fecha}", &bloco[..recuo], &bloco[recuo..])
                    }
                }
            };
            let delta = novo.len() as isize - bloco.len() as isize;
            let fim_novo = (fim as isize + delta) as usize;
            substituir(s, ini..fim, novo, ini..fim_novo, window, cx);
        }
        Comando::MoverLinhaAcima | Comando::MoverLinhaAbaixo => {
            let (a, b) = linhas_de(&t, &sel);
            let acima = cmd == Comando::MoverLinhaAcima;
            if (acima && a == 0) || (!acima && b + 1 >= total) {
                return;
            }
            let (x, y) = if acima { (a - 1, b) } else { (a, b + 1) };
            let ini = t.line_start_offset(x);
            let fim = fim_da_linha(&t, y);
            let bloco = t.slice(ini..fim).to_string();
            let mut linhas: Vec<&str> = bloco.split('\n').collect();
            if acima {
                let primeira = linhas.remove(0);
                linhas.push(primeira);
            } else {
                let ultima = linhas.pop().unwrap();
                linhas.insert(0, ultima);
            }
            let novo = linhas.join("\n");
            let desloc = if acima { -(t.slice_line(x).to_string().trim_end_matches('\n').len() as isize + 1) } else { t.slice_line(y).to_string().trim_end_matches('\n').len() as isize + 1 };
            let depois = (sel.start as isize + desloc) as usize..(sel.end as isize + desloc) as usize;
            substituir(s, ini..fim, novo, depois, window, cx);
        }
        Comando::CopiarLinhaAbaixo | Comando::CopiarLinhaAcima => {
            let (a, b) = linhas_de(&t, &sel);
            let ini = t.line_start_offset(a);
            let fim = fim_da_linha(&t, b);
            let bloco = t.slice(ini..fim).to_string();
            let novo = format!("{bloco}\n{bloco}");
            let depois = if cmd == Comando::CopiarLinhaAbaixo {
                let d = bloco.len() + 1;
                sel.start + d..sel.end + d
            } else {
                sel.clone()
            };
            substituir(s, ini..fim, novo, depois, window, cx);
        }
        Comando::ExcluirLinha => {
            let (a, b) = linhas_de(&t, &sel);
            let ini = t.line_start_offset(a);
            let (ini, fim) = if b + 1 < total { (ini, t.line_start_offset(b + 1)) } else { (ini.saturating_sub(usize::from(a > 0)), t.len()) };
            substituir(s, ini..fim, String::new(), ini..ini, window, cx);
        }
        Comando::InserirLinhaAbaixo => {
            let l = t.offset_to_position(sel.end).line as usize;
            let fim = fim_da_linha(&t, l);
            let linha = t.slice_line(l).to_string();
            let recuo: String = linha.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            let novo = format!("\n{recuo}");
            let c = fim + novo.len();
            substituir(s, fim..fim, novo, c..c, window, cx);
        }
        Comando::SelecionarLinha => {
            let (a, b) = linhas_de(&t, &sel);
            let fim = if b + 1 < total { t.line_start_offset(b + 1) } else { t.len() };
            s.set_selected_range(t.line_start_offset(a)..fim, cx);
        }
        Comando::SelecionarProxima => {
            let texto = t.to_string();
            if sel.is_empty() {
                // Sem seleção: seleciona a palavra sob o cursor, como `selectNextOccurrence`.
                let eh = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
                let ini = texto[..sel.start].char_indices().rev().take_while(|(_, c)| eh(*c)).last().map_or(sel.start, |(i, _)| i);
                let fim = texto[sel.start..].char_indices().find(|(_, c)| !eh(*c)).map_or(texto.len(), |(i, _)| sel.start + i);
                if ini < fim {
                    s.set_selected_range(ini..fim, cx);
                }
                return;
            }
            let alvo = &texto[sel.clone()];
            let achado = texto[sel.end..].find(alvo).map(|i| sel.end + i).or_else(|| texto[..sel.start].find(alvo));
            if let Some(i) = achado {
                s.add_selection(i..i + alvo.len(), cx);
            }
        }
        Comando::IrParaPar => {
            let texto = t.to_string();
            if let Some(p) = par_de(&texto, sel.end) {
                s.set_selected_range(p..p, cx);
            }
        }
    }
}

/// Posição do par do colchete junto ao cursor.
fn par_de(t: &str, pos: usize) -> Option<usize> {
    let b = t.as_bytes();
    let (i, c) = [pos, pos.wrapping_sub(1)].into_iter().filter_map(|i| b.get(i).map(|c| (i, *c))).find(|(_, c)| b"()[]{}".contains(c))?;
    let (abre, fecha, frente) = match c {
        b'(' => (b'(', b')', true),
        b'[' => (b'[', b']', true),
        b'{' => (b'{', b'}', true),
        b')' => (b'(', b')', false),
        b']' => (b'[', b']', false),
        _ => (b'{', b'}', false),
    };
    let mut n = 0i32;
    if frente {
        for (j, &x) in b.iter().enumerate().skip(i) {
            n += i32::from(x == abre) - i32::from(x == fecha);
            if n == 0 {
                return Some(j + 1);
            }
        }
    } else {
        for j in (0..=i).rev() {
            n += i32::from(b[j] == fecha) - i32::from(b[j] == abre);
            if n == 0 {
                return Some(j);
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comando {
    Comentar,
    MoverLinhaAcima,
    MoverLinhaAbaixo,
    CopiarLinhaAbaixo,
    CopiarLinhaAcima,
    ExcluirLinha,
    SelecionarProxima,
    SelecionarLinha,
    InserirLinhaAbaixo,
    IrParaPar,
}

/// Envolve o editor do Kit trocando, só durante a montagem dele, as cores do tema pelas do editor do app. O Kit lê o
/// tema global ao montar o `Editor`; o resto da interface continua com o tema do app.
pub struct ComTemaDoEditor(pub Option<AnyElement>);

/// Como [`ComTemaDoEditor`], com outra troca de tema (o Preview do Markdown).
pub struct ComTema(pub Option<AnyElement>, pub fn(&mut Theme));

impl IntoElement for ComTema {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for ComTema {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, window: &mut Window, cx: &mut App,
    ) -> (gpui::LayoutId, AnyElement) {
        let antes = cx.theme().clone();
        (self.1)(Theme::global_mut(cx));
        let mut el = self.0.take().unwrap_or_else(|| div().into_any_element());
        let id = el.request_layout(window, cx);
        *Theme::global_mut(cx) = antes;
        (id, el)
    }

    fn prepaint(
        &mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, _: gpui::Bounds<Pixels>, el: &mut AnyElement,
        window: &mut Window, cx: &mut App,
    ) {
        let antes = cx.theme().clone();
        (self.1)(Theme::global_mut(cx));
        el.prepaint(window, cx);
        *Theme::global_mut(cx) = antes;
    }

    fn paint(
        &mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, _: gpui::Bounds<Pixels>, el: &mut AnyElement,
        _: &mut (), window: &mut Window, cx: &mut App,
    ) {
        let antes = cx.theme().clone();
        (self.1)(Theme::global_mut(cx));
        el.paint(window, cx);
        *Theme::global_mut(cx) = antes;
    }
}

impl IntoElement for ComTemaDoEditor {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for ComTemaDoEditor {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, window: &mut Window, cx: &mut App,
    ) -> (gpui::LayoutId, AnyElement) {
        let antes = cx.theme().clone();
        {
            let t = Theme::global_mut(cx);
            t.foreground = cor::texto();
            t.muted_foreground = cor::numero();
            t.selection = cor::selecao();
            t.caret = cor::cursor();
            let mut h = (*t.highlight_theme).clone();
            h.style.editor_background = Some(cor::fundo());
            h.style.editor_foreground = Some(cor::texto());
            h.style.editor_active_line = None;
            h.style.editor_line_number = Some(cor::numero());
            h.style.editor_active_line_number = Some(cor::texto());
            h.style.editor_gutter_background = Some(cor::fundo());
            t.highlight_theme = std::sync::Arc::new(h);
        }
        let mut el = self.0.take().unwrap_or_else(|| div().into_any_element());
        let id = el.request_layout(window, cx);
        *Theme::global_mut(cx) = antes;
        (id, el)
    }

    fn prepaint(
        &mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, _: gpui::Bounds<Pixels>, el: &mut AnyElement,
        window: &mut Window, cx: &mut App,
    ) {
        el.prepaint(window, cx);
    }

    fn paint(
        &mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, _: gpui::Bounds<Pixels>, el: &mut AnyElement,
        _: &mut (), window: &mut Window, cx: &mut App,
    ) {
        el.paint(window, cx);
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn ir_para_valida() {
        let tamanho = |l: usize| [3usize, 0, 10][l - 1];
        assert_eq!(ler_ir_para("2", 3, tamanho), Ok((2, 1)));
        assert_eq!(ler_ir_para(" 3 : 11 ", 3, tamanho), Ok((3, 11)));
        assert_eq!(ler_ir_para("3:12", 3, tamanho), Err("A coluna vai de 1 a 11 nesta linha.".into()));
        assert_eq!(ler_ir_para("4", 3, tamanho), Err("A linha vai de 1 a 3.".into()));
        assert_eq!(ler_ir_para("a", 3, tamanho), Err("Digite linha ou linha:coluna, só números.".into()));
    }

    #[test]
    fn mapear_posicao() {
        assert_eq!(mapear("abc", "abXc", 1), 1);
        assert_eq!(mapear("abc", "abXc", 3), 4);
        assert_eq!(diferenca("ação", "açao"), (3, 5, 4));
    }

    #[test]
    fn deslocamento_por_linha_coluna() {
        let t = Rope::from("ab\ncdé\n");
        assert_eq!(deslocamento(&t, 1, 1), 0);
        assert_eq!(deslocamento(&t, 2, 3), 5);
        assert_eq!(deslocamento(&t, 2, 99), 7);
        assert_eq!(deslocamento(&t, 99, 1), t.line_start_offset(t.lines_len() - 1));
    }

    #[test]
    fn par() {
        assert_eq!(par_de("a(b[c])", 1), Some(7));
        assert_eq!(par_de("a(b[c])", 7), Some(1));
    }
}
