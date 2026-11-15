//! Testes dos painéis de terminal numa janela sem tela, com PTY real: porte de check-shell-tabs-ui, check-term-links-ui,
//! check-terminal-webgl-ui (no que vale sem WebGL), check-update-clis-ui e do fluxo do orquestrador com o CLI falso
//! (src-tauri/examples/fake_cli.rs).
use super::motor::chave;
use super::painel::Terminais;
use super::vista::{AbrirLinks, AbrirUrl, Alvo, VistaTerminal};
use crate::backend::Backend;
use crate::bancada::Bancada;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as TPoint};
use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    point, px, size, AppContext, Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, Point,
    TestAppContext, WindowBounds, WindowHandle, WindowOptions,
};
use orquestrador_core::agents::{cli, session};
use orquestrador_core::config::Choice;
use orquestrador_core::core::ORCHESTRATOR;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("orq-gpui-term-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("pasta temporária");
        Tmp(dir)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// O CLI falso e o executável do app que atende `--hook` e `--mcp` (o do Tauri tem o mesmo despacho), compilados pelo
/// `cargo test --workspace`.
fn falsos() -> Option<(PathBuf, PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    let fake = exe.ancestors().map(|d| d.join("examples").join("fake_cli.exe")).find(|p| p.is_file())?;
    let pasta = fake.parent()?.parent()?.to_path_buf();
    let app = ["orquestrador-gpui.exe", "orquestrador.exe"].iter().map(|n| pasta.join(n)).find(|p| p.is_file())?;
    Some((fake, app))
}

fn montar(cx: &mut TestAppContext, tmp: &Tmp, exes: cli::Exes) -> (WindowHandle<Root>, Entity<Bancada>) {
    let places = session::Places { temp: tmp.0.join("temp"), codex_rules: tmp.0.join("rules") };
    for p in [&places.temp, &places.codex_rules] {
        std::fs::create_dir_all(p).expect("pasta da sessão");
    }
    let (backend, _tx, eventos) =
        Backend::com(exes, places, tmp.0.join("config.json"), Some(tmp.0.join("interface.json")));
    let backend = Rc::new(backend);
    // As threads do PTY e do core acordam tarefas da janela, como no app.
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tema::aplicar(cx);
        let opcoes = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(crate::tema::JANELA_LARGURA), px(crate::tema::JANELA_ALTURA)),
            })),
            ..Default::default()
        };
        let (janela, bancada) =
            gpui_kit::open_window(opcoes, cx, move |_, cx| cx.new(|cx| Bancada::new(backend, eventos, Vec::new(), cx)))
                .expect("janela de teste");
        (janela.downcast::<Root>().expect("Root do Kit"), bancada)
    })
}

fn exes_padrao() -> cli::Exes {
    cli::Exes::new("claude.exe", "codex.exe", "orquestrador-gpui.exe")
}

fn desenhar(cx: &mut TestAppContext, janela: WindowHandle<Root>) {
    cx.update_window(janela.into(), |_, window, cx| window.render_frame(cx)).expect("janela");
}

