//! Limite do sistema: tecla → eco no ConPTY sem o app. Abre 4 ConPTYs com `node flood.mjs` (6 mil linhas/s cada, lidas
//! e descartadas) e um quinto com pwsh, onde escreve uma letra e mede até ela voltar na leitura. Mesmo cenário da bancada
//! `bancada_latencia` de `crates/app`. Uso: `cargo run --release -p orquestrador-core --example eco_conpty -- <flood.mjs>`.
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const TAMANHO: PtySize = PtySize { cols: 62, rows: 30, pixel_width: 0, pixel_height: 0 };

fn estat(nome: &str, v: &mut [f64]) {
    v.sort_by(|a, b| a.total_cmp(b));
    let p = |q: f64| v[((v.len() as f64 - 1.) * q).round() as usize];
    println!("{nome}: n={} p50={:.2} p95={:.2} p99={:.2} max={:.2}", v.len(), p(0.5), p(0.95), p(0.99), v[v.len() - 1]);
}

/// Espera em `rx` um pedaço que contenha `letra`; devolve quando chegou.
fn ate(rx: &Receiver<(Instant, Vec<u8>)>, letra: u8, limite: Duration) -> Option<Instant> {
    let fim = Instant::now() + limite;
    loop {
        let (t, b) = rx.recv_timeout(fim.saturating_duration_since(Instant::now())).ok()?;
        if b.contains(&letra) {
            return Some(t);
        }
    }
}

fn drenar(rx: &Receiver<(Instant, Vec<u8>)>, quieto: Duration) {
    while rx.recv_timeout(quieto).is_ok() {}
}

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(ms: u32) -> u32;
}

fn main() {
    // `ORQ_TIMER=1`: pede 1 ms de resolução do timer do sistema, para ver se o atraso do pwsh é o tique de 15,6 ms.
    if std::env::var_os("ORQ_TIMER").is_some() {
        unsafe { timeBeginPeriod(1) };
    }
    let flood = std::env::args().nth(1).expect("caminho do flood.mjs");
    let amostras: usize = std::env::var("ORQ_AMOSTRAS").ok().and_then(|v| v.parse().ok()).unwrap_or(250);
    let historico = std::env::var_os("APPDATA")
        .map(|a| std::path::PathBuf::from(a).join(r"Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt"));
    let antes = historico.as_ref().and_then(|h| std::fs::read(h).ok());
    let sistema = native_pty_system();

    let par = sistema.openpty(TAMANHO).unwrap();
    // `ORQ_SHELL` troca o shell do eco (para separar conhost de PSReadLine); o padrão é o do app.
    let shell = std::env::var("ORQ_SHELL").unwrap_or_else(|_| "pwsh -NoLogo".into());
    let mut partes = shell.split_whitespace();
    let mut cmd = CommandBuilder::new(partes.next().unwrap());
    cmd.args(partes);
    let _pwsh = par.slave.spawn_command(cmd).unwrap();
    drop(par.slave);
    let mut leitor = par.master.try_clone_reader().unwrap();
    let mut escritor = par.master.take_writer().unwrap();
    let _ = escritor.write_all(b"\x1b[1;1R");
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16384];
        while let Ok(n @ 1..) = leitor.read(&mut buf) {
            if tx.send((Instant::now(), buf[..n].to_vec())).is_err() {
                break;
            }
        }
    });
    drenar(&rx, Duration::from_secs(3));
    escritor.write_all(b"Set-PSReadLineOption -HistorySaveStyle SaveNothing -PredictionSource None\r").unwrap();
    drenar(&rx, Duration::from_secs(1));

    let mut medir = |rotulo: &str| {
        let (mut eco, mut escrita) = (Vec::new(), Vec::new());
        let mut seed = 7u32;
        for i in 0..amostras {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            std::thread::sleep(Duration::from_millis(20 + (seed >> 26) as u64));
            drenar(&rx, Duration::ZERO);
            let letra = b'a' + (i % 26) as u8;
            let t0 = Instant::now();
            escritor.write_all(&[letra]).and_then(|_| escritor.flush()).unwrap();
            escrita.push(t0.elapsed().as_secs_f64() * 1000.);
            if std::env::var_os("ORQ_DEPURAR").is_some() && i < 12 {
                let fim = Instant::now() + Duration::from_millis(60);
                while let Ok((t, b)) = rx.recv_timeout(fim.saturating_duration_since(Instant::now())) {
                    println!("{i} +{:.2} ms {:?}", t.saturating_duration_since(t0).as_secs_f64() * 1000., String::from_utf8_lossy(&b));
                }
            }
            let t = ate(&rx, letra, Duration::from_secs(5));
            eco.push(t.map_or(5000., |t| t.saturating_duration_since(t0).as_secs_f64() * 1000.));
            escritor.write_all(b"\x7f").unwrap();
            drenar(&rx, Duration::from_millis(15));
        }
        estat(&format!("{rotulo} eco"), &mut eco);
        estat(&format!("{rotulo} write"), &mut escrita);
    };
    medir("sem carga   ");

    let mut floods = Vec::new();
    for tag in ["1", "2", "3", "4"] {
        let par = sistema.openpty(TAMANHO).unwrap();
        let mut cmd = CommandBuilder::new("node");
        cmd.args([flood.as_str(), "900000", tag, "6000"]);
        let filho = par.slave.spawn_command(cmd).unwrap();
        drop(par.slave);
        let mut leitor = par.master.try_clone_reader().unwrap();
        let mut w = par.master.take_writer().unwrap();
        let _ = w.write_all(b"\x1b[1;1R");
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            while let Ok(1..) = leitor.read(&mut buf) {}
        });
        floods.push((filho, par.master, w));
    }
    std::thread::sleep(Duration::from_secs(3));
    medir("4 despejando");
    for (mut f, _, _) in floods {
        let _ = f.kill();
    }
    if let (Some(h), Some(antes)) = (historico, antes) {
        if std::fs::read(&h).is_ok_and(|d| d != antes && d.starts_with(&antes)) {
            std::fs::write(&h, &antes).unwrap();
        }
    }
}
