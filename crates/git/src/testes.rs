//! Testes da tela Git e das vistas numa janela sem tela, com o simulado.
use crate::comum::{Alvo, Lado};
use crate::conflito::{EventoConflito, VisaoConflito};
use crate::conflito_texto::Escolha;
use crate::diff::{EventoDiff, VisaoDiff};
use crate::fonte::{Estado, Fonte, FonteGit, Kind};
use crate::guarda::GuardaGit;
use crate::simulado::{ModoSimulado, Simulado};
use crate::tela::{EventoGit, ModoCommit, Remoto, TelaGit, Vista};
use crate::diff::Modo;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AnyWindowHandle, AppContext, Bounds, Entity, Point, TestAppContext, WindowBounds, WindowOptions};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

fn iniciar(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        orquestrador_ui::tema::aplicar(cx);
        crate::init(cx);
    });
}

fn opcoes() -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: Point::default(), size: size(px(1400.), px(900.)) })),
        ..Default::default()
    }
}

fn montar(cx: &mut TestAppContext, s: &Arc<Simulado>) -> (AnyWindowHandle, Entity<TelaGit>, Rc<RefCell<Vec<EventoGit>>>) {
    iniciar(cx);
    let fonte: Fonte = s.clone();
    let eventos = Rc::new(RefCell::new(Vec::new()));
    let ev = eventos.clone();
    let (janela, tela) = cx.update(|cx| {
        let (janela, tela) =
            gpui_kit::open_window(opcoes(), cx, move |window, cx| cx.new(|cx| TelaGit::new(fonte, GuardaGit::new(), window, cx)))
                .expect("janela de teste");
        cx.subscribe(&tela, move |_, e: &EventoGit, _| ev.borrow_mut().push(e.clone())).detach();
        (janela, tela)
    });
    cx.run_until_parked();
    (janela, tela, eventos)
}

fn status_de(tela: &Entity<TelaGit>, cx: &mut TestAppContext) -> crate::fonte::Status {
    cx.read(|cx| match tela.read(cx).estado() {
        Some(Estado::Repo { status }) => status.clone(),
        e => panic!("sem repositório: {e:?}"),
    })
}

fn na_janela<R>(cx: &mut TestAppContext, j: AnyWindowHandle, f: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App) -> R) -> R {
    cx.update_window(j, |_, window, cx| f(window, cx)).expect("janela")
}

#[gpui_kit::test]
fn status_lido_com_secoes_e_contagem(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (j, tela, eventos) = montar(cx, &s);
    let st = status_de(&tela, cx);
    assert_eq!(st.files.len(), 12);
    assert_eq!(st.files.iter().filter(|f| f.staged.is_some()).count(), 4);
    assert_eq!(st.files.iter().filter(|f| f.unstaged.is_some()).count(), 11);
    assert_eq!((st.ahead, st.behind), (1, 0));
    assert!(eventos.borrow().iter().any(|e| matches!(e, EventoGit::Estado(Some(Estado::Repo { .. })))), "badge recebe o estado");
    na_janela(cx, j, |w, cx| {
        w.render_frame(cx);
        assert!(w.find("tela-git").visible());
        assert!(w.find("aba-alteracoes").visible());
    });
}

