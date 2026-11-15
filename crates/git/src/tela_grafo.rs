//! Seção Grafo da tela Git (porte de `src/git/git-graph-view.tsx`) e a lista de commits paginada e o commit aberto, comuns
//! ao grafo e ao histórico (`usePagedList`, `useCommitDetail` e `CommitFileList` de `git-history.tsx`).
use crate::comum::{self, cor_faixa, cor_tipo, descricao, dividir, etiqueta, icone_arquivo, letra, palavra, secundario, Alvo};
use crate::fonte::{CommitFile, GraphCommit, RefKind, Status};
use crate::grafo::{montar_grafo, LinhaGrafo, Metade};
use crate::tela::{EstadoHover, EventoGit, Ocupado, Remoto, TelaGit, GRAFO_MIN, ARQUIVOS_MIN, LINHA, PAGINA};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::{
    anchored, canvas, deferred, div, point, prelude::*, px, AnyElement, App, Bounds, ClipboardItem, Context, Hsla,
    MouseButton, PathBuilder, Pixels, Point, Window,
};
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::{icone, TooltipTexto as _};
use std::time::Duration;

/// Faixas de 11 px (a primeira começa em 11); linha de 22.
const FAIXA: f32 = 11.;
const FECHAR_HOVER: Duration = Duration::from_millis(120);
const VIZINHAS: Duration = Duration::from_millis(80);
const ESPERA_CARTAO: Duration = Duration::from_millis(500);
const PRE_LEITURA_ARQUIVO: Duration = Duration::from_millis(100);

fn x_faixa(f: usize) -> f32 {
    (f as f32 + 1.) * FAIXA
}

fn largura(l: &LinhaGrafo) -> f32 {
    (l.largura as f32 + 1.) * FAIXA
}

fn circulo(window: &mut Window, c: Point<Pixels>, r: f32, cor: Hsla) {
    let b = Bounds::new(point(c.x - px(r), c.y - px(r)), gpui_kit::size(px(2. * r), px(2. * r)));
    window.paint_quad(gpui_kit::fill(b, cor).corner_radii(px(r)));
}

/// Faixas de uma linha de commit: a metade de cima liga a borda de cima ao meio, a de baixo o meio à borda de baixo;
/// reta sem troca de faixa, curva com tangente vertical na troca. O ponto corta a linha com o fundo, como no VS Code.
fn faixas_commit(linha: LinhaGrafo, head: bool) -> impl IntoElement {
    let w = largura(&linha);
    canvas(|_, _, _| (), move |b, _, window, _| {
        let o = b.origin;
        let meio = LINHA / 2.;
        for s in &linha.segmentos {
            let cima = s.metade == Metade::Cima;
            let (x1, y1, x2, y2) = (x_faixa(s.de), if cima { 0. } else { meio }, x_faixa(s.para), if cima { meio } else { LINHA });
            let mut p = PathBuilder::stroke(px(1.));
            p.move_to(o + point(px(x1), px(y1)));
            if s.de == s.para {
                p.line_to(o + point(px(x2), px(y2)));
            } else {
                let m = (y1 + y2) / 2.;
                p.cubic_bezier_to(o + point(px(x2), px(y2)), o + point(px(x1), px(m)), o + point(px(x2), px(m)));
            }
            if let Ok(p) = p.build() {
                window.paint_path(p, cor_faixa(s.cor));
            }
        }
        let c = o + point(px(x_faixa(linha.coluna)), px(meio));
        let fundo = cor::pitch_black();
        let r = if head { 7. } else { 5. };
        circulo(window, c, r + 1., fundo);
        circulo(window, c, r - 1., cor_faixa(linha.cor));
        if head {
            circulo(window, c, 4., fundo);
        }
    })
    .w(px(w))
    .h(px(LINHA))
    .flex_none()
}

/// Faixas que seguem para baixo ao lado de uma linha de arquivo do commit aberto.
fn faixas_vao(linha: LinhaGrafo) -> impl IntoElement {
    let w = largura(&linha);
    canvas(|_, _, _| (), move |b, _, window, _| {
        for s in linha.segmentos.iter().filter(|s| s.metade == Metade::Baixo) {
            let x = px(x_faixa(s.para));
            let mut p = PathBuilder::stroke(px(1.));
            p.move_to(b.origin + point(x, px(0.)));
            p.line_to(b.origin + point(x, px(LINHA)));
            if let Ok(p) = p.build() {
                window.paint_path(p, cor_faixa(s.cor));
            }
        }
    })
    .w(px(w))
    .h(px(LINHA))
    .flex_none()
}

impl TelaGit {
    // ---- Lista paginada ----

