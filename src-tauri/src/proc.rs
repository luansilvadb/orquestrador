//! Processos: comandos sem janela, Git, o job que mata a árvore inteira e a prova de vida do dono
//! de uma sessão.
use crate::platform::{self, Job, Situacao};
use std::collections::HashSet;
use std::path::Path;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Um comando que não abre janela de console (o app é de janela; cada filho abriria a sua).
pub fn quiet(program: &str) -> Command {
    let mut cmd = Command::new(program);
    platform::sem_janela(&mut cmd);
    cmd
}

/// A saída de um comando rodado com prazo.
pub struct Saida {
    pub status: std::process::ExitStatus,
    pub out: Vec<u8>,
    pub err: Vec<u8>,
    /// O prazo estourou e a árvore foi encerrada: `out` e `err` trazem o que saiu até ali.
    pub estourou: bool,
}

/// Por que o comando com prazo não deixou saída: ele não iniciou, não pôde ser aguardado (ou encerrado), ou a saída
/// não pôde ser lida. Quem chama põe o nome do programa na mensagem.
#[derive(Debug)]
pub enum FalhaPrazo {
    Iniciar(io::Error),
    Aguardar(io::Error),
    Ler(io::Error),
}

/// Roda `cmd` com `entrada` na entrada padrão e um prazo para execução, escrita e drenagem dos pipes. Leitores e
/// escritor correm em paralelo; no estouro, a árvore é encerrada e os bytes capturados voltam com `estourou`.
pub fn saida_com_prazo(mut cmd: Command, entrada: &[u8], prazo: Duration) -> Result<Saida, FalhaPrazo> {
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let started = Instant::now();
    let (mut child, job) = platform::spawn_com_prazo(&mut cmd).map_err(FalhaPrazo::Iniciar)?;
    // Os três pipes foram pedidos `piped`: as tomadas existem.
    let mut stdin = child.stdin.take().expect("stdin pedido com pipe");
    let stdout = child.stdout.take().expect("stdout pedido com pipe");
    let stderr = child.stderr.take().expect("stderr pedido com pipe");
    let bytes = entrada.to_vec();
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(&bytes);
    });
    let out_reader = thread::spawn(move || ler_pipe(stdout));
    let err_reader = thread::spawn(move || ler_pipe(stderr));
    let mut status = None;
    let estourou = loop {
        if started.elapsed() >= prazo {
            job.kill();
            let _ = child.kill();
            if status.is_none() {
                match child.wait() {
                    Ok(exit) => status = Some(exit),
                    Err(error) => {
                        let _ = juntar_saidas(writer, out_reader, err_reader);
                        return Err(FalhaPrazo::Aguardar(error));
                    }
                }
            }
            break true;
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(exit) => status = exit,
                Err(error) => {
                    job.kill();
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = juntar_saidas(writer, out_reader, err_reader);
                    return Err(FalhaPrazo::Aguardar(error));
                }
            }
        }
        if status.is_some() && writer.is_finished() && out_reader.is_finished() && err_reader.is_finished() {
            break false;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let (out, err) = juntar_saidas(writer, out_reader, err_reader)?;
    let status = status.expect("o processo terminou ou foi aguardado");
    Ok(Saida { status, out, err, estourou })
}

