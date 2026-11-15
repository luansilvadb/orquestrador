//! O estado de cada terminal, em Rust o tempo todo: um `alacritty_terminal::Term` por terminal, alimentado pela saída do
//! PTY na thread de envio do core (fora da thread da janela). A janela só lê a grade dos terminais visíveis ao desenhar.
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use orquestrador_core::core::projects::Projects;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

/// Linhas de histórico, as mesmas do xterm (`scrollback: 10000` em term.ts).
pub const HISTORICO: usize = 10_000;
/// O parser solta o lock a cada pedaço deste tamanho: um despejo grande não segura o desenho nem a digitação.
const PEDACO: usize = 16 << 10;
/// Tamanho de um executor antes de aparecer (`EXECUTOR_SIZE` do core).
pub const TAMANHO_EXECUTOR: (u16, u16) = (110, 30);

/// Projeto (`None` é o terminal sem projeto) e id do terminal: o mesmo id existe em projetos diferentes.
pub type Chave = (Option<String>, String);
pub type Avisos = async_channel::Receiver<Chave>;

pub fn chave(projeto: Option<&str>, id: &str) -> Chave {
    (projeto.map(str::to_string), id.to_string())
}

#[derive(Clone, Copy, Debug)]
pub struct Tamanho {
    pub colunas: usize,
    pub linhas: usize,
}

impl Dimensions for Tamanho {
    fn total_lines(&self) -> usize {
        self.linhas
    }
    fn screen_lines(&self) -> usize {
        self.linhas
    }
    fn columns(&self) -> usize {
        self.colunas
    }
}

/// Recolhe o que o emulador pede para fora: respostas ao programa (DSR, DA) e o título.
#[derive(Clone, Default)]
pub struct Ouvinte {
    respostas: Arc<Mutex<Vec<String>>>,
}

impl EventListener for Ouvinte {
    fn send_event(&self, evento: Event) {
        if let Event::PtyWrite(texto) = evento {
            self.respostas.lock().unwrap_or_else(|e| e.into_inner()).push(texto);
        }
    }
}

pub struct Estado {
    pub term: Term<Ouvinte>,
    processador: Processor,
}

pub struct Motor {
    estado: Mutex<Estado>,
    ouvinte: Ouvinte,
    /// Há saída nova desde o último desenho; só o primeiro pedaço depois de um desenho avisa a janela.
    sujo: AtomicBool,
    /// Quantos pedaços de saída já foram processados (a medição de latência compara antes e depois da tecla).
    pub geracao: std::sync::atomic::AtomicU64,
    /// Quando o último pedaço terminou de ser processado (só para a medição).
    pub processado_em: Mutex<Option<std::time::Instant>>,
    pub chave: Chave,
}

impl Motor {
    fn novo(chave: Chave, colunas: u16, linhas: u16) -> Motor {
        let ouvinte = Ouvinte::default();
        let config = Config { scrolling_history: HISTORICO, ..Config::default() };
        let tamanho = Tamanho { colunas: colunas.max(2) as usize, linhas: linhas.max(1) as usize };
        let term = Term::new(config, &tamanho, ouvinte.clone());
        Motor { estado: Mutex::new(Estado { term, processador: Processor::new() }), ouvinte, sujo: AtomicBool::new(false), geracao: Default::default(), processado_em: Mutex::new(None), chave }
    }

    pub fn travar(&self) -> MutexGuard<'_, Estado> {
        self.estado.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Processa a saída e devolve as respostas do emulador. `true` em `avisar` quando a janela precisa saber.
    pub fn processar(&self, bytes: &[u8]) -> (Vec<String>, bool) {
        for pedaco in bytes.chunks(PEDACO) {
            let mut e = self.travar();
            let Estado { term, processador } = &mut *e;
            processador.advance(term, pedaco);
        }
        if super::vista::medir::ligado() {
            *self.processado_em.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
        }
        self.geracao.fetch_add(1, Ordering::AcqRel);
        let respostas = std::mem::take(&mut *self.ouvinte.respostas.lock().unwrap_or_else(|e| e.into_inner()));
        (respostas, !self.sujo.swap(true, Ordering::AcqRel))
    }

    /// Chamado ao desenhar: a próxima saída volta a avisar.
    pub fn limpar(&self) {
        self.sujo.store(false, Ordering::Release);
    }

    pub fn tamanho(&self) -> (u16, u16) {
        let e = self.travar();
        (e.term.columns() as u16, e.term.screen_lines() as u16)
    }

    /// `true` se o tamanho mudou.
    pub fn redimensionar(&self, colunas: u16, linhas: u16) -> bool {
        let mut e = self.travar();
        let (c, l) = (colunas.max(2) as usize, linhas.max(1) as usize);
        if e.term.columns() == c && e.term.screen_lines() == l {
            return false;
        }
        e.term.resize(Tamanho { colunas: c, linhas: l });
        true
    }
}

/// Todos os terminais vivos, de todos os projetos. A saída de um terminal sem motor cria o motor.
pub struct Registro {
    motores: Mutex<HashMap<Chave, Arc<Motor>>>,
    /// Terminais que acabaram: saída atrasada deles não recria nada.
    esquecidos: Mutex<HashSet<Chave>>,
    aviso: async_channel::Sender<Chave>,
    projetos: Arc<OnceLock<Weak<Projects>>>,
    /// Entrada dos terminais numa thread própria: um PTY que não lê não trava a janela; a ordem das teclas fica.
    escritor: Mutex<std::sync::mpsc::Sender<(Chave, String)>>,
}

