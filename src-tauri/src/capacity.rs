//! Quantos executores podem estar vivos ao mesmo tempo. Não há opção do usuário: o teto sai da CPU e da memória
//! física do computador, e do que está livre agora.
//! ponytail: orçamento aproximado, não a medida do peso de cada tarefa. Cada executor conta como 1 GiB, e o
//! sistema e o app ficam com 2 GiB antes disso.
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::Duration;

const GIB: u64 = 1 << 30;
/// Tirada da RAM antes de contar executores: sistema, outros programas e o próprio app.
const RESERVA: u64 = 2 * GIB;
/// Orçamento de memória por executor.
const POR_EXECUTOR: u64 = GIB;
const MIN: usize = 1;
const MAX: usize = 16;
/// Teto quando CPU ou memória não puderam ser lidas: conservador, e o app abre do mesmo jeito.
const FALLBACK: usize = 2;

/// Memória física, em bytes. `available` é a parte livre agora, não a memória virtual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
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
            queue: Arc::new((Mutex::new(Queue { live: 0, waiting: VecDeque::new() }), Condvar::new())),
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
        // Fila vazia ou sem vaga: espera um pedido, uma liberação ou o próximo passe, que relê a memória.
        if queue.waiting.is_empty() || admission(global.resources.cpus, memory, queue.live) <= queue.live {
            let (next, _) = wake.wait_timeout(queue, Duration::from_millis(200)).unwrap_or_else(|e| e.into_inner());
            queue = next;
        }
        let admission = admission(global.resources.cpus, memory, queue.live);
        let count = admission.saturating_sub(queue.live);
        let mut starts = Vec::new();
        for _ in 0..count {
            let Some(job) = queue.waiting.pop_front() else { break };
            starts.push(job.start);
            queue.live += 1;
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

impl Resources {
    /// O computador de verdade.
    pub fn real() -> Resources {
        Resources { cpus: thread::available_parallelism().ok().map(|n| n.get()), memory: Arc::new(memory) }
    }
}

/// O teto de admissão de um passe: `min(CPU, RAM total, vivos + vagas pela RAM livre)`, com pelo menos um quando
/// nada está vivo, para a fila não parar. Sem a memória, o teto cai para o conservador. Quem chama passa `live`
/// como os vivos agora: nunca manda parar ninguém, só deixa de abrir novos enquanto `live` for igual ou maior.
pub fn admission(cpus: Option<usize>, memory: Option<Memory>, live: usize) -> usize {
    // Teto pela CPU: metade das lógicas, de 1 a 16.
    let cpu = cpus.map_or(FALLBACK, |n| (n / 2).clamp(MIN, MAX));
    let Some(memory) = memory else { return cpu.min(FALLBACK) };
    // Pela memória (a total dá o teto, a livre agora dá as vagas): o que sobra depois da reserva, um executor por 1 GiB.
    let slots = |b: u64| (b.saturating_sub(RESERVA) / POR_EXECUTOR).min(MAX as u64) as usize;
    let admitted = cpu.min(slots(memory.total).max(MIN)).min(live.saturating_add(slots(memory.available)));
    if live == 0 { admitted.max(MIN) } else { admitted }
}

fn memory() -> Option<Memory> {
    crate::platform::memoria().map(|(total, available)| Memory { total, available })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(total_gib: u64, available_gib: u64) -> Option<Memory> {
        Some(Memory { total: total_gib * GIB, available: available_gib * GIB })
    }

    #[test]
    fn cpu_da_metade_entre_1_e_16() {
        for (cpus, want) in [(1, 1), (2, 1), (3, 1), (4, 2), (8, 4), (16, 8), (32, 16), (64, 16)] {
            assert_eq!(admission(Some(cpus), mem(64, 64), 0), want, "{cpus} CPUs");
        }
    }

    #[test]
    fn ram_total_baixa_limita_e_ram_alta_deixa_a_cpu_mandar() {
        assert_eq!(admission(Some(32), mem(4, 4), 0), 2, "4 GiB: sobram 2 GiB, dois executores");
        assert_eq!(admission(Some(32), mem(8, 8), 0), 6, "8 GiB: sobram 6 GiB");
        assert_eq!(admission(Some(32), mem(64, 64), 0), 16, "RAM alta: a CPU (16) manda");
        assert_eq!(admission(Some(8), mem(64, 64), 0), 4, "CPU baixa: a CPU (4) manda");
    }

    #[test]
    fn ram_livre_baixa_com_vivos_nao_abre_e_sem_vivos_deixa_um() {
        assert_eq!(admission(Some(16), mem(64, 2), 3), 3, "3 vivos e nenhuma vaga: nenhum novo");
        assert_eq!(admission(Some(16), mem(64, 3), 3), 4, "uma vaga de 1 GiB");
        assert_eq!(admission(Some(16), mem(64, 2), 0), 1, "sem vivos, um começa mesmo sem vaga");
        assert_eq!(admission(Some(16), mem(2, 1), 0), 1, "RAM total que não comporta nenhum: ainda um");
        assert_eq!(admission(Some(16), mem(0, 0), 0), 1, "zero de memória não zera o teto");
    }

    #[test]
    fn memoria_desconhecida_ou_falha_da_no_maximo_dois_e_pelo_menos_um() {
        assert_eq!(admission(None, mem(64, 64), 0), 2, "CPU desconhecida");
        assert_eq!(admission(Some(32), None, 0), 2, "memória desconhecida: no máximo 2");
        assert_eq!(admission(None, None, 0), 2, "nada lido");
        assert_eq!(admission(Some(1), None, 0), 1, "a CPU já dava 1");
        assert_eq!(admission(None, None, 5), 2, "com 5 vivos, só diz que não cabe novo: quem chama não mata ninguém");
    }

    #[test]
    fn reducao_do_teto_so_segura_novos_e_retomada_abre_a_fila() {
        // 5 vivos e a RAM total caiu para 4 GiB: o teto passa a 2, abaixo dos vivos. Não há início novo.
        assert_eq!(admission(Some(16), mem(4, 4), 5), 2);
        // A memória voltou: o mesmo vivo ganha vagas e o teto sobe.
        assert_eq!(admission(Some(16), mem(64, 6), 1), 5);
    }

    #[test]
    fn valores_extremos_nao_estouram() {
        let cheio = Some(Memory { total: u64::MAX, available: u64::MAX });
        assert_eq!(admission(Some(64), cheio, usize::MAX), 16);
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
        assert!(m.total > 0 && m.available <= m.total, "{m:?}");
    }
}
