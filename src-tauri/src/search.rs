//! A busca de texto nos arquivos do projeto, pelo `git grep`: cobre o rastreado e o não rastreado, deixa de
//! fora o que o `.gitignore` ignora, os binários e o `.git`. O Git faz o trabalho; aqui só se lê a saída.
use crate::proc;
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;

/// Mais linhas casadas que isto não voltam: a resposta marca `truncated`.
const MAX_LINES: usize = 2000;
/// Tamanho máximo do texto de uma linha, em caracteres.
const MAX_TEXT: usize = 400;
/// Quantos caracteres antes da ocorrência a janela começa, quando a linha é mais longa que a janela.
const CONTEXT_BEFORE: usize = 40;

/// O resultado da busca. `files` vem em ordem alfabética por caminho; `truncated` avisa que passou de 2000 linhas.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    pub files: Vec<FileMatches>,
    pub truncated: bool,
}

/// Um arquivo com linhas casadas.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMatches {
    /// Relativo à raiz do projeto, com "/".
    pub path: String,
    /// Em ordem de número de linha; uma entrada por linha que casa.
    pub matches: Vec<Match>,
}

/// Uma linha que casa. `line` começa em 1.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Match {
    pub line: u32,
    /// A linha sem CR nem LF no fim; linha longa vem como janela.
    pub text: String,
}

/// Uma linha casada como o Git a deu, antes de ordenar e cortar.
struct Hit {
    path: String,
    line: u32,
    /// Coluna da primeira ocorrência, em bytes e a partir de 1.
    col: usize,
    text: String,
}

/// Busca `query` nos arquivos da pasta `dir`. Consulta vazia ou só de espaços não roda o Git. Regex inválida
/// e outras falhas devolvem o que o Git escreveu.
pub fn search(dir: &Path, query: &str, case_sensitive: bool, whole_word: bool, regex: bool) -> Result<Results, String> {
    if query.contains(['\n', '\r', '\0']) {
        return Err("A consulta não pode ter quebra de linha.".into());
    }
    if query.trim().is_empty() {
        return Ok(Results { files: Vec::new(), truncated: false });
    }
    let mut args = vec!["grep", "--untracked", "-I", "-n", "--column", "--no-color", "-z"];
    if !case_sensitive {
        args.push("-i");
    }
    if whole_word {
        args.push("-w");
    }
    args.push(if regex { "-P" } else { "-F" });
    // `-e` antes do padrão: um padrão que começa com hífen não vira opção.
    args.extend(["-e", query]);
    let mut hits = parse(&grep(dir, &args)?);
    hits.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    let truncated = hits.len() > MAX_LINES;
    hits.truncate(MAX_LINES);

    let mut files: Vec<FileMatches> = Vec::new();
    for hit in hits {
        let m = Match { line: hit.line, text: window(&hit.text, hit.col) };
        match files.last_mut() {
            Some(f) if f.path == hit.path => f.matches.push(m),
            _ => files.push(FileMatches { path: hit.path, matches: vec![m] }),
        }
    }
    Ok(Results { files, truncated })
}

/// Roda o `git grep` sem espera de digitação. Sem casamento o Git sai com código 1 e nada a dizer: vira saída
/// vazia. Qualquer outra falha devolve o que o Git escreveu.
fn grep(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = proc::quiet("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("git não pôde ser executado: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Some(1) => Ok(String::new()),
        code => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let why = stderr.trim();
            Err(if why.is_empty() { format!("código {}", code.unwrap_or(-1)) } else { why.to_string() })
        }
    }
}

/// Lê a saída de `grep -z -n --column`: cada registro é `caminho NUL linha NUL coluna NUL texto LF`. O NUL
/// separa os campos, então os dois-pontos do texto não atrapalham.
fn parse(out: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut rest = out;
    while let Some((path, after)) = rest.split_once('\0') {
        let Some((line, after)) = after.split_once('\0') else { break };
        let Some((col, after)) = after.split_once('\0') else { break };
        let (text, after) = after.split_once('\n').unwrap_or((after, ""));
        rest = after;
        let (Ok(line), Ok(col)) = (line.parse::<u32>(), col.parse::<usize>()) else { continue };
        hits.push(Hit { path: path.to_string(), line, col, text: text.to_string() });
    }
    hits
}

