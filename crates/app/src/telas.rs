//! As telas de cada projeto (árvore, editor, pesquisa e Git) e as ligações entre elas, como `src/App.tsx` faz com
//! `GitFilesScreen`, `SearchScreen`, `GitSidebar` e `EditorWorkspace`.
use crate::bancada::{Bancada, Rota};
use gpui_kit::{AnyView, AnyWindowHandle, App, AppContext, Context, Entity, Subscription, WeakEntity, Window};
use orquestrador_arvore::{EventoArvore, Explorador};
use orquestrador_editor::modelo::Escopo;
use orquestrador_editor::{Abertura, Alvo, AreaEditor, EventoEditor, FabricaConteudo, Lado};
use orquestrador_git::fonte::Estado;
use orquestrador_git::{
    EventoConflito, EventoDiff, EventoGit, Fonte, FonteCore, GuardaGit, TelaGit, VisaoConflito, VisaoDiff,
};
use orquestrador_midia::{EventoMidia, VisaoMidia};
use orquestrador_pesquisa::{EventoPesquisa, TelaPesquisa};
use orquestrador_ui::avisos::{self, Aviso};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Vista de conteúdo de uma aba do editor que a bancada criou (precisa saber quando fica à vista).
#[derive(Clone)]
pub enum Conteudo {
    Midia(gpui_kit::WeakEntity<VisaoMidia>),
    Diff(gpui_kit::WeakEntity<VisaoDiff>),
    Conflito(gpui_kit::WeakEntity<VisaoConflito>),
}

/// O que a bancada aplicou por último às telas, para só repassar mudanças.
#[derive(Clone, Copy, Default, PartialEq)]
struct Aplicado {
    projeto_ativo: bool,
    arquivos: bool,
    git: bool,
    travado: bool,
    chat: bool,
}

pub struct Telas {
    pub raiz: String,
    pub arvore: Entity<Explorador>,
    pub editor: Entity<AreaEditor>,
    pub pesquisa: Entity<TelaPesquisa>,
    pub git: Entity<TelaGit>,
    pub guarda: GuardaGit,
    pub fonte: Fonte,
    /// O último estado que a tela Git publicou (badge, barra de status, árvore).
    pub estado_git: Option<Estado>,
    /// Alvo da aba ativa do grupo focado.
    pub alvo: Option<Alvo>,
    pub grupos: usize,
    pub sincronizando: bool,
    pub conteudos: Rc<RefCell<Vec<(Alvo, Conteudo)>>>,
    aplicado: Option<Aplicado>,
    /// Últimas abas e sujos repassados à árvore e à pesquisa.
    abas_sujos: (Vec<String>, Vec<String>),
    _subs: Vec<Subscription>,
}

/// Branch e contadores para a barra de status; `None` fora do modo `repo`.
pub struct ResumoGit {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub alterados: usize,
}

impl Telas {
    pub fn resumo(&self) -> Option<ResumoGit> {
        match &self.estado_git {
            Some(Estado::Repo { status }) => Some(ResumoGit {
                branch: status.branch.clone(),
                ahead: status.ahead,
                behind: status.behind,
                alterados: status.files.len(),
            }),
            _ => None,
        }
    }
}

pub fn para_git(a: &Alvo) -> Option<orquestrador_git::Alvo> {
    use orquestrador_git::{Alvo as G, Lado as L};
    Some(match a {
        Alvo::Diff { side, path } => G::Diff {
            lado: if *side == Lado::Staged { L::Preparadas } else { L::Alteracoes },
            caminho: path.clone(),
        },
        Alvo::Commit { hash, path } => G::Commit { hash: hash.clone(), caminho: path.clone() },
        Alvo::Conflict { path } => G::Conflito { caminho: path.clone() },
        _ => return None,
    })
}

pub fn do_git(a: &orquestrador_git::Alvo) -> Alvo {
    use orquestrador_git::{Alvo as G, Lado as L};
    match a {
        G::Diff { lado, caminho } => Alvo::Diff {
            side: if *lado == L::Preparadas { Lado::Staged } else { Lado::Unstaged },
            path: caminho.clone(),
        },
        G::Commit { hash, caminho } => Alvo::Commit { hash: hash.clone(), path: caminho.clone() },
        G::Conflito { caminho } => Alvo::Conflict { path: caminho.clone() },
    }
}

