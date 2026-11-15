//! Aba de conflito de merge (porte de `src/editor/conflict-view.tsx` e do `saveConflict` do workspace): escolha por trecho
//! (marcadores normais e diff3), restauração de um lado, aceite de remoção e o texto final editável. O rascunho é a fonte:
//! os trechos são lidos do texto dele a cada render, e a edição à mão vale junto com as escolhas.
use crate::comum::{self, botao};
use crate::conflito_texto::{contar_blocos, ler_conflito, montar_conflito, tem_marcadores, Escolha};
use crate::fonte::{Conflict, Fonte};
use crate::guarda::GuardaGit;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::{Sizable as _, WindowExt as _};
use orquestrador_editor::armazem::Buffer;
use orquestrador_editor::documento::{self, ComTemaDoEditor, Documento};
use orquestrador_editor::minimapa::Minimapa;
use gpui_kit::{
    actions, div, prelude::*, px, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, Subscription,
    Task, Window,
};
use orquestrador_ui::tema::{self, cor};

actions!(visao_conflito, [Salvar]);

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("ctrl-s", Salvar, Some("VisaoConflito"))]);
}

/// O rascunho de um conflito: o texto carregado (`origem`), o atual e a revisão da pasta que a tela leu. A bancada pode
/// guardá-lo (recuperação) e devolvê-lo com [`VisaoConflito::restaurar`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RascunhoConflito {
    pub caminho: String,
    pub origem: String,
    pub texto: String,
    pub revisao: String,
}

impl RascunhoConflito {
    pub fn sujo(&self) -> bool {
        self.texto != self.origem
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventoConflito {
    /// O rascunho mudou (para a aba mostrar sujo e a recuperação guardar).
    Rascunho(RascunhoConflito),
    /// Gravou ou resolveu: a lateral relê o status.
    AtualizarGit,
    /// Resolvido ou removido: a aba fecha.
    Fechar,
}

pub struct VisaoConflito {
    fonte: Fonte,
    guarda: GuardaGit,
    caminho: String,
    conflito: Option<Conflict>,
    rascunho: Option<RascunhoConflito>,
    erro: String,
    velho: bool,
    ocupado: bool,
    marcando: bool,
    travado: bool,
    arquivo_sujo: bool,
    editor: Entity<EditorState>,
    _documento: Documento,
    foco: FocusHandle,
    _editor: Subscription,
    _tarefas: Vec<Task<()>>,
}

impl EventEmitter<EventoConflito> for VisaoConflito {}

impl Focusable for VisaoConflito {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

impl VisaoConflito {
    /// `rascunho` vem da recuperação (ou `None`): com ele, a leitura só confere a revisão.
    pub fn new(
        fonte: Fonte,
        guarda: GuardaGit,
        caminho: String,
        rascunho: Option<RascunhoConflito>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // O texto final no editor do app, com a mesma configuração do de arquivo (realce, calha, quebra, guias), como o
        // `CodeEditor` que o React usa aqui.
        let doc = Documento::novo(Buffer::do_disco(&format!("conflito:{caminho}"), &caminho, String::new(), false, false), window, cx);
        let editor = doc.editor.clone();
        let sub = cx.subscribe(&editor, |this: &mut Self, editor, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let texto = editor.read(cx).value().to_string();
                if let Some(r) = this.rascunho.as_mut() {
                    if r.texto != texto {
                        r.texto = texto;
                        cx.emit(EventoConflito::Rascunho(r.clone()));
                        cx.notify();
                    }
                }
            }
        });
        let mut v = VisaoConflito {
            fonte,
            guarda,
            caminho,
            conflito: None,
            rascunho,
            erro: String::new(),
            velho: false,
            ocupado: false,
            marcando: false,
            travado: false,
            arquivo_sujo: false,
            editor,
            _documento: doc,
            foco: cx.focus_handle(),
            _editor: sub,
            _tarefas: Vec::new(),
        };
        if let Some(r) = v.rascunho.clone() {
            v.por_texto(&r.texto, window, cx);
        }
        v.carregar(false, window, cx);
        v
    }

    pub fn caminho(&self) -> &str {
        &self.caminho
    }

    pub fn rascunho(&self) -> Option<&RascunhoConflito> {
        self.rascunho.as_ref()
    }

    /// A pasta mudou desde o rascunho (a revisão lida não é a dele).
    pub fn velho(&self) -> bool {
        self.velho
    }

    /// A revisão do conflito como a tela leu por último.
    pub fn revisao_lida(&self) -> Option<String> {
        self.conflito.as_ref().map(|c| c.revision.clone())
    }

    pub fn definir_travado(&mut self, travado: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.travado = travado;
        self.aplicar_somente_leitura(window, cx);
        cx.notify();
    }

    /// O arquivo também está aberto no editor com alterações: gravar a resolução fica bloqueado.
    pub fn definir_arquivo_sujo(&mut self, sujo: bool, cx: &mut Context<Self>) {
        self.arquivo_sujo = sujo;
        cx.notify();
    }

    /// A aba voltou a ser a atual: relê a pasta (a revisão do rascunho pode ter ficado velha).
    pub fn mostrar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.carregar(false, window, cx);
    }

