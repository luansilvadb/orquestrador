//! Git gutter do editor, porte de `src/editor/git-gutter.ts`, `git-hunk.ts` e `git-stage.ts`: na calha, uma barra nas
//! linhas acrescentadas e modificadas e um triângulo onde linhas foram apagadas (cheios fora do índice, só o contorno
//! no preparado); marcas na régua da barra de rolagem; e o bloco aberto, um widget entre as linhas com as linhas
//! antigas do HEAD e as ações (preparar, restaurar, anterior/próxima, fechar). As comparações são buffer × HEAD (o que
//! aparece) e buffer × índice (se o bloco já está preparado), linha a linha pelo `orquestrador-hunks`.
use crate::realce;
use gpui::{
    canvas, div, fill, point, prelude::*, px, rgb, rgba, size, AnyElement, App, Bounds, CursorStyle, DispatchPhase, Entity,
    HighlightStyle, Hitbox, HitboxBehavior, Hsla, MouseButton, MouseDownEvent, PaintQuad, Pixels,
    SharedString, StyledText, Window,
};
use gpui_kit::component::input::EditorState;
use orquestrador_hunks::{self as hunks, Faixa, Trecho};
use orquestrador_ui::tema;
use gpui_kit::TestSupportExt as _;
use orquestrador_ui::widgets::{icone, TooltipTexto};
use std::path::PathBuf;
use std::rc::Rc;

/// Cores do VS Code Dark Modern (`editorGutter.*Background`).
pub fn cor(tipo: Tipo) -> Hsla {
    match tipo {
        Tipo::Adicionado => rgb(0x2ea043).into(),
        Tipo::Modificado => rgb(0x0078d4).into(),
        Tipo::Apagado => rgb(0xf85149).into(),
    }
}
/// `editorOverviewRuler.*Foreground`: a cor do gutter a 60%.
pub fn cor_regua(tipo: Tipo) -> Hsla {
    cor(tipo).opacity(0.6)
}
/// `diffEditor.removedLineBackground` e `insertedLineBackground`.
pub fn cor_removida() -> Hsla {
    rgba(0xff000033).into()
}
pub fn cor_inserida() -> Hsla {
    rgba(0x9bb95533).into()
}

/// Linhas antigas mostradas no bloco; o resto vira "… mais N linhas". Acima de tantos caracteres, sem cor de sintaxe.
pub const LIMITE_ANTIGAS: usize = 200;
const LIMITE_COR_ANTIGAS: usize = 20_000;
/// Barra do bloco aberto.
pub const BARRA: f32 = 22.;
/// Largura da faixa do gutter e da barra dentro dela, a 1 px da borda.
pub const LARGURA_CALHA: f32 = 6.;
const LARGURA_BARRA: f32 = 3.;

/// Texto-base do Git de um arquivo, com `\n`: `head` ausente em arquivo novo; `index` ausente em conflito.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BaseGit {
    pub head: Option<String>,
    pub index: Option<String>,
}

/// De onde vem a base e para onde vai o índice (`git_file_base` e `git_write_index`). Roda fora da thread da interface.
pub trait FonteGutter: Send + Sync {
    /// `None`: sem marcadores (fora de repositório, não rastreado, binário, ignorado).
    fn base(&self, caminho: &str) -> Result<Option<BaseGit>, String>;
    /// Grava o texto inteiro no índice; `esperado` é o índice que a tela viu (recusa se mudou).
    fn gravar_indice(&self, caminho: &str, texto: &str, esperado: &str) -> Result<(), String>;
}

/// A fonte padrão: os comandos do core no repositório do projeto.
pub struct FonteCore {
    pub raiz: PathBuf,
}

impl FonteGutter for FonteCore {
    fn base(&self, caminho: &str) -> Result<Option<BaseGit>, String> {
        let b = orquestrador_core::ide::git::file_base(&self.raiz, caminho)?;
        // Os textos chegam com o fim de linha do checkout; o editor trabalha com `\n`.
        Ok(b.map(|b| BaseGit {
            head: b.head.map(|t| crate::armazem::normalizar_quebras(&t)),
            index: b.index.map(|t| crate::armazem::normalizar_quebras(&t)),
        }))
    }
    fn gravar_indice(&self, caminho: &str, texto: &str, esperado: &str) -> Result<(), String> {
        orquestrador_core::ide::git::write_index(&self.raiz, caminho, texto, esperado)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Adicionado,
    Modificado,
    Apagado,
}

/// Um bloco buffer × HEAD, em linhas do buffer (base 1). O apagado tem `de == ate`, a linha da marca; `fim` é o apagado
/// depois da última linha (marca embaixo dela). `trecho` é o trecho A = HEAD, B = buffer, em bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bloco {
    pub tipo: Tipo,
    pub preparado: bool,
    pub de: usize,
    pub ate: usize,
    pub fim: bool,
    pub trecho: Trecho,
}

