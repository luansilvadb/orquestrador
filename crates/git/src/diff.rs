//! Comparador do editor central (porte de `DiffTab` e `CommitTab` de `src/editor/diff-view.tsx`): o arquivo inteiro no
//! comparador próprio (`crate::comparador`), lado a lado por padrão ou unificado, com realce `syntect`/Dark+ e diferença
//! por palavra. No diff do índice, as ações por bloco (preparar, tirar do preparo, descartar) e descartar o arquivo; no
//! commit, só leitura.
use crate::comparador::{limitar_pct, Comparador, Corpo};
use crate::comparar::{self, BlocoPatch, Comparacao, Intervalo, TipoUnidade};
use crate::comum::{self, acao, botao, Alvo};
use crate::fonte::{Diff as DiffGit, Fonte, HunkRef, ReviewDiff};
use crate::guarda::GuardaGit;
use crate::sondagem::Sondagem;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{
    actions, div, prelude::*, px, rgb, App, Context, CursorStyle, EventEmitter, FocusHandle, Focusable, KeyBinding, KeyDownEvent,
    MouseButton, MouseDownEvent, Task, Window,
};
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::TooltipTexto as _;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

actions!(visao_diff, [ProximaMudanca, MudancaAnterior, Copiar, RolarAcima, RolarAbaixo, PaginaAcima, PaginaAbaixo, Inicio, Fim]);

/// Atalhos F8 e Shift+F8 (vale em toda a aba), a cópia da seleção e a rolagem pelo teclado, como no corpo focado do React.
pub fn init(cx: &mut App) {
    let c = Some("VisaoDiff");
    cx.bind_keys([
        KeyBinding::new("f8", ProximaMudanca, c),
        KeyBinding::new("shift-f8", MudancaAnterior, c),
        KeyBinding::new("ctrl-c", Copiar, c),
        KeyBinding::new("ctrl-insert", Copiar, c),
        KeyBinding::new("up", RolarAcima, c),
        KeyBinding::new("down", RolarAbaixo, c),
        KeyBinding::new("pageup", PaginaAcima, c),
        KeyBinding::new("pagedown", PaginaAbaixo, c),
        KeyBinding::new("home", Inicio, c),
        KeyBinding::new("end", Fim, c),
        KeyBinding::new("ctrl-home", Inicio, c),
        KeyBinding::new("ctrl-end", Fim, c),
    ]);
}

/// Intervalo da releitura do diff do índice (`POLL_MS`).
pub const RELEITURA: Duration = Duration::from_millis(2500);

pub use crate::comparador::cores;

/// Lado a lado ou unificado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modo {
    LadoALado,
    Unificado,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventoDiff {
    /// Uma ação mudou o índice ou a pasta: a lateral relê o status.
    AtualizarGit,
    /// Abrir o arquivo na linha do bloco atual.
    AbrirLinha { caminho: String, linha: usize },
}

enum Leitura {
    Indice(ReviewDiff),
    Commit(DiffGit),
}

/// A comparação desenhada: o modelo das unidades e o estado do comparador.
struct Mostrado {
    patch: String,
    comparacao: Comparacao,
    estado: Rc<RefCell<Comparador>>,
    _realce: Task<()>,
}

pub struct VisaoDiff {
    fonte: Fonte,
    guarda: GuardaGit,
    alvo: Alvo,
    leitura: Option<Leitura>,
    erro: String,
    lado: bool,
    atual: usize,
    ocupado: bool,
    travado: bool,
    rascunho: bool,
    ativa: bool,
    tarde: bool,
    mostrado: Option<Mostrado>,
    sondagem: Option<Sondagem>,
    foco: FocusHandle,
    foco_divisor: FocusHandle,
    /// Ponteiro parado sobre o divisor há 300 ms: a linha fica azul e larga.
    divisor_realce: bool,
    divisor_sobre: bool,
    _tarefas: Vec<Task<()>>,
}

impl EventEmitter<EventoDiff> for VisaoDiff {}

