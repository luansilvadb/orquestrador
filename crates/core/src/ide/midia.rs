//! O que abre na aba de mídia e como: a única regra para o core (IPC e protocolo `orq-media`), o editor e o
//! `orquestrador-midia`. O core não decodifica nada: quem conhece os formatos (o crate `image` e o Media Foundation do Windows) se registra
//! em [`registrar`], e sem registro nada é mídia.
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoMidia {
    Imagem,
    Audio,
    Video,
}

/// Quem sabe decodificar. As extensões chegam em minúsculas e sem ponto.
pub trait Decodificadores: Send + Sync {
    /// O MIME se a extensão é de imagem que se lê.
    fn imagem(&self, ext: &str) -> Option<&'static str>;
    /// O MIME (ou `None` sem um conhecido) se a extensão é de um demuxer de áudio ou vídeo.
    fn av(&self, ext: &str) -> Option<Option<&'static str>>;
    /// Abre o arquivo e diz se tem vídeo (que não é capa) ou só áudio; `None` se não abre ou se é [`texto`].
    fn sondar(&self, caminho: &Path) -> Option<TipoMidia>;
}

static REGISTRO: OnceLock<&'static dyn Decodificadores> = OnceLock::new();

/// Registra os decodificadores; só o primeiro registro vale.
pub fn registrar(d: &'static dyn Decodificadores) {
    let _ = REGISTRO.set(d);
}

fn registro() -> Option<&'static dyn Decodificadores> {
    REGISTRO.get().copied()
}

/// A extensão em minúsculas; nome que começa com ponto e não tem outro ponto não tem extensão.
fn ext(caminho: &Path) -> Option<String> {
    let nome = caminho.file_name()?.to_string_lossy();
    let ponto = nome.rfind('.').filter(|&p| p > 0)?;
    Some(nome[ponto + 1..].to_ascii_lowercase())
}

/// Só pelo nome, sem ler o arquivo: imagem que se lê, ou extensão de demuxer (áudio se o MIME dela é de áudio).
pub fn por_extensao(caminho: &Path) -> Option<TipoMidia> {
    let (r, e) = (registro()?, ext(caminho)?);
    if r.imagem(&e).is_some() {
        return Some(TipoMidia::Imagem);
    }
    let mime = r.av(&e)?;
    Some(if mime.is_some_and(|m| m.starts_with("audio/")) { TipoMidia::Audio } else { TipoMidia::Video })
}

/// O tipo do arquivo. Imagem pela extensão; texto não é mídia; áudio e vídeo pelos fluxos que o
/// Media Foundation acha, com ou sem extensão conhecida. Extensão de demuxer que não abre segue pelo [`por_extensao`]: a aba mostra o erro.
pub fn classificar(caminho: &Path) -> Option<TipoMidia> {
    let r = registro()?;
    let pelo_nome = por_extensao(caminho);
    if pelo_nome == Some(TipoMidia::Imagem) {
        return pelo_nome;
    }
    r.sondar(caminho).or_else(|| pelo_nome.filter(|_| !texto(caminho)))
}

/// O começo do arquivo não tem NUL (a regra de `files::is_binary`). Arquivo vazio ou que não se lê não conta como texto.
pub fn texto(caminho: &Path) -> bool {
    let mut inicio = Vec::new();
    let lido = std::fs::File::open(caminho).and_then(|f| f.take(super::files::BINARY_SNIFF as u64).read_to_end(&mut inicio));
    lido.is_ok() && !inicio.is_empty() && !super::files::is_binary(&inicio)
}

/// O MIME pela extensão; genérico quando nenhum decodificador conhece um.
pub fn mime(caminho: &Path) -> &'static str {
    let generico = "application/octet-stream";
    let (Some(r), Some(e)) = (registro(), ext(caminho)) else { return generico };
    r.imagem(&e).or_else(|| r.av(&e).flatten()).unwrap_or(generico)
}

/// Registro de teste, para quem não tem os decodificadores: tabela fixa, sonda pela extensão.
#[doc(hidden)]
pub mod teste {
    use super::*;

    struct Tabela;
    const IMAGENS: &[(&str, &str)] = &[("png", "image/png"), ("jpg", "image/jpeg")];
    const AV: &[(&str, &str, TipoMidia)] = &[
        ("mp4", "video/mp4", TipoMidia::Video),
        ("m4v", "video/mp4", TipoMidia::Video),
        ("mov", "video/mp4", TipoMidia::Video),
        ("webm", "video/webm", TipoMidia::Video),
        ("mp3", "audio/mpeg", TipoMidia::Audio),
        ("wav", "audio/wav", TipoMidia::Audio),
        ("ogg", "audio/ogg", TipoMidia::Audio),
        ("oga", "audio/ogg", TipoMidia::Audio),
        ("flac", "audio/flac", TipoMidia::Audio),
        ("m4a", "audio/mp4", TipoMidia::Audio),
    ];

    impl Decodificadores for Tabela {
        fn imagem(&self, ext: &str) -> Option<&'static str> {
            IMAGENS.iter().find(|i| i.0 == ext).map(|i| i.1)
        }
        fn av(&self, ext: &str) -> Option<Option<&'static str>> {
            AV.iter().find(|a| a.0 == ext).map(|a| Some(a.1))
        }
        fn sondar(&self, caminho: &Path) -> Option<TipoMidia> {
            let e = ext(caminho)?;
            AV.iter().find(|a| a.0 == e).map(|a| a.2)
        }
    }

    /// Registra a tabela de teste (se ninguém registrou antes).
    pub fn registrar() {
        super::registrar(&Tabela);
    }
}
