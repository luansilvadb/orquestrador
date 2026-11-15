//! Comparador do editor central (porte de `DiffTab` e `CommitTab` de `src/editor/diff-view.tsx`): o arquivo inteiro num
//! `Diff` do GPUI Kit, lado a lado por padrão ou unificado, com realce `syntect`/Dark+ e diferença por palavra. No diff
//! do índice, as ações por bloco (preparar, tirar do preparo, descartar) e descartar o arquivo; no commit, só leitura.
use crate::comparar::{self, BlocoPatch, Comparacao, Intervalo, Tipo, TipoUnidade};
use crate::comum::{self, acao, botao, Alvo};
use crate::fonte::{Diff as DiffGit, Fonte, HunkRef, ReviewDiff};
use crate::guarda::GuardaGit;
use crate::sondagem::Sondagem;
use crate::tema_local::tema_local;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::diff::{
    Diff, DiffChangeIndicator, DiffDecoration, DiffFile, DiffHunkSeparator, DiffInlineUnit, DiffLinePosition,
    DiffLineRange, DiffMode, DiffSide, DiffState,
};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{
    actions, div, prelude::*, px, rgb, rgba, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding,
    SharedString, Task, Window,
};
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::TooltipTexto as _;
use gpui_kit::component::diff::DiffGutterMarker;
use gpui_kit::{canvas, Bounds, Pixels};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

actions!(visao_diff, [ProximaMudanca, MudancaAnterior]);

/// Atalhos F8 e Shift+F8 (vale em toda a aba).
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("f8", ProximaMudanca, Some("VisaoDiff")),
        KeyBinding::new("shift-f8", MudancaAnterior, Some("VisaoDiff")),
    ]);
}

/// Altura de uma linha do Kit: Consolas 14 px × 1,6.
const ALTURA_LINHA: f32 = 22.4;

/// Intervalo da releitura do diff do índice (`POLL_MS`).
pub const RELEITURA: Duration = Duration::from_millis(2500);

