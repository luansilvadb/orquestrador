//! A tela Arquivos sem o editor: porte de `src/explorer/explorer.tsx` e `explorer-tree.tsx`. Barra com as ações da view,
//! seção recolhível com o nome do projeto, linhas virtuais de 22 px com recuo de 8 px por nível, decorações do Git,
//! renomear e criar na própria linha, menus de contexto, diálogo de excluir, arrastar para mover e copiar, recortar e
//! colar pelo clipboard de arquivos do Windows. A lista se atualiza sozinha (o `usePolling` do React: 1,5 s com a
//! janela em foco, 10 s sem foco).
use crate::modelo::{self, Arvore, Criando, Linha, Mods, TipoCaminho, TipoLinha};
use orquestrador_core::ide::files::{self, DirListing, Entry, Moved};
use orquestrador_core::ide::git::GitFile;
use orquestrador_core::platform;
use orquestrador_ui::avisos::{self, Aviso};
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::{self, icone, TooltipTexto};
use orquestrador_ui::icone_arquivo;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::TestSupportExt;
use gpui_kit::{
    anchored, deferred, div, img, point, prelude::*, px, rgb, rgba, uniform_list, AnyElement, App, ClickEvent,
    ClipboardItem, Context, DragMoveEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla,
    KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, ScrollStrategy, SharedString, Subscription,
    UniformListScrollHandle, WeakEntity, Window, actions, Bounds, KeyBinding,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Linha de 22 px e recuo de 8 px por nível, as medidas do Explorer do Antigravity.
pub const ALTURA_LINHA: f32 = 22.;
const RECUO: f32 = 8.;
const POLL: Duration = Duration::from_millis(1500);
const POLL_SEM_FOCO: Duration = Duration::from_secs(10);
actions!(arvore, [FocarProximo, FocarAnterior]);

/// Tab e Shift+Tab percorrem o foco como no navegador; o painel da árvore é uma parada. Uma vez no início do app.
pub fn registrar_atalhos(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("tab", FocarProximo, None), KeyBinding::new("shift-tab", FocarAnterior, None)]);
    cx.on_action(|_: &FocarProximo, cx| percorrer_foco(true, cx));
    cx.on_action(|_: &FocarAnterior, cx| percorrer_foco(false, cx));
}

fn percorrer_foco(adiante: bool, cx: &mut App) {
    let Some(janela) = cx.active_window().or_else(|| cx.windows().first().copied()) else { return };
    // Adiado: a janela está emprestada durante o despacho da ação.
    cx.defer(move |cx| {
        let _ = janela.update(cx, |_, window, cx| if adiante { window.focus_next(cx) } else { window.focus_prev(cx) });
    });
}

/// Quantos itens o diálogo de excluir lista antes de resumir.
const LISTA_MAX: usize = 10;

/// O que a tela pede à bancada.
#[derive(Clone, Debug, PartialEq)]
pub enum EventoArvore {
    /// Abrir o arquivo: `fixo` falso é a prévia (a aba seguinte substitui); `ao_lado`, na área da direita.
    Abrir { caminho: String, fixo: bool, ao_lado: bool },
    /// Clique simples começando num arquivo: a bancada pode ler o arquivo antes do clique terminar.
    PreCarregar(String),
    /// Abrir um terminal na pasta (relativa; "" é a raiz).
    AbrirNoTerminal(String),
    /// Colar o caminho no rascunho do Orquestrador (só emitido com [`Explorador::definir_chat`] ligado).
    AdicionarAoChat(String),
    /// Histórico Git do arquivo ou da pasta (só com [`Explorador::definir_historico`] ligado).
    HistoricoGit(String),
    /// Pesquisa no projeto ("") ou na pasta, com o campo focado.
    Pesquisar(String),
    /// Item renomeado: abas e rascunhos trocam o prefixo `de` por `para`.
    Renomeado { de: String, para: String },
    /// Itens enviados à Lixeira: as abas deles (e das pastas, o que está dentro) fecham.
    Excluidos(Vec<String>),
    /// Itens movidos (arrastar ou colar um recorte): abas e rascunhos trocam o prefixo.
    Movidos(Vec<Moved>),
}

#[derive(Clone)]
struct Renomeando {
    caminho: String,
    erro: String,
    ocupado: bool,
}

#[derive(Clone)]
struct Excluindo {
    caminhos: Vec<String>,
    erro: String,
    ocupado: bool,
}

#[derive(Clone)]
struct CriandoItem {
    pasta: bool,
    dir: String,
    erro: String,
    ocupado: bool,
}

/// Os itens que o arraste leva.
#[derive(Clone)]
pub struct Arrasto {
    caminhos: Vec<String>,
}

/// A imagem do arraste: a cópia da linha agarrada, translúcida e na mesma posição sob o ponteiro, como o Chromium
/// desenha um elemento `draggable` (com o fundo de hover ou de seleção do momento).
#[derive(Clone)]
struct CopiaLinha {
    largura: Pixels,
    nivel: usize,
    expansivel: bool,
    aberta: bool,
    icone: SharedString,
    nome: SharedString,
    cor_nome: Option<Hsla>,
    letra: SharedString,
    selecionada: bool,
}

struct VistaArrasto {
    copia: CopiaLinha,
    /// O GPUI prende a imagem no ponto em que o arraste começou (depois do limiar); o Chromium, no ponto do clique.
    ajuste: Point<Pixels>,
}

/// Opacidade da imagem de arraste do Chromium (`kDragImageAlpha`).
const OPACIDADE_ARRASTO: f32 = 0.75;

impl Render for VistaArrasto {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let c = &self.copia;
        let mut guias = div().absolute().top_0().bottom_0().left(px(16.));
        for n in 0..c.nivel {
            guias = guias.child(div().absolute().top_0().bottom_0().left(px(RECUO * n as f32)).w(px(1.)).bg(cor_guia()));
        }
        let seta = icone("icons/chevron-right.svg", 14., cor::text_secondary());
        let seta = if c.aberta { seta.with_transformation(gpui_kit::Transformation::rotate(gpui_kit::percentage(0.25))) } else { seta };
        div()
            .debug_selector(|| "arvore-arrasto".into())
            .relative()
            .left(self.ajuste.x)
            .top(self.ajuste.y)
            .flex()
            .items_center()
            .w(c.largura)
            .h(px(ALTURA_LINHA))
            .pl(px(RECUO * c.nivel as f32))
            .pr(px(8.))
            .opacity(OPACIDADE_ARRASTO)
            .font_family(tema::FONTE)
            .text_size(tema::TEXTO_BODY)
            .text_color(cor::text_primary())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(4.))
                    .right(px(4.))
                    .rounded(tema::RAIO_XS)
                    .bg(if c.selecionada { fundo_selecionado() } else { fundo_hover() }),
            )
            .child(guias)
            .child(
                div()
                    .ml(px(8.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(16.))
                    .h(px(ALTURA_LINHA))
                    .when(c.expansivel, |d| d.child(seta)),
            )
            .child(img(c.icone.clone()).size(px(16.)).flex_none().mr(px(6.)))
            .child(
                div().flex().flex_1().min_w_0().items_center().pt(px(1.)).pb(px(3.)).gap(px(8.)).child(
                    div().flex_1().min_w_0().truncate().when_some(c.cor_nome, |d, cor| d.text_color(cor)).child(c.nome.clone()),
                )
                .when(!c.letra.is_empty(), |d| {
                    d.child(
                        div()
                            .flex_none()
                            .text_size(tema::TEXTO_LABEL)
                            .font_weight(FontWeight::MEDIUM)
                            .when_some(c.cor_nome, |d, cor| d.text_color(cor))
                            .child(c.letra.clone()),
                    )
                }),
            )
    }
}

#[derive(Clone, Debug)]
enum Acao {
    NovoArquivo(String),
    NovaPasta(String),
    AbrirAoLado(String),
    Revelar(String, bool),
    Terminal(String),
    Historico(String),
    Pesquisar(String),
    Chat(Vec<String>),
    Recortar(Vec<String>),
    Copiar(Vec<String>),
    Colar(String),
    CopiarCaminhos(TipoCaminho, Vec<String>),
    Renomear(String),
    Excluir(Vec<String>),
}

enum ItemMenu {
    Acao { rotulo: String, atalho: &'static str, acao: Option<Acao>, motivo: &'static str },
    Divisor,
}

fn item(rotulo: impl Into<String>, atalho: &'static str, acao: Acao) -> ItemMenu {
    ItemMenu::Acao { rotulo: rotulo.into(), atalho, acao: Some(acao), motivo: "" }
}

fn bloqueado(rotulo: impl Into<String>, atalho: &'static str, motivo: &'static str) -> ItemMenu {
    ItemMenu::Acao { rotulo: rotulo.into(), atalho, acao: None, motivo }
}

struct Menu {
    posicao: Point<Pixels>,
    itens: Vec<ItemMenu>,
    destacado: Option<usize>,
    foco: FocusHandle,
}

/// A tela. Crie com [`Explorador::new`]; a bancada mantém o Git, a aba aberta, os rascunhos e as abas atualizados pelos
/// métodos `definir_*` e trata os [`EventoArvore`].
pub struct Explorador {
    raiz: PathBuf,
    projeto: SharedString,
    arvore: Arvore,
    linhas: Rc<Vec<Linha>>,
    linhas_validas: bool,
    erro: String,
    selecao: Vec<String>,
    ancora: Option<String>,
    /// A linha com o "foco" do teclado (o `tabIndex` itinerante do React); None é o painel.
    cursor: Option<String>,
    aberto: Option<String>,
    decoracoes: HashMap<String, modelo::Decoracao>,
    sujos: Vec<String>,
    abas: Vec<String>,
    chat: bool,
    historico: bool,
    renomeando: Option<Renomeando>,
    excluindo: Option<Excluindo>,
    criando: Option<CriandoItem>,
    soltar: Option<(String, Option<String>)>,
    recolhida: bool,
    menu: Option<Menu>,
    acorde: Option<Instant>,
    ativa: bool,
    janela_ativa: bool,
    proxima_rodada: Instant,
    consultando: bool,
    foco: FocusHandle,
    foco_dialogo: FocusHandle,
    campo: Entity<InputState>,
    rolagem: UniformListScrollHandle,
    /// Linha a mostrar quando entrar na lista (Revelar antes da lista chegar).
    rolar_para: Option<String>,
    /// Devolve o foco ao painel no próximo desenho (fim de um pedido assíncrono).
    focar_painel: bool,
    origem_lista: Point<Pixels>,
    /// Limites de cada linha desenhada no último quadro (menu pelo teclado e largura da cópia no arraste).
    limites_linhas: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// Janela minimizada: a atualização periódica para, como o `usePolling` com a página oculta.
    minimizada: bool,
    /// Onde o botão desceu na última linha clicada (o ponto em que a cópia do arraste fica presa).
    agarrado: Rc<std::cell::Cell<Point<Pixels>>>,
    largura_campo: Pixels,
    _subs: Vec<Subscription>,
}

impl EventEmitter<EventoArvore> for Explorador {}