/// O texto de uma linha sem CR no fim. Linha com mais de 400 caracteres vira a janela de 400 que começa uns 40
/// caracteres antes da ocorrência (`col`, em bytes e a partir de 1), sem cortar um caractere no meio.
fn window(line: &str, col: usize) -> String {
    let text = line.trim_end_matches('\r');
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= MAX_TEXT {
        return text.to_string();
    }
    let mut at = col.saturating_sub(1).min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let hit = text[..at].chars().count();
    let start = hit.saturating_sub(CONTEXT_BEFORE);
    chars[start..(start + MAX_TEXT).min(chars.len())].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Pasta temporária do teste. Some quando o teste acaba, mesmo se ele falhar.
    struct Tmp(std::path::PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Tmp {
            let path = std::env::temp_dir().join(format!("orq-search-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Tmp(path)
        }

        fn dir(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        proc::git_saida(dir, args).unwrap();
    }

    /// Repositório novo, sem commit, com identidade e fim de linha fixos: nada do usuário nem do Windows entra.
    fn repo(tag: &str) -> Tmp {
        let tmp = Tmp::new(tag);
        git(tmp.dir(), &["init", "-q", "-b", "main"]);
        for (key, value) in [("user.name", "teste"), ("user.email", "teste@teste"), ("core.autocrlf", "false"), ("commit.gpgsign", "false")] {
            git(tmp.dir(), &["config", key, value]);
        }
        tmp
    }

    fn write(dir: &Path, rel: &str, data: impl AsRef<[u8]>) {
        let file = dir.join(rel);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, data).unwrap();
    }

    /// Tudo o que está na pasta vira arquivo rastreado.
    fn commit_all(dir: &Path) {
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "teste"]);
    }

    /// Busca literal, sem diferenciar caixa e sem palavra inteira.
    fn buscar(dir: &Path, query: &str) -> Results {
        search(dir, query, false, false, false).unwrap()
    }

    /// As linhas casadas de um arquivo, como (número, texto).
    fn linhas(r: &Results, path: &str) -> Vec<(u32, String)> {
        r.files.iter().find(|f| f.path == path).map(|f| f.matches.iter().map(|m| (m.line, m.text.clone())).collect()).unwrap_or_default()
    }

    fn caminhos(r: &Results) -> Vec<&str> {
        r.files.iter().map(|f| f.path.as_str()).collect()
    }

    #[test]
    fn texto_literal_diferencia_ou_nao_caixa() {
        let t = repo("caixa");
        write(t.dir(), "a.txt", "Ola mundo\nola minúsculo\nsem nada\n");
        commit_all(t.dir());
        assert_eq!(linhas(&buscar(t.dir(), "OLA"), "a.txt"), vec![(1, "Ola mundo".to_string()), (2, "ola minúsculo".to_string())]);
        let r = search(t.dir(), "ola", true, false, false).unwrap();
        assert_eq!(linhas(&r, "a.txt"), vec![(2, "ola minúsculo".to_string())]);
    }

    #[test]
    fn palavra_inteira_nao_pega_pedaco() {
        let t = repo("palavra");
        write(t.dir(), "a.txt", "ola\nolaria\nesta ola aqui\n");
        commit_all(t.dir());
        let r = search(t.dir(), "ola", false, true, false).unwrap();
        assert_eq!(linhas(&r, "a.txt"), vec![(1, "ola".to_string()), (3, "esta ola aqui".to_string())]);
    }

    #[test]
    fn regex_e_literal_com_o_mesmo_texto() {
        let t = repo("regex");
        write(t.dir(), "a.txt", "pag 12\npag \\d+\nsem\n");
        commit_all(t.dir());
        let regex = search(t.dir(), "\\d+", false, false, true).unwrap();
        assert_eq!(linhas(&regex, "a.txt"), vec![(1, "pag 12".to_string())]);
        let literal = search(t.dir(), "\\d+", false, false, false).unwrap();
        assert_eq!(linhas(&literal, "a.txt"), vec![(2, "pag \\d+".to_string())]);
    }

    #[test]
    fn padrao_com_hifen_nao_vira_opcao() {
        let t = repo("hifen");
        write(t.dir(), "a.txt", "um -x dois\nsem\n");
        commit_all(t.dir());
        assert_eq!(linhas(&buscar(t.dir(), "-x"), "a.txt"), vec![(1, "um -x dois".to_string())]);
        assert_eq!(linhas(&search(t.dir(), "-x", false, false, true).unwrap(), "a.txt"), vec![(1, "um -x dois".to_string())]);
    }

    #[test]
    fn arquivo_nao_rastreado_aparece_e_ignorado_nao() {
        let t = repo("ignorado");
        write(t.dir(), ".gitignore", "*.log\n");
        write(t.dir(), "rastreado.txt", "ola\n");
        commit_all(t.dir());
        write(t.dir(), "novo.txt", "ola novo\n");
        write(t.dir(), "oculto.log", "ola escondido\n");
        assert_eq!(caminhos(&buscar(t.dir(), "ola")), vec!["novo.txt", "rastreado.txt"]);
    }

    #[test]
    fn binario_fica_de_fora() {
        let t = repo("binario");
        write(t.dir(), "bin.dat", b"ola\0binario\n");
        write(t.dir(), "texto.txt", "ola\n");
        commit_all(t.dir());
        assert_eq!(caminhos(&buscar(t.dir(), "ola")), vec!["texto.txt"]);
    }

    #[test]
    fn nada_encontrado_da_resposta_vazia() {
        let t = repo("vazio");
        write(t.dir(), "a.txt", "abc\n");
        commit_all(t.dir());
        let r = buscar(t.dir(), "zzz");
        assert!(r.files.is_empty());
        assert!(!r.truncated);
    }

    #[test]
    fn consulta_vazia_nao_roda_o_git() {
        // Pasta que não existe: se o Git rodasse, daria erro.
        let longe = std::env::temp_dir().join("orq-search-nao-existe-consulta");
        for query in ["", "   "] {
            let r = search(&longe, query, false, false, false).unwrap();
            assert!(r.files.is_empty());
            assert!(!r.truncated);
        }
    }

    #[test]
    fn regex_invalida_da_erro_do_git() {
        let t = repo("invalida");
        write(t.dir(), "a.txt", "abc\n");
        commit_all(t.dir());
        let err = search(t.dir(), "(", false, false, true).unwrap_err();
        assert!(err.contains("fatal"), "{err}");
    }

    #[test]
    fn nome_com_espaco_e_acento() {
        let t = repo("nome");
        write(t.dir(), "arq com espaço é.txt", "ola\n");
        commit_all(t.dir());
        assert_eq!(caminhos(&buscar(t.dir(), "ola")), vec!["arq com espaço é.txt"]);
    }

    #[test]
    fn linha_com_crlf_sai_sem_cr() {
        let t = repo("crlf");
        write(t.dir(), "a.txt", "ola crlf\r\nsegunda\r\n");
        commit_all(t.dir());
        assert_eq!(linhas(&buscar(t.dir(), "ola"), "a.txt"), vec![(1, "ola crlf".to_string())]);
    }

    #[test]
    fn dois_pontos_no_texto_nao_atrapalham() {
        let t = repo("dois-pontos");
        write(t.dir(), "a.txt", "chave: ola: 12:30:00\n");
        commit_all(t.dir());
        assert_eq!(linhas(&buscar(t.dir(), "ola"), "a.txt"), vec![(1, "chave: ola: 12:30:00".to_string())]);
    }

    #[test]
    fn linha_longa_cortada_contem_a_ocorrencia() {
        let t = repo("longa");
        let longa = format!("{}ALVO{}", "a".repeat(1000), "b".repeat(1000));
        write(t.dir(), "a.txt", format!("{longa}\n"));
        commit_all(t.dir());
        let r = buscar(t.dir(), "alvo");
        let texto = &linhas(&r, "a.txt")[0].1;
        assert_eq!(texto, &format!("{}ALVO{}", "a".repeat(40), "b".repeat(400 - 44)));
    }

    #[test]
    fn linha_longa_com_acento_corta_no_caractere() {
        let t = repo("acento-longa");
        // 500 caracteres de dois bytes antes da ocorrência: a coluna em bytes cai sobre um limite de caractere.
        write(t.dir(), "a.txt", format!("{}ALVO\n", "ç".repeat(500)));
        commit_all(t.dir());
        let r = buscar(t.dir(), "alvo");
        assert_eq!(linhas(&r, "a.txt")[0].1, format!("{}ALVO", "ç".repeat(40)));
    }

    #[test]
    fn corte_em_2000_linhas_marca_truncated() {
        let t = repo("corte");
        let mut data = String::new();
        for i in 1..=2100 {
            data += &format!("linha ola {i}\n");
        }
        write(t.dir(), "grande.txt", data);
        commit_all(t.dir());
        let r = buscar(t.dir(), "ola");
        assert!(r.truncated);
        let total: usize = r.files.iter().map(|f| f.matches.len()).sum();
        assert_eq!(total, 2000);
        assert_eq!(r.files[0].matches.first().unwrap().line, 1);
        assert_eq!(r.files[0].matches.last().unwrap().line, 2000);
    }

    #[test]
    fn ordem_de_arquivos_e_de_linhas() {
        let t = repo("ordem");
        write(t.dir(), "z.txt", "ola a\nnada\nola b\nola c\n");
        write(t.dir(), "a.txt", "ola\n");
        commit_all(t.dir());
        // Não rastreado, no meio da ordem alfabética: tem de sair entre a.txt e z.txt.
        write(t.dir(), "m/b.txt", "ola\n");
        let r = buscar(t.dir(), "ola");
        assert_eq!(caminhos(&r), vec!["a.txt", "m/b.txt", "z.txt"]);
        assert_eq!(linhas(&r, "z.txt").iter().map(|(n, _)| *n).collect::<Vec<_>>(), vec![1, 3, 4]);
    }
}