/// Caminho absoluto de um caminho do projeto (relativo com `/`) ou já absoluto.
pub fn absoluto(raiz: &str, caminho: &str) -> PathBuf {
    let p = Path::new(caminho);
    if p.is_absolute() || raiz.is_empty() {
        p.to_path_buf()
    } else {
        Path::new(raiz).join(caminho.replace('/', "\\"))
    }
}

/// A aba de conflito até a vista existir (ver a fábrica).
struct CascaConflito(Option<Entity<VisaoConflito>>);

impl gpui_kit::Render for CascaConflito {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui_kit::IntoElement {
        use gpui_kit::{ParentElement, Styled};
        gpui_kit::div().size_full().children(self.0.clone())
    }
}

/// Executa `f` na janela, fora do evento que o pediu.
fn na_janela(janela: AnyWindowHandle, cx: &mut App, f: impl FnOnce(&mut Window, &mut App) + 'static) {
    cx.defer(move |cx| {
        let _ = janela.update(cx, |_, window, cx| f(window, cx));
    });
}

/// A fábrica das abas que não são texto: diff, commit, conflito e mídia.
fn fabrica(
    raiz: String, fonte: Fonte, guarda: GuardaGit, editor: WeakEntity<AreaEditor>, git: WeakEntity<TelaGit>,
    conteudos: Rc<RefCell<Vec<(Alvo, Conteudo)>>>, travado: Rc<std::cell::Cell<bool>>, bancada: WeakEntity<Bancada>,
    projeto: String,
) -> FabricaConteudo {
    Rc::new(move |alvo: &Alvo, window: &mut Window, cx: &mut App| -> Option<AnyView> {
        let janela = window.window_handle();
        // A vista nasce depois do aviso de aba ativa: a visibilidade é reaplicada em seguida.
        {
            let (b, p) = (bancada.clone(), projeto.clone());
            na_janela(janela, cx, move |window, cx| {
                let _ = b.update(cx, |b, cx| b.atualizar_visiveis(Some(&p), window, cx));
            });
        }
        let cutucar = {
            let git = git.clone();
            move |cx: &mut App| {
                if let Some(g) = git.upgrade() {
                    g.read(cx).cutucar();
                }
            }
        };
        match alvo {
            Alvo::File { path, .. } => {
                let abs = absoluto(&raiz, path);
                orquestrador_midia::midia_de(&abs)?;
                let v = cx.new(|cx| VisaoMidia::new(abs, window, cx));
                let (b, p, ed, a) = (bancada.clone(), projeto.clone(), editor.clone(), alvo.clone());
                cx.subscribe(&v, move |_, e: &EventoMidia, cx| {
                    if let EventoMidia::VoltarParaAba = e {
                        let (b, p, ed, a) = (b.clone(), p.clone(), ed.clone(), a.clone());
                        na_janela(janela, cx, move |window, cx| {
                            let _ = b.update(cx, |b, cx| b.voltar_para_aba(&p, &ed, &a, window, cx));
                        });
                    }
                })
                .detach();
                conteudos.borrow_mut().push((alvo.clone(), Conteudo::Midia(v.downgrade())));
                Some(v.into())
            }
            Alvo::Diff { .. } | Alvo::Commit { .. } => {
                let g = para_git(alvo)?;
                let v = cx.new(|cx| VisaoDiff::new(fonte.clone(), guarda.clone(), g, window, cx));
                let trav = travado.get();
                v.update(cx, |v, cx| v.definir_travado(trav, cx));
                let editor = editor.clone();
                cx.subscribe(&v, move |_, e: &EventoDiff, cx| match e {
                    EventoDiff::AtualizarGit => cutucar(cx),
                    EventoDiff::AbrirLinha { caminho, linha } => {
                        let (editor, caminho, linha) = (editor.clone(), caminho.clone(), *linha as u64);
                        na_janela(janela, cx, move |window, cx| {
                            let op = Abertura { permanente: true, linha: Some(linha), ..Default::default() };
                            let _ = editor.update(cx, |ed, cx| ed.abrir_arquivo(&caminho, op, window, cx));
                        });
                    }
                })
                .detach();
                conteudos.borrow_mut().push((alvo.clone(), Conteudo::Diff(v.downgrade())));
                Some(v.into())
            }
            Alvo::Conflict { path } => {
                // A fábrica roda dentro do `update` do editor, que guarda o rascunho (da recuperação ou da sessão): a vista
                // nasce logo depois, numa casca, já com ele.
                let casca = cx.new(|_| CascaConflito(None));
                let (fonte, guarda, editor, git, conteudos, travado, bancada, projeto, alvo, caminho) = (
                    fonte.clone(),
                    guarda.clone(),
                    editor.clone(),
                    git.clone(),
                    conteudos.clone(),
                    travado.clone(),
                    bancada.clone(),
                    projeto.clone(),
                    alvo.clone(),
                    path.clone(),
                );
                let c = casca.downgrade();
                na_janela(janela, cx, move |window, cx| {
                    let Some(casca) = c.upgrade() else { return };
                    let guardado = editor.upgrade().and_then(|ed| ed.read(cx).rascunho_conflito(&caminho)).map(|r| {
                        orquestrador_git::RascunhoConflito { caminho: r.path, origem: r.origin, texto: r.draft, revisao: r.revision }
                    });
                    let v = cx.new(|cx| VisaoConflito::new(fonte, guarda, caminho.clone(), guardado, window, cx));
                    let trav = travado.get();
                    v.update(cx, |v, cx| v.definir_travado(trav, window, cx));
                    let cutucar = move |cx: &mut App| {
                        if let Some(g) = git.upgrade() {
                            g.read(cx).cutucar();
                        }
                    };
                    cx.subscribe(&v, move |_, e: &EventoConflito, cx| match e {
                        EventoConflito::AtualizarGit => cutucar(cx),
                        EventoConflito::Rascunho(r) => {
                            let (editor, r) = (editor.clone(), r.clone());
                            na_janela(janela, cx, move |_, cx| {
                                let _ = editor.update(cx, |ed, cx| {
                                    ed.definir_rascunho_conflito(&r.caminho, &r.origem, &r.texto, &r.revisao, cx)
                                });
                            });
                        }
                        EventoConflito::Fechar => {
                            let (editor, caminho) = (editor.clone(), caminho.clone());
                            na_janela(janela, cx, move |window, cx| {
                                let _ = editor.update(cx, |ed, cx| {
                                    let id = ed
                                        .workspace()
                                        .abas()
                                        .find(|t| matches!(&t.alvo, Alvo::Conflict { path } if *path == caminho))
                                        .map(|t| t.id.clone());
                                    if let Some(id) = id {
                                        ed.fechar(Escopo::Uma(id), window, cx);
                                    }
                                });
                            });
                        }
                    })
                    .detach();
                    conteudos.borrow_mut().push((alvo, Conteudo::Conflito(v.downgrade())));
                    casca.update(cx, |c, cx| {
                        c.0 = Some(v);
                        cx.notify();
                    });
                    let _ = bancada.update(cx, |b, cx| b.atualizar_visiveis(Some(&projeto), window, cx));
                });
                Some(casca.into())
            }
            Alvo::Draft { .. } => None,
        }
    })
}

