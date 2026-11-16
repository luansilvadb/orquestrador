//! Contas do git gutter com a fixture `src/marcadores.ts` e a quebra de linha da captura, numa janela sem tela.
use gpui_kit::base::input::{browser_linebreaks, wrap_line_boundaries, WrappingIndent};
use gpui_kit::base::Root;
use gpui_kit::component::input::EditorState;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size, AppContext, Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, Point, TestAppContext, WindowBounds, WindowHandle, WindowOptions};
use orquestrador_editor::armazem::Chaves;
use orquestrador_editor::gutter_git::{BaseGit, FonteGutter, Tipo};
use orquestrador_editor::{documento, Abertura, AreaEditor};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

// ---- Quebra de linha ----

/// Linhas visuais de `texto` com uma fonte monoespaçada de 1 px por caractere.
fn linhas_visuais(texto: &str, largura: f32) -> Vec<&str> {
    let quebras = wrap_line_boundaries(texto, px(largura), WrappingIndent::Same, |t| px(t.chars().count() as f32));
    let mut out = Vec::new();
    let mut ini = 0;
    for q in quebras.into_iter().chain([texto.len()]) {
        out.push(&texto[ini..q]);
        ini = q;
    }
    out
}

#[test]
fn quebra_nos_pontos_da_captura() {
    // Na captura, 40 colunas cabem e 41 não; os espaços ficam no fim da linha e contam (`break-spaces`).
    let l = 40.5;
    assert_eq!(
        linhas_visuais("// Recuperação: localStorage por projeto (`orquestrador.editor:<project>`), validada ao ler; nunca grava no disco.", l),
        ["// Recuperação: localStorage por ", "projeto (`orquestrador.editor:", "<project>`), validada ao ler; nunca ", "grava no disco."]
    );
    assert_eq!(
        linhas_visuais("import { ActionIcon, Button, ContextMenuTrigger, type ContextMenuItem, DropdownMenu, Modal, Text, toast } from '@lobehub/ui/base-ui';", l),
        ["import { ActionIcon, Button, ", "ContextMenuTrigger, type ", "ContextMenuItem, DropdownMenu, Modal, ", "Text, toast } from '@lobehub/ui/base-", "ui';"]
    );
    assert_eq!(
        linhas_visuais("import { action, blocked, DIVIDER, fileName, useChord } from './file-tabs';", l),
        ["import { action, blocked, DIVIDER, ", "fileName, useChord } from './file-tabs';"]
    );
    // Continuação no recuo da linha (`wrap-indent.ts`): cabe a largura menos o recuo.
    assert_eq!(
        linhas_visuais(
            "  forwardRef, Fragment, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type DragEvent, type FocusEvent, type KeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode,",
            l
        ),
        [
            "  forwardRef, Fragment, useEffect, ",
            "useImperativeHandle, useLayoutEffect, ",
            "useMemo, useRef, useState, type ",
            "DragEvent, type FocusEvent, type ",
            "KeyboardEvent, type PointerEvent as ",
            "ReactPointerEvent, type ReactNode,",
        ]
    );
}

#[test]
fn oportunidades_de_quebra_do_chromium() {
    let pos = |t: &str, depois: &str| t.find(depois).unwrap() + depois.len();
    let t = "editor:<project>";
    assert!(browser_linebreaks(t).contains(&pos(t, ":")), "antes de `<`");
    let t = "base-ui";
    assert!(browser_linebreaks(t).contains(&pos(t, "-")), "depois de `-`");
    for t in ["a,b", "a:b", "a.b", "f(x)", "f(-1)", "{a}"] {
        assert_eq!(browser_linebreaks(t), [t.len()], "{t}: sem quebra no meio");
    }
    assert!(browser_linebreaks("ab-12").contains(&3), "`-` antes de dígito, depois de alfanumérico");
    // Cada espaço é um ponto de quebra (`break-spaces`).
    assert_eq!(browser_linebreaks("a   b"), [2, 3, 4, 5]);
}

