//! Instância única, no lugar do `tauri-plugin-single-instance`: um mutex nomeado diz se já há janela; a segunda instância
//! manda os argumentos e a pasta atual por um pipe nomeado e sai. A primeira os converte com `open_paths::extract_paths`.
use crate::backend::{Evento, Remetente};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Pedido {
    args: Vec<String>,
    cwd: String,
}

/// Resultado da tentativa de ser a instância principal.
pub enum Instancia {
    Principal(Guarda),
    /// Outra instância recebeu os argumentos; esta deve sair.
    Repassada,
}

/// Mantém o mutex enquanto o app vive.
pub struct Guarda {
    #[cfg(windows)]
    mutex: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl Drop for Guarda {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.mutex) };
    }
}

fn nome_com_usuario(prefixo: &str) -> String {
    let usuario = std::env::var("USERNAME").unwrap_or_default();
    let limpo: String = usuario.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("{prefixo}-{limpo}")
}

fn caminho_pipe() -> String {
    format!(r"\\.\pipe\{}", nome_com_usuario("orquestrador-gpui"))
}

fn largo(texto: &str) -> Vec<u16> {
    texto.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
pub fn reivindicar(args: &[String], cwd: &Path) -> Instancia {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let nome = largo(&format!("Local\\{}", nome_com_usuario("orquestrador-gpui-instancia")));
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, nome.as_ptr()) };
    let existe = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if mutex.is_null() || !existe {
        return Instancia::Principal(Guarda { mutex });
    }
    unsafe { windows_sys::Win32::Foundation::CloseHandle(mutex) };
    let pedido = Pedido { args: args.to_vec(), cwd: cwd.to_string_lossy().into_owned() };
    // A principal pode estar entre duas conexões do pipe: tenta por até ~2 s.
    for _ in 0..40 {
        if let Ok(mut pipe) = std::fs::OpenOptions::new().write(true).open(caminho_pipe()) {
            if serde_json::to_writer(&mut pipe, &pedido).is_ok() {
                return Instancia::Repassada;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // Sem resposta, a principal travou ou está saindo: abre uma janela própria.
    Instancia::Principal(Guarda { mutex: std::ptr::null_mut() })
}

#[cfg(not(windows))]
pub fn reivindicar(_args: &[String], _cwd: &Path) -> Instancia {
    Instancia::Principal(Guarda {})
}

/// Atende o pipe numa thread própria, repassando cada pedido como `Evento::AbrirCaminhos`.
#[cfg(windows)]
pub fn escutar(remetente: Remetente) {
    use std::io::Read;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_INBOUND;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    let nome = largo(&caminho_pipe());
    let _ = std::thread::Builder::new().name("instancia-unica".into()).spawn(move || loop {
        let pipe = unsafe {
            CreateNamedPipeW(
                nome.as_ptr(), PIPE_ACCESS_INBOUND, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES, 0, 64 * 1024, 0, std::ptr::null(),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            return;
        }
        let conectado = unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        // O File fecha o handle ao sair do escopo.
        let mut arquivo = unsafe { std::fs::File::from_raw_handle(pipe as _) };
        if !conectado {
            continue;
        }
        let mut texto = String::new();
        if arquivo.read_to_string(&mut texto).is_err() {
            continue;
        }
        if let Ok(pedido) = serde_json::from_str::<Pedido>(&texto) {
            let caminhos = caminhos_do_pedido(&pedido.args, Path::new(&pedido.cwd));
            if remetente.send_blocking(Evento::AbrirCaminhos(caminhos)).is_err() {
                return;
            }
        }
    });
}

#[cfg(not(windows))]
pub fn escutar(_remetente: Remetente) {}

/// Mesmo filtro do Tauri: só caminhos, sem as opções.
pub fn caminhos_do_pedido(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    orquestrador_core::open_paths::extract_paths(args, cwd)
}