/// Roda o app até `pronto`, dando tempo às threads do PTY e do core.
#[track_caller]
fn esperar(cx: &mut TestAppContext, janela: WindowHandle<Root>, limite: Duration, mut pronto: impl FnMut(&mut TestAppContext) -> bool) {
    let fim = Instant::now() + limite;
    loop {
        cx.run_until_parked();
        desenhar(cx, janela);
        if pronto(cx) {
            return;
        }
        assert!(Instant::now() < fim, "o tempo acabou ({:?})", std::panic::Location::caller());
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn terminais(cx: &mut TestAppContext, b: &Entity<Bancada>) -> Entity<Terminais> {
    cx.update(|cx| b.read(cx).terminais.clone())
}

fn shells(cx: &mut TestAppContext, t: &Entity<Terminais>) -> Vec<String> {
    cx.update(|cx| t.read(cx).projetos.get(&None).map(|p| p.shells.iter().map(|s| s.id.clone()).collect()).unwrap_or_default())
}

fn ativa(cx: &mut TestAppContext, t: &Entity<Terminais>) -> String {
    cx.update(|cx| t.read(cx).projetos[&None].shell_ativa.clone())
}

fn vista(cx: &mut TestAppContext, t: &Entity<Terminais>, id: &str) -> Entity<VistaTerminal> {
    cx.update(|cx| t.read(cx).vistas[&chave(None, id)].clone())
}

fn texto_da_tela(cx: &mut TestAppContext, v: &Entity<VistaTerminal>) -> String {
    cx.update(|cx| {
        let motor = v.read(cx).motor.clone();
        let e = motor.travar();
        let g = e.term.grid();
        let mut s = String::new();
        for l in 0..g.screen_lines() as i32 {
            for c in 0..g.columns() {
                s.push(g[TPoint::new(Line(l), Column(c))].c);
            }
            s.push('\n');
        }
        s
    })
}

/// Abre o painel pelo botão da barra de status e completa `n` shells pelo "+", esperando cada um abrir.
fn abrir_shells(cx: &mut TestAppContext, janela: WindowHandle<Root>, b: &Entity<Bancada>, n: usize) -> Vec<String> {
    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, cx| window.click("status-terminal", cx)).expect("janela");
    let t = terminais(cx, b);
    for _ in 1..n {
        cx.run_until_parked();
        desenhar(cx, janela);
        cx.update_window(janela.into(), |_, window, cx| window.click("novo-terminal", cx)).expect("janela");
    }
    esperar(cx, janela, Duration::from_secs(20), |cx| {
        let ids = shells(cx, &t);
        ids.len() == n && cx.update(|cx| ids.iter().all(|id| t.read(cx).vistas[&chave(None, id)].read(cx).vivo))
    });
    shells(cx, &t)
}

/// Manda um evento de ponteiro à janela, como o sistema.
fn evento(cx: &mut TestAppContext, janela: WindowHandle<Root>, e: gpui_kit::PlatformInput) {
    cx.update_window(janela.into(), |_, window, cx| {
        window.dispatch_event(e, cx);
    })
    .expect("janela");
    cx.run_until_parked();
    desenhar(cx, janela);
}

#[gpui_kit::test]
fn arrastar_aba_de_shell_como_a_de_projeto(cx: &mut TestAppContext) {
    use gpui_kit::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput};
    let tmp = Tmp::new("arrasto-shell");
    let (janela, b) = montar(cx, &tmp, exes_padrao());
    let ids = abrir_shells(cx, janela, &b, 3);
    let t = terminais(cx, &b);
    desenhar(cx, janela);
    let caixa = |cx: &mut TestAppContext, i: usize| {
        cx.update(|cx| t.read(cx).faixa_shells.rolagem().bounds_for_item(i).expect("aba desenhada"))
    };
    // Janela inativa cancela o arrasto, como no React (`blur`).
    cx.update_window(janela.into(), |_, window, _| window.activate_window()).expect("janela");
    cx.run_until_parked();
    let primeira = caixa(cx, 0);
    let y = primeira.center().y;
    let x0 = primeira.origin.x + px(20.);
    let mover = |x| PlatformInput::MouseMove(MouseMoveEvent { position: point(x, y), pressed_button: Some(MouseButton::Left), ..Default::default() });
    evento(cx, janela, PlatformInput::MouseMove(MouseMoveEvent { position: point(x0, y), ..Default::default() }));
    evento(cx, janela, PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: point(x0, y), click_count: 1, ..Default::default() }));
    evento(cx, janela, mover(x0 + px(2.)));
    assert!(cx.update(|cx| !t.read(cx).faixa_shells.arrasto().expect("segurada").movendo), "abaixo de 4 px ainda é clique");
    // Passou dos 4 px: a segurada vira a ativa e as vizinhas abrem espaço.
    let fim = caixa(cx, 2).right();
    evento(cx, janela, mover(fim));
    assert_eq!(ativa(cx, &t), ids[0]);
    let a = cx.update(|cx| t.read(cx).faixa_shells.arrasto()).expect("em arrasto");
    assert!(a.movendo && a.alvo == 2 && a.deslocamento(1) < 0.);
    evento(cx, janela, PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: point(fim, y), click_count: 1, ..Default::default() }));
    assert_eq!(shells(cx, &t), vec![ids[1].clone(), ids[2].clone(), ids[0].clone()]);

    // Esc no meio do arrasto devolve tudo, e não chega ao terminal.
    let x0 = caixa(cx, 0).origin.x + px(20.);
    evento(cx, janela, PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: point(x0, y), click_count: 1, ..Default::default() }));
    evento(cx, janela, mover(fim));
    cx.simulate_keystrokes(janela.into(), "escape");
    cx.run_until_parked();
    assert!(cx.update(|cx| t.read(cx).faixa_shells.arrasto().is_none()), "Esc cancelou");
    evento(cx, janela, PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: point(fim, y), click_count: 1, ..Default::default() }));
    assert_eq!(shells(cx, &t), vec![ids[1].clone(), ids[2].clone(), ids[0].clone()]);
}

