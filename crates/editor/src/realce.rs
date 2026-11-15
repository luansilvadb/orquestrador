//! Realce de sintaxe com `syntect` e o tema dark-plus do VS Code (as cores de `shiki-colors.ts`), mais as cores dos
//! pares de colchetes de `bracket-colors.ts` e o marcador de lista do Markdown. O tema vem de `assets/dark-plus.json`, o
//! mesmo do Shiki; as gramáticas são as do `two-face` (as do bat), escolhidas pela linguagem de [`crate::linguagem`].
use crate::linguagem;
use gpui::{FontStyle, FontWeight, HighlightStyle, Hsla, Rgba, SharedString, Task, UnderlineStyle};
use gpui_kit::component::input::{EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, InputHighlighterFactory, Rope, RopeExt};
use serde::Deserialize;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use syntect::highlighting::{
    Color, FontStyle as Estilo, HighlightIterator, HighlightState, Highlighter, ScopeSelectors, StyleModifier, Theme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

/// Acima disto (caracteres), o arquivo fica sem cor, como o `LIMIT` do Shiki e o `COLOR_LIMIT` do editor.
pub const LIMITE_COR: usize = 200_000;
/// Documentos até este tamanho são recoloridos na hora da tecla; maiores, em segundo plano.
const SINCRONO_ATE: usize = 8 * 1024;
const ESPERA: Duration = Duration::from_millis(30);

/// As gramáticas do `two-face` e, por cima (vencem na busca por escopo), as TextMate do Shiki convertidas para
/// sublime-syntax (`assets/gramaticas`), já montadas e ligadas em `assets/sintaxes.packdump` (teste `gerar_sintaxes`).
static SINTAXES: LazyLock<SyntaxSet> = LazyLock::new(|| syntect::dumps::from_binary(include_bytes!("../assets/sintaxes.packdump")));

#[derive(Deserialize)]
struct TemaJson {
    #[serde(rename = "tokenColors")]
    token_colors: Vec<Regra>,
}

#[derive(Deserialize)]
struct Regra {
    scope: Option<Escopos>,
    settings: Ajustes,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Escopos {
    Um(String),
    Varios(Vec<String>),
}

#[derive(Deserialize)]
struct Ajustes {
    foreground: Option<String>,
    #[serde(rename = "fontStyle")]
    font_style: Option<String>,
}

fn cor_hex(h: &str) -> Option<Color> {
    let h = h.trim_start_matches('#');
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some(Color { r: v(0)?, g: v(2)?, b: v(4)?, a: 255 }),
        8 => Some(Color { r: v(0)?, g: v(2)?, b: v(4)?, a: v(6)? }),
        _ => None,
    }
}

/// O dark-plus do Shiki como tema do `syntect`.
static TEMA: LazyLock<Theme> = LazyLock::new(|| {
    let json: TemaJson = serde_json::from_str(include_str!("../assets/dark-plus.json")).expect("dark-plus.json válido");
    let mut tema = Theme { settings: ThemeSettings { foreground: cor_hex("#D4D4D4"), ..Default::default() }, ..Default::default() };
    for r in json.token_colors {
        let estilo = r.settings.font_style.as_deref().map(|f| {
            let mut e = Estilo::empty();
            for p in f.split_whitespace() {
                match p {
                    "italic" => e |= Estilo::ITALIC,
                    "bold" => e |= Estilo::BOLD,
                    "underline" => e |= Estilo::UNDERLINE,
                    _ => {}
                }
            }
            e
        });
        let modificador = StyleModifier { foreground: r.settings.foreground.as_deref().and_then(cor_hex), background: None, font_style: estilo };
        let escopos = match r.scope {
            None => {
                if let Some(c) = modificador.foreground {
                    tema.settings.foreground = Some(c);
                }
                continue;
            }
            Some(Escopos::Um(s)) => vec![s],
            Some(Escopos::Varios(v)) => v,
        };
        for e in escopos {
            if let Ok(scope) = ScopeSelectors::from_str(&e) {
                tema.scopes.push(ThemeItem { scope, style: modificador });
            }
        }
    }
    // No VS Code, entre regras de mesma especificidade vence a última; no `syntect`, a primeira.
    tema.scopes.reverse();
    tema
});

/// Cor padrão do texto colorido (o `editor.foreground` do dark-plus) e do texto sem linguagem (`#CCCCCC` do editor).
pub fn cor_texto() -> Hsla {
    gpui::rgb(0xd4d4d4).into()
}

const NIVEIS: [u32; 3] = [0xFFD700, 0xDA70D6, 0x179FFF];
fn cor_inesperado() -> Hsla {
    gpui::rgba(0xff1212cc).into()
}
fn cor_lista() -> Hsla {
    gpui::rgb(0x6796e6).into()
}

fn hsla(c: Color) -> Hsla {
    Rgba { r: c.r as f32 / 255., g: c.g as f32 / 255., b: c.b as f32 / 255., a: c.a as f32 / 255. }.into()
}

/// A gramática do `syntect` para a linguagem Shiki.
pub(crate) fn sintaxe(lang: &str, caminho: &str) -> Option<&'static SyntaxReference> {
    let x = sintaxe_sem_registro(lang, caminho);
    if x.is_some() {
        CARREGADAS.lock().unwrap_or_else(|e| e.into_inner()).insert(lang.to_string());
    }
    x
}

