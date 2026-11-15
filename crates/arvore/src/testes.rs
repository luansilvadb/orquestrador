//! Porte da parte de árvore de `tests/check-files-search-ui.mjs`, numa janela de teste do Kit com o core numa pasta
//! temporária de verdade (o lugar do `src/backend/mock.ts`). Excluir manda os arquivos temporários à Lixeira. Recortar,
//! copiar e colar usam o clipboard de arquivos do Windows, que é do usuário: ficam de fora daqui.
use crate::{EventoArvore, Explorador};
use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, size, AppContext, Bounds, Entity, Point, SharedString, TestAppContext, VisualTestContext, WindowBounds,
    WindowHandle, WindowOptions,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("orq-arvore-teste-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn arquivo(raiz: &Path, rel: &str, texto: &str) {
    let p = raiz.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, texto).unwrap();
}

/// A árvore do mock (`src/backend/mock.ts`), sem Git.
fn exemplo(raiz: &Path) {
    for rel in [
        ".gitignore", "README.md", "debug.log", "package.json", "assets/logo.png", "dados/exportacao.json",
        "docs/design-system.md", "docs/rascunho-painel.md", "docs/apagado.md", "docs/visual-comparador.md",
        "src/App.tsx", "src/main.tsx", "src/lib/utils.ts", "dist/index.js",
    ] {
        arquivo(raiz, rel, rel);
    }
    std::fs::create_dir_all(raiz.join("tests")).unwrap();
}

struct Mesa {
    janela: WindowHandle<Root>,
    arvore: Entity<Explorador>,
    eventos: Rc<RefCell<Vec<EventoArvore>>>,
    cx: VisualTestContext,
}

fn montar(cx: &mut TestAppContext, raiz: &Path) -> Mesa {
    let raiz = raiz.to_path_buf();
    let (janela, arvore) = cx.update(|cx| {
        gpui_kit::init(cx);
        orquestrador_ui::tema::aplicar(cx);
        let opcoes = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds { origin: Point::default(), size: size(px(700.), px(600.)) })),
            ..Default::default()
        };
        let (janela, a) =
            gpui_kit::open_window(opcoes, cx, |window, cx| cx.new(|cx| Explorador::new(raiz, "exemplo", window, cx))).unwrap();
        (janela.downcast::<Root>().unwrap(), a)
    });
    let eventos = Rc::new(RefCell::new(Vec::new()));
    let e = eventos.clone();
    cx.update(|cx| {
        cx.subscribe(&arvore, move |_, ev: &EventoArvore, _| e.borrow_mut().push(ev.clone())).detach();
    });
    let vcx = VisualTestContext::from_window(janela.into(), cx);
    let mut m = Mesa { janela, arvore, eventos, cx: vcx };
    m.esperar();
    m
}

impl Mesa {
    /// Deixa o relógio da atualização correr e os pedidos ao core terminarem.
    fn esperar(&mut self) {
        for _ in 0..3 {
            self.cx.executor().advance_clock(Duration::from_millis(200));
            self.cx.run_until_parked();
            // O core roda de verdade (disco e `git`): dá tempo ao trabalho fora do executor.
            std::thread::sleep(Duration::from_millis(30));
            self.cx.run_until_parked();
        }
    }

