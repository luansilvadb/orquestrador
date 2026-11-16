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
    let exes = cli::Exes::new("claude.exe", "codex.exe", "orquestrador.exe");
    let (backend, _tx, eventos) =
        Backend::com(exes, places, tmp.0.join("config.json"), Some(tmp.0.join("interface.json")));
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

        // Sem projeto, o botão da tela aberta fecha a faixa.
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
fn fechar_aba_de_projeto_ativa_a_vizinha_da_esquerda(cx: &mut TestAppContext) {
    let tmp = Tmp::new("vizinha");
    let (_janela, bancada) = montar(cx, &tmp);
    abrir_pastas(cx, &bancada, &tmp, &["a", "b", "c", "d"]);
    let ids: Vec<String> = cx.update(|cx| bancada.read(cx).abas.iter().map(|a| a.id.clone()).collect());
    let ativa = |cx: &mut TestAppContext| cx.update(|cx| bancada.read(cx).ativa.clone());
    let remover = |cx: &mut TestAppContext, id: &str| cx.update(|cx| bancada.update(cx, |b, cx| b.remover_aba(id, cx)));
    cx.update(|cx| bancada.update(cx, |b, _| b.ativa = Some(ids[2].clone())));
    // Outra que não a ativa: a ativa fica.
    remover(cx, &ids[3]);
    assert_eq!(ativa(cx), Some(ids[2].clone()));
    // A ativa no meio: a da esquerda.
    remover(cx, &ids[2]);
    assert_eq!(ativa(cx), Some(ids[1].clone()));
    // A ativa na primeira posição: a nova primeira.
    cx.update(|cx| bancada.update(cx, |b, _| b.ativa = Some(ids[0].clone())));
    remover(cx, &ids[0]);
    assert_eq!(ativa(cx), Some(ids[1].clone()));
    remover(cx, &ids[1]);
    assert_eq!(ativa(cx), None);
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
    let exes = cli::Exes::new("claude.exe", "codex.exe", "orquestrador.exe");
    let (backend, _tx, eventos) =
        Backend::com(exes, places, tmp.0.join("config.json"), Some(tmp.0.join("interface.json")));
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

// ---------- ligações entre as telas ----------

/// Projeto com README e src/a.rs, aberto e desenhado.
fn com_projeto(cx: &mut TestAppContext, tmp: &Tmp, nome: &str) -> (WindowHandle<Root>, Entity<Bancada>, String) {
    let pasta = tmp.0.join(nome);
    std::fs::create_dir_all(pasta.join("src")).expect("pasta do projeto");
    std::fs::write(pasta.join("README.md"), "# Orquestrador\n\nlinha dois\n").expect("README");
    std::fs::write(pasta.join("src").join("a.rs"), "fn main() {}\n").expect("a.rs");
    let (janela, bancada) = montar(cx, tmp);
    cx.update(|cx| bancada.update(cx, |b, cx| b.abrir(pasta, cx)));
    cx.run_until_parked();
    desenhar(cx, janela);
    let id = cx.update(|cx| bancada.read(cx).ativa.clone().expect("aba ativa"));
    (janela, bancada, id)
}

fn desenhar(cx: &mut TestAppContext, janela: WindowHandle<Root>) {
    cx.update_window(janela.into(), |_, window, cx| window.render_frame(cx)).expect("janela");
    cx.run_until_parked();
    cx.update_window(janela.into(), |_, window, cx| window.render_frame(cx)).expect("janela");
}

fn abertos(cx: &mut TestAppContext, bancada: &Entity<Bancada>, id: &str) -> Vec<String> {
    cx.update(|cx| bancada.read(cx).telas[id].editor.read(cx).caminhos_abertos())
}

fn alvos(cx: &mut TestAppContext, bancada: &Entity<Bancada>, id: &str) -> Vec<orquestrador_editor::Alvo> {
    cx.update(|cx| bancada.read(cx).telas[id].editor.read(cx).workspace().abas().map(|t| t.alvo.clone()).collect())
}

#[gpui_kit::test]
fn arvore_abre_no_editor_e_remapeia(cx: &mut TestAppContext) {
    use orquestrador_arvore::EventoArvore;
    let tmp = Tmp::new("arvore");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    let arvore = cx.update(|cx| bancada.read(cx).telas[&id].arvore.clone());
    cx.update(|cx| {
        arvore.update(cx, |_, cx| cx.emit(EventoArvore::Abrir { caminho: "README.md".into(), fixo: true, ao_lado: false }))
    });
    desenhar(cx, janela);
    assert_eq!(abertos(cx, &bancada, &id), ["README.md"]);

    cx.update(|cx| {
        arvore.update(cx, |_, cx| cx.emit(EventoArvore::Renomeado { de: "README.md".into(), para: "LEIA.md".into() }))
    });
    desenhar(cx, janela);
    assert_eq!(abertos(cx, &bancada, &id), ["LEIA.md"]);

    cx.update(|cx| arvore.update(cx, |_, cx| cx.emit(EventoArvore::Excluidos(vec!["LEIA.md".into()]))));
    desenhar(cx, janela);
    assert!(abertos(cx, &bancada, &id).is_empty());

    // Pesquisar na pasta e Histórico Git trocam a tela da lateral.
    cx.update(|cx| arvore.update(cx, |_, cx| cx.emit(EventoArvore::Pesquisar("src".into()))));
    desenhar(cx, janela);
    assert_eq!(cx.update(|cx| bancada.read(cx).rota), Rota::Pesquisa);
    cx.update(|cx| arvore.update(cx, |_, cx| cx.emit(EventoArvore::HistoricoGit("src/a.rs".into()))));
    desenhar(cx, janela);
    assert_eq!(cx.update(|cx| bancada.read(cx).rota), Rota::Git);

    // Abrir no Terminal cria um shell na pasta e abre o painel.
    cx.update(|cx| arvore.update(cx, |_, cx| cx.emit(EventoArvore::AbrirNoTerminal("src".into()))));
    cx.update(|cx| {
        let b = bancada.read(cx);
        assert!(b.terminal_aberto);
        let p = &b.terminais.read(cx).projetos[&Some(id.clone())];
        assert_eq!(p.shells.last().and_then(|s| s.cwd.clone()).as_deref(), Some("src"));
    });
}

#[gpui_kit::test]
fn pesquisa_abre_na_linha(cx: &mut TestAppContext) {
    use orquestrador_pesquisa::EventoPesquisa;
    let tmp = Tmp::new("pesquisa");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    let pesquisa = cx.update(|cx| bancada.read(cx).telas[&id].pesquisa.clone());
    cx.update(|cx| {
        pesquisa.update(cx, |_, cx| {
            cx.emit(EventoPesquisa::AbrirArquivo { caminho: "src/a.rs".into(), linha: 1, coluna: 4, permanente: true })
        })
    });
    desenhar(cx, janela);
    assert_eq!(abertos(cx, &bancada, &id), ["src/a.rs"]);
}

#[gpui_kit::test]
fn git_abre_diff_e_arquivo(cx: &mut TestAppContext) {
    use orquestrador_git::{Alvo as G, EventoGit, Lado as L};
    let tmp = Tmp::new("git");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    let git = cx.update(|cx| bancada.read(cx).telas[&id].git.clone());
    cx.update(|cx| {
        git.update(cx, |_, cx| {
            cx.emit(EventoGit::AbrirAlvo { alvo: G::Diff { lado: L::Alteracoes, caminho: "README.md".into() }, permanente: true })
        })
    });
    desenhar(cx, janela);
    assert!(alvos(cx, &bancada, &id).iter().any(|a| matches!(
        a,
        orquestrador_editor::Alvo::Diff { side: orquestrador_editor::Lado::Unstaged, path } if path == "README.md"
    )));
    cx.update(|cx| git.update(cx, |_, cx| cx.emit(EventoGit::AbrirArquivo { caminho: "src/a.rs".into(), permanente: true })));
    desenhar(cx, janela);
    assert!(abertos(cx, &bancada, &id).contains(&"src/a.rs".to_string()));
}

#[gpui_kit::test]
fn ctrl_clique_do_terminal_abre_no_editor(cx: &mut TestAppContext) {
    use crate::terminal::vista::{AbrirLinks, Alvo};
    let tmp = Tmp::new("links");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    cx.update(|cx| bancada.update(cx, |b, cx| b.ir(Rota::Git, cx)));
    cx.update_window(janela.into(), |_, window, cx| {
        let f = cx.global::<AbrirLinks>().0.clone().expect("a bancada liga os links");
        f(&Alvo::Arquivo { caminho: "README.md".into(), linha: Some(3), coluna: Some(1) }, window, cx);
        f(&Alvo::Pasta("src".into()), window, cx);
    })
    .expect("janela");
    desenhar(cx, janela);
    assert_eq!(abertos(cx, &bancada, &id), ["README.md"]);
    assert_eq!(cx.update(|cx| bancada.read(cx).rota), Rota::Arquivos, "pasta revelada na árvore");
}

#[gpui_kit::test]
fn trocar_de_projeto_preserva_editor_e_vista(cx: &mut TestAppContext) {
    let tmp = Tmp::new("troca");
    let (janela, bancada, a) = com_projeto(cx, &tmp, "a");
    let pasta_b = tmp.0.join("b");
    std::fs::create_dir_all(&pasta_b).expect("b");
    cx.update(|cx| bancada.update(cx, |b, cx| b.abrir(pasta_b, cx)));
    cx.run_until_parked();
    desenhar(cx, janela);
    let b_id = cx.update(|cx| bancada.read(cx).ativa.clone().expect("b ativa"));
    assert_ne!(a, b_id);
    cx.update(|cx| bancada.update(cx, |b, cx| b.ir(Rota::Git, cx)));

    cx.update(|cx| bancada.update(cx, |b, cx| b.selecionar(&a, cx)));
    desenhar(cx, janela);
    let editor_a = cx.update(|cx| bancada.read(cx).telas[&a].editor.clone());
    cx.update_window(janela.into(), |_, window, cx| {
        let op = orquestrador_editor::Abertura { permanente: true, ..Default::default() };
        editor_a.update(cx, |e, cx| e.abrir_arquivo("README.md", op, window, cx));
    })
    .expect("janela");
    desenhar(cx, janela);
    assert_eq!(cx.update(|cx| bancada.read(cx).rota), Rota::Arquivos, "cada projeto tem a sua tela");

    cx.update(|cx| bancada.update(cx, |b, cx| b.selecionar(&b_id, cx)));
    desenhar(cx, janela);
    assert_eq!(cx.update(|cx| bancada.read(cx).rota), Rota::Git, "a vista de b voltou");
    assert!(abertos(cx, &bancada, &b_id).is_empty());

    cx.update(|cx| bancada.update(cx, |b, cx| b.selecionar(&a, cx)));
    desenhar(cx, janela);
    assert_eq!(abertos(cx, &bancada, &a), ["README.md"]);
    assert_eq!(cx.update(|cx| bancada.read(cx).telas[&a].editor.clone()), editor_a, "a mesma entidade, sem remontar");
}

/// Abre README.md no editor do projeto e acrescenta texto, deixando-o sujo.
fn sujar_readme(cx: &mut TestAppContext, janela: WindowHandle<Root>, bancada: &Entity<Bancada>, id: &str) {
    let editor = cx.update(|cx| bancada.read(cx).telas[id].editor.clone());
    cx.update_window(janela.into(), |_, window, cx| {
        let op = orquestrador_editor::Abertura { permanente: true, ..Default::default() };
        editor.update(cx, |e, cx| e.abrir_arquivo("README.md", op, window, cx));
    })
    .expect("janela");
    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, cx| {
        let doc = editor.read(cx).editor_de("README.md").expect("documento aberto");
        doc.update(cx, |s, cx| s.replace("x", window, cx));
    })
    .expect("janela");
    desenhar(cx, janela);
    assert_eq!(cx.update(|cx| editor.read(cx).nao_salvos()), ["README.md"]);
}

