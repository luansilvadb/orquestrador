//! Processos: comandos sem janela, Git, o job que mata a árvore inteira e a prova de vida do dono
//! de uma sessão.
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
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

/// A saída do Git lida como UTF-8, ou o motivo da falha.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = quiet("git").arg("-C").arg(dir).args(args).output().map_err(|e| format!("git não pôde ser executado: {e}"))?;
    if !out.status.success() {
        return Err(format!("código {}", out.status.code().unwrap_or(-1)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Pasta -> raiz do repositório Git (com barras invertidas), ou `None` fora de um work tree.
pub fn git_root(folder: &Path) -> Option<String> {
    if !folder.is_dir() {
        return None;
    }
    let top = git(folder, &["rev-parse", "--show-toplevel"]).ok()?;
    let top = top.trim();
    (!top.is_empty()).then(|| top.replace('/', "\\"))
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

/// Hora de criação de um processo, no mesmo número que `StartTime.ToFileTimeUtc()` do PowerShell.
pub fn start_time(pid: u32) -> Option<u64> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut times: [FILETIME; 4] = std::mem::zeroed();
        let ok = GetProcessTimes(process, &mut times[0], &mut times[1], &mut times[2], &mut times[3]);
        CloseHandle(process);
        (ok != 0).then(|| filetime(times[0]))
    }
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
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return if GetLastError() == ERROR_INVALID_PARAMETER { Owner::Dead } else { Owner::Unknown };
        }
        let mut times: [FILETIME; 4] = std::mem::zeroed();
        let mut code = 0u32;
        let ok = GetProcessTimes(process, &mut times[0], &mut times[1], &mut times[2], &mut times[3]) != 0
            && GetExitCodeProcess(process, &mut code) != 0;
        CloseHandle(process);
        match ok {
            false => Owner::Unknown,
            true if code == STILL_ACTIVE && filetime(times[0]) == started => Owner::Alive,
            true => Owner::Dead,
        }
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
}
