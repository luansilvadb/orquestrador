//! O que é mídia: os decodificadores do crate `image` e do Media Foundation do Windows, registrados no core
//! (`ide::midia`), que é a regra única.
use crate::mf;
use orquestrador_core::ide::midia::{self, Decodificadores};
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Media::MediaFoundation::{MFMediaType_Audio, MFMediaType_Video};
use windows::Win32::System::Registry::*;

pub use orquestrador_core::ide::midia::TipoMidia;

struct Formatos;

/// Subchaves de `raiz\caminho`.
fn subchaves(raiz: HKEY, caminho: &str) -> Vec<String> {
    let mut chave = HKEY::default();
    // SAFETY: chave aberta só para leitura e fechada no fim; buffers locais.
    unsafe {
        if RegOpenKeyExW(raiz, &HSTRING::from(caminho), None, KEY_READ, &mut chave) != ERROR_SUCCESS {
            return Vec::new();
        }
        let mut nomes = Vec::new();
        let mut buf = [0u16; 256];
        for i in 0.. {
            let mut n = buf.len() as u32;
            if RegEnumKeyExW(chave, i, Some(PWSTR(buf.as_mut_ptr())), &mut n, None, None, None, None) != ERROR_SUCCESS {
                break;
            }
            nomes.push(String::from_utf16_lossy(&buf[..n as usize]));
        }
        let _ = RegCloseKey(chave);
        nomes
    }
}

/// Valor de texto `valor` em `raiz\chave`.
fn valor(raiz: HKEY, chave: &str, valor: &str) -> Option<String> {
    let mut buf = [0u16; 256];
    let mut bytes = (buf.len() * 2) as u32;
    // SAFETY: buffer local com o tamanho informado em bytes.
    let r = unsafe { RegGetValueW(raiz, &HSTRING::from(chave), &HSTRING::from(valor), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut bytes)) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let n = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// Extensão (sem ponto, minúsculas) de cada formato que o Media Foundation abre (os byte stream handlers) ou que o
/// Windows percebe como áudio ou vídeo, com o MIME de áudio ou vídeo do `Content Type`, se houver.
static AV: LazyLock<HashMap<String, Option<&'static str>>> = LazyLock::new(|| {
    let ext = |chave: &str| {
        let e = chave.strip_prefix('.')?.to_ascii_lowercase();
        (!e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some(e)
    };
    let mime = |chave: &str| -> Option<&'static str> {
        let m = valor(HKEY_CLASSES_ROOT, chave, "Content Type")?.to_ascii_lowercase();
        (m.starts_with("audio/") || m.starts_with("video/")).then(|| &*Box::leak(m.into_boxed_str()))
    };
    let mut mapa = HashMap::new();
    for chave in subchaves(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows Media Foundation\ByteStreamHandlers") {
        if let Some(e) = ext(&chave) {
            mapa.insert(e, mime(&chave));
        }
    }
    for chave in subchaves(HKEY_CLASSES_ROOT, "").into_iter().filter(|c| c.starts_with('.')) {
        let Some(e) = ext(&chave).filter(|e| !mapa.contains_key(e)) else { continue };
        if valor(HKEY_CLASSES_ROOT, &chave, "PerceivedType").is_some_and(|p| p.eq_ignore_ascii_case("audio") || p.eq_ignore_ascii_case("video")) {
            mapa.insert(e, mime(&chave));
        }
    }
    mapa
});

impl Decodificadores for Formatos {
    fn imagem(&self, ext: &str) -> Option<&'static str> {
        let f = image::ImageFormat::from_extension(ext).filter(|f| f.reading_enabled())?;
        Some(f.to_mime_type())
    }

    fn av(&self, ext: &str) -> Option<Option<&'static str>> {
        AV.get(ext).copied()
    }

    fn sondar(&self, caminho: &Path) -> Option<TipoMidia> {
        if midia::texto(caminho) {
            return None;
        }
        let fluxos = mf::em_sessao(|| mf::fluxos(caminho)).ok()?;
        if fluxos.iter().any(|f| f.0 == MFMediaType_Video) {
            Some(TipoMidia::Video)
        } else if fluxos.iter().any(|f| f.0 == MFMediaType_Audio) {
            Some(TipoMidia::Audio)
        } else {
            None
        }
    }
}

/// Registra os formatos no core. Idempotente; quem classifica pelo crate já chama.
pub fn iniciar() {
    midia::registrar(&Formatos);
}

/// O tipo do arquivo (`ide::midia::classificar`); `None` se não é mídia. Pode abrir o arquivo para sondar.
pub fn midia_de(caminho: &Path) -> Option<TipoMidia> {
    iniciar();
    midia::classificar(caminho)
}

/// Nome do arquivo para rótulos.
pub fn nome_de(caminho: &Path) -> String {
    caminho.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| caminho.display().to_string())
}

/// Tempo como no controle do Chromium: `m:ss`, ou `h:mm:ss` a partir de uma hora.
pub fn formatar_tempo(segundos: f64) -> String {
    let s = if segundos.is_finite() && segundos > 0. { segundos.floor() as u64 } else { 0 };
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn extensoes_sem_abrir() {
        iniciar();
        let ext = |n: &str| midia::por_extensao(Path::new(n));
        for n in ["a/logo.PNG", "x.ico", "x.tiff", "x.tga", "x.qoi", "x.jpeg"] {
            assert_eq!(ext(n), Some(TipoMidia::Imagem), "{n}");
        }
        for n in ["tom.wav", "x.mp3", "x.flac", "x.m4a"] {
            assert_eq!(ext(n), Some(TipoMidia::Audio), "{n}");
        }
        // O Opus depende da Web Media Extensions; sem ela, a extensão não é conhecida.
        assert!(matches!(ext("x.opus"), Some(TipoMidia::Audio) | None));
        // MIME de vídeo, ou nenhum: até sondar, conta como vídeo.
        for n in ["clipe.webm", "x.mov", "x.mkv", "x.avi", "x.ts"] {
            assert_eq!(ext(n), Some(TipoMidia::Video), "{n}");
        }
        for n in [".png", "README.md", "semponto", "x.svg"] {
            assert_eq!(ext(n), None, "{n}");
        }
        assert_eq!(midia::mime(Path::new("a.ico")), "image/x-icon");
        assert_eq!(midia::mime(Path::new("a.mp3")), "audio/mpeg");
        assert_eq!(midia::mime(Path::new("a.zzz")), "application/octet-stream");
    }

    #[test]
    fn texto_com_extensao_de_demuxer_nao_e_midia() {
        let dir = std::env::temp_dir().join(format!("orq-tipos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["notas.txt", "tipos.ts", "legenda.srt", "x.mp4"] {
            let c = dir.join(n);
            std::fs::write(&c, "não é mídia
").unwrap();
            assert_eq!(midia_de(&c), None, "{n}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tempo() {
        assert_eq!(formatar_tempo(0.), "0:00");
        assert_eq!(formatar_tempo(2.04), "0:02");
        assert_eq!(formatar_tempo(65.9), "1:05");
        assert_eq!(formatar_tempo(3725.), "1:02:05");
        assert_eq!(formatar_tempo(f64::NAN), "0:00");
    }
}
