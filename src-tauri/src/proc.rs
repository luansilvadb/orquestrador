//! Processos: comandos sem janela, Git, o job que mata a árvore inteira e a prova de vida do dono
//! de uma sessão.
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, FILETIME, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
    PROCESS_TERMINATE,
};

/// Um comando que não abre janela de console (o app é de janela; cada filho abriria a sua).
pub fn quiet(program: &str) -> Command {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Roda o Git sem nada que espere digitação: sem entrada, sem pedido de credencial, sem editor. Sucesso
/// devolve (stdout, stderr); falha devolve o que o Git escreveu (stderr e stdout, aparados), ou o código
/// de saída quando ele não escreveu nada. Os caminhos são pathspec literal e o nome sai em UTF-8 sem aspas.
fn run(dir: &Path, args: &[&str]) -> Result<(String, String), String> {
    let out = quiet("git")
        .arg("-C")
        .arg(dir)
        .args(["--literal-pathspecs", "-c", "core.quotepath=false"])
        .args(args)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", ":")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .output()
        .map_err(|e| format!("git não pôde ser executado: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        return Ok((stdout, stderr));
    }
    let why: Vec<&str> = [stderr.trim(), stdout.trim()].into_iter().filter(|s| !s.is_empty()).collect();
    Err(if why.is_empty() { format!("código {}", out.status.code().unwrap_or(-1)) } else { why.join("\n") })
}

/// A saída (stdout) do Git sem espera de digitação. Na falha, o erro traz o texto do Git.
pub fn git_saida(dir: &Path, args: &[&str]) -> Result<String, String> {
    run(dir, args).map(|(out, _)| out)
}

/// A saída completa do Git (stdout e stderr juntos), para mostrar na tela. Na falha, o erro traz o texto do Git.
pub fn git_completa(dir: &Path, args: &[&str]) -> Result<String, String> {
    run(dir, args).map(|(out, err)| out + &err)
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

/// Nome da branch atual; em HEAD destacado, o hash curto do commit. Erro se nem isso der para ler.
pub fn branch(dir: &Path) -> Result<String, String> {
    // `symbolic-ref` também acha a branch de um repositório sem commit; `-q` deixa o HEAD destacado sair com erro.
    if let Ok(name) = git_saida(dir, &["symbolic-ref", "--short", "-q", "HEAD"]) {
        return Ok(name.trim().to_string());
    }
    let hash = git_saida(dir, &["rev-parse", "--short", "HEAD"]).map_err(|why| format!("Não foi possível ler a branch: {why}"))?;
    Ok(hash.trim().to_string())
}

/// Job do Windows: tudo o que o processo criar morre junto, no `kill` ou quando o app fecha o
/// handle (inclusive ao morrer), então não sobra processo órfão.
pub struct Job(HANDLE);
unsafe impl Send for Job {}

impl Job {
    pub fn adopt(pid: u32) -> Option<Job> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let job = Job(job);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                return None;
            }
            let ok = AssignProcessToJobObject(job.0, process);
            CloseHandle(process);
            (ok != 0).then_some(job)
        }
    }

    pub fn kill(&self) {
        unsafe { TerminateJobObject(self.0, 1) };
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn filetime(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

/// Abre o processo só para consulta, lê com `ler` e fecha o handle em qualquer caso. `Err` traz o código
/// do Windows quando o processo não abre; `Ok(None)` quando abre mas `ler` não consegue.
fn consulta<T>(pid: u32, ler: impl FnOnce(HANDLE) -> Option<T>) -> Result<Option<T>, u32> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Err(GetLastError());
        }
        let lido = ler(process);
        CloseHandle(process);
        Ok(lido)
    }
}

/// Hora de criação de um processo aberto para consulta.
fn hora_de_criacao(process: HANDLE) -> Option<u64> {
    unsafe {
        let mut times: [FILETIME; 4] = std::mem::zeroed();
        let ok = GetProcessTimes(process, &mut times[0], &mut times[1], &mut times[2], &mut times[3]);
        (ok != 0).then(|| filetime(times[0]))
    }
}

/// Código de saída de um processo aberto para consulta: `STILL_ACTIVE` enquanto ele roda.
fn codigo_de_saida(process: HANDLE) -> Option<u32> {
    unsafe {
        let mut code = 0u32;
        (GetExitCodeProcess(process, &mut code) != 0).then_some(code)
    }
}

/// Hora de criação de um processo, no mesmo número que `StartTime.ToFileTimeUtc()` do PowerShell.
pub fn start_time(pid: u32) -> Option<u64> {
    consulta(pid, hora_de_criacao).ok().flatten()
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
    const STILL_ACTIVE: u32 = 259;
    match consulta(pid, |p| Some((hora_de_criacao(p)?, codigo_de_saida(p)?))) {
        Err(erro) if erro == ERROR_INVALID_PARAMETER => Owner::Dead,
        Err(_) | Ok(None) => Owner::Unknown,
        Ok(Some((criada, code))) if code == STILL_ACTIVE && criada == started => Owner::Alive,
        Ok(Some(_)) => Owner::Dead,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let dir = std::env::temp_dir().join(format!("orq-proc-{}-branch", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git_saida(&dir, &["init", "-q"]).unwrap();
        // Sem commit ainda: a branch já tem nome, mesmo sem nascer.
        git_saida(&dir, &["symbolic-ref", "HEAD", "refs/heads/trabalho"]).unwrap();
        assert_eq!(branch(&dir).unwrap(), "trabalho");
        git_saida(&dir, &["-c", "user.name=teste", "-c", "user.email=teste@teste", "commit", "--allow-empty", "-q", "-m", "x"]).unwrap();
        assert_eq!(branch(&dir).unwrap(), "trabalho");
        git_saida(&dir, &["checkout", "--detach", "-q"]).unwrap();
        let hash = git_saida(&dir, &["rev-parse", "--short", "HEAD"]).unwrap();
        assert_eq!(branch(&dir).unwrap(), hash.trim());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
