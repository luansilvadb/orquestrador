//! Cenários de `tests/check-editor-ui.mjs`, `check-editor-tabs-ui.mjs`, `check-tab-lists-ui.mjs`,
//! `check-markdown-preview-ui.mjs` e da parte de editor de `check-files-search-ui.mjs`, numa janela sem tela com um
//! projeto em pasta temporária.
use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Bounds, Entity, Point, TestAppContext, VisualTestContext, WindowBounds, WindowHandle, WindowOptions};
use orquestrador_editor::armazem::Chaves;
use orquestrador_editor::modelo::{Escopo, Vista};
use orquestrador_editor::{Abertura, Alvo, AreaEditor, Grupo};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("orq-editor-teste-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("src/a.ts"), "const a = 1;\nconst b = 2;\n").unwrap();
        std::fs::write(dir.join("src/b.ts"), "export const b = 2;\n").unwrap();
        std::fs::write(dir.join("src/c.ts"), "export const c = 3;\n").unwrap();
        std::fs::write(dir.join("docs/guia.md"), "# Guia\n\ntexto\n").unwrap();
        std::fs::write(dir.join("grande.json"), vec![b' '; 1_100_000]).unwrap();
        std::fs::write(dir.join("bin.dat"), [0u8, 1, 2, 0, 255]).unwrap();
        Tmp(dir)
    }
    fn raiz(&self) -> String {
        self.0.to_string_lossy().to_string()
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Memoria(BTreeMap<String, String>);
impl Chaves for Memoria {
    fn ler(&self, c: &str) -> Option<String> {
        self.0.get(c).cloned()
    }
    fn gravar(&mut self, c: &str, t: &str) -> Result<(), String> {
        self.0.insert(c.into(), t.into());
        Ok(())
    }
    fn apagar(&mut self, c: &str) {
        self.0.remove(c);
    }
    fn chaves(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

type Armazem = Rc<RefCell<Memoria>>;

fn montar(cx: &mut TestAppContext, raiz: &str, mem: Armazem) -> (WindowHandle<Root>, Entity<AreaEditor>) {
    let raiz = raiz.to_string();
    cx.update(|cx| {
        gpui_kit::init(cx);
        orquestrador_ui::tema::aplicar(cx);
        let opcoes = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin: Point::default(), size: size(px(1000.), px(800.)) })),
            ..Default::default()
        };
        let (janela, area) =
            gpui_kit::open_window(opcoes, cx, |window, cx| cx.new(|cx| AreaEditor::com_armazenamento(raiz, mem, window, cx))).expect("janela");
        (janela.downcast::<Root>().expect("Root"), area)
    })
}

fn com<R>(cx: &mut TestAppContext, j: WindowHandle<Root>, a: &Entity<AreaEditor>, f: impl FnOnce(&mut AreaEditor, &mut gpui_kit::Window, &mut gpui_kit::Context<AreaEditor>) -> R) -> R {
    let a = a.clone();
    let r = cx.update_window(j.into(), |_, window, cx| a.update(cx, |a, cx| f(a, window, cx))).expect("janela");
    cx.run_until_parked();
    r
}

fn nomes(cx: &mut TestAppContext, a: &Entity<AreaEditor>, g: Grupo) -> Vec<(String, bool)> {
    cx.read(|cx| {
        a.read(cx)
            .workspace()
            .grupo(g)
            .map(|g| g.tabs.iter().map(|t| (t.alvo.caminho().unwrap_or("").to_string(), t.preview)).collect())
            .unwrap_or_default()
    })
}

fn editor_ativo(cx: &mut TestAppContext, a: &Entity<AreaEditor>) -> Entity<gpui_kit::component::input::EditorState> {
    cx.read(|cx| {
        let area = a.read(cx);
        let alvo = area.workspace().aba_focada().unwrap().alvo.clone();
        area.editor_de(alvo.caminho().unwrap()).expect("editor")
    })
}