    fn aplicar_somente_leitura(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ro = self.travado || self.marcando;
        self.editor.update(cx, |e, cx| e.set_readonly(ro, cx));
        let _ = window;
    }

    fn por_texto(&mut self, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        let texto = texto.to_string();
        self.editor.update(cx, |e, cx| {
            if e.value().as_ref() != texto {
                e.set_value(texto, window, cx);
            }
        });
    }

    fn trocar_rascunho(&mut self, r: RascunhoConflito, window: &mut Window, cx: &mut Context<Self>) {
        self.por_texto(&r.texto, window, cx);
        self.rascunho = Some(r.clone());
        cx.emit(EventoConflito::Rascunho(r));
        cx.notify();
    }

    /// `reset` troca o rascunho pelo texto atual da pasta (só depois da confirmação).
    pub fn carregar(&mut self, reset: bool, window: &mut Window, cx: &mut Context<Self>) {
        let fonte = self.fonte.clone();
        let caminho = self.caminho.clone();
        let tarefa = cx.background_spawn(async move { fonte.conflict(&caminho) });
        self._tarefas.push(cx.spawn_in(window, async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match r {
                    Err(e) => this.erro = e,
                    Ok(c) => {
                        this.erro.clear();
                        match this.rascunho.clone() {
                            Some(r) if !reset => this.velho = r.revisao != c.revision,
                            _ => {
                                let texto = c.worktree.clone().unwrap_or_default();
                                let r = RascunhoConflito {
                                    caminho: this.caminho.clone(),
                                    origem: texto.clone(),
                                    texto,
                                    revisao: c.revision.clone(),
                                };
                                this.velho = false;
                                this.trocar_rascunho(r, window, cx);
                            }
                        }
                        this.conflito = Some(c);
                    }
                }
                cx.notify();
            });
        }));
    }

    /// Escolhe o lado de um trecho do rascunho atual; os demais ficam com marcadores.
    pub fn escolher(&mut self, indice: usize, escolha: Escolha, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rascunho.clone() else { return };
        let (pecas, malformado) = ler_conflito(&r.texto);
        if malformado || self.conflito.is_none() {
            return;
        }
        let escolhas: Vec<Escolha> =
            (0..contar_blocos(&pecas)).map(|i| if i == indice { escolha } else { Escolha::Pendente }).collect();
        self.trocar_rascunho(RascunhoConflito { texto: montar_conflito(&pecas, &escolhas), ..r }, window, cx);
    }

    /// Restaura a versão de um lado no rascunho. Não grava.
    pub fn restaurar_lado(&mut self, texto: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(r) = self.rascunho.clone() {
            self.trocar_rascunho(RascunhoConflito { texto, ..r }, window, cx);
        }
    }

    /// Grava a resolução; `marcar` tira do conflito (sem marcadores no texto).
    pub fn salvar(&mut self, marcar: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rascunho.clone() else { return };
        if self.ocupado || self.travado || (!r.sujo() && !marcar) {
            return;
        }
        if marcar && tem_marcadores(&r.texto) {
            comum::falha("Ainda há marcadores de conflito no texto. Remova-os antes de marcar como resolvido.", window, cx);
            return;
        }
        if self.arquivo_sujo {
            comum::falha("Salve ou descarte a edição aberta deste arquivo antes de gravar a resolução.", window, cx);
            return;
        }
        if !self.guarda.entrar(window, cx) {
            return;
        }
        self.ocupado = true;
        self.marcando = marcar;
        self.aplicar_somente_leitura(window, cx);
        cx.notify();
        let fonte = self.fonte.clone();
        let (caminho, enviado, revisao) = (self.caminho.clone(), r.texto.clone(), r.revisao.clone());
        let tarefa = cx.background_spawn(async move {
            fonte.write_resolution(&caminho, &enviado, &revisao, marcar)?;
            if marcar {
                Ok(None)
            } else {
                fonte.conflict(&caminho).map(Some)
            }
        });
        let enviado = r.texto.clone();
        self._tarefas.push(cx.spawn_in(window, async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.guarda.sair();
                this.ocupado = false;
                this.marcando = false;
                this.aplicar_somente_leitura(window, cx);
                match r {
                    Err(e) => comum::falha(e, window, cx),
                    Ok(None) => {
                        cx.emit(EventoConflito::AtualizarGit);
                        cx.emit(EventoConflito::Fechar);
                    }
                    // Quem digitou durante a gravação continua com o texto: só a base e a revisão trocam.
                    Ok(Some(fresco)) => {
                        if let Some(atual) = this.rascunho.clone() {
                            let novo = RascunhoConflito { origem: enviado, revisao: fresco.revision.clone(), ..atual };
                            this.rascunho = Some(novo.clone());
                            cx.emit(EventoConflito::Rascunho(novo));
                        }
                        this.velho = false;
                        this.conflito = Some(fresco);
                        cx.emit(EventoConflito::AtualizarGit);
                    }
                }
                cx.notify();
            });
        }));
    }

    pub fn aceitar_remocao(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = &self.conflito else { return };
        let revisao = c.revision.clone();
        let caminho = self.caminho.clone();
        let this = cx.entity();
        let corpo = format!("O arquivo {caminho} sai do projeto, conforme o lado que o apagou.");
        window.open_alert_dialog(cx, move |alerta, _, _| {
            let (this, revisao) = (this.clone(), revisao.clone());
            alerta
                .title("Aceitar a remoção do arquivo?")
                .description(corpo.clone())
                .ok_text("Aceitar remoção")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Cancelar")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let revisao = revisao.clone();
                    this.update(cx, |this, cx| this.remover_confirmado(revisao, window, cx));
                    true
                })
        });
    }

    /// Aceita a remoção depois da confirmação, com a revisão de quando a tela leu; pasta mudada é recusada pelo Git.
    pub fn remover_confirmado(&mut self, revisao: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.ocupado || self.travado {
            return;
        }
        self.ocupado = true;
        let fonte = self.fonte.clone();
        let caminho = self.caminho.clone();
        let tarefa = cx.background_spawn(async move { fonte.accept_deletion(&caminho, &revisao) });
        self._tarefas.push(cx.spawn_in(window, async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.ocupado = false;
                match r {
                    Err(e) => comum::falha(e, window, cx),
                    Ok(()) => {
                        cx.emit(EventoConflito::AtualizarGit);
                        cx.emit(EventoConflito::Fechar);
                    }
                }
                cx.notify();
            });
        }));
    }

    fn atualizar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.velho {
            self.carregar(false, window, cx);
            return;
        }
        let this = cx.entity();
        window.open_alert_dialog(cx, move |alerta, _, _| {
            let this = this.clone();
            alerta
                .title("Recarregar do Git?")
                .description("O rascunho deste conflito será trocado pelo texto atual da pasta. As escolhas feitas aqui se perdem.")
                .ok_text("Recarregar")
                .cancel_text("Cancelar")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    this.update(cx, |this, cx| this.carregar(true, window, cx));
                    true
                })
        });
    }
}