    fn janela<R>(&mut self, f: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::App) -> R) -> R {
        self.cx.update_window(self.janela.into(), |_, window, cx| f(window, cx)).unwrap()
    }

    fn id(caminho: &str) -> SharedString {
        format!("arvore-linha-{caminho}").into()
    }

    fn tem(&mut self, caminho: &str) -> bool {
        let id = Self::id(caminho);
        self.janela(|w, cx| {
            w.render_frame(cx);
            w.try_find(id).is_some()
        })
    }

    fn clicar(&mut self, caminho: &str) {
        let id = Self::id(caminho);
        self.janela(|w, cx| w.click(id, cx));
        self.esperar();
    }

    fn clicar_com(&mut self, caminho: &str, m: gpui_kit::Modifiers) {
        let id = Self::id(caminho);
        self.janela(|w, cx| w.click_with_modifiers(id, m, cx));
        self.esperar();
    }

    fn clicar_id(&mut self, id: &str) {
        let id = SharedString::from(id.to_string());
        self.janela(|w, cx| w.click(id, cx));
        self.esperar();
    }

    fn direito(&mut self, caminho: &str) {
        let id = Self::id(caminho);
        self.janela(|w, cx| w.right_click(id, cx));
        self.esperar();
    }

    fn menu(&mut self, rotulo: &str) {
        self.clicar_id(&format!("arvore-menu-{rotulo}"));
    }

    fn tecla(&mut self, k: &str) {
        self.janela(|w, cx| w.press(k, cx));
        self.esperar();
    }

    fn digitar(&mut self, texto: &str) {
        self.janela(|w, cx| w.input(texto, cx));
        self.esperar();
    }

    /// Troca o texto todo do campo (o `fill` do Playwright).
    fn preencher(&mut self, texto: &str) {
        self.tecla("ctrl-a");
        self.tecla("backspace");
        self.digitar(texto);
    }

    fn ler<R>(&mut self, f: impl FnOnce(&mut Explorador, &mut gpui_kit::Context<Explorador>) -> R) -> R {
        let a = self.arvore.clone();
        self.cx.update(|_, cx| a.update(cx, f))
    }

    fn selecao(&mut self) -> Vec<String> {
        self.ler(|a, _| a.selecao().to_vec())
    }

    fn campo(&mut self) -> Option<(String, String)> {
        self.ler(|a, cx| a.estado_campo(cx))
    }

    fn erro_campo(&mut self) -> Option<String> {
        self.ler(|a, cx| a.motivo_campo(cx))
    }

    fn expandir(&mut self, pastas: &[&str]) {
        let p: Vec<String> = pastas.iter().map(|s| s.to_string()).collect();
        let a = self.arvore.clone();
        self.cx.update(|window, cx| a.update(cx, |a, cx| a.expandir(p, window, cx)));
        self.esperar();
    }

    fn focado(&mut self, dialogo: bool) -> bool {
        let a = self.arvore.clone();
        self.cx.update(|window, cx| {
            let a = a.read(cx);
            if dialogo { a.foco_no_dialogo(window) } else { a.foco_no_painel(window) }
        })
    }

    fn eventos(&mut self) -> Vec<EventoArvore> {
        std::mem::take(&mut *self.eventos.borrow_mut())
    }
}

/// Abre o arquivo sem compartilhar: mover ou apagar falha até soltar.
fn travar(p: &Path) -> std::fs::File {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().read(true).share_mode(0).open(p).unwrap()
    }
    #[cfg(not(windows))]
    {
        std::fs::File::open(p).unwrap()
    }
}

fn mods(ctrl: bool, shift: bool) -> gpui_kit::Modifiers {
    gpui_kit::Modifiers { control: ctrl, shift, ..Default::default() }
}

