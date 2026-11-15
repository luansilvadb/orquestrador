//! A coluna central do editor (porte de `src/editor/editor-workspace.tsx`): até dois grupos de abas lado a lado, cada
//! aba um arquivo (editor do Kit), um rascunho sem caminho, um Markdown com Preview ou um conteúdo de outro tipo (diff,
//! commit, conflito, mídia) que a bancada fornece por [`FabricaConteudo`]. Buffers e rascunhos são do documento
//! (`doc_id`), não da aba. API pública e eventos em `README.md`.
use crate::armazem::{
    self, chave_conflito, codificar_recuperacao, decodificar_recuperacao, ha_rascunho_em, ler_recuperacao,
    normalizar_quebras, para_disco, quebra_de, remapear_dados, ArquivoChaves, Buffer, BufferRecuperado, Chaves, Dados,
    RascunhoConflito, Recuperacao, Rolagem, Sujeira,
};
use crate::caminhos::{self, absoluto_no_projeto, dentro_de, eh_absoluto, nome_arquivo, relativo_a, Cursor, TipoCopia};
use crate::documento::{self, Comando, ComTemaDoEditor, Documento};
use crate::linguagem::eh_markdown;
use crate::modelo::{self as m, Alvo, Escopo, Grupo, Vista, Workspace};
use crate::{abrir_rapido, formatar, migalhas, minimapa};
use gpui::{
    anchored, deferred, div, prelude::*, px, AnyElement, AnyView, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    Focusable, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString, Subscription, Task,
    Window,
};
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState, RopeExt as _};
use gpui_kit::component::Sizable as _;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use orquestrador_core::ide::files;
use orquestrador_ui::avisos::{self, Aviso};
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::{self, icone, item_menu, TooltipTexto};
use orquestrador_ui::{icone_arquivo, Aba, EstadoFaixa, EstadoLista, FaixaAbas, ItemLista, ListaAbas, MoverAba};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

mod git;
mod pre;

/// Intervalo da consulta ao disco das abas ativas (o `POLL_MS` do React).
pub const CONSULTA: Duration = Duration::from_millis(2500);
const PROPORCAO_PADRAO: f32 = 0.5;
const ESPERA_RECUPERACAO: Duration = Duration::from_millis(400);

/// Avisos para a bancada.
#[derive(Clone, Debug, PartialEq)]
pub enum EventoEditor {
    /// Alvo da aba ativa no grupo focado mudou (a barra do Git marca o diff aberto).
    AbaAtiva(Option<Alvo>),
    /// Gravação no disco feita pelo editor (Ctrl+S, salvar tudo, comparação, resolução).
    Salvo,
    /// Uma operação mexeu em arquivos do repositório: a bancada atualiza o Git.
    AtualizarGit,
    /// "Revelar no Explorer" da aba.
    RevelarNoExplorer(String),
    /// Quantos grupos existem (1 ou 2).
    Grupos(usize),
    /// Layout da bancada guardado na recuperação, entregue uma vez na montagem.
    RestaurarLayout(String),
    /// Próxima (1) ou anterior (-1) alteração do Git no documento (Alt+F5): quem cuida do gutter do Git navega.
    IrParaAlteracao { doc_id: String, passo: i32 },
}

/// Opções de abertura (`OpenOptions`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Abertura {
    pub permanente: bool,
    pub linha: Option<u64>,
    pub coluna: Option<u64>,
    pub lado: Option<Grupo>,
}

/// Conteúdo de uma aba que não é texto (diff, commit, conflito, mídia): a bancada devolve a vista para o alvo, ou
/// `None` quando não é dela. Chamada uma vez por aba; a vista fica guardada enquanto a aba existir.
pub type FabricaConteudo = Rc<dyn Fn(&Alvo, &mut Window, &mut App) -> Option<AnyView>>;

/// Chamado quando o editor descarta o rascunho de um conflito (descartar no "fechar com não salvos" ou tudo): a vista
/// do conflito relê o arquivo (`VisaoConflito::carregar(true, ..)`).
pub type AoDescartarConflito = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// Chamado quando um documento ganha editor: o lugar para o Git ligar decorações de gutter, régua e widgets.
pub type AoAbrirDocumento = Rc<dyn Fn(&InfoDocumento, &Entity<EditorState>, &mut Window, &mut App)>;

#[derive(Clone)]
pub struct InfoDocumento {
    pub doc_id: String,
    /// Relativo ao projeto, absoluto (arquivo solto) ou vazio (rascunho).
    pub caminho: String,
    /// Decorações do gutter (16 px à esquerda dos números), da régua de visão geral e de trechos.
    pub gutter: gpui_kit::component::input::GutterDecorationCollection,
    pub regua: gpui_kit::component::input::OverviewRulerDecorationCollection,
    pub trechos: gpui_kit::component::input::RangeDecorationCollection,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Situacao {
    erro: Option<String>,
    binario: bool,
    grande: bool,
}

/// Versão do disco diferente de um rascunho sujo: a tela oferece comparar, nunca troca sozinha.
#[derive(Clone, Debug, PartialEq)]
struct VersaoDisco {
    texto: String,
    bom: bool,
    somente_leitura: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum ModoComparacao {
    Manter,
    Usar,
}

struct Comparacao {
    doc_id: String,
    caminho: String,
    modo: ModoComparacao,
    disco: VersaoDisco,
    rascunho: String,
}

type Resposta = Box<dyn FnOnce(&mut AreaEditor, Option<&'static str>, &mut Window, &mut Context<AreaEditor>)>;

struct Pergunta {
    titulo: SharedString,
    corpo: SharedString,
    acoes: Vec<(&'static str, &'static str, bool)>,
    resposta: Option<Resposta>,
}

struct IrPara {
    campo: Entity<InputState>,
    erro: Option<String>,
    doc_id: String,
    _sub: Subscription,
}

/// Ação de atalho, consumida uma vez.
type AcaoUnica = Box<dyn FnOnce(&mut AreaEditor, &mut Window, &mut Context<AreaEditor>)>;
/// Ação de item de menu.
type AcaoMenu = Box<dyn Fn(&mut AreaEditor, &mut Window, &mut Context<AreaEditor>)>;

/// A coluna central do editor.
pub struct AreaEditor {
    projeto: String,
    ativo: bool,
    ws: Workspace,
    docs: HashMap<String, Documento>,
    conflitos: std::collections::BTreeMap<String, RascunhoConflito>,
    situacao: HashMap<String, Situacao>,
    disco: HashMap<String, VersaoDisco>,
    cursores: HashMap<String, Cursor>,
    rolagens: HashMap<String, Rolagem>,
    conteudos: HashMap<String, AnyView>,
    fabrica: Option<FabricaConteudo>,
    ao_abrir: Vec<AoAbrirDocumento>,
    git: HashMap<String, crate::gutter_git::EstadoGit>,
    fonte_git: Option<std::sync::Arc<dyn crate::gutter_git::FonteGutter>>,
    pre_leituras: HashMap<String, pre::PreLeitura>,
    ao_descartar_conflito: Option<AoDescartarConflito>,
    proporcao: f32,
    aviso: Option<String>,
    layout: Option<String>,
    faixas: [EstadoFaixa; 2],
    listas: [EstadoLista; 2],
    salvando: HashSet<String>,
    pergunta: Option<Pergunta>,
    comparacao: Option<Comparacao>,
    ir_para: Option<IrPara>,
    abrir_rapido: Option<Entity<abrir_rapido::AbrirRapido>>,
    migalha: Option<migalhas::Seletor>,
    arrasto_divisor: Option<(f32, f32, f32)>,
    largura: f32,
    acorde: Option<std::time::Instant>,
    armazenamento: Rc<RefCell<dyn Chaves>>,
    intocado: bool,
    ultimo_alvo: Option<Option<Alvo>>,
    ultimo_grupos: usize,
    rascunho_novo: Option<String>,
    revelado: [Option<(String, usize)>; 2],
    com_alteracoes: HashSet<String>,
    arrasto_aba: Option<(Grupo, String)>,
    caixas_faixa: [Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>; 2],
    ponteiro: gpui::Point<gpui::Pixels>,
    pub foco: FocusHandle,
    _subs: HashMap<String, Subscription>,
    _sub_rapido: Option<Subscription>,
    tarefa_recuperacao: Option<Task<()>>,
    _consulta: Task<()>,
}

impl EventEmitter<EventoEditor> for AreaEditor {}

impl Focusable for AreaEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

fn idx(g: Grupo) -> usize {
    match g {
        Grupo::A => 0,
        Grupo::B => 1,
    }
}

fn novo_doc_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("doc-{t:x}-{}", N.fetch_add(1, Ordering::Relaxed))
}

/// Mesmo nome em pastas diferentes: a pasta vira o detalhe da aba.
fn detalhes(caminhos: &[&str]) -> HashMap<String, String> {
    let mut conta: HashMap<&str, usize> = HashMap::new();
    for c in caminhos {
        *conta.entry(nome_arquivo(c)).or_default() += 1;
    }
    caminhos
        .iter()
        .map(|c| {
            let d = if conta[nome_arquivo(c)] > 1 {
                let (_, pasta) = caminhos::separar(c);
                if pasta.is_empty() { "raiz".to_string() } else { pasta.to_string() }
            } else {
                String::new()
            };
            (c.to_string(), d)
        })
        .collect()
}

/// Nome curto nos avisos.
fn nome_de(a: &Alvo) -> String {
    match a {
        Alvo::Draft { num, .. } => m::nome_rascunho(*num),
        a => nome_arquivo(a.caminho().unwrap_or("")).to_string(),
    }
}

/// Texto da aba na lista e na dica.
pub fn rotulo(a: &Alvo) -> String {
    match a {
        Alvo::File { path, .. } => path.clone(),
        Alvo::Diff { side, path } => format!("{path} ({})", if *side == m::Lado::Staged { "preparadas" } else { "alterações" }),
        Alvo::Commit { hash, path } => format!("{path} @ {}", &hash[..hash.len().min(7)]),
        Alvo::Conflict { path } => format!("{path} (conflito)"),
        Alvo::Draft { num, .. } => m::nome_rascunho(*num),
    }
}

/// Título curto da aba.
pub fn titulo(a: &Alvo) -> String {
    match a {
        Alvo::Draft { num, .. } => m::nome_rascunho(*num),
        Alvo::File { path, .. } => nome_arquivo(path).to_string(),
        Alvo::Diff { side, path } => {
            format!("{} · {}", nome_arquivo(path), if *side == m::Lado::Staged { "preparadas" } else { "alterações" })
        }
        Alvo::Commit { hash, path } => format!("{} · {}", &hash[..hash.len().min(7)], nome_arquivo(path)),
        Alvo::Conflict { path } => format!("{} · conflito", nome_arquivo(path)),
    }
}

/// X de 20 px à direita da aba, visível na ativa e com o ponteiro sobre a aba (`grupo`); com alteração não salva, o ponto
/// fica no lugar dele até o ponteiro chegar. O ● do React sai da fonte do sistema com ~6 px; o da Inter é maior, então
/// o ponto é desenhado.
fn fechar_ou_modificado(
    id: impl Into<gpui::ElementId>, grupo: impl Into<SharedString>, ativa: bool, modificado: bool,
    ao_fechar: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let grupo: SharedString = grupo.into();
    div().absolute().right(px(4.)).top_0().bottom_0().flex().items_center().child(
        div()
            .relative()
            .size(px(20.))
            .when(modificado, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .group_hover(grupo.clone(), |s| s.opacity(0.))
                        .child(div().mt(px(2.)).size(px(6.5)).rounded_full().bg(cor::text_primary())),
                )
            })
            .child(
                div()
                    .id(id)
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(tema::RAIO_MENU)
                    .hover(|s| s.bg(cor::fill_hover()))
                    .when(!(ativa && !modificado), |d| d.opacity(0.))
                    .group_hover(grupo, |s| s.opacity(1.))
                    .tooltip_texto("Fechar (Ctrl+F4)")
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |e, w, cx| {
                        cx.stop_propagation();
                        ao_fechar(e, w, cx);
                    })
                    .child(icone(orquestrador_ui::recursos::icone::FECHAR, 16., cor::text_secondary())),
            ),
    )
}

fn icone_alvo(a: &Alvo, tamanho: f32) -> AnyElement {
    match a {
        Alvo::Draft { .. } => icone(orquestrador_ui::recursos::icone::ARQUIVOS, tamanho, cor::text_secondary())
            .path("icons/file-text.svg")
            .into_any_element(),
        a => gpui::img(icone_arquivo(a.caminho().unwrap_or(""), false, false)).size(px(tamanho)).flex_none().into_any_element(),
    }
}

impl AreaEditor {
    /// `projeto`: raiz absoluta (vazia sem projeto), chave da recuperação.
    pub fn new(projeto: impl Into<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::com_armazenamento(projeto, Rc::new(RefCell::new(ArquivoChaves::padrao())), window, cx)
    }

