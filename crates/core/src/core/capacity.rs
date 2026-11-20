//! Admissão de executores com teto de CPU por lote e orçamento global de memória.
//! ponytail: orçamento aproximado, não a medida do peso de cada tarefa. Cada executor conta como 1 GiB, e o
//! sistema e o app ficam com 2 GiB antes disso. Recém-aberto ainda não alocou: durante a partida, conta como já alocado.
use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

const GIB: u64 = 1 << 30;
/// Tirada da RAM antes de contar executores: sistema, outros programas e o próprio app.
const RESERVA: u64 = 2 * GIB;
/// Orçamento de memória por executor.
const POR_EXECUTOR: u64 = GIB;
/// Commit (memória virtual) reservado e pedido por executor.
const RESERVA_COMMIT: u64 = 2 * GIB;
const COMMIT_POR_EXECUTOR: u64 = 2 * GIB;
/// Tempo que um executor recém-aberto leva para alocar; até lá ele reserva o orçamento inteiro.
const PARTIDA: Duration = Duration::from_secs(15);
const MIN: usize = 1;
const MAX: usize = 16;
/// Teto quando CPU ou memória não puderam ser lidas: conservador, e o app abre do mesmo jeito.
const FALLBACK: usize = 2;

/// Memória, em bytes. `available` é a física livre agora; `commit`, o commit disponível.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
    pub commit: u64,
}

/// De onde vêm CPU e memória. `cpus` é lido uma vez, ao criar o Core; `memory` é lido a cada passe da fila.
#[derive(Clone)]
pub struct Resources {
    pub cpus: Option<usize>,
    pub memory: Arc<dyn Fn() -> Option<Memory> + Send + Sync>,
}

struct Executor {
    owner: String,
    id: String,
    batch: String,
}

impl Executor {
    fn matches(&self, owner: &str, id: &str) -> bool {
        self.owner == owner && self.id == id
    }
}

struct Request {
    executor: Executor,
    start: Box<dyn FnOnce(Grant) + Send>,
}

pub struct Grant {
    start: StartMarker,
}

#[derive(Clone)]
pub(super) struct StartMarker {
    global: Weak<Global>,
    executor: Arc<Executor>,
}

impl Grant {
    pub(super) fn start_marker(&self) -> StartMarker {
        self.start.clone()
    }
}

impl StartMarker {
    pub(super) fn started(self) {
        if let Some(global) = self.global.upgrade() {
            let mut queue = global.queue.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, started)) = queue.active.iter_mut().find(|(executor, _)| Arc::ptr_eq(executor, &self.executor)) {
                *started = Some(Instant::now());
            }
        }
    }
}

impl Drop for Grant {
    fn drop(&mut self) {
        if let Some(global) = self.start.global.upgrade() {
            let (queue, wake) = &global.queue;
            queue.lock().unwrap_or_else(|e| e.into_inner()).active
                .retain(|(executor, _)| !Arc::ptr_eq(executor, &self.start.executor));
            wake.notify_one();
        }
    }
}

struct Queue {
    // Sem horário até o processo abrir: a concessão já reserva memória enquanto espera o start.
    active: Vec<(Arc<Executor>, Option<Instant>)>,
    waiting: VecDeque<Request>,
}

/// Admissão única para todos os projetos. A vaga fica ocupada enquanto a concessão existir.
pub struct Global {
    resources: Resources,
    queue: (Mutex<Queue>, Condvar),
}

impl Global {
    pub fn new(resources: Resources) -> Arc<Global> {
        let global = Arc::new(Global {
            resources,
            queue: (Mutex::new(Queue { active: Vec::new(), waiting: VecDeque::new() }), Condvar::new()),
        });
        let weak = Arc::downgrade(&global);
        thread::spawn(move || schedule(weak));
        global
    }

    /// Põe um executor no fim da fila global; chamadas repetidas para o mesmo nó não duplicam a vaga.
    pub fn request(&self, owner: &str, batch: &str, id: &str, start: impl FnOnce(Grant) + Send + 'static) {
        let (queue, wake) = &self.queue;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.waiting.iter().any(|job| job.executor.matches(owner, id))
            || queue.active.iter().any(|(executor, _)| executor.matches(owner, id))
        {
            return;
        }
        let executor = Executor { owner: owner.into(), id: id.into(), batch: batch.into() };
        queue.waiting.push_back(Request { executor, start: Box::new(start) });
        wake.notify_one();
    }

    pub fn cancel(&self, owner: &str, id: &str) {
        self.cancel_matching(|executor| executor.matches(owner, id));
    }

    pub fn cancel_owner(&self, owner: &str) {
        self.cancel_matching(|executor| executor.owner == owner);
    }