/// Começos de linha (bytes) de `t`.
fn inicios(t: &str) -> Vec<usize> {
    std::iter::once(0).chain(t.match_indices('\n').map(|(i, _)| i + 1)).collect()
}

/// Linha (base 1) da posição.
fn linha_de(inicios: &[usize], pos: usize) -> usize {
    inicios.partition_point(|&i| i <= pos).max(1)
}

/// Os blocos do buffer contra o HEAD, cada um preparado se nenhum trecho buffer × índice o toca (`hunksOf`).
pub fn blocos(head: &str, indice: &str, doc: &str) -> Vec<Bloco> {
    let contra_head = hunks::trechos(head, doc);
    let contra_indice = if indice == head { contra_head.clone() } else { hunks::trechos(indice, doc) };
    let ini = inicios(doc);
    let mut out = Vec::with_capacity(contra_head.len());
    let mut i = 0;
    for t in contra_head {
        let tipo = if t.de_a == t.ate_a {
            Tipo::Adicionado
        } else if t.de_b == t.ate_b {
            Tipo::Apagado
        } else {
            Tipo::Modificado
        };
        // Os dois conjuntos vêm em ordem no buffer: o ponteiro só avança.
        while i < contra_indice.len() && contra_indice[i].ate_b < t.de_b {
            i += 1;
        }
        let preparado = !contra_indice[i..].iter().take_while(|c| c.de_b <= t.ate_b).any(|c| hunks::tocam((*c).into(), t.into()));
        let fim = t.de_b > doc.len();
        let de = linha_de(&ini, t.de_b.min(doc.len()));
        // `endB` do `Chunk`: o fim sem a quebra que fecha o trecho.
        let fim_b = t.de_b.max(t.ate_b.saturating_sub(1));
        let ate = if tipo == Tipo::Apagado { de } else { linha_de(&ini, fim_b.min(doc.len())) };
        out.push(Bloco { tipo, preparado, de, ate, fim, trecho: t });
    }
    out
}

/// Índice do bloco que contém a posição do buffer, inclusive nas pontas (`hunkAtPos`).
pub fn bloco_em(blocos: &[Bloco], pos: usize) -> Option<usize> {
    blocos.iter().position(|b| b.trecho.de_b <= pos && pos <= b.trecho.ate_b)
}

/// Índice do bloco que contém a linha (base 1).
pub fn bloco_na_linha(blocos: &[Bloco], linha: usize) -> Option<usize> {
    let i = blocos.partition_point(|b| b.ate < linha);
    blocos.get(i).filter(|b| b.de <= linha).map(|_| i)
}

/// Estado do Git de um documento aberto.
#[derive(Clone, Debug, Default)]
pub struct EstadoGit {
    /// `None`: sem marcadores (sem base, rascunho, acima do limite).
    pub base: Option<BaseGit>,
    head: String,
    indice: String,
    pub blocos: Vec<Bloco>,
    /// Começo (`de_b`) do bloco aberto.
    aberto: Option<usize>,
    texto: String,
    pub ocupado: bool,
}

impl EstadoGit {
    /// Troca a base e recalcula contra `doc`. Devolve se mudou.
    pub fn definir_base(&mut self, base: Option<BaseGit>, doc: &str) -> bool {
        if base == self.base && self.texto == doc {
            return false;
        }
        if let Some(b) = &base {
            // HEAD ausente (novo no índice) aparece todo como acrescentado; índice ausente (conflito) cai no HEAD.
            self.head = b.head.clone().unwrap_or_default();
            self.indice = b.index.clone().or_else(|| b.head.clone()).unwrap_or_default();
        }
        self.base = base;
        self.texto = doc.to_string();
        self.recalcular_blocos();
        true
    }

    /// O documento mudou (digitação, desfazer, restaurar): o bloco aberto acompanha a edição.
    pub fn editar(&mut self, doc: &str) {
        if self.texto == doc {
            return;
        }
        self.aberto = self.aberto.map(|p| crate::documento::mapear(&self.texto, doc, p));
        self.texto = doc.to_string();
        self.recalcular_blocos();
    }

    fn recalcular_blocos(&mut self) {
        self.blocos = match &self.base {
            Some(_) if crate::armazem::tamanho_js(&self.texto) <= realce::LIMITE_COR => blocos(&self.head, &self.indice, &self.texto),
            _ => Vec::new(),
        };
        // Se nenhum bloco contém mais a posição (restaurado, base nova), fecha.
        self.aberto = self.aberto.and_then(|p| bloco_em(&self.blocos, p)).map(|i| self.blocos[i].trecho.de_b);
    }

    /// Índice do bloco aberto.
    pub fn aberto(&self) -> Option<usize> {
        self.aberto.and_then(|p| bloco_em(&self.blocos, p))
    }