    /// Como [`AreaEditor::new`], com a recuperação noutro armazenamento (testes e vitrine).
    pub fn com_armazenamento(
        projeto: impl Into<String>, armazenamento: Rc<RefCell<dyn Chaves>>, window: &mut Window, cx: &mut Context<Self>,
    ) -> Self {
        documento::registrar_atalhos(cx);
        crate::markdown::instalar_realce_de_codigo(cx);
        let projeto = projeto.into();
        let consulta = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(CONSULTA).await;
            let Ok(()) = this.update_in(cx, |a, window, cx| a.consultar_disco(window, cx)) else { break };
        });
        let mut a = AreaEditor {
            projeto: String::new(),
            ativo: true,
            ws: Workspace::default(),
            docs: HashMap::new(),
            conflitos: Default::default(),
            situacao: HashMap::new(),
            disco: HashMap::new(),
            cursores: HashMap::new(),
            rolagens: HashMap::new(),
            conteudos: HashMap::new(),
            fabrica: None,
            ao_abrir: Vec::new(),
            git: HashMap::new(),
            fonte_git: None,
            pre_leituras: HashMap::new(),
            ao_descartar_conflito: None,
            proporcao: PROPORCAO_PADRAO,
            aviso: None,
            layout: None,
            faixas: [EstadoFaixa::new(), EstadoFaixa::new()],
            listas: [EstadoLista::new(cx), EstadoLista::new(cx)],
            salvando: HashSet::new(),
            pergunta: None,
            comparacao: None,
            ir_para: None,
            abrir_rapido: None,
            migalha: None,
            arrasto_divisor: None,
            largura: 0.,
            acorde: None,
            armazenamento,
            intocado: false,
            ultimo_alvo: None,
            ultimo_grupos: 1,
            rascunho_novo: None,
            revelado: [None, None],
            com_alteracoes: HashSet::new(),
            arrasto_aba: None,
            caixas_faixa: [Rc::default(), Rc::default()],
            ponteiro: Default::default(),
            foco: cx.focus_handle(),
            _subs: HashMap::new(),
            _sub_rapido: None,
            tarefa_recuperacao: None,
            _consulta: consulta,
        };
        a.carregar_projeto(projeto, window, cx);
        a
    }

    // ---- Estado público ----

    pub fn projeto(&self) -> &str {
        &self.projeto
    }

    pub fn workspace(&self) -> &Workspace {
        &self.ws
    }

    /// Troca o projeto: grava a recuperação do atual e restaura a do novo.
    pub fn definir_projeto(&mut self, projeto: impl Into<String>, window: &mut Window, cx: &mut Context<Self>) {
        let projeto = projeto.into();
        if projeto == self.projeto {
            return;
        }
        self.gravar_recuperacao(cx);
        self.carregar_projeto(projeto, window, cx);
    }

    /// Área visível (a tela Arquivos ativa): liga a consulta ao disco.
    pub fn definir_ativo(&mut self, ativo: bool) {
        self.ativo = ativo;
    }

    /// Layout opaco da bancada, guardado junto da recuperação.
    pub fn definir_layout(&mut self, layout: Option<String>, cx: &mut Context<Self>) {
        self.layout = layout;
        self.agendar_recuperacao(cx);
    }

    /// Conteúdo das abas de diff, commit, conflito e mídia.
    pub fn definir_fabrica_conteudo(&mut self, f: FabricaConteudo) {
        self.fabrica = Some(f);
    }

    /// Registra quem liga decorações nos editores (gutter do Git etc.). Vale para os documentos já abertos também.
    pub fn ao_abrir_documento(&mut self, f: AoAbrirDocumento, window: &mut Window, cx: &mut Context<Self>) {
        for d in self.docs.values() {
            f(&info_de(d), &d.editor, window, cx);
        }
        self.ao_abrir.push(f);
    }

    /// Estado do editor do documento aberto em `caminho` (relativo ou absoluto), para decorações.
    pub fn editor_de(&self, caminho: &str) -> Option<Entity<EditorState>> {
        let c = caminho.to_lowercase();
        self.docs.values().find(|d| d.buffer.path.to_lowercase() == c).map(|d| d.editor.clone())
    }

    /// Decorações do documento aberto em `caminho`.
    pub fn decoracoes(&self, caminho: &str) -> Option<InfoDocumento> {
        let c = caminho.to_lowercase();
        self.docs.values().find(|d| d.buffer.path.to_lowercase() == c).map(info_de)
    }

    // ---- Abrir ----

    fn sujos(&self) -> HashSet<String> {
        let mut s: HashSet<String> = self.docs.values().filter(|d| d.sujo()).map(|d| d.buffer.doc_id.clone()).collect();
        s.extend(self.conflitos.values().filter(|c| c.sujo()).map(|c| chave_conflito(&c.path)));
        s
    }

    fn com_grupo(&self, g: Grupo) -> Workspace {
        let mut s = self.ws.clone();
        if s.grupo(g).is_none() && s.groups.len() < m::MAX_GRUPOS {
            s.groups.push(m::GrupoAbas { id: g, tabs: vec![], active: None });
        }
        s
    }

    fn definir_ws(&mut self, ws: Workspace, cx: &mut Context<Self>) {
        if ws == self.ws {
            return;
        }
        self.ws = ws;
        self.intocado = false;
        self.podar(cx);
        self.avisar_mudancas(cx);
        self.agendar_recuperacao(cx);
        cx.notify();
    }

    fn avisar_mudancas(&mut self, cx: &mut Context<Self>) {
        let alvo = self.ws.aba_focada().map(|t| t.alvo.clone());
        if self.ultimo_alvo.as_ref() != Some(&alvo) {
            self.ultimo_alvo = Some(alvo.clone());
            cx.emit(EventoEditor::AbaAtiva(alvo));
        }
        if self.ultimo_grupos != self.ws.groups.len() {
            self.ultimo_grupos = self.ws.groups.len();
            cx.emit(EventoEditor::Grupos(self.ultimo_grupos));
        }
    }

    fn doc_id_para(&self, caminho: &str) -> String {
        let c = caminho.to_lowercase();
        if let Some(d) = self.docs.values().find(|d| d.buffer.path.to_lowercase() == c) {
            return d.buffer.doc_id.clone();
        }
        self.ws
            .abas()
            .find_map(|t| match &t.alvo {
                Alvo::File { doc_id, path } if path.to_lowercase() == c => Some(doc_id.clone()),
                _ => None,
            })
            .unwrap_or_else(novo_doc_id)
    }

    /// Abre o arquivo (relativo ao projeto ou absoluto). Sem `permanente`, vira prévia. Linha e coluna (1-based) revelam.
    pub fn abrir_arquivo(&mut self, caminho: &str, op: Abertura, window: &mut Window, cx: &mut Context<Self>) {
        let caminho = caminho.replace('\\', "/");
        let caminho = if eh_absoluto(&caminho) {
            relativo_a(&self.projeto, &caminho).unwrap_or_else(|| caminho.clone())
        } else {
            caminho
        };
        if !m::caminho_arquivo_seguro(&caminho) {
            return;
        }
        let doc_id = self.doc_id_para(&caminho);
        let alvo = Alvo::arquivo(doc_id.clone(), caminho.clone());
        self.abrir_alvo(alvo, op, window, cx);
        if !self.docs.contains_key(&doc_id) && !self.eh_conteudo(&doc_id) {
            self.ler_arquivo(&doc_id, &caminho, op.linha.map(|l| (l, op.coluna.unwrap_or(1))), window, cx);
        } else if let (Some(l), Some(d)) = (op.linha, self.docs.get(&doc_id)) {
            d.revelar(l, op.coluna.unwrap_or(1), window, cx);
        }
    }

    /// Abre à direita: com dois grupos, o segundo; com um, o outro id.
    pub fn abrir_ao_lado(&mut self, caminho: &str, mut op: Abertura, window: &mut Window, cx: &mut Context<Self>) {
        op.lado = Some(if self.ws.groups.len() >= 2 { self.ws.groups[1].id } else { self.ws.groups[0].id.outro() });
        self.abrir_arquivo(caminho, op, window, cx);
    }

    /// Abre um alvo do modelo (diff, commit, conflito ou arquivo com `doc_id`).
    pub fn abrir_alvo(&mut self, alvo: Alvo, op: Abertura, window: &mut Window, cx: &mut Context<Self>) {
        let g = op.lado.unwrap_or(self.ws.focus);
        let base = self.com_grupo(g);
        let novo = if op.permanente { m::abrir_permanente(&base, g, alvo) } else { m::abrir_previa(&base, g, alvo, &self.sujos()) };
        self.definir_ws(novo, cx);
        self.criar_conteudos(window, cx);
        self.focar_ativo(window, cx);
    }

    fn eh_conteudo(&self, doc_id: &str) -> bool {
        self.ws.abas().any(|t| t.alvo.doc_id() == Some(doc_id) && self.conteudos.contains_key(&t.id))
    }

    /// Vistas de conteúdo das abas novas que não são texto.
    fn criar_conteudos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.fabrica.clone() else { return };
        let faltam: Vec<(String, Alvo)> = self
            .ws
            .abas()
            .filter(|t| !self.conteudos.contains_key(&t.id) && !matches!(t.alvo, Alvo::Draft { .. }))
            .filter(|t| !matches!(&t.alvo, Alvo::File { doc_id, .. } if self.docs.contains_key(doc_id) || self.situacao.contains_key(doc_id)))
            .map(|t| (t.id.clone(), t.alvo.clone()))
            .collect();
        for (id, alvo) in faltam {
            if let Some(v) = f(&alvo, window, cx) {
                self.conteudos.insert(id, v);
            }
        }
    }

    fn ler_arquivo(&mut self, doc_id: &str, caminho: &str, revelar: Option<(u64, u64)>, window: &mut Window, cx: &mut Context<Self>) {
        // A pré-leitura (passar o mouse, apertar o botão) já pode ter o texto, ou estar a caminho.
        if self.usar_pre_leitura(doc_id, caminho, revelar, window, cx) {
            return;
        }
        let raiz = PathBuf::from(&self.projeto);
        let rel = caminho.to_string();
        let leitura = cx.background_spawn(async move { ler_conteudo(&raiz, &rel) });
        let (doc_id, caminho) = (doc_id.to_string(), caminho.to_string());
        cx.spawn_in(window, async move |this, cx| {
            let r = leitura.await;
            let _ = this.update_in(cx, |a, window, cx| a.aplicar_leitura_inicial(&doc_id, &caminho, revelar, r, window, cx));
        })
        .detach();
    }

    fn aplicar_leitura_inicial(
        &mut self, doc_id: &str, caminho: &str, revelar: Option<(u64, u64)>, r: Result<files::Content, String>, window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A aba pode ter fechado durante a leitura.
        if !self.ws.abas().any(|t| t.alvo.doc_id() == Some(doc_id)) || self.docs.contains_key(doc_id) {
            return;
        }
        match r {
            Err(e) => {
                self.situacao.insert(doc_id.to_string(), Situacao { erro: Some(e), ..Default::default() });
            }
            Ok(c) if c.binary || c.too_large => {
                self.situacao.insert(doc_id.to_string(), Situacao { binario: c.binary, grande: !c.binary, ..Default::default() });
            }
            Ok(c) => {
                self.situacao.remove(doc_id);
                let b = Buffer::do_disco(doc_id, caminho, c.text, c.bom, c.read_only);
                self.instalar_documento(b, window, cx);
                if let (Some((l, col)), Some(d)) = (revelar, self.docs.get(doc_id)) {
                    d.revelar(l, col, window, cx);
                }
            }
        }
        cx.notify();
    }

    fn instalar_documento(&mut self, b: Buffer, window: &mut Window, cx: &mut Context<Self>) {
        let doc_id = b.doc_id.clone();
        let d = Documento::novo(b, window, cx);
        let editor = d.editor.clone();
        let id = doc_id.clone();
        let sub = cx.subscribe_in(&editor, window, move |a, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => a.ao_editar(&id, cx),
            InputEvent::Focus => a.ao_focar_documento(&id, cx),
            _ => {
                let _ = window;
            }
        });
        self._subs.insert(doc_id.clone(), sub);
        let info = info_de(&d);
        // Restaura cursor e rolagem guardados.
        if let Some(c) = self.cursores.get(&doc_id).copied() {
            d.revelar(c.line, c.col, window, cx);
        }
        if let Some(r) = self.rolagens.get(&doc_id).copied() {
            editor.update(cx, |s, cx| s.set_scroll_offset(gpui::point(px(-r.left as f32), px(-r.top as f32)), cx));
        }
        self.docs.insert(doc_id.clone(), d);
        for f in self.ao_abrir.clone() {
            f(&info, &editor, window, cx);
        }
        self.git_carregar(&doc_id, cx);
    }

    fn ao_editar(&mut self, doc_id: &str, cx: &mut Context<Self>) {
        self.git_ao_editar(doc_id, cx);
        let Some(d) = self.docs.get_mut(doc_id) else { return };
        d.sincronizar(cx);
        let (l, c) = d.cursor(cx);
        self.cursores.insert(doc_id.to_string(), Cursor { line: l, col: c });
        // Editar torna a aba permanente.
        let mut ws = self.ws.clone();
        for t in self.ws.abas() {
            if t.alvo.doc_id() == Some(doc_id) && t.preview {
                ws = m::marcar_editada(&ws, &t.id);
            }
        }
        self.intocado = false;
        self.definir_ws(ws, cx);
        self.agendar_recuperacao(cx);
        cx.notify();
    }

    fn ao_focar_documento(&mut self, doc_id: &str, cx: &mut Context<Self>) {
        if let Some((g, _)) = self.ws.groups.iter().find_map(|g| {
            let t = g.tabs.iter().find(|t| Some(&t.id) == g.active.as_ref())?;
            (t.alvo.doc_id() == Some(doc_id)).then_some((g.id, ()))
        }) {
            if self.ws.focus != g {
                let mut ws = self.ws.clone();
                ws.focus = g;
                self.definir_ws(ws, cx);
            }
        }
    }

    /// Rascunho novo ("Sem título-N"), permanente, ativo e com o foco no texto.
    pub fn novo_arquivo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let g = self.ws.focus;
        self.novo_rascunho(g, window, cx);
    }

    fn novo_rascunho(&mut self, g: Grupo, window: &mut Window, cx: &mut Context<Self>) {
        let doc_id = novo_doc_id();
        self.instalar_documento(Buffer::vazio(&doc_id), window, cx);
        let base = self.com_grupo(g);
        self.rascunho_novo = Some(doc_id.clone());
        self.definir_ws(m::abrir_rascunho(&base, g, &doc_id), cx);
        if let Some(d) = self.docs.get(&doc_id) {
            d.editor.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    /// Abre arquivos escolhidos no diálogo do sistema (menu Arquivo > Abrir arquivo).
    pub fn abrir_pelo_dialogo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: true, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(caminhos))) = escolha.await else { return };
            let _ = this.update_in(cx, |a, window, cx| {
                for c in caminhos {
                    a.abrir_arquivo(&c.to_string_lossy(), Abertura { permanente: true, ..Default::default() }, window, cx);
                }
            });
        })
        .detach();
    }

    // ---- Fechar ----

    /// Fecha com a decisão de rascunho antes (Salvar, Descartar, Cancelar).
    pub fn fechar(&mut self, escopo: Escopo, _window: &mut Window, cx: &mut Context<Self>) {
        let plano = m::planejar_fechamento(&self.ws, &escopo, &self.sujos());
        if plano.abas.is_empty() {
            return;
        }
        let fechando: HashSet<&String> = plano.abas.iter().collect();
        let ficam: HashSet<String> = self.ws.abas().filter(|t| !fechando.contains(&t.id)).map(|t| t.alvo.chave()).collect();
        let nao_salvas: Vec<(String, Alvo)> = plano
            .nao_salvas
            .iter()
            .filter_map(|id| self.ws.localizar(id).map(|(_, t)| (t.id.clone(), t.alvo.clone())))
            .filter(|(_, a)| !ficam.contains(&a.chave()))
            .collect();
        if nao_salvas.is_empty() {
            self.concluir_fechamento(escopo, HashSet::new(), cx);
            return;
        }
        let nomes = nao_salvas.iter().map(|(_, a)| nome_de(a)).collect::<Vec<_>>().join(", ");
        self.perguntar(
            "Fechar com alterações não salvas?",
            format!("Há alterações não salvas em {nomes}. Salvar antes de fechar, descartar ou cancelar?"),
            vec![("discard", "Descartar", true), ("save", "Salvar", false)],
            move |a, escolha, window, cx| match escolha {
                Some("discard") => {
                    let descartadas: HashSet<String> = nao_salvas.iter().map(|(id, _)| id.clone()).collect();
                    for (_, alvo) in &nao_salvas {
                        a.descartar_alvo(alvo, window, cx);
                    }
                    a.concluir_fechamento(escopo.clone(), descartadas, cx);
                }
                Some("save") => {
                    let alvos: Vec<Alvo> = nao_salvas.iter().map(|(_, a)| a.clone()).collect();
                    a.salvar_varios(alvos, window, cx, move |a, ok, _window, cx| {
                        if ok {
                            a.concluir_fechamento(escopo.clone(), HashSet::new(), cx);
                        }
                    });
                }
                _ => {}
            },
            cx,
        );
    }

    fn concluir_fechamento(&mut self, escopo: Escopo, descartadas: HashSet<String>, cx: &mut Context<Self>) {
        let de_novo = m::planejar_fechamento(&self.ws, &escopo, &self.sujos());
        let sujos = self.sujos();
        let pendentes: Vec<String> = de_novo
            .abas
            .iter()
            .filter_map(|id| self.ws.localizar(id).map(|(_, t)| t))
            .filter(|t| m::esta_sujo(&t.alvo, &sujos) && !descartadas.contains(&t.id))
            .filter(|t| !self.ws.abas().any(|o| !de_novo.abas.contains(&o.id) && o.alvo.chave() == t.alvo.chave()))
            .map(|t| nome_de(&t.alvo))
            .collect();
        if !pendentes.is_empty() {
            self.aviso = Some(format!("Ainda há alterações não salvas em {}. Nada foi fechado.", pendentes.join(", ")));
            cx.notify();
            return;
        }
        let novo = m::fechar_abas(&self.ws, &de_novo.abas);
        self.definir_ws(novo, cx);
    }

    fn descartar_alvo(&mut self, alvo: &Alvo, window: &mut Window, cx: &mut Context<Self>) {
        match alvo {
            Alvo::File { doc_id, .. } | Alvo::Draft { doc_id, .. } => {
                if let Some(d) = self.docs.get_mut(doc_id) {
                    let limpo = normalizar_quebras(&d.buffer.raw);
                    d.trocar_texto(&limpo, window, cx);
                }
            }
            Alvo::Conflict { path } => {
                if let Some(c) = self.conflitos.get_mut(&chave_conflito(path)) {
                    c.draft = c.origin.clone();
                }
                if let Some(f) = self.ao_descartar_conflito.clone() {
                    f(path, window, cx);
                }
            }
            _ => {}
        }
    }

    /// Buffers e conteúdos sem aba saem; o rascunho aberto mantém o buffer.
    fn podar(&mut self, cx: &mut Context<Self>) {
        let vivos = m::docs_vivos(&self.ws);
        let ids: HashSet<String> = self.ws.abas().map(|t| t.id.clone()).collect();
        let conflitos: HashSet<String> = self.ws.abas().filter_map(|t| match &t.alvo {
            Alvo::Conflict { path } => Some(chave_conflito(path)),
            _ => None,
        }).collect();
        self.docs.retain(|k, _| vivos.contains(k));
        self._subs.retain(|k, _| vivos.contains(k));
        self.situacao.retain(|k, _| vivos.contains(k));
        self.disco.retain(|k, _| vivos.contains(k));
        self.cursores.retain(|k, _| vivos.contains(k));
        self.rolagens.retain(|k, _| vivos.contains(k));
        self.git.retain(|k, _| vivos.contains(k));
        self.conteudos.retain(|k, _| ids.contains(k));
        self.conflitos.retain(|k, _| conflitos.contains(k));
        let _ = cx;
    }

    // ---- Salvar ----

    fn aba_focada(&self) -> Option<m::Aba> {
        self.ws.aba_focada().cloned()
    }

    /// Ctrl+S na aba ativa do grupo focado.
    pub fn salvar_ativa(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = self.aba_focada() {
            self.salvar_alvo(t.alvo, window, cx, |_, _, _, _| {});
        }
    }

    /// Salvar como na aba ativa.
    pub fn salvar_ativa_como(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = self.aba_focada() {
            match &t.alvo {
                Alvo::Draft { doc_id, .. } => self.salvar_rascunho(doc_id.clone(), window, cx, |_, _, _, _| {}),
                Alvo::File { doc_id, .. } => self.salvar_como(doc_id.clone(), window, cx),
                _ => {}
            }
        }
    }

    /// Salva arquivos e rascunhos de conflito; rascunho sem caminho abre o "Salvar como" um por um. `ao_fim` recebe se
    /// tudo ficou limpo.
    pub fn salvar_tudo(&mut self, window: &mut Window, cx: &mut Context<Self>, ao_fim: impl FnOnce(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static) {
        let mut alvos: Vec<Alvo> = Vec::new();
        for t in self.ws.abas() {
            let suja = m::esta_sujo(&t.alvo, &self.sujos());
            if suja && !alvos.iter().any(|a| a.chave() == t.alvo.chave()) {
                alvos.push(t.alvo.clone());
            }
        }
        self.salvar_varios(alvos, window, cx, move |a, ok, window, cx| {
            let limpo = ok && a.sujos().is_empty();
            ao_fim(a, limpo, window, cx)
        });
    }

    fn salvar_varios(
        &mut self, mut alvos: Vec<Alvo>, window: &mut Window, cx: &mut Context<Self>,
        ao_fim: impl FnOnce(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if alvos.is_empty() {
            ao_fim(self, true, window, cx);
            return;
        }
        let primeiro = alvos.remove(0);
        self.salvar_alvo(primeiro.clone(), window, cx, move |a, ok, window, cx| {
            if !ok {
                a.aviso = Some(format!("Não foi possível salvar {}. Nada foi fechado.", nome_de(&primeiro)));
                ao_fim(a, false, window, cx);
                return;
            }
            a.salvar_varios(alvos, window, cx, ao_fim);
        });
    }

    fn salvar_alvo(
        &mut self, alvo: Alvo, window: &mut Window, cx: &mut Context<Self>,
        ao_fim: impl FnOnce(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static,
    ) {
        match alvo {
            Alvo::File { doc_id, .. } => self.salvar_buffer(doc_id, window, cx, ao_fim),
            Alvo::Draft { doc_id, .. } => self.salvar_rascunho(doc_id, window, cx, ao_fim),
            Alvo::Conflict { path } => {
                // A gravação da resolução é do crate de conflito (via conteúdo); aqui só o texto do rascunho.
                let ok = !self.conflitos.get(&chave_conflito(&path)).is_some_and(|c| c.sujo());
                ao_fim(self, ok, window, cx)
            }
            _ => ao_fim(self, true, window, cx),
        }
    }

    fn salvar_buffer(
        &mut self, doc_id: String, window: &mut Window, cx: &mut Context<Self>,
        ao_fim: impl FnOnce(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some(d) = self.docs.get_mut(&doc_id) else { return ao_fim(self, true, window, cx) };
        d.sincronizar(cx);
        if !d.sujo() {
            return ao_fim(self, true, window, cx);
        }
        if self.ws.abas().any(|t| matches!(&t.alvo, Alvo::Draft { doc_id: x, .. } if *x == doc_id)) {
            return self.salvar_rascunho(doc_id, window, cx, ao_fim);
        }
        let d = &self.docs[&doc_id];
        if d.buffer.read_only || self.salvando.contains(&doc_id) {
            return ao_fim(self, false, window, cx);
        }
        self.salvando.insert(doc_id.clone());
        let enviado = para_disco(&d.buffer.draft, d.buffer.newline);
        let (raiz, rel, base, bom) = (PathBuf::from(&self.projeto), d.buffer.path.clone(), d.buffer.raw.clone(), d.buffer.bom);
        let env = enviado.clone();
        let tarefa = cx.background_spawn(async move {
            let r = gravar_verificado(&raiz, &rel, &env, &base, bom);
            let releitura = r.as_ref().err().map(|_| ler_conteudo(&raiz, &rel));
            (r, releitura)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (r, releitura) = tarefa.await;
            let _ = this.update_in(cx, |a, window, cx| {
                a.salvando.remove(&doc_id);
                let ok = match r {
                    Ok(()) => {
                        if let Some(d) = a.docs.get_mut(&doc_id) {
                            d.buffer.raw = enviado;
                            d.sincronizar(cx);
                        }
                        a.git_carregar(&doc_id, cx);
                        a.disco.remove(&doc_id);
                        a.situacao.remove(&doc_id);
                        cx.emit(EventoEditor::AtualizarGit);
                        cx.emit(EventoEditor::Salvo);
                        true
                    }
                    Err(motivo) => {
                        match (releitura, a.docs.get(&doc_id)) {
                            (Some(Ok(f)), Some(d)) if normalizar_quebras(&f.text) != normalizar_quebras(&d.buffer.raw) || f.bom != d.buffer.bom => {
                                a.disco.insert(doc_id.clone(), VersaoDisco { texto: f.text, bom: f.bom, somente_leitura: f.read_only });
                                a.situacao.remove(&doc_id);
                            }
                            (Some(Err(e)), _) => {
                                a.situacao.insert(doc_id.clone(), Situacao { erro: Some(format!("{motivo} {e}")), ..Default::default() });
                            }
                            _ => {
                                a.situacao.insert(doc_id.clone(), Situacao { erro: Some(motivo), ..Default::default() });
                            }
                        }
                        false
                    }
                };
                a.agendar_recuperacao(cx);
                cx.notify();
                ao_fim(a, ok, window, cx);
            });
        })
        .detach();
    }

    /// Diálogo "Salvar como": começa na pasta do projeto (ou do arquivo solto).
    fn pasta_dialogo(pasta: Option<String>) -> PathBuf {
        pasta.map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    fn salvar_rascunho(
        &mut self, doc_id: String, window: &mut Window, cx: &mut Context<Self>,
        ao_fim: impl FnOnce(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some((aba_id, num)) = self.ws.abas().find_map(|t| match &t.alvo {
            Alvo::Draft { doc_id: x, num } if *x == doc_id => Some((t.id.clone(), *num)),
            _ => None,
        }) else {
            return ao_fim(self, false, window, cx);
        };
        if self.salvando.contains(&doc_id) {
            return ao_fim(self, false, window, cx);
        }
        self.salvando.insert(doc_id.clone());
        let pasta = (!self.projeto.is_empty()).then(|| self.projeto.clone());
        let escolha = cx.prompt_for_new_path(&Self::pasta_dialogo(pasta), Some(&m::nome_rascunho(num)));
        cx.spawn_in(window, async move |this, cx| {
            let escolhido = escolha.await.ok().and_then(Result::ok).flatten();
            let _ = this.update_in(cx, |a, window, cx| {
                a.salvando.remove(&doc_id);
                let Some(p) = escolhido else { return ao_fim(a, false, window, cx) };
                match a.gravar_em_novo_caminho(&doc_id, &p, Some(&aba_id), window, cx) {
                    Ok(()) => ao_fim(a, true, window, cx),
                    Err(e) => {
                        avisos::mostrar(Aviso::erro(e, 7000), cx);
                        ao_fim(a, false, window, cx)
                    }
                }
            });
        })
        .detach();
    }

    fn salvar_como(&mut self, doc_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.docs.get(&doc_id) else { return };
        let caminho = d.buffer.path.clone();
        let pasta = if !self.projeto.is_empty() {
            Some(self.projeto.clone())
        } else if eh_absoluto(&caminho) {
            Some(caminhos::separar(&caminho.replace('\\', "/")).1.to_string())
        } else {
            None
        };
        let escolha = cx.prompt_for_new_path(&Self::pasta_dialogo(pasta), Some(nome_arquivo(&caminho)));
        cx.spawn_in(window, async move |this, cx| {
            let Some(p) = escolha.await.ok().and_then(Result::ok).flatten() else { return };
            let _ = this.update_in(cx, |a, window, cx| {
                let abs = p.to_string_lossy().replace('\\', "/");
                let novo = relativo_a(&a.projeto, &abs).unwrap_or(abs);
                if novo.to_lowercase() == caminho.to_lowercase() {
                    a.salvar_buffer(doc_id.clone(), window, cx, |_, _, _, _| {});
                    return;
                }
                if let Err(e) = a.gravar_em_novo_caminho(&doc_id, &p, None, window, cx) {
                    avisos::mostrar(Aviso::erro(e, 7000), cx);
                }
            });
        })
        .detach();
    }

    /// `writeNewFile` + troca de caminho do documento (rascunho promovido ou salvar como).
    fn gravar_em_novo_caminho(
        &mut self, doc_id: &str, escolhido: &Path, rascunho: Option<&str>, window: &mut Window, cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let abs = escolhido.to_string_lossy().replace('\\', "/");
        let caminho = relativo_a(&self.projeto, &abs).unwrap_or(abs);
        if !eh_absoluto(&caminho) && !m::caminho_seguro(&caminho) {
            return Err("Caminho inválido.".into());
        }
        let existente = self.ws.abas().find_map(|t| match &t.alvo {
            Alvo::File { doc_id: x, path } if path.to_lowercase() == caminho.to_lowercase() && x != doc_id => Some((t.id.clone(), x.clone())),
            _ => None,
        });
        if let Some((_, x)) = &existente {
            if self.docs.get(x).is_some_and(|d| d.sujo()) {
                return Err(format!("Salve ou descarte a edição aberta de {} antes de salvar por cima dele.", nome_arquivo(&caminho)));
            }
        }
        let d = self.docs.get_mut(doc_id).ok_or("Documento fechado.")?;
        d.sincronizar(cx);
        let enviado = para_disco(&d.buffer.draft, d.buffer.newline);
        let raiz = PathBuf::from(&self.projeto);
        gravar_novo(&raiz, &caminho, &enviado)?;
        let salvo = ler_conteudo(&raiz, &caminho)?;
        if let Some((aba, x)) = existente {
            let fonte: Vec<String> = self.ws.abas().filter(|t| t.alvo.doc_id() == Some(doc_id)).map(|t| t.id.clone()).collect();
            let ws = m::ativar(&m::fechar_abas(&self.ws, &fonte), &aba);
            self.definir_ws(ws, cx);
            self.recarregar_doc(&x, window, cx);
        } else {
            let d = self.docs.get_mut(doc_id).ok_or("Documento fechado.")?;
            d.buffer.raw = salvo.text.clone();
            d.buffer.bom = salvo.bom;
            d.buffer.newline = quebra_de(&salvo.text);
            d.definir_caminho(&caminho, cx);
            d.sincronizar(cx);
            let ws = match rascunho {
                Some(aba) => m::promover_rascunho(&self.ws, aba, &caminho),
                None => m::redirecionar_doc(&self.ws, doc_id, &caminho),
            };
            self.disco.remove(doc_id);
            self.situacao.remove(doc_id);
            self.definir_ws(ws, cx);
            self.git_carregar(doc_id, cx);
        }
        cx.emit(EventoEditor::AtualizarGit);
        cx.emit(EventoEditor::Salvo);
        cx.notify();
        Ok(())
    }

    // ---- Disco ----

    fn recarregar_doc(&mut self, doc_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(caminho) = self.ws.abas().find_map(|t| match &t.alvo {
            Alvo::File { doc_id: x, path } if x == doc_id => Some(path.clone()),
            _ => None,
        }) else {
            return;
        };
        if self.salvando.contains(doc_id) || self.eh_conteudo(doc_id) {
            return;
        }
        let raw_antes = self.docs.get(doc_id).map(|d| d.buffer.raw.clone());
        let raiz = PathBuf::from(&self.projeto);
        let rel = caminho.clone();
        let leitura = cx.background_spawn(async move { ler_conteudo(&raiz, &rel) });
        let doc_id = doc_id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            let r = leitura.await;
            let _ = this.update_in(cx, |a, window, cx| a.aplicar_leitura(&doc_id, &caminho, raw_antes, r, window, cx));
        })
        .detach();
    }

    fn aplicar_leitura(
        &mut self, doc_id: &str, caminho: &str, raw_antes: Option<String>, r: Result<files::Content, String>, window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ws.abas().any(|t| t.alvo.doc_id() == Some(doc_id)) {
            return;
        }
        // Gravação durante a leitura: a resposta pode ser de antes dela.
        if self.salvando.contains(doc_id) || self.docs.get(doc_id).map(|d| d.buffer.raw.clone()) != raw_antes {
            return;
        }
        let mudou = |a: &mut Self, s: Situacao| {
            if a.situacao.get(doc_id) != Some(&s) {
                a.situacao.insert(doc_id.to_string(), s);
                true
            } else {
                false
            }
        };
        let f = match r {
            Err(e) => {
                if mudou(self, Situacao { erro: Some(e), ..Default::default() }) {
                    cx.notify();
                }
                return;
            }
            Ok(f) if f.binary || f.too_large => {
                if mudou(self, Situacao { binario: f.binary, grande: !f.binary, ..Default::default() }) {
                    cx.notify();
                }
                return;
            }
            Ok(f) => f,
        };
        if self.situacao.remove(doc_id).is_some() {
            cx.notify();
        }
        let Some(d) = self.docs.get_mut(doc_id) else {
            self.instalar_documento(Buffer::do_disco(doc_id, caminho, f.text, f.bom, f.read_only), window, cx);
            cx.notify();
            return;
        };
        if normalizar_quebras(&d.buffer.raw) == normalizar_quebras(&f.text) && d.buffer.bom == f.bom {
            if self.disco.remove(doc_id).is_some() {
                cx.notify();
            }
            return;
        }
        if !d.sujo() {
            d.buffer.raw = f.text.clone();
            d.buffer.bom = f.bom;
            d.buffer.newline = quebra_de(&f.text);
            d.definir_somente_leitura(f.read_only, cx);
            let limpo = normalizar_quebras(&f.text);
            d.trocar_texto(&limpo, window, cx);
            self.git_carregar(doc_id, cx);
            cx.notify();
            return;
        }
        let v = VersaoDisco { texto: f.text, bom: f.bom, somente_leitura: f.read_only };
        if self.disco.get(doc_id) != Some(&v) {
            self.disco.insert(doc_id.to_string(), v);
            cx.notify();
        }
    }

    /// Consulta periódica das abas de arquivo ativas (detecção de arquivo alterado no disco).
    fn consultar_disco(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ativo {
            return;
        }
        let ativos: Vec<String> = self
            .ws
            .groups
            .iter()
            .filter_map(|g| g.tabs.iter().find(|t| Some(&t.id) == g.active.as_ref()))
            .filter_map(|t| match &t.alvo {
                Alvo::File { doc_id, .. } => Some(doc_id.clone()),
                _ => None,
            })
            .collect();
        for d in ativos {
            self.recarregar_doc(&d, window, cx);
            self.git_carregar(&d, cx);
        }
    }

    /// Relê do disco os arquivos abertos que estão limpos.
    pub fn atualizar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.ws.abas().filter(|&t| matches!(t.alvo, Alvo::File { .. })).map(|t| t.alvo.doc_id().unwrap().to_string()).collect();
        for d in ids {
            self.recarregar_doc(&d, window, cx);
        }
    }

    fn abrir_comparacao(&mut self, doc_id: &str, modo: ModoComparacao, cx: &mut Context<Self>) {
        let (Some(d), Some(v)) = (self.docs.get(doc_id), self.disco.get(doc_id)) else { return };
        if modo == ModoComparacao::Manter && v.somente_leitura {
            avisos::mostrar(Aviso::erro("Arquivo somente leitura: a edição não pode ser gravada aqui.", 5000), cx);
            return;
        }
        self.comparacao = Some(Comparacao {
            doc_id: doc_id.into(),
            caminho: d.buffer.path.clone(),
            modo,
            disco: v.clone(),
            rascunho: d.buffer.draft.clone(),
        });
        cx.notify();
    }

    fn confirmar_comparacao(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.comparacao.take() else { return };
        cx.notify();
        let Some(d) = self.docs.get_mut(&c.doc_id) else { return };
        if c.modo == ModoComparacao::Usar {
            d.buffer.raw = c.disco.texto.clone();
            d.buffer.bom = c.disco.bom;
            d.buffer.newline = quebra_de(&c.disco.texto);
            d.definir_somente_leitura(c.disco.somente_leitura, cx);
            d.trocar_texto(&normalizar_quebras(&c.disco.texto), window, cx);
            self.disco.remove(&c.doc_id);
            self.git_carregar(&c.doc_id, cx);
            return;
        }
        if self.salvando.contains(&c.doc_id) {
            return;
        }
        let enviado = para_disco(&c.rascunho, d.buffer.newline);
        let r = gravar_verificado(&PathBuf::from(&self.projeto), &d.buffer.path, &enviado, &c.disco.texto, c.disco.bom);
        match r {
            Ok(()) => {
                d.buffer.raw = enviado;
                d.buffer.bom = c.disco.bom;
                d.sincronizar(cx);
                self.disco.remove(&c.doc_id);
                self.situacao.remove(&c.doc_id);
                self.git_carregar(&c.doc_id, cx);
                cx.emit(EventoEditor::AtualizarGit);
                cx.emit(EventoEditor::Salvo);
            }
            Err(e) => {
                avisos::mostrar(Aviso::erro(e, 7000), cx);
                self.recarregar_doc(&c.doc_id, window, cx);
            }
        }
    }

    // ---- Formatação ----

    fn formatar_documento(&mut self, doc_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.docs.get(doc_id) else { return };
        if d.buffer.path.is_empty() || d.buffer.read_only {
            let t = if d.buffer.path.is_empty() { "Salve o arquivo para formatar." } else { "O arquivo é somente leitura." };
            avisos::mostrar(Aviso::info(t, 3000), cx);
            return;
        }
        let abs = absoluto_no_projeto(&self.projeto, &d.buffer.path);
        let fonte = d.texto(cx);
        let cursor = d.editor.read(cx).cursor();
        let tarefa = cx.background_spawn({
            let fonte = fonte.clone();
            async move { formatar::formatar(Path::new(&abs), &fonte) }
        });
        let doc_id = doc_id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update_in(cx, |a, window, cx| match r {
                Err(formatar::Falha::Indisponivel(m)) => avisos::mostrar(Aviso::info(m, 3000), cx),
                Err(formatar::Falha::Erro(m)) => avisos::mostrar(Aviso::erro(m, 7000), cx),
                Ok(f) => {
                    let Some(d) = a.docs.get_mut(&doc_id) else { return };
                    let formatado = normalizar_quebras(&f.texto);
                    if d.texto(cx) != fonte || formatado == fonte {
                        return;
                    }
                    let novo_cursor = documento::mapear(&fonte, &formatado, cursor);
                    d.editor.update(cx, |s, cx| {
                        s.select_all(window, cx);
                        s.replace(formatado.clone(), window, cx);
                        s.set_selected_range(novo_cursor..novo_cursor, cx);
                    });
                    if let Some(q) = f.quebra {
                        d.buffer.newline = q;
                    }
                    d.sincronizar(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    // ---- Operações de aba ----

    pub fn ativar(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let ws = m::ativar(&self.ws, id);
        self.definir_ws(ws, cx);
        self.focar_ativo(window, cx);
    }

    fn focar_ativo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.ws.aba_focada().and_then(|t| t.alvo.doc_id()).and_then(|d| self.docs.get(d)) {
            d.editor.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    fn copiar_caminho(&mut self, tipo: TipoCopia, alvo: &Alvo, cx: &mut Context<Self>) {
        let Some(c) = alvo.caminho() else { return };
        let cursor = alvo.doc_id().and_then(|d| self.cursores.get(d)).copied();
        if let Some(t) = caminhos::texto_caminho(tipo, c, &self.projeto, cursor) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(t));
            avisos::mostrar(Aviso::sucesso("Caminho copiado", 2000), cx);
        }
    }

    pub fn reabrir_fechada(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let docs: Vec<(String, String)> = self.docs.values().map(|d| (d.buffer.path.to_lowercase(), d.buffer.doc_id.clone())).collect();
        let ws = m::reabrir_fechada(&self.ws, |t| match t {
            Alvo::File { path, .. } => {
                let doc = docs.iter().find(|(p, _)| *p == path.to_lowercase()).map(|(_, d)| d.clone()).unwrap_or_else(novo_doc_id);
                Alvo::arquivo(doc, path.clone())
            }
            t => t.clone(),
        });
        self.definir_ws(ws, cx);
        self.ler_faltantes(window, cx);
        self.criar_conteudos(window, cx);
    }

    /// Lê os arquivos com aba e sem documento.
    fn ler_faltantes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let faltam: Vec<(String, String)> = self
            .ws
            .abas()
            .filter_map(|t| match &t.alvo {
                Alvo::File { doc_id, path } if !self.docs.contains_key(doc_id) && !self.situacao.contains_key(doc_id) && !self.conteudos.contains_key(&t.id) => {
                    Some((doc_id.clone(), path.clone()))
                }
                _ => None,
            })
            .collect();
        let mut vistos = HashSet::new();
        for (d, p) in faltam {
            if vistos.insert(d.clone()) {
                self.ler_arquivo(&d, &p, None, window, cx);
            }
        }
    }

    pub fn dividir(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let ws = m::dividir_aba(&self.ws, id);
        self.definir_ws(ws, cx);
        self.criar_conteudos(window, cx);
    }

    fn mover_para_outro(&mut self, id: &str, cx: &mut Context<Self>) {
        let para = self.ws.localizar(id).map(|(g, _)| g.id.outro()).unwrap_or(Grupo::B);
        let base = self.com_grupo(para);
        self.definir_ws(m::mover_aba(&base, id, para, None), cx);
    }

    pub fn unir_grupos(&mut self, cx: &mut Context<Self>) {
        let ws = m::unir_grupos(&self.ws);
        self.definir_ws(ws, cx);
    }

    // ---- Renomear e excluir (chamados pela bancada depois da operação de arquivo) ----

    /// Depois de renomear ou mover: troca o caminho de abas e documentos, sem remontar buffer.
    pub fn remapear(&mut self, de: &str, para: &str, cx: &mut Context<Self>) {
        let ws = m::remapear(&self.ws, de, para);
        for d in self.docs.values_mut() {
            if !d.buffer.path.is_empty() && dentro_de(&d.buffer.path, de) {
                let novo = format!("{para}{}", &d.buffer.path[de.len()..]);
                d.definir_caminho(&novo, cx);
            }
        }
        let dados = remapear_dados(&Dados { buffers: Default::default(), conflicts: self.conflitos.clone() }, de, para);
        self.conflitos = dados.conflicts;
        self.situacao.clear();
        self.definir_ws(ws, cx);
        let ids: Vec<String> = self.docs.keys().cloned().collect();
        for d in ids {
            self.git_carregar(&d, cx);
        }
    }

    /// Depois de excluir: fecha as abas e descarta os buffers (quem chama já decidiu).
    pub fn remover(&mut self, caminhos: &[String], cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .ws
            .abas()
            .filter(|t| !matches!(t.alvo, Alvo::Commit { .. } | Alvo::Draft { .. }))
            .filter(|t| t.alvo.caminho().is_some_and(|p| caminhos.iter().any(|b| dentro_de(p, b))))
            .map(|t| t.id.clone())
            .collect();
        let ws = m::fechar_abas(&self.ws, &ids);
        self.definir_ws(ws, cx);
    }

    /// Há rascunho neste caminho (ou dentro dele)? Bloqueia descarte no Git.
    pub fn ha_rascunho(&self, caminho: &str, cx: &App) -> bool {
        ha_rascunho_em(&self.dados(cx), caminho)
    }

    /// Caminhos (e nomes de rascunho) com alteração não salva, para perguntar antes de fechar a janela.
    pub fn nao_salvos(&self) -> Vec<String> {
        let mut out: Vec<String> = self.docs.values().filter(|d| d.sujo() && !d.buffer.path.is_empty()).map(|d| d.buffer.path.clone()).collect();
        for t in self.ws.abas() {
            if let Alvo::Draft { doc_id, num } = &t.alvo {
                if self.docs.get(doc_id).is_some_and(|d| d.sujo()) {
                    out.push(m::nome_rascunho(*num));
                }
            }
        }
        out.extend(self.conflitos.values().filter(|c| c.sujo()).map(|c| c.path.clone()));
        out
    }

    /// Descarta todos os rascunhos (volta ao texto do disco).
    pub fn descartar_tudo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for d in self.docs.values_mut() {
            if d.sujo() {
                let t = normalizar_quebras(&d.buffer.raw);
                d.trocar_texto(&t, window, cx);
            }
        }
        let mut descartados = Vec::new();
        for c in self.conflitos.values_mut() {
            if c.sujo() {
                descartados.push(c.path.clone());
            }
            c.draft = c.origin.clone();
        }
        if let Some(f) = self.ao_descartar_conflito.clone() {
            for p in descartados {
                f(&p, window, cx);
            }
        }
        self.agendar_recuperacao(cx);
        cx.notify();
    }

    /// Caminhos de todas as abas, para checar colisão antes de criar ou mover.
    pub fn caminhos_abertos(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for t in self.ws.abas() {
            if let Some(p) = t.alvo.caminho() {
                if !v.contains(&p.to_string()) {
                    v.push(p.to_string());
                }
            }
        }
        v
    }

    /// Seleção principal (ou a palavra sob o cursor) no arquivo ativo; vazio se tem quebra de linha ou não há.
    pub fn texto_selecionado(&self, cx: &App) -> String {
        let Some(d) = self.ws.aba_focada().and_then(|t| t.alvo.doc_id()).and_then(|d| self.docs.get(d)) else { return String::new() };
        let s = d.editor.read(cx);
        let t = s.text().to_string();
        let r = s.selected_range();
        let r = if r.is_empty() {
            let eh = |c: char| c.is_alphanumeric() || c == '_';
            let ini = t[..r.start].char_indices().rev().take_while(|(_, c)| eh(*c)).last().map_or(r.start, |(i, _)| i);
            let fim = t[r.start..].char_indices().find(|(_, c)| !eh(*c)).map_or(t.len(), |(i, _)| r.start + i);
            ini..fim
        } else {
            r
        };
        let sel = t.get(r).unwrap_or("");
        if sel.contains('\n') { String::new() } else { sel.to_string() }
    }

    /// Abertura rápida (Ctrl+P).
    pub fn abrir_rapido(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raiz = PathBuf::from(&self.projeto);
        let v = cx.new(|cx| abrir_rapido::AbrirRapido::new(raiz, window, cx));
        self._sub_rapido = Some(cx.subscribe_in(&v, window, |a, _, ev: &abrir_rapido::Evento, window, cx| {
            match ev {
                abrir_rapido::Evento::Fechar => {}
                abrir_rapido::Evento::Abrir { caminho, permanente, lado } => {
                    let op = Abertura { permanente: *permanente, ..Default::default() };
                    if *lado {
                        a.abrir_ao_lado(caminho, op, window, cx);
                    } else {
                        a.abrir_arquivo(caminho, op, window, cx);
                    }
                }
            }
            a.abrir_rapido = None;
            a._sub_rapido = None;
            cx.notify();
        }));
        let campo = v.read(cx).campo();
        campo.update(cx, |s, cx| s.focus(window, cx));
        self.abrir_rapido = Some(v);
        cx.notify();
    }

    // ---- Pergunta (modal) ----

    fn perguntar(
        &mut self, titulo: &str, corpo: String, acoes: Vec<(&'static str, &'static str, bool)>,
        resposta: impl FnOnce(&mut AreaEditor, Option<&'static str>, &mut Window, &mut Context<AreaEditor>) + 'static, cx: &mut Context<Self>,
    ) {
        if self.pergunta.is_some() {
            return;
        }
        self.pergunta = Some(Pergunta { titulo: titulo.to_string().into(), corpo: corpo.into(), acoes, resposta: Some(Box::new(resposta)) });
        cx.notify();
    }

    fn responder(&mut self, chave: Option<&'static str>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut p) = self.pergunta.take() {
            if let Some(r) = p.resposta.take() {
                r(self, chave, window, cx);
            }
        }
        cx.notify();
    }

    // ---- Recuperação ----

    fn dados(&self, _cx: &App) -> Dados {
        Dados {
            buffers: self.docs.iter().map(|(k, d)| (k.clone(), d.buffer.clone())).collect(),
            conflicts: self.conflitos.clone(),
        }
    }

    fn carregar_projeto(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        self.projeto = projeto;
        self.docs.clear();
        self._subs.clear();
        self.situacao.clear();
        self.disco.clear();
        self.conteudos.clear();
        self.conflitos.clear();
        self.cursores.clear();
        self.rolagens.clear();
        let mut notas = Vec::new();
        let lido = {
            let mut a = self.armazenamento.borrow_mut();
            ler_recuperacao(&mut *a, &self.projeto)
        };
        let Some(texto) = lido else {
            self.ws = Workspace::default();
            self.proporcao = PROPORCAO_PADRAO;
            self.aviso = None;
            self.avisar_mudancas(cx);
            cx.notify();
            return;
        };
        let projeto_rec = if self.projeto.is_empty() { armazem::SEM_PROJETO.to_string() } else { self.projeto.clone() };
        let (rec, n) = decodificar_recuperacao(&texto, &projeto_rec);
        notas.extend(n);
        let carregado = rec.workspace.as_deref().map(|w| m::carregar(w, true));
        let ws = carregado.as_ref().map(|c| c.estado.clone()).unwrap_or_default();
        if let Some(c) = &carregado {
            notas.extend(c.notas.clone());
        }
        let rascunhos: HashSet<String> = ws.abas().filter(|&t| matches!(t.alvo, Alvo::Draft { .. })).map(|t| t.alvo.doc_id().unwrap().to_string()).collect();
        self.ws = ws;
        for d in &rascunhos {
            self.instalar_documento(Buffer::vazio(d), window, cx);
        }
        for b in rec.buffers {
            if let Some(c) = b.cursor {
                self.cursores.insert(b.doc_id.clone(), c);
            }
            if let Some(r) = b.scroll {
                self.rolagens.insert(b.doc_id.clone(), r);
            }
            if let Some(s) = b.dirty {
                if !b.path.is_empty() || rascunhos.contains(&b.doc_id) {
                    let buf = Buffer { doc_id: b.doc_id.clone(), path: b.path.clone(), raw: s.raw, bom: s.bom, newline: s.newline, draft: s.draft, read_only: s.read_only };
                    self.docs.remove(&b.doc_id);
                    self.instalar_documento(buf, window, cx);
                }
            }
        }
        for c in rec.conflicts {
            self.conflitos.insert(chave_conflito(&c.path), c);
        }
        self.proporcao = rec.split.map(|s| s as f32).unwrap_or(PROPORCAO_PADRAO);
        if let Some(l) = rec.layout {
            self.layout = Some(l.clone());
            cx.emit(EventoEditor::RestaurarLayout(l));
        }
        self.aviso = (!notas.is_empty()).then(|| notas.join(" "));
        self.intocado = true;
        self.ler_faltantes(window, cx);
        self.criar_conteudos(window, cx);
        self.avisar_mudancas(cx);
        cx.notify();
    }

    fn agendar_recuperacao(&mut self, cx: &mut Context<Self>) {
        self.tarefa_recuperacao = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ESPERA_RECUPERACAO).await;
            let _ = this.update(cx, |a, cx| a.gravar_recuperacao(cx));
        }));
    }

    /// Grava a recuperação agora (antes de fechar o app).
    pub fn gravar_recuperacao(&mut self, cx: &mut Context<Self>) {
        if self.intocado {
            return;
        }
        let mut buffers: Vec<BufferRecuperado> = Vec::new();
        let mut vistos = HashSet::new();
        for t in self.ws.abas() {
            let (doc_id, caminho, rascunho) = match &t.alvo {
                Alvo::File { doc_id, path } => (doc_id, path.as_str(), false),
                Alvo::Draft { doc_id, .. } => (doc_id, "", true),
                _ => continue,
            };
            if !vistos.insert(doc_id.clone()) {
                continue;
            }
            let d = self.docs.get(doc_id);
            if let Some(d) = d {
                let (l, c) = d.cursor(cx);
                self.cursores.insert(doc_id.clone(), Cursor { line: l, col: c });
                let o = d.editor.read(cx).scroll_offset();
                self.rolagens.insert(doc_id.clone(), Rolagem { top: -f64::from(f32::from(o.y)), left: -f64::from(f32::from(o.x)) });
            }
            buffers.push(BufferRecuperado {
                doc_id: doc_id.clone(),
                path: caminho.to_string(),
                cursor: self.cursores.get(doc_id).copied(),
                scroll: self.rolagens.get(doc_id).copied(),
                dirty: d.filter(|d| rascunho || d.sujo()).map(|d| Sujeira {
                    raw: d.buffer.raw.clone(),
                    bom: d.buffer.bom,
                    newline: d.buffer.newline,
                    draft: d.buffer.draft.clone(),
                    read_only: d.buffer.read_only,
                }),
            });
        }
        let conflitos: Vec<RascunhoConflito> = self.conflitos.values().filter(|c| c.sujo()).cloned().collect();
        let tem = self.ws.abas().next().is_some() || !conflitos.is_empty() || self.layout.is_some();
        let chave = armazem::chave_recuperacao(&self.projeto);
        let mut a = self.armazenamento.borrow_mut();
        if !tem {
            a.apagar(&chave);
            return;
        }
        let rec = Recuperacao {
            workspace: serde_json::to_string(&self.ws).ok(),
            layout: self.layout.clone(),
            split: Some(f64::from(self.proporcao)),
            buffers,
            conflicts: conflitos,
        };
        let projeto_rec = if self.projeto.is_empty() { armazem::SEM_PROJETO.to_string() } else { self.projeto.clone() };
        let (texto, notas) = codificar_recuperacao(&projeto_rec, &rec);
        if let Err(e) = a.gravar(&chave, &texto) {
            a.apagar(&chave);
            self.aviso = Some(format!("Não foi possível guardar a recuperação ({e}). A edição continua, mas não será recuperada."));
        } else if !notas.is_empty() {
            self.aviso = Some(notas.join(" "));
        }
        drop(a);
        cx.notify();
    }

    // ---- Teclado ----

    fn tecla(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let (ctrl, shift, alt) = (k.modifiers.control, k.modifiers.shift, k.modifiers.alt);
        let tecla = k.key.as_str();
        let focada = self.aba_focada();
        if let Some(t) = self.acorde.take() {
            if t.elapsed() < Duration::from_millis(1500) {
                if ctrl && !shift && !alt && tecla == "k" {
                    self.acorde = Some(std::time::Instant::now());
                } else if !ctrl && !alt && !shift && tecla == "w" {
                    let g = self.ws.focus;
                    self.fechar(Escopo::Todas(g), window, cx);
                } else if ctrl && shift && !alt && tecla == "c" {
                    if let Some(f) = &focada {
                        self.copiar_caminho(TipoCopia::Relativo, &f.alvo.clone(), cx);
                    }
                } else if !ctrl && !alt && shift && tecla == "enter" {
                    if let Some(f) = &focada {
                        let ws = m::alternar_fixa(&self.ws, &f.id);
                        self.definir_ws(ws, cx);
                    }
                } else {
                    return;
                }
                cx.stop_propagation();
                return;
            }
        }
        let acao: Option<AcaoUnica> = if ctrl && !shift && !alt && tecla == "k" {
            Some(Box::new(|a, _, _| a.acorde = Some(std::time::Instant::now())))
        } else if ctrl && !shift && !alt && tecla == "n" {
            Some(Box::new(|a, w, cx| a.novo_arquivo(w, cx)))
        } else if ctrl && !shift && !alt && (tecla == "w" || tecla == "f4") && focada.is_some() {
            let id = focada.clone().unwrap().id;
            Some(Box::new(move |a, w, cx| a.fechar(Escopo::Uma(id), w, cx)))
        } else if alt && shift && !ctrl && tecla == "c" && focada.is_some() {
            let alvo = focada.clone().unwrap().alvo;
            Some(Box::new(move |a, _, cx| a.copiar_caminho(TipoCopia::Absoluto, &alvo, cx)))
        } else if ctrl && tecla == "tab" {
            Some(Box::new(move |a, w, cx| {
                let ws = m::navegar(&a.ws, if shift { -1 } else { 1 });
                a.definir_ws(ws, cx);
                a.focar_ativo(w, cx);
            }))
        } else if ctrl && shift && tecla == "t" {
            Some(Box::new(|a, w, cx| a.reabrir_fechada(w, cx)))
        } else if ctrl && tecla == "\\" && focada.is_some() {
            let id = focada.clone().unwrap().id;
            Some(Box::new(move |a, w, cx| a.dividir(&id, w, cx)))
        } else if ctrl && !shift && !alt && tecla == "p" {
            Some(Box::new(|a, w, cx| a.abrir_rapido(w, cx)))
        } else {
            None
        };
        if let Some(f) = acao {
            cx.stop_propagation();
            f(self, window, cx);
        }
    }

    fn comando(&mut self, cmd: Comando, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.ws.aba_focada().and_then(|t| t.alvo.doc_id()).and_then(|d| self.docs.get(d)) else { return };
        let caminho = d.buffer.path.clone();
        d.editor.update(cx, |s, cx| documento::executar(s, cmd, &caminho, window, cx));
    }

    fn abrir_ir_para(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc_id) = self.ws.aba_focada().and_then(|t| t.alvo.doc_id()).map(str::to_string) else { return };
        let campo = cx.new(|cx| InputState::new(window, cx).placeholder("ex.: 42:3"));
        let sub = cx.subscribe_in(&campo, window, |a, campo, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { .. } => a.confirmar_ir_para(window, cx),
            InputEvent::Change => {
                let _ = campo;
                if let Some(i) = a.ir_para.as_mut() {
                    i.erro = None;
                }
                cx.notify();
            }
            _ => {}
        });
        campo.update(cx, |s, cx| s.focus(window, cx));
        self.ir_para = Some(IrPara { campo, erro: None, doc_id, _sub: sub });
        cx.notify();
    }

    fn confirmar_ir_para(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = &self.ir_para else { return };
        let Some(d) = self.docs.get(&i.doc_id) else { return };
        let valor = i.campo.read(cx).value().to_string();
        let t = d.editor.read(cx).text().clone();
        let linhas: Vec<String> = t.to_string().split('\n').map(str::to_string).collect();
        match documento::ler_ir_para(&valor, linhas.len(), |l| linhas[l - 1].encode_utf16().count()) {
            Ok((l, c)) => {
                d.revelar(l, c, window, cx);
                self.ir_para = None;
            }
            Err(e) => {
                if let Some(i) = self.ir_para.as_mut() {
                    i.erro = Some(e);
                }
            }
        }
        cx.notify();
    }

    fn fechar_ir_para(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ir_para = None;
        self.focar_ativo(window, cx);
        cx.notify();
    }
}

