//! A View da aba de mídia. Imagem: `Ajustar` (cabe no painel sem ampliar) ou `Tamanho real` (pixels naturais, com rolagem).
//! Áudio e vídeo: controles no desenho dos nativos do Chromium que o `<audio controls>`/`<video controls>` mostrava (tocar,
//! tempo, barra, volume e mudo, tela cheia no vídeo, menu com download e velocidade). Não toca sozinho e pausa ao sair da
//! vista (`definir_visivel(false)`, janela minimizada) e ao fechar (soltar a entidade para o decodificador e o áudio).
use crate::imagem::{self, FalhaImagem};
use crate::pip::{self, AreaPip, JanelaPip};
use crate::player::{Estado, Player};
use crate::tipos::{self, formatar_tempo, TipoMidia};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::Sizable;
use gpui_kit::{
    actions, anchored, canvas, deferred, div, img, point, prelude::*, px, rgb, rgba, Anchor, App, Bounds, Context,
    EventEmitter, FocusHandle, Focusable, FontWeight, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit,
    Pixels, RenderImage, SharedString, Task, Window, WindowHandle, AnyWindowHandle,
};
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::icone;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

actions!(midia, [AlternarReproducao, Voltar, Avancar, AumentarVolume, DiminuirVolume, AlternarMudo, AlternarTelaCheia, SairTelaCheia]);

/// Velocidades do menu, as do Chromium.
pub const VELOCIDADES: &[f64] = &[0.25, 0.5, 0.75, 1., 1.25, 1.5, 1.75, 2.];

const CONTEXTO: &str = "VisaoMidia";
/// Os controles do vídeo somem depois deste tempo sem mexer o ponteiro, tocando.
const CONTROLES_SOMEM: Duration = Duration::from_millis(2500);

/// Eventos para a bancada.
#[derive(Clone, Debug, PartialEq)]
pub enum EventoMidia {
    /// Leitura, decodificação ou abertura falhou; a aba mostra a mensagem.
    Falhou(String),
    /// Começou ou parou de tocar.
    Reproducao { tocando: bool },
    /// "Voltar para a guia" na janela de picture-in-picture: a bancada ativa esta aba.
    VoltarParaAba,
}

enum Corpo {
    NaoMidia,
    Imagem(Imagem),
    Player(Box<Reproducao>),
}

enum FonteImagem {
    Lendo,
    Erro(SharedString),
    Pronta { imagem: Arc<RenderImage>, largura: u32, altura: u32 },
    NaoExibe,
}

struct Imagem {
    fonte: FonteImagem,
    real: bool,
    _carga: Task<()>,
}

struct Reproducao {
    video: bool,
    player: Player,
    estado: Estado,
    quadro: Option<Arc<RenderImage>>,
    /// Texturas trocadas no último passo; saem do atlas no passo seguinte, depois de o quadro novo ter sido desenhado.
    descartar: Vec<Arc<RenderImage>>,
    sem_quadro: bool,
    barra: Rc<Cell<Bounds<Pixels>>>,
    barra_volume: Rc<Cell<Bounds<Pixels>>>,
    /// Área do vídeo na última pintura e a escala da janela, para o tamanho de saída do swscale.
    area: Rc<Cell<(Bounds<Pixels>, f32)>>,
    refazer_quadro: bool,
    arrasto: Option<Arrasto>,
    volume_aberto: bool,
    ultimo_movimento: Instant,
    tela_cheia: bool,
    pip: Option<WindowHandle<JanelaPip>>,
    area_pip: AreaPip,
    _relogio: Task<()>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrasto {
    Posicao { tocava: bool },
    Volume,
}

pub struct VisaoMidia {
    caminho: PathBuf,
    nome: SharedString,
    tipo: Option<TipoMidia>,
    corpo: Corpo,
    foco: FocusHandle,
    visivel: bool,
    janela: AnyWindowHandle,
    _assinaturas: Vec<gpui_kit::Subscription>,
}

impl EventEmitter<EventoMidia> for VisaoMidia {}

impl Focusable for VisaoMidia {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

/// Atalhos do controle com foco: espaço/K tocam e pausam, setas buscam 5 s e mudam o volume, M silencia, F alterna a tela cheia.
pub fn registrar_atalhos(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", AlternarReproducao, Some(CONTEXTO)),
        KeyBinding::new("k", AlternarReproducao, Some(CONTEXTO)),
        KeyBinding::new("left", Voltar, Some(CONTEXTO)),
        KeyBinding::new("right", Avancar, Some(CONTEXTO)),
        KeyBinding::new("up", AumentarVolume, Some(CONTEXTO)),
        KeyBinding::new("down", DiminuirVolume, Some(CONTEXTO)),
        KeyBinding::new("m", AlternarMudo, Some(CONTEXTO)),
        KeyBinding::new("f", AlternarTelaCheia, Some(CONTEXTO)),
        KeyBinding::new("escape", SairTelaCheia, Some(CONTEXTO)),
    ]);
}

