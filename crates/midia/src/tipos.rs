//! Tipos de mídia pela extensão, espelho de `TYPES` em `src/editor/media-view.tsx` e das listas `MEDIA_EXTS`, `AUDIO_EXTS`
//! e `VIDEO_EXTS` de `crates/core/src/ide/files.rs`.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoMidia {
    Imagem,
    Audio,
    Video,
}

const TIPOS: &[(&str, TipoMidia, &str)] = &[
    ("png", TipoMidia::Imagem, "image/png"),
    ("jpg", TipoMidia::Imagem, "image/jpeg"),
    ("jpeg", TipoMidia::Imagem, "image/jpeg"),
    ("gif", TipoMidia::Imagem, "image/gif"),
    ("webp", TipoMidia::Imagem, "image/webp"),
    ("bmp", TipoMidia::Imagem, "image/bmp"),
    ("mp3", TipoMidia::Audio, "audio/mpeg"),
    ("wav", TipoMidia::Audio, "audio/wav"),
    ("ogg", TipoMidia::Audio, "audio/ogg"),
    ("oga", TipoMidia::Audio, "audio/ogg"),
    ("flac", TipoMidia::Audio, "audio/flac"),
    ("m4a", TipoMidia::Audio, "audio/mp4"),
    ("mp4", TipoMidia::Video, "video/mp4"),
    ("m4v", TipoMidia::Video, "video/mp4"),
    ("webm", TipoMidia::Video, "video/webm"),
    ("mov", TipoMidia::Video, "video/quicktime"),
];

/// Todas as extensões aceitas, em minúsculas.
pub fn extensoes() -> impl Iterator<Item = &'static str> {
    TIPOS.iter().map(|(e, _, _)| *e)
}

/// Tipo e MIME pelo nome do arquivo; `None` se não é mídia. Nome que começa com ponto e não tem outro ponto não tem extensão.
pub fn midia_de(caminho: &Path) -> Option<(TipoMidia, &'static str)> {
    let nome = caminho.file_name()?.to_string_lossy();
    let ponto = nome.rfind('.')?;
    if ponto == 0 {
        return None;
    }
    let ext = nome[ponto + 1..].to_ascii_lowercase();
    TIPOS.iter().find(|(e, _, _)| *e == ext).map(|(_, t, m)| (*t, *m))
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
    fn extensoes_e_tipos() {
        assert_eq!(midia_de(Path::new("a/logo.PNG")), Some((TipoMidia::Imagem, "image/png")));
        assert_eq!(midia_de(Path::new("tom.wav")).map(|m| m.0), Some(TipoMidia::Audio));
        assert_eq!(midia_de(Path::new("clipe.webm")).map(|m| m.0), Some(TipoMidia::Video));
        assert_eq!(midia_de(Path::new("x.mov")), Some((TipoMidia::Video, "video/quicktime")));
        assert_eq!(midia_de(Path::new(".png")), None);
        assert_eq!(midia_de(Path::new("README.md")), None);
        assert_eq!(midia_de(Path::new("semponto")), None);
        assert_eq!(midia_de(Path::new("x.svg")), None);
        assert_eq!(extensoes().count(), 16);
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
