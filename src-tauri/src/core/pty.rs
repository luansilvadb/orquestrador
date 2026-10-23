//! Um CLI rodando num PTY (ConPTY), com a saída guardada desde o início e numerada.
use crate::agents::cli::Spawn;
use crate::platform::Job;
use crate::proc::cli_do_job;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// Tudo o que um terminal já escreveu. `seq` conta os pedaços: retrato vazio é 0, o primeiro
/// `term-data` é 1.
#[derive(Default)]
pub struct Buffer {
    pub seq: u64,
    pub data: Vec<u8>,
}

pub type Shared = Arc<Mutex<Buffer>>;

pub struct Term {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    job: Option<Job>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { cols: cols.max(2), rows: rows.max(2), pixel_width: 0, pixel_height: 0 }
}

impl Term {
    /// `on_data(seq, bytes)` roda na thread de leitura, já com o pedaço guardado; `on_exit(código)`
    /// roda quando o processo termina.
    pub fn spawn(
        cmd: &Spawn,
        (cols, rows): (u16, u16),
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
                if bytes.is_empty() {
                    continue;
                }
                let mut buf = buffer.lock().unwrap();
                // Teto de memória: passou de 16 MB, a metade mais antiga sai.
                if buf.data.len() > 16 << 20 {
                    let half = buf.data.len() / 2;
                    buf.data.drain(..half);
                }
                buf.data.extend_from_slice(bytes);
                buf.seq += 1;
                on_data(buf.seq, bytes);
            }
        });
        std::thread::spawn(move || on_exit(child.wait().ok().map(|s| s.exit_code())));
        Ok(Term { master: pair.master, writer, job, killer })
    }

    pub fn write(&mut self, data: &[u8]) {
        let _ = self.writer.write_all(data).and_then(|_| self.writer.flush());
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
