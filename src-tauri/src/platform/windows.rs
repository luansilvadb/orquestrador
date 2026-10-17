//! As primitivas do Windows: Job Objects, consulta de processo, criação sem janela, COM e a Lixeira, o Explorer, o
//! clipboard, a memória física e as pastas do usuário. Cada função é a chamada da API; a regra do app fica fora.
use super::{ErroShell, Situacao, Snapshot};
use std::ffi::{c_void, OsStr};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GlobalFree, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_INVALID_PARAMETER, ERROR_SHARING_VIOLATION,
    FILETIME, HANDLE, POINT, STILL_ACTIVE,
};
use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, FILE_SHARE_READ};
use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, CREATE_NO_WINDOW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::Shell::{
    DragQueryFileW, SHCreateItemFromParsingName, DROPFILES, FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION,
    FOF_NOERRORUI, FOF_RENAMEONCOLLISION, FOF_SILENT,
};

/// Os PowerShells que o shell do usuário tenta, nesta ordem: o primeiro que iniciar vale.
pub const SHELLS: [&str; 2] = ["pwsh.exe", "powershell.exe"];

/// Um comando que não abre janela de console (o app é de janela; cada filho abriria a sua).
pub fn sem_janela(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
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

    /// Os PIDs que estão no job agora. `None` se a lista não couber ou não puder ser lida.
    pub fn pids(&self) -> Option<Vec<u32>> {
        // ponytail: lista de até 512 processos; um job maior que isso dá `None` (o terminal cola como shell comum).
        let mut buf = vec![0usize; 1 + 512];
        let list = buf.as_mut_ptr() as *mut JOBOBJECT_BASIC_PROCESS_ID_LIST;
        let size = (buf.len() * std::mem::size_of::<usize>()) as u32;
        unsafe {
            if QueryInformationJobObject(self.0, JobObjectBasicProcessIdList, list as *mut _, size, std::ptr::null_mut()) == 0 {
                return None;
            }
            let pids = std::slice::from_raw_parts((*list).ProcessIdList.as_ptr(), (*list).NumberOfProcessIdsInList as usize);
            Some(pids.iter().map(|&pid| pid as u32).collect())
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// `claude` ou `codex` e a hora de início do processo, quando o executável dele é `claude.exe` ou `codex.exe`.
pub fn cli_do_processo(pid: u32) -> Option<(&'static str, u64)> {
    consulta(pid, |p| Some((cli_do_executavel(p)?, hora_de_criacao(p)?))).ok().flatten()
}

/// `claude` ou `codex` quando o executável do processo aberto para consulta é `claude.exe` ou `codex.exe`.
fn cli_do_executavel(process: HANDLE) -> Option<&'static str> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) } == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    match Path::new(&path).file_name()?.to_string_lossy().to_lowercase().as_str() {
        "claude.exe" => Some("claude"),
        "codex.exe" => Some("codex"),
        _ => None,
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

/// O que se sabe do processo `pid`: se não existe, se não pode ser lido, ou a hora de início e se ainda roda.
pub fn situacao_do_processo(pid: u32) -> Situacao {
    match consulta(pid, |p| Some((hora_de_criacao(p)?, codigo_de_saida(p)?))) {
        Err(erro) if erro == ERROR_INVALID_PARAMETER => Situacao::Inexistente,
        Err(_) | Ok(None) => Situacao::Ilegivel,
        Ok(Some((criada, code))) => Situacao::Lido { criada, rodando: code == STILL_ACTIVE as u32 },
    }
}

/// Memória física em bytes: (total, livre agora). `None` se a API falhar.
pub fn memoria() -> Option<(u64, u64)> {
    // SAFETY: MEMORYSTATUSEX é só números; a estrutura é zerada e `dwLength` é o tamanho dela, como a API exige.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: a API só escreve dentro da estrutura passada, que vive até a chamada acabar.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return None;
    }
    Some((status.ullTotalPhys, status.ullAvailPhys))
}

/// `%LOCALAPPDATA%`, se estiver definido e não for vazio.
pub fn pasta_local_do_usuario() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// `%USERPROFILE%`; vazio se não estiver definido.
pub fn pasta_pessoal() -> PathBuf {
    PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default())
}

/// Muda o nome (ou move) sem sobrescrever: `MoveFileExW` sem `MOVEFILE_REPLACE_EXISTING` falha se o destino existir
/// (inclusive um link). O `std::fs::rename` do Windows sobrescreve, por isso não é usado.
pub fn mover_sem_substituir(from: &Path, to: &Path) -> io::Result<()> {
    let (src, dst) = (wide(from.as_os_str()), wide(to.as_os_str()));
    if unsafe { MoveFileExW(src.as_ptr(), dst.as_ptr(), 0) } != 0 {
        return Ok(());
    }
    Err(io::Error::last_os_error())
}

/// O destino já existe (arquivo, pasta ou link).
pub fn ja_existe(erro: &io::Error) -> bool {
    matches!(erro.raw_os_error().map(|c| c as u32), Some(ERROR_ALREADY_EXISTS) | Some(ERROR_FILE_EXISTS))
}

/// Outro programa está gravando o arquivo agora.
pub fn em_uso(erro: &io::Error) -> bool {
    erro.raw_os_error().map(|c| c as u32) == Some(ERROR_SHARING_VIOLATION)
}

/// Cria um link simbólico de arquivo. Só os testes usam; no Windows exige privilégio.
pub fn symlink_arquivo(alvo: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(alvo, link)
}

/// Abre `path` para ler e gravar sem `FILE_SHARE_WRITE`: outro programa não consegue gravar nele enquanto o `File` vive.
pub fn abrir_para_gravar(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ).open(path)
}