impl VisaoMidia {
    /// `caminho` absoluto do arquivo; o tipo vem de `tipos::midia_de`. Fora de mídia, a view fica vazia.
    pub fn new(caminho: impl Into<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut v = VisaoMidia {
            caminho: PathBuf::new(),
            nome: SharedString::default(),
            tipo: None,
            corpo: Corpo::NaoMidia,
            foco: cx.focus_handle(),
            visivel: true,
            janela: window.window_handle(),
            _assinaturas: Vec::new(),
        };
        // Minimizar a janela pausa, como o `visibilitychange` do WebView2.
        v._assinaturas.push(cx.observe_window_bounds(window, |v, window, cx| v.conferir_janela(window, cx)));
        v._assinaturas.push(cx.observe_window_activation(window, |v, window, cx| v.conferir_janela(window, cx)));
        // Fechar a aba fecha a janela de picture-in-picture.
        v._assinaturas.push(cx.on_release(|v, cx| v.fechar_janela_pip(cx)));
        v.carregar(caminho.into(), cx);
        v
    }

    pub fn foco(&self) -> FocusHandle {
        self.foco.clone()
    }

    pub fn caminho(&self) -> &Path {
        &self.caminho
    }

    pub fn tipo(&self) -> Option<TipoMidia> {
        self.tipo
    }

    /// Renomeação com a aba aberta: recarrega do caminho novo.
    pub fn trocar_caminho(&mut self, caminho: impl Into<PathBuf>, cx: &mut Context<Self>) {
        self.carregar(caminho.into(), cx);
    }

    /// A bancada avisa quando a aba sai ou volta à vista (troca de aba, grupo oculto). Sair pausa; voltar não retoma.
    pub fn definir_visivel(&mut self, visivel: bool, cx: &mut Context<Self>) {
        self.visivel = visivel;
        // Em picture-in-picture o vídeo continua tocando fora da aba, como no Chromium.
        if !visivel && !self.pip_aberta() {
            self.pausar(cx);
        }
    }

    // Consultas para testes e para a bancada.

    pub fn tocando(&self) -> bool {
        matches!(&self.corpo, Corpo::Player(r) if r.estado.tocando)
    }

    pub fn estado_reproducao(&self) -> Option<Estado> {
        match &self.corpo {
            Corpo::Player(r) => Some(r.player.estado()),
            _ => None,
        }
    }

    /// Tamanho natural da imagem ou do vídeo, quando conhecido.
    pub fn tamanho(&self) -> Option<(u32, u32)> {
        match &self.corpo {
            Corpo::Imagem(Imagem { fonte: FonteImagem::Pronta { largura, altura, .. }, .. }) => Some((*largura, *altura)),
            Corpo::Player(r) => r.estado.info.video,
            _ => None,
        }
    }

    /// Texto do status da faixa (`32 × 32 px`, `Imagem`, `Vídeo`); `None` sem faixa.
    pub fn status(&self) -> Option<String> {
        let px = |(w, h): (u32, u32)| format!("{w} × {h} px");
        match &self.corpo {
            Corpo::Imagem(i) if !matches!(i.fonte, FonteImagem::Lendo | FonteImagem::Erro(_)) => {
                Some(self.tamanho().map(px).unwrap_or_else(|| "Imagem".into()))
            }
            Corpo::Player(r) if r.video => Some(r.estado.info.video.map(px).unwrap_or_else(|| "Vídeo".into())),
            _ => None,
        }
    }

    /// Aviso ou erro mostrado na aba, como o texto do `Note` ou do `role="alert"`.
    pub fn aviso(&self) -> Option<String> {
        match &self.corpo {
            Corpo::Imagem(i) => match &i.fonte {
                FonteImagem::Lendo => Some("Lendo o arquivo…".into()),
                FonteImagem::Erro(e) => Some(e.to_string()),
                FonteImagem::NaoExibe => Some("Não foi possível exibir esta imagem.".into()),
                FonteImagem::Pronta { .. } => None,
            },
            Corpo::Player(r) => r.estado.info.erro.as_ref().map(|_| mensagem_erro(r.video)),
            Corpo::NaoMidia => None,
        }
    }

    pub fn tamanho_real(&self) -> bool {
        matches!(&self.corpo, Corpo::Imagem(i) if i.real)
    }

    pub fn tela_cheia(&self) -> bool {
        matches!(&self.corpo, Corpo::Player(r) if r.tela_cheia)
    }

    pub fn pip_aberta(&self) -> bool {
        matches!(&self.corpo, Corpo::Player(r) if r.pip.is_some())
    }

    /// A janela de picture-in-picture, se aberta.
    pub fn janela_pip(&self) -> Option<WindowHandle<JanelaPip>> {
        match &self.corpo {
            Corpo::Player(r) => r.pip,
            _ => None,
        }
    }

