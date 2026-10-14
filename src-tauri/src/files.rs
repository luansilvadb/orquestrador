//! O explorador de arquivos do projeto: lista pastas, lê e grava conteúdo de arquivos.
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

/// Arquivo com mais que isto não é lido: a tela mostra só o aviso.
const MAX_FILE: u64 = 1_048_576;
/// Quantos bytes do começo de um arquivo olhar para decidir se é binário (a mesma regra do Git).
const BINARY_SNIFF: usize = 8000;

/// Uma entrada da pasta listada: só o nome, o caminho é o da pasta pedida mais esse nome.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

/// O conteúdo de um arquivo. `text` vai sem BOM e vazio quando o arquivo é binário ou grande demais.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    pub binary: bool,
    pub too_large: bool,
    pub read_only: bool,
    pub text: String,
}

/// Um nível da pasta `rel` do projeto (com barras "/", vazio é a raiz). Pastas primeiro, cada grupo em
/// ordem alfabética sem diferenciar maiúsculas. Na raiz, o `.git` fica de fora.
pub fn list(project: &Path, rel: &str) -> Result<Vec<Entry>, String> {
    let dir = inside(project, rel, true)?;
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

/// O arquivo `rel` do projeto (com barras "/"). Arquivo grande ou binário volta sem `text`.
pub fn read(project: &Path, rel: &str) -> Result<Content, String> {
    let file = inside(project, rel, false)?;
    let size = std::fs::metadata(&file).map_err(|e| format!("Não foi possível ler o arquivo: {e}"))?.len();
    if size > MAX_FILE {
        return Ok(Content { binary: false, too_large: true, read_only: false, text: String::new() });
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("Não foi possível ler o arquivo: {e}"))?;
    if bytes[..bytes.len().min(BINARY_SNIFF)].contains(&0) {
        return Ok(Content { binary: true, too_large: false, read_only: false, text: String::new() });
    }
    let text = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes[..]);
    Ok(Content { binary: false, too_large: false, read_only: std::str::from_utf8(text).is_err(), text: String::from_utf8_lossy(text).into_owned() })
}

/// Grava o texto em um arquivo existente do projeto, preservando o BOM UTF-8 quando houver.
pub fn write(project: &Path, rel: &str, text: &str) -> Result<(), String> {
    let file = inside(project, rel, false)?;
    if text.len() as u64 > MAX_FILE {
        return Err("O arquivo ficaria grande demais para abrir aqui.".into());
    }
    let current = std::fs::read(&file).map_err(|e| format!("Não foi possível ler o arquivo: {e}"))?;
    if current.len() as u64 > MAX_FILE || current[..current.len().min(BINARY_SNIFF)].contains(&0) {
        return Err("Não é possível gravar um arquivo binário.".into());
    }
    let (bom, content) = match current.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(content) => (true, content),
        None => (false, current.as_slice()),
    };
    if std::str::from_utf8(content).is_err() {
        return Err("Não é possível gravar um arquivo que não é UTF-8 válido.".into());
    }
    let mut bytes = Vec::with_capacity(text.len() + if bom { 3 } else { 0 });
    if bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(&file, bytes).map_err(|e| format!("Não foi possível gravar o arquivo: {e}"))
}

