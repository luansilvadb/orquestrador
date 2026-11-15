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
    // Painéis de terminal (fase 2).
    "icons/claude.svg",
    "icons/codex.svg",
    "icons/circle-question-mark.svg",
    "icons/clock.svg",
    "icons/loader-circle.svg",
    "icons/circle-check.svg",
    "icons/square.svg",
    "icons/circle-x.svg",
    "icons/sliders-horizontal.svg",
    "icons/chevron-down.svg",
    "icons/chevron-up.svg",
    "icons/chevron-left.svg",
    "icons/chevron-right.svg",
    "icons/chevrons-up-down.svg",
    "icons/list.svg",
    "icons/check.svg",
    // Pesquisa (crate orquestrador-pesquisa).
    "icons/case-sensitive.svg",
    "icons/whole-word.svg",
    "icons/regex.svg",
    "icons/ellipsis.svg",
    "icons/list-tree.svg",
    "icons/list-x.svg",
    "icons/chevrons-down-up.svg",
    "icons/replace-all.svg",
    "icons/rotate-cw.svg",
    "icons/book-open.svg",
    "icons/case-upper.svg",
    // Editor (crate orquestrador-editor).
    "icons/pin.svg",
    "icons/columns-2.svg",
    "icons/file-text.svg",
    "icons/copy.svg",
    "icons/arrow-up.svg",
    "icons/arrow-down.svg",
    // Git (crate orquestrador-git).
    "icons/git-branch.svg",
    "icons/minus.svg",
    "icons/square-arrow-out-up-right.svg",
    "icons/arrow-left.svg",
    "icons/cloud.svg",
    "icons/tag.svg",
    "icons/target.svg",
    "icons/cloud-download.svg",
    "icons/arrow-up-from-line.svg",
    "icons/arrow-down-to-line.svg",
    "icons/external-link.svg",
    "icons/rows-2.svg",
    "icons/arrow-right.svg",
    "icons/undo-2.svg",
    // Mídia (crate orquestrador-midia).
    "icons/midia-tocar.svg",
    "icons/midia-pausar.svg",
    "icons/midia-volume.svg",
    "icons/midia-mudo.svg",
    "icons/midia-tela-cheia.svg",
    "icons/midia-sair-tela-cheia.svg",
    "icons/midia-mais.svg",
    // Árvore de arquivos (crate orquestrador-arvore) e avisos.
    "icons/file-plus.svg",
    "icons/folder-plus.svg",
    "icons/file.svg",
    "icons/info.svg",
    "icons/triangle-alert.svg",
    "icons/circle-check-big.svg",
];

pub struct Recursos;

impl AssetSource for Recursos {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = PROPRIOS.iter().find(|(nome, _)| *nome == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = crate::icones_arquivo::carregar(path) {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut nomes: Vec<SharedString> =
            PROPRIOS.iter().filter(|(nome, _)| nome.starts_with(path)).map(|(nome, _)| (*nome).into()).collect();
        nomes.extend(crate::icones_arquivo::listar(path));
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
