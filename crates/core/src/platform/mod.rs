//! Tudo o que é específico do sistema operacional: processos e Job, consulta de processo, COM, Lixeira, Explorer,
//! clipboard, memória, pastas do usuário e os nomes dos shells. `windows.rs` implementa; `other.rs` (macOS, Linux)
//! responde "Ainda não suportado neste sistema.", ou `None`/vazio quando o chamador já trata a ausência. Fora daqui
//! nenhum módulo chama a API do sistema diretamente.
use std::path::PathBuf;

#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
mod other;

#[cfg(windows)]
pub use windows::{
    abrir_para_gravar, abrir_pasta_no_explorador, clear_if_same, cli_do_processo, copiar_ou_mover, em_uso,
    ja_existe, local_padrao_do_cli, mandar_para_lixeira, memoria, mover_sem_substituir, open_url, pastas_do_path_registrado,
    por_no_path_do_usuario, powershell_do_sistema,
    pasta_local_do_usuario, pasta_pessoal, revelar_no_explorador,
    sem_janela, set_files, situacao_do_processo, snapshot, spawn_com_prazo, start_time, symlink_arquivo, Job, SHELLS,
};
#[cfg(not(windows))]
pub use other::{
    abrir_para_gravar, abrir_pasta_no_explorador, clear_if_same, cli_do_processo, copiar_ou_mover, em_uso,
    ja_existe, local_padrao_do_cli, mandar_para_lixeira, memoria, mover_sem_substituir, open_url, pastas_do_path_registrado,
    por_no_path_do_usuario, powershell_do_sistema,
    pasta_local_do_usuario, pasta_pessoal, revelar_no_explorador,
    sem_janela, set_files, situacao_do_processo, snapshot, spawn_com_prazo, start_time, symlink_arquivo, Job, SHELLS,
};

/// Uma pasta na forma de comparar: sem espaços nem aspas em volta, sem a barra final (`\` ou `/`) e em minúsculas.
pub fn normalizar_pasta(p: &str) -> String {
    p.trim().trim_matches('"').trim().trim_end_matches(['\\', '/']).to_lowercase()
}

/// Os itens de um valor de PATH: divididos por `;`, sem espaços nem aspas em volta e sem os vazios.
pub fn itens_do_path(valor: &str) -> impl Iterator<Item = &str> {
    valor.split(';').map(|p| p.trim().trim_matches('"').trim()).filter(|p| !p.is_empty())
}

/// Os arquivos no clipboard: os caminhos, na ordem, e se foram recortados (`cut`) ou copiados. `paths` vem vazio quando
/// o clipboard não tem arquivos.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

/// O que `situacao_do_processo` acha: não existe, existe mas não dá para ler, ou foi lido (a hora de início e se ainda roda).
#[derive(Debug, PartialEq)]
pub enum Situacao {
    Inexistente,
    Ilegivel,
    Lido { criada: u64, rodando: bool },
}

/// Por que uma operação de shell (Lixeira ou colagem) não foi feita. A mensagem para o usuário fica em `files.rs`.
#[derive(Debug, PartialEq)]
pub enum ErroShell {
    /// O COM não iniciou na thread.
    SemCom,
    /// O Windows não criou a operação.
    SemOperacao,
    /// O caminho do item não foi achado pelo Shell.
    ItemAusente,
    /// O Windows recusou, abortou ou não executou a operação.
    Recusada,
    /// Este sistema ainda não faz a operação.
    NaoSuportado,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasta_normalizada_ignora_caixa_barra_final_aspas_e_espacos() {
        let base = normalizar_pasta(r"C:\Users\u\.local\bin");
        for variante in [r"c:\users\U\.LOCAL\bin", r"C:\Users\u\.local\bin\", r"C:\Users\u\.local\bin/", r#" "C:\Users\u\.local\bin\" "#] {
            assert_eq!(normalizar_pasta(variante), base, "{variante}");
        }
        assert_ne!(normalizar_pasta(r"C:\Users\u\.local"), base);
    }

    #[test]
    fn itens_do_path_sem_vazios_nem_aspas() {
        let itens: Vec<&str> = itens_do_path(r#"C:\a;; "C:\b c" ;C:\d\;"#).collect();
        assert_eq!(itens, [r"C:\a", r"C:\b c", r"C:\d\"]);
    }
}