fn juntar_saidas(
    writer: thread::JoinHandle<()>,
    out_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    err_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
) -> Result<(Vec<u8>, Vec<u8>), FalhaPrazo> {
    let _ = writer.join();
    let out = out_reader.join().map_err(leitura_caiu).and_then(|result| result.map_err(FalhaPrazo::Ler));
    let err = err_reader.join().map_err(leitura_caiu).and_then(|result| result.map_err(FalhaPrazo::Ler));
    match (out, err) {
        (Ok(out), Ok(err)) => Ok((out, err)),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

/// A thread de leitura caiu (pânico): não há saída para devolver.
fn leitura_caiu(_: Box<dyn std::any::Any + Send>) -> FalhaPrazo {
    FalhaPrazo::Ler(io::Error::other("a leitura da saída do comando foi interrompida"))
}

fn ler_pipe(mut pipe: impl Read) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    pipe.read_to_end(&mut data)?;
    Ok(data)
}

/// Por que o Git não deu saída: ele nem começou (não está instalado ou não está no PATH), ou respondeu com erro.
#[derive(Debug)]
pub enum Falha {
    Ausente(String),
    Erro(String),
}

impl From<Falha> for String {
    fn from(falha: Falha) -> String {
        match falha {
            Falha::Ausente(texto) | Falha::Erro(texto) => texto,
        }
    }
}

/// O comando do Git em `dir` com as opções globais `globais`, o nome em UTF-8 sem aspas e o ambiente sem espera de
/// digitação. A entrada e a saída ficam com quem chama.
fn comando(dir: &Path, globais: &[&str], args: &[&str]) -> Command {
    let mut cmd = quiet("git");
    cmd.arg("-C")
        .arg(dir)
        .args(globais)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        // O status periódico não regrava o índice: sem isso, ele segura o `index.lock` e o `git commit` de um agente falha.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_EDITOR", ":")
        .env("GIT_MERGE_AUTOEDIT", "no");
    cmd
}

/// Roda o Git sem nada que espere digitação: sem pedido de credencial, sem editor. `entrada` vai pela entrada
/// padrão; sem ela, a entrada é nula. Sucesso devolve os bytes de (stdout, stderr); falha devolve o que o Git
/// escreveu (stderr e stdout, aparados e em UTF-8 com troca de byte inválido), ou o código de saída quando ele não
/// escreveu nada. Os caminhos são pathspec literal e o nome sai em UTF-8 sem aspas.
fn run(dir: &Path, args: &[&str], entrada: Option<&[u8]>) -> Result<(Vec<u8>, Vec<u8>), Falha> {
    run_as(dir, &["--literal-pathspecs"], args, entrada)
}

/// O `run` com as opções globais `globais` no lugar do `--literal-pathspecs`, que o `check-ignore` recusa.
fn run_as(dir: &Path, globais: &[&str], args: &[&str], entrada: Option<&[u8]>) -> Result<(Vec<u8>, Vec<u8>), Falha> {
    let mut cmd = comando(dir, globais, args);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = match entrada {
        None => cmd.stdin(Stdio::null()).output(),
        Some(bytes) => cmd.stdin(Stdio::piped()).spawn().and_then(|mut child| {
            // A escrita corre em outra thread enquanto a saída é lida: escrever tudo antes trava quando a saída enche o
            // pipe. Se o Git sair antes de ler tudo, o erro dele é o que importa: por isso a falha de escrita não é reportada.
            let writer = child.stdin.take().map(|mut stdin| {
                let bytes = bytes.to_vec();
                std::thread::spawn(move || {
                    let _ = stdin.write_all(&bytes);
                })
            });
            let out = child.wait_with_output();
            if let Some(writer) = writer {
                let _ = writer.join();
            }
            out
        }),
    }
    .map_err(|e| Falha::Ausente(format!("git não pôde ser executado: {e}")))?;
    if out.status.success() {
        return Ok((out.stdout, out.stderr));
    }
    let why: Vec<String> = [&out.stderr, &out.stdout].into_iter().map(|bytes| String::from_utf8_lossy(bytes).trim().to_string()).filter(|s| !s.is_empty()).collect();
    Err(Falha::Erro(if why.is_empty() { format!("código {}", out.status.code().unwrap_or(-1)) } else { why.join("\n") }))
}

/// A saída (stdout) do Git sem espera de digitação. Na falha, o erro traz o texto do Git.
pub fn git_saida(dir: &Path, args: &[&str]) -> Result<String, String> {
    git_falha(dir, args).map_err(String::from)
}

/// Igual a `git_saida`, mas a falha diz se o Git nem começou (`Ausente`) ou se ele recusou (`Erro`).
pub fn git_falha(dir: &Path, args: &[&str]) -> Result<String, Falha> {
    run(dir, args, None).map(|(out, _)| String::from_utf8_lossy(&out).into_owned())
}

/// A saída completa do Git (stdout e stderr juntos), para mostrar na tela. Na falha, o erro traz o texto do Git.
pub fn git_completa(dir: &Path, args: &[&str]) -> Result<String, String> {
    run(dir, args, None).map(|(out, err)| String::from_utf8_lossy(&out).into_owned() + &String::from_utf8_lossy(&err)).map_err(String::from)
}

/// A saída (stdout) do Git em bytes crus, sem trocar byte inválido: para saber se um conteúdo é UTF-8. Na falha, o erro é o de `run`.
pub fn git_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    run(dir, args, None).map(|(out, _)| out).map_err(String::from)
}

