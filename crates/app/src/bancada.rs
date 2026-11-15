//! A janela principal: estado da moldura (abas de projeto, tela da navegação, painéis) e a ligação com o core.
use crate::backend::{self, Backend, Evento, Receptor, ResumoGit, RECENTES_MAX};
use crate::estado::{Paineis, Projetos};
use crate::tema;
use crate::terminal::painel::{EventoTerminais, Terminais};
use gpui_kit::{
    actions, App, AppContext, Context, FocusHandle, Focusable, PathPromptOptions, Pixels, Point, Task, WeakEntity,
    Window,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

actions!(orquestrador, [AbrirPasta, FecharPasta, AlternarTerminal, Sair]);

/// Projeto recente escolhido no submenu.
#[derive(Clone, PartialEq, gpui_kit::Action)]
#[action(namespace = orquestrador, no_json)]
pub struct AbrirRecente(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rota {
    Arquivos,
    Pesquisa,
    Git,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Aba {
    pub id: String,
    pub caminho: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Divisor {
    Lateral,
    Painel,
    Terminal,
}

#[derive(Clone, Copy, Debug)]
pub struct Arrasto {
    pub divisor: Divisor,
    pub inicio: Point<Pixels>,
    pub valor: f32,
}

pub struct Bancada {
    pub backend: Rc<Backend>,
    pub abas: Vec<Aba>,
    pub ativa: Option<String>,
    pub recentes: Vec<String>,
    pub rota: Rota,
    /// Sem projeto, a navegação abre e fecha a faixa lateral (o `sideOpen` do React).
    pub lateral_aberta: bool,
    pub terminal_aberto: bool,
    pub largura_lateral: f32,
    pub largura_painel: f32,
    pub altura_terminal: f32,
    pub git: HashMap<String, ResumoGit>,
    pub erro: Option<String>,
    pub arrasto: Option<Arrasto>,
    pub foco: FocusHandle,
    /// Arrasto e rolagem da faixa de abas de projeto (moldura.rs).
    pub abas_ui: orquestrador_ui::EstadoFaixa,
    /// Orquestrador, executores e shells de cada projeto (terminal/painel.rs).
    pub terminais: gpui_kit::Entity<Terminais>,
    _tarefas: Vec<Task<()>>,
}

impl Focusable for Bancada {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

impl Bancada {
    /// `iniciais` são os caminhos da linha de comando; as abas salvas abrem antes deles.
    pub fn new(backend: Rc<Backend>, eventos: Receptor, iniciais: Vec<PathBuf>, cx: &mut Context<Self>) -> Self {
        orquestrador_ui::estado::instalar(backend.estado.clone(), cx);
        let salvas: Projetos = backend.estado.borrow().ler();
        let paineis: Paineis = backend.estado.borrow().ler();
        let mut tarefas = vec![cx.spawn(async move |this: WeakEntity<Self>, cx| {
            while let Ok(evento) = eventos.recv().await {
                if this.update(cx, |this, cx| this.receber(evento, cx)).is_err() {
                    break;
                }
            }
        })];
        if backend.poll_git {
            tarefas.push(cx.spawn(async move |this: WeakEntity<Self>, cx| loop {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                if this.update(cx, |this, cx| this.atualizar_git(cx)).is_err() {
                    break;
                }
            }));
        }
        let terminais = cx.new(|cx| Terminais::new(backend.clone(), cx));
        // O estado dos painéis de terminal vive em `Terminais`; mudou, a janela redesenha.
        cx.observe(&terminais, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&terminais, |this, _, e: &EventoTerminais, cx| match e {
            EventoTerminais::FecharPainel => {
                if this.terminal_aberto {
                    this.alternar_terminal(cx);
                }
            }
        })
        .detach();
        let mut bancada = Bancada {
            terminais,
            backend,
            abas: Vec::new(),
            ativa: None,
            recentes: salvas.recentes.clone(),
            rota: Rota::Arquivos,
            lateral_aberta: false,
            terminal_aberto: paineis.terminal_aberto,
            largura_lateral: paineis.largura_lateral.unwrap_or(tema::LATERAL),
            largura_painel: paineis.largura_painel.unwrap_or(tema::PAINEL),
            altura_terminal: paineis.altura_terminal.unwrap_or(tema::TERMINAL),
            git: HashMap::new(),
            erro: None,
            arrasto: None,
            foco: cx.focus_handle(),
            abas_ui: Default::default(),
            _tarefas: tarefas,
        };
        bancada.restaurar(salvas, iniciais, cx);
        bancada
    }

    pub fn aba_ativa(&self) -> Option<&Aba> {
        self.ativa.as_ref().and_then(|id| self.abas.iter().find(|a| &a.id == id))
    }

    pub fn git_ativo(&self) -> Option<&ResumoGit> {
        self.ativa.as_ref().and_then(|id| self.git.get(id))
    }

    /// A faixa lateral aparece com projeto aberto ou quando a navegação a abriu.
    pub fn lateral_visivel(&self) -> bool {
        self.ativa.is_some() || self.lateral_aberta
    }

    fn restaurar(&mut self, salvas: Projetos, iniciais: Vec<PathBuf>, cx: &mut Context<Self>) {
        let projetos = self.backend.projetos.clone();
        let abertas = salvas.abertas.clone();
        let ativa = salvas.ativa.clone();
        let pastas: Vec<PathBuf> = iniciais.into_iter().filter(|p| backend::eh_pasta(p)).collect();
        if abertas.is_empty() && pastas.is_empty() {
            return;
        }
        let tarefa = cx.background_spawn(async move {
            // Pasta salva que sumiu não volta; as demais seguem.
            let abrir = |lista: Vec<PathBuf>| -> Vec<_> {
                lista.iter().filter_map(|c| projetos.open_project(c).ok()).collect()
            };
            (abrir(abertas.iter().map(PathBuf::from).collect()), abrir(pastas))
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let (salvos, da_linha) = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                for aberto in salvos.iter().chain(&da_linha) {
                    this.incluir(aberto.id.clone(), aberto.project.clone());
                }
                for aberto in &da_linha {
                    this.lembrar(&aberto.project);
                }
                let alvo = da_linha.last().map(|a| a.id.clone()).or_else(|| {
                    ativa.and_then(|c| salvos.iter().find(|a| a.project == c).map(|a| a.id.clone()))
                        .or_else(|| salvos.first().map(|a| a.id.clone()))
                });
                if let Some(id) = alvo {
                    this.selecionar(&id, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn incluir(&mut self, id: String, caminho: String) {
        if !self.abas.iter().any(|a| a.id == id) {
            self.abas.push(Aba { id, caminho });
        }
    }

    fn receber(&mut self, evento: Evento, cx: &mut Context<Self>) {
        match evento {
            Evento::Core { nome, payload } => {
                self.terminais.update(cx, |t, cx| t.receber(&nome, &payload, cx));
                // Qualquer mudança de estado do projeto ativo pode mexer no Git (lote terminado, commit do executor).
                let projeto = payload.get("project").and_then(|v| v.as_str()).map(str::to_string);
                if projeto.is_some() && projeto == self.ativa {
                    self.atualizar_git(cx);
                }
            }
            Evento::AbrirCaminhos(caminhos) => {
                for caminho in caminhos.into_iter().filter(|p| backend::eh_pasta(p)) {
                    self.abrir(caminho, cx);
                }
                cx.emit_activate();
            }
        }
    }

    pub fn escolher_pasta(&mut self, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Pasta do projeto".into()),
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Ok(Ok(Some(caminhos))) = escolha.await else { return };
            let Some(pasta) = caminhos.into_iter().next() else { return };
            let _ = this.update(cx, |this, cx| this.abrir(pasta, cx));
        })
        .detach();
    }

    /// Abre (ou só seleciona, se já aberta) a pasta, pelo `Projects::open_project` do core, fora da thread da janela.
    pub fn abrir(&mut self, pasta: PathBuf, cx: &mut Context<Self>) {
        let projetos = self.backend.projetos.clone();
        let tarefa = cx.background_spawn(async move { projetos.open_project(&pasta) });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let resultado = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                match resultado {
                    Ok(aberto) => {
                        this.erro = None;
                        this.incluir(aberto.id.clone(), aberto.project.clone());
                        this.lembrar(&aberto.project);
                        this.selecionar(&aberto.id, cx);
                    }
                    Err(e) => this.erro = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn lembrar(&mut self, caminho: &str) {
        self.recentes.retain(|c| !Path::new(c).eq(Path::new(caminho)));
        self.recentes.insert(0, caminho.to_string());
        self.recentes.truncate(RECENTES_MAX);
    }

    pub fn selecionar(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.abas.iter().any(|a| a.id == id) {
            return;
        }
        self.ativa = Some(id.to_string());
        self.salvar();
        self.atualizar_git(cx);
        cx.notify();
    }

    /// Fecha a aba e encerra o projeto no core; a vizinha à direita (ou à esquerda) vira a ativa.
    pub fn fechar(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(indice) = self.abas.iter().position(|a| a.id == id) else { return };
        self.abas.remove(indice);
        self.git.remove(id);
        if self.ativa.as_deref() == Some(id) {
            self.ativa = self.abas.get(indice).or_else(|| self.abas.last()).map(|a| a.id.clone());
        }
        self.terminais.update(cx, |t, cx| t.fechar_projeto(id, cx));
        let projetos = self.backend.projetos.clone();
        let id = id.to_string();
        cx.background_spawn(async move {
            let _ = projetos.close_project(&id);
        })
        .detach();
        self.salvar();
        self.atualizar_git(cx);
        cx.notify();
    }

    pub fn fechar_ativa(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.ativa.clone() {
            self.fechar(&id, cx);
        }
    }

    pub fn salvar(&self) {
        let abas = Projetos {
            abertas: self.abas.iter().map(|a| a.caminho.clone()).collect(),
            ativa: self.aba_ativa().map(|a| a.caminho.clone()),
            recentes: self.recentes.clone(),
        };
        self.backend.estado.borrow_mut().gravar(&abas);
    }

    pub fn salvar_paineis(&self) {
        let paineis = Paineis {
            largura_lateral: Some(self.largura_lateral),
            largura_painel: Some(self.largura_painel),
            altura_terminal: Some(self.altura_terminal),
            terminal_aberto: self.terminal_aberto,
        };
        self.backend.estado.borrow_mut().gravar(&paineis);
    }

    pub fn atualizar_git(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.ativa.clone() else { return };
        let projetos = self.backend.projetos.clone();
        let tarefa = cx.background_spawn({
            let id = id.clone();
            async move { backend::resumo_git(&projetos, &id) }
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let resumo = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                let mudou = match resumo {
                    Some(r) => this.git.insert(id, r.clone()) != Some(r),
                    None => this.git.remove(&id).is_some(),
                };
                if mudou {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Navegação: sem projeto, o botão da tela aberta fecha a faixa (como `go` no App.tsx).
    pub fn ir(&mut self, rota: Rota, cx: &mut Context<Self>) {
        if self.ativa.is_none() {
            self.lateral_aberta = !self.lateral_aberta || rota != self.rota;
        }
        self.rota = rota;
        cx.notify();
    }

    pub fn alternar_terminal(&mut self, cx: &mut Context<Self>) {
        self.terminal_aberto = !self.terminal_aberto;
        if self.terminal_aberto {
            let raiz = self.aba_ativa().map(|a| a.caminho.clone());
            let ativa = self.ativa.clone();
            self.terminais.update(cx, |t, cx| t.painel_aberto(ativa, raiz, cx));
        }
        self.salvar_paineis();
        cx.notify();
    }

    pub fn comecar_arrasto(&mut self, divisor: Divisor, inicio: Point<Pixels>) {
        let valor = match divisor {
            Divisor::Lateral => self.largura_lateral,
            Divisor::Painel => self.largura_painel,
            Divisor::Terminal => self.altura_terminal,
        };
        self.arrasto = Some(Arrasto { divisor, inicio, valor });
    }

    /// Aplica o arrasto com os limites do App.tsx: o centro nunca fica abaixo de `CENTRO_MIN` nem o editor de `ARQUIVO_MIN`.
    pub fn arrastar(&mut self, posicao: Point<Pixels>, janela: gpui_kit::Size<Pixels>, cx: &mut Context<Self>) {
        let Some(a) = self.arrasto else { return };
        let dx = f32::from(posicao.x - a.inicio.x);
        let dy = f32::from(posicao.y - a.inicio.y);
        let largura = f32::from(janela.width) - f32::from(tema::NAV);
        let painel = if self.ativa.is_some() { self.largura_painel } else { 0. };
        let lateral = if self.lateral_visivel() { self.largura_lateral } else { 0. };
        match a.divisor {
            Divisor::Lateral => {
                let max = (largura - painel - tema::CENTRO_MIN).max(tema::LATERAL_MIN);
                self.largura_lateral = (a.valor + dx).clamp(tema::LATERAL_MIN, max);
            }
            Divisor::Painel => {
                let max = (largura - lateral - tema::CENTRO_MIN).max(tema::PAINEL_MIN);
                self.largura_painel = (a.valor - dx).clamp(tema::PAINEL_MIN, max);
            }
            Divisor::Terminal => {
                let centro = f32::from(janela.height) - f32::from(tema::TITLEBAR) - f32::from(tema::STATUS_BAR);
                let max = (centro - tema::ARQUIVO_MIN).max(tema::TERMINAL_MIN);
                self.altura_terminal = (a.valor - dy).clamp(tema::TERMINAL_MIN, max);
            }
        }
        cx.notify();
    }

    pub fn soltar(&mut self, cx: &mut Context<Self>) {
        if self.arrasto.take().is_some() {
            self.salvar_paineis();
            cx.notify();
        }
    }

    pub fn titulo(&self) -> String {
        match self.aba_ativa() {
            Some(a) => format!("{} - Orquestrador", backend::nome_da_pasta(&a.caminho)),
            None => "Orquestrador".into(),
        }
    }

    pub fn abrir_recente(&mut self, caminho: &str, cx: &mut Context<Self>) {
        self.abrir(PathBuf::from(caminho), cx);
    }

    pub fn sair(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        window.remove_window();
    }
}

/// Traz a janela para a frente quando outra instância repassa caminhos.
trait Ativar {
    fn emit_activate(&mut self);
}

impl Ativar for Context<'_, Bancada> {
    fn emit_activate(&mut self) {
        self.defer(|cx| {
            if let Some(janela) = cx.active_window().or_else(|| cx.windows().into_iter().next()) {
                let _ = janela.update(cx, |_, window, _| window.activate_window());
            }
        });
    }
}
