//! O painel Git do projeto: o estado da árvore, o diff de um arquivo, preparar e tirar do índice, o commit,
//! a sincronização com o remoto e a troca de branch. Tudo passa pelo `git` com `proc::git_saida` ou
//! `proc::git_completa`, então nada espera digitação e o erro traz o que o Git escreveu.
use crate::proc;
use serde::Serialize;
use std::path::{Component, Path};

/// Diff com mais texto que isto não é montado: a tela mostra só o aviso.
const MAX_DIFF: usize = 1_048_576;
/// Quantos bytes do começo de um arquivo o Git olha para decidir se ele é binário.
const BINARY_SNIFF: usize = 8000;

/// O que aconteceu com o arquivo num dos lados (índice ou pasta de trabalho).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

/// Um arquivo alterado. `staged` é a mudança no índice, `unstaged` a da pasta de trabalho; um arquivo pode ter as duas.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFile {
    /// Relativo à raiz do projeto, com "/".
    pub path: String,
    /// De onde veio, só no renomeado.
    pub orig_path: Option<String>,
    pub staged: Option<Kind>,
    pub unstaged: Option<Kind>,
}

/// O estado do repositório: a branch (ou o hash em HEAD destacado), o quanto está à frente e atrás do remoto e o que está alterado.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    /// Em ordem alfabética por caminho.
    pub files: Vec<GitFile>,
}

/// O diff de um arquivo. `patch` é o diff unificado bruto (cabeçalho `diff --git`, `---`, `+++` e os `@@`); vazio quando não há alteração.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub binary: bool,
    pub too_large: bool,
    pub patch: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branches {
    pub current: String,
    /// Branches locais, em ordem alfabética.
    pub local: Vec<String>,
}

/// O estado do repositório. A branch segue a regra de `proc::branch`: o nome, ou o hash em HEAD destacado.
pub fn status(dir: &Path) -> Result<Status, String> {
    // `-z` põe os caminhos sem aspas e separa os registros com NUL; `core.quotepath` já vem desligado (ver `proc`).
    let out = proc::git_saida(dir, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"])?;
    Ok(parse_status(proc::branch(dir)?, &out))
}

/// Lê a saída de `status --porcelain=v2 --branch -z`. Cada registro acaba em NUL; o renomeado traz o caminho
/// de origem no registro seguinte.
fn parse_status(branch: String, out: &str) -> Status {
    let mut st = Status { branch, ahead: 0, behind: 0, files: Vec::new() };
    let mut records = out.split('\0');
    while let Some(rec) = records.next() {
        if let Some(ab) = rec.strip_prefix("# branch.ab ") {
            let mut counts = ab.split_whitespace();
            st.ahead = counts.next().and_then(|n| n.trim_start_matches('+').parse().ok()).unwrap_or(0);
            st.behind = counts.next().and_then(|n| n.trim_start_matches('-').parse().ok()).unwrap_or(0);
        } else if let Some(rest) = rec.strip_prefix("1 ") {
            // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            let f: Vec<&str> = rest.splitn(8, ' ').collect();
            if let [xy, _, _, _, _, _, _, path] = f.as_slice() {
                st.files.push(entry(xy, path, None));
            }
        } else if let Some(rest) = rec.strip_prefix("2 ") {
            // 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>, e o caminho de origem no registro seguinte
            let f: Vec<&str> = rest.splitn(9, ' ').collect();
            let orig = records.next();
            if let [xy, _, _, _, _, _, _, _, path] = f.as_slice() {
                st.files.push(entry(xy, path, orig));
            }
        } else if let Some(rest) = rec.strip_prefix("u ") {
            // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>: conflito
            let f: Vec<&str> = rest.splitn(10, ' ').collect();
            if let [_, _, _, _, _, _, _, _, _, path] = f.as_slice() {
                st.files.push(GitFile { path: path.to_string(), orig_path: None, staged: None, unstaged: Some(Kind::Conflicted) });
            }
        } else if let Some(path) = rec.strip_prefix("? ") {
            st.files.push(GitFile { path: path.to_string(), orig_path: None, staged: None, unstaged: Some(Kind::Untracked) });
        }
    }
    st.files.sort_by(|a, b| a.path.cmp(&b.path));
    st
}

/// Um registro `1` ou `2`: as duas letras do `XY` dizem o que mudou no índice (X) e na pasta (Y).
fn entry(xy: &str, path: &str, orig: Option<&str>) -> GitFile {
    let mut letters = xy.chars();
    let (x, y) = (letters.next().unwrap_or('.'), letters.next().unwrap_or('.'));
    GitFile { path: path.to_string(), orig_path: orig.map(str::to_string), staged: kind(x), unstaged: kind(y) }
}

fn kind(letter: char) -> Option<Kind> {
    match letter {
        'M' | 'T' => Some(Kind::Modified),
        'A' | 'C' => Some(Kind::Added),
        'D' => Some(Kind::Deleted),
        'R' => Some(Kind::Renamed),
        _ => None,
    }
}

/// O diff de `rel`. `staged` compara o índice com o HEAD; senão, a pasta de trabalho com o índice. Arquivo não
/// rastreado vira o arquivo inteiro como linhas acrescentadas.
pub fn diff(dir: &Path, rel: &str, staged: bool) -> Result<Diff, String> {
    let rel = spec(rel)?;
    if !staged && proc::git_saida(dir, &["ls-files", "--", rel])?.is_empty() {
        return untracked(&dir.join(rel), rel);
    }
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames", "-U3"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", rel]);
    Ok(read_diff(&proc::git_saida(dir, &args)?))
}

