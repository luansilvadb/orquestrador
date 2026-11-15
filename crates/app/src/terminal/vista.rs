//! Um terminal na tela: a grade do motor desenhada por um elemento próprio, com teclado (IME incluso), seleção, cópia,
//! colagem, rolagem, eventos de mouse para os programas que pedem e Ctrl+clique em links (porte de term.ts e
//! term-links.ts). Só os terminais visíveis passam pelo desenho; os escondidos seguem só no motor.
use super::colar::{self, Cli, Colagem};
use super::cores;
use super::links::{self, Link};
use super::motor::{Chave, Motor, Registro};
use super::teclas;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as TPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::CursorShape;
use gpui_kit::{
    div, fill, font, outline, point, prelude::*, px, size, App, BorderStyle, Bounds, ClipboardEntry, ClipboardItem,
    Context, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, Font, FontFallbacks,
    FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, KeyDownEvent, LayoutId,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollEvent,
    ShapedLine, SharedString, Size, StrikethroughStyle, Style, Task, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window,
};
use orquestrador_core::core::projects::Projects;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Fonte e medidas do xterm de term.ts: Cascadia Mono 13 px, `lineHeight: 1.15`.
pub const FONTE: &str = "Cascadia Mono";
const RESERVAS: [&str; 3] = ["Cascadia Code", "Consolas", "Courier New"];
pub const TAMANHO_FONTE: f32 = 13.;
const ALTURA_LINHA: f32 = 1.15;
/// Intervalo do piscar do xterm.
const PISCAR: Duration = Duration::from_millis(600);
/// Quanto tempo a existência de um caminho fica guardada (o hover pede a mesma linha várias vezes).
const VALIDADE_CAMINHO: Duration = Duration::from_secs(5);

/// Medição opcional de latência de digitação, ligada por `ORQ_MEDIR`: grava p50/p95 em `%TEMP%\orq-latencia.txt` a cada 50
/// amostras.
pub mod medir {
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    pub fn ligado() -> bool {
        static L: OnceLock<bool> = OnceLock::new();
        *L.get_or_init(|| std::env::var_os("ORQ_MEDIR").is_some())
    }

    /// `d`: tecla até o quadro; `saida`: tecla até a saída processada pelo motor (o resto é a janela).
    pub fn amostra(d: Duration, saida: Option<Duration>) {
        static A: Mutex<Vec<(f64, f64)>> = Mutex::new(Vec::new());
        let mut a = A.lock().unwrap_or_else(|e| e.into_inner());
        let ms = |d: Duration| d.as_secs_f64() * 1000.;
        a.push((ms(d), saida.map_or(0., ms)));
        if a.len() % 50 == 0 {
            let est = |f: &dyn Fn(&(f64, f64)) -> f64| {
                let mut o: Vec<f64> = a.iter().map(f).collect();
                o.sort_by(|x, y| x.total_cmp(y));
                let p = |q: f64| o[((o.len() as f64 - 1.) * q).round() as usize];
                format!("p50={:.2} p95={:.2} max={:.2}", p(0.5), p(0.95), o[o.len() - 1])
            };
            let linha = format!("n={} total[{}] pty[{}] janela[{}]
", a.len(), est(&|x| x.0), est(&|x| x.1), est(&|x| x.0 - x.1));
            let arq = std::env::temp_dir().join("orq-latencia.txt");
            let _ = std::fs::OpenOptions::new().create(true).append(true).open(arq).and_then(|mut f| std::io::Write::write_all(&mut f, linha.as_bytes()));
        }
    }
}

/// O que um link abre. `Arquivo` e `Pasta` vão ao editor e à árvore pela extensão `AbrirLinks`.
#[derive(Clone, Debug, PartialEq)]
pub enum Alvo {
    Arquivo { caminho: String, linha: Option<u32>, coluna: Option<u32> },
    /// Pasta dentro do projeto, relativa a ele.
    Pasta(String),
}

/// Ponto de extensão para a fase de Arquivos: quem abre arquivo no editor (na linha) e revela pasta na árvore. Sem
/// ele, Ctrl+clique em caminho não faz nada; URL abre sempre.
#[derive(Default)]
pub struct AbrirLinks(pub Option<Rc<dyn Fn(&Alvo, &mut Window, &mut App)>>);

impl gpui_kit::Global for AbrirLinks {}

/// Troca quem abre URL (os testes); sem ela, o navegador padrão pelo core.
#[derive(Default)]
pub struct AbrirUrl(pub Option<Rc<dyn Fn(&str)>>);

impl gpui_kit::Global for AbrirUrl {}

/// Erros dos terminais (colagem impossível, falha do core) para o aviso da janela.
#[derive(Default)]
pub struct AvisarErro(pub Option<Rc<dyn Fn(String, &mut Window, &mut App)>>);

impl gpui_kit::Global for AvisarErro {}

pub fn avisar(msg: String, window: &mut Window, cx: &mut App) {
    let f = cx.try_global::<AvisarErro>().and_then(|a| a.0.clone());
    match f {
        Some(f) => f(msg, window, cx),
        None => eprintln!("{msg}"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Arquivo,
    Pasta,
}

/// Existência por caminho absoluto, guardada por pouco tempo.
fn tipo_do_caminho(abs: &str) -> Option<Tipo> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Option<Tipo>)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let chave = abs.to_lowercase();
    let agora = Instant::now();
    if let Some((quando, tipo)) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&chave) {
        if agora.duration_since(*quando) < VALIDADE_CAMINHO {
            return *tipo;
        }
    }
    let tipo = std::fs::metadata(Path::new(abs)).ok().map(|m| if m.is_dir() { Tipo::Pasta } else { Tipo::Arquivo });
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    if c.len() > 500 {
        c.retain(|_, (q, _)| agora.duration_since(*q) < VALIDADE_CAMINHO);
    }
    c.insert(chave, (agora, tipo));
    tipo
}

/// Link sob o mouse, de célula a célula (inclusive), em linhas absolutas da grade.
#[derive(Clone, Debug)]
pub(crate) struct LinkSob {
    inicio: TPoint,
    fim: TPoint,
    abrir: Abrir,
}

