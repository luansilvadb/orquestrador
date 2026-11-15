//! Quantos executores podem estar vivos ao mesmo tempo. Não há opção do usuário: o teto sai da CPU e da memória
//! física do computador, e do que está livre agora.
//! ponytail: orçamento aproximado, não a medida do peso de cada tarefa. Cada executor conta como 1 GiB, e o
//! sistema e o app ficam com 2 GiB antes disso. Recém-aberto ainda não alocou: durante a partida, conta como já alocado.
use std::collections::VecDeque;
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

struct Request {
    owner: String,
    id: String,
    start: Box<dyn FnOnce() + Send>,
}

struct Queue {
    live: usize,
    /// Quando cada admitido recente começou; vencida a `PARTIDA`, sai.
    starting: VecDeque<Instant>,
    waiting: VecDeque<Request>,
}

/// Admissão única para todos os projetos. A vaga fica ocupada até o Core liberar o executor.
pub struct Global {
    resources: Resources,
    queue: Arc<(Mutex<Queue>, Condvar)>,
}

impl Global {
    pub fn new(resources: Resources) -> Arc<Global> {
        let global = Arc::new(Global {
            resources,
            queue: Arc::new((Mutex::new(Queue { live: 0, starting: VecDeque::new(), waiting: VecDeque::new() }), Condvar::new())),
        });
        let weak = Arc::downgrade(&global);
        thread::spawn(move || schedule(weak));
        global
    }

    /// Põe um executor no fim da fila global; chamadas repetidas para o mesmo nó não duplicam a vaga.
    pub fn request(&self, owner: &str, id: &str, start: impl FnOnce() + Send + 'static) {
        let (queue, wake) = &*self.queue;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        if !queue.waiting.iter().any(|job| job.owner == owner && job.id == id) {
            queue.waiting.push_back(Request { owner: owner.into(), id: id.into(), start: Box::new(start) });
        }
        wake.notify_one();
    }

    pub fn cancel(&self, owner: &str, id: &str) {
        let (queue, wake) = &*self.queue;
        queue.lock().unwrap_or_else(|e| e.into_inner()).waiting.retain(|job| job.owner != owner || job.id != id);
        wake.notify_one();
    }

    pub fn cancel_owner(&self, owner: &str) {
        let (queue, wake) = &*self.queue;
        queue.lock().unwrap_or_else(|e| e.into_inner()).waiting.retain(|job| job.owner != owner);
        wake.notify_one();
    }

    pub fn release(&self) {
        let (queue, wake) = &*self.queue;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.live = queue.live.saturating_sub(1);
        wake.notify_one();
    }
}

fn schedule(weak: Weak<Global>) {
    loop {
        let Some(global) = weak.upgrade() else { return };
        let memory = (global.resources.memory)();
        let (queue, wake) = &*global.queue;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        prune(&mut queue.starting);
        // Fila vazia ou sem vaga: espera um pedido, uma liberação ou o próximo passe, que relê a memória e a partida.
        if queue.waiting.is_empty()
            || admission(global.resources.cpus, memory, queue.live, queue.starting.len()) <= queue.live
        {
            let (next, _) = wake.wait_timeout(queue, Duration::from_millis(200)).unwrap_or_else(|e| e.into_inner());
            queue = next;
            prune(&mut queue.starting);
        }
        let admission = admission(global.resources.cpus, memory, queue.live, queue.starting.len());
        let count = admission.saturating_sub(queue.live);
        let mut starts = Vec::new();
        for _ in 0..count {
            let Some(job) = queue.waiting.pop_front() else { break };
            starts.push(job.start);
            queue.live += 1;
            queue.starting.push_back(Instant::now());
        }
        drop(queue);
        for start in starts {
            let global = Arc::downgrade(&global);
            if catch_unwind(AssertUnwindSafe(start)).is_err() {
                if let Some(global) = global.upgrade() {
                    global.release();
                }
            }
        }
    }
}

fn prune(starting: &mut VecDeque<Instant>) {
    while starting.front().is_some_and(|t| t.elapsed() >= PARTIDA) {
        starting.pop_front();
    }
}

impl Resources {
    /// O computador de verdade.
    pub fn real() -> Resources {
        Resources { cpus: thread::available_parallelism().ok().map(|n| n.get()), memory: Arc::new(memory) }
    }
}

