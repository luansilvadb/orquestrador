//! Latência de eco de um PTY puro (sem Tauri nem WebView): escreve 1 caractere no PTY e mede até o eco chegar à leitura.
//! Piso do caminho ConPTY + shell, para comparar com o eco do app (keydown -> letra no DOM do xterm: p50 23,2 ms em docs/desempenho-base.md).
//!
//! Rodar: cargo run --release --example eco_pty --manifest-path src-tauri/Cargo.toml [-- <amostras> [--v=psrl,sempsrl,laco,cmd] [cenário ...]]
//! (amostras: padrão 250; ECO_DEBUG=1 mostra o texto lido; sem cenário roda só 62x24).
//! Cenário: COLSxROWS[>RxL | >>RxL][+job][+thr][+cwd][+env][+below][+eco]
//!   `>RxL`   resize logo depois do spawn (antes do primeiro prompt); `>>RxL` resize depois do prompt desenhado.
//!   `+job`   o filho entra num Job com KILL_ON_JOB_CLOSE (`platform::Job::adopt`, como `Term::spawn`).
//!   `+thr`   leitura em duas threads com coalescência de 4 ms e corte do `ESC[6n` inicial (como `core/pty.rs`).
//!   `+cwd`   cwd = D:\orquestrador (pasta do projeto, como o app) em vez de %TEMP%.
//!   `+env`   ambiente mínimo do Windows (env_clear + variáveis básicas), sem as do processo pai.
//!   `+app`   o shell sobe por `core::pty::Term::spawn` (código do app; já inclui Job, thr e escrita pelo lock); +cwd vale, os demais fatores não.
//!   `+below` o example roda em BELOW_NORMAL durante o spawn (o filho herda); `+eco` EcoQoS (power throttling) no filho.
//!
//! Replica `core/pty.rs` e `agents/cli.rs::shell`: portable-pty 0.9, ConPTY, 62x24 por padrão, `pwsh.exe -NoLogo`, ambiente herdado,
//! resposta imediata `ESC[1;1R` ao pedido de posição do cursor, leitura em blocos de 16 KiB, escrita com `write_all` + `flush`.
//! Desvio: o PSReadLine (predição e histórico desligados como em scripts/medir-desempenho.mjs) é configurado por `-NoExit -Command`,
//! para a linha de configuração não entrar no histórico do usuário. Nenhuma amostra aperta Enter, então nada é gravado.
//! Variantes: pwsh com PSReadLine (o shell do app), pwsh sem PSReadLine, pwsh com laço ReadKey/Write e cmd.exe (para separar ConPTY, shell e editor de linha).
//! Cada amostra: espera a saída silenciar 100 ms, escreve uma letra (a-z em rodízio), espera a letra na saída (sem sequências de
//! escape), anota o tempo, manda Esc para limpar a linha (as 10 primeiras amostras são aquecimento e não contam). Percentis por posto mais próximo, como o script de base.

#[cfg(windows)]
mod medida {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::{Read, Write};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
    use std::time::{Duration, Instant};

    /// Tira as sequências de escape (CSI, OSC, ESC + intermediários) e deixa só o texto, mesmo quebradas entre leituras.
    #[derive(Default)]
    struct Texto(u8);
    impl Texto {
        fn feed(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
            for &b in bytes {
                self.0 = match (self.0, b) {
                    (0, 0x1b) => 1,
                    (0, _) => {
                        out.push(b);
                        0
                    }
                    (1, b'[') => 2,
                    (1, b']') => 3,
                    (1, 0x20..=0x2f) => 5,
                    (1, _) => 0,
                    (2, 0x40..=0x7e) => 0,
                    (3, 0x07) => 0,
                    (3, 0x1b) => 4,
                    (4, b'\\') => 0,
                    (4, _) => 3,
                    (5, 0x30..=0x7e) => 0,
                    (s, _) => s,
                };
            }
        }
    }

    type Chunk = (Instant, Vec<u8>);
    const AQUECE: usize = 10;

