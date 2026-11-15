//! A tela Pesquisar (porte de `src/search/search.tsx`): cabeçalho com Atualizar, Limpar, Árvore/Lista e Recolher; a seta
//! à esquerda abre o Substituir; o `...` abre os filtros de caminho; resumo e avisos; lista virtual de resultados; diálogo
//! do Substituir Tudo. Busca, prévia e aplicação rodam no motor do core, fora da thread da interface.
use orquestrador_ui::icones_arquivo::icone_arquivo;
use crate::logica::{self, ArquivoVisivel, Linha, Ocorrencia, SubstitutosArquivo, Visao};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::{
    anchored, deferred, div, point, prelude::*, px, rgba, img, uniform_list, AnyElement, App, ClipboardItem, Context,
    Entity, EventEmitter, FocusHandle, Focusable, FontWeight, HighlightStyle, Hsla, KeyDownEvent, MouseButton,
    ScrollStrategy, SharedString, StrikethroughStyle, StyledText, Task, UniformListScrollHandle, WeakEntity, Window,
};
use orquestrador_core::ide::search::{self, ApplyRequest, Pick, Preview, Query, Results};
use orquestrador_ui::estado::{self, Secao};
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::{self, icone, TooltipTexto};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// O que roda com a prévia nova (Substituir Tudo ou troca por arquivo sem prévia pronta).
type DepoisPrevia = Box<dyn FnOnce(&mut TelaPesquisa, Previa, &mut Context<TelaPesquisa>)>;

const ESPERA: Duration = Duration::from_millis(300);
const LINHA: f32 = 22.;
const RECUO: f32 = 8.;

mod icones {
    pub const ATUALIZAR: &str = "icons/rotate-cw.svg";
    pub const LIMPAR: &str = "icons/list-x.svg";
    pub const ARVORE: &str = "icons/list-tree.svg";
    pub const LISTA: &str = "icons/list.svg";
    pub const RECOLHER: &str = "icons/chevrons-down-up.svg";
    pub const EXPANDIR: &str = "icons/chevrons-up-down.svg";
    pub const SETA: &str = "icons/chevron-right.svg";
    pub const SETA_ABERTA: &str = "icons/chevron-down.svg";
    pub const CAIXA: &str = "icons/case-sensitive.svg";
    pub const PALAVRA: &str = "icons/whole-word.svg";
    pub const REGEX: &str = "icons/regex.svg";
    pub const PRESERVAR: &str = "icons/case-upper.svg";
    pub const SUBSTITUIR_TUDO: &str = "icons/replace-all.svg";
    pub const DETALHES: &str = "icons/ellipsis.svg";
    pub const ABERTOS: &str = "icons/book-open.svg";
    pub const FECHAR: &str = "icons/x.svg";
}

// Cores do `index.css` (.search-hit*) e do antd escuro (fill-secondary/tertiary).
fn cor_achado() -> Hsla { rgba(0xea5c0054).into() }
fn cor_removido() -> Hsla { rgba(0xff000033).into() }
fn cor_inserido() -> Hsla { rgba(0x9ccc2c33).into() }
fn fill_secundario() -> Hsla { rgba(0xffffff1f).into() }
fn fill_terciario() -> Hsla { rgba(0xffffff14).into() }
fn fundo_campo() -> Hsla { rgba(0xffffff17).into() }

/// O que a bancada trata.
#[derive(Clone, Debug, PartialEq)]
pub enum EventoPesquisa {
    /// Abre o arquivo na linha e na coluna (UTF-16, a partir de 1). Clique simples pede prévia; duplo clique e Enter
    /// pedem aba permanente.
    AbrirArquivo { caminho: String, linha: u32, coluna: u32, permanente: bool },
    /// Botão do mouse desceu sobre um resultado: a bancada pode ler o arquivo antes do clique terminar.
    PreLer { caminho: String },
    /// A substituição gravou arquivos: recarregar o que estiver aberto.
    ArquivosAlterados { caminhos: Vec<String> },
    /// Mensagem curta para o usuário (cópia feita).
    Aviso(String),
}

/// Preferência salva: visão da lista e histórico de consultas (no React, `localStorage`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PreferenciaPesquisa {
    pub visao: Visao,
    pub historico: Vec<String>,
}