/// Abre o Explorer com `path` selecionado.
pub fn revelar_no_explorador(path: &Path) -> io::Result<()> {
    abrir_cru("explorer.exe", &format!("/select,\"{}\"", path.display()))
}

/// Abre o Explorer na pasta `path`.
pub fn abrir_pasta_no_explorador(path: &Path) -> io::Result<()> {
    abrir_cru("explorer.exe", &format!("\"{}\"", path.display()))
}

/// Abre o diálogo "Como você quer abrir este arquivo?" do Windows, o mesmo do Abrir com... do Explorer.
pub fn dialogo_abrir_com(path: &Path) -> io::Result<()> {
    abrir_cru("rundll32.exe", &format!("shell32.dll,OpenAs_RunDLL \"{}\"", path.display()))
}

/// Abre `programa` sem janela com `arg` como linha de comando crua. O código de saída não é lido: o Explorer sai com 1
/// mesmo quando funciona.
fn abrir_cru(programa: &str, arg: &str) -> io::Result<()> {
    let mut cmd = Command::new(programa);
    sem_janela(&mut cmd);
    cmd.raw_arg(arg).spawn().map(drop)
}

/// Manda `file` (arquivo ou pasta já validada) para a Lixeira pelo `IFileOperation` com `FOFX_RECYCLEONDELETE`: a operação
/// pede reciclagem e não tem o caminho de apagar de vez. Não dá para mudar isso com `SHFileOperationW`, que aceita a
/// exclusão definitiva por diálogo.
pub fn mandar_para_lixeira(file: &Path) -> Result<(), ErroShell> {
    let _apartment = Apartment::enter()?;
    let flags = FOFX_RECYCLEONDELETE | FOF_ALLOWUNDO | FOF_SILENT | FOF_NOERRORUI;
    // SAFETY: as chamadas COM são feitas na thread que acabou de inicializar o COM, e cada `Com` solta a sua referência.
    unsafe {
        run_operation(flags, |vtbl, op| {
            let item = shell_item(file)?;
            Ok(((vtbl.delete_item)(op, item.0, std::ptr::null_mut()), vec![item]))
        })
    }
}

/// Copia ou move `src` para dentro de `parent` com o nome `name`, pelo IFileOperation. Sem confirmação e com renomeação
/// em conflito: nada existente é sobrescrito; um conflito que apareça na hora ganha outro nome.
pub fn copiar_ou_mover(src: &Path, parent: &Path, name: &str, cut: bool) -> Result<(), ErroShell> {
    let _apartment = Apartment::enter()?;
    let name_w = wide(OsStr::new(name));
    let flags = FOF_RENAMEONCOLLISION | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI;
    // SAFETY: as chamadas COM são feitas na thread que acabou de inicializar o COM, e cada `Com` solta a sua referência.
    unsafe {
        run_operation(flags, |vtbl, op| {
            let item = shell_item(src)?;
            let folder = shell_item(parent)?;
            let hr = if cut {
                (vtbl.move_item)(op, item.0, folder.0, name_w.as_ptr(), std::ptr::null_mut())
            } else {
                (vtbl.copy_item)(op, item.0, folder.0, name_w.as_ptr(), std::ptr::null_mut())
            };
            Ok((hr, vec![item, folder]))
        })
    }
}