    pub fn abrir(&mut self, bloco: Option<usize>) {
        self.aberto = bloco.and_then(|i| self.blocos.get(i)).map(|b| b.trecho.de_b);
    }

    /// Clicar no marcador: abre o bloco, ou fecha se já era ele.
    pub fn alternar(&mut self, bloco: usize) {
        let atual = self.aberto();
        self.abrir(if atual == Some(bloco) { None } else { Some(bloco) });
    }

    /// Há índice para preparar (sem ele, conflito).
    pub fn pode_preparar(&self) -> bool {
        self.base.as_ref().is_some_and(|b| b.index.is_some())
    }

    /// Linhas antigas do bloco (o trecho do HEAD sem a quebra que o fecha).
    pub fn antigas(&self, bloco: usize) -> Vec<String> {
        self.blocos.get(bloco).map(|b| hunks::linhas_antigas(&self.head, &b.trecho)).unwrap_or_default()
    }

    /// Restaurar: o trecho do buffer a trocar e o texto do HEAD que entra.
    pub fn restauracao(&self, bloco: usize) -> Option<(std::ops::Range<usize>, String)> {
        let t = self.blocos.get(bloco)?.trecho;
        // Posições "tamanho + 1" (trecho até o fim) caem no tamanho.
        let novo = self.head[t.de_a.min(self.head.len())..t.ate_a.min(self.head.len())].to_string();
        Some((t.de_b.min(self.texto.len())..t.ate_b.min(self.texto.len()), novo))
    }

    /// Preparar (ou tirar do preparo, se já está): o índice novo e o que a tela viu.
    pub fn preparo(&self, bloco: usize) -> Option<(String, String)> {
        let b = self.blocos.get(bloco)?;
        let indice = self.base.as_ref()?.index.clone()?;
        let faixa = Faixa::from(b.trecho);
        let novo = if b.preparado {
            hunks::tirar_bloco(self.base.as_ref()?.head.as_deref().unwrap_or(""), &indice, &self.texto, faixa)
        } else {
            hunks::preparar_bloco(&indice, &self.texto, faixa)
        };
        Some((novo, indice))
    }

    /// O bloco seguinte (`1`) ou anterior (`-1`) a partir do aberto ou da linha do cursor, dando a volta.
    pub fn vizinho(&self, linha_cursor: usize, direcao: i32) -> Option<usize> {
        let inicios: Vec<usize> = self.blocos.iter().map(|b| b.de).collect();
        hunks::ir_para_bloco(&inicios, self.aberto(), linha_cursor, direcao)
    }

    /// Altura e linha (base 0; o número de linhas põe abaixo da última) do widget do bloco aberto.
    pub fn bloco_reservado(&self, linhas: usize, altura_linha: f32) -> Option<(usize, f32)> {
        let i = self.aberto()?;
        let b = &self.blocos[i];
        let antigas = self.antigas(i).len();
        let linha = if b.tipo == Tipo::Apagado && b.fim { linhas } else { b.de - 1 };
        Some((linha, altura_widget(antigas, altura_linha)))
    }
}

/// A barra, as linhas antigas (até o limite e a do "mais") e as duas bordas de 1 px.
pub fn altura_widget(antigas: usize, altura_linha: f32) -> f32 {
    BARRA + antigas.min(LIMITE_ANTIGAS + 1) as f32 * altura_linha + 2.
}

/// Ações do bloco aberto, ligadas pela área do editor.
#[derive(Clone)]
pub struct Acoes {
    pub preparar: Rc<dyn Fn(&mut Window, &mut App)>,
    pub restaurar: Rc<dyn Fn(&mut Window, &mut App)>,
    pub ir: Rc<dyn Fn(i32, &mut Window, &mut App)>,
    pub fechar: Rc<dyn Fn(&mut Window, &mut App)>,
    /// Clique num marcador: o índice do bloco.
    pub alternar: Rc<dyn Fn(usize, &mut Window, &mut App)>,
}

/// O que o widget mostra.
pub struct InfoBloco {
    pub antigas: Vec<String>,
    pub preparado: bool,
    pub em: usize,
    pub total: usize,
    pub pode_preparar: bool,
    pub somente_leitura: bool,
    pub com_indice: bool,
    pub ocupado: bool,
    pub caminho: String,
}

fn botao(
    id: &'static str, dica: SharedString, icone_svg: &'static str, texto: Option<&'static str>, bloqueado: bool,
    ocupado: bool, acao: Rc<dyn Fn(&mut Window, &mut App)>,
) -> impl IntoElement {
    let apagado = bloqueado || ocupado;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(4.))
        .h(px(20.))
        .min_w(px(20.))
        .when(texto.is_some(), |d| d.pl(px(4.)).pr(px(6.)))
        .when(texto.is_none(), |d| d.px(px(3.)))
        .rounded(px(4.))
        .whitespace_nowrap()
        .when(apagado, |d| d.opacity(0.4))
        .when(!apagado, |d| d.cursor_pointer().hover(|s| s.bg(rgba(0xffffff1a))))
        .when(ocupado, |d| d.cursor(CursorStyle::OperationNotAllowed))
        .tooltip_texto(dica)
        .child(icone(icone_svg, 14., rgb(0xcccccc).into()))
        .children(texto)
        // O foco fica no texto (o Esc continua fechando) e o clique não mexe na seleção.
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .on_click(move |_, window, cx| {
            if !apagado {
                acao(window, cx);
            }
        })
        .test_support()
}