#[gpui_kit::test]
fn lista_de_abas_pelo_teclado(cx: &mut TestAppContext) {
    let tmp = Tmp::new("lista-teclado");
    let (janela, b) = montar(cx, &tmp, exes_padrao());
    let ids = abrir_shells(cx, janela, &b, 3);
    let t = terminais(cx, &b);
    desenhar(cx, janela);
    let aberta = |cx: &mut TestAppContext| cx.update(|cx| t.read(cx).lista_shells.aberta());
    cx.update_window(janela.into(), |_, window, cx| window.click("lista-shells", cx)).expect("janela");
    cx.run_until_parked();
    desenhar(cx, janela);
    assert!(aberta(cx));
    // Seta para baixo destaca a primeira; Delete a fecha e o menu fica.
    cx.simulate_keystrokes(janela.into(), "down delete");
    esperar(cx, janela, Duration::from_secs(10), |cx| shells(cx, &t).len() == 2);
    assert_eq!(shells(cx, &t), vec![ids[1].clone(), ids[2].clone()]);
    assert!(aberta(cx), "fechar uma aba não fecha o menu");
    // End destaca a última e Enter a escolhe, fechando o menu.
    cx.simulate_keystrokes(janela.into(), "home enter");
    cx.run_until_parked();
    assert!(!aberta(cx));
    assert_eq!(ativa(cx, &t), ids[1]);
    cx.update_window(janela.into(), |_, window, cx| window.click("lista-shells", cx)).expect("janela");
    cx.run_until_parked();
    cx.simulate_keystrokes(janela.into(), "escape");
    cx.run_until_parked();
    assert!(!aberta(cx), "Esc fecha");
}

#[gpui_kit::test]
fn abas_de_shell_ordem_selecao_e_vizinha_ao_fechar(cx: &mut TestAppContext) {
    let tmp = Tmp::new("abas");
    let (janela, b) = montar(cx, &tmp, exes_padrao());
    let ids = abrir_shells(cx, janela, &b, 3);
    let (a, bb, c) = (ids[0].clone(), ids[1].clone(), ids[2].clone());
    let t = terminais(cx, &b);
    assert_eq!(ativa(cx, &t), c, "a nova aba é a ativa");
    let numeros: Vec<usize> = cx.update(|cx| t.read(cx).projetos[&None].shells.iter().map(|s| s.n).collect());
    assert_eq!(numeros, vec![1, 2, 3]);

    // Clique seleciona sem mover; arrastar a primeira para depois da última reordena e ela segue ativa.
    t.update(cx, |t, cx| {
        t.selecionar_shell(&None, a.clone(), cx);
        t.mover_shell(&None, &a, 2, cx);
    });
    assert_eq!(shells(cx, &t), vec![bb.clone(), c.clone(), a.clone()]);
    assert_eq!(ativa(cx, &t), a);

    // Fechar a ativa (a última) passa para a vizinha da esquerda na ordem nova.
    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, cx| window.click(gpui_kit::SharedString::from(format!("fechar-shell-{a}")), cx))
        .expect("janela");
    esperar(cx, janela, Duration::from_secs(10), |cx| shells(cx, &t).len() == 2);
    assert_eq!(shells(cx, &t), vec![bb.clone(), c.clone()]);
    assert_eq!(ativa(cx, &t), c);

    // Para a esquerda, além da primeira: alcança a posição 0. O número fica: o fechado não renumera.
    t.update(cx, |t, cx| t.mover_shell(&None, &c, 0, cx));
    assert_eq!(shells(cx, &t), vec![c.clone(), bb.clone()]);

    // Fechar a última aba fecha o painel; o shell morreu no core.
    for id in [c, bb] {
        desenhar(cx, janela);
        cx.update_window(janela.into(), |_, window, cx| window.click(gpui_kit::SharedString::from(format!("fechar-shell-{id}")), cx))
            .expect("janela");
        esperar(cx, janela, Duration::from_secs(10), |cx| !shells(cx, &t).contains(&id));
    }
    assert!(cx.update(|cx| !b.read(cx).terminal_aberto), "sem abas o painel se esconde");
}

