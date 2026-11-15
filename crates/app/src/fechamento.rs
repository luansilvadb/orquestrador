//! Confirmação de fechamento da janela e do projeto (o `Modal` de `closeAsk` no App.tsx): alterações não salvas
//! (salvar, descartar ou cancelar) e terminais trabalhando (encerrar ou cancelar).
use crate::bancada::Bancada;
use crate::tema::{self, cor};
use gpui_kit::{
    anchored, deferred, div, point, prelude::*, px, rgba, App, Context, Entity, FocusHandle, KeyDownEvent, MouseButton,
    SharedString, Window,
};
use gpui_kit::TestSupportExt;
use orquestrador_core::core::projects::ProjectSnapshot;
use orquestrador_editor::AreaEditor;
use orquestrador_ui::avisos::{self, Aviso};

#[derive(Clone, Debug, PartialEq)]
pub enum Escopo {
    Janela,
    Projeto(String),
}

pub struct Fechamento {
    pub escopo: Escopo,
    /// Arquivos não salvos; os de outro projeto levam "pasta: " na frente.
    pub arquivos: Vec<String>,
    /// Há terminais em execução (janela) ou trabalho no projeto.
    pub terminais: bool,
    pub trabalho: Option<ProjectSnapshot>,
    pub salvando: bool,
    /// Gravações de editores em curso no "Salvar" e se todas deram certo.
    aguardando: usize,
    tudo_salvo: bool,
    pub foco: FocusHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resposta {
    Cancelar,
    Descartar,
    Salvar,
    Fechar,
}

fn tem_trabalho(s: &ProjectSnapshot) -> bool {
    s.orchestrator_open || s.executors_running > 0 || s.executors_queued > 0
}

impl Bancada {
    /// Os editores que o fechamento considera, com o prefixo dos arquivos.
    fn editores(&self, escopo: &Escopo) -> Vec<(Entity<AreaEditor>, String)> {
        match escopo {
            Escopo::Projeto(id) => self.telas.get(id).map(|t| (t.editor.clone(), String::new())).into_iter().collect(),
            Escopo::Janela => {
                let mut v: Vec<(Entity<AreaEditor>, String)> = self.solto.iter().map(|e| (e.clone(), String::new())).collect();
                for a in &self.abas {
                    if let Some(t) = self.telas.get(&a.id) {
                        let prefixo = if self.ativa.as_deref() == Some(a.id.as_str()) {
                            String::new()
                        } else {
                            format!("{}: ", crate::backend::nome_da_pasta(&a.caminho))
                        };
                        v.push((t.editor.clone(), prefixo));
                    }
                }
                v
            }
        }
    }

    fn nao_salvos(&self, escopo: &Escopo, cx: &App) -> Vec<String> {
        self.editores(escopo)
            .into_iter()
            .flat_map(|(e, p)| e.read(cx).nao_salvos().into_iter().map(move |c| format!("{p}{c}")))
            .collect()
    }

    fn gravar_recuperacoes(&self, escopo: &Escopo, cx: &mut Context<Self>) {
        for (e, _) in self.editores(escopo) {
            e.update(cx, |e, cx| e.gravar_recuperacao(cx));
        }
    }

    fn perguntar(&mut self, escopo: Escopo, arquivos: Vec<String>, terminais: bool, trabalho: Option<ProjectSnapshot>, window: &mut Window, cx: &mut Context<Self>) {
        let foco = cx.focus_handle();
        window.focus(&foco, cx);
        self.fechamento = Some(Fechamento {
            escopo,
            arquivos,
            terminais,
            trabalho,
            salvando: false,
            aguardando: 0,
            tudo_salvo: true,
            foco,
        });
        cx.notify();
    }

    pub(crate) fn pedir_fechar_projeto(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let escopo = Escopo::Projeto(id.to_string());
        let trabalho = self.backend.projetos.snapshot(id).ok();
        let ocupado = trabalho.as_ref().is_some_and(tem_trabalho);
        let arquivos = self.nao_salvos(&escopo, cx);
        if !arquivos.is_empty() {
            self.perguntar(escopo, arquivos, ocupado, trabalho.filter(|_| ocupado), window, cx);
        } else if ocupado {
            self.perguntar(escopo, Vec::new(), true, trabalho, window, cx);
        } else {
            self.gravar_recuperacoes(&escopo, cx);
            self.fechar_confirmado(id, cx);
        }
    }