    /// Um cenário da varredura: tamanho do PTY e os fatores que o example liga um por vez.
    #[derive(Clone, Default)]
    pub struct Cfg {
        pub nome: String,
        pub tam: (u16, u16),
        pub resize_ja: Option<(u16, u16)>,
        pub resize_prompt: Option<(u16, u16)>,
        pub job: bool,
        pub thr: bool,
        pub cwd: bool,
        pub env: bool,
        pub below: bool,
        pub eco: bool,
        pub app: bool,
    }

    fn par(t: &str) -> Option<(u16, u16)> {
        let (c, r) = t.split_once('x')?;
        Some((c.parse().ok()?, r.parse().ok()?))
    }

    pub fn cenario(spec: &str) -> Option<Cfg> {
        let mut partes = spec.split('+');
        let tamanho = partes.next()?;
        let mut c = Cfg { nome: spec.to_string(), ..Default::default() };
        if let Some((a, b)) = tamanho.split_once(">>") {
            c.tam = par(a)?;
            c.resize_prompt = Some(par(b)?);
        } else if let Some((a, b)) = tamanho.split_once('>') {
            c.tam = par(a)?;
            c.resize_ja = Some(par(b)?);
        } else {
            c.tam = par(tamanho)?;
        }
        for m in partes {
            match m {
                "job" => c.job = true,
                "thr" => c.thr = true,
                "cwd" => c.cwd = true,
                "env" => c.env = true,
                "below" => c.below = true,
                "eco" => c.eco = true,
                "app" => c.app = true,
                _ => return None,
            }
        }
        Some(c)
    }

    /// Ambiente mínimo de um logon do Windows, sem as variáveis do processo pai.
    fn env_minimo(cmd: &mut CommandBuilder) {
        let manter = [
            "SystemRoot", "windir", "SystemDrive", "ComSpec", "PATH", "PATHEXT", "USERNAME", "USERDOMAIN", "COMPUTERNAME", "USERPROFILE",
            "HOMEDRIVE", "HOMEPATH", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432",
            "ProgramData", "CommonProgramFiles", "ALLUSERSPROFILE", "PUBLIC", "OS", "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE",
            "PROCESSOR_IDENTIFIER", "PROCESSOR_LEVEL", "PROCESSOR_REVISION", "DriverData", "LOGONSERVER", "SESSIONNAME",
        ];
        let tudo: Vec<_> = std::env::vars_os().collect();
        cmd.env_clear();
        for (k, v) in tudo {
            if manter.iter().any(|m| k.to_string_lossy().eq_ignore_ascii_case(m)) {
                cmd.env(k, v);
            }
        }
    }