/// Linhas antigas com a sintaxe do arquivo, lidas fora de contexto (o "se for simples" do React).
fn linhas_antigas(caminho: &str, antigas: &[String]) -> Vec<AnyElement> {
    let texto = antigas[..antigas.len().min(LIMITE_ANTIGAS)].join("\n");
    let marcas = if texto.len() <= LIMITE_COR_ANTIGAS { realce::colorir_caminho(caminho, &texto) } else { Vec::new() };
    let mut out = Vec::new();
    let mut pos = 0;
    for l in texto.split('\n') {
        let fim = pos + l.len();
        let destaques: Vec<(std::ops::Range<usize>, HighlightStyle)> = marcas
            .iter()
            .filter(|(r, _)| r.end > pos && r.start < fim)
            .map(|(r, s)| (r.start.max(pos) - pos..r.end.min(fim) - pos, *s))
            .filter(|(r, _)| !r.is_empty())
            .collect();
        let linha: SharedString = l.to_string().into();
        out.push(
            div()
                .h(px(tema::LINHA_CODIGO))
                .whitespace_nowrap()
                .child(StyledText::new(linha).with_highlights(destaques))
                .into_any_element(),
        );
        pos = fim + 1;
    }
    if antigas.len() > LIMITE_ANTIGAS {
        let resto = antigas.len() - LIMITE_ANTIGAS;
        out.push(
            div()
                .h(px(tema::LINHA_CODIGO))
                .whitespace_nowrap()
                .italic()
                .text_color(rgb(0x9d9d9d))
                .child(format!("… mais {resto} {}", if resto == 1 { "linha" } else { "linhas" }))
                .into_any_element(),
        );
    }
    out
}

/// O widget do bloco aberto, na largura da área de texto.
pub fn widget(info: InfoBloco, acoes: &Acoes, largura: Pixels) -> AnyElement {
    let titulo = format!("Alteração {} de {}{}", info.em, info.total, if info.preparado { " · preparada" } else { "" });
    let ocupado = info.ocupado;
    let leitura = "O arquivo é somente leitura.";
    let mut barra = div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(2.))
        .h(px(BARRA))
        .px(px(8.))
        .font_family(tema::FONTE)
        .text_size(px(12.))
        .line_height(px(BARRA))
        .text_color(rgb(0xcccccc))
        .child(div().mr_auto().min_w_0().truncate().text_color(rgb(0x9d9d9d)).child(titulo));
    if info.com_indice {
        let bloqueio = if info.somente_leitura {
            Some(leitura)
        } else if !info.pode_preparar {
            Some("Sem índice para este arquivo (conflito): não há como preparar.")
        } else {
            None
        };
        let (dica, svg, texto) = if info.preparado {
            ("Tirar este bloco do preparo", "icons/minus.svg", "Tirar do preparo")
        } else {
            ("Preparar este bloco (o texto do editor, mesmo não salvo)", "icons/plus.svg", "Preparar")
        };
        barra = barra.child(botao(
            "git-hunk-preparar", bloqueio.unwrap_or(dica).into(), svg, Some(texto), bloqueio.is_some(), ocupado,
            acoes.preparar.clone(),
        ));
    }
    let restaurar_dica = if info.somente_leitura { leitura } else { "Restaurar as linhas do HEAD neste bloco (Ctrl+Z desfaz)" };
    let (ir1, ir2) = (acoes.ir.clone(), acoes.ir.clone());
    barra = barra
        .child(botao(
            "git-hunk-restaurar", restaurar_dica.into(), "icons/undo-2.svg", Some("Restaurar"), info.somente_leitura,
            ocupado, acoes.restaurar.clone(),
        ))
        .child(botao(
            "git-hunk-anterior", "Alteração anterior (Shift+Alt+F5)".into(), "icons/chevron-up.svg", None,
            false, ocupado, Rc::new(move |w, cx| ir1(-1, w, cx)),
        ))
        .child(botao(
            "git-hunk-proxima", "Próxima alteração (Alt+F5)".into(), "icons/chevron-down.svg", None, false,
            ocupado, Rc::new(move |w, cx| ir2(1, w, cx)),
        ))
        .child(botao("git-hunk-fechar", "Fechar (Esc)".into(), "icons/x.svg", None, false, ocupado, acoes.fechar.clone()));
    let mut raiz = div()
        .id("git-hunk")
        .occlude()
        .w(largura)
        .flex()
        .flex_col()
        .overflow_hidden()
        .border_t_1()
        .border_b_1()
        .border_color(rgb(0x454545))
        .bg(crate::documento::cor::fundo())
        .child(barra);
    if !info.antigas.is_empty() {
        raiz = raiz.child(
            div()
                .flex()
                .flex_col()
                .bg(cor_removida())
                .font_family(tema::FONTE_CODIGO)
                .text_size(px(tema::TAMANHO_CODIGO))
                .line_height(px(tema::LINHA_CODIGO))
                .text_color(crate::documento::cor::texto())
                .children(linhas_antigas(&info.caminho, &info.antigas)),
        );
    }
    raiz.into_any_element()
}