#[gpui_kit::test]
fn escondido_nao_desenha_e_ao_voltar_mostra_o_atual_sem_redimensionar(cx: &mut TestAppContext) {
    let tmp = Tmp::new("escondido");
    let (janela, b) = montar(cx, &tmp, exes_padrao());
    let ids = abrir_shells(cx, janela, &b, 4);
    let t = terminais(cx, &b);
    let registro = cx.update(|cx| b.read(cx).backend.terminais.clone());
    let vistas: Vec<_> = ids.iter().map(|id| vista(cx, &t, id)).collect();
    let antes: Vec<u64> = cx.update(|cx| vistas.iter().map(|v| v.read(cx).pinturas).collect());
    let tamanhos: Vec<(u16, u16)> = cx.update(|cx| vistas.iter().map(|v| v.read(cx).tamanho()).collect());
    // Saída intensa em todos; a última linha de cada um é a marca.
    let pedaco: String = (0..200).map(|i| format!("linha {i} {}\r\n", "x".repeat(60))).collect();
    for (n, id) in ids.iter().enumerate() {
        for _ in 0..20 {
            registro.receber(None, id, pedaco.as_bytes());
        }
        registro.receber(None, id, format!("MARCA-{n}\r\n").as_bytes());
    }
    cx.run_until_parked();
    desenhar(cx, janela);
    let depois: Vec<u64> = cx.update(|cx| vistas.iter().map(|v| v.read(cx).pinturas).collect());
    let visivel = ids.iter().position(|id| *id == ativa(cx, &t)).expect("ativa");
    for i in 0..4 {
        if i == visivel {
            assert!(depois[i] > antes[i], "o visível desenha");
        } else {
            assert_eq!(depois[i], antes[i], "o escondido {i} não desenha");
        }
    }
    // Trocar de aba: o conteúdo atual aparece, sem mudar colunas e linhas.
    for (i, id) in ids.iter().enumerate() {
        t.update(cx, |t, cx| t.selecionar_shell(&None, id.clone(), cx));
        desenhar(cx, janela);
        assert!(texto_da_tela(cx, &vistas[i]).contains(&format!("MARCA-{i}")));
    }
    let fim: Vec<(u16, u16)> = cx.update(|cx| vistas.iter().map(|v| v.read(cx).tamanho()).collect());
    assert_eq!(fim, tamanhos, "trocar de aba não redimensiona");
}

