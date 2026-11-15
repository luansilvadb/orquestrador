//! Testes da moldura numa janela sem tela: navegação, painel Terminal e abas de projeto com o core em pastas temporárias.
use crate::backend::Backend;
use crate::estado::{Estado, Paineis, Projetos};
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
        Backend::com(exes, places, tmp.0.join("config.json"), Some(tmp.0.join("interface.json")), false);
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
    let salvas: Projetos = Estado::abrir(Some(tmp.0.join("interface.json"))).ler();
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
    let salvas: Projetos = Estado::abrir(Some(tmp.0.join("interface.json"))).ler();
    assert!(salvas.abertas.is_empty());
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("painel-orquestrador").is_none());
    })
    .expect("janela");
}

/// Abre as pastas em ordem e devolve os caminhos das abas.
fn abrir_pastas(cx: &mut TestAppContext, bancada: &Entity<Bancada>, tmp: &Tmp, nomes: &[&str]) {
    for nome in nomes {
        let pasta = tmp.0.join(nome);
        std::fs::create_dir_all(&pasta).expect("pasta do projeto");
        cx.update(|cx| bancada.update(cx, |b, cx| b.abrir(pasta, cx)));
        cx.run_until_parked();
    }
}

fn nomes(cx: &mut TestAppContext, bancada: &Entity<Bancada>) -> Vec<String> {
    cx.update(|cx| bancada.read(cx).abas.iter().map(|a| crate::backend::nome_da_pasta(&a.caminho)).collect())
}

#[gpui_kit::test]
fn reordena_abas_e_grava_a_ordem(cx: &mut TestAppContext) {
    let tmp = Tmp::new("reordenar");
    let (janela, bancada) = montar(cx, &tmp);
    abrir_pastas(cx, &bancada, &tmp, &["a", "b", "c"]);
    assert_eq!(nomes(cx, &bancada), ["a", "b", "c"]);

    cx.update(|cx| bancada.update(cx, |b, _| b.mover_aba(0, 2)));
    assert_eq!(nomes(cx, &bancada), ["b", "c", "a"]);
    let salvas: Projetos = Estado::abrir(Some(tmp.0.join("interface.json"))).ler();
    let gravados: Vec<String> = salvas.abertas.iter().map(|c| crate::backend::nome_da_pasta(c)).collect();
    assert_eq!(gravados, ["b", "c", "a"]);

    // Arrasto pela faixa: segura a primeira e solta além da última.
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        bancada.update(cx, |b, cx| {
            let n = b.abas.len();
            let (x, larg) = {
                let r = b.abas_ui.rolagem();
                let caixa = r.bounds_for_item(0).expect("aba desenhada");
                (f32::from(caixa.origin.x) + 10., f32::from(caixa.size.width))
            };
            b.abas_ui.pressionar(0, n, 0, x);
            b.abas_ui.mover(x + 2.);
            assert!(!b.abas_ui.arrasto().expect("segurada").movendo, "abaixo de 4 px ainda é clique");
            b.abas_ui.mover(x + larg + 20.);
            if let (_, Some(m)) = b.abas_ui.soltar(true) {
                b.mover_aba(m.de, m.para);
            }
            cx.notify();
        });
    })
    .expect("janela");
    assert_eq!(nomes(cx, &bancada), ["c", "b", "a"]);

    // Esc (soltar sem confirmar) devolve à ordem anterior.
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        bancada.update(cx, |b, cx| {
            let n = b.abas.len();
            let x = f32::from(b.abas_ui.rolagem().bounds_for_item(0).expect("aba").origin.x) + 10.;
            b.abas_ui.pressionar(0, n, 0, x);
            b.abas_ui.mover(x + 500.);
            assert_eq!(b.abas_ui.soltar(false), (true, None));
            let _ = cx;
        });
    })
    .expect("janela");
    assert_eq!(nomes(cx, &bancada), ["c", "b", "a"]);
}

