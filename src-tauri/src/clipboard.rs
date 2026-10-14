//! Os arquivos no clipboard do Windows, no formato que o Explorer usa: a lista de arquivos (`CF_HDROP`) e o efeito
//! preferido (`Preferred DropEffect`, mover no recorte e copiar na cópia). A página não vê o caminho deles (o evento
//! de colar só entrega o conteúdo), então quem lê e quem escreve é o backend. Colar é feito pelo Explorer.
use std::path::PathBuf;
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::UI::Shell::{DragQueryFileW, DROPFILES};

const CF_HDROP: u32 = 15;
const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_MOVE: u32 = 2;
const PREFERRED_DROP_EFFECT: &str = "Preferred DropEffect";

/// O que está no clipboard como arquivos: os caminhos, na ordem, e se foram recortados (`cut`) ou copiados.
/// `paths` vem vazio quando o clipboard não tem arquivos.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
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
/// Devolve se esvaziou.
pub fn clear_if_same(expected: &Snapshot) -> bool {
    if !open() {
        return false;
    }
    let same = read_open() == *expected;
    unsafe {
        if same {
            EmptyClipboard();
        }
        CloseClipboard();
    }
    same
}

/// Os caminhos completos dos arquivos copiados, na ordem do clipboard. Vazio quando não há arquivo ou
/// quando o clipboard não abre.
pub fn files() -> Vec<String> {
    snapshot().paths.iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

/// Lê o `Preferred DropEffect` do clipboard aberto. Sem o formato, ou sem o bit de mover, é cópia.
/// Quem chama já abriu o clipboard.
unsafe fn read_cut() -> bool {
    let name: Vec<u16> = PREFERRED_DROP_EFFECT.encode_utf16().chain(std::iter::once(0)).collect();
    let format = RegisterClipboardFormatW(name.as_ptr());
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
    use std::os::windows::ffi::OsStrExt;
    // Cabeçalho DROPFILES, depois os caminhos em UTF-16, cada um terminado em zero, e um zero extra no fim.
    let header = DROPFILES {
        pFiles: std::mem::size_of::<DROPFILES>() as u32,
        pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
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
    let name: Vec<u16> = PREFERRED_DROP_EFFECT.encode_utf16().chain(std::iter::once(0)).collect();

    if !open() {
        return Err("A área de transferência está ocupada. Tente de novo.".into());
    }
    let result = unsafe {
        if EmptyClipboard() == 0 {
            Err("Não foi possível limpar a área de transferência.".to_string())
        } else {
            put(CF_HDROP, &bytes).and_then(|_| {
                // `RegisterClipboardFormatW` devolve 0 quando o formato não existe; sem ele o efeito não iria junto.
                let format = RegisterClipboardFormatW(name.as_ptr());
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
        assert_eq!(files(), vec![path.to_string_lossy().into_owned()]);
        assert_eq!(snapshot(), Snapshot { paths: vec![path.clone()], cut: true });
        let _ = std::fs::remove_file(&path);
    }
}
