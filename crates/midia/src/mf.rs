//! Media Foundation do Windows: sessão COM e MF por thread, o arquivo como `IMFByteStream` e a sondagem dos fluxos.
use std::ffi::c_void;
use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;
use windows::core::{implement, Interface, Ref, BOOL, GUID, HRESULT, HSTRING};
use windows::Win32::Foundation::{E_NOTIMPL, S_FALSE, S_OK, STG_E_ACCESSDENIED, STG_E_INVALIDFUNCTION, STG_E_READFAULT};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;

/// COM (MTA) e Media Foundation ligados na thread atual enquanto vive.
pub(crate) struct Sessao;

impl Sessao {
    pub(crate) fn iniciar() -> Result<Sessao, String> {
        // SAFETY: inicialização por thread, desfeita no `Drop`.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(|e| format!("COM indisponível: {}", e.message()))?;
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) {
                CoUninitialize();
                return Err(format!("Media Foundation indisponível: {}", e.message()));
            }
        }
        Ok(Sessao)
    }
}

impl Drop for Sessao {
    fn drop(&mut self) {
        // SAFETY: par do `iniciar` na mesma thread.
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

/// Roda `f` numa thread com sessão própria: quem chama pode estar num apartamento STA (a thread da UI).
pub(crate) fn em_sessao<T: Send>(f: impl FnOnce() -> Result<T, String> + Send) -> Result<T, String> {
    std::thread::scope(|s| {
        s.spawn(|| {
            let _sessao = Sessao::iniciar()?;
            f()
        })
        .join()
        .unwrap_or_else(|_| Err("A sondagem do Media Foundation falhou.".into()))
    })
}

/// `IStream` sobre o `File` do Rust, que abre com FILE_SHARE_DELETE: renomear ou apagar o arquivo com a aba aberta
/// funciona, o que o `MFCreateFile` não permite.
#[implement(IStream)]
struct Fluxo {
    arquivo: Mutex<File>,
    tamanho: u64,
}

impl ISequentialStream_Impl for Fluxo_Impl {
    fn Read(&self, pv: *mut c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        let mut lido = 0usize;
        if cb > 0 && !pv.is_null() {
            // SAFETY: o chamador garante `cb` bytes graváveis em `pv`.
            let buf = unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>(), cb as usize) };
            let mut f = self.arquivo.lock().unwrap_or_else(|e| e.into_inner());
            while lido < buf.len() {
                match f.read(&mut buf[lido..]) {
                    Ok(0) => break,
                    Ok(n) => lido += n,
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => return STG_E_READFAULT,
                }
            }
        }
        if !pcbread.is_null() {
            // SAFETY: ponteiro de saída do chamador.
            unsafe { *pcbread = lido as u32 };
        }
        if lido < cb as usize {
            S_FALSE
        } else {
            S_OK
        }
    }

    fn Write(&self, _pv: *const c_void, _cb: u32, _pcbwritten: *mut u32) -> HRESULT {
        STG_E_ACCESSDENIED
    }
}

impl IStream_Impl for Fluxo_Impl {
    fn Seek(&self, dlibmove: i64, dworigin: STREAM_SEEK, plibnewposition: *mut u64) -> windows::core::Result<()> {
        let de = match dworigin {
            STREAM_SEEK_SET => SeekFrom::Start(dlibmove as u64),
            STREAM_SEEK_CUR => SeekFrom::Current(dlibmove),
            STREAM_SEEK_END => SeekFrom::End(dlibmove),
            _ => return Err(STG_E_INVALIDFUNCTION.into()),
        };
        let pos = self.arquivo.lock().unwrap_or_else(|e| e.into_inner()).seek(de).map_err(|_| windows::core::Error::from(STG_E_INVALIDFUNCTION))?;
        if !plibnewposition.is_null() {
            // SAFETY: ponteiro de saída do chamador.
            unsafe { *plibnewposition = pos };
        }
        Ok(())
    }

    fn SetSize(&self, _libnewsize: u64) -> windows::core::Result<()> {
        Err(STG_E_ACCESSDENIED.into())
    }

    fn CopyTo(&self, _pstm: Ref<IStream>, _cb: u64, _pcbread: *mut u64, _pcbwritten: *mut u64) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn Commit(&self, _grfcommitflags: &STGC) -> windows::core::Result<()> {
        Ok(())
    }