impl Focusable for VisaoDiff {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

impl VisaoDiff {
    /// `alvo` é `Alvo::Diff` (índice, relido a cada 2,5 s com a aba ativa) ou `Alvo::Commit` (lido uma vez).
    pub fn new(fonte: Fonte, guarda: GuardaGit, alvo: Alvo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut v = VisaoDiff {
            fonte,
            guarda,
            alvo,
            leitura: None,
            erro: String::new(),
            lado: true,
            atual: 0,
            ocupado: false,
            travado: false,
            rascunho: false,
            ativa: false,
            tarde: false,
            mostrado: None,
            sondagem: None,
            foco: cx.focus_handle(),
            foco_divisor: cx.focus_handle(),
            divisor_realce: false,
            divisor_sobre: false,
            _tarefas: Vec::new(),
        };
        // "Lendo…" só aparece depois de 200 ms, para a leitura rápida não piscar.
        v._tarefas.push(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(200)).await;
            let _ = this.update(cx, |this, cx| {
                this.tarde = true;
                cx.notify();
            });
        }));
        if matches!(v.alvo, Alvo::Commit { .. }) {
            v.ler_commit(cx);
        }
        v.definir_ativa(true, window, cx);
        v
    }

    pub fn alvo(&self) -> &Alvo {
        &self.alvo
    }

    /// Aba visível e tela ativa: só então o diff do índice é relido. Ao voltar, relê na hora.
    pub fn definir_ativa(&mut self, ativa: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.ativa = ativa;
        if !matches!(self.alvo, Alvo::Diff { .. }) {
            return;
        }
        if ativa && self.sondagem.is_none() {
            self.sondagem = Some(Sondagem::iniciar(RELEITURA, |this: &mut Self, _, cx| this.ler_indice(cx), window, cx));
        } else if !ativa {
            self.sondagem = None;
        }
    }

    /// Executores vivos: as mutações ficam desligadas.
    pub fn definir_travado(&mut self, travado: bool, cx: &mut Context<Self>) {
        self.travado = travado;
        cx.notify();
    }

    /// Há rascunho aberto deste arquivo no editor: descartar fica bloqueado.
    pub fn definir_rascunho(&mut self, rascunho: bool, cx: &mut Context<Self>) {
        self.rascunho = rascunho;
        cx.notify();
    }

    /// O modo pedido (arquivo novo é sempre desenhado unificado).
    pub fn modo(&self) -> Modo {
        if self.lado { Modo::LadoALado } else { Modo::Unificado }
    }

    pub fn alternar_modo(&mut self, cx: &mut Context<Self>) {
        self.lado = !self.lado;
        cx.notify();
    }

    /// Proporção do original no lado a lado (15 a 85); vale para a aba e sobrevive à troca de modo.
    pub fn proporcao(&self) -> f32 {
        self.mostrado.as_ref().map_or(50., |m| m.estado.borrow().pct)
    }

    pub fn definir_proporcao(&mut self, pct: f32, cx: &mut Context<Self>) {
        if let Some(m) = &self.mostrado {
            m.estado.borrow_mut().pct = limitar_pct(pct);
            cx.notify();
        }
    }

    /// Rolagem do corpo (o `scrollTop`).
    pub fn rolagem(&self) -> f32 {
        self.mostrado.as_ref().map_or(0., |m| m.estado.borrow().rolagem)
    }

    /// Faixa (topo, base) da unidade atual no palco, depois do primeiro desenho.
    pub fn faixa_atual(&self) -> Option<(f32, f32)> {
        let m = self.mostrado.as_ref()?;
        let e = m.estado.borrow();
        e.faixa(e.atual?)
    }

    /// Texto selecionado no comparador.
    pub fn selecao(&self) -> Option<String> {
        self.mostrado.as_ref().and_then(|m| m.estado.borrow().texto_selecionado())
    }

    /// Unidade atual (bloco no índice, trecho no commit), limitada à última.
    pub fn atual(&self) -> Option<usize> {
        let n = self.total();
        (n > 0).then(|| self.atual.min(n - 1))
    }

    fn total(&self) -> usize {
        match &self.leitura {
            Some(Leitura::Indice(r)) if r.untracked => 0,
            Some(Leitura::Indice(r)) => comparar::blocos_patch(&r.diff.patch).len(),
            Some(_) => self.mostrado.as_ref().map_or(0, |m| m.comparacao.unidades.len()),
            None => 0,
        }
    }

    pub fn passo(&mut self, por: i32, cx: &mut Context<Self>) {
        let n = self.total();
        if n == 0 {
            return;
        }
        let cur = self.atual.min(n - 1) as i32;
        self.escolher((cur + por).clamp(0, n as i32 - 1) as usize, cx);
    }

    /// Escolhe a unidade; a rolagem vai a ela quando a unidade muda (o `jump` do React).
    pub fn escolher(&mut self, unidade: usize, cx: &mut Context<Self>) {
        self.atual = unidade;
        cx.notify();
    }

    fn rolar(&mut self, por: impl FnOnce(f32, f32) -> f32, cx: &mut Context<Self>) {
        if let Some(m) = &self.mostrado {
            let mut e = m.estado.borrow_mut();
            let alto = f32::from(e.caixa.size.height);
            let y = por(e.rolagem, alto);
            e.rolar_para(y);
            cx.notify();
        }
    }

    fn ler_indice(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let Alvo::Diff { lado, caminho } = self.alvo.clone() else { return Task::ready(()) };
        let fonte = self.fonte.clone();
        let tarefa = cx.background_spawn(async move { fonte.review_diff(&caminho, lado.staged()) });
        cx.spawn(async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        this.erro.clear();
                        this.receber(Leitura::Indice(r), cx);
                    }
                    Err(e) => this.erro = e,
                }
                cx.notify();
            });
        })
    }

    fn ler_commit(&mut self, cx: &mut Context<Self>) {
        let Alvo::Commit { hash, caminho } = self.alvo.clone() else { return };
        let fonte = self.fonte.clone();
        let tarefa = cx.background_spawn(async move { fonte.commit_diff(&hash, &caminho) });
        self._tarefas.push(cx.spawn(async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(d) => this.receber(Leitura::Commit(d), cx),
                    Err(e) => this.erro = e,
                }
                cx.notify();
            });
        }));
    }

    /// Resposta nova: só refaz o comparador quando o patch mostrado mudou (o poll igual não rola nem pisca). Refeito, o
    /// comparador mantém a rolagem e a proporção, e só rola se a unidade atual mudar.
    fn receber(&mut self, leitura: Leitura, cx: &mut Context<Self>) {
        let (mostrar, blocos) = match &leitura {
            Leitura::Indice(r) => {
                let ok = !r.display.binary && !r.display.too_large && !r.diff.too_large && !r.diff.binary && !r.diff.patch.is_empty();
                let blocos = (!r.untracked && !r.diff.patch.is_empty()).then(|| comparar::intervalos(&r.diff.patch));
                (ok.then(|| r.display.patch.clone()), blocos)
            }
            Leitura::Commit(d) => ((!d.binary && !d.too_large && !d.patch.is_empty()).then(|| d.patch.clone()), None),
        };
        self.leitura = Some(leitura);
        let Some(patch) = mostrar else {
            self.mostrado = None;
            return;
        };
        if self.mostrado.as_ref().is_some_and(|m| m.patch == patch) {
            return;
        }
        let anterior = self.mostrado.take();
        self.mostrado = montar(&patch, blocos.as_deref(), self.alvo.caminho(), anterior.as_ref(), cx);
    }

    fn bloco_ref(&self, i: usize) -> Option<(HunkRef, BlocoPatch, String)> {
        let Some(Leitura::Indice(r)) = &self.leitura else { return None };
        let blocos = comparar::blocos_patch(&r.diff.patch);
        let b = blocos.get(i)?.clone();
        Some((
            HunkRef { path: self.alvo.caminho().to_string(), index: i, header: b.cabecalho.clone(), revision: r.revision.clone() },
            b,
            comparar::cabecalho_arquivo(&r.diff.patch).to_string(),
        ))
    }

    /// Uma operação por vez; erro em aviso e relê de qualquer jeito.
    fn rodar(
        &mut self,
        trabalho: impl FnOnce(&dyn crate::FonteGit) -> Result<(), String> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ocupado || self.travado || !self.guarda.entrar(window, cx) {
            return;
        }
        self.ocupado = true;
        cx.notify();
        let fonte = self.fonte.clone();
        let tarefa = cx.background_spawn(async move { trabalho(&*fonte) });
        self._tarefas.push(cx.spawn_in(window, async move |this, cx| {
            let r = tarefa.await;
            let releitura = this.update(cx, |this, cx| this.ler_indice(cx));
            if let Ok(t) = releitura {
                t.await;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.ocupado = false;
                this.guarda.sair();
                match r {
                    Ok(()) => cx.emit(EventoDiff::AtualizarGit),
                    Err(e) => comum::falha(e, window, cx),
                }
                cx.notify();
            });
        }));
    }

    pub fn preparar_bloco(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((r, _, _)) = self.bloco_ref(i) {
            self.rodar(move |f| f.stage_hunk(&r), window, cx);
        }
    }

    pub fn tirar_bloco(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((r, _, _)) = self.bloco_ref(i) {
            self.rodar(move |f| f.unstage_hunk(&r), window, cx);
        }
    }

    /// Confirma mostrando o bloco capturado agora; a confirmação usa este mesmo `HunkRef`.
    pub fn descartar_bloco(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((r, b, cab)) = self.bloco_ref(i) else { return };
        let this = cx.entity();
        confirmar(
            "Descartar este bloco?",
            format!("{cab}\n{}", b.texto),
            "Descartar",
            move |window, cx| {
                let r = r.clone();
                this.update(cx, |this, cx| this.descartar_bloco_confirmado(r, window, cx));
            },
            window,
            cx,
        );
    }

    /// O descarte depois da confirmação, com o bloco que ela mostrou. Rascunho aberto agora ainda bloqueia.
    pub fn descartar_bloco_confirmado(&mut self, r: HunkRef, window: &mut Window, cx: &mut Context<Self>) {
        if self.rascunho {
            comum::falha("Há um rascunho aberto deste arquivo. Salve ou descarte antes de descartar o bloco.", window, cx);
            return;
        }
        self.rodar(move |f| f.discard_hunk(&r), window, cx);
    }

    /// O `HunkRef` do bloco `i` como está na tela agora (o que a confirmação captura).
    pub fn bloco(&self, i: usize) -> Option<HunkRef> {
        self.bloco_ref(i).map(|(r, _, _)| r)
    }

    pub fn total_unidades(&self) -> usize {
        self.total()
    }

    pub fn descartar_arquivo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Leitura::Indice(r)) = &self.leitura else { return };
        let (patch, revisao) = (r.diff.patch.clone(), r.revision.clone());
        let caminho = self.alvo.caminho().to_string();
        let this = cx.entity();
        confirmar(
            "Descartar as alterações do arquivo?",
            patch,
            "Descartar",
            move |window, cx| {
                let (caminho, revisao) = (caminho.clone(), revisao.clone());
                this.update(cx, |this, cx| {
                    // Rascunho pode ter surgido durante a confirmação: a checagem vale agora.
                    if this.rascunho {
                        comum::falha("Há um rascunho aberto deste arquivo. Salve ou descarte antes de descartar as alterações.", window, cx);
                        return;
                    }
                    this.rodar(move |f| f.discard_file(&caminho, &revisao), window, cx);
                });
            },
            window,
            cx,
        );
    }

    pub fn preparar_arquivo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let caminho = self.alvo.caminho().to_string();
        self.rodar(move |f| f.stage(&[caminho]), window, cx);
    }

    fn barra(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let caminho = self.alvo.caminho().to_string();
        let staged = matches!(self.alvo, Alvo::Diff { lado: crate::Lado::Preparadas, .. });
        let commit = match &self.alvo {
            Alvo::Commit { hash, .. } => Some(hash[..hash.len().min(7)].to_string()),
            _ => None,
        };
        let n = self.total();
        let cur = self.atual();
        let novo = self.mostrado.as_ref().is_some_and(|m| comparar::arquivo_novo(&m.patch));
        let nao_rastreado = matches!(&self.leitura, Some(Leitura::Indice(r)) if r.untracked);
        let bloqueado = self.travado || self.ocupado;
        let unidade = if commit.is_some() { "Mudança" } else { "Bloco" };
        let contador = match (&commit, n, cur) {
            (Some(h), _, _) => format!("commit {h} · somente leitura"),
            (None, _, _) if nao_rastreado => "arquivo novo, não rastreado".into(),
            (None, 0, _) => (if staged { "preparadas" } else { "alterações" }).into(),
            (None, n, Some(c)) => format!("bloco {} de {n}", c + 1),
            _ => String::new(),
        };
        let rotulo = |t: &str| div().flex_none().whitespace_nowrap().text_size(px(11.)).text_color(comum::descricao()).child(t.to_string());
        let barra = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(if commit.is_some() { 6. } else { 2. }))
            .h(px(28.))
            .px(px(8.))
            .overflow_hidden()
            .border_b_1()
            .border_color(cor::hairline())
            .child(
                div()
                    .id("caminho")
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(tema::FONTE_MONO)
                    .text_size(tema::TEXTO_LABEL)
                    .text_color(comum::descricao())
                    .tooltip_texto(caminho.clone())
                    .child(caminho.clone()),
            )
            .child(rotulo(&contador))
            .when(commit.is_some() && n > 0, |d| d.child(rotulo(&format!("mudança {} de {n}", cur.unwrap_or(0) + 1))));
        let barra = barra.when(n > 0, |d| {
            let c = cur.unwrap_or(0);
            d.child(
                acao("anterior", "icons/chevron-up.svg", c > 0)
                    .tooltip_texto(format!("{unidade} anterior (Shift+F8)"))
                    .on_click(cx.listener(|this, _, _, cx| this.passo(-1, cx))),
            )
            .child(
                acao("proximo", "icons/chevron-down.svg", c + 1 < n)
                    .tooltip_texto(if commit.is_some() { "Próxima mudança (F8)" } else { "Próximo bloco (F8)" })
                    .on_click(cx.listener(|this, _, _, cx| this.passo(1, cx))),
            )
        });
        let tem_bloco = commit.is_none() && !nao_rastreado && n > 0;
        let barra = barra.when(tem_bloco, |d| {
            let c = cur.unwrap_or(0);
            let linha = self.mostrado.as_ref().and_then(|m| m.comparacao.unidades.get(c)).map_or(1, |u| u.linha);
            let caminho2 = caminho.clone();
            let d = d.child(
                acao("abrir-bloco", "icons/file-text.svg", true)
                    .tooltip_texto(format!("Abrir o bloco {} de {n} no arquivo", c + 1))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(EventoDiff::AbrirLinha { caminho: caminho2.clone(), linha }))),
            );
            let d = if staged {
                d.child(
                    acao("tirar-bloco", "icons/minus.svg", !bloqueado)
                        .tooltip_texto(format!("Tirar o bloco {} de {n} do preparo", c + 1))
                        .on_click(cx.listener(move |this, _, w, cx| this.tirar_bloco(c, w, cx))),
                )
            } else {
                d.child(
                    acao("preparar-bloco", "icons/plus.svg", !bloqueado)
                        .tooltip_texto(format!("Preparar o bloco {} de {n}", c + 1))
                        .on_click(cx.listener(move |this, _, w, cx| this.preparar_bloco(c, w, cx))),
                )
            };
            d.when(!staged, |d| {
                d.child(
                    acao("descartar-bloco", "icons/arrow-right.svg", !bloqueado && !self.rascunho)
                        .tooltip_texto(if self.rascunho {
                            "Resolva o rascunho aberto deste arquivo antes de descartar.".to_string()
                        } else {
                            format!("Descartar o bloco {} de {n}", c + 1)
                        })
                        .on_click(cx.listener(move |this, _, w, cx| {
                            if !this.travado && !this.ocupado && !this.rascunho {
                                this.descartar_bloco(c, w, cx)
                            }
                        })),
                )
            })
        });
        barra
            .when(nao_rastreado && !staged, |d| {
                d.child(botao("preparar-arquivo", "Preparar arquivo", !bloqueado).on_click(cx.listener(|this, _, w, cx| this.preparar_arquivo(w, cx))))
            })
            .when(!novo && self.mostrado.is_some(), |d| {
                let split = self.lado;
                d.child(
                    acao("modo", if split { "icons/rows-2.svg" } else { "icons/columns-2.svg" }, true)
                        .tooltip_texto(if split { "Unificado" } else { "Lado a lado" })
                        .on_click(cx.listener(|this, _, _, cx| this.alternar_modo(cx))),
                )
            })
            .when(commit.is_none() && !staged, |d| {
                d.child(
                    acao("descartar-arquivo", "icons/undo-2.svg", !bloqueado && !self.rascunho)
                        .tooltip_texto(if self.rascunho {
                            "Resolva o rascunho aberto deste arquivo antes de descartar."
                        } else {
                            "Descartar todas as alterações deste arquivo"
                        })
                        .on_click(cx.listener(|this, _, w, cx| {
                            if !this.travado && !this.ocupado && !this.rascunho {
                                this.descartar_arquivo(w, cx)
                            }
                        })),
                )
            })
    }

    /// Divisor do lado a lado: 8 px de pegada no vão, linha de 1 px; com o ponteiro parado 300 ms ou arrastando, 4 px azuis.
    fn divisor(&self, centro: f32, arrastando: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let forte = arrastando || self.divisor_realce;
        div()
            .id("divisor")
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(centro - 4.))
            .w(px(8.))
            .cursor(CursorStyle::ResizeLeftRight)
            .track_focus(&self.foco_divisor)
            .block_mouse_except_scroll()
            .on_hover(cx.listener(|this, sobre: &bool, _, cx| {
                this.divisor_sobre = *sobre;
                if !*sobre {
                    this.divisor_realce = false;
                    cx.notify();
                    return;
                }
                this._tarefas.push(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_millis(300)).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.divisor_sobre {
                            this.divisor_realce = true;
                            cx.notify();
                        }
                    });
                }));
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.foco_divisor, cx);
                    let Some(m) = &this.mostrado else { return };
                    if ev.click_count >= 2 {
                        m.estado.borrow_mut().pct = 50.;
                    } else {
                        m.estado.borrow_mut().iniciar_divisor(f32::from(ev.position.x));
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                let d = match ev.keystroke.key.as_str() {
                    "left" => -2.,
                    "right" => 2.,
                    _ => return,
                };
                let p = this.proporcao();
                this.definir_proporcao(p + d, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(if forte { 2. } else { 3.5 }))
                    .w(px(if forte { 4. } else { 1. }))
                    .bg(if forte { comparador_cor_atual() } else { rgb(0x6e6e6e).into() }),
            )
            .into_any_element()
    }

    /// As ações do bloco atual no vão, centradas na altura dele (rolam com o conteúdo).
    fn acoes_vao(&self, centro: f32, meio: f32, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let staged = matches!(self.alvo, Alvo::Diff { lado: crate::Lado::Preparadas, .. });
        let nao_rastreado = matches!(&self.leitura, Some(Leitura::Indice(r)) if r.untracked);
        let (Some(c), Alvo::Diff { .. }) = (self.atual(), &self.alvo) else { return None };
        if nao_rastreado {
            return None;
        }
        let n = self.total();
        let bloqueado = self.travado || self.ocupado;
        // Ícone de 24 px, `padding` 2 e borda 1: 30 px de largura, e a altura de um ou dois ícones com 2 px entre eles.
        let alto = if staged { 30. } else { 56. };
        let mut caixa = div()
            .id("acoes-vao")
            .absolute()
            .left(px(centro - 15.))
            .top(px(meio - alto / 2.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(0x454545))
            .bg(rgb(0x292929))
            .block_mouse_except_scroll();
        if !staged {
            caixa = caixa.child(
                acao("vao-descartar", "icons/arrow-right.svg", !bloqueado && !self.rascunho)
                    .tooltip_texto(format!("Descartar o bloco {} de {n}", c + 1))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if !this.travado && !this.ocupado && !this.rascunho {
                            this.descartar_bloco(c, w, cx)
                        }
                    })),
            );
        }
        caixa = if staged {
            caixa.child(
                acao("vao-tirar", "icons/minus.svg", !bloqueado)
                    .tooltip_texto(format!("Tirar o bloco {} de {n} do preparo", c + 1))
                    .on_click(cx.listener(move |this, _, w, cx| this.tirar_bloco(c, w, cx))),
            )
        } else {
            caixa.child(
                acao("vao-preparar", "icons/plus.svg", !bloqueado)
                    .tooltip_texto(format!("Preparar o bloco {} de {n}", c + 1))
                    .on_click(cx.listener(move |this, _, w, cx| this.preparar_bloco(c, w, cx))),
            )
        };
        Some(caixa.into_any_element())
    }

    /// Faixa de visão geral à direita: uma marca por unidade, na posição dela no palco.
    fn regua(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut regua = div().absolute().top_0().right_0().bottom_0().w(px(10.));
        let Some(m) = &self.mostrado else { return regua };
        let e = m.estado.borrow();
        if e.geo.as_ref().is_none_or(|g| g.lado != e.lado) {
            return regua;
        }
        let altura = e.altura_palco().max(1.);
        let cur = self.atual();
        let unidade = if matches!(self.alvo, Alvo::Commit { .. }) { "Mudança" } else { "Bloco" };
        let alto_regua = f32::from(e.caixa.size.height);
        for (i, u) in m.comparacao.unidades.iter().enumerate() {
            let Some((topo, base)) = e.faixa(i) else { continue };
            let y = topo / altura * alto_regua;
            let h = ((base - topo) / altura * alto_regua).max(3.);
            let marca = div()
                .id(("marca", i))
                .absolute()
                .right_0()
                .w_full()
                .top(px(y))
                .h(px(h))
                .cursor_pointer()
                .block_mouse_except_scroll()
                .tooltip_texto(format!("{unidade} {}, linha {}", i + 1, u.linha))
                .on_click(cx.listener(move |this, _, _, cx| this.escolher(i, cx)));
            let marca = match u.tipo {
                TipoUnidade::Inclusao => marca.bg(rgb(0x9ccc2c)),
                TipoUnidade::Remocao => marca.bg(rgb(0xff4d4d)),
                TipoUnidade::Troca => marca.flex().flex_col().child(div().flex_1().bg(rgb(0xff4d4d))).child(div().flex_1().bg(rgb(0x9ccc2c))),
            };
            // A atual mantém a cor e ganha um contorno claro de 1 px por fora (`box-shadow: 0 0 0 1px #fff`).
            if cur == Some(i) {
                regua = regua.child(div().absolute().right(px(-1.)).w(px(12.)).top(px(y - 1.)).h(px(h + 2.)).bg(rgb(0xffffff)));
            }
            regua = regua.child(marca);
        }
        regua
    }
}