/// O `git_bytes` que lê no máximo `limite` bytes da saída: com `limite` bytes lidos, o Git é encerrado e volta só esse
/// começo. Mede uma saída grande sem guardá-la inteira. O comando é o de `run`.
pub fn git_bytes_ate(dir: &Path, args: &[&str], limite: usize) -> Result<Vec<u8>, String> {
    let mut cmd = comando(dir, &["--literal-pathspecs"], args);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let ausente = |e: std::io::Error| format!("git não pôde ser executado: {e}");
    let mut child = cmd.spawn().map_err(ausente)?;
    // O stderr é lido em outra thread: cheio, ele pararia o Git antes de a saída chegar ao limite.
    let erro = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.read_to_end(&mut bytes);
            bytes
        })
    });
    let mut out = Vec::new();
    let lido = child.stdout.take().map_or(Ok(0), |stdout| stdout.take(limite as u64).read_to_end(&mut out));
    let cortado = out.len() >= limite;
    if cortado {
        let _ = child.kill();
    }
    let status = child.wait().map_err(ausente)?;
    let err = erro.and_then(|t| t.join().ok()).unwrap_or_default();
    lido.map_err(|e| format!("Não foi possível ler a saída do git: {e}"))?;
    if cortado || status.success() {
        return Ok(out);
    }
    let why: Vec<String> = [&err, &out].into_iter().map(|bytes| String::from_utf8_lossy(bytes).trim().to_string()).filter(|s| !s.is_empty()).collect();
    Err(if why.is_empty() { format!("código {}", status.code().unwrap_or(-1)) } else { why.join("\n") })
}

/// O Git com `entrada` pela entrada padrão, como o `git apply` que recebe o patch. Na falha, o erro é o de `run`.
pub fn git_entrada(dir: &Path, args: &[&str], entrada: &[u8]) -> Result<(), String> {
    run(dir, args, Some(entrada)).map(drop).map_err(String::from)
}

/// O `git_entrada` que devolve a saída (stdout), como o hash do `git hash-object --stdin`. Na falha, o erro é o de `run`.
pub fn git_entrada_saida(dir: &Path, args: &[&str], entrada: &[u8]) -> Result<String, String> {
    run(dir, args, Some(entrada)).map(|(out, _)| String::from_utf8_lossy(&out).into_owned()).map_err(String::from)
}

