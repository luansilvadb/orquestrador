//! Abertura rápida (Ctrl+P), porte de `src/editor/quick-open.tsx`: um campo e a lista de arquivos do projeto que casam
//! com o termo (`search::quick_open` do core). Setas escolhem, Enter abre como prévia, Shift+Enter abre de vez, Ctrl+Enter
//! abre à direita e Esc fecha.
use gpui::{
    anchored, div, prelude::*, px, App, Context, EventEmitter, KeyDownEvent, MouseButton, ScrollHandle,
    SharedString, Subscription, Task, Window,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::Sizable;
use orquestrador_core::ide::search;
use orquestrador_ui::icone_arquivo;
use orquestrador_ui::tema::{self, cor};
use std::path::PathBuf;
use std::time::Duration;

/// Espera a digitação parar antes de consultar.
const ESPERA: Duration = Duration::from_millis(120);

#[derive(Clone, Debug, PartialEq)]
pub enum Evento {
    Abrir { caminho: String, permanente: bool, lado: bool },
    Fechar,
}

/// Resposta do core e o texto que a produziu.
struct Achado {
    consulta: String,
    caminhos: Vec<String>,
    total: usize,
}

pub struct AbrirRapido {
    raiz: PathBuf,
    campo: gpui::Entity<InputState>,
    achado: Option<Achado>,
    erro: Option<String>,
    indice: usize,
    seq: u64,
    rolagem: ScrollHandle,
    _tarefa: Option<Task<()>>,
    _sub: Subscription,
}

impl EventEmitter<Evento> for AbrirRapido {}

impl AbrirRapido {
    pub fn new(raiz: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let campo = cx.new(|cx| InputState::new(window, cx).placeholder("Nome ou caminho do arquivo"));
        let sub = cx.subscribe_in(&campo, window, |a, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                a.consultar(cx);
            }
        });
        AbrirRapido { raiz, campo, achado: None, erro: None, indice: 0, seq: 0, rolagem: ScrollHandle::new(), _tarefa: None, _sub: sub }
    }

    pub fn focar(&self, window: &mut Window, cx: &mut App) {
        self.campo.update(cx, |s, cx| s.focus(window, cx));
    }

    pub fn campo(&self) -> gpui::Entity<InputState> {
        self.campo.clone()
    }

    /// Define o termo (vitrine e testes).
    pub fn definir_termo(&mut self, termo: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.campo.update(cx, |s, cx| s.set_value(termo.to_string(), window, cx));
        self.consultar(cx);
    }

    fn termo(&self, cx: &App) -> String {
        self.campo.read(cx).value().trim().to_string()
    }

    fn consultar(&mut self, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        self.erro = None;
        let texto = self.termo(cx);
        if texto.is_empty() {
            self.achado = None;
            self._tarefa = None;
            cx.notify();
            return;
        }
        let raiz = self.raiz.clone();
        self._tarefa = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ESPERA).await;
            let t = texto.clone();
            let r = cx.background_spawn(async move { search::quick_open(&raiz, &t) }).await;
            let _ = this.update(cx, |a, cx| {
                if seq != a.seq {
                    return;
                }
                match r {
                    Ok(q) => {
                        a.achado = Some(Achado { consulta: texto, caminhos: q.paths, total: q.total });
                        a.indice = 0;
                    }
                    Err(e) => {
                        a.achado = None;
                        a.erro = Some(e);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn abrir(&mut self, i: usize, permanente: bool, lado: bool, cx: &mut Context<Self>) {
        if let Some(c) = self.achado.as_ref().and_then(|a| a.caminhos.get(i)) {
            cx.emit(Evento::Abrir { caminho: c.clone(), permanente, lado });
        }
    }

    fn tecla(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let n = self.achado.as_ref().map_or(0, |a| a.caminhos.len());
        match k.key.as_str() {
            "escape" => cx.emit(Evento::Fechar),
            "down" | "up" => {
                if n > 0 {
                    self.indice = if k.key == "down" { (self.indice + 1) % n } else { (self.indice + n - 1) % n };
                    self.rolagem.scroll_to_item(self.indice);
                }
            }
            "enter" => {
                // Texto novo ainda sem resposta: abrir aqui seria abrir o arquivo do texto anterior.
                let termo = self.termo(cx);
                if self.achado.as_ref().is_some_and(|a| a.consulta == termo) {
                    let i = self.indice;
                    if k.modifiers.control {
                        self.abrir(i, true, true, cx);
                    } else if k.modifiers.shift {
                        self.abrir(i, true, false, cx);
                    } else {
                        self.abrir(i, false, false, cx);
                    }
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for AbrirRapido {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let termo = self.termo(cx);
        let novo = self.achado.as_ref().is_some_and(|a| a.consulta == termo);
        let pendente = !termo.is_empty() && !novo;
        let total = if novo { self.achado.as_ref().map_or(0, |a| a.total) } else { 0 };
        let caminhos = self.achado.as_ref().map(|a| a.caminhos.clone()).unwrap_or_default();
        let resumo: SharedString = if let Some(e) = &self.erro {
            e.clone().into()
        } else if termo.is_empty() {
            "Digite parte do nome ou do caminho. Enter abre como prévia; Shift+Enter, de vez; Ctrl+Enter, à direita.".into()
        } else if pendente {
            "Procurando…".into()
        } else if total == 0 {
            "Nenhum arquivo casa com o termo.".into()
        } else {
            let mostrando = if total > caminhos.len() { format!(", mostrando {}", caminhos.len()) } else { String::new() };
            format!("{total} {}{mostrando}.", if total == 1 { "arquivo" } else { "arquivos" }).into()
        };
        let tela = window.viewport_size();
        let mut lista = div().id("abrir-rapido-lista").flex().flex_col().min_h_0().overflow_y_scroll().track_scroll(&self.rolagem);
        for (i, c) in caminhos.iter().enumerate() {
            let (nome, pasta) = crate::caminhos::separar(c);
            let sel = i == self.indice;
            lista = lista.child(
                div()
                    .id(SharedString::from(format!("rapido-{i}")))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(tema::RAIO_XS)
                    .cursor_pointer()
                    .text_size(tema::TEXTO_BODY)
                    .when(sel, |d| d.bg(gpui::rgb(0x1b1b1b)))
                    .on_hover(cx.listener(move |a, dentro: &bool, _, cx| {
                        if *dentro && a.indice != i {
                            a.indice = i;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |a, ev: &gpui::ClickEvent, _, cx| a.abrir(i, ev.click_count() >= 2, false, cx)))
                    .child(gpui::img(icone_arquivo(c, false, false)).size(px(16.)).flex_none())
                    .child(div().flex_none().font_weight(gpui::FontWeight::MEDIUM).child(nome.to_string()))
                    .when(!pasta.is_empty(), |d| d.child(div().min_w_0().truncate().text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child(pasta.to_string()))),
            );
        }
        let campo = self.campo.clone();
        anchored().position(gpui::point(px(0.), px(0.))).child(
            div()
                .id("abrir-rapido")
                .occlude()
                .w(tela.width)
                .h(tela.height)
                .flex()
                .justify_center()
                .items_start()
                .px(px(16.))
                .pt(tela.height * 0.12)
                .bg(gpui::rgba(0x0000004d))
                .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| cx.emit(Evento::Fechar)))
                .child(
                    div()
                        .id("abrir-rapido-cartao")
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .capture_key_down(cx.listener(|a, ev: &KeyDownEvent, _, cx| a.tecla(ev, cx)))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .w_full()
                        .max_w(px(576.))
                        .max_h(tela.height * 0.7)
                        .p(px(12.))
                        .rounded(tema::RAIO_MD)
                        .border_1()
                        .border_color(cor::hairline())
                        .bg(cor::panel_black())
                        .font_family(tema::FONTE)
                        .text_color(cor::text_primary())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .h(px(40.))
                                .px(px(1.))
                                .rounded(tema::RAIO_MD)
                                .bg(gpui::rgb(0x1b1b1b))
                                .text_size(tema::TEXTO_TITLE)
                                .child(Input::new(&campo).appearance(false).large()),
                        )
                        .child(div().text_size(tema::TEXTO_LABEL).text_color(if self.erro.is_some() { cor::signal_error() } else { cor::text_secondary() }).child(resumo))
                        .child(lista),
                ),
        )
    }
}
