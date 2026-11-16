//! Pré-leitura e o rascunho das abas de conflito.
//!
//! Árvore, pesquisa e Git chamam [`AreaEditor::pre_carregar`] ao passar o mouse ou apertar o botão sobre um arquivo:
//! a leitura e a gramática começam antes do clique, e abrir usa o resultado. A entrada vence
//! 5 s depois de pedida, ou 3 s depois de pronta; além disso há um teto de entradas.
use super::*;
use std::time::Instant;

/// Prazo de uma pré-leitura pendente e de uma pronta.
const PRAZO_PENDENTE: Duration = Duration::from_secs(5);
const PRAZO_PRONTA: Duration = Duration::from_secs(3);
/// Teto de entradas guardadas (a mais antiga sai primeiro).
pub(super) const LIMITE_PRE_LEITURAS: usize = 16;

type Espera = (String, String, Option<(u64, u64)>);

pub(super) struct PreLeitura {
    pedida: Instant,
    pronta: Option<(Instant, Result<files::Content, String>)>,
    /// Aberturas que chegaram antes do resultado: `doc_id`, caminho e o que revelar.
    esperando: Vec<Espera>,
}

impl PreLeitura {
    fn viva(&self, agora: Instant) -> bool {
        match &self.pronta {
            Some((quando, _)) => agora.duration_since(*quando) < PRAZO_PRONTA,
            None => agora.duration_since(self.pedida) < PRAZO_PENDENTE,
        }
    }
}

fn chave(caminho: &str) -> String {
    caminho.to_lowercase()
}

/// Extensão de mídia (`ide::midia::por_extensao`, sem abrir o arquivo) que não é de linguagem: o `.ts` e o `.txt`
/// também são de demuxer e seguem com pré-leitura.
fn eh_midia(caminho: &str) -> bool {
    orquestrador_core::ide::midia::por_extensao(std::path::Path::new(caminho)).is_some() && crate::linguagem::linguagem(caminho).is_none()
}

impl AreaEditor {
    /// Começa a ler `caminho` (relativo ao projeto ou absoluto) e a carregar a gramática dele, para abrir sem espera.
    /// Não faz nada para mídia, para arquivo já aberto e para o que já está sendo lido.
    pub fn pre_carregar(&mut self, caminho: &str, window: &mut Window, cx: &mut Context<Self>) {
        if caminho.is_empty() || eh_midia(caminho) {
            return;
        }
        let k = chave(caminho);
        if self.docs.values().any(|d| d.buffer.path.to_lowercase() == k) {
            return;
        }
        let agora = Instant::now();
        self.pre_leituras.retain(|_, e| e.viva(agora) || !e.esperando.is_empty());
        if self.pre_leituras.contains_key(&k) {
            return;
        }
        while self.pre_leituras.len() >= LIMITE_PRE_LEITURAS {
            let Some(velha) = self.pre_leituras.iter().filter(|(_, e)| e.esperando.is_empty()).min_by_key(|(_, e)| e.pedida).map(|(c, _)| c.clone())
            else {
                break;
            };
            self.pre_leituras.remove(&velha);
        }
        self.pre_leituras.insert(k.clone(), PreLeitura { pedida: agora, pronta: None, esperando: Vec::new() });
        let (raiz, rel) = (PathBuf::from(&self.projeto), caminho.to_string());
        let leitura = cx.background_spawn(async move {
            // A gramática carrega junto (o `preloadLanguage`): o primeiro realce não espera o `syntect`.
            if let Some(l) = crate::linguagem::linguagem(&rel) {
                let _ = crate::realce::sintaxe(&l, &rel);
            }
            ler_conteudo(&raiz, &rel)
        });
        cx.spawn_in(window, async move |this, cx| {
            let r = leitura.await;
            let _ = this.update_in(cx, |a, window, cx| a.pre_leitura_pronta(&k, r, window, cx));
        })
        .detach();
    }

