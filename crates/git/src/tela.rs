//! Barra lateral do Git (porte de `src/git/git.tsx`): abas Alterações e Histórico. Alterações tem a branch com ahead/behind,
//! a mensagem e o botão do commit (com o modo Commit ou Commit e Push), as seções Preparadas e Alterações (lista virtual,
//! stage/unstage por arquivo e em lote, Ctrl+clique marca) e o Grafo. Fora do modo `repo`, só o estado vazio. O status é
//! relido a cada 1,5 s com foco (10 s sem), como o `usePolled` do React; cada ação relê na hora.
use crate::comum::{self, acao, acao_secao, cor_tipo, descricao, dividir, icone_arquivo, letra, palavra, secundario, Alvo, Lado};
use crate::fonte::{Branches, CommitDetail, CommitHoverDetail, Estado, Fonte, GitFile, GraphCommit, Kind, Status};
use crate::grafo::LinhaGrafo;
use crate::guarda::GuardaGit;
use crate::sondagem::{Sondagem, RITMO};
use gpui_kit::component::input::{Enter, Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::Sizable as _;
use gpui_kit::TestSupportExt as _;
use gpui_kit::{
    anchored, canvas, deferred, div, prelude::*, px, uniform_list, App, Bounds, ClickEvent, Context, Entity,
    EventEmitter, FocusHandle, Focusable, Hsla, MouseButton, Pixels, ScrollHandle, SharedString, Subscription, Task,
    UniformListScrollHandle, Window,
};
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::{icone, TooltipTexto as _};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

/// Linha das listas (cabeçalho de seção, arquivo, commit do grafo).
pub const LINHA: f32 = 22.;
/// Commits por página, no histórico e no grafo.
pub const PAGINA: usize = 50;
/// Altura mínima do grafo aberto e da lista de arquivos ao arrastar o divisor; setas andam de 16 em 16.
pub const GRAFO_MIN: f32 = 96.;
pub const ARQUIVOS_MIN: f32 = 96.;

/// O que a tela pede à bancada.
#[derive(Clone, Debug, PartialEq)]
pub enum EventoGit {
    /// Clique abre em prévia; duplo clique abre de vez (`permanente`).
    AbrirAlvo { alvo: Alvo, permanente: bool },
    /// Mouse pressionado ou parado num arquivo: a bancada pode ler o diff antes do clique.
    PreCarregar { alvo: Alvo, hover: bool },
    AbrirArquivo { caminho: String, permanente: bool },
    /// O estado do Git mudou (badge da navegação e barra de status). `None` enquanto não há leitura.
    Estado(Option<Estado>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vista {
    Alteracoes,
    Historico,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remoto {
    Fetch,
    Pull,
    Push,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ocupado {
    Remoto(Remoto),
    Commit,
    Stage,
    Branch,
}

impl Ocupado {
    fn texto(self) -> &'static str {
        match self {
            Ocupado::Remoto(Remoto::Fetch) => "Buscando…",
            Ocupado::Remoto(Remoto::Pull) => "Recebendo…",
            Ocupado::Remoto(Remoto::Push) => "Enviando…",
            Ocupado::Commit => "Commitando…",
            Ocupado::Stage => "Atualizando o preparo…",
            Ocupado::Branch => "Mudando de branch…",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModoCommit {
    Commit,
    CommitPush,
}

impl ModoCommit {
    fn rotulo(self) -> &'static str {
        match self {
            ModoCommit::Commit => "Commit",
            ModoCommit::CommitPush => "Commit e Push",
        }
    }
}

/// Lista paginada de commits: a anterior fica até a nova chegar (grafo) ou some ao recarregar (histórico).
#[derive(Default)]
pub(crate) struct Paginada {
    pub itens: Option<Vec<GraphCommit>>,
    pub mais: bool,
    pub lendo: bool,
    pub erro: String,
    pub seq: u64,
    pub escopo: Option<String>,
}

/// O commit aberto (no histórico ou no grafo) e o detalhe dele.
#[derive(Default)]
pub(crate) struct Detalhe {
    pub aberto: Option<String>,
    pub pendente: Option<String>,
    pub detalhe: Option<CommitDetail>,
    pub erro: String,
    pub seq: u64,
}

#[derive(Clone)]
pub(crate) enum EstadoHover {
    Lendo,
    Pronto(CommitHoverDetail),
    Erro(String),
}

/// Cartão de hover do grafo: um só, ancorado na linha ativa.
#[derive(Default)]
pub(crate) struct Hover {
    pub cartao: Option<String>,
    pub ativo: Option<(String, u64)>,
    pub seq: u64,
    pub abrir: Option<Task<()>>,
    pub fechar: Option<Task<()>>,
    pub vizinhas: Option<Task<()>>,
}

#[derive(Default, Clone, Copy)]
struct Recolhido {
    preparadas: bool,
    alteracoes: bool,
    grafo: bool,
}

/// Uma linha da lista virtual de arquivos.
#[derive(Clone, Copy)]
enum Item {
    Cabecalho(Lado),
    Arquivo(Lado, usize),
    Vazio,
}

pub struct TelaGit {
    pub(crate) fonte: Fonte,
    pub(crate) guarda: GuardaGit,
    foco: FocusHandle,
    pub(crate) foco_divisor: FocusHandle,
    estado: Option<Estado>,
    erro: String,
    sondagem: Option<Sondagem>,
    ativa: bool,
    travado: bool,
    pub(crate) selecionado: Option<Alvo>,
    pub(crate) vista: Vista,
    pub(crate) ocupado: Option<Ocupado>,
    mensagem: Entity<TextareaState>,
    modo: ModoCommit,
    menu_modo: bool,
    recolhido: Recolhido,
    pub(crate) altura_grafo: Option<f32>,
    marcados: HashSet<(Lado, String)>,
    lista: UniformListScrollHandle,
    branch_aberto: bool,
    branches: Option<Branches>,
    erro_branch: String,
    nome_branch: Entity<InputState>,
    iniciando: bool,
    // Grafo.
    pub(crate) grafo: Paginada,
    pub(crate) grafo_linhas: Vec<LinhaGrafo>,
    pub(crate) grafo_carimbo: Option<String>,
    pub(crate) grafo_tick: u64,
    pub(crate) grafo_detalhe: Detalhe,
    pub(crate) grafo_rolagem: ScrollHandle,
    pub(crate) hover: Hover,
    pub(crate) hovers: HashMap<String, EstadoHover>,
    pub(crate) linhas_grafo: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    pub(crate) arrasto: Option<(f32, f32, f32)>,
    pub(crate) medidas: Rc<Cell<(f32, f32, f32)>>,
    // Histórico.
    pub(crate) historico: Paginada,
    pub(crate) historico_detalhe: Detalhe,
    pub(crate) pedido: (u64, Option<String>),
    pub(crate) pedido_carregado: Option<u64>,
    pub(crate) historico_rolagem: ScrollHandle,
    pub(crate) detalhes: HashMap<String, CommitDetail>,
    pub(crate) lendo_detalhe: HashSet<String>,
    pub(crate) hover_arquivo: Option<Task<()>>,
    pub(crate) giro: HashMap<&'static str, std::time::Instant>,
    _subs: Vec<Subscription>,
    pub(crate) _tarefas: Vec<Task<()>>,
}

impl EventEmitter<EventoGit> for TelaGit {}

impl Focusable for TelaGit {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

/// Caminhos de um arquivo para preparar ou tirar: renomeado leva o nome antigo junto.
fn caminhos_de(f: &GitFile) -> Vec<String> {
    let mut v = vec![f.path.clone()];
    v.extend(f.orig_path.clone());
    v
}

fn tipo(f: &GitFile, lado: Lado) -> Option<Kind> {
    if lado.staged() {
        f.staged
    } else {
        f.unstaged
    }
}

impl TelaGit {
    pub fn new(fonte: Fonte, guarda: GuardaGit, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mensagem = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 4).placeholder("Mensagem (Ctrl+Enter)"));
        let nome_branch = cx.new(|cx| InputState::new(window, cx).placeholder("Nova branch"));
        let subs = vec![
            cx.observe(&mensagem, |_, _, cx| cx.notify()),
            cx.subscribe_in(&nome_branch, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.criar_branch(window, cx);
                }
                cx.notify();
            }),
        ];
        let mut t = TelaGit {
            fonte,
            guarda,
            foco: cx.focus_handle(),
            foco_divisor: cx.focus_handle(),
            estado: None,
            erro: String::new(),
            sondagem: None,
            ativa: true,
            travado: false,
            selecionado: None,
            vista: Vista::Alteracoes,
            ocupado: None,
            mensagem,
            modo: ModoCommit::Commit,
            menu_modo: false,
            recolhido: Recolhido::default(),
            altura_grafo: None,
            marcados: HashSet::new(),
            lista: UniformListScrollHandle::new(),
            branch_aberto: false,
            branches: None,
            erro_branch: String::new(),
            nome_branch,
            iniciando: false,
            grafo: Paginada::default(),
            grafo_linhas: Vec::new(),
            grafo_carimbo: None,
            grafo_tick: 0,
            grafo_detalhe: Detalhe::default(),
            grafo_rolagem: ScrollHandle::new(),
            hover: Hover::default(),
            hovers: HashMap::new(),
            linhas_grafo: Rc::default(),
            arrasto: None,
            medidas: Rc::default(),
            historico: Paginada::default(),
            historico_detalhe: Detalhe::default(),
            pedido: (0, None),
            pedido_carregado: None,
            historico_rolagem: ScrollHandle::new(),
            detalhes: HashMap::new(),
            lendo_detalhe: HashSet::new(),
            hover_arquivo: None,
            giro: HashMap::new(),
            _subs: subs,
            _tarefas: Vec::new(),
        };
        t.sondagem = Some(Sondagem::iniciar(RITMO, |this: &mut Self, _, cx| this.atualizar(cx), window, cx));
        t
    }

    // ---- API da bancada ----

    pub fn estado(&self) -> Option<&Estado> {
        self.estado.as_ref()
    }

    pub fn vista(&self) -> Vista {
        self.vista
    }

    /// A tela Git está aberta: sem isso, grafo e histórico não consultam o Git (o status continua, para o badge).
    pub fn definir_ativa(&mut self, ativa: bool, cx: &mut Context<Self>) {
        self.ativa = ativa;
        self.talvez_carregar(cx);
        cx.notify();
    }

    /// Executores vivos: Receber e trocar/criar branch explicam a recusa.
    pub fn definir_travado(&mut self, travado: bool, cx: &mut Context<Self>) {
        self.travado = travado;
        cx.notify();
    }

    /// Alvo aberto no editor: marca a linha de diff (do mesmo lado), de conflito ou do arquivo do commit.
    pub fn definir_selecionado(&mut self, alvo: Option<Alvo>, cx: &mut Context<Self>) {
        if self.selecionado != alvo {
            self.selecionado = alvo;
            cx.notify();
        }
    }

    /// Pedido de histórico: abre a aba Histórico no projeto (`None`) ou no arquivo.
    pub fn pedir_historico(&mut self, caminho: Option<String>, cx: &mut Context<Self>) {
        self.pedido = (self.pedido.0 + 1, caminho);
        self.vista = Vista::Historico;
        self.talvez_carregar(cx);
        cx.notify();
    }

    /// A mensagem do commit (como se digitada).
    pub fn definir_mensagem(&mut self, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        let t = texto.to_string();
        self.mensagem.update(cx, |m, cx| m.set_value(t, window, cx));
        cx.notify();
    }

    pub fn mensagem(&self, cx: &App) -> String {
        self.texto_mensagem(cx)
    }

    /// Commits carregados no grafo, na ordem da tela.
    pub fn grafo_commits(&self) -> &[GraphCommit] {
        self.grafo.itens.as_deref().unwrap_or_default()
    }

    /// Largura (em faixas) de cada linha do grafo.
    pub fn grafo_larguras(&self) -> Vec<usize> {
        self.grafo_linhas.iter().map(|l| l.largura).collect()
    }

    pub fn historico_commits(&self) -> &[GraphCommit] {
        self.historico.itens.as_deref().unwrap_or_default()
    }

    /// Clique num commit do grafo: abre os arquivos dele ali; de novo, recolhe.
    pub fn alternar_commit_do_grafo(&mut self, hash: &str, cx: &mut Context<Self>) {
        self.alternar_commit(true, hash, cx);
    }

    /// Quantos arquivos do commit aberto no grafo estão na tela (`None` sem commit aberto ou ainda lendo).
    pub fn arquivos_abertos_no_grafo(&self) -> Option<usize> {
        self.grafo_detalhe.aberto.as_ref()?;
        self.grafo_detalhe.detalhe.as_ref().map(|d| d.files.len())
    }

    pub fn arquivos_abertos_no_historico(&self) -> Option<usize> {
        self.historico_detalhe.aberto.as_ref()?;
        self.historico_detalhe.detalhe.as_ref().map(|d| d.files.len())
    }

    /// Abre um commit na aba Histórico (a lista dele, como o clique na linha).
    pub fn abrir_commit_do_historico(&mut self, hash: &str, cx: &mut Context<Self>) {
        self.vista = Vista::Historico;
        self.abrir_commit(false, hash, cx);
    }

    pub fn mostrar_vista(&mut self, vista: Vista, cx: &mut Context<Self>) {
        self.vista = vista;
        self.talvez_carregar(cx);
        cx.notify();
    }

    /// Evento de executor: rodada imediata do status, mesmo sem foco.
    pub fn cutucar(&self) {
        if let Some(s) = &self.sondagem {
            s.cutucar();
        }
    }

    /// Relê o status agora; a tarefa termina quando a resposta chegou.
    pub fn atualizar(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let fonte = self.fonte.clone();
        let tarefa = cx.background_spawn(async move { fonte.estado() });
        cx.spawn(async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| this.receber(r, cx));
        })
    }

    /// Resposta do status: só notifica quando mudou (o poll igual não redesenha nada).
    fn receber(&mut self, r: Result<Estado, String>, cx: &mut Context<Self>) {
        match r {
            Ok(e) => {
                let mudou = self.estado.as_ref() != Some(&e);
                let tinha_erro = !self.erro.is_empty();
                self.erro.clear();
                if mudou {
                    self.estado = Some(e);
                    self.limpar_marcados();
                    cx.emit(EventoGit::Estado(self.estado.clone()));
                    self.talvez_carregar(cx);
                }
                if mudou || tinha_erro {
                    cx.notify();
                }
            }
            Err(e) => {
                if self.estado.is_some() || self.erro != e {
                    self.estado = None;
                    self.erro = e;
                    cx.emit(EventoGit::Estado(None));
                    cx.notify();
                }
            }
        }
    }

    fn limpar_marcados(&mut self) {
        let Some(Estado::Repo { status }) = &self.estado else { return };
        self.marcados.retain(|(lado, p)| status.files.iter().any(|f| &f.path == p && tipo(f, *lado).is_some()));
    }

    pub(crate) fn status(&self) -> Option<&Status> {
        match &self.estado {
            Some(Estado::Repo { status }) => Some(status),
            _ => None,
        }
    }

    /// Carrega o grafo e o histórico quando estão visíveis e a chave mudou desde a última carga.
    pub(crate) fn talvez_carregar(&mut self, cx: &mut Context<Self>) {
        if !self.ativa {
            return;
        }
        let Some(s) = self.status() else { return };
        if self.vista == Vista::Alteracoes && !self.recolhido.grafo {
            let carimbo = format!("{}|{}|{}|{}|{}", self.grafo_tick, s.head, s.branch, s.ahead, s.behind);
            if self.grafo_carimbo.as_ref() != Some(&carimbo) {
                self.grafo_carimbo = Some(carimbo);
                self.recarregar_grafo(cx);
            }
        }
        if self.vista == Vista::Historico && self.pedido_carregado != Some(self.pedido.0) {
            self.pedido_carregado = Some(self.pedido.0);
            let escopo = self.pedido.1.clone();
            self.recarregar_historico(escopo, cx);
        }
    }

    pub(crate) fn recolhido_grafo_estado(&self) -> bool {
        self.recolhido.grafo
    }

    pub(crate) fn alternar_recolhido_grafo(&mut self) {
        self.recolhido.grafo = !self.recolhido.grafo;
    }

    pub(crate) fn giro(&mut self, chave: &'static str) {
        self.giro.insert(chave, std::time::Instant::now());
    }

    // ---- Ações ----

    /// Uma operação por vez, com a trava compartilhada. Libera no fim, sucesso ou erro; o status é relido de qualquer jeito.
    fn com_ocupado<R: Send + 'static>(
        &mut self,
        tipo: Ocupado,
        trabalho: impl FnOnce(&dyn crate::FonteGit) -> Result<R, String> + Send + 'static,
        depois: impl FnOnce(&mut Self, Result<R, String>, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ocupado.is_some() || !self.guarda.entrar(window, cx) {
            return;
        }
        self.ocupado = Some(tipo);
        cx.notify();
        let fonte = self.fonte.clone();
        let tarefa = cx.background_spawn(async move { trabalho(&*fonte) });
        self._tarefas.push(cx.spawn_in(window, async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.ocupado = None;
                this.guarda.sair();
                if let Err(e) = &r {
                    comum::falha(e.clone(), window, cx);
                }
                depois(this, r, window, cx);
                cx.notify();
            });
            if let Ok(t) = this.update(cx, |this, cx| this.atualizar(cx)) {
                t.await;
            }
        }));
    }

    pub fn remoto(&mut self, tipo: Remoto, window: &mut Window, cx: &mut Context<Self>) {
        if tipo == Remoto::Pull && self.travado {
            comum::aviso(comum::TRAVADO, window, cx);
            return;
        }
        let feito = match tipo {
            Remoto::Fetch => "Busca concluída",
            Remoto::Pull => "Commits recebidos",
            Remoto::Push => "Enviado",
        };
        self.com_ocupado(
            Ocupado::Remoto(tipo),
            move |f| match tipo {
                Remoto::Fetch => f.fetch(),
                Remoto::Pull => f.pull(),
                Remoto::Push => f.push(),
            },
            move |this, r, window, cx| {
                this.grafo_tick += 1;
                if let Ok(saida) = r {
                    comum::sucesso(feito, &comum::ultima_linha(&saida), window, cx);
                }
            },
            window,
            cx,
        );
    }

    fn texto_mensagem(&self, cx: &App) -> String {
        self.mensagem.read(cx).value().to_string()
    }

    fn pode_commitar(&self, cx: &App) -> bool {
        let preparados = self.status().is_some_and(|s| s.files.iter().any(|f| f.staged.is_some()));
        !self.texto_mensagem(cx).trim().is_empty() && preparados && self.ocupado.is_none()
    }

    /// Commit, e depois o push. Commit feito limpa a mensagem (se ainda é a enviada) mesmo se o envio falhar.
    pub fn commitar(&mut self, modo: ModoCommit, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pode_commitar(cx) {
            return;
        }
        let enviado = self.texto_mensagem(cx);
        let msg = enviado.trim().to_string();
        let com_push = modo == ModoCommit::CommitPush;
        self.com_ocupado(
            Ocupado::Commit,
            move |f| {
                f.commit(&msg)?;
                Ok(if com_push { Some(f.push()) } else { None })
            },
            move |this, r, window, cx| {
                this.grafo_tick += 1;
                let Ok(push) = r else { return };
                if this.texto_mensagem(cx) == enviado {
                    this.mensagem.update(cx, |m, cx| m.set_value("", window, cx));
                }
                match push {
                    None => comum::sucesso("Commit criado", "", window, cx),
                    Some(Ok(saida)) => comum::sucesso("Enviado", &comum::ultima_linha(&saida), window, cx),
                    Some(Err(e)) => {
                        comum::falha(e, window, cx);
                        comum::aviso_longo(
                            "Commit criado, mas o envio falhou",
                            "Use Enviar no gráfico para tentar de novo. Não precisa commitar outra vez.",
                            window,
                            cx,
                        );
                    }
                }
            },
            window,
            cx,
        );
    }

    /// Preparar ou tirar do preparo: lista vazia vale para tudo. Deu certo, a marcação some.
    pub fn preparar(&mut self, caminhos: Vec<String>, tirar: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.com_ocupado(
            Ocupado::Stage,
            move |f| if tirar { f.unstage(&caminhos) } else { f.stage(&caminhos) },
            |this, r, _, _| {
                if r.is_ok() {
                    this.marcados.clear();
                }
            },
            window,
            cx,
        );
    }

    /// Prepara (ou tira) a linha; marcada junto com outras, age sobre todas as marcadas da seção.
    pub fn preparar_linha(&mut self, f: &GitFile, lado: Lado, window: &mut Window, cx: &mut Context<Self>) {
        let lista: Vec<GitFile> = self.arquivos(lado).into_iter().cloned().collect();
        let lote: Vec<&GitFile> = if self.marcados.contains(&(lado, f.path.clone())) {
            lista.iter().filter(|x| self.marcados.contains(&(lado, x.path.clone()))).collect()
        } else {
            Vec::new()
        };
        let escolhidos: Vec<String> = if lote.len() > 1 { lote.iter().flat_map(|x| caminhos_de(x)).collect() } else { caminhos_de(f) };
        self.preparar(escolhidos, lado.staged(), window, cx);
    }

    fn arquivos(&self, lado: Lado) -> Vec<&GitFile> {
        self.status().map(|s| s.files.iter().filter(|f| tipo(f, lado).is_some()).collect()).unwrap_or_default()
    }

    fn alvo_de(f: &GitFile, lado: Lado) -> Alvo {
        if tipo(f, lado) == Some(Kind::Conflicted) {
            Alvo::Conflito { caminho: f.path.clone() }
        } else {
            Alvo::Diff { lado, caminho: f.path.clone() }
        }
    }

    pub fn escolher_linha(&mut self, f: &GitFile, lado: Lado, ctrl: bool, cx: &mut Context<Self>) {
        let chave = (lado, f.path.clone());
        if ctrl {
            if !self.marcados.remove(&chave) {
                self.marcados.insert(chave);
            }
            cx.notify();
            return;
        }
        self.marcados = HashSet::from([chave]);
        cx.emit(EventoGit::AbrirAlvo { alvo: Self::alvo_de(f, lado), permanente: false });
        cx.notify();
    }

    fn iniciar_repositorio(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.iniciando {
            return;
        }
        self.iniciando = true;
        cx.notify();
        let fonte = self.fonte.clone();
        let tarefa = cx.background_spawn(async move { fonte.init() });
        self._tarefas.push(cx.spawn_in(window, async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update_in(cx, |_, window, cx| {
                if let Err(e) = r {
                    comum::falha(e, window, cx);
                }
            });
            if let Ok(t) = this.update(cx, |this, cx| this.atualizar(cx)) {
                t.await;
            }
            let _ = this.update(cx, |this, cx| {
                this.iniciando = false;
                cx.notify();
            });
        }));
    }

    // ---- Branch ----

    fn abrir_branches(&mut self, abrir: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.branch_aberto = abrir;
        if abrir {
            self.branches = None;
            self.erro_branch.clear();
            self.nome_branch.update(cx, |i, cx| i.set_value("", window, cx));
            let fonte = self.fonte.clone();
            let tarefa = cx.background_spawn(async move { fonte.branches() });
            self._tarefas.push(cx.spawn(async move |this, cx| {
                let r = tarefa.await;
                let _ = this.update(cx, |this, cx| {
                    if this.branch_aberto {
                        match r {
                            Ok(b) => this.branches = Some(b),
                            Err(e) => this.erro_branch = e,
                        }
                        cx.notify();
                    }
                });
            }));
        }
        cx.notify();
    }

    fn trocar_branch(&mut self, nome: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.branches.as_ref().is_some_and(|b| b.current == nome) || self.ocupado.is_some() {
            return;
        }
        if self.travado {
            comum::aviso(comum::TRAVADO, window, cx);
            return;
        }
        self.com_ocupado(Ocupado::Branch, move |f| f.switch(&nome), |this, r, _, _| {
            if r.is_ok() {
                this.branch_aberto = false;
            }
        }, window, cx);
    }

    fn criar_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let nome = self.nome_branch.read(cx).value().trim().to_string();
        if nome.is_empty() || self.ocupado.is_some() {
            return;
        }
        if self.travado {
            comum::aviso(comum::TRAVADO, window, cx);
            return;
        }
        self.com_ocupado(Ocupado::Branch, move |f| f.create_branch(&nome), |this, r, _, _| {
            if r.is_ok() {
                this.branch_aberto = false;
            }
        }, window, cx);
    }

    // ---- Desenho ----

    fn abas(&self, contagem: Option<usize>, cx: &mut Context<Self>) -> impl IntoElement {
        let aba = |id: &'static str, vista: Vista, rotulo: &'static str, extra: Option<String>, cx: &mut Context<Self>| {
            let ativa = self.vista == vista;
            div()
                .id(id)
                .relative()
                .flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .px(px(8.))
                .cursor_pointer()
                .text_size(tema::TEXTO_BODY)
                .when(ativa, |d| d.text_color(cor::text_primary()).font_weight(gpui_kit::FontWeight::MEDIUM))
                .when(!ativa, |d| d.text_color(secundario()).hover(|s| s.text_color(cor::text_primary())))
                .child(rotulo)
                .when_some(extra, |d, e| d.child(div().text_size(px(12.6)).opacity(0.7).child(e)))
                .when(ativa, |d| d.child(div().absolute().left_0().right_0().bottom_0().h(px(2.)).bg(cor::ink_white())))
                .on_click(cx.listener(move |this, _, _, cx| this.mostrar_vista(vista, cx)))
                .test_support()
        };
        div()
            .relative()
            .flex()
            .flex_none()
            .h(tema::FAIXA)
            .child(div().absolute().left_0().right_0().bottom_0().h(px(1.)).bg(cor::hairline()))
            .child(aba("aba-alteracoes", Vista::Alteracoes, "Alterações", contagem.map(|n| format!("({n})")), cx))
            .child(aba("aba-historico", Vista::Historico, "Histórico", None, cx))
    }

    fn vazio(&self, estado: &Estado, cx: &mut Context<Self>) -> impl IntoElement {
        let texto = match estado {
            Estado::NoRepo => "A pasta aberta não tem um repositório Git. Inicialize um para usar os recursos do Git aqui: alterações, commits, branches e histórico.".to_string(),
            Estado::NoGit => "O Git não foi encontrado neste computador. Instale o Git e abra o app de novo.".to_string(),
            Estado::RepoAbove { root } => format!("Esta pasta está dentro do repositório {root}, que não é a raiz do projeto. O painel Git só funciona com a raiz do repositório aberta como projeto."),
            Estado::Repo { .. } => String::new(),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(12.))
            .py(px(8.))
            .child(div().text_size(tema::TEXTO_LABEL).line_height(px(19.)).text_color(descricao()).child(texto))
            .when(matches!(estado, Estado::NoRepo), |d| {
                d.child(
                    div()
                        .id("inicializar")
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .h(tema::CAMPO)
                        .w_full()
                        .rounded(tema::RAIO_SM)
                        .bg(cor::ink_white())
                        .text_color(cor::pitch_black())
                        .text_size(tema::TEXTO_BODY)
                        .when(self.iniciando, |d| d.opacity(0.6).child(icone("icons/loader-circle.svg", 14., cor::pitch_black())))
                        .when(!self.iniciando, |d| d.cursor_pointer().hover(|s| s.bg(cor::ink_white_hover())))
                        .child("Inicializar repositório")
                        .on_click(cx.listener(|this, _, w, cx| this.iniciar_repositorio(w, cx)))
                        .test_support(),
                )
            })
    }

    /// A animação do botão `chave`, se ainda está correndo.
    pub(crate) fn anim(&self, chave: &'static str, tipo: comum::Animacao) -> Option<(comum::Animacao, std::time::Instant)> {
        self.giro.get(chave).filter(|t| t.elapsed() < Duration::from_millis(700)).map(|t| (tipo, *t))
    }

    fn cabecalho_branch(&self, s: &Status, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let titulo = format!("{} a enviar, {} a receber", comum::commits(s.ahead), comum::commits(s.behind));
        let seta = |c: &'static str| icone(c, 12., descricao());
        let botao_branch = div()
            .id("branch")
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .h(px(24.))
            .px(px(8.))
            .rounded(tema::RAIO_SM)
            .cursor_pointer()
            .hover(|d| d.bg(cor::fill_muted()))
            .child(icone("icons/git-branch.svg", 14., cor::text_primary()))
            .child(div().min_w_0().truncate().font_family(tema::FONTE_MONO).text_size(px(13.)).child(s.branch.clone()))
            .tooltip_texto(format!("Branch atual: {}. Trocar ou criar uma branch", s.branch))
            .on_click(cx.listener(|this, _, w, cx| {
                let abrir = !this.branch_aberto;
                this.abrir_branches(abrir, w, cx)
            }));
        div()
            .flex()
            .flex_col()
            .flex_none()
            .px(px(12.))
            .pt(px(6.))
            .gap(px(2.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(botao_branch)
                            .when(self.branch_aberto, |d| d.child(self.popup_branch(cx))),
                    )
                    .child(
                        div()
                            .id("ahead-behind")
                            .flex()
                            .flex_none()
                            .items_center()
                            .text_size(tema::TEXTO_LABEL)
                            .text_color(descricao())
                            .tooltip_texto(titulo)
                            .child(seta("icons/arrow-up.svg"))
                            .child(s.ahead.to_string())
                            .child(div().w(px(4.)))
                            .child(seta("icons/arrow-down.svg"))
                            .child(s.behind.to_string()),
                    )
                    .child(
                        comum::acao_com("atualizar-status", comum::icone_animado("icons/refresh-cw.svg", 14., descricao(), self.anim("status", comum::Animacao::Giro), window), true)
                            .tooltip_texto("Atualizar status do Git")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.giro("status");
                                let t = this.atualizar(cx);
                                this._tarefas.push(t);
                                cx.notify();
                            })),
                    ),
            )
            .when_some(self.ocupado.filter(|o| !matches!(o, Ocupado::Remoto(_))), |d, o| {
                d.child(div().text_size(tema::TEXTO_LABEL).text_color(descricao()).child(o.texto()))
            })
    }

    fn popup_branch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let desligado = self.ocupado.is_some();
        let mut corpo = div()
            .id("popup-branch")
            .w(px(256.))
            .mt(px(4.))
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(tema::RAIO_MD)
            .bg(cor::raised_black())
            .border_1()
            .border_color(cor::hairline())
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, w, cx| this.abrir_branches(false, w, cx)));
        let texto = |t: String, c: Hsla| div().px(px(4.)).text_size(tema::TEXTO_LABEL).text_color(c).child(t);
        if let Some(o) = self.ocupado {
            corpo = corpo.child(texto(o.texto().into(), descricao()));
        }
        if !self.erro_branch.is_empty() {
            corpo = corpo.child(texto(self.erro_branch.clone(), comum::perigo()));
        }
        match &self.branches {
            None if self.erro_branch.is_empty() => corpo = corpo.child(texto("Lendo as branches…".into(), descricao())),
            None => {}
            Some(b) => {
                let mut lista = div().id("lista-branches").flex().flex_col().gap(px(2.)).max_h(px(240.)).overflow_y_scroll();
                for (i, nome) in b.local.iter().enumerate() {
                    let atual = *nome == b.current;
                    let nome2 = nome.clone();
                    lista = lista.child(
                        div()
                            .id(("branch-item", i))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(24.))
                            .px(px(7.))
                            .rounded(tema::RAIO_XS)
                            .font_family(tema::FONTE_MONO)
                            .text_size(tema::TEXTO_LABEL)
                            .when(!desligado, |d| d.cursor_pointer().hover(|s| s.bg(cor::fill_hover())))
                            .when(desligado, |d| d.opacity(0.4))
                            .child(div().w(px(14.)).flex_none().when(atual, |d| d.child(icone("icons/check.svg", 14., cor::text_primary()))))
                            .child(div().min_w_0().truncate().child(nome.clone()))
                            .tooltip_texto(match self.ocupado {
                                Some(o) => o.texto().to_string(),
                                None if atual => "Branch atual".to_string(),
                                None => format!("Trocar para {nome}"),
                            })
                            .on_click(cx.listener(move |this, _, w, cx| this.trocar_branch(nome2.clone(), w, cx))),
                    );
                }
                corpo = corpo.child(lista);
            }
        }
        let pode_criar = !desligado && !self.nome_branch.read(cx).value().trim().is_empty();
        corpo.child(
            div()
                .flex()
                .gap(px(4.))
                .child(div().flex_1().min_w_0().font_family(tema::FONTE_MONO).child(Input::new(&self.nome_branch).small().disabled(desligado)))
                .child(
                    div()
                        .id("criar-branch")
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(px(24.))
                        .px(px(7.))
                        .rounded(tema::RAIO_XS)
                        .bg(cor::ink_white())
                        .text_color(cor::pitch_black())
                        .text_size(tema::TEXTO_LABEL)
                        .when(!pode_criar, |d| d.opacity(0.4))
                        .when(pode_criar, |d| d.cursor_pointer())
                        .tooltip_texto("Cria a branch e troca para ela")
                        .child("Criar")
                        .on_click(cx.listener(|this, _, w, cx| this.criar_branch(w, cx))),
                ),
        )
    }

    fn bloco_commit(&self, s: &Status, cx: &mut Context<Self>) -> impl IntoElement {
        let preparados = s.files.iter().filter(|f| f.staged.is_some()).count();
        let texto = self.texto_mensagem(cx);
        let pode = self.pode_commitar(cx);
        let dica = if preparados == 0 {
            "Prepare arquivos para commitar"
        } else if texto.trim().is_empty() {
            "Escreva a mensagem do commit"
        } else {
            ""
        };
        let (fundo, frente, hover) = if pode {
            (cor::ink_white(), cor::pitch_black(), cor::ink_white_hover())
        } else {
            (cor::fill_hover(), secundario(), gpui_kit::rgba(0xffffff26).into())
        };
        let commitando = self.ocupado == Some(Ocupado::Commit);
        let modo = self.modo;
        let campo = div()
            .capture_action(cx.listener(|this, a: &Enter, w, cx| {
                if a.secondary {
                    cx.stop_propagation();
                    let m = this.modo;
                    this.commitar(m, w, cx);
                }
            }))
            .child(
                Textarea::new(&self.mensagem)
                    .bordered(false)
                    .appearance(false)
                    .aria_label(format!("Mensagem do commit (Ctrl+Enter: {})", modo.rotulo()))
                    .rounded(tema::RAIO_XS)
                    .bg(cor::fill_muted())
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .min_h(px(26.))
                    .xsmall()
                    .px(px(2.))
                    .py(px(4.)),
            );
        let botao = div()
            .relative()
            .flex()
            .h(px(28.))
            .rounded(px(2.))
            .overflow_hidden()
            .bg(fundo)
            .text_color(frente)
            .child(
                div()
                    .id("commit")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .px(px(4.))
                    .text_size(px(13.))
                    .when(pode && !commitando, |d| d.cursor_pointer().hover(move |s| s.bg(hover)))
                    .child(icone(if commitando { "icons/loader-circle.svg" } else { "icons/check.svg" }, 16., frente))
                    .child(div().truncate().child(modo.rotulo()))
                    .tooltip_texto(if dica.is_empty() { modo.rotulo() } else { dica })
                    .on_click(cx.listener(move |this, _, w, cx| this.commitar(modo, w, cx))),
            )
            .child(div().my(px(5.)).w(px(1.)).bg(frente).opacity(0.4))
            .child(
                div()
                    .id("modo-commit")
                    .flex()
                    .flex_none()
                    .w(px(26.))
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(icone("icons/chevron-down.svg", 16., frente))
                    .tooltip_texto("Escolher a ação do commit")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu_modo = !this.menu_modo;
                        cx.notify();
                    })),
            );
        let menu = self.menu_modo.then(|| {
            let item = |m: ModoCommit, cx: &mut Context<Self>| {
                div()
                    .id(m.rotulo())
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(tema::ITEM_MENU)
                    .px(px(8.))
                    .rounded(tema::RAIO_MENU)
                    .text_size(tema::TEXTO_MENU)
                    .cursor_pointer()
                    .hover(|s| s.bg(cor::fill_hover()))
                    .child(div().w(px(14.)).when(self.modo == m, |d| d.child(icone("icons/check.svg", 14., cor::text_primary()))))
                    .child(m.rotulo())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.modo = m;
                        this.menu_modo = false;
                        cx.notify();
                    }))
            };
            deferred(
                anchored().anchor(gpui_kit::Anchor::TopRight).child(
                    div()
                        .id("menu-modo")
                        .mt(px(4.))
                        .p(px(4.))
                        .min_w(px(160.))
                        .flex()
                        .flex_col()
                        .rounded(tema::RAIO_MD)
                        .bg(cor::raised_black())
                        .border_1()
                        .border_color(cor::hairline())
                        .occlude()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu_modo = false;
                            cx.notify();
                        }))
                        .child(item(ModoCommit::Commit, cx))
                        .child(item(ModoCommit::CommitPush, cx)),
                ),
            )
        });
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(4.))
            .px(px(12.))
            .py(px(4.))
            .child(campo)
            .child(div().flex().flex_col().items_end().child(div().w_full().child(botao)).children(menu))
            .when(preparados == 0, |d| d.child(div().text_size(tema::TEXTO_LABEL).text_color(descricao()).child(dica)))
    }

    fn itens(&self, s: &Status) -> Vec<Item> {
        let mut v = Vec::new();
        let preparadas: Vec<usize> = s.files.iter().enumerate().filter(|(_, f)| f.staged.is_some()).map(|(i, _)| i).collect();
        let alteracoes: Vec<usize> = s.files.iter().enumerate().filter(|(_, f)| f.unstaged.is_some()).map(|(i, _)| i).collect();
        if !preparadas.is_empty() {
            v.push(Item::Cabecalho(Lado::Preparadas));
            if !self.recolhido.preparadas {
                v.extend(preparadas.iter().map(|&i| Item::Arquivo(Lado::Preparadas, i)));
            }
        }
        v.push(Item::Cabecalho(Lado::Alteracoes));
        if !self.recolhido.alteracoes {
            if alteracoes.is_empty() {
                v.push(Item::Vazio);
            }
            v.extend(alteracoes.iter().map(|&i| Item::Arquivo(Lado::Alteracoes, i)));
        }
        v
    }

    fn cabecalho_secao(&self, lado: Lado, n: usize, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let staged = lado.staged();
        let recolhida = if staged { self.recolhido.preparadas } else { self.recolhido.alteracoes };
        let rotulo = if staged { "Tirar todas do preparo" } else { "Preparar todas" };
        let habilitada = self.ocupado.is_none() && n > 0;
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(2.))
            .h(px(LINHA))
            .pr(px(8.))
            .child(
                div()
                    .id(if staged { "secao-preparadas" } else { "secao-alteracoes" })
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .mx(px(2.))
                            .child(icone(if recolhida { "icons/chevron-right.svg" } else { "icons/chevron-down.svg" }, 16., cor::text_primary())),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(tema::TEXTO_LABEL)
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(if staged { "Preparadas" } else { "Alterações" }),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .rounded_full()
                            .bg(cor::fill_hover())
                            .px(px(5.))
                            .text_size(px(11.))
                            .line_height(px(18.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(n.to_string()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if staged {
                            this.recolhido.preparadas = !this.recolhido.preparadas;
                        } else {
                            this.recolhido.alteracoes = !this.recolhido.alteracoes;
                        }
                        cx.notify();
                    })),
            )
            .child(
                acao_secao(if staged { "tirar-todas" } else { "preparar-todas" }, if staged { "icons/minus.svg" } else { "icons/plus.svg" }, habilitada)
                    .tooltip_texto(rotulo)
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if this.ocupado.is_none() {
                            this.preparar(Vec::new(), staged, w, cx)
                        }
                    })),
            )
            .into_any_element()
    }

    fn linha_arquivo(&self, f: &GitFile, lado: Lado, i: usize, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(k) = tipo(f, lado) else { return div().into_any_element() };
        let staged = lado.staged();
        let (nome, pasta) = dividir(&f.path);
        let completo = match &f.orig_path {
            Some(o) => format!("{} (renomeado de {o})", f.path),
            None => f.path.clone(),
        };
        let aberta = match &self.selecionado {
            Some(Alvo::Conflito { caminho }) => *caminho == f.path && k == Kind::Conflicted,
            Some(Alvo::Diff { lado: l, caminho }) => *l == lado && *caminho == f.path,
            _ => false,
        };
        let marcado = self.marcados.contains(&(lado, f.path.clone()));
        let marcados_aqui = self.arquivos(lado).iter().filter(|x| self.marcados.contains(&(lado, x.path.clone()))).count();
        let lote = marcados_aqui > 1 && marcado;
        let rotulo_stage = format!("{}{}", if staged { "Tirar do preparo" } else { "Preparar" }, if lote { format!(" ({marcados_aqui})") } else { String::new() });
        let ocupado = self.ocupado.is_some();
        let c = cor_tipo(k);
        let grupo: SharedString = format!("linha-{}-{i}", if staged { "p" } else { "a" }).into();
        let f2 = f.clone();
        let f3 = f.clone();
        let f4 = f.clone();
        let f5 = f.clone();
        let entidade = cx.entity();
        let menu_f = f.clone();
        let rotulo_menu = rotulo_stage.clone();
        div()
            .id((if staged { "arquivo-p" } else { "arquivo-a" }, i))
            .group(grupo.clone())
            .w_full()
            .flex()
            .items_center()
            .h(px(LINHA))
            .pr(px(8.))
            .relative()
            .hover(|d| d.bg(cor::fill_muted()))
            .when(aberta, |d| d.bg(cor::fill_muted()).child(div().absolute().left_0().top_0().bottom_0().w(px(2.)).bg(cor::ink_white())))
            .when(marcado, |d| d.border_1().border_color(gpui_kit::rgba(0x6f6f6f66)))
            .context_menu(move |menu, _, _| {
                let (e1, e2, e3) = (entidade.clone(), entidade.clone(), entidade.clone());
                let (fa, fb, fc) = (menu_f.clone(), menu_f.clone(), menu_f.clone());
                let menu = menu.item(
                    PopupMenuItem::new(if k == Kind::Conflicted { "Resolver conflito" } else { "Revisar alterações" }).on_click(
                        move |_, _, cx| e1.update(cx, |_, cx| cx.emit(EventoGit::AbrirAlvo { alvo: TelaGit::alvo_de(&fa, lado), permanente: true })),
                    ),
                );
                let menu = if k == Kind::Deleted {
                    menu
                } else {
                    menu.item(PopupMenuItem::new("Abrir arquivo").on_click(move |_, _, cx| {
                        e2.update(cx, |_, cx| cx.emit(EventoGit::AbrirArquivo { caminho: fb.path.clone(), permanente: true }))
                    }))
                };
                menu.item(PopupMenuItem::new(rotulo_menu.clone()).disabled(ocupado).on_click(move |_, w, cx| {
                    e3.update(cx, |this, cx| this.preparar_linha(&fc, lado, w, cx))
                }))
            })
            .child(
                div()
                    .id("abrir")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(20.))
                    .pr(px(4.))
                    .text_size(px(13.))
                    .tooltip_texto(format!("{completo}: {}. Ver alterações (duplo clique abre de vez; Ctrl+clique marca)", palavra(k)))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |_, e: &gpui_kit::MouseDownEvent, _, cx| {
                        if !e.modifiers.control && !e.modifiers.shift && !e.modifiers.platform && tipo(&f5, lado) != Some(Kind::Conflicted) {
                            cx.emit(EventoGit::PreCarregar { alvo: Alvo::Diff { lado, caminho: f5.path.clone() }, hover: false });
                        }
                    }))
                    .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                        if e.click_count() >= 2 {
                            cx.emit(EventoGit::AbrirAlvo { alvo: TelaGit::alvo_de(&f2, lado), permanente: true });
                        } else {
                            this.escolher_linha(&f2, lado, e.modifiers().control || e.modifiers().platform, cx);
                        }
                    }))
                    .child(icone_arquivo(&f.path, 16.))
                    .child(div().flex_shrink_0().max_w(gpui_kit::relative(0.7)).truncate().text_color(c).child(nome.to_string()))
                    .when(!pasta.is_empty(), |d| {
                        d.child(div().flex_1().min_w_0().truncate().text_size(px(11.7)).opacity(0.7).child(pasta.to_string()))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .w(px(48.))
                    .justify_end()
                    .items_center()
                    .invisible()
                    .group_hover(grupo, |s| s.visible())
                    .when(k != Kind::Deleted, |d| {
                        d.child(acao("abrir-editor", "icons/square-arrow-out-up-right.svg", true).tooltip_texto("Abrir no editor").on_click(
                            cx.listener(move |_, _, _, cx| cx.emit(EventoGit::AbrirArquivo { caminho: f3.path.clone(), permanente: true })),
                        ))
                    })
                    .child(
                        acao("stage-linha", if staged { "icons/minus.svg" } else { "icons/plus.svg" }, !ocupado)
                            .tooltip_texto(if staged { "Tirar do preparo" } else { "Preparar" })
                            .on_click(cx.listener(move |this, _, w, cx| {
                                if this.ocupado.is_none() {
                                    this.preparar_linha(&f4, lado, w, cx)
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .id("letra")
                    .w(px(16.))
                    .flex_none()
                    .text_center()
                    .text_size(tema::TEXTO_LABEL)
                    .text_color(c)
                    .tooltip_texto(palavra(k))
                    .child(letra(k)),
            )
            .into_any_element()
    }

    fn lista_arquivos(&self, s: &Status, cx: &mut Context<Self>) -> impl IntoElement {
        let itens = Rc::new(self.itens(s));
        let n = itens.len();
        let medidas = self.medidas.clone();
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .flex_basis(px(0.))
            .min_h(px(0.))
            .child(
                uniform_list(
                    "arquivos",
                    n,
                    cx.processor(move |this, faixa: std::ops::Range<usize>, _window, cx| {
                        let Some(s) = this.status().cloned() else { return Vec::new() };
                        let preparadas = s.files.iter().filter(|f| f.staged.is_some()).count();
                        let alteracoes = s.files.iter().filter(|f| f.unstaged.is_some()).count();
                        faixa
                            .filter_map(|i| itens.get(i).copied())
                            .map(|item| match item {
                                Item::Cabecalho(l) => this.cabecalho_secao(l, if l.staged() { preparadas } else { alteracoes }, cx),
                                Item::Arquivo(l, i) => this.linha_arquivo(&s.files[i], l, i, cx),
                                Item::Vazio => div()
                                    .w_full()
                                    .h(px(LINHA))
                                    .px(px(20.))
                                    .flex()
                                    .items_center()
                                    .text_size(tema::TEXTO_LABEL)
                                    .text_color(descricao())
                                    .child("Nenhuma alteração.")
                                    .into_any_element(),
                            })
                            .collect()
                    }),
                )
                .track_scroll(&self.lista)
                .size_full(),
            )
            .child(
                canvas(move |b, _, _| {
                    let (l, _, g) = medidas.get();
                    let _ = l;
                    medidas.set((f32::from(b.size.height), 0., g));
                }, |_, _, _, _| {})
                .absolute()
                .size_full(),
            )
    }
}

impl Render for TelaGit {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::TestSupportExt as _;
        self.conteudo(window, cx).test_support()
    }
}

impl TelaGit {
    fn conteudo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let raiz = div()
            .id("tela-git")
            .key_context("TelaGit")
            .track_focus(&self.foco)
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(cor::pitch_black())
            .text_color(cor::text_primary())
            .font_family(tema::FONTE)
            .when(self.arrasto.is_some(), |d| {
                d.cursor_row_resize()
                    .on_mouse_move(cx.listener(|this, e: &gpui_kit::MouseMoveEvent, _, cx| this.mover_divisor(f32::from(e.position.y), cx)))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                        this.arrasto = None;
                        cx.notify();
                    }))
            });
        match self.estado.clone() {
            Some(e) if !matches!(e, Estado::Repo { .. }) => return raiz.child(self.vazio(&e, cx)),
            _ => {}
        }
        let contagem = self.status().map(|s| s.files.len());
        let raiz = raiz.child(self.abas(contagem, cx));
        if self.vista == Vista::Historico {
            return raiz.child(self.historico(window, cx));
        }
        let Some(s) = self.status().cloned() else {
            let texto = if self.erro.is_empty() { "Lendo o Git…".to_string() } else { self.erro.clone() };
            return raiz.child(div().p(px(12.)).text_size(tema::TEXTO_LABEL).text_color(if self.erro.is_empty() { descricao() } else { comum::perigo() }).child(texto));
        };
        raiz.child(self.cabecalho_branch(&s, window, cx))
            .child(self.bloco_commit(&s, cx))
            .child(self.lista_arquivos(&s, cx))
            .children(self.secao_grafo(&s, window, cx))
            .children(self.cartao_hover(cx))
    }
}