impl Focusable for Explorador {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

impl Explorador {
    /// `raiz` é a pasta do projeto; `projeto`, o nome mostrado na seção. A primeira leitura sai na hora.
    pub fn new(raiz: PathBuf, projeto: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let campo = cx.new(|cx| InputState::new(window, cx));
        // Parada de Tab, como o `tabIndex` itinerante da árvore no React.
        let foco = cx.focus_handle().tab_stop(true);
        let subs = vec![
            cx.subscribe_in(&campo, window, Self::no_campo),
            // O acorde Ctrl+K cancela quando o foco sai da árvore.
            cx.on_blur(&foco, window, |this, _, _| this.acorde = None),
            cx.observe_window_activation(window, |this, window, cx| {
                this.janela_ativa = window.is_window_active();
                if this.janela_ativa {
                    this.cutucar(cx);
                }
            }),
        ];
        let mut this = Explorador {
            raiz,
            projeto: projeto.into(),
            arvore: Arvore::default(),
            linhas: Rc::new(Vec::new()),
            linhas_validas: false,
            erro: String::new(),
            selecao: Vec::new(),
            ancora: None,
            cursor: None,
            aberto: None,
            decoracoes: HashMap::new(),
            sujos: Vec::new(),
            abas: Vec::new(),
            chat: false,
            historico: false,
            renomeando: None,
            excluindo: None,
            criando: None,
            soltar: None,
            recolhida: false,
            menu: None,
            acorde: None,
            ativa: true,
            janela_ativa: window.is_window_active(),
            proxima_rodada: cx.background_executor().now(),
            consultando: false,
            foco,
            foco_dialogo: cx.focus_handle(),
            campo,
            rolagem: UniformListScrollHandle::new(),
            rolar_para: None,
            focar_painel: false,
            origem_lista: Point::default(),
            limites_linhas: Rc::default(),
            minimizada: false,
            agarrado: Rc::default(),
            largura_campo: px(200.),
            _subs: subs,
        };
        this.iniciar_relogio(window, cx);
        this
    }

    // ---------- API para a bancada ----------

    /// O status do Git (None fora do modo `repo`): cores e letras das linhas.
    pub fn definir_git(&mut self, arquivos: Option<&[GitFile]>, cx: &mut Context<Self>) {
        self.decoracoes = arquivos.map(modelo::decoracoes).unwrap_or_default();
        cx.notify();
    }

    /// O arquivo da aba ativa: marcado quando não há seleção. Trocar limpa a seleção.
    pub fn definir_aberto(&mut self, caminho: Option<String>, cx: &mut Context<Self>) {
        if self.aberto != caminho {
            self.aberto = caminho;
            self.selecao.clear();
            cx.notify();
        }
    }

    /// Arquivos com alteração não salva: o diálogo de excluir avisa.
    pub fn definir_sujos(&mut self, caminhos: Vec<String>) {
        self.sujos = caminhos;
    }

    /// Arquivos com aba (ou rascunho) aberta: mover ou colar por cima deles é recusado antes de mexer no disco.
    pub fn definir_abas(&mut self, caminhos: Vec<String>) {
        self.abas = caminhos;
    }

    /// Orquestrador rodando: "Adicionar ao Chat" fica habilitado.
    pub fn definir_chat(&mut self, disponivel: bool) {
        self.chat = disponivel;
    }

    /// Modo `repo`: o menu ganha "Histórico Git".
    pub fn definir_historico(&mut self, disponivel: bool) {
        self.historico = disponivel;
    }

    /// Tela à vista: a atualização periódica só roda com ela ativa.
    pub fn definir_ativa(&mut self, ativa: bool, cx: &mut Context<Self>) {
        if self.ativa != ativa {
            self.ativa = ativa;
            if ativa {
                self.cutucar(cx);
            }
        }
    }

    /// Rodada imediata da atualização (o `pokePolling('files')`): depois de salvar, de um comando do Git etc.
    pub fn atualizar(&mut self, cx: &mut Context<Self>) {
        self.cutucar(cx);
    }

    /// Abre as pastas de um arquivo de cima para baixo, seleciona-o e rola até ele.
    pub fn revelar(&mut self, caminho: String, window: &mut Window, cx: &mut Context<Self>) {
        let partes: Vec<&str> = caminho.split('/').collect();
        let pastas: Vec<String> = (1..partes.len()).map(|i| partes[..i].join("/")).collect();
        let raiz = self.raiz.clone();
        cx.spawn_in(window, async move |this, cx| {
            for dir in pastas {
                let r = {
                    let raiz = raiz.clone();
                    let d = dir.clone();
                    cx.background_executor().spawn(async move { files::list_many(&raiz, &[d]) }).await
                };
                let ok = this.update(cx, |this, cx| this.aplicar_abertura(&dir, r, cx)).unwrap_or(false);
                if !ok {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.selecao = vec![caminho.clone()];
                this.ancora = Some(caminho.clone());
                this.rolar_para = Some(caminho);
                cx.notify();
            });
        })
        .detach();
    }

    /// Abre as pastas dadas (cada uma com a lista lida antes), na ordem.
    pub fn expandir(&mut self, pastas: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let raiz = self.raiz.clone();
        cx.spawn_in(window, async move |this, cx| {
            for dir in pastas {
                let r = {
                    let (raiz, d) = (raiz.clone(), dir.clone());
                    cx.background_executor().spawn(async move { files::list_many(&raiz, &[d]) }).await
                };
                let _ = this.update(cx, |this, cx| this.aplicar_abertura(&dir, r, cx));
            }
        })
        .detach();
    }

    /// Troca a seleção.
    pub fn selecionar(&mut self, caminhos: Vec<String>, cx: &mut Context<Self>) {
        self.ancora = caminhos.last().cloned();
        self.selecao = caminhos;
        cx.notify();
    }

    /// Abre o menu de contexto do item (como o clique direito nele) ou, com `None`, o da raiz.
    pub fn abrir_menu_de_contexto(&mut self, caminho: Option<String>, posicao: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.garantir_linhas();
        match caminho.and_then(|c| self.linhas.iter().find(|l| l.chave == c).cloned()) {
            Some(l) => self.clique_direito_linha(&l, posicao, window, cx),
            None => {
                let itens = self.menu_raiz();
                self.abrir_menu(itens, posicao, window, cx);
            }
        }
    }

    /// Começa a renomear o item na própria linha (o F2).
    pub fn renomear(&mut self, caminho: String, window: &mut Window, cx: &mut Context<Self>) {
        self.iniciar_renomear(caminho, window, cx);
    }

    /// Começa a criar um arquivo (ou pasta) em `dir`, na própria árvore.
    pub fn criar(&mut self, pasta: bool, dir: String, window: &mut Window, cx: &mut Context<Self>) {
        self.iniciar_criar(pasta, dir, window, cx);
    }

    /// Abre o diálogo de excluir os itens.
    pub fn excluir(&mut self, caminhos: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.iniciar_excluir(caminhos, window, cx);
    }

    /// O que está selecionado (vazio: a aba aberta aparece marcada).
    pub fn selecao(&self) -> &[String] {
        &self.selecao
    }

    /// As linhas à vista, na ordem da tela.
    pub fn linhas(&mut self) -> Rc<Vec<Linha>> {
        self.garantir_linhas();
        self.linhas.clone()
    }

    pub fn raiz(&self) -> &std::path::Path {
        &self.raiz
    }

    // ---------- estado ----------

    fn mudou(&mut self) {
        self.linhas_validas = false;
    }

    fn garantir_linhas(&mut self) {
        if self.linhas_validas {
            return;
        }
        let criando = self.criando.as_ref().map(|c| Criando { dir: &c.dir, pasta: c.pasta });
        self.linhas = Rc::new(modelo::linhas(&self.arvore, criando));
        self.linhas_validas = true;
    }

    fn marcados(&self) -> Vec<String> {
        if !self.selecao.is_empty() {
            self.selecao.clone()
        } else {
            self.aberto.iter().cloned().collect()
        }
    }

    fn aviso(&self, a: Aviso, cx: &mut App) {
        avisos::mostrar(a, cx);
    }

    // ---------- leitura das pastas ----------

    fn cutucar(&mut self, cx: &mut Context<Self>) {
        self.proxima_rodada = cx.background_executor().now();
        cx.notify();
    }

