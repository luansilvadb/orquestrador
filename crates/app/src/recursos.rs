//! Ícones e a marca embutidos no binário; o resto vem dos recursos padrão do GPUI Kit.
use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

macro_rules! embutidos {
    ($($nome:literal),* $(,)?) => {
        &[$(($nome, include_bytes!(concat!("../assets/", $nome)) as &[u8])),*]
    };
}

const PROPRIOS: &[(&str, &[u8])] = embutidos![
    "app-mark.svg",
    "icons/app-mark-mono.svg",
    "icons/files.svg",
    "icons/search.svg",
    "icons/source-control.svg",
    "icons/folder.svg",
    "icons/plus.svg",
    "icons/x.svg",
    "icons/square-terminal.svg",
    "icons/refresh-cw.svg",
    "icons/window-minimize.svg",
    "icons/window-maximize.svg",
    "icons/window-restore.svg",
    "icons/window-close.svg",
];

pub struct Recursos;

impl AssetSource for Recursos {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = PROPRIOS.iter().find(|(nome, _)| *nome == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut nomes: Vec<SharedString> =
            PROPRIOS.iter().filter(|(nome, _)| nome.starts_with(path)).map(|(nome, _)| (*nome).into()).collect();
        nomes.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(nomes)
    }
}

pub mod icone {
    pub const ARQUIVOS: &str = "icons/files.svg";
    pub const PESQUISAR: &str = "icons/search.svg";
    pub const GIT: &str = "icons/source-control.svg";
    pub const PASTA: &str = "icons/folder.svg";
    pub const MAIS: &str = "icons/plus.svg";
    pub const FECHAR: &str = "icons/x.svg";
    pub const TERMINAL: &str = "icons/square-terminal.svg";
    pub const SINCRONIZAR: &str = "icons/refresh-cw.svg";
    pub const MINIMIZAR: &str = "icons/window-minimize.svg";
    pub const MAXIMIZAR: &str = "icons/window-maximize.svg";
    pub const RESTAURAR: &str = "icons/window-restore.svg";
    pub const FECHAR_JANELA: &str = "icons/window-close.svg";
    pub const MARCA: &str = "app-mark.svg";
    pub const MARCA_MONO: &str = "icons/app-mark-mono.svg";
}