    fn Revert(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn LockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: &LOCKTYPE) -> windows::core::Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn UnlockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: u32) -> windows::core::Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }

    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> windows::core::Result<()> {
        if pstatstg.is_null() {
            return Err(STG_E_INVALIDFUNCTION.into());
        }
        // SAFETY: ponteiro de saída do chamador; sem nome (`pwcsName` nulo).
        unsafe { *pstatstg = STATSTG { r#type: STGTY_STREAM.0 as u32, cbSize: self.tamanho, grfMode: STGM_READ, ..Default::default() } };
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IStream> {
        Err(E_NOTIMPL.into())
    }
}

/// O arquivo como byte stream do MF. Exige sessão na thread.
pub(crate) fn abrir(caminho: &Path) -> Result<IMFByteStream, String> {
    let erro = |e: &dyn std::fmt::Display| format!("Não foi possível abrir o arquivo: {e}");
    let arquivo = File::open(caminho).map_err(|e| erro(&e))?;
    let tamanho = arquivo.metadata().map_err(|e| erro(&e))?.len();
    let fluxo: IStream = Fluxo { arquivo: Mutex::new(arquivo), tamanho }.into();
    // SAFETY: o byte stream guarda sua referência ao `IStream`.
    unsafe { MFCreateMFByteStreamOnStream(&fluxo) }.map_err(|e| erro(&e.message()))
}

/// A fonte de mídia do arquivo; o resolvedor tenta todos os formatos quando a extensão não bate (ou falta).
fn fonte(caminho: &Path) -> Result<IMFMediaSource, String> {
    let bs = abrir(caminho)?;
    let url = HSTRING::from(caminho.as_os_str());
    let erro = |e: windows::core::Error| format!("Formato não reconhecido: {}", e.message());
    let flags = MF_RESOLUTION_MEDIASOURCE.0 | MF_RESOLUTION_READ.0 | MF_RESOLUTION_CONTENT_DOES_NOT_HAVE_TO_MATCH_EXTENSION_OR_MIME_TYPE.0;
    // SAFETY: chamadas COM com a sessão da thread.
    unsafe {
        let resolvedor = MFCreateSourceResolver().map_err(erro)?;
        let mut tipo = MF_OBJECT_INVALID;
        let mut objeto = None;
        resolvedor.CreateObjectFromByteStream(&bs, &url, flags as u32, None, &mut tipo, &mut objeto).map_err(erro)?;
        objeto.ok_or("Formato não reconhecido.")?.cast::<IMFMediaSource>().map_err(erro)
    }
}

/// Tipo principal e subtipo (codec) de cada fluxo do arquivo. Exige sessão na thread.
pub(crate) fn fluxos(caminho: &Path) -> Result<Vec<(GUID, GUID)>, String> {
    let fonte = fonte(caminho)?;
    // SAFETY: chamadas COM com a sessão da thread; a fonte é desligada no fim.
    let r = unsafe {
        (|| {
            let pd = fonte.CreatePresentationDescriptor()?;
            let mut v = Vec::new();
            for i in 0..pd.GetStreamDescriptorCount()? {
                let mut selecionado = BOOL(0);
                let mut sd = None;
                pd.GetStreamDescriptorByIndex(i, &mut selecionado, &mut sd)?;
                let Some(sd) = sd else { continue };
                let h = sd.GetMediaTypeHandler()?;
                let sub = h.GetMediaTypeByIndex(0).and_then(|t| t.GetGUID(&MF_MT_SUBTYPE)).unwrap_or_default();
                v.push((h.GetMajorType()?, sub));
            }
            Ok::<_, windows::core::Error>(v)
        })()
    };
    // SAFETY: a fonte não é mais usada.
    unsafe {
        let _ = fonte.Shutdown();
    }
    r.map_err(|e| e.message())
}

/// Há decodificador instalado para o fluxo (ou ele já vem descomprimido).
pub(crate) fn decodifica(principal: GUID, sub: GUID) -> bool {
    if [MFAudioFormat_PCM, MFAudioFormat_Float].contains(&sub) {
        return true;
    }
    let categoria = if principal == MFMediaType_Video { MFT_CATEGORY_VIDEO_DECODER } else { MFT_CATEGORY_AUDIO_DECODER };
    let entrada = MFT_REGISTER_TYPE_INFO { guidMajorType: principal, guidSubtype: sub };
    let mut lista: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut n = 0u32;
    // SAFETY: a lista vem alocada pelo COM; cada item é solto e o vetor liberado.
    unsafe {
        if MFTEnumEx(categoria, MFT_ENUM_FLAG_ALL, Some(&entrada), None, &mut lista, &mut n).is_err() {
            return false;
        }
        if !lista.is_null() {
            for i in 0..n as usize {
                drop(std::ptr::read(lista.add(i)));
            }
            CoTaskMemFree(Some(lista as *const c_void));
        }
    }
    n > 0
}

/// A extensão da Microsoft Store que falta para tocar o arquivo, se é esse o problema. Exige sessão na thread.
pub(crate) fn extensao_que_falta(caminho: &Path) -> Option<&'static str> {
    const WEB_MEDIA: &str = "Web Media Extensions";
    let Ok(fluxos) = fluxos(caminho) else {
        let ext = caminho.extension()?.to_string_lossy().to_ascii_lowercase();
        return ["ogg", "oga", "opus"].contains(&ext.as_str()).then_some(WEB_MEDIA);
    };
    fluxos.into_iter().filter(|&(p, s)| !decodifica(p, s)).find_map(|(_, s)| match s {
        s if s == MFVideoFormat_HEVC => Some("HEVC Video Extensions"),
        s if s == MFVideoFormat_VP90 || s == MFVideoFormat_VP80 => Some("VP9 Video Extensions"),
        s if s == MFVideoFormat_AV1 => Some("AV1 Video Extension"),
        s if s == MFVideoFormat_MPEG2 => Some("MPEG-2 Video Extension"),
        s if s == MFAudioFormat_Opus => Some(WEB_MEDIA),
        _ => None,
    })
}