/// O teto de admissão de um passe: `min(CPU, RAM total, vivos + min(vagas pela RAM livre, vagas pelo commit))`, com
/// pelo menos um quando nada está vivo, para a fila não parar. `starting` são os admitidos ainda na partida (no
/// máximo `live`): descontam 1 GiB da RAM livre e 2 GiB do commit cada. Sem a memória, o teto cai para o
/// conservador. Quem chama passa `live` como os vivos agora: nunca manda parar
/// ninguém, só deixa de abrir novos enquanto `live` for igual ou maior.
pub fn admission(cpus: Option<usize>, memory: Option<Memory>, live: usize, starting: usize) -> usize {
    // Teto pela CPU: metade das lógicas, de 1 a 16.
    let cpu = cpus.map_or(FALLBACK, |n| (n / 2).clamp(MIN, MAX));
    let Some(memory) = memory else { return cpu.min(FALLBACK) };
    // Pela memória (a total dá o teto, a livre agora dá as vagas): o que sobra depois da reserva, um executor por 1 GiB.
    let slots = |b: u64, reserva: u64, por: u64| (b.saturating_sub(reserva) / por).min(MAX as u64) as usize;
    let starting = starting.min(live) as u64;
    let free = memory.available.saturating_sub(starting.saturating_mul(POR_EXECUTOR));
    let commit = memory.commit.saturating_sub(starting.saturating_mul(COMMIT_POR_EXECUTOR));
    let vagas = slots(free, RESERVA, POR_EXECUTOR).min(slots(commit, RESERVA_COMMIT, COMMIT_POR_EXECUTOR));
    let admitted = cpu.min(slots(memory.total, RESERVA, POR_EXECUTOR).max(MIN)).min(live.saturating_add(vagas));
    if live == 0 { admitted.max(MIN) } else { admitted }
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
    fn cpu_da_metade_entre_1_e_16() {
        for (cpus, want) in [(1, 1), (2, 1), (3, 1), (4, 2), (8, 4), (16, 8), (32, 16), (64, 16)] {
            assert_eq!(admission(Some(cpus), mem(64, 64), 0, 0), want, "{cpus} CPUs");
        }
    }

    #[test]
    fn ram_total_baixa_limita_e_ram_alta_deixa_a_cpu_mandar() {
        assert_eq!(admission(Some(32), mem(4, 4), 0, 0), 2, "4 GiB: sobram 2 GiB, dois executores");
        assert_eq!(admission(Some(32), mem(8, 8), 0, 0), 6, "8 GiB: sobram 6 GiB");
        assert_eq!(admission(Some(32), mem(64, 64), 0, 0), 16, "RAM alta: a CPU (16) manda");
        assert_eq!(admission(Some(8), mem(64, 64), 0, 0), 4, "CPU baixa: a CPU (4) manda");
    }

    #[test]
    fn ram_livre_baixa_com_vivos_nao_abre_e_sem_vivos_deixa_um() {
        assert_eq!(admission(Some(16), mem(64, 2), 3, 0), 3, "3 vivos e nenhuma vaga: nenhum novo");
        assert_eq!(admission(Some(16), mem(64, 3), 3, 0), 4, "uma vaga de 1 GiB");
        assert_eq!(admission(Some(16), mem(64, 2), 0, 0), 1, "sem vivos, um começa mesmo sem vaga");
        assert_eq!(admission(Some(16), mem(2, 1), 0, 0), 1, "RAM total que não comporta nenhum: ainda um");
        assert_eq!(admission(Some(16), mem(0, 0), 0, 0), 1, "zero de memória não zera o teto");
    }

    #[test]
    fn memoria_desconhecida_ou_falha_da_no_maximo_dois_e_pelo_menos_um() {
        assert_eq!(admission(None, mem(64, 64), 0, 0), 2, "CPU desconhecida");
        assert_eq!(admission(Some(32), None, 0, 0), 2, "memória desconhecida: no máximo 2");
        assert_eq!(admission(None, None, 0, 0), 2, "nada lido");
        assert_eq!(admission(Some(1), None, 0, 0), 1, "a CPU já dava 1");
        assert_eq!(admission(None, None, 5, 0), 2, "com 5 vivos, só diz que não cabe novo: quem chama não mata ninguém");
    }

    #[test]
    fn reducao_do_teto_so_segura_novos_e_retomada_abre_a_fila() {
        // 5 vivos e a RAM total caiu para 4 GiB: o teto passa a 2, abaixo dos vivos. Não há início novo.
        assert_eq!(admission(Some(16), mem(4, 4), 5, 0), 2);
        // A memória voltou: o mesmo vivo ganha vagas e o teto sobe.
        assert_eq!(admission(Some(16), mem(64, 6), 1, 0), 5);
    }

    #[test]
    fn valores_extremos_nao_estouram() {
        let cheio = Some(Memory { total: u64::MAX, available: u64::MAX, commit: u64::MAX });
        assert_eq!(admission(Some(64), cheio, usize::MAX, usize::MAX), 16);
    }

    fn com_commit(total_gib: u64, available_gib: u64, commit_gib: u64) -> Option<Memory> {
        Some(Memory { commit: commit_gib * GIB, ..mem(total_gib, available_gib).unwrap() })
    }

    #[test]
    fn rajada_nao_conta_a_mesma_ram_livre_de_novo() {
        // 16 GiB, 6 livres, 24 lógicas (CPU 12, RAM total 14): ninguém alocou ainda, a livre não muda.
        let m = com_commit(16, 6, 64);
        let primeiro = admission(Some(24), m, 0, 0);
        assert_eq!(primeiro, 4, "6 - 2 de reserva: quatro vagas");
        for _ in 0..5 {
            assert_eq!(admission(Some(24), m, primeiro, primeiro), primeiro, "na partida, a mesma RAM não abre mais");
        }
        // Vencida a partida, os quatro já alocaram ~0,55 GiB cada: 3,8 livres dão uma vaga.
        let depois = Some(Memory { available: GIB * 38 / 10, ..m.unwrap() });
        assert_eq!(admission(Some(24), depois, 4, 0), 5);
    }

    #[test]
    fn commit_curto_bloqueia_e_folgado_nao() {
        assert_eq!(admission(Some(32), com_commit(64, 64, 3), 3, 0), 3, "3 - 2 de reserva: nem 2 GiB para um executor");
        assert_eq!(admission(Some(32), com_commit(64, 64, 8), 3, 0), 6, "8 - 2: três de 2 GiB");
        assert_eq!(admission(Some(32), com_commit(64, 64, 8), 3, 1), 5, "um na partida desconta 2 GiB do commit");
        assert_eq!(admission(Some(32), mem(64, 64), 3, 0), 16, "commit folgado: só a RAM física");
        assert_eq!(admission(Some(32), com_commit(64, 64, 0), 0, 0), 1, "sem vivos, um começa mesmo sem commit");
    }

    #[test]
    fn executor_da_partida_que_morre_libera_a_reserva() {
        assert_eq!(admission(Some(32), com_commit(64, 4, 64), 2, 2), 2, "4 - 2 na partida - 2 de reserva: nenhuma");
        assert_eq!(admission(Some(32), com_commit(64, 4, 64), 1, 2), 2, "um morreu: só o vivo reserva, uma vaga");
        assert_eq!(admission(Some(32), com_commit(64, 4, 64), 0, 2), 2, "todos morreram: nenhum reserva");
    }

    #[test]
    fn zero_vivos_sempre_admite_um() {
        for m in [None, mem(0, 0), com_commit(1, 0, 0), com_commit(64, 2, 64)] {
            for starting in [0, 3, usize::MAX] {
                assert!(admission(Some(32), m, 0, starting) >= 1, "{m:?} {starting}");
            }
        }
    }

    #[test]
    fn fila_global_libera_a_proxima_vaga_em_ordem_de_chegada() {
        use std::sync::mpsc;

        let global = Global::new(Resources { cpus: Some(4), memory: Arc::new(|| mem(64, 64)) });
        let (tx, rx) = mpsc::channel();
        for (owner, id) in [("A", "a1"), ("A", "a2"), ("B", "b1")] {
            let tx = tx.clone();
            global.request(owner, id, move || tx.send(format!("{owner}-{id}")).unwrap());
        }
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "A-a1");
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "A-a2");
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "B espera enquanto as duas vagas estão ocupadas");
        global.release();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "B-b1");
        global.release();
        global.release();
    }

    #[test]
    fn fila_sem_vaga_espera_em_vez_de_girar() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let passes = Arc::new(AtomicUsize::new(0));
        let counted = passes.clone();
        let memory = Arc::new(move || {
            counted.fetch_add(1, Ordering::Relaxed);
            mem(64, 64)
        });
        let global = Global::new(Resources { cpus: Some(2), memory });
        global.request("A", "a1", || {});
        global.request("A", "a2", || {});
        thread::sleep(Duration::from_millis(100));
        let before = passes.load(Ordering::Relaxed);
        thread::sleep(Duration::from_millis(1000));
        let during = passes.load(Ordering::Relaxed) - before;
        assert!(during <= 10, "{during} passes em 1 s com a vaga ocupada");
        global.release();
        global.release();
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
