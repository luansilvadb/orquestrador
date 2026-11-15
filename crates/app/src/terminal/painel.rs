//! Os painéis de terminal (porte de terminal-screen.tsx, shell-panel.tsx e da parte de terminais do App.tsx): o
//! Orquestrador à direita, com um executor por aba, e o painel Terminal embaixo, com uma aba por shell. Cada projeto tem o
//! seu estado, e nenhum terminal é destruído ao trocar de tela ou de projeto.
use super::escolha::{self, EventoEscolha, FormEscolha};
use super::motor::{chave, Chave, TAMANHO_EXECUTOR};
use super::ui::{self, Variante};
use super::vista::{EventoVista, VistaTerminal};
use super::colar::Cli;
use crate::backend::Backend;
use orquestrador_ui::widgets::TooltipTexto;
use orquestrador_ui::{Aba, EstadoFaixa, EstadoLista, FaixaAbas, ItemLista, ListaAbas, MoverAba};
use crate::tema::{self, cor};
use gpui_kit::TestSupportExt;
use gpui_kit::{
    canvas, div, point, prelude::*, px, AnyElement, App, Context, Entity, EventEmitter,
    FontWeight, Hsla, MouseButton, SharedString, Subscription, Task, WeakEntity, Window,
};
use orquestrador_core::config::{Choice, Options};
use orquestrador_core::core::ORCHESTRATOR;
use serde_json::Value;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// Abaixo desta largura do painel Terminal a faixa de abas vira um seletor do terminal ativo (shell-panel.tsx).
const LARGURA_COMPACTA: f32 = 460.;

/// Estado do executor: rótulo, ícone e cor (a cor predefinida do Lobe UI, a mesma no Tag e no ícone da aba).
pub fn estado_info(estado: &str) -> (&'static str, &'static str, Hsla) {
    match estado {
        "awaiting-choice" => ("aguardando escolha", "icons/circle-question-mark.svg", gpui_kit::rgb(0xd89614).into()),
        "queued" => ("na fila", "icons/clock.svg", gpui_kit::rgb(0x8c8c8c).into()),
        "running" => ("executando", "icons/loader-circle.svg", gpui_kit::rgb(0x3c89e8).into()),
        "done" => ("concluído", "icons/circle-check.svg", gpui_kit::rgb(0x6abe39).into()),
        "stopped" => ("interrompido", "icons/square.svg", gpui_kit::rgb(0xe87a1a).into()),
        _ => ("falhou", "icons/circle-x.svg", gpui_kit::rgb(0xe84749).into()),
    }
}

pub fn finalizado(estado: &str) -> bool {
    matches!(estado, "done" | "stopped" | "failed")
}

fn cancelavel(estado: &str) -> bool {
    matches!(estado, "queued" | "running")
}

/// Texto da camada que cobre o terminal do executor, por estado.
fn texto_da_capa(estado: &str) -> Option<&'static str> {
    match estado {
        "awaiting-choice" => Some("Aguardando a escolha dos executores."),
        "queued" => Some("Na fila: começa automaticamente quando houver capacidade, sem ajuste manual."),
        _ => None,
    }
}

fn icone_da_ferramenta(tool: &str) -> &'static str {
    match tool {
        "codex" => "icons/codex.svg",
        _ => "icons/claude.svg",
    }
}