/// Dos caminhos dados (relativos a `dir`, com "/"), os que o `.gitignore` ignora, num `git check-ignore` só. O Git sai
/// com 1 quando nenhum é ignorado: isso não é falha. Fora de um repositório, o erro é o do Git.
pub fn git_ignorados(dir: &Path, paths: &[String]) -> Result<HashSet<String>, String> {
    let entrada: Vec<u8> = paths.iter().flat_map(|p| p.bytes().chain([0])).collect();
    match run_as(dir, &[], &["check-ignore", "-z", "--stdin"], Some(&entrada)) {
        Ok((out, _)) => Ok(out.split(|&b| b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect()),
        Err(Falha::Erro(why)) if why == "código 1" => Ok(HashSet::new()),
        Err(falha) => Err(falha.into()),
    }
}

/// Pasta -> raiz do repositório Git (com barras invertidas), ou `None` fora de um work tree.
pub fn git_root(folder: &Path) -> Option<String> {
    if !folder.is_dir() {
        return None;
    }
    let top = git_saida(folder, &["rev-parse", "--show-toplevel"]).ok()?;
    let top = top.trim();
    (!top.is_empty()).then(|| top.replace('/', "\\"))
}

/// Se os dois caminhos são a mesma pasta. Compara o caminho real sem diferença de maiúsculas, porque o Git e o
/// diálogo escrevem a mesma pasta de jeitos diferentes no Windows. Pasta que não existe não é a mesma.
pub fn mesma_pasta(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase(),
        _ => false,
    }
}

/// Nome da branch atual; em HEAD destacado, o hash curto do commit. Erro se nem isso der para ler.
pub fn branch(dir: &Path) -> Result<String, String> {
    // `symbolic-ref` também acha a branch de um repositório sem commit; `-q` deixa o HEAD destacado sair com erro.
    if let Ok(name) = git_saida(dir, &["symbolic-ref", "--short", "-q", "HEAD"]) {
        return Ok(name.trim().to_string());
    }
    let hash = git_saida(dir, &["rev-parse", "--short", "HEAD"]).map_err(|why| format!("Não foi possível ler a branch: {why}"))?;
    Ok(hash.trim().to_string())
}

/// O CLI interativo (`claude` ou `codex`) que roda agora no job, pelo executável de cada processo vivo dele. Com
/// os dois (um chamou o outro), vale o que começou primeiro: é o de fora, dono do terminal. `None` sem nenhum.
pub fn cli_do_job(job: &Job) -> Option<&'static str> {
    job.pids()?.into_iter().filter_map(platform::cli_do_processo).min_by_key(|&(_, criada)| criada).map(|(cli, _)| cli)
}

#[derive(Debug, PartialEq)]
pub enum Owner {
    Alive,
    Dead,
    Unknown,
}