    pub(crate) fn quadro_atual(&self) -> Option<Arc<RenderImage>> {
        match &self.corpo {
            Corpo::Player(r) => r.quadro.clone(),
            _ => None,
        }
    }

    // Ações.

    /// "Picture-in-picture" do menu: abre a janela flutuante com o mesmo player; com ela aberta, volta para a aba.
    pub fn alternar_pip(&mut self, cx: &mut Context<Self>) {
        if self.pip_aberta() {
            self.voltar_da_pip(cx);
        } else {
            self.abrir_pip(cx);
        }
    }

    pub fn abrir_pip(&mut self, cx: &mut Context<Self>) {
        let Corpo::Player(r) = &mut self.corpo else { return };
        if !r.video || r.pip.is_some() || r.estado.info.erro.is_some() || !r.estado.info.pronto {
            return;
        }
        if r.tela_cheia {
            r.tela_cheia = false;
            let _ = self.janela.update(cx, |_, window, _| {
                if window.is_fullscreen() {
                    window.toggle_fullscreen();
                }
            });
        }
        let Corpo::Player(r) = &mut self.corpo else { return };
        let opcoes = pip::opcoes(r.estado.info.video.unwrap_or((16, 9)), cx);
        let area = r.area_pip.clone();
        let visao = cx.entity().downgrade();
        // Adiado: a janela nova desenha na hora e lê esta View, que está emprestada agora.
        cx.defer(move |cx| {
            let v = visao.clone();
            let aberta = cx.open_window(opcoes, move |window, cx| cx.new(|cx| JanelaPip::new(v, area, window, cx)));
            let _ = visao.update(cx, |v, cx| match aberta {
                Ok(h) => match &mut v.corpo {
                    Corpo::Player(r) if r.pip.is_none() => {
                        r.pip = Some(h);
                        r.refazer_quadro = true;
                        cx.notify();
                    }
                    _ => {
                        let _ = h.update(cx, |_, window, _| window.remove_window());
                    }
                },
                Err(e) => cx.emit(EventoMidia::Falhou(format!("Não foi possível abrir o picture-in-picture: {e}"))),
            });
        });
    }

    /// "Voltar para a guia": fecha a janela sem pausar e devolve o vídeo e o foco à aba.
    pub fn voltar_da_pip(&mut self, cx: &mut Context<Self>) {
        if !self.pip_aberta() {
            return;
        }
        self.fechar_janela_pip(cx);
        let _ = self.janela.update(cx, |_, window, _| window.activate_window());
        cx.emit(EventoMidia::VoltarParaAba);
        cx.notify();
    }

    /// O "×" da janela: fecha e pausa, como o Chromium.
    pub fn fechar_pip(&mut self, cx: &mut Context<Self>) {
        if self.pip_aberta() {
            self.fechar_janela_pip(cx);
            self.pausar(cx);
        }
    }

    /// A janela sumiu por fora (Alt+F4 ou o sistema): o mesmo que o "×".
    pub(crate) fn pip_fechada(&mut self, cx: &mut Context<Self>) {
        if let Corpo::Player(r) = &mut self.corpo {
            if r.pip.take().is_some() {
                r.refazer_quadro = true;
                self.pausar(cx);
            }
        }
    }

    fn fechar_janela_pip(&mut self, cx: &mut App) {
        let Corpo::Player(r) = &mut self.corpo else { return };
        let Some(h) = r.pip.take() else { return };
        r.refazer_quadro = true;
        // Adiado: o clique pode ter vindo da própria janela, que está emprestada durante o evento.
        cx.defer(move |cx| {
            let _ = h.update(cx, |_, window, _| window.remove_window());
        });
    }

    pub fn alternar_tamanho(&mut self, cx: &mut Context<Self>) {
        if let Corpo::Imagem(i) = &mut self.corpo {
            i.real = !i.real;
            cx.notify();
        }
    }

    pub fn tocar(&mut self, cx: &mut Context<Self>) {
        if let Corpo::Player(r) = &mut self.corpo {
            r.player.tocar();
            r.ultimo_movimento = Instant::now();
            self.atualizar(cx);
        }
    }

    pub fn pausar(&mut self, cx: &mut Context<Self>) {
        if let Corpo::Player(r) = &mut self.corpo {
            r.player.pausar();
            self.atualizar(cx);
        }
    }

    pub fn alternar_reproducao(&mut self, cx: &mut Context<Self>) {
        if self.tocando() {
            self.pausar(cx);
        } else {
            self.tocar(cx);
        }
    }

    pub fn buscar(&mut self, segundos: f64, cx: &mut Context<Self>) {
        if let Corpo::Player(r) = &mut self.corpo {
            if r.estado.info.erro.is_none() {
                r.player.buscar(segundos);
                r.sem_quadro = true;
                self.atualizar(cx);
            }
        }
    }