/// A saída do `git diff`: binário e diff grande viram aviso; o resto segue como patch, sem mexer no texto.
fn read_diff(text: &str) -> Diff {
    if text.lines().any(|l| l.starts_with("Binary files ")) {
        return Diff { binary: true, too_large: false, patch: String::new() };
    }
    if text.len() > MAX_DIFF {
        return Diff { binary: false, too_large: true, patch: String::new() };
    }
    Diff { binary: false, too_large: false, patch: text.to_string() }
}

/// Um arquivo que o Git não conhece: um patch de arquivo novo, com todas as linhas acrescentadas. Sem o arquivo no disco, não há diff.
fn untracked(file: &Path, rel: &str) -> Result<Diff, String> {
    let empty = Diff { binary: false, too_large: false, patch: String::new() };
    if !file.is_file() {
        return Ok(empty);
    }
    let io = |e: std::io::Error| format!("Não foi possível ler {}: {e}", file.display());
    if std::fs::metadata(file).map_err(io)?.len() > MAX_DIFF as u64 {
        return Ok(Diff { too_large: true, ..empty });
    }
    let bytes = std::fs::read(file).map_err(io)?;
    if bytes[..bytes.len().min(BINARY_SNIFF)].contains(&0) {
        return Ok(Diff { binary: true, ..empty });
    }
    let lines: Vec<String> = String::from_utf8_lossy(&bytes).lines().map(str::to_string).collect();
    if lines.is_empty() {
        return Ok(empty);
    }
    let body: String = lines.iter().map(|l| format!("+{l}\n")).collect();
    let patch = format!(
        "diff --git a/{rel} b/{rel}\nnew file mode 100644\n--- /dev/null\n+++ b/{rel}\n@@ -0,0 +1,{} @@\n{body}",
        lines.len()
    );
    Ok(Diff { patch, ..empty })
}

/// Prepara os caminhos. Lista vazia prepara tudo, inclusive o não rastreado e o apagado.
pub fn stage(dir: &Path, paths: &[String]) -> Result<(), String> {
    let specs = specs(paths)?;
    let mut args = vec!["add", "-A"];
    if !specs.is_empty() {
        args.push("--");
        args.extend(specs);
    }
    proc::git_saida(dir, &args).map(|_| ())
}

/// Tira os caminhos do índice; a pasta de trabalho fica como está. Lista vazia tira tudo.
pub fn unstage(dir: &Path, paths: &[String]) -> Result<(), String> {
    let specs = specs(paths)?;
    if has_head(dir) {
        let mut args = vec!["reset", "-q"];
        if !specs.is_empty() {
            args.push("--");
            args.extend(specs);
        }
        return proc::git_saida(dir, &args).map(|_| ());
    }
    // Sem commit não há HEAD para o `reset`. Tirar do índice é `rm --cached`, que não apaga nada da pasta.
    let mut args = vec!["rm", "-r", "--cached", "-q", "-f", "--ignore-unmatch", "--"];
    args.extend(if specs.is_empty() { vec!["."] } else { specs });
    proc::git_saida(dir, &args).map(|_| ())
}

fn has_head(dir: &Path) -> bool {
    proc::git_saida(dir, &["rev-parse", "-q", "--verify", "HEAD"]).is_ok()
}