impl Secao for PreferenciaPesquisa {
    const NOME: &'static str = "pesquisa";
    const VERSAO: u32 = 1;
    fn validar(&mut self) {
        let mut vistos = HashSet::new();
        self.historico.retain(|q| !q.is_empty() && vistos.insert(q.clone()));
        self.historico.truncate(logica::HISTORICO_MAX);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Opcao {
    Caixa,
    Palavra,
    Regex,
}

/// A prévia que está valendo: a consulta e a substituição que a geraram e a resposta.
#[derive(Clone)]
struct Previa {
    consulta: Query,
    substituto: String,
    dados: Rc<Preview>,
}

struct Relatorio {
    trocadas: usize,
    arquivos: usize,
    substituto: String,
    erros: Vec<(String, String)>,
    fora: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modo {
    Depois,
    Agora,
    Nada,
}

pub struct TelaPesquisa {
    foco: FocusHandle,
    foco_lista: FocusHandle,
    raiz: Option<PathBuf>,
    abertos: Vec<String>,
    alterados: HashSet<String>,

    campo: Entity<InputState>,
    campo_substituir: Entity<InputState>,
    campo_incluir: Entity<InputState>,
    campo_excluir: Entity<InputState>,
    consulta: String,
    substituto: String,
    incluir: String,
    excluir: String,
    caixa: bool,
    palavra: bool,
    regex: bool,
    so_abertos: bool,
    preservar: bool,

    resultados: Option<Rc<Results>>,
    // Padrão da resposta que está na tela: o destaque usa este, não o dos campos.
    padrao: Option<Regex>,
    erro: String,
    pendente: bool,
    visao: Visao,
    recolhidos: HashSet<String>,
    arquivos_fora: HashSet<String>,
    ocorrencias_fora: HashSet<String>,
    historico: Vec<String>,
    // Posição na navegação do histórico (-1 é o texto digitado) e o rascunho guardado ao começar a navegar.
    navegando: isize,
    digitado: String,

    detalhes: bool,
    substituir: bool,
    fora_aberto: bool,
    previa: Option<Previa>,
    erro_previa: String,
    relatorio: Option<Relatorio>,
    aplicando: bool,
    confirmando: Option<Previa>,
    atual: Option<(Rc<str>, u32, u32)>,
    linha_focada: Option<String>,
    lista_com_foco: bool,

    // Derivados, refeitos só quando o que os gera muda (digitar nos campos não os recalcula).
    todas: Vec<ArquivoVisivel>,
    arquivos: Vec<ArquivoVisivel>,
    parcial: HashSet<String>,
    linhas: Rc<Vec<Linha>>,
    total: usize,
    substitutos: HashMap<String, SubstitutosArquivo>,

    rolagem: UniformListScrollHandle,
    // Só a resposta mais nova vale; a tarefa largada é cancelada (o resultado de quem já rodava é descartado).
    geracao: u64,
    geracao_previa: u64,
    espera: Option<Task<()>>,
    busca: Option<Task<()>>,
    espera_previa: Option<Task<()>>,
    tarefa_previa: Option<Task<()>>,
    aplicacao: Option<Task<()>>,
    giro: Option<Task<()>>,
    passo_giro: u32,
}

impl EventEmitter<EventoPesquisa> for TelaPesquisa {}

impl Focusable for TelaPesquisa {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

impl TelaPesquisa {
    /// `raiz` é a pasta do projeto ativo (a do `core.repo()`); sem projeto, a busca não roda.
    pub fn new(raiz: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pref: PreferenciaPesquisa = estado::ler(cx);
        let novo = |dica: &str, window: &mut Window, cx: &mut Context<Self>| {
            let dica = SharedString::from(dica.to_string());
            cx.new(|cx| InputState::new(window, cx).placeholder(dica))
        };
        let campo = novo("Pesquisar", window, cx);
        let campo_substituir = novo("Substituir", window, cx);
        let campo_incluir = novo("por exemplo, *.ts, ./src", window, cx);
        let campo_excluir = novo("por exemplo, *.test.ts, ./dist", window, cx);
        for (c, qual) in [(&campo, 0u8), (&campo_substituir, 1), (&campo_incluir, 2), (&campo_excluir, 3)] {
            cx.subscribe_in(c, window, move |this, c, ev: &InputEvent, window, cx| this.no_campo(qual, c, ev, window, cx))
                .detach();
        }
        TelaPesquisa {
            foco: cx.focus_handle(),
            foco_lista: cx.focus_handle(),
            raiz,
            abertos: Vec::new(),
            alterados: HashSet::new(),
            campo,
            campo_substituir,
            campo_incluir,
            campo_excluir,
            consulta: String::new(),
            substituto: String::new(),
            incluir: String::new(),
            excluir: String::new(),
            caixa: false,
            palavra: false,
            regex: false,
            so_abertos: false,
            preservar: false,
            resultados: None,
            padrao: None,
            erro: String::new(),
            pendente: false,
            visao: pref.visao,
            recolhidos: HashSet::new(),
            arquivos_fora: HashSet::new(),
            ocorrencias_fora: HashSet::new(),
            historico: pref.historico,
            navegando: -1,
            digitado: String::new(),
            detalhes: false,
            substituir: false,
            fora_aberto: false,
            previa: None,
            erro_previa: String::new(),
            relatorio: None,
            aplicando: false,
            confirmando: None,
            atual: None,
            linha_focada: None,
            lista_com_foco: false,
            todas: Vec::new(),
            arquivos: Vec::new(),
            parcial: HashSet::new(),
            linhas: Rc::new(Vec::new()),
            total: 0,
            substitutos: HashMap::new(),
            rolagem: UniformListScrollHandle::new(),
            geracao: 0,
            geracao_previa: 0,
            espera: None,
            busca: None,
            espera_previa: None,
            tarefa_previa: None,
            aplicacao: None,
            giro: None,
            passo_giro: 0,
        }
    }

    // ---------- API da bancada ----------

    /// Troca o projeto: a busca atual é refeita na pasta nova.
    pub fn definir_raiz(&mut self, raiz: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.raiz != raiz {
            self.raiz = raiz;
            self.buscar(true, cx);
        }
    }

    /// Caminhos (relativos, com `/`) abertos no editor, para "só nos editores abertos".
    pub fn definir_abertos(&mut self, abertos: Vec<String>, cx: &mut Context<Self>) {
        let mudou = self.abertos != abertos;
        self.abertos = abertos;
        if mudou && self.so_abertos {
            self.buscar(true, cx);
        }
    }

    /// Caminhos com alteração não salva no editor: marcados "alterado" e deixados fora da substituição.
    pub fn definir_alterados(&mut self, alterados: HashSet<String>, cx: &mut Context<Self>) {
        self.alterados = alterados;
        cx.notify();
    }

    /// Ao entrar na tela e a cada Ctrl+Shift+F: o campo recebe o foco com o texto selecionado.
    pub fn focar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.campo.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
    }

    /// Localizar na pasta (`pasta` relativa, "" é o projeto): vira `./pasta` no incluir, os detalhes abrem e a busca refaz.
    pub fn pesquisar_na_pasta(&mut self, pasta: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.incluir = if pasta.is_empty() { String::new() } else { format!("./{pasta}") };
        let v = self.incluir.clone();
        self.campo_incluir.update(cx, |s, cx| s.set_value(v, window, cx));
        self.detalhes = true;
        if !self.consulta.is_empty() {
            self.agora(cx);
        }
        self.campo.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    /// Texto selecionado no editor: vira a consulta, a busca roda na hora e o campo fica selecionado.
    pub fn semear(&mut self, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.navegando = -1;
        self.trocar_consulta(texto, window, cx);
        self.relatorio = None;
        self.agora(cx);
        self.focar(window, cx);
    }

    /// Gravação no editor: refaz a busca sem entrar no histórico e sem tirar o recolhido e o dispensado.
    pub fn atualizar_apos_gravar(&mut self, cx: &mut Context<Self>) {
        if !self.consulta.is_empty() {
            self.buscar(true, cx);
        }
    }

    /// F4 (`dir` 1) e Shift+F4 (`dir` -1): anda pelos resultados, com volta, e abre como prévia.
    pub fn passo(&mut self, dir: i32, cx: &mut Context<Self>) {
        let atual = self.atual.as_ref().map(|(c, l, col)| (&**c, *l, *col));
        let Some(o) = logica::passo(&self.arquivos, atual, dir) else { return };
        self.recolhidos.remove(&logica::chave_arquivo(&o.caminho));
        for a in logica::ancestrais(&o.caminho) {
            self.recolhidos.remove(&a);
        }
        self.marcar(&o);
        self.refazer_linhas();
        self.rolar_atual();
        cx.emit(EventoPesquisa::AbrirArquivo { caminho: o.caminho.to_string(), linha: o.linha, coluna: o.col, permanente: false });
        cx.notify();
    }

    /// Abre o Substituir com `texto` (a prévia segue os campos).
    pub fn definir_substituicao(&mut self, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.substituir = true;
        self.substituto = texto.to_string();
        let v = self.substituto.clone();
        self.campo_substituir.update(cx, |s, cx| s.set_value(v, window, cx));
        self.mudou(Modo::Nada, cx);
    }

    /// Consulta digitada (para testes e para a bancada saber o que está no campo).
    pub fn consulta(&self) -> &str {
        &self.consulta
    }

    /// Texto do resumo como aparece na tela.
    pub fn status(&self) -> String {
        if !self.erro.is_empty() {
            return self.erro.clone();
        }
        if self.pendente {
            return "Pesquisando…".into();
        }
        match &self.resultados {
            Some(r) => logica::resumo(self.total, self.arquivos.len(), r.truncated),
            None if !self.consulta.is_empty() => "Pesquisando…".into(),
            None => String::new(),
        }
    }

    // ---------- formulário ----------

    fn no_campo(&mut self, qual: u8, c: &Entity<InputState>, ev: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            InputEvent::Change => {
                let v = c.read(cx).value().to_string();
                match qual {
                    0 if v != self.consulta => {
                        // Digitar sai da navegação do histórico: o texto novo vira o rascunho.
                        self.navegando = -1;
                        self.consulta = v;
                        self.mudou(Modo::Depois, cx);
                    }
                    1 if v != self.substituto => {
                        self.substituto = v;
                        self.mudou(Modo::Nada, cx);
                    }
                    2 if v != self.incluir => {
                        self.incluir = v;
                        self.mudou(Modo::Depois, cx);
                    }
                    3 if v != self.excluir => {
                        self.excluir = v;
                        self.mudou(Modo::Depois, cx);
                    }
                    _ => {}
                }
            }
            InputEvent::PressEnter { .. } if qual != 1 => self.agora(cx),
            _ => {}
        }
        let _ = window;
    }

    /// Algo do formulário mudou: o relatório sai, a busca roda (agora, depois ou não) e a prévia acompanha.
    fn mudou(&mut self, modo: Modo, cx: &mut Context<Self>) {
        self.relatorio = None;
        match modo {
            Modo::Depois => {
                self.espera = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(ESPERA).await;
                    let _ = this.update(cx, |this, cx| this.buscar(false, cx));
                }));
            }
            Modo::Agora => self.agora(cx),
            Modo::Nada => {}
        }
        self.agendar_previa(cx);
        cx.notify();
    }

    fn agora(&mut self, cx: &mut Context<Self>) {
        self.espera = None;
        self.buscar(false, cx);
    }

    fn alternar(&mut self, o: Opcao, cx: &mut Context<Self>) {
        match o {
            Opcao::Caixa => self.caixa = !self.caixa,
            Opcao::Palavra => self.palavra = !self.palavra,
            Opcao::Regex => self.regex = !self.regex,
        }
        self.mudou(Modo::Agora, cx);
    }

    fn trocar_consulta(&mut self, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.consulta = texto.to_string();
        let v = self.consulta.clone();
        self.campo.update(cx, |s, cx| s.set_value(v, window, cx));
    }