/// Cores do comparador (as do `unsafeCSS` do React, tiradas do VS Code).
pub mod cores {
    use super::*;
    pub fn fundo() -> Hsla {
        rgb(0x1f1f1f).into()
    }
    pub fn removida() -> Hsla {
        rgba(0xff000033).into()
    }
    pub fn incluida() -> Hsla {
        rgba(0x9bb95533).into()
    }
    pub fn numero() -> Hsla {
        rgb(0x6e7681).into()
    }
    pub fn atual() -> Hsla {
        rgb(0x007fd4).into()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventoDiff {
    /// Uma ação mudou o índice ou a pasta: a lateral relê o status.
    AtualizarGit,
    /// Abrir o arquivo na linha do bloco atual.
    AbrirLinha { caminho: String, linha: usize },
}

#[derive(Clone, Copy)]
struct Rolagem {
    acima: usize,
    passos: u8,
}

enum Leitura {
    Indice(ReviewDiff),
    Commit(DiffGit),
}

/// A comparação desenhada: o estado do Kit e o modelo das unidades.
struct Mostrado {
    patch: String,
    estado: Entity<DiffState>,
    caminho: SharedString,
    comparacao: Comparacao,
    unidade_de: HashMap<String, usize>,
    geometria: Rc<Geometria>,
}

/// Onde cada linha de mudança visível foi desenhada no último quadro (medido pela marca da calha de cada uma) e a caixa
/// do corpo: o Kit não expõe a geometria das linhas, e a barra da unidade atual, as ações no vão e a régua dependem dela.
#[derive(Default)]
struct Geometria {
    quadro: RefCell<HashMap<String, Bounds<Pixels>>>,
    ultimo: RefCell<HashMap<String, Bounds<Pixels>>>,
    corpo: Cell<Bounds<Pixels>>,
}

pub struct VisaoDiff {
    fonte: Fonte,
    guarda: GuardaGit,
    alvo: Alvo,
    leitura: Option<Leitura>,
    erro: String,
    modo: DiffMode,
    atual: usize,
    ocupado: bool,
    travado: bool,
    rascunho: bool,
    ativa: bool,
    tarde: bool,
    rolar: Option<Rolagem>,
    mostrado: Option<Mostrado>,
    sondagem: Option<Sondagem>,
    foco: FocusHandle,
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
            modo: DiffMode::Split,
            atual: 0,
            ocupado: false,
            travado: false,
            rascunho: false,
            ativa: false,
            tarde: false,
            rolar: None,
            mostrado: None,
            sondagem: None,
            foco: cx.focus_handle(),
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

    pub fn modo(&self) -> DiffMode {
        self.modo
    }

    pub fn alternar_modo(&mut self, cx: &mut Context<Self>) {
        self.modo = if self.modo == DiffMode::Split { DiffMode::Unified } else { DiffMode::Split };
        if let Some(m) = &self.mostrado {
            let modo = self.modo;
            m.estado.update(cx, |s, cx| s.set_mode(modo, cx));
        }
        self.rolar_para_atual(cx);
        cx.notify();
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

    pub fn escolher(&mut self, unidade: usize, cx: &mut Context<Self>) {
        self.atual = unidade;
        self.rolar_para_atual(cx);
        cx.notify();
    }

    /// Pede a rolagem até a unidade atual; ela sai nos próximos desenhos, com a altura do corpo já medida.
    fn rolar_para_atual(&mut self, cx: &mut Context<Self>) {
        self.rolar = Some(Rolagem { acima: 0, passos: 0 });
        cx.notify();
    }

    /// Como o React, a primeira linha da unidade fica a um terço da altura. O Kit só rola uma linha até o topo e as linhas
    /// quebram, então a linha do topo é ajustada uma de cada vez, medindo onde a unidade caiu, até chegar ao terço.
    fn passo_rolagem(&mut self, cx: &mut Context<Self>) {
        let Some(mut r) = self.rolar.take() else { return };
        let Some(m) = &self.mostrado else { return };
        let altura = f32::from(m.geometria.corpo.get().size.height);
        if altura <= 0. {
            self.rolar = Some(r);
            return;
        }
        let Some(u) = self.atual() else { return };
        let Some(un) = m.comparacao.unidades.get(u).copied() else { return };
        let alvo = altura / 3.;
        if r.passos > 0 {
            let Some((topo, _)) = self.faixa_atual() else { return };
            if topo + ALTURA_LINHA / 2. < alvo && r.passos < 40 {
                r.acima += 1;
            } else {
                if topo > alvo + ALTURA_LINHA && r.acima > 0 {
                    r.acima -= 1;
                    self.rolar_linha(un, r.acima, cx);
                }
                return;
            }
        }
        r.passos += 1;
        self.rolar_linha(un, r.acima, cx);
        self.rolar = Some(r);
    }

    /// Rola até a linha `acima` linhas antes da primeira da unidade.
    fn rolar_linha(&self, un: crate::comparar::Unidade, acima: usize, cx: &mut Context<Self>) {
        let Some(m) = &self.mostrado else { return };
        let primeira = match self.modo {
            DiffMode::Split => un.lado.map(|(a, _)| {
                let l = m.comparacao.lado[a.saturating_sub(acima)];
                l.direita.map(|n| (DiffSide::Modified, n)).or(l.esquerda.map(|n| (DiffSide::Original, n)))
            }),
            _ => un.unificado.map(|(a, _)| {
                let l = m.comparacao.unificado[a.saturating_sub(acima)];
                match l.tipo {
                    Tipo::Removida => l.antiga.map(|n| (DiffSide::Original, n)),
                    _ => l.nova.map(|n| (DiffSide::Modified, n)),
                }
            }),
        }
        .flatten();
        if let Some((lado, linha)) = primeira {
            let pos = DiffLinePosition::new(m.caminho.clone(), lado, linha);
            m.estado.update(cx, |s, cx| s.scroll_to_line(pos, cx));
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

    /// Resposta nova: só refaz o comparador quando o patch mostrado mudou (o poll igual não rola nem pisca).
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
        self.mostrado = montar(&patch, blocos.as_deref(), self.modo, cx);
        if self.mostrado.is_some() {
            self.rolar_para_atual(cx);
        }
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
                let split = self.modo == DiffMode::Split;
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

    /// Faixa vertical (topo, base) da unidade atual no corpo, pelas linhas visíveis dela no último quadro.
    fn faixa_atual(&self) -> Option<(f32, f32)> {
        let m = self.mostrado.as_ref()?;
        let u = self.atual()?;
        let un = m.comparacao.unidades.get(u)?;
        let topos = self.topos();
        let (a, b) = if self.modo == DiffMode::Split { un.lado? } else { un.unificado? };
        let visiveis: Vec<f32> = (a..=b).filter_map(|i| topos.get(i).copied().flatten()).collect();
        let topo = *visiveis.first()?;
        // A base é o topo da linha seguinte (a linha pode quebrar); sem ela, a última medida mais uma linha.
        let base = topos.get(b + 1).copied().flatten().unwrap_or(visiveis.last().copied().unwrap_or(topo) + ALTURA_LINHA);
        Some((topo, base))
    }

    /// Topo de cada linha do modo atual no corpo, onde foi medida no último quadro.
    fn topos(&self) -> Vec<Option<f32>> {
        let Some(m) = &self.mostrado else { return Vec::new() };
        let corpo = m.geometria.corpo.get();
        let ultimo = m.geometria.ultimo.borrow();
        let topo = |k: &String| ultimo.get(k).map(|b| f32::from(b.top() - corpo.top()));
        if self.modo == DiffMode::Split {
            m.comparacao.lado.iter().map(|l| chaves_lado(l).iter().filter_map(topo).reduce(f32::min)).collect()
        } else {
            m.comparacao.unificado.iter().map(|l| chave_unif(l).as_ref().and_then(topo)).collect()
        }
    }

    /// A barra azul da unidade atual e, no lado a lado, as ações do bloco no meio, na altura dela.
    fn sobreposicao(&self, cx: &mut Context<Self>) -> Vec<gpui_kit::AnyElement> {
        let Some((topo, base)) = self.faixa_atual() else { return Vec::new() };
        let mut v = vec![div().absolute().left_0().top(px(topo)).h(px(base - topo)).w(px(3.)).bg(cores::atual()).into_any_element()];
        let staged = matches!(self.alvo, Alvo::Diff { lado: crate::Lado::Preparadas, .. });
        let nao_rastreado = matches!(&self.leitura, Some(Leitura::Indice(r)) if r.untracked);
        let (Some(c), Alvo::Diff { .. }) = (self.atual(), &self.alvo) else { return v };
        if self.modo != DiffMode::Split || nao_rastreado {
            return v;
        }
        let n = self.total();
        let bloqueado = self.travado || self.ocupado;
        let corpo = self.mostrado.as_ref().map(|m| m.geometria.corpo.get()).unwrap_or_default();
        let meio_x = f32::from(corpo.size.width) / 2. - 5.;
        let meio_y = ((topo + base) / 2.).clamp(0., f32::from(corpo.size.height));
        let alto = if staged { 30. } else { 56. };
        let mut caixa = div()
            .absolute()
            .left(px(meio_x - 15.))
            .top(px(meio_y - alto / 2.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(2.))
            .rounded(tema::RAIO_XS)
            .border_1()
            .border_color(rgb(0x454545))
            .bg(rgb(0x292929));
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
        v.push(caixa.into_any_element());
        v
    }

    /// Faixa de visão geral à direita: uma marca por unidade, na proporção das linhas do modo atual.
    fn regua(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut regua = div().absolute().top_0().right_0().bottom_0().w(px(10.));
        let Some(m) = &self.mostrado else { return regua };
        let linhas = match self.modo {
            DiffMode::Split => m.comparacao.lado.len(),
            _ => m.comparacao.unificado.len(),
        }
        .max(1) as f32;
        // O arquivo inteiro cabe na altura; curto, ocupa só a parte de cima, como o palco de altura mínima do React.
        let visivel = f32::from(m.geometria.corpo.get().size.height).max(1.);
        // Altura média das linhas medidas (as quebras contam) estima a altura do arquivo inteiro.
        let medidas: Vec<(usize, f32)> = self.topos().into_iter().enumerate().filter_map(|(i, t)| t.map(|t| (i, t))).collect();
        let media = match (medidas.first(), medidas.last()) {
            (Some(&(i, a)), Some(&(j, b))) if j > i => (b - a) / (j - i) as f32,
            _ => ALTURA_LINHA,
        };
        let total = linhas.max(visivel / media);
        let cur = self.atual();
        let unidade = if matches!(self.alvo, Alvo::Commit { .. }) { "Mudança" } else { "Bloco" };
        for (i, u) in m.comparacao.unidades.iter().enumerate() {
            let faixa = if self.modo == DiffMode::Split { u.lado } else { u.unificado };
            let Some((a, b)) = faixa else { continue };
            let topo = a as f32 / total;
            let alto = (b + 1 - a) as f32 / total;
            let marca = div()
                .id(("marca", i))
                .absolute()
                .right_0()
                .w_full()
                .top(gpui_kit::relative(topo))
                .h(gpui_kit::relative(alto))
                .min_h(px(3.))
                .cursor_pointer()
                .when(cur == Some(i), |d| d.border_1().border_color(rgb(0xffffff)))
                .tooltip_texto(format!("{unidade} {}, linha {}", i + 1, u.linha))
                .on_click(cx.listener(move |this, _, _, cx| this.escolher(i, cx)));
            let marca = match u.tipo {
                TipoUnidade::Inclusao => marca.bg(rgb(0x9ccc2c)),
                TipoUnidade::Remocao => marca.bg(rgb(0xff4d4d)),
                TipoUnidade::Troca => marca.flex().flex_col().child(div().flex_1().bg(rgb(0xff4d4d))).child(div().flex_1().bg(rgb(0x9ccc2c))),
            };
            regua = regua.child(marca);
        }
        regua
    }
}

/// O `DiffState` e o modelo das unidades para um patch. `None` se o Kit não interpreta o patch.
fn montar(patch: &str, blocos: Option<&[Intervalo]>, modo: DiffMode, cx: &mut Context<VisaoDiff>) -> Option<Mostrado> {
    let arquivo = DiffFile::parse(patch).ok()?.into_iter().next()?;
    let caminho = arquivo.path().clone();
    let arquivo = arquivo.with_language(caminho.clone());
    let comparacao = comparar::comparar(patch, blocos);
    let mut unidade_de = HashMap::new();
    for l in &comparacao.unificado {
        if let (Some(u), Some(k)) = (l.unidade, comparar::chave_unificada(l)) {
            unidade_de.insert(k, u);
        }
    }
    let estado = cx.new(|cx| {
        DiffState::new([arquivo], cx)
            .with_mode(modo)
            .with_context_lines(None)
            .with_inline_unit(Some(DiffInlineUnit::Word))
            .with_syntax_highlighter(crate::realce::realcar)
    });
    Some(Mostrado { patch: patch.to_string(), estado, caminho, comparacao, unidade_de, geometria: Rc::default() })
}

/// Fundo das linhas mudadas, nas cores do VS Code (o Kit pinta com as cores do tema).
fn decoracoes(m: &Mostrado) -> Vec<DiffDecoration> {
    let mut v = Vec::new();
    let mut corrida: Option<(DiffSide, usize, usize)> = None;
    let fechar = |c: Option<(DiffSide, usize, usize)>, v: &mut Vec<DiffDecoration>| {
        if let Some((lado, a, b)) = c {
            let fundo = if lado == DiffSide::Original { cores::removida() } else { cores::incluida() };
            v.push(DiffDecoration::new(DiffLineRange::new(m.caminho.clone(), lado, a, b)).with_background(fundo));
        }
    };
    for l in &m.comparacao.unificado {
        let atual = match l.tipo {
            Tipo::Removida => l.antiga.map(|n| (DiffSide::Original, n)),
            Tipo::Incluida => l.nova.map(|n| (DiffSide::Modified, n)),
            Tipo::Contexto => None,
        };
        match (atual, corrida) {
            (Some((lado, n)), Some((l2, a, b))) if lado == l2 && n == b + 1 => corrida = Some((lado, a, n)),
            (Some((lado, n)), c) => {
                fechar(c, &mut v);
                corrida = Some((lado, n, n));
            }
            (None, c) => {
                fechar(c, &mut v);
                corrida = None;
            }
        }
    }
    fechar(corrida, &mut v);
    v
}

/// Uma marca por linha de mudança: o sinal (− ou +) depois do número, como no React, e a medida da linha.
fn marcas(m: &Mostrado) -> Vec<DiffGutterMarker> {
    // Todas as linhas dos dois lados ganham marca (vazia no contexto): a medida da linha seguinte dá a base de uma linha
    // quebrada, e a régua usa a altura média medida.
    let mut v = Vec::new();
    for l in &m.comparacao.unificado {
        let (d, a) = match l.tipo {
            Tipo::Removida => (Some("−"), None),
            Tipo::Incluida => (None, Some("+")),
            Tipo::Contexto => (Some(""), Some("")),
        };
        if let (Some(sinal), Some(n)) = (d, l.antiga) {
            v.push(DiffGutterMarker::new(SharedString::from(format!("d{n}")), DiffLinePosition::new(m.caminho.clone(), DiffSide::Original, n)).with_label(sinal));
        }
        if let (Some(sinal), Some(n)) = (a, l.nova) {
            v.push(DiffGutterMarker::new(SharedString::from(format!("a{n}")), DiffLinePosition::new(m.caminho.clone(), DiffSide::Modified, n)).with_label(sinal));
        }
    }
    v
}

/// Chaves com que uma linha de cada modo é medida.
fn chaves_lado(l: &crate::comparar::LinhaLado) -> Vec<String> {
    l.esquerda.map(|n| format!("d{n}")).into_iter().chain(l.direita.map(|n| format!("a{n}"))).collect()
}

fn chave_unif(l: &crate::comparar::LinhaUnificada) -> Option<String> {
    match (l.tipo, l.antiga, l.nova) {
        (Tipo::Incluida, _, Some(n)) => Some(format!("a{n}")),
        (_, Some(n), _) => Some(format!("d{n}")),
        (_, None, Some(n)) => Some(format!("a{n}")),
        _ => None,
    }
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
        let _ = m;
        self.passo_rolagem(cx);
        let Some(m) = &self.mostrado else { return raiz };
        let unidade_de = m.unidade_de.clone();
        let this = cx.entity().downgrade();
        let corpo = Diff::new(&m.estado)
            .header_visible(false)
            .hunk_separator(DiffHunkSeparator::LineInfo)
            .change_indicator(DiffChangeIndicator::None)
            .change_background(false)
            .soft_wrap(true)
            .decorations(decoracoes(m))
            .gutter_markers(marcas(m))
            .render_gutter_marker({
                let g = m.geometria.clone();
                move |marca, _, _| {
                    let g = g.clone();
                    let pos = marca.position();
                    let chave = format!("{}{}", if pos.side() == DiffSide::Original { "d" } else { "a" }, pos.line());
                    div()
                        .relative()
                        .size_full()
                        .flex()
                        .items_center()
                        .text_color(cores::numero())
                        .child(marca.label().clone())
                        .child(canvas(move |b, _, _| {
                            g.quadro.borrow_mut().insert(chave.clone(), b);
                        }, |_, _, _, _| {}).absolute().size_full())
                }
            })
            .on_line_click(move |pos, _, _, cx| {
                let k = format!("{}{}", if pos.side() == DiffSide::Original { "d" } else { "a" }, pos.line());
                if let Some(&u) = unidade_de.get(&k) {
                    let _ = this.update(cx, |this, cx| this.escolher(u, cx));
                }
            })
            .size_full();
        let corpo = tema_local(corpo, |t| {
            t.background = cores::fundo();
            t.foreground = crate::realce::cor_texto();
            t.border = rgb(0x3c3c3c).into();
            t.muted = rgba(0xcccccc40).into();
            t.muted_foreground = cores::numero();
            // O Kit pinta a diferença por palavra com 30% destas: dá o #ff000033 e o #9ccc2c33 do React.
            t.danger = rgba(0xff0000aa).into();
            t.success = rgba(0x9ccc2caa).into();
            t.mono_font_family = "Consolas".into();
            // O Kit usa rem × tamanho mono / 16, e o rem do app é 14: 16 dá a Consolas de 14 px do React.
            t.mono_font_size = px(16.);
        });
        let (g1, g2) = (m.geometria.clone(), m.geometria.clone());
        // Antes das linhas: limpa o quadro. Depois: se a geometria mudou, desenha de novo com a barra no lugar.
        let antes = canvas(move |b, window, _| {
            g1.quadro.borrow_mut().clear();
            if g1.corpo.replace(b) != b {
                window.refresh();
            }
        }, |_, _, _, _| {})
        .absolute()
        .size_full();
        let depois = canvas(move |_, window, _| {
            let novo = g2.quadro.borrow().clone();
            if *g2.ultimo.borrow() != novo {
                *g2.ultimo.borrow_mut() = novo;
                window.refresh();
            }
        }, |_, _, _, _| {})
        .absolute()
        .size_0();
        let sobre = self.sobreposicao(cx);
        raiz.child(self.barra(cx)).child(
            div()
                .relative()
                .flex()
                .flex_1()
                .min_h_0()
                .child(antes)
                .child(div().flex_1().min_w_0().child(corpo))
                .child(depois)
                .children(sobre)
                .child(self.regua(cx)),
        )
    }
}