fn comparador_cor_atual() -> gpui_kit::Hsla {
    cores::atual()
}

/// O comparador para um patch. `None` se o patch não tem blocos. `anterior` é o da mesma aba antes da releitura: a
/// rolagem, a proporção e a unidade passam para o novo, e a rolagem só muda se a unidade mudar.
fn montar(patch: &str, blocos: Option<&[Intervalo]>, caminho: &str, anterior: Option<&Mostrado>, cx: &mut Context<VisaoDiff>) -> Option<Mostrado> {
    if comparar::intervalos(patch).is_empty() {
        return None;
    }
    let comparacao = comparar::comparar(patch, blocos);
    let unidades: Vec<_> = comparacao.unificado.iter().map(|l| l.unidade).collect();
    let mut c = Comparador::new(patch, caminho, &unidades, comparacao.unidades.len());
    if let Some(a) = anterior {
        let a = a.estado.borrow();
        c.pct = a.pct;
        c.rolagem = a.rolagem;
        c.atual = a.atual;
        c.caixa = a.caixa;
        c.lado = a.lado && !c.modelo.arquivo_novo;
        c.pular = false;
    }
    let (antigo, novo) = c.textos();
    let modelo = (*c.modelo).clone();
    let estado = Rc::new(RefCell::new(c));
    let cam = caminho.to_string();
    // O realce chega depois, como o do worker do React: o diff aparece sem cores e ganha as cores em seguida.
    let tarefa = cx.background_spawn(async move { Comparador::realcar(&cam, &modelo, &antigo, &novo) });
    let e2 = estado.clone();
    let realce = cx.spawn(async move |this, cx| {
        let (a, n) = tarefa.await;
        e2.borrow_mut().definir_realce(a, n);
        let _ = this.update(cx, |_, cx| cx.notify());
    });
    Some(Mostrado { patch: patch.to_string(), comparacao, estado, _realce: realce })
}