    /// Limpar: como o VS Code, tira a consulta, a substituição, os filtros e os resultados.
    fn limpar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navegando = -1;
        self.consulta.clear();
        self.substituto.clear();
        self.incluir.clear();
        self.excluir.clear();
        self.so_abertos = false;
        for c in [&self.campo, &self.campo_substituir, &self.campo_incluir, &self.campo_excluir] {
            c.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.mudou(Modo::Agora, cx);
        self.campo.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Posição que o próximo ArrowUp alcança. A consulta atual já está no topo do histórico (entrou ao buscar), então,
    /// sem navegação, a primeira subida pula essa.
    fn subida(&self) -> isize {
        if self.navegando < 0 && self.historico.first() == Some(&self.consulta) { 1 } else { self.navegando + 1 }
    }

    /// Troca o campo pela consulta do histórico (`i`) ou pelo rascunho (-1) e busca na hora, sem reordenar o histórico.
    fn lembrar(&mut self, i: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.navegando = i;
        let t = if i < 0 { self.digitado.clone() } else { self.historico[i as usize].clone() };
        self.trocar_consulta(&t, window, cx);
        self.relatorio = None;
        self.espera = None;
        self.executar(false, false, cx);
        self.agendar_previa(cx);
        cx.notify();
    }

    fn tecla_campo(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if k.modifiers.modified() {
            return;
        }
        match k.key.as_str() {
            "up" if (self.subida() as usize) < self.historico.len() => {
                if self.navegando < 0 {
                    self.digitado = self.consulta.clone();
                }
                let i = self.subida();
                self.lembrar(i, window, cx);
                cx.stop_propagation();
            }
            "down" if self.navegando >= 0 => {
                let i = self.navegando - 1;
                self.lembrar(i, window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn filtros(&self) -> Query {
        Query {
            text: self.consulta.clone(),
            case_sensitive: self.caixa,
            whole_word: self.palavra,
            regex: self.regex,
            include: logica::termos(&self.incluir),
            exclude: logica::termos(&self.excluir),
            only: self.so_abertos.then(|| self.abertos.clone()),
            preserve_case: self.preservar,
        }
    }

    // ---------- busca ----------

    fn buscar(&mut self, silencioso: bool, cx: &mut Context<Self>) {
        self.executar(silencioso, !silencioso, cx);
    }

    /// Uma busca. Consulta vazia limpa sem chamar o core. `silencioso` é a atualização depois de gravar: mantém
    /// recolhidos e dispensados.
    fn executar(&mut self, silencioso: bool, historico: bool, cx: &mut Context<Self>) {
        self.geracao += 1;
        let id = self.geracao;
        self.erro.clear();
        if self.consulta.is_empty() {
            self.busca = None;
            self.resultados = None;
            self.pendente = false;
            self.refazer(cx);
            return;
        }
        if historico {
            logica::lembrar(&mut self.historico, &self.consulta);
            self.salvar(cx);
        }
        let Some(raiz) = self.raiz.clone() else {
            self.erro = "Nenhum projeto aberto.".into();
            self.pendente = false;
            cx.notify();
            return;
        };
        self.pendente = true;
        self.girar(cx);
        let q = self.filtros();
        let tarefa = cx.background_spawn(async move {
            let padrao = search::compile(&q).ok();
            (search::search(&raiz, &q), padrao)
        });
        self.busca = Some(cx.spawn(async move |this, cx| {
            let (r, padrao) = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                if id != this.geracao {
                    return;
                }
                this.pendente = false;
                match r {
                    Ok(r) => {
                        if silencioso {
                            // Recolhido só sai se o arquivo sumiu da busca.
                            let ficam: HashSet<String> = r.files.iter().map(|f| logica::chave_arquivo(&f.path)).collect();
                            this.recolhidos.retain(|k| !k.starts_with("f:") || ficam.contains(k));
                        } else {
                            this.recolhidos.clear();
                            this.arquivos_fora.clear();
                            this.ocorrencias_fora.clear();
                        }
                        this.resultados = Some(Rc::new(r));
                        this.padrao = padrao;
                    }
                    Err(e) => {
                        this.resultados = None;
                        this.erro = e;
                    }
                }
                this.refazer(cx);
                this.agendar_previa(cx);
            });
        }));
        cx.notify();
    }

    /// Redesenha o Atualizar girando enquanto a busca roda.
    fn girar(&mut self, cx: &mut Context<Self>) {
        if self.giro.is_some() {
            return;
        }
        self.giro = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(orquestrador_ui::widgets::PASSO_GIRO).await;
            let segue = this
                .update(cx, |this, cx| {
                    this.passo_giro = (this.passo_giro + 1) % 30;
                    cx.notify();
                    if !this.pendente {
                        this.giro = None;
                    }
                    this.pendente
                })
                .unwrap_or(false);
            if !segue {
                break;
            }
        }));
    }

    fn salvar(&self, cx: &mut Context<Self>) {
        estado::gravar(&PreferenciaPesquisa { visao: self.visao, historico: self.historico.clone() }, cx);
    }

    /// Refaz os derivados da resposta (ocorrências, dispensados, linhas e substitutos).
    fn refazer(&mut self, cx: &mut Context<Self>) {
        self.todas = match &self.resultados {
            Some(r) => logica::todas(&r.files, self.padrao.as_ref()),
            None => Vec::new(),
        };
        self.refazer_visiveis();
        self.refazer_substitutos();
        cx.notify();
    }

    fn refazer_visiveis(&mut self) {
        let (a, p) = logica::visiveis(&self.todas, &self.arquivos_fora, &self.ocorrencias_fora);
        self.arquivos = a;
        self.parcial = p;
        self.total = self.arquivos.iter().map(|f| f.ocorrencias.len()).sum();
        self.refazer_linhas();
    }

    fn refazer_linhas(&mut self) {
        self.linhas = Rc::new(logica::linhas_da_tela(&self.arquivos, self.visao, &self.recolhidos));
    }

    // ---------- substituição ----------

    /// A prévia aparece com o Substituir aberto e algum texto nele (vazio apaga, mas só se vê ao confirmar).
    fn mostra_substituicao(&self) -> bool {
        self.substituir && !self.substituto.is_empty() && !self.consulta.is_empty()
    }

    fn previa_viva(&self) -> Option<&Previa> {
        let p = self.previa.as_ref()?;
        (self.mostra_substituicao() && p.consulta == self.filtros() && p.substituto == self.substituto).then_some(p)
    }