// ---- Janela ----

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str, arquivos: &[(&str, &str)]) -> Tmp {
        let dir = std::env::temp_dir().join(format!("orq-editor-git-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (c, t) in arquivos {
            let p = dir.join(c);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        }
        Tmp(dir)
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

/// O Git simulado: a base de `src/marcadores.ts`, as leituras e as gravações do índice.
#[derive(Default)]
struct Git {
    base: Mutex<BTreeMap<String, BaseGit>>,
    leituras: Mutex<Vec<String>>,
    gravacoes: Mutex<Vec<(String, String, String)>>,
}

impl FonteGutter for Git {
    fn base(&self, caminho: &str) -> Result<Option<BaseGit>, String> {
        self.leituras.lock().unwrap().push(caminho.into());
        Ok(self.base.lock().unwrap().get(caminho).cloned())
    }
    fn gravar_indice(&self, caminho: &str, texto: &str, esperado: &str) -> Result<(), String> {
        let mut b = self.base.lock().unwrap();
        let base = b.get_mut(caminho).ok_or("fora do índice")?;
        if base.index.as_deref() != Some(esperado) {
            return Err("O preparado deste arquivo mudou desde a última leitura.".into());
        }
        base.index = Some(texto.into());
        self.gravacoes.lock().unwrap().push((caminho.into(), texto.into(), esperado.into()));
        Ok(())
    }
}

fn marcadores() -> (String, String, String) {
    let mut head = String::from("// Marcadores de mudança do editor.\n");
    for i in 1..=80 {
        head.push_str(&format!("export const v{i} = {i};\n"));
    }
    let indice = head.replace("export const v2 = 2;", "export const v2 = 20;");
    let pasta = indice
        .replace("export const v6 = 6;\n", "export const v6 = 6;\nexport const extra1 = 1;\nexport const extra2 = 2;\n")
        .replace("export const v12 = 12;\n", "")
        .replace("export const v20 = 20;", "export const v20 = 200;");
    (head, indice, pasta)
}

const ESPERADO: [(usize, Tipo, bool); 4] =
    [(3, Tipo::Modificado, true), (8, Tipo::Adicionado, false), (15, Tipo::Apagado, false), (22, Tipo::Modificado, false)];

struct Cena {
    _tmp: Tmp,
    j: WindowHandle<Root>,
    a: Entity<AreaEditor>,
    git: Arc<Git>,
    head: String,
    indice: String,
}

fn montar(cx: &mut TestAppContext, tag: &str) -> Cena {
    let (head, indice, pasta) = marcadores();
    let tmp = Tmp::new(tag, &[("src/marcadores.ts", &pasta), ("docs/rascunho.md", "# rascunho\n")]);
    let git = Arc::new(Git::default());
    git.base.lock().unwrap().insert("src/marcadores.ts".into(), BaseGit { head: Some(head.clone()), index: Some(indice.clone()) });
    let raiz = tmp.0.to_string_lossy().to_string();
    let g = git.clone();
    let (j, a) = cx.update(|cx| {
        gpui_kit::init(cx);
        orquestrador_ui::tema::aplicar(cx);
        let opcoes = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin: Point::default(), size: size(px(1000.), px(800.)) })),
            ..Default::default()
        };
        let (janela, area) = gpui_kit::open_window(opcoes, cx, |window, cx| {
            cx.new(|cx| {
                let mut a = AreaEditor::com_armazenamento(raiz, Rc::new(RefCell::new(Memoria::default())), window, cx);
                a.definir_fonte_git(g, cx);
                a
            })
        })
        .expect("janela");
        (janela.downcast::<Root>().expect("Root"), area)
    });
    let c = Cena { _tmp: tmp, j, a, git, head, indice };
    c.com(cx, |a, w, cx| a.abrir_arquivo("src/marcadores.ts", Abertura { permanente: true, ..Default::default() }, w, cx));
    c.quadro(cx);
    c
}