#[derive(Clone, Debug)]
enum Abrir {
    Url(String),
    Caminho { abs: String, tipo: Tipo, linha: Option<u32>, coluna: Option<u32> },
}

#[derive(Clone, Copy, Default)]
struct Geometria {
    origem: gpui_kit::Point<Pixels>,
    celula: Size<Pixels>,
}

pub struct VistaTerminal {
    pub chave: Chave,
    pub motor: Arc<Motor>,
    registro: Arc<Registro>,
    projetos: Arc<Projects>,
    /// Com teclado (orquestrador e shells); o executor é só leitura.
    pub escrevivel: bool,
    /// O CLI fixo do terminal (o do orquestrador); `None` pergunta ao core a cada colagem.
    pub cli: Option<Cli>,
    /// Pasta do projeto, para os caminhos relativos dos links.
    pub raiz: Option<String>,
    pub foco: FocusHandle,
    /// Há PTY do outro lado para receber o tamanho.
    pub vivo: bool,
    /// A aba, o painel e o projeto aparecem.
    pub visivel: bool,
    cursor_aceso: bool,
    focado: bool,
    geo: Geometria,
    selecionando: bool,
    botao: Option<MouseButton>,
    ctrl: bool,
    pub(crate) link: Option<LinkSob>,
    rolagem: f32,
    mouse: gpui_kit::Point<Pixels>,
    marcado: Option<String>,
    /// Foco pedido sem a janela à mão (abrir o painel, trocar de aba): aplicado no próximo desenho.
    pub pedir_foco: bool,
    medido: bool,
    /// Quantas vezes a grade foi preparada para desenho (os testes conferem que o escondido não desenha).
    pub pinturas: u64,
    /// Medição (`ORQ_MEDIR`): quando a última tecla saiu e a assinatura da linha do cursor naquela hora.
    tecla_em: Option<(Instant, u64)>,
    _piscar: Task<()>,
}

pub enum EventoVista {
    /// O primeiro desenho mediu a grade: o shell pode abrir com o tamanho certo.
    Medido,
}

impl gpui_kit::EventEmitter<EventoVista> for VistaTerminal {}

impl Focusable for VistaTerminal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

impl VistaTerminal {
    pub fn new(
        registro: Arc<Registro>, projetos: Arc<Projects>, motor: Arc<Motor>, escrevivel: bool, raiz: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let piscar = cx.spawn(async move |this: gpui_kit::WeakEntity<Self>, cx| loop {
            cx.background_executor().timer(PISCAR).await;
            let vivo = this.update(cx, |v, cx| {
                if v.escrevivel && v.visivel && v.focado && v.cursor_pisca() {
                    v.cursor_aceso = !v.cursor_aceso;
                    cx.notify();
                }
            });
            if vivo.is_err() {
                break;
            }
        });
        VistaTerminal {
            chave: motor.chave.clone(),
            motor,
            registro,
            projetos,
            escrevivel,
            cli: None,
            raiz,
            foco: cx.focus_handle(),
            vivo: false,
            visivel: false,
            cursor_aceso: true,
            focado: false,
            geo: Geometria::default(),
            selecionando: false,
            botao: None,
            ctrl: false,
            link: None,
            rolagem: 0.,
            mouse: Default::default(),
            marcado: None,
            pedir_foco: false,
            medido: false,
            pinturas: 0,
            tecla_em: None,
            _piscar: piscar,
        }
    }

    fn cursor_pisca(&self) -> bool {
        self.motor.travar().term.cursor_style().blinking
    }

    /// Troca o motor (outra sessão no mesmo lugar) e zera o que dependia do anterior.
    pub fn trocar_motor(&mut self, motor: Arc<Motor>, cx: &mut Context<Self>) {
        self.motor = motor;
        self.link = None;
        self.marcado = None;
        cx.notify();
    }

    /// Origem da grade e tamanho da célula no último desenho.
    #[cfg(test)]
    pub fn geometria(&self) -> (gpui_kit::Point<Pixels>, Size<Pixels>) {
        (self.geo.origem, self.geo.celula)
    }

    pub fn tamanho(&self) -> (u16, u16) {
        self.motor.tamanho()
    }

    /// Marca que o PTY existe e manda o tamanho atual.
    pub fn ligar(&mut self, cx: &mut Context<Self>) {
        self.vivo = true;
        self.mandar_tamanho(cx);
    }

    fn mandar_tamanho(&self, cx: &mut App) {
        if !self.vivo {
            return;
        }
        let (c, l) = self.motor.tamanho();
        let (projetos, (projeto, id)) = (self.projetos.clone(), self.chave.clone());
        cx.background_spawn(async move {
            if let Ok(core) = projetos.shell_core(projeto.as_deref()) {
                let _ = core.term_resize(&id, c, l);
            }
        })
        .detach();
    }

    pub fn escrever(&self, texto: &str) {
        if self.escrevivel && !texto.is_empty() {
            self.registro.escrever(&self.chave, texto.to_string());
        }
    }

    /// Entrada do usuário: volta ao fim do histórico e o cursor acende, como no xterm.
    fn digitar(&mut self, texto: &str, cx: &mut Context<Self>) {
        if !self.escrevivel {
            return;
        }
        if medir::ligado() {
            self.tecla_em = Some((Instant::now(), self.motor.assinatura_cursor()));
        }
        {
            let mut e = self.motor.travar();
            if e.term.grid().display_offset() != 0 {
                e.term.scroll_display(Scroll::Bottom);
            }
        }
        self.cursor_aceso = true;
        self.motor.esperar_eco();
        self.escrever(texto);
        cx.notify();
    }

    fn modo(&self) -> TermMode {
        *self.motor.travar().term.mode()
    }

    // ---------- teclado ----------

