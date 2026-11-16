//! O git gutter na área do editor: a base é lida ao
//! abrir, depois de gravar (o `raw` muda), a cada consulta com a aba visível e logo depois de preparar; a digitação
//! recalcula os blocos na hora.
use super::*;
use crate::gutter_git::{self as gg, BaseGit, EstadoGit, FonteCore, FonteGutter};
use std::sync::Arc;

impl AreaEditor {
    /// Troca de onde vêm a base do Git e a gravação do índice (padrão: o core no repositório do projeto). Relê as bases.
    pub fn definir_fonte_git(&mut self, fonte: Arc<dyn FonteGutter>, cx: &mut Context<Self>) {
        self.fonte_git = Some(fonte);
        let ids: Vec<String> = self.docs.keys().cloned().collect();
        for d in ids {
            self.git_carregar(&d, cx);
        }
    }

    fn fonte_git_efetiva(&self) -> Option<Arc<dyn FonteGutter>> {
        match &self.fonte_git {
            Some(f) => Some(f.clone()),
            None if !self.projeto.is_empty() => Some(Arc::new(FonteCore { raiz: PathBuf::from(&self.projeto) })),
            None => None,
        }
    }

    /// Blocos do Git do documento (vazio sem base). Para a bancada e os testes.
    pub fn alteracoes_git(&self, doc_id: &str) -> Vec<gg::Bloco> {
        self.git.get(doc_id).map(|e| e.blocos.clone()).unwrap_or_default()
    }

    /// Índice do bloco aberto no documento.
    pub fn alteracao_aberta(&self, doc_id: &str) -> Option<usize> {
        self.git.get(doc_id).and_then(EstadoGit::aberto)
    }

    /// Lê a base de novo. Rascunho e arquivo fora do projeto ficam sem marcadores, sem consultar o Git.
    pub(super) fn git_carregar(&mut self, doc_id: &str, cx: &mut Context<Self>) {
        let Some(d) = self.docs.get(doc_id) else { return };
        let caminho = d.buffer.path.clone();
        let fonte = self.fonte_git_efetiva().filter(|_| !caminho.is_empty() && !eh_absoluto(&caminho));
        let Some(fonte) = fonte else {
            self.git_definir_base(doc_id, None, cx);
            return;
        };
        let rel = caminho.clone();
        // Erro de Git não interrompe a edição: só tira os marcadores.
        let tarefa = cx.background_spawn(async move { fonte.base(&rel).ok().flatten() });
        let doc_id = doc_id.to_string();
        cx.spawn(async move |this, cx| {
            let base = tarefa.await;
            let _ = this.update(cx, |a, cx| {
                if a.docs.get(&doc_id).is_some_and(|d| d.buffer.path == caminho) {
                    a.git_definir_base(&doc_id, base, cx);
                }
            });
        })
        .detach();
    }

    fn git_definir_base(&mut self, doc_id: &str, base: Option<BaseGit>, cx: &mut Context<Self>) {
        let Some(d) = self.docs.get(doc_id) else { return };
        let texto = d.texto(cx);
        if self.git.entry(doc_id.to_string()).or_default().definir_base(base, &texto) {
            self.git_aplicar(doc_id, cx);
        }
    }

    pub(super) fn git_ao_editar(&mut self, doc_id: &str, cx: &mut Context<Self>) {
        let (Some(d), Some(e)) = (self.docs.get(doc_id), self.git.get_mut(doc_id)) else { return };
        if e.base.is_none() {
            return;
        }
        e.editar(&d.texto(cx));
        self.git_aplicar(doc_id, cx);
    }

    /// Régua, fundo das linhas novas, altura reservada do bloco aberto e o menu de navegação.
    fn git_aplicar(&mut self, doc_id: &str, cx: &mut Context<Self>) {
        let (Some(d), Some(e)) = (self.docs.get(doc_id), self.git.get(doc_id)) else { return };
        let texto = d.texto(cx);
        d.trechos.set(gg::linhas_novas(e, &texto), cx);
        let linhas = d.editor.read(cx).text().lines_len();
        let reservado = e.bloco_reservado(linhas, tema::LINHA_CODIGO);
        d.editor.update(cx, |s, cx| s.set_blocks(reservado.map(|(l, h)| vec![(l, px(h))]).unwrap_or_default(), cx));
        if e.blocos.is_empty() {
            self.com_alteracoes.remove(doc_id);
        } else {
            self.com_alteracoes.insert(doc_id.to_string());
        }
        cx.notify();
    }

