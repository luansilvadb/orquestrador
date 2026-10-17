//! Quantos executores podem estar vivos ao mesmo tempo. Não há opção do usuário: o teto sai da CPU e da memória
//! física do computador, e do que está livre agora.
//! ponytail: orçamento aproximado, não a medida do peso de cada tarefa. Cada executor conta como 1 GiB, e o
//! sistema e o app ficam com 2 GiB antes disso.
use std::sync::Arc;
use std::thread;

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
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: MEMORYSTATUSEX é só números; a estrutura é zerada e `dwLength` é o tamanho dela, como a API exige.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: a API só escreve dentro da estrutura passada, que vive até a chamada acabar.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return None;
    }
    Some(Memory { total: status.ullTotalPhys, available: status.ullAvailPhys })
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
    fn leitura_real_da_memoria_e_das_cpus() {
        let real = Resources::real();
        assert!(real.cpus.is_some_and(|n| n >= 1), "available_parallelism devolve pelo menos 1");
        let m = (real.memory)().expect("o Windows responde GlobalMemoryStatusEx");
        assert!(m.total > 0 && m.available <= m.total, "{m:?}");
    }
}