    pub(crate) fn tecla(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let m = k.modifiers;
        let so_ctrl = m.control && !m.alt && !m.platform;
        // Ctrl+C com seleção copia; Ctrl+V cola (term.ts). Ctrl+R e F5 recarregariam o webview e são bloqueados sempre (App.tsx).
        if so_ctrl && !m.shift && k.key == "c" && self.copiar(cx) {
            cx.stop_propagation();
            return;
        }
        if so_ctrl && k.key == "v" {
            cx.stop_propagation();
            self.colar(window, cx);
            return;
        }
        if (so_ctrl && k.key == "r") || (k.key == "f5" && !m.alt) {
            cx.stop_propagation();
            return;
        }
        if !self.escrevivel {
            return;
        }
        if let Some(seq) = teclas::sequencia(k, self.modo().contains(TermMode::APP_CURSOR)) {
            cx.stop_propagation();
            self.digitar(&String::from_utf8_lossy(&seq), cx);
        }
    }

    fn copiar(&mut self, cx: &mut Context<Self>) -> bool {
        let texto = self.motor.travar().term.selection_to_string().filter(|t| !t.is_empty());
        match texto {
            Some(t) => {
                cx.write_to_clipboard(ClipboardItem::new_string(t));
                true
            }
            None => false,
        }
    }