    fn eco_qos(h: windows_sys::Win32::Foundation::HANDLE) {
        use windows_sys::Win32::System::Threading::{
            ProcessPowerThrottling, SetProcessInformation, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
        };
        let st = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        };
        unsafe {
            SetProcessInformation(h, ProcessPowerThrottling, &st as *const _ as *const _, std::mem::size_of_val(&st) as u32);
        }
    }

    /// Descarta a saída até ficar `quiet` sem nada novo (ou até `cap`). Devolve se chegou algo.
    fn silenciar(rx: &Receiver<Chunk>, quiet: Duration, cap: Duration) -> bool {
        let fim = Instant::now() + cap;
        let mut viu = false;
        loop {
            let resta = fim.saturating_duration_since(Instant::now());
            if resta.is_zero() {
                return viu;
            }
            match rx.recv_timeout(quiet.min(resta)) {
                Ok(_) => viu = true,
                Err(RecvTimeoutError::Timeout) if Instant::now() < fim => return viu,
                Err(_) => return viu,
            }
        }
    }

    pub struct Resultado {
        pub eco: Vec<f64>,
        pub primeiro_byte: Vec<f64>,
        pub sem_eco: usize,
    }

    fn tamanho(c: u16, r: u16) -> PtySize {
        PtySize { cols: c, rows: r, pixel_width: 0, pixel_height: 0 }
    }

    pub fn medir(programa: &str, args: &[&str], amostras: usize, cfg: &Cfg) -> Result<Resultado, String> {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, OpenProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_SET_INFORMATION,
        };
        let pair = native_pty_system().openpty(tamanho(cfg.tam.0, cfg.tam.1)).map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(programa);
        cmd.args(args);
        cmd.cwd(if cfg.cwd { std::path::PathBuf::from(r"D:\orquestrador") } else { std::env::temp_dir() });
        if cfg.env {
            env_minimo(&mut cmd);
        }
        if cfg.below {
            unsafe { SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS) };
        }
        let spawn = pair.slave.spawn_command(cmd);
        if cfg.below {
            unsafe { SetPriorityClass(GetCurrentProcess(), NORMAL_PRIORITY_CLASS) };
        }
        let mut filho = spawn.map_err(|e| format!("{e}"))?;
        drop(pair.slave);
        let _job = if cfg.job { filho.process_id().and_then(orquestrador_lib::platform::Job::adopt) } else { None };
        if cfg.job && _job.is_none() {
            let _ = filho.kill();
            return Err("Job::adopt falhou".into());
        }
        if cfg.eco {
            if let Some(pid) = filho.process_id() {
                unsafe {
                    let h = OpenProcess(PROCESS_SET_INFORMATION, 0, pid);
                    if !h.is_null() {
                        eco_qos(h);
                        windows_sys::Win32::Foundation::CloseHandle(h);
                    }
                }
            }
        }
        if let Some((c, r)) = cfg.resize_ja {
            pair.master.resize(tamanho(c, r)).map_err(|e| e.to_string())?;
        }
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        // O ConPTY abre pedindo a posição do cursor; o app responde na hora (core/pty.rs).
        let _ = writer.write_all(b"\x1b[1;1R");

        let (tx, rx) = mpsc::channel::<Chunk>();
        if cfg.thr {
            // Como `core/pty.rs`: a thread de leitura só lê e repassa (cortando o ESC[6n inicial); a de envio agrupa o que chega
            // com o intervalo de 4 ms ainda aberto e entrega.
            const QUERY: &[u8] = b"\x1b[6n";
            let (tx1, rx1) = mpsc::channel::<Vec<u8>>();
            std::thread::spawn(move || {
                let mut bloco = [0u8; 16384];
                let mut primeiro = true;
                while let Ok(n) = reader.read(&mut bloco) {
                    if n == 0 {
                        break;
                    }
                    let mut bytes = &bloco[..n];
                    if std::mem::take(&mut primeiro) && bytes.starts_with(QUERY) {
                        bytes = &bytes[QUERY.len()..];
                    }
                    if !bytes.is_empty() && tx1.send(bytes.to_vec()).is_err() {
                        break;
                    }
                }
            });
            std::thread::spawn(move || {
                let mut last: Option<Instant> = None;
                while let Ok(mut batch) = rx1.recv() {
                    let deadline = last.map_or_else(Instant::now, |t| t + Duration::from_millis(4));
                    while batch.len() < 256 << 10 {
                        match rx1.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                            Ok(mais) => batch.extend_from_slice(&mais),
                            Err(_) => break,
                        }
                    }
                    if tx.send((Instant::now(), batch)).is_err() {
                        break;
                    }
                    last = Some(Instant::now());
                }
            });
        } else {
            std::thread::spawn(move || {
                let mut bloco = [0u8; 16384];
                while let Ok(n) = reader.read(&mut bloco) {
                    if n == 0 || tx.send((Instant::now(), bloco[..n].to_vec())).is_err() {
                        break;
                    }
                }
            });
        }

        if !silenciar(&rx, Duration::from_millis(1500), Duration::from_secs(20)) {
            let _ = filho.kill();
            return Err("o shell não imprimiu nada em 20 s".into());
        }
        if let Some((c, r)) = cfg.resize_prompt {
            pair.master.resize(tamanho(c, r)).map_err(|e| e.to_string())?;
            silenciar(&rx, Duration::from_millis(1500), Duration::from_secs(10));
        }

        let r = colher(&rx, &mut |b: &[u8]| drop(writer.write_all(b).and_then(|_| writer.flush())), amostras, programa);
        let _ = filho.kill();
        let _ = filho.wait();
        Ok(r)
    }

    /// Mesma medida, mas o shell sobe por `core::pty::Term::spawn` (o código do app: Job, corte do ESC[6n, coalescência de 4 ms,
    /// escrita pelo lock do `Writer`), com `agents::cli::Spawn` igual ao de `cli::shell` (cwd do projeto, sem env), mais os
    /// argumentos da variante.
    pub fn medir_app(programa: &str, args: &[&str], amostras: usize, cfg: &Cfg) -> Result<Resultado, String> {
        use orquestrador_lib::agents::cli::Spawn;
        use orquestrador_lib::core::pty::{Buffer, Term};
        let (tx, rx) = mpsc::channel::<Chunk>();
        let spawn = Spawn {
            program: programa.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: if cfg.cwd { r"D:\orquestrador".to_string() } else { std::env::temp_dir().to_string_lossy().into_owned() },
            env: vec![],
        };
        let buffer = std::sync::Arc::new(std::sync::Mutex::new(Buffer::default()));
        let mut term = Term::spawn(&spawn, cfg.tam, buffer, move |_, b| drop(tx.send((Instant::now(), b.to_vec()))), |_| {})?;
        if let Some((c, r)) = cfg.resize_ja {
            term.resize(c, r);
        }
        if !silenciar(&rx, Duration::from_millis(1500), Duration::from_secs(20)) {
            return Err("o shell não imprimiu nada em 20 s".into());
        }
        if let Some((c, r)) = cfg.resize_prompt {
            term.resize(c, r);
            silenciar(&rx, Duration::from_millis(1500), Duration::from_secs(10));
        }
        let r = colher(&rx, &mut |b: &[u8]| term.write(b), amostras, programa);
        drop(term); // mata o Job
        Ok(r)
    }

    fn colher(rx: &Receiver<Chunk>, escrever: &mut dyn FnMut(&[u8]), amostras: usize, programa: &str) -> Resultado {
        let debug = std::env::var_os("ECO_DEBUG").is_some();
        let mut r = Resultado { eco: vec![], primeiro_byte: vec![], sem_eco: 0 };
        let mut semente = 0x9e3779b9u32;
        // As primeiras amostras (aquecimento) não entram: o prompt do pwsh pode terminar de desenhar depois do silêncio inicial
        // e o texto dele contém letras que seriam tomadas por eco.
        for i in 0..amostras + AQUECE {
            silenciar(rx, Duration::from_millis(100), Duration::from_secs(3));
            semente ^= semente << 13;
            semente ^= semente >> 17;
            semente ^= semente << 5;
            std::thread::sleep(Duration::from_millis(20 + (semente % 61) as u64));
            while rx.try_recv().is_ok() {}

            let letra = b'a' + (i % 26) as u8;
            let t0 = Instant::now();
            escrever(&[letra]);
            let prazo = t0 + Duration::from_secs(5);
            let (mut texto, mut limpo, mut primeiro) = (Texto::default(), Vec::new(), None);
            let mut eco = None;
            while eco.is_none() {
                let resta = prazo.saturating_duration_since(Instant::now());
                let Ok((t, bytes)) = rx.recv_timeout(resta) else { break };
                primeiro.get_or_insert(t);
                texto.feed(&bytes, &mut limpo);
                if limpo.contains(&letra) {
                    eco = Some(t);
                }
            }
            if debug && i < AQUECE + 3 && i >= AQUECE - 2 {
                eprintln!("  [{programa}] amostra {i}: texto lido = {:?}", String::from_utf8_lossy(&limpo));
            }
            let conta = i >= AQUECE;
            match eco {
                Some(t) if conta => {
                    r.eco.push((t - t0).as_secs_f64() * 1000.0);
                    r.primeiro_byte.push((primeiro.unwrap() - t0).as_secs_f64() * 1000.0);
                }
                None if conta => r.sem_eco += 1,
                _ => {}
            }
            escrever(&[0x1b]);
        }
        r
    }
}