/// Linguagens já pedidas nesta sessão. O Shiki do React nasce com `langs: []` e carrega a gramática de cada arquivo
/// aberto; um bloco cercado do Markdown só ganha realce se a linguagem dele já estiver carregada.
static CARREGADAS: LazyLock<Mutex<std::collections::HashSet<String>>> = LazyLock::new(Default::default);

fn embutida_carregada(lang: &str) -> bool {
    CARREGADAS.lock().unwrap_or_else(|e| e.into_inner()).contains(lang)
}

fn sintaxe_sem_registro(lang: &str, caminho: &str) -> Option<&'static SyntaxReference> {
    let s = &*SINTAXES;
    if let Some(x) = s.find_syntax_by_name(&format!("shiki-{lang}")) {
        return Some(x);
    }
    let especial = match lang {
        "tsx" => Some("TypeScriptReact"),
        "typescript" => Some("TypeScript"),
        "javascript" | "jsx" => Some("JavaScript (Babel)"),
        "docker" => Some("Dockerfile"),
        "bat" => Some("Batch File"),
        "dotenv" => Some("DotENV"),
        "shellscript" => Some("Bourne Again Shell (bash)"),
        "csharp" => Some("C#"),
        "cpp" => Some("C++"),
        _ => None,
    };
    let ext = caminho.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let info = linguagem::info_shiki(lang);
    especial
        .and_then(|n| s.find_syntax_by_name(n))
        .or_else(|| s.find_syntax_by_token(lang))
        .or_else(|| info.and_then(|i| s.find_syntax_by_name(&i.name)))
        .or_else(|| info.and_then(|i| i.aliases.iter().find_map(|a| s.find_syntax_by_token(a))))
        .or_else(|| (!ext.is_empty()).then(|| s.find_syntax_by_extension(ext)).flatten())
        .filter(|x| x.name != "Plain Text")
}

/// Trechos coloridos, em ordem e sem sobreposição.
pub type Trechos = Vec<(Range<usize>, HighlightStyle)>;

fn estilo(st: syntect::highlighting::Style) -> HighlightStyle {
    HighlightStyle {
        color: Some(hsla(st.foreground)),
        font_style: st.font_style.contains(Estilo::ITALIC).then_some(FontStyle::Italic),
        font_weight: st.font_style.contains(Estilo::BOLD).then_some(FontWeight::BOLD),
        underline: st.font_style.contains(Estilo::UNDERLINE).then(|| UnderlineStyle { thickness: gpui::px(1.), ..Default::default() }),
        ..Default::default()
    }
}

static REALCADOR: LazyLock<Highlighter<'static>> = LazyLock::new(|| Highlighter::new(&TEMA));

/// Linhas entre dois estados guardados do parser.
const PASSO: usize = 16;
/// Até esta distância (linhas) do fim da parte exata, a região pedida é colorida exata na hora; acima, sai de um estado
/// próximo (ou do inicial) e fica provisória até o segundo plano chegar, como o CodeMirror faz ao pular o parse.
const SALTO: usize = 96;
/// Duração de cada fatia do trabalho em segundo plano (o desenho espera no máximo uma fatia).
const FATIA: Duration = Duration::from_millis(4);

/// Estado no início de uma linha: parser, pilha de escopos do tema e colchetes abertos (caractere e nível).
#[derive(Clone, PartialEq, Eq)]
struct Estado {
    parse: Option<ParseState>,
    tema: HighlightState,
    abertos: Vec<(u8, usize)>,
}

impl Estado {
    fn inicial(sintaxe: Option<&SyntaxReference>) -> Estado {
        Estado { parse: sintaxe.map(ParseState::new), tema: HighlightState::new(&REALCADOR, ScopeStack::new()), abertos: Vec::new() }
    }
}

#[derive(Clone, Copy)]
struct Token {
    ini: u32,
    fim: u32,
    estilo: u16,
}

struct Linha {
    tokens: Box<[Token]>,
    exata: bool,
}