#[gpui_kit::test]
fn fechar_aba_com_nao_salvos_pergunta(cx: &mut TestAppContext) {
    let tmp = Tmp::new("fechar");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    sujar_readme(cx, janela, &bancada, &id);

    let fechar = gpui_kit::SharedString::from(format!("fechar-aba-{id}"));
    cx.update_window(janela.into(), |_, window, cx| {
        window.click(fechar.clone(), cx);
        window.render_frame(cx);
        assert!(window.find("fechar-confirmacao").visible());
        window.click("fechar-cancelar", cx);
        window.render_frame(cx);
        assert!(window.try_find("fechar-confirmacao").is_none());
    })
    .expect("janela");
    assert_eq!(cx.update(|cx| bancada.read(cx).abas.len()), 1, "Cancelar mantém a aba");

    cx.update_window(janela.into(), |_, window, cx| {
        window.click(fechar.clone(), cx);
        window.render_frame(cx);
        window.click("fechar-descartar", cx);
    })
    .expect("janela");
    cx.run_until_parked();
    assert!(cx.update(|cx| bancada.read(cx).abas.is_empty()));
    let disco = std::fs::read_to_string(tmp.0.join("p").join("README.md")).expect("README");
    assert!(disco.starts_with('#'), "Descartar não grava");
}