fn info_de(d: &Documento) -> InfoDocumento {
    InfoDocumento {
        doc_id: d.buffer.doc_id.clone(),
        caminho: d.buffer.path.clone(),
        gutter: d.gutter.clone(),
        regua: d.regua.clone(),
        trechos: d.trechos.clone(),
    }
}

/// Leitura do arquivo relativo ao projeto ou absoluto.
pub fn ler_conteudo(raiz: &Path, caminho: &str) -> Result<files::Content, String> {
    if eh_absoluto(caminho) {
        files::read_at(Path::new(caminho))
    } else {
        files::read(raiz, caminho)
    }
}

fn gravar_verificado(raiz: &Path, caminho: &str, texto: &str, base: &str, bom: bool) -> Result<(), String> {
    if eh_absoluto(caminho) {
        files::write_checked_at(Path::new(caminho), texto, base, bom)
    } else {
        files::write_checked(raiz, caminho, texto, base, bom)
    }
}

/// `writeNewFile`: cria o arquivo (ou, se já existe e o diálogo confirmou a sobrescrita, grava sobre a versão atual).
fn gravar_novo(raiz: &Path, caminho: &str, texto: &str) -> Result<(), String> {
    let nao_texto = |c: &files::Content| (c.binary || c.too_large).then(|| format!("{} não é texto: não dá para salvar por cima dele.", nome_arquivo(caminho)));
    if eh_absoluto(caminho) {
        let (base, bom) = match files::read_at(Path::new(caminho)) {
            Ok(c) => {
                if let Some(e) = nao_texto(&c) {
                    return Err(e);
                }
                (c.text, c.bom)
            }
            Err(_) => (String::new(), false),
        };
        return files::write_checked_at(Path::new(caminho), texto, &base, bom);
    }
    let (nome, pasta) = caminhos::separar(caminho);
    let (base, bom) = match files::create_file(raiz, pasta, nome) {
        Ok(_) => (String::new(), false),
        Err(e) if e.starts_with("Já existe um item") => {
            let c = files::read(raiz, caminho)?;
            if let Some(e) = nao_texto(&c) {
                return Err(e);
            }
            (c.text, c.bom)
        }
        Err(e) => return Err(e),
    };
    files::write_checked(raiz, caminho, texto, &base, bom)
}