/// Cores de uma linha (`linha` com o fim de linha), em deslocamentos relativos a ela, avançando o estado.
fn colorir_linha(estado: &mut Estado, linha: &str) -> Trechos {
    let (fora_str, fora_com, lista, embutido) = &*ESCOPOS;
    let mut da_linha: Trechos = Vec::new();
    let ops = match estado.parse.as_mut().map(|p| p.parse_line(linha, &SINTAXES)) {
        Some(Ok(ops)) => Some(ops),
        Some(Err(_)) => {
            estado.parse = None;
            None
        }
        None => None,
    };
    // Faixas de string/comentário da linha, onde os colchetes não contam.
    let mut fora: Vec<Range<usize>> = Vec::new();
    let mut listas: Vec<Range<usize>> = Vec::new();
    let mut sem_realce: Vec<Range<usize>> = Vec::new();
    if let Some(ops) = &ops {
        let mut pilha = estado.tema.path.clone();
        let mut cursor = 0;
        let mut marcar = |a: usize, b: usize, pilha: &ScopeStack| {
            if a >= b {
                return;
            }
            let escopos = pilha.as_slice();
            if escopos.iter().any(|e| fora_str.is_prefix_of(*e) || fora_com.is_prefix_of(*e)) {
                fora.push(a..b);
            }
            if escopos.iter().any(|e| lista.is_prefix_of(*e)) {
                listas.push(a..b);
            }
            if let Some(e) = escopos.iter().find(|e| embutido.is_prefix_of(**e)) {
                let nome = e.build_string();
                if !embutida_carregada(nome.trim_start_matches("meta.embedded.block.")) {
                    sem_realce.push(a..b);
                }
            }
        };
        for (i, op) in ops {
            marcar(cursor, *i, &pilha);
            let _ = pilha.apply(op);
            cursor = *i;
        }
        marcar(cursor, linha.len(), &pilha);
        let mut pos = 0;
        let util = linha.trim_end_matches(['\n', '\r']).len();
        for (st, pedaco) in HighlightIterator::new(&mut estado.tema, ops, linha, &REALCADOR) {
            let fim = pos + pedaco.len();
            let alcance = pos..fim.min(util).max(pos);
            if !alcance.is_empty() {
                let e = estilo(st);
                match da_linha.last_mut() {
                    Some((r, s)) if r.end == alcance.start && *s == e => r.end = alcance.end,
                    _ => da_linha.push((alcance, e)),
                }
            }
            pos = fim;
        }
    }
    for r in sem_realce {
        let util = linha.trim_end_matches(['\n', '\r']).len();
        let r = r.start..r.end.min(util);
        if !r.is_empty() {
            cortar(&mut da_linha, r.clone());
            sobrepor(&mut da_linha, r, cor_texto());
        }
    }
    // Colchetes, como o caminho "pares lidos do texto" de bracket-colors.ts.
    let abertos = &mut estado.abertos;
    for (i, b) in linha.bytes().enumerate() {
        if !b"()[]{}".contains(&b) || fora.iter().any(|r| r.contains(&i)) {
            continue;
        }
        let cor = match b {
            b'(' | b'[' | b'{' => {
                let n = abertos.len();
                abertos.push((b, n));
                gpui::rgb(NIVEIS[n % 3]).into()
            }
            _ => {
                let abre = match b {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                match abertos.iter().rposition(|(c, _)| *c == abre) {
                    Some(topo) => {
                        let n = abertos[topo].1;
                        abertos.truncate(topo);
                        gpui::rgb(NIVEIS[n % 3]).into()
                    }
                    None => cor_inesperado(),
                }
            }
        };
        sobrepor(&mut da_linha, i..i + 1, cor);
    }
    for r in listas {
        sobrepor(&mut da_linha, r, cor_lista());
    }
    da_linha
}

fn acima_do_limite(texto: &Rope) -> bool {
    texto.len() > LIMITE_COR && (texto.len() > LIMITE_COR * 4 || texto.len_utf16() > LIMITE_COR)
}

/// Realce incremental: guarda o estado do parser a cada [`PASSO`] linhas e as cores por linha; colore primeiro o que
/// é pedido (a parte visível) e o resto em fatias; depois de uma edição, reprocessa a partir da linha editada até o
/// estado voltar a ser igual ao guardado.
pub struct Motor {
    sintaxe: Option<&'static SyntaxReference>,
    texto: Rope,
    ligado: bool,
    linhas: Vec<Option<Linha>>,
    /// Estado no início da linha `i`. Até `frente` são exatos; depois, podem ser de antes da última edição.
    estados: Vec<Option<Box<Estado>>>,
    /// Linhas `[0, frente)` têm cores exatas.
    frente: usize,
    /// Faixas de linhas cujas cores valem se o estado na entrada delas for o guardado (texto intacto desde então).
    reserva: Vec<Range<usize>>,
    paleta: Vec<HighlightStyle>,
    /// Linhas visíveis pedidas por último, para só redesenhar quando o trabalho as alcança.
    visivel: Range<usize>,
    /// Linhas processadas desde a criação (medida dos testes).
    pub processadas: usize,
}

// SAFETY: o que impede o `Send` automático são as `Region` do Oniguruma guardadas no `ParseState` (ponteiros para
// memória que só ele possui). O `Motor` fica num `Mutex` e é usado por uma thread de cada vez, sem compartilhar essas
// regiões.
unsafe impl Send for Motor {}

impl Motor {
    pub fn novo(sintaxe: Option<&'static SyntaxReference>, texto: Rope) -> Motor {
        let n = texto.lines_len();
        let ligado = !acima_do_limite(&texto);
        let mut estados: Vec<Option<Box<Estado>>> = (0..n).map(|_| None).collect();
        if ligado {
            estados[0] = Some(Box::new(Estado::inicial(sintaxe)));
        }
        Motor {
            sintaxe,
            texto,
            ligado,
            linhas: (0..n).map(|_| None).collect(),
            estados,
            frente: 0,
            reserva: Vec::new(),
            paleta: Vec::new(),
            visivel: 0..0,
            processadas: 0,
        }
    }

    /// Todas as linhas exatas.
    pub fn pronto(&self) -> bool {
        !self.ligado || self.frente >= self.linhas.len()
    }

    /// Estados guardados (medida de memória dos testes).
    pub fn estados_guardados(&self) -> usize {
        self.estados.iter().filter(|e| e.is_some()).count()
    }

    fn indice(&mut self, e: HighlightStyle) -> u16 {
        match self.paleta.iter().position(|p| *p == e) {
            Some(i) => i as u16,
            None => {
                self.paleta.push(e);
                (self.paleta.len() - 1) as u16
            }
        }
    }

    /// Linha `i` com o fim de linha (as gramáticas do `two-face` contam com ele).
    fn texto_linha(&self, i: usize) -> String {
        let mut l = self.texto.slice_line(i).to_string();
        if i + 1 < self.linhas.len() {
            l.push('\n');
        }
        l
    }

    /// Colore a linha `i` a partir de `estado`, guardando as cores.
    fn processar(&mut self, i: usize, estado: &mut Estado, exata: bool) {
        let linha = self.texto_linha(i);
        let trechos = colorir_linha(estado, &linha);
        let tokens = trechos.into_iter().map(|(r, s)| Token { ini: r.start as u32, fim: r.end as u32, estilo: self.indice(s) }).collect();
        self.linhas[i] = Some(Linha { tokens, exata });
        self.processadas += 1;
    }

    /// Estado exato no início de `frente`, reprocessando desde o último guardado.
    fn estado_na_frente(&mut self) -> Estado {
        let j = (0..=self.frente.min(self.linhas.len().saturating_sub(1))).rev().find(|&j| self.estados[j].is_some()).unwrap_or(0);
        let mut e = self.estados[j].as_deref().cloned().unwrap_or_else(|| Estado::inicial(self.sintaxe));
        for i in j..self.frente {
            self.processar(i, &mut e, true);
        }
        e
    }

    /// Avança a parte exata até a linha `ate` (exclusiva) ou até o tempo acabar. Devolve se tocou a parte visível.
    fn avancar(&mut self, ate: usize, prazo: Option<std::time::Instant>) -> bool {
        let ate = ate.min(self.linhas.len());
        if !self.ligado || self.frente >= ate {
            return false;
        }
        let mut tocou = false;
        let mut e = self.estado_na_frente();
        let mut i = self.frente;
        while i < ate {
            if i > 0 {
                // Convergência: o estado de entrada é o guardado e as linhas seguintes não mudaram desde então.
                if let Some(k) = self.reserva.iter().position(|r| r.contains(&i)) {
                    if self.estados[i].as_deref() == Some(&e) {
                        let fim = self.reserva[k].end;
                        self.reserva.drain(..=k);
                        tocou |= self.visivel.start < fim && i < self.visivel.end;
                        i = fim;
                        self.frente = i;
                        if i >= ate {
                            break;
                        }
                        e = self.estado_na_frente();
                        continue;
                    }
                }
                if i % PASSO == 0 || self.estados[i].is_some() {
                    self.estados[i] = Some(Box::new(e.clone()));
                }
            }
            tocou |= self.visivel.contains(&i);
            self.processar(i, &mut e, true);
            i += 1;
            self.frente = i;
            self.reserva.retain(|r| r.end > i);
            if prazo.is_some_and(|p| std::time::Instant::now() >= p) {
                break;
            }
        }
        tocou
    }

    /// Fatia de trabalho em segundo plano. Devolve (terminou, tocou a parte visível).
    pub fn fatia(&mut self, tempo: Duration) -> (bool, bool) {
        let tocou = self.avancar(usize::MAX, Some(std::time::Instant::now() + tempo));
        (self.pronto(), tocou)
    }

    /// Garante cores nas linhas `[a, b)`: exatas se perto da parte exata, provisórias se longe.
    pub fn preparar(&mut self, a: usize, b: usize) {
        let b = b.min(self.linhas.len());
        self.visivel = a..b;
        if !self.ligado || a >= b {
            return;
        }
        if a <= self.frente + SALTO {
            self.avancar(b, None);
            return;
        }
        if (a..b).all(|i| self.linhas[i].is_some()) {
            return;
        }
        // Provisório: do último estado guardado perto de `a` (talvez desatualizado), ou do inicial.
        let j = (a.saturating_sub(SALTO)..=a).rev().find(|&j| self.estados[j].is_some());
        let (inicio, mut e) = match j {
            Some(j) => (j, self.estados[j].as_deref().cloned().unwrap()),
            None => (a, Estado::inicial(self.sintaxe)),
        };
        for i in inicio..b {
            if i < a || self.linhas[i].as_ref().is_some_and(|l| l.exata) {
                let mut lixo = e.clone();
                // Só para levar o estado adiante.
                let _ = colorir_linha(&mut lixo, &self.texto_linha(i));
                e = lixo;
                continue;
            }
            self.processar(i, &mut e, false);
        }
    }

    /// Aplica uma edição do Kit (`texto` já editado).
    pub fn editar(&mut self, ed: &InputEdit, texto: Rope) {
        self.texto = texto;
        let ligado = !acima_do_limite(&self.texto);
        if ligado != self.ligado {
            *self = Motor::novo(self.sintaxe, self.texto.clone());
            return;
        }
        if !ligado {
            let n = self.texto.lines_len();
            self.linhas.resize_with(n, || None);
            self.estados.resize_with(n, || None);
            return;
        }
        let a = ed.start_position.row;
        if a >= self.linhas.len() {
            *self = Motor::novo(self.sintaxe, self.texto.clone());
            return;
        }
        let oe = ed.old_end_position.row.min(self.linhas.len().saturating_sub(1)).max(a);
        let ne = ed.new_end_position.row.max(a);
        let d = ne as isize - oe as isize;
        let mover = |x: usize| (x as isize + d).max(0) as usize;
        self.linhas.splice(a..=oe, (a..=ne).map(|_| None));
        self.estados.splice(a + 1..=oe, (a + 1..=ne).map(|_| None));
        // A edição pode ter vindo depois de linhas que o Kit conta de outro jeito; o vetor segue o texto.
        let n = self.texto.lines_len();
        self.linhas.resize_with(n, || None);
        self.estados.resize_with(n, || None);
        let mut reserva: Vec<Range<usize>> = Vec::new();
        for r in std::mem::take(&mut self.reserva) {
            if r.start < a {
                reserva.push(r.start..r.end.min(a));
            }
            if r.end > oe + 1 {
                reserva.push(mover(r.start.max(oe + 1))..mover(r.end));
            }
        }
        if self.frente > oe + 1 {
            reserva.push(ne + 1..mover(self.frente));
        }
        reserva.retain(|r| !r.is_empty() && r.start < n);
        reserva.sort_by_key(|r| r.start);
        self.reserva = reserva;
        self.frente = self.frente.min(a);
    }

    /// Estilos de `alcance` (bytes), sem buracos.
    pub fn estilos(&self, alcance: &Range<usize>) -> Trechos {
        let mut out = Vec::new();
        let mut cursor = alcance.start;
        if self.ligado && alcance.start < alcance.end {
            let a = self.texto.offset_to_point(alcance.start).row;
            let b = self.texto.offset_to_point(alcance.end.saturating_sub(1).max(alcance.start)).row;
            for i in a..=b.min(self.linhas.len().saturating_sub(1)) {
                let Some(l) = &self.linhas[i] else { continue };
                let base = self.texto.line_start_offset(i);
                for t in l.tokens.iter() {
                    let (x, y) = ((base + t.ini as usize).max(alcance.start), (base + t.fim as usize).min(alcance.end));
                    if x >= y || y <= cursor {
                        continue;
                    }
                    if cursor < x {
                        out.push((cursor..x, HighlightStyle::default()));
                    }
                    out.push((x.max(cursor)..y, self.paleta[t.estilo as usize]));
                    cursor = y;
                }
            }
        }
        if cursor < alcance.end {
            out.push((cursor..alcance.end, HighlightStyle::default()));
        }
        out
    }

    /// Todos os trechos, em deslocamentos absolutos.
    pub fn trechos(&self) -> Trechos {
        let mut out = Vec::new();
        for (i, l) in self.linhas.iter().enumerate() {
            let Some(l) = l else { continue };
            let base = self.texto.line_start_offset(i);
            out.extend(l.tokens.iter().map(|t| (base + t.ini as usize..base + t.fim as usize, self.paleta[t.estilo as usize])));
        }
        out
    }
}

/// Colore o texto inteiro: sintaxe pelo tema e, por cima, os pares de colchetes fora de string e comentário.
pub fn colorir(sintaxe: Option<&'static SyntaxReference>, texto: &str) -> Trechos {
    let mut m = Motor::novo(sintaxe, Rope::from(texto));
    m.avancar(usize::MAX, None);
    m.trechos()
}

/// Colore um trecho solto com a linguagem do arquivo (as linhas antigas do bloco do Git).
pub fn colorir_caminho(caminho: &str, texto: &str) -> Trechos {
    let sintaxe = linguagem::linguagem(caminho).as_deref().and_then(|l| sintaxe(l, caminho));
    colorir(sintaxe, texto)
}

/// Escopos que contam nos colchetes e no marcador de lista.
static ESCOPOS: LazyLock<(Scope, Scope, Scope, Scope)> = LazyLock::new(|| {
    (
        Scope::new("string").unwrap(),
        Scope::new("comment").unwrap(),
        Scope::new("punctuation.definition.list").unwrap(),
        Scope::new("meta.embedded.block").unwrap(),
    )
});

/// Tira dos trechos o `alcance` (os pedaços de fora ficam).
fn cortar(trechos: &mut Trechos, alcance: Range<usize>) {
    let mut saida = Vec::with_capacity(trechos.len() + 1);
    for (r, s) in trechos.drain(..) {
        if r.end <= alcance.start || r.start >= alcance.end {
            saida.push((r, s));
            continue;
        }
        if r.start < alcance.start {
            saida.push((r.start..alcance.start, s));
        }
        if r.end > alcance.end {
            saida.push((alcance.end..r.end, s));
        }
    }
    *trechos = saida;
}

/// Troca a cor de `alcance` mantendo o resto do estilo, partindo o trecho que o contém.
fn sobrepor(trechos: &mut Trechos, alcance: Range<usize>, cor: Hsla) {
    let i = trechos.partition_point(|(r, _)| r.end <= alcance.start);
    let novo = |base: Option<&HighlightStyle>| HighlightStyle { color: Some(cor), ..base.cloned().unwrap_or_default() };
    match trechos.get(i).cloned() {
        Some((r, s)) if r.start <= alcance.start && r.end >= alcance.end => {
            let mut partes = Vec::with_capacity(3);
            if r.start < alcance.start {
                partes.push((r.start..alcance.start, s));
            }
            partes.push((alcance.clone(), novo(Some(&s))));
            if alcance.end < r.end {
                partes.push((alcance.end..r.end, s));
            }
            trechos.splice(i..=i, partes);
        }
        _ => trechos.insert(i, (alcance, novo(None))),
    }
}

/// Dobras por chaves fora de strings e comentários de linha.
fn dobras(texto: &str) -> Vec<FoldRange> {
    let mut inicios = Vec::new();
    let mut out = Vec::new();
    for (n, linha) in texto.lines().enumerate() {
        let mut cs = linha.chars().peekable();
        let mut aspas = None;
        let mut escape = false;
        while let Some(c) = cs.next() {
            match aspas {
                Some(a) => {
                    if escape {
                        escape = false;
                    } else if c == '\\' {
                        escape = true;
                    } else if c == a {
                        aspas = None;
                    }
                }
                None => match c {
                    '/' if cs.peek() == Some(&'/') => break,
                    '"' | '\'' | '`' => aspas = Some(c),
                    '{' | '[' | '(' => inicios.push(n),
                    '}' | ']' | ')' => {
                        if let Some(i) = inicios.pop() {
                            if n > i {
                                out.push(FoldRange::new(i, n));
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
    }
    out.sort_by_key(|r| r.start_line);
    out.dedup_by_key(|r| r.start_line);
    out
}

struct Realce {
    lang: SharedString,
    motor: Arc<Mutex<Motor>>,
    dobras: Rc<RefCell<Vec<FoldRange>>>,
    tarefa: Option<Task<()>>,
    tarefa_dobras: Option<Task<()>>,
}

impl Realce {
    /// Continua a colorir em segundo plano, redesenhando quando o trabalho chega à parte visível.
    fn agendar(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<EditorState>) {
        if self.motor.lock().is_ok_and(|m| m.pronto()) {
            self.tarefa = None;
            return;
        }
        let motor = self.motor.clone();
        self.tarefa = Some(cx.spawn_in(window, async move |ent, cx| loop {
            let m = motor.clone();
            let (fim, tocou) = cx.background_executor().spawn(async move { m.lock().map(|mut m| m.fatia(FATIA)).unwrap_or((true, false)) }).await;
            if tocou && ent.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
            if fim {
                break;
            }
        }));
    }

    fn atualizar_dobras(&mut self, edicao: bool, texto: &Rope, dobrar: bool, window: &mut gpui::Window, cx: &mut gpui::Context<EditorState>) {
        if !dobrar {
            self.tarefa_dobras = None;
            self.dobras.borrow_mut().clear();
            return;
        }
        if texto.len() <= SINCRONO_ATE {
            self.tarefa_dobras = None;
            *self.dobras.borrow_mut() = dobras(&texto.to_string());
            return;
        }
        let (dobras_rc, texto) = (self.dobras.clone(), texto.clone());
        self.tarefa_dobras = Some(cx.spawn_in(window, async move |ent, cx| {
            if edicao {
                cx.background_executor().timer(ESPERA).await;
            }
            let d = cx.background_executor().spawn(async move { dobras(&texto.to_string()) }).await;
            *dobras_rc.borrow_mut() = d.clone();
            let _ = ent.update(cx, |s, cx| s.apply_highlighter_fold_candidates(d, cx));
        }));
    }
}

impl InputHighlighter for Realce {
    fn language(&self) -> SharedString {
        self.lang.clone()
    }

    fn update(&mut self, edit: Option<InputEdit>, texto: &Rope, dobrar: bool, window: &mut gpui::Window, cx: &mut gpui::Context<EditorState>) {
        if let Ok(mut m) = self.motor.lock() {
            match &edit {
                Some(e) => m.editar(e, texto.clone()),
                None => *m = Motor::novo(m.sintaxe, texto.clone()),
            }
        }
        self.agendar(window, cx);
        self.atualizar_dobras(edit.is_some(), texto, dobrar, window, cx);
    }

    fn styles(&self, alcance: &Range<usize>, _: &dyn HighlightStyleResolver) -> Vec<(Range<usize>, HighlightStyle)> {
        let Ok(mut m) = self.motor.lock() else { return vec![(alcance.clone(), HighlightStyle::default())] };
        let a = m.texto.offset_to_point(alcance.start).row;
        let b = m.texto.offset_to_point(alcance.end.saturating_sub(1).max(alcance.start)).row + 1;
        m.preparar(a, b);
        m.estilos(alcance)
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.dobras.borrow().clone()
    }
}

/// Fábrica para o `EditorState`: o "nome de linguagem" que o editor recebe é o caminho do arquivo, e daí sai a
/// linguagem Shiki e a gramática. Sem gramática, ainda colore os colchetes, como o editor do React.
pub fn fabrica() -> InputHighlighterFactory {
    Rc::new(|caminho: &str| {
        let lang = linguagem::linguagem(caminho);
        let sintaxe = lang.as_deref().and_then(|l| sintaxe(l, caminho));
        Some(Box::new(Realce {
            lang: caminho.to_string().into(),
            motor: Arc::new(Mutex::new(Motor::novo(sintaxe, Rope::new()))),
            dobras: Rc::default(),
            tarefa: None,
            tarefa_dobras: None,
        }) as Box<dyn InputHighlighter>)
    })
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::time::Instant;

    fn cor_de(trechos: &Trechos, t: &str, trecho: &str) -> Option<Hsla> {
        let i = t.find(trecho).unwrap();
        trechos.iter().find(|(r, _)| r.contains(&i)).and_then(|(_, s)| s.color)
    }

    #[test]
    fn tema_dark_plus_e_colchetes() {
        let t = "const x: number = f(a[1]);\n// (comentário)\n";
        let s = sintaxe("typescript", "a.ts");
        assert!(s.is_some(), "gramática TypeScript");
        let tr = colorir(s, t);
        // `const` é storage.type: #569CD6 no dark-plus.
        assert_eq!(cor_de(&tr, t, "const"), Some(gpui::rgb(0x569cd6).into()));
        assert_eq!(cor_de(&tr, t, "("), Some(gpui::rgb(0xFFD700).into()), "primeiro nível dourado");
        assert_eq!(cor_de(&tr, t, "["), Some(gpui::rgb(0xDA70D6).into()), "segundo nível orquídea");
        // Dentro do comentário o parêntese fica com a cor do comentário (#6A9955).
        let i = t.find("(com").unwrap();
        let c = tr.iter().find(|(r, _)| r.contains(&i)).and_then(|(_, s)| s.color);
        assert_eq!(c, Some(gpui::rgb(0x6a9955).into()));
    }

    #[test]
    fn fechamento_sem_abertura_vermelho() {
        let t = "a)";
        let tr = colorir(None, t);
        assert_eq!(cor_de(&tr, t, ")"), Some(cor_inesperado()));
    }

    #[test]
    fn bloco_cercado_so_colore_com_a_linguagem_carregada() {
        let md = "```ts\nconst x = 1;\n```\n";
        let cor = |t: &Trechos| t.iter().filter(|(r, _)| r.start >= 6 && r.end <= 18).map(|(_, s)| s.color).collect::<Vec<_>>();
        let antes = colorir(sintaxe("markdown", "a.md"), md);
        // Outros testes podem carregar o TypeScript ao mesmo tempo.
        if !embutida_carregada("typescript") {
            assert!(cor(&antes).iter().all(|c| *c == Some(cor_texto())));
        }
        assert!(sintaxe("typescript", "a.ts").is_some());
        let depois = colorir(sintaxe("markdown", "a.md"), md);
        assert!(cor(&depois).iter().any(|c| *c != Some(cor_texto())));
    }

    #[test]
    fn sintaxes_das_linguagens_comuns() {
        for (l, c) in [
            ("typescript", "a.ts"),
            ("tsx", "a.tsx"),
            ("javascript", "a.js"),
            ("json", "a.json"),
            ("rust", "a.rs"),
            ("python", "a.py"),
            ("css", "a.css"),
            ("scss", "a.scss"),
            ("html", "a.html"),
            ("markdown", "a.md"),
            ("yaml", "a.yaml"),
            ("toml", "Cargo.toml"),
            ("bat", "a.cmd"),
            ("dotenv", ".env"),
            ("docker", "Dockerfile"),
            ("csharp", "a.cs"),
            ("cpp", "a.cpp"),
            ("c", "a.c"),
            ("go", "a.go"),
        ] {
            assert!(sintaxe(l, c).is_some(), "{l}");
        }
    }

    /// TSX real repetido até `linhas`.
    fn amostra(linhas: usize) -> String {
        let base = include_str!("../examples/dados/editor-workspace.tsx");
        base.lines().cycle().take(linhas).map(|l| format!("{l}\n")).collect()
    }

    fn edicao(antes: &Rope, depois: &Rope, ini: usize, fim_velho: usize, fim_novo: usize) -> InputEdit {
        InputEdit {
            start_byte: ini,
            old_end_byte: fim_velho,
            new_end_byte: fim_novo,
            start_position: antes.offset_to_point(ini),
            old_end_position: antes.offset_to_point(fim_velho),
            new_end_position: depois.offset_to_point(fim_novo),
        }
    }

    fn estilos_de(m: &mut Motor) -> Trechos {
        let n = m.texto.lines_len();
        m.preparar(0, n);
        m.estilos(&(0..m.texto.len()))
    }

    /// Só os trechos com estilo, para comparar com [`colorir`].
    fn com_estilo(t: Trechos) -> Trechos {
        t.into_iter().filter(|(_, s)| *s != HighlightStyle::default()).collect()
    }

    // Limites em release (as metas); em debug, folga para o código sem otimização.
    fn limite(ms: u64) -> Duration {
        Duration::from_millis(if cfg!(debug_assertions) { ms * 30 } else { ms })
    }

    fn terminar(m: &mut Motor) {
        while !m.fatia(Duration::from_secs(1)).0 {}
    }

    #[test]
    fn tela_visivel_colorida_na_hora() {
        let s = sintaxe("tsx", "a.tsx");
        // Gramática e tema carregados, como depois do primeiro arquivo.
        let _ = colorir(s, "const x = 1;\n");
        // 50 mil linhas curtas, abaixo do limite de cor.
        let curto: String = "a;\n".repeat(50_000);
        for (nome, t) in [("3000", amostra(3000)), ("50 mil", curto)] {
            let ini = Instant::now();
            let mut m = Motor::novo(s, Rope::from(t.as_str()));
            let alcance = 0..m.texto.line_start_offset(60);
            m.preparar(0, 60);
            let e = m.estilos(&alcance);
            let gasto = ini.elapsed();
            eprintln!("{nome} linhas: tela visível em {gasto:?}");
            assert!(e.iter().any(|(_, s)| s.color.is_some()));
            assert!(gasto < limite(50), "{nome}: {gasto:?}");
            assert!(m.processadas <= 60 + PASSO, "só a parte visível: {}", m.processadas);
            // Pular para o meio também é imediato (provisório até o segundo plano chegar).
            let ini = Instant::now();
            let meio = m.linhas.len() / 2;
            m.preparar(meio, meio + 60);
            let gasto = ini.elapsed();
            eprintln!("{nome} linhas: meio em {gasto:?}");
            assert!(gasto < limite(50), "{nome} meio: {gasto:?}");
            assert!(m.linhas[meio + 10].is_some());
            let ini = Instant::now();
            terminar(&mut m);
            eprintln!("{nome} linhas: arquivo todo em segundo plano em {:?}", ini.elapsed());
            // Memória: um estado a cada PASSO linhas.
            assert!(m.estados_guardados() <= m.linhas.len() / PASSO + 2, "{}", m.estados_guardados());
        }
    }

    #[test]
    fn provisorio_vira_exato() {
        // Um comentário de bloco aberto longe da região pedida: o provisório erra e o segundo plano corrige.
        let mut t = String::from("/*\n");
        t.push_str(&"const x = 1;\n".repeat(400));
        t.push_str("*/\n");
        let s = sintaxe("typescript", "a.ts");
        let mut m = Motor::novo(s, Rope::from(t.as_str()));
        m.preparar(300, 310);
        terminar(&mut m);
        assert_eq!(com_estilo(estilos_de(&mut m)), colorir(s, &t));
    }

    #[test]
    fn digitar_no_meio_reprocessa_pouco_e_fica_igual() {
        let s = sintaxe("tsx", "a.tsx");
        let t = amostra(3000);
        let mut rope = Rope::from(t.as_str());
        let mut m = Motor::novo(s, rope.clone());
        terminar(&mut m);
        // Letras numa linha do meio, uma de cada vez.
        let linha = 1500;
        for (k, c) in "abc".chars().enumerate() {
            let pos = rope.line_start_offset(linha) + k;
            let antes = rope.clone();
            rope.insert(pos, &c.to_string());
            let ed = edicao(&antes, &rope, pos, pos, pos + 1);
            let p0 = m.processadas;
            let ini = Instant::now();
            m.editar(&ed, rope.clone());
            m.preparar(linha - 20, linha + 40);
            let gasto = ini.elapsed();
            terminar(&mut m);
            let feitas = m.processadas - p0;
            eprintln!("tecla: {feitas} linhas, {gasto:?}");
            assert!(feitas <= 2 * PASSO + 1, "recoloriu demais: {feitas}");
            assert!(gasto < limite(30), "{gasto:?}");
        }
        // Quebra de linha, remoção de várias linhas e um comentário aberto no fim.
        for (a, b, novo) in [(10, Some(10), "\n\n"), (20, Some(40), ""), (2900, None, "/* aberto\nx\n")] {
            let (ini, fim) = (rope.line_start_offset(a), b.map_or(rope.len(), |b| rope.line_start_offset(b)));
            let antes = rope.clone();
            rope.remove(ini..fim);
            rope.insert(ini, novo);
            m.editar(&edicao(&antes, &rope, ini, fim, ini + novo.len()), rope.clone());
            m.preparar(0, 50);
            terminar(&mut m);
        }
        assert_eq!(com_estilo(estilos_de(&mut m)), colorir(s, &rope.to_string()));
    }

    #[test]
    fn colchete_novo_recolore_o_resto() {
        let s = sintaxe("typescript", "a.ts");
        let t = "f(1);\n".repeat(100);
        let mut rope = Rope::from(t.as_str());
        let mut m = Motor::novo(s, rope.clone());
        terminar(&mut m);
        let antes = rope.clone();
        rope.insert(0, "(");
        m.editar(&edicao(&antes, &rope, 0, 0, 1), rope.clone());
        terminar(&mut m);
        assert_eq!(com_estilo(estilos_de(&mut m)), colorir(s, &rope.to_string()));
    }

    #[test]
    fn estilos_cobrem_o_alcance() {
        let mut m = Motor::novo(None, Rope::from("ab(c)\nx"));
        m.preparar(0, 2);
        let e = m.estilos(&(1..7));
        assert_eq!(e.first().map(|(r, _)| r.start), Some(1));
        assert_eq!(e.last().map(|(r, _)| r.end), Some(7));
        assert!(e.windows(2).all(|w| w[0].0.end == w[1].0.start));
        assert_eq!(cor_de(&e, "ab(c)\nx", "("), Some(gpui::rgb(NIVEIS[0]).into()));
    }
}

#[cfg(test)]
#[path = "../tests/shiki/comparar.rs"]
mod comparar_shiki;