#[gpui_kit::test]
fn fechar_janela_com_nao_salvos_pergunta_e_salva(cx: &mut TestAppContext) {
    let tmp = Tmp::new("janela");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    sujar_readme(cx, janela, &bancada, &id);
    cx.update_window(janela.into(), |_, window, cx| {
        let pode = bancada.update(cx, |b, cx| b.pode_fechar_janela(window, cx));
        assert!(!pode, "não salvo segura a janela");
        window.render_frame(cx);
        assert!(window.find("fechar-confirmacao").visible());
        window.click("fechar-salvar", cx);
    })
    .expect("janela");
    cx.run_until_parked();
    assert!(cx.update(|cx| bancada.read(cx).janela_liberada), "salvo, a janela fecha");
    let disco = std::fs::read_to_string(tmp.0.join("p").join("README.md")).expect("README");
    assert!(disco.starts_with('x'), "Salvar gravou: {disco:?}");
}

#[test]
fn caminho_relativo_ao_projeto() {
    use crate::bancada::relativo;
    assert_eq!(relativo("C:\\p\\exemplo", "c:\\P\\exemplo\\src\\a.rs").as_deref(), Some("src/a.rs"));
    assert_eq!(relativo("C:\\p\\exemplo", "C:\\p\\exemplo2\\a.rs"), None);
    assert_eq!(relativo("C:\\p\\exemplo", "C:\\p\\exemplo"), None);
}

#[gpui_kit::test]
fn fabrica_cria_midia_e_conflito(cx: &mut TestAppContext) {
    use crate::telas::Conteudo;
    let tmp = Tmp::new("fabrica");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../midia/tests/fixtures/logo.png"),
        tmp.0.join("p").join("logo.png"),
    )
    .expect("png");
    let editor = cx.update(|cx| bancada.read(cx).telas[&id].editor.clone());
    cx.update_window(janela.into(), |_, window, cx| {
        let op = orquestrador_editor::Abertura { permanente: true, ..Default::default() };
        editor.update(cx, |e, cx| {
            e.abrir_arquivo("logo.png", op, window, cx);
            e.abrir_alvo(orquestrador_editor::Alvo::Conflict { path: "src/a.rs".into() }, op, window, cx);
        });
    })
    .expect("janela");
    desenhar(cx, janela);
    cx.update(|cx| {
        let t = &bancada.read(cx).telas[&id];
        let c = t.conteudos.borrow();
        assert!(c.iter().any(|(_, v)| matches!(v, Conteudo::Midia(_))), "aba de imagem");
        assert!(c.iter().any(|(_, v)| matches!(v, Conteudo::Conflito(_))), "aba de conflito");
    });
}

#[gpui_kit::test]
fn caminho_externo_abre_no_projeto_que_o_contem(cx: &mut TestAppContext) {
    let tmp = Tmp::new("externo");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    let solto = tmp.0.join("solto.ts");
    std::fs::write(&solto, "export const solto = true;\n").expect("solto");
    let dentro = tmp.0.join("p").join("src").join("a.rs");
    cx.update(|cx| bancada.update(cx, |b, cx| b.abrir_caminhos(vec![dentro, solto.clone()], cx)));
    desenhar(cx, janela);
    let abertos = abertos(cx, &bancada, &id);
    assert!(abertos.contains(&"src/a.rs".to_string()), "do projeto, relativo: {abertos:?}");
    assert!(abertos.iter().any(|c| c.ends_with("solto.ts")), "fora de projeto, no ativo: {abertos:?}");
    // Pasta já aberta só foca a aba existente.
    cx.update(|cx| bancada.update(cx, |b, cx| b.abrir(tmp.0.join("p"), cx)));
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| bancada.read(cx).abas.len()), 1);
}