    /// A prévia acompanha os campos (com a mesma espera da busca) e cada resposta nova da busca.
    fn agendar_previa(&mut self, cx: &mut Context<Self>) {
        self.espera_previa = None;
        if !self.mostra_substituicao() {
            self.geracao_previa += 1;
            self.tarefa_previa = None;
            self.erro_previa.clear();
            self.refazer_substitutos();
            return;
        }
        self.espera_previa = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ESPERA).await;
            let _ = this.update(cx, |this, cx| {
                this.carregar_previa(None, cx);
            });
        }));
    }

    /// Pede a prévia para os campos atuais. `depois` roda com a prévia nova (Substituir Tudo sem prévia pronta).
    fn carregar_previa(&mut self, depois: Option<DepoisPrevia>, cx: &mut Context<Self>) {
        self.geracao_previa += 1;
        let id = self.geracao_previa;
        let Some(raiz) = self.raiz.clone() else { return };
        let q = self.filtros();
        let substituto = self.substituto.clone();
        let (q2, s2) = (q.clone(), substituto.clone());
        let tarefa = cx.background_spawn(async move { search::preview(&raiz, &q2, &s2) });
        self.tarefa_previa = Some(cx.spawn(async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                if id != this.geracao_previa {
                    return;
                }
                match r {
                    Ok(dados) => {
                        let p = Previa { consulta: q, substituto, dados: Rc::new(dados) };
                        this.previa = Some(p.clone());
                        this.erro_previa.clear();
                        this.refazer_substitutos();
                        if let Some(f) = depois {
                            f(this, p, cx);
                        }
                    }
                    Err(e) => {
                        this.previa = None;
                        this.erro_previa = e;
                        this.refazer_substitutos();
                    }
                }
                cx.notify();
            });
        }));
    }

    fn refazer_substitutos(&mut self) {
        self.substitutos.clear();
        let (Some(p), Some(re)) = (self.previa_viva(), self.padrao.as_ref()) else { return };
        let mut out = HashMap::new();
        for f in &p.dados.files {
            out.insert(f.path.clone(), logica::substitutos_arquivo(&f.original, &f.result, re));
        }
        self.substitutos = out;
    }

    /// O que entra no lugar da ocorrência, ou `None` quando a prévia não diz (arquivo fora dela ou linha ambígua).
    fn substituto_de(&self, o: &Ocorrencia) -> Option<&str> {
        o.trecho?;
        self.substitutos.get(&*o.caminho)?.get(&o.linha)?.as_ref()?.get(&o.col).map(String::as_str)
    }

    /// Substituir Tudo (Ctrl+Alt+Enter): com a prévia pronta, pede confirmação; sem ela, pede a prévia e confirma depois.
    pub fn substituir_tudo(&mut self, cx: &mut Context<Self>) {
        if self.consulta.is_empty() || self.aplicando || self.arquivos.is_empty() {
            return;
        }
        match self.previa_viva().cloned() {
            Some(p) => self.confirmando = Some(p),
            None => self.carregar_previa(Some(Box::new(|this, p, cx| {
                this.confirmando = Some(p);
                cx.notify();
            })), cx),
        }
        cx.notify();
    }

    fn substituir_arquivo(&mut self, caminho: String, cx: &mut Context<Self>) {
        if self.consulta.is_empty() || self.aplicando {
            return;
        }
        match self.previa_viva().cloned() {
            Some(p) => self.aplicar(vec![caminho], p, cx),
            None => self.carregar_previa(Some(Box::new(move |this, p, cx| this.aplicar(vec![caminho], p, cx))), cx),
        }
    }

    /// Arquivos da prévia que estão na tela, na ordem dela.
    fn arquivos_confirmar<'a>(&self, p: &'a Previa) -> Vec<&'a search::PreviewFile> {
        let vis: HashSet<&str> = self.arquivos.iter().map(|f| &*f.caminho).collect();
        p.dados.files.iter().filter(|f| vis.contains(f.path.as_str())).collect()
    }

    /// Grava a substituição nos arquivos dados. Arquivo com rascunho, com ocorrência dispensada ou fora da prévia fica de
    /// fora e é dito no relatório.
    fn aplicar(&mut self, caminhos: Vec<String>, p: Previa, cx: &mut Context<Self>) {
        let Some(raiz) = self.raiz.clone() else { return };
        let vis: HashSet<&str> = self.arquivos.iter().map(|f| &*f.caminho).collect();
        let mut fora = Vec::new();
        let mut escolhas = Vec::new();
        for f in p.dados.files.iter().filter(|f| vis.contains(f.path.as_str()) && caminhos.contains(&f.path)) {
            let por = if self.alterados.contains(&f.path) {
                "alteração não salva no editor"
            } else if self.parcial.contains(&f.path) {
                "ocorrência dispensada (a troca é do arquivo inteiro)"
            } else {
                ""
            };
            if por.is_empty() {
                escolhas.push(Pick { path: f.path.clone(), revision: f.revision.clone() });
            } else {
                fora.push(format!("{}: {por}", f.path));
            }
        }
        for c in &caminhos {
            if !p.dados.files.iter().any(|f| &f.path == c) {
                fora.push(format!("{c}: ficou fora da prévia (limite de arquivos)"));
            }
        }
        if escolhas.is_empty() {
            self.relatorio = Some(Relatorio { trocadas: 0, arquivos: 0, substituto: p.substituto, erros: vec![], fora });
            cx.notify();
            return;
        }
        self.aplicando = true;
        let pedido = ApplyRequest { query: p.consulta.clone(), replacement: p.substituto.clone(), files: escolhas };
        let tarefa = cx.background_spawn(async move { search::apply(&raiz, &pedido) });
        let substituto = p.substituto;
        self.aplicacao = Some(cx.spawn(async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                this.aplicando = false;
                this.relatorio = Some(match r {
                    Ok(feito) => {
                        if !feito.applied.is_empty() {
                            let caminhos = feito.applied.iter().map(|d| d.path.clone()).collect();
                            cx.emit(EventoPesquisa::ArquivosAlterados { caminhos });
                        }
                        Relatorio {
                            trocadas: feito.applied.iter().map(|d| d.count).sum(),
                            arquivos: feito.applied.len(),
                            substituto,
                            erros: feito.errors.into_iter().map(|e| (e.path, e.error)).collect(),
                            fora,
                        }
                    }
                    Err(e) => Relatorio { trocadas: 0, arquivos: 0, substituto, erros: vec![(String::new(), e)], fora },
                });
                this.buscar(true, cx);
            });
        }));
        cx.notify();
    }

    fn confirmar(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.confirmando.take() else { return };
        let caminhos = self.arquivos_confirmar(&p).iter().map(|f| f.path.clone()).collect();
        self.aplicar(caminhos, p, cx);
    }

    // ---------- resultados ----------

    fn marcar(&mut self, o: &Ocorrencia) {
        self.atual = Some((o.caminho.clone(), o.linha, o.col));
    }

    fn eh_atual(&self, o: &Ocorrencia) -> bool {
        self.atual.as_ref().is_some_and(|(c, l, col)| **c == *o.caminho && *l == o.linha && *col == o.col)
    }

    fn rolar_atual(&self) {
        let i = self.linhas.iter().position(|l| match l {
            Linha::Ocorrencia { arquivo, indice, .. } => self.eh_atual(&self.arquivos[*arquivo].ocorrencias[*indice]),
            _ => false,
        });
        if let Some(i) = i {
            self.rolagem.scroll_to_item(i, ScrollStrategy::Nearest);
        }
    }

    fn colapsaveis(&self) -> Vec<String> {
        match self.visao {
            Visao::Arvore => logica::linhas_da_tela(&self.arquivos, self.visao, &HashSet::new())
                .into_iter()
                .filter(|l| !matches!(l, Linha::Ocorrencia { .. }))
                .map(|l| l.chave().to_string())
                .collect(),
            Visao::Lista => self.arquivos.iter().map(|f| logica::chave_arquivo(&f.caminho)).collect(),
        }
    }

    fn algum_aberto(&self) -> bool {
        self.colapsaveis().iter().any(|k| !self.recolhidos.contains(k))
    }

    fn alternar_tudo(&mut self, cx: &mut Context<Self>) {
        if self.algum_aberto() {
            self.recolhidos = self.colapsaveis().into_iter().collect();
        } else {
            self.recolhidos.clear();
        }
        self.refazer_linhas();
        cx.notify();
    }

    fn alternar_linha(&mut self, chave: &str, cx: &mut Context<Self>) {
        if !self.recolhidos.remove(chave) {
            self.recolhidos.insert(chave.to_string());
        }
        self.refazer_linhas();
        cx.notify();
    }

    fn alternar_visao(&mut self, cx: &mut Context<Self>) {
        self.visao = if self.visao == Visao::Lista { Visao::Arvore } else { Visao::Lista };
        self.salvar(cx);
        self.refazer_linhas();
        cx.notify();
    }

    /// Dispensar só tira da tela. Com `indice` (clique ou tecla), o foco passa à vizinha.
    fn dispensar_ocorrencia(&mut self, chave: String, indice: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let vizinha = indice.and_then(|i| logica::vizinha(&self.linhas, i));
        self.ocorrencias_fora.insert(chave);
        self.depois_de_dispensar(indice.is_some(), vizinha, window, cx);
    }

    fn dispensar_arquivo(&mut self, caminho: String, indice: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let vizinha = indice.and_then(|i| logica::vizinha(&self.linhas, i));
        self.arquivos_fora.insert(caminho);
        self.depois_de_dispensar(indice.is_some(), vizinha, window, cx);
    }

    fn depois_de_dispensar(&mut self, mover: bool, vizinha: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.refazer_visiveis();
        self.refazer_substitutos();
        if mover {
            match vizinha.filter(|k| self.linhas.iter().any(|l| l.chave() == k)) {
                Some(k) => self.focar_linha(Some(k), window, cx),
                None => self.focar_linha(None, window, cx),
            }
        }
        cx.notify();
    }

    /// Foco do teclado numa linha (rolada para a vista); `None` devolve o foco ao campo.
    fn focar_linha(&mut self, chave: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        match chave {
            None => {
                self.linha_focada = None;
                self.campo.update(cx, |s, cx| s.focus(window, cx));
            }
            Some(k) => {
                if let Some(i) = self.linhas.iter().position(|l| l.chave() == k) {
                    self.rolagem.scroll_to_item(i, ScrollStrategy::Nearest);
                }
                self.linha_focada = Some(k);
                self.foco_lista.focus(window, cx);
            }
        }
        cx.notify();
    }

    fn indice_focado(&self) -> Option<usize> {
        let k = self.linha_focada.as_ref()?;
        self.linhas.iter().position(|l| l.chave() == k)
    }

    /// Setas sobem e descem; Início e Fim vão à primeira e à última; Enter abre; Espaço abre como prévia ou alterna;
    /// Delete dispensa.
    fn tecla_lista(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.linhas.len();
        if n == 0 || ev.keystroke.modifiers.modified() {
            return;
        }
        let i = self.indice_focado();
        let alvo = match (ev.keystroke.key.as_str(), i) {
            ("home", _) => Some(0),
            ("end", _) => Some(n - 1),
            ("down", Some(i)) => Some((i + 1).min(n - 1)),
            ("up", Some(i)) => Some(i.saturating_sub(1)),
            ("down", None) => Some(0),
            _ => None,
        };
        if let Some(a) = alvo {
            let k = self.linhas[a].chave().to_string();
            self.focar_linha(Some(k), window, cx);
            cx.stop_propagation();
            return;
        }
        let Some(i) = i else { return };
        let linha = self.linhas[i].clone();
        match (ev.keystroke.key.as_str(), &linha) {
            ("delete", Linha::Ocorrencia { chave, .. }) => self.dispensar_ocorrencia(chave.clone(), Some(i), window, cx),
            ("delete", Linha::Arquivo { arquivo, .. }) => {
                let c = self.arquivos[*arquivo].caminho.to_string();
                self.dispensar_arquivo(c, Some(i), window, cx);
            }
            ("enter" | "space", Linha::Ocorrencia { arquivo, indice, .. }) => {
                let o = self.arquivos[*arquivo].ocorrencias[*indice].clone();
                self.abrir(&o, ev.keystroke.key == "enter", cx);
            }
            ("enter" | "space", l) => {
                let k = l.chave().to_string();
                self.alternar_linha(&k, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn abrir(&mut self, o: &Ocorrencia, permanente: bool, cx: &mut Context<Self>) {
        self.marcar(o);
        cx.emit(EventoPesquisa::AbrirArquivo { caminho: o.caminho.to_string(), linha: o.linha, coluna: o.col, permanente });
        cx.notify();
    }

    fn copiar(texto: String, aviso: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(texto));
        cx.emit(EventoPesquisa::Aviso(aviso));
    }

    fn texto_tudo(&self) -> String {
        self.arquivos.iter().map(|f| logica::texto_bloco(&f.caminho, &f.ocorrencias)).collect::<Vec<_>>().join("\n\n")
    }

    // ---------- desenho ----------

    #[allow(clippy::too_many_arguments)]
    fn acao_cabecalho(
        &self,
        id: &'static str,
        caminho: &'static str,
        dica: &'static str,
        habilitada: bool,
        girar: bool,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut ic = icone(caminho, 16., cor::text_secondary());
        if girar {
            ic = ic.with_transformation(gpui_kit::Transformation::rotate(gpui_kit::percentage(self.passo_giro as f32 / 30.)));
        }
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .size(px(22.))
            .rounded(tema::RAIO_XS)
            .when(habilitada, |d| {
                d.hover(|s| s.bg(cor::fill_hover())).on_click(cx.listener(move |this, _, w, cx| f(this, w, cx)))
            })
            .when(!habilitada, |d| d.opacity(0.35))
            .tooltip_texto(dica)
            .child(ic)
            .into_any_element()
    }

    /// Botão de 20 px dentro do campo, com ícone de 16 px; ligado fica com fundo e borda.
    fn alternador(id: &'static str, caminho: &'static str, dica: &'static str, ligado: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .size(px(20.))
            .rounded(px(3.))
            .border_1()
            .border_color(if ligado { cor::focus_blue() } else { gpui_kit::transparent_black() })
            .when(ligado, |d| d.bg(rgba(0x007fd440)))
            .when(!ligado, |d| d.hover(|s| s.bg(cor::fill_hover())))
            .tooltip_texto(dica)
            .child(icone(caminho, 16., if ligado { cor::text_primary() } else { cor::text_secondary() }))
    }

    fn caixa_campo(campo: &Entity<InputState>, sufixo: Option<AnyElement>) -> gpui_kit::Div {
        div()
            .flex()
            .items_center()
            .h(px(26.))
            .min_w_0()
            .flex_1()
            .pl(px(0.))
            .pr(px(3.))
            .gap(px(2.))
            .rounded(tema::RAIO_SM)
            .bg(fundo_campo())
            .text_size(tema::TEXTO_MENU)
            .child(div().flex_1().min_w_0().child(Input::new(campo).appearance(false).h(px(24.))))
            .children(sufixo)
    }

    fn render_campos(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let opcoes = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                Self::alternador("op-caixa", icones::CAIXA, "Diferenciar maiúsculas de minúsculas", self.caixa)
                    .on_click(cx.listener(|this, _, _, cx| this.alternar(Opcao::Caixa, cx))),
            )
            .child(
                Self::alternador("op-palavra", icones::PALAVRA, "Palavra inteira", self.palavra)
                    .on_click(cx.listener(|this, _, _, cx| this.alternar(Opcao::Palavra, cx))),
            )
            .child(
                Self::alternador("op-regex", icones::REGEX, "Expressão regular", self.regex)
                    .on_click(cx.listener(|this, _, _, cx| this.alternar(Opcao::Regex, cx))),
            );
        let campo = div()
            .flex()
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| this.tecla_campo(ev, w, cx)))
            .child(Self::caixa_campo(&self.campo, Some(opcoes.into_any_element())));
        let substituir = self.substituir.then(|| {
            let pode = !self.consulta.is_empty() && !self.arquivos.is_empty() && !self.aplicando;
            div()
                .mt(px(6.))
                .flex()
                .items_center()
                .gap(px(4.))
                .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    let m = &ev.keystroke.modifiers;
                    if ev.keystroke.key == "enter" && m.control && m.alt {
                        this.substituir_tudo(cx);
                        cx.stop_propagation();
                    }
                }))
                .child(Self::caixa_campo(
                    &self.campo_substituir,
                    Some(
                        Self::alternador("op-preservar", icones::PRESERVAR, "Preservar Maiúsculas e Minúsculas", self.preservar)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.preservar = !this.preservar;
                                this.mudou(Modo::Nada, cx);
                            }))
                            .into_any_element(),
                    ),
                ))
                .child(
                    div()
                        .id("substituir-tudo")
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_none()
                        .size(px(24.))
                        .rounded(tema::RAIO_SM)
                        .when(pode, |d| {
                            d.hover(|s| s.bg(cor::fill_hover())).on_click(cx.listener(|this, _, _, cx| this.substituir_tudo(cx)))
                        })
                        .when(!pode, |d| d.opacity(0.35))
                        .tooltip_texto("Substituir Tudo (Ctrl+Alt+Enter)")
                        .child(icone(icones::SUBSTITUIR_TUDO, 16., cor::text_secondary())),
                )
        });
        let seta = div()
            .id("alternar-substituir")
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(3.))
            .hover(|s| s.bg(fill_terciario()))
            .tooltip_texto("Ativar/Desativar Substituição")
            .on_click(cx.listener(|this, _, _, cx| {
                this.substituir = !this.substituir;
                this.relatorio = None;
                this.agendar_previa(cx);
                cx.notify();
            }))
            .child(icone(if self.substituir { icones::SETA_ABERTA } else { icones::SETA }, 16., cor::text_secondary()));
        let rotulo = |t: &'static str| div().pt(px(4.)).text_size(px(11.)).line_height(px(18.)).truncate().child(t);
        let detalhes = self.detalhes.then(|| {
            div()
                .pb(px(4.))
                .child(rotulo("arquivos a serem incluídos"))
                .child(div().flex().child(Self::caixa_campo(
                    &self.campo_incluir,
                    Some(
                        Self::alternador(
                            "op-abertos",
                            icones::ABERTOS,
                            "Fazer a pesquisa somente nos Editores Abertos",
                            self.so_abertos,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.so_abertos = !this.so_abertos;
                            this.mudou(Modo::Agora, cx);
                        }))
                        .into_any_element(),
                    ),
                )))
                .child(rotulo("arquivos a serem excluídos"))
                .child(div().flex().child(Self::caixa_campo(&self.campo_excluir, None)))
        });
        div()
            .flex_none()
            .pt(px(6.))
            .pr(px(12.))
            .pb(px(6.))
            .pl(px(2.))
            .child(div().relative().pl(px(18.)).child(seta).child(campo).children(substituir))
            .child(
                div()
                    .relative()
                    .ml(px(18.))
                    .min_h(px(16.))
                    .child(
                        div()
                            .id("alternar-detalhes")
                            .absolute()
                            .right(px(-2.))
                            .top_0()
                            .h(px(16.))
                            .w(px(25.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .hover(|s| s.bg(fill_terciario()))
                            .tooltip_texto("Ativar/Desativar os Detalhes da Pesquisa")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.detalhes = !this.detalhes;
                                cx.notify();
                            }))
                            .child(icone(icones::DETALHES, 16., cor::text_secondary())),
                    )
                    .children(detalhes),
            )
    }

    fn render_mensagens(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut n = 0usize;
        let mut p = |t: String, c: Hsla| {
            n += 1;
            widgets::texto_pretty(("mensagem", n), t, tema::TEXTO_MENU, FontWeight::NORMAL, window, cx).text_color(c)
        };
        let status = self.status();
        let mut el = div()
            .flex_none()
            .mt(px(-5.))
            .pr(px(12.))
            .pb(px(4.))
            .pl(px(22.))
            .text_size(tema::TEXTO_MENU)
            .line_height(px(19.5))
            .text_color(cor::text_secondary());
        if !status.is_empty() {
            el = el.child(p(status, if self.erro.is_empty() { cor::text_secondary() } else { cor::signal_error() }));
        }
        if self.mostra_substituicao() && !self.erro_previa.is_empty() {
            el = el.child(p(self.erro_previa.clone(), cor::signal_error()));
        }
        if let Some(v) = self.previa_viva().filter(|v| v.dados.omitted_files > 0) {
            el = el.child(p(
                format!(
                    "{} fora da prévia (limite). Não serão alterados.",
                    logica::plural(v.dados.omitted_files, "arquivo com ocorrência ficou", "arquivos com ocorrência ficaram")
                ),
                cor::signal_warning(),
            ));
        }
        if let Some(r) = &self.relatorio {
            if r.arquivos > 0 {
                let por = if r.substituto.is_empty() { "nada (apagadas)".to_string() } else { format!("“{}”", r.substituto) };
                el = el.child(p(
                    format!(
                        "{} em {} por {por}.",
                        logica::plural(r.trocadas, "ocorrência substituída", "ocorrências substituídas"),
                        logica::plural(r.arquivos, "arquivo", "arquivos")
                    ),
                    cor::text_secondary(),
                ));
            }
            for (c, e) in &r.erros {
                el = el.child(p(if c.is_empty() { e.clone() } else { format!("{c}: {e}") }, cor::signal_error()));
            }
            if !r.fora.is_empty() {
                el = el.child(p(format!("Ficaram de fora: {}.", r.fora.join("; ")), cor::signal_warning()));
            }
        }
        if let Some(r) = self.resultados.clone().filter(|r| !r.skipped.is_empty()) {
            let n = r.skipped.len() + r.skipped_omitted;
            let aberto = self.fora_aberto;
            let mut d = div().text_size(tema::TEXTO_LABEL).child(
                div()
                    .id("fora")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.fora_aberto = !this.fora_aberto;
                        cx.notify();
                    }))
                    .child(div().text_size(px(9.)).child(if aberto { "▼" } else { "▶" }))
                    .child(format!("{} de fora", logica::plural(n, "arquivo ficou", "arquivos ficaram"))),
            );
            if aberto {
                for s in &r.skipped {
                    d = d.child(div().truncate().child(format!("{}: {}", s.path, logica::motivo(s.reason))));
                }
                if r.skipped_omitted > 0 {
                    d = d.child(format!("e mais {}", r.skipped_omitted));
                }
            }
            el = el.child(d);
        }
        el
    }

    fn render_linha(&self, i: usize, linha: &Linha, eu: &WeakEntity<Self>, cx: &mut Context<Self>) -> AnyElement {
        let focada = self.lista_com_foco && self.linha_focada.as_deref() == Some(linha.chave());
        let base = |id: SharedString| {
            div()
                .id(id)
                .group("linha")
                .relative()
                .flex()
                .items_center()
                .h(px(LINHA))
                .w_full()
                .pl(px(linha.nivel() as f32 * RECUO))
                .text_size(tema::TEXTO_MENU)
                .hover(|s| s.bg(fill_terciario()))
                .when(focada, |d| d.border_1().border_color(cor::focus_blue()))
        };
        let fechar = |id: &str| {
            div()
                .id(SharedString::from(format!("x-{id}")))
                .flex()
                .items_center()
                .justify_center()
                .size(px(20.))
                .rounded(tema::RAIO_XS)
                .hover(|s| s.bg(cor::fill_hover()))
                .tooltip_texto("Ignorar (Delete)")
                .child(icone(icones::FECHAR, 14., cor::text_secondary()))
        };
        match linha {
            Linha::Ocorrencia { chave, arquivo, indice, .. } => {
                let o = self.arquivos[*arquivo].ocorrencias[*indice].clone();
                let atual = self.eh_atual(&o);
                let riscar = self.mostra_substituicao() && o.trecho.is_some();
                let rep = if self.mostra_substituicao() { self.substituto_de(&o) } else { None };
                let (texto, destaques) = trecho_da_linha(&o, riscar, rep);
                let (c1, c2, c3) = (chave.clone(), o.clone(), o.clone());
                let caminho = o.caminho.to_string();
                let e1 = eu.clone();
                base(SharedString::from(format!("o-{chave}")))
                    .when(atual, |d| d.bg(fill_secundario()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .pl(px(30.))
                            .pr(px(8.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(StyledText::new(texto).with_highlights(destaques)),
                    )
                    .child(
                        div().invisible().group_hover("linha", |s| s.visible()).flex_none().pr(px(8.)).child(
                            fechar(chave).on_click(cx.listener(move |this, _, w, cx| {
                                cx.stop_propagation();
                                this.dispensar_ocorrencia(c1.clone(), Some(i), w, cx)
                            })),
                        ),
                    )
                    .on_mouse_down(MouseButton::Left, cx.listener(move |_, ev: &gpui_kit::MouseDownEvent, _, cx| {
                        if !ev.modifiers.modified() {
                            cx.emit(EventoPesquisa::PreLer { caminho: caminho.clone() });
                        }
                    }))
                    .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, w, cx| {
                        this.linha_focada = Some(c2.chave());
                        this.foco_lista.focus(w, cx);
                        this.abrir(&c2, ev.click_count() >= 2, cx);
                    }))
                    .context_menu(move |m: PopupMenu, _, _| {
                        let o = c3.clone();
                        let (a, b, c, d) = (e1.clone(), e1.clone(), e1.clone(), e1.clone());
                        let (t, p) = (o.texto.trim_start().to_string(), o.caminho.to_string());
                        let k = o.chave();
                        m.item(PopupMenuItem::new("Copiar").on_click(move |_, _, cx| {
                            let t = t.clone();
                            let _ = a.update(cx, |_, cx| Self::copiar(t, "Linha copiada".into(), cx));
                        }))
                        .item(PopupMenuItem::new("Copiar caminho").on_click(move |_, _, cx| {
                            let p = p.clone();
                            let _ = b.update(cx, |_, cx| Self::copiar(p, "Caminho copiado".into(), cx));
                        }))
                        .item(PopupMenuItem::new("Copiar tudo").on_click(move |_, _, cx| {
                            let _ = c.update(cx, |this, cx| {
                                let aviso = logica::plural(this.total, "resultado copiado", "resultados copiados");
                                Self::copiar(this.texto_tudo(), aviso, cx)
                            });
                        }))
                        .separator()
                        .item(PopupMenuItem::new("Dispensar").on_click(move |_, w, cx| {
                            let k = k.clone();
                            let _ = d.update(cx, |this, cx| this.dispensar_ocorrencia(k, None, w, cx));
                        }))
                    })
                    .into_any_element()
            }
            Linha::Pasta { chave, caminho, rotulo, total, .. } => {
                let aberta = !self.recolhidos.contains(chave);
                let k = chave.clone();
                base(SharedString::from(format!("p-{chave}")))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.linha_focada = Some(k.clone());
                        this.foco_lista.focus(w, cx);
                        this.alternar_linha(&k, cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .pl(px(8.))
                            .child(icone(if aberta { icones::SETA_ABERTA } else { icones::SETA }, 16., cor::text_secondary()))
                            .child(img(icone_arquivo(caminho, true, aberta)).size(px(16.)).flex_none().ml(px(6.)))
                            .child(div().ml(px(6.)).flex_1().min_w_0().truncate().child(rotulo.clone()))
                            .child(contador(*total)),
                    )
                    .into_any_element()
            }
            Linha::Arquivo { chave, arquivo, arvore, .. } => {
                let f = &self.arquivos[*arquivo];
                let caminho = f.caminho.to_string();
                let aberto = !self.recolhidos.contains(chave);
                let (pasta, nome) = logica::dividir(&caminho);
                let alterado = self.alterados.contains(&caminho);
                let (k, c1, c2, c3) = (chave.clone(), caminho.clone(), caminho.clone(), caminho.clone());
                let mostra = self.mostra_substituicao();
                let aplicando = self.aplicando;
                let ocorrencias = f.ocorrencias.clone();
                let e1 = eu.clone();
                base(SharedString::from(format!("a-{chave}")))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.linha_focada = Some(k.clone());
                        this.foco_lista.focus(w, cx);
                        this.alternar_linha(&k, cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .pl(px(8.))
                            .child(icone(if aberto { icones::SETA_ABERTA } else { icones::SETA }, 16., cor::text_secondary()))
                            .child(img(icone_arquivo(&caminho, false, false)).size(px(16.)).flex_none().ml(px(6.)))
                            .child(
                                div()
                                    .ml(px(6.))
                                    .flex()
                                    .flex_1()
                                    .min_w_0()
                                    .items_baseline()
                                    .child(div().flex_none().max_w(gpui_kit::relative(0.7)).truncate().child(nome.to_string()))
                                    .when(!pasta.is_empty() && !arvore, |d| {
                                        d.child(
                                            div()
                                                .ml(px(6.))
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(px(11.7))
                                                .text_color(cor::text_secondary())
                                                .child(pasta.to_string()),
                                        )
                                    }),
                            )
                            .when(alterado, |d| {
                                d.child(div().ml(px(4.)).flex_none().text_size(px(11.)).text_color(cor::signal_warning()).child("alterado"))
                            })
                            .child(div().group_hover("linha", |s| s.invisible()).child(contador(f.ocorrencias.len()))),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(8.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .invisible()
                            .group_hover("linha", |s| s.visible())
                            .when(mostra, |d| {
                                d.child(
                                    div()
                                        .id(SharedString::from(format!("r-{caminho}")))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(px(20.))
                                        .rounded(tema::RAIO_XS)
                                        .when(aplicando, |d| d.opacity(0.35))
                                        .hover(|s| s.bg(cor::fill_hover()))
                                        .tooltip_texto("Substituir Tudo")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.substituir_arquivo(c1.clone(), cx);
                                        }))
                                        .child(icone(icones::SUBSTITUIR_TUDO, 14., cor::text_secondary())),
                                )
                            })
                            .child(fechar(chave).on_click(cx.listener(move |this, _, w, cx| {
                                cx.stop_propagation();
                                this.dispensar_arquivo(c2.clone(), Some(i), w, cx);
                            }))),
                    )
                    .context_menu(move |m: PopupMenu, _, _| {
                        let (a, b, d) = (e1.clone(), e1.clone(), e1.clone());
                        let (p1, p2) = (c3.clone(), c3.clone());
                        let bloco = logica::texto_bloco(&c3, &ocorrencias);
                        m.item(PopupMenuItem::new("Copiar caminho").on_click(move |_, _, cx| {
                            let p = p1.clone();
                            let _ = a.update(cx, |_, cx| Self::copiar(p, "Caminho copiado".into(), cx));
                        }))
                        .item(PopupMenuItem::new("Copiar tudo").on_click(move |_, _, cx| {
                            let t = bloco.clone();
                            let _ = b.update(cx, |_, cx| Self::copiar(t, "Resultados do arquivo copiados".into(), cx));
                        }))
                        .separator()
                        .item(PopupMenuItem::new("Dispensar").on_click(move |_, w, cx| {
                            let p = p2.clone();
                            let _ = d.update(cx, |this, cx| this.dispensar_arquivo(p, None, w, cx));
                        }))
                    })
                    .into_any_element()
            }
        }
    }

    fn render_confirmar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let p = self.confirmando.clone()?;
        let arquivos = self.arquivos_confirmar(&p);
        let n: usize = arquivos.iter().map(|f| f.count).sum();
        let titulo = format!(
            "Substituir {} em {}?",
            logica::plural(n, "ocorrência", "ocorrências"),
            logica::plural(arquivos.len(), "arquivo", "arquivos")
        );
        let por = if p.substituto.is_empty() { "nada (apagar)".to_string() } else { format!("“{}”", p.substituto) };
        let corpo = format!(
            "Por {por}. As alterações vão para o disco; para voltar atrás, use o Git do projeto. Arquivos com alteração não salva ou com ocorrência dispensada ficam de fora."
        );
        let lista = div()
            .id("confirmar-lista")
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(180.))
            .overflow_y_scroll()
            .font_family(tema::FONTE_MONO)
            .font_weight(FontWeight::BOLD)
            .text_size(tema::TEXTO_LABEL)
            .children(
                arquivos
                    .iter()
                    .take(logica::LISTA_CONFIRMAR)
                    .map(|f| div().truncate().child(format!("{} ({})", f.path, f.count))),
            )
            .when(arquivos.len() > logica::LISTA_CONFIRMAR, |d| {
                d.child(
                    div()
                        .font_family(tema::FONTE)
                        .font_weight(FontWeight::NORMAL)
                        .text_color(cor::text_secondary())
                        .child(format!("e mais {}", arquivos.len() - logica::LISTA_CONFIRMAR)),
                )
            });
        let botao = |id: &'static str, rotulo: &'static str, perigo: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .h(tema::CAMPO)
                .px(px(12.))
                .rounded(tema::RAIO_SM)
                .text_size(tema::TEXTO_BODY)
                .font_weight(FontWeight::MEDIUM)
                .when(perigo, |d| d.bg(cor::signal_error()).text_color(cor::text_primary()).hover(|s| s.opacity(0.9)))
                .when(!perigo, |d| d.bg(cor::pitch_black()).text_color(cor::text_primary()).hover(|s| s.bg(cor::raised_black())))
                .child(rotulo)
        };
        // As partes do `Modal` do Lobe UI: cabeçalho de 48 px com o título centrado, conteúdo com 16 px dos lados e embaixo,
        // rodapé com 12 px em volta dos botões.
        let caixa = div()
            .id("confirmar")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(522.))
            .flex()
            .flex_col()
            .rounded(tema::RAIO_LG)
            .bg(cor::raised_black())
            .border_1()
            .border_color(rgba(0xffffff1f))
            .text_color(cor::text_primary())
            .font_family(tema::FONTE)
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_h(px(48.))
                    .px(px(16.))
                    .py(px(8.))
                    .text_size(tema::TEXTO_BODY)
                    .line_height(px(19.6))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(titulo),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .px(px(16.))
                    .pb(px(16.))
                    // Os espaços do DirectWrite saem um pouco mais largos que os do Chromium: a folga de 4 px sobre o recuo
                    // mantém as quebras de linha do React.
                    .child(
                        div()
                            .mr(px(-4.))
                            .line_height(px(22.))
                            .child(widgets::texto_pretty("confirmar-corpo", corpo, tema::TEXTO_BODY, FontWeight::NORMAL, window, cx)),
                    )
                    .child(lista),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(12.))
                    .child(botao("confirmar-cancelar", "Cancelar", false).on_click(cx.listener(|this, _, _, cx| {
                        this.confirmando = None;
                        cx.notify();
                    })))
                    .child(botao("confirmar-substituir", "Substituir", true).on_click(cx.listener(|this, _, _, cx| this.confirmar(cx)))),
            );
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    widgets::mascara("confirmar-mascara", window)
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                            if !this.aplicando {
                                this.confirmando = None;
                                cx.notify();
                            }
                        }))
                        .child(caixa),
                ),
            )
            .with_priority(1),
        )
    }
}