#[gpui_kit::test]
fn clique_vazio_limpa_selecao_e_ctrl_a_inclui_linhas_virtuais(cx: &mut TestAppContext) {
    let tmp = Tmp::new("ctrl-a");
    exemplo(&tmp.0);
    for i in 0..180 {
        arquivo(&tmp.0, &format!("grande/item-{i:03}.md"), &format!("{i}\n"));
    }
    let mut m = montar(cx, &tmp.0);
    assert!(m.tem("grande"));
    m.clicar("README.md");
    assert_eq!(m.selecao(), ["README.md"]);
    assert!(matches!(m.eventos().last(), Some(EventoArvore::Abrir { fixo: false, .. })), "clique simples abre prévia");
    // Clique na área vazia (abaixo das linhas): foca o painel e limpa a seleção.
    m.janela(|w, cx| w.click_at("arvore-painel", gpui_kit::point(px(10.), px(520.)), cx));
    m.esperar();
    assert!(m.selecao().is_empty(), "vazio limpa a seleção");
    m.tecla("ctrl-a");
    assert!(m.selecao().contains(&"grande".to_string()), "pasta fechada entra na seleção");
    assert!(!m.selecao().iter().any(|s| s.starts_with("grande/")), "descendente fechado fica fora");
    m.expandir(&["docs", "grande"]);
    // Com a lista cheia não sobra área vazia à vista: o evento vai direto ao painel, como no React.
    let a = m.arvore.clone();
    m.cx.update(|window, cx| a.update(cx, |a, cx| a.clique_vazio(window, cx)));
    m.esperar();
    assert!(m.selecao().is_empty());
    m.tecla("ctrl-a");
    let s = m.selecao();
    assert!(s.contains(&"docs".to_string()) && s.contains(&"grande".to_string()));
    assert!(s.contains(&"grande/item-179.md".to_string()), "linha virtual (fora da tela) também selecionada");
    assert!(!m.tem("grande/item-179.md"), "último arquivo fora da tela não é desenhado");
    m.tecla("end");
    assert!(m.tem("grande/item-179.md") && m.tem("README.md"), "End rola até o fim da lista");
    m.clicar("grande/item-179.md");
    m.tecla("ctrl-a");
    assert!(m.selecao().contains(&"grande/item-179.md".to_string()), "Ctrl+A também vale com o cursor na linha");
    m.clicar("grande/item-179.md");
    m.tecla("f2");
    assert_eq!(m.campo().map(|c| c.0), Some("item-179.md".into()));
    m.preencher("outro-nome.md");
    m.tecla("ctrl-a");
    assert_eq!(m.campo().unwrap().1, "outro-nome.md", "campo conserva Ctrl+A de texto");
    m.tecla("escape");
    assert!(m.campo().is_none());
}

#[gpui_kit::test]
fn criar_na_pasta_renomear_pasta_e_excluir_cancelado(cx: &mut TestAppContext) {
    let tmp = Tmp::new("criar");
    exemplo(&tmp.0);
    let mut m = montar(cx, &tmp.0);
    m.expandir(&["docs"]);
    m.direito("docs");
    m.menu("Nova Pasta...");
    assert_eq!(m.campo().map(|c| c.0), Some(String::new()), "o campo aparece na árvore, sem diálogo");
    m.tecla("escape");
    assert!(m.campo().is_none(), "Esc cancela");
    m.direito("docs");
    m.menu("Nova Pasta...");
    m.digitar("a:b");
    assert!(m.erro_campo().unwrap().contains("caractere que o Windows não aceita"));
    m.preencher("projeto");
    assert_eq!(m.erro_campo(), None, "nome válido tira o motivo");
    m.tecla("enter");
    assert!(tmp.0.join("docs/projeto").is_dir());
    assert!(m.tem("docs/projeto"));
    m.direito("docs/projeto");
    m.menu("Novo Arquivo...");
    m.digitar("marcador.md");
    m.tecla("enter");
    assert!(tmp.0.join("docs/projeto/marcador.md").is_file());
    assert!(m.tem("docs/projeto/marcador.md"));
    assert!(m.eventos().contains(&EventoArvore::Abrir { caminho: "docs/projeto/marcador.md".into(), fixo: true, ao_lado: false }));

    // Renomear a pasta: o evento leva a bancada a trocar o caminho das abas e dos rascunhos.
    m.direito("docs/projeto");
    m.menu("Renomear...");
    assert_eq!(m.campo().unwrap().1, "projeto", "pasta: o nome inteiro selecionado");
    m.preencher("projeto-novo");
    m.tecla("enter");
    assert!(tmp.0.join("docs/projeto-novo/marcador.md").is_file());
    assert!(m.tem("docs/projeto-novo/marcador.md"), "a pasta renomeada continua aberta");
    assert!(!m.tem("docs/projeto"));
    assert!(m.eventos().contains(&EventoArvore::Renomeado { de: "docs/projeto".into(), para: "docs/projeto-novo".into() }));

    // Excluir cancelado: nada muda.
    m.ler(|a, _| a.definir_sujos(vec!["docs/projeto-novo/marcador.md".into()]));
    m.direito("docs/projeto-novo");
    m.menu("Excluir...");
    let (pergunta, aviso) = m.ler(|a, _| a.textos_dialogo()).unwrap();
    assert_eq!(pergunta, "Tem certeza de que deseja excluir 'projeto-novo' e o conteúdo dela?");
    assert!(aviso.unwrap().contains("Há alterações não salvas em um item: docs/projeto-novo"));
    m.clicar_id("arvore-excluir-cancelar");
    assert!(m.ler(|a, _| a.textos_dialogo()).is_none());
    assert!(tmp.0.join("docs/projeto-novo/marcador.md").is_file());
    assert!(m.tem("docs/projeto-novo/marcador.md"));
}

