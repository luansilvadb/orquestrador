//! Os arquivos que o clipboard do Windows traz quando o usuário copia no Explorer (`CF_HDROP`). A página
//! não vê o caminho deles (o evento de colar só entrega o conteúdo), então quem lê é o backend.
use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows_sys::Win32::UI::Shell::DragQueryFileW;

const CF_HDROP: u32 = 15;

/// Os caminhos completos dos arquivos copiados, na ordem do clipboard. Vazio quando não há arquivo ou
/// quando o clipboard não abre.
pub fn files() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        // Outro programa pode estar com o clipboard aberto por um instante.
        let opened = (0..10).any(|_| {
            let ok = OpenClipboard(std::ptr::null_mut()) != 0;
            if !ok {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            ok
        });
        if !opened {
            return out;
        }
        let drop = GetClipboardData(CF_HDROP);
        if !drop.is_null() {
            let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
            for i in 0..count {
                let len = DragQueryFileW(drop, i, std::ptr::null_mut(), 0) as usize;
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(drop, i, buf.as_mut_ptr(), buf.len() as u32);
                out.push(String::from_utf16_lossy(&buf[..len]));
            }
        }
        CloseClipboard();
    }
    out
}