impl Cena {
    fn com<R>(&self, cx: &mut TestAppContext, f: impl FnOnce(&mut AreaEditor, &mut gpui_kit::Window, &mut gpui_kit::Context<AreaEditor>) -> R) -> R {
        let a = self.a.clone();
        let r = cx.update_window(self.j.into(), |_, window, cx| a.update(cx, |a, cx| f(a, window, cx))).expect("janela");
        cx.run_until_parked();
        r
    }
    fn quadro(&self, cx: &mut TestAppContext) {
        cx.update_window(self.j.into(), |_, window, cx| window.render_frame(cx)).unwrap();
        cx.run_until_parked();
        cx.update_window(self.j.into(), |_, window, cx| window.render_frame(cx)).unwrap();
    }
    fn doc(&self, cx: &mut TestAppContext) -> String {
        cx.read(|cx| self.a.read(cx).workspace().aba_focada().unwrap().alvo.doc_id().unwrap().to_string())
    }
    fn editor(&self, cx: &mut TestAppContext) -> Entity<EditorState> {
        cx.read(|cx| self.a.read(cx).editor_de("src/marcadores.ts").expect("editor"))
    }
    fn blocos(&self, cx: &mut TestAppContext) -> Vec<(usize, Tipo, bool)> {
        let d = self.doc(cx);
        cx.read(|cx| self.a.read(cx).alteracoes_git(&d).iter().map(|b| (b.de, b.tipo, b.preparado)).collect())
    }
    fn aberto(&self, cx: &mut TestAppContext) -> Option<usize> {
        let d = self.doc(cx);
        cx.read(|cx| self.a.read(cx).alteracao_aberta(&d))
    }
    fn texto(&self, cx: &mut TestAppContext) -> String {
        let e = self.editor(cx);
        cx.read(|cx| e.read(cx).text().to_string())
    }
    fn editar(&self, cx: &mut TestAppContext, faixa: std::ops::Range<usize>, novo: &str) {
        let e = self.editor(cx);
        let novo = novo.to_string();
        cx.update_window(self.j.into(), |_, window, cx| {
            e.update(cx, |s, cx| {
                s.set_selected_range(faixa, cx);
                s.replace(novo, window, cx);
            })
        })
        .unwrap();
        cx.run_until_parked();
    }
    /// Foca o editor e dispara a ação (Alt+F5 e afins).
    fn acao(&self, cx: &mut TestAppContext, acao: Box<dyn gpui_kit::Action>) {
        let e = self.editor(cx);
        cx.update_window(self.j.into(), |_, window, cx| {
            e.update(cx, |s, cx| s.focus(window, cx));
            window.dispatch_action(acao, cx);
        })
        .unwrap();
        cx.run_until_parked();
        self.quadro(cx);
    }
    fn clicar(&self, cx: &mut TestAppContext, id: &'static str) {
        cx.update_window(self.j.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
        cx.run_until_parked();
        self.quadro(cx);
    }
    fn linha_do_cursor(&self, cx: &mut TestAppContext) -> usize {
        let e = self.editor(cx);
        cx.read(|cx| e.read(cx).cursor_position().line as usize + 1)
    }
    /// Topo do texto da linha (base 1), em coordenadas da janela.
    fn topo_da_linha(&self, cx: &mut TestAppContext, linha: usize) -> gpui_kit::Pixels {
        let e = self.editor(cx);
        cx.read(|cx| {
            let s = e.read(cx);
            let mapa = s.display_map();
            let r = mapa.buffer_line_to_display_row(linha - 1);
            s.text_bounds().unwrap().top() + mapa.display_row_top(r, s.line_height().unwrap())
        })
    }
}

#[gpui_kit::test]
fn os_tres_tipos_digitacao_e_salvar(cx: &mut TestAppContext) {
    let c = montar(cx, "tipos");
    assert_eq!(c.blocos(cx), ESPERADO);
    // Linha nova no fim da linha 30 (`v28`): a 31 vira acrescentada na hora, sem nova leitura da base.
    let leituras = c.git.leituras.lock().unwrap().len();
    let fim = c.texto(cx).find("export const v28 = 28;").unwrap() + "export const v28 = 28;".len();
    c.editar(cx, fim..fim, "\nexport const nova = 0;");
    let mut com_nova = ESPERADO.to_vec();
    com_nova.push((31, Tipo::Adicionado, false));
    assert_eq!(c.blocos(cx), com_nova);
    assert!(c.git.leituras.lock().unwrap().len() - leituras <= 1, "o marcador não dependeu de nova leitura da base");
    // Desfazer a edição: os marcadores voltam.
    c.editar(cx, fim..fim + "\nexport const nova = 0;".len(), "");
    assert_eq!(c.blocos(cx), ESPERADO);
    // Salvar relê a base logo depois da gravação.
    let antes = c.git.leituras.lock().unwrap().len();
    c.editar(cx, fim..fim, "\nexport const salva = 1;");
    c.com(cx, |a, w, cx| a.salvar_ativa(w, cx));
    assert!(c.git.leituras.lock().unwrap().len() > antes, "nova leitura da base depois de salvar");
}

#[gpui_kit::test]
fn sem_marcadores_em_nao_rastreado_e_rascunho(cx: &mut TestAppContext) {
    let c = montar(cx, "sem");
    c.com(cx, |a, w, cx| a.abrir_arquivo("docs/rascunho.md", Abertura { permanente: true, ..Default::default() }, w, cx));
    let d = c.doc(cx);
    assert!(cx.read(|cx| c.a.read(cx).alteracoes_git(&d).is_empty()), "não rastreado sem marcador");
    let leituras = c.git.leituras.lock().unwrap().len();
    c.com(cx, |a, w, cx| a.novo_arquivo(w, cx));
    let d = c.doc(cx);
    assert!(cx.read(|cx| c.a.read(cx).alteracoes_git(&d).is_empty()), "rascunho sem marcador");
    assert_eq!(c.git.leituras.lock().unwrap().len(), leituras, "rascunho não consulta o Git");
}

#[gpui_kit::test]
fn bloco_entre_linhas_reserva_altura_e_acompanha_o_texto(cx: &mut TestAppContext) {
    let c = montar(cx, "bloco");
    let topo22 = c.topo_da_linha(cx, 22);
    let topo21 = c.topo_da_linha(cx, 21);
    // Alt+F5 a partir da linha 1 vai à primeira alteração; mais três chegam à da linha 22.
    for _ in 0..4 {
        c.acao(cx, Box::new(documento::ProximaAlteracao));
    }
    assert_eq!(c.aberto(cx), Some(3));
    assert_eq!(c.linha_do_cursor(cx), 22);
    let e = c.editor(cx);
    let caixa = cx.read(|cx| e.read(cx).block_bounds(21)).expect("bloco reservado acima da linha 22");
    // Barra de 22 px, uma linha antiga de 19 px e as duas bordas.
    assert_eq!(caixa.size.height, px(43.));
    // Rolar para revelar muda tudo junto; a distância entre a 21 e a 22 cresce exatamente a altura do bloco.
    let (n21, n22) = (c.topo_da_linha(cx, 21), c.topo_da_linha(cx, 22));
    assert_eq!(n22 - n21, (topo22 - topo21) + px(43.));
    assert_eq!(caixa.bottom(), n22, "o widget fica logo acima da primeira linha do bloco");
    // Clicar no meio do bloco não põe o cursor numa linha que não existe: o widget fica por cima.
    let antes = c.linha_do_cursor(cx);
    let meio = caixa.center();
    cx.update_window(c.j.into(), |_, window, cx| {
        let base = MouseDownEvent { button: MouseButton::Left, position: meio, modifiers: Modifiers::default(), click_count: 1, first_mouse: false };
        window.dispatch_event(PlatformInput::MouseDown(base), cx);
        window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: meio, modifiers: Modifiers::default(), click_count: 1 }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(c.linha_do_cursor(cx), antes);
    // Digitar dentro do bloco mantém aberto.
    let fim = c.texto(cx).find("v20 = 200;").unwrap() + "v20 = 200;".len();
    c.editar(cx, fim..fim, " // x");
    assert_eq!(c.aberto(cx), Some(3));
    // Restaurar volta o HEAD, fecha e não grava nada.
    c.clicar(cx, "git-hunk-restaurar");
    assert!(c.texto(cx).contains("export const v20 = 20;\n"));
    assert_eq!(c.aberto(cx), None);
    assert!(c.git.gravacoes.lock().unwrap().is_empty());
    assert_eq!(c.blocos(cx), ESPERADO[..3]);
    c.quadro(cx);
    assert_eq!(cx.read(|cx| e.read(cx).block_bounds(21)), None, "fechado, nada reservado");
    assert_eq!(c.topo_da_linha(cx, 22) - c.topo_da_linha(cx, 21), topo22 - topo21);
}

#[gpui_kit::test]
fn navegacao_da_a_volta_e_esc_fecha(cx: &mut TestAppContext) {
    let c = montar(cx, "navegar");
    let mut visitadas = Vec::new();
    for _ in 0..5 {
        c.acao(cx, Box::new(documento::ProximaAlteracao));
        visitadas.push((c.aberto(cx), c.linha_do_cursor(cx)));
    }
    assert_eq!(visitadas, [(Some(0), 3), (Some(1), 8), (Some(2), 15), (Some(3), 22), (Some(0), 3)]);
    c.acao(cx, Box::new(documento::AlteracaoAnterior));
    assert_eq!((c.aberto(cx), c.linha_do_cursor(cx)), (Some(3), 22), "Shift+Alt+F5 no primeiro vai ao último");
    c.clicar(cx, "git-hunk-proxima");
    assert_eq!(c.aberto(cx), Some(0), "seta para baixo dá a volta");
    c.clicar(cx, "git-hunk-anterior");
    assert_eq!(c.aberto(cx), Some(3), "seta para cima dá a volta");
    cx.simulate_keystrokes(c.j.into(), "escape");
    cx.run_until_parked();
    assert_eq!(c.aberto(cx), None, "Esc fecha");
    c.acao(cx, Box::new(documento::ProximaAlteracao));
    c.clicar(cx, "git-hunk-fechar");
    assert_eq!(c.aberto(cx), None, "o X fecha");
}

#[gpui_kit::test]
fn clicar_no_marcador_abre_e_fecha(cx: &mut TestAppContext) {
    let c = montar(cx, "marcador");
    let e = c.editor(cx);
    let x = cx.read(|cx| {
        let s = e.read(cx);
        s.input_bounds().left() + s.gutter_width().unwrap() - px(27.) + px(2.)
    });
    // A posição do marcador é relida a cada clique: o bloco aberto empurra a linha 8 para baixo.
    let clicar = |cx: &mut TestAppContext| {
        let p = gpui_kit::point(x, c.topo_da_linha(cx, 8) + px(5.));
        cx.update_window(c.j.into(), |_, window, cx| {
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: p, modifiers: Modifiers::default(), click_count: 1, first_mouse: false }),
                cx,
            );
            window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: p, modifiers: Modifiers::default(), click_count: 1 }), cx);
        })
        .unwrap();
        cx.run_until_parked();
        c.quadro(cx);
    };
    clicar(cx);
    assert_eq!(c.aberto(cx), Some(1), "clicar no marcador da linha 8 abre o bloco");
    clicar(cx);
    assert_eq!(c.aberto(cx), None, "clicar de novo fecha");
}