    pub fn definir_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        if let Corpo::Player(r) = &mut self.corpo {
            r.player.definir_volume(volume);
            // Como no Chromium, arrastar o volume para cima tira o mudo.
            if volume > 0. {
                r.player.definir_mudo(false);
            }
            self.atualizar(cx);
        }
    }

    pub fn alternar_mudo(&mut self, cx: &mut Context<Self>) {
        if let Corpo::Player(r) = &mut self.corpo {
            r.player.definir_mudo(!r.estado.mudo);
            self.atualizar(cx);
        }
    }

    pub fn definir_velocidade(&mut self, velocidade: f64, cx: &mut Context<Self>) {
        if let Corpo::Player(r) = &mut self.corpo {
            r.player.definir_velocidade(velocidade);
            r.sem_quadro = true;
            self.atualizar(cx);
        }
    }

    pub fn alternar_tela_cheia(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Corpo::Player(r) = &mut self.corpo {
            if !r.video {
                return;
            }
            r.tela_cheia = !r.tela_cheia;
            if r.tela_cheia != window.is_fullscreen() {
                window.toggle_fullscreen();
            }
            cx.notify();
        }
    }

    /// "Fazer download" do menu: copia o arquivo para onde o usuário escolher.
    pub fn baixar(&mut self, cx: &mut Context<Self>) {
        let origem = self.caminho.clone();
        let pasta = dirs_downloads().unwrap_or_else(|| origem.parent().map(Path::to_path_buf).unwrap_or_default());
        let escolha = cx.prompt_for_new_path(&pasta, Some(&self.nome));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(destino))) = escolha.await else { return };
            if let Err(e) = std::fs::copy(&origem, &destino) {
                let _ = this.update(cx, |_, cx| cx.emit(EventoMidia::Falhou(format!("Não foi possível salvar o arquivo: {e}"))));
            }
        })
        .detach();
    }

    fn carregar(&mut self, caminho: PathBuf, cx: &mut Context<Self>) {
        self.fechar_janela_pip(cx);
        self.nome = tipos::nome_de(&caminho).into();
        self.tipo = tipos::midia_de(&caminho);
        self.caminho = caminho.clone();
        self.corpo = match self.tipo {
            None => Corpo::NaoMidia,
            Some(TipoMidia::Imagem) => {
                let leitura = cx.background_executor().spawn(async move { imagem::carregar(&caminho) });
                let carga = cx.spawn(async move |this, cx| {
                    let r = leitura.await;
                    let _ = this.update(cx, |v, cx| {
                        let Corpo::Imagem(i) = &mut v.corpo else { return };
                        i.fonte = match r {
                            Ok(d) => FonteImagem::Pronta { imagem: d.imagem, largura: d.largura, altura: d.altura },
                            Err(FalhaImagem::Leitura(e)) => {
                                cx.emit(EventoMidia::Falhou(e.clone()));
                                FonteImagem::Erro(e.into())
                            }
                            Err(FalhaImagem::Decodificacao) => {
                                cx.emit(EventoMidia::Falhou("Não foi possível exibir esta imagem.".into()));
                                FonteImagem::NaoExibe
                            }
                        };
                        cx.notify();
                    });
                });
                Corpo::Imagem(Imagem { fonte: FonteImagem::Lendo, real: false, _carga: carga })
            }
            Some(t) => {
                // As mesmas regras do protocolo `orq-media` (caminho absoluto, extensão de áudio ou vídeo).
                let player = match orquestrador_core::ide::files::stream_path_at(&caminho) {
                    Ok(c) => Player::abrir(&c),
                    Err((_, e)) => Player::com_erro(e),
                };
                let estado = player.estado();
                let relogio = cx.spawn(async move |this, cx| loop {
                    let intervalo = match this.update(cx, |v, cx| v.passo(cx)) {
                        Ok(i) => i,
                        Err(_) => break,
                    };
                    cx.background_executor().timer(intervalo).await;
                });
                Corpo::Player(Box::new(Reproducao {
                    video: t == TipoMidia::Video,
                    player,
                    estado,
                    quadro: None,
                    descartar: Vec::new(),
                    sem_quadro: true,
                    barra: Rc::default(),
                    barra_volume: Rc::default(),
                    area: Rc::new(Cell::new((Bounds::default(), 1.))),
                    refazer_quadro: false,
                    arrasto: None,
                    volume_aberto: false,
                    ultimo_movimento: Instant::now(),
                    tela_cheia: false,
                    pip: None,
                    area_pip: Rc::new(Cell::new((Bounds::default(), 1.))),
                    _relogio: relogio,
                }))
            }
        };
        cx.notify();
    }

    /// Um passo do relógio da UI: pega o quadro da hora, solta as texturas velhas e avisa mudanças. Devolve quando voltar.
    fn passo(&mut self, cx: &mut Context<Self>) -> Duration {
        let Corpo::Player(r) = &mut self.corpo else { return Duration::from_secs(1) };
        for velho in r.descartar.drain(..) {
            cx.drop_image(velho, None);
        }
        let mut mudou = false;
        let (area, escala) = if r.pip.is_some() { r.area_pip.get() } else { r.area.get() };
        let alvo = (f32::from(area.size.width) * escala, f32::from(area.size.height) * escala);
        if r.video && r.player.definir_alvo(alvo.0 as u32, alvo.1 as u32) {
            r.refazer_quadro = true;
        }
        if r.refazer_quadro && r.estado.info.pronto {
            r.refazer_quadro = false;
            // Parado, o quadro da tela é refeito no tamanho novo; tocando, os próximos já vêm nele.
            if !r.estado.tocando && r.estado.info.erro.is_none() {
                r.player.buscar(r.estado.posicao);
                r.sem_quadro = true;
            }
        }
        if let Some(q) = r.player.quadro(r.sem_quadro) {
            if let Some(novo) = imagem::quadro_bgra(q.largura, q.altura, q.bgra) {
                if let Some(velho) = r.quadro.replace(novo) {
                    r.descartar.push(velho);
                }
                r.sem_quadro = false;
                mudou = true;
            }
        }
        let antes = std::mem::replace(&mut r.estado, r.player.estado());
        if let Some(e) = r.estado.info.erro.clone().filter(|_| antes.info.erro.is_none()) {
            cx.emit(EventoMidia::Falhou(e));
        }
        if antes.tocando != r.estado.tocando {
            cx.emit(EventoMidia::Reproducao { tocando: r.estado.tocando });
        }
        let controles_visiveis = !r.estado.tocando || r.ultimo_movimento.elapsed() < CONTROLES_SOMEM + Duration::from_millis(100);
        if mudou || antes != r.estado || controles_visiveis && r.estado.tocando {
            cx.notify();
        }
        if r.estado.tocando || !r.estado.info.pronto || r.sem_quadro {
            Duration::from_millis(8)
        } else {
            Duration::from_millis(100)
        }
    }

    fn atualizar(&mut self, cx: &mut Context<Self>) {
        if let Corpo::Player(r) = &mut self.corpo {
            let antes = r.estado.tocando;
            r.estado = r.player.estado();
            if antes != r.estado.tocando {
                cx.emit(EventoMidia::Reproducao { tocando: r.estado.tocando });
            }
        }
        cx.notify();
    }

    fn conferir_janela(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if orquestrador_ui::janela::minimizada(window) && !self.pip_aberta() {
            self.pausar(cx);
        }
    }

    fn mover_arrasto(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Corpo::Player(r) = &mut self.corpo else { return };
        match r.arrasto {
            Some(Arrasto::Posicao { .. }) => {
                let f = fracao(r.barra.get(), x);
                if let Some(d) = r.estado.info.duracao {
                    r.player.buscar(f * d);
                    r.sem_quadro = true;
                }
            }
            Some(Arrasto::Volume) => {
                let f = fracao(r.barra_volume.get(), x) as f32;
                r.player.definir_volume(f);
                r.player.definir_mudo(f == 0.);
            }
            None => return,
        }
        self.atualizar(cx);
    }

    fn soltar_arrasto(&mut self, cx: &mut Context<Self>) {
        let Corpo::Player(r) = &mut self.corpo else { return };
        if let Some(Arrasto::Posicao { tocava: true }) = r.arrasto.take() {
            r.player.tocar();
        }
        self.atualizar(cx);
    }
}