#[gpui_kit::test]
fn renomear_inline_seleciona_base_cancela_recusa_e_renomeia(cx: &mut TestAppContext) {
    let tmp = Tmp::new("renomear");
    exemplo(&tmp.0);
    arquivo(&tmp.0, "docs/ren-a.md", "A\n");
    arquivo(&tmp.0, "docs/ren-b.md", "B\n");
    arquivo(&tmp.0, "docs/.ren-oculto", "O\n");
    std::fs::create_dir_all(tmp.0.join("docs/rn-pasta")).unwrap();
    let mut m = montar(cx, &tmp.0);
    m.expandir(&["docs"]);

    m.clicar("docs/ren-a.md");
    m.tecla("f2");
    assert_eq!(m.campo().unwrap().1, "ren-a", "seleção inicial: só o nome base");
    m.tecla("escape");
    assert!(m.campo().is_none());
    assert!(tmp.0.join("docs/ren-a.md").is_file(), "Esc não renomeia");
    assert!(m.focado(false), "o foco volta ao painel");

    m.direito("docs/ren-a.md");
    m.menu("Renomear...");
    assert_eq!(m.campo().unwrap().1, "ren-a", "o menu também seleciona o nome base");
    m.preencher("ren-b.md");
    assert!(m.erro_campo().unwrap().starts_with("Um arquivo ou pasta ren-b.md já existe nesta localização."));
    m.tecla("enter");
    assert!(m.campo().is_some(), "colisão não fecha o campo");
    assert_eq!(std::fs::read_to_string(tmp.0.join("docs/ren-a.md")).unwrap(), "A\n");
    assert_eq!(std::fs::read_to_string(tmp.0.join("docs/ren-b.md")).unwrap(), "B\n");
    m.preencher("a:b");
    assert!(m.erro_campo().unwrap().contains("caractere que o Windows não aceita"));
    m.tecla("enter");
    assert!(m.campo().is_some(), "nome inválido não fecha o campo");
    m.preencher(" ren-a2.md ");
    assert!(m.erro_campo().unwrap().contains("espaço, nem terminar com ponto"));
    m.tecla("enter");
    assert!(m.campo().is_some());
    assert!(tmp.0.join("docs/ren-a.md").is_file());
    m.tecla("escape");

    m.clicar("docs/ren-a.md");
    m.tecla("f2");
    m.preencher("ren-c.md");
    m.tecla("enter");
    assert!(tmp.0.join("docs/ren-c.md").is_file() && !tmp.0.join("docs/ren-a.md").exists());
    assert!(m.tem("docs/ren-c.md") && !m.tem("docs/ren-a.md"));
    assert_eq!(m.selecao(), ["docs/ren-c.md"], "o item renomeado fica selecionado");
    assert!(m.focado(false), "o foco volta à linha");

    m.clicar("docs/.ren-oculto");
    m.tecla("f2");
    assert_eq!(m.campo().unwrap().1, ".ren-oculto", "arquivo que começa com ponto: nome inteiro");
    m.tecla("escape");

    m.clicar("docs/rn-pasta");
    m.tecla("f2");
    assert_eq!(m.campo().unwrap().1, "rn-pasta");
    m.preencher("rn-nova");
    m.tecla("enter");
    assert!(m.tem("docs/rn-nova") && !m.tem("docs/rn-pasta"));
}