#[gpui_kit::test]
fn ctrl_sublinha_e_ctrl_clique_abre_caminho_na_linha_e_url(cx: &mut TestAppContext) {
    let tmp = Tmp::new("links");
    let pasta = tmp.0.join("projeto");
    std::fs::create_dir_all(pasta.join("src")).expect("pasta");
    std::fs::write(pasta.join("src").join("App.tsx"), "export function App() {\n}\n").expect("arquivo");
    let (janela, b) = montar(cx, &tmp, exes_padrao());
    b.update(cx, |b, cx| b.abrir(pasta.clone(), cx));
    esperar(cx, janela, Duration::from_secs(10), |cx| cx.update(|cx| b.read(cx).ativa.is_some()));
    let projeto = cx.update(|cx| b.read(cx).ativa.clone());
    let abertos: Rc<RefCell<Vec<Alvo>>> = Rc::default();
    let urls: Rc<RefCell<Vec<String>>> = Rc::default();
    let (a2, u2) = (abertos.clone(), urls.clone());
    cx.update(|cx| {
        cx.set_global(AbrirLinks(Some(Rc::new(move |alvo, _, _| a2.borrow_mut().push(alvo.clone())))));
        cx.set_global(AbrirUrl(Some(Rc::new(move |u| u2.borrow_mut().push(u.to_string())))));
    });
    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, cx| window.click("status-terminal", cx)).expect("janela");
    let t = terminais(cx, &b);
    esperar(cx, janela, Duration::from_secs(20), |cx| {
        cx.update(|cx| {
            let p = t.read(cx).projetos.get(&projeto).map(|p| p.shell_ativa.clone()).unwrap_or_default();
            !p.is_empty() && t.read(cx).vistas[&chave(projeto.as_deref(), &p)].read(cx).vivo
        })
    });
    let id = cx.update(|cx| t.read(cx).projetos[&projeto].shell_ativa.clone());
    let v = cx.update(|cx| t.read(cx).vistas[&chave(projeto.as_deref(), &id)].clone());
    // Limpa a tela do shell e escreve a linha com o caminho e a URL, como a saída simulada do check.
    let registro = cx.update(|cx| b.read(cx).backend.terminais.clone());
    std::thread::sleep(Duration::from_millis(1500));
    registro.receber(projeto.as_deref(), &id, b"\x1b[2J\x1b[Herro em src/App.tsx:3 veja https://example.com/doc.\r\n");
    desenhar(cx, janela);
    // Posição de um caractere na tela, pela geometria que o desenho guardou.
    let pos = |cx: &mut TestAppContext, coluna: usize| -> gpui_kit::Point<gpui_kit::Pixels> {
        cx.update(|cx| {
            let (o, c) = v.read(cx).geometria();
            point(o.x + c.width * (coluna as f32 + 0.5), o.y + c.height * 0.5)
        })
    };
    let caminho = pos(cx, "erro em ".len() + 2);
    let mover = |cx: &mut TestAppContext, p, ctrl: bool| {
        let m = Modifiers { control: ctrl, ..Default::default() };
        v.update(cx, |v, cx| v.mouse_move(&MouseMoveEvent { position: p, pressed_button: None, modifiers: m }, cx));
    };
    mover(cx, caminho, false);
    assert!(cx.update(|cx| v.read(cx).link.is_none()), "sem Ctrl não sublinha");
    mover(cx, caminho, true);
    assert!(cx.update(|cx| v.read(cx).link.is_some()), "com Ctrl o link aparece");
    v.update(cx, |v, cx| v.modificadores(Modifiers::default(), cx));
    assert!(cx.update(|cx| v.read(cx).link.is_none()), "soltar o Ctrl tira o sublinhado");

    let clicar = |cx: &mut TestAppContext, p| {
        let m = Modifiers { control: true, ..Default::default() };
        cx.update_window(janela.into(), |_, window, cx| {
            v.update(cx, |v, cx| {
                v.mouse_down(&MouseDownEvent { button: MouseButton::Left, position: p, modifiers: m, click_count: 1, first_mouse: false }, window, cx)
            })
        })
        .expect("janela");
    };
    clicar(cx, caminho);
    assert_eq!(*abertos.borrow(), vec![Alvo::Arquivo { caminho: "src/App.tsx".into(), linha: Some(3), coluna: None }]);
    assert!(cx.update(|cx| v.read(cx).motor.travar().term.selection.is_none()), "Ctrl+clique não seleciona");

    let url = pos(cx, "erro em src/App.tsx:3 veja ".len() + 4);
    clicar(cx, url);
    assert_eq!(*urls.borrow(), vec!["https://example.com/doc".to_string()]);
}