#[gpui_kit::test]
fn fechar_janela_lista_arquivos_do_projeto_inativo_com_prefixo(cx: &mut TestAppContext) {
    let tmp = Tmp::new("prefixo");
    let (janela, bancada, a) = com_projeto(cx, &tmp, "a");
    sujar_readme(cx, janela, &bancada, &a);
    let b = tmp.0.join("b");
    std::fs::create_dir_all(&b).expect("b");
    cx.update(|cx| bancada.update(cx, |x, cx| x.abrir(b, cx)));
    cx.run_until_parked();
    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, cx| {
        assert!(!bancada.update(cx, |b, cx| b.pode_fechar_janela(window, cx)));
        assert_eq!(bancada.read(cx).fechamento.as_ref().expect("pergunta").arquivos, ["a: README.md"]);
        window.render_frame(cx);
        window.click("fechar-cancelar", cx);
    })
    .expect("janela");
    assert!(cx.update(|cx| !bancada.read(cx).janela_liberada), "Cancelar não fecha");
}

#[gpui_kit::test]
fn dica_do_fechar_alinha_a_direita(cx: &mut TestAppContext) {
    let tmp = Tmp::new("dica");
    let (janela, bancada) = montar(cx, &tmp);
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        bancada.update(cx, |b, cx| {
            b.dica_janela = Some(("Fechar", 0));
            cx.notify();
        });
        window.render_frame(cx);
        let dica = window.find("dica-janela");
        let largura = window.viewport_size().width;
        assert!((dica.bounds().right() - (largura - gpui_kit::px(4.))).abs() < gpui_kit::px(1.), "borda direita 4 px dentro da janela");
        assert_eq!(dica.bounds().top(), crate::tema::TITLEBAR + gpui_kit::px(4.));
    })
    .expect("janela");
}

#[gpui_kit::test]
fn dica_da_janela_some_com_menu_ou_modal(cx: &mut TestAppContext) {
    let tmp = Tmp::new("dica-menu");
    let (janela, bancada) = montar(cx, &tmp);
    cx.update_window(janela.into(), |_, window, cx| {
        bancada.update(cx, |b, cx| {
            b.dica_janela = Some(("Fechar", 0));
            cx.notify();
        });
        window.render_frame(cx);
        assert!(window.find("dica-janela").bounds().size.height > gpui_kit::px(0.), "dica à vista");
        orquestrador_ui::widgets::marcar_sobreposicao();
        bancada.update(cx, |_, cx| cx.notify());
        window.render_frame(cx);
        assert_eq!(window.find("dica-janela").bounds().size.height, gpui_kit::px(0.), "some com o menu aberto");
        std::thread::sleep(std::time::Duration::from_millis(60));
        bancada.update(cx, |_, cx| cx.notify());
        window.render_frame(cx);
        assert_eq!(window.find("dica-janela").bounds().size.height, gpui_kit::px(0.), "só volta num novo hover");
    })
    .expect("janela");
}

#[gpui_kit::test]
fn tab_percorre_os_botoes_do_fechamento(cx: &mut TestAppContext) {
    use crate::fechamento::Resposta;
    let tmp = Tmp::new("fechar-tab");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    sujar_readme(cx, janela, &bancada, &id);
    let fechar = gpui_kit::SharedString::from(format!("fechar-aba-{id}"));
    let focado = |cx: &mut TestAppContext| cx.update(|cx| bancada.read(cx).fechamento.as_ref().map(|f| f.focado));
    cx.update_window(janela.into(), |_, window, cx| window.click(fechar.clone(), cx)).expect("janela");
    desenhar(cx, janela);
    assert_eq!(focado(cx), Some(Resposta::Cancelar), "nasce no Cancelar");
    let mut passos = Vec::new();
    for tecla in ["tab", "tab", "tab", "shift-tab", "shift-tab"] {
        cx.simulate_keystrokes(janela.into(), tecla);
        desenhar(cx, janela);
        passos.push(focado(cx).expect("diálogo aberto"));
    }
    use Resposta::*;
    assert_eq!(passos, [Descartar, Salvar, Cancelar, Salvar, Descartar], "dá a volta nos dois sentidos");
    // Enter aciona o botão focado (Descartar), não mais o Cancelar.
    cx.simulate_keystrokes(janela.into(), "enter");
    cx.run_until_parked();
    assert!(cx.update(|cx| bancada.read(cx).abas.is_empty()), "Descartar fechou o projeto");
}

#[gpui_kit::test]
fn ctrl_p_nao_e_consumido_em_campo_de_texto(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let tmp = Tmp::new("ctrl-p");
    let (janela, bancada, _) = com_projeto(cx, &tmp, "p");
    // Os atalhos de projeto esperam o portão dos CLIs.
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !cx.update(|cx| bancada.read(cx).terminais.read(cx).pronto(cx)) {
        assert!(std::time::Instant::now() < limite, "portão pronto");
        std::thread::sleep(std::time::Duration::from_millis(50));
        cx.run_until_parked();
    }
    // O arquivo rápido nasce com o foco no próprio campo: o campo focado diz se ele abriu.
    let campo = |cx: &mut TestAppContext| {
        cx.update_window(janela.into(), |_, window, cx| {
            window.render_frame(cx);
            window.focused_input(cx)
        })
        .expect("janela")
    };
    // Com o foco no campo da pesquisa a tecla segue para o campo.
    cx.update_window(janela.into(), |_, window, cx| bancada.update(cx, |b, cx| b.abrir_pesquisa(window, cx))).expect("janela");
    desenhar(cx, janela);
    let pesquisa = campo(cx).expect("campo da pesquisa focado");
    cx.simulate_keystrokes(janela.into(), "ctrl-p");
    cx.run_until_parked();
    assert_eq!(campo(cx), Some(pesquisa.clone()), "num campo, Ctrl+P não abre o arquivo rápido");
    // Fora de campos, abre.
    cx.update_window(janela.into(), |_, window, cx| {
        let f = bancada.read(cx).foco.clone();
        window.focus(&f, cx);
    })
    .expect("janela");
    desenhar(cx, janela);
    assert!(campo(cx).is_none());
    cx.simulate_keystrokes(janela.into(), "ctrl-p");
    cx.run_until_parked();
    let aberto = campo(cx);
    assert!(aberto.is_some() && aberto != Some(pesquisa), "fora de campos, Ctrl+P abre o arquivo rápido e foca o campo dele");
}