#[cfg(windows)]
fn linha(rotulo: &str, v: &[f64]) {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| s[((q / 100.0 * s.len() as f64).ceil() as usize).clamp(1, s.len()) - 1];
    let media = s.iter().sum::<f64>() / s.len() as f64;
    println!(
        "  {rotulo:<14} n={:<4} min {:>6.2}  p50 {:>6.2}  p95 {:>6.2}  p99 {:>6.2}  máx {:>6.2}  média {:>6.2}",
        s.len(), s[0], p(50.0), p(95.0), p(99.0), s[s.len() - 1], media
    );
}

#[cfg(windows)]
fn main() {
    let mut n = 250usize;
    let mut quais: Option<Vec<String>> = None;
    let mut cenarios = Vec::new();
    for a in std::env::args().skip(1) {
        if a == "--timer" {
            // Diagnóstico: resolução de timer do sistema e o custo real de `sleep(1 ms)` neste processo (Rust nativo, sem shell).
            #[link(name = "ntdll")]
            extern "system" {
                fn NtQueryTimerResolution(min: *mut u32, max: *mut u32, cur: *mut u32) -> i32;
            }
            let (mut mn, mut mx, mut cur) = (0u32, 0u32, 0u32);
            unsafe { NtQueryTimerResolution(&mut mn, &mut mx, &mut cur) };
            let v: Vec<f64> = (0..200)
                .map(|_| {
                    let t = std::time::Instant::now();
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            println!("timer (100 ns): mais grosso {mn}, mais fino {mx}, atual {cur}");
            linha("sleep(1 ms)", &v);
            return;
        }
        if let Some(v) = a.strip_prefix("--v=") {
            quais = Some(v.split(',').map(str::to_string).collect());
        } else if let Ok(k) = a.parse::<usize>() {
            n = k;
        } else if let Some(c) = medida::cenario(&a) {
            cenarios.push(c);
        } else {
            eprintln!("argumento inválido: {a}");
            std::process::exit(2);
        }
    }
    if cenarios.is_empty() {
        cenarios.push(medida::cenario("62x24").unwrap());
    }
    // PSReadLine sem predição e sem gravar histórico, como a medição de base; configurado antes do primeiro prompt.
    let sem_historico = "Set-PSReadLineOption -HistorySaveStyle SaveNothing -PredictionSource None";
    let sem_psrl = "Remove-Module PSReadLine";
    // Laço mínimo: lê a tecla e a escreve de volta, sem editor de linha. Separa o custo do PSReadLine do custo do ConPTY.
    let laco = "[Console]::Write('> '); while ($true) { $k = [Console]::ReadKey($true); if ($k.KeyChar -ne [char]27) { [Console]::Write($k.KeyChar) } }";
    let variantes: [(&str, &str, &str, Vec<&str>); 4] = [
        ("psrl", "pwsh + PSReadLine (shell do app)", "pwsh.exe", vec!["-NoLogo", "-NoExit", "-Command", sem_historico]),
        ("sempsrl", "pwsh sem PSReadLine", "pwsh.exe", vec!["-NoLogo", "-NoExit", "-Command", sem_psrl]),
        ("laco", "pwsh ReadKey + Write (sem editor de linha)", "pwsh.exe", vec!["-NoLogo", "-NoProfile", "-Command", laco]),
        ("cmd", "cmd.exe", "cmd.exe", vec![]),
    ];
    println!("Eco de 1 caractere em PTY puro (portable-pty, ConPTY), ms, {n} amostras por variante");
    for cfg in &cenarios {
        println!("\n== cenário {} ==", cfg.nome);
        for (chave, nome, programa, args) in &variantes {
            if quais.as_ref().is_some_and(|q| !q.iter().any(|x| x == chave)) {
                continue;
            }
            println!("{nome}");
            let r = if cfg.app { medida::medir_app(programa, args, n, cfg) } else { medida::medir(programa, args, n, cfg) };
            match r {
                Ok(r) => {
                    if r.eco.is_empty() {
                        println!("  nenhuma amostra com eco ({} sem eco em 5 s)", r.sem_eco);
                        continue;
                    }
                    linha("eco da letra", &r.eco);
                    linha("1º byte lido", &r.primeiro_byte);
                    if r.sem_eco > 0 {
                        println!("  sem eco em 5 s: {} (fora dos percentis)", r.sem_eco);
                    }
                }
                Err(e) => println!("  falhou: {e}"),
            }
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("eco_pty mede o ConPTY do Windows.");
}