// ---- Desenho ----

const ALTURA_FAIXA: f32 = 30.;

fn fundo_faixa() -> gpui::Hsla {
    cor::pitch_black()
}
fn fundo_container() -> gpui::Hsla {
    cor::panel_black()
}
fn cor_primaria() -> gpui::Hsla {
    // `--ant-color-primary` do Lobe escuro.
    gpui::rgb(0xffffff).into()
}

impl AreaEditor {
    fn faixa(&mut self, g: Grupo, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(grupo) = self.ws.grupo(g).cloned() else { return div().into_any_element() };
        let ent = cx.entity().downgrade();
        let focado = self.ws.focus == g;
        let sujos = self.sujos();
        let caminhos: Vec<String> = self.ws.abas().filter_map(|t| t.alvo.caminho().map(str::to_string)).collect();
        let det = detalhes(&caminhos.iter().map(String::as_str).collect::<Vec<_>>());
        let ativa = grupo.active.clone();
        // A faixa só rola até a ativa depois de medida: na primeira vez, revela de novo no quadro seguinte.
        let chave = ativa.clone().map(|a| (a, grupo.tabs.len()));
        if self.revelado[idx(g)] != chave {
            self.revelado[idx(g)] = chave;
            let estado = self.faixas[idx(g)].clone();
            let vista = cx.entity_id();
            window.on_next_frame(move |_, cx| {
                estado.revelar(None, None);
                cx.notify(vista);
            });
        }
        let mut abas = Vec::new();
        for (i, t) in grupo.tabs.iter().enumerate() {
            let e_ativa = Some(&t.id) == ativa.as_ref();
            let suja = m::esta_sujo(&t.alvo, &sujos);
            let detalhe = t.alvo.caminho().and_then(|p| det.get(p)).cloned().unwrap_or_default();
            let gid = SharedString::from(format!("aba-{}", t.id));
            let (id1, id2, ent1, ent2) = (t.id.clone(), t.id.clone(), ent.clone(), ent.clone());
            let seletor = gid.to_string();
            let el = div()
                .id(gid.clone())
                .debug_selector(move || seletor.clone())
                .group(gid.clone())
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .h_full()
                .max_w(px(224.))
                .gap(px(6.))
                .pl(px(10.))
                .pr(px(28.))
                .border_r_1()
                .border_color(cor::hairline())
                .text_size(tema::TEXTO_MENU)
                .text_color(if e_ativa { cor::text_primary() } else { cor::text_secondary() })
                .when(e_ativa, |d| d.bg(fundo_container()))
                .when(!e_ativa, |d| d.hover(|s| s.bg(fundo_container())))
                .tooltip_texto(rotulo(&t.alvo))
                .on_click(move |ev: &ClickEvent, window, cx| {
                    let _ = ev;
                    let _ = ent1.update(cx, |a, cx| a.ativar(&id1, window, cx));
                })
                .when(e_ativa, |d| {
                    d.child(div().absolute().left_0().right_0().top_0().h(px(1.)).bg(if focado { cor_primaria() } else { cor::text_tertiary() }))
                        .child(div().absolute().left_0().right_0().bottom(px(-1.)).h(px(1.)).bg(fundo_container()))
                })
                .child(if t.pinned { icone("icons/pin.svg", 16., cor::text_secondary()).into_any_element() } else { icone_alvo(&t.alvo, 16.) })
                .child(div().min_w_0().truncate().child(titulo(&t.alvo)))
                .when(!detalhe.is_empty(), |d| d.child(div().min_w_0().truncate().text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child(detalhe)))
                .child(fechar_ou_modificado(SharedString::from(format!("fechar-{}", t.id)), gid, e_ativa, suja, move |_, window, cx| {
                    let _ = ent2.update(cx, |a, cx| a.fechar(Escopo::Uma(id2.clone()), window, cx));
                }));
            let menu_ent = ent.clone();
            let menu_aba = t.clone();
            abas.push(
                Aba::new(t.id.clone(), el)
                    .fixada(t.pinned)
                    .previa(t.preview)
                    .menu_contexto(move |menu, window, cx| {
                        let Some(e) = menu_ent.upgrade() else { return menu };
                        e.update(cx, |a, cx| a.menu_aba(menu, &menu_aba, window, cx))
                    }),
            );
            let _ = i;
        }
        let ent_mover = ent.clone();
        let ent_iniciar = ent.clone();
        let fixadas: Vec<String> = grupo.tabs.iter().map(|t| t.id.clone()).collect();
        let ids_iniciar = fixadas.clone();
        let faixa = FaixaAbas::new(SharedString::from(format!("faixa-{g:?}")), &self.faixas[idx(g)])
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .items_stretch()
            .abas(abas)
            .ativa(ativa.clone())
            .ao_mover(move |mv: &MoverAba, _window, cx| {
                let id = fixadas[mv.de].clone();
                let _ = ent_mover.update(cx, |a, cx| {
                    // Solta sobre a faixa do outro grupo: quem move é o soltar da área.
                    if a.destino_arrasto().is_some() {
                        return;
                    }
                    let ws = m::reordenar_aba(&a.ws, &id, mv.para);
                    a.definir_ws(ws, cx);
                });
            })
            .ao_iniciar(move |i: &usize, window, cx| {
                let id = ids_iniciar[*i].clone();
                let _ = ent_iniciar.update(cx, |a, cx| {
                    a.arrasto_aba = Some((g, id.clone()));
                    a.ativar(&id, window, cx);
                });
            });
        let ent_dbl = ent.clone();
        let faixa = div()
            .id(SharedString::from(format!("faixa-vazia-{g:?}")))
            .debug_selector(move || format!("faixa-vazia-{g:?}"))
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .on_click(move |ev: &ClickEvent, window, cx| {
                // Duplo clique fora das abas: rascunho novo neste grupo.
                if ev.click_count() == 2 {
                    let _ = ent_dbl.update(cx, |a, cx| a.novo_rascunho(g, window, cx));
                }
            })
            .child(faixa);
        let ferramentas = self.ferramentas(g, &grupo, window, cx);
        let caixa = self.caixas_faixa[idx(g)].clone();
        let alvo_arrasto = self.destino_arrasto() == Some(g);
        div()
            .relative()
            .flex()
            .flex_none()
            .h(px(ALTURA_FAIXA))
            .bg(if alvo_arrasto { gpui::rgba(0xffffff14).into() } else { fundo_faixa() })
            .child(gpui::canvas(move |b, _, _| caixa.set(b), |_, _, _, _| {}).absolute().size_full())
            .child(div().absolute().left_0().right_0().bottom_0().h(px(1.)).bg(cor::hairline()))
            .child(faixa)
            .child(ferramentas)
            .into_any_element()
    }