/// Texto completo da escolha, para o tooltip.
pub fn rotulo_da_escolha(c: &Choice) -> String {
    let mut partes = vec![escolha::nome_da_ferramenta(&c.tool).to_string()];
    partes.push(if c.model.is_empty() { "padrão do CLI".into() } else { c.model.clone() });
    if !c.effort.is_empty() {
        partes.push(c.effort.clone());
    }
    partes.join(" · ")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Executor {
    pub id: String,
    pub titulo: String,
    pub estado: String,
    pub escolha: Option<Choice>,
    /// O número exibido: o menor livre entre os executores abertos ao chegar.
    pub n: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sobre {
    Inicio,
    Executores,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Orquestrador {
    pub escolha: Option<Choice>,
    pub sobre: Option<Sobre>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shell {
    pub id: String,
    pub n: usize,
    /// '' até o shell responder; nasce com o último programa aberto.
    pub programa: String,
    /// Pasta relativa ao projeto em que o shell abre ("Abrir no Terminal" da árvore); `None` é a raiz.
    pub cwd: Option<String>,
}

#[derive(Default)]
pub struct Projeto {
    pub orquestrador: Option<Orquestrador>,
    pub executores: Vec<Executor>,
    pub escolha_executores: Option<Choice>,
    pub aviso: String,
    /// A aba escolhida no painel do Orquestrador; vazia é o orquestrador.
    pub aba: String,
    pub opcoes: Option<Options>,
    pub sem_git: Option<&'static str>,
    pub shells: Vec<Shell>,
    pub shell_ativa: String,
}

impl Projeto {
    /// Aba de executor que sumiu volta para o orquestrador.
    pub fn aba_ativa(&self) -> String {
        if self.executores.iter().any(|e| e.id == self.aba) {
            self.aba.clone()
        } else {
            ORCHESTRATOR.to_string()
        }
    }
}

pub enum EventoTerminais {
    /// A última aba de shell fechou: o painel Terminal se esconde.
    FecharPainel,
}

pub struct Terminais {
    backend: Rc<Backend>,
    pub projetos: HashMap<Option<String>, Projeto>,
    pub vistas: HashMap<Chave, Entity<VistaTerminal>>,
    form: Option<(Option<String>, Entity<FormEscolha>, Subscription)>,
    lista_orq: EstadoLista,
    pub(crate) lista_shells: EstadoLista,
    pub(crate) faixa_shells: EstadoFaixa,
    faixa_orq: EstadoFaixa,
    largura_cabecalho: Rc<Cell<f32>>,
    seq_shell: u64,
    ultimo_programa: String,
    assinaturas: HashMap<Chave, Subscription>,
    /// O portão de abertura: sem ele liberado, nenhum terminal abre (o `gated` do Tauri).
    pub abertura: Entity<super::portao::Abertura>,
    /// A janela, guardada na primeira ação com ela: o que chega do core (sem janela) abre o formulário e avisa erros por ela.
    janela: Option<gpui_kit::AnyWindowHandle>,
    _tarefas: Vec<Task<()>>,
}

impl EventEmitter<EventoTerminais> for Terminais {}

impl Terminais {
    pub fn new(backend: Rc<Backend>, cx: &mut Context<Self>) -> Self {
        let avisos = backend.avisos.clone();
        // Saída nova: só o terminal visível redesenha, uma vez por quadro (o motor só avisa de novo depois do desenho).
        let tarefa = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            while let Ok(chave) = avisos.recv().await {
                let ok = this.update(cx, |this, cx| {
                    if let Some(v) = this.vistas.get(&chave) {
                        if v.read(cx).visivel {
                            v.update(cx, |_, cx| cx.notify());
                        }
                    }
                });
                if ok.is_err() {
                    break;
                }
            }
        });
        let exes = backend.projetos.exes().clone();
        let abertura = cx.new(|cx| super::portao::Abertura::new(exes, cx));
        cx.observe(&abertura, |_, _, cx| cx.notify()).detach();
        // Giro dos executores rodando: o painel redesenha a cada passo, e só enquanto há algum.
        let giro = cx.spawn(async move |this: WeakEntity<Self>, cx| loop {
            cx.background_executor().timer(ui::PASSO_GIRO).await;
            let ok = this.update(cx, |this, cx| {
                if this.projetos.values().any(|p| p.executores.iter().any(|e| e.estado == "running")) {
                    cx.notify();
                }
            });
            if ok.is_err() {
                break;
            }
        });
        Terminais {
            janela: None,
            abertura,
            backend,
            projetos: HashMap::new(),
            vistas: HashMap::new(),
            form: None,
            lista_orq: EstadoLista::new(cx),
            lista_shells: EstadoLista::new(cx),
            faixa_shells: EstadoFaixa::new(),
            faixa_orq: EstadoFaixa::new(),
            largura_cabecalho: Rc::new(Cell::new(f32::MAX)),
            seq_shell: 0,
            ultimo_programa: String::new(),
            assinaturas: HashMap::new(),
            _tarefas: vec![tarefa, giro],
        }
        .com_avisos(cx)
    }

    /// Erros dos terminais viram o aviso de erro do Kit (o toast do React).
    fn com_avisos(self, cx: &mut Context<Self>) -> Self {
        cx.set_global(super::vista::AvisarErro(Some(Rc::new(|msg, _window, cx| {
            orquestrador_ui::avisos::mostrar(orquestrador_ui::avisos::Aviso::erro(msg, 7000), cx)
        }))));
        self
    }

    pub fn projeto(&mut self, p: &Option<String>) -> &mut Projeto {
        self.projetos.entry(p.clone()).or_default()
    }

    fn vista(&mut self, k: &Chave, escrevivel: bool, raiz: Option<String>, cx: &mut Context<Self>) -> Entity<VistaTerminal> {
        if let Some(v) = self.vistas.get(k) {
            return v.clone();
        }
        let (c, l) = if escrevivel { (80, 24) } else { TAMANHO_EXECUTOR };
        let motor = self.backend.terminais.motor(k, c, l);
        let (registro, projetos) = (self.backend.terminais.clone(), self.backend.projetos.clone());
        let v = cx.new(|cx| VistaTerminal::new(registro, projetos, motor, escrevivel, raiz, cx));
        let assinatura = cx.subscribe(&v, |this, v, e: &EventoVista, cx| match e {
            EventoVista::Medido => this.medido(v, cx),
        });
        self.assinaturas.insert(k.clone(), assinatura);
        self.vistas.insert(k.clone(), v.clone());
        v
    }

    fn largar_vista(&mut self, k: &Chave, fim: bool) {
        self.vistas.remove(k);
        self.assinaturas.remove(k);
        self.backend.terminais.remover(k, fim);
    }

    // ---------- eventos do core ----------

    pub fn receber(&mut self, nome: &str, payload: &Value, cx: &mut Context<Self>) {
        let projeto = payload.get("project").and_then(Value::as_str).map(str::to_string);
        let texto = |k: &str| payload.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let escolha = || payload.get("choice").and_then(|c| serde_json::from_value::<Choice>(c.clone()).ok());
        match nome {
            "node-added" => {
                let id = texto("id");
                let estado = texto("state");
                let p = self.projeto(&projeto);
                if p.orquestrador.is_none() || p.executores.iter().any(|e| e.id == id) {
                    return;
                }
                let mut n = 1;
                while p.executores.iter().any(|e| e.n == n) {
                    n += 1;
                }
                p.executores.push(Executor { id: id.clone(), titulo: texto("title"), estado, escolha: escolha(), n });
                // O executor nasce no tamanho do painel, como o TermHost escondido que já tem o tamanho do contêiner.
                let k = chave(projeto.as_deref(), &id);
                let tamanho = self.vistas.get(&chave(projeto.as_deref(), ORCHESTRATOR)).map(|v| v.read(cx).tamanho());
                let raiz = self.vistas.get(&chave(projeto.as_deref(), ORCHESTRATOR)).and_then(|v| v.read(cx).raiz.clone());
                let v = self.vista(&k, false, raiz, cx);
                v.update(cx, |v, cx| {
                    if let Some((c, l)) = tamanho {
                        v.motor.redimensionar(c, l);
                    }
                    v.ligar(cx);
                });
                cx.notify();
            }
            "node-state" => {
                let id = texto("id");
                let estado = texto("state");
                let k = chave(projeto.as_deref(), &id);
                let p = self.projeto(&projeto);
                let Some(i) = p.executores.iter().position(|e| e.id == id) else { return };
                if finalizado(&estado) {
                    // Executor que terminou fecha a própria aba; a resposta dele já seguiu para o orquestrador.
                    p.executores.remove(i);
                    self.largar_vista(&k, true);
                } else {
                    p.executores[i].estado = estado.clone();
                    p.executores[i].escolha = escolha();
                    if estado == "running" {
                        if let Some(v) = self.vistas.get(&k) {
                            v.update(cx, |v, cx| v.ligar(cx));
                        }
                    }
                }
                cx.notify();
            }
            "orchestrator-exit" => {
                let codigo = payload.get("code").and_then(Value::as_i64);
                let aviso = match codigo {
                    Some(c) => format!("O orquestrador terminou (código de saída {c})."),
                    None => "O orquestrador terminou (sem código de saída).".into(),
                };
                self.reiniciar_orquestrador(&projeto, aviso, cx);
            }
            "shell-exit" => {
                let id = texto("id");
                if self.projeto(&projeto).shells.iter().any(|s| s.id == id) {
                    self.largar_shell(&projeto, &id, cx);
                }
            }
            _ => {}
        }
    }

    fn reiniciar_orquestrador(&mut self, projeto: &Option<String>, aviso: String, cx: &mut Context<Self>) {
        let p = self.projeto(projeto);
        let executores: Vec<String> = p.executores.drain(..).map(|e| e.id).collect();
        p.orquestrador = None;
        p.escolha_executores = None;
        p.aviso = aviso;
        for id in executores {
            self.largar_vista(&chave(projeto.as_deref(), &id), true);
        }
        if self.form.as_ref().is_some_and(|(fp, _, _)| fp == projeto) {
            self.form = None;
        }
        cx.notify();
    }

    /// O projeto fechou: o core já encerrou os terminais dele.
    pub fn fechar_projeto(&mut self, id: &str, cx: &mut Context<Self>) {
        let p = Some(id.to_string());
        self.projetos.remove(&p);
        self.vistas.retain(|(proj, _), _| proj.as_deref() != Some(id));
        self.assinaturas.retain(|(proj, _), _| proj.as_deref() != Some(id));
        self.backend.terminais.remover_projeto(id);
        if self.form.as_ref().is_some_and(|(fp, _, _)| *fp == p) {
            self.form = None;
        }
        cx.notify();
    }

    // ---------- orquestrador ----------

    pub fn pronto(&self, cx: &App) -> bool {
        self.abertura.read(cx).pronto()
    }

    pub fn abrir_orquestrador(&mut self, projeto: Option<String>, raiz: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pronto(cx) {
            return;
        }
        self.janela = Some(window.window_handle());
        if self.projeto(&projeto).orquestrador.is_some() {
            return;
        }
        let projetos = self.backend.projetos.clone();
        let repo = raiz.clone();
        let tarefa = cx.background_spawn(async move {
            let opcoes = projetos.options();
            let sem_git = repo.as_deref().and_then(|r| sem_git(std::path::Path::new(r)));
            (opcoes, sem_git)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (opcoes, sem_git) = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| match opcoes {
                Ok(opcoes) => {
                    let p = this.projeto(&projeto);
                    p.opcoes = Some(opcoes.clone());
                    p.aviso.clear();
                    p.sem_git = sem_git;
                    p.orquestrador = Some(Orquestrador { escolha: None, sobre: Some(Sobre::Inicio) });
                    p.aba = ORCHESTRATOR.into();
                    // Sessão nova: terminal novo no lugar do anterior.
                    let k = chave(projeto.as_deref(), ORCHESTRATOR);
                    let motor = this.backend.terminais.recriar(&k, 80, 24);
                    let v = this.vista(&k, true, raiz.clone(), cx);
                    v.update(cx, |v, cx| {
                        v.vivo = false;
                        v.raiz = raiz.clone();
                        v.trocar_motor(motor, cx);
                    });
                    this.abrir_form(projeto.clone(), Sobre::Inicio, window, cx);
                }
                Err(e) => super::vista::avisar(e, window, cx),
            });
        })
        .detach();
    }

    fn abrir_form(&mut self, projeto: Option<String>, sobre: Sobre, window: &mut Window, cx: &mut Context<Self>) {
        let p = self.projeto(&projeto);
        let Some(opcoes) = p.opcoes.clone() else { return };
        let aguardando = p.executores.iter().any(|e| e.estado == "awaiting-choice");
        let (titulo, dicas, inicial, rotulo, cancelavel): (&str, Vec<SharedString>, Choice, &str, bool) = match sobre {
            Sobre::Inicio => {
                let mut dicas: Vec<SharedString> = vec!["Só lê o projeto e delega. Você conversa com ele neste terminal.".into()];
                if let Some(fix) = p.sem_git {
                    dicas.push(
                        format!("Esta pasta não tem Git: o que os executores mudarem não fica registrado e não pode ser desfeito pelo Git. {fix}").into(),
                    );
                }
                ("Orquestrador", dicas, opcoes.last.orchestrator.clone(), "Abrir terminal", true)
            }
            Sobre::Executores if aguardando => (
                "Escolha dos executores",
                vec!["Vale para este e para os próximos executores enquanto o orquestrador estiver aberto.".into()],
                p.escolha_executores.clone().unwrap_or(opcoes.last.executor.clone()),
                "Usar nos executores",
                false,
            ),
            Sobre::Executores => (
                "Executores",
                vec!["Ferramenta, modelo e esforço dos próximos executores. Os que já estão rodando não mudam.".into()],
                p.escolha_executores.clone().unwrap_or(opcoes.last.executor.clone()),
                "Usar nos executores",
                true,
            ),
        };
        if let Some(o) = p.orquestrador.as_mut() {
            o.sobre = Some(sobre);
        }
        let form = cx.new(|cx| FormEscolha::new(titulo, dicas, opcoes, &inicial, rotulo, cancelavel, cx));
        let proj = projeto.clone();
        let assinatura = cx.subscribe_in(&form, window, move |this, form, e: &EventoEscolha, window, cx| match e {
            EventoEscolha::Confirmar(c) => this.confirmar(proj.clone(), sobre, c.clone(), form.clone(), window, cx),
            EventoEscolha::Cancelar => this.cancelar(proj.clone(), sobre, window, cx),
        });
        let foco = form.read(cx).foco.clone();
        window.focus(&foco, cx);
        self.form = Some((projeto, form, assinatura));
        cx.notify();
    }

    fn fechar_form(&mut self, projeto: &Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        if let Some(o) = self.projeto(projeto).orquestrador.as_mut() {
            o.sobre = None;
        }
        // O foco não volta ao botão que abriu: vai ao terminal do orquestrador.
        self.focar(&chave(projeto.as_deref(), ORCHESTRATOR), window, cx);
        cx.notify();
    }

    fn cancelar(&mut self, projeto: Option<String>, sobre: Sobre, window: &mut Window, cx: &mut Context<Self>) {
        match sobre {
            Sobre::Inicio => {
                self.form = None;
                self.reiniciar_orquestrador(&projeto, String::new(), cx);
                self.largar_vista(&chave(projeto.as_deref(), ORCHESTRATOR), false);
            }
            Sobre::Executores => self.fechar_form(&projeto, window, cx),
        }
    }

    fn confirmar(
        &mut self, projeto: Option<String>, sobre: Sobre, escolha: Choice, form: Entity<FormEscolha>, window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projetos = self.backend.projetos.clone();
        let Some(id) = projeto.clone() else { return };
        let k = chave(projeto.as_deref(), ORCHESTRATOR);
        let tamanho = self.vistas.get(&k).map(|v| v.read(cx).tamanho()).unwrap_or((80, 24));
        let c = escolha.clone();
        let tarefa = cx.background_spawn(async move {
            match sobre {
                Sobre::Inicio => projetos.core(&id)?.start_orchestrator(c, tamanho.0, tamanho.1),
                Sobre::Executores => projetos.set_executor_choice(&id, c),
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| match r {
                Err(e) => form.update(cx, |f, cx| f.falhou(e, cx)),
                Ok(()) => {
                    match sobre {
                        Sobre::Inicio => {
                            if let Some(o) = this.projeto(&projeto).orquestrador.as_mut() {
                                o.escolha = Some(escolha.clone());
                            }
                            if let Some(v) = this.vistas.get(&k) {
                                v.update(cx, |v, cx| {
                                    v.cli = Cli::de(&escolha.tool);
                                    v.ligar(cx);
                                });
                            }
                        }
                        Sobre::Executores => this.projeto(&projeto).escolha_executores = Some(escolha.clone()),
                    }
                    this.fechar_form(&projeto, window, cx);
                }
            });
        })
        .detach();
    }

    fn configurar_executores(&mut self, projeto: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.is_some() {
            return;
        }
        let projetos = self.backend.projetos.clone();
        let tarefa = cx.background_spawn(async move { projetos.options() });
        cx.spawn_in(window, async move |this, cx| {
            let opcoes = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Ok(o) = opcoes {
                    this.projeto(&projeto).opcoes = Some(o);
                }
                this.abrir_form(projeto.clone(), Sobre::Executores, window, cx);
            });
        })
        .detach();
    }

    fn parar(&mut self, projeto: Option<String>, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let projetos = self.backend.projetos.clone();
        let tarefa = cx.background_spawn(async move {
            projetos.core(projeto.as_deref().unwrap_or_default())?.stop_executor(&id)
        });
        cx.spawn_in(window, async move |_, cx| {
            if let Err(e) = tarefa.await {
                let _ = cx.update(|window, cx| super::vista::avisar(e, window, cx));
            }
        })
        .detach();
    }

    fn escolher_aba(&mut self, projeto: &Option<String>, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.projeto(projeto).aba = id.clone();
        if id == ORCHESTRATOR {
            self.focar(&chave(projeto.as_deref(), ORCHESTRATOR), window, cx);
        }
        self.lista_orq.fechar();
        cx.notify();
    }

    pub fn focar(&self, k: &Chave, window: &mut Window, cx: &mut App) {
        if let Some(v) = self.vistas.get(k) {
            let foco = v.read(cx).foco.clone();
            window.defer(cx, move |window, cx| window.focus(&foco, cx));
        }
    }

    /// O foco está num terminal (os atalhos de projeto deixam a tecla seguir para ele, como o `.xterm` do React).
    pub fn foco_em_terminal(&self, window: &Window, cx: &App) -> bool {
        self.vistas.values().any(|v| v.read(cx).foco.is_focused(window))
    }

    /// Cola o caminho absoluto no rascunho do orquestrador, sem Enter ("Adicionar ao Chat" da árvore). `false` sem
    /// orquestrador aberto.
    pub fn colar_no_orquestrador(&mut self, projeto: Option<String>, caminho: String, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let k = chave(projeto.as_deref(), ORCHESTRATOR);
        let aberto = self.projeto(&projeto).orquestrador.as_ref().is_some_and(|o| o.escolha.is_some());
        match self.vistas.get(&k).cloned() {
            Some(v) if aberto => {
                v.update(cx, |v, cx| v.colar_caminhos(vec![caminho], window, cx));
                self.focar(&k, window, cx);
                true
            }
            _ => false,
        }
    }

    // ---------- shells ----------

    /// Nova aba de shell: id novo e ativa. O shell abre quando o terminal é medido (`medido`).
    pub fn adicionar_shell(&mut self, projeto: Option<String>, raiz: Option<String>, cx: &mut Context<Self>) {
        self.adicionar_shell_em(projeto, raiz, None, cx);
    }

    /// Nova aba de shell na pasta `cwd` (relativa ao projeto).
    pub fn adicionar_shell_em(&mut self, projeto: Option<String>, raiz: Option<String>, cwd: Option<String>, cx: &mut Context<Self>) {
        if !self.pronto(cx) {
            return;
        }
        self.seq_shell += 1;
        let id = format!("shell-{}", self.seq_shell);
        let programa = self.ultimo_programa.clone();
        let p = self.projeto(&projeto);
        let mut n = 1;
        while p.shells.iter().any(|s| s.n == n) {
            n += 1;
        }
        p.shells.push(Shell { id: id.clone(), n, programa, cwd: cwd.filter(|c| !c.is_empty()) });
        p.shell_ativa = id.clone();
        // A aba nova entra na área visível da faixa.
        let ultima = p.shells.len() - 1;
        self.faixa_shells.rolagem().scroll_to_item(ultima);
        let k = chave(projeto.as_deref(), &id);
        self.backend.terminais.recriar(&k, 80, 24);
        let v = self.vista(&k, true, raiz, cx);
        v.update(cx, |v, _| v.pedir_foco = true);
        cx.notify();
    }

    /// Ctrl+` e o botão da barra de status: com o painel abrindo e sem shell, cria o primeiro; senão foca o ativo.
    pub fn painel_aberto(&mut self, projeto: Option<String>, raiz: Option<String>, cx: &mut Context<Self>) {
        let p = self.projeto(&projeto);
        if p.shells.is_empty() {
            self.adicionar_shell(projeto, raiz, cx);
        } else {
            let k = chave(projeto.as_deref(), &p.shell_ativa.clone());
            if let Some(v) = self.vistas.get(&k) {
                v.update(cx, |v, _| v.pedir_foco = true);
            }
        }
    }

    fn medido(&mut self, v: Entity<VistaTerminal>, cx: &mut Context<Self>) {
        let (projeto, id) = v.read(cx).chave.clone();
        if !id.starts_with(orquestrador_core::core::SHELL_PREFIX) || v.read(cx).vivo {
            return;
        }
        let (c, l) = v.read(cx).tamanho();
        let cwd = self.projeto(&projeto).shells.iter().find(|s| s.id == id).and_then(|s| s.cwd.clone());
        let projetos = self.backend.projetos.clone();
        let (p2, id2) = (projeto.clone(), id.clone());
        let tarefa =
            cx.background_spawn(async move { projetos.shell_core(p2.as_deref())?.start_shell_in(&id2, c, l, cwd.as_deref()) });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                let existe = this.projeto(&projeto).shells.iter().any(|s| s.id == id);
                match r {
                    Ok(programa) if existe => {
                        this.ultimo_programa = programa.clone();
                        if let Some(s) = this.projeto(&projeto).shells.iter_mut().find(|s| s.id == id) {
                            s.programa = programa;
                        }
                        v.update(cx, |v, cx| v.ligar(cx));
                    }
                    // A aba fechou enquanto o shell abria: o que acabou de nascer no core morre de novo.
                    Ok(_) => {
                        let projetos = this.backend.projetos.clone();
                        cx.background_spawn(async move {
                            if let Ok(core) = projetos.shell_core(projeto.as_deref()) {
                                let _ = core.close_shell(&id);
                            }
                        })
                        .detach();
                    }
                    Err(e) => {
                        this.largar_shell(&projeto, &id, cx);
                        if let Some(janela) = this.janela.or_else(|| cx.active_window()) {
                            let _ = janela.update(cx, |_, window, cx| super::vista::avisar(e, window, cx));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Tira a aba e o terminal dela; quem chama decide se o shell morre. A última fecha o painel.
    fn largar_shell(&mut self, projeto: &Option<String>, id: &str, cx: &mut Context<Self>) {
        let p = self.projeto(projeto);
        let Some(i) = p.shells.iter().position(|s| s.id == id) else { return };
        p.shells.remove(i);
        let vazio = p.shells.is_empty();
        if vazio {
            p.shell_ativa.clear();
        } else if p.shell_ativa == id {
            p.shell_ativa = p.shells.get(i).or_else(|| i.checked_sub(1).and_then(|j| p.shells.get(j))).map(|s| s.id.clone()).unwrap_or_default();
            let k = chave(projeto.as_deref(), &p.shell_ativa.clone());
            if let Some(v) = self.vistas.get(&k) {
                v.update(cx, |v, _| v.pedir_foco = true);
            }
        }
        self.largar_vista(&chave(projeto.as_deref(), id), true);
        if vazio {
            cx.emit(EventoTerminais::FecharPainel);
        }
        cx.notify();
    }

    /// X da aba: mata o shell no core e só depois tira a aba. Se o core recusar, a aba fica.
    fn fechar_shell(&mut self, projeto: Option<String>, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let projetos = self.backend.projetos.clone();
        let (p2, id2) = (projeto.clone(), id.clone());
        let tarefa = cx.background_spawn(async move { projetos.shell_core(p2.as_deref())?.close_shell(&id2) });
        cx.spawn_in(window, async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| match r {
                Ok(()) => this.largar_shell(&projeto, &id, cx),
                Err(e) => super::vista::avisar(e, window, cx),
            });
        })
        .detach();
    }

    pub(crate) fn selecionar_shell(&mut self, projeto: &Option<String>, id: String, cx: &mut Context<Self>) {
        self.projeto(projeto).shell_ativa = id.clone();
        if let Some(v) = self.vistas.get(&chave(projeto.as_deref(), &id)) {
            v.update(cx, |v, _| v.pedir_foco = true);
        }
        if let Some(i) = self.projeto(projeto).shells.iter().position(|s| s.id == id) {
            self.faixa_shells.rolagem().scroll_to_item(i);
        }
        self.lista_shells.fechar();
        cx.notify();
    }

    /// Leva a aba para `indice`, contado entre as outras.
    pub fn mover_shell(&mut self, projeto: &Option<String>, id: &str, indice: usize, cx: &mut Context<Self>) {
        let p = self.projeto(projeto);
        let Some(i) = p.shells.iter().position(|s| s.id == id) else { return };
        let s = p.shells.remove(i);
        let indice = indice.min(p.shells.len());
        p.shells.insert(indice, s);
        cx.notify();
    }

    /// Os shells do terminal sem projeto morrem quando a primeira pasta abre (o ProjectApp sem projeto sai, no React).
    pub fn soltar_sem_projeto(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.projetos.get_mut(&None) else { return };
        let ids: Vec<String> = p.shells.drain(..).map(|s| s.id).collect();
        p.shell_ativa.clear();
        let projetos = self.backend.projetos.clone();
        for id in ids {
            self.largar_vista(&chave(None, &id), true);
            let projetos = projetos.clone();
            cx.background_spawn(async move {
                let _ = projetos.shell_core(None).and_then(|c| c.close_shell(&id));
            })
            .detach();
        }
    }

    /// Visibilidade de todos os terminais: só a aba ativa do projeto ativo, com o painel aberto, aparece.
    pub fn marcar_visiveis(&mut self, ativo: &Option<String>, painel_terminal: bool, cx: &mut Context<Self>) {
        let p = self.projetos.get(ativo);
        let orq = p.filter(|p| p.orquestrador.is_some()).map(|p| chave(ativo.as_deref(), &p.aba_ativa()));
        let shell = p.filter(|_| painel_terminal).map(|p| chave(ativo.as_deref(), &p.shell_ativa));
        for (k, v) in &self.vistas {
            let visivel = Some(k) == orq.as_ref() || Some(k) == shell.as_ref();
            if v.read(cx).visivel != visivel {
                v.update(cx, |v, _| v.visivel = visivel);
            }
        }
    }
}

/// O que falta para o Git registrar o que os executores mudarem (o `NO_GIT_FIX` do App.tsx).
fn sem_git(pasta: &std::path::Path) -> Option<&'static str> {
    use orquestrador_core::ide::git;
    match git::estado(pasta).ok()? {
        git::Estado::Repo { .. } => None,
        git::Estado::NoRepo => Some("Para ter isso, inicialize o repositório no painel Git."),
        git::Estado::NoGit => Some("Para ter isso, instale o Git e abra o app de novo."),
        git::Estado::RepoAbove { .. } => Some("Para ter isso, abra a raiz do repositório como projeto."),
    }
}