fn fracao(b: Bounds<Pixels>, x: Pixels) -> f64 {
    if b.size.width <= px(0.) {
        return 0.;
    }
    f64::from(((x - b.origin.x) / b.size.width).clamp(0., 1.))
}

fn mensagem_erro(video: bool) -> String {
    let (o, que) = if video { ("este", "vídeo") } else { ("este", "áudio") };
    format!("Não foi possível tocar {o} {que}. O arquivo pode ter sumido ou o formato pode não ser suportado.")
}

fn dirs_downloads() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Downloads")).filter(|p| p.is_dir())
}

/// Aviso no lugar do corpo da aba.
fn nota(texto: impl Into<SharedString>, perigo: bool) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .p(px(16.))
        .child(
            div()
                .text_size(tema::TEXTO_BODY)
                .line_height(px(22.))
                .text_color(if perigo { cor::signal_error() } else { cor::text_secondary() })
                .child(texto.into()),
        )
}

/// Faixa de 32 px com o status à esquerda (`role="status"`).
fn faixa(status: String) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .h(px(32.))
        .border_b_1()
        .border_color(cor::hairline())
        .child(div().min_w_0().truncate().text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child(status))
}

// Cores medidas nas capturas dos controles nativos (tema escuro do Chromium).
fn painel_audio() -> gpui_kit::Hsla {
    rgb(0x353535).into()
}
fn trilho_cheio() -> gpui_kit::Hsla {
    rgb(0xb5b5b5).into()
}
fn trilho_video() -> gpui_kit::Hsla {
    rgb(0xadadad).into()
}