    fn ferramentas(&mut self, g: Grupo, grupo: &m::GrupoAbas, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let ent = cx.entity().downgrade();
        let sujos = self.sujos();
        let caminhos: Vec<String> = self.ws.abas().filter_map(|t| t.alvo.caminho().map(str::to_string)).collect();
        let det = detalhes(&caminhos.iter().map(String::as_str).collect::<Vec<_>>());
        let itens: Vec<ItemLista> = grupo
            .tabs
            .iter()
            .map(|t| {
                let (id1, id2, e1, e2) = (t.id.clone(), t.id.clone(), ent.clone(), ent.clone());
                let detalhe = t.alvo.caminho().and_then(|p| det.get(p)).cloned().unwrap_or_default();
                let icone_el = if t.pinned { icone("icons/pin.svg", 16., cor::text_secondary()).into_any_element() } else { icone_alvo(&t.alvo, 16.) };
                let rotulo_el = div()
                    .flex()
                    .items_center()
                    .child(titulo(&t.alvo))
                    .when(!detalhe.is_empty(), |d| d.child(div().text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child(format!("\u{2002}{detalhe}"))));
                ItemLista::new(t.id.clone(), icone_el, rotulo_el, move |window, cx| {
                    let _ = e1.update(cx, |a, cx| a.ativar(&id1, window, cx));
                })
                .modificado(m::esta_sujo(&t.alvo, &sujos))
                .ao_fechar(move |window, cx| {
                    let _ = e2.update(cx, |a, cx| a.fechar(Escopo::Uma(id2.clone()), window, cx));
                })
            })
            .collect();
        let ativa = grupo.active.clone();
        let tem = !grupo.tabs.is_empty();
        let (e_div, e_menu) = (ent.clone(), ent.clone());
        let ativa_div = ativa.clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .pr(px(8.))
            .pl(px(4.))
            .when(tem, |d| {
                d.child(ListaAbas::new(SharedString::from(format!("lista-{g:?}")), &self.listas[idx(g)], itens).ativa(ativa.unwrap_or_default()))
            })
            .child(
                orquestrador_ui::widgets::acao(SharedString::from(format!("dividir-{g:?}")), "icons/columns-2.svg", ativa_div.is_some())
                    .tooltip_texto("Dividir o Editor à Direita (Ctrl+\\)")
                    .on_click(move |_, window, cx| {
                        if let Some(id) = &ativa_div {
                            let _ = e_div.update(cx, |a, cx| a.dividir(id, window, cx));
                        }
                    }),
            )
            .child(
                div().child(
                    gpui_kit::component::button::Button::new(SharedString::from(format!("mais-{g:?}")))
                        .ghost()
                        .xsmall()
                        .size(px(24.))
                        .child(icone("icons/ellipsis.svg", 14., cor::text_secondary()))
                        .tooltip("Mais Ações...")
                        .dropdown_menu(move |menu, window, cx| {
                            let Some(e) = e_menu.upgrade() else { return menu };
                            e.update(cx, |a, cx| a.menu_grupo(menu, g, window, cx))
                        }),
                ),
            )
            .into_any_element()
    }