fn faixa() -> gpui_kit::Div {
    div().flex().flex_none().border_b_1().border_color(cor::hairline()).px(px(12.))
}

fn texto(t: impl Into<gpui_kit::SharedString>, c: gpui_kit::Hsla) -> gpui_kit::Div {
    div().text_size(tema::TEXTO_LABEL).text_color(c).child(t.into())
}

impl Render for VisaoConflito {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let raiz = div()
            .key_context("VisaoConflito")
            .track_focus(&self.foco)
            .on_action(cx.listener(|this, _: &Salvar, w, cx| this.salvar(false, w, cx)))
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(cor::pitch_black())
            .text_color(cor::text_primary())
            .font_family(tema::FONTE);
        let aviso = |t: String, perigo: bool| {
            div()
                .flex()
                .flex_1()
                .p(px(16.))
                .text_size(tema::TEXTO_BODY)
                .text_color(if perigo { comum::perigo() } else { comum::descricao() })
                .child(t)
        };
        let (Some(c), Some(r)) = (self.conflito.clone(), self.rascunho.clone()) else {
            return raiz.child(if !self.erro.is_empty() && self.conflito.is_none() {
                aviso(self.erro.clone(), true)
            } else {
                aviso("Lendo o conflito…".into(), false)
            });
        };
        if c.binary {
            return raiz.child(aviso(
                "Conflito em arquivo binário: escolha um lado pelo Git; não há texto para resolver aqui.".into(),
                false,
            ));
        }
        let (pecas, malformado) = ler_conflito(&r.texto);
        let n = contar_blocos(&pecas);
        let resolvivel = !malformado;
        let marcadores = tem_marcadores(&r.texto);
        let remocao = c.ours.is_none() || c.theirs.is_none();
        let ausente = c.worktree.is_none();
        let bloq = self.travado || self.ocupado;
        let pode_marcar = resolvivel && !marcadores && !bloq && !self.arquivo_sujo;
        let pode_salvar = r.sujo() && !bloq && !self.arquivo_sujo;
        let topo = faixa()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .py(px(4.))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(tema::FONTE_MONO)
                    .text_size(tema::TEXTO_LABEL)
                    .text_color(comum::descricao())
                    .child(self.caminho.clone()),
            )
            .child(texto("conflito", comum::descricao()))
            .when(self.velho, |d| d.child(texto("A pasta mudou desde o seu rascunho.", comum::perigo())))
            .when(!self.erro.is_empty(), |d| d.child(texto(self.erro.clone(), comum::perigo())))
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        botao("atualizar", if self.velho { "Recarregar do Git" } else { "Atualizar" }, !self.ocupado)
                            .on_click(cx.listener(|this, _, w, cx| this.atualizar(w, cx))),
                    )
                    .child(botao("salvar", "Salvar", pode_salvar).on_click(cx.listener(move |this, _, w, cx| {
                        if pode_salvar {
                            this.salvar(false, w, cx)
                        }
                    })))
                    .child(botao("marcar", "Marcar como resolvido", pode_marcar).on_click(cx.listener(move |this, _, w, cx| {
                        if pode_marcar {
                            this.salvar(true, w, cx)
                        }
                    }))),
            );
        let mut corpo = raiz.child(topo);
        if self.arquivo_sujo {
            corpo = corpo.child(faixa().items_center().h(px(28.)).child(texto(
                "Este arquivo também está aberto no editor com alterações. Salve ou descarte antes de gravar a resolução.",
                comum::perigo(),
            )));
        }
        if ausente {
            let frase = if remocao {
                "Um lado apagou o arquivo: aceite a remoção ou restaure a versão de um lado."
            } else {
                "Restaure a versão de um lado para editar e salvar."
            };
            corpo = corpo.child(
                faixa()
                    .flex_col()
                    .gap(px(6.))
                    .py(px(6.))
                    .child(texto(format!("O arquivo não existe na pasta. {frase}"), comum::descricao()))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .when_some(c.ours.clone(), |d, t| {
                                d.child(botao("restaurar-atual", "Restaurar versão atual", !bloq).on_click(cx.listener(
                                    move |this, _, w, cx| {
                                        if !this.travado && !this.ocupado {
                                            this.restaurar_lado(t.clone(), w, cx)
                                        }
                                    },
                                )))
                            })
                            .when_some(c.theirs.clone(), |d, t| {
                                d.child(botao("restaurar-recebida", "Restaurar versão recebida", !bloq).on_click(cx.listener(
                                    move |this, _, w, cx| {
                                        if !this.travado && !this.ocupado {
                                            this.restaurar_lado(t.clone(), w, cx)
                                        }
                                    },
                                )))
                            }),
                    ),
            );
        }
        if remocao && !ausente {
            corpo = corpo.child(faixa().items_center().py(px(4.)).child(texto(
                "Um lado apagou o arquivo. Marcar como resolvido mantém o arquivo como está.",
                comum::descricao(),
            )));
        }
        if remocao {
            corpo = corpo.child(
                faixa().items_center().py(px(4.)).child(
                    botao("aceitar-remocao", "Aceitar a remoção", !bloq)
                        .text_color(comum::perigo())
                        .on_click(cx.listener(|this, _, w, cx| {
                            if !this.travado && !this.ocupado {
                                this.aceitar_remocao(w, cx)
                            }
                        })),
                ),
            );
        }
        if !ausente && !resolvivel {
            corpo = corpo.child(faixa().py(px(6.)).child(texto(
                if malformado { "Marcadores de conflito incompletos: edite o texto final à mão." } else { "Edite o texto final à mão." },
                comum::descricao(),
            )));
        }
        if !ausente && resolvivel && n > 0 {
            let mut lista = div()
                .id("trechos")
                .flex()
                .flex_col()
                .flex_none()
                .gap(px(6.))
                .p(px(8.))
                .max_h(px(160.))
                .overflow_y_scroll()
                .border_b_1()
                .border_color(cor::hairline())
                .child(texto(
                    format!("{} no texto atual:", if n == 1 { "1 trecho com conflito".to_string() } else { format!("{n} trechos com conflito") }),
                    comum::descricao(),
                ));
            for i in 0..n {
                let mut linha = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(div().flex_1().min_w_0().text_size(tema::TEXTO_LABEL).child(format!("Trecho {}", i + 1)));
                for e in [Escolha::Atual, Escolha::Recebida, Escolha::Ambas] {
                    linha = linha.child(botao(("escolha", i * 3 + e as usize), e.rotulo(), !bloq).on_click(cx.listener(
                        move |this, _, w, cx| {
                            if !this.travado && !this.ocupado {
                                this.escolher(i, e, w, cx)
                            }
                        },
                    )));
                }
                lista = lista.child(linha);
            }
            corpo = corpo.child(lista);
        }
        corpo.child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .bg(documento::cor::fundo())
                .child(
                    div().flex_1().min_w_0().h_full().child(ComTemaDoEditor(Some(
                        // Sem o recuo vertical do Kit: a primeira linha encosta no topo, como o CodeMirror do React.
                        Editor::new(&self.editor)
                            .appearance(false)
                            .bordered(false)
                            .xsmall()
                            .h_full()
                            .font_family(documento::FONTE_EDITOR)
                            .text_size(documento::TAMANHO_EDITOR)
                            .line_height(documento::LINHA_EDITOR)
                            .into_any_element(),
                    ))),
                )
                .child(Minimapa::new(self.editor.clone())),
        )
    }
}