/// Commita o que está no índice. Os hooks do repositório rodam normalmente.
pub fn commit(dir: &Path, message: &str) -> Result<(), String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Escreva a mensagem do commit.".into());
    }
    if proc::git_saida(dir, &["diff", "--cached", "--name-only", "-z"])?.is_empty() {
        return Err("Não há nada preparado para o commit.".into());
    }
    proc::git_saida(dir, &["commit", "-q", "-m", message]).map(|_| ())
}

/// Busca do remoto. Devolve a saída do Git.
pub fn fetch(dir: &Path) -> Result<String, String> {
    remotes(dir)?;
    proc::git_completa(dir, &["fetch"])
}

/// `git pull` simples: respeita a configuração do usuário, e sem editor para o commit de merge.
pub fn pull(dir: &Path) -> Result<String, String> {
    remotes(dir)?;
    proc::git_completa(dir, &["pull"])
}

/// Envia a branch. Sem upstream, publica em `origin` e configura o upstream.
pub fn push(dir: &Path) -> Result<String, String> {
    let names = remotes(dir)?;
    if upstream(dir).is_some() {
        return proc::git_completa(dir, &["push"]);
    }
    if !names.iter().any(|n| n == "origin") {
        return Err("A branch atual não tem upstream e não há remoto origin para publicá-la.".into());
    }
    proc::git_completa(dir, &["push", "-u", "origin", "HEAD"])
}

/// Os nomes dos remotos. Sem nenhum, nada de sincronizar.
fn remotes(dir: &Path) -> Result<Vec<String>, String> {
    let names: Vec<String> = proc::git_saida(dir, &["remote"])?.lines().map(str::to_string).collect();
    if names.is_empty() {
        return Err("Nenhum remoto configurado neste repositório. Configure um remoto, como origin, para sincronizar.".into());
    }
    Ok(names)
}