    fn menu_grupo(&mut self, menu: PopupMenu, g: Grupo, _window: &mut Window, cx: &mut Context<Self>) -> PopupMenu {
        let ent = cx.entity().downgrade();
        let sujos = self.sujos();
        let livres: Vec<&m::Aba> = self.ws.grupo(g).map(|x| x.tabs.iter().filter(|t| !t.pinned).collect()).unwrap_or_default();
        let salvas = livres.iter().any(|t| !m::esta_sujo(&t.alvo, &sujos));
        let item = |rotulo: &'static str, atalho: &'static str, ok: bool, f: AcaoMenu| {
            let e = ent.clone();
            let f = Rc::new(f);
            PopupMenuItem::element(item_menu(rotulo, if ok { atalho } else { "" })).disabled(!ok).on_click(move |_, window, cx| {
                let f = f.clone();
                let _ = e.update(cx, |a, cx| f(a, window, cx));
            })
        };
        menu.item(item("Fechar Todas", "Ctrl+K W", !livres.is_empty(), Box::new(move |a, w, cx| a.fechar(Escopo::Todas(g), w, cx))))
            .item(item("Fechar Salvas", "", salvas, Box::new(move |a, w, cx| a.fechar(Escopo::Salvas(g), w, cx))))
            .separator()
            .item(item("Salvar Tudo", "", true, Box::new(|a, w, cx| a.salvar_tudo(w, cx, |_, _, _, _| {}))))
            .item(item("Reabrir Fechada", "Ctrl+Shift+T", !self.ws.closed.is_empty(), Box::new(|a, w, cx| a.reabrir_fechada(w, cx))))
            .item(item("Unir Grupos", "", self.ws.groups.len() == 2, Box::new(|a, _, cx| a.unir_grupos(cx))))
    }

    fn menu_aba(&mut self, menu: PopupMenu, aba: &m::Aba, _window: &mut Window, cx: &mut Context<Self>) -> PopupMenu {
        let ent = cx.entity().downgrade();
        let sujos = self.sujos();
        let alvo = aba.alvo.clone();
        let suja = m::esta_sujo(&alvo, &sujos);
        let grupo = self.ws.localizar(&aba.id).map(|(g, _)| g.clone());
        let (outras, direita, salvas, todas) = grupo.as_ref().map_or((0, 0, 0, 0), |g| {
            let at = g.tabs.iter().position(|t| t.id == aba.id).unwrap_or(0);
            (
                g.tabs.iter().filter(|t| t.id != aba.id && !t.pinned).count(),
                g.tabs[at + 1..].iter().filter(|t| !t.pinned).count(),
                g.tabs.iter().filter(|t| !t.pinned && !m::esta_sujo(&t.alvo, &sujos)).count(),
                g.tabs.iter().filter(|t| !t.pinned).count(),
            )
        });
        let gid = grupo.map(|g| g.id).unwrap_or(Grupo::A);
        let somente_leitura = alvo.doc_id().and_then(|d| self.docs.get(d)).is_some_and(|d| d.buffer.read_only);
        let rascunho = matches!(alvo, Alvo::Draft { .. });
        let tem_texto = matches!(alvo, Alvo::File { .. } | Alvo::Conflict { .. } | Alvo::Draft { .. });
        let pode_salvar = tem_texto && (rascunho || suja) && !somente_leitura;
        let fora = matches!(&alvo, Alvo::File { path, .. } if eh_absoluto(path) && relativo_a(&self.projeto, path).is_none());
        let tem_cursor = matches!(&alvo, Alvo::File { doc_id, .. } if self.cursores.contains_key(doc_id) || self.docs.contains_key(doc_id));
        let id = aba.id.clone();
        let item = |rotulo: &'static str, atalho: &'static str, ok: bool, f: AcaoMenu| {
            let e = ent.clone();
            let f = Rc::new(f);
            PopupMenuItem::element(item_menu(rotulo, if ok { atalho } else { "" })).disabled(!ok).on_click(move |_, window, cx| {
                let f = f.clone();
                let _ = e.update(cx, |a, cx| f(a, window, cx));
            })
        };
        let (a1, a2, a3, a4, a5) = (alvo.clone(), alvo.clone(), alvo.clone(), alvo.clone(), alvo.clone());
        let (i1, i2, i3, i4, i5, i6) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
        menu.min_w(px(320.))
            .item(item("Salvar", "Ctrl+S", pode_salvar, Box::new(move |a, w, cx| a.salvar_alvo(a1.clone(), w, cx, |_, _, _, _| {}))))
            .item(item("Salvar Tudo", "", true, Box::new(|a, w, cx| a.salvar_tudo(w, cx, |_, _, _, _| {}))))
            .separator()
            .item(item("Fechar", "Ctrl+F4", true, Box::new(move |a, w, cx| a.fechar(Escopo::Uma(i1.clone()), w, cx))))
            .item(item("Fechar Outras", "", outras > 0, Box::new(move |a, w, cx| a.fechar(Escopo::Outras(i2.clone()), w, cx))))
            .item(item("Fechar à Direita", "", direita > 0, Box::new(move |a, w, cx| a.fechar(Escopo::Direita(i3.clone()), w, cx))))
            .item(item("Fechar Salvas", "", salvas > 0, Box::new(move |a, w, cx| a.fechar(Escopo::Salvas(gid), w, cx))))
            .item(item("Fechar Todas", "Ctrl+K W", todas > 0, Box::new(move |a, w, cx| a.fechar(Escopo::Todas(gid), w, cx))))
            .separator()
            .item(item("Copiar Caminho Relativo", "Ctrl+K Ctrl+Shift+C", !rascunho, Box::new(move |a, _, cx| a.copiar_caminho(TipoCopia::Relativo, &a2, cx))))
            .item(item("Copiar o Caminho", "Shift+Alt+C", !rascunho, Box::new(move |a, _, cx| a.copiar_caminho(TipoCopia::Absoluto, &a3, cx))))
            .item(item("Copiar Caminho de Navegação", "", tem_cursor, Box::new(move |a, _, cx| a.copiar_caminho(TipoCopia::Navegacao, &a4, cx))))
            .separator()
            .item(item("Dividir à Direita", "Ctrl+\\", true, Box::new(move |a, w, cx| a.dividir(&i4, w, cx))))
            .item(item("Mover para o Outro Grupo", "", true, Box::new(move |a, _, cx| a.mover_para_outro(&i5, cx))))
            .item(item("Unir Grupos", "", self.ws.groups.len() == 2, Box::new(|a, _, cx| a.unir_grupos(cx))))
            .item(item("Reabrir Fechada", "Ctrl+Shift+T", !self.ws.closed.is_empty(), Box::new(|a, w, cx| a.reabrir_fechada(w, cx))))
            .separator()
            .item(item(if aba.pinned { "Desafixar" } else { "Fixar" }, "Ctrl+K Shift+Enter", true, Box::new(move |a, _, cx| {
                let ws = m::alternar_fixa(&a.ws, &i6);
                a.definir_ws(ws, cx);
            })))
            .item(item("Revelar no Explorer", "", !rascunho && !fora, Box::new(move |_, _, cx| {
                if let Some(p) = a5.caminho() {
                    cx.emit(EventoEditor::RevelarNoExplorer(p.to_string()));
                }
            })))
    }