    /// Vai para a alteração seguinte (`1`) ou anterior (`-1`), com o bloco aberto, dando a volta (Alt+F5).
    pub(super) fn git_ir(&mut self, doc_id: &str, passo: i32, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(d), Some(e)) = (self.docs.get(doc_id), self.git.get_mut(doc_id)) else { return };
        let Some(i) = e.vizinho(d.cursor(cx).0 as usize, passo) else { return };
        e.abrir(Some(i));
        let linha = e.blocos[i].de as u64;
        d.revelar(linha, 1, window, cx);
        self.git_aplicar(doc_id, cx);
        cx.emit(EventoEditor::IrParaAlteracao { doc_id: doc_id.to_string(), passo });
    }

    pub(super) fn git_alternar(&mut self, doc_id: &str, bloco: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.git.get_mut(doc_id) else { return };
        e.alternar(bloco);
        // O foco vai para o texto, para o Esc fechar.
        if let Some(d) = self.docs.get(doc_id) {
            d.editor.update(cx, |s, cx| s.focus(window, cx));
        }
        self.git_aplicar(doc_id, cx);
    }

    /// Fecha o bloco aberto; `false` se não havia nenhum.
    pub(super) fn git_fechar(&mut self, doc_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(e) = self.git.get_mut(doc_id).filter(|e| e.aberto().is_some()) else { return false };
        e.abrir(None);
        self.git_aplicar(doc_id, cx);
        true
    }

    /// Troca as linhas do bloco aberto pelas do HEAD como edição normal (Desfazer, buffer sujo; não grava nada).
    fn git_restaurar(&mut self, doc_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(d), Some(e)) = (self.docs.get(doc_id), self.git.get(doc_id)) else { return };
        if d.buffer.read_only {
            return;
        }
        let Some((faixa, novo)) = e.aberto().and_then(|i| e.restauracao(i)) else { return };
        d.editor.update(cx, |s, cx| {
            s.set_selected_range(faixa.clone(), cx);
            s.replace(novo, window, cx);
            s.focus(window, cx);
        });
    }

    /// Prepara (ou tira do preparo) o bloco aberto com o texto do editor, mesmo não salvo, e relê a base na hora.
    fn git_preparar(&mut self, doc_id: &str, cx: &mut Context<Self>) {
        let Some(fonte) = self.fonte_git_efetiva() else { return };
        let Some(d) = self.docs.get(doc_id) else { return };
        let caminho = d.buffer.path.clone();
        let Some(e) = self.git.get_mut(doc_id).filter(|e| !e.ocupado) else { return };
        let Some((novo, esperado)) = e.aberto().and_then(|i| e.preparo(i)) else { return };
        e.ocupado = true;
        cx.notify();
        let tarefa = cx.background_spawn(async move { fonte.gravar_indice(&caminho, &novo, &esperado) });
        let doc_id = doc_id.to_string();
        cx.spawn(async move |this, cx| {
            let r = tarefa.await;
            let _ = this.update(cx, |a, cx| {
                if let Some(e) = a.git.get_mut(&doc_id) {
                    e.ocupado = false;
                }
                match r {
                    Ok(()) => cx.emit(EventoEditor::AtualizarGit),
                    Err(m) => avisos::mostrar(Aviso::erro(m, 7000), cx),
                }
                a.git_carregar(&doc_id, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Camada dos marcadores e do bloco aberto por cima do editor do documento.
    pub(super) fn git_camada(&self, doc_id: &str, editor: &Entity<EditorState>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let e = self.git.get(doc_id).filter(|e| !e.blocos.is_empty())?;
        let d = self.docs.get(doc_id)?;
        let ent = cx.entity().downgrade();
        let id = doc_id.to_string();
        let acao = |f: fn(&mut AreaEditor, &str, &mut Window, &mut Context<AreaEditor>)| -> crate::gutter_git::Acao {
            let (ent, id) = (ent.clone(), id.clone());
            Rc::new(move |window: &mut Window, cx: &mut App| {
                let _ = ent.update(cx, |a, cx| f(a, &id, window, cx));
            })
        };
        let acoes = gg::Acoes {
            preparar: acao(|a, id, _, cx| a.git_preparar(id, cx)),
            restaurar: acao(|a, id, w, cx| a.git_restaurar(id, w, cx)),
            fechar: acao(|a, id, w, cx| {
                a.git_fechar(id, cx);
                if let Some(d) = a.docs.get(id) {
                    d.editor.update(cx, |s, cx| s.focus(w, cx));
                }
            }),
            ir: {
                let (ent, id) = (ent.clone(), id.clone());
                Rc::new(move |passo, window: &mut Window, cx: &mut App| {
                    let _ = ent.update(cx, |a, cx| a.git_ir(&id, passo, window, cx));
                })
            },
            alternar: {
                let (ent, id) = (ent.clone(), id.clone());
                Rc::new(move |bloco, window: &mut Window, cx: &mut App| {
                    let _ = ent.update(cx, |a, cx| a.git_alternar(&id, bloco, window, cx));
                })
            },
        };
        let s = editor.read(cx);
        let aberto = e.aberto().map(|i| {
            let b = &e.blocos[i];
            let linha = if b.tipo == gg::Tipo::Apagado && b.fim { s.text().lines_len() } else { b.de - 1 };
            let largura = (s.input_bounds().size.width - s.gutter_width().unwrap_or_default() - px(gg::SOB_MINIMAPA)).max(px(0.));
            let info = gg::InfoBloco {
                antigas: e.antigas(i),
                preparado: b.preparado,
                em: i + 1,
                total: e.blocos.len(),
                pode_preparar: e.pode_preparar(),
                somente_leitura: d.buffer.read_only,
                com_indice: true,
                ocupado: e.ocupado,
                caminho: d.buffer.path.clone(),
            };
            (linha, gg::widget(info, &acoes, largura))
        });
        Some(gg::camada(editor.clone(), e.blocos.clone(), aberto, acoes).into_any_element())
    }
}