/// Badge com a contagem: pílula de 18 px, 11 px de texto.
fn contador(n: usize) -> impl IntoElement {
    div()
        .ml(px(4.))
        .mr(px(12.))
        .flex_none()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(fill_secundario())
        .text_size(px(11.))
        .child(n.to_string())
}

/// O texto da linha (antes cortado pelo `lcut`, ocorrência, substituto e depois) e os destaques de cada parte.
fn trecho_da_linha(o: &Ocorrencia, riscar: bool, rep: Option<&str>) -> (SharedString, Vec<(std::ops::Range<usize>, HighlightStyle)>) {
    let Some((a, b)) = o.trecho else {
        return (logica::lcut(&o.texto, logica::DEPOIS_MAX).into(), vec![]);
    };
    let antes = logica::lcut(&o.texto[..a], logica::ANTES_MAX);
    let dentro = &o.texto[a..b];
    let depois: String = o.texto[b..].chars().take(logica::DEPOIS_MAX).collect();
    let mut t = antes;
    let mut d = Vec::new();
    let i = t.len();
    t.push_str(dentro);
    let estilo = if riscar {
        HighlightStyle {
            background_color: Some(cor_removido()),
            strikethrough: Some(StrikethroughStyle { thickness: px(1.), color: None }),
            ..Default::default()
        }
    } else {
        HighlightStyle { background_color: Some(cor_achado()), ..Default::default() }
    };
    if !dentro.is_empty() {
        d.push((i..t.len(), estilo));
    }
    if let Some(r) = rep.filter(|_| riscar) {
        let j = t.len();
        t.push_str(r);
        if !r.is_empty() {
            d.push((j..t.len(), HighlightStyle { background_color: Some(cor_inserido()), ..Default::default() }));
        }
    }
    t.push_str(&depois);
    (t.into(), d)
}

