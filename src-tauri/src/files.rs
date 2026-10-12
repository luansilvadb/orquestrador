//! O explorador de arquivos do projeto: lê uma pasta por vez, sem escrever nada.
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

/// Uma entrada da pasta listada: só o nome, o caminho é o da pasta pedida mais esse nome.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

/// Um nível da pasta `rel` do projeto (com barras "/", vazio é a raiz). Pastas primeiro, cada grupo em
/// ordem alfabética sem diferenciar maiúsculas. Na raiz, o `.git` fica de fora.
pub fn list(project: &Path, rel: &str) -> Result<Vec<Entry>, String> {
    let dir = inside(project, rel)?;
    let root = rel.is_empty();
    let read = std::fs::read_dir(&dir).map_err(|e| format!("Não foi possível ler a pasta: {e}"))?;
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if root && name == ".git" {
                return None;
            }
            // `is_dir` segue link simbólico e junction; o que não dá para saber conta como arquivo.
            let dir = e.path().is_dir();
            Some(Entry { name, dir })
        })
        .collect();
    entries.sort_by_key(|e| (!e.dir, e.name.to_lowercase()));
    Ok(entries)
}

/// O caminho real de `rel` dentro do projeto. Recusa o que sai da pasta, o que não existe e o que não é pasta.
fn inside(project: &Path, rel: &str) -> Result<PathBuf, String> {
    // Só nomes normais: `..`, barra inicial, unidade (`D:`) e `.` ficam de fora.
    if !Path::new(rel).components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("Caminho fora do projeto: {rel}"));
    }
    let base = project.canonicalize().map_err(|e| format!("Não foi possível abrir o projeto: {e}"))?;
    let real = base.join(rel).canonicalize().map_err(|_| format!("Pasta não encontrada: {rel}"))?;
    // Um link simbólico que aponta para fora do projeto também sai da pasta.
    if !real.starts_with(&base) {
        return Err(format!("Caminho fora do projeto: {rel}"));
    }
    if !real.is_dir() {
        return Err(format!("Não é uma pasta: {rel}"));
    }
    Ok(real)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, dir: bool) -> Entry {
        Entry { name: name.into(), dir }
    }

    /// Um projeto de teste: pastas e arquivos com nomes que misturam caixa, mais um `.git` na raiz e outro na subpasta.
    fn fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("orq-files-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["Zeta", "alfa", "Beta", ".git", "sub/.git", "sub/dentro"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["b.txt", "A.txt", ".env", "sub/x.md"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        root
    }

    #[test]
    fn pastas_primeiro_e_alfabetico_sem_diferenciar_caixa() {
        let root = fixture("ordem");
        let got = list(&root, "").unwrap();
        assert_eq!(got, vec![e("alfa", true), e("Beta", true), e("sub", true), e("Zeta", true), e(".env", false), e("A.txt", false), e("b.txt", false)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn git_fica_fora_da_raiz_e_aparece_na_subpasta() {
        let root = fixture("git");
        assert!(!list(&root, "").unwrap().iter().any(|x| x.name == ".git"));
        assert_eq!(list(&root, "sub").unwrap(), vec![e(".git", true), e("dentro", true), e("x.md", false)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lista_so_um_nivel_da_subpasta() {
        let root = fixture("sub");
        assert_eq!(list(&root, "sub/dentro").unwrap(), vec![]);
        assert_eq!(list(&root, "Zeta").unwrap(), vec![]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn recusa_caminho_que_sai_da_pasta_ou_e_absoluto() {
        let root = fixture("fora");
        for rel in ["..", "../x", "sub/../../x", "/", "/etc", "C:/", "\\\\servidor\\x", "./sub", "sub/.."] {
            let got = list(&root, rel);
            assert!(got.is_err(), "{rel} deveria ser recusado");
        }
        let absoluto = root.to_string_lossy().into_owned();
        assert_eq!(list(&root, &absoluto).unwrap_err(), format!("Caminho fora do projeto: {absoluto}"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pasta_inexistente_e_arquivo_sao_recusados_em_portugues() {
        let root = fixture("inexistente");
        assert_eq!(list(&root, "nao-existe").unwrap_err(), "Pasta não encontrada: nao-existe");
        assert_eq!(list(&root, "sub/x.md").unwrap_err(), "Não é uma pasta: sub/x.md");
        let _ = std::fs::remove_dir_all(&root);
    }
}