    /// Arquivo ou imagem no clipboard: o que vai ao terminal sai de `colar::colagem`, com o CLI que roda nele agora. Só
    /// texto vai pela colagem comum. O que não dá para colar aparece num aviso.
    fn colar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.escrevivel {
            return;
        }
        let item = cx.read_from_clipboard();
        let mut imagem = false;
        let mut texto = String::new();
        let mut arquivos: Vec<String> = Vec::new();
        if let Some(item) = &item {
            for entrada in item.entries() {
                match entrada {
                    ClipboardEntry::String(s) => texto.push_str(s.text()),
                    ClipboardEntry::Image(_) => imagem = true,
                    ClipboardEntry::ExternalPaths(p) => {
                        arquivos.extend(p.paths().iter().map(|p| p.display().to_string()));
                    }
                }
            }
        }
        // O Explorer põe os arquivos no formato próprio do Windows: o core lê na hora, na ordem.
        let snap = orquestrador_core::platform::snapshot();
        if !snap.paths.is_empty() {
            arquivos = snap.paths.iter().map(|p| p.display().to_string()).collect();
        }
        if arquivos.is_empty() && !imagem {
            if !texto.is_empty() {
                let bracketed = self.modo().contains(TermMode::BRACKETED_PASTE);
                self.digitar(&colar::texto_colado(&texto, bracketed), cx);
            }
            return;
        }
        let cli = self.cli.or_else(|| {
            let (projeto, id) = &self.chave;
            self.projetos.shell_core(projeto.as_deref()).ok()?.term_cli(id).ok().flatten().and_then(Cli::de)
        });
        let bracketed = self.modo().contains(TermMode::BRACKETED_PASTE);
        match colar::colagem(cli, &arquivos, imagem, &texto, bracketed) {
            Colagem::Escrever(s) => self.digitar(&s, cx),
            Colagem::Colar(s) => self.digitar(&colar::texto_colado(&s, bracketed), cx),
            Colagem::Erro(msg) => avisar(msg, window, cx),
        }
    }

    /// Ponto de extensão da fase de Arquivos ("copiar caminho para o orquestrador").
    #[allow(dead_code)]
    /// Cola caminhos como arquivos copiados no Explorer (o "copiar caminho para o orquestrador"). Não manda Enter.
    pub fn colar_caminhos(&mut self, caminhos: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let bracketed = self.modo().contains(TermMode::BRACKETED_PASTE);
        match colar::colagem(self.cli, &caminhos, false, "", bracketed) {
            Colagem::Escrever(s) => self.digitar(&s, cx),
            Colagem::Colar(s) => self.digitar(&colar::texto_colado(&s, bracketed), cx),
            Colagem::Erro(msg) => avisar(msg, window, cx),
        }
    }

    // ---------- mouse ----------

    /// Célula da grade sob a posição (linha absoluta, com o deslocamento do histórico) e o lado dela.
    fn celula(&self, pos: gpui_kit::Point<Pixels>) -> (TPoint, Side) {
        let e = self.motor.travar();
        let g = e.term.grid();
        let Geometria { origem, celula } = self.geo;
        let cw = f32::from(celula.width).max(1.);
        let ch = f32::from(celula.height).max(1.);
        let x = (f32::from(pos.x - origem.x) / cw).max(0.);
        let y = (f32::from(pos.y - origem.y) / ch).floor() as i32;
        let col = (x.floor() as usize).min(g.columns() - 1);
        let lado = if x.fract() < 0.5 { Side::Left } else { Side::Right };
        let linha = y.clamp(0, g.screen_lines() as i32 - 1) - g.display_offset() as i32;
        (TPoint::new(Line(linha), Column(col)), lado)
    }

    /// Coordenadas do mouse 1-based na tela, para os relatórios.
    fn celula_tela(&self, pos: gpui_kit::Point<Pixels>) -> (usize, usize) {
        let Geometria { origem, celula } = self.geo;
        let e = self.motor.travar();
        let g = e.term.grid();
        let x = (f32::from(pos.x - origem.x) / f32::from(celula.width).max(1.)).floor().max(0.) as usize;
        let y = (f32::from(pos.y - origem.y) / f32::from(celula.height).max(1.)).floor().max(0.) as usize;
        (x.min(g.columns() - 1) + 1, y.min(g.screen_lines() - 1) + 1)
    }

    fn relata_mouse(&self, shift: bool) -> bool {
        self.escrevivel && !shift && self.modo().intersects(TermMode::MOUSE_MODE)
    }

    /// Relatório do mouse: SGR (1006) ou o formato normal (X10), com modificadores.
    fn relatar(&self, mut codigo: u8, soltou: bool, pos: gpui_kit::Point<Pixels>, m: gpui_kit::Modifiers) {
        let (x, y) = self.celula_tela(pos);
        if m.shift {
            codigo += 4;
        }
        if m.alt {
            codigo += 8;
        }
        if m.control {
            codigo += 16;
        }
        let modo = self.modo();
        let s = if modo.contains(TermMode::SGR_MOUSE) {
            format!("\x1b[<{codigo};{x};{y}{}", if soltou { 'm' } else { 'M' })
        } else {
            let b = if soltou { 3 + (codigo & !3) } else { codigo };
            if x > 223 || y > 223 {
                return;
            }
            let c = |v: u32| char::from_u32(v).unwrap_or(' ');
            format!("\x1b[M{}{}{}", c(32 + b as u32), c(32 + x as u32), c(32 + y as u32))
        };
        self.escrever(&s);
    }

    fn codigo_botao(b: MouseButton) -> Option<u8> {
        match b {
            MouseButton::Left => Some(0),
            MouseButton::Middle => Some(1),
            MouseButton::Right => Some(2),
            _ => None,
        }
    }

    pub(crate) fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.foco, cx);
        self.mouse = e.position;
        if e.button == MouseButton::Left && e.modifiers.control {
            self.ctrl = true;
            self.atualizar_link(cx);
            if let Some(link) = self.link.clone() {
                self.abrir(link.abrir, window, cx);
                return;
            }
        }
        if self.relata_mouse(e.modifiers.shift) {
            if let Some(c) = Self::codigo_botao(e.button) {
                self.botao = Some(e.button);
                self.relatar(c, false, e.position, e.modifiers);
            }
            return;
        }
        if e.button != MouseButton::Left {
            return;
        }
        let (p, lado) = self.celula(e.position);
        let tipo = match e.click_count {
            2 => SelectionType::Semantic,
            n if n >= 3 => SelectionType::Lines,
            _ => SelectionType::Simple,
        };
        {
            let mut est = self.motor.travar();
            est.term.selection = Some(Selection::new(tipo, p, lado));
        }
        self.selecionando = true;
        cx.notify();
    }

    pub(crate) fn mouse_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.mouse = e.position;
        if self.ctrl != e.modifiers.control {
            self.ctrl = e.modifiers.control;
        }
        if self.selecionando {
            let Geometria { origem, celula } = self.geo;
            let (c, l) = self.motor.tamanho();
            let fundo = origem.y + celula.height * l as f32;
            // Arrastar para fora rola o histórico, como no xterm.
            let rolar = if e.position.y < origem.y { 1 } else if e.position.y > fundo { -1 } else { 0 };
            let _ = c;
            let (p, lado) = self.celula(e.position);
            let mut est = self.motor.travar();
            if rolar != 0 {
                est.term.scroll_display(Scroll::Delta(rolar));
            }
            if let Some(sel) = est.term.selection.as_mut() {
                sel.update(p, lado);
            }
            drop(est);
            cx.notify();
            return;
        }
        if self.relata_mouse(e.modifiers.shift) {
            let modo = self.modo();
            match (self.botao, e.pressed_button) {
                (Some(b), Some(_)) if modo.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) => {
                    if let Some(c) = Self::codigo_botao(b) {
                        self.relatar(c + 32, false, e.position, e.modifiers);
                    }
                }
                (_, None) if modo.contains(TermMode::MOUSE_MOTION) => self.relatar(3 + 32, false, e.position, e.modifiers),
                _ => {}
            }
        }
        self.atualizar_link(cx);
    }

    pub(crate) fn mouse_up(&mut self, e: &MouseUpEvent, cx: &mut Context<Self>) {
        if let Some(b) = self.botao.take() {
            if b == e.button {
                if let Some(c) = Self::codigo_botao(b) {
                    self.relatar(c, true, e.position, e.modifiers);
                }
            }
        }
        if self.selecionando {
            self.selecionando = false;
            let mut est = self.motor.travar();
            if est.term.selection.as_ref().is_some_and(|s| s.is_empty()) {
                est.term.selection = None;
            }
            drop(est);
            cx.notify();
        }
    }

    fn rolar(&mut self, e: &ScrollEvent, cx: &mut Context<Self>) {
        let ch = self.geo.celula.height.max(px(1.));
        let delta = e.delta.pixel_delta(ch);
        self.rolagem += f32::from(delta.y) / f32::from(ch);
        let linhas = self.rolagem.trunc() as i32;
        if linhas == 0 {
            return;
        }
        self.rolagem -= linhas as f32;
        let modo = self.modo();
        if self.relata_mouse(e.modifiers.shift) {
            let codigo = if linhas > 0 { 64 } else { 65 };
            for _ in 0..linhas.unsigned_abs() {
                self.relatar(codigo, false, e.position, e.modifiers);
            }
            return;
        }
        if modo.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) && self.escrevivel {
            let app = modo.contains(TermMode::APP_CURSOR);
            let seta = match (linhas > 0, app) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            self.escrever(&seta.repeat(linhas.unsigned_abs() as usize));
            return;
        }
        self.motor.travar().term.scroll_display(Scroll::Delta(linhas));
        self.atualizar_link(cx);
        cx.notify();
    }

    pub(crate) fn modificadores(&mut self, m: gpui_kit::Modifiers, cx: &mut Context<Self>) {
        if self.ctrl != m.control {
            self.ctrl = m.control;
            self.atualizar_link(cx);
            cx.notify();
        }
    }

    // ---------- links ----------

    /// Recalcula o link sob o mouse (só com Ctrl): a linha lógica (as linhas quebradas juntas), os links dela e a existência
    /// do caminho. Hiperlink OSC 8 da célula vale antes.
    fn atualizar_link(&mut self, cx: &mut Context<Self>) {
        let antes = self.link.as_ref().map(|l| (l.inicio, l.fim));
        self.link = if self.ctrl && self.geo.celula.width > px(0.) { self.link_em(self.mouse) } else { None };
        if antes != self.link.as_ref().map(|l| (l.inicio, l.fim)) {
            cx.notify();
        }
    }

    fn link_em(&self, pos: gpui_kit::Point<Pixels>) -> Option<LinkSob> {
        let Geometria { origem, celula } = self.geo;
        let (c, l) = self.motor.tamanho();
        let dentro = pos.x >= origem.x
            && pos.y >= origem.y
            && pos.x < origem.x + celula.width * c as f32
            && pos.y < origem.y + celula.height * l as f32;
        if !dentro {
            return None;
        }
        let (alvo, _) = self.celula(pos);
        let est = self.motor.travar();
        let g = est.term.grid();
        if let Some(h) = g[alvo].hyperlink() {
            let abrir = abrir_uri(h.uri())?;
            return Some(LinkSob { inicio: alvo, fim: alvo, abrir });
        }
        let ultima = g.columns() - 1;
        let topo = -(g.history_size() as i32);
        let fundo = g.screen_lines() as i32 - 1;
        let mut primeira = alvo.line.0;
        while primeira > topo && g[Line(primeira - 1)][Column(ultima)].flags.contains(Flags::WRAPLINE) {
            primeira -= 1;
        }
        let mut final_ = alvo.line.0;
        while final_ < fundo && g[Line(final_)][Column(ultima)].flags.contains(Flags::WRAPLINE) {
            final_ += 1;
        }
        let mut texto = String::new();
        let mut celulas: Vec<(TPoint, usize)> = Vec::new();
        for linha in primeira..=final_ {
            let row = &g[Line(linha)];
            for col in 0..g.columns() {
                let cel = &row[Column(col)];
                if cel.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                let largura = if cel.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
                texto.push(cel.c);
                celulas.push((TPoint::new(Line(linha), Column(col)), largura));
            }
        }
        drop(est);
        let indice = celulas.iter().position(|(p, _)| *p == alvo)?;
        let link = links::achar_links(&texto).into_iter().find(|l| {
            let (a, b) = l.intervalo();
            indice >= a && indice < b
        })?;
        let (a, b) = link.intervalo();
        let inicio = celulas.get(a)?.0;
        let (p_fim, w) = *celulas.get(b - 1)?;
        let fim = TPoint::new(p_fim.line, Column(p_fim.column.0 + w - 1));
        let abrir = match link {
            Link::Url { url, .. } => Abrir::Url(url),
            Link::Caminho { caminho, linha, coluna, .. } => {
                let abs = links::resolver_caminho(&caminho, self.raiz.as_deref())?;
                let tipo = tipo_do_caminho(&abs)?;
                Abrir::Caminho { abs, tipo, linha, coluna }
            }
        };
        Some(LinkSob { inicio, fim, abrir })
    }

    fn abrir(&mut self, abrir: Abrir, window: &mut Window, cx: &mut Context<Self>) {
        let alvo = match abrir {
            Abrir::Url(url) => {
                if let Some(f) = cx.try_global::<AbrirUrl>().and_then(|a| a.0.clone()) {
                    f(&url);
                    return;
                }
                cx.background_spawn(async move {
                    let _ = orquestrador_core::platform::open_url(&url);
                })
                .detach();
                return;
            }
            Abrir::Caminho { abs, tipo, linha, coluna } => {
                let dentro = self.raiz.as_deref().and_then(|r| links::relativo_a(r, &abs));
                match tipo {
                    Tipo::Pasta => match dentro {
                        Some(rel) => Alvo::Pasta(rel),
                        None => return,
                    },
                    Tipo::Arquivo => Alvo::Arquivo { caminho: dentro.unwrap_or(abs), linha, coluna },
                }
            }
        };
        let f = cx.try_global::<AbrirLinks>().and_then(|a| a.0.clone());
        if let Some(f) = f {
            f(&alvo, window, cx);
        }
    }
}