#[gpui_kit::test]
fn preparar_todas_e_tirar_todas(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (j, tela, _) = montar(cx, &s);
    na_janela(cx, j, |w, cx| tela.update(cx, |t, cx| t.preparar(Vec::new(), false, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_stage"), 1);
    let st = status_de(&tela, cx);
    // Conflito e binário sem texto do Git ficam em Alterações; o não rastreado vira novo no índice.
    assert_eq!(st.files.iter().find(|f| f.path == "docs/rascunho-painel.md").and_then(|f| f.staged), Some(Kind::Added));
    assert!(st.files.iter().filter(|f| f.unstaged == Some(Kind::Conflicted)).count() == 2);
    na_janela(cx, j, |w, cx| tela.update(cx, |t, cx| t.preparar(Vec::new(), true, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_unstage"), 1);
    let st = status_de(&tela, cx);
    assert_eq!(st.files.iter().filter(|f| f.staged.is_some()).count(), 0, "tirar todas esvazia as preparadas");
}

#[gpui_kit::test]
fn ctrl_clique_marca_e_prepara_em_lote(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (j, tela, eventos) = montar(cx, &s);
    let st = status_de(&tela, cx);
    let a = st.files.iter().find(|f| f.path == "README.md").unwrap().clone();
    let b = st.files.iter().find(|f| f.path == "src/sem-quebra.ts").unwrap().clone();
    tela.update(cx, |t, cx| {
        t.escolher_linha(&a, Lado::Alteracoes, true, cx);
        t.escolher_linha(&b, Lado::Alteracoes, true, cx);
    });
    assert!(!eventos.borrow().iter().any(|e| matches!(e, EventoGit::AbrirAlvo { .. })), "Ctrl+clique só marca");
    na_janela(cx, j, |w, cx| tela.update(cx, |t, cx| t.preparar_linha(&a, Lado::Alteracoes, w, cx)));
    cx.run_until_parked();
    let st = status_de(&tela, cx);
    for p in ["README.md", "src/sem-quebra.ts"] {
        let f = st.files.iter().find(|f| f.path == p).unwrap();
        assert!(f.staged.is_some() && f.unstaged.is_none(), "{p} preparado no lote");
    }
    // Clique simples abre a revisão em prévia; conflito abre o conflito.
    let c = st.files.iter().find(|f| f.path == "src/conflito.ts").unwrap().clone();
    tela.update(cx, |t, cx| t.escolher_linha(&c, Lado::Alteracoes, false, cx));
    assert!(eventos.borrow().contains(&EventoGit::AbrirAlvo { alvo: Alvo::Conflito { caminho: "src/conflito.ts".into() }, permanente: false }));
}

#[gpui_kit::test]
fn commit_limpa_a_mensagem_e_recarrega_o_grafo(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (j, tela, _) = montar(cx, &s);
    let grafo_antes = s.contar("git_graph");
    assert!(grafo_antes >= 1, "o grafo carrega com a tela ativa");
    let head = status_de(&tela, cx).head;
    na_janela(cx, j, |w, cx| {
        tela.update(cx, |t, cx| {
            t.definir_mensagem("Novo commit", w, cx);
            t.commitar(ModoCommit::Commit, w, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(s.contar("git_commit"), 1);
    assert_eq!(s.contar("git_push"), 0);
    assert_eq!(cx.read(|cx| tela.read(cx).mensagem(cx)), "", "commit feito limpa a mensagem");
    assert_ne!(status_de(&tela, cx).head, head);
    assert!(s.contar("git_graph") > grafo_antes, "o grafo recarrega depois do commit");
    let topo = cx.read(|cx| tela.read(cx).grafo_commits().first().map(|c| (c.commit.subject.clone(), c.refs.iter().any(|r| r.name == "main"))));
    assert_eq!(topo, Some(("Novo commit".to_string(), true)), "o commit novo no topo com a etiqueta main");
}

#[gpui_kit::test]
fn commit_e_push_e_falha_mantem_a_mensagem(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (j, tela, _) = montar(cx, &s);
    s.falhar("git_commit", Some("recusado"));
    na_janela(cx, j, |w, cx| {
        tela.update(cx, |t, cx| {
            t.definir_mensagem("Mensagem", w, cx);
            t.commitar(ModoCommit::CommitPush, w, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| tela.read(cx).mensagem(cx)), "Mensagem", "commit com erro mantém a mensagem");
    assert!(cx.read(orquestrador_ui::avisos::visiveis).iter().any(|a| a.titulo.as_ref() == "recusado"), "o erro do Git em aviso");
    assert_eq!(s.contar("git_push"), 0, "sem commit, sem push");
    s.falhar("git_commit", None);
    na_janela(cx, j, |w, cx| tela.update(cx, |t, cx| t.commitar(ModoCommit::CommitPush, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_push"), 1, "commit e push envia depois do commit");
}

#[gpui_kit::test]
fn sem_mensagem_ou_sem_preparado_nao_commita(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (j, tela, _) = montar(cx, &s);
    na_janela(cx, j, |w, cx| tela.update(cx, |t, cx| t.commitar(ModoCommit::Commit, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_commit"), 0);
}

#[gpui_kit::test]
fn executor_ativo_recusa_receber_e_trocar_branch(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (j, tela, _) = montar(cx, &s);
    tela.update(cx, |t, cx| t.definir_travado(true, cx));
    na_janela(cx, j, |w, cx| tela.update(cx, |t, cx| t.remoto(Remoto::Pull, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_pull"), 0, "Receber com executor ativo só explica");
    assert!(cx.read(orquestrador_ui::avisos::visiveis).iter().any(|a| a.titulo.as_ref() == crate::comum::TRAVADO));
    na_janela(cx, j, |w, cx| tela.update(cx, |t, cx| t.remoto(Remoto::Fetch, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_fetch"), 1, "Buscar continua");
}

#[gpui_kit::test]
fn guarda_compartilhada_recusa_a_segunda_operacao(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    iniciar(cx);
    let fonte: Fonte = s.clone();
    let guarda = GuardaGit::new();
    let g2 = guarda.clone();
    let (j, tela) = cx.update(|cx| {
        gpui_kit::open_window(opcoes(), cx, move |window, cx| cx.new(|cx| TelaGit::new(fonte, g2, window, cx))).unwrap()
    });
    cx.run_until_parked();
    // Outra tela (o editor) está gravando: a lateral espera e avisa.
    assert!(na_janela(cx, j, |w, cx| guarda.entrar(w, cx)));
    na_janela(cx, j, |w, cx| tela.update(cx, |t, cx| t.preparar(Vec::new(), false, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_stage"), 0);
    assert!(cx.read(orquestrador_ui::avisos::visiveis).iter().any(|a| a.titulo.as_ref() == "Espere a operação do Git em andamento terminar."));
    guarda.sair();
    na_janela(cx, j, |w, cx| tela.update(cx, |t, cx| t.preparar(Vec::new(), false, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_stage"), 1);
    assert!(!guarda.ocupada(), "a operação libera a trava no fim");
}

#[gpui_kit::test]
fn sem_repositorio_inicializa_e_volta_ao_normal(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::new(ModoSimulado::SemRepo));
    let (j, tela, _) = montar(cx, &s);
    assert_eq!(cx.read(|cx| tela.read(cx).estado().cloned()), Some(Estado::NoRepo));
    na_janela(cx, j, |w, cx| {
        w.render_frame(cx);
        assert!(w.try_find("aba-alteracoes").is_none(), "sem abas fora do modo repo");
        w.click("inicializar", cx);
    });
    cx.run_until_parked();
    assert_eq!(s.contar("git_init"), 1);
    assert!(matches!(cx.read(|cx| tela.read(cx).estado().cloned()), Some(Estado::Repo { .. })));
    assert_eq!(s.contar("git_graph"), 1, "o grafo só consulta no modo repo");
}

#[gpui_kit::test]
fn git_ausente_e_repositorio_acima_dizem_o_motivo(cx: &mut TestAppContext) {
    for (modo, esperado) in [(ModoSimulado::SemGit, Estado::NoGit), (ModoSimulado::RepoAcima, Estado::RepoAbove { root: "C:\\projetos".into() })] {
        let s = Arc::new(Simulado::new(modo));
        let (j, tela, _) = montar(cx, &s);
        assert_eq!(cx.read(|cx| tela.read(cx).estado().cloned()), Some(esperado));
        na_janela(cx, j, |w, cx| {
            w.render_frame(cx);
            assert!(w.try_find("inicializar").is_none(), "sem botão");
        });
        assert_eq!(s.contar("git_graph") + s.contar("git_history"), 0);
    }
}

#[gpui_kit::test]
fn troca_de_modo_em_execucao_sem_recarregar(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (_, tela, _) = montar(cx, &s);
    s.trocar_modo(ModoSimulado::SemRepo);
    let t = tela.update(cx, |t, cx| t.atualizar(cx));
    cx.run_until_parked();
    drop(t);
    assert_eq!(cx.read(|cx| tela.read(cx).estado().cloned()), Some(Estado::NoRepo));
}

#[gpui_kit::test]
fn grafo_com_os_commits_etiquetas_e_faixas(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (_, tela, _) = montar(cx, &s);
    let commits = cx.read(|cx| tela.read(cx).grafo_commits().to_vec());
    assert_eq!(commits.iter().map(|c| c.commit.short.as_str()).collect::<Vec<_>>(), ["9f3a1c2", "4b2e8d0", "d7e1a9c", "a2c4e6f"]);
    let nomes: Vec<String> = commits.iter().flat_map(|c| c.refs.iter().map(|r| r.name.clone())).collect();
    assert_eq!(nomes, ["main", "v0.1.7", "origin/main", "feat/painel-git"]);
    assert!(commits[0].refs[0].current, "main atual");
    // O merge desenha duas faixas.
    let larguras = cx.read(|cx| tela.read(cx).grafo_larguras());
    assert_eq!(larguras, [1, 2, 2, 2]);
    // Recolher não consulta; abrir de novo com a mesma chave também não.
    let antes = s.contar("git_graph");
    tela.update(cx, |t, cx| t.alternar_grafo(cx));
    tela.update(cx, |t, cx| t.alternar_grafo(cx));
    cx.run_until_parked();
    assert_eq!(s.contar("git_graph"), antes);
}

#[gpui_kit::test]
fn grafo_abre_os_arquivos_do_commit_e_recolhe(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (_, tela, _) = montar(cx, &s);
    let hash = "4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11";
    tela.update(cx, |t, cx| t.alternar_commit_do_grafo(hash, cx));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| tela.read(cx).arquivos_abertos_no_grafo()), Some(4));
    tela.update(cx, |t, cx| t.alternar_commit_do_grafo(hash, cx));
    assert_eq!(cx.read(|cx| tela.read(cx).arquivos_abertos_no_grafo()), None, "clicar de novo recolhe");
    // Detalhe em cache: abrir de novo não relê.
    tela.update(cx, |t, cx| t.alternar_commit_do_grafo(hash, cx));
    cx.run_until_parked();
    assert_eq!(s.contar("git_commit_detail"), 1);
}

#[gpui_kit::test]
fn historico_do_projeto_e_do_arquivo(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (_, tela, _) = montar(cx, &s);
    assert_eq!(s.contar("git_history"), 0, "o histórico só consulta quando aberto");
    tela.update(cx, |t, cx| t.pedir_historico(None, cx));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| tela.read(cx).vista()), Vista::Historico);
    assert_eq!(cx.read(|cx| tela.read(cx).historico_commits().len()), 4);
    // Voltar à aba não recarrega à toa.
    tela.update(cx, |t, cx| {
        t.mostrar_vista(Vista::Alteracoes, cx);
        t.mostrar_vista(Vista::Historico, cx);
    });
    cx.run_until_parked();
    assert_eq!(s.contar("git_history"), 1);
    tela.update(cx, |t, cx| t.pedir_historico(Some("src/main.tsx".into()), cx));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| tela.read(cx).historico_commits().len()), 1, "só o commit que mexeu no arquivo");
    tela.update(cx, |t, cx| t.abrir_commit_do_historico("a2c4e6f8b0d2e4f6a8c0b2d4e6f8a0c2e4b6d8f0", cx));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| tela.read(cx).arquivos_abertos_no_historico()), Some(6));
}

#[gpui_kit::test]
fn poll_igual_nao_avisa_e_cutucar_adianta(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let (_, tela, eventos) = montar(cx, &s);
    let n_eventos = eventos.borrow().len();
    let lidas = s.contar("git_status");
    cx.executor().advance_clock(Duration::from_secs(12));
    cx.run_until_parked();
    assert!(s.contar("git_status") > lidas, "o status é relido sozinho");
    assert_eq!(eventos.borrow().len(), n_eventos, "resposta igual não emite estado novo");
    let lidas = s.contar("git_status");
    tela.update(cx, |t, _| t.cutucar());
    cx.run_until_parked();
    assert_eq!(s.contar("git_status"), lidas + 1, "evento de executor adianta a rodada");
    // Rajada de cutucadas vira no máximo uma rodada pendente.
    tela.update(cx, |t, _| {
        for _ in 0..10 {
            t.cutucar();
        }
    });
    cx.run_until_parked();
    assert!(s.contar("git_status") <= lidas + 3);
}

fn montar_vista<V: 'static + gpui_kit::Render>(
    cx: &mut TestAppContext,
    construir: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::Context<V>) -> V + 'static,
) -> (AnyWindowHandle, Entity<V>) {
    iniciar(cx);
    let r = cx.update(|cx| gpui_kit::open_window(opcoes(), cx, move |window, cx| cx.new(|cx| construir(window, cx))).unwrap());
    cx.run_until_parked();
    r
}

#[gpui_kit::test]
fn diff_prepara_bloco_com_a_revisao_vista(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let fonte: Fonte = s.clone();
    let alvo = Alvo::Diff { lado: Lado::Alteracoes, caminho: "src/editor-workspace.tsx".into() };
    let (j, v) = montar_vista(cx, move |w, cx| VisaoDiff::new(fonte, GuardaGit::new(), alvo, w, cx));
    let eventos = Rc::new(RefCell::new(Vec::new()));
    let ev = eventos.clone();
    cx.update(|cx| cx.subscribe(&v, move |_, e: &EventoDiff, _| ev.borrow_mut().push(e.clone())).detach());
    assert_eq!(cx.read(|cx| v.read(cx).total_unidades()), 2);
    assert_eq!(cx.read(|cx| v.read(cx).modo()), Modo::LadoALado, "lado a lado por padrão");
    let r = cx.read(|cx| v.read(cx).bloco(0)).unwrap();
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.preparar_bloco(0, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_stage_hunk"), 1);
    assert!(eventos.borrow().contains(&EventoDiff::AtualizarGit));
    assert_eq!(cx.read(|cx| v.read(cx).total_unidades()), 1, "o bloco preparado sai do lado das alterações");
    // O bloco antigo (revisão velha) é recusado pelo Git e nada muda.
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.descartar_bloco_confirmado(r, w, cx)));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| v.read(cx).total_unidades()), 1);
    // Rascunho aberto bloqueia o descarte.
    let r = cx.read(|cx| v.read(cx).bloco(0)).unwrap();
    v.update(cx, |v, cx| v.definir_rascunho(true, cx));
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.descartar_bloco_confirmado(r.clone(), w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_discard_hunk"), 1, "só a tentativa com revisão velha chegou ao Git");
    v.update(cx, |v, cx| v.definir_rascunho(false, cx));
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.descartar_bloco_confirmado(r, w, cx)));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| v.read(cx).total_unidades()), 0);
}

#[gpui_kit::test]
fn diff_navega_e_troca_o_modo(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let fonte: Fonte = s.clone();
    let alvo = Alvo::Diff { lado: Lado::Alteracoes, caminho: "src/sem-quebra.ts".into() };
    let (_, v) = montar_vista(cx, move |w, cx| VisaoDiff::new(fonte, GuardaGit::new(), alvo, w, cx));
    assert_eq!(cx.read(|cx| v.read(cx).total_unidades()), 3, "três blocos, o último sem quebra no fim");
    v.update(cx, |v, cx| {
        v.passo(1, cx);
        v.passo(1, cx);
        v.passo(1, cx);
    });
    assert_eq!(cx.read(|cx| v.read(cx).atual()), Some(2), "Próximo para no último");
    v.update(cx, |v, cx| v.passo(-5, cx));
    assert_eq!(cx.read(|cx| v.read(cx).atual()), Some(0));
    v.update(cx, |v, cx| v.alternar_modo(cx));
    assert_eq!(cx.read(|cx| v.read(cx).modo()), Modo::Unificado);
    // Poll com a resposta igual não refaz o comparador.
    let lidas = s.contar("git_review_diff");
    cx.executor().advance_clock(Duration::from_secs(12));
    cx.run_until_parked();
    assert!(s.contar("git_review_diff") > lidas);
    assert_eq!(cx.read(|cx| v.read(cx).atual()), Some(0));
}

#[gpui_kit::test]
fn diff_f8_rola_a_unidade_ao_terco(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let fonte: Fonte = s.clone();
    let alvo = Alvo::Diff { lado: Lado::Alteracoes, caminho: "src/sem-quebra.ts".into() };
    let (j, v) = montar_vista(cx, move |w, cx| VisaoDiff::new(fonte, GuardaGit::new(), alvo, w, cx));
    let desenhar = |cx: &mut TestAppContext| {
        na_janela(cx, j, |w, _| w.refresh());
        cx.run_until_parked();
    };
    desenhar(cx);
    assert!(cx.read(|cx| v.read(cx).faixa_atual()).is_some(), "a geometria sai do primeiro desenho");
    for passo in [1, 1, -1] {
        v.update(cx, |v, cx| v.passo(passo, cx));
        desenhar(cx);
        let (topo, _) = cx.read(|cx| v.read(cx).faixa_atual()).unwrap();
        let rolagem = cx.read(|cx| v.read(cx).rolagem());
        // 900 de janela; o corpo é a janela menos a barra de 28 px.
        let alvo = (topo - (900. - 28.) / 3.).max(0.);
        assert!((rolagem - alvo).abs() < 1. || rolagem < alvo, "rolagem {rolagem}, alvo {alvo}");
    }
    // O divisor fica entre 15% e 85% e sobrevive à troca de modo.
    v.update(cx, |v, cx| v.definir_proporcao(95., cx));
    assert_eq!(cx.read(|cx| v.read(cx).proporcao()), 85.);
    v.update(cx, |v, cx| v.alternar_modo(cx));
    desenhar(cx);
    v.update(cx, |v, cx| v.alternar_modo(cx));
    desenhar(cx);
    assert_eq!(cx.read(|cx| v.read(cx).proporcao()), 85.);
}

#[test]
fn comparador_aguenta_milhares_de_linhas() {
    use crate::comparador::Comparador;
    let mut p = String::from("--- a/x.ts
+++ b/x.ts
@@ -1,20000 +1,20000 @@
");
    for i in 0..20_000 {
        if i % 50 == 0 {
            p.push_str(&format!("-const v{i} = antigo({i});
+const v{i} = novo({i}, extra);
"));
        } else {
            p.push_str(&format!(" const v{i} = igual({i});
"));
        }
    }
    let inicio = std::time::Instant::now();
    let c = crate::comparar::comparar(&p, None);
    let u: Vec<_> = c.unificado.iter().map(|l| l.unidade).collect();
    let cmp = Comparador::new(&p, "x.ts", &u, c.unidades.len());
    assert_eq!(cmp.unidades.len(), 400);
    assert_eq!(cmp.modelo.lado.len(), 20_000);
    assert!(inicio.elapsed() < Duration::from_secs(5), "modelo em {:?}", inicio.elapsed());
}

#[gpui_kit::test]
fn diff_nao_rastreado_so_prepara_o_arquivo(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let fonte: Fonte = s.clone();
    let alvo = Alvo::Diff { lado: Lado::Alteracoes, caminho: "docs/rascunho-painel.md".into() };
    let (j, v) = montar_vista(cx, move |w, cx| VisaoDiff::new(fonte, GuardaGit::new(), alvo, w, cx));
    assert_eq!(cx.read(|cx| v.read(cx).total_unidades()), 0, "não rastreado não tem blocos do Git");
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.preparar_arquivo(w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_stage"), 1);
}

#[gpui_kit::test]
fn diff_do_commit_le_uma_vez(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let fonte: Fonte = s.clone();
    let alvo = Alvo::Commit { hash: "4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11".into(), caminho: "src/lib/utils.ts".into() };
    let (_, v) = montar_vista(cx, move |w, cx| VisaoDiff::new(fonte, GuardaGit::new(), alvo, w, cx));
    assert_eq!(cx.read(|cx| v.read(cx).total_unidades()), 1);
    cx.executor().advance_clock(Duration::from_secs(12));
    cx.run_until_parked();
    assert_eq!(s.contar("git_commit_diff"), 1, "histórico é imutável: lê uma vez");
}

#[gpui_kit::test]
fn commit_so_renomeado_mostra_comparador_vazio(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    s.patch_de_commit("src/lib/utils.ts", "diff --git a/src/utils.ts b/src/lib/utils.ts\nsimilarity index 100%\nrename from src/utils.ts\nrename to src/lib/utils.ts\n");
    let fonte: Fonte = s.clone();
    let alvo = Alvo::Commit { hash: "4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11".into(), caminho: "src/lib/utils.ts".into() };
    let (j, v) = montar_vista(cx, move |w, cx| VisaoDiff::new(fonte, GuardaGit::new(), alvo, w, cx));
    // Desenha a vista com o comparador sem fileiras.
    na_janela(cx, j, |w, cx| v.update(cx, |_, cx| { cx.notify(); w.refresh(); }));
    cx.run_until_parked();
    assert!(cx.read(|cx| v.read(cx).tem_comparador()), "renomeação pura não é erro de interpretação");
    assert_eq!(cx.read(|cx| v.read(cx).total_unidades()), 0);
}

const DOIS: &str = "topo\n<<<<<<< ours\natual-1\n=======\nrecebida-1\n>>>>>>> theirs\nmeio\n<<<<<<< ours\natual-2\n=======\nrecebida-2\n>>>>>>> theirs\nfim\n";

#[gpui_kit::test]
fn conflito_escolhe_salva_e_marca(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    s.conflito("src/dois.ts", Some("atual-1\natual-2\n"), Some("recebida-1\nrecebida-2\n"), Some(DOIS));
    let fonte: Fonte = s.clone();
    let (j, v) = montar_vista(cx, move |w, cx| VisaoConflito::new(fonte, GuardaGit::new(), "src/dois.ts".into(), None, w, cx));
    let eventos = Rc::new(RefCell::new(Vec::new()));
    let ev = eventos.clone();
    cx.update(|cx| cx.subscribe(&v, move |_, e: &EventoConflito, _| ev.borrow_mut().push(e.clone())).detach());
    na_janela(cx, j, |w, cx| {
        v.update(cx, |v, cx| {
            v.escolher(0, Escolha::Atual, w, cx);
            v.escolher(0, Escolha::Recebida, w, cx);
        })
    });
    let texto = cx.read(|cx| v.read(cx).rascunho().unwrap().texto.clone());
    assert_eq!(texto, "topo\natual-1\nmeio\nrecebida-2\nfim\n", "texto fora dos trechos preservado");
    let revisao = cx.read(|cx| v.read(cx).rascunho().unwrap().revisao.clone());
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.salvar(false, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.texto("src/dois.ts").as_deref(), Some("topo\natual-1\nmeio\nrecebida-2\nfim\n"));
    let r = cx.read(|cx| v.read(cx).rascunho().unwrap().clone());
    assert_ne!(r.revisao, revisao, "Salvar atualiza a revisão");
    assert!(!r.sujo());
    let conflitos = s.contar("git_conflict");
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.salvar(true, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_conflict"), conflitos, "marcar não lê o conflito de novo");
    assert!(eventos.borrow().contains(&EventoConflito::Fechar));
    assert_eq!(s.estado().ok().and_then(|e| match e {
        Estado::Repo { status } => status.files.into_iter().find(|f| f.path == "src/dois.ts").map(|f| (f.staged, f.unstaged)),
        _ => None,
    }), Some((Some(Kind::Modified), None)), "resolvido sai do conflito e fica preparado");
}

#[gpui_kit::test]
fn conflito_com_marcadores_nao_marca_e_edicao_externa_deixa_velho(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    s.conflito("src/dois.ts", Some("a\n"), Some("b\n"), Some(DOIS));
    let fonte: Fonte = s.clone();
    let (j, v) = montar_vista(cx, move |w, cx| VisaoConflito::new(fonte, GuardaGit::new(), "src/dois.ts".into(), None, w, cx));
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.salvar(true, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_write_resolution"), 0, "ainda há marcadores");
    s.escrever("src/dois.ts", "mudou por fora\n");
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.mostrar(w, cx)));
    cx.run_until_parked();
    assert!(cx.read(|cx| v.read(cx).velho()), "a pasta mudou desde o rascunho");
    assert_eq!(cx.read(|cx| v.read(cx).rascunho().unwrap().texto.clone()), DOIS, "o rascunho fica até a confirmação");
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.carregar(true, w, cx)));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| v.read(cx).rascunho().unwrap().texto.clone()), "mudou por fora\n");
}

#[gpui_kit::test]
fn conflito_remocao_aceita_apaga(cx: &mut TestAppContext) {
    let s = Arc::new(Simulado::default());
    let fonte: Fonte = s.clone();
    let (j, v) = montar_vista(cx, move |w, cx| VisaoConflito::new(fonte, GuardaGit::new(), "docs/apagado.md".into(), None, w, cx));
    let rev = cx.read(|cx| v.read(cx).revisao_lida()).unwrap();
    na_janela(cx, j, |w, cx| v.update(cx, |v, cx| v.remover_confirmado(rev, w, cx)));
    cx.run_until_parked();
    assert_eq!(s.contar("git_accept_deletion"), 1);
    assert!(s.texto("docs/apagado.md").is_none());
}

#[gpui_kit::test]
fn pre_leitura_abre_a_aba_sem_ler_de_novo(cx: &mut TestAppContext) {
    use crate::pre_leitura::PreLeiturasGit;
    iniciar(cx);
    let s = Arc::new(Simulado::default());
    let fonte: Fonte = s.clone();
    let pre = PreLeiturasGit::new();
    let alvo = Alvo::Diff { lado: Lado::Alteracoes, caminho: "src/editor-workspace.tsx".into() };
    cx.update(|cx| {
        pre.pre_carregar(&fonte, &alvo, false, cx);
        // Pedido repetido (hover e botão) não lê de novo.
        pre.pre_carregar(&fonte, &alvo, true, cx);
    });
    cx.run_until_parked();
    assert_eq!(s.contar("git_review_diff"), 1);
    assert!(cx.read(|cx| pre.pronto(&alvo, cx)));
    let (p2, a2, f2) = (pre.clone(), alvo.clone(), fonte.clone());
    let (_, v) = montar_vista(cx, move |w, cx| VisaoDiff::com_pre_leitura(f2, GuardaGit::new(), a2, &p2, w, cx));
    assert_eq!(cx.read(|cx| v.read(cx).total_unidades()), 2, "a aba abre com o diff pré-lido");
    assert_eq!(s.contar("git_review_diff"), 1, "a primeira rodada usa a pré-leitura");
    assert!(!cx.read(|cx| pre.pre_carregado(&alvo, cx)), "abrir consome a entrada");
}

#[gpui_kit::test]
fn pre_leitura_pendente_e_esperada_e_vence(cx: &mut TestAppContext) {
    use crate::pre_leitura::PreLeiturasGit;
    iniciar(cx);
    let s = Arc::new(Simulado::default());
    let fonte: Fonte = s.clone();
    let pre = PreLeiturasGit::new();
    let alvo = Alvo::Commit { hash: "4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11".into(), caminho: "src/lib/utils.ts".into() };
    // Pendente quando a aba abre: a vista espera a mesma leitura.
    cx.update(|cx| pre.pre_carregar(&fonte, &alvo, true, cx));
    let (p2, a2, f2) = (pre.clone(), alvo.clone(), fonte.clone());
    let (_, v) = montar_vista(cx, move |w, cx| VisaoDiff::com_pre_leitura(f2, GuardaGit::new(), a2, &p2, w, cx));
    cx.run_until_parked();
    assert_eq!(s.contar("git_commit_diff"), 1);
    assert!(cx.read(|cx| v.read(cx).total_unidades()) > 0);
    // Pronta e não usada: vence 3 s depois.
    cx.update(|cx| pre.pre_carregar(&fonte, &alvo, true, cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| pre.pre_carregado(&alvo, cx)));
    cx.executor().advance_clock(Duration::from_millis(3100));
    assert!(!cx.read(|cx| pre.pre_carregado(&alvo, cx)));
    // A espera da abertura termina quando a leitura chega.
    let feito = Rc::new(std::cell::Cell::new(false));
    let f3 = feito.clone();
    cx.update(|cx| {
        let t = pre.aguardar(&fonte, &alvo, cx);
        cx.spawn(async move |_| {
            t.await;
            f3.set(true);
        })
        .detach();
    });
    cx.run_until_parked();
    assert!(feito.get());
    assert_eq!(s.contar("git_commit_diff"), 3);
}