#[gpui_kit::test]
fn excluir_delete_e_menu_abrem_o_dialogo_com_foco_e_enter_confirma(cx: &mut TestAppContext) {
    let tmp = Tmp::new("excluir");
    exemplo(&tmp.0);
    arquivo(&tmp.0, "apagar-a.md", "A");
    arquivo(&tmp.0, "apagar-b.md", "B");
    let mut m = montar(cx, &tmp.0);
    for (nome, por_tecla) in [("apagar-a.md", true), ("apagar-b.md", false)] {
        m.clicar(nome);
        if por_tecla {
            m.tecla("delete");
        } else {
            m.direito(nome);
            m.menu("Excluir...");
        }
        let (pergunta, _) = m.ler(|a, _| a.textos_dialogo()).unwrap();
        assert_eq!(pergunta, format!("Tem certeza de que deseja excluir '{nome}'?"));
        assert!(m.focado(true), "foco no diálogo: Enter confirma");
        m.tecla("enter");
        assert!(m.ler(|a, _| a.textos_dialogo()).is_none());
        assert!(!tmp.0.join(nome).exists(), "foi para a Lixeira");
        assert!(!m.tem(nome));
        assert!(m.eventos().contains(&EventoArvore::Excluidos(vec![nome.into()])));
    }
}

#[gpui_kit::test]
fn selecao_shift_ctrl_e_arraste_move_com_reconciliacao(cx: &mut TestAppContext) {
    let tmp = Tmp::new("mover");
    exemplo(&tmp.0);
    for n in ["alfa", "beta", "delta", "gama"] {
        arquivo(&tmp.0, &format!("docs/{n}.md"), &n.to_uppercase());
    }
    let mut m = montar(cx, &tmp.0);
    m.expandir(&["docs"]);
    m.clicar("docs/beta.md");
    m.clicar_com("docs/delta.md", mods(false, true));
    assert_eq!(m.selecao(), ["docs/beta.md", "docs/delta.md"], "Shift entre vizinhos pega só os dois");
    assert!(!m.selecao().contains(&"docs/alfa.md".to_string()));
    m.clicar_com("docs/delta.md", mods(true, false));
    assert_eq!(m.selecao(), ["docs/beta.md"], "Ctrl tira");
    m.clicar_com("docs/delta.md", mods(true, false));
    assert_eq!(m.selecao(), ["docs/beta.md", "docs/delta.md"], "Ctrl põe");
    assert!(!m.eventos().iter().any(|e| matches!(e, EventoArvore::Abrir { caminho, .. } if caminho == "docs/delta.md")), "Ctrl/Shift só selecionam");

    // Arraste da seleção para "dados": os dois vão, e a seleção acompanha.
    m.janela(|w, cx| w.drag_to(Mesa::id("docs/beta.md"), Mesa::id("dados"), cx));
    m.esperar();
    assert!(tmp.0.join("dados/beta.md").is_file() && tmp.0.join("dados/delta.md").is_file());
    assert!(!tmp.0.join("docs/beta.md").exists());
    assert!(!m.tem("docs/beta.md"));
    assert!(m.eventos().iter().any(|e| matches!(e, EventoArvore::Movidos(v) if v.len() == 2)));
    assert_eq!(m.selecao(), ["dados/beta.md", "dados/delta.md"]);
    assert!(m.tem("dados/beta.md"), "o destino abre");

    // Falha de I/O no meio do lote (gama travado por outro processo): alfa sai, gama fica, e a árvore reconcilia só o feito.
    let trava = travar(&tmp.0.join("docs/gama.md"));
    m.clicar("docs/alfa.md");
    m.clicar_com("docs/gama.md", mods(true, false));
    m.janela(|w, cx| w.drag_to(Mesa::id("docs/alfa.md"), Mesa::id("tests"), cx));
    m.esperar();
    drop(trava);
    assert!(tmp.0.join("tests/alfa.md").is_file(), "alfa foi feito");
    assert!(tmp.0.join("docs/gama.md").is_file() && !tmp.0.join("tests/gama.md").exists(), "gama (falhou) ficou");
    assert!(!m.tem("docs/alfa.md") && m.tem("docs/gama.md"));
    assert!(m.eventos().contains(&EventoArvore::Movidos(vec![orquestrador_core::ide::files::Moved { from: "docs/alfa.md".into(), to: "tests/alfa.md".into() }])));
    let ultimo = m.cx.update(|_, cx| orquestrador_ui::avisos::visiveis(cx)).last().map(|a| a.titulo.to_string()).unwrap();
    assert!(ultimo.ends_with("Antes disso, 1 item foi movido."), "{ultimo}");

    // Soltar na própria pasta é recusado sem mexer no disco.
    m.clicar("docs/gama.md");
    m.janela(|w, cx| w.drag_to(Mesa::id("docs/gama.md"), Mesa::id("docs/design-system.md"), cx));
    m.esperar();
    assert!(tmp.0.join("docs/gama.md").is_file());
}