#[gpui_kit::test]
fn executor_escondido_guarda_a_saida_com_cor_e_aparece_na_aba(cx: &mut TestAppContext) {
    let tmp = Tmp::new("executor");
    let pasta = tmp.0.join("projeto");
    std::fs::create_dir_all(&pasta).expect("pasta");
    let (janela, b) = montar(cx, &tmp, exes_padrao());
    b.update(cx, |b, cx| b.abrir(pasta.clone(), cx));
    esperar(cx, janela, Duration::from_secs(10), |cx| cx.update(|cx| b.read(cx).ativa.is_some()));
    let projeto = cx.update(|cx| b.read(cx).ativa.clone());
    let t = terminais(cx, &b);
    // O orquestrador aberto (sem PTY) e um executor chegando pelo evento do core.
    t.update(cx, |t, cx| {
        t.projeto(&projeto).orquestrador = Some(super::painel::Orquestrador { escolha: Some(Choice::new("claude", "", "")), sobre: None });
        let ev = serde_json::json!({ "project": projeto, "id": "exec-1", "batch": "b", "index": 0, "title": "tarefa", "state": "running", "choice": null });
        t.receber("node-added", &ev, cx);
    });
    let registro = cx.update(|cx| b.read(cx).backend.terminais.clone());
    for i in 0..50 {
        registro.receber(projeto.as_deref(), "exec-1", format!("linha {i}\r\n").as_bytes());
    }
    registro.receber(projeto.as_deref(), "exec-1", b"\x1b[31mVERMELHO\x1b[0m\r\n");
    desenhar(cx, janela);
    let v = cx.update(|cx| t.read(cx).vistas[&chave(projeto.as_deref(), "exec-1")].clone());
    assert_eq!(cx.update(|cx| v.read(cx).pinturas), 0, "escondido, não desenhou");
    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, cx| window.click("aba-exec-1", cx)).expect("janela");
    desenhar(cx, janela);
    assert!(cx.update(|cx| v.read(cx).pinturas) > 0, "na aba, desenha");
    let tela = texto_da_tela(cx, &v);
    assert!(tela.contains("VERMELHO") && tela.contains("linha 49"));
    let vermelho = cx.update(|cx| {
        let motor = v.read(cx).motor.clone();
        let e = motor.travar();
        let g = e.term.grid();
        (0..g.screen_lines() as i32).any(|l| {
            let c = &g[TPoint::new(Line(l), Column(0))];
            c.c == 'V' && super::cores::resolver(c.fg, e.term.colors(), false) == super::cores::indice(1)
        })
    });
    assert!(vermelho, "a cor vem junto");
    // O executor terminou: a aba sai e o terminal some do registro.
    t.update(cx, |t, cx| {
        t.receber("node-state", &serde_json::json!({ "project": projeto, "id": "exec-1", "state": "done", "choice": null }), cx)
    });
    assert!(registro.obter(&chave(projeto.as_deref(), "exec-1")).is_none());
    registro.receber(projeto.as_deref(), "exec-1", b"atrasada");
    assert!(registro.obter(&chave(projeto.as_deref(), "exec-1")).is_none(), "saída atrasada não recria");
}

#[gpui_kit::test]
fn portao_na_fase_clis_mostra_as_linhas_sem_tentar_de_novo(cx: &mut TestAppContext) {
    use super::portao::{Abertura, Portao};
    use orquestrador_core::clis::{Andamento, Passo};
    let tmp = Tmp::new("portao");
    let (janela, b) = montar(cx, &tmp, exes_padrao());
    let abertura = cx.update(|cx| b.read(cx).terminais.read(cx).abertura.clone());
    abertura.update(cx, |a: &mut Abertura, cx| {
        a.estado = Portao::Clis {
            claude: Andamento { step: Passo::Updating, version: Some("2.1.325".into()) },
            codex: Andamento { step: Passo::Installing, version: None },
        };
        cx.notify();
    });
    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, _| {
        assert!(window.try_find("portao-status").is_some(), "a tela do portão aparece");
        assert!(window.try_find("portao-tentar").is_none(), "sem Tentar novamente");
    })
    .expect("janela");
    // Bloqueado, nenhum terminal abre.
    let t = terminais(cx, &b);
    t.update(cx, |t, cx| t.adicionar_shell(None, None, cx));
    assert!(shells(cx, &t).is_empty());
}