/// Um PID sozinho não prova nada, o Windows reaproveita: só é o mesmo dono se a hora de início
/// também bater. Processo que não pode ser lido é `Unknown`.
pub fn owner_state(pid: u32, started: u64) -> Owner {
    match platform::situacao_do_processo(pid) {
        Situacao::Inexistente => Owner::Dead,
        Situacao::Ilegivel => Owner::Unknown,
        Situacao::Lido { criada, rodando } if rodando && criada == started => Owner::Alive,
        Situacao::Lido { .. } => Owner::Dead,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use crate::platform::start_time;
    use crate::testutil::{git, write, Tmp};

    #[test]
    fn saida_com_prazo_devolve_o_que_o_comando_escreveu() {
        let mut cmd = quiet("git");
        cmd.args(["--version"]);
        let saida = saida_com_prazo(cmd, b"", Duration::from_secs(10)).unwrap();
        assert!(saida.status.success() && !saida.estourou, "estourou: {}", saida.estourou);
        assert!(String::from_utf8_lossy(&saida.out).contains("git version"));
    }

    /// Um comando que passa do prazo é morto na hora: `saida_com_prazo` não espera o filho terminar.
    #[test]
    fn comando_que_passa_do_prazo_e_morto_e_volta_sem_esperar() {
        #[cfg(windows)]
        let cmd = {
            let mut cmd = quiet("ping");
            cmd.args(["-n", "30", "127.0.0.1"]);
            cmd
        };
        #[cfg(not(windows))]
        let cmd = {
            let mut cmd = quiet("sleep");
            cmd.arg("30");
            cmd
        };
        let started = Instant::now();
        let saida = saida_com_prazo(cmd, b"", Duration::from_millis(300)).unwrap();
        assert!(saida.estourou, "o prazo tinha de estourar");
        assert!(!saida.status.success(), "o processo foi morto");
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    }

    #[cfg(windows)]
    #[test]
    fn dono_vivo_morto_e_pid_reaproveitado() {
        let me = std::process::id();
        let started = start_time(me).unwrap();
        assert_eq!(owner_state(me, started), Owner::Alive);
        assert_eq!(owner_state(me, started + 1), Owner::Dead);
        let mut child = quiet("cmd").args(["/c", "exit"]).spawn().unwrap();
        let (pid, t) = (child.id(), start_time(child.id()).unwrap());
        child.wait().unwrap();
        drop(child);
        assert_eq!(owner_state(pid, t), Owner::Dead);
    }

    #[cfg(windows)]
    #[test]
    fn job_mata_a_arvore() {
        // cmd -> ping: matar o job leva os dois.
        let mut child = quiet("cmd").args(["/c", "ping -n 30 127.0.0.1 > nul"]).spawn().unwrap();
        let job = Job::adopt(child.id()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        job.kill();
        assert!(!child.wait().unwrap().success());
    }

    #[test]
    fn branch_le_o_nome_e_em_head_destacado_o_hash() {
        let dir = &Tmp::new("orq-proc", "branch").0;
        git_saida(&dir, &["init", "-q"]).unwrap();
        // Sem commit ainda: a branch já tem nome, mesmo sem nascer.
        git_saida(&dir, &["symbolic-ref", "HEAD", "refs/heads/trabalho"]).unwrap();
        assert_eq!(branch(&dir).unwrap(), "trabalho");
        git_saida(&dir, &["-c", "user.name=teste", "-c", "user.email=teste@teste", "commit", "--allow-empty", "-q", "-m", "x"]).unwrap();
        assert_eq!(branch(&dir).unwrap(), "trabalho");
        git_saida(&dir, &["checkout", "--detach", "-q"]).unwrap();
        let hash = git_saida(&dir, &["rev-parse", "--short", "HEAD"]).unwrap();
        assert_eq!(branch(&dir).unwrap(), hash.trim());
    }

    #[test]
    fn ignorados_com_entrada_grande_termina() {
        let dir = &Tmp::new("orq-proc", "ignorados").0;
        git(dir, &["init", "-q"]);
        write(dir, ".gitignore", "ignorada/\n");
        // Saída maior que o buffer do pipe: travava quando a entrada era escrita inteira antes da leitura.
        let paths: Vec<String> = (0..5000).map(|i| format!("ignorada/arquivo-{i}.txt")).collect();
        let ignorados = git_ignorados(dir, &paths).unwrap();
        assert_eq!(ignorados.len(), 5000);
        assert!(paths.iter().all(|p| ignorados.contains(p)));
    }

    #[test]
    fn leitura_limitada_corta_a_saida_e_traz_o_erro_do_git() {
        let dir = &Tmp::new("orq-proc", "limitada").0;
        git(dir, &["init", "-q"]);
        write(dir, "a.txt", "x".repeat(10_000));
        let objeto = git(dir, &["hash-object", "-w", "a.txt"]);
        let objeto = objeto.trim();
        assert_eq!(git_bytes_ate(dir, &["cat-file", "blob", objeto], 100).unwrap(), vec![b'x'; 100]);
        assert_eq!(git_bytes_ate(dir, &["cat-file", "blob", objeto], 20_000).unwrap().len(), 10_000);
        assert!(git_bytes_ate(dir, &["cat-file", "blob", "nao-existe"], 100).unwrap_err().contains("nao-existe"));
    }

    #[cfg(windows)]
    #[test]
    fn mesma_pasta_ignora_maiusculas_e_pasta_inexistente_nao_e() {
        let dir = &Tmp::new("orq-proc", "mesma").0;
        assert!(mesma_pasta(&dir, Path::new(&dir.to_string_lossy().to_uppercase())));
        assert!(!mesma_pasta(&dir, &dir.join("nao-existe")));
    }
}