/// Um marcador na calha, em coordenadas da janela.
struct Marca {
    bloco: usize,
    tipo: Tipo,
    preparado: bool,
    /// Barra: topo e base; apagado: a altura da ponta do triângulo.
    topo: Pixels,
    base: Pixels,
    primeira: bool,
    ultima: bool,
}

/// O editor passa por baixo do minimapa (`area.rs`): o Kit reserva 10 px à direita da quebra e o texto do CodeMirror
/// vai até a borda do minimapa (40 colunas cabem, 41 não, nas capturas). O widget do bloco termina nessa borda.
pub const SOB_MINIMAPA: f32 = 13.;

/// Barra de rolagem vertical do tema (file-view.tsx) e altura mínima de uma marca nela.
const LARGURA_BARRA_ROLAGEM: f32 = 14.;
const MARCA_MINIMA: f32 = 3.;

/// Marcas da régua (`overview` de git-gutter.ts): posição do bloco na altura do documento, na escala da faixa. Sem
/// rolagem vertical não há faixa. Devolve as marcas (y relativo à faixa), o topo e a altura da faixa.
fn regua(s: &EditorState, blocos: &[Bloco]) -> Option<(Vec<(Tipo, Pixels, Pixels)>, Pixels, Pixels)> {
    let lh = s.line_height()?;
    let mapa = s.display_map();
    let entrada = s.input_bounds();
    let total = mapa.display_row_top(mapa.display_row_count(), lh);
    let faixa = entrada.size.height;
    if total <= faixa || faixa <= px(0.) {
        return None;
    }
    let topo_linha = |n: usize| mapa.buffer_line_to_display_row_range(n - 1).map(|r| (mapa.display_row_top(r.start, lh), mapa.display_row_top(r.start, lh) + lh * r.len() as f32));
    let mut out = Vec::new();
    for b in blocos {
        let (Some(a), Some(z)) = (topo_linha(b.de), topo_linha(b.ate)) else { continue };
        let (y, h) = if b.tipo == Tipo::Apagado { (if b.fim { a.1 } else { a.0 }, px(0.)) } else { (a.0, z.1 - a.0) };
        let altura = (h / total * faixa).max(px(MARCA_MINIMA));
        let y = y / total * faixa - if b.tipo == Tipo::Apagado { px(MARCA_MINIMA / 2.) } else { px(0.) };
        // A marca não passa da faixa (a última linha fica encostada embaixo).
        out.push((b.tipo, y.max(px(0.)).min(faixa - altura), altura));
    }
    Some((out, entrada.top(), faixa))
}

/// Linha (base 0) → cor da faixa de git do minimapa (`minimapOf`).
pub fn faixa_minimapa(blocos: &[Bloco]) -> Vec<(usize, Hsla)> {
    blocos.iter().flat_map(|b| (b.de..=b.ate).map(move |n| (n - 1, cor(b.tipo)))).collect()
}

/// Recuo da faixa do Git a partir do começo do texto: os números terminam 24 px antes dele (6 de margem e 18 da
/// dobra do Kit); nas capturas do React, a faixa começa logo depois deles.
const RECUO_CALHA: f32 = 27.;

fn marcas(s: &EditorState, blocos: &[Bloco]) -> Option<(Vec<Marca>, Pixels, Bounds<Pixels>)> {
    let texto = s.text_bounds()?;
    let lh = s.line_height()?;
    let calha = s.gutter_width()?;
    let entrada = s.input_bounds();
    let mapa = s.display_map();
    let x = entrada.left() + calha - px(RECUO_CALHA);
    let mut out = Vec::new();
    for (i, b) in blocos.iter().enumerate() {
        for n in b.de..=b.ate {
            let Some(r) = mapa.buffer_line_to_display_row_range(n - 1) else { continue };
            let topo = texto.top() + mapa.display_row_top(r.start, lh);
            let base = topo + lh * r.len() as f32;
            if base < entrada.top() - lh || topo > entrada.bottom() + lh {
                continue;
            }
            let (topo, base) = if b.tipo == Tipo::Apagado { (if b.fim { base } else { topo }, base) } else { (topo, base) };
            out.push(Marca { bloco: i, tipo: b.tipo, preparado: b.preparado, topo, base, primeira: n == b.de, ultima: n == b.ate });
        }
    }
    Some((out, x, entrada))
}

