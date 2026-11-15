//! O estado de cada terminal, em Rust o tempo todo: um `alacritty_terminal::Term` por terminal, alimentado pela saída do
//! PTY na thread de envio do core (fora da thread da janela). A janela só lê a grade dos terminais visíveis ao desenhar.
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use orquestrador_core::core::projects::Projects;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
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

/// Janela que se redesenha na hora quando chega o eco de uma tecla (HWND; 0 sem janela).
static JANELA: AtomicIsize = AtomicIsize::new(0);
/// Por quanto tempo depois de uma tecla a saída do terminal redesenha na hora: o eco do pwsh chega em ~12 ms.
const JANELA_ECO_US: u64 = 50_000;

pub fn ligar_janela(hwnd: isize) {
    JANELA.store(hwnd, Ordering::Release);
}

fn agora_us() -> u64 {
    static INICIO: OnceLock<std::time::Instant> = OnceLock::new();
    INICIO.get_or_init(std::time::Instant::now).elapsed().as_micros() as u64 + 1
}

/// Pede `WM_PAINT` já, sem esperar o próximo vsync. O aviso postado antes chega primeiro à thread da janela (mensagens
/// postadas vêm antes do `WM_PAINT`), então o quadro já sai com a vista suja.
#[cfg(windows)]
fn redesenhar_ja() {
    #[link(name = "user32")]
    extern "system" {
        fn InvalidateRect(hwnd: isize, rect: *const std::ffi::c_void, apagar: i32) -> i32;
    }
    let h = JANELA.load(Ordering::Acquire);
    if h != 0 {
        unsafe { InvalidateRect(h, std::ptr::null(), 0) };
    }
}

#[cfg(not(windows))]
fn redesenhar_ja() {}

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
    /// Até quando (`agora_us`) a saída redesenha na hora: o eco da última tecla.
    eco_ate: AtomicU64,
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
        Motor { estado: Mutex::new(Estado { term, processador: Processor::new() }), ouvinte, sujo: AtomicBool::new(false), eco_ate: AtomicU64::new(0), processado_em: Mutex::new(None), chave }
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
        let respostas = std::mem::take(&mut *self.ouvinte.respostas.lock().unwrap_or_else(|e| e.into_inner()));
        (respostas, !self.sujo.swap(true, Ordering::AcqRel))
    }

    /// Medição: resumo da linha do cursor e da posição dele. O pwsh manda `ESC[?25l` logo após a tecla e o caractere
    /// 10 ms ou mais depois, então "chegou saída" não serve de eco; mudar a linha do cursor serve.
    pub fn assinatura_cursor(&self) -> u64 {
        use alacritty_terminal::index::{Column, Point};
        use std::hash::{Hash, Hasher};
        let e = self.travar();
        let g = e.term.grid();
        let p = g.cursor.point;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (p.line.0, p.column.0).hash(&mut h);
        for c in 0..g.columns() {
            g[Point::new(p.line, Column(c))].c.hash(&mut h);
        }
        h.finish()
    }

    /// Tecla enviada: a saída dos próximos `JANELA_ECO_US` é o eco e vai à tela sem esperar o vsync.
    pub fn esperar_eco(&self) {
        self.eco_ate.store(agora_us() + JANELA_ECO_US, Ordering::Release);
    }

    fn em_eco(&self) -> bool {
        let ate = self.eco_ate.load(Ordering::Acquire);
        ate != 0 && agora_us() < ate
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

/// Medido (docs/desempenho-gpui.md): o PowerShell só ecoa a tecla no tique seguinte do timer do Windows, de 15,6 ms por
/// padrão, o que põe 0 a 25 ms em cada eco; com 1 ms o eco fica em ~12 ms constantes. Vale para o processo todo.
#[cfg(windows)]
fn timer_de_1ms() {
    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(ms: u32) -> u32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn SetProcessInformation(processo: isize, classe: i32, info: *const std::ffi::c_void, tamanho: u32) -> i32;
    }
    /// `PROCESS_POWER_THROTTLING_STATE` com `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` no controle e estado
    /// zerado: o Windows 11 deixa de ignorar o pedido quando a janela fica encoberta ou minimizada.
    #[repr(C)]
    struct Estrangulamento {
        versao: u32,
        controle: u32,
        estado: u32,
    }
    static FEITO: std::sync::Once = std::sync::Once::new();
    FEITO.call_once(|| unsafe {
        let e = Estrangulamento { versao: 1, controle: 0x4, estado: 0 };
        SetProcessInformation(GetCurrentProcess(), 4, &e as *const _ as _, std::mem::size_of::<Estrangulamento>() as u32);
        timeBeginPeriod(1);
    });
}