#[gpui_kit::test]
fn tab_entra_na_arvore(cx: &mut TestAppContext) {
    let tmp = Tmp::new("tab-arvore");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    cx.update(|cx| bancada.update(cx, |b, cx| b.ir(Rota::Arquivos, cx)));
    desenhar(cx, janela);
    let arvore = cx.update(|cx| bancada.read(cx).telas[&id].arvore.clone());
    let mut chegou = false;
    for _ in 0..40 {
        cx.simulate_keystrokes(janela.into(), "tab");
        desenhar(cx, janela);
        chegou = cx
            .update_window(janela.into(), |_, window, cx| {
                gpui_kit::Focusable::focus_handle(arvore.read(cx), cx).contains_focused(window, cx)
            })
            .expect("janela");
        if chegou {
            break;
        }
    }
    assert!(chegou, "Tab chega ao painel da árvore");
}

#[gpui_kit::test]
fn voltar_da_pip_ativa_o_projeto_e_a_aba_da_midia(cx: &mut TestAppContext) {
    use crate::telas::Conteudo;
    let tmp = Tmp::new("pip");
    let (janela, bancada, a) = com_projeto(cx, &tmp, "a");
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../midia/tests/fixtures/logo.png"), tmp.0.join("a").join("logo.png"))
        .expect("png");
    let editor = cx.update(|cx| bancada.read(cx).telas[&a].editor.clone());
    cx.update_window(janela.into(), |_, window, cx| {
        let op = orquestrador_editor::Abertura { permanente: true, ..Default::default() };
        editor.update(cx, |e, cx| {
            e.abrir_arquivo("logo.png", op, window, cx);
            e.abrir_arquivo("README.md", op, window, cx);
        });
    })
    .expect("janela");
    desenhar(cx, janela);
    let ativa_do_editor = |cx: &mut TestAppContext| {
        cx.update(|cx| editor.read(cx).workspace().aba_focada().map(|t| t.alvo.clone()))
    };
    assert!(matches!(ativa_do_editor(cx), Some(orquestrador_editor::Alvo::File { path, .. }) if path == "README.md"));
    let pasta_b = tmp.0.join("b");
    std::fs::create_dir_all(&pasta_b).expect("b");
    cx.update(|cx| bancada.update(cx, |b, cx| b.abrir(pasta_b, cx)));
    cx.run_until_parked();
    desenhar(cx, janela);
    assert_ne!(cx.update(|cx| bancada.read(cx).ativa.clone()).as_deref(), Some(a.as_str()));

    let midia = cx.update(|cx| {
        let t = &bancada.read(cx).telas[&a];
        let c = t.conteudos.borrow();
        c.iter().find_map(|(_, v)| if let Conteudo::Midia(w) = v { w.upgrade() } else { None }).expect("aba de mídia")
    });
    cx.update(|cx| midia.update(cx, |_, cx| cx.emit(orquestrador_midia::EventoMidia::VoltarParaAba)));
    cx.run_until_parked();
    desenhar(cx, janela);
    assert_eq!(cx.update(|cx| bancada.read(cx).ativa.clone()).as_deref(), Some(a.as_str()), "o projeto da mídia volta");
    assert!(matches!(ativa_do_editor(cx), Some(orquestrador_editor::Alvo::File { path, .. }) if path == "logo.png"));
}

#[gpui_kit::test]
fn arvore_e_pesquisa_pre_carregam_no_editor(cx: &mut TestAppContext) {
    use orquestrador_arvore::EventoArvore;
    use orquestrador_pesquisa::EventoPesquisa;
    let tmp = Tmp::new("pre");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "p");
    let (arvore, pesquisa, editor) = cx.update(|cx| {
        let t = &bancada.read(cx).telas[&id];
        (t.arvore.clone(), t.pesquisa.clone(), t.editor.clone())
    });
    cx.update(|cx| arvore.update(cx, |_, cx| cx.emit(EventoArvore::PreCarregar("README.md".into()))));
    cx.update(|cx| pesquisa.update(cx, |_, cx| cx.emit(EventoPesquisa::PreLer { caminho: "src/a.rs".into() })));
    desenhar(cx, janela);
    cx.update(|cx| {
        let ed = editor.read(cx);
        assert!(ed.pre_carregado("README.md"), "a árvore pré-carrega");
        assert!(ed.pre_carregado("src/a.rs"), "a pesquisa pré-carrega");
    });
}