/// O upstream da branch atual, se ela tiver um.
fn upstream(dir: &Path) -> Option<String> {
    proc::git_saida(dir, &["rev-parse", "-q", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A branch atual e as branches locais.
pub fn branches(dir: &Path) -> Result<Branches, String> {
    let refs = proc::git_saida(dir, &["for-each-ref", "--format=%(refname)", "refs/heads/"])?;
    let mut local: Vec<String> = refs.lines().filter_map(|r| r.strip_prefix("refs/heads/")).map(str::to_string).collect();
    local.sort();
    Ok(Branches { current: proc::branch(dir)?, local })
}

/// Troca para uma branch local existente. Sem `--no-guess` o Git criaria uma branch a partir de um remoto de mesmo nome.
pub fn switch(dir: &Path, name: &str) -> Result<(), String> {
    ensure_name(dir, name)?;
    proc::git_saida(dir, &["switch", "--no-guess", name]).map(|_| ())
}

/// Cria a branch a partir do HEAD e troca para ela.
pub fn create_branch(dir: &Path, name: &str) -> Result<(), String> {
    ensure_name(dir, name)?;
    let reference = format!("refs/heads/{name}");
    let exists = proc::git_saida(dir, &["show-ref", "--verify", "--quiet", &reference]).is_ok();
    if exists {
        return Err(format!("Já existe uma branch chamada {name}."));
    }
    proc::git_saida(dir, &["switch", "-c", name]).map(|_| ())
}

/// Nome que o Git aceita como branch e que não começa por hífen (senão vira opção do comando).
fn ensure_name(dir: &Path, name: &str) -> Result<(), String> {
    let reference = format!("refs/heads/{name}");
    let valid = !name.starts_with('-') && proc::git_saida(dir, &["check-ref-format", &reference]).is_ok();
    if valid {
        Ok(())
    } else {
        Err(format!("Nome de branch inválido: {name}"))
    }
}

/// Caminhos do projeto como pathspec: só nomes normais, sem barra inicial, unidade, `..` nem `.`.
fn spec(rel: &str) -> Result<&str, String> {
    if rel.is_empty() {
        return Err("Caminho vazio.".into());
    }
    if !Path::new(rel).components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("Caminho fora do projeto: {rel}"));
    }
    Ok(rel)
}

fn specs(paths: &[String]) -> Result<Vec<&str>, String> {
    paths.iter().map(|p| spec(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Pasta temporária do teste. Some quando o teste acaba, mesmo se ele falhar.
    struct Tmp(std::path::PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Tmp {
            let path = std::env::temp_dir().join(format!("orq-git-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Tmp(path)
        }

        fn at(&self, name: &str) -> std::path::PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        proc::git_saida(dir, args).unwrap()
    }

    /// Identidade e fim de linha fixos na configuração local: nada do usuário nem do Windows entra no teste.
    fn identity(dir: &Path) {
        for (key, value) in [("user.name", "teste"), ("user.email", "teste@teste"), ("core.autocrlf", "false"), ("core.safecrlf", "false"), ("commit.gpgsign", "false")] {
            git(dir, &["config", key, value]);
        }
    }

    /// Repositório novo, sem commit, com a branch `main`.
    fn repo(tmp: &Tmp, name: &str) -> std::path::PathBuf {
        let dir = tmp.at(name);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        identity(&dir);
        dir
    }

    fn write(dir: &Path, rel: &str, data: impl AsRef<[u8]>) {
        let file = dir.join(rel);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, data).unwrap();
    }

    fn commit_all(dir: &Path, message: &str) {
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", message]);
    }

    fn find<'a>(st: &'a Status, path: &str) -> &'a GitFile {
        st.files.iter().find(|f| f.path == path).unwrap_or_else(|| panic!("{path} não está no status: {st:?}"))
    }

    fn git_status_of(dir: &Path) -> Status {
        status(dir).unwrap()
    }

    #[test]
    fn status_mostra_cada_mudanca_no_lugar_certo() {
        let tmp = Tmp::new("status");
        let dir = repo(&tmp, "r");
        write(&dir, "modificado.txt", "um\n");
        write(&dir, "apagado.txt", "x\n");
        write(&dir, "antigo ação.txt", "um texto comprido o bastante para o Git ver a renomeação sem dúvida\n".repeat(3));
        write(&dir, "parcial.txt", "1\n");
        commit_all(&dir, "base");

        write(&dir, "modificado.txt", "dois\n");
        fs::remove_file(dir.join("apagado.txt")).unwrap();
        git(&dir, &["mv", "antigo ação.txt", "novo x.txt"]);
        write(&dir, "parcial.txt", "2\n");
        git(&dir, &["add", "parcial.txt"]);
        write(&dir, "parcial.txt", "3\n");
        write(&dir, "pasta nova/arq ção.txt", "a");
        write(&dir, "novo.txt", "n");
        git(&dir, &["add", "novo.txt"]);

        let st = git_status_of(&dir);
        assert_eq!(st.branch, "main");
        assert_eq!((upstream(&dir), st.ahead, st.behind), (None, 0, 0));
        assert_eq!(find(&st, "modificado.txt"), &GitFile { path: "modificado.txt".into(), orig_path: None, staged: None, unstaged: Some(Kind::Modified) });
        assert_eq!(find(&st, "apagado.txt").unstaged, Some(Kind::Deleted));
        let renamed = find(&st, "novo x.txt");
        assert_eq!(renamed.orig_path.as_deref(), Some("antigo ação.txt"));
        assert_eq!((renamed.staged, renamed.unstaged), (Some(Kind::Renamed), None));
        let partial = find(&st, "parcial.txt");
        assert_eq!((partial.staged, partial.unstaged), (Some(Kind::Modified), Some(Kind::Modified)));
        assert_eq!(find(&st, "pasta nova/arq ção.txt").unstaged, Some(Kind::Untracked));
        assert_eq!(find(&st, "novo.txt").staged, Some(Kind::Added));
        let paths: Vec<&str> = st.files.iter().map(|f| f.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "os arquivos saem em ordem alfabética");
    }

    #[test]
    fn status_de_conflito_e_de_head_destacado_e_repositorio_sem_commit() {
        let tmp = Tmp::new("conflito");
        let dir = repo(&tmp, "r");
        write(&dir, "x.txt", "base\n");
        commit_all(&dir, "base");
        git(&dir, &["switch", "-q", "-c", "outra"]);
        write(&dir, "x.txt", "outra\n");
        commit_all(&dir, "outra");
        git(&dir, &["switch", "-q", "main"]);
        write(&dir, "x.txt", "main\n");
        commit_all(&dir, "main");
        // O merge para no conflito; o erro é o esperado.
        let _ = proc::git_saida(&dir, &["merge", "-q", "outra"]);
        let st = git_status_of(&dir);
        assert_eq!(find(&st, "x.txt").staged, None);
        assert_eq!(find(&st, "x.txt").unstaged, Some(Kind::Conflicted));
        git(&dir, &["merge", "--abort"]);

        git(&dir, &["checkout", "-q", "--detach"]);
        let hash = git(&dir, &["rev-parse", "--short", "HEAD"]);
        assert_eq!(git_status_of(&dir).branch, hash.trim());

        let empty = repo(&tmp, "vazio");
        write(&empty, "a.txt", "a");
        let st = git_status_of(&empty);
        assert_eq!(st.branch, "main");
        assert_eq!(find(&st, "a.txt").unstaged, Some(Kind::Untracked));
    }

    #[test]
    fn status_conta_ahead_e_behind_contra_o_remoto_local() {
        let tmp = Tmp::new("ahead");
        let (bare, dir) = with_remote(&tmp);
        let st = git_status_of(&dir);
        assert_eq!((upstream(&dir).as_deref(), st.ahead, st.behind), (Some("origin/main"), 0, 0));

        write(&dir, "b.txt", "b");
        commit_all(&dir, "b");
        assert_eq!(git_status_of(&dir).ahead, 1);

        other_pushes(&tmp, &bare, "c.txt");
        git(&dir, &["fetch", "-q"]);
        let st = git_status_of(&dir);
        assert_eq!((st.ahead, st.behind), (1, 1));
    }

    #[test]
    fn diff_traz_os_numeros_de_linha_de_cada_lado() {
        let tmp = Tmp::new("diff-linhas");
        let dir = repo(&tmp, "r");
        write(&dir, "f.txt", "a\nb\nc\nd\n");
        commit_all(&dir, "base");
        write(&dir, "f.txt", "a\nB\nc\nd\ne\n");
        let d = diff(&dir, "f.txt", false).unwrap();
        assert!(!d.binary && !d.too_large);
        // O hash do `index` muda a cada commit: o teste olha o cabeçalho e o corpo do hunk, que são fixos.
        assert!(d.patch.starts_with("diff --git a/f.txt b/f.txt\n"), "cabeçalho: {}", d.patch);
        assert!(d.patch.ends_with("--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,5 @@\n a\n-b\n+B\n c\n d\n+e\n"), "patch: {}", d.patch);
    }

    #[test]
    fn diff_de_nao_rastreado_apagado_e_sem_diferenca() {
        let tmp = Tmp::new("diff-outros");
        let dir = repo(&tmp, "r");
        write(&dir, "apagar.txt", "x\ny\n");
        write(&dir, "igual.txt", "z\n");
        commit_all(&dir, "base");

        write(&dir, "novo ação.txt", "um\ndois\n");
        let d = diff(&dir, "novo ação.txt", false).unwrap();
        assert_eq!(
            d.patch,
            "diff --git a/novo ação.txt b/novo ação.txt\nnew file mode 100644\n--- /dev/null\n+++ b/novo ação.txt\n@@ -0,0 +1,2 @@\n+um\n+dois\n"
        );

        fs::remove_file(dir.join("apagar.txt")).unwrap();
        let d = diff(&dir, "apagar.txt", false).unwrap();
        assert!(d.patch.ends_with("@@ -1,2 +0,0 @@\n-x\n-y\n"), "patch: {}", d.patch);

        assert_eq!(diff(&dir, "igual.txt", false).unwrap(), Diff { binary: false, too_large: false, patch: String::new() });
        assert_eq!(diff(&dir, "sumiu.txt", false).unwrap().patch, "");
    }

    #[test]
    fn diff_staged_compara_o_indice_com_o_head_e_o_outro_com_o_indice() {
        let tmp = Tmp::new("diff-staged");
        let dir = repo(&tmp, "r");
        write(&dir, "f.txt", "um\n");
        commit_all(&dir, "base");
        write(&dir, "f.txt", "dois\n");
        git(&dir, &["add", "f.txt"]);
        write(&dir, "f.txt", "tres\n");

        let staged = diff(&dir, "f.txt", true).unwrap();
        assert!(staged.patch.ends_with("@@ -1 +1 @@\n-um\n+dois\n"), "patch: {}", staged.patch);
        let unstaged = diff(&dir, "f.txt", false).unwrap();
        assert!(unstaged.patch.ends_with("@@ -1 +1 @@\n-dois\n+tres\n"), "patch: {}", unstaged.patch);
    }

    #[test]
    fn diff_de_binario_e_de_texto_grande_vem_so_com_o_aviso() {
        let tmp = Tmp::new("diff-binario");
        let dir = repo(&tmp, "r");
        write(&dir, "dado.bin", [0u8, 1, 2, 0, 255]);
        write(&dir, "grande.txt", "a\n".repeat(600_000));
        commit_all(&dir, "base");

        write(&dir, "dado.bin", [9u8, 0, 8]);
        write(&dir, "grande.txt", "b\n".repeat(600_000));
        assert_eq!(diff(&dir, "dado.bin", false).unwrap(), Diff { binary: true, too_large: false, patch: String::new() });
        assert_eq!(diff(&dir, "grande.txt", false).unwrap(), Diff { binary: false, too_large: true, patch: String::new() });

        write(&dir, "novo.bin", [0u8, 0, 0]);
        assert!(diff(&dir, "novo.bin", false).unwrap().binary);
    }

    #[test]
    fn stage_e_unstage_por_arquivo_e_com_lista_vazia() {
        let tmp = Tmp::new("stage");
        let dir = repo(&tmp, "r");
        write(&dir, "base.txt", "b");
        commit_all(&dir, "base");

        write(&dir, "a.txt", "a");
        write(&dir, "c.txt", "c");
        fs::remove_file(dir.join("base.txt")).unwrap();
        stage(&dir, &["a.txt".into()]).unwrap();
        let st = git_status_of(&dir);
        assert_eq!(find(&st, "a.txt").staged, Some(Kind::Added));
        assert_eq!(find(&st, "c.txt").unstaged, Some(Kind::Untracked));
        assert_eq!(find(&st, "base.txt").unstaged, Some(Kind::Deleted));

        stage(&dir, &[]).unwrap();
        let st = git_status_of(&dir);
        assert_eq!(find(&st, "c.txt").staged, Some(Kind::Added));
        assert_eq!(find(&st, "base.txt").staged, Some(Kind::Deleted));
        assert!(st.files.iter().all(|f| f.unstaged.is_none()));

        unstage(&dir, &["a.txt".into()]).unwrap();
        let st = git_status_of(&dir);
        assert_eq!((find(&st, "a.txt").staged, find(&st, "a.txt").unstaged), (None, Some(Kind::Untracked)));
        assert_eq!(find(&st, "c.txt").staged, Some(Kind::Added));

        unstage(&dir, &[]).unwrap();
        let st = git_status_of(&dir);
        assert!(st.files.iter().all(|f| f.staged.is_none()));
        assert!(dir.join("c.txt").is_file() && dir.join("a.txt").is_file(), "tirar do índice não apaga nada da pasta");
    }

    #[test]
    fn caminho_com_asterisco_e_com_hifen_nao_vira_padrao_nem_opcao() {
        let tmp = Tmp::new("pathspec");
        let dir = repo(&tmp, "r");
        write(&dir, "base.txt", "b");
        commit_all(&dir, "base");
        // `[ab].txt` é um padrão de glob que casaria com `a.txt`; como pathspec literal, só o próprio arquivo entra.
        write(&dir, "[ab].txt", "1");
        write(&dir, "a.txt", "2");
        write(&dir, "-x.txt", "3");
        stage(&dir, &["[ab].txt".into(), "-x.txt".into()]).unwrap();
        let st = git_status_of(&dir);
        assert_eq!(find(&st, "[ab].txt").staged, Some(Kind::Added));
        assert_eq!(find(&st, "-x.txt").staged, Some(Kind::Added));
        assert_eq!(find(&st, "a.txt").unstaged, Some(Kind::Untracked));
    }

    #[test]
    fn caminho_que_sai_da_pasta_e_recusado() {
        let tmp = Tmp::new("fora");
        let dir = repo(&tmp, "r");
        for rel in ["..", "../x", "a/../../x", "/etc", "C:/x", ".", "./a", ""] {
            assert!(stage(&dir, &[rel.into()]).is_err(), "{rel} deveria ser recusado");
            assert!(diff(&dir, rel, false).is_err(), "{rel} deveria ser recusado no diff");
        }
        assert_eq!(stage(&dir, &["../x".into()]).unwrap_err(), "Caminho fora do projeto: ../x");
    }

    #[test]
    fn stage_e_unstage_funcionam_sem_nenhum_commit() {
        let tmp = Tmp::new("unborn");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        write(&dir, "b.txt", "b");
        stage(&dir, &["a.txt".into()]).unwrap();
        assert_eq!(find(&git_status_of(&dir), "a.txt").staged, Some(Kind::Added));

        unstage(&dir, &["a.txt".into()]).unwrap();
        let st = git_status_of(&dir);
        assert_eq!(find(&st, "a.txt").staged, None);
        assert_eq!(find(&st, "b.txt").unstaged, Some(Kind::Untracked));
        assert!(dir.join("a.txt").is_file());

        stage(&dir, &[]).unwrap();
        unstage(&dir, &[]).unwrap();
        assert!(git_status_of(&dir).files.iter().all(|f| f.staged.is_none()));
        assert!(dir.join("a.txt").is_file() && dir.join("b.txt").is_file());
    }

    #[test]
    fn commit_com_sucesso_mensagem_vazia_e_nada_preparado() {
        let tmp = Tmp::new("commit");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        assert_eq!(commit(&dir, "   \n ").unwrap_err(), "Escreva a mensagem do commit.");
        assert_eq!(commit(&dir, "sem nada").unwrap_err(), "Não há nada preparado para o commit.");

        stage(&dir, &[]).unwrap();
        commit(&dir, "  Primeiro commit  ").unwrap();
        assert_eq!(git(&dir, &["log", "-1", "--format=%s"]).trim(), "Primeiro commit");
        assert!(git_status_of(&dir).files.is_empty());
    }

    #[test]
    fn commit_roda_o_hook_do_repositorio() {
        let tmp = Tmp::new("hook");
        let dir = repo(&tmp, "r");
        write(&dir, ".git/hooks/pre-commit", "#!/bin/sh\necho bloqueado pelo hook >&2\nexit 1\n");
        write(&dir, "a.txt", "a");
        stage(&dir, &[]).unwrap();
        let why = commit(&dir, "tentativa").unwrap_err();
        assert!(why.contains("bloqueado pelo hook"), "o erro traz o stderr do hook: {why}");
    }

    #[test]
    fn sincronizar_sem_remoto_da_erro_em_portugues() {
        let tmp = Tmp::new("sem-remoto");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        commit_all(&dir, "a");
        for why in [fetch(&dir).unwrap_err(), pull(&dir).unwrap_err(), push(&dir).unwrap_err()] {
            assert_eq!(why, "Nenhum remoto configurado neste repositório. Configure um remoto, como origin, para sincronizar.");
        }
    }

    /// Um remoto bare numa pasta temporária e um repositório que já o tem como `origin`, com a branch publicada.
    fn with_remote(tmp: &Tmp) -> (std::path::PathBuf, std::path::PathBuf) {
        let bare = tmp.at("remoto.git");
        git(&tmp.0, &["init", "-q", "--bare", "-b", "main", "remoto.git"]);
        let dir = repo(tmp, "r");
        write(&dir, "a.txt", "a");
        commit_all(&dir, "a");
        git(&dir, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(&dir, &["push", "-q", "-u", "origin", "main"]);
        (bare, dir)
    }

    /// Um segundo clone do remoto que publica um commit novo, para o repositório ficar atrás.
    fn other_pushes(tmp: &Tmp, bare: &Path, file: &str) {
        let outro = tmp.at("outro");
        git(&tmp.0, &["clone", "-q", bare.to_str().unwrap(), outro.to_str().unwrap()]);
        identity(&outro);
        write(&outro, file, "de fora");
        commit_all(&outro, file);
        git(&outro, &["push", "-q", "origin", "main"]);
    }

    #[test]
    fn fetch_traz_o_que_o_remoto_tem() {
        let tmp = Tmp::new("fetch");
        let (bare, dir) = with_remote(&tmp);
        other_pushes(&tmp, &bare, "de-fora.txt");
        fetch(&dir).unwrap();
        assert_eq!(git_status_of(&dir).behind, 1);
    }

    #[test]
    fn pull_traz_o_commit_do_remoto() {
        let tmp = Tmp::new("pull");
        let (bare, dir) = with_remote(&tmp);
        other_pushes(&tmp, &bare, "de-fora.txt");
        pull(&dir).unwrap();
        assert!(dir.join("de-fora.txt").is_file());
        assert_eq!(git_status_of(&dir).behind, 0);
    }

    #[test]
    fn pull_com_historia_dividida_faz_merge_sem_abrir_editor() {
        let tmp = Tmp::new("pull-merge");
        let (bare, dir) = with_remote(&tmp);
        git(&dir, &["config", "pull.rebase", "false"]);
        write(&dir, "local.txt", "local");
        commit_all(&dir, "local");
        other_pushes(&tmp, &bare, "de-fora.txt");
        // Se o merge abrisse editor, este teste ficaria preso; o `GIT_MERGE_AUTOEDIT=no` evita isso.
        pull(&dir).unwrap();
        assert!(dir.join("de-fora.txt").is_file() && dir.join("local.txt").is_file());
        assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]).trim(), "4");
    }

    #[test]
    fn push_com_upstream_envia_o_commit() {
        let tmp = Tmp::new("push");
        let (bare, dir) = with_remote(&tmp);
        write(&dir, "b.txt", "b");
        commit_all(&dir, "b");
        push(&dir).unwrap();
        assert_eq!(git(&bare, &["rev-parse", "main"]), git(&dir, &["rev-parse", "HEAD"]));
    }

    #[test]
    fn push_de_branch_sem_upstream_publica_em_origin_e_configura_o_upstream() {
        let tmp = Tmp::new("push-novo");
        let (bare, dir) = with_remote(&tmp);
        git(&dir, &["switch", "-q", "-c", "feature"]);
        write(&dir, "f.txt", "f");
        commit_all(&dir, "f");
        assert_eq!(upstream(&dir), None);
        push(&dir).unwrap();
        assert!(git(&bare, &["branch", "--list", "feature"]).contains("feature"));
        assert_eq!(upstream(&dir).as_deref(), Some("origin/feature"));
    }

    #[test]
    fn push_sem_upstream_e_sem_origin_da_erro() {
        let tmp = Tmp::new("push-sem-origin");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        commit_all(&dir, "a");
        let outro = tmp.at("outro.git");
        git(&tmp.0, &["init", "-q", "--bare", "outro.git"]);
        git(&dir, &["remote", "add", "outro", outro.to_str().unwrap()]);
        assert!(push(&dir).unwrap_err().contains("origin"));
    }

    #[test]
    fn branches_lista_troca_e_cria() {
        let tmp = Tmp::new("branches");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        commit_all(&dir, "a");
        git(&dir, &["branch", "b-feature"]);
        git(&dir, &["branch", "a-ajuste"]);

        let b = branches(&dir).unwrap();
        assert_eq!((b.current.as_str(), b.local.clone()), ("main", vec!["a-ajuste".to_string(), "b-feature".into(), "main".into()]));

        switch(&dir, "a-ajuste").unwrap();
        assert_eq!(branches(&dir).unwrap().current, "a-ajuste");

        create_branch(&dir, "nova").unwrap();
        let b = branches(&dir).unwrap();
        assert_eq!(b.current, "nova");
        assert!(b.local.contains(&"nova".to_string()));
    }

    #[test]
    fn branch_com_nome_invalido_ou_repetido_da_erro_em_portugues() {
        let tmp = Tmp::new("branch-erro");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        commit_all(&dir, "a");
        for name in ["", "x..y", "com espaco", "-x"] {
            assert_eq!(create_branch(&dir, name).unwrap_err(), format!("Nome de branch inválido: {name}"));
            assert_eq!(switch(&dir, name).unwrap_err(), format!("Nome de branch inválido: {name}"));
        }
        assert_eq!(create_branch(&dir, "main").unwrap_err(), "Já existe uma branch chamada main.");
        assert_eq!(branches(&dir).unwrap().local, vec!["main".to_string()]);
    }

    #[test]
    fn erro_do_git_chega_com_o_texto_do_stderr() {
        let tmp = Tmp::new("stderr");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        commit_all(&dir, "a");
        let why = switch(&dir, "nao-existe").unwrap_err();
        assert!(why.contains("nao-existe"), "o erro traz o que o Git escreveu: {why}");
        assert_eq!(branches(&dir).unwrap().current, "main");
    }

    #[test]
    fn repositorio_sem_commit_lista_branch_e_troca_pelo_nome() {
        let tmp = Tmp::new("branches-unborn");
        let dir = repo(&tmp, "r");
        let b = branches(&dir).unwrap();
        assert_eq!((b.current.as_str(), b.local.is_empty()), ("main", true));
        create_branch(&dir, "trabalho").unwrap();
        assert_eq!(branches(&dir).unwrap().current, "trabalho");
    }
}