    fn ir_para_alteracao(&mut self, passo: i32, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.ws.aba_focada().and_then(|t| t.alvo.doc_id()).filter(|d| self.com_alteracoes.contains(*d)) {
            let d = d.to_string();
            self.git_ir(&d, passo, window, cx);
        }
    }

    /// Menu de contexto do texto (`editItems` de file-view.tsx).
    fn menu_editor(&mut self, menu: PopupMenu, doc_id: &str, _window: &mut Window, cx: &mut Context<Self>) -> PopupMenu {
        use gpui_kit::component::input as kit;
        let Some(d) = self.docs.get(doc_id) else { return menu };
        let leitura = d.buffer.read_only;
        let selecao = !d.editor.read(cx).selected_range().is_empty();
        let sem_caminho = d.buffer.path.is_empty();
        let foco = d.editor.read(cx).focus_handle(cx);
        let alteracoes = self.com_alteracoes.contains(doc_id);
        let it = |m: PopupMenu, rotulo: &'static str, atalho: &'static str, acao: Box<dyn gpui::Action>, ok: bool| {
            m.menu_element_with_disabled(acao, !ok, item_menu(rotulo, if ok || rotulo == "Formatar Documento" { atalho } else { "" }))
        };
        let m = menu.action_context(foco).min_w(px(300.));
        let m = it(m, "Desfazer", "Ctrl+Z", Box::new(kit::Undo), !leitura);
        let m = it(m, "Refazer", "Ctrl+Y", Box::new(kit::Redo), !leitura).separator();
        let m = it(m, "Cortar", "Ctrl+X", Box::new(kit::Cut), !leitura && selecao);
        let m = it(m, "Copiar", "Ctrl+C", Box::new(kit::Copy), selecao);
        let m = it(m, "Colar", "Ctrl+V", Box::new(kit::Paste), !leitura);
        let m = it(m, "Selecionar Tudo", "Ctrl+A", Box::new(kit::SelectAll), true);
        let m = it(m, "Selecionar Próxima Ocorrência", "Ctrl+D", Box::new(documento::SelecionarProxima), true).separator();
        let m = it(m, "Comentar", "Ctrl+/", Box::new(documento::Comentar), !leitura);
        let m = it(m, "Mover Linha para Cima", "Alt+↑", Box::new(documento::MoverLinhaAcima), !leitura);
        let m = it(m, "Mover Linha para Baixo", "Alt+↓", Box::new(documento::MoverLinhaAbaixo), !leitura);
        let m = it(m, "Duplicar Linha", "Shift+Alt+↓", Box::new(documento::CopiarLinhaAbaixo), !leitura);
        let m = it(m, "Excluir Linha", "Ctrl+Shift+K", Box::new(documento::ExcluirLinha), !leitura);
        let m = it(m, "Formatar Documento", "Shift+Alt+F", Box::new(documento::Formatar), !leitura && !sem_caminho).separator();
        let m = it(m, "Localizar", "Ctrl+F", Box::new(kit::Search), true);
        let m = it(m, "Substituir", "Ctrl+H", Box::new(kit::Replace), !leitura);
        let m = it(m, "Ir para a Linha", "Ctrl+G", Box::new(documento::IrParaLinha), true).separator();
        let m = it(m, "Próxima Alteração", "Alt+F5", Box::new(documento::ProximaAlteracao), alteracoes);
        it(m, "Alteração Anterior", "Shift+Alt+F5", Box::new(documento::AlteracaoAnterior), alteracoes)
    }

    fn corpo(&mut self, g: Grupo, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(grupo) = self.ws.grupo(g).cloned() else { return div().into_any_element() };
        let ent = cx.entity().downgrade();
        let Some(aba) = grupo.tabs.iter().find(|t| Some(&t.id) == grupo.active.as_ref()).cloned() else {
            return div()
                .id(SharedString::from(format!("vazio-{g:?}")))
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .bg(cor::pitch_black())
                .on_click(move |ev: &ClickEvent, window, cx| {
                    if ev.click_count() == 2 {
                        let _ = ent.update(cx, |a, cx| a.novo_rascunho(g, window, cx));
                    }
                })
                .child(marca_dagua())
                .into_any_element();
        };
        if let Some(v) = self.conteudos.get(&aba.id) {
            return div().flex().flex_col().flex_1().min_h_0().child(v.clone()).into_any_element();
        }
        let Some(doc_id) = aba.alvo.doc_id().map(str::to_string) else {
            return nota("Sem visualização para esta aba.", false, window, cx).into_any_element();
        };
        let s = self.situacao.get(&doc_id).cloned().unwrap_or_default();
        if s.binario {
            return nota("Arquivo binário: não há texto para mostrar aqui.", false, window, cx).into_any_element();
        }
        if s.grande {
            return nota("Arquivo grande demais para mostrar aqui.", false, window, cx).into_any_element();
        }
        let Some(d) = self.docs.get(&doc_id) else {
            return match s.erro {
                Some(e) => {
                    let ent = ent.clone();
                    let id = doc_id.clone();
                    nota(&e, true, window, cx)
                        .child(
                            orquestrador_ui::widgets::botao_pequeno("tentar-de-novo").child("Tentar de novo").on_click(move |_, window, cx| {
                                let _ = ent.update(cx, |a, cx| {
                                    a.situacao.remove(&id);
                                    a.ler_faltantes(window, cx);
                                });
                            }),
                        )
                        .into_any_element()
                }
                None => nota("Lendo o arquivo…", false, window, cx).into_any_element(),
            };
        };
        let markdown = matches!(&aba.alvo, Alvo::File { path, .. } if eh_markdown(path));
        let previa = markdown && aba.view == Some(Vista::Preview);
        let mut notas: Vec<String> = Vec::new();
        if d.buffer.read_only {
            notas.push("Arquivo não está em UTF-8; somente leitura.".into());
        }
        if let Some(e) = &s.erro {
            notas.push(e.clone());
        }
        let editor = d.editor.clone();
        let mut col = div().flex().flex_col().flex_1().min_h_0();
        if !notas.is_empty() {
            col = col.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(32.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(cor::hairline())
                    .text_size(tema::TEXTO_LABEL)
                    .text_color(if s.erro.is_some() { cor::signal_error() } else { cor::text_secondary() })
                    .child(div().truncate().child(notas.join(" "))),
            );
        }
        if self.disco.contains_key(&doc_id) {
            let (e1, e2, d1, d2) = (ent.clone(), ent.clone(), doc_id.clone(), doc_id.clone());
            col = col.child(
                div()
                    .flex()
                    .flex_none()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .min_h(px(32.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(cor::hairline())
                    .child(div().text_size(tema::TEXTO_LABEL).text_color(cor::signal_error()).child("O arquivo mudou no disco e você tem alterações."))
                    .child(orquestrador_ui::widgets::botao_pequeno("usar-disco").child("Comparar e usar o disco").on_click(move |_, _, cx| {
                        let _ = e1.update(cx, |a, cx| a.abrir_comparacao(&d1, ModoComparacao::Usar, cx));
                    }).test_support())
                    .child(orquestrador_ui::widgets::botao_pequeno("manter-edicao").child("Comparar e salvar minha edição").on_click(move |_, _, cx| {
                        let _ = e2.update(cx, |a, cx| a.abrir_comparacao(&d2, ModoComparacao::Manter, cx));
                    }).test_support()),
            );
        }
        if previa {
            let texto: SharedString = d.buffer.draft.clone().into();
            let caminho = d.buffer.path.clone();
            return col.child(crate::markdown::previa(&aba.id, texto, caminho, ent.clone(), window, cx)).into_any_element();
        }
        let camada_git = self.git_camada(&doc_id, &editor, cx);
        let id_doc = doc_id.clone();
        let ent_k = ent.clone();
        let editor_el = div()
            .key_context(documento::CONTEXTO)
            .on_action(cx.listener(|a, _: &documento::Salvar, w, cx| a.salvar_ativa(w, cx)))
            .on_action(cx.listener(move |a, _: &documento::Formatar, w, cx| {
                if let Some(d) = a.ws.aba_focada().and_then(|t| t.alvo.doc_id()).map(str::to_string) {
                    a.formatar_documento(&d, w, cx);
                }
            }))
            .on_action(cx.listener(|a, _: &documento::IrParaLinha, w, cx| a.abrir_ir_para(w, cx)))
            .on_action(cx.listener(|a, _: &documento::Comentar, w, cx| a.comando(Comando::Comentar, w, cx)))
            .on_action(cx.listener(|a, _: &documento::MoverLinhaAcima, w, cx| a.comando(Comando::MoverLinhaAcima, w, cx)))
            .on_action(cx.listener(|a, _: &documento::MoverLinhaAbaixo, w, cx| a.comando(Comando::MoverLinhaAbaixo, w, cx)))
            .on_action(cx.listener(|a, _: &documento::CopiarLinhaAbaixo, w, cx| a.comando(Comando::CopiarLinhaAbaixo, w, cx)))
            .on_action(cx.listener(|a, _: &documento::CopiarLinhaAcima, w, cx| a.comando(Comando::CopiarLinhaAcima, w, cx)))
            .on_action(cx.listener(|a, _: &documento::ExcluirLinha, w, cx| a.comando(Comando::ExcluirLinha, w, cx)))
            .on_action(cx.listener(|a, _: &documento::SelecionarProxima, w, cx| a.comando(Comando::SelecionarProxima, w, cx)))
            .on_action(cx.listener(|a, _: &documento::SelecionarLinha, w, cx| a.comando(Comando::SelecionarLinha, w, cx)))
            .on_action(cx.listener(|a, _: &documento::InserirLinhaAbaixo, w, cx| a.comando(Comando::InserirLinhaAbaixo, w, cx)))
            .on_action(cx.listener(|a, _: &documento::IrParaPar, w, cx| a.comando(Comando::IrParaPar, w, cx)))
            .on_action(cx.listener(|a, _: &documento::ProximaAlteracao, w, cx| a.ir_para_alteracao(1, w, cx)))
            .on_action(cx.listener(|a, _: &documento::AlteracaoAnterior, w, cx| a.ir_para_alteracao(-1, w, cx)))
            // O Esc fecha o bloco do Git antes de qualquer atalho do editor (o `Prec.high` do React).
            .capture_key_down(cx.listener({
                let id = doc_id.clone();
                move |a, ev: &KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" && a.git_fechar(&id, cx) {
                        cx.stop_propagation();
                    }
                }
            }))
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .bg(documento::cor::fundo())
            .child(
                // Medido nas capturas do React: o texto do CodeMirror quebra na borda do minimapa e o Kit reserva 10 px à
                // direita; o editor passa por baixo do minimapa, desenhado por cima (`gutter_git::SOB_MINIMAPA`).
                div().flex_1().min_w_0().h_full().mr(-px(crate::gutter_git::SOB_MINIMAPA)).child(ComTemaDoEditor(Some(
                    Editor::new(&editor)
                        .appearance(false)
                        .bordered(false)
                        .h_full()
                        // O `.cm-content` não tem padding: o tamanho XSmall tira os 8 px de cima e deixa 4 px nas laterais;
                        // mais 2 px à esquerda põem o texto onde o CodeMirror põe, e a quebra fica na mesma coluna.
                        .with_size(gpui_kit::component::Size::XSmall)
                        .pl(px(2.))
                        .font_family(documento::FONTE_EDITOR)
                        .text_size(documento::TAMANHO_EDITOR)
                        .line_height(documento::LINHA_EDITOR)
                        .into_any_element(),
                ))),
            )
            .context_menu(move |menu, window, cx| {
                let Some(e) = ent_k.upgrade() else { return menu };
                e.update(cx, |a, cx| a.menu_editor(menu, &id_doc, window, cx))
            })
            .child({
                // Sem base do Git o React não configura a faixa no minimapa, e o texto começa na borda.
                let mini = minimapa::Minimapa::new(editor.clone());
                match self.git.get(&doc_id).filter(|e| e.base.is_some()) {
                    Some(e) => mini.com_git(crate::gutter_git::faixa_minimapa(&e.blocos)),
                    None => mini,
                }
            })
            // Depois do minimapa: a régua fica por cima dele, como o `zIndex` do React.
            .children(camada_git);
        col.child(editor_el).into_any_element()
    }

    fn grupo(&mut self, g: Grupo, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let ent = cx.entity().downgrade();
        let ativa = self.ws.grupo(g).and_then(|gr| gr.tabs.iter().find(|t| Some(&t.id) == gr.active.as_ref()).cloned());
        let faixa = self.faixa(g, window, cx);
        let migalha = match &ativa {
            Some(t) if matches!(t.alvo, Alvo::File { .. }) => {
                let caminho = t.alvo.caminho().unwrap_or("").to_string();
                let extra = eh_markdown(&caminho).then(|| {
                    let (id, ent) = (t.id.clone(), ent.clone());
                    crate::markdown::seletor(t.view == Some(Vista::Preview), move |previa, _window, cx| {
                        let _ = ent.update(cx, |a, cx| {
                            let ws = m::definir_vista(&a.ws, &id, previa.then_some(Vista::Preview));
                            a.definir_ws(ws, cx);
                        });
                    })
                    .into_any_element()
                });
                Some(migalhas::barra(&caminho, g, self.migalha.as_ref(), extra, ent.clone()).into_any_element())
            }
            _ => None,
        };
        let corpo = self.corpo(g, window, cx);
        div()
            .id(SharedString::from(format!("grupo-{g:?}")))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .on_mouse_down(MouseButton::Left, cx.listener(move |a, _: &MouseDownEvent, _, cx| {
                if a.ws.focus != g && a.ws.grupo(g).is_some() {
                    let mut ws = a.ws.clone();
                    ws.focus = g;
                    a.definir_ws(ws, cx);
                }
            }))
            .child(faixa)
            .children(migalha)
            .child(div().relative().flex().flex_col().flex_1().min_h_0().child(corpo))
            .into_any_element()
    }

    /// Esc (ou a janela sem foco) cancela o arrasto na faixa de origem; sem isto, soltar sobre a outra faixa ainda moveria a aba.
    fn conferir_arrasto_cancelado(&mut self) {
        if let Some((origem, _)) = &self.arrasto_aba {
            if self.faixas[idx(*origem)].arrasto().is_none() {
                self.arrasto_aba = None;
            }
        }
    }

    /// Grupo de destino quando a aba arrastada está sobre a faixa do outro grupo.
    fn destino_arrasto(&self) -> Option<Grupo> {
        let (origem, _) = self.arrasto_aba.as_ref()?;
        let outro = origem.outro();
        self.ws.grupo(outro)?;
        self.caixas_faixa[idx(outro)].get().contains(&self.ponteiro).then_some(outro)
    }