    fn pagina(&mut self, grafo: bool, pular: usize, escopo: Option<String>, somar: bool, cx: &mut Context<Self>) {
        let fonte = self.fonte.clone();
        let lista = if grafo { &mut self.grafo } else { &mut self.historico };
        let seq = lista.seq;
        lista.lendo = true;
        let tarefa = cx.background_spawn(async move {
            if grafo {
                fonte.graph(PAGINA, pular)
            } else {
                fonte.history(PAGINA, pular, escopo.as_deref())
            }
        });
        self._tarefas.push(cx.spawn(async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                let lista = if grafo { &mut this.grafo } else { &mut this.historico };
                if lista.seq != seq {
                    return;
                }
                lista.lendo = false;
                match r {
                    Ok(v) => {
                        lista.mais = v.len() == PAGINA;
                        match (&mut lista.itens, somar) {
                            (Some(itens), true) => itens.extend(v),
                            (itens, _) => *itens = Some(v),
                        }
                        if !somar {
                            lista.erro.clear();
                        }
                    }
                    Err(e) => lista.erro = e,
                }
                if grafo {
                    let itens = this.grafo.itens.as_deref().unwrap_or_default();
                    this.grafo_linhas = montar_grafo(itens.iter().map(|c| (c.commit.hash.as_str(), c.parents.as_slice())));
                }
                cx.notify();
            });
        }));
    }

    /// Grafo: a lista anterior fica na tela até a nova chegar.
    pub(crate) fn recarregar_grafo(&mut self, cx: &mut Context<Self>) {
        self.grafo.seq += 1;
        self.pagina(true, 0, None, false, cx);
    }

    /// Histórico: recarregar esvazia a lista e fecha o commit aberto.
    pub(crate) fn recarregar_historico(&mut self, escopo: Option<String>, cx: &mut Context<Self>) {
        self.fechar_commit(false, cx);
        let h = &mut self.historico;
        h.seq += 1;
        h.escopo = escopo.clone();
        h.itens = None;
        h.mais = false;
        h.erro.clear();
        self.pagina(false, 0, escopo, false, cx);
    }

    fn carregar_mais(&mut self, grafo: bool, cx: &mut Context<Self>) {
        let lista = if grafo { &self.grafo } else { &self.historico };
        let Some(itens) = &lista.itens else { return };
        if lista.lendo {
            return;
        }
        let (n, escopo) = (itens.len(), lista.escopo.clone());
        self.pagina(grafo, n, escopo, true, cx);
        cx.notify();
    }

    // ---- Commit aberto ----

    /// Lê o detalhe uma vez (cache de 500); a pré-leitura do mousedown só aquece o cache.
    pub(crate) fn pre_ler_commit(&mut self, hash: &str, cx: &mut Context<Self>) {
        if self.detalhes.contains_key(hash) || !self.lendo_detalhe.insert(hash.to_string()) {
            return;
        }
        let fonte = self.fonte.clone();
        let h = hash.to_string();
        let tarefa = cx.background_spawn(async move { fonte.commit_detail(&h) });
        let h = hash.to_string();
        self._tarefas.push(cx.spawn(async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                this.lendo_detalhe.remove(&h);
                let erro = match r {
                    Ok(d) => {
                        if this.detalhes.len() >= 500 {
                            if let Some(k) = this.detalhes.keys().next().cloned() {
                                this.detalhes.remove(&k);
                            }
                        }
                        this.detalhes.insert(h.clone(), d);
                        String::new()
                    }
                    Err(e) => e,
                };
                for grafo in [true, false] {
                    let d = if grafo { &mut this.grafo_detalhe } else { &mut this.historico_detalhe };
                    if d.pendente.as_deref() == Some(&h) {
                        d.pendente = None;
                        d.aberto = Some(h.clone());
                        d.detalhe = this.detalhes.get(&h).cloned();
                        d.erro = erro.clone();
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(crate) fn abrir_commit(&mut self, grafo: bool, hash: &str, cx: &mut Context<Self>) {
        let d = if grafo { &mut self.grafo_detalhe } else { &mut self.historico_detalhe };
        d.seq += 1;
        let seq = d.seq;
        if let Some(c) = self.detalhes.get(hash).cloned() {
            d.pendente = None;
            d.aberto = Some(hash.to_string());
            d.detalhe = Some(c);
            d.erro.clear();
            cx.notify();
            return;
        }
        d.pendente = Some(hash.to_string());
        // "Lendo o commit…" só aparece se a leitura passar de 150 ms.
        let h = hash.to_string();
        self._tarefas.push(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let _ = this.update(cx, |this, cx| {
                let d = if grafo { &mut this.grafo_detalhe } else { &mut this.historico_detalhe };
                if d.seq == seq && d.pendente.as_deref() == Some(&h) {
                    d.aberto = Some(h.clone());
                    d.detalhe = None;
                    d.erro.clear();
                    cx.notify();
                }
            });
        }));
        self.pre_ler_commit(hash, cx);
    }

    pub(crate) fn fechar_commit(&mut self, grafo: bool, cx: &mut Context<Self>) {
        let d = if grafo { &mut self.grafo_detalhe } else { &mut self.historico_detalhe };
        d.seq += 1;
        d.pendente = None;
        d.aberto = None;
        d.detalhe = None;
        d.erro.clear();
        cx.notify();
    }

    pub(crate) fn alternar_commit(&mut self, grafo: bool, hash: &str, cx: &mut Context<Self>) {
        let d = if grafo { &self.grafo_detalhe } else { &self.historico_detalhe };
        if d.aberto.as_deref() == Some(hash) || d.pendente.as_deref() == Some(hash) {
            self.fechar_commit(grafo, cx);
        } else {
            self.abrir_commit(grafo, hash, cx);
        }
    }

    /// Arquivos de um commit, uma linha de 22 px cada; clique prévia, duplo clique de vez; o aberto no editor fica marcado.
    pub(crate) fn arquivos_commit(&self, hash: &str, arquivos: &[CommitFile], vao: Option<&LinhaGrafo>, cx: &mut Context<Self>) -> AnyElement {
        let mut lista = div().flex().flex_col();
        for (i, f) in arquivos.iter().enumerate() {
            let (nome, pasta) = dividir(&f.path);
            let aberto = matches!(&self.selecionado, Some(Alvo::Commit { hash: h, caminho }) if h.eq_ignore_ascii_case(hash) && *caminho == f.path);
            let completo = match &f.orig_path {
                Some(o) => format!("{} (renomeado de {o})", f.path),
                None => f.path.clone(),
            };
            let alvo = Alvo::Commit { hash: hash.to_string(), caminho: f.path.clone() };
            let (a1, a2, a3, a4) = (alvo.clone(), alvo.clone(), alvo.clone(), alvo);
            let c = cor_tipo(f.kind);
            lista = lista.child(
                div()
                    .flex()
                    .min_w_0()
                    .children(vao.cloned().map(faixas_vao))
                    .child(
                        div()
                            .id(("arquivo-commit", i))
                            .relative()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .h(px(LINHA))
                            .items_center()
                            .gap(px(6.))
                            .pl(px(20.))
                            .pr(px(12.))
                            .text_size(px(13.))
                            .hover(|d| d.bg(cor::fill_muted()))
                            .when(aberto, |d| d.bg(cor::fill_muted()).child(div().absolute().left_0().top_0().bottom_0().w(px(2.)).bg(cor::ink_white())))
                            .tooltip_texto(format!("{completo}: {}. Ver a alteração neste commit (duplo clique abre de vez)", palavra(f.kind)))
                            .on_hover(cx.listener(move |this, dentro: &bool, _, cx| {
                                this.hover_arquivo = None;
                                if *dentro {
                                    let alvo = a1.clone();
                                    this.hover_arquivo = Some(cx.spawn(async move |this, cx| {
                                        cx.background_executor().timer(PRE_LEITURA_ARQUIVO).await;
                                        let _ = this.update(cx, |_, cx| cx.emit(EventoGit::PreCarregar { alvo, hover: true }));
                                    }));
                                }
                            }))
                            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
                                this.hover_arquivo = None;
                                if !e.modifiers.control && !e.modifiers.shift && !e.modifiers.platform {
                                    cx.emit(EventoGit::PreCarregar { alvo: a2.clone(), hover: false });
                                }
                            }))
                            .on_click(cx.listener(move |_, e: &gpui_kit::ClickEvent, _, cx| {
                                let permanente = e.click_count() >= 2;
                                cx.emit(EventoGit::AbrirAlvo { alvo: if permanente { a3.clone() } else { a4.clone() }, permanente });
                            }))
                            .child(icone_arquivo(&f.path, 16.))
                            .child(div().flex_shrink_0().max_w(gpui_kit::relative(0.7)).truncate().text_color(c).child(nome.to_string()))
                            .when(!pasta.is_empty(), |d| {
                                d.child(div().flex_1().min_w_0().truncate().text_size(px(11.7)).opacity(0.7).child(pasta.to_string()))
                            })
                            .child(div().ml_auto().flex_none().pl(px(4.)).text_size(tema::TEXTO_LABEL).text_color(c).child(letra(f.kind))),
                    ),
            );
        }
        lista.into_any_element()
    }

    // ---- Divisor ----

    pub(crate) fn mover_divisor(&mut self, y: f32, cx: &mut Context<Self>) {
        let Some((y0, inicio, max)) = self.arrasto else { return };
        self.altura_grafo = Some((inicio - (y - y0)).clamp(GRAFO_MIN, max.max(GRAFO_MIN)));
        cx.notify();
    }

    // ---- Hover do commit ----

    fn ler_hover(&mut self, hash: &str, cx: &mut Context<Self>) {
        if matches!(self.hovers.get(hash), Some(EstadoHover::Pronto(_) | EstadoHover::Lendo)) {
            return;
        }
        self.hovers.insert(hash.to_string(), EstadoHover::Lendo);
        let fonte = self.fonte.clone();
        let h = hash.to_string();
        let tarefa = cx.background_spawn(async move { fonte.commit_hover(&h) });
        let h = hash.to_string();
        self._tarefas.push(cx.spawn(async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |this, cx| {
                if this.hovers.len() > 500 {
                    if let Some(k) = this.hovers.keys().next().cloned() {
                        this.hovers.remove(&k);
                    }
                }
                this.hovers.insert(h.clone(), match r {
                    Ok(d) => EstadoHover::Pronto(d),
                    Err(e) => EstadoHover::Erro(e),
                });
                // O cartão anterior fica até este chegar.
                if this.hover.ativo.as_ref().is_some_and(|(a, _)| *a == h) {
                    this.hover.abrir = None;
                    this.hover.cartao = Some(h.clone());
                }
                cx.notify();
            });
        }));
    }

    fn entrar_hover(&mut self, hash: String, indice: usize, cx: &mut Context<Self>) {
        self.hover.abrir = None;
        self.hover.fechar = None;
        self.hover.vizinhas = None;
        self.hover.seq += 1;
        let seq = self.hover.seq;
        self.hover.ativo = Some((hash.clone(), seq));
        let vizinhas: Vec<String> = [-1i64, 1, -2, 2]
            .iter()
            .filter_map(|d| self.grafo.itens.as_ref()?.get((indice as i64 + d).max(-1) as usize).filter(|_| indice as i64 + d >= 0))
            .map(|c| c.commit.hash.clone())
            .collect();
        // Com o cartão aberto a vizinhança é lida já; sem ele, só depois de o ponteiro parar um instante.
        if self.hover.cartao.is_some() {
            for h in vizinhas {
                self.pre_ler_hover(&h, cx);
            }
        } else {
            self.hover.vizinhas = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(VIZINHAS).await;
                let _ = this.update(cx, |this, cx| {
                    if this.hover.ativo.as_ref().is_some_and(|(_, s)| *s == seq) {
                        for h in &vizinhas {
                            this.pre_ler_hover(h, cx);
                        }
                    }
                });
            }));
        }
        if matches!(self.hovers.get(&hash), Some(EstadoHover::Pronto(_))) {
            self.hover.cartao = Some(hash);
            cx.notify();
            return;
        }
        let h = hash.clone();
        self.hover.abrir = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ESPERA_CARTAO).await;
            let _ = this.update(cx, |this, cx| {
                if this.hover.ativo.as_ref().is_some_and(|(_, s)| *s == seq) {
                    this.hover.cartao = Some(h);
                    cx.notify();
                }
            });
        }));
        self.ler_hover(&hash, cx);
    }

    /// Lê sem abrir cartão; um erro some e a leitura de verdade repete.
    fn pre_ler_hover(&mut self, hash: &str, cx: &mut Context<Self>) {
        if matches!(self.hovers.get(hash), Some(EstadoHover::Erro(_))) {
            self.hovers.remove(hash);
        }
        self.ler_hover(hash, cx);
    }

    fn sair_hover(&mut self, hash: &str, cx: &mut Context<Self>) {
        if self.hover.ativo.as_ref().is_none_or(|(a, _)| a != hash) {
            return;
        }
        self.hover.seq += 1;
        self.hover.ativo = None;
        self.hover.abrir = None;
        self.hover.vizinhas = None;
        self.fechar_hover_logo(cx);
    }

    fn fechar_hover_logo(&mut self, cx: &mut Context<Self>) {
        self.hover.fechar = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FECHAR_HOVER).await;
            let _ = this.update(cx, |this, cx| this.fechar_hover(cx));
        }));
    }

    pub(crate) fn fechar_hover(&mut self, cx: &mut Context<Self>) {
        self.hover.seq += 1;
        self.hover.ativo = None;
        self.hover.abrir = None;
        self.hover.fechar = None;
        self.hover.vizinhas = None;
        self.hover.cartao = None;
        cx.notify();
    }

    pub(crate) fn cartao_hover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let hash = self.hover.cartao.clone()?;
        let commit = self.grafo.itens.as_ref()?.iter().find(|c| c.commit.hash == hash)?.clone();
        let ancora = self.linhas_grafo.borrow().get(&hash).copied()?;
        let estado = self.hovers.get(&hash).cloned();
        let c = &commit.commit;
        let mut corpo = div()
            .id("cartao-commit")
            .w(px(384.))
            .max_h(px(512.))
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(tema::RAIO_MD)
            .bg(cor::raised_black())
            .border_1()
            .border_color(cor::hairline())
            .occlude()
            .on_hover(cx.listener(|this, dentro: &bool, _, cx| {
                if *dentro {
                    this.hover.fechar = None;
                } else {
                    this.fechar_hover_logo(cx);
                }
            }))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child(c.author.clone()))
                    .child(
                        div()
                            .text_size(tema::TEXTO_LABEL)
                            .text_color(descricao())
                            .child(format!(" {} · {}", comum::data_relativa(&c.date), comum::quando_longo(&c.date))),
                    ),
            );
        match estado {
            Some(EstadoHover::Lendo) | None => {
                corpo = corpo.child(div().text_size(tema::TEXTO_LABEL).text_color(descricao()).child("Lendo detalhes…"))
            }
            Some(EstadoHover::Erro(e)) => corpo = corpo.child(div().text_size(tema::TEXTO_LABEL).text_color(comum::perigo()).child(e)),
            Some(EstadoHover::Pronto(d)) => {
                let refs: Vec<_> = commit.refs.iter().filter(|r| r.kind != RefKind::Head).cloned().collect();
                let ins = if d.insertions == 1 { "inserção(+)" } else { "inserções(+)" };
                let del = if d.deletions == 1 { "remoção(-)" } else { "remoções(-)" };
                let arq = if d.files_changed == 1 { "arquivo alterado" } else { "arquivos alterados" };
                let hash_copia = c.hash.clone();
                let url = d.github_url.clone();
                corpo = corpo
                    .children(comum::coautores(&d.message).into_iter().map(|n| div().text_size(tema::TEXTO_LABEL).child(format!("{n} (coautor)"))))
                    .child(
                        div()
                            .id("mensagem")
                            .min_h_0()
                            .overflow_y_scroll()
                            .text_size(px(13.))
                            .children(d.message.split('\n').map(|l| div().min_h(px(18.)).child(l.to_string()))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .text_size(tema::TEXTO_LABEL)
                            .child(format!("{} {arq}, ", d.files_changed))
                            .child(div().text_color(cor::signal_success()).child(format!("{} {ins}", d.insertions)))
                            .child(", ")
                            .child(div().text_color(cor::signal_error()).child(format!("{} {del}", d.deletions))),
                    )
                    .when(!refs.is_empty(), |d| d.child(div().flex().flex_wrap().gap(px(4.)).children(refs.iter().map(|r| etiqueta(r, true)))))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(div().font_family(tema::FONTE_MONO).text_size(tema::TEXTO_LABEL).child(c.short.clone()))
                            .child(
                                comum::botao("copiar-hash", "Copiar hash", true)
                                    .on_click(cx.listener(move |_, _, w, cx| copiar(&hash_copia, "Hash copiado", w, cx))),
                            )
                            .when_some(url, |d, url| {
                                d.child(comum::botao("abrir-github", "Abrir no GitHub", true).on_click(cx.listener(move |this, _, w, cx| {
                                    let fonte = this.fonte.clone();
                                    let url = url.clone();
                                    let t = cx.background_spawn(async move { fonte.abrir_url(&url) });
                                    this._tarefas.push(cx.spawn_in(w, async move |this, cx| {
                                        if let Err(e) = t.await {
                                            let _ = this.update_in(cx, |_, w, cx| comum::falha(e, w, cx));
                                        }
                                    }));
                                })))
                            }),
                    );
            }
        }
        // `placement="right"` do Lobe: à direita da linha com 6 px de afastamento e centrado nela. A altura vem do desenho
        // anterior; até ser medida, o cartão fica invisível.
        let altura = self.hover.altura.clone();
        let h = altura.get();
        let corpo = corpo.when(h <= 0., |d| d.invisible()).child(
            canvas(move |b, window, _| {
                let nova = f32::from(b.size.height);
                if altura.replace(nova) != nova {
                    window.refresh();
                }
            }, |_, _, _, _| {})
            .absolute()
            .size_full(),
        );
        let topo = ancora.top() + ancora.size.height / 2. - px(h / 2.);
        Some(
            deferred(
                anchored()
                    .position_mode(gpui_kit::AnchoredPositionMode::Window)
                    .position(point(ancora.right() + px(6.), topo))
                    .snap_to_window_with_margin(px(8.))
                    .child(corpo),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    // ---- Seção ----

    pub(crate) fn secao_grafo(&self, s: &Status, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let aberta = !self.recolhido_grafo_estado();
        let travado = self.ocupado.is_some();
        let rodando = match self.ocupado {
            Some(Ocupado::Remoto(r)) => Some(r),
            _ => None,
        };
        let rotulo = |r: Remoto| match r {
            Remoto::Fetch => "Buscar".to_string(),
            Remoto::Pull if s.behind > 0 => format!("Receber ({})", comum::commits(s.behind)),
            Remoto::Pull => "Receber".into(),
            Remoto::Push if s.ahead > 0 => format!("Enviar ({})", comum::commits(s.ahead)),
            Remoto::Push => "Enviar".into(),
        };
        let botao_remoto = |id: &'static str, r: Remoto, caminho: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            let tipo = if r == Remoto::Push { comum::Animacao::Subir } else { comum::Animacao::Descer };
            let svg = comum::icone_animado(caminho, 14., descricao(), self.anim(id, tipo), window);
            comum::acao_secao_com(id, svg, !travado)
                .when(rodando == Some(r), |d| d.opacity(0.6))
                .tooltip_texto(rotulo(r))
                .on_click(cx.listener(move |this, _, w, cx| {
                    this.giro(id);
                    if this.ocupado.is_none() {
                        this.remoto(r, w, cx)
                    }
                    cx.notify();
                }))
        };
        let cabecalho = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .h(px(LINHA))
            .pr(px(8.))
            .child(
                div()
                    .id("secao-grafo")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .cursor_pointer()
                    .child(div().mx(px(2.)).child(icone(if aberta { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" }, 16., cor::text_primary())))
                    .child(div().min_w_0().truncate().text_size(tema::TEXTO_LABEL).font_weight(gpui_kit::FontWeight::SEMIBOLD).child("Grafo"))
                    .on_click(cx.listener(|this, _, _, cx| this.alternar_grafo(cx))),
            )
            .child(botao_remoto("buscar", Remoto::Fetch, "icons/cloud-download.svg", window, cx))
            .child(botao_remoto("enviar", Remoto::Push, "icons/arrow-up-from-line.svg", window, cx))
            .child(botao_remoto("receber", Remoto::Pull, "icons/arrow-down-to-line.svg", window, cx))
            .child(
                comum::acao_secao_com(
                    "atualizar-grafo",
                    comum::icone_animado("icons/refresh-cw.svg", 14., descricao(), self.anim("grafo", comum::Animacao::Giro), window),
                    true,
                )
                .tooltip_texto("Atualizar o gráfico")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.giro("grafo");
                    this.grafo_tick += 1;
                    this.talvez_carregar(cx);
                })),
            );
        let mut secao = div()
            .flex()
            .flex_col()
            .flex_none()
            .min_h_0()
            .child(cabecalho);
        if aberta {
            secao = match self.altura_grafo {
                Some(h) => secao.h(px(h)),
                None => secao.flex_1().flex_basis(px(0.)).min_h(px(GRAFO_MIN)),
            };
            let medidas = self.medidas.clone();
            secao = secao.child(
                div()
                    .id("lista-grafo")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.grafo_rolagem)
                    .child(self.lista_grafo(s, cx))
                    .child(canvas(move |b, _, _| {
                        let (l, _, _) = medidas.get();
                        medidas.set((l, 0., f32::from(b.size.height) + LINHA));
                    }, |_, _, _, _| {}).absolute().size_full()),
            );
        }
        let mut v = Vec::new();
        if aberta {
            v.push(
                div()
                    .id("divisor-grafo")
                    .h(px(4.))
                    .flex_none()
                    .cursor_row_resize()
                    .hover(|d| d.bg(cor::hairline()))
                    .tooltip_texto("Arrastar para redimensionar (setas do teclado também)")
                    .track_focus(&self.foco_divisor)
                    .focus(|d| d.bg(cor::text_tertiary()))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &gpui_kit::MouseDownEvent, w, cx| {
                        let (lista, _, grafo) = this.medidas.get();
                        let inicio = this.altura_grafo.unwrap_or(grafo);
                        this.arrasto = Some((f32::from(e.position.y), inicio, lista + inicio - ARQUIVOS_MIN));
                        w.focus(&this.foco_divisor, cx);
                        cx.notify();
                    }))
                    // Setas para cima e para baixo andam de 16 em 16, entre os mínimos dos dois lados.
                    .on_key_down(cx.listener(|this, e: &gpui_kit::KeyDownEvent, _, cx| {
                        let passo = match e.keystroke.key.as_str() {
                            "up" => 16.,
                            "down" => -16.,
                            _ => return,
                        };
                        cx.stop_propagation();
                        let (lista, _, grafo) = this.medidas.get();
                        let atual = this.altura_grafo.unwrap_or(grafo);
                        this.altura_grafo = Some((atual + passo).clamp(GRAFO_MIN, (lista + atual - ARQUIVOS_MIN).max(GRAFO_MIN)));
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        v.push(secao.into_any_element());
        v
    }

    fn lista_grafo(&self, s: &Status, cx: &mut Context<Self>) -> AnyElement {
        let Some(itens) = &self.grafo.itens else {
            let e = &self.grafo.erro;
            return comum::nota(if e.is_empty() { "Lendo o gráfico…".to_string() } else { e.clone() }, !e.is_empty()).into_any_element();
        };
        if itens.is_empty() {
            let e = &self.grafo.erro;
            let t = if !e.is_empty() {
                e.clone()
            } else if s.head.is_empty() {
                "Este repositório ainda não tem commit.".into()
            } else {
                "Nenhum commit para mostrar.".into()
            };
            return comum::nota(t, !e.is_empty()).into_any_element();
        }
        let largura_painel = f32::from(self.grafo_rolagem.bounds().size.width);
        let mut lista = div().flex().flex_col();
        for (i, c) in itens.iter().enumerate() {
            let Some(linha) = self.grafo_linhas.get(i).cloned() else { continue };
            lista = lista.child(self.linha_grafo(c, i, linha, s, largura_painel, cx));
        }
        lista
            .when(!self.grafo.erro.is_empty(), |d| d.child(comum::nota(self.grafo.erro.clone(), true)))
            .when(self.grafo.mais, |d| {
                d.child(
                    div().px(px(12.)).py(px(6.)).child(
                        comum::botao("mais-grafo", if self.grafo.lendo { "Carregando…" } else { "Carregar mais" }, !self.grafo.lendo)
                            .w_full()
                            .on_click(cx.listener(|this, _, _, cx| this.carregar_mais(true, cx))),
                    ),
                )
            })
            .into_any_element()
    }

    fn linha_grafo(&self, c: &GraphCommit, i: usize, linha: LinhaGrafo, s: &Status, largura: f32, cx: &mut Context<Self>) -> AnyElement {
        let head = c.commit.hash == s.head;
        let aberto = self.grafo_detalhe.aberto.as_deref() == Some(&c.commit.hash);
        let hash = c.commit.hash.clone();
        let (h1, h2, h3, h4) = (hash.clone(), hash.clone(), hash.clone(), hash.clone());
        let assunto = if c.commit.subject.is_empty() { "(sem mensagem)".to_string() } else { c.commit.subject.clone() };
        let (copia_hash, copia_assunto) = (c.commit.hash.clone(), c.commit.subject.clone());
        let registro = self.linhas_grafo.clone();
        let chave = hash.clone();
        let chave_foco = format!("c:{hash}");
        let foco_l = self.foco_linha(&chave_foco, cx);
        let (foco_clique, registro2, k2) = (foco_l.clone(), self.limites_linha.clone(), chave_foco.clone());
        let linha_commit = div()
            .id(("commit-grafo", i))
            .track_focus(&foco_l)
            .on_action(cx.listener(move |this, _: &crate::tela::MenuContexto, window, cx| this.menu_pelo_teclado(&chave_foco, window, cx)))
            .relative()
            .flex()
            .w_full()
            .min_w_0()
            .h(px(LINHA))
            .items_center()
            .gap(px(4.))
            .pr(px(12.))
            .hover(|d| d.bg(cor::fill_muted()))
            .when(aberto, |d| d.bg(cor::fill_muted()))
            .on_hover(cx.listener(move |this, dentro: &bool, _, cx| {
                if *dentro {
                    this.entrar_hover(h1.clone(), i, cx);
                } else {
                    this.sair_hover(&h1, cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, window, cx| {
                window.focus(&foco_clique, cx);
                if !e.modifiers.control && !e.modifiers.shift && !e.modifiers.platform {
                    this.pre_ler_commit(&h2, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.alternar_commit(true, &h3, cx)))
            .context_menu(move |menu, _, _| {
                let (a, b) = (copia_hash.clone(), copia_assunto.clone());
                menu.item(PopupMenuItem::new("Copiar hash").on_click(move |_, w, cx| copiar(&a, "Hash copiado", w, cx)))
                    .item(PopupMenuItem::new("Copiar assunto").on_click(move |_, w, cx| copiar(&b, "Assunto copiado", w, cx)))
            })
            .child(canvas(move |b, _, _| {
                registro.borrow_mut().insert(chave.clone(), b);
                registro2.borrow_mut().insert(k2.clone(), b);
            }, |_, _, _, _| {}).absolute().size_full())
            .child(faixas_commit(linha.clone(), head))
            .child(
                div()
                    .flex_1()
                    .min_w(px(96.))
                    .truncate()
                    .text_size(px(13.))
                    .when(head, |d| d.font_weight(gpui_kit::FontWeight::SEMIBOLD))
                    .child(assunto),
            )
            .children(c.refs.iter().map(|r| etiqueta(r, largura >= 352.)))
            .when(largura >= 384., |d| {
                d.child(div().min_w_0().max_w(gpui_kit::relative(0.3)).truncate().text_size(px(11.7)).opacity(0.7).child(c.commit.author.clone()))
            });
        let _ = h4;
        let detalhe = aberto.then(|| match &self.grafo_detalhe.detalhe {
            Some(d) if d.commit.hash == c.commit.hash => {
                if d.files.is_empty() {
                    nota_vao(&linha, "Nenhum arquivo neste commit.", false)
                } else {
                    self.arquivos_commit(&c.commit.hash, &d.files, Some(&linha), cx)
                }
            }
            _ => {
                let e = &self.grafo_detalhe.erro;
                nota_vao(&linha, if e.is_empty() { "Lendo os arquivos…" } else { e }, !e.is_empty())
            }
        });
        div().flex().flex_col().child(linha_commit).children(detalhe).into_any_element()
    }

    pub(crate) fn alternar_grafo(&mut self, cx: &mut Context<Self>) {
        self.alternar_recolhido_grafo();
        self.talvez_carregar(cx);
        cx.notify();
    }

    // ---- Histórico ----

    pub(crate) fn historico(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let no_detalhe = self.historico_detalhe.aberto.is_some();
        let escopo = self.historico.escopo.clone();
        let cabecalho = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .px(px(12.))
            .py(px(6.))
            .when(no_detalhe, |d| {
                d.child(
                    comum::acao("voltar", "icons/arrow-left.svg", true)
                        .tooltip_texto("Voltar à lista")
                        .on_click(cx.listener(|this, _, _, cx| this.fechar_commit(false, cx))),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(tema::TEXTO_LABEL)
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(secundario())
                    .child(if no_detalhe { "Commit" } else { "Histórico" }),
            )
            .when(!no_detalhe, |d| {
                d.child(
                    comum::acao_com(
                        "atualizar-historico",
                        comum::icone_animado("icons/refresh-cw.svg", 14., descricao(), self.anim("historico", comum::Animacao::Giro), window),
                        true,
                    )
                        .tooltip_texto("Atualizar o histórico")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.giro.insert("historico", std::time::Instant::now());
                            let e = this.historico.escopo.clone();
                            this.recarregar_historico(e, cx);
                        })),
                )
            });
        let barra_escopo = (!no_detalhe).then_some(escopo.clone()).flatten().map(|p| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .py(px(4.))
                .child(icone_arquivo(&p, 14.))
                .child(div().flex_1().min_w_0().truncate().font_family(tema::FONTE_MONO).text_size(tema::TEXTO_LABEL).child(dividir(&p).0.to_string()))
                .child(
                    div()
                        .id("projeto-inteiro")
                        .h(px(24.))
                        .px(px(7.))
                        .flex()
                        .items_center()
                        .rounded(tema::RAIO_XS)
                        .text_size(tema::TEXTO_LABEL)
                        .cursor_pointer()
                        .hover(|d| d.bg(cor::fill_hover()))
                        .tooltip_texto("Mostrar os commits do projeto inteiro")
                        .child("Projeto inteiro")
                        .on_click(cx.listener(|this, _, _, cx| this.recarregar_historico(None, cx))),
                )
        });
        let corpo = self.corpo_historico(cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(cabecalho)
            .children(barra_escopo)
            .child(div().id("rolagem-historico").flex().flex_col().flex_1().min_h_0().py(px(4.)).overflow_y_scroll().track_scroll(&self.historico_rolagem).child(corpo))
            .into_any_element()
    }

    fn corpo_historico(&self, cx: &mut Context<Self>) -> AnyElement {
        let d = &self.historico_detalhe;
        if d.aberto.is_some() {
            let Some(det) = &d.detalhe else {
                return comum::nota(if d.erro.is_empty() { "Lendo o commit…".to_string() } else { d.erro.clone() }, !d.erro.is_empty())
                    .into_any_element();
            };
            let c = &det.commit;
            return div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .px(px(12.))
                        .py(px(6.))
                        .border_b_1()
                        .border_color(cor::hairline())
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .child(if c.subject.is_empty() { "(sem mensagem)".to_string() } else { c.subject.clone() }),
                        )
                        .child(div().font_family(tema::FONTE_MONO).text_size(px(11.)).text_color(descricao()).child(c.hash.clone()))
                        .child(div().text_size(tema::TEXTO_LABEL).text_color(descricao()).child(format!("{} · {}", c.author, comum::quando_medio(&c.date)))),
                )
                .when(det.files.is_empty(), |x| x.child(comum::nota("Nenhum arquivo neste commit.", false)))
                .when(!det.files.is_empty(), |x| {
                    x.child(
                        div()
                            .px(px(12.))
                            .py(px(4.))
                            .text_size(tema::TEXTO_LABEL)
                            .text_color(descricao())
                            .child(if det.files.len() == 1 { "1 arquivo".to_string() } else { format!("{} arquivos", det.files.len()) }),
                    )
                    .child(self.arquivos_commit(&c.hash, &det.files, None, cx))
                })
                .into_any_element();
        }
        let h = &self.historico;
        let Some(itens) = &h.itens else {
            return comum::nota(if h.erro.is_empty() { "Lendo o histórico…".to_string() } else { h.erro.clone() }, !h.erro.is_empty())
                .into_any_element();
        };
        if itens.is_empty() {
            let t = if !h.erro.is_empty() {
                h.erro.clone()
            } else if h.escopo.is_some() {
                "Nenhum commit mudou este arquivo.".into()
            } else {
                "Nenhum commit neste repositório.".into()
            };
            return comum::nota(t, !h.erro.is_empty()).into_any_element();
        }
        let largura = f32::from(self.historico_rolagem.bounds().size.width);
        let mut lista = div().flex().flex_col();
        for (i, c) in itens.iter().enumerate() {
            let (h1, h2) = (c.commit.hash.clone(), c.commit.hash.clone());
            let inicial = c.commit.author.trim().chars().next().map(|x| x.to_uppercase().to_string()).unwrap_or("?".into());
            lista = lista.child(
                div()
                    .id(("commit-historico", i))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(12.))
                    .py(px(6.))
                    .cursor_pointer()
                    .hover(|d| d.bg(cor::fill_muted()))
                    .tooltip_texto(format!("{} · {}", c.commit.short, c.commit.subject))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
                        if !e.modifiers.control && !e.modifiers.shift && !e.modifiers.platform {
                            this.pre_ler_commit(&h1, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.alternar_commit(false, &h2, cx)))
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(96.))
                                    .truncate()
                                    .text_size(px(13.))
                                    .child(if c.commit.subject.is_empty() { "(sem mensagem)".to_string() } else { c.commit.subject.clone() }),
                            )
                            .children(c.refs.iter().map(|r| etiqueta(r, largura >= 352.))),
                    )
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.))
                            .text_color(secundario())
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .size(px(16.))
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(cor::fill_hover())
                                    .text_size(px(10.))
                                    .child(inicial),
                            )
                            .child(div().min_w_0().truncate().child(c.commit.author.clone()))
                            .child("·")
                            .child(div().id(("data", i)).flex_none().tooltip_texto(comum::quando_medio(&c.commit.date)).child(comum::data_relativa(&c.commit.date)))
                            .child("·")
                            .child(div().flex_none().font_family(tema::FONTE_MONO).child(c.commit.short.clone())),
                    ),
            );
        }
        lista
            .when(!h.erro.is_empty(), |d| d.child(comum::nota(h.erro.clone(), true)))
            .when(h.mais, |d| {
                d.child(
                    div().px(px(12.)).py(px(6.)).child(
                        comum::botao("mais-historico", if h.lendo { "Carregando…" } else { "Carregar mais" }, !h.lendo)
                            .w_full()
                            .on_click(cx.listener(|this, _, _, cx| this.carregar_mais(false, cx))),
                    ),
                )
            })
            .into_any_element()
    }
}

fn nota_vao(linha: &LinhaGrafo, texto: &str, perigo: bool) -> AnyElement {
    div()
        .flex()
        .min_w_0()
        .child(faixas_vao(linha.clone()))
        .child(
            div()
                .flex()
                .h(px(LINHA))
                .min_w_0()
                .items_center()
                .pl(px(20.))
                .text_size(tema::TEXTO_LABEL)
                .text_color(if perigo { comum::perigo() } else { descricao() })
                .child(texto.to_string()),
        )
        .into_any_element()
}

fn copiar(texto: &str, aviso: &str, window: &mut Window, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(texto.to_string()));
    comum::sucesso(aviso.to_string(), "", window, cx);
}