impl Render for VisaoMidia {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let raiz = div().id("visao-midia").key_context(CONTEXTO).track_focus(&self.foco).flex().flex_col().flex_1().size_full().min_h_0().bg(cor::pitch_black()).text_color(cor::text_primary()).font_family(tema::FONTE);
        match &self.corpo {
            Corpo::NaoMidia => raiz,
            Corpo::Imagem(_) => raiz.child(self.render_imagem(cx)),
            Corpo::Player(_) => raiz
                .on_action(cx.listener(|v, _: &AlternarReproducao, _, cx| v.alternar_reproducao(cx)))
                .on_action(cx.listener(|v, _: &Voltar, _, cx| {
                    let p = v.estado_reproducao().map(|e| e.posicao).unwrap_or(0.);
                    v.buscar(p - 5., cx)
                }))
                .on_action(cx.listener(|v, _: &Avancar, _, cx| {
                    let p = v.estado_reproducao().map(|e| e.posicao).unwrap_or(0.);
                    v.buscar(p + 5., cx)
                }))
                .on_action(cx.listener(|v, _: &AumentarVolume, _, cx| {
                    let e = v.estado_reproducao().map(|e| e.volume).unwrap_or(1.);
                    v.definir_volume((e + 0.05).min(1.), cx)
                }))
                .on_action(cx.listener(|v, _: &DiminuirVolume, _, cx| {
                    let e = v.estado_reproducao().map(|e| e.volume).unwrap_or(1.);
                    v.definir_volume((e - 0.05).max(0.), cx)
                }))
                .on_action(cx.listener(|v, _: &AlternarMudo, _, cx| v.alternar_mudo(cx)))
                .on_action(cx.listener(|v, _: &AlternarTelaCheia, window, cx| v.alternar_tela_cheia(window, cx)))
                .on_action(cx.listener(|v, _: &SairTelaCheia, window, cx| {
                    if v.tela_cheia() {
                        v.alternar_tela_cheia(window, cx)
                    }
                }))
                .on_mouse_move(cx.listener(|v, e: &MouseMoveEvent, _, cx| {
                    if let Corpo::Player(r) = &mut v.corpo {
                        r.ultimo_movimento = Instant::now();
                    }
                    if e.dragging() {
                        v.mover_arrasto(e.position.x, cx);
                    }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|v, _, _, cx| v.soltar_arrasto(cx)))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|v, _, _, cx| v.soltar_arrasto(cx)))
                .child(self.render_player(window, cx)),
        }
    }
}

impl VisaoMidia {
    fn render_imagem(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Corpo::Imagem(i) = &self.corpo else { unreachable!() };
        match &i.fonte {
            FonteImagem::Lendo => return nota("Lendo o arquivo…", false).into_any_element(),
            FonteImagem::Erro(e) => return nota(e.clone(), true).into_any_element(),
            _ => {}
        }
        let rotulo = if i.real { "Ajustar" } else { "Tamanho real" };
        let botao = div()
            .id("alternar-tamanho")
            .debug_selector(|| "alternar-tamanho".into())
            .flex()
            .flex_none()
            .items_center()
            .h(px(24.))
            .px(px(8.))
            .rounded(tema::RAIO_XS)
            .bg(cor::panel_black())
            .border_1()
            .border_color(rgb(0x1f1f1f))
            .text_size(tema::TEXTO_LABEL)
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.bg(cor::raised_black()))
            .cursor_pointer()
            .on_click(cx.listener(|v, _, _, cx| v.alternar_tamanho(cx)))
            .child(rotulo);
        let corpo = match &i.fonte {
            FonteImagem::Pronta { imagem, largura, altura } if i.real => div()
                .id("imagem-rolagem")
                .flex()
                .flex_1()
                .min_h_0()
                .overflow_scroll()
                .child(
                    div().flex().min_w_full().min_h_full().child(
                        img(imagem.clone())
                            .m_auto()
                            .flex_none()
                            .w(px(*largura as f32))
                            .h(px(*altura as f32))
                            .object_fit(ObjectFit::Fill)
                            .debug_selector(|| "midia-imagem".into()),
                    ),
                )
                .into_any_element(),
            FonteImagem::Pronta { imagem, .. } => div()
                .flex()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .p(px(16.))
                .debug_selector(|| "midia-area".into())
                .child(img(imagem.clone()).size_full().object_fit(ObjectFit::ScaleDown).debug_selector(|| "midia-imagem".into()))
                .into_any_element(),
            _ => nota("Não foi possível exibir esta imagem.", true).into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(faixa(self.status().unwrap_or_default()).child(div().flex_1()).child(botao))
            .child(corpo)
            .into_any_element()
    }