/// Repositório com `x.txt` em conflito de merge.
fn repo_em_conflito(pasta: &std::path::Path) {
    let git = |args: &[&str]| {
        let s = std::process::Command::new("git").args(args).current_dir(pasta).output().expect("git");
        assert!(s.status.success() || args[0] == "merge", "git {args:?}: {}", String::from_utf8_lossy(&s.stderr));
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(pasta.join("x.txt"), "base\n").expect("x");
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    git(&["checkout", "-qb", "outro"]);
    std::fs::write(pasta.join("x.txt"), "outro\n").expect("x");
    git(&["commit", "-qam", "outro"]);
    git(&["checkout", "-q", "main"]);
    std::fs::write(pasta.join("x.txt"), "main\n").expect("x");
    git(&["commit", "-qam", "main"]);
    git(&["merge", "-q", "outro"]);
}

#[gpui_kit::test]
fn rascunho_do_conflito_suja_a_aba_e_descartar_rele(cx: &mut TestAppContext) {
    use crate::telas::Conteudo;
    let tmp = Tmp::new("conflito");
    let pasta = tmp.0.join("p");
    std::fs::create_dir_all(&pasta).expect("pasta");
    repo_em_conflito(&pasta);
    let (janela, bancada) = montar(cx, &tmp);
    cx.update(|cx| bancada.update(cx, |b, cx| b.abrir(pasta.clone(), cx)));
    cx.run_until_parked();
    desenhar(cx, janela);
    let id = cx.update(|cx| bancada.read(cx).ativa.clone().expect("aba ativa"));
    let editor = cx.update(|cx| bancada.read(cx).telas[&id].editor.clone());
    cx.update_window(janela.into(), |_, window, cx| {
        let op = orquestrador_editor::Abertura { permanente: true, ..Default::default() };
        editor.update(cx, |e, cx| e.abrir_alvo(orquestrador_editor::Alvo::Conflict { path: "x.txt".into() }, op, window, cx));
    })
    .expect("janela");
    desenhar(cx, janela);
    let vista = cx.update(|cx| {
        let t = &bancada.read(cx).telas[&id];
        let c = t.conteudos.borrow();
        c.iter().find_map(|(_, v)| if let Conteudo::Conflito(w) = v { w.upgrade() } else { None }).expect("aba de conflito")
    });
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while cx.update(|cx| vista.read(cx).rascunho().is_none()) {
        assert!(std::time::Instant::now() < limite, "conflito lido");
        std::thread::sleep(std::time::Duration::from_millis(50));
        desenhar(cx, janela);
    }
    let original = cx.update(|cx| vista.read(cx).rascunho().expect("rascunho").texto.clone());
    assert!(cx.update(|cx| editor.read(cx).nao_salvos()).is_empty(), "limpo ao abrir");

    cx.update_window(janela.into(), |_, window, cx| vista.update(cx, |v, cx| v.restaurar_lado("resolvido\n".into(), window, cx)))
        .expect("janela");
    desenhar(cx, janela);
    assert_eq!(cx.update(|cx| editor.read(cx).nao_salvos()), ["x.txt"], "sujo entra no fechar com não salvos");
    let guardado = cx.update(|cx| editor.read(cx).rascunho_conflito("x.txt")).expect("rascunho guardado");
    assert_eq!(guardado.draft, "resolvido\n");

    cx.update_window(janela.into(), |_, window, cx| editor.update(cx, |e, cx| e.descartar_tudo(window, cx))).expect("janela");
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while cx.update(|cx| vista.read(cx).rascunho().map(|r| r.texto.clone())) != Some(original.clone()) {
        assert!(std::time::Instant::now() < limite, "descartar relê a vista");
        std::thread::sleep(std::time::Duration::from_millis(50));
        desenhar(cx, janela);
    }
    assert!(cx.update(|cx| editor.read(cx).nao_salvos()).is_empty());
}

#[gpui_kit::test]
fn subpasta_de_repositorio_avisa_que_abriu_a_raiz(cx: &mut TestAppContext) {
    let tmp = Tmp::new("subpasta");
    let raiz = tmp.0.join("repo");
    std::fs::create_dir_all(raiz.join("src")).expect("pasta");
    let ok = std::process::Command::new("git").args(["init", "-q"]).current_dir(&raiz).status().expect("git").success();
    assert!(ok, "git init");
    let (_janela, bancada) = montar(cx, &tmp);
    cx.update(|cx| bancada.update(cx, |b, cx| b.abrir(raiz.join("src"), cx)));
    cx.run_until_parked();
    let avisos = cx.update(|cx| orquestrador_ui::avisos::visiveis(cx));
    assert!(
        avisos.iter().any(|a| a.titulo.starts_with("Esta pasta está dentro do repositório") && a.titulo.ends_with("O projeto aberto é a raiz dele.")),
        "{:?}",
        avisos.iter().map(|a| a.titulo.clone()).collect::<Vec<_>>()
    );
}

// Porte de tests/check-titlebar-ui.mjs e check-project-tabs-ui.mjs (o que não depende do navegador).

#[gpui_kit::test]
fn controles_da_janela_colados_a_direita(cx: &mut TestAppContext) {
    let tmp = Tmp::new("controles");
    let (janela, _) = montar(cx, &tmp);
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        let largura = window.viewport_size().width;
        let b = ["janela-minimizar", "janela-maximizar", "janela-fechar"].map(|id| window.find(id).bounds());
        for x in &b {
            assert_eq!(x.size.width, crate::tema::BOTAO_JANELA, "46 px de largura");
            assert_eq!(x.size.height, crate::tema::TITLEBAR - px(1.), "altura útil sem a borda");
            assert_eq!(x.top(), px(0.));
        }
        assert_eq!(b[2].right(), largura, "Fechar colado à direita");
        assert_eq!(b[1].right(), b[2].left());
        assert_eq!(b[0].right(), b[1].left());
    })
    .expect("janela");
}

#[gpui_kit::test]
fn titulo_acompanha_o_projeto(cx: &mut TestAppContext) {
    let tmp = Tmp::new("titulo");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "exemplo");
    assert_eq!(cx.update(|cx| bancada.read(cx).titulo()), "exemplo - Orquestrador");
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(gpui_kit::SharedString::from(format!("fechar-aba-{id}")), cx);
    })
    .expect("janela");
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| bancada.read(cx).titulo()), "Orquestrador");
}

#[gpui_kit::test]
fn trocar_de_projeto_preserva_o_texto_digitado(cx: &mut TestAppContext) {
    let tmp = Tmp::new("troca-texto");
    let (janela, bancada, a) = com_projeto(cx, &tmp, "a");
    sujar_readme(cx, janela, &bancada, &a);
    abrir_pastas(cx, &bancada, &tmp, &["b"]);
    desenhar(cx, janela);
    assert_ne!(cx.update(|cx| bancada.read(cx).ativa.clone()), Some(a.clone()), "abrir a segunda pasta ativa a aba nova");
    cx.update(|cx| bancada.update(cx, |b, cx| b.selecionar(&a, cx)));
    desenhar(cx, janela);
    let texto = cx.update(|cx| {
        let ed = bancada.read(cx).telas[&a].editor.clone();
        let doc = ed.read(cx).editor_de("README.md").expect("documento aberto");
        doc.read(cx).text().to_string()
    });
    assert!(texto.contains('x'), "{texto:?}");
}