    /// Soltar a aba arrastada sobre a faixa do outro grupo: entra antes da primeira aba cujo meio fica à direita.
    fn soltar_aba(&mut self, cx: &mut Context<Self>) {
        let destino = self.destino_arrasto();
        let Some((_, id)) = self.arrasto_aba.take() else { return };
        let Some(para) = destino else { return };
        let n = self.ws.grupo(para).map_or(0, |g| g.tabs.len());
        let x = self.ponteiro.x;
        let indice = (0..n)
            .filter(|&i| self.faixas[idx(para)].rolagem().bounds_for_item(i).is_some_and(|b| b.origin.x + b.size.width / 2. < x))
            .count();
        let ws = m::mover_aba(&self.ws, &id, para, Some(indice));
        self.definir_ws(ws, cx);
    }

    pub(crate) fn abrir_seletor_migalha(&mut self, g: Grupo, indice: usize, caminho: &str, window: &mut Window, cx: &mut Context<Self>) {
        let partes: Vec<&str> = caminho.split('/').filter(|s| !s.is_empty()).collect();
        let pasta = partes[..indice].join("/");
        let marcado = partes[..=indice].join("/");
        self.migalha = Some(migalhas::Seletor::new(g, indice, pasta.clone(), marcado));
        self.ler_pasta_migalha(pasta, window, cx);
        cx.notify();
    }

    pub(crate) fn ler_pasta_migalha(&mut self, pasta: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.migalha.as_mut() {
            s.pasta = pasta.clone();
            s.entradas = None;
            s.erro = None;
        }
        let raiz = PathBuf::from(&self.projeto);
        let p = pasta.clone();
        let t = cx.background_spawn(async move { files::list_many(&raiz, &[p]) });
        cx.spawn_in(window, async move |this, cx| {
            let r = t.await;
            let _ = this.update(cx, |a, cx| {
                if let Some(s) = a.migalha.as_mut().filter(|s| s.pasta == pasta) {
                    match r {
                        Ok(mut v) if !v.is_empty() => match v.remove(0) {
                            files::DirListing::Ok { entries } => s.entradas = Some(entries),
                            files::DirListing::Error { message } => s.erro = Some(message),
                        },
                        Ok(_) => s.erro = Some(format!("Pasta não encontrada: {pasta}")),
                        Err(e) => s.erro = Some(e),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn fechar_migalha(&mut self, cx: &mut Context<Self>) {
        self.migalha = None;
        cx.notify();
    }

    /// Link relativo do Preview de Markdown: abre no editor.
    pub(crate) fn abrir_do_markdown(&mut self, caminho: String, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_arquivo(&caminho, Abertura::default(), window, cx);
    }

    /// Passos da vitrine que dependem da leitura em segundo plano (`examples/vitrine.rs`).
    #[doc(hidden)]
    pub fn passo_vitrine(&mut self, modo: &str, window: &mut Window, cx: &mut Context<Self>) {
        let focada = self.ws.aba_focada().cloned();
        let doc = focada.as_ref().and_then(|t| t.alvo.doc_id()).and_then(|d| self.docs.get(d)).map(|d| d.editor.clone());
        match modo {
            "varias" => {
                if let Some(e) = doc {
                    e.update(cx, |s, cx| {
                        let fim = s.text().len();
                        s.set_selected_range(fim..fim, cx);
                        s.insert("
linha nova", window, cx);
                    });
                }
            }
            "lista" => {
                let g = self.ws.focus;
                self.listas[idx(g)].abrir(window, cx);
            }
            "md-preview" => {
                if let Some(t) = focada {
                    let ws = m::definir_vista(&self.ws, &t.id, Some(Vista::Preview));
                    self.definir_ws(ws, cx);
                }
            }
            "rapido" => {
                if let Some(v) = self.abrir_rapido.clone() {
                    v.update(cx, |r, cx| r.definir_termo("app", window, cx));
                }
            }
            "git-bloco" => {
                if let Some(d) = focada.as_ref().and_then(|t| t.alvo.doc_id()).map(str::to_string) {
                    self.git_ir(&d, 1, window, cx);
                }
            }
            "arquivos" => {
                if let Some(e) = doc {
                    // A captura do React tem a linha 26 no topo.
                    e.update(cx, |s, cx| {
                        if let Some(lh) = s.line_height() {
                            let topo = s.display_map().buffer_line_box_top(25, lh);
                            s.set_scroll_offset(gpui::point(px(0.), -topo), cx);
                        }
                    });
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn migalha_mut(&mut self) -> Option<&mut migalhas::Seletor> {
        self.migalha.as_mut()
    }

    fn modal_pergunta(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.pergunta.as_ref()?;
        let mut rodape = div().flex().flex_wrap().justify_end().gap(px(8.)).child(
            orquestrador_ui::widgets::botao("pergunta-cancelar", "Cancelar", orquestrador_ui::widgets::Variante::Padrao, false)
                .on_click(cx.listener(|a, _, w, cx| a.responder(None, w, cx)))
                .test_support(),
        );
        for (chave, rotulo_acao, perigo) in &p.acoes {
            let chave: &'static str = chave;
            let variante = if *perigo { orquestrador_ui::widgets::Variante::Padrao } else { orquestrador_ui::widgets::Variante::Primario };
            let mut b = orquestrador_ui::widgets::botao(chave, *rotulo_acao, variante, false).on_click(cx.listener(move |a, _, w, cx| a.responder(Some(chave), w, cx)));
            if *perigo {
                b = b.text_color(cor::signal_error()).border_color(cor::signal_error());
            }
            rodape = rodape.child(b.test_support());
        }
        Some(modal(
            "pergunta",
            480.,
            p.titulo.clone(),
            widgets::texto_pretty("pergunta-corpo", p.corpo.clone(), tema::TEXTO_BODY, gpui::FontWeight::NORMAL, window, cx)
                .line_height(px(22.4))
                .text_color(cor::text_secondary())
                .into_any_element(),
            rodape.into_any_element(),
            window,
            cx.listener(|a, _: &MouseDownEvent, w, cx| a.responder(None, w, cx)),
        ))
    }

    fn modal_comparacao(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = self.comparacao.as_ref()?;
        let usar = c.modo == ModoComparacao::Usar;
        let titulo_m = if usar { "Usar a versão do disco?" } else { "Salvar sua edição sobre o disco?" };
        let corpo = crate::comparar::vista(&c.caminho, &c.disco.texto, &c.rascunho, usar);
        let rodape = div()
            .flex()
            .justify_end()
            .gap(px(8.))
            .child(orquestrador_ui::widgets::botao("cmp-cancelar", "Cancelar", orquestrador_ui::widgets::Variante::Padrao, false).on_click(cx.listener(|a, _, _, cx| {
                a.comparacao = None;
                cx.notify();
            })))
            .child(
                orquestrador_ui::widgets::botao(
                    "cmp-confirmar",
                    if usar { "Descartar minha edição e usar o disco" } else { "Salvar minha edição sobre o disco" },
                    if usar { orquestrador_ui::widgets::Variante::Padrao } else { orquestrador_ui::widgets::Variante::Primario },
                    false,
                )
                .when(usar, |b| b.text_color(cor::signal_error()).border_color(cor::signal_error()))
                .on_click(cx.listener(|a, _, w, cx| a.confirmar_comparacao(w, cx))),
            );
        Some(modal("comparacao", 960., titulo_m.into(), corpo, rodape.into_any_element(), window, cx.listener(|a, _: &MouseDownEvent, _, cx| {
            a.comparacao = None;
            cx.notify();
        })))
    }

    fn painel_ir_para(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let i = self.ir_para.as_ref()?;
        Some(
            div()
                .absolute()
                .top(px(8.))
                .right(px(16.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .p(px(8.))
                .rounded(tema::RAIO_SM)
                .border_1()
                .border_color(cor::hairline())
                .bg(cor::raised_black())
                .on_key_down(cx.listener(|a, ev: &KeyDownEvent, w, cx| {
                    if ev.keystroke.key == "escape" {
                        cx.stop_propagation();
                        a.fechar_ir_para(w, cx);
                    }
                }))
                .child(div().text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child("Linha:coluna"))
                .child(div().w(px(112.)).child(Input::new(&i.campo).small()))
                .child(orquestrador_ui::widgets::botao_pequeno("ir-para").child("Ir").on_click(cx.listener(|a, _, w, cx| a.confirmar_ir_para(w, cx))))
                .children(i.erro.clone().map(|e| div().w_full().text_size(tema::TEXTO_LABEL).text_color(cor::signal_error()).child(e)))
                .into_any_element(),
        )
    }
}

/// Marca d'água do grupo vazio: a marca do app a 6%.
fn marca_dagua() -> impl IntoElement {
    gpui::svg().path(orquestrador_ui::recursos::icone::MARCA_MONO).size(px(240.)).text_color(cor::marca_dagua())
}

/// Nota no lugar do editor (`Note` de media-view.tsx): texto cinza a 16 px da borda.
fn nota(texto: &str, perigo: bool, window: &mut Window, cx: &mut App) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .gap(px(8.))
        .p(px(16.))
        .bg(cor::pitch_black())
        .text_size(tema::TEXTO_BODY)
        .line_height(px(22.))
        .text_color(if perigo { cor::signal_error() } else { cor::text_tertiary() })
        .child(widgets::texto_pretty("nota", texto.to_string(), tema::TEXTO_BODY, gpui::FontWeight::NORMAL, window, cx))
}

/// Modal do Lobe UI: a máscara comum, cartão `raised_black` com borda e raio de 8 px.
fn modal(
    id: &'static str, largura: f32, titulo: SharedString, corpo: AnyElement, rodape: AnyElement, window: &Window,
    fora: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    deferred(
        anchored().position(gpui::point(px(0.), px(0.))).child(
            widgets::mascara(id, window)
                .flex()
                .items_start()
                .justify_center()
                .pt(px(100.))
                .child(
                    div()
                        .id("modal-cartao")
                        .occlude()
                        .w(px(largura))
                        .max_w(gpui::relative(0.9))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .p(px(20.))
                        .rounded(tema::RAIO_MD)
                        .bg(cor::raised_black())
                        .border_1()
                        .border_color(gpui::rgba(0xffffff26))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(div().text_size(tema::TEXTO_TITLE).font_weight(gpui::FontWeight::SEMIBOLD).child(titulo))
                        .child(corpo)
                        .child(rodape),
                )
                .on_mouse_down(MouseButton::Left, fora),
        ),
    )
    .with_priority(2)
    .into_any_element()
}

impl Render for AreaEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.avisar_mudancas(cx);
        self.conferir_arrasto_cancelado();
        let dois = self.ws.groups.len() == 2;
        let mostrar_inicio = self.projeto.is_empty() && self.ws.groups.iter().all(|g| g.tabs.is_empty());
        let grupos: Vec<Grupo> = self.ws.groups.iter().map(|g| g.id).collect();
        let mut linha = div().id("grupos").relative().flex().flex_1().min_h_0().h_full();
        if mostrar_inicio {
            let ent = cx.entity().downgrade();
            linha = linha.child(
                div()
                    .id("inicio")
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .on_click(move |ev: &ClickEvent, window, cx| {
                        if ev.click_count() == 2 {
                            let _ = ent.update(cx, |a, cx| a.novo_arquivo(window, cx));
                        }
                    })
                    .child(marca_dagua()),
            );
        } else {
            for (i, g) in grupos.iter().enumerate() {
                if i == 1 {
                    linha = linha.child(
                        div()
                            .id("divisor-grupos")
                            .w(px(4.))
                            .flex_none()
                            .h_full()
                            .cursor_col_resize()
                            .hover(|s| s.bg(cor::hairline()))
                            .on_mouse_down(MouseButton::Left, cx.listener(|a, ev: &MouseDownEvent, _, cx| {
                                a.arrasto_divisor = Some((f32::from(ev.position.x), a.proporcao, a.largura));
                                cx.notify();
                            })),
                    );
                }
                let fr = if dois { if i == 0 { self.proporcao } else { 1. - self.proporcao } } else { 1. };
                let el = self.grupo(*g, window, cx);
                linha = linha.child(div().flex().min_w_0().min_h_0().h_full().flex_grow(1.).flex_shrink(1.).flex_basis(gpui::relative(fr)).child(el));
            }
        }
        if self.arrasto_aba.is_some() {
            linha = linha
                .on_mouse_move(cx.listener(|a, ev: &MouseMoveEvent, _, cx| {
                    let antes = a.destino_arrasto();
                    a.conferir_arrasto_cancelado();
                    a.ponteiro = ev.position;
                    if a.destino_arrasto() != antes {
                        cx.notify();
                    }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|a, _: &MouseUpEvent, _, cx| {
                    a.soltar_aba(cx);
                    cx.notify();
                }));
        }
        if self.arrasto_divisor.is_some() {
            linha = linha
                .on_mouse_move(cx.listener(|a, ev: &MouseMoveEvent, _, cx| {
                    if let Some((x0, p0, larg)) = a.arrasto_divisor {
                        if larg > 0. {
                            a.proporcao = (p0 + (f32::from(ev.position.x) - x0) / larg).clamp(0.15, 0.85);
                            cx.notify();
                        }
                    }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|a, _: &MouseUpEvent, _, cx| {
                    a.arrasto_divisor = None;
                    a.agendar_recuperacao(cx);
                    cx.notify();
                }));
        }
        let ent = cx.entity().downgrade();
        linha = linha.child(
            gpui::canvas(
                move |b, _, cx| {
                    let _ = ent.update(cx, |a, _| a.largura = f32::from(b.size.width));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
        let aviso = self.aviso.clone().map(|t| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .h(px(28.))
                .px(px(12.))
                .border_b_1()
                .border_color(cor::hairline())
                .child(div().min_w_0().truncate().text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child(t))
                .child(div().ml_auto().child(orquestrador_ui::widgets::acao("fechar-aviso", orquestrador_ui::recursos::icone::FECHAR, true).on_click(cx.listener(|a, _, _, cx| {
                    a.aviso = None;
                    cx.notify();
                }))))
        });
        let pergunta = self.modal_pergunta(window, cx);
        let comparacao = self.modal_comparacao(window, cx);
        let ir_para = self.painel_ir_para(cx);
        let migalha = self.migalha.as_ref().map(|s| migalhas::seletor(s, cx.entity().downgrade()));
        for alvo in crate::markdown::pedidos() {
            cx.defer_in(window, move |a, w, cx| a.abrir_do_markdown(alvo, w, cx));
        }
        div()
            .id("area-editor")
            .track_focus(&self.foco)
            .key_context("AreaEditor")
            .capture_key_down(cx.listener(|a, ev: &KeyDownEvent, w, cx| a.tecla(ev, w, cx)))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(cor::pitch_black())
            .font_family(tema::FONTE)
            .text_color(cor::text_primary())
            .children(aviso)
            .child(linha)
            .children(ir_para)
            .children(self.abrir_rapido.clone().map(|v| deferred(v).with_priority(2)))
            .children(migalha)
            .children(pergunta)
            .children(comparacao)
    }
}