impl Registro {
    pub fn novo() -> (Arc<Registro>, Avisos) {
        let (aviso, avisos) = async_channel::unbounded();
        let projetos: Arc<OnceLock<Weak<Projects>>> = Arc::default();
        let (escritor, entradas) = std::sync::mpsc::channel::<(Chave, String)>();
        let destino = projetos.clone();
        std::thread::spawn(move || {
            while let Ok(((projeto, id), texto)) = entradas.recv() {
                if let Some(core) = destino.get().and_then(Weak::upgrade).and_then(|p| p.shell_core(projeto.as_deref()).ok()) {
                    let _ = core.term_write(&id, &texto);
                }
            }
        });
        let registro = Registro {
            motores: Mutex::default(),
            esquecidos: Mutex::default(),
            aviso,
            projetos,
            escritor: Mutex::new(escritor),
        };
        (Arc::new(registro), avisos)
    }

    pub fn ligar(&self, projetos: &Arc<Projects>) {
        let _ = self.projetos.set(Arc::downgrade(projetos));
    }

    fn mapa(&self) -> MutexGuard<'_, HashMap<Chave, Arc<Motor>>> {
        self.motores.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn escrever(&self, chave: &Chave, texto: String) {
        let _ = self.escritor.lock().unwrap_or_else(|e| e.into_inner()).send((chave.clone(), texto));
    }

    #[cfg(test)]
    pub fn obter(&self, chave: &Chave) -> Option<Arc<Motor>> {
        self.mapa().get(chave).cloned()
    }

    /// O motor do terminal, criado no tamanho dado se ainda não existe.
    pub fn motor(&self, chave: &Chave, colunas: u16, linhas: u16) -> Arc<Motor> {
        self.mapa().entry(chave.clone()).or_insert_with(|| Arc::new(Motor::novo(chave.clone(), colunas, linhas))).clone()
    }

    /// Um terminal novo no lugar do anterior com a mesma chave (outra sessão do orquestrador).
    pub fn recriar(&self, chave: &Chave, colunas: u16, linhas: u16) -> Arc<Motor> {
        self.esquecidos.lock().unwrap_or_else(|e| e.into_inner()).remove(chave);
        let motor = Arc::new(Motor::novo(chave.clone(), colunas, linhas));
        self.mapa().insert(chave.clone(), motor.clone());
        motor
    }

    /// Solta a memória do terminal; `fim` também ignora a saída que ainda chegar dele.
    pub fn remover(&self, chave: &Chave, fim: bool) {
        if fim {
            self.esquecidos.lock().unwrap_or_else(|e| e.into_inner()).insert(chave.clone());
        }
        self.mapa().remove(chave);
    }

    pub fn remover_projeto(&self, projeto: &str) {
        self.mapa().retain(|(p, _), _| p.as_deref() != Some(projeto));
    }

    /// Saída do PTY, na thread de envio do terminal no core.
    pub fn receber(&self, projeto: Option<&str>, id: &str, bytes: &[u8]) {
        let chave = chave(projeto, id);
        if self.esquecidos.lock().unwrap_or_else(|e| e.into_inner()).contains(&chave) {
            return;
        }
        let (c, l) = if orquestrador_core::core::is_interactive(id) { (80, 24) } else { TAMANHO_EXECUTOR };
        let motor = self.motor(&chave, c, l);
        let (respostas, avisar) = motor.processar(bytes);
        for r in respostas {
            self.escrever(&chave, r);
        }
        if avisar {
            let _ = self.aviso.try_send(chave);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use alacritty_terminal::index::{Column, Line, Point};

    fn linha(motor: &Motor, n: i32) -> String {
        let e = motor.travar();
        let g = e.term.grid();
        (0..g.columns()).map(|c| g[Point::new(Line(n), Column(c))].c).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn saida_vira_grade_e_so_o_primeiro_pedaco_avisa() {
        let (registro, avisos) = Registro::novo();
        registro.receber(None, "shell-1", b"ola\r\n\x1b[31mmundo\x1b[0m");
        registro.receber(None, "shell-1", b"!");
        assert_eq!(avisos.try_recv().ok(), Some(chave(None, "shell-1")));
        assert!(avisos.try_recv().is_err(), "sem desenho no meio, não avisa de novo");
        let motor = registro.obter(&chave(None, "shell-1")).unwrap();
        assert_eq!(linha(&motor, 0), "ola");
        assert_eq!(linha(&motor, 1), "mundo!");
        motor.limpar();
        registro.receber(None, "shell-1", b"x");
        assert!(avisos.try_recv().is_ok());
    }

    #[test]
    fn executor_nasce_no_tamanho_do_core_e_esquecido_nao_volta() {
        let (registro, _avisos) = Registro::novo();
        registro.receber(Some("p"), "exec-1", b"a");
        assert_eq!(registro.obter(&chave(Some("p"), "exec-1")).unwrap().tamanho(), TAMANHO_EXECUTOR);
        registro.remover(&chave(Some("p"), "exec-1"), true);
        registro.receber(Some("p"), "exec-1", b"b");
        assert!(registro.obter(&chave(Some("p"), "exec-1")).is_none());
    }

    #[test]
    fn historico_tem_o_teto_do_xterm() {
        let (registro, _a) = Registro::novo();
        let motor = registro.motor(&chave(None, "shell-9"), 20, 5);
        let mut saida = Vec::new();
        for i in 0..(HISTORICO + 500) {
            saida.extend_from_slice(format!("{i}\r\n").as_bytes());
        }
        motor.processar(&saida);
        assert_eq!(motor.travar().term.grid().history_size(), HISTORICO);
    }
}
