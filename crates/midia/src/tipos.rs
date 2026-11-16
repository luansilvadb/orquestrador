//! O que é mídia: os decodificadores do crate `image` e do FFmpeg, registrados no core (`ide::midia`), que é a regra única.
use ffmpeg_next as ff;
use orquestrador_core::ide::midia::{self, Decodificadores};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::path::Path;
use std::sync::LazyLock;

pub use orquestrador_core::ide::midia::TipoMidia;

struct Formatos;

/// Extensão de cada demuxer e muxer do FFmpeg (o muxer cobre o `wav`, o `ts` e o `opus`, que o demuxer acha pelo
/// conteúdo), com o primeiro MIME que algum deles declara.
static AV: LazyLock<HashMap<String, Option<&'static str>>> = LazyLock::new(|| {
    crate::player::iniciar_ffmpeg();
    let texto = |p: *const c_char| (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_str().ok()).flatten();
    let mut mapa: HashMap<String, Option<&'static str>> = HashMap::new();
    let mut juntar = |exts: Option<&'static str>, mime: Option<&'static str>| {
        // O de vídeo vence: o Matroska declara `audio/webm` antes de `video/webm`.
        let peso = |m: &Option<&str>| m.map_or(0, |m| if m.starts_with("video/") { 2 } else { 1 });
        let mimes = || mime.into_iter().flat_map(|m| m.split(',')).map(str::trim);
        let mime = mimes().find(|m| m.starts_with("video/")).or_else(|| mimes().find(|m| m.starts_with("audio/")));
        for e in exts.into_iter().flat_map(|e| e.split(',')).map(|e| e.trim().to_ascii_lowercase()).filter(|e| !e.is_empty()) {
            let m = mapa.entry(e).or_default();
            if peso(&mime) > peso(m) {
                *m = mime;
            }
        }
    };
    // SAFETY: as tabelas de formatos são estáticas; as strings vivem até o fim do processo.
    unsafe {
        let mut op: *mut c_void = std::ptr::null_mut();
        while let Some(f) = ff::ffi::av_demuxer_iterate(&mut op).as_ref() {
            juntar(texto(f.extensions), texto(f.mime_type));
        }
        let mut op: *mut c_void = std::ptr::null_mut();
        while let Some(f) = ff::ffi::av_muxer_iterate(&mut op).as_ref() {
            juntar(texto(f.extensions), texto(f.mime_type));
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
        let entrada = crate::player::abrir_entrada(caminho).ok()?;
        let decodifica = |s: &ff::format::stream::Stream| ff::decoder::find(s.parameters().id()).is_some();
        let video = entrada.streams().filter(|s| s.parameters().medium() == ff::media::Type::Video).any(|s| {
            !s.disposition().contains(ff::format::stream::Disposition::ATTACHED_PIC) && decodifica(&s)
        });
        let audio = entrada.streams().filter(|s| s.parameters().medium() == ff::media::Type::Audio).any(|s| decodifica(&s));
        if video {
            Some(TipoMidia::Video)
        } else if audio {
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
        for n in ["tom.wav", "x.mp3", "x.opus", "x.flac"] {
            assert_eq!(ext(n), Some(TipoMidia::Audio), "{n}");
        }
        // Sem MIME de áudio: até sondar, conta como vídeo.
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