const CLSID_FILE_OPERATION: GUID = GUID { data1: 0x3AD05575, data2: 0x8857, data3: 0x4850, data4: [0x92, 0x77, 0x11, 0xB8, 0x5B, 0xDB, 0x8E, 0x09] };
const IID_IFILE_OPERATION: GUID = GUID { data1: 0x947AAB5F, data2: 0x0A5C, data3: 0x4C13, data4: [0xB4, 0xD6, 0x4B, 0xF7, 0x83, 0x6F, 0xC9, 0xF8] };
const IID_ISHELL_ITEM: GUID = GUID { data1: 0x43826D1E, data2: 0xE718, data3: 0x42EE, data4: [0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE] };

/// Vtable de IUnknown: o começo de todo objeto COM. `release` serve também ao `IShellItem` e ao `IFileOperation`.
#[repr(C)]
struct Unknown {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// Vtable de `IFileOperation` (shobjidl_core.h), na ordem da interface. Só `SetOperationFlags`, `MoveItem`, `CopyItem`,
/// `DeleteItem`, `PerformOperations` e `GetAnyOperationsAborted` são chamados; os demais slots são só espaço, com o mesmo tamanho de um ponteiro.
#[repr(C)]
struct FileOperationVtbl {
    base: Unknown,
    advise: usize,
    unadvise: usize,
    set_operation_flags: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    set_progress_message: usize,
    set_progress_dialog: usize,
    set_properties: usize,
    set_owner_window: usize,
    apply_properties_to_item: usize,
    apply_properties_to_items: usize,
    rename_item: usize,
    rename_items: usize,
    move_item: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const u16, *mut c_void) -> HRESULT,
    move_items: usize,
    copy_item: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const u16, *mut c_void) -> HRESULT,
    copy_items: usize,
    delete_item: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
    delete_items: usize,
    new_item: usize,
    perform_operations: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    get_any_operations_aborted: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
}

/// Um ponteiro COM que solta a referência ao sair do escopo.
struct Com(*mut c_void);

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: o ponteiro veio de uma chamada COM bem-sucedida e esta struct é a dona da referência.
        unsafe { ((**(self.0 as *const *const Unknown)).release)(self.0) };
    }
}

/// Roda uma IFileOperation com `flags`. `add` pede as ações sobre o vtable e devolve o HRESULT delas com os `IShellItem`
/// usados, que ficam vivos até a execução. Depois executa e confere o aborto: só `Ok` se tudo deu certo.
unsafe fn run_operation(flags: u32, add: impl FnOnce(&FileOperationVtbl, *mut c_void) -> Result<(HRESULT, Vec<Com>), ErroShell>) -> Result<(), ErroShell> {
    let mut op: *mut c_void = std::ptr::null_mut();
    let hr = CoCreateInstance(&CLSID_FILE_OPERATION, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_IFILE_OPERATION, &mut op);
    if hr < 0 || op.is_null() {
        return Err(ErroShell::SemOperacao);
    }
    let op = Com(op);
    let vtbl = &**(op.0 as *const *const FileOperationVtbl);
    let flagged = (vtbl.set_operation_flags)(op.0, flags);
    let (mut hr, _items) = if flagged >= 0 { add(vtbl, op.0)? } else { (flagged, Vec::new()) };
    if hr >= 0 {
        hr = (vtbl.perform_operations)(op.0);
    }
    let mut aborted = 0;
    let aborted_hr = (vtbl.get_any_operations_aborted)(op.0, &mut aborted);
    if hr < 0 || aborted_hr < 0 || aborted != 0 {
        return Err(ErroShell::Recusada);
    }
    Ok(())
}

/// Um `IShellItem` para o caminho `path` do sistema de arquivos. O Shell recusa `/`, então as barras viram `\` aqui.
unsafe fn shell_item(path: &Path) -> Result<Com, ErroShell> {
    let mut wide = wide(path.as_os_str());
    for unit in wide.iter_mut().filter(|u| **u == b'/' as u16) {
        *unit = b'\\' as u16;
    }
    let mut item: *mut c_void = std::ptr::null_mut();
    let hr = SHCreateItemFromParsingName(wide.as_ptr(), std::ptr::null_mut(), &IID_ISHELL_ITEM, &mut item);
    if hr < 0 || item.is_null() {
        return Err(ErroShell::ItemAusente);
    }
    Ok(Com(item))
}

/// Caminho com zero final, para as APIs do Windows.
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Inicializa o COM na thread que chama e desfaz ao sair, inclusive quando a chamada já o tinha inicializado.
struct Apartment;