    fn cancel_matching(&self, matches: impl Fn(&Executor) -> bool) {
        let (queue, wake) = &self.queue;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.waiting.retain(|job| !matches(&job.executor));
        wake.notify_one();
    }
}

fn batch_limit(cpus: Option<usize>) -> usize {
    cpus.map_or(FALLBACK, |n| n.saturating_sub(2).clamp(MIN, MAX))
}

fn schedule(weak: Weak<Global>) {
    loop {
        let Some(global) = weak.upgrade() else { return };
        let memory = (global.resources.memory)();
        let cpus = global.resources.cpus;
        let limit = batch_limit(cpus);
        let (queue, wake) = &global.queue;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        let live = queue.active.len();
        let starting = || {
            queue.active.iter().filter(|(_, time)| time.is_none_or(|t| t.elapsed() < PARTIDA)).count()
        };
        let index = if !queue.waiting.is_empty() && live < admission(cpus, memory, live, starting) {
            let mut counts = HashMap::new();
            for (executor, _) in &queue.active {
                *counts.entry((executor.owner.as_str(), executor.batch.as_str())).or_insert(0) += 1;
            }
            queue.waiting.iter().position(|job| {
                let key = (job.executor.owner.as_str(), job.executor.batch.as_str());
                counts.get(&key).copied().unwrap_or(0) < limit
            })
        } else {
            None
        };
        if let Some(index) = index {
            let job = queue.waiting.remove(index).unwrap();
            let executor = Arc::new(job.executor);
            queue.active.push((executor.clone(), None));
            drop(queue);
            let grant = Grant { start: StartMarker { global: weak.clone(), executor } };
            let _ = catch_unwind(AssertUnwindSafe(|| (job.start)(grant)));
            continue;
        }
        let _ = wake.wait_timeout(queue, Duration::from_millis(200)).unwrap_or_else(|e| e.into_inner());
    }
}

impl Resources {
    /// O computador de verdade.
    pub fn real() -> Resources {
        Resources { cpus: thread::available_parallelism().ok().map(|n| n.get()), memory: Arc::new(memory) }
    }
}

/// Teto global pela memória, com piso de uma vaga. Cada partida reserva 1 GiB de RAM e 2 GiB de commit.
fn admission(cpus: Option<usize>, memory: Option<Memory>, live: usize, starting: impl FnOnce() -> usize) -> usize {
    let Some(memory) = memory else { return batch_limit(cpus).min(FALLBACK) };
    let slots = |b: u64, reserva: u64, por: u64| usize::try_from(b.saturating_sub(reserva) / por).unwrap_or(usize::MAX);
    let starting = starting().min(live) as u64;
    let free = memory.available.saturating_sub(starting.saturating_mul(POR_EXECUTOR));
    let commit = memory.commit.saturating_sub(starting.saturating_mul(COMMIT_POR_EXECUTOR));
    let vagas = slots(free, RESERVA, POR_EXECUTOR).min(slots(commit, RESERVA_COMMIT, COMMIT_POR_EXECUTOR));
    let ceiling = if cpus.is_none() { FALLBACK } else { usize::MAX };
    slots(memory.total, RESERVA, POR_EXECUTOR).min(live.saturating_add(vagas)).min(ceiling).max(MIN)
}