#[gpui_kit::test]
fn barra_secao_ignorados_e_decoracoes(cx: &mut TestAppContext) {
    let tmp = Tmp::new("visual");
    exemplo(&tmp.0);
    std::fs::write(tmp.0.join(".gitignore"), "dist\n*.log\n").unwrap();
    let git = std::process::Command::new("git").args(["init", "-q"]).current_dir(&tmp.0).status();
    let mut m = montar(cx, &tmp.0);
    if git.is_ok_and(|s| s.success()) {
        let ignorados = m.ler(|a, _| {
            a.linhas().iter().filter(|l| l.entrada.as_ref().is_some_and(|e| e.ignored)).map(|l| l.chave.clone()).collect::<Vec<_>>()
        });
        assert_eq!(ignorados, ["dist", "debug.log"], "o .gitignore apaga dist e *.log");
    }
    // Recolher Tudo fecha as abertas.
    m.expandir(&["docs"]);
    assert!(m.tem("docs/apagado.md"));
    m.clicar_id("arvore-recolher");
    assert!(!m.tem("docs/apagado.md"));
    // Novo Arquivo da barra com a pasta docs selecionada: cria dentro dela.
    m.clicar("docs");
    m.clicar_id("arvore-novo-arquivo");
    m.digitar("pela-barra.md");
    m.tecla("enter");
    assert!(tmp.0.join("docs/pela-barra.md").is_file());
    assert!(m.tem("docs/pela-barra.md"));
    // A seção do projeto recolhe e reabre a árvore.
    m.clicar_id("arvore-secao");
    assert!(!m.tem("docs"));
    m.clicar_id("arvore-secao");
    assert!(m.tem("docs"));
}

#[gpui_kit::test]
fn pasta_grande_desenha_so_o_visivel_e_revelar_rola(cx: &mut TestAppContext) {
    let tmp = Tmp::new("grande");
    exemplo(&tmp.0);
    for i in 0..420 {
        arquivo(&tmp.0, &format!("grande/item-{i:03}.md"), "");
    }
    let mut m = montar(cx, &tmp.0);
    m.expandir(&["grande"]);
    assert!(m.tem("grande/item-000.md"));
    assert!(!m.tem("grande/item-419.md"), "o fim da pasta não é desenhado");
    let desenhadas = m.janela(|w, cx| {
        w.render_frame(cx);
        (0..420).filter(|i| w.try_find(Mesa::id(&format!("grande/item-{i:03}.md"))).is_some()).count()
    });
    assert!(desenhadas < 100, "só as linhas à vista ({desenhadas})");
    m.clicar("grande/item-000.md");
    m.tecla("end");
    m.tecla("up");
    m.tecla("up");
    m.tecla("up");
    m.tecla("up");
    m.tecla("up");
    m.tecla("up");
    m.tecla("up");
    assert!(m.tem("grande/item-419.md"), "subindo da raiz, a seta entra no fim da pasta");
    m.tecla("home");
    assert!(!m.tem("grande/item-419.md"));
    let a = m.arvore.clone();
    m.cx.update(|window, cx| a.update(cx, |a, cx| a.revelar("grande/item-419.md".into(), window, cx)));
    m.esperar();
    assert!(m.tem("grande/item-419.md"), "Revelar rola até o item");
    assert_eq!(m.selecao(), ["grande/item-419.md"]);
}