impl Apartment {
    fn enter() -> Result<Apartment, ErroShell> {
        // SAFETY: COM é inicializado na própria thread, e o Drop chama `CoUninitialize` para cada chamada bem-sucedida.
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        if hr < 0 {
            return Err(ErroShell::SemCom);
        }
        Ok(Apartment)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: par do `CoInitializeEx` de `enter`.
        unsafe { CoUninitialize() };
    }
}

/// Abre o clipboard. Outro programa pode estar com ele aberto por um instante, então tenta algumas vezes.
fn open() -> bool {
    (0..10).any(|_| {
        let ok = unsafe { OpenClipboard(std::ptr::null_mut()) } != 0;
        if !ok {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        ok
    })
}

const CF_HDROP: u32 = 15;
const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_MOVE: u32 = 2;
const PREFERRED_DROP_EFFECT: &str = "Preferred DropEffect";

/// Os arquivos e o efeito (copiar ou recortar) do clipboard, lidos na mesma abertura. Vazio quando o clipboard
/// não abre ou não tem arquivo.
pub fn snapshot() -> Snapshot {
    if !open() {
        return Snapshot::default();
    }
    let out = read_open();
    unsafe { CloseClipboard() };
    out
}

/// Lê os arquivos e o efeito do clipboard já aberto por quem chama. Não fecha o clipboard.
fn read_open() -> Snapshot {
    let mut out = Snapshot::default();
    unsafe {
        let drop = GetClipboardData(CF_HDROP);
        if !drop.is_null() {
            let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
            for i in 0..count {
                let len = DragQueryFileW(drop, i, std::ptr::null_mut(), 0) as usize;
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(drop, i, buf.as_mut_ptr(), buf.len() as u32);
                out.paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
            }
        }
        out.cut = read_cut();
    }
    out
}

/// Esvazia o clipboard só se ele ainda tem exatamente `expected` (os mesmos arquivos, na ordem, com o mesmo efeito).
/// Lê e esvazia na mesma abertura, então um clipboard trocado por outro programa ou pelo usuário fica como está.
pub fn clear_if_same(expected: &Snapshot) {
    if !open() {
        return;
    }
    if read_open() == *expected {
        unsafe { EmptyClipboard() };
    }
    unsafe { CloseClipboard() };
}

/// O formato "Preferred DropEffect" registrado no Windows. `0` se o sistema não o dá.
fn drop_effect_format() -> u32 {
    let name: Vec<u16> = PREFERRED_DROP_EFFECT.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { RegisterClipboardFormatW(name.as_ptr()) }
}

/// Lê o `Preferred DropEffect` do clipboard aberto. Sem o formato, ou sem o bit de mover, é cópia.
/// Quem chama já abriu o clipboard.
unsafe fn read_cut() -> bool {
    let format = drop_effect_format();
    if format == 0 {
        return false;
    }
    let mem = GetClipboardData(format);
    if mem.is_null() {
        return false;
    }
    let ptr = GlobalLock(mem) as *const u8;
    if ptr.is_null() {
        return false;
    }
    let cut = GlobalSize(mem) >= 4 && cut_from(std::slice::from_raw_parts(ptr, 4));
    GlobalUnlock(mem);
    cut
}

/// Diz se os bytes do efeito pedem mover. Só os quatro primeiros bytes (um DWORD em little-endian) contam.
fn cut_from(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) & DROPEFFECT_MOVE != 0
}

