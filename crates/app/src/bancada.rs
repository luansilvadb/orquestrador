//! A janela principal: estado da moldura (abas de projeto, tela da navegação, painéis) e a ligação com o core.
use crate::backend::{self, Backend, Evento, Receptor, RECENTES_MAX};
use crate::estado::{Paineis, Projetos};
use crate::fechamento::Fechamento;
use crate::telas::Telas;
use crate::tema;
use crate::terminal::painel::{finalizado, EventoTerminais, Terminais};
use gpui_kit::{
    actions, App, AppContext, Context, Entity, FocusHandle, Focusable, PathPromptOptions, Pixels, Point, Subscription,
    Task, WeakEntity, Window,
};
use orquestrador_editor::{Abertura, AreaEditor};
use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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

/// O que cada `ProjectApp` do React guarda para si: rota, faixa, painel Terminal e medidas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vista {
    pub rota: Rota,
    pub lateral_aberta: bool,
    pub terminal_aberto: bool,
    pub largura_lateral: f32,
    pub largura_painel: f32,
    pub altura_terminal: f32,
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
    /// A vista de cada projeto fora de foco (o `None` é o modo sem projeto); a do ativo está nos campos acima.
    pub vistas: HashMap<Option<String>, Vista>,
    /// A vista com que um projeto nasce (as medidas gravadas em `paineis`).
    pub vista_inicial: Vista,
    pub erro: Option<String>,
    pub arrasto: Option<Arrasto>,
    pub foco: FocusHandle,
    /// Arrasto e rolagem da faixa de abas de projeto (moldura.rs).
    pub abas_ui: orquestrador_ui::EstadoFaixa,
    /// Orquestrador, executores e shells de cada projeto (terminal/painel.rs).
    pub terminais: Entity<Terminais>,
    /// Árvore, editor, pesquisa e Git de cada projeto aberto (telas.rs).
    pub telas: HashMap<String, Telas>,
    /// O editor sem projeto.
    pub solto: Option<Entity<AreaEditor>>,
    /// Executores vivos por projeto, lido pelas abas de diff e conflito que nascem depois.
    pub travados: HashMap<String, Rc<Cell<bool>>>,
    pub assinaturas_editor: Vec<Subscription>,
    /// Arquivos pedidos (linha de comando, outra instância) para projetos que ainda não têm telas.
    pub pendentes: HashMap<String, Vec<String>>,
    /// Arquivos soltos pedidos antes de existir o editor sem projeto.
    pub soltos_pendentes: Vec<String>,
    /// A confirmação de fechamento na tela (fechamento.rs).
    pub fechamento: Option<Fechamento>,
    /// O fechamento da janela já foi liberado (salvo, descartado ou confirmado).
    pub janela_liberada: bool,
    /// Projeto em fechamento: o X e "Fechar pasta" não respondem de novo até terminar.
    pub fechando: Option<String>,
    /// Ctrl+K pressionado há pouco (Ctrl+K Ctrl+O abre pasta).
    pub acorde_k: Option<std::time::Instant>,
    /// Dica dos botões da janela à vista (moldura.rs).
    pub dica_janela: Option<(&'static str, u8)>,
    pub dica_tarefa: Option<Task<()>>,
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
        if !cx.has_global::<GitIniciado>() {
            orquestrador_git::init(cx);
            orquestrador_midia::registrar_atalhos(cx);
            orquestrador_arvore::registrar_atalhos(cx);
            // O Tab global da árvore percorre o foco; no terminal a tecla vai ao shell (completar), como no xterm.
            cx.bind_keys([
                gpui_kit::KeyBinding::new("tab", gpui_kit::NoAction, Some("Terminal")),
                gpui_kit::KeyBinding::new("shift-tab", gpui_kit::NoAction, Some("Terminal")),
            ]);
            cx.set_global(GitIniciado);
        }
        let salvas: Projetos = backend.estado.borrow().ler();
        let paineis: Paineis = backend.estado.borrow().ler();
        let tarefas = vec![cx.spawn(async move |this: WeakEntity<Self>, cx| {
            while let Ok(evento) = eventos.recv().await {
                if this.update(cx, |this, cx| this.receber(evento, cx)).is_err() {
                    break;
                }
            }
        })];
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
        // Ctrl+clique de caminho nos terminais vai ao editor e à árvore do projeto ativo.
        let eu = cx.weak_entity();
        cx.set_global(crate::terminal::vista::AbrirLinks(Some(Rc::new(move |alvo, window, cx| {
            let _ = eu.update(cx, |b, cx| b.abrir_link(alvo, window, cx));
        }))));
        let vista_inicial = Vista {
            rota: Rota::Arquivos,
            lateral_aberta: false,
            terminal_aberto: paineis.terminal_aberto,
            largura_lateral: paineis.largura_lateral.unwrap_or(tema::LATERAL),
            largura_painel: paineis.largura_painel.unwrap_or(tema::PAINEL),
            altura_terminal: paineis.altura_terminal.unwrap_or(tema::TERMINAL),
        };
        let mut bancada = Bancada {
            terminais,
            backend,
            abas: Vec::new(),
            ativa: None,
            recentes: salvas.recentes.clone(),
            rota: Rota::Arquivos,
            lateral_aberta: false,
            terminal_aberto: false,
            largura_lateral: 0.,
            largura_painel: 0.,
            altura_terminal: 0.,
            vistas: HashMap::new(),
            vista_inicial,
            erro: None,
            arrasto: None,
            foco: cx.focus_handle(),
            abas_ui: Default::default(),
            telas: HashMap::new(),
            solto: None,
            travados: HashMap::new(),
            assinaturas_editor: Vec::new(),
            pendentes: HashMap::new(),
            soltos_pendentes: Vec::new(),
            fechamento: None,
            janela_liberada: false,
            fechando: None,
            acorde_k: None,
            dica_janela: None,
            dica_tarefa: None,
            _tarefas: tarefas,
        };
        bancada.aplicar_vista(vista_inicial);
        bancada.restaurar(salvas, iniciais, cx);
        bancada
    }

    pub fn aba_ativa(&self) -> Option<&Aba> {
        self.ativa.as_ref().and_then(|id| self.abas.iter().find(|a| &a.id == id))
    }

    /// A faixa lateral aparece com projeto aberto ou quando a navegação a abriu.
    pub fn lateral_visivel(&self) -> bool {
        self.ativa.is_some() || self.lateral_aberta
    }

    pub fn vista_atual(&self) -> Vista {
        Vista {
            rota: self.rota,
            lateral_aberta: self.lateral_aberta,
            terminal_aberto: self.terminal_aberto,
            largura_lateral: self.largura_lateral,
            largura_painel: self.largura_painel,
            altura_terminal: self.altura_terminal,
        }
    }

    fn aplicar_vista(&mut self, v: Vista) {
        self.rota = v.rota;
        self.lateral_aberta = v.lateral_aberta;
        self.terminal_aberto = v.terminal_aberto;
        self.largura_lateral = v.largura_lateral;
        self.largura_painel = v.largura_painel;
        self.altura_terminal = v.altura_terminal;
    }

    /// Executores ativos (rodando ou na fila) do projeto: o ponto na aba.
    pub fn ocupacao(&self, id: &str, cx: &App) -> usize {
        self.terminais
            .read(cx)
            .projetos
            .get(&Some(id.to_string()))
            .map_or(0, |p| p.executores.iter().filter(|e| !finalizado(&e.estado)).count())
    }

    fn restaurar(&mut self, salvas: Projetos, iniciais: Vec<PathBuf>, cx: &mut Context<Self>) {
        let projetos = self.backend.projetos.clone();
        let abertas = salvas.abertas.clone();
        let ativa = salvas.ativa.clone();
        if abertas.is_empty() && iniciais.is_empty() {
            return;
        }
        let tarefa = cx.background_spawn(async move {
            // Pasta salva que sumiu não volta; as demais seguem.
            abertas.iter().filter_map(|c| projetos.open_project(Path::new(c)).ok()).collect::<Vec<_>>()
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let salvos = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                for aberto in &salvos {
                    this.incluir(aberto.id.clone(), aberto.project.clone());
                }
                let alvo = ativa
                    .and_then(|c| salvos.iter().find(|a| a.project.eq_ignore_ascii_case(&c)).map(|a| a.id.clone()))
                    .or_else(|| salvos.first().map(|a| a.id.clone()));
                if let Some(id) = alvo {
                    this.selecionar(&id, cx);
                }
                // Os caminhos da linha de comando seguem a mesma regra dos repassados por outra instância.
                this.abrir_caminhos(iniciais, cx);
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
                let projeto = payload.get("project").and_then(|v| v.as_str()).map(str::to_string);
                // Executor que terminou e orquestrador que saiu podem ter mexido no repositório.
                let terminou = match nome.as_str() {
                    "node-state" => payload.get("state").and_then(|v| v.as_str()).is_some_and(finalizado),
                    "orchestrator-exit" => true,
                    _ => false,
                };
                if let (true, Some(p)) = (terminou, projeto.as_deref()) {
                    self.cutucar_git(p, cx);
                }
                cx.notify();
            }
            Evento::AbrirCaminhos(caminhos) => {
                self.abrir_caminhos(caminhos, cx);
                cx.emit_activate();
            }
        }
    }

    /// Pastas abrem como projeto; arquivo abre no projeto que o contém, senão no ativo, senão no editor sem projeto
    /// (o `openAbsolute` do App.tsx).
    pub fn abrir_caminhos(&mut self, caminhos: Vec<PathBuf>, cx: &mut Context<Self>) {
        for caminho in caminhos {
            if backend::eh_pasta(&caminho) {
                self.abrir(caminho, cx);
            } else if caminho.is_file() {
                self.abrir_arquivo_externo(&caminho, cx);
            }
        }
    }

    fn abrir_arquivo_externo(&mut self, caminho: &Path, cx: &mut Context<Self>) {
        let texto = caminho.to_string_lossy().into_owned();
        let dentro = self.abas.iter().find_map(|a| relativo(&a.caminho, &texto).map(|r| (a.id.clone(), r)));
        let (id, rel) = match dentro {
            Some((id, rel)) => (Some(id), rel),
            None => (self.ativa.clone(), texto.replace('\\', "/")),
        };
        match id {
            Some(id) => {
                self.pendentes.entry(id.clone()).or_default().push(rel);
                self.selecionar(&id, cx);
                self.rota = Rota::Arquivos;
            }
            None => self.soltos_pendentes.push(texto),
        }
        cx.notify();
    }

    /// Arquivos pedidos que já podem abrir (o editor do projeto existe).
    pub(crate) fn drenar_pendentes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let op = Abertura { permanente: true, ..Default::default() };
        if let Some(id) = self.ativa.clone() {
            if self.telas.contains_key(&id) {
                for c in self.pendentes.remove(&id).unwrap_or_default() {
                    self.abrir_no_editor(Some(&id), &c, op, window, cx);
                }
            }
        } else if self.solto.is_some() {
            for c in std::mem::take(&mut self.soltos_pendentes) {
                self.abrir_no_editor(None, &c, op, window, cx);
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

    /// Arquivo > Abrir arquivo e Ctrl+O: o caminho escolhido segue a regra dos caminhos externos.
    pub fn escolher_arquivo(&mut self, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Ok(Ok(Some(caminhos))) = escolha.await else { return };
            let _ = this.update(cx, |this, cx| this.abrir_caminhos(caminhos, cx));
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
                        if aberto.subfolder {
                            let texto = format!(
                                "Esta pasta está dentro do repositório {}. O projeto aberto é a raiz dele.",
                                aberto.project
                            );
                            orquestrador_ui::avisos::mostrar(orquestrador_ui::avisos::Aviso::info(texto, 6000), cx);
                        }
                    }
                    Err(e) => {
                        orquestrador_ui::avisos::mostrar(orquestrador_ui::avisos::Aviso::erro(e.clone(), 7000), cx);
                        this.erro = Some(e);
                    }
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
        self.salvar();
    }

    /// Troca a aba ativa guardando a vista da anterior; as telas e os terminais dela seguem montados.
    pub fn selecionar(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.abas.iter().any(|a| a.id == id) {
            return;
        }
        if self.ativa.as_deref() != Some(id) {
            let atual = self.vista_atual();
            self.vistas.insert(self.ativa.clone(), atual);
            let nova = self.vistas.remove(&Some(id.to_string())).unwrap_or(self.vista_inicial);
            self.aplicar_vista(nova);
            self.ativa = Some(id.to_string());
        }
        self.salvar();
        cx.notify();
    }

    /// X da aba e "Fechar pasta": pergunta pelos não salvos e pelo trabalho do projeto antes (`mayCloseProject`).
    pub fn fechar(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.fechando.as_deref() == Some(id) || self.fechamento.is_some() {
            return;
        }
        self.pedir_fechar_projeto(id, window, cx);
    }

    /// Fecha a aba e encerra o projeto no core; a vizinha à direita (ou à esquerda) vira a ativa.
    pub fn fechar_confirmado(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(indice) = self.abas.iter().position(|a| a.id == id) else { return };
        if let Some(t) = self.telas.get(id) {
            t.editor.update(cx, |ed, cx| ed.gravar_recuperacao(cx));
        }
        self.abas.remove(indice);
        self.vistas.remove(&Some(id.to_string()));
        if self.ativa.as_deref() == Some(id) {
            let proxima = self.abas.get(indice).or_else(|| self.abas.last()).map(|a| a.id.clone());
            let nova = self.vistas.remove(&proxima).unwrap_or(match proxima {
                Some(_) => self.vista_inicial,
                None => Vista { lateral_aberta: false, ..self.vista_inicial },
            });
            self.aplicar_vista(nova);
            self.ativa = proxima;
        }
        self.soltar_telas(id);
        self.terminais.update(cx, |t, cx| t.fechar_projeto(id, cx));
        let projetos = self.backend.projetos.clone();
        let id = id.to_string();
        let tarefa = cx.background_spawn(async move { projetos.close_project(&id) });
        cx.spawn(async move |_, cx| {
            if let Err(e) = tarefa.await {
                let _ = cx.update(|cx| orquestrador_ui::avisos::mostrar(orquestrador_ui::avisos::Aviso::erro(e, 7000), cx));
            }
        })
        .detach();
        self.salvar();
        cx.notify();
    }

    pub fn fechar_ativa(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.ativa.clone() {
            self.fechar(&id, window, cx);
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

    pub fn salvar_paineis(&mut self) {
        let paineis = Paineis {
            largura_lateral: Some(self.largura_lateral),
            largura_painel: Some(self.largura_painel),
            altura_terminal: Some(self.altura_terminal),
            terminal_aberto: self.terminal_aberto,
        };
        self.vista_inicial = Vista { rota: Rota::Arquivos, lateral_aberta: false, ..self.vista_atual() };
        self.backend.estado.borrow_mut().gravar(&paineis);
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

    /// Mínimo da coluna central: um por grupo do editor (o `CENTER_MIN_WIDTH * groupCount` do App.tsx).
    fn centro_min(&self) -> f32 {
        tema::CENTRO_MIN * self.telas_ativas().map_or(1, |t| t.grupos.max(1)) as f32
    }

    /// Aplica o arrasto com os limites do App.tsx: o centro nunca fica abaixo do mínimo nem o editor de `ARQUIVO_MIN`.
    pub fn arrastar(&mut self, posicao: Point<Pixels>, janela: gpui_kit::Size<Pixels>, cx: &mut Context<Self>) {
        let Some(a) = self.arrasto else { return };
        let dx = f32::from(posicao.x - a.inicio.x);
        let dy = f32::from(posicao.y - a.inicio.y);
        let largura = f32::from(janela.width) - f32::from(tema::NAV);
        let painel = if self.ativa.is_some() { self.largura_painel } else { 0. };
        let lateral = if self.lateral_visivel() { self.largura_lateral } else { 0. };
        let centro_min = self.centro_min();
        match a.divisor {
            Divisor::Lateral => {
                let max = (largura - painel - centro_min).max(tema::LATERAL_MIN);
                self.largura_lateral = (a.valor + dx).clamp(tema::LATERAL_MIN, max);
            }
            Divisor::Painel => {
                let max = (largura - lateral - centro_min).max(tema::PAINEL_MIN);
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
            self.enviar_layout(cx);
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

    /// Arquivo > Sair: o mesmo caminho do X da janela (pergunta antes, se preciso).
    pub fn sair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pode_fechar_janela(window, cx) {
            window.remove_window();
        }
    }
}

/// `orquestrador_git::init` e os atalhos da mídia, uma vez por app.
struct GitIniciado;
impl gpui_kit::Global for GitIniciado {}

/// Caminho relativo (com `/`) de `caminho` dentro de `raiz`, sem diferenciar maiúsculas (Windows).
pub fn relativo(raiz: &str, caminho: &str) -> Option<String> {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_string();
    let (r, c) = (norm(raiz), norm(caminho));
    if c.len() <= r.len() || !c[..r.len()].eq_ignore_ascii_case(&r) || c.as_bytes()[r.len()] != b'/' {
        return None;
    }
    Some(c[r.len() + 1..].to_string())
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