impl Bancada {
    /// Cria as telas do projeto ativo na primeira vez que ele aparece (precisa da janela).
    pub(crate) fn garantir_telas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.solto.is_none() {
            let ed = cx.new(|cx| AreaEditor::new("", window, cx));
            self.ligar_editor(None, &ed, window, cx);
            self.solto = Some(ed);
        }
        let Some(aba) = self.aba_ativa().cloned() else { return };
        if self.telas.contains_key(&aba.id) {
            return;
        }
        let core = match self.backend.projetos.core(&aba.id) {
            Ok(c) => c,
            Err(_) => return,
        };
        let id = aba.id.clone();
        let raiz = aba.caminho.clone();
        let fonte: Fonte = std::sync::Arc::new(FonteCore(core));
        let guarda = GuardaGit::new();
        let nome = crate::backend::nome_da_pasta(&raiz);
        let arvore = cx.new(|cx| Explorador::new(PathBuf::from(&raiz), nome, window, cx));
        // Nasce sem projeto: a recuperação (abas de diff e mídia, layout) só é lida depois da fábrica e das assinaturas.
        let editor = cx.new(|cx| AreaEditor::new("", window, cx));
        let pesquisa = cx.new(|cx| TelaPesquisa::new(Some(PathBuf::from(&raiz)), window, cx));
        let git = cx.new(|cx| TelaGit::new(fonte.clone(), guarda.clone(), window, cx));
        let conteudos: Rc<RefCell<Vec<(Alvo, Conteudo)>>> = Default::default();
        let travado = self.travado_rc(&id);
        let eu = cx.weak_entity();
        editor.update(cx, |ed, _| {
            ed.definir_fabrica_conteudo(fabrica(
                raiz.clone(),
                fonte.clone(),
                guarda.clone(),
                editor.downgrade(),
                git.downgrade(),
                conteudos.clone(),
                travado,
                eu,
                id.clone(),
            ))
        });
        // Descartar o rascunho de um conflito relê a vista do zero.
        let cs = conteudos.clone();
        editor.update(cx, |ed, _| {
            ed.ao_descartar_conflito(Rc::new(move |caminho: &str, window: &mut Window, cx: &mut App| {
                let vista = cs.borrow().iter().find_map(|(a, c)| match (a, c) {
                    (Alvo::Conflict { path }, Conteudo::Conflito(v)) if path == caminho => v.upgrade(),
                    _ => None,
                });
                if let Some(v) = vista {
                    v.update(cx, |v, cx| v.carregar(true, window, cx));
                }
            }))
        });
        let mut subs = Vec::new();
        let p = id.clone();
        subs.push(cx.subscribe_in(&arvore, window, move |this, _, e: &EventoArvore, window, cx| {
            this.evento_arvore(&p, e.clone(), window, cx)
        }));
        let p = id.clone();
        subs.push(cx.subscribe_in(&pesquisa, window, move |this, _, e: &EventoPesquisa, window, cx| {
            this.evento_pesquisa(&p, e, window, cx)
        }));
        let p = id.clone();
        subs.push(cx.subscribe_in(&git, window, move |this, _, e: &EventoGit, window, cx| {
            this.evento_git(&p, e, window, cx)
        }));
        self.telas.insert(
            id.clone(),
            Telas {
                raiz,
                arvore,
                editor: editor.clone(),
                pesquisa,
                git,
                guarda,
                fonte,
                estado_git: None,
                alvo: None,
                grupos: 1,
                sincronizando: false,
                conteudos,
                aplicado: None,
                abas_sujos: Default::default(),
                _subs: subs,
            },
        );
        self.ligar_editor(Some(id.clone()), &editor, window, cx);
        let r = self.telas[&id].raiz.clone();
        editor.update(cx, |ed, cx| ed.definir_projeto(r, window, cx));
        self.enviar_layout(cx);
    }

    fn ligar_editor(&mut self, projeto: Option<String>, editor: &Entity<AreaEditor>, window: &mut Window, cx: &mut Context<Self>) {
        let p = projeto.clone();
        let s = cx.subscribe_in(editor, window, move |this, _, e: &EventoEditor, window, cx| {
            this.evento_editor(p.as_deref(), e, window, cx)
        });
        // Abas e sujos mudam a cada edição: árvore e pesquisa leem a lista atual.
        let p = projeto.clone();
        let o = cx.observe(editor, move |this, _, cx| this.repassar_abas(p.as_deref(), cx));
        self.assinaturas_editor.push(s);
        self.assinaturas_editor.push(o);
    }

    /// O editor do projeto (ou o solto, sem projeto).
    pub fn editor_de(&self, projeto: Option<&str>) -> Option<Entity<AreaEditor>> {
        match projeto {
            Some(id) => self.telas.get(id).map(|t| t.editor.clone()),
            None => self.solto.clone(),
        }
    }

    pub fn editor_ativo(&self) -> Option<Entity<AreaEditor>> {
        self.editor_de(self.ativa.as_deref())
    }

    /// "Voltar para a guia" da picture-in-picture: traz o projeto e a aba de mídia de volta à frente.
    pub(crate) fn voltar_para_aba(
        &mut self, projeto: &str, editor: &WeakEntity<AreaEditor>, alvo: &Alvo, window: &mut Window, cx: &mut Context<Self>,
    ) {
        self.selecionar(projeto, cx);
        let _ = editor.update(cx, |ed, cx| {
            let id = ed.workspace().abas().find(|t| t.alvo == *alvo).map(|t| t.id.clone());
            if let Some(id) = id {
                ed.ativar(&id, window, cx);
            }
        });
        self.aplicar_telas(window, cx);
    }

    pub fn telas_ativas(&self) -> Option<&Telas> {
        self.ativa.as_ref().and_then(|id| self.telas.get(id))
    }

    pub(crate) fn abrir_no_editor(
        &mut self, projeto: Option<&str>, caminho: &str, op: Abertura, window: &mut Window, cx: &mut Context<Self>,
    ) {
        if let Some(ed) = self.editor_de(projeto) {
            ed.update(cx, |ed, cx| ed.abrir_arquivo(caminho, op, window, cx));
        }
    }

    fn evento_arvore(&mut self, p: &str, e: EventoArvore, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.telas.get(p) else { return };
        let (editor, raiz) = (t.editor.clone(), t.raiz.clone());
        match e {
            EventoArvore::Abrir { caminho, fixo, ao_lado } => {
                let op = Abertura { permanente: fixo, ..Default::default() };
                editor.update(cx, |ed, cx| {
                    if ao_lado {
                        ed.abrir_ao_lado(&caminho, op, window, cx)
                    } else {
                        ed.abrir_arquivo(&caminho, op, window, cx)
                    }
                });
            }
            EventoArvore::PreCarregar(caminho) => editor.update(cx, |ed, cx| ed.pre_carregar(&caminho, window, cx)),
            EventoArvore::AbrirNoTerminal(dir) => {
                let projeto = Some(p.to_string());
                self.terminais.update(cx, |t, cx| t.adicionar_shell_em(projeto, Some(raiz), Some(dir), cx));
                if !self.terminal_aberto {
                    self.terminal_aberto = true;
                    self.salvar_paineis();
                }
            }
            EventoArvore::AdicionarAoChat(caminho) => {
                let abs = absoluto(&raiz, &caminho).to_string_lossy().into_owned();
                let projeto = Some(p.to_string());
                let ok = self.terminais.update(cx, |t, cx| t.colar_no_orquestrador(projeto, abs, window, cx));
                if !ok {
                    avisos::mostrar(Aviso::erro("O Orquestrador não está aberto.", 7000), cx);
                }
            }
            EventoArvore::HistoricoGit(caminho) => {
                t.git.update(cx, |g, cx| g.pedir_historico(Some(caminho), cx));
                self.rota = Rota::Git;
            }
            EventoArvore::Pesquisar(dir) => {
                t.pesquisa.update(cx, |s, cx| {
                    s.pesquisar_na_pasta(&dir, window, cx);
                    s.focar(window, cx);
                });
                self.rota = Rota::Pesquisa;
            }
            EventoArvore::Renomeado { de, para } => editor.update(cx, |ed, cx| ed.remapear(&de, &para, cx)),
            EventoArvore::Excluidos(caminhos) => editor.update(cx, |ed, cx| ed.remover(&caminhos, cx)),
            EventoArvore::Movidos(lista) => editor.update(cx, |ed, cx| {
                for m in &lista {
                    ed.remapear(&m.from, &m.to, cx);
                }
            }),
        }
        cx.notify();
    }

    fn evento_pesquisa(&mut self, p: &str, e: &EventoPesquisa, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.telas.get(p) else { return };
        match e {
            EventoPesquisa::AbrirArquivo { caminho, linha, coluna, permanente } => {
                let op = Abertura {
                    permanente: *permanente,
                    linha: Some(*linha as u64),
                    coluna: Some(*coluna as u64),
                    lado: None,
                };
                t.editor.update(cx, |ed, cx| ed.abrir_arquivo(caminho, op, window, cx));
            }
            EventoPesquisa::PreLer { caminho } => t.editor.update(cx, |ed, cx| ed.pre_carregar(caminho, window, cx)),
            EventoPesquisa::ArquivosAlterados { .. } => {
                t.git.read(cx).cutucar();
                t.editor.update(cx, |ed, cx| ed.atualizar(window, cx));
            }
            EventoPesquisa::Aviso(texto) => avisos::mostrar(Aviso::sucesso(texto.clone(), 3000), cx),
        }
    }

    fn evento_git(&mut self, p: &str, e: &EventoGit, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.telas.get_mut(p) else { return };
        match e {
            EventoGit::AbrirAlvo { alvo, permanente } => {
                let op = Abertura { permanente: *permanente, ..Default::default() };
                let alvo = do_git(alvo);
                t.editor.update(cx, |ed, cx| ed.abrir_alvo(alvo, op, window, cx));
            }
            // Os alvos do Git são diff, commit e conflito; a pré-leitura do editor só cobre arquivos de texto.
            EventoGit::PreCarregar { .. } => {}
            EventoGit::AbrirArquivo { caminho, permanente } => {
                let op = Abertura { permanente: *permanente, ..Default::default() };
                t.editor.update(cx, |ed, cx| ed.abrir_arquivo(caminho, op, window, cx));
            }
            EventoGit::Estado(estado) => {
                t.estado_git = estado.clone();
                let arquivos = match estado {
                    Some(Estado::Repo { status }) => Some(status.files.clone()),
                    _ => None,
                };
                let repo = arquivos.is_some();
                t.arvore.update(cx, |a, cx| {
                    a.definir_git(arquivos.as_deref(), cx);
                    a.definir_historico(repo);
                });
                cx.notify();
            }
        }
    }

    fn evento_editor(&mut self, p: Option<&str>, e: &EventoEditor, window: &mut Window, cx: &mut Context<Self>) {
        match e {
            EventoEditor::AbaAtiva(alvo) => {
                if let Some(t) = p.and_then(|p| self.telas.get_mut(p)) {
                    t.alvo = alvo.clone();
                    let aberto = alvo.as_ref().and_then(|a| a.caminho()).map(str::to_string);
                    t.arvore.update(cx, |a, cx| a.definir_aberto(aberto, cx));
                    let sel = alvo.as_ref().and_then(para_git);
                    t.git.update(cx, |g, cx| g.definir_selecionado(sel, cx));
                }
                self.atualizar_visiveis(p, window, cx);
                self.repassar_abas(p, cx);
            }
            EventoEditor::Salvo => {
                if let Some(t) = p.and_then(|p| self.telas.get(p)) {
                    t.pesquisa.update(cx, |s, cx| s.atualizar_apos_gravar(cx));
                }
            }
            EventoEditor::AtualizarGit => {
                if let Some(t) = p.and_then(|p| self.telas.get(p)) {
                    t.git.read(cx).cutucar();
                }
            }
            EventoEditor::RevelarNoExplorer(caminho) => {
                if let Some(t) = p.and_then(|p| self.telas.get(p)) {
                    let c = caminho.clone();
                    t.arvore.update(cx, |a, cx| a.revelar(c, window, cx));
                    self.rota = Rota::Arquivos;
                    cx.notify();
                }
            }
            EventoEditor::Grupos(n) => {
                if let Some(t) = p.and_then(|p| self.telas.get_mut(p)) {
                    t.grupos = *n;
                    cx.notify();
                }
            }
            EventoEditor::RestaurarLayout(texto) => self.restaurar_layout(texto, cx),
            // O gutter do Git (próxima/anterior alteração) não está ligado nesta bancada.
            EventoEditor::IrParaAlteracao { .. } => {}
        }
    }

    /// Abas abertas e arquivos sujos do editor para a árvore (colisões, aviso de excluir) e a pesquisa.
    fn repassar_abas(&mut self, p: Option<&str>, cx: &mut Context<Self>) {
        let Some(t) = p.and_then(|p| self.telas.get_mut(p)) else { return };
        let ed = t.editor.read(cx);
        let atual = (ed.caminhos_abertos(), ed.nao_salvos());
        if t.abas_sujos == atual {
            return;
        }
        t.abas_sujos = atual.clone();
        let (abertos, sujos) = atual;
        let (arvore, pesquisa) = (t.arvore.clone(), t.pesquisa.clone());
        arvore.update(cx, |a, _| {
            a.definir_abas(abertos.clone());
            a.definir_sujos(sujos.clone());
        });
        pesquisa.update(cx, |s, cx| {
            s.definir_abertos(abertos, cx);
            s.definir_alterados(sujos.into_iter().collect::<HashSet<_>>(), cx);
        });
    }

    /// Mídia fora de vista pausa; diff à vista relê; conflito que volta a ser o atual relê o arquivo.
    pub(crate) fn atualizar_visiveis(&mut self, p: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = p.and_then(|p| self.telas.get(p)) else { return };
        let ws = t.editor.read(cx).workspace();
        let visiveis: Vec<Alvo> = ws
            .groups
            .iter()
            .filter_map(|g| g.active.as_ref().and_then(|a| g.tabs.iter().find(|x| &x.id == a)).map(|x| x.alvo.clone()))
            .collect();
        let projeto_ativo = self.ativa.as_deref() == p;
        let mut lista = t.conteudos.borrow_mut();
        lista.retain(|(_, c)| match c {
            Conteudo::Midia(w) => w.upgrade().is_some(),
            Conteudo::Diff(w) => w.upgrade().is_some(),
            Conteudo::Conflito(w) => w.upgrade().is_some(),
        });
        let atual = t.alvo.clone();
        for (alvo, c) in lista.clone() {
            let visivel = projeto_ativo && visiveis.contains(&alvo);
            match c {
                Conteudo::Midia(w) => {
                    if let Some(v) = w.upgrade() {
                        v.update(cx, |v, cx| v.definir_visivel(visivel, cx));
                    }
                }
                Conteudo::Diff(w) => {
                    if let Some(v) = w.upgrade() {
                        v.update(cx, |v, cx| v.definir_ativa(visivel, window, cx));
                    }
                }
                Conteudo::Conflito(w) => {
                    if let (Some(v), true) = (w.upgrade(), atual.as_ref() == Some(&alvo)) {
                        v.update(cx, |v, cx| v.mostrar(window, cx));
                    }
                }
            }
        }
    }

    /// Repassa às telas o que depende da bancada: projeto à vista, tela da lateral, executores e orquestrador.
    pub(crate) fn aplicar_telas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.telas.keys().cloned().collect();
        for id in ids {
            let projeto_ativo = self.ativa.as_deref() == Some(id.as_str());
            let (travado, chat) = {
                let t = self.terminais.read(cx);
                let p = t.projetos.get(&Some(id.clone()));
                (
                    p.is_some_and(|p| !p.executores.is_empty()),
                    p.is_some_and(|p| p.orquestrador.as_ref().is_some_and(|o| o.escolha.is_some())),
                )
            };
            let novo = Aplicado {
                projeto_ativo,
                arquivos: projeto_ativo && self.rota == Rota::Arquivos,
                git: projeto_ativo && self.rota == Rota::Git,
                travado,
                chat,
            };
            let Some(t) = self.telas.get_mut(&id) else { continue };
            let antes = t.aplicado.replace(novo);
            if antes == Some(novo) {
                continue;
            }
            let antes = antes.unwrap_or(Aplicado { projeto_ativo: !novo.projeto_ativo, arquivos: !novo.arquivos, git: !novo.git, travado: !novo.travado, chat: !novo.chat });
            if antes.arquivos != novo.arquivos || antes.chat != novo.chat {
                t.arvore.update(cx, |a, cx| {
                    a.definir_ativa(novo.arquivos, cx);
                    a.definir_chat(novo.chat);
                });
            }
            if antes.git != novo.git {
                t.git.update(cx, |g, cx| g.definir_ativa(novo.git, cx));
            }
            if antes.travado != novo.travado {
                t.git.update(cx, |g, cx| g.definir_travado(novo.travado, cx));
                if let Some(c) = self.travados.get(&id) {
                    c.set(novo.travado);
                }
                for (_, c) in t.conteudos.borrow().iter() {
                    match c {
                        Conteudo::Diff(w) => {
                            if let Some(v) = w.upgrade() {
                                v.update(cx, |v, cx| v.definir_travado(novo.travado, cx));
                            }
                        }
                        Conteudo::Conflito(w) => {
                            if let Some(v) = w.upgrade() {
                                v.update(cx, |v, cx| v.definir_travado(novo.travado, window, cx));
                            }
                        }
                        Conteudo::Midia(_) => {}
                    }
                }
            }
            if antes.projeto_ativo != novo.projeto_ativo {
                t.editor.update(cx, |ed, _| ed.definir_ativo(novo.projeto_ativo));
                let p = id.clone();
                self.atualizar_visiveis(Some(&p), window, cx);
            }
        }
        if let Some(s) = &self.solto {
            let ativo = self.ativa.is_none();
            s.update(cx, |ed, _| ed.definir_ativo(ativo));
        }
    }

    fn travado_rc(&mut self, id: &str) -> Rc<std::cell::Cell<bool>> {
        self.travados.entry(id.to_string()).or_default().clone()
    }

    /// Layout da bancada guardado na recuperação do editor do projeto ativo (`layoutText` no App.tsx).
    pub(crate) fn enviar_layout(&self, cx: &mut Context<Self>) {
        let Some(t) = self.telas_ativas() else { return };
        let texto = serde_json::json!({
            "sideWidth": self.largura_lateral,
            "panelWidth": self.largura_painel,
            "shellHeight": self.altura_terminal,
        })
        .to_string();
        t.editor.update(cx, |ed, cx| ed.definir_layout(Some(texto), cx));
    }

    fn restaurar_layout(&mut self, texto: &str, cx: &mut Context<Self>) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(texto) else { return };
        let ler = |k: &str, min: f32| v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32).filter(|x| x.is_finite() && *x >= min);
        if let Some(x) = ler("sideWidth", crate::tema::LATERAL_MIN) {
            self.largura_lateral = x;
        }
        if let Some(x) = ler("panelWidth", crate::tema::PAINEL_MIN) {
            self.largura_painel = x;
        }
        if let Some(x) = ler("shellHeight", crate::tema::TERMINAL_MIN) {
            self.altura_terminal = x;
        }
        self.salvar_paineis();
        cx.notify();
    }

    /// Executor terminou ou o orquestrador mudou: a tela Git relê o status já.
    pub(crate) fn cutucar_git(&self, projeto: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.telas.get(projeto) {
            t.git.read(cx).cutucar();
        }
    }

    /// Solta as telas do projeto fechado; a recuperação do editor já foi gravada.
    pub(crate) fn soltar_telas(&mut self, id: &str) {
        self.telas.remove(id);
        self.travados.remove(id);
    }

    /// Ctrl+Shift+F: a seleção do editor vira a consulta; a pesquisa abre com o campo focado.
    pub(crate) fn abrir_pesquisa(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.telas_ativas() else { return };
        let texto = t.editor.read(cx).texto_selecionado(cx);
        let pesquisa = t.pesquisa.clone();
        pesquisa.update(cx, |s, cx| {
            if !texto.is_empty() {
                s.semear(&texto, window, cx);
            }
            s.focar(window, cx);
        });
        self.rota = Rota::Pesquisa;
        cx.notify();
    }

    /// F4 e Shift+F4.
    pub(crate) fn passo_pesquisa(&mut self, dir: i32, cx: &mut Context<Self>) {
        let Some(t) = self.telas_ativas() else { return };
        t.pesquisa.update(cx, |s, cx| s.passo(dir, cx));
        self.rota = Rota::Pesquisa;
        cx.notify();
    }

    /// Ctrl+clique num caminho do terminal: arquivo abre no editor na linha; pasta é revelada na árvore.
    pub(crate) fn abrir_link(&mut self, alvo: &crate::terminal::vista::Alvo, window: &mut Window, cx: &mut Context<Self>) {
        use crate::terminal::vista::Alvo as L;
        match alvo {
            L::Arquivo { caminho, linha, coluna } => {
                let op = Abertura {
                    permanente: true,
                    linha: linha.map(u64::from),
                    coluna: coluna.map(u64::from),
                    lado: None,
                };
                let p = self.ativa.clone();
                self.abrir_no_editor(p.as_deref(), caminho, op, window, cx);
            }
            L::Pasta(rel) => {
                if let Some(t) = self.telas_ativas() {
                    let c = rel.clone();
                    t.arvore.update(cx, |a, cx| a.revelar(c, window, cx));
                    self.rota = Rota::Arquivos;
                    cx.notify();
                }
            }
        }
    }

    /// Sincronizar da barra de status: recebe e, havendo commits a enviar, envia (o `sync` do App.tsx).
    pub(crate) fn sincronizar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.ativa.clone() else { return };
        let ocupado = self.terminais.read(cx).projetos.get(&Some(id.clone())).is_some_and(|p| !p.executores.is_empty());
        if ocupado {
            avisos::mostrar(Aviso::alerta("Há executores trabalhando. Espere o lote terminar.", 4000), cx);
            return;
        }
        let Some(t) = self.telas.get_mut(&id) else { return };
        if t.sincronizando || !t.guarda.entrar(window, cx) {
            return;
        }
        t.sincronizando = true;
        let (fonte, guarda, git) = (t.fonte.clone(), t.guarda.clone(), t.git.clone());
        let tarefa = cx.background_spawn(async move {
            fonte.pull()?;
            if let Ok(Estado::Repo { status }) = fonte.estado() {
                if status.ahead > 0 {
                    fonte.push()?;
                }
            }
            Ok::<(), String>(())
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(()) => avisos::mostrar(Aviso::sucesso("Alterações sincronizadas", 3000), cx),
                    Err(e) => avisos::mostrar(Aviso::erro(e, 7000), cx),
                }
                guarda.sair();
                if let Some(t) = this.telas.get_mut(&id) {
                    t.sincronizando = false;
                }
                git.read(cx).cutucar();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

