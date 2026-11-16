//! Pré-leitura das abas de diff e de commit: a tela Git emite
//! `EventoGit::PreCarregar` ao apertar o botão num arquivo (e, nas listas de arquivos de commit, depois de 100 ms com o
//! ponteiro parado); a bancada chama [`PreLeiturasGit::pre_carregar`], e a [`crate::VisaoDiff`] criada com
//! [`crate::VisaoDiff::com_pre_leitura`] usa a leitura em curso ou pronta em vez de pedir outra. O realce de sintaxe
//! é calculado junto, e a aba já abre colorida.
//!
//! A entrada vence 5 s depois de pedida ou 3 s depois de pronta, e abrir a consome.
use crate::comparador::{Comparador, Realce};
use crate::comum::Alvo;
use crate::fonte::{Diff, Fonte, ReviewDiff};
use crate::{comparar, comparador};
use gpui_kit::{App, AppContext as _, Task};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Prazo de uma pré-leitura pendente e de uma pronta.
pub const PRAZO_PENDENTE: Duration = Duration::from_secs(5);
pub const PRAZO_PRONTA: Duration = Duration::from_secs(3);
/// Espera máxima da abertura pela leitura, contada do botão apertado (ou do pedido de abrir).
pub const PRAZO_ABERTURA: Duration = Duration::from_millis(500);
/// Permanência do ponteiro numa linha de arquivo de commit antes de pré-ler.
pub const PERMANENCIA_HOVER: Duration = Duration::from_millis(100);
/// Teto de entradas guardadas (a mais antiga sai primeiro).
pub const LIMITE: usize = 16;

/// O que a leitura trouxe.
#[derive(Clone, Debug)]
pub(crate) enum Lido {
    Indice(ReviewDiff),
    Commit(Diff),
}

/// Resultado de uma leitura: o diff e, quando dá para comparar, o realce do patch mostrado.
#[derive(Clone)]
pub(crate) struct Leitura {
    pub lido: Result<Lido, String>,
    pub realce: Option<(String, Realce, Realce)>,
}

/// A leitura entregue a quem abre: pronta, ou a chegar pelo canal.
pub(crate) enum Consumo {
    Pronta(Leitura),
    Pendente(async_channel::Receiver<Leitura>),
}

impl Consumo {
    pub async fn obter(self) -> Option<Leitura> {
        match self {
            Consumo::Pronta(l) => Some(l),
            Consumo::Pendente(rx) => rx.recv().await.ok(),
        }
    }
}

#[derive(Default)]
struct Vaga {
    pronta: Option<(Instant, Leitura)>,
    ouvintes: Vec<async_channel::Sender<Leitura>>,
}

struct Entrada {
    pedida: Instant,
    /// Botão apertado (não só hover): início do prazo de abertura.
    prazo_desde: Option<Instant>,
    vaga: Rc<RefCell<Vaga>>,
}

impl Entrada {
    fn viva(&self, agora: Instant) -> bool {
        match &self.vaga.borrow().pronta {
            Some((quando, _)) => agora.duration_since(*quando) < PRAZO_PRONTA,
            None => agora.duration_since(self.pedida) < PRAZO_PENDENTE,
        }
    }
}

/// Chave: lado, hash e caminho sem diferença de caixa. Conflito não tem pré-leitura.
fn chave(alvo: &Alvo) -> Option<String> {
    match alvo {
        Alvo::Diff { lado, caminho } => Some(format!("diff:{}:{}", if lado.staged() { "staged" } else { "unstaged" }, caminho.to_lowercase())),
        Alvo::Commit { hash, caminho } => Some(format!("commit:{}:{}", hash.to_lowercase(), caminho.to_lowercase())),
        Alvo::Conflito { .. } => None,
    }
}

/// Lê o alvo e calcula o realce do que a aba vai mostrar. Bloqueia: roda no executor de fundo.
pub(crate) fn ler(fonte: &Fonte, alvo: &Alvo) -> Leitura {
    let lido = match alvo {
        Alvo::Diff { lado, caminho } => fonte.review_diff(caminho, lado.staged()).map(Lido::Indice),
        Alvo::Commit { hash, caminho } => fonte.commit_diff(hash, caminho).map(Lido::Commit),
        Alvo::Conflito { .. } => Err("conflito não tem diff".into()),
    };
    let realce = lido.as_ref().ok().and_then(|l| {
        let (patch, blocos) = mostrar(l);
        let patch = patch?;
        let (antigo, novo) = realcar(&patch, blocos.as_deref(), alvo.caminho())?;
        Some((patch, antigo, novo))
    });
    Leitura { lido, realce }
}

/// O patch que a aba mostra (nenhum para binário, grande demais ou vazio) e os blocos do Git, como `VisaoDiff` os usa.
pub(crate) fn mostrar(l: &Lido) -> (Option<String>, Option<Vec<comparar::Intervalo>>) {
    match l {
        Lido::Indice(r) => {
            let ok = !r.display.binary && !r.display.too_large && !r.diff.too_large && !r.diff.binary && !r.diff.patch.is_empty();
            let blocos = (!r.untracked && !r.diff.patch.is_empty()).then(|| comparar::intervalos(&r.diff.patch));
            (ok.then(|| r.display.patch.clone()), blocos)
        }
        Lido::Commit(d) => ((!d.binary && !d.too_large && !d.patch.is_empty()).then(|| d.patch.clone()), None),
    }
}