/// O caminho real de `rel` dentro do projeto. Recusa o que sai da pasta, o que não existe e o que não é do tipo pedido
/// (pasta se `dir`, senão arquivo).
fn inside(project: &Path, rel: &str, dir: bool) -> Result<PathBuf, String> {
    // Só nomes normais: `..`, barra inicial, unidade (`D:`) e `.` ficam de fora.
    if !Path::new(rel).components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("Caminho fora do projeto: {rel}"));
    }
    let base = project.canonicalize().map_err(|e| format!("Não foi possível abrir o projeto: {e}"))?;
    let real = base.join(rel).canonicalize().map_err(|_| {
        if dir { format!("Pasta não encontrada: {rel}") } else { format!("Arquivo não encontrado: {rel}") }
    })?;
    // Um link simbólico que aponta para fora do projeto também sai da pasta.
    if !real.starts_with(&base) {
        return Err(format!("Caminho fora do projeto: {rel}"));
    }
    if real.is_dir() != dir {
        return Err(if dir { format!("Não é uma pasta: {rel}") } else { format!("Não é um arquivo: {rel}") });
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

    #[test]
    fn texto_com_acento_e_crlf_volta_igual_e_sem_bom() {
        let root = fixture("texto");
        std::fs::write(root.join("sub/nota.md"), "\u{feff}Olá\r\nmundo\r\n").unwrap();
        assert_eq!(read(&root, "sub/nota.md").unwrap(), Content { binary: false, too_large: false, read_only: false, text: "Olá\r\nmundo\r\n".into() });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn arquivo_vazio_devolve_texto_vazio() {
        let root = fixture("vazio");
        assert_eq!(read(&root, "A.txt").unwrap(), Content { binary: false, too_large: false, read_only: false, text: String::new() });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn arquivo_com_byte_nulo_e_binario() {
        let root = fixture("binario");
        std::fs::write(root.join("img.bin"), [0x89, b'P', 0, b'N', b'G']).unwrap();
        assert_eq!(read(&root, "img.bin").unwrap(), Content { binary: true, too_large: false, read_only: false, text: String::new() });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn arquivo_com_mais_de_1_mib_volta_too_large() {
        let root = fixture("grande");
        std::fs::write(root.join("grande.txt"), vec![b'a'; 1_048_577]).unwrap();
        assert_eq!(read(&root, "grande.txt").unwrap(), Content { binary: false, too_large: true, read_only: false, text: String::new() });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn recusa_caminho_de_leitura_que_sai_da_pasta_ou_e_absoluto() {
        let root = fixture("ler-fora");
        for rel in ["..", "../x", "sub/../../x", "/etc", "C:/", "./sub/x.md"] {
            let got = read(&root, rel);
            assert!(got.is_err(), "{rel} deveria ser recusado");
        }
        let absoluto = root.join("sub/x.md").to_string_lossy().into_owned();
        assert_eq!(read(&root, &absoluto).unwrap_err(), format!("Caminho fora do projeto: {absoluto}"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn arquivo_inexistente_e_pasta_sao_recusados_em_portugues() {
        let root = fixture("ler-inexistente");
        assert_eq!(read(&root, "nao-existe.md").unwrap_err(), "Arquivo não encontrado: nao-existe.md");
        assert_eq!(read(&root, "sub").unwrap_err(), "Não é um arquivo: sub");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn grava_e_rele_acentos_e_crlf_iguais() {
        let root = fixture("grava-texto");
        write(&root, "A.txt", "Olá\r\nmundo\r\n").unwrap();
        assert_eq!(read(&root, "A.txt").unwrap().text, "Olá\r\nmundo\r\n");
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), "Olá\r\nmundo\r\n".as_bytes());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn preserva_bom_existente_e_nao_adiciona_outro() {
        let root = fixture("grava-bom");
        std::fs::write(root.join("A.txt"), b"\xEF\xBB\xBFantes").unwrap();
        write(&root, "A.txt", "depois").unwrap();
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), b"\xEF\xBB\xBFdepois");
        write(&root, "b.txt", "depois").unwrap();
        assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"depois");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn utf8_invalido_fica_read_only_e_gravacao_nao_muda_bytes() {
        let root = fixture("grava-utf8-invalido");
        let original = [b'a', 0xFF, b'b'];
        std::fs::write(root.join("A.txt"), original).unwrap();
        assert_eq!(read(&root, "A.txt").unwrap().read_only, true);
        assert!(write(&root, "A.txt", "substituto").is_err());
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), original);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn binario_e_recusado_na_gravacao() {
        let root = fixture("grava-binario");
        let original = [b'a', 0, b'b'];
        std::fs::write(root.join("A.txt"), original).unwrap();
        assert!(write(&root, "A.txt", "texto").is_err());
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), original);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn texto_acima_de_1_mib_e_recusado() {
        let root = fixture("grava-grande");
        let erro = write(&root, "A.txt", &"a".repeat(1_048_577)).unwrap_err();
        assert!(erro.contains("grande demais para abrir aqui"));
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), b"");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn caminho_invalido_arquivo_inexistente_e_pasta_nao_criam_nada() {
        let root = fixture("grava-caminhos");
        assert!(write(&root, "../novo.txt", "x").is_err());
        assert_eq!(write(&root, "nao-existe.txt", "x").unwrap_err(), "Arquivo não encontrado: nao-existe.txt");
        assert_eq!(write(&root, "sub", "x").unwrap_err(), "Não é um arquivo: sub");
        assert!(!root.join("novo.txt").exists());
        assert!(!root.join("nao-existe.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