    /// Há pré-leitura viva para o caminho (pendente ou pronta). Para a bancada e os testes.
    pub fn pre_carregado(&self, caminho: &str) -> bool {
        self.pre_leituras.get(&chave(caminho)).is_some_and(|e| e.viva(Instant::now()))
    }

    fn pre_leitura_pronta(&mut self, k: &str, r: Result<files::Content, String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.pre_leituras.get_mut(k) else { return };
        let esperando = std::mem::take(&mut e.esperando);
        if esperando.is_empty() {
            e.pronta = Some((Instant::now(), r));
            return;
        }
        // Quem abriu durante a leitura recebe o resultado; a entrada foi consumida.
        self.pre_leituras.remove(k);
        for (doc_id, caminho, revelar) in esperando {
            self.aplicar_leitura_inicial(&doc_id, &caminho, revelar, r.clone(), window, cx);
        }
    }

    /// Abrir com pré-leitura viva: usa o resultado pronto, ou espera o pendente. `false` sem pré-leitura.
    pub(super) fn usar_pre_leitura(
        &mut self, doc_id: &str, caminho: &str, revelar: Option<(u64, u64)>, window: &mut Window, cx: &mut Context<Self>,
    ) -> bool {
        let k = chave(caminho);
        let agora = Instant::now();
        let Some(e) = self.pre_leituras.get_mut(&k).filter(|e| e.viva(agora)) else {
            self.pre_leituras.remove(&k);
            return false;
        };
        if e.pronta.is_none() {
            e.esperando.push((doc_id.to_string(), caminho.to_string(), revelar));
            return true;
        }
        let Some(PreLeitura { pronta: Some((_, r)), .. }) = self.pre_leituras.remove(&k) else { return false };
        self.aplicar_leitura_inicial(doc_id, caminho, revelar, r, window, cx);
        true
    }

    // ---- Rascunho das abas de conflito ----

    /// O rascunho da aba de conflito (`EventoConflito::Rascunho` do `orquestrador-git`, campo a campo): sujo marca a
    /// aba, entra na recuperação e no "fechar com não salvos".
    pub fn definir_rascunho_conflito(&mut self, caminho: &str, origem: &str, texto: &str, revisao: &str, cx: &mut Context<Self>) {
        let novo = RascunhoConflito { path: caminho.into(), origin: origem.into(), draft: texto.into(), revision: revisao.into() };
        let k = chave_conflito(caminho);
        if self.conflitos.get(&k) == Some(&novo) {
            return;
        }
        self.conflitos.insert(k, novo);
        self.agendar_recuperacao(cx);
        cx.notify();
    }

    /// Quem relê a vista do conflito quando o editor descarta o rascunho dela.
    pub fn ao_descartar_conflito(&mut self, f: AoDescartarConflito) {
        self.ao_descartar_conflito = Some(f);
    }

    /// O rascunho guardado (da recuperação ou do último [`Self::definir_rascunho_conflito`]), para a fábrica de conteúdo
    /// devolver à `VisaoConflito` que cria.
    pub fn rascunho_conflito(&self, caminho: &str) -> Option<RascunhoConflito> {
        self.conflitos.get(&chave_conflito(caminho)).cloned()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn midia_fica_sem_pre_leitura() {
        orquestrador_core::ide::midia::teste::registrar();
        assert!(eh_midia("assets/logo.PNG"));
        assert!(eh_midia("v/a.webm"));
        assert!(!eh_midia("src/a.ts"));
        assert!(!eh_midia(".png"));
    }

    #[test]
    fn prazos_da_pre_leitura() {
        let agora = Instant::now();
        let mut e = PreLeitura { pedida: agora - Duration::from_secs(4), pronta: None, esperando: Vec::new() };
        assert!(e.viva(agora));
        e.pedida = agora - Duration::from_secs(6);
        assert!(!e.viva(agora), "pendente vence em 5 s");
        e.pronta = Some((agora - Duration::from_secs(2), Err(String::new())));
        assert!(e.viva(agora), "pronta vale 3 s depois de chegar");
        e.pronta = Some((agora - Duration::from_secs(4), Err(String::new())));
        assert!(!e.viva(agora));
    }
}