fn digitar(cx: &mut TestAppContext, j: WindowHandle<Root>, e: &Entity<gpui_kit::component::input::EditorState>, texto: &str) {
    let e = e.clone();
    let t = texto.to_string();
    cx.update_window(j.into(), |_, window, cx| {
        e.update(cx, |s, cx| {
            let fim = s.text().len();
            s.set_selected_range(fim..fim, cx);
            s.insert(t, window, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn previa_e_permanente(cx: &mut TestAppContext) {
    let tmp = Tmp::new("previa");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/a.ts", Abertura::default(), w, cx));
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/b.ts", Abertura::default(), w, cx));
    assert_eq!(nomes(cx, &a, Grupo::A), [("src/b.ts".into(), true)], "um clique substitui a prévia");
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/b.ts", Abertura { permanente: true, ..Default::default() }, w, cx));
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/c.ts", Abertura::default(), w, cx));
    assert_eq!(nomes(cx, &a, Grupo::A), [("src/b.ts".into(), false), ("src/c.ts".into(), true)]);
    // Editar torna a prévia permanente e suja.
    let e = editor_ativo(cx, &a);
    digitar(cx, j, &e, "// x");
    assert_eq!(nomes(cx, &a, Grupo::A)[1], ("src/c.ts".into(), false));
    assert_eq!(cx.read(|cx| a.read(cx).nao_salvos()), ["src/c.ts"]);
}

#[gpui_kit::test]
fn salvar_grava_no_disco(cx: &mut TestAppContext) {
    let tmp = Tmp::new("salvar");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/a.ts", Abertura { permanente: true, ..Default::default() }, w, cx));
    let e = editor_ativo(cx, &a);
    digitar(cx, j, &e, "const c = 3;\n");
    com(cx, j, &a, |a, w, cx| a.salvar_ativa(w, cx));
    assert_eq!(std::fs::read_to_string(tmp.0.join("src/a.ts")).unwrap(), "const a = 1;\nconst b = 2;\nconst c = 3;\n");
    assert!(cx.read(|cx| a.read(cx).nao_salvos()).is_empty());
}

#[gpui_kit::test]
fn fechar_sujo_pergunta_e_descartar_fecha(cx: &mut TestAppContext) {
    let tmp = Tmp::new("fechar");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/a.ts", Abertura { permanente: true, ..Default::default() }, w, cx));
    let e = editor_ativo(cx, &a);
    digitar(cx, j, &e, "x");
    let id = cx.read(|cx| a.read(cx).workspace().aba_focada().unwrap().id.clone());
    com(cx, j, &a, |a, w, cx| a.fechar(Escopo::Uma(id), w, cx));
    assert_eq!(nomes(cx, &a, Grupo::A).len(), 1, "a pergunta segura o fechamento");
    cx.update_window(j.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("discard", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(nomes(cx, &a, Grupo::A).is_empty());
    assert_eq!(std::fs::read_to_string(tmp.0.join("src/a.ts")).unwrap(), "const a = 1;\nconst b = 2;\n", "descartar não grava");
}

#[gpui_kit::test]
fn dividir_mover_e_unir(cx: &mut TestAppContext) {
    let tmp = Tmp::new("dividir");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/a.ts", Abertura { permanente: true, ..Default::default() }, w, cx));
    let id = cx.read(|cx| a.read(cx).workspace().aba_focada().unwrap().id.clone());
    com(cx, j, &a, |a, w, cx| a.dividir(&id, w, cx));
    assert_eq!(cx.read(|cx| a.read(cx).workspace().groups.len()), 2);
    // O mesmo documento nos dois grupos compartilha o editor.
    let e = editor_ativo(cx, &a);
    digitar(cx, j, &e, "// y");
    assert_eq!(cx.read(|cx| a.read(cx).nao_salvos()), ["src/a.ts"]);
    com(cx, j, &a, |a, _, cx| a.unir_grupos(cx));
    assert_eq!(cx.read(|cx| a.read(cx).workspace().groups.len()), 1);
    com(cx, j, &a, |a, w, cx| a.abrir_ao_lado("src/b.ts", Abertura::default(), w, cx));
    assert_eq!(nomes(cx, &a, Grupo::B), [("src/b.ts".into(), true)]);
}

#[gpui_kit::test]
fn markdown_alterna_preview(cx: &mut TestAppContext) {
    let tmp = Tmp::new("markdown");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("docs/guia.md", Abertura { permanente: true, ..Default::default() }, w, cx));
    cx.update_window(j.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("md-preview", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let vista = cx.read(|cx| a.read(cx).workspace().aba_focada().unwrap().view);
    assert_eq!(vista, Some(Vista::Preview));
    cx.update_window(j.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("md-codigo", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| a.read(cx).workspace().aba_focada().unwrap().view), None);
}

#[gpui_kit::test]
fn binario_e_grande(cx: &mut TestAppContext) {
    let tmp = Tmp::new("binario");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("grande.json", Abertura { permanente: true, ..Default::default() }, w, cx));
    assert!(cx.read(|cx| a.read(cx).editor_de("grande.json")).is_none(), "grande demais não abre editor");
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("bin.dat", Abertura { permanente: true, ..Default::default() }, w, cx));
    assert!(cx.read(|cx| a.read(cx).editor_de("bin.dat")).is_none());
}

#[gpui_kit::test]
fn recuperacao_devolve_rascunho(cx: &mut TestAppContext) {
    let tmp = Tmp::new("recuperacao");
    let mem: Armazem = Rc::default();
    let (j, a) = montar(cx, &tmp.raiz(), mem.clone());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/a.ts", Abertura { permanente: true, linha: Some(2), coluna: Some(3), ..Default::default() }, w, cx));
    let e = editor_ativo(cx, &a);
    digitar(cx, j, &e, "// rascunho");
    com(cx, j, &a, |a, _, cx| a.gravar_recuperacao(cx));
    assert_eq!(mem.borrow().0.len(), 1);
    // Outra área com o mesmo armazenamento: o rascunho volta sujo.
    let (j2, a2) = montar(cx, &tmp.raiz(), mem.clone());
    cx.run_until_parked();
    let _ = j2;
    assert_eq!(cx.read(|cx| a2.read(cx).nao_salvos()), ["src/a.ts"]);
    let texto = cx.read(|cx| a2.read(cx).editor_de("src/a.ts").unwrap().read(cx).text().to_string());
    assert!(texto.ends_with("// rascunho"));
}

#[gpui_kit::test]
fn disco_alterado(cx: &mut TestAppContext) {
    let tmp = Tmp::new("disco");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/b.ts", Abertura { permanente: true, ..Default::default() }, w, cx));
    // Limpo: relê do disco.
    std::fs::write(tmp.0.join("src/b.ts"), "export const b = 20;\n").unwrap();
    com(cx, j, &a, |a, w, cx| a.atualizar(w, cx));
    let t = cx.read(|cx| a.read(cx).editor_de("src/b.ts").unwrap().read(cx).text().to_string());
    assert_eq!(t, "export const b = 20;\n");
    // Sujo: não troca o texto; mostra o aviso de versão do disco.
    let e = editor_ativo(cx, &a);
    digitar(cx, j, &e, "// minha");
    std::fs::write(tmp.0.join("src/b.ts"), "export const b = 200;\n").unwrap();
    com(cx, j, &a, |a, w, cx| a.atualizar(w, cx));
    let t = cx.read(|cx| a.read(cx).editor_de("src/b.ts").unwrap().read(cx).text().to_string());
    assert!(t.ends_with("// minha"));
    cx.update_window(j.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("usar-disco").is_some(), "botão de comparar com o disco");
    })
    .unwrap();
}

#[gpui_kit::test]
fn rascunho_novo_e_reabrir_fechada(cx: &mut TestAppContext) {
    let tmp = Tmp::new("rascunho");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.novo_arquivo(w, cx));
    let alvo = cx.read(|cx| a.read(cx).workspace().aba_focada().unwrap().alvo.clone());
    assert!(matches!(alvo, Alvo::Draft { num: 1, .. }));
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/a.ts", Abertura { permanente: true, ..Default::default() }, w, cx));
    let id = cx.read(|cx| a.read(cx).workspace().aba_focada().unwrap().id.clone());
    com(cx, j, &a, |a, w, cx| a.fechar(Escopo::Uma(id), w, cx));
    com(cx, j, &a, |a, w, cx| a.reabrir_fechada(w, cx));
    assert!(nomes(cx, &a, Grupo::A).iter().any(|(p, _)| p == "src/a.ts"));
    assert!(cx.read(|cx| a.read(cx).editor_de("src/a.ts")).is_some(), "reaberta lê o arquivo de novo");
}

#[gpui_kit::test]
fn abrir_rapido_encontra_e_abre(cx: &mut TestAppContext) {
    let tmp = Tmp::new("rapido");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_rapido(w, cx));
    cx.update_window(j.into(), |_, window, cx| window.render_frame(cx)).unwrap();
    let mut vcx = VisualTestContext::from_window(j.into(), cx);
    vcx.simulate_input("b.ts");
    vcx.executor().advance_clock(std::time::Duration::from_millis(200));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(nomes(cx, &a, Grupo::A), [("src/b.ts".into(), true)]);
}

#[gpui_kit::test]
fn texto_selecionado_e_caminhos(cx: &mut TestAppContext) {
    let tmp = Tmp::new("selecao");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/a.ts", Abertura { permanente: true, linha: Some(1), coluna: Some(8), ..Default::default() }, w, cx));
    assert_eq!(cx.read(|cx| a.read(cx).texto_selecionado(cx)), "a", "sem seleção, a palavra sob o cursor");
    assert_eq!(cx.read(|cx| a.read(cx).caminhos_abertos()), ["src/a.ts"]);
    com(cx, j, &a, |a, _, cx| a.remapear("src", "lib", cx));
    assert_eq!(cx.read(|cx| a.read(cx).caminhos_abertos()), ["lib/a.ts"]);
    com(cx, j, &a, |a, _, cx| a.remover(&["lib".to_string()], cx));
    assert!(cx.read(|cx| a.read(cx).caminhos_abertos()).is_empty());
}

#[gpui_kit::test]
fn esc_no_arrasto_entre_grupos_nao_move(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton};
    let tmp = Tmp::new("arrasto-esc");
    let (j, a) = montar(cx, &tmp.raiz(), Rc::default());
    com(cx, j, &a, |a, w, cx| a.abrir_arquivo("src/a.ts", Abertura { permanente: true, ..Default::default() }, w, cx));
    com(cx, j, &a, |a, w, cx| a.abrir_ao_lado("src/b.ts", Abertura { permanente: true, ..Default::default() }, w, cx));
    let id = cx.read(|cx| a.read(cx).workspace().grupo(Grupo::A).unwrap().tabs[0].id.clone());
    let mut vcx = VisualTestContext::from_window(j.into(), cx);
    // A faixa cancela o arrasto com a janela inativa.
    vcx.update(|w, _| w.activate_window());
    vcx.run_until_parked();
    let arrastar = |vcx: &mut VisualTestContext, esc: bool| {
        let aba = vcx.debug_bounds(Box::leak(format!("aba-{id}").into_boxed_str())).expect("aba").center();
        let faixa = vcx.debug_bounds("faixa-vazia-B").expect("faixa B");
        let alvo = gpui_kit::point(faixa.right() - px(20.), faixa.center().y);
        vcx.simulate_mouse_down(aba, MouseButton::Left, Modifiers::none());
        vcx.update(|w, _| w.refresh());
        vcx.run_until_parked();
        for p in [aba + gpui_kit::point(px(10.), px(0.)), alvo - gpui_kit::point(px(5.), px(0.)), alvo] {
            vcx.simulate_mouse_move(p, Some(MouseButton::Left), Modifiers::none());
            vcx.run_until_parked();
        }
        if esc {
            vcx.simulate_keystrokes("escape");
            vcx.run_until_parked();
            vcx.simulate_mouse_move(alvo + gpui_kit::point(px(2.), px(0.)), Some(MouseButton::Left), Modifiers::none());
            vcx.run_until_parked();
        }
        vcx.simulate_mouse_up(alvo, MouseButton::Left, Modifiers::none());
        vcx.run_until_parked();
    };
    arrastar(&mut vcx, true);
    assert_eq!(nomes(&mut vcx, &a, Grupo::A), [("src/a.ts".into(), false)], "Esc cancela o arrasto");
    arrastar(&mut vcx, false);
    assert!(nomes(&mut vcx, &a, Grupo::A).is_empty(), "sem Esc, a aba vai para o outro grupo");
}