    /// `on_window_should_close`: `true` libera; senão abre a pergunta e o fluxo fecha a janela depois.
    pub fn pode_fechar_janela(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.janela_liberada {
            return true;
        }
        if self.fechamento.is_some() {
            return false;
        }
        let escopo = Escopo::Janela;
        let arquivos = self.nao_salvos(&escopo, cx);
        let rodando = self.backend.projetos.has_running();
        if arquivos.is_empty() && !rodando {
            self.gravar_recuperacoes(&escopo, cx);
            self.janela_liberada = true;
            return true;
        }
        self.perguntar(escopo, arquivos, rodando, None, window, cx);
        false
    }

    pub fn responder_fechamento(&mut self, r: Resposta, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.fechamento.as_mut() else { return };
        // Com o salvar em curso, nenhuma resposta vale: quem fecha o diálogo é o fluxo.
        if f.salvando {
            return;
        }
        let escopo = f.escopo.clone();
        match r {
            Resposta::Cancelar => {
                self.fechamento = None;
                window.focus(&self.foco.clone(), cx);
            }
            Resposta::Descartar => {
                for (e, _) in self.editores(&escopo) {
                    e.update(cx, |e, cx| e.descartar_tudo(window, cx));
                }
                self.concluir_fechamento(escopo, window, cx);
            }
            Resposta::Fechar => self.concluir_fechamento(escopo, window, cx),
            Resposta::Salvar => {
                let editores = self.editores(&escopo);
                let f = self.fechamento.as_mut().expect("fechamento");
                f.salvando = true;
                f.aguardando = editores.len();
                f.tudo_salvo = true;
                if editores.is_empty() {
                    self.salvo_um(true, window, cx);
                }
                let eu = cx.weak_entity();
                for (e, _) in editores {
                    let eu = eu.clone();
                    e.update(cx, |e, cx| {
                        e.salvar_tudo(window, cx, move |_, ok, window, cx| {
                            let eu = eu.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = eu.update(cx, |b, cx| b.salvo_um(ok, window, cx));
                            });
                        })
                    });
                }
            }
        }
        cx.notify();
    }

    fn salvo_um(&mut self, ok: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.fechamento.as_mut() else { return };
        f.aguardando = f.aguardando.saturating_sub(1);
        f.tudo_salvo &= ok;
        if f.aguardando > 0 {
            return;
        }
        let escopo = f.escopo.clone();
        let tudo = f.tudo_salvo;
        let restam = self.nao_salvos(&escopo, cx);
        if tudo && restam.is_empty() {
            self.concluir_fechamento(escopo, window, cx);
            return;
        }
        avisos::mostrar(Aviso::erro("Não foi possível salvar tudo. Nada foi fechado.", 7000), cx);
        let f = self.fechamento.as_mut().expect("fechamento");
        f.salvando = false;
        if restam.is_empty() {
            self.fechamento = None;
        } else {
            f.arquivos = restam;
        }
        cx.notify();
    }

    fn concluir_fechamento(&mut self, escopo: Escopo, window: &mut Window, cx: &mut Context<Self>) {
        self.fechamento = None;
        self.gravar_recuperacoes(&escopo, cx);
        match escopo {
            Escopo::Projeto(id) => {
                self.fechar_confirmado(&id, cx);
                window.focus(&self.foco.clone(), cx);
            }
            Escopo::Janela => {
                self.janela_liberada = true;
                window.remove_window();
            }
        }
        cx.notify();
    }

    fn tecla_fechamento(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Foco inicial no Cancelar: Enter recusa o fechamento, como no React; Esc também.
        if matches!(e.keystroke.key.as_str(), "escape" | "enter") {
            cx.stop_propagation();
            self.responder_fechamento(Resposta::Cancelar, window, cx);
        }
    }

    /// O diálogo, por cima de tudo, com a máscara escura (clique nela cancela).
    pub(crate) fn dialogo_fechamento(&self, window: &Window, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let f = self.fechamento.as_ref()?;
        let n = f.arquivos.len();
        let titulo = if n > 0 {
            "Fechar com alterações não salvas?"
        } else if f.trabalho.is_some() {
            "Fechar o projeto?"
        } else {
            "Fechar o Orquestrador?"
        };
        let paragrafo = |t: String| {
            div().text_size(tema::TEXTO_BODY).line_height(px(22.4)).text_color(cor::text_secondary()).child(t)
        };
        let mut corpo = div().flex().flex_col().gap(px(8.)).pt(px(6.));
        if n > 0 {
            let lista = if n == 1 { format!("em {}", f.arquivos[0]) } else { format!("em {n} arquivos: {}", f.arquivos.join(", ")) };
            corpo = corpo
                .child(paragrafo(format!("Há alterações não salvas {lista}.")))
                .child(paragrafo("Salvar grava os arquivos. Descartar joga a edição fora.".into()));
            if f.terminais {
                corpo = corpo.child(paragrafo(
                    "Encerrar fecha os terminais em execução, inclusive os shells: o trabalho dos executores para no meio."
                        .into(),
                ));
            }
        } else if let Some(t) = &f.trabalho {
            let mut s = String::new();
            if t.orchestrator_open {
                s.push_str("O orquestrador aberto será encerrado. ");
            }
            if t.executors_running > 0 {
                s.push_str(&format!("{} executores em execução serão encerrados. ", t.executors_running));
            }
            if t.executors_queued > 0 {
                s.push_str(&format!("{} executores na fila serão removidos. ", t.executors_queued));
            }
            s.push_str("Os shells deste projeto também serão fechados.");
            corpo = corpo.child(paragrafo(s));
        } else {
            corpo = corpo.child(paragrafo(
                "Todos os terminais serão encerrados, inclusive os shells. O trabalho dos executores para no meio.".into(),
            ));
        }
        let salvando = f.salvando;
        let botao = |id: &'static str, rotulo: SharedString, tipo: u8| {
            // 0: padrão (com o foco), 1: perigo suave, 2: preenchido.
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .h(tema::CAMPO)
                .px(px(15.))
                .rounded(tema::RAIO_SM)
                .text_size(tema::TEXTO_BODY)
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .when(tipo == 0, |d| {
                    d.bg(cor::pitch_black())
                        .border_1()
                        .border_color(cor::focus_blue())
                        .text_color(cor::text_primary())
                        .hover(|s| s.border_color(rgba(0xffffff4d)))
                })
                .when(tipo == 1, |d| {
                    d.bg(cor::pitch_black())
                        .border_1()
                        .border_color(cor::signal_error())
                        .text_color(cor::signal_error())
                        .hover(|s| s.bg(cor::signal_error().opacity(0.1)))
                })
                .when(tipo == 2, |d| d.bg(rgba(0xffffff1a)).text_color(cor::text_primary()).hover(|s| s.bg(rgba(0xffffff26))))
                .when(salvando && tipo != 2, |d| d.opacity(0.4))
                .child(rotulo)
                .test_support()
        };
        let encerrar = f.terminais;
        let mut rodape = div().flex().flex_wrap().justify_end().gap(px(8.)).pt(px(24.)).child(
            botao("fechar-cancelar", "Cancelar".into(), 0)
                .on_click(cx.listener(|this, _, w, cx| this.responder_fechamento(Resposta::Cancelar, w, cx))),
        );
        if n > 0 {
            rodape = rodape
                .child(
                    botao(
                        "fechar-descartar",
                        if encerrar { "Descartar e encerrar todos" } else { "Descartar" }.into(),
                        1,
                    )
                    .on_click(cx.listener(|this, _, w, cx| this.responder_fechamento(Resposta::Descartar, w, cx))),
                )
                .child(
                    botao(
                        "fechar-salvar",
                        if salvando {
                            "Salvando…"
                        } else if encerrar {
                            "Salvar e encerrar todos"
                        } else {
                            "Salvar"
                        }
                        .into(),
                        2,
                    )
                    .on_click(cx.listener(|this, _, w, cx| this.responder_fechamento(Resposta::Salvar, w, cx))),
                );
        } else {
            rodape = rodape.child(
                botao(
                    "fechar-encerrar",
                    if f.trabalho.is_some() { "Fechar projeto e encerrar" } else { "Fechar e encerrar todos" }.into(),
                    1,
                )
                .on_click(cx.listener(|this, _, w, cx| this.responder_fechamento(Resposta::Fechar, w, cx))),
            );
        }
        let caixa = div()
            .id("fechar-confirmacao")
            .track_focus(&f.foco)
            .on_key_down(cx.listener(Self::tecla_fechamento))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .occlude()
            .w(px(420.))
            .p(px(20.))
            .rounded(tema::RAIO_MD)
            .border_1()
            .border_color(cor::hairline())
            .bg(cor::raised_black())
            .shadow_lg()
            .text_color(cor::text_primary())
            .child(div().text_size(tema::TEXTO_TITLE).font_weight(gpui_kit::FontWeight::SEMIBOLD).line_height(px(22.4)).child(titulo))
            .child(corpo)
            .child(rodape)
            .test_support();
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("fechar-mascara")
                        .w(window.viewport_size().width)
                        .h(window.viewport_size().height)
                        .occlude()
                        .bg(rgba(0x00000073))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, w, cx| this.responder_fechamento(Resposta::Cancelar, w, cx)),
                        )
                        .child(caixa),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