fn realcar(patch: &str, blocos: Option<&[comparar::Intervalo]>, caminho: &str) -> Option<(Realce, Realce)> {
    if comparar::intervalos(patch).is_empty() {
        return None;
    }
    let cmp = comparar::comparar(patch, blocos);
    let unidades: Vec<_> = cmp.unificado.iter().map(|l| l.unidade).collect();
    let c = Comparador::new(patch, caminho, &unidades, cmp.unidades.len());
    let (antigo, novo) = c.textos();
    Some(comparador::Comparador::realcar(caminho, &c.modelo, &antigo, &novo))
}

/// Cache das pré-leituras de um projeto. Clonar compartilha o mesmo cache.
#[derive(Clone, Default)]
pub struct PreLeiturasGit {
    entradas: Rc<RefCell<HashMap<String, Entrada>>>,
}

impl PreLeiturasGit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Começa a ler o alvo (o `prefetchTarget`). `hover` só aquece: o prazo de abertura conta do botão apertado.
    /// Alvo já em leitura viva não é lido de novo.
    pub fn pre_carregar(&self, fonte: &Fonte, alvo: &Alvo, hover: bool, cx: &mut App) {
        let agora = cx.background_executor().now();
        let Some(k) = chave(alvo) else { return };
        let mut entradas = self.entradas.borrow_mut();
        entradas.retain(|_, e| e.viva(agora));
        if let Some(e) = entradas.get_mut(&k) {
            if !hover {
                e.prazo_desde = Some(agora);
            }
            return;
        }
        while entradas.len() >= LIMITE {
            let Some(velha) = entradas.iter().min_by_key(|(_, e)| e.pedida).map(|(c, _)| c.clone()) else { break };
            entradas.remove(&velha);
        }
        let vaga = Rc::new(RefCell::new(Vaga::default()));
        entradas.insert(k, Entrada { pedida: agora, prazo_desde: (!hover).then_some(agora), vaga: vaga.clone() });
        drop(entradas);
        let (fonte, alvo) = (fonte.clone(), alvo.clone());
        let tarefa = cx.background_spawn(async move { ler(&fonte, &alvo) });
        cx.spawn(async move |cx| {
            let l = tarefa.await;
            let agora = cx.background_executor().now();
            let mut v = vaga.borrow_mut();
            for tx in v.ouvintes.drain(..) {
                let _ = tx.try_send(l.clone());
            }
            v.pronta = Some((agora, l));
        })
        .detach();
    }

    /// Há leitura viva (pendente ou pronta) do alvo.
    pub fn pre_carregado(&self, alvo: &Alvo, cx: &App) -> bool {
        let agora = cx.background_executor().now();
        chave(alvo).is_some_and(|k| self.entradas.borrow().get(&k).is_some_and(|e| e.viva(agora)))
    }

    /// A leitura terminou (com diff ou erro).
    pub fn pronto(&self, alvo: &Alvo, cx: &App) -> bool {
        let agora = cx.background_executor().now();
        chave(alvo).is_some_and(|k| self.entradas.borrow().get(&k).is_some_and(|e| e.viva(agora) && e.vaga.borrow().pronta.is_some()))
    }

    /// Descarta a leitura do alvo (quando a aba já está aberta).
    pub fn descartar(&self, alvo: &Alvo) {
        if let Some(k) = chave(alvo) {
            self.entradas.borrow_mut().remove(&k);
        }
    }

    /// Espera para abrir a aba: começa a leitura se não houver e termina quando ela chega ou
    /// quando passam 500 ms do botão apertado (ou de agora, sem pré-leitura pelo botão). A bancada abre a aba depois.
    pub fn aguardar(&self, fonte: &Fonte, alvo: &Alvo, cx: &mut App) -> Task<()> {
        self.pre_carregar(fonte, alvo, true, cx);
        let agora = cx.background_executor().now();
        let (desde, vaga) = {
            let mut entradas = self.entradas.borrow_mut();
            let Some(e) = chave(alvo).and_then(|k| entradas.get_mut(&k)) else { return Task::ready(()) };
            (e.prazo_desde.take().unwrap_or(agora), e.vaga.clone())
        };
        if vaga.borrow().pronta.is_some() {
            return Task::ready(());
        }
        let (tx, rx) = async_channel::bounded(1);
        vaga.borrow_mut().ouvintes.push(tx);
        let resto = (desde + PRAZO_ABERTURA).saturating_duration_since(agora);
        let timer = cx.background_executor().timer(resto);
        cx.spawn(async move |_| {
            futures_lite::future::or(timer, async { let _ = rx.recv().await; }).await;
        })
    }

    /// Entrega a leitura viva do alvo a quem abre e a tira do cache.
    pub(crate) fn consumir(&self, alvo: &Alvo, cx: &App) -> Option<Consumo> {
        let e = self.entradas.borrow_mut().remove(&chave(alvo)?)?;
        if !e.viva(cx.background_executor().now()) {
            return None;
        }
        let mut v = e.vaga.borrow_mut();
        if let Some((_, l)) = &v.pronta {
            return Some(Consumo::Pronta(l.clone()));
        }
        let (tx, rx) = async_channel::bounded(1);
        v.ouvintes.push(tx);
        Some(Consumo::Pendente(rx))
    }
}
