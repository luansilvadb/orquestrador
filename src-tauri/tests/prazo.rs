#![cfg(windows)]

use orquestrador_lib::{platform, proc};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MODE: &str = "ORQ_PRAZO_TEST_MODE";
const READY: &str = "ORQ_PRAZO_TEST_READY";
const PRAZO: Duration = Duration::from_secs(2);
const MARGEM: Duration = Duration::from_secs(4);

#[test]
fn cli_auxiliar() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    match mode.as_str() {
        "descendente" => {
            println!("DESCENDENTE-OUT");
            eprintln!("DESCENDENTE-ERR");
            io::stdout().flush().unwrap();
            io::stderr().flush().unwrap();
            let pid = std::process::id();
            let started = platform::start_time(pid).expect("o descendente está vivo");
            let mut marker = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(std::env::var_os(READY).unwrap())
                .unwrap();
            writeln!(marker, "{pid} {started}").unwrap();
            thread::sleep(Duration::from_secs(20));
        }
        "pai-sai" | "pai-vivo" => {
            println!("PAI-OUT");
            eprintln!("PAI-ERR");
            io::stdout().flush().unwrap();
            io::stderr().flush().unwrap();
            let child = helper(
                "descendente",
                Some(Path::new(&std::env::var_os(READY).unwrap())),
            )
            .spawn()
            .unwrap();
            aguardar_pronto(Path::new(&std::env::var_os(READY).unwrap()));
            drop(child);
            if mode == "pai-vivo" {
                thread::sleep(Duration::from_secs(30));
            }
        }
        "sem-entrada" => thread::sleep(Duration::from_secs(30)),
        "saida-grande" => {
            io::stdout().write_all(&vec![b'O'; 1024 * 1024]).unwrap();
            io::stderr().write_all(&vec![b'E'; 1024 * 1024]).unwrap();
        }
        _ => panic!("modo auxiliar desconhecido: {mode}"),
    }
}

#[test]
fn pai_que_sai_e_descendente_que_segura_os_pipes_estouram_e_sao_encerrados() {
    let marker = marker();
    let started = Instant::now();
    let saida =
        proc::saida_com_prazo(helper("pai-sai", Some(&marker.arquivo)), b"", PRAZO).unwrap();
    assert!(
        started.elapsed() < PRAZO + MARGEM,
        "retorno demorou {:?}",
        started.elapsed()
    );
    assert!(saida.estourou);
    assert!(
        saida.status.success(),
        "o status de saída do pai deve ser preservado"
    );
    assert!(contem(&saida.out, b"PAI-OUT"));
    assert!(contem(&saida.err, b"PAI-ERR"));
    assert!(contem(&saida.out, b"DESCENDENTE-OUT"));
    assert!(contem(&saida.err, b"DESCENDENTE-ERR"));
    let (pid, created) = dono(&marker.arquivo);
    aguardar_morto(pid, created);
}

#[test]
fn pai_vivo_e_descendente_sao_encerrados_no_prazo() {
    let marker = marker();
    let started = Instant::now();
    let saida =
        proc::saida_com_prazo(helper("pai-vivo", Some(&marker.arquivo)), b"", PRAZO).unwrap();
    assert!(
        started.elapsed() < PRAZO + MARGEM,
        "retorno demorou {:?}",
        started.elapsed()
    );
    assert!(saida.estourou);
    assert!(!saida.status.success());
    assert!(contem(&saida.out, b"DESCENDENTE-OUT"));
    assert!(contem(&saida.err, b"DESCENDENTE-ERR"));
    let (pid, created) = dono(&marker.arquivo);
    aguardar_morto(pid, created);
}

#[test]
fn escrita_de_entrada_grande_nao_prende_o_retorno() {
    let input = vec![b'I'; 16 * 1024 * 1024];
    let started = Instant::now();
    let saida = proc::saida_com_prazo(helper("sem-entrada", None), &input, PRAZO).unwrap();
    assert!(
        started.elapsed() < PRAZO + MARGEM,
        "retorno demorou {:?}",
        started.elapsed()
    );
    assert!(saida.estourou);
    assert!(!saida.status.success());
}

#[test]
fn leitura_de_stdout_e_stderr_grandes_acontece_em_paralelo() {
    let saida =
        proc::saida_com_prazo(helper("saida-grande", None), b"", Duration::from_secs(10)).unwrap();
    assert!(saida.status.success() && !saida.estourou);
    assert_eq!(
        saida.out.iter().filter(|&&byte| byte == b'O').count(),
        1024 * 1024
    );
    assert_eq!(
        saida.err.iter().filter(|&&byte| byte == b'E').count(),
        1024 * 1024
    );
}

fn helper(mode: &str, marker: Option<&Path>) -> Command {
    let executable = std::env::current_exe().unwrap();
    let mut cmd = proc::quiet(executable.to_string_lossy().as_ref());
    cmd.args(["--exact", "cli_auxiliar", "--nocapture"])
        .env(MODE, mode);
    if let Some(marker) = marker {
        cmd.env(READY, marker);
    }
    cmd
}

struct Marcador {
    diretorio: PathBuf,
    arquivo: PathBuf,
}

impl Drop for Marcador {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.arquivo);
        let _ = std::fs::remove_dir(&self.diretorio);
    }
}

fn marker() -> Marcador {
    loop {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let diretorio =
            std::env::temp_dir().join(format!("orq-prazo-{}-{unique}", std::process::id()));
        match std::fs::create_dir(&diretorio) {
            Ok(()) => {
                return Marcador {
                    arquivo: diretorio.join("descendente.pid"),
                    diretorio,
                };
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("não foi possível criar diretório temporário: {error}"),
        }
    }
}

fn aguardar_pronto(marker: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "descendente não criou o marcador"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn dono(marker: &Path) -> (u32, u64) {
    let contents = std::fs::read_to_string(marker).unwrap();
    let mut fields = contents.split_whitespace();
    let pid = fields.next().unwrap().parse().unwrap();
    let created = fields.next().unwrap().parse().unwrap();
    assert!(fields.next().is_none(), "marcador inválido: {contents:?}");
    (pid, created)
}

fn aguardar_morto(pid: u32, created: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if proc::owner_state(pid, created) == proc::Owner::Dead {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("o descendente {pid} continuou vivo após o retorno");
}

fn contem(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}