fn triangulo(x: Pixels, y: Pixels, cheio: bool, cor: Hsla, window: &mut Window) {
    // 5×8 apontando para o texto, como o SVG do React; o vazado é o contorno de 1 px por dentro.
    let (topo, meio, base) = (y - px(4.), y, y + px(4.));
    let mut p = if cheio { gpui::PathBuilder::fill() } else { gpui::PathBuilder::stroke(px(1.)) };
    if cheio {
        p.move_to(point(x, topo));
        p.line_to(point(x + px(5.), meio));
        p.line_to(point(x, base));
    } else {
        p.move_to(point(x + px(0.5), topo + px(1.2)));
        p.line_to(point(x + px(0.5), base - px(1.2)));
        p.line_to(point(x + px(4.), meio));
    }
    p.close();
    if let Ok(caminho) = p.build() {
        window.paint_path(caminho, cor);
    }
}

/// Camada por cima do editor: marcadores da calha (com o clique que abre o bloco) e o widget do bloco aberto na altura
/// reservada. Lê a geometria do editor no mesmo quadro (vem depois dele na árvore).
pub fn camada(editor: Entity<EditorState>, blocos: Vec<Bloco>, aberto: Option<(usize, AnyElement)>, acoes: Acoes) -> impl IntoElement {
    canvas(
        {
            let editor = editor.clone();
            move |_, window, cx| {
                let s = editor.read(cx);
                let geometria = marcas(s, &blocos);
                let caixa = aberto.as_ref().and_then(|(linha, _)| s.block_bounds(*linha)).map(|mut c| {
                    c.size.width = (c.size.width - px(SOB_MINIMAPA)).max(px(0.));
                    c
                });
                let regua = regua(s, &blocos);
                let mut widget = None;
                if let (Some((_, mut el)), Some(caixa)) = (aberto, caixa) {
                    let espaco = size(gpui::AvailableSpace::Definite(caixa.size.width), gpui::AvailableSpace::Definite(caixa.size.height));
                    window.with_content_mask(Some(gpui::ContentMask { bounds: caixa }), |window| {
                        el.prepaint_as_root(caixa.origin, espaco, window, cx);
                    });
                    widget = Some((el, caixa));
                }
                let alvos: Vec<(usize, Hitbox)> = geometria
                    .as_ref()
                    .map(|(ms, x, entrada)| {
                        let mut v: Vec<(usize, Bounds<Pixels>)> = Vec::new();
                        for m in ms {
                            let (topo, base) =
                                if m.tipo == Tipo::Apagado { (m.topo - px(4.), m.topo + px(4.)) } else { (m.topo, m.base) };
                            let r = Bounds::from_corners(point(*x, topo.max(entrada.top())), point(*x + px(LARGURA_CALHA), base.min(entrada.bottom())));
                            match v.last_mut() {
                                Some((b, u)) if *b == m.bloco => *u = u.union(&r),
                                _ => v.push((m.bloco, r)),
                            }
                        }
                        v.into_iter().filter(|(_, r)| r.size.height > px(0.)).map(|(b, r)| (b, window.insert_hitbox(r, HitboxBehavior::Normal))).collect()
                    })
                    .unwrap_or_default();
                (geometria, widget, alvos, regua)
            }
        },
        move |caixa_camada, (geometria, widget, alvos, regua), window, cx| {
            // Régua na faixa da barra de rolagem vertical (os 14 px da direita, por cima do minimapa), faixa da esquerda.
            if let Some((marcas_regua, topo, altura)) = regua {
                let x = caixa_camada.right() - px(LARGURA_BARRA_ROLAGEM);
                for (tipo, y, h) in marcas_regua {
                    let r = Bounds::new(point(x, topo + y), size(px(LARGURA_CALHA), h));
                    let r = Bounds::from_corners(r.origin, point(r.right(), r.bottom().min(topo + altura)));
                    window.paint_quad(fill(r, cor_regua(tipo)));
                }
            }
            if let Some((ms, x, entrada)) = geometria {
                window.with_content_mask(Some(gpui::ContentMask { bounds: entrada }), |window| {
                    for m in &ms {
                        let c = cor(m.tipo);
                        let bx = x + px(1.);
                        if m.tipo == Tipo::Apagado {
                            triangulo(bx, m.topo, !m.preparado, c, window);
                            continue;
                        }
                        let r = Bounds::from_corners(point(bx, m.topo), point(bx + px(LARGURA_BARRA), m.base));
                        if !m.preparado {
                            window.paint_quad(fill(r, c));
                            continue;
                        }
                        // Preparado: só o contorno; a borda de cima fecha na primeira linha e a de baixo na última.
                        let borda = |a: gpui::Point<Pixels>, b: gpui::Point<Pixels>| -> PaintQuad { fill(Bounds::from_corners(a, b), c) };
                        window.paint_quad(borda(point(bx, m.topo), point(bx + px(1.), m.base)));
                        window.paint_quad(borda(point(bx + px(LARGURA_BARRA - 1.), m.topo), point(bx + px(LARGURA_BARRA), m.base)));
                        if m.primeira {
                            window.paint_quad(borda(point(bx, m.topo), point(bx + px(LARGURA_BARRA), m.topo + px(1.))));
                        }
                        if m.ultima {
                            window.paint_quad(borda(point(bx, m.base - px(1.)), point(bx + px(LARGURA_BARRA), m.base)));
                        }
                    }
                });
            }
            if let Some((mut w, caixa)) = widget {
                window.with_content_mask(Some(gpui::ContentMask { bounds: caixa }), |window| w.paint(window, cx));
            }
            for (_, h) in &alvos {
                window.set_cursor_style(CursorStyle::PointingHand, h);
            }
            let alternar = acoes.alternar.clone();
            window.on_mouse_event(move |ev: &MouseDownEvent, fase, window, cx| {
                if fase != DispatchPhase::Bubble || ev.button != MouseButton::Left {
                    return;
                }
                if let Some((b, _)) = alvos.iter().find(|(_, h)| h.is_hovered(window)) {
                    cx.stop_propagation();
                    alternar(*b, window, cx);
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Fundo das linhas novas do bloco aberto.
pub fn linhas_novas(estado: &EstadoGit, texto: &str) -> Vec<gpui_kit::component::input::RangeDecoration> {
    use gpui_kit::component::input::{RangeDecoration, RangeDecorationStyle};
    let ini = inicios(texto);
    estado
        .aberto()
        .map(|i| &estado.blocos[i])
        .filter(|b| b.tipo != Tipo::Apagado)
        .map(|b| {
            let a = ini.get(b.de - 1).copied().unwrap_or(texto.len());
            let z = ini.get(b.ate).map(|p| p - 1).unwrap_or(texto.len()).max(a);
            vec![RangeDecoration::new(a..z).with_style(RangeDecorationStyle::Lines).with_color(cor_inserida())]
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod testes {
    //! Porte das contas de `tests/check-git-gutter-ui.mjs` (fixture `src/marcadores.ts` de `mock-fixtures.ts`).
    use super::*;

    pub fn marcadores() -> (String, String, String) {
        let mut head = String::from("// Marcadores de mudança do editor.\n");
        for i in 1..=80 {
            head.push_str(&format!("export const v{i} = {i};\n"));
        }
        let indice = head.replace("export const v2 = 2;", "export const v2 = 20;");
        let pasta = indice
            .replace("export const v6 = 6;\n", "export const v6 = 6;\nexport const extra1 = 1;\nexport const extra2 = 2;\n")
            .replace("export const v12 = 12;\n", "")
            .replace("export const v20 = 20;", "export const v20 = 200;");
        (head, indice, pasta)
    }

    fn resumo(bs: &[Bloco]) -> Vec<(usize, usize, Tipo, bool)> {
        bs.iter().map(|b| (b.de, b.ate, b.tipo, b.preparado)).collect()
    }

    #[test]
    fn os_tres_tipos_nas_linhas_certas() {
        let (head, indice, pasta) = marcadores();
        let bs = blocos(&head, &indice, &pasta);
        assert_eq!(
            resumo(&bs),
            vec![
                (3, 3, Tipo::Modificado, true),
                (8, 9, Tipo::Adicionado, false),
                (15, 15, Tipo::Apagado, false),
                (22, 22, Tipo::Modificado, false),
            ]
        );
        assert_eq!(bloco_na_linha(&bs, 9), Some(1));
        assert_eq!(bloco_na_linha(&bs, 10), None);
    }

    #[test]
    fn apagado_no_fim_marca_a_linha_vazia_final() {
        let bs = blocos("a\nb\nc\n", "a\nb\n", "a\nb\n");
        assert_eq!(resumo(&bs), vec![(3, 3, Tipo::Apagado, true)]);
        assert!(!bs[0].fim);
    }

    #[test]
    fn digitar_linha_nova_e_desfazer() {
        let (head, indice, pasta) = marcadores();
        let mut e = EstadoGit::default();
        e.definir_base(Some(BaseGit { head: Some(head), index: Some(indice) }), &pasta);
        // Linha nova depois da 30 (`v28`): a 31 vira acrescentada.
        let nova = pasta.replace("export const v28 = 28;\n", "export const v28 = 28;\nexport const nova = 0;\n");
        e.editar(&nova);
        assert_eq!(e.blocos.last().map(|b| (b.de, b.tipo)), Some((31, Tipo::Adicionado)));
        e.editar(&pasta);
        assert_eq!(e.blocos.len(), 4);
    }

    #[test]
    fn bloco_aberto_acompanha_a_digitacao_e_fecha_ao_restaurar() {
        let (head, indice, pasta) = marcadores();
        let mut e = EstadoGit::default();
        e.definir_base(Some(BaseGit { head: Some(head.clone()), index: Some(indice.clone()) }), &pasta);
        e.alternar(3);
        assert_eq!(e.aberto(), Some(3));
        assert_eq!(e.antigas(3), vec!["export const v20 = 20;"]);
        // Digitar dentro do bloco (fim da linha 22) mantém aberto.
        let digitado = pasta.replace("v20 = 200;", "v20 = 200; // x");
        e.editar(&digitado);
        assert_eq!(e.aberto(), Some(3));
        // Antes dele também: a posição acompanha.
        let antes = format!("// topo\n{digitado}");
        e.editar(&antes);
        assert_eq!(e.aberto().map(|i| e.blocos[i].de), Some(23));
        // Restaurar: o trecho volta ao HEAD e o bloco some.
        let (r, novo) = e.restauracao(e.aberto().unwrap()).unwrap();
        let restaurado = format!("{}{}{}", &antes[..r.start], novo, &antes[r.end..]);
        e.editar(&restaurado);
        assert_eq!(e.aberto(), None);
        assert_eq!(e.blocos.len(), 4, "o bloco do topo acrescentado entra no lugar do restaurado");
        // Clicar de novo fecha.
        e.alternar(0);
        e.alternar(0);
        assert_eq!(e.aberto(), None);
    }

    #[test]
    fn preparar_e_tirar_do_preparo() {
        let (head, indice, pasta) = marcadores();
        let mut e = EstadoGit::default();
        e.definir_base(Some(BaseGit { head: Some(head.clone()), index: Some(indice.clone()) }), &pasta);
        let (novo, esperado) = e.preparo(1).unwrap();
        assert_eq!(esperado, indice);
        let com_extra = indice.replace("export const v6 = 6;\n", "export const v6 = 6;\nexport const extra1 = 1;\nexport const extra2 = 2;\n");
        assert_eq!(novo, com_extra, "só as duas linhas acrescentadas entram");
        e.definir_base(Some(BaseGit { head: Some(head.clone()), index: Some(novo) }), &pasta);
        assert!(e.blocos[1].preparado);
        let (volta, _) = e.preparo(1).unwrap();
        assert_eq!(volta, indice, "tirar do preparo volta o índice");
        // O bloco que já vinha preparado (linha 3): tirar do preparo deixa o índice igual ao HEAD.
        e.definir_base(Some(BaseGit { head: Some(head.clone()), index: Some(indice) }), &pasta);
        assert_eq!(e.preparo(0).unwrap().0, head);
        // Sem índice (conflito) não prepara.
        e.definir_base(Some(BaseGit { head: Some(head), index: None }), &pasta);
        assert!(!e.pode_preparar());
        assert_eq!(e.preparo(0), None);
    }

    #[test]
    fn navegacao_da_a_volta() {
        let (head, indice, pasta) = marcadores();
        let mut e = EstadoGit::default();
        e.definir_base(Some(BaseGit { head: Some(head), index: Some(indice) }), &pasta);
        let mut linhas = Vec::new();
        let mut cursor = 1;
        for _ in 0..5 {
            let i = e.vizinho(cursor, 1).unwrap();
            e.abrir(Some(i));
            cursor = e.blocos[i].de;
            linhas.push(cursor);
        }
        assert_eq!(linhas, vec![3, 8, 15, 22, 3]);
        let i = e.vizinho(cursor, -1).unwrap();
        assert_eq!(e.blocos[i].de, 22, "Shift+Alt+F5 no primeiro vai ao último");
    }

    #[test]
    fn widget_reserva_a_altura_do_react() {
        let (head, indice, pasta) = marcadores();
        let mut e = EstadoGit::default();
        e.definir_base(Some(BaseGit { head: Some(head), index: Some(indice) }), &pasta);
        e.abrir(Some(3));
        // Acima da primeira linha do bloco (22, base 0 = 21): barra 22 + uma linha antiga + 2 de borda.
        assert_eq!(e.bloco_reservado(83, 19.), Some((21, 22. + 19. + 2.)));
        assert_eq!(altura_widget(500, 19.), 22. + 201. * 19. + 2.);
    }

    #[test]
    fn sem_base_ou_grande_demais_sem_marcadores() {
        let mut e = EstadoGit::default();
        e.definir_base(None, "a\n");
        assert!(e.blocos.is_empty());
        let grande = "x\n".repeat(realce::LIMITE_COR);
        e.definir_base(Some(BaseGit { head: Some(String::new()), index: Some(String::new()) }), &grande);
        assert!(e.blocos.is_empty());
    }
}