fn memory() -> Option<Memory> {
    crate::platform::memoria().map(|(total, available, commit)| Memory { total, available, commit })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(total_gib: u64, available_gib: u64) -> Option<Memory> {
        Some(Memory { total: total_gib * GIB, available: available_gib * GIB, commit: u64::MAX })
    }

    #[test]
    fn cpu_reserva_duas_logicas_e_limita_cada_lote() {
        for (cpus, want) in [(0, 1), (1, 1), (2, 1), (3, 1), (4, 2), (8, 6), (16, 14), (17, 15), (18, 16), (19, 16), (usize::MAX, 16)] {
            assert_eq!(batch_limit(Some(cpus)), want, "{cpus} CPUs");
        }
        assert_eq!(batch_limit(None), 2);
    }

    #[test]
    fn ram_total_limita_a_admissao_global() {
        assert_eq!(admission(Some(32), mem(4, 4), 0, || 0), 2);
        assert_eq!(admission(Some(32), mem(8, 8), 0, || 0), 6);
        assert_eq!(admission(Some(32), mem(64, 64), 0, || 0), 62);
    }

    #[test]
    fn ram_livre_baixa_com_vivos_nao_abre_e_sem_vivos_deixa_um() {
        assert_eq!(admission(Some(32), mem(64, 2), 3, || 0), 3, "3 vivos e nenhuma vaga: nenhum novo");
        assert_eq!(admission(Some(32), mem(64, 3), 3, || 0), 4, "uma vaga de 1 GiB");
        assert_eq!(admission(Some(32), mem(64, 2), 0, || 0), 1, "sem vivos, um começa mesmo sem vaga");
        assert_eq!(admission(Some(32), mem(2, 1), 0, || 0), 1, "RAM total que não comporta nenhum: ainda um");
        assert_eq!(admission(Some(32), mem(0, 0), 0, || 0), 1, "zero de memória não zera o teto");
    }

    #[test]
    fn memoria_desconhecida_ou_falha_da_no_maximo_dois_e_pelo_menos_um() {
        assert_eq!(admission(None, mem(64, 64), 0, || 0), 2);
        assert_eq!(admission(Some(32), None, 0, || unreachable!()), 2);
        assert_eq!(admission(Some(3), None, 0, || unreachable!()), 1);
        assert_eq!(admission(None, None, 5, || unreachable!()), 2);
    }

    #[test]
    fn reducao_do_teto_so_segura_novos_e_retomada_abre_a_fila() {
        // 5 vivos e a RAM total caiu para 4 GiB: o teto passa a 2, abaixo dos vivos. Não há início novo.
        assert_eq!(admission(Some(32), mem(4, 4), 5, || 0), 2);
        // A memória voltou: o mesmo vivo ganha vagas e o teto sobe.
        assert_eq!(admission(Some(32), mem(64, 6), 1, || 0), 5);
    }

    #[test]
    fn valores_extremos_nao_estouram() {
        let cheio = Some(Memory { total: u64::MAX, available: u64::MAX, commit: u64::MAX });
        assert_eq!(admission(Some(32), cheio, usize::MAX, || usize::MAX), usize::try_from((u64::MAX - RESERVA) / GIB).unwrap_or(usize::MAX));
    }

    fn com_commit(total_gib: u64, available_gib: u64, commit_gib: u64) -> Option<Memory> {
        Some(Memory { commit: commit_gib * GIB, ..mem(total_gib, available_gib).unwrap() })
    }

    #[test]
    fn rajada_nao_conta_a_mesma_ram_livre_de_novo() {
        // 16 GiB, 6 livres, teto pela RAM total de 14: ninguém alocou ainda, a livre não muda.
        let m = com_commit(16, 6, 64);
        let primeiro = admission(Some(32), m, 0, || 0);
        assert_eq!(primeiro, 4, "6 - 2 de reserva: quatro vagas");
        for _ in 0..5 {
            assert_eq!(admission(Some(32), m, primeiro, || primeiro), primeiro, "na partida, a mesma RAM não abre mais");
        }
        // Vencida a partida, os quatro já alocaram ~0,55 GiB cada: 3,8 livres dão uma vaga.
        let depois = Some(Memory { available: GIB * 38 / 10, ..m.unwrap() });
        assert_eq!(admission(Some(32), depois, 4, || 0), 5);
    }

    #[test]
    fn commit_curto_bloqueia_e_folgado_nao() {
        assert_eq!(admission(Some(32), com_commit(64, 64, 3), 3, || 0), 3, "3 - 2 de reserva: nem 2 GiB para um executor");
        assert_eq!(admission(Some(32), com_commit(64, 64, 8), 3, || 0), 6, "8 - 2: três de 2 GiB");
        assert_eq!(admission(Some(32), com_commit(64, 64, 8), 3, || 1), 5, "um na partida desconta 2 GiB do commit");
        assert_eq!(admission(Some(32), mem(64, 64), 3, || 0), 62, "commit folgado: só a RAM física");
        assert_eq!(admission(Some(32), com_commit(64, 64, 0), 0, || 0), 1, "sem vivos, um começa mesmo sem commit");
    }

    #[test]
    fn executor_da_partida_que_morre_libera_a_reserva() {
        assert_eq!(admission(Some(32), com_commit(64, 4, 64), 2, || 2), 2, "4 - 2 na partida - 2 de reserva: nenhuma");
        assert_eq!(admission(Some(32), com_commit(64, 4, 64), 1, || 2), 2, "um morreu: só o vivo reserva, uma vaga");
        assert_eq!(admission(Some(32), com_commit(64, 4, 64), 0, || 2), 2, "todos morreram: nenhum reserva");
    }

    #[test]
    fn zero_vivos_sempre_admite_um() {
        for m in [None, mem(0, 0), com_commit(1, 0, 0), com_commit(64, 2, 64)] {
            for starting in [0, 3, usize::MAX] {
                assert!(admission(Some(32), m, 0, || starting) >= 1, "{m:?} {starting}");
            }
        }
    }

    #[test]
    fn lote_saturado_espera_e_a_memoria_e_compartilhada() {
        use std::sync::mpsc;

        let global = Global::new(Resources { cpus: Some(2), memory: Arc::new(|| mem(64, 4)) });
        let (tx, rx) = mpsc::channel();
        for (owner, batch, id) in [("A", "lote", "1"), ("A", "lote", "2"), ("B", "lote", "1"), ("A", "outro", "3")] {
            let tx = tx.clone();
            global.request(owner, batch, id, move |grant| tx.send((owner, id, grant)).unwrap());
        }
        let next = || rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (owner, id, a1) = next();
        assert_eq!((owner, id), ("A", "1"));
        let (owner, id, b1) = next();
        assert_eq!((owner, id), ("B", "1"));
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());

        drop(b1);
        let (owner, id, a3) = next();
        assert_eq!((owner, id), ("A", "3"));
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());

        global.cancel("A", "1");
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(a1);
        let (owner, id, _a2) = next();
        assert_eq!((owner, id), ("A", "2"));
        drop(a3);
    }

    #[test]
    fn fallback_limita_executores_de_lotes_distintos() {
        use std::sync::mpsc;

        for (cpus, memory, limit) in [(None, mem(64, 64), 2), (Some(3), None, 1)] {
            let global = Global::new(Resources { cpus, memory: Arc::new(move || memory) });
            let (tx, rx) = mpsc::channel();
            for batch in ["1", "2", "3"] {
                let tx = tx.clone();
                global.request("A", batch, batch, move |grant| tx.send(grant).unwrap());
            }
            let next = || rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let mut active: Vec<_> = (0..limit).map(|_| next()).collect();
            assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "{cpus:?} CPUs");
            drop(active.pop());
            let _grant = next();
        }
    }

    #[test]
    fn cancelar_dono_remove_pendencias_e_preserva_concessao_ativa() {
        use std::sync::mpsc;

        let global = Global::new(Resources { cpus: Some(2), memory: Arc::new(|| mem(64, 64)) });
        let (tx, rx) = mpsc::channel();
        let request = |id| {
            let tx = tx.clone();
            global.request("A", "lote", id, move |grant| tx.send(grant).unwrap());
        };
        let next = || rx.recv_timeout(Duration::from_secs(2)).unwrap();
        request("1");
        let active = next();
        request("2");
        global.cancel_owner("A");
        request("1");
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(active);
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        request("1");
        let _active = next();
    }

    #[test]
    fn rele_memoria_entre_partidas_de_lotes_distintos() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::mpsc;

        let available = Arc::new(AtomicU64::new(2));
        let memory = available.clone();
        let global = Global::new(Resources {
            cpus: Some(2), memory: Arc::new(move || mem(64, memory.load(Ordering::Relaxed))),
        });
        let (tx, rx) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let first = tx.clone();
        global.request("A", "primeiro", "1", move |grant| {
            first.send(grant).unwrap();
            wait.recv().unwrap();
        });
        let next = || rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let first = next();
        let second = tx.clone();
        let memory = available.clone();
        global.request("A", "segundo", "2", move |grant| {
            memory.store(2, Ordering::Relaxed);
            second.send(grant).unwrap();
        });
        global.request("A", "terceiro", "3", move |grant| tx.send(grant).unwrap());
        available.store(6, Ordering::Relaxed);
        release.send(()).unwrap();
        let _second = next();
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        available.store(5, Ordering::Relaxed);
        let _third = next();
        drop(first);
    }

    #[test]
    fn fila_sem_vaga_espera_em_vez_de_girar() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::mpsc;

        let passes = Arc::new(AtomicUsize::new(0));
        let counted = passes.clone();
        let memory = Arc::new(move || {
            counted.fetch_add(1, Ordering::Relaxed);
            mem(64, 64)
        });
        let global = Global::new(Resources { cpus: Some(2), memory });
        let (tx, rx) = mpsc::channel();
        global.request("A", "lote", "a1", move |grant| tx.send(grant).unwrap());
        let _grant = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        global.request("A", "lote", "a2", |_| {});
        let before = passes.load(Ordering::Relaxed);
        thread::sleep(Duration::from_millis(1000));
        let during = passes.load(Ordering::Relaxed) - before;
        assert!(during <= 10, "{during} passes em 1 s com a vaga ocupada");
    }

    #[cfg(windows)]
    #[test]
    fn leitura_real_da_memoria_e_das_cpus() {
        let real = Resources::real();
        assert!(real.cpus.is_some_and(|n| n >= 1), "available_parallelism devolve pelo menos 1");
        let m = (real.memory)().expect("o Windows responde GlobalMemoryStatusEx");
        assert!(m.total > 0 && m.available <= m.total && m.commit > 0, "{m:?}");
    }
}
