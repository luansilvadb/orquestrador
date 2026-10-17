//! Processos: comandos sem janela, Git, o job que mata a árvore inteira e a prova de vida do dono
//! de uma sessão.
use crate::platform::{self, Job, Situacao};
use std::collections::HashSet;
use std::path::Path;
use std::io::Write;
use std::process::{Command, Stdio};

/// Um comando que não abre janela de console (o app é de janela; cada filho abriria a sua).
pub fn quiet(program: &str) -> Command {
    let mut cmd = Command::new(program);
    platform::sem_janela(&mut cmd);
    cmd
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

/// Roda o Git sem nada que espere digitação: sem pedido de credencial, sem editor. `entrada` vai pela entrada
/// padrão; sem ela, a entrada é nula. Sucesso devolve os bytes de (stdout, stderr); falha devolve o que o Git
/// escreveu (stderr e stdout, aparados e em UTF-8 com troca de byte inválido), ou o código de saída quando ele não
/// escreveu nada. Os caminhos são pathspec literal e o nome sai em UTF-8 sem aspas.
fn run(dir: &Path, args: &[&str], entrada: Option<&[u8]>) -> Result<(Vec<u8>, Vec<u8>), Falha> {
    run_as(dir, &["--literal-pathspecs"], args, entrada)
}

/// O `run` com as opções globais `globais` no lugar do `--literal-pathspecs`, que o `check-ignore` recusa.
fn run_as(dir: &Path, globais: &[&str], args: &[&str], entrada: Option<&[u8]>) -> Result<(Vec<u8>, Vec<u8>), Falha> {
    let mut cmd = quiet("git");
    cmd.arg("-C")
        .arg(dir)
        .args(globais)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", ":")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = match entrada {
        None => cmd.stdin(Stdio::null()).output(),
        Some(bytes) => cmd.stdin(Stdio::piped()).spawn().and_then(|mut child| {
            // Se o Git sair antes de ler tudo, o erro dele é o que importa: por isso a falha de escrita não é reportada.
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(bytes);
            }
            child.wait_with_output()
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

/// O Git com `entrada` pela entrada padrão, como o `git apply` que recebe o patch. Na falha, o erro é o de `run`.
pub fn git_entrada(dir: &Path, args: &[&str], entrada: &[u8]) -> Result<(), String> {
    run(dir, args, Some(entrada)).map(drop).map_err(String::from)
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
    use crate::testutil::Tmp;

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

    #[cfg(windows)]
    #[test]
    fn mesma_pasta_ignora_maiusculas_e_pasta_inexistente_nao_e() {
        let dir = &Tmp::new("orq-proc", "mesma").0;
        assert!(mesma_pasta(&dir, Path::new(&dir.to_string_lossy().to_uppercase())));
        assert!(!mesma_pasta(&dir, &dir.join("nao-existe")));
    }
}