#[gpui_kit::test]
fn caminho_externo_de_pasta_vira_aba_ativa(cx: &mut TestAppContext) {
    let tmp = Tmp::new("externo-pasta");
    let (janela, bancada, a) = com_projeto(cx, &tmp, "a");
    let fora = tmp.0.join("fora");
    std::fs::create_dir_all(&fora).expect("pasta");
    cx.update(|cx| bancada.update(cx, |b, cx| b.abrir_caminhos(vec![fora], cx)));
    cx.run_until_parked();
    desenhar(cx, janela);
    assert_eq!(nomes(cx, &bancada), ["a", "fora"]);
    let ativa = cx.update(|cx| bancada.read(cx).ativa.clone()).expect("ativa");
    assert_ne!(ativa, a);
    assert_eq!(cx.update(|cx| crate::backend::nome_da_pasta(&bancada.read(cx).aba_ativa().unwrap().caminho)), "fora");
}

#[gpui_kit::test]
fn fechar_a_ultima_aba_fecha_a_lateral(cx: &mut TestAppContext) {
    let tmp = Tmp::new("ultima");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "a");
    cx.update(|cx| bancada.update(cx, |b, _| b.lateral_aberta = true));
    cx.update(|cx| bancada.update(cx, |b, cx| b.fechar_confirmado(&id, cx)));
    cx.run_until_parked();
    desenhar(cx, janela);
    assert!(cx.update(|cx| !bancada.read(cx).lateral_aberta), "sem projeto, tela limpa");
    cx.update_window(janela.into(), |_, window, _| assert!(window.try_find("tela-arquivos").is_none())).expect("janela");
}

#[gpui_kit::test]
fn reabrir_restaura_ordem_e_aba_ativa(cx: &mut TestAppContext) {
    let tmp = Tmp::new("reabrir");
    let (_, bancada) = montar(cx, &tmp);
    abrir_pastas(cx, &bancada, &tmp, &["a", "b", "c"]);
    let b_id = cx.update(|cx| bancada.read(cx).abas[1].id.clone());
    cx.update(|cx| bancada.update(cx, |b, cx| b.selecionar(&b_id, cx)));
    cx.update(|cx| bancada.update(cx, |b, _| b.mover_aba(0, 2)));
    assert_eq!(nomes(cx, &bancada), ["b", "c", "a"]);
    let (_, outra) = montar(cx, &tmp);
    cx.run_until_parked();
    assert_eq!(nomes(cx, &outra), ["b", "c", "a"]);
    assert_eq!(cx.update(|cx| crate::backend::nome_da_pasta(&outra.read(cx).aba_ativa().expect("ativa").caminho)), "b");
}

#[test]
fn cores_dos_controles_da_janela() {
    use crate::tema::cor;
    assert_eq!(cor::titlebar_text(), gpui_kit::rgb(0xcccccc).into(), "ícone #CCCCCC");
    assert_eq!(cor::fill_hover(), gpui_kit::rgba(0xffffff1a).into(), "hover branco a 10%");
    assert_eq!(cor::close_red(), gpui_kit::rgba(0xe81123e6).into(), "Fechar #E81123 a 90%");
}

// Lacunas do porte de check-titlebar-ui.mjs e check-project-tabs-ui.mjs.

#[test]
fn maximizar_alterna_dica_e_icone() {
    let (rotulo, icone) = crate::moldura::controle_maximizar(false);
    let (rotulo_max, icone_max) = crate::moldura::controle_maximizar(true);
    assert_eq!((rotulo, rotulo_max), ("Maximizar", "Restaurar"));
    assert_ne!(icone, icone_max, "o desenho de Restaurar difere do de Maximizar");
    assert_eq!(crate::moldura::controle_maximizar(false).1, icone, "Restaurar volta ao desenho original");
}

#[gpui_kit::test]
fn janela_normal_mostra_maximizar(cx: &mut TestAppContext) {
    let tmp = Tmp::new("maximizar");
    let (janela, _) = montar(cx, &tmp);
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.is_maximized());
        assert!(window.find("janela-maximizar").visible());
    })
    .expect("janela");
}

#[gpui_kit::test]
fn sem_projeto_tela_limpa_e_convites(cx: &mut TestAppContext) {
    let tmp = Tmp::new("sem-projeto");
    let (janela, bancada) = montar(cx, &tmp);
    cx.run_until_parked();
    cx.update_window(janela.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("tela-arquivos").is_none(), "faixa fechada");
        assert!(window.try_find("painel-orquestrador").is_none(), "painel do Orquestrador oculto");
        window.click("nav-git", cx);
        window.render_frame(cx);
        assert!(window.find("tela-git").visible());
        assert!(window.find("abrir-pasta-convite").visible(), "convite com Abrir pasta");
        window.click("nav-pesquisa", cx);
        window.render_frame(cx);
        assert!(window.find("tela-pesquisa").visible());
        window.click("nav-pesquisa", cx);
        window.render_frame(cx);
        assert!(window.try_find("tela-pesquisa").is_none(), "o mesmo botão fecha a faixa");
    })
    .expect("janela");
    assert_eq!(cx.update(|cx| bancada.read(cx).titulo()), "Orquestrador");
}