/// OSC 8: URL web abre no navegador; `file://` vira caminho absoluto.
fn abrir_uri(uri: &str) -> Option<Abrir> {
    if links::eh_url_web(uri) {
        return Some(Abrir::Url(uri.to_string()));
    }
    let resto = uri.strip_prefix("file://").or_else(|| uri.strip_prefix("FILE://"))?;
    let resto = resto.strip_prefix("localhost").unwrap_or(resto);
    let resto = resto.strip_prefix('/').unwrap_or(resto);
    let resto = resto.split(['?', '#']).next().unwrap_or("");
    let decodificado = decodificar(resto);
    let abs = links::resolver_caminho(&decodificado, None)?;
    let tipo = tipo_do_caminho(&abs)?;
    Some(Abrir::Caminho { abs, tipo, linha: None, coluna: None })
}

fn decodificar(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl EntityInputHandler for VistaTerminal {
    fn text_for_range(&mut self, _: Range<usize>, _: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        None
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marcado.as_ref().map(|m| 0..m.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marcado = None;
        cx.notify();
    }

    fn replace_text_in_range(&mut self, _: Option<Range<usize>>, texto: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.marcado = None;
        self.digitar(texto, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self, _: Option<Range<usize>>, texto: &str, _: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>,
    ) {
        self.marcado = (!texto.is_empty()).then(|| texto.to_string());
        cx.notify();
    }

    /// A janela do IME aparece junto do cursor.
    fn bounds_for_range(&mut self, _: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let est = self.motor.travar();
        let c = est.term.grid().cursor.point;
        let off = est.term.grid().display_offset() as i32;
        let Geometria { origem, celula } = self.geo;
        let y = (c.line.0 + off) as f32;
        Some(Bounds::new(point(origem.x + celula.width * c.column.0 as f32, origem.y + celula.height * y), celula))
    }

    fn character_index_for_point(&mut self, _: gpui_kit::Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl Render for VistaTerminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pedir_foco && self.visivel {
            self.pedir_foco = false;
            let foco = self.foco.clone();
            window.defer(cx, move |window, cx| window.focus(&foco, cx));
        }
        // 12 px à esquerda: o texto começa na coluna dos cabeçalhos dos painéis (`top-2 right-0.5 bottom-1 left-3`).
        div()
            .id(SharedString::from(format!("term-{}", self.chave.1)))
            .key_context("Terminal")
            .track_focus(&self.foco)
            .on_key_down(cx.listener(Self::tecla))
            .size_full()
            .pt(px(8.))
            .pr(px(2.))
            .pb(px(4.))
            .pl(px(12.))
            .bg(cores::hsla(cores::FUNDO))
            .child(GradeTerminal { vista: cx.entity() })
    }
}

/// O elemento que desenha a grade.
pub struct GradeTerminal {
    vista: Entity<VistaTerminal>,
}

impl IntoElement for GradeTerminal {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct Pintura {
    hitbox: Hitbox,
    fundos: Vec<(Bounds<Pixels>, Hsla)>,
    linhas: Vec<(gpui_kit::Point<Pixels>, ShapedLine)>,
    cursor: Option<(Bounds<Pixels>, CursorShape, bool, Option<(gpui_kit::Point<Pixels>, ShapedLine)>)>,
    marcado: Option<(Bounds<Pixels>, gpui_kit::Point<Pixels>, ShapedLine)>,
    altura: Pixels,
    mouse_relatado: bool,
    mao: bool,
}

/// Medidas da célula: largura do avanço do "m" e a altura do xterm (altura da fonte × 1,15, arredondada para baixo).
pub fn medir_celula(window: &Window) -> (Font, Size<Pixels>) {
    let fonte = fonte_base();
    let ts = window.text_system();
    let id = ts.resolve_font(&fonte);
    let tam = px(TAMANHO_FONTE);
    let largura = ts.advance(id, tam, 'm').map(|s| s.width).unwrap_or(px(7.8));
    let alto = f32::from(ts.ascent(id, tam)) + f32::from(ts.descent(id, tam)).abs();
    let altura = (alto.ceil() * ALTURA_LINHA).floor();
    (fonte, size(largura, px(altura)))
}

fn fonte_base() -> Font {
    Font { fallbacks: Some(FontFallbacks::from_fonts(RESERVAS.iter().map(|s| s.to_string()).collect())), ..font(FONTE) }
}

#[derive(Clone, PartialEq)]
struct Estilo {
    cor: Hsla,
    negrito: bool,
    italico: bool,
    sublinhado: Option<(Hsla, bool)>,
    riscado: bool,
}

struct Lote {
    coluna: usize,
    texto: String,
    runs: Vec<TextRun>,
    ultimo: Option<Estilo>,
}

impl Lote {
    fn novo(coluna: usize) -> Lote {
        Lote { coluna, texto: String::new(), runs: Vec::new(), ultimo: None }
    }

    fn empurrar(&mut self, s: &str, estilo: &Estilo, fonte: &Font) {
        if self.ultimo.as_ref() == Some(estilo) {
            if let Some(r) = self.runs.last_mut() {
                r.len += s.len();
            }
        } else {
            let mut f = fonte.clone();
            if estilo.negrito {
                f.weight = FontWeight::BOLD;
            }
            if estilo.italico {
                f.style = FontStyle::Italic;
            }
            self.runs.push(TextRun {
                len: s.len(),
                font: f,
                color: estilo.cor,
                background_color: None,
                underline: estilo.sublinhado.map(|(cor, ondulado)| UnderlineStyle { thickness: px(1.), color: Some(cor), wavy: ondulado }),
                strikethrough: estilo.riscado.then(|| StrikethroughStyle { thickness: px(1.), color: Some(estilo.cor) }),
            });
            self.ultimo = Some(estilo.clone());
        }
        self.texto.push_str(s);
    }
}

impl Element for GradeTerminal {
    type RequestLayoutState = ();
    type PrepaintState = Pintura;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut estilo = Style::default();
        estilo.size.width = gpui_kit::relative(1.).into();
        estilo.size.height = gpui_kit::relative(1.).into();
        (window.request_layout(estilo, [], cx), ())
    }

    fn prepaint(
        &mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (),
        window: &mut Window, cx: &mut App,
    ) -> Pintura {
        let (fonte, celula) = medir_celula(window);
        let colunas = ((f32::from(bounds.size.width) / f32::from(celula.width)).floor() as u16).max(2);
        let linhas = ((f32::from(bounds.size.height) / f32::from(celula.height)).floor() as u16).max(1);
        let focado_agora = self.vista.read(cx).foco.is_focused(window);
        let mudou = self.vista.update(cx, |v, cx| {
            v.geo = Geometria { origem: bounds.origin, celula };
            v.pinturas += 1;
            let mudou = v.motor.redimensionar(colunas, linhas);
            if mudou {
                v.mandar_tamanho(cx);
            }
            if !v.medido {
                v.medido = true;
                cx.emit(EventoVista::Medido);
            }
            // Foco no terminal (modo 1004): o programa recebe `CSI I`/`CSI O`.
            if v.focado != focado_agora {
                v.focado = focado_agora;
                v.cursor_aceso = true;
                if v.escrevivel && v.modo().contains(TermMode::FOCUS_IN_OUT) {
                    v.escrever(if focado_agora { "\x1b[I" } else { "\x1b[O" });
                }
            }
            mudou
        });
        let _ = mudou;
        let v = self.vista.read(cx);
        let (link, ctrl, aceso, escrevivel, marcado) = (v.link.clone(), v.ctrl, v.cursor_aceso, v.escrevivel, v.marcado.clone());
        // Sem PTY (o diálogo do orquestrador ainda aberto) não há cursor, como o xterm que o React só monta depois.
        let vivo = v.vivo;
        let motor = v.motor.clone();
        let tecla_em = v.tecla_em;
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        motor.limpar();
        // Tecla → eco no quadro: a primeira preparação de desenho com saída nova depois da tecla.
        if let Some((t0, g0)) = tecla_em {
            if motor.assinatura_cursor() != g0 {
                let p = *motor.processado_em.lock().unwrap_or_else(|e| e.into_inner());
                medir::amostra(t0.elapsed(), p.map(|p| p.saturating_duration_since(t0)));
                self.vista.update(cx, |v, _| v.tecla_em = None);
            }
        }
        let est = motor.travar();
        let term = &est.term;
        let g = term.grid();
        let off = g.display_offset() as i32;
        let cols = g.columns();
        let trocadas = term.colors();
        let selecao = term.selection.as_ref().and_then(|s| s.to_range(term));
        let fundo_padrao = cores::hsla(cores::FUNDO);
        let cor_selecao = cores::hsla(if focado_agora { cores::SELECAO } else { cores::SELECAO_INATIVA });
        let modo = *term.mode();
        let mut fundos: Vec<(Bounds<Pixels>, Hsla)> = Vec::new();
        let mut lotes: Vec<(usize, Lote)> = Vec::new();
        let x_de = |c: usize| bounds.origin.x + celula.width * c as f32;
        let y_de = |r: usize| bounds.origin.y + celula.height * r as f32;
        let mut buf = [0u8; 4];

        for r in 0..g.screen_lines() {
            let linha = Line(r as i32 - off);
            let row = &g[linha];
            let mut lote = Lote::novo(0);
            let mut fundo_atual: Option<(usize, Hsla)> = None;
            let fechar_fundo = |fundo_atual: &mut Option<(usize, Hsla)>, ate: usize, fundos: &mut Vec<_>| {
                if let Some((de, cor)) = fundo_atual.take() {
                    fundos.push((Bounds::new(point(x_de(de), y_de(r)), size(celula.width * (ate - de) as f32, celula.height)), cor));
                }
            };
            let mut c = 0;
            while c < cols {
                let cel = &row[Column(c)];
                if cel.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) && c > 0 && lote.texto.is_empty() {
                    c += 1;
                    lote.coluna = c;
                    continue;
                }
                let p = TPoint::new(linha, Column(c));
                let f = cel.flags;
                let negrito = f.contains(Flags::BOLD);
                let mut fg = cores::resolver(cel.fg, trocadas, negrito);
                let mut bg = cores::resolver(cel.bg, trocadas, false);
                if f.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let mut cor = cores::hsla(fg);
                if f.contains(Flags::DIM) {
                    cor.a *= 0.5;
                }
                if f.contains(Flags::HIDDEN) {
                    cor = cores::hsla(bg);
                }
                let mut cor_fundo = (bg != cores::FUNDO || f.contains(Flags::INVERSE)).then(|| cores::hsla(bg));
                if selecao.is_some_and(|s| s.contains(p)) {
                    cor_fundo = Some(cor_selecao);
                }
                let largura = if f.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
                // Fundo: retângulos contínuos da mesma cor.
                match (cor_fundo, fundo_atual) {
                    (Some(b), Some((_, a))) if a == b => {}
                    (Some(b), _) => {
                        fechar_fundo(&mut fundo_atual, c, &mut fundos);
                        fundo_atual = Some((c, b));
                    }
                    (None, _) => fechar_fundo(&mut fundo_atual, c, &mut fundos),
                }
                let no_link = ctrl
                    && link.as_ref().is_some_and(|l| {
                        let dentro_ini = p.line > l.inicio.line || (p.line == l.inicio.line && p.column >= l.inicio.column);
                        let dentro_fim = p.line < l.fim.line || (p.line == l.fim.line && p.column <= l.fim.column);
                        dentro_ini && dentro_fim
                    });
                let sublinhado = if no_link {
                    Some((cor, false))
                } else if f.intersects(Flags::ALL_UNDERLINES) {
                    let cu = cel.underline_color().map(|u| cores::hsla(cores::resolver(u, trocadas, false))).unwrap_or(cor);
                    Some((cu, f.contains(Flags::UNDERCURL)))
                } else {
                    None
                };
                let estilo = Estilo { cor, negrito, italico: f.contains(Flags::ITALIC), sublinhado, riscado: f.contains(Flags::STRIKEOUT) };
                let ch = if cel.c == '\0' || cel.c == '\t' { ' ' } else { cel.c };
                let simples = ch == ' ' && estilo.sublinhado.is_none() && !estilo.riscado;
                if largura == 2 {
                    // Caractere largo em lote próprio: a largura forçada vale por glifo.
                    if !lote.texto.is_empty() {
                        lotes.push((r, std::mem::replace(&mut lote, Lote::novo(c))));
                    }
                    let mut largo = Lote::novo(c);
                    largo.empurrar(ch.encode_utf8(&mut buf), &estilo, &fonte);
                    lotes.push((r, largo));
                    c += 2;
                    lote = Lote::novo(c);
                    continue;
                }
                if simples && lote.texto.is_empty() {
                    c += 1;
                    lote.coluna = c;
                    continue;
                }
                lote.empurrar(ch.encode_utf8(&mut buf), &estilo, &fonte);
                if let Some(zw) = cel.zerowidth() {
                    for z in zw {
                        lote.empurrar(z.encode_utf8(&mut buf), &estilo, &fonte);
                    }
                }
                c += 1;
            }
            fechar_fundo(&mut fundo_atual, cols, &mut fundos);
            // Espaços no fim do lote não precisam de forma.
            let tamanho = lote.texto.trim_end_matches(' ').len();
            if tamanho > 0 {
                let corte = lote.texto.len() - tamanho;
                lote.texto.truncate(tamanho);
                let mut sobra = corte;
                while sobra > 0 {
                    let Some(r) = lote.runs.last_mut() else { break };
                    let tira = sobra.min(r.len);
                    r.len -= tira;
                    sobra -= tira;
                    if r.len == 0 {
                        lote.runs.pop();
                    }
                }
                lotes.push((r, lote));
            }
        }

        // Cursor: forma do DECSCUSR, piscando como o xterm; sem foco, contorno.
        let rc = term.renderable_content().cursor;
        let ponto = rc.point;
        let linha_tela = ponto.line.0 + off;
        let cursor = if rc.shape != CursorShape::Hidden && linha_tela >= 0 && (linha_tela as usize) < g.screen_lines() && escrevivel && vivo {
            let largo = g[ponto].flags.contains(Flags::WIDE_CHAR);
            let caixa = Bounds::new(
                point(x_de(ponto.column.0), y_de(linha_tela as usize)),
                size(celula.width * if largo { 2. } else { 1. }, celula.height),
            );
            let mostrar = aceso || !focado_agora || !term.cursor_style().blinking;
            let ch = g[ponto].c;
            let texto = if focado_agora && rc.shape == CursorShape::Block && ch != ' ' && ch != '\0' {
                let s: SharedString = ch.to_string().into();
                let run = TextRun { len: s.len(), font: fonte.clone(), color: cores::hsla(cores::CURSOR_TEXTO), background_color: None, underline: None, strikethrough: None };
                Some((caixa.origin, window.text_system().shape_line(s, px(TAMANHO_FONTE), &[run], Some(celula.width))))
            } else {
                None
            };
            mostrar.then_some((caixa, rc.shape, focado_agora, texto))
        } else {
            None
        };
        let marcado = marcado.map(|m| {
            let caixa_y = y_de(linha_tela.max(0) as usize);
            let origem = point(x_de(ponto.column.0), caixa_y);
            let s: SharedString = m.into();
            let run = TextRun {
                len: s.len(),
                font: fonte.clone(),
                color: cores::hsla(cores::TEXTO),
                background_color: None,
                underline: Some(UnderlineStyle { thickness: px(1.), color: None, wavy: false }),
                strikethrough: None,
            };
            let linha = window.text_system().shape_line(s, px(TAMANHO_FONTE), &[run], None);
            (Bounds::new(origem, size(linha.width(), celula.height)), origem, linha)
        });
        drop(est);

        let linhas = lotes
            .into_iter()
            .map(|(r, lote)| {
                let origem = point(x_de(lote.coluna), y_de(r));
                (origem, window.text_system().shape_line(lote.texto.into(), px(TAMANHO_FONTE), &lote.runs, Some(celula.width)))
            })
            .collect();
        let _ = fundo_padrao;
        Pintura {
            hitbox,
            fundos,
            linhas,
            cursor,
            marcado,
            altura: celula.height,
            mouse_relatado: escrevivel && modo.intersects(TermMode::MOUSE_MODE),
            mao: ctrl && link.is_some(),
        }
    }

    fn paint(
        &mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (),
        p: &mut Pintura, window: &mut Window, cx: &mut App,
    ) {
        let estilo_cursor = if p.mao {
            gpui_kit::CursorStyle::PointingHand
        } else if p.mouse_relatado {
            gpui_kit::CursorStyle::Arrow
        } else {
            gpui_kit::CursorStyle::IBeam
        };
        window.set_cursor_style(estilo_cursor, &p.hitbox);
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            for (caixa, cor) in &p.fundos {
                window.paint_quad(fill(*caixa, *cor));
            }
            for (origem, linha) in &p.linhas {
                let _ = linha.paint(*origem, p.altura, TextAlign::Left, None, window, cx);
            }
            if let Some((caixa, forma, focado, texto)) = &p.cursor {
                let cor = cores::hsla(cores::CURSOR);
                match (forma, focado) {
                    (CursorShape::Block, true) => {
                        window.paint_quad(fill(*caixa, cor));
                        if let Some((origem, linha)) = texto {
                            let _ = linha.paint(*origem, p.altura, TextAlign::Left, None, window, cx);
                        }
                    }
                    (CursorShape::Beam, _) => {
                        window.paint_quad(fill(Bounds::new(caixa.origin, size(px(1.), caixa.size.height)), cor));
                    }
                    (CursorShape::Underline, _) => {
                        let y = caixa.origin.y + caixa.size.height - px(1.);
                        window.paint_quad(fill(Bounds::new(point(caixa.origin.x, y), size(caixa.size.width, px(1.))), cor));
                    }
                    _ => window.paint_quad(outline(*caixa, cor, BorderStyle::Solid)),
                }
            }
            if let Some((caixa, origem, linha)) = &p.marcado {
                window.paint_quad(fill(*caixa, cores::hsla(cores::FUNDO)));
                let _ = linha.paint(*origem, p.altura, TextAlign::Left, None, window, cx);
            }
        });

        let vista = self.vista.clone();
        let foco = vista.read(cx).foco.clone();
        window.handle_input(&foco, ElementInputHandler::new(bounds, vista.clone()), cx);

        let hitbox = p.hitbox.clone();
        let v = vista.clone();
        window.on_mouse_event(move |e: &MouseDownEvent, fase, window, cx| {
            if fase.bubble() && hitbox.is_hovered(window) {
                cx.stop_propagation();
                v.update(cx, |v, cx| v.mouse_down(e, window, cx));
            }
        });
        let hitbox = p.hitbox.clone();
        let v = vista.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, fase, window, cx| {
            if !fase.bubble() {
                return;
            }
            let ativo = {
                let r = v.read(cx);
                r.selecionando || r.botao.is_some()
            };
            if ativo || hitbox.is_hovered(window) {
                v.update(cx, |v, cx| v.mouse_move(e, cx));
            } else if v.read(cx).link.is_some() {
                v.update(cx, |v, cx| {
                    v.link = None;
                    cx.notify();
                });
            }
        });
        let v = vista.clone();
        window.on_mouse_event(move |e: &MouseUpEvent, fase, _, cx| {
            if fase.bubble() {
                v.update(cx, |v, cx| v.mouse_up(e, cx));
            }
        });
        let hitbox = p.hitbox.clone();
        let v = vista.clone();
        window.on_mouse_event(move |e: &ScrollEvent, fase, window, cx| {
            if fase.bubble() && hitbox.is_hovered(window) {
                cx.stop_propagation();
                v.update(cx, |v, cx| v.rolar(e, cx));
            }
        });
        let v = vista;
        window.on_key_event(move |e: &ModifiersChangedEvent, fase, _, cx| {
            if fase.bubble() {
                v.update(cx, |v, cx| v.modificadores(e.modifiers, cx));
            }
        });
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn uri_file_vira_caminho_e_web_vira_url() {
        assert!(matches!(abrir_uri("https://example.com"), Some(Abrir::Url(u)) if u == "https://example.com"));
        let dir = std::env::temp_dir();
        let uri = format!("file:///{}", dir.display().to_string().replace('\\', "/").replace(' ', "%20"));
        assert!(matches!(abrir_uri(&uri), Some(Abrir::Caminho { tipo: Tipo::Pasta, .. })));
        assert!(abrir_uri("ftp://x").is_none());
    }

    #[test]
    fn decodifica_porcento() {
        assert_eq!(decodificar("a%20b%C3%A7"), "a bç");
        assert_eq!(decodificar("100%"), "100%");
    }
}