impl Registro {
    pub fn novo() -> (Arc<Registro>, Avisos) {
        #[cfg(windows)]
        timer_de_1ms();
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
        if motor.em_eco() {
            redesenhar_ja();
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

/// Bancada de latência sem janela: o core de verdade, 4 pwsh despejando e um quinto recebendo teclas pelo mesmo caminho
/// da vista (`Registro::escrever`). Mede tecla → eco no `Term`. Uso (cerca de 1 min, Windows com pwsh):
/// `cargo test --release -p orquestrador-app bancada_latencia -- --ignored --nocapture`.
#[cfg(all(test, windows))]
mod bancada {
    use super::*;
    use orquestrador_core::agents::{cli, session};
    use orquestrador_core::core::projects::{Projects, TermSink};
    use std::time::{Duration, Instant};

    /// Compila `xtask` (o gerador `despejar`) no mesmo diretório de alvo deste teste e devolve o exe.
    fn xtask() -> std::path::PathBuf {
        let alvo = std::env::current_exe().unwrap().ancestors().nth(3).unwrap().to_path_buf();
        let ok = std::process::Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", "xtask"])
            .env("CARGO_TARGET_DIR", &alvo)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "cargo build -p xtask");
        alvo.join("debug").join("xtask.exe")
    }

    const SEM_HISTORICO: &str = "Set-PSReadLineOption -HistorySaveStyle SaveNothing -PredictionSource None\r";

    fn texto_cursor(m: &Motor) -> String {
        use alacritty_terminal::index::{Column, Point};
        let e = m.travar();
        let g = e.term.grid();
        let l = g.cursor.point.line;
        (0..g.columns()).map(|c| g[Point::new(l, Column(c))].c).collect::<String>().trim_end().to_string()
    }

    fn esperar(limite: Duration, mut f: impl FnMut() -> bool) -> bool {
        let t = Instant::now();
        while t.elapsed() < limite {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        false
    }

    fn estat(nome: &str, v: &mut [f64]) -> String {
        v.sort_by(|a, b| a.total_cmp(b));
        let p = |q: f64| v[((v.len() as f64 - 1.) * q).round() as usize];
        format!("{nome}: n={} p50={:.2} p95={:.2} p99={:.2} max={:.2}", v.len(), p(0.5), p(0.95), p(0.99), v[v.len() - 1])
    }

    fn ram_mb() -> String {
        let cmd = format!("$p=Get-Process -Id {}; '{{0:N0}} / {{1:N0}}' -f ($p.PrivateMemorySize64/1MB),($p.WorkingSet64/1MB)", std::process::id());
        let s = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &cmd]).output();
        s.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    }

    /// A letra esperada no terminal alvo e, quando ela chega na saída, os instantes antes e depois de o `Term` processá-la.
    #[derive(Default)]
    struct Espera {
        letra: Option<u8>,
        t0: Option<Instant>,
        entregue: Option<Instant>,
        processado: Option<Instant>,
    }

    /// Devolve, em ms desde a tecla: a letra entregue pelo core e a letra processada no `Term`.
    fn amostras(registro: &Registro, alvo: &Chave, n: usize, espera: &Mutex<Espera>) -> (Vec<f64>, Vec<f64>) {
        let motor = registro.obter(alvo).unwrap();
        let (mut entregue, mut processado) = (Vec::new(), Vec::new());
        let mut seed = 7u32;
        let ms = |d: Duration| d.as_secs_f64() * 1000.;
        for i in 0..n {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            std::thread::sleep(Duration::from_millis(20 + (seed >> 26) as u64));
            let letra = b'a' + (i % 26) as u8;
            let t0 = Instant::now();
            *espera.lock().unwrap() = Espera { letra: Some(letra), t0: Some(t0), ..Espera::default() };
            registro.escrever(alvo, (letra as char).to_string());
            esperar(Duration::from_secs(5), || espera.lock().unwrap().processado.is_some());
            let e = std::mem::take(&mut *espera.lock().unwrap());
            let d = |t: Option<Instant>| t.map_or(5000., |t| ms(t.saturating_duration_since(t0)));
            entregue.push(d(e.entregue));
            processado.push(d(e.processado));
            registro.escrever(alvo, "".into());
            esperar(Duration::from_secs(5), || !texto_cursor(&motor).ends_with(letra as char));
        }
        (entregue, processado)
    }

    #[test]
    #[ignore]
    fn bancada_latencia() {
        let tmp = std::env::temp_dir().join(format!("orq-bancada-{}", std::process::id()));
        let places = session::Places { temp: tmp.join("temp"), codex_rules: tmp.join("rules") };
        std::fs::create_dir_all(&places.temp).unwrap();
        let despejar = xtask();
        // O PSReadLine grava no histórico real do usuário; o que esta bancada acrescentar é desfeito no fim.
        let historico = std::env::var_os("APPDATA")
            .map(|a| std::path::PathBuf::from(a).join(r"Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt"));
        let antes = historico.as_ref().and_then(|h| std::fs::read(h).ok());

        let (registro, _avisos) = Registro::novo();
        let alvo = chave(None, "shell-9");
        let espera: Arc<Mutex<Espera>> = Arc::default();
        let (destino, marca, id_alvo) = (registro.clone(), espera.clone(), alvo.1.clone());
        let sink: TermSink = Arc::new(move |p, id, _seq, bytes| {
            let tem = id == id_alvo && marca.lock().unwrap().letra.is_some_and(|l| bytes.contains(&l));
            let t = Instant::now();
            if id == id_alvo && std::env::var_os("ORQ_DEPURAR").is_some() {
                let t0 = marca.lock().unwrap().t0;
                eprintln!("+{:.2} {:?}", t0.map_or(0., |t0| (t - t0).as_secs_f64() * 1000.), String::from_utf8_lossy(bytes));
            }
            destino.receber(p, id, bytes);
            if tem {
                let mut e = marca.lock().unwrap();
                if e.processado.is_none() {
                    (e.entregue, e.processado) = (Some(t), Some(Instant::now()));
                }
            }
        });
        let exes = cli::Exes::new("claude.exe", "codex.exe", "orquestrador.exe");
        let projetos = Arc::new(Projects::new(exes, places, tmp.join("config.json"), Arc::new(|_, _| {})).with_term_sink(sink));
        registro.ligar(&projetos);
        let core = projetos.shell_core(None).unwrap();
        let ids = ["shell-1", "shell-2", "shell-3", "shell-4", "shell-9"];
        for id in ids {
            registro.motor(&chave(None, id), 62, 30);
            core.start_shell_in(id, 62, 30, None).unwrap();
        }
        for id in ids {
            let m = registro.obter(&chave(None, id)).unwrap();
            assert!(esperar(Duration::from_secs(30), || texto_cursor(&m).ends_with('>')), "prompt de {id}");
            registro.escrever(&chave(None, id), SEM_HISTORICO.into());
        }
        std::thread::sleep(Duration::from_secs(3));
        eprintln!("RAM ociosa (MB privada / working set): {}", ram_mb());
        let n = std::env::var("ORQ_AMOSTRAS").ok().and_then(|v| v.parse().ok()).unwrap_or(250);
        let (mut c, mut eco) = amostras(&registro, &alvo, n, &espera);
        eprintln!("sem carga    {}", estat("core entregou", &mut c));
        eprintln!("sem carga    {}", estat("eco no Term", &mut eco));
        for id in &ids[..4] {
            registro.escrever(&chave(None, id), format!("& '{}' despejar 900000 {} 6000\r", despejar.display(), &id[6..]));
        }
        std::thread::sleep(Duration::from_secs(3));
        let (mut c, mut eco) = amostras(&registro, &alvo, n, &espera);
        eprintln!("4 despejando {}", estat("core entregou", &mut c));
        eprintln!("4 despejando {}", estat("eco no Term", &mut eco));
        eprintln!("RAM com carga: {}", ram_mb());
        for id in &ids[..4] {
            registro.escrever(&chave(None, id), "\x03".into());
        }
        std::thread::sleep(Duration::from_secs(10));
        eprintln!("RAM 10 s depois: {}", ram_mb());
        projetos.shutdown();
        if let (Some(h), Some(antes)) = (historico, antes) {
            if std::fs::read(&h).is_ok_and(|d| d != antes && d.starts_with(&antes)) {
                std::fs::write(&h, &antes).unwrap();
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