/// Diálogo de confirmação com o texto em monoespaçado (o `ask` do workspace do React).
pub fn confirmar(
    titulo: &'static str,
    corpo: String,
    ok: &'static str,
    aceitar: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let aceitar = std::rc::Rc::new(aceitar);
    window.open_alert_dialog(cx, move |alerta, _, _| {
        let aceitar = aceitar.clone();
        alerta
            .title(titulo)
            .description(
                div()
                    .id("corpo")
                    .max_h(px(256.))
                    .overflow_y_scroll()
                    .font_family(tema::FONTE_MONO)
                    .text_size(tema::TEXTO_LABEL)
                    .whitespace_nowrap()
                    .children(corpo.lines().map(|l| div().child(if l.is_empty() { " ".to_string() } else { l.to_string() }))),
            )
            .ok_text(ok)
            .ok_variant(ButtonVariant::Danger)
            .cancel_text("Cancelar")
            .show_cancel(true)
            .on_ok(move |_, window, cx| {
                aceitar(window, cx);
                true
            })
    });
}

impl Render for VisaoDiff {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let raiz = div()
            .key_context("VisaoDiff")
            .track_focus(&self.foco)
            .on_action(cx.listener(|this, _: &ProximaMudanca, _, cx| this.passo(1, cx)))
            .on_action(cx.listener(|this, _: &MudancaAnterior, _, cx| this.passo(-1, cx)))
            .on_action(cx.listener(|this, _: &Copiar, _, cx| {
                if let Some(m) = &this.mostrado {
                    m.estado.borrow().copiar(cx);
                }
            }))
            // Setas rolam 40 px e as páginas a altura menos uma faixa, como a rolagem do Chromium num elemento focado.
            .on_action(cx.listener(|this, _: &RolarAcima, _, cx| this.rolar(|y, _| y - 40., cx)))
            .on_action(cx.listener(|this, _: &RolarAbaixo, _, cx| this.rolar(|y, _| y + 40., cx)))
            .on_action(cx.listener(|this, _: &PaginaAcima, _, cx| this.rolar(|y, h| y - (h * 0.875).max(40.), cx)))
            .on_action(cx.listener(|this, _: &PaginaAbaixo, _, cx| this.rolar(|y, h| y + (h * 0.875).max(40.), cx)))
            .on_action(cx.listener(|this, _: &Inicio, _, cx| this.rolar(|_, _| 0., cx)))
            .on_action(cx.listener(|this, _: &Fim, _, cx| this.rolar(|_, _| f32::MAX, cx)))
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(cores::fundo())
            .text_color(cor::text_primary())
            .font_family(tema::FONTE);
        let commit = matches!(self.alvo, Alvo::Commit { .. });
        let aviso = |t: &str, perigo: bool| {
            div()
                .flex()
                .flex_1()
                .p(px(16.))
                .text_size(tema::TEXTO_BODY)
                .text_color(if perigo { comum::perigo() } else { comum::descricao() })
                .child(t.to_string())
        };
        let Some(leitura) = &self.leitura else {
            if !self.erro.is_empty() {
                return raiz.child(aviso(&self.erro.clone(), true));
            }
            let texto = if commit { "Lendo o commit…" } else { "Lendo as alterações…" };
            return raiz.when(self.tarde, |d| d.child(aviso(texto, false)));
        };
        let nota = match leitura {
            Leitura::Indice(r) if r.diff.binary => Some("Arquivo binário: o diff não é mostrado."),
            Leitura::Indice(r) if r.diff.too_large || r.display.too_large => Some("Diff grande demais para mostrar aqui."),
            Leitura::Indice(r) if r.diff.patch.is_empty() => Some("Não há alterações neste lado."),
            Leitura::Commit(d) if d.binary => Some("Arquivo binário: o diff não é mostrado."),
            Leitura::Commit(d) if d.too_large => Some("Diff grande demais para mostrar aqui."),
            Leitura::Commit(d) if d.patch.is_empty() => Some("Este commit não altera o texto deste arquivo."),
            _ => None,
        };
        if let Some(n) = nota {
            return raiz.child(aviso(n, false));
        }
        let Some(m) = &self.mostrado else {
            return raiz.child(aviso("Não foi possível interpretar o diff.", true));
        };
        let estado = m.estado.clone();
        // O estado do comparador segue a vista: unidade atual e modo.
        {
            let mut e = estado.borrow_mut();
            let a = self.atual();
            if e.atual != a {
                e.escolher(a);
            }
            e.definir_lado(self.lado);
        }
        let this = cx.entity().downgrade();
        let this2 = this.clone();
        let corpo = Corpo::new(
            estado.clone(),
            cx.entity_id(),
            move |u, _, cx| {
                let _ = this.update(cx, |this, cx| this.escolher(u, cx));
            },
            move |window, cx| {
                let _ = this2.update(cx, |this, cx| window.focus(&this.foco, cx));
            },
        );
        let (sobre, lado) = {
            let e = estado.borrow();
            let mut v = Vec::new();
            if let Some(g) = e.geo.clone().filter(|g| g.lado == e.lado && e.lado) {
                let centro = g.colunas.centro;
                v.push(self.divisor(centro, e.arrastando_divisor(), cx));
                if let Some((topo, base)) = e.atual.and_then(|u| e.faixa(u)) {
                    let meio = (topo + base) / 2. - e.rolagem;
                    v.extend(self.acoes_vao(centro, meio, cx));
                }
            }
            (v, e.lado)
        };
        let _ = lado;
        raiz.child(self.barra(cx)).child(
            div()
                .relative()
                .flex()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(corpo)
                .children(sobre)
                .child(self.regua(cx)),
        )
    }
}