#[test]
fn estado_grava_reabre_e_preserva_secoes_desconhecidas() {
    let tmp = Tmp::new("estado");
    let arquivo = tmp.0.join("sub").join("interface.json");
    let mut e = Estado::abrir(Some(arquivo.clone()));
    assert_eq!(e.ler::<Projetos>(), Projetos::default(), "arquivo ausente começa vazio");
    let projetos = Projetos { abertas: vec!["C:/x".into()], ativa: Some("C:/x".into()), recentes: vec!["C:/x".into()] };
    let paineis = Paineis { largura_lateral: Some(300.), largura_painel: Some(500.), altura_terminal: Some(200.), terminal_aberto: true };
    e.gravar(&projetos);
    e.gravar(&paineis);
    assert!(!arquivo.with_extension("json.tmp").exists(), "o temporário foi renomeado");

    // Seção de outra fase sobrevive à regravação.
    let mut json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&arquivo).expect("lido")).expect("JSON");
    json["editor"] = serde_json::json!({ "versao": 1, "dados": { "x": 1 } });
    std::fs::write(&arquivo, json.to_string()).expect("gravado");

    let mut e = Estado::abrir(Some(arquivo.clone()));
    assert_eq!(e.ler::<Projetos>(), projetos);
    assert_eq!(e.ler::<Paineis>(), paineis);
    e.gravar(&Projetos::default());
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&arquivo).expect("lido")).expect("JSON");
    assert_eq!(json["editor"]["dados"]["x"], 1);
}

#[test]
fn estado_tolera_arquivo_corrompido_e_valores_invalidos() {
    let tmp = Tmp::new("corrompido");
    let arquivo = tmp.0.join("interface.json");
    std::fs::write(&arquivo, "{ isto não é JSON").expect("gravado");
    let mut e = Estado::abrir(Some(arquivo.clone()));
    assert_eq!(e.ler::<Paineis>(), Paineis::default());
    e.gravar(&Paineis { terminal_aberto: true, ..Default::default() });
    assert!(Estado::abrir(Some(arquivo.clone())).ler::<Paineis>().terminal_aberto, "regravou por cima do corrompido");

    // Versão desconhecida volta ao padrão sem derrubar a outra seção; medida fora de faixa é descartada.
    let json = serde_json::json!({
        "projetos": { "versao": 99, "dados": { "abertas": ["C:/x"] } },
        "paineis": { "versao": 1, "dados": { "larguraLateral": 5.0, "larguraPainel": 600.0, "terminalAberto": "sim" } },
    });
    std::fs::write(&arquivo, json.to_string()).expect("gravado");
    let e = Estado::abrir(Some(arquivo.clone()));
    assert_eq!(e.ler::<Projetos>(), Projetos::default());
    assert_eq!(e.ler::<Paineis>(), Paineis::default(), "tipo errado num campo invalida a seção");
    let json = serde_json::json!({ "paineis": { "versao": 1, "dados": { "larguraLateral": 5.0, "larguraPainel": 600.0 } } });
    std::fs::write(&arquivo, json.to_string()).expect("gravado");
    let p = Estado::abrir(Some(arquivo)).ler::<Paineis>();
    assert_eq!(p.largura_lateral, None);
    assert_eq!(p.largura_painel, Some(600.));
}

/// Bancada sem janela: com o Terminal aberto e desenhado, shells de verdade rodariam fora do executor de teste.
fn bancada_sem_janela(cx: &mut TestAppContext, tmp: &Tmp) -> Entity<Bancada> {
    let places = session::Places { temp: tmp.0.join("temp"), codex_rules: tmp.0.join("rules") };
    let exes = cli::Exes::new("claude.exe", "codex.exe", "orquestrador-gpui.exe");
    let (backend, _tx, eventos) =
        Backend::com(exes, places, tmp.0.join("config.json"), Some(tmp.0.join("interface.json")), false);
    let backend = Rc::new(backend);
    cx.update(|cx| cx.new(|cx| Bancada::new(backend, eventos, Vec::new(), cx)))
}

#[gpui_kit::test]
fn paineis_e_terminal_voltam_ao_reabrir(cx: &mut TestAppContext) {
    let tmp = Tmp::new("paineis");
    let bancada = bancada_sem_janela(cx, &tmp);
    cx.update(|cx| {
        bancada.update(cx, |b, cx| {
            b.alternar_terminal(cx);
            b.comecar_arrasto(crate::bancada::Divisor::Lateral, gpui_kit::point(px(0.), px(0.)));
            b.arrastar(gpui_kit::point(px(40.), px(0.)), size(px(1400.), px(900.)), cx);
            b.soltar(cx);
        })
    });
    let salvo: Paineis = Estado::abrir(Some(tmp.0.join("interface.json"))).ler();
    assert_eq!(salvo.largura_lateral, Some(crate::tema::LATERAL + 40.));
    assert!(salvo.terminal_aberto);

    // Uma segunda bancada no mesmo arquivo é o app reaberto.
    let reaberta = bancada_sem_janela(cx, &tmp);
    cx.update(|cx| {
        let b = reaberta.read(cx);
        assert_eq!(b.largura_lateral, crate::tema::LATERAL + 40.);
        assert_eq!(b.largura_painel, crate::tema::PAINEL);
        assert!(b.terminal_aberto);
    });
}