#[gpui_kit::test]
fn fechar_a_ultima_pasta_volta_a_tela_sem_projeto(cx: &mut TestAppContext) {
    let tmp = Tmp::new("ultima-volta");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "a");
    cx.update(|cx| bancada.update(cx, |b, cx| b.fechar_confirmado(&id, cx)));
    cx.run_until_parked();
    desenhar(cx, janela);
    assert!(cx.update(|cx| bancada.read(cx).abas.is_empty()));
    cx.update_window(janela.into(), |_, window, _| {
        assert!(window.try_find("tela-arquivos").is_none());
        assert!(window.try_find("painel-orquestrador").is_none());
        assert!(window.find("area-editor").visible(), "o editor sem projeto fica no centro");
    })
    .expect("janela");
    assert!(cx.update(|cx| bancada.read(cx).solto.is_some()), "editor sem projeto montado");
}

#[gpui_kit::test]
fn fechar_com_erro_do_core_mantem_a_aba_e_mostra_o_erro(cx: &mut TestAppContext) {
    let tmp = Tmp::new("fechar-erro");
    let (janela, bancada, id) = com_projeto(cx, &tmp, "a");
    // O core já não conhece o projeto: o `close_project` da aba falha.
    cx.update(|cx| bancada.read(cx).backend.projetos.close_project(&id)).expect("fecha no core");
    let clicar = |cx: &mut TestAppContext| {
        cx.update_window(janela.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(gpui_kit::SharedString::from(format!("fechar-aba-{id}")), cx);
        })
        .expect("janela");
        cx.run_until_parked();
    };
    clicar(cx);
    assert_eq!(nomes(cx, &bancada), ["a"], "a aba fica");
    assert!(cx.update(|cx| bancada.read(cx).fechando.is_none()), "o X volta a responder");
    let erros = |cx: &mut TestAppContext| {
        cx.update(|cx| orquestrador_ui::avisos::visiveis(cx)).iter().filter(|a| a.titulo == "Este projeto não está aberto. Abra a pasta novamente.").count()
    };
    let antes = erros(cx);
    assert!(antes >= 1, "{:?}", cx.update(|cx| orquestrador_ui::avisos::visiveis(cx)).iter().map(|a| a.titulo.clone()).collect::<Vec<_>>());
    clicar(cx);
    assert_eq!(nomes(cx, &bancada), ["a"]);
    assert!(erros(cx) >= 1);
    assert!(cx.update(|cx| bancada.read(cx).fechando.is_none()));
}

#[gpui_kit::test]
fn fechar_projeto_com_orquestrador_pede_confirmacao(cx: &mut TestAppContext) {
    use crate::fechamento::{texto_do_trabalho, Escopo, Resposta};
    use orquestrador_core::core::projects::ProjectSnapshot;
    let tmp = Tmp::new("fechar-orq");
    let (janela, bancada) = montar(cx, &tmp);
    abrir_pastas(cx, &bancada, &tmp, &["a", "b"]);
    let b_id = cx.update(|cx| bancada.read(cx).ativa.clone()).expect("ativa");
    let trabalho = ProjectSnapshot { orchestrator_open: true, executors_running: 0, executors_queued: 0 };
    assert_eq!(texto_do_trabalho(&trabalho), "O orquestrador aberto será encerrado. Os shells deste projeto também serão fechados.");
    assert_eq!(
        texto_do_trabalho(&ProjectSnapshot { orchestrator_open: false, executors_running: 2, executors_queued: 3 }),
        "2 executores em execução serão encerrados. 3 executores na fila serão removidos. Os shells deste projeto também serão fechados."
    );
    cx.update_window(janela.into(), |_, window, cx| {
        bancada.update(cx, |b, cx| b.perguntar(Escopo::Projeto(b_id.clone()), Vec::new(), true, Some(trabalho), window, cx));
        window.render_frame(cx);
        assert!(window.find("fechar-encerrar").visible(), "Fechar projeto e encerrar");
        assert!(window.try_find("fechar-salvar").is_none());
        assert_eq!(bancada.read(cx).fechamento.as_ref().unwrap().botoes(), [Resposta::Cancelar, Resposta::Fechar]);
        window.click("fechar-encerrar", cx);
    })
    .expect("janela");
    cx.run_until_parked();
    assert_eq!(nomes(cx, &bancada), ["a"]);
}

#[gpui_kit::test]
fn sincronizar_fica_no_projeto_de_origem(cx: &mut TestAppContext) {
    use orquestrador_git::simulado::{ModoSimulado, Simulado};
    use std::sync::Arc;
    let tmp = Tmp::new("sincronizar");
    let (janela, bancada) = montar(cx, &tmp);
    abrir_pastas(cx, &bancada, &tmp, &["a", "b"]);
    desenhar(cx, janela);
    let (a, b) = cx.update(|cx| (bancada.read(cx).abas[0].id.clone(), bancada.read(cx).abas[1].id.clone()));
    let (sa, sb) = (Arc::new(Simulado::new(ModoSimulado::Repo)), Arc::new(Simulado::new(ModoSimulado::Repo)));
    cx.update(|cx| {
        bancada.update(cx, |bc, cx| {
            bc.telas.get_mut(&a).unwrap().fonte = sa.clone();
            bc.telas.get_mut(&b).unwrap().fonte = sb.clone();
            bc.selecionar(&a, cx);
        })
    });
    desenhar(cx, janela);
    cx.update_window(janela.into(), |_, window, cx| {
        bancada.update(cx, |bc, cx| {
            bc.sincronizar(window, cx);
            // Troca de aba com a sincronização em curso.
            bc.selecionar(&b, cx);
        })
    })
    .expect("janela");
    cx.run_until_parked();
    desenhar(cx, janela);
    assert_eq!(sa.contar("git_pull"), 1, "pull no projeto de origem");
    assert_eq!(sb.contar("git_pull") + sb.contar("git_push"), 0, "nada no projeto que ficou ativo");
    assert!(cx.update(|cx| !bancada.read(cx).telas[&a].sincronizando), "terminou na origem");
    assert_eq!(cx.update(|cx| bancada.read(cx).ativa.clone()), Some(b));
}