impl Render for TelaPesquisa {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Sem foco na lista, nenhuma linha mostra a moldura do teclado.
        self.lista_com_foco = self.foco_lista.is_focused(window);
        let tem = !self.consulta.is_empty();
        let pode_limpar = tem
            || !self.substituto.is_empty()
            || !self.incluir.is_empty()
            || !self.excluir.is_empty()
            || self.resultados.is_some();
        let lista = self.visao == Visao::Lista;
        let aberto = self.algum_aberto();
        let girar = self.pendente;
        let cabecalho = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(36.))
            .px(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(tema::TEXTO_LABEL)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cor::text_secondary())
                    .child("Pesquisar"),
            )
            .child(self.acao_cabecalho("cab-atualizar", icones::ATUALIZAR, "Atualizar", tem, girar, |t, _, cx| t.agora(cx), cx))
            .child(self.acao_cabecalho(
                "cab-limpar",
                icones::LIMPAR,
                "Limpar os Resultados da Pesquisa",
                pode_limpar,
                false,
                |t, w, cx| t.limpar(w, cx),
                cx,
            ))
            .child(self.acao_cabecalho(
                "cab-visao",
                if lista { icones::ARVORE } else { icones::LISTA },
                if lista { "Exibir como Árvore" } else { "Exibir como Lista" },
                true,
                false,
                |t, _, cx| t.alternar_visao(cx),
                cx,
            ))
            .child(self.acao_cabecalho(
                "cab-recolher",
                if aberto { icones::RECOLHER } else { icones::EXPANDIR },
                if aberto { "Recolher Tudo" } else { "Expandir Tudo" },
                !self.arquivos.is_empty(),
                false,
                |t, _, cx| t.alternar_tudo(cx),
                cx,
            ));
        let linhas = self.linhas.clone();
        let eu = cx.weak_entity();
        let resultados = div()
            .id("resultados")
            .track_focus(&self.foco_lista)
            .flex_1()
            .min_h_0()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| this.tecla_lista(ev, w, cx)))
            .child(
                uniform_list(
                    "linhas",
                    linhas.len(),
                    cx.processor(move |this, faixa: std::ops::Range<usize>, _, cx| {
                        faixa.map(|i| this.render_linha(i, &linhas[i], &eu, cx)).collect::<Vec<_>>()
                    }),
                )
                .size_full()
                .track_scroll(&self.rolagem),
            );
        let campos = self.render_campos(cx);
        let mensagens = self.render_mensagens(window, cx);
        div()
            .id("pesquisa")
            .track_focus(&self.foco)
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .font_family(tema::FONTE)
            .text_color(cor::text_primary())
            .child(cabecalho)
            .child(campos)
            .child(mensagens)
            .child(resultados)
            .children(self.render_confirmar(window, cx))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                if this.confirmando.is_some() && ev.keystroke.key == "escape" && !this.aplicando {
                    this.confirmando = None;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext};

    fn pasta(tag: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("orq-pesquisa-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("src")).unwrap();
        std::fs::write(raiz.join("src/termo-a.ts"), "const termo = 1;
").unwrap();
        std::fs::write(raiz.join("src/termo-b.ts"), "let x = termo + termo;
").unwrap();
        raiz
    }

    fn montar(raiz: PathBuf, cx: &mut TestAppContext) -> (Entity<TelaPesquisa>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            orquestrador_ui::tema::aplicar(cx);
        });
        let (tela, vcx) = cx.add_window_view(|window, cx| TelaPesquisa::new(Some(raiz), window, cx));
        (tela, vcx)
    }

    fn esperar(vcx: &mut VisualTestContext) {
        for _ in 0..50 {
            // Os temporizadores (espera da digitação e da prévia) andam no relógio simulado.
            vcx.executor().advance_clock(ESPERA);
            vcx.run_until_parked();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[gpui_kit::test]
    fn busca_dispensa_f4_e_substitui(cx: &mut TestAppContext) {
        let raiz = pasta("fluxo");
        let (tela, vcx) = montar(raiz.clone(), cx);
        let eventos = Rc::new(std::cell::RefCell::new(Vec::new()));
        let e2 = eventos.clone();
        vcx.update(|_, cx| {
            cx.subscribe(&tela, move |_, ev: &EventoPesquisa, _| e2.borrow_mut().push(ev.clone())).detach();
        });
        vcx.update(|w, cx| tela.update(cx, |t, cx| t.semear("termo", w, cx)));
        esperar(vcx);
        vcx.update(|_, cx| assert_eq!(tela.read(cx).status(), "3 resultados em 2 arquivos"));

        // F4 vai à primeira ocorrência; Shift+F4 dá a volta à última.
        vcx.update(|_, cx| tela.update(cx, |t, cx| t.passo(1, cx)));
        vcx.update(|_, cx| tela.update(cx, |t, cx| t.passo(-1, cx)));
        let abertos: Vec<_> = eventos
            .borrow()
            .iter()
            .filter_map(|e| match e {
                EventoPesquisa::AbrirArquivo { caminho, linha, coluna, .. } => Some((caminho.clone(), *linha, *coluna)),
                _ => None,
            })
            .collect();
        assert_eq!(abertos, vec![("src/termo-a.ts".into(), 1, 7), ("src/termo-b.ts".into(), 1, 17)]);

        // Prévia: cada ocorrência com o substituto; nada gravado.
        vcx.update(|w, cx| tela.update(cx, |t, cx| t.definir_substituicao("novo", w, cx)));
        esperar(vcx);
        vcx.update(|_, cx| {
            let t = tela.read(cx);
            let subs: Vec<_> = t.arquivos.iter().flat_map(|f| f.ocorrencias.iter()).map(|o| t.substituto_de(o).map(str::to_string)).collect();
            assert_eq!(subs, vec![Some("novo".into()); 3]);
        });
        assert_eq!(std::fs::read_to_string(raiz.join("src/termo-a.ts")).unwrap(), "const termo = 1;
");

        // Rascunho em termo-b barra a troca dele; termo-a é gravado.
        vcx.update(|_, cx| tela.update(cx, |t, cx| t.definir_alterados(["src/termo-b.ts".to_string()].into(), cx)));
        vcx.update(|_, cx| tela.update(cx, |t, cx| t.substituir_tudo(cx)));
        vcx.update(|_, cx| tela.update(cx, |t, cx| t.confirmar(cx)));
        esperar(vcx);
        assert_eq!(std::fs::read_to_string(raiz.join("src/termo-a.ts")).unwrap(), "const novo = 1;
");
        assert_eq!(std::fs::read_to_string(raiz.join("src/termo-b.ts")).unwrap(), "let x = termo + termo;
");
        vcx.update(|_, cx| {
            let t = tela.read(cx);
            let r = t.relatorio.as_ref().unwrap();
            assert_eq!((r.trocadas, r.arquivos), (1, 1));
            assert_eq!(r.fora, vec!["src/termo-b.ts: alteração não salva no editor".to_string()]);
            assert_eq!(t.status(), "2 resultados em 1 arquivo");
        });
        assert!(eventos.borrow().iter().any(|e| matches!(e, EventoPesquisa::ArquivosAlterados { .. })));
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[gpui_kit::test]
    fn historico_e_dispensar(cx: &mut TestAppContext) {
        let raiz = pasta("hist");
        let (tela, vcx) = montar(raiz.clone(), cx);
        vcx.update(|w, cx| tela.update(cx, |t, cx| t.semear("const", w, cx)));
        esperar(vcx);
        vcx.update(|w, cx| tela.update(cx, |t, cx| t.semear("termo", w, cx)));
        esperar(vcx);
        // ArrowUp traz a anterior (não a atual) e busca; ArrowDown volta.
        vcx.update(|w, cx| tela.update(cx, |t, cx| {
            assert_eq!(t.subida(), 1);
            t.digitado = t.consulta.clone();
            t.lembrar(1, w, cx);
        }));
        esperar(vcx);
        vcx.update(|_, cx| assert_eq!((tela.read(cx).consulta(), tela.read(cx).status()), ("const", "1 resultado em 1 arquivo".into())));
        vcx.update(|w, cx| tela.update(cx, |t, cx| t.lembrar(0, w, cx)));
        esperar(vcx);
        vcx.update(|_, cx| assert_eq!(tela.read(cx).status(), "3 resultados em 2 arquivos"));
        // Dispensar ocorrência e arquivo só tira da tela.
        vcx.update(|w, cx| tela.update(cx, |t, cx| {
            let k = logica::chave_ocorrencia("src/termo-b.ts", 1, 17);
            t.dispensar_ocorrencia(k, None, w, cx);
            assert_eq!(t.status(), "2 resultados em 2 arquivos");
            assert!(t.parcial.contains("src/termo-b.ts"));
            t.dispensar_arquivo("src/termo-b.ts".into(), None, w, cx);
            assert_eq!(t.status(), "1 resultado em 1 arquivo");
            // Árvore: "src" com a contagem de dentro; recolher tira os arquivos.
            t.alternar_visao(cx);
            assert!(matches!(&t.linhas[0], Linha::Pasta { rotulo, total: 1, .. } if rotulo == "src"));
            t.alternar_linha("d:src", cx);
            assert_eq!(t.linhas.len(), 1);
        }));
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