    fn render_player(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Corpo::Player(r) = &self.corpo else { unreachable!() };
        let erro = r.estado.info.erro.is_some();
        let alerta = erro.then(|| mensagem_erro(r.video));
        if !r.video {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .gap(px(16.))
                .p(px(24.))
                .child(div().max_w_full().truncate().text_size(tema::TEXTO_BODY).line_height(px(22.)).child(self.nome.clone()))
                .child(self.controles(false, cx))
                .when_some(alerta, |d, a| {
                    d.child(div().text_size(tema::TEXTO_LABEL).text_color(cor::signal_error()).child(a))
                })
                .into_any_element();
        }
        let tela = self.tela_video(cx);
        let cheia = r.tela_cheia;
        let corpo = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .when(!cheia, |d| d.child(tela))
            .when_some(alerta, |d, a| {
                d.child(div().p(px(16.)).text_size(tema::TEXTO_LABEL).text_color(cor::signal_error()).child(a))
            });
        let mut col = div().flex().flex_col().flex_1().min_h_0().child(faixa(self.status().unwrap_or_default())).child(corpo);
        if cheia {
            let tamanho = window.viewport_size();
            col = col.child(
                deferred(
                    anchored().position(point(px(0.), px(0.))).child(
                        div().w(tamanho.width).h(tamanho.height).bg(cor::pitch_black()).flex().child(self.tela_video(cx)),
                    ),
                )
                .with_priority(10),
            );
        }
        col.into_any_element()
    }

    /// O quadro (contido, sem cortar) com os controles sobrepostos embaixo; clicar no quadro toca e pausa.
    fn tela_video(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Corpo::Player(r) = &self.corpo else { unreachable!() };
        let mostrar = !r.estado.tocando || r.ultimo_movimento.elapsed() < CONTROLES_SOMEM || r.arrasto.is_some();
        div()
            .id("video")
            .debug_selector(|| "midia-video-area".into())
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .size_full()
            .bg(cor::pitch_black())
            .child({
                let area = r.area.clone();
                canvas(move |b, window, _| area.set((b, window.scale_factor())), |_, _, _, _| {}).absolute().size_full()
            })
            .when(r.pip.is_some(), |d| {
                d.child(
                    div()
                        .absolute().top_0().left_0()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(tema::TEXTO_BODY)
                        .text_color(cor::text_secondary())
                        .debug_selector(|| "midia-pip-aviso".into())
                        .child("Reproduzindo em picture-in-picture"),
                )
            })
            .when_some(r.quadro.clone().filter(|_| r.pip.is_none()), |d, q| d.child(img(q).size_full().object_fit(ObjectFit::Contain).debug_selector(|| "midia-video".into())))
            .on_click(cx.listener(|v, e: &gpui_kit::ClickEvent, window, cx| {
                window.focus(&v.foco.clone(), cx);
                if e.click_count() >= 2 {
                    v.alternar_tela_cheia(window, cx);
                } else {
                    v.alternar_reproducao(cx);
                }
            }))
            .when(mostrar, |d| d.child(div().absolute().left_0().right_0().bottom_0().child(self.controles(true, cx))))
    }

