//! Um CLI rodando num PTY (ConPTY), com a saída guardada desde o início e numerada.
use crate::agents::cli::Spawn;
use crate::platform::Job;
use crate::proc::cli_do_job;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

/// Tudo o que um terminal já escreveu. `seq` conta os pedaços: retrato vazio é 0, o primeiro
/// `term-data` é 1.
#[derive(Default)]
pub struct Buffer {
    pub seq: u64,
    pub data: Vec<u8>,
}

pub type Shared = Arc<Mutex<Buffer>>;

/// Entrada do PTY, com lock próprio: quem escreve não passa pelo lock do `Core`. O `Term` é o dono; quem só escreve guarda
/// um `Weak`, que deixa de valer quando o terminal sai.
pub type Writer = Arc<Mutex<Box<dyn Write + Send>>>;
pub type WeakWriter = Weak<Mutex<Box<dyn Write + Send>>>;

/// Escreve em `writer`, com o flush. Erro de PTY fechado é ignorado, como sempre.
pub fn write_to(writer: &Writer, data: &[u8]) {
    let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());
    let _ = w.write_all(data).and_then(|_| w.flush());
}

/// Intervalo mínimo entre dois envios de um terminal com teclado. Depois de silêncio o primeiro pedaço sai na hora (o eco de
/// uma tecla não espera nada); só o que chega logo depois de um envio é agrupado, até o intervalo fechar, para uma saída
/// contínua não virar uma mensagem de IPC por leitura.
pub const INTERACTIVE_GAP: Duration = Duration::from_millis(4);
/// Executor: ninguém digita nele, então agrupa por ~2 quadros. Com N executores em paralelo, o webview recebe 30 mensagens/s
/// de cada um em vez de 250.
pub const READ_ONLY_GAP: Duration = Duration::from_millis(32);
/// Teto de um envio agrupado.
const MAX_BATCH: usize = 256 << 10;

pub struct Term {
    master: Box<dyn MasterPty + Send>,
    writer: Writer,
    job: Option<Job>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { cols: cols.max(2), rows: rows.max(2), pixel_width: 0, pixel_height: 0 }
}

impl Term {
    /// `on_data(seq, bytes)` roda na thread de leitura, já com o pedaço guardado; `on_exit(código)`
    /// roda quando o processo termina. `gap`: intervalo mínimo entre dois envios.
    pub fn spawn(
        cmd: &Spawn,
        (cols, rows): (u16, u16),
        gap: Duration,
        buffer: Shared,
        on_data: impl Fn(u64, &[u8]) + Send + 'static,
        on_exit: impl FnOnce(Option<u32>) + Send + 'static,
    ) -> Result<Term, String> {
        let fail = |e: String| format!("Não foi possível iniciar `{}`: {e}", cmd.program);
        let pair = native_pty_system().openpty(size(cols, rows)).map_err(|e| fail(e.to_string()))?;
        let mut builder = CommandBuilder::new(&cmd.program);
        builder.args(&cmd.args);
        builder.cwd(&cmd.cwd);
        for (key, value) in &cmd.env {
            builder.env(key, value);
        }
        let mut child = pair.slave.spawn_command(builder).map_err(|e| fail(format!("{e}. Confira se `{}` está instalado e no PATH", cmd.program)))?;
        drop(pair.slave);
        let job = child.process_id().and_then(Job::adopt);
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader().map_err(|e| fail(e.to_string()))?;
        let mut writer = pair.master.take_writer().map_err(|e| fail(e.to_string()))?;
        // Medido: o ConPTY abre pedindo a posição do cursor (ESC[6n) e o CLI não desenha nada
        // enquanto ninguém responde. Quem responde é o app, porque o teclado do frontend não
        // chega aos executores e o bloco pode nem estar montado; o pedido é tirado da saída para
        // o xterm não responder de novo.
        const QUERY: &[u8] = b"\x1b[6n";
        let _ = writer.write_all(b"\x1b[1;1R");

        // Duas threads: a de leitura só lê e repassa; a de envio guarda no buffer e entrega, agrupando o que chega com
        // o intervalo ainda aberto. O buffer e o `seq` seguem juntos, então um envio agrupado é um único `seq`.
        let (tx, rx) = channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 16384];
            let mut first = true;
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut bytes = &chunk[..n];
                if std::mem::take(&mut first) && bytes.starts_with(QUERY) {
                    bytes = &bytes[QUERY.len()..];
                }
                if !bytes.is_empty() && tx.send(bytes.to_vec()).is_err() {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let mut last: Option<Instant> = None;
            while let Ok(mut batch) = rx.recv() {
                let deadline = last.map_or_else(Instant::now, |t| t + gap);
                while batch.len() < MAX_BATCH {
                    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(more) => batch.extend_from_slice(&more),
                        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
                    }
                }
                let mut buf = buffer.lock().unwrap();
                // Teto de memória: passou de 16 MB, a metade mais antiga sai.
                if buf.data.len() > 16 << 20 {
                    let half = buf.data.len() / 2;
                    buf.data.drain(..half);
                }
                buf.data.extend_from_slice(&batch);
                buf.seq += 1;
                let seq = buf.seq;
                // O IPC sai sem o lock: o retrato não espera o envio. Um retrato no meio já contém o pedaço, e o `seq` o descarta no frontend.
                drop(buf);
                on_data(seq, &batch);
                last = Some(Instant::now());
            }
        });
        std::thread::spawn(move || on_exit(child.wait().ok().map(|s| s.exit_code())));
        Ok(Term { master: pair.master, writer: Arc::new(Mutex::new(writer)), job, killer })
    }

    /// Escrita direta, para quem tem o `Term`. O `Core` escreve por `writer`, sem o seu lock.
    pub fn write(&mut self, data: &[u8]) {
        write_to(&self.writer, data);
    }

    pub fn writer(&self) -> WeakWriter {
        Arc::downgrade(&self.writer)
    }

    /// O CLI (`claude` ou `codex`) que roda agora neste terminal, pelos processos dele (ver `cli_do_job`).
    pub fn cli(&self) -> Option<&'static str> {
        cli_do_job(self.job.as_ref()?)
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(size(cols, rows));
    }
}

/// Ao sair de escopo, mata a árvore inteira de processos.
impl Drop for Term {
    fn drop(&mut self) {
        match &self.job {
            Some(job) => job.kill(),
            None => drop(self.killer.kill()),
        }
    }
}