// ---------- desenho ----------

/// Aba no estilo `variant="square"`: 36 px, linha de 2 px sob a ativa.
fn aba(id: impl Into<gpui_kit::ElementId>, ativa: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .flex_none()
        .gap(px(6.))
        .h_full()
        .text_size(tema::TEXTO_MENU)
        .font_weight(FontWeight::MEDIUM)
        .text_color(cor::text_primary())
        .when(ativa, |d| d.child(div().absolute().left_0().right_0().bottom_0().h(px(2.)).bg(cor::ink_white())))
}

fn faixa() -> gpui_kit::Div {
    div()
        .relative()
        .flex()
        .flex_none()
        .items_stretch()
        .gap(px(4.))
        .h(tema::FAIXA)
        .px(px(4.))
        .child(div().absolute().left_0().right_0().bottom_0().h(px(1.)).bg(cor::hairline()))
}

impl Terminais {
    /// Ação de menu que volta a esta entidade.
    fn acao(cx: &mut Context<Self>, f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> impl Fn(&mut Window, &mut App) + 'static {
        let this = cx.entity().downgrade();
        move |window, cx| {
            let _ = this.update(cx, |t, cx| f(t, window, cx));
        }
    }

    /// O painel do Orquestrador (sem o contêiner nem a alça, que ficam em paineis.rs).
    pub fn painel_orquestrador(&mut self, projeto: Option<String>, raiz: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        self.projeto(&projeto);
        let p = &self.projetos[&projeto];
        let ativa = p.aba_ativa();
        let orq = p.orquestrador.clone();
        let executores = p.executores.clone();
        let escolha_exec = p.escolha_executores.clone();
        let aviso = p.aviso.clone();
        let aguardando = executores.iter().any(|e| e.estado == "awaiting-choice");
        // Executor aguardando escolha: a janela dele é a única e não fecha sem confirmar.
        if aguardando && orq.as_ref().is_some_and(|o| o.sobre != Some(Sobre::Executores)) && self.form.is_none() {
            let projeto = projeto.clone();
            let this = cx.entity().downgrade();
            let janela = self.janela;
            cx.defer(move |cx| {
                if let Some(janela) = janela.or_else(|| cx.active_window()) {
                    let _ = janela.update(cx, |_, window, cx| this.update(cx, |t, cx| t.configurar_executores(projeto, window, cx)));
                }
            });
        }
        let no = executores.iter().find(|e| e.id == ativa).cloned();
        let icone_orq = orq.as_ref().and_then(|o| o.escolha.as_ref()).map(|c| icone_da_ferramenta(&c.tool)).unwrap_or(crate::recursos::icone::TERMINAL);

        // Faixa de abas.
        let p1 = projeto.clone();
        let mut abas = vec![Aba::new(
            ORCHESTRATOR,
            aba("aba-orquestrador", ativa == ORCHESTRATOR)
                .px(px(12.))
                .on_click(cx.listener(move |this, _, window, cx| this.escolher_aba(&p1, ORCHESTRATOR.into(), window, cx)))
                .child(ui::icone(icone_orq, 14., cor::text_primary()))
                .child("Orquestrador")
                .test_support(),
        )];
        if orq.is_some() {
            for e in &executores {
                let (rotulo, icone, cor_estado) = estado_info(&e.estado);
                let pe = projeto.clone();
                let id = e.id.clone();
                abas.push(Aba::new(
                    e.id.clone(),
                    aba(SharedString::from(format!("aba-{}", e.id)), ativa == e.id)
                        .px(px(12.))
                        .tooltip_texto(format!("Executor #{}, {}: {}", e.n, rotulo, e.titulo))
                        .on_click(cx.listener(move |this, _, window, cx| this.escolher_aba(&pe, id.clone(), window, cx)))
                        .child(ui::icone_estado(&e.estado, icone, 14., cor_estado))
                        .child(format!("#{}", e.n))
                        .test_support(),
                ));
            }
        }
        let abas = FaixaAbas::new("faixa-orquestrador", &self.faixa_orq)
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .abas(abas)
            .ativa(Some(ativa.clone()));
        let mostrar_lista = orq.is_some() && !executores.is_empty();
        let mut itens = vec![{
            let p = projeto.clone();
            ItemLista::new(
                ORCHESTRATOR,
                ui::icone(icone_orq, 14., cor::text_primary()),
                "Orquestrador",
                Self::acao(cx, move |t, w, cx| t.escolher_aba(&p, ORCHESTRATOR.into(), w, cx)),
            )
        }];
        for e in &executores {
            let (rotulo, icone, cor_estado) = estado_info(&e.estado);
            let (p, id) = (projeto.clone(), e.id.clone());
            itens.push(
                ItemLista::new(
                    e.id.clone(),
                    ui::icone_estado(&e.estado, icone, 14., cor_estado),
                    format!("#{}\u{2002}{}", e.n, e.titulo),
                    Self::acao(cx, move |t, w, cx| t.escolher_aba(&p, id.clone(), w, cx)),
                )
                .estado(rotulo),
            );
        }
        let faixa_el = faixa().child(abas).when(mostrar_lista, |d| {
            d.child(ListaAbas::new("lista-orquestrador", &self.lista_orq, itens).ativa(ativa.clone()))
        });

        // Cabeçalho da aba ativa.
        let cabecalho = orq.as_ref().map(|o| {
            let base = div()
                .id("cabecalho-orquestrador")
                .flex()
                .items_center()
                .flex_none()
                .gap(px(8.))
                .h(tema::FAIXA)
                .px(px(12.))
                .whitespace_nowrap()
                .border_b_1()
                .border_color(cor::hairline())
                .text_size(tema::TEXTO_BODY);
            match &no {
                Some(n) => {
                    let (rotulo, icone, cor_estado) = estado_info(&n.estado);
                    let (pp, id) = (projeto.clone(), n.id.clone());
                    base.when(n.estado != "running", |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .min_w_0()
                                .flex_shrink(1.)
                                .h(px(22.))
                                .px(px(7.))
                                .rounded(tema::RAIO_XS)
                                .bg(cor_estado.opacity(0.15))
                                .text_color(cor_estado)
                                .text_size(tema::TEXTO_LABEL)
                                .child(ui::icone(icone, 12., cor_estado))
                                .child(div().truncate().child(rotulo)),
                        )
                    })
                    .child(div().flex_none().font_family(super::vista::FONTE).text_color(cor::text_secondary()).child(format!("#{}", n.n)))
                    .child(div().flex_1().min_w(px(64.)).truncate().child(n.titulo.clone()))
                    .when_some(n.escolha.clone(), |d, c| d.child(texto_da_escolha(&c, true)))
                    .when(cancelavel(&n.estado), |d| {
                        d.child(
                            ui::botao_pequeno("parar-executor")
                                .child(ui::icone("icons/square.svg", 12., cor::text_primary()))
                                .child("Parar")
                                .on_click(cx.listener(move |this, _, window, cx| this.parar(pp.clone(), id.clone(), window, cx))),
                        )
                    })
                }
                None => {
                    let pp = projeto.clone();
                    base.when_some(o.escolha.clone(), |d, c| {
                        d.child(texto_da_escolha(&c, false)).child(
                            ui::botao_pequeno("configurar-executores")
                                .ml_auto()
                                .tooltip_texto(match &escolha_exec {
                                    Some(e) => format!("Executores: {}. Escolha dos executores; a troca vale para os próximos", rotulo_da_escolha(e)),
                                    None => "Configurar executores; a escolha vale para os próximos".into(),
                                })
                                .child(ui::icone("icons/sliders-horizontal.svg", 14., cor::text_primary()))
                                .map(|b| match &escolha_exec {
                                    Some(e) => b
                                        .child("Executores")
                                        .child(ui::icone(icone_da_ferramenta(&e.tool), 14., cor::text_primary()))
                                        .child(escolha::nome_da_ferramenta(&e.tool).to_string()),
                                    None => b.child("Configurar executores"),
                                })
                                .on_click(cx.listener(move |this, _, window, cx| this.configurar_executores(pp.clone(), window, cx))),
                        )
                    })
                }
            }
        });

        // Área do terminal.
        let mut area = div().id("area-orquestrador").relative().flex_1().min_h_0().overflow_hidden().bg(cor::terminal_black());
        if orq.is_some() {
            let k = chave(projeto.as_deref(), &ativa);
            let v = if ativa == ORCHESTRATOR {
                Some(self.vista(&k, true, raiz.clone(), cx))
            } else {
                Some(self.vista(&k, false, raiz.clone(), cx))
            };
            if let Some(v) = v {
                area = area.child(div().absolute().inset_0().child(v));
            }
            if let Some(texto) = no.as_ref().and_then(|n| texto_da_capa(&n.estado)) {
                area = area.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(16.))
                        .bg(gpui_kit::rgba(0x000000f2))
                        .child(div().max_w(px(300.)).text_center().text_size(tema::TEXTO_BODY).text_color(cor::text_secondary()).child(texto)),
                );
            }
        } else {
            let pp = projeto.clone();
            let rr = raiz.clone();
            area = area.child(
                div().absolute().inset_0().flex().items_center().justify_center().child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .p(px(16.))
                        .w(px(380.))
                        .max_w_full()
                        .when(!aviso.is_empty(), |d| {
                            d.child(
                                div()
                                    .id("aviso-orquestrador")
                                    .px(px(12.))
                                    .py(px(6.))
                                    .rounded(tema::RAIO_SM)
                                    .border_1()
                                    .border_color(cor::hairline())
                                    .bg(cor::panel_black())
                                    .text_size(tema::TEXTO_BODY)
                                    .child(aviso.clone()),
                            )
                        })
                        .child(
                            ui::botao("abrir-terminal", "Abrir terminal", Variante::Primario, false)
                                .on_click(cx.listener(move |this, _, window, cx| this.abrir_orquestrador(pp.clone(), rr.clone(), window, cx)))
                                .test_support(),
                        )
                        .child(
                            div()
                                .text_size(tema::TEXTO_LABEL)
                                .text_color(cor::text_secondary())
                                .text_center()
                                .child("Abre o orquestrador, com quem você conversa. Os executores aparecem em abas quando ele delega."),
                        ),
                ),
            );
        }
        let form = self.form.as_ref().filter(|(fp, _, _)| *fp == projeto).map(|(_, f, _)| f.clone());

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(faixa_el)
            .child(div().flex().flex_col().flex_1().min_h_0().children(cabecalho).child(area))
            .children(form)
            .into_any_element()
    }

    /// O painel Terminal (sem o contêiner nem a alça).
    pub fn painel_terminal(&mut self, projeto: Option<String>, raiz: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        let p = self.projeto(&projeto);
        let shells = p.shells.clone();
        let ativa = p.shell_ativa.clone();
        let atual = shells.iter().find(|s| s.id == ativa).cloned();
        let pasta = raiz.as_deref().map(crate::backend::nome_da_pasta).unwrap_or_default();
        let titulo = |s: &Shell| -> String {
            match (s.programa.is_empty(), pasta.is_empty()) {
                (false, false) => format!("{pasta} — {}", s.programa),
                (false, true) => s.programa.clone(),
                (true, false) => pasta.clone(),
                (true, true) => "Terminal".into(),
            }
        };
        let rotulo = |s: &Shell| -> String {
            if s.programa.is_empty() {
                format!("Terminal {}", s.n)
            } else {
                format!("Terminal {} · {}", s.n, s.programa)
            }
        };
        let compacto = self.largura_cabecalho.get() < LARGURA_COMPACTA;
        let itens: Vec<ItemLista> = {
            shells
                .iter()
                .map(|s| {
                    let (p1, p2, k1, k2) = (projeto.clone(), projeto.clone(), s.id.clone(), s.id.clone());
                    ItemLista::new(
                        s.id.clone(),
                        ui::icone(crate::recursos::icone::TERMINAL, 14., cor::text_primary()),
                        rotulo(s),
                        Self::acao(cx, move |t, _, cx| t.selecionar_shell(&p1, k1.clone(), cx)),
                    )
                    .ao_fechar(Self::acao(cx, move |t, w, cx| t.fechar_shell(p2.clone(), k2.clone(), w, cx)))
                })
                .collect()
        };
        let pa = projeto.clone();
        let ra = raiz.clone();
        let mais = ui::acao("novo-terminal", crate::recursos::icone::MAIS, true)
            .self_center()
            .tooltip_texto("Novo terminal")
            .on_click(cx.listener(move |this, _, _, cx| this.adicionar_shell(pa.clone(), ra.clone(), cx)))
            .test_support();
        let medida = self.largura_cabecalho.clone();
        let medidor = canvas(
            move |b, _, _| {
                let w = f32::from(b.size.width);
                (medida.get() != w).then(|| medida.set(w))
            },
            |_, mudou, window, _| {
                if mudou.is_some() {
                    window.refresh();
                }
            },
        )
        .absolute()
        .inset_0();

        let mut cabecalho = faixa().id("cabecalho-terminal").child(medidor);
        if compacto {
            let pf = projeto.clone();
            let idf = atual.as_ref().map(|s| s.id.clone());
            cabecalho = cabecalho
                .child(
                    ListaAbas::new("lista-shells", &self.lista_shells, itens)
                        .ativa(ativa.clone())
                        .a_esquerda()
                        .flex_1()
                        .min_w_0()
                        .gatilho(
                            div()
                                .id("terminal-ativo")
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .flex_1()
                                .min_w_0()
                                .self_center()
                                .h(px(24.))
                                .px(px(7.))
                                .rounded(tema::RAIO_SM)
                                .text_size(tema::TEXTO_MENU)
                                .hover(|s| s.bg(cor::fill_hover()))
                                .child(ui::icone(crate::recursos::icone::TERMINAL, 14., cor::text_primary()))
                                .when_some(atual.clone(), |d, s| {
                                    d.child(div().flex_none().child(format!("Terminal {}", s.n))).when(!s.programa.is_empty(), |d| {
                                        d.child(div().min_w_0().truncate().text_color(cor::text_secondary()).child(format!("· {}", s.programa)))
                                    })
                                })
                                .child(div().ml_auto().child(ui::icone("icons/chevrons-up-down.svg", 14., cor::text_secondary()))),
                        ),
                )
                .child(
                    ui::acao("fechar-terminal-ativo", crate::recursos::icone::FECHAR, atual.is_some()).self_center().on_click(
                        cx.listener(move |this, _, window, cx| {
                            if let Some(id) = idf.clone() {
                                this.fechar_shell(pf.clone(), id, window, cx);
                            }
                        }),
                    ),
                )
                .child(mais);
        } else {
            let mut abas = Vec::with_capacity(shells.len());
            for s in shells.iter() {
                let (ps, pc) = (projeto.clone(), projeto.clone());
                let (ids, idc) = (s.id.clone(), s.id.clone());
                abas.push(Aba::new(
                    s.id.clone(),
                    aba(SharedString::from(format!("shell-{}", s.id)), s.id == ativa)
                        .pl(px(12.))
                        .pr(px(28.))
                        .tooltip_texto(titulo(s))
                        .on_click(cx.listener(move |this, _, _, cx| this.selecionar_shell(&ps, ids.clone(), cx)))
                        .child(ui::icone(crate::recursos::icone::TERMINAL, 14., cor::text_secondary()))
                        .child(div().flex_none().child(format!("Terminal {}", s.n)))
                        .when(!s.programa.is_empty(), |d| {
                            d.child(div().max_w(px(128.)).truncate().text_color(cor::text_secondary()).font_weight(FontWeight::NORMAL).child(format!("· {}", s.programa)))
                        })
                        .child(
                            div().absolute().right(px(4.)).top_0().bottom_0().flex().items_center().child(
                                ui::acao(SharedString::from(format!("fechar-shell-{}", s.id)), crate::recursos::icone::FECHAR, true)
                                    .tooltip_texto(format!("Fechar o terminal {}", titulo(s)))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.fechar_shell(pc.clone(), idc.clone(), window, cx);
                                    }))
                                    .test_support(),
                            ),
                        ),
                ));
            }
            let ids: Vec<String> = shells.iter().map(|s| s.id.clone()).collect();
            let (pm, pi) = (projeto.clone(), projeto.clone());
            let ids_i = ids.clone();
            let faixa_abas = FaixaAbas::new("faixa-shells", &self.faixa_shells)
                .flex()
                .flex_initial()
                .min_w_0()
                .h_full()
                .abas(abas)
                .ativa(Some(ativa.clone()).filter(|a| !a.is_empty()))
                // Como no React (`z-20 bg-card`): a arrastada cobre as vizinhas com o fundo do painel.
                .estilo_arrastada(|s| s.bg(cor::painel_arrastavel()))
                .ao_iniciar(cx.listener(move |this, de: &usize, _, cx| {
                    if let Some(id) = ids_i.get(*de) {
                        this.selecionar_shell(&pi, id.clone(), cx);
                    }
                }))
                .ao_mover(cx.listener(move |this, m: &MoverAba, _, cx| {
                    if let Some(id) = ids.get(m.de) {
                        this.mover_shell(&pm, id, m.para, cx);
                    }
                }));
            let max = self.faixa_shells.rolagem().max_offset();
            let off = self.faixa_shells.rolagem().offset();
            let transborda = max.x > px(0.5);
            let antes = off.x < px(-0.5);
            let depois = off.x > -max.x + px(0.5);
            let pagina = |this: &mut Self, dir: f32| {
                let h = this.faixa_shells.rolagem();
                let largura = h.bounds().size.width * 0.8;
                let x = (h.offset().x - largura * dir).clamp(-h.max_offset().x, px(0.));
                h.set_offset(point(x, h.offset().y));
            };
            cabecalho = cabecalho
                .child(faixa_abas)
                .child(mais)
                .child(div().flex_1())
                .when(transborda, |d| {
                    d.child(
                        ui::acao("abas-anteriores", "icons/chevron-left.svg", antes)
                            .self_center()
                            .tooltip_texto("Mostra as abas anteriores")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                pagina(this, -1.);
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::acao("abas-seguintes", "icons/chevron-right.svg", depois)
                            .self_center()
                            .tooltip_texto("Mostra as abas seguintes")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                pagina(this, 1.);
                                cx.notify();
                            })),
                    )
                })
                .when(!shells.is_empty(), |d| d.child(ListaAbas::new("lista-shells", &self.lista_shells, itens).ativa(ativa.clone())));
        }
        cabecalho = cabecalho.child(
            ui::acao("esconder-terminal", "icons/chevron-down.svg", true)
                .self_center()
                .tooltip_texto("Esconde o painel; os shells continuam rodando")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(EventoTerminais::FecharPainel)))
                .test_support(),
        );

        let mut area = div().relative().flex_1().min_h_0().overflow_hidden().bg(cor::terminal_black());
        if !ativa.is_empty() {
            let v = self.vista(&chave(projeto.as_deref(), &ativa), true, raiz, cx);
            area = area.child(div().absolute().inset_0().child(v));
        }
        div().flex().flex_col().flex_1().min_h_0().overflow_hidden().child(cabecalho).child(area).into_any_element()
    }
}

/// Ferramenta em texto normal; modelo em monoespaçado, que corta; esforço como detalhe. `compacto` esconde modelo e esforço
/// em painel estreito.
fn texto_da_escolha(c: &Choice, compacto: bool) -> impl IntoElement {
    let detalhe = |d: gpui_kit::Div| d.when(compacto, |d| d.flex_shrink(1.));
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w_0()
        .when(compacto, |d| d.flex_none())
        .child(div().flex_none().font_weight(FontWeight::MEDIUM).child(escolha::nome_da_ferramenta(&c.tool).to_string()))
        .when(!c.model.is_empty(), |d| {
            d.child(detalhe(div().min_w_0().truncate().font_family(super::vista::FONTE).text_color(cor::text_secondary()).child(c.model.clone())))
        })
        .when(c.model.is_empty() && c.effort.is_empty(), |d| {
            d.child(detalhe(div().min_w_0().truncate().text_color(cor::text_secondary()).child("padrão do CLI")))
        })
        .when(!c.effort.is_empty(), |d| {
            d.child(div().flex_none().font_family(super::vista::FONTE).text_color(cor::text_secondary()).child(c.effort.clone()))
        })
}