    /// Relógio de 100 ms: dispara a rodada quando ela vence, uma por vez, só com a tela ativa e a seção aberta.
    fn iniciar_relogio(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let vivo = this.update_in(cx, |this, window, cx| {
                let minimizada = janela_minimizada(window);
                if this.minimizada != minimizada {
                    this.minimizada = minimizada;
                    // Ao restaurar, a rodada sai na hora (o `visibilitychange` do React).
                    if !minimizada {
                        this.proxima_rodada = cx.background_executor().now();
                    }
                }
                if this.minimizada {
                    return;
                }
                if this.ativa && !this.recolhida && !this.consultando && cx.background_executor().now() >= this.proxima_rodada {
                    this.rodada(cx);
                }
            });
            if vivo.is_err() {
                break;
            }
        })
        .detach();
    }

    /// Consulta a raiz e cada pasta aberta numa chamada só (um `git check-ignore` para todas). Falha da raiz aparece
    /// como texto; falha do pedido inteiro mostra o motivo e fica para a volta seguinte.
    fn rodada(&mut self, cx: &mut Context<Self>) {
        self.consultando = true;
        let caminhos: Vec<String> = std::iter::once(String::new()).chain(self.arvore.abertas.iter().cloned()).collect();
        let raiz = self.raiz.clone();
        let pedido = caminhos.clone();
        let tarefa = cx.background_spawn(async move { files::list_many(&raiz, &pedido) });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                this.consultando = false;
                let espera = if this.janela_ativa { POLL } else { POLL_SEM_FOCO };
                this.proxima_rodada = cx.background_executor().now() + espera;
                match r {
                    Err(e) => this.erro = e,
                    Ok(resultados) => {
                        this.erro = match resultados.first() {
                            Some(DirListing::Error { message }) => message.clone(),
                            _ => String::new(),
                        };
                        this.aplicar_listagens(&caminhos, resultados);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn aplicar_listagens(&mut self, caminhos: &[String], resultados: Vec<DirListing>) {
        let rs: Vec<Result<Vec<Entry>, String>> = resultados
            .into_iter()
            .map(|r| match r {
                DirListing::Ok { entries } => Ok(entries),
                DirListing::Error { message } => Err(message),
            })
            .collect();
        if self.arvore.aplicar(caminhos, &rs) {
            self.mudou();
        }
    }

    /// Resultado da leitura de uma pasta pedida para abrir: abre já com o conteúdo, ou tira da árvore a que sumiu.
    fn aplicar_abertura(&mut self, dir: &str, r: Result<Vec<DirListing>, String>, cx: &mut Context<Self>) -> bool {
        let ok = match r {
            Ok(mut v) if !v.is_empty() => match v.remove(0) {
                DirListing::Ok { entries } => {
                    self.arvore.abrir_com(dir, entries);
                    true
                }
                DirListing::Error { .. } => false,
            },
            _ => false,
        };
        if !ok {
            self.arvore.tirar(dir);
        }
        self.mudou();
        cx.notify();
        ok
    }

    fn abrir_pasta(&mut self, dir: String, cx: &mut Context<Self>) {
        let raiz = self.raiz.clone();
        let d = dir.clone();
        let tarefa = cx.background_spawn(async move { files::list_many(&raiz, &[d]) });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| this.aplicar_abertura(&dir, r, cx));
        })
        .detach();
    }

    fn alternar(&mut self, caminho: &str, cx: &mut Context<Self>) {
        if self.arvore.aberta(caminho) {
            self.arvore.fechar(caminho);
            self.mudou();
            cx.notify();
        } else {
            self.abrir_pasta(caminho.to_string(), cx);
        }
    }

    /// Relê a raiz e as pastas dadas (as abertas), sem abrir a que estava fechada.
    fn reler(&mut self, dirs: Vec<String>, cx: &mut Context<Self>) {
        let mut caminhos = vec![String::new()];
        for d in dirs {
            if !caminhos.contains(&d) {
                caminhos.push(d);
            }
        }
        let raiz = self.raiz.clone();
        let pedido = caminhos.clone();
        let tarefa = cx.background_spawn(async move { files::list_many(&raiz, &pedido) });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            if let Ok(r) = tarefa.await {
                let _ = this.update(cx, |this, cx| {
                    this.aplicar_listagens(&caminhos, r);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn reler_tudo(&mut self, cx: &mut Context<Self>) {
        let abertas = self.arvore.abertas.clone();
        self.reler(abertas, cx);
    }

    /// Depois de uma ação em lote: tira o que saiu (`gone`), leva a seleção para onde os itens foram e relê as pastas.
    fn assentar(&mut self, dirs: Vec<String>, saiu: &[String], movidos: &[Moved], cx: &mut Context<Self>) {
        for p in saiu {
            self.arvore.tirar(p);
        }
        self.mudou();
        self.selecao = self
            .selecao
            .iter()
            .map(|p| modelo::remapear(p, movidos))
            .filter(|p| !saiu.iter().any(|g| modelo::sob(g, p)))
            .collect();
        self.reler(dirs, cx);
    }

    // ---------- seleção e clique ----------

    fn clicar_linha(&mut self, linha: &Linha, mods: Mods, window: &mut Window, cx: &mut Context<Self>) {
        if !linha.real() {
            return;
        }
        self.garantir_linhas();
        // Ctrl soma à marcação exibida, que sem seleção é o arquivo aberto (como o `selected` do explorer-tree.tsx).
        let marcados = self.marcados();
        let (nova, ancora) = modelo::selecionar(&self.linhas, &marcados, self.ancora.as_deref(), linha, mods);
        self.selecao = nova;
        self.ancora = ancora;
        self.cursor = Some(linha.chave.clone());
        window.focus(&self.foco, cx);
        if !mods.ctrl && !mods.shift {
            if linha.tipo == TipoLinha::Arquivo {
                cx.emit(EventoArvore::Abrir { caminho: linha.chave.clone(), fixo: false, ao_lado: false });
            } else {
                self.alternar(&linha.chave, cx);
            }
        }
        cx.notify();
    }

    fn duplo_clique(&mut self, linha: &Linha, cx: &mut Context<Self>) {
        if linha.tipo == TipoLinha::Arquivo && self.arvore.eh_arquivo(&linha.chave) {
            cx.emit(EventoArvore::Abrir { caminho: linha.chave.clone(), fixo: true, ao_lado: false });
        }
    }

    fn clique_direito_linha(&mut self, linha: &Linha, posicao: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if !linha.real() {
            return;
        }
        let marcados = self.marcados();
        let alvos = if marcados.contains(&linha.chave) && !self.selecao.is_empty() {
            self.selecao.clone()
        } else {
            vec![linha.chave.clone()]
        };
        self.selecao = alvos.clone();
        self.cursor = Some(linha.chave.clone());
        let itens = self.menu_item(&alvos);
        self.abrir_menu(itens, posicao, window, cx);
    }

    pub(crate) fn clique_vazio(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.foco, cx);
        self.cursor = None;
        self.ancora = None;
        self.selecao.clear();
        cx.notify();
    }

    // ---------- menus ----------

    fn tem_arquivos_no_clipboard() -> bool {
        !platform::snapshot().paths.is_empty()
    }

    /// Menu do item (ou da seleção): uma ação real por item; vários itens desabilitam as de um só, com o motivo.
    fn menu_item(&self, caminhos: &[String]) -> Vec<ItemMenu> {
        let colar = Self::tem_arquivos_no_clipboard();
        let varios = caminhos.len() > 1;
        let um = caminhos[0].clone();
        let pasta = !varios && !self.arvore.eh_arquivo(&um);
        let dir_colar = if pasta { um.clone() } else { modelo::dividir(&um).0.to_string() };
        let unico = |rotulo: &str, atalho: &'static str, acao: Acao| {
            if varios {
                bloqueado(rotulo, atalho, "Escolha só um item para esta ação.")
            } else {
                item(rotulo, atalho, acao)
            }
        };
        let mut secoes: Vec<Vec<ItemMenu>> = Vec::new();
        if pasta {
            secoes.push(vec![
                item("Novo Arquivo...", "", Acao::NovoArquivo(um.clone())),
                item("Nova Pasta...", "", Acao::NovaPasta(um.clone())),
            ]);
        }
        let mut s = Vec::new();
        if !pasta {
            s.push(unico("Abrir à Direita", "", Acao::AbrirAoLado(um.clone())));
        }
        s.push(unico("Revelar no Explorador", "Shift+Alt+R", Acao::Revelar(um.clone(), pasta)));
        s.push(unico("Abrir no Terminal", "", Acao::Terminal(dir_colar.clone())));
        if self.historico {
            s.push(unico("Histórico Git", "", Acao::Historico(um.clone())));
        }
        if pasta {
            s.push(unico("Localizar na Pasta...", "", Acao::Pesquisar(um.clone())));
        }
        secoes.push(s);
        let rotulo_chat = if pasta { "Adicionar Pasta ao Chat" } else { "Adicionar ao Chat" };
        secoes.push(vec![if self.chat {
            item(rotulo_chat, "", Acao::Chat(caminhos.to_vec()))
        } else {
            bloqueado(rotulo_chat, "", "Abra o Orquestrador para mandar ao chat.")
        }]);
        secoes.push(vec![
            item("Recortar", "Ctrl+X", Acao::Recortar(caminhos.to_vec())),
            item("Copiar", "Ctrl+C", Acao::Copiar(caminhos.to_vec())),
            if colar { item("Colar", "Ctrl+V", Acao::Colar(dir_colar)) } else { bloqueado("Colar", "Ctrl+V", "Não há arquivo ou pasta copiado.") },
        ]);
        secoes.push(vec![
            item(
                if varios { "Copiar Caminhos" } else { "Copiar Caminho" },
                "Shift+Alt+C",
                Acao::CopiarCaminhos(TipoCaminho::Absoluto, caminhos.to_vec()),
            ),
            item(
                if varios { "Copiar Caminhos Relativos" } else { "Copiar Caminho Relativo" },
                "Ctrl+K Ctrl+Shift+C",
                Acao::CopiarCaminhos(TipoCaminho::Relativo, caminhos.to_vec()),
            ),
        ]);
        secoes.push(vec![
            if varios { bloqueado("Renomear...", "F2", "Escolha só um item para renomear.") } else { item("Renomear...", "F2", Acao::Renomear(um)) },
            item(
                if varios { format!("Excluir {} itens...", caminhos.len()) } else { "Excluir...".into() },
                "Del",
                Acao::Excluir(caminhos.to_vec()),
            ),
        ]);
        juntar_secoes(secoes)
    }

    /// Menu da raiz (área vazia do painel): sempre na raiz do projeto.
    fn menu_raiz(&self) -> Vec<ItemMenu> {
        let colar = Self::tem_arquivos_no_clipboard();
        juntar_secoes(vec![
            vec![item("Novo Arquivo...", "", Acao::NovoArquivo(String::new())), item("Nova Pasta...", "", Acao::NovaPasta(String::new()))],
            vec![
                item("Revelar no Explorador de Arquivos", "Shift+Alt+R", Acao::Revelar(String::new(), true)),
                item("Pesquisar no Projeto", "Shift+Alt+F", Acao::Pesquisar(String::new())),
            ],
            vec![
                if self.chat {
                    item("Adicionar Pasta ao Chat", "", Acao::Chat(vec![String::new()]))
                } else {
                    bloqueado("Adicionar Pasta ao Chat", "", "Abra o Orquestrador para mandar a pasta ao chat.")
                },
                if colar { item("Colar", "Ctrl+V", Acao::Colar(String::new())) } else { bloqueado("Colar", "Ctrl+V", "Não há arquivo ou pasta copiado.") },
            ],
            vec![
                item("Copiar o Caminho", "Shift+Alt+C", Acao::CopiarCaminhos(TipoCaminho::Absoluto, vec![String::new()])),
                item("Copiar Caminho Relativo", "Ctrl+K Ctrl+Shift+C", Acao::CopiarCaminhos(TipoCaminho::Relativo, vec![String::new()])),
            ],
        ])
    }

    fn abrir_menu(&mut self, itens: Vec<ItemMenu>, posicao: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let foco = cx.focus_handle();
        window.focus(&foco, cx);
        self.menu = Some(Menu { posicao, itens, destacado: None, foco });
        cx.notify();
    }

    fn fechar_menu(&mut self, devolver_foco: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            if devolver_foco {
                window.focus(&self.foco, cx);
            }
            cx.notify();
        }
    }

    fn escolher_menu(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let acao = match self.menu.as_ref().and_then(|m| m.itens.get(i)) {
            Some(ItemMenu::Acao { acao: Some(a), .. }) => a.clone(),
            _ => return,
        };
        self.fechar_menu(true, window, cx);
        self.executar(acao, window, cx);
    }

    fn executar(&mut self, acao: Acao, window: &mut Window, cx: &mut Context<Self>) {
        match acao {
            Acao::NovoArquivo(dir) => self.iniciar_criar(false, dir, window, cx),
            Acao::NovaPasta(dir) => self.iniciar_criar(true, dir, window, cx),
            Acao::AbrirAoLado(p) => cx.emit(EventoArvore::Abrir { caminho: p, fixo: false, ao_lado: true }),
            Acao::Revelar(p, pasta) => self.revelar_no_windows(p, pasta, cx),
            Acao::Terminal(d) => cx.emit(EventoArvore::AbrirNoTerminal(d)),
            Acao::Historico(p) => cx.emit(EventoArvore::HistoricoGit(p)),
            Acao::Pesquisar(d) => cx.emit(EventoArvore::Pesquisar(d)),
            Acao::Chat(ps) => {
                for p in ps {
                    cx.emit(EventoArvore::AdicionarAoChat(p));
                }
            }
            Acao::Recortar(ps) => self.copiar_itens(ps, true, cx),
            Acao::Copiar(ps) => self.copiar_itens(ps, false, cx),
            Acao::Colar(d) => self.colar(d, cx),
            Acao::CopiarCaminhos(t, ps) => self.copiar_caminhos(t, &ps, cx),
            Acao::Renomear(p) => self.iniciar_renomear(p, window, cx),
            Acao::Excluir(ps) => self.iniciar_excluir(ps, window, cx),
        }
    }

    // ---------- ações que só chamam o core ----------

    fn em_segundo_plano<R: Send + 'static>(
        &self, cx: &mut Context<Self>, trabalho: impl FnOnce(PathBuf) -> R + Send + 'static,
        depois: impl FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
    ) {
        let raiz = self.raiz.clone();
        let tarefa = cx.background_spawn(async move { trabalho(raiz) });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| depois(this, r, cx));
        })
        .detach();
    }

    fn relatar(&self, e: String, cx: &mut App) {
        self.aviso(Aviso::erro(e, 7000), cx);
    }

    fn revelar_no_windows(&mut self, caminho: String, pasta: bool, cx: &mut Context<Self>) {
        self.em_segundo_plano(
            cx,
            move |raiz| if pasta { files::reveal_dir(&raiz, &caminho) } else { files::reveal(&raiz, &caminho) },
            |this, r, cx| {
                if let Err(e) = r {
                    this.relatar(e, cx);
                }
            },
        );
    }

    fn copiar_itens(&mut self, caminhos: Vec<String>, recorte: bool, cx: &mut Context<Self>) {
        let n = caminhos.len();
        self.em_segundo_plano(
            cx,
            move |raiz| files::copy_items(&raiz, &caminhos, recorte),
            move |this, r, cx| match r {
                Ok(()) => this.aviso(Aviso::sucesso(modelo::texto_copiados(n, recorte), 4000), cx),
                Err(e) => this.relatar(e, cx),
            },
        );
    }

    fn copiar_caminhos(&mut self, tipo: TipoCaminho, caminhos: &[String], cx: &mut Context<Self>) {
        let projeto = self.raiz.to_string_lossy().to_string();
        let textos: Vec<String> = caminhos.iter().map(|c| modelo::texto_caminho(tipo, c, &projeto)).collect();
        if textos.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(textos.join("\n")));
        let titulo = if textos.len() == 1 { "Caminho copiado".to_string() } else { format!("{} caminhos copiados", textos.len()) };
        self.aviso(Aviso::sucesso(titulo, 2000), cx);
    }

    /// Primeiro destino que já tem aba aberta: mover ou colar ali criaria dois documentos no mesmo caminho.
    fn colisao(&self, alvos: &[String]) -> Option<String> {
        alvos
            .iter()
            .find(|t| self.abas.iter().any(|p| modelo::sob(t, p)))
            .map(|t| format!("Feche a aba aberta em “{t}” antes de mover ou colar aqui."))
    }

    /// Cola o clipboard de arquivos em `dir`. Recorte tira os itens de onde estavam e emite [`EventoArvore::Movidos`].
    fn colar(&mut self, dir: String, cx: &mut Context<Self>) {
        let snap = platform::snapshot();
        if snap.paths.is_empty() {
            self.relatar("Não há arquivo ou pasta copiado para colar.".into(), cx);
            return;
        }
        let projeto = self.raiz.to_string_lossy().to_string();
        if snap.cut {
            let origens: Vec<String> =
                snap.paths.iter().filter_map(|p| modelo::relativo_a(&projeto, &p.to_string_lossy())).collect();
            let alvos: Vec<String> = origens.iter().map(|s| modelo::juntar(&dir, modelo::nome_de(s))).collect();
            if let Some(e) = self.colisao(&alvos) {
                self.relatar(e, cx);
                return;
            }
        }
        let d = dir.clone();
        self.em_segundo_plano(
            cx,
            move |raiz| {
                let r = files::paste_detailed(&raiz, &d, &snap);
                if snap.cut && !r.done.is_empty() {
                    platform::clear_if_same(&snap);
                }
                (r, snap.cut)
            },
            move |this, (lote, recorte), cx| {
                let mut movidos = Vec::new();
                if recorte {
                    for m in &lote.done {
                        if let Some(de) = modelo::relativo_a(&projeto, &m.from) {
                            movidos.push(Moved { from: de, to: m.to.clone() });
                        }
                    }
                }
                if !movidos.is_empty() {
                    cx.emit(EventoArvore::Movidos(movidos.clone()));
                }
                let saiu: Vec<String> = movidos.iter().map(|m| m.from.clone()).collect();
                this.selecao.retain(|p| !saiu.iter().any(|m| modelo::sob(m, p)));
                for p in &saiu {
                    this.arvore.tirar(p);
                }
                this.mudou();
                this.reler_tudo(cx);
                if let Some(e) = lote.error {
                    this.relatar(e, cx);
                    return;
                }
                if !dir.is_empty() {
                    this.abrir_pasta(dir.clone(), cx);
                }
                this.aviso(Aviso::sucesso(modelo::texto_colados(saiu.len(), &dir), 3000), cx);
            },
        );
    }

    /// Move (arrastar e soltar): só o que passou na checagem vai ao core; parada no meio mostra o erro e reconcilia.
    fn mover_para(&mut self, caminhos: Vec<String>, dest: String, cx: &mut Context<Self>) {
        if let Some(p) = modelo::problema_soltar(&caminhos, &dest) {
            self.aviso(Aviso::alerta(p, 4000), cx);
            return;
        }
        let movendo: Vec<String> = caminhos.into_iter().filter(|p| modelo::dividir(p).0 != dest).collect();
        let alvos: Vec<String> = movendo.iter().map(|p| modelo::juntar(&dest, modelo::nome_de(p))).collect();
        if let Some(e) = self.colisao(&alvos) {
            self.relatar(e, cx);
            return;
        }
        let (m, d) = (movendo.clone(), dest.clone());
        self.em_segundo_plano(
            cx,
            move |raiz| files::move_items(&raiz, &m, &d),
            move |this, lote, cx| {
                if !lote.done.is_empty() {
                    cx.emit(EventoArvore::Movidos(lote.done.clone()));
                }
                let mut dirs: Vec<String> = movendo.iter().map(|p| modelo::dividir(p).0.to_string()).collect();
                dirs.push(dest.clone());
                let saiu: Vec<String> = lote.done.iter().map(|m| m.from.clone()).collect();
                this.assentar(dirs, &saiu, &lote.done, cx);
                if !lote.done.is_empty() && !dest.is_empty() {
                    this.abrir_pasta(dest.clone(), cx);
                }
                match lote.error {
                    Some(e) => this.relatar(modelo::texto_falha_mover(&e, lote.done.len()), cx),
                    None => this.aviso(Aviso::sucesso(modelo::texto_movidos(lote.done.len(), &dest), 3000), cx),
                }
            },
        );
    }

    // ---------- criar e renomear (o campo na linha) ----------

    /// Novo Arquivo e Nova Pasta da barra: na pasta selecionada (ou na do arquivo selecionado); sem seleção única, na raiz.
    fn dir_da_barra(&self) -> String {
        let um = if self.selecao.len() == 1 { self.selecao[0].clone() } else { String::new() };
        if !um.is_empty() && self.arvore.eh_arquivo(&um) {
            modelo::dividir(&um).0.to_string()
        } else {
            um
        }
    }

    fn iniciar_criar(&mut self, pasta: bool, dir: String, window: &mut Window, cx: &mut Context<Self>) {
        self.recolhida = false;
        self.renomeando = None;
        if !dir.is_empty() && !self.arvore.aberta(&dir) {
            self.abrir_pasta(dir.clone(), cx);
        }
        self.criando = Some(CriandoItem { pasta, dir: dir.clone(), erro: String::new(), ocupado: false });
        self.mudou();
        self.preparar_campo("", 0, window, cx);
        self.rolar_para = Some(format!("{}/", modelo::juntar(&dir, "\0novo")));
        cx.notify();
    }

    fn iniciar_renomear(&mut self, caminho: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.selecao.len() > 1 {
            self.aviso(Aviso::alerta("Renomeie um item de cada vez.", 3000), cx);
            return;
        }
        self.criando = None;
        self.mudou();
        let nome = modelo::nome_de(&caminho).to_string();
        let fim = modelo::fim_do_nome_base(&nome, self.arvore.eh_arquivo(&caminho));
        self.renomeando = Some(Renomeando { caminho: caminho.clone(), erro: String::new(), ocupado: false });
        self.preparar_campo(&nome, fim, window, cx);
        self.rolar_para = Some(caminho);
        cx.notify();
    }

    fn preparar_campo(&mut self, valor: &str, fim_selecao: usize, window: &mut Window, cx: &mut Context<Self>) {
        let valor = valor.to_string();
        self.campo.update(cx, |c, cx| {
            c.set_value(valor, window, cx);
            c.set_selected_range(0..fim_selecao, cx);
            c.set_disabled(false, cx);
        });
        let foco = self.campo.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
    }

    fn valor_campo(&self, cx: &App) -> String {
        self.campo.read(cx).value().to_string()
    }

    fn no_campo(&mut self, _: &Entity<InputState>, ev: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            InputEvent::Change => {
                if let Some(c) = &mut self.criando {
                    c.erro.clear();
                }
                if let Some(r) = &mut self.renomeando {
                    r.erro.clear();
                }
                cx.notify();
            }
            InputEvent::PressEnter { .. } => self.confirmar_campo(false, window, cx),
            InputEvent::Blur => {
                // Sair do campo confirma se o nome é válido e cancela se não é.
                if self.criando.as_ref().is_some_and(|c| !c.ocupado) {
                    let nome = self.valor_campo(cx);
                    if !nome.trim().is_empty() && self.erro_criando(&nome, cx).is_none() {
                        self.confirmar_campo(true, window, cx);
                    } else {
                        self.cancelar_campo(true, window, cx);
                    }
                } else if let Some(r) = self.renomeando.clone().filter(|r| !r.ocupado) {
                    let nome = self.valor_campo(cx);
                    if !r.erro.is_empty() || nome.trim().is_empty() || modelo::problema_renomear(&self.arvore, &r.caminho, &nome).is_some() {
                        self.cancelar_campo(true, window, cx);
                    } else {
                        self.confirmar_campo(true, window, cx);
                    }
                }
            }
            InputEvent::Focus => {}
        }
    }

    fn erro_criando(&self, nome: &str, _cx: &App) -> Option<String> {
        let c = self.criando.as_ref()?;
        if !c.erro.is_empty() {
            return Some(c.erro.clone());
        }
        let t = nome.trim();
        if t.is_empty() {
            None
        } else {
            modelo::erro_nome(t)
        }
    }

    /// O motivo mostrado embaixo do campo, ou None.
    fn erro_do_campo(&self, cx: &App) -> Option<String> {
        let nome = self.valor_campo(cx);
        if self.criando.is_some() {
            return self.erro_criando(&nome, cx);
        }
        let r = self.renomeando.as_ref()?;
        if !r.erro.is_empty() {
            return Some(r.erro.clone());
        }
        modelo::problema_renomear(&self.arvore, &r.caminho, &nome)
    }

    fn cancelar_campo(&mut self, saindo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let voltar = self.renomeando.take().map(|r| r.caminho);
        if self.criando.take().is_some() {
            self.mudou();
        }
        if !saindo {
            window.focus(&self.foco, cx);
            if let Some(c) = voltar {
                self.cursor = Some(c.clone());
                self.rolar_para = Some(c);
            }
        }
        cx.notify();
    }

    fn confirmar_campo(&mut self, saindo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.criando.is_some() {
            self.enviar_criar(window, cx);
        } else if self.renomeando.is_some() {
            self.enviar_renomear(saindo, window, cx);
        }
    }

    /// Criar: o nome é checado aqui para mostrar o motivo; o core confere de novo e nunca sobrescreve. Arquivo novo
    /// abre fixo; pasta nova só aparece.
    fn enviar_criar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.criando.clone() else { return };
        if c.ocupado {
            return;
        }
        let nome = self.valor_campo(cx).trim().to_string();
        if nome.is_empty() {
            self.cancelar_campo(false, window, cx);
            return;
        }
        if let Some(e) = modelo::erro_nome(&nome) {
            self.criando.as_mut().unwrap().erro = e;
            cx.notify();
            return;
        }
        self.criando.as_mut().unwrap().ocupado = true;
        self.campo.update(cx, |c, cx| c.set_disabled(true, cx));
        let (dir, pasta) = (c.dir.clone(), c.pasta);
        self.em_segundo_plano(
            cx,
            move |raiz| if pasta { files::create_dir(&raiz, &dir, &nome) } else { files::create_file(&raiz, &dir, &nome) },
            move |this, r, cx| match r {
                Ok(criado) => {
                    this.criando = None;
                    this.mudou();
                    if c.dir.is_empty() {
                        this.reler(Vec::new(), cx);
                    } else {
                        this.abrir_pasta(c.dir.clone(), cx);
                    }
                    if !pasta {
                        cx.emit(EventoArvore::Abrir { caminho: criado, fixo: true, ao_lado: false });
                    }
                    this.aviso(Aviso::sucesso(if pasta { "Pasta criada" } else { "Arquivo criado" }, 3000), cx);
                    cx.notify();
                }
                Err(e) => {
                    if let Some(c) = &mut this.criando {
                        c.ocupado = false;
                        c.erro = e;
                    }
                    this.campo.update(cx, |c, cx| c.set_disabled(false, cx));
                    cx.notify();
                }
            },
        );
    }

    fn enviar_renomear(&mut self, saindo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.renomeando.clone() else { return };
        if r.ocupado {
            return;
        }
        let nome = self.valor_campo(cx);
        if nome == modelo::nome_de(&r.caminho) {
            self.cancelar_campo(saindo, window, cx);
            return;
        }
        if let Some(e) = modelo::problema_renomear(&self.arvore, &r.caminho, &nome) {
            self.renomeando.as_mut().unwrap().erro = e;
            cx.notify();
            return;
        }
        self.renomeando.as_mut().unwrap().ocupado = true;
        self.campo.update(cx, |c, cx| c.set_disabled(true, cx));
        let pasta = !self.arvore.eh_arquivo(&r.caminho);
        let reabrir = pasta && self.arvore.aberta(&r.caminho);
        let caminho = r.caminho.clone();
        self.em_segundo_plano(
            cx,
            move |raiz| files::rename(&raiz, &caminho, &nome),
            move |this, res, cx| match res {
                Ok(novo) => {
                    this.renomeando = None;
                    cx.emit(EventoArvore::Renomeado { de: r.caminho.clone(), para: novo.clone() });
                    let dir = modelo::dividir(&r.caminho).0.to_string();
                    let saiu = if pasta { vec![r.caminho.clone()] } else { Vec::new() };
                    this.assentar(vec![dir], &saiu, &[Moved { from: r.caminho.clone(), to: novo.clone() }], cx);
                    this.selecao = vec![novo.clone()];
                    this.ancora = Some(novo.clone());
                    if !saindo {
                        this.cursor = Some(novo.clone());
                        this.rolar_para = Some(novo.clone());
                        this.focar_painel = true;
                    }
                    if reabrir {
                        this.abrir_pasta(novo, cx);
                    }
                    this.aviso(Aviso::sucesso(if pasta { "Pasta renomeada" } else { "Arquivo renomeado" }, 3000), cx);
                    cx.notify();
                }
                Err(e) => {
                    if let Some(r) = &mut this.renomeando {
                        r.ocupado = false;
                        r.erro = e;
                    }
                    this.campo.update(cx, |c, cx| c.set_disabled(false, cx));
                    cx.notify();
                }
            },
        );
    }

    // ---------- excluir ----------

    fn iniciar_excluir(&mut self, caminhos: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.excluindo = Some(Excluindo { caminhos, erro: String::new(), ocupado: false });
        window.focus(&self.foco_dialogo, cx);
        cx.notify();
    }

    fn fechar_excluir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.excluindo.as_ref().is_some_and(|e| !e.ocupado) {
            self.excluindo = None;
            window.focus(&self.foco, cx);
            cx.notify();
        }
    }

    /// Vai para a Lixeira. Parada no meio mantém o diálogo com o que sobrou e o erro; o feito sai da árvore.
    fn confirmar_excluir(&mut self, cx: &mut Context<Self>) {
        let Some(e) = self.excluindo.as_mut() else { return };
        if e.ocupado {
            return;
        }
        e.ocupado = true;
        e.erro.clear();
        let caminhos = e.caminhos.clone();
        cx.notify();
        let pedido = caminhos.clone();
        self.em_segundo_plano(
            cx,
            move |raiz| files::recycle_many(&raiz, &pedido),
            move |this, lote, cx| {
                if !lote.done.is_empty() {
                    cx.emit(EventoArvore::Excluidos(lote.done.clone()));
                }
                let dirs = caminhos.iter().map(|p| modelo::dividir(p).0.to_string()).collect();
                this.assentar(dirs, &lote.done, &[], cx);
                match lote.error {
                    None => {
                        this.excluindo = None;
                        this.focar_painel = true;
                        this.aviso(Aviso::sucesso(modelo::texto_excluidos(lote.done.len()), 3000), cx);
                    }
                    Some(erro) => {
                        let resto = caminhos.iter().filter(|p| !lote.done.iter().any(|g| modelo::sob(g, p))).cloned().collect();
                        this.excluindo = Some(Excluindo { caminhos: resto, erro, ocupado: false });
                    }
                }
                cx.notify();
            },
        );
    }

    // ---------- teclado ----------

    fn chave_cursor(&mut self) -> Option<String> {
        self.garantir_linhas();
        let c = self.cursor.clone()?;
        self.linhas.iter().any(|l| l.chave == c).then_some(c)
    }

    fn ir_para(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(l) = self.linhas.get(i) {
            self.cursor = Some(l.chave.clone());
            self.rolagem.scroll_to_item(i, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    /// Teclas da árvore (`explorer-tree.tsx`) e do painel (`onTreeKey`). Com o cursor numa linha, valem para a seleção (ou
    /// só para a linha, se ela não está nela); sem cursor, para a raiz.
    fn tecla(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.foco.is_focused(window) {
            return;
        }
        let k = &ev.keystroke;
        let m = k.modifiers;
        let tecla = k.key.as_str();
        self.garantir_linhas();
        let linhas = self.linhas.clone();
        let cursor = self.chave_cursor();
        let indice = cursor.as_ref().and_then(|c| linhas.iter().position(|l| &l.chave == c));
        let real = cursor.as_ref().is_some_and(|c| !modelo::provisoria(c));
        if cursor.is_some() && !real {
            return;
        }
        let ctrl = m.control && !m.alt && !m.platform;
        let simples = !m.control && !m.alt && !m.platform && !m.shift;
        let alt_shift = m.shift && m.alt && !m.control && !m.platform;

        if ctrl && !m.shift && tecla == "a" {
            self.selecao = linhas.iter().filter(|l| l.real()).map(|l| l.chave.clone()).collect();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if (m.shift && tecla == "f10") || tecla == "menu" || tecla == "contextmenu" || tecla == "apps" {
            cx.stop_propagation();
            let posicao = self.posicao_menu_teclado(indice, window);
            match cursor.filter(|_| real).and_then(|c| linhas.iter().find(|l| l.chave == c).cloned()) {
                Some(l) => self.clique_direito_linha(&l, posicao, window, cx),
                None => {
                    let itens = self.menu_raiz();
                    self.abrir_menu(itens, posicao, window, cx);
                }
            }
            return;
        }
        // Navegação da árvore (sem Ctrl nem Alt).
        if !m.control && !m.alt && !m.platform && !linhas.is_empty() {
            let n = linhas.len();
            let ultimo = n - 1;
            let ativa = indice.or_else(|| self.marcados().iter().find_map(|k| linhas.iter().position(|l| &l.chave == k))).unwrap_or(0);
            let linha = linhas.get(indice.unwrap_or(ativa)).cloned();
            let feito = match tecla {
                "down" => {
                    self.ir_para(indice.map_or(0, |i| (i + 1) % n), cx);
                    true
                }
                "up" => {
                    self.ir_para(indice.map_or(ultimo, |i| (i + n - 1) % n), cx);
                    true
                }
                "home" => {
                    self.ir_para(0, cx);
                    true
                }
                "end" => {
                    self.ir_para(ultimo, cx);
                    true
                }
                "left" => {
                    if let Some(l) = linha.clone() {
                        if l.expansivel && self.arvore.aberta(&l.chave) {
                            self.alternar(&l.chave, cx);
                        } else if !l.pai.is_empty() {
                            if let Some(i) = linhas.iter().position(|x| x.chave == l.pai) {
                                self.ir_para(i, cx);
                            }
                        }
                    }
                    true
                }
                "right" => {
                    if let Some(l) = linha.clone().filter(|l| l.expansivel) {
                        if self.arvore.aberta(&l.chave) {
                            if let Some(i) = linhas.iter().position(|x| x.chave == l.chave) {
                                self.ir_para(i + 1, cx);
                            }
                        } else {
                            self.alternar(&l.chave, cx);
                        }
                    }
                    true
                }
                "space" => {
                    if let Some(l) = linha.clone().filter(|_| indice.is_some()) {
                        self.clicar_linha(&l, Mods { ctrl: false, shift: m.shift }, window, cx);
                    }
                    true
                }
                _ if k.key_char.as_deref() == Some("*") || tecla == "*" => {
                    if let Some(l) = linha.clone() {
                        let irmas: Vec<String> = linhas
                            .iter()
                            .filter(|r| r.pai == l.pai && r.expansivel && !self.arvore.aberta(&r.chave))
                            .map(|r| r.chave.clone())
                            .collect();
                        for p in irmas {
                            self.abrir_pasta(p, cx);
                        }
                    }
                    true
                }
                _ => false,
            };
            if feito {
                cx.stop_propagation();
                return;
            }
            if tecla == "enter" {
                if let Some(l) = linha.clone().filter(|_| indice.is_some()) {
                    self.clicar_linha(&l, Mods { ctrl: false, shift: m.shift }, window, cx);
                }
            }
        }

        let alvos: Vec<String> = match &cursor {
            Some(c) if real => {
                if self.selecao.contains(c) {
                    self.selecao.clone()
                } else {
                    vec![c.clone()]
                }
            }
            _ => Vec::new(),
        };
        let caminho = if real { cursor.clone().unwrap_or_default() } else { String::new() };
        if let Some(t) = self.acorde.take() {
            if cx.background_executor().now().duration_since(t) <= Duration::from_millis(1500) {
                if ctrl && !m.shift && tecla == "k" {
                    self.acorde = Some(cx.background_executor().now());
                    cx.stop_propagation();
                    return;
                }
                if ctrl && m.shift && tecla == "c" && !alvos.is_empty() {
                    cx.stop_propagation();
                    self.copiar_caminhos(TipoCaminho::Relativo, &alvos, cx);
                }
                return;
            }
        }
        let pasta = real && !self.arvore.eh_arquivo(&caminho);
        let dir_de = if real {
            if pasta { caminho.clone() } else { modelo::dividir(&caminho).0.to_string() }
        } else {
            String::new()
        };
        let mut tratado = true;
        if ctrl && !m.shift && tecla == "k" {
            self.acorde = Some(cx.background_executor().now());
        } else if ctrl && !m.shift && tecla == "v" {
            self.colar(dir_de, cx);
        } else if real && ctrl && !m.shift && tecla == "x" {
            self.copiar_itens(alvos.clone(), true, cx);
        } else if real && ctrl && !m.shift && tecla == "c" {
            self.copiar_itens(alvos.clone(), false, cx);
        } else if real && simples && tecla == "f2" {
            self.iniciar_renomear(caminho.clone(), window, cx);
        } else if real && simples && tecla == "delete" {
            self.iniciar_excluir(alvos.clone(), window, cx);
        } else if real && simples && tecla == "enter" && !pasta {
            cx.emit(EventoArvore::Abrir { caminho: caminho.clone(), fixo: true, ao_lado: false });
        } else if alt_shift && tecla == "r" {
            self.revelar_no_windows(caminho.clone(), !real || pasta, cx);
        } else if alt_shift && tecla == "f" {
            cx.emit(EventoArvore::Pesquisar(dir_de));
        } else if alt_shift && tecla == "c" {
            let alvos = if real { alvos.clone() } else { vec![String::new()] };
            self.copiar_caminhos(TipoCaminho::Absoluto, &alvos, cx);
        } else {
            tratado = tecla == "enter";
        }
        if !tratado {
            return;
        }
        if real && !self.selecao.contains(&caminho) {
            self.selecao = vec![caminho];
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn posicao_menu_teclado(&self, indice: Option<usize>, window: &Window) -> Point<Pixels> {
        // Sem ponteiro: abre na linha (24 px da borda, no meio dela) ou no canto do painel.
        let base = self.origem_lista;
        let topo = self.rolagem.0.borrow().base_handle.offset().y;
        let desenhada = indice
            .and_then(|i| self.linhas.get(i))
            .and_then(|l| self.limites_linhas.borrow().get(&l.chave).copied());
        if let Some(b) = desenhada {
            // A conta do React: `rect.left + 24`, `rect.top + rect.height / 2` da linha.
            return point(b.origin.x + px(24.), b.origin.y + b.size.height / 2.);
        }
        match indice {
            // Fora da vista: a mesma conta pela posição virtual (a lista tem 2 px de respiro em cima).
            Some(i) => point(base.x + px(24.), base.y + topo + px(2. + ALTURA_LINHA * i as f32 + ALTURA_LINHA / 2.)),
            None => {
                let _ = window;
                point(base.x + px(24.), base.y + px(ALTURA_LINHA / 2.))
            }
        }
    }

    fn tecla_menu(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_mut() else { return };
        let ativos: Vec<usize> = menu
            .itens
            .iter()
            .enumerate()
            .filter(|(_, i)| matches!(i, ItemMenu::Acao { acao: Some(_), .. }))
            .map(|(i, _)| i)
            .collect();
        cx.stop_propagation();
        if ativos.is_empty() {
            if ev.keystroke.key == "escape" {
                self.fechar_menu(true, window, cx);
            }
            return;
        }
        let pos = menu.destacado.and_then(|d| ativos.iter().position(|a| *a == d));
        match ev.keystroke.key.as_str() {
            "down" => menu.destacado = Some(ativos[pos.map_or(0, |p| (p + 1) % ativos.len())]),
            "up" => menu.destacado = Some(ativos[pos.map_or(ativos.len() - 1, |p| (p + ativos.len() - 1) % ativos.len())]),
            "home" => menu.destacado = Some(ativos[0]),
            "end" => menu.destacado = Some(ativos[ativos.len() - 1]),
            "enter" | "space" => {
                if let Some(d) = menu.destacado {
                    self.escolher_menu(d, window, cx);
                }
                return;
            }
            "escape" => {
                self.fechar_menu(true, window, cx);
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    fn tecla_dialogo(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "enter" => self.confirmar_excluir(cx),
            "escape" => self.fechar_excluir(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    // ---------- arraste ----------

    fn destino_de(&self, linha: &Linha) -> Option<String> {
        match linha.tipo {
            TipoLinha::Espera => Some(linha.chave.trim_end_matches('/').to_string()),
            TipoLinha::Nova => None,
            TipoLinha::Arquivo => Some(linha.pai.clone()),
            TipoLinha::Pasta => Some(linha.chave.clone()),
        }
    }

    fn arrastar_sobre(&mut self, dest: Option<String>, caminhos: &[String], cx: &mut Context<Self>) {
        let novo = dest.map(|d| {
            let p = modelo::problema_soltar(caminhos, &d);
            (d, p)
        });
        if novo.is_some() && self.soltar != novo {
            self.soltar = novo;
            cx.notify();
        }
    }

    fn soltar_aqui(&mut self, a: &Arrasto, cx: &mut Context<Self>) {
        let Some((dest, _)) = self.soltar.take() else { return };
        self.mover_para(a.caminhos.clone(), dest, cx);
        cx.notify();
    }
}

fn juntar_secoes(secoes: Vec<Vec<ItemMenu>>) -> Vec<ItemMenu> {
    let mut out = Vec::new();
    for (i, s) in secoes.into_iter().enumerate() {
        if i > 0 {
            out.push(ItemMenu::Divisor);
        }
        out.extend(s);
    }
    out
}

fn hsla(c: u32) -> Hsla {
    rgb(c).into()
}

/// `--ant-color-fill-secondary` (seleção) e `fill-tertiary` (hover), medidos nas capturas.
fn fundo_selecionado() -> Hsla {
    rgba(0xffffff1a).into()
}
fn fundo_hover() -> Hsla {
    rgba(0xffffff0f).into()
}
/// Cor das guias de recuo (`--ant-color-border-secondary`).
fn cor_guia() -> Hsla {
    rgb(0x1a1a1a).into()
}
fn cor_primaria() -> Hsla {
    cor::ink_white()
}

// ---------- desenho ----------

impl Explorador {
    fn barra(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let foco_dentro = self.foco.contains_focused(window, cx);
        let acao = |id: &'static str, caminho: &'static str, dica: &'static str| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .flex_none()
                .size(px(24.))
                .rounded(tema::RAIO_SM)
                .text_color(cor::text_tertiary())
                .hover(|s| s.bg(cor::fill_hover()))
                .tooltip_texto(dica)
                .child(icone(caminho, 14., cor::text_tertiary()))
                .test_support()
        };
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .h(tema::FAIXA)
            .px(px(12.))
            .flex_none()
            .border_b_1()
            .border_color(cor::hairline())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(tema::TEXTO_LABEL)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cor::text_secondary())
                    .child("Arquivos"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(2.))
                    .opacity(if foco_dentro { 1. } else { 0. })
                    .group_hover("explorador", |s| s.opacity(1.))
                    .child(acao("arvore-novo-arquivo", "icons/file-plus.svg", "Novo Arquivo...").on_click(cx.listener(
                        |this, _, window, cx| {
                            let d = this.dir_da_barra();
                            this.iniciar_criar(false, d, window, cx);
                        },
                    )))
                    .child(acao("arvore-nova-pasta", "icons/folder-plus.svg", "Nova Pasta...").on_click(cx.listener(
                        |this, _, window, cx| {
                            let d = this.dir_da_barra();
                            this.iniciar_criar(true, d, window, cx);
                        },
                    )))
                    .child(
                        acao("arvore-atualizar", "icons/refresh-cw.svg", "Atualizar")
                            .on_click(cx.listener(|this, _, _, cx| this.reler_tudo(cx))),
                    )
                    .child(acao("arvore-recolher", "icons/chevrons-down-up.svg", "Recolher Tudo").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.arvore.abertas.clear();
                            this.mudou();
                            cx.notify();
                        },
                    ))),
            )
    }

    fn secao(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let chevron = icone("icons/chevron-right.svg", 16., cor::text_primary());
        let chevron = if self.recolhida {
            chevron
        } else {
            chevron.with_transformation(gpui_kit::Transformation::rotate(gpui_kit::percentage(0.25)))
        };
        div()
            .id("arvore-secao")
            .mt(px(4.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(ALTURA_LINHA))
            .px(px(8.))
            .cursor_pointer()
            .text_size(tema::TEXTO_MENU)
            .font_weight(FontWeight::MEDIUM)
            .text_color(cor::text_primary())
            .hover(|s| s.bg(fundo_hover()))
            .on_click(cx.listener(|this, _, _, cx| {
                this.recolhida = !this.recolhida;
                if !this.recolhida {
                    this.cutucar(cx);
                }
                cx.notify();
            }))
            .child(chevron)
            .child(div().truncate().line_height(px(ALTURA_LINHA)).child(self.projeto.clone()))
            .test_support()
    }

    fn linha(&self, i: usize, linha: &Linha, marcados: &[String], window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let aberta = linha.expansivel && self.arvore.aberta(&linha.chave);
        let selecionada = marcados.contains(&linha.chave);
        let moldura = self.soltar.as_ref().filter(|(d, _)| linha.real() && *d == linha.chave).map(|(_, p)| p.is_some());
        let grupo = SharedString::from(format!("arvore-linha-{i}"));
        let renomeando = self.renomeando.as_ref().is_some_and(|r| r.caminho == linha.chave);
        let editando = renomeando || linha.tipo == TipoLinha::Nova;

        // Fundo do hover e da seleção, recuado 4 px de cada lado.
        let fundo = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(4.))
            .right(px(4.))
            .rounded(tema::RAIO_XS)
            .when(selecionada, |d| d.bg(fundo_selecionado()))
            .when(!selecionada && !editando, |d| d.group_hover(grupo.clone(), |s| s.bg(fundo_hover())));

        // Guias de recuo: uma linha de 1 px por nível, a 16 px + 8 px * nível; aparecem com o ponteiro na árvore.
        let mut guias = div().absolute().top_0().bottom_0().left(px(16.)).invisible().group_hover("arvore-painel", |s| s.visible());
        for n in 0..linha.nivel {
            guias = guias.child(div().absolute().top_0().bottom_0().left(px(RECUO * n as f32)).w(px(1.)).bg(cor_guia()));
        }

        let chave = linha.chave.clone();
        let seta = {
            let ic = icone("icons/chevron-right.svg", 14., cor::text_secondary());
            let ic = if aberta { ic.with_transformation(gpui_kit::Transformation::rotate(gpui_kit::percentage(0.25))) } else { ic };
            let l = linha.clone();
            div()
                .id(SharedString::from(format!("arvore-seta-{i}")))
                .ml(px(8.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(16.))
                .h(px(ALTURA_LINHA))
                .rounded(tema::RAIO_XS)
                .when(!linha.expansivel, |d| d.invisible())
                .when(linha.expansivel, |d| {
                    d.hover(|s| s.bg(fundo_selecionado())).child(ic).on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.alternar(&l.chave, cx);
                    }))
                })
        };

        let icone_linha: AnyElement = match linha.tipo {
            TipoLinha::Espera => div().mr(px(6.)).child(icone("icons/file.svg", 16., cor::text_secondary())).into_any_element(),
            TipoLinha::Nova => {
                let c = self.criando.as_ref();
                let pasta = c.is_some_and(|c| c.pasta);
                let nome = self.valor_campo(cx);
                let nome = if nome.trim().is_empty() { if pasta { "pasta".into() } else { "arquivo".into() } } else { nome.trim().to_string() };
                img(icone_arquivo(&nome, pasta, false)).size(px(16.)).flex_none().mr(px(6.)).into_any_element()
            }
            _ => {
                let e = linha.entrada.as_ref().unwrap();
                img(icone_arquivo(&e.name, e.dir, self.arvore.aberta(&linha.chave))).size(px(16.)).flex_none().mr(px(6.)).into_any_element()
            }
        };

        let titulo: AnyElement = match linha.tipo {
            TipoLinha::Espera => div()
                .text_size(tema::TEXTO_LABEL)
                .text_color(cor::text_secondary())
                .child("Lendo a pasta…")
                .into_any_element(),
            _ if editando => self.campo_na_linha(window, cx),
            _ => {
                let e = linha.entrada.as_ref().unwrap();
                let deco = self.decoracoes.get(&linha.chave).cloned();
                let (cor_nome, letra, dica): (Option<Hsla>, String, String) = match (&deco, e.ignored) {
                    (Some(d), _) => (Some(hsla(d.cor)), d.letra.clone(), format!("{} · {}", e.name, d.titulo)),
                    (None, true) => (Some(hsla(modelo::IGNORADO)), String::new(), format!("{} · {}", e.name, modelo::TITULO_IGNORADO)),
                    (None, false) => (None, String::new(), e.name.clone()),
                };
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id(SharedString::from(format!("arvore-nome-{i}")))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when_some(cor_nome, |d, c| d.text_color(c))
                            .tooltip_texto(dica)
                            .child(e.name.clone()),
                    )
                    .when(!letra.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .text_size(tema::TEXTO_LABEL)
                                .font_weight(FontWeight::MEDIUM)
                                .when_some(cor_nome, |d, c| d.text_color(c))
                                .child(letra),
                        )
                    })
                    .into_any_element()
            }
        };

        let l1 = linha.clone();
        let l2 = linha.clone();
        let l3 = linha.clone();
        let l4 = linha.clone();
        let mut el = div()
            .id(SharedString::from(format!("arvore-linha-{chave}")))
            .group(grupo)
            .relative()
            .flex()
            .items_center()
            .h(px(ALTURA_LINHA))
            .w_full()
            .pl(px(RECUO * linha.nivel as f32))
            .pr(px(8.))
            .cursor_pointer()
            .text_size(tema::TEXTO_BODY)
            .text_color(cor::text_primary())
            .child(fundo)
            .child(guias)
            .when_some(moldura, |d, problema| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_1()
                        .rounded(tema::RAIO_XS)
                        .border_color(if problema { cor::signal_error() } else { cor_primaria() }),
                )
            })
            .child(seta)
            .child(icone_linha)
            .child(div().flex().flex_1().min_w_0().items_center().when(!editando, |d| d.pt(px(1.)).pb(px(3.))).child(titulo));
        if !editando {
            el = el
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.agarrado.set(ev.position);
                    cx.stop_propagation();
                    let m = ev.modifiers;
                    if l1.tipo == TipoLinha::Arquivo && !m.control && !m.shift && !m.platform {
                        cx.emit(EventoArvore::PreCarregar(l1.chave.clone()));
                    }
                    if this.menu.is_some() {
                        this.menu = None;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    let m = ev.modifiers();
                    if ev.click_count() >= 2 {
                        this.duplo_clique(&l2, cx);
                        return;
                    }
                    this.clicar_linha(&l2, Mods { ctrl: m.control || m.platform, shift: m.shift }, window, cx);
                }))
                .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.clique_direito_linha(&l3, ev.position, window, cx);
                }));
        } else {
            el = el.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        }
        {
            let limites = self.limites_linhas.clone();
            let k = linha.chave.clone();
            el = el.child(
                gpui_kit::canvas(move |b, _, _| {
                    limites.borrow_mut().insert(k.clone(), b);
                }, |_, _, _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        }
        if linha.real() && !renomeando {
            let caminhos = if self.selecao.contains(&linha.chave) { self.selecao.clone() } else { vec![linha.chave.clone()] };
            let e = linha.entrada.as_ref().unwrap();
            let deco = self.decoracoes.get(&linha.chave);
            let copia = CopiaLinha {
                largura: px(0.),
                nivel: linha.nivel,
                expansivel: linha.expansivel,
                aberta,
                icone: icone_arquivo(&e.name, e.dir, aberta),
                nome: e.name.clone().into(),
                cor_nome: match (deco, e.ignored) {
                    (Some(d), _) => Some(hsla(d.cor)),
                    (None, true) => Some(hsla(modelo::IGNORADO)),
                    (None, false) => None,
                },
                letra: deco.map(|d| d.letra.clone()).unwrap_or_default().into(),
                selecionada,
            };
            let limites = self.limites_linhas.clone();
            let fraco = cx.weak_entity();
            let chave_arrasto = linha.chave.clone();
            let agarrado = self.agarrado.clone();
            el = el.on_drag(Arrasto { caminhos }, move |_, deslocamento, _, cx| {
                let k = chave_arrasto.clone();
                let _ = fraco.update(cx, |this, cx| {
                    if !this.selecao.contains(&k) {
                        this.selecao = vec![k];
                        cx.notify();
                    }
                });
                let mut copia = copia.clone();
                let b = limites.borrow().get(&chave_arrasto).copied().unwrap_or_default();
                copia.largura = if b.size.width > px(0.) { b.size.width } else { px(200.) };
                let ajuste = b.origin + deslocamento - agarrado.get();
                cx.new(|_| VistaArrasto { copia, ajuste })
            });
        }
        el = el.on_drag_move::<Arrasto>(cx.listener(move |this, ev: &DragMoveEvent<Arrasto>, _, cx| {
            if ev.bounds.contains(&ev.event.position) {
                let caminhos = ev.drag(cx).caminhos.clone();
                let dest = this.destino_de(&l4);
                this.arrastar_sobre(dest, &caminhos, cx);
            }
        }));
        el.test_support().into_any_element()
    }

    fn campo_na_linha(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let erro = self.erro_do_campo(cx);
        let focado = self.campo.read(cx).focus_handle(cx).is_focused(window);
        let borda = if erro.is_some() {
            cor::signal_error()
        } else if focado {
            cor_primaria()
        } else {
            cor::hairline()
        };
        let caixa = div()
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .h(px(ALTURA_LINHA))
            .px(px(4.))
            .rounded(tema::RAIO_XS)
            .border_1()
            .border_color(borda)
            .bg(cor::fill_muted())
            .text_size(tema::TEXTO_MENU)
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.cancelar_campo(false, window, cx);
                }
            }))
            .child(medidor_campo(cx.weak_entity()))
            .child(div().flex_1().min_w_0().child(Input::new(&self.campo).appearance(false).px_0().py_0().h(px(20.)).text_size(tema::TEXTO_MENU)));
        let Some(erro) = erro else { return caixa.into_any_element() };
        // O motivo vai fora da linha (ela corta o que passa dela), preso embaixo do campo.
        caixa
            .child(
                div().absolute().left(px(-1.)).right(px(-1.)).bottom_0().h_0().child(
                    deferred(
                        anchored().snap_to_window().child(
                            div()
                                .occlude()
                                .mt(px(1.))
                                .w(self.largura_campo)
                                .px(px(6.))
                                .py(px(4.))
                                .rounded_b(tema::RAIO_XS)
                                .border_1()
                                .border_t_0()
                                .border_color(cor::signal_error())
                                .bg(rgb(0x50242e))
                                .text_size(tema::TEXTO_LABEL)
                                .line_height(px(17.))
                                .text_color(cor::text_primary())
                                .child(erro),
                        ),
                    )
                    .with_priority(1),
                ),
            )
            .into_any_element()
    }

    fn menu_el(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        widgets::marcar_sobreposicao();
        let mut caixa = div()
            .id("arvore-menu")
            .track_focus(&menu.foco)
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(200.))
            .p(px(4.))
            .rounded(tema::RAIO_MD)
            .bg(cor::raised_black())
            .border_1()
            .border_color(rgba(0xffffff1a))
            .shadow_lg()
            .text_size(tema::TEXTO_MENU)
            .on_key_down(cx.listener(Self::tecla_menu))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.fechar_menu(false, window, cx)));
        for (i, it) in menu.itens.iter().enumerate() {
            match it {
                ItemMenu::Divisor => {
                    caixa = caixa.child(div().my(px(5.)).mx(px(-4.)).h(px(1.)).bg(cor::hairline()));
                }
                ItemMenu::Acao { rotulo, atalho, acao, motivo } => {
                    let habilitado = acao.is_some();
                    let destacado = menu.destacado == Some(i);
                    let mut linha = div()
                        .id(SharedString::from(format!("arvore-menu-{rotulo}")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(24.))
                        .h(tema::ITEM_MENU)
                        .pl(px(26.))
                        .pr(px(26.))
                        .rounded(tema::RAIO_MENU)
                        .text_color(if habilitado { cor::text_primary() } else { rgba(0xffffff40).into() })
                        .when(destacado, |d| d.bg(cor::fill_hover()))
                        .child(div().font_weight(FontWeight::MEDIUM).child(rotulo.clone()))
                        .when(habilitado, |d| d.child(div().text_color(cor::text_tertiary()).child(*atalho)));
                    if habilitado {
                        linha = linha
                            .on_hover(cx.listener(move |this, dentro: &bool, _, cx| {
                                if let Some(m) = this.menu.as_mut() {
                                    if *dentro && m.destacado != Some(i) {
                                        m.destacado = Some(i);
                                        cx.notify();
                                    } else if !*dentro && m.destacado == Some(i) {
                                        m.destacado = None;
                                        cx.notify();
                                    }
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| this.escolher_menu(i, window, cx)));
                    } else {
                        linha = linha.tooltip_texto(*motivo);
                    }
                    caixa = caixa.child(linha.test_support());
                }
            }
        }
        Some(
            deferred(anchored().position(menu.posicao).snap_to_window_with_margin(px(8.)).child(caixa))
                .with_priority(2)
                .into_any_element(),
        )
    }

    fn dialogo_el(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let e = self.excluindo.as_ref()?;
        let tem_pasta = e.caminhos.iter().any(|p| !self.arvore.eh_arquivo(p));
        let (pergunta, detalhe) = modelo::textos_excluir(&e.caminhos, tem_pasta);
        let sujos = modelo::sujos_de(&e.caminhos, &self.sujos);
        let ocupado = e.ocupado;
        let botao = |id: &'static str, primario: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .h(px(28.))
                .min_w(px(64.))
                .px(px(10.))
                .rounded(px(2.))
                .border_1()
                .border_color(rgba(0xffffff14))
                .text_size(tema::TEXTO_MENU)
                .when(primario, |d| {
                    d.bg(cor::ink_white()).text_color(cor::pitch_black()).when(!ocupado, |d| d.hover(|s| s.bg(cor::ink_white_hover())))
                })
                .when(!primario, |d| {
                    d.bg(rgba(0xffffff1a)).text_color(cor::text_primary()).when(!ocupado, |d| d.hover(|s| s.bg(rgba(0xffffff26))))
                })
                .when(ocupado, |d| d.opacity(0.4))
                .test_support()
        };
        let alerta = |texto: String, tom: Hsla, caminho: &'static str| {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .px(px(12.))
                .py(px(8.))
                .rounded(tema::RAIO_MD)
                .border_1()
                .border_color(tom.opacity(0.35))
                .bg(tom.opacity(0.1))
                .child(div().mt(px(3.)).child(icone(caminho, 14., tom)))
                .child(div().flex_1().min_w_0().child(texto))
        };
        let mut corpo = div().flex().flex_col().flex_1().min_w_0().pl(px(24.)).child(
            div()
                .flex()
                .items_center()
                .min_h(px(48.))
                .mb(px(8.))
                .text_size(px(18.))
                .line_height(px(22.))
                .child(div().flex_1().min_w_0().child(pergunta)),
        );
        corpo = corpo.child(div().child(detalhe));
        let mut extras = div().flex().flex_col().gap(px(8.)).mt(px(8.));
        let mut tem_extra = false;
        if e.caminhos.len() > 1 {
            tem_extra = true;
            let mut lista = div().id("arvore-excluir-lista").flex().flex_col().max_h(px(180.)).overflow_y_scroll();
            for p in e.caminhos.iter().take(LISTA_MAX) {
                lista = lista.child(div().truncate().child(p.clone()));
            }
            if e.caminhos.len() > LISTA_MAX {
                lista = lista.child(div().opacity(0.7).child(format!("e mais {}", e.caminhos.len() - LISTA_MAX)));
            }
            extras = extras.child(lista);
        }
        if let Some(a) = modelo::aviso_rascunhos(&sujos) {
            tem_extra = true;
            extras = extras.child(alerta(a, cor::signal_warning(), "icons/triangle-alert.svg"));
        }
        if !e.erro.is_empty() {
            tem_extra = true;
            extras = extras.child(alerta(e.erro.clone(), cor::signal_error(), "icons/circle-x.svg"));
        }
        if tem_extra {
            corpo = corpo.child(extras);
        }
        let caixa = div()
            .id("arvore-excluir")
            .track_focus(&self.foco_dialogo)
            .occlude()
            .on_key_down(cx.listener(Self::tecla_dialogo))
            .flex_none()
            .w(px(522.))
            .p(px(10.))
            .rounded(px(3.))
            .border_1()
            .border_color(rgba(0xffffff14))
            .bg(cor::raised_black())
            .shadow_lg()
            .text_color(cor::text_primary())
            .child(
                div().flex().justify_end().h(px(26.)).pb(px(4.)).child(
                    div()
                        .id("arvore-excluir-fechar")
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(22.))
                        .rounded(px(5.))
                        .hover(|s| s.bg(rgba(0xffffff26)))
                        .tooltip_texto("Fechar")
                        .on_click(cx.listener(|this, _, window, cx| this.fechar_excluir(window, cx)))
                        .child(icone("icons/x.svg", 16., cor::text_primary())),
                ),
            )
            .child(
                div()
                    .flex()
                    .px(px(10.))
                    .text_size(tema::TEXTO_MENU)
                    .line_height(px(22.))
                    .child(icone("icons/info.svg", 48., cor::signal_info()))
                    .child(corpo),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(10.))
                    .px(px(15.))
                    .pt(px(24.))
                    .pb(px(14.))
                    .child(
                        botao("arvore-excluir-confirmar", true)
                            .when(ocupado, |d| d.child(icone("icons/loader-circle.svg", 14., cor::pitch_black())))
                            .child("Mover para a Lixeira")
                            .on_click(cx.listener(|this, _, _, cx| this.confirmar_excluir(cx))),
                    )
                    .child(
                        botao("arvore-excluir-cancelar", false)
                            .child("Cancelar")
                            .on_click(cx.listener(|this, _, window, cx| this.fechar_excluir(window, cx))),
                    ),
            );
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    widgets::mascara("arvore-excluir-mascara", window)
                        .flex()
                        .items_start()
                        .justify_center()
                        .pt(px(100.))
                        .child(caixa),
                ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