#[gpui_kit::test]
fn preparar_e_tirar_do_preparo_gravam_o_indice(cx: &mut TestAppContext) {
    let c = montar(cx, "preparar");
    c.acao(cx, Box::new(documento::ProximaAlteracao));
    c.acao(cx, Box::new(documento::ProximaAlteracao));
    assert_eq!(c.aberto(cx), Some(1));
    c.clicar(cx, "git-hunk-preparar");
    let com_extra = c.indice.replace("export const v6 = 6;\n", "export const v6 = 6;\nexport const extra1 = 1;\nexport const extra2 = 2;\n");
    {
        let g = c.git.gravacoes.lock().unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0], ("src/marcadores.ts".into(), com_extra.clone(), c.indice.clone()), "só as duas linhas acrescentadas entram");
    }
    assert_eq!(c.blocos(cx)[1], (8, Tipo::Adicionado, true), "releu a base: vazado");
    assert_eq!(c.aberto(cx), Some(1), "o bloco continua aberto");
    c.clicar(cx, "git-hunk-preparar");
    assert_eq!(c.git.gravacoes.lock().unwrap()[1].1, c.indice, "tirar do preparo volta o índice");
    assert_eq!(c.blocos(cx), ESPERADO);
    // O bloco que já vinha preparado (linha 3): tirar do preparo deixa o índice igual ao HEAD.
    c.acao(cx, Box::new(documento::AlteracaoAnterior));
    assert_eq!(c.aberto(cx), Some(0));
    c.clicar(cx, "git-hunk-preparar");
    assert_eq!(c.git.gravacoes.lock().unwrap()[2].1, c.head);
}