    /// Os controles: no áudio, a pílula de 54 px; no vídeo, a fileira de botões com a barra embaixo.
    fn controles(&self, video: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Corpo::Player(r) = &self.corpo else { unreachable!() };
        let e = &r.estado;
        let dur = e.info.duracao.unwrap_or(0.);
        let tempo = format!("{} / {}", formatar_tempo(e.posicao), formatar_tempo(dur));
        let progresso = if dur > 0. { (e.posicao / dur).clamp(0., 1.) as f32 } else { 0. };
        let habilitado = e.info.erro.is_none() && e.info.pronto;
        let tocar = botao_icone_sel("tocar", if e.tocando { "icons/midia-pausar.svg" } else { "icons/midia-tocar.svg" }, 20.)
            .on_click(cx.listener(|v, _, window, cx| {
                window.focus(&v.foco.clone(), cx);
                v.alternar_reproducao(cx)
            }));
        let volume_icone = if e.mudo || e.volume == 0. { "icons/midia-mudo.svg" } else { "icons/midia-volume.svg" };
        let nivel = if e.mudo { 0. } else { e.volume };
        let volume = div()
            .id("volume")
            .flex()
            .flex_none()
            .items_center()
            .on_hover(cx.listener(|v, dentro: &bool, _, cx| {
                if let Corpo::Player(r) = &mut v.corpo {
                    r.volume_aberto = *dentro;
                    cx.notify();
                }
            }))
            .when(r.volume_aberto || r.arrasto == Some(Arrasto::Volume), |d| {
                d.child(barra("barra-volume", nivel, r.barra_volume.clone(), trilho_cheio(), 52.).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|v, ev: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if let Corpo::Player(r) = &mut v.corpo {
                            r.arrasto = Some(Arrasto::Volume);
                        }
                        v.mover_arrasto(ev.position.x, cx);
                    }),
                ))
            })
            .child(
                botao_icone("mudo", volume_icone, 20.).on_click(cx.listener(|v, _, _, cx| v.alternar_mudo(cx))),
            );
        let barra_tempo = barra("barra-tempo", progresso, r.barra.clone(), if video { trilho_video() } else { trilho_cheio() }, 0.)
            .when(!video, |d| d.flex_1().ml(px(13.)).mr(px(16.)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|v, ev: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let Corpo::Player(r) = &mut v.corpo else { return };
                    if r.estado.info.erro.is_some() {
                        return;
                    }
                    let tocava = r.estado.tocando;
                    r.player.pausar();
                    r.arrasto = Some(Arrasto::Posicao { tocava });
                    v.mover_arrasto(ev.position.x, cx);
                }),
            );
        let entidade = cx.entity().downgrade();
        let velocidade = e.velocidade;
        let pode_pip = video && habilitado;
        let mais = Button::new("mais")
            .ghost()
            .xsmall()
            .size(px(32.))
            .rounded_full()
            .child(icone("icons/midia-mais.svg", 20., cor::text_primary()))
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, window, cx| {
                let e1 = entidade.clone();
                let e2 = entidade.clone();
                let e3 = entidade.clone();
                menu.min_w(px(200.))
                    .item(PopupMenuItem::new("Fazer download").on_click(move |_, _, cx| {
                        let _ = e1.update(cx, |v, cx| v.baixar(cx));
                    }))
                    .when(pode_pip, |m| {
                        m.item(PopupMenuItem::new("Picture-in-picture").on_click(move |_, _, cx| {
                            let _ = e3.update(cx, |v, cx| v.alternar_pip(cx));
                        }))
                    })
                    .submenu("Velocidade de reprodução", window, cx, move |sub, _, _| {
                        VELOCIDADES.iter().fold(sub, |sub, &vel| {
                            let e = e2.clone();
                            let rotulo = if vel == 1. { "Normal".to_string() } else { format!("{vel}").replace('.', ",") };
                            sub.item(PopupMenuItem::new(rotulo).checked((vel - velocidade).abs() < 1e-6).on_click(move |_, _, cx| {
                                let _ = e.update(cx, |v, cx| v.definir_velocidade(vel, cx));
                            }))
                        })
                    })
            });
        let texto = div().flex_none().text_size(tema::TEXTO_BODY).text_color(cor::text_primary()).child(tempo);
        if !video {
            return div()
                .id("controle-audio")
                .flex()
                .items_center()
                .w_full()
                .max_w(px(560.))
                .h(px(54.))
                .pl(px(11.))
                .pr(px(9.))
                .rounded(px(27.))
                .bg(painel_audio())
                .when(!habilitado, |d| d.opacity(0.6))
                .child(tocar)
                .child(div().ml(px(4.)).child(texto))
                .child(barra_tempo)
                .child(volume)
                .child(mais)
                .into_any_element();
        }
        let tela_cheia = botao_icone(
            "tela-cheia",
            if r.tela_cheia { "icons/midia-sair-tela-cheia.svg" } else { "icons/midia-tela-cheia.svg" },
            20.,
        )
        .on_click(cx.listener(|v, _, window, cx| v.alternar_tela_cheia(window, cx)));
        div()
            .id("controle-video")
            .flex()
            .flex_col()
            .w_full()
            .px(px(8.))
            .pb(px(14.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .when(!habilitado, |d| d.opacity(0.6))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.))
                    .child(tocar)
                    .child(div().ml(px(7.)).child(texto))
                    .child(div().flex_1())
                    .child(volume)
                    .child(div().ml(px(16.)).child(tela_cheia))
                    .child(div().ml(px(16.)).child(mais)),
            )
            .child(div().px(px(7.)).child(barra_tempo))
            .into_any_element()
    }
}

fn botao_icone_sel(id: &'static str, caminho: &'static str, tamanho: f32) -> gpui_kit::Stateful<gpui_kit::Div> {
    botao_icone(id, caminho, tamanho).debug_selector(move || id.into())
}

/// Botão redondo de 32 px com o ícone branco.
fn botao_icone(id: &'static str, caminho: &'static str, tamanho: f32) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(32.))
        .rounded_full()
        .cursor_pointer()
        .hover(|s| s.bg(rgba(0xffffff1a)))
        .child(icone(caminho, tamanho, cor::text_primary()))
}

/// Trilho de 4 px, cinza com a parte tocada em branco; a área de clique tem 16 px de altura. `largura` 0 ocupa o espaço do pai.
fn barra(
    id: &'static str, fracao: f32, limites: Rc<Cell<Bounds<Pixels>>>, fundo: gpui_kit::Hsla, largura: f32,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .h(px(16.))
        .cursor_pointer()
        .when(largura > 0., |d| d.w(px(largura)).flex_none())
        .when(largura == 0., |d| d.w_full())
        .child(
            canvas(move |b, _, _| limites.set(b), |_, _, _, _| {}).absolute().size_full(),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(4.))
                .rounded(px(2.))
                .bg(fundo)
                .child(div().absolute().left_0().top_0().h_full().rounded(px(2.)).bg(cor::text_primary()).w(gpui_kit::relative(fracao))),
        )
}