impl Render for Explorador {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.garantir_linhas();
        self.limites_linhas.borrow_mut().clear();
        if self.focar_painel {
            self.focar_painel = false;
            window.focus(&self.foco, cx);
        }
        if self.soltar.is_some() && !cx.has_active_drag() {
            self.soltar = None;
        }
        let linhas = self.linhas.clone();
        if let Some(alvo) = self.rolar_para.clone() {
            if let Some(i) = linhas.iter().position(|l| l.chave == alvo) {
                self.rolagem.scroll_to_item(i, ScrollStrategy::Nearest);
                self.rolar_para = None;
            }
        }
        let marcados = self.marcados();
        let n = linhas.len();
        let lista = uniform_list(
            "arvore-linhas",
            n,
            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                let linhas = this.linhas.clone();
                range.filter_map(|i| linhas.get(i).map(|l| this.linha(i, l, &marcados, window, cx))).collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.rolagem)
        .flex_1()
        .pt(px(2.))
        .pb(px(2.));

        let painel = div()
            .id("arvore-painel")
            .group("arvore-painel")
            .track_focus(&self.foco)
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .when(self.recolhida, |d| d.hidden())
            .on_key_down(cx.listener(Self::tecla))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.clique_vazio(window, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                let itens = this.menu_raiz();
                this.abrir_menu(itens, ev.position, window, cx);
            }))
            .on_drag_move::<Arrasto>(cx.listener(|this, ev: &DragMoveEvent<Arrasto>, _, cx| {
                if ev.bounds.contains(&ev.event.position) {
                    let caminhos = ev.drag(cx).caminhos.clone();
                    this.arrastar_sobre(Some(String::new()), &caminhos, cx);
                }
            }))
            .on_drop(cx.listener(|this, a: &Arrasto, _, cx| this.soltar_aqui(a, cx)))
            .when(!self.erro.is_empty(), |d| {
                d.child(div().px(px(12.)).py(px(4.)).text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child(self.erro.clone()))
            })
            .when(!self.arvore.filhos.contains_key("") && self.erro.is_empty(), |d| {
                d.child(div().px(px(12.)).py(px(4.)).text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child("Lendo a pasta…"))
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(
                        canvas_origem(cx.weak_entity()),
                    )
                    .child(lista)
                    .child(gpui_kit::base::Scrollbar::vertical(&self.rolagem).viewport_from_layout()),
            )
            .when_some(self.soltar.clone(), |d, (dest, problema)| {
                let texto = problema.clone().unwrap_or_else(|| format!("Soltar em {}", modelo::onde(&dest)));
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .px(px(12.))
                        .py(px(4.))
                        .truncate()
                        .bg(cor::panel_black())
                        .border_t_1()
                        .border_color(cor::hairline())
                        .text_size(tema::TEXTO_LABEL)
                        .text_color(if problema.is_some() { cor::signal_error() } else { cor::text_secondary() })
                        .child(texto),
                )
            })
            .test_support();

        div()
            .id("explorador")
            .group("explorador")
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(cor::pitch_black())
            .text_color(cor::text_primary())
            .font_family(tema::FONTE)
            .child(self.barra(window, cx))
            .child(self.secao(cx))
            .child(painel)
            .children(self.menu_el(cx))
            .children(self.dialogo_el(window, cx))
    }
}

