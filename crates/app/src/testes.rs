//! Testes da moldura numa janela sem tela: navegação, painel Terminal e abas de projeto com o core em pastas temporárias.
use crate::backend::{AbasSalvas, Backend};
use crate::bancada::{Bancada, Rota};
use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Bounds, Entity, Point, TestAppContext, WindowBounds, WindowHandle, WindowOptions};
use orquestrador_core::agents::{cli, session};
use std::path::PathBuf;
use std::rc::Rc;

/// Pasta temporária própria do teste; some no fim, mesmo com falha.
struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("orq-gpui-teste-{}-{tag}", std::process::id()));
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

fn montar(cx: &mut TestAppContext, tmp: &Tmp) -> (WindowHandle<Root>, Entity<Bancada>) {
    let places = session::Places { temp: tmp.0.join("temp"), codex_rules: tmp.0.join("rules") };
    let exes = cli::Exes::new("claude.exe", "codex.exe", "orquestrador-gpui.exe");
    let (backend, _tx, eventos) =
        Backend::com(exes, places, tmp.0.join("config.json"), Some(tmp.0.join("abas.json")), false);
    let backend = Rc::new(backend);
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

#[gpui_kit::test]
fn activity_bar_troca_a_tela(cx: &mut TestAppContext) {
    let tmp = Tmp::new("nav");
    let (janela, bancada) = montar(cx, &tmp);
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("tela-arquivos").is_none(), "sem projeto a faixa nasce fechada");

        window.click("nav-pesquisa", cx);
        assert!(window.find("tela-pesquisa").visible());
        assert_eq!(bancada.read(cx).rota, Rota::Pesquisa);

        window.click("nav-git", cx);
        assert!(window.find("tela-git").visible());
        assert!(window.try_find("tela-pesquisa").is_none());

        window.click("nav-arquivos", cx);
        assert!(window.find("tela-arquivos").visible());

        // Sem projeto, o botão da tela aberta fecha a faixa, como no React.
        window.click("nav-arquivos", cx);
        assert!(window.try_find("tela-arquivos").is_none());
    })
    .expect("janela");
}

#[gpui_kit::test]
fn botao_terminal_abre_e_fecha_o_painel(cx: &mut TestAppContext) {
    let tmp = Tmp::new("terminal");
    let (janela, bancada) = montar(cx, &tmp);
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("painel-terminal").is_none());
        window.click("status-terminal", cx);
        assert!(window.find("painel-terminal").visible());
        assert!(bancada.read(cx).terminal_aberto);
        window.click("status-terminal", cx);
        assert!(window.try_find("painel-terminal").is_none());
    })
    .expect("janela");
}

#[gpui_kit::test]
fn abre_e_fecha_aba_de_projeto(cx: &mut TestAppContext) {
    let tmp = Tmp::new("projeto");
    let pasta = tmp.0.join("exemplo");
    std::fs::create_dir_all(&pasta).expect("pasta do projeto");
    let (janela, bancada) = montar(cx, &tmp);

    cx.update(|cx| bancada.update(cx, |b, cx| b.abrir(pasta.clone(), cx)));
    cx.run_until_parked();

    let id = cx.update(|cx| {
        let b = bancada.read(cx);
        assert_eq!(b.abas.len(), 1);
        assert_eq!(crate::backend::nome_da_pasta(&b.abas[0].caminho), "exemplo");
        assert!(b.backend.projetos.core(&b.abas[0].id).is_ok(), "o core registrou o projeto");
        b.ativa.clone().expect("aba ativa")
    });
    let salvas: AbasSalvas = serde_json::from_str(&std::fs::read_to_string(tmp.0.join("abas.json")).expect("abas gravadas"))
        .expect("JSON das abas");
    assert_eq!(salvas.abertas.len(), 1);
    assert_eq!(salvas.recentes.len(), 1);

    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("painel-orquestrador").visible());
        window.click(gpui_kit::SharedString::from(format!("fechar-aba-{id}")), cx);
    })
    .expect("janela");
    cx.run_until_parked();

    cx.update(|cx| {
        let b = bancada.read(cx);
        assert!(b.abas.is_empty());
        assert!(b.ativa.is_none());
        assert!(b.backend.projetos.core(&id).is_err(), "o core encerrou o projeto");
    });
    let salvas: AbasSalvas =
        serde_json::from_str(&std::fs::read_to_string(tmp.0.join("abas.json")).expect("abas gravadas")).expect("JSON");
    assert!(salvas.abertas.is_empty());
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("painel-orquestrador").is_none());
    })
    .expect("janela");
}