/// O fluxo inteiro com o CLI falso: abrir o orquestrador, escolher, ver os executores em abas aguardando a escolha, escolher
/// e ver as abas fecharem com o lote entregue.
#[gpui_kit::test]
fn orquestrador_delega_e_executores_aparecem_em_abas(cx: &mut TestAppContext) {
    let Some((fake, app)) = falsos() else {
        panic!("falta examples\\fake_cli.exe e o executável do app em target (rode `cargo test --workspace`)");
    };
    let tmp = Tmp::new("fluxo");
    let pasta = tmp.0.join("projeto");
    std::fs::create_dir_all(&pasta).expect("pasta");
    let git = |args: &[&str]| std::process::Command::new("git").args(args).current_dir(&pasta).output().expect("git");
    git(&["init", "-q", "-b", "main"]);
    git(&["-c", "user.email=a@b", "-c", "user.name=a", "commit", "-q", "--allow-empty", "-m", "x"]);
    std::env::set_var("ORQ_FAKE_DELEGAR", "2");
    let exes = cli::Exes::new(fake.display().to_string(), fake.display().to_string(), app.display().to_string());
    let (janela, b) = montar(cx, &tmp, exes);
    b.update(cx, |b, cx| b.abrir(pasta.clone(), cx));
    esperar(cx, janela, Duration::from_secs(10), |cx| cx.update(|cx| b.read(cx).ativa.is_some()));
    let projeto = cx.update(|cx| b.read(cx).ativa.clone());
    let t = terminais(cx, &b);

    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, cx| window.click("abrir-terminal", cx)).expect("janela");
    esperar(cx, janela, Duration::from_secs(60), |cx| cx.update_window(janela.into(), |_, w, _| w.try_find("escolha-confirmar").is_some()).unwrap());
    cx.update_window(janela.into(), |_, window, cx| {
        window.click("ferramenta-claude", cx);
    })
    .expect("janela");
    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, cx| window.click("escolha-confirmar", cx)).expect("janela");
    let orq = chave(projeto.as_deref(), ORCHESTRATOR);
    esperar(cx, janela, Duration::from_secs(30), |cx| cx.update(|cx| t.read(cx).projetos[&projeto].executores.len() == 2));
    let estados: Vec<String> = cx.update(|cx| t.read(cx).projetos[&projeto].executores.iter().map(|e| e.estado.clone()).collect());
    assert_eq!(estados, vec!["awaiting-choice", "awaiting-choice"]);
    // A janela de escolha dos executores abre sozinha e não fecha sem confirmar.
    esperar(cx, janela, Duration::from_secs(60), |cx| cx.update_window(janela.into(), |_, w, _| w.try_find("escolha-confirmar").is_some()).unwrap());
    cx.update_window(janela.into(), |_, window, _| assert!(window.try_find("escolha-fechar").is_none())).expect("janela");
    cx.update_window(janela.into(), |_, window, cx| window.click("escolha-confirmar", cx)).expect("janela");
    esperar(cx, janela, Duration::from_secs(60), |cx| {
        cx.update(|cx| {
            let p = &t.read(cx).projetos[&projeto];
            p.executores.is_empty() && p.escolha_executores.is_some()
        })
    });
    let v = cx.update(|cx| t.read(cx).vistas[&orq].clone());
    esperar(cx, janela, Duration::from_secs(30), |cx| texto_da_tela(cx, &v).contains("lote entregue"));
    // Digitar no orquestrador chega ao CLI.
    v.update(cx, |v, _| v.escrever("ola\r"));
    esperar(cx, janela, Duration::from_secs(10), |cx| texto_da_tela(cx, &v).contains("eco:ola"));
    std::env::remove_var("ORQ_FAKE_DELEGAR");
}