/// Põe `bytes` num bloco de memória global móvel, pronto para o clipboard. O bloco é do sistema depois de
/// `SetClipboardData`; se a entrega falha, quem chama libera.
fn global(bytes: &[u8]) -> Result<HANDLE, String> {
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if mem.is_null() {
            return Err("Sem memória para copiar o arquivo.".into());
        }
        let ptr = GlobalLock(mem) as *mut u8;
        if ptr.is_null() {
            GlobalFree(mem);
            return Err("Sem memória para copiar o arquivo.".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        GlobalUnlock(mem);
        Ok(mem)
    }
}

/// Entrega um bloco ao clipboard no formato `format`. Se o sistema recusa, o bloco é liberado aqui.
fn put(format: u32, bytes: &[u8]) -> Result<(), String> {
    let mem = global(bytes)?;
    if unsafe { SetClipboardData(format, mem) }.is_null() {
        unsafe { GlobalFree(mem) };
        return Err("Não foi possível pôr o arquivo na área de transferência.".into());
    }
    Ok(())
}

/// Os bytes do que o Explorer põe no clipboard: o cabeçalho DROPFILES com a lista de caminhos, e o efeito preferido.
/// Separado de `set_files` para o teste conferir o formato sem tocar no clipboard.
fn payload(paths: &[PathBuf], cut: bool) -> (Vec<u8>, u32) {
    // Cabeçalho DROPFILES, depois os caminhos em UTF-16, cada um terminado em zero, e um zero extra no fim.
    let header = DROPFILES {
        pFiles: std::mem::size_of::<DROPFILES>() as u32,
        pt: POINT { x: 0, y: 0 },
        fNC: 0,
        fWide: 1,
    };
    let mut bytes = unsafe {
        std::slice::from_raw_parts(&header as *const DROPFILES as *const u8, std::mem::size_of::<DROPFILES>()).to_vec()
    };
    for path in paths {
        for unit in path.as_os_str().encode_wide().chain(std::iter::once(0)) {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&[0, 0]);
    (bytes, if cut { DROPEFFECT_MOVE } else { DROPEFFECT_COPY })
}

/// Põe os arquivos no clipboard como o Explorer faz ao copiar ou recortar. `cut` marca só o efeito de mover: nada é movido
/// nem apagado aqui; o movimento acontece quando alguém cola no Explorer. Só dá `Ok` se os dois formatos entraram; se
/// falhar no meio, o clipboard é esvaziado para não ficar um recorte pela metade.
pub fn set_files(paths: &[PathBuf], cut: bool) -> Result<(), String> {
    let (bytes, effect) = payload(paths, cut);

    if !open() {
        return Err("A área de transferência está ocupada. Tente de novo.".into());
    }
    let result = unsafe {
        if EmptyClipboard() == 0 {
            Err("Não foi possível limpar a área de transferência.".to_string())
        } else {
            put(CF_HDROP, &bytes).and_then(|_| {
                // `RegisterClipboardFormatW` devolve 0 quando o formato não existe; sem ele o efeito não iria junto.
                let format = drop_effect_format();
                if format == 0 {
                    return Err("Não foi possível pôr o efeito do arquivo na área de transferência.".to_string());
                }
                put(format, &effect.to_le_bytes())
            })
        }
    };
    if result.is_err() {
        // Quem escreveu é o próprio backend: esvaziar aqui não apaga nada do usuário, só o que ficou pela metade.
        unsafe { EmptyClipboard() };
    }
    unsafe { CloseClipboard() };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O formato que o Explorer lê: cabeçalho com `fWide`, caminho em UTF-16 com zero final e um zero extra, e o efeito.
    #[test]
    fn formato_tem_cabecalho_caminho_utf16_e_efeito() {
        let path = PathBuf::from(r"C:\projeto\ação.txt");
        let (bytes, effect) = payload(std::slice::from_ref(&path), true);
        let header = std::mem::size_of::<DROPFILES>();
        assert_eq!(effect, DROPEFFECT_MOVE);
        assert_eq!(&bytes[0..4], &(header as u32).to_le_bytes());
        assert_eq!(&bytes[header - 4..header], &1u32.to_le_bytes());
        let units: Vec<u16> = bytes[header..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let mut want: Vec<u16> = r"C:\projeto\ação.txt".encode_utf16().collect();
        want.extend([0, 0]);
        assert_eq!(units, want);
        assert_eq!(payload(&[path], false).1, DROPEFFECT_COPY);
    }

    /// Só o bit de mover conta como recorte; efeito ausente ou curto é cópia.
    #[test]
    fn efeito_de_mover_e_recorte_e_o_resto_e_copia() {
        assert!(cut_from(&DROPEFFECT_MOVE.to_le_bytes()));
        assert!(!cut_from(&DROPEFFECT_COPY.to_le_bytes()));
        assert!(!cut_from(&[]));
        assert!(!cut_from(&[2, 0, 0]));
    }

    /// Ida e volta pelo clipboard REAL do Windows. Substitui o conteúdo do clipboard do usuário a cada execução, então
    /// não roda no `cargo test` comum. Rode só à mão (`cargo test -- --ignored clipboard`) com o clipboard sem nada importante.
    #[test]
    #[ignore = "usa o clipboard real do Windows e substitui o que o usuário copiou; rode à mão com --ignored"]
    fn arquivo_posto_no_clipboard_volta_pela_lista() {
        let path = std::env::temp_dir().join(format!("orq-clip-{}.txt", std::process::id()));
        std::fs::write(&path, "x").unwrap();
        set_files(std::slice::from_ref(&path), true).unwrap();
        assert_eq!(snapshot().paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(), vec![path.to_string_lossy().into_owned()]);
        assert_eq!(snapshot(), Snapshot { paths: vec![path.clone()], cut: true });
        let _ = std::fs::remove_file(&path);
    }
}