#[gpui_kit::test]
fn falha_na_raiz_mostra_o_motivo_e_a_rodada_seguinte_recupera(cx: &mut TestAppContext) {
    let tmp = Tmp::new("falha");
    let raiz = tmp.0.join("projeto");
    exemplo(&raiz);
    let mut m = montar(cx, &raiz);
    assert!(m.tem("README.md"));
    let fora = tmp.0.join("fora");
    std::fs::rename(&raiz, &fora).unwrap();
    // A janela de teste não tem foco: a rodada é a de 10 s.
    m.cx.executor().advance_clock(Duration::from_secs(11));
    m.esperar();
    assert!(!m.ler(|a, _| a.erro().to_string()).is_empty(), "o motivo aparece");
    assert!(m.tem("README.md"), "a árvore anterior continua na tela");
    std::fs::rename(&fora, &raiz).unwrap();
    arquivo(&raiz, "novo-depois-da-falha.txt", "");
    m.cx.executor().advance_clock(Duration::from_secs(11));
    m.esperar();
    assert!(m.tem("novo-depois-da-falha.txt"));
    assert!(m.ler(|a, _| a.erro().is_empty()), "o erro some");
}

#[gpui_kit::test]
fn menus_da_raiz_e_de_varios_itens(cx: &mut TestAppContext) {
    let tmp = Tmp::new("menus");
    exemplo(&tmp.0);
    let mut m = montar(cx, &tmp.0);
    m.janela(|w, cx| w.click_with_options("arvore-painel", gpui_kit::test::ClickOptions::new().with_button(gpui_kit::MouseButton::Right).with_offset(gpui_kit::point(px(10.), px(520.))), cx));
    m.esperar();
    let rotulos = m.ler(|a, _| a.rotulos_menu());
    assert_eq!(
        rotulos,
        [
            "Novo Arquivo...", "Nova Pasta...", "-", "Revelar no Explorador de Arquivos", "Pesquisar no Projeto", "-",
            "Adicionar Pasta ao Chat", "Colar", "-", "Copiar o Caminho", "Copiar Caminho Relativo"
        ]
    );
    m.tecla("escape");
    m.ler(|a, _| a.definir_historico(true));
    m.clicar("README.md");
    m.clicar_com("package.json", mods(true, false));
    m.direito("package.json");
    let (rotulos, habilitados) = m.ler(|a, _| (a.rotulos_menu(), a.habilitados_menu()));
    assert!(rotulos.contains(&"Excluir 2 itens...".to_string()));
    assert!(rotulos.contains(&"Copiar Caminhos Relativos".to_string()));
    assert!(!habilitados.contains(&"Renomear...".to_string()), "vários itens: Renomear desabilitado");
    assert!(!habilitados.contains(&"Histórico Git".to_string()), "ação de um só item desabilitada");
    m.tecla("escape");
    m.clicar("README.md");
    m.direito("README.md");
    m.menu("Copiar Caminho Relativo");
    let texto = m.cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(texto.as_deref(), Some("README.md"));
    assert_eq!(m.cx.update(|_, cx| orquestrador_ui::avisos::visiveis(cx)).last().map(|a| a.titulo.to_string()), Some("Caminho copiado".into()));
    m.direito("docs");
    m.menu("Localizar na Pasta...");
    assert!(m.eventos().contains(&EventoArvore::Pesquisar("docs".into())));
}