#[cfg(test)]
thread_local! {
    /// A janela de teste não é Win32: os testes dizem se ela está minimizada.
    pub(crate) static MINIMIZADA_NO_TESTE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn janela_minimizada(_: &Window) -> bool {
    MINIMIZADA_NO_TESTE.with(|m| m.get())
}

#[cfg(all(windows, not(test)))]
fn janela_minimizada(window: &Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        // SAFETY: o HWND é da janela viva que o GPUI acabou de entregar.
        Ok(RawWindowHandle::Win32(h)) => unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(h.hwnd.get() as _) != 0
        },
        _ => false,
    }
}

#[cfg(all(not(windows), not(test)))]
fn janela_minimizada(_: &Window) -> bool {
    false
}

/// Mede a origem e a largura da lista (para o menu aberto pelo teclado e a largura do motivo embaixo do campo).
fn canvas_origem(vista: WeakEntity<Explorador>) -> impl IntoElement {
    gpui_kit::canvas(
        move |bounds, _, cx| {
            let _ = vista.update(cx, |this, _| {
                this.origem_lista = bounds.origin;
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

fn medidor_campo(vista: WeakEntity<Explorador>) -> impl IntoElement {
    gpui_kit::canvas(
        move |bounds, _, cx| {
            let _ = vista.update(cx, |this, _| this.largura_campo = bounds.size.width + px(2.));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// Leituras para os testes.
#[cfg(test)]
impl Explorador {
    /// O valor e o trecho selecionado do campo da linha, se há um.
    pub(crate) fn estado_campo(&self, cx: &App) -> Option<(String, String)> {
        if self.renomeando.is_none() && self.criando.is_none() {
            return None;
        }
        let c = self.campo.read(cx);
        Some((c.value().to_string(), c.selected_text().to_string()))
    }

    pub(crate) fn motivo_campo(&self, cx: &App) -> Option<String> {
        if self.renomeando.is_none() && self.criando.is_none() {
            return None;
        }
        self.erro_do_campo(cx)
    }

    pub(crate) fn textos_dialogo(&self) -> Option<(String, Option<String>)> {
        let e = self.excluindo.as_ref()?;
        let tem_pasta = e.caminhos.iter().any(|p| !self.arvore.eh_arquivo(p));
        let (pergunta, _) = modelo::textos_excluir(&e.caminhos, tem_pasta);
        Some((pergunta, modelo::aviso_rascunhos(&modelo::sujos_de(&e.caminhos, &self.sujos))))
    }

    pub(crate) fn erro(&self) -> &str {
        &self.erro
    }

    pub(crate) fn posicao_menu(&self) -> Option<Point<Pixels>> {
        self.menu.as_ref().map(|m| m.posicao)
    }

    pub(crate) fn limites_linha(&self, chave: &str) -> Option<Bounds<Pixels>> {
        self.limites_linhas.borrow().get(chave).copied()
    }

    pub(crate) fn rodadas_paradas(&self) -> bool {
        self.minimizada
    }

    pub(crate) fn rotulos_menu(&self) -> Vec<String> {
        self.menu
            .iter()
            .flat_map(|m| m.itens.iter())
            .map(|i| match i {
                ItemMenu::Divisor => "-".to_string(),
                ItemMenu::Acao { rotulo, .. } => rotulo.clone(),
            })
            .collect()
    }

    pub(crate) fn habilitados_menu(&self) -> Vec<String> {
        self.menu
            .iter()
            .flat_map(|m| m.itens.iter())
            .filter_map(|i| match i {
                ItemMenu::Acao { rotulo, acao: Some(_), .. } => Some(rotulo.clone()),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn foco_no_painel(&self, window: &Window) -> bool {
        self.foco.is_focused(window)
    }

    pub(crate) fn foco_no_dialogo(&self, window: &Window) -> bool {
        self.foco_dialogo.is_focused(window)
    }
}
