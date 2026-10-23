//! O painel Git do projeto: o estado da árvore, o diff de um arquivo, preparar e tirar do índice, o commit,
//! a sincronização com o remoto e a troca de branch. Tudo passa pelo `git` com `proc::git_saida` ou
//! `proc::git_completa`, então nada espera digitação e o erro traz o que o Git escreveu.
use crate::{files, proc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Diff com mais texto que isto não é montado: a tela mostra só o aviso.
const MAX_DIFF: usize = 1_048_576;

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

/// O estado do repositório: a branch (ou o hash em HEAD destacado), o hash completo do HEAD (vazio sem commit), o quanto está à frente e atrás do remoto e o que está alterado.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub branch: String,
    pub head: String,
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

/// Como o projeto se liga ao Git. Sem repositório ou com o Git ausente, não há lista de alterações.
#[derive(Clone, Debug, PartialEq)]
pub enum Modo {
    /// O projeto é a raiz de um repositório.
    Repo,
    /// A pasta não está em nenhum repositório.
    NoRepo,
    /// O programa `git` não pôde ser executado.
    NoGit,
    /// A pasta está dentro de um repositório cuja raiz (com barras invertidas) não é o projeto.
    RepoAbove(String),
}

/// O estado do Git que a tela mostra. `repo` traz o `Status`; os outros modos só dizem por que não há lista.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Estado {
    Repo { status: Status },
    NoRepo,
    NoGit,
    RepoAbove { root: String },
}

/// Em qual modo o projeto está. `Err` só quando há `.git` acima da pasta e o Git recusa (dono diferente, repositório
/// corrompido): o texto é o do Git, e não se oferece `git init` por cima de um repositório que existe.
pub fn modo(dir: &Path) -> Result<Modo, String> {
    let top = match proc::git_falha(dir, &["rev-parse", "--show-toplevel"]) {
        Ok(top) => top.trim().to_string(),
        Err(proc::Falha::Ausente(_)) => return Ok(Modo::NoGit),
        Err(proc::Falha::Erro(why)) => {
            if dir.ancestors().any(|p| p.join(".git").exists()) {
                return Err(why);
            }
            return Ok(Modo::NoRepo);
        }
    };
    if top.is_empty() {
        return Err("O Git não informou a raiz do repositório.".into());
    }
    if proc::mesma_pasta(dir, Path::new(&top)) {
        Ok(Modo::Repo)
    } else {
        Ok(Modo::RepoAbove(top.replace('/', "\\")))
    }
}

/// O estado do Git para a tela: a lista só existe no modo `Repo`.
pub fn estado(dir: &Path) -> Result<Estado, String> {
    Ok(match modo(dir)? {
        Modo::Repo => Estado::Repo { status: status(dir)? },
        Modo::NoRepo => Estado::NoRepo,
        Modo::NoGit => Estado::NoGit,
        Modo::RepoAbove(root) => Estado::RepoAbove { root },
    })
}

/// Cria o repositório na pasta do projeto, só quando ela não está em nenhum. Sem `-b` (vale a branch padrão do
/// usuário), sem commit e sem stage. Nos outros modos, recusa sem rodar o Git.
pub fn init(dir: &Path) -> Result<(), String> {
    match modo(dir)? {
        Modo::NoRepo => proc::git_saida(dir, &["init", "-q"]).map(|_| ()),
        Modo::Repo => Err("A pasta já é um repositório Git.".into()),
        Modo::NoGit => Err("O Git não foi encontrado neste computador.".into()),
        Modo::RepoAbove(root) => Err(format!("A pasta está dentro do repositório {root}; não dá para criar outro aqui.")),
    }
}

/// Lê a saída de `status --porcelain=v2 --branch -z`. Cada registro acaba em NUL; o renomeado traz o caminho
/// de origem no registro seguinte.
fn parse_status(branch: String, out: &str) -> Status {
    let mut st = Status { branch, head: String::new(), ahead: 0, behind: 0, files: Vec::new() };
    let mut records = out.split('\0');
    while let Some(rec) = records.next() {
        if let Some(oid) = rec.strip_prefix("# branch.oid ") {
            // `(initial)` é o repositório sem commit: o HEAD não tem hash.
            st.head = if oid == "(initial)" { String::new() } else { oid.to_string() };
        } else if let Some(ab) = rec.strip_prefix("# branch.ab ") {
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
/// rastreado vira o arquivo inteiro como linhas acrescentadas. Só os testes usam esta forma; a tela usa `review_diff`.
#[cfg(test)]
pub fn diff(dir: &Path, rel: &str, staged: bool) -> Result<Diff, String> {
    Ok(snapshot(dir, spec(rel)?, staged)?.diff)
}

/// O diff de `rel` lido uma vez, com o texto que a revisão cobre. `untracked` diz que o arquivo só existe na pasta:
/// então o `raw` é o patch sintético que a tela mostra, e não há bloco a tratar.
struct Lido {
    diff: Diff,
    raw: String,
    untracked: bool,
}

/// Lê o diff de `rel` uma vez só, para a tela e para a revisão saírem do mesmo snapshot. Pasta é recusada: o patch
/// dela junta arquivos, e um bloco desse patch cairia no cabeçalho do primeiro arquivo.
fn snapshot(dir: &Path, rel: &str, staged: bool) -> Result<Lido, String> {
    if is_folder(dir, rel)? {
        return Err(format!("{rel} é uma pasta: o diff é de um arquivo por vez."));
    }
    if !staged && !tracked_file(dir, rel)? {
        let diff = untracked(&guarded(dir, rel)?, rel)?;
        return Ok(Lido { raw: diff.patch.clone(), diff, untracked: true });
    }
    let raw = raw_diff(dir, rel, staged, CONTEXTO_BLOCO)?;
    Ok(Lido { diff: read_diff(&raw), raw, untracked: false })
}

/// Contexto do diff que vira bloco: o padrão do Git. A revisão e as ações por bloco dependem dele.
const CONTEXTO_BLOCO: &str = "-U3";
/// Contexto do arquivo inteiro: só o comparador o mostra (`ReviewDiff::display`) e o histórico, que é imutável.
const CONTEXTO_TODO: &str = "-U1000000";

/// O diff bruto de `rel`, como o Git escreve (sem `read_diff`). Sua impressão digital é a revisão que a UI devolve.
fn raw_diff(dir: &Path, rel: &str, staged: bool, contexto: &str) -> Result<String, String> {
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames", contexto];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", rel]);
    proc::git_saida(dir, &args)
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
    if files::is_binary(&bytes) {
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
    let mut args = vec!["add", "-A", "--"];
    args.extend(specs);
    proc::git_saida(dir, &args).map(|_| ())
}

/// Tira os caminhos do índice; a pasta de trabalho fica como está. Lista vazia tira tudo.
pub fn unstage(dir: &Path, paths: &[String]) -> Result<(), String> {
    let specs = specs(paths)?;
    if has_head(dir) {
        let mut args = vec!["reset", "-q", "--"];
        args.extend(specs);
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

// ---------------------------------------------------------------------------------------------------------------
// Revisão: blocos (hunks), descarte por arquivo, histórico, commits e conflitos de merge.
// ---------------------------------------------------------------------------------------------------------------

/// Quantos commits uma página do histórico pode trazer, no máximo.
const MAX_HISTORY: usize = 200;

/// Um bloco (hunk) do diff de um arquivo, como a UI o recebeu. `index` é a posição entre os blocos do diff,
/// `header` é a linha `@@` e `revision` é a impressão digital do diff bruto que a UI viu. O patch nunca vem da UI.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HunkRef {
    pub path: String,
    pub index: usize,
    pub header: String,
    pub revision: String,
}

/// O diff de um arquivo com a revisão. A UI devolve `revision` e o `index`/`header` do bloco. `untracked` diz que o
/// arquivo não é rastreado: ele não tem bloco, só o patch de arquivo novo. `display` é o arquivo inteiro, com todas as
/// linhas antes, entre e depois das mudanças: só o comparador o mostra. Os blocos (`diff`) e a revisão seguem o `-U3`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDiff {
    pub diff: Diff,
    pub display: Diff,
    pub revision: String,
    pub untracked: bool,
}

/// Um commit do histórico. `date` vem em ISO 8601 estrito; a tela formata.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

/// Um arquivo mudado num commit. `orig_path` só existe quando o Git viu renomeação.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitFile {
    pub path: String,
    pub orig_path: Option<String>,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetail {
    pub commit: Commit,
    pub files: Vec<CommitFile>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitHoverDetail {
    pub author: String,
    pub date: String,
    pub message: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub github_url: Option<String>,
}

/// Um arquivo em conflito. `ours` e `theirs` são os estágios 2 e 3 do índice; `None` quando o lado não existe (ex.: o
/// outro lado apagou o arquivo). `worktree` é o texto atual da pasta, sem BOM e com `\n`. `binary`
/// vale também para o texto que não é UTF-8 válido: nesse caso `worktree` vem `None` e a tela não o edita. `revision` é a
/// impressão digital dos estágios do índice e dos bytes da pasta: quem grava ou remove manda de volta esta revisão.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub worktree: Option<String>,
    pub binary: bool,
    pub revision: String,
}

/// Divide um patch em (cabeçalho do arquivo, blocos). Cada bloco começa numa linha `@@`; as linhas do corpo começam
/// com ` `, `+`, `-` ou `\`, então nenhuma delas se confunde com um cabeçalho de bloco. Os bytes ficam como estão.
fn split_hunks(patch: &str) -> (&str, Vec<&str>) {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in patch.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            starts.push(offset);
        }
        offset += line.len();
    }
    let Some(&first) = starts.first() else {
        return (patch, Vec::new());
    };
    let blocks = starts
        .iter()
        .enumerate()
        .map(|(i, &start)| &patch[start..starts.get(i + 1).copied().unwrap_or(patch.len())])
        .collect();
    (&patch[..first], blocks)
}

/// A linha `@@` de um bloco, sem a quebra de linha.
fn block_header(block: &str) -> &str {
    block.split('\n').next().unwrap_or("").trim_end_matches('\r')
}

/// O diff de `rel` com a revisão e o arquivo inteiro. A UI guarda a revisão e manda de volta com o `index` do bloco.
/// O arquivo inteiro vem de outra leitura do Git: se o diff de blocos mudou entre as duas, não bate com a revisão. Então
/// lê de novo, uma vez; se ainda muda, recusa, e a tela relê.
pub fn review_diff(dir: &Path, rel: &str, staged: bool) -> Result<ReviewDiff, String> {
    let rel = spec(rel)?;
    for _ in 0..2 {
        let lido = snapshot(dir, rel, staged)?;
        let revision = files::fingerprint(lido.raw.as_bytes());
        if lido.untracked || lido.diff.binary || lido.diff.too_large {
            return Ok(ReviewDiff { revision, display: lido.diff.clone(), diff: lido.diff, untracked: lido.untracked });
        }
        let display = read_diff(&raw_diff(dir, rel, staged, CONTEXTO_TODO)?);
        if files::fingerprint(raw_diff(dir, rel, staged, CONTEXTO_BLOCO)?.as_bytes()) == revision {
            return Ok(ReviewDiff { revision, display, diff: lido.diff, untracked: false });
        }
    }
    Err(stale(rel))
}

/// Os caminhos do índice, com `-z`: nome com quebra de linha não vira duas linhas. Pathspec literal, como em `proc`.
fn index_paths(dir: &Path, rel: &str) -> Result<Vec<Vec<u8>>, String> {
    let out = proc::git_bytes(dir, &["ls-files", "-z", "--", rel])?;
    Ok(out.split(|b| *b == 0).filter(|p| !p.is_empty()).map(<[u8]>::to_vec).collect())
}

/// Caminho que é um arquivo do índice, pelo nome exato (não pasta, nem prefixo de outro nome).
fn tracked_file(dir: &Path, rel: &str) -> Result<bool, String> {
    Ok(index_paths(dir, rel)?.iter().any(|p| p == rel.as_bytes()))
}

/// Pasta com arquivos rastreados ou com o nome na pasta de trabalho. Um patch de pasta junta arquivos, então é recusado.
fn is_folder(dir: &Path, rel: &str) -> Result<bool, String> {
    if dir.join(rel).is_dir() {
        return Ok(true);
    }
    let under = format!("{rel}/");
    Ok(index_paths(dir, rel)?.iter().any(|p| p.starts_with(under.as_bytes())))
}

fn stale(rel: &str) -> String {
    format!("{rel} mudou desde que o diff foi lido. Atualize o diff antes de agir.")
}

/// O patch do bloco pedido, lido agora do Git. Recusa se o diff mudou desde a revisão, se o bloco não existe
/// mais ou se a linha `@@` não bate. Binário e texto grande não têm bloco a tratar.
fn current_block(dir: &Path, hunk: &HunkRef, staged: bool) -> Result<String, String> {
    let rel = spec(&hunk.path)?;
    let lido = snapshot(dir, rel, staged)?;
    if lido.untracked {
        return Err(format!("{rel} não está rastreado: prepare ou descarte o arquivo inteiro, não por bloco."));
    }
    if files::fingerprint(lido.raw.as_bytes()) != hunk.revision {
        return Err(stale(rel));
    }
    if lido.diff.binary {
        return Err(format!("{rel} é binário: trate o arquivo inteiro, não por bloco."));
    }
    if lido.diff.too_large {
        return Err(format!("O diff de {rel} é grande demais para trabalhar por bloco."));
    }
    let (head, blocks) = split_hunks(&lido.raw);
    match blocks.get(hunk.index) {
        Some(block) if block_header(block) == hunk.header => Ok(format!("{head}{block}")),
        _ => Err(format!("O bloco {} de {rel} não existe mais. Atualize o diff.", hunk.index + 1)),
    }
}

/// Prepara um bloco da pasta de trabalho. Os outros blocos do arquivo ficam na pasta, como estavam. Sem `--index`:
/// só `--cached` toca o índice. O `git apply` é atômico: se o trecho não aplica, ele não muda nada, então não há
/// `--check` antes.
pub fn stage_hunk(dir: &Path, hunk: &HunkRef) -> Result<(), String> {
    let patch = current_block(dir, hunk, false)?;
    proc::git_entrada(dir, &["apply", "--cached"], patch.as_bytes()).map_err(|why| format!("O bloco não pôde ser preparado: {why}"))
}

/// Tira um bloco do índice; a pasta de trabalho não muda. Os outros blocos preparados ficam preparados.
pub fn unstage_hunk(dir: &Path, hunk: &HunkRef) -> Result<(), String> {
    let patch = current_block(dir, hunk, true)?;
    proc::git_entrada(dir, &["apply", "--cached", "-R"], patch.as_bytes())
        .map_err(|why| format!("O bloco não pôde ser tirado do preparo: {why}"))
}

/// Descarta um bloco da alteração não preparada: a pasta volta a como estava no índice nesse trecho. O índice não
/// é tocado, então o que estava preparado continua preparado. Não existe descarte de bloco do lado preparado.
pub fn discard_hunk(dir: &Path, hunk: &HunkRef) -> Result<(), String> {
    let patch = current_block(dir, hunk, false)?;
    proc::git_entrada(dir, &["apply", "-R"], patch.as_bytes()).map_err(|why| format!("O bloco não pôde ser descartado: {why}"))
}

/// Descarta a alteração não preparada de um arquivo rastreado. `revision_seen` é a revisão do diff não preparado
/// que a UI mostrou; se o arquivo mudou desde então, nada é feito. O índice não muda. Não rastreado é recusado.
pub fn discard_file(dir: &Path, rel: &str, revision_seen: &str) -> Result<(), String> {
    let rel = spec(rel)?;
    if !tracked_file(dir, rel)? {
        return Err(format!("{rel} não está rastreado: o Git não o descarta. Mande-o para a Lixeira pela tela."));
    }
    if files::fingerprint(raw_diff(dir, rel, false, CONTEXTO_BLOCO)?.as_bytes()) != revision_seen {
        return Err(stale(rel));
    }
    proc::git_saida(dir, &["restore", "--worktree", "--", rel]).map(|_| ())
}

/// `--max-count` e `--skip` de uma página do `log`: `limit` fica entre 1 e `MAX_HISTORY`. Sem HEAD (repositório sem
/// commit) não há página, então `None`.
fn pagina(dir: &Path, limit: usize, skip: usize) -> Option<[String; 2]> {
    has_head(dir).then(|| [format!("--max-count={}", limit.clamp(1, MAX_HISTORY)), format!("--skip={skip}")])
}

/// Commits do HEAD, do mais novo para o mais antigo. `limit` fica entre 1 e 200; `skip` pula os já mostrados.
/// Com `path`, só os commits que tocaram o caminho, seguindo renomeações. Repositório sem commit: lista vazia.
pub fn history(dir: &Path, limit: usize, skip: usize, path: Option<&str>) -> Result<Vec<Commit>, String> {
    let Some([max, skip]) = pagina(dir, limit, skip) else { return Ok(Vec::new()) };
    let mut args = vec!["log", GRAPH_FORMAT, "--no-color", &max, &skip];
    if let Some(path) = path {
        args.extend(["--follow", "--", spec(path)?]);
    }
    Ok(parse_graph(&proc::git_saida(dir, &args)?).into_iter().map(|g| g.commit).collect())
}

/// Os registros de uma saída do Git: separados por 0x1e, sem as quebras de linha do começo e sem os vazios.
fn records(out: &str) -> impl Iterator<Item = &str> {
    out.split('\x1e').map(|rec| rec.trim_start_matches('\n')).filter(|rec| !rec.is_empty())
}

/// Um commit do gráfico: o `Commit` mais os pais e as refs que apontam para ele. No JSON, `commit` é achatado: os campos
/// do `Commit` ficam no mesmo nível que `parents` e `refs`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphCommit {
    #[serde(flatten)]
    pub commit: Commit,
    /// Hashes completos dos pais; vazio no commit raiz.
    pub parents: Vec<String>,
    pub refs: Vec<GitRef>,
}

/// O tipo de uma ref do gráfico. A ordem das variantes é a ordem da lista: branches, remotas, tags, HEAD destacado.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RefKind {
    Branch,
    Remote,
    Tag,
    Head,
}

/// Uma ref que aponta para um commit. `name` é `X` para a branch, `R/X` para a remota, `X` para a tag e `HEAD` no
/// HEAD destacado. `current` vale só para a branch onde o HEAD está (ou para o HEAD destacado).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRef {
    pub name: String,
    pub kind: RefKind,
    pub current: bool,
}

/// Campos separados por 0x1f e registros por 0x1e. O assunto vem por último, então um 0x1f nele não embaralha os outros campos.
const GRAPH_FORMAT: &str = "--format=%H%x1f%h%x1f%an%x1f%aI%x1f%P%x1f%D%x1f%s%x1e";

/// Commits de HEAD e, se a branch atual tem upstream, também do upstream, em ordem topológica (mais novo primeiro).
/// `limit` fica entre 1 e 200; `skip` pula os já mostrados. Repositório sem commit: lista vazia.
pub fn graph(dir: &Path, limit: usize, skip: usize) -> Result<Vec<GraphCommit>, String> {
    let Some([max, skip]) = pagina(dir, limit, skip) else { return Ok(Vec::new()) };
    let mut args = vec!["log", GRAPH_FORMAT, "--no-color", "--decorate=full", "--topo-order", &max, &skip, "HEAD"];
    if upstream(dir).is_some() {
        args.push("@{upstream}");
    }
    // O `--` fecha a lista de revisões: nada que venha depois vira caminho nem opção.
    args.push("--");
    Ok(parse_graph(&proc::git_saida(dir, &args)?))
}

/// Lê a saída de `log`/`show` com `GRAPH_FORMAT`. Registro que não tem todos os campos é descartado.
fn parse_graph(out: &str) -> Vec<GraphCommit> {
    records(out)
        .filter_map(|rec| {
            let f: Vec<&str> = rec.splitn(7, '\x1f').collect();
            match f.as_slice() {
                [hash, short, author, date, parents, refs, subject] => Some(GraphCommit {
                    commit: Commit { hash: hash.to_string(), short: short.to_string(), author: author.to_string(), date: date.to_string(), subject: subject.to_string() },
                    parents: parents.split_whitespace().map(str::to_string).collect(),
                    refs: parse_refs(refs),
                }),
                _ => None,
            }
        })
        .collect()
}

/// Lê o `%D` com `--decorate=full`, como `HEAD -> refs/heads/X, refs/remotes/R/X, tag: refs/tags/T`. Só entra o que
/// o gráfico mostra: `refs/remotes/*/HEAD`, stash, notes e outras refs ficam de fora. Os atuais vêm primeiro.
fn parse_refs(decorations: &str) -> Vec<GitRef> {
    let mut refs = Vec::new();
    for item in decorations.split(", ").filter(|s| !s.is_empty()) {
        if item == "HEAD" {
            refs.push(GitRef { name: "HEAD".into(), kind: RefKind::Head, current: true });
            continue;
        }
        let (current, reference) = match item.strip_prefix("HEAD -> ") {
            Some(reference) => (true, reference),
            None => (false, item),
        };
        let found = if let Some(name) = reference.strip_prefix("refs/heads/") {
            Some((name, RefKind::Branch))
        } else if let Some(name) = reference.strip_prefix("refs/remotes/") {
            (!name.ends_with("/HEAD")).then_some((name, RefKind::Remote))
        } else if let Some(name) = reference.strip_prefix("tag: refs/tags/") {
            Some((name, RefKind::Tag))
        } else {
            None
        };
        if let Some((name, kind)) = found {
            refs.push(GitRef { name: name.to_string(), kind, current });
        }
    }
    refs.sort_by_key(|r| (!r.current, r.kind));
    refs
}

/// Resolve um commit por hash (curto ou completo). Só dígitos hexadecimais entram: nada que comece por hífen
/// vira opção do Git, e nomes de branch ou expressões não são aceitos aqui.
fn resolve_commit(dir: &Path, id: &str) -> Result<String, String> {
    let hex = (4..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit());
    if !hex {
        return Err(format!("Commit inválido: {id}"));
    }
    let rev = format!("{id}^{{commit}}");
    proc::git_saida(dir, &["rev-parse", "--verify", "-q", &rev])
        .map(|h| h.trim().to_string())
        .map_err(|_| format!("Commit não encontrado: {id}"))
}

/// Cabeçalho do commit e os arquivos que ele mudou. Commit raiz também lista tudo como adicionado.
pub fn commit_detail(dir: &Path, id: &str) -> Result<CommitDetail, String> {
    let hash = resolve_commit(dir, id)?;
    let commit = parse_graph(&proc::git_saida(dir, &["show", "-s", GRAPH_FORMAT, &hash])?)
        .into_iter()
        .next()
        .ok_or_else(|| format!("Commit não encontrado: {id}"))?
        .commit;
    Ok(CommitDetail { commit, files: commit_files(dir, &hash)? })
}

/// Detalhes carregados só quando o usuário abre o hover do gráfico.
pub fn commit_hover(dir: &Path, id: &str) -> Result<CommitHoverDetail, String> {
    let hash = resolve_commit(dir, id)?;
    let commit = parse_graph(&proc::git_saida(dir, &["show", "-s", GRAPH_FORMAT, &hash])?)
        .into_iter()
        .next()
        .ok_or_else(|| format!("Commit não encontrado: {id}"))?
        .commit;
    let message = proc::git_saida(dir, &["show", "-s", "--format=%B", &hash])?;
    let stats = proc::git_saida(
        dir,
        &["show", "--shortstat", "--format=", "--diff-merges=first-parent", &hash],
    )?;
    let (files_changed, insertions, deletions) = parse_shortstat(&stats);
    let github_url = proc::git_saida(dir, &["remote", "get-url", "origin"])
        .ok()
        .and_then(|remote| github_commit_url_from_remote(remote.trim(), &hash));
    Ok(CommitHoverDetail {
        author: commit.author,
        date: commit.date,
        message,
        files_changed,
        insertions,
        deletions,
        github_url,
    })
}

fn parse_shortstat(out: &str) -> (usize, usize, usize) {
    let mut files = 0;
    let mut insertions = 0;
    let mut deletions = 0;
    if let Some(line) = out.lines().find(|line| line.contains(" changed")) {
        for part in line.split(',') {
            let mut words = part.split_whitespace();
            let Some(count) = words.next().and_then(|n| n.parse::<usize>().ok()) else {
                continue;
            };
            match words.next().unwrap_or("") {
                "file" | "files" => files = count,
                "insertion(+)" | "insertions(+)" => insertions = count,
                "deletion(-)" | "deletions(-)" => deletions = count,
                _ => {}
            }
        }
    }
    (files, insertions, deletions)
}

fn github_commit_url_from_remote(remote: &str, hash: &str) -> Option<String> {
    if hash.is_empty() || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let path = if let Some(rest) = remote.strip_prefix("https://") {
        let (authority, path) = rest.split_once('/')?;
        authority.rsplit('@').next()?.eq_ignore_ascii_case("github.com").then_some(path)?
    } else if let Some(rest) = remote.strip_prefix("ssh://") {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?.split(':').next()?;
        host.eq_ignore_ascii_case("github.com").then_some(path)?
    } else {
        let (_, rest) = remote.split_once('@')?;
        let (host, path) = rest.split_once(':')?;
        host.eq_ignore_ascii_case("github.com").then_some(path)?
    };
    let mut parts = path.trim_end_matches('/').split('/');
    let owner = parts.next()?;
    let raw_repo = parts.next()?;
    let repo = raw_repo.strip_suffix(".git").unwrap_or(raw_repo);
    let valid_part = |part: &str| {
        !part.is_empty() && part != "." && part != ".."
            && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    if parts.next().is_some() || !valid_part(owner) || !valid_part(repo) || !valid_part(hash) {
        return None;
    }
    Some(format!("https://github.com/{owner}/{repo}/commit/{hash}"))
}

/// Arquivos de um commit. `-M` mostra renomeação; merge é comparado com o primeiro pai.
fn commit_files(dir: &Path, hash: &str) -> Result<Vec<CommitFile>, String> {
    let out = proc::git_saida(
        dir,
        &["diff-tree", "--root", "-r", "-M", "--no-commit-id", "--name-status", "-z", "--diff-merges=first-parent", hash],
    )?;
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let letter = status.chars().next().unwrap_or(' ');
        let orig_path = if matches!(letter, 'R' | 'C') { fields.next().map(str::to_string) } else { None };
        if let Some(path) = fields.next() {
            files.push(CommitFile { path: path.to_string(), orig_path, kind: kind(letter).unwrap_or(Kind::Modified) });
        }
    }
    Ok(files)
}

/// O diff de um commit, ou só de um caminho dele, com o arquivo inteiro (o commit não muda, então a tela mostra tudo).
/// Em renomeação, o caminho de origem entra junto, para o diff mostrar a mudança inteira. Diff grande vira só o aviso.
pub fn commit_diff(dir: &Path, id: &str, path: Option<&str>) -> Result<Diff, String> {
    let hash = resolve_commit(dir, id)?;
    let mut specs: Vec<String> = Vec::new();
    if let Some(path) = path {
        let rel = spec(path)?;
        specs.push(rel.to_string());
        if let Some(orig) = commit_files(dir, &hash)?.into_iter().find(|f| f.path == rel).and_then(|f| f.orig_path) {
            specs.push(orig);
        }
    }
    let mut args = vec![
        "diff-tree", "--root", "-r", "-p", "-M", "--no-commit-id", "--no-color", "--no-ext-diff", "--no-textconv", CONTEXTO_TODO,
        "--diff-merges=first-parent", &hash, "--",
    ];
    args.extend(specs.iter().map(String::as_str));
    Ok(read_diff(&proc::git_saida(dir, &args)?))
}

/// Estágios de conflito do caminho exato `rel` (1 base, 2 atual, 3 recebido), com os bytes crus dos registros do
/// `ls-files -u`. Os registros entram na revisão: um estágio trocado muda a revisão mesmo com a pasta igual.
fn unmerged(dir: &Path, rel: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let raw = proc::git_bytes(dir, &["ls-files", "-u", "-z", "--", rel])?;
    let mut stages = Vec::new();
    for record in raw.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        // `modo SP hash SP estágio TAB caminho`; o pathspec pode trazer outros caminhos (pasta), que não entram.
        let Some(tab) = record.iter().position(|&b| b == b'\t') else { continue };
        if &record[tab + 1..] != rel.as_bytes() {
            continue;
        }
        let stage = record[..tab].split(|&b| b == b' ').nth(2).and_then(|s| std::str::from_utf8(s).ok()?.parse::<u8>().ok());
        stages.extend(stage);
    }
    Ok((raw, stages))
}

/// Tamanho máximo de um lado ou da pasta para a tela. Acima disso o conflito se resolve pelo terminal.
const MAX_CONFLITO: u64 = 1_048_576;

/// Nome do lado pelo estágio (1 base, 2 atual, 3 recebido), para as mensagens.
fn nome_do_lado(n: u8) -> &'static str {
    match n {
        1 => "base",
        2 => "atual",
        _ => "recebido",
    }
}

fn grande(rel: &str) -> String {
    format!("{rel} é grande demais para resolver aqui. Resolva pelo terminal.")
}

/// Um conflito lido de uma vez: os estágios e registros do índice e os bytes da pasta (`None` quando o arquivo não existe).
/// A revisão cobre os dois, então um estágio trocado recusa a gravação mesmo com a pasta igual.
struct Snap {
    stages: Vec<u8>,
    index: Vec<u8>,
    worktree: Option<Vec<u8>>,
}

impl Snap {
    /// Lê o conflito de `rel`: o caminho precisa ser seguro, o arquivo precisa estar em conflito e a pasta precisa caber na tela.
    fn read(dir: &Path, rel: &str) -> Result<Snap, String> {
        let file = guarded(dir, rel)?;
        let (index, stages) = unmerged(dir, rel)?;
        if stages.is_empty() {
            return Err(format!("{rel} não está em conflito."));
        }
        Ok(Snap { stages, index, worktree: read_capped(&file, rel)? })
    }

    fn revision(&self) -> String {
        let mut all = self.index.clone();
        match &self.worktree {
            Some(bytes) => {
                all.push(1);
                all.extend_from_slice(bytes);
            }
            None => all.push(0),
        }
        files::fingerprint(&all)
    }

    /// Os três lados como texto. Lado que não é UTF-8 ou grande demais recusa o conflito inteiro: não há como mostrá-lo.
    fn sides(&self, dir: &Path, rel: &str) -> Result<[Option<String>; 3], String> {
        Ok([self.side(dir, rel, 1)?, self.side(dir, rel, 2)?, self.side(dir, rel, 3)?])
    }

    fn side(&self, dir: &Path, rel: &str, n: u8) -> Result<Option<String>, String> {
        if !self.stages.contains(&n) {
            return Ok(None);
        }
        let spec = format!(":{n}:{rel}");
        let size: u64 = proc::git_saida(dir, &["cat-file", "-s", &spec])?.trim().parse().unwrap_or(u64::MAX);
        if size > MAX_CONFLITO {
            return Err(grande(rel));
        }
        let bytes = proc::git_bytes(dir, &["cat-file", "blob", &spec])?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| format!("O lado {} de {rel} não é UTF-8 válido. Resolva pelo terminal.", nome_do_lado(n)))
    }
}

/// Os bytes da pasta, até `MAX_CONFLITO`. Arquivo que não existe vira `None`. Pasta, ou arquivo grande demais, é recusado.
fn read_capped(file: &Path, rel: &str) -> Result<Option<Vec<u8>>, String> {
    if file.is_dir() {
        return Err(format!("{rel} é uma pasta, não um arquivo."));
    }
    let handle = match File::open(file) {
        Ok(handle) => handle,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Não foi possível ler {rel}: {e}")),
    };
    // Lê no máximo um byte além do limite: o tamanho é conferido sem ler o arquivo inteiro.
    let mut bytes = Vec::new();
    handle.take(MAX_CONFLITO + 1).read_to_end(&mut bytes).map_err(|e| format!("Não foi possível ler {rel}: {e}"))?;
    if bytes.len() as u64 > MAX_CONFLITO {
        return Err(grande(rel));
    }
    Ok(Some(bytes))
}

/// Caminho de `rel` dentro do projeto, para ler sem seguir nada: `files::no_git` e `files::no_links` recusam `.git`,
/// barra invertida e link ou junction em qualquer nível. A pasta do projeto fica de fora: ela pode ser um link legítimo.
/// O último nível pode não existir: o conflito de um arquivo apagado não o tem.
fn guarded(dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = spec(rel)?;
    files::no_git(rel)?;
    files::no_links(dir, rel)?;
    Ok(dir.join(rel))
}

/// BOM e CRLF do arquivo em conflito. O marcador `<<<<<<<` vem antes do conteúdo, então o BOM pode estar no começo
/// do primeiro lado e não no da pasta: olha a pasta e os lados.
fn estilo(pasta: Option<&[u8]>, lados: &[&str]) -> (bool, bool) {
    let bom = pasta.is_some_and(|b| b.starts_with(&files::BOM)) || lados.iter().any(|t| t.starts_with('\u{feff}'));
    let crlf = pasta.is_some_and(|b| b.windows(2).any(|w| w == b"\r\n")) || lados.iter().any(|t| t.contains("\r\n"));
    (bom, crlf)
}

/// O texto sem o BOM, que é o que a tela mostra.
fn sem_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Os bytes sem o BOM, que é o que a tela mostra.
fn sem_bom_bytes(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&files::BOM).unwrap_or(bytes)
}

/// O texto da pasta para a tela: sem BOM e com `\n`. Quem chama já confirmou que é UTF-8.
fn texto_da_tela(bytes: &[u8]) -> String {
    sem_bom(&String::from_utf8_lossy(sem_bom_bytes(bytes))).replace("\r\n", "\n")
}

/// Os lados de um conflito e o texto atual da pasta. Sem o estágio, o lado é `None`. Binário, ou texto que não é UTF-8,
/// vem com `binary` e sem `worktree`: a tela não o edita. Lado que não é UTF-8, ou grande demais, recusa o conflito inteiro.
pub fn conflict(dir: &Path, rel: &str) -> Result<Conflict, String> {
    let rel = spec(rel)?;
    let snap = Snap::read(dir, rel)?;
    let [_, ours, theirs] = snap.sides(dir, rel)?;
    let binary = snap.worktree.as_deref().is_some_and(|b| files::is_binary(b) || std::str::from_utf8(sem_bom_bytes(b)).is_err());
    let worktree = snap.worktree.as_deref().filter(|_| !binary).map(texto_da_tela);
    let sem = |t: Option<String>| t.map(|s| sem_bom(&s).replace("\r\n", "\n"));
    Ok(Conflict { ours: sem(ours), theirs: sem(theirs), worktree, binary, revision: snap.revision() })
}

/// Linha de marcador de conflito (`<<<<<<<`, `|||||||`, `=======`, `>>>>>>>`), com ou sem o rótulo depois.
fn has_markers(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim_end_matches('\r');
        l == "=======" || ["<<<<<<<", "|||||||", ">>>>>>>"].iter().any(|m| l == *m || l.starts_with(&format!("{m} ")))
    })
}

/// O texto da resolução com as quebras do arquivo (`crlf`). O BOM não entra aqui: a gravação o devolve ao arquivo.
fn layout(content: &str, crlf: bool) -> String {
    let body = content.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    if crlf {
        body.replace('\n', "\r\n")
    } else {
        body
    }
}

/// Grava o texto de resolução de um conflito. Só grava se o conflito ainda é o que a UI leu (`revision`: pasta e estágios),
/// e mantém BOM e CRLF. A gravação com arquivo na pasta é a verificada de `files::write_checked`. Com `mark_resolved`, prepara
/// o arquivo depois de gravar, mas recusa se ainda houver marcadores (aí nem grava). Sem `mark_resolved`, grava o rascunho e
/// deixa o arquivo em conflito. Arquivo que sumiu da pasta é criado sem sobrescrever: se alguém o criou depois da leitura,
/// a revisão não vale mais.
pub fn write_resolution(dir: &Path, rel: &str, content: &str, revision: &str, mark_resolved: bool) -> Result<(), String> {
    let rel = spec(rel)?;
    let snap = Snap::read(dir, rel)?;
    if mark_resolved && has_markers(content) {
        return Err("Ainda há marcadores de conflito no texto (<<<<<<<, =======, >>>>>>>). Remova-os antes de marcar como resolvido.".into());
    }
    if snap.revision() != revision {
        return Err(stale(rel));
    }
    let worktree = snap.worktree.as_deref();
    let lados = snap.sides(dir, rel)?;
    let refs: Vec<&str> = lados.iter().flatten().map(String::as_str).collect();
    if refs.iter().any(|t| misto(t)) {
        return Err(format!("{rel} mistura CRLF e LF: resolva este conflito fora do app."));
    }
    let (bom, crlf) = estilo(worktree, &refs);
    let body = layout(content, crlf);
    match worktree {
        Some(bytes) => {
            // Binário e texto que não é UTF-8 são recusados por `write_checked` antes de gravar; no UTF-8 a conversão não
            // troca nada. O BOM do próprio arquivo volta sozinho na gravação; o BOM que só os lados têm (o arquivo começa
            // pelo marcador) entra no texto.
            let base = String::from_utf8_lossy(sem_bom_bytes(bytes)).into_owned();
            let file_bom = bytes.starts_with(&files::BOM);
            let text = if bom && !file_bom { format!("\u{feff}{body}") } else { body };
            files::write_checked(dir, rel, &text, &base, file_bom)?;
        }
        None => {
            // A pasta não tem o arquivo: cria sem sobrescrever. Se outro programa o criou depois da leitura, a revisão já não vale.
            let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
            if let Err(why) = files::create_file(dir, parent, name) {
                return Err(if fs::symlink_metadata(dir.join(rel)).is_ok() { stale(rel) } else { why });
            }
            let text = if bom { format!("\u{feff}{body}") } else { body };
            if let Err(why) = files::write_checked(dir, rel, &text, "", false) {
                // O arquivo acabou de nascer vazio: se a gravação falhou, ele some de novo, sem tocar em conteúdo alheio.
                let file = dir.join(rel);
                if fs::metadata(&file).is_ok_and(|m| m.len() == 0) {
                    let _ = fs::remove_file(&file);
                }
                return Err(why);
            }
        }
    }
    if mark_resolved {
        proc::git_saida(dir, &["add", "--", rel]).map_err(|why| format!("Gravado, mas não foi possível preparar {rel}: {why}"))?;
    }
    Ok(())
}

/// Aceita a remoção num conflito em que um lado apagou o arquivo. Um lado tem de estar ausente e o outro existir: conflito
/// em que os dois existem não é remoção. Com arquivo na pasta, ele vai para a Lixeira antes de o Git registrar a remoção.
/// Se o Git recusar, a mensagem diz o que ficou feito.
pub fn accept_deletion(dir: &Path, rel: &str, revision: &str) -> Result<(), String> {
    let rel = spec(rel)?;
    let snap = Snap::read(dir, rel)?;
    if snap.revision() != revision {
        return Err(stale(rel));
    }
    match (snap.stages.contains(&2), snap.stages.contains(&3)) {
        (true, true) => return Err(format!("Os dois lados de {rel} existem: não é remoção. Resolva o texto ou escolha um lado.")),
        (false, false) => return Err(format!("Nenhum lado de {rel} existe: não há o que remover.")),
        _ => {}
    }
    let recycled = snap.worktree.is_some();
    if recycled {
        files::recycle(dir, rel)?;
    }
    proc::git_saida(dir, &["rm", "-q", "-f", "--", rel]).map(drop).map_err(|why| {
        if recycled {
            format!("{rel} foi para a Lixeira, mas o Git não registrou a remoção: {why}. O conflito continua no índice; o arquivo pode ser restaurado pela Lixeira.")
        } else {
            format!("O Git não registrou a remoção de {rel}: {why}. Nada foi alterado.")
        }
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Texto-base do editor: o arquivo no HEAD e no índice, para os marcadores de mudança no gutter.
// ---------------------------------------------------------------------------------------------------------------

/// O texto de `rel` no HEAD e no índice (estágio 0), comparável linha a linha com o que `files::read` devolve: com os
/// filtros e o fim de linha do checkout e sem o BOM. Cada lado é `None` quando o arquivo não está nele.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBase {
    pub head: Option<String>,
    pub index: Option<String>,
}

/// O texto-base de `rel` para o editor. `None` fora do modo repositório, em pasta, em arquivo que não está nem no HEAD
/// nem no índice e quando um dos lados é binário ou maior que `files::MAX_FILE`: aí o editor não mostra marcadores.
/// Só lê o repositório. A presença de cada lado vem de `ls-files -s` e `ls-tree`, então objeto ausente não vira erro.
pub fn file_base(dir: &Path, rel: &str) -> Result<Option<FileBase>, String> {
    if modo(dir)? != Modo::Repo {
        return Ok(None);
    }
    let rel = spec(rel)?;
    guarded(dir, rel)?;
    if is_folder(dir, rel)? {
        return Ok(None);
    }
    let no_indice = index_blob(dir, rel)?;
    let no_head = has_head(dir) && head_blob(dir, rel)?;
    if !no_indice && !no_head {
        return Ok(None);
    }
    let head = if no_head { texto_base(dir, &format!("HEAD:{rel}"))? } else { None };
    let index = if no_indice { texto_base(dir, &format!(":{rel}"))? } else { None };
    // Lado presente que volta sem texto é binário ou grande demais.
    if (no_head && head.is_none()) || (no_indice && index.is_none()) {
        return Ok(None);
    }
    Ok(Some(FileBase { head, index }))
}

/// `rel` está no índice no estágio 0 como arquivo (não submódulo).
fn index_blob(dir: &Path, rel: &str) -> Result<bool, String> {
    Ok(index_mode(dir, rel)?.is_some_and(|modo| modo != "160000"))
}

/// O modo de `rel` no índice no estágio 0 (`100644`, `100755`, `120000`, `160000`), ou `None` quando não há esse estágio.
/// Registro de `ls-files -s`: `modo SP hash SP estágio TAB caminho`.
fn index_mode(dir: &Path, rel: &str) -> Result<Option<String>, String> {
    let raw = proc::git_bytes(dir, &["ls-files", "-s", "-z", "--", rel])?;
    Ok(raw.split(|b| *b == 0).find_map(|record| {
        let tab = record.iter().position(|&b| b == b'\t')?;
        let campos: Vec<&[u8]> = record[..tab].split(|&b| b == b' ').collect();
        (&record[tab + 1..] == rel.as_bytes() && campos.len() == 3 && campos[2] == b"0").then(|| String::from_utf8_lossy(campos[0]).into_owned())
    }))
}

/// `rel` está no HEAD como blob. Registro de `ls-tree`: `modo SP tipo SP hash TAB caminho`.
fn head_blob(dir: &Path, rel: &str) -> Result<bool, String> {
    let raw = proc::git_bytes(dir, &["ls-tree", "-z", "HEAD", "--", rel])?;
    Ok(raw.split(|b| *b == 0).any(|record| {
        let Some(tab) = record.iter().position(|&b| b == b'\t') else { return false };
        &record[tab + 1..] == rel.as_bytes() && record[..tab].split(|&b| b == b' ').nth(1) == Some(b"blob")
    }))
}

/// O texto de `objeto` (`HEAD:<rel>` ou `:<rel>`, que já se sabe existir) como o checkout o escreveria, sem o BOM.
/// `None` quando é binário ou maior que `files::MAX_FILE`, antes ou depois dos filtros.
fn texto_base(dir: &Path, objeto: &str) -> Result<Option<String>, String> {
    let size: u64 = proc::git_saida(dir, &["cat-file", "-s", objeto])?.trim().parse().unwrap_or(u64::MAX);
    if size > files::MAX_FILE {
        return Ok(None);
    }
    let bytes = proc::git_bytes(dir, &["cat-file", "--filters", objeto])?;
    if bytes.len() as u64 > files::MAX_FILE || files::is_binary(&bytes) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(sem_bom_bytes(&bytes)).into_owned()))
}

/// Texto com `\r\n` e `\n` solto ao mesmo tempo: a gravação não sabe que fim de linha cada linha nova leva.
fn misto(text: &str) -> bool {
    let crlf = text.matches("\r\n").count();
    crlf > 0 && crlf != text.matches('\n').count()
}

/// O texto na forma que o editor compara: todo `\r\n` e `\r` vira `\n`.
fn so_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Grava `text` (com `\n` e sem BOM) como o conteúdo de `rel` no índice, para preparar ou tirar do preparo um trecho pelo
/// editor. `expected` é o texto do índice que a tela viu, na forma de `file_base` com `so_lf`: se o índice mudou desde
/// então, nada é gravado. O texto volta à forma do arquivo antes de gravar: `\r\n` quando o checkout do índice usa `\r\n`
/// e o BOM quando o blob do índice o tem. O `hash-object --path` aplica os filtros de limpeza e o `core.autocrlf`, como
/// um `git add`, e o `update-index --cacheinfo` mantém o modo do índice. Só o índice muda: a pasta de trabalho não.
pub fn write_index(dir: &Path, rel: &str, text: &str, expected: &str) -> Result<(), String> {
    if modo(dir)? != Modo::Repo {
        return Err("O projeto não é a raiz de um repositório Git.".into());
    }
    let rel = spec(rel)?;
    guarded(dir, rel)?;
    if is_folder(dir, rel)? {
        return Err(format!("{rel} é uma pasta, não um arquivo."));
    }
    let perm = match index_mode(dir, rel)?.as_deref() {
        Some(m @ ("100644" | "100755")) => m.to_string(),
        Some(_) => return Err(format!("{rel} não é um arquivo comum no índice (link ou submódulo).")),
        None => return Err(format!("{rel} não está no índice (não rastreado, em conflito ou removido do índice): prepare o arquivo inteiro.")),
    };
    let indice = format!(":{rel}");
    let Some(atual) = texto_base(dir, &indice)? else {
        return Err(format!("{rel} é binário ou grande demais no índice para preparar por trecho."));
    };
    if so_lf(&atual) != expected {
        return Err(format!("O índice de {rel} mudou desde que o editor o leu. Tente de novo."));
    }
    if misto(&atual) {
        return Err(format!("{rel} mistura CRLF e LF no índice: prepare o arquivo inteiro."));
    }
    let mut novo = if atual.contains("\r\n") { text.replace("\r\n", "\n").replace('\n', "\r\n") } else { text.to_string() };
    let blob = proc::git_bytes(dir, &["cat-file", "blob", &indice])?;
    if blob.starts_with(&files::BOM) {
        novo.insert(0, '\u{feff}');
    }
    let caminho = format!("--path={rel}");
    let mut args = vec!["hash-object", "-w", "--stdin", caminho.as_str()];
    // O `git add` não converte o fim de linha quando a conversão é automática e o blob no índice já tem `\r`; o
    // `hash-object --stdin` não lê o índice e converteria o arquivo inteiro. Sem os atributos, desligar o `core.autocrlf`
    // basta; o `text=auto` não se desliga por configuração, então aí o blob vai sem filtro.
    if blob.contains(&b'\r') {
        match atributos_eol(dir, rel)? {
            (text, _) if text == "auto" => args[3] = "--no-filters",
            (text, eol) if text == "unspecified" && eol == "unspecified" => args.splice(0..0, ["-c", "core.autocrlf=false"]).for_each(drop),
            _ => {}
        }
    }
    let hash = proc::git_entrada_saida(dir, &args, novo.as_bytes())?;
    let info = format!("{perm},{},{rel}", hash.trim());
    proc::git_saida(dir, &["update-index", "--cacheinfo", &info]).map(drop)
}

/// Os atributos `text` e `eol` de `rel` (`set`, `unset`, `unspecified` ou o valor). Registro de `check-attr -z`:
/// `caminho NUL atributo NUL valor NUL`.
fn atributos_eol(dir: &Path, rel: &str) -> Result<(String, String), String> {
    let out = proc::git_saida(dir, &["check-attr", "-z", "text", "eol", "--", rel])?;
    let campos: Vec<&str> = out.split('\0').collect();
    let valor = |nome: &str| campos.chunks(3).find(|c| c.len() == 3 && c[1] == nome).map_or("unspecified", |c| c[2]).to_string();
    Ok((valor("text"), valor("eol")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{commit_all, git, identity, write, Tmp};
    #[cfg(windows)]
    use crate::testutil::junction;
    use std::fs;

    /// Repositório novo, sem commit, com a branch `main`.
    fn repo(tmp: &Tmp, name: &str) -> std::path::PathBuf {
        let dir = tmp.0.join(name);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        identity(&dir);
        dir
    }

    fn find<'a>(st: &'a Status, path: &str) -> &'a GitFile {
        st.files.iter().find(|f| f.path == path).unwrap_or_else(|| panic!("{path} não está no status: {st:?}"))
    }

    fn git_status_of(dir: &Path) -> Status {
        status(dir).unwrap()
    }

    #[test]
    fn status_mostra_cada_mudanca_no_lugar_certo() {
        let tmp = Tmp::new("orq-git", "status");
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
        assert_eq!(st.head, git(&dir, &["rev-parse", "HEAD"]).trim());
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
        let tmp = Tmp::new("orq-git", "conflito");
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
        let st = git_status_of(&dir);
        assert_eq!(st.branch, hash.trim());
        assert_eq!(st.head, git(&dir, &["rev-parse", "HEAD"]).trim());

        let empty = repo(&tmp, "vazio");
        write(&empty, "a.txt", "a");
        let st = git_status_of(&empty);
        assert_eq!(st.branch, "main");
        assert_eq!(st.head, "", "sem commit o HEAD não tem hash");
        assert_eq!(find(&st, "a.txt").unstaged, Some(Kind::Untracked));
    }

    #[test]
    fn status_conta_ahead_e_behind_contra_o_remoto_local() {
        let tmp = Tmp::new("orq-git", "ahead");
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
        let tmp = Tmp::new("orq-git", "diff-linhas");
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
        let tmp = Tmp::new("orq-git", "diff-outros");
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
        let tmp = Tmp::new("orq-git", "diff-staged");
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
        let tmp = Tmp::new("orq-git", "diff-binario");
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
        let tmp = Tmp::new("orq-git", "stage");
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
        let tmp = Tmp::new("orq-git", "pathspec");
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
        let tmp = Tmp::new("orq-git", "fora");
        let dir = repo(&tmp, "r");
        for rel in ["..", "../x", "a/../../x", "/etc", "C:/x", ".", "./a", ""] {
            assert!(stage(&dir, &[rel.into()]).is_err(), "{rel} deveria ser recusado");
            assert!(diff(&dir, rel, false).is_err(), "{rel} deveria ser recusado no diff");
        }
        assert_eq!(stage(&dir, &["../x".into()]).unwrap_err(), "Caminho fora do projeto: ../x");
    }

    #[test]
    fn stage_e_unstage_funcionam_sem_nenhum_commit() {
        let tmp = Tmp::new("orq-git", "unborn");
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
        let tmp = Tmp::new("orq-git", "commit");
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
        let tmp = Tmp::new("orq-git", "hook");
        let dir = repo(&tmp, "r");
        write(&dir, ".git/hooks/pre-commit", "#!/bin/sh\necho bloqueado pelo hook >&2\nexit 1\n");
        write(&dir, "a.txt", "a");
        stage(&dir, &[]).unwrap();
        let why = commit(&dir, "tentativa").unwrap_err();
        assert!(why.contains("bloqueado pelo hook"), "o erro traz o stderr do hook: {why}");
    }

    #[test]
    fn sincronizar_sem_remoto_da_erro_em_portugues() {
        let tmp = Tmp::new("orq-git", "sem-remoto");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        commit_all(&dir, "a");
        for why in [fetch(&dir).unwrap_err(), pull(&dir).unwrap_err(), push(&dir).unwrap_err()] {
            assert_eq!(why, "Nenhum remoto configurado neste repositório. Configure um remoto, como origin, para sincronizar.");
        }
    }

    /// Um remoto bare numa pasta temporária e um repositório que já o tem como `origin`, com a branch publicada.
    fn with_remote(tmp: &Tmp) -> (std::path::PathBuf, std::path::PathBuf) {
        let bare = tmp.0.join("remoto.git");
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
        let outro = tmp.0.join("outro");
        git(&tmp.0, &["clone", "-q", bare.to_str().unwrap(), outro.to_str().unwrap()]);
        identity(&outro);
        write(&outro, file, "de fora");
        commit_all(&outro, file);
        git(&outro, &["push", "-q", "origin", "main"]);
    }

    #[test]
    fn fetch_traz_o_que_o_remoto_tem() {
        let tmp = Tmp::new("orq-git", "fetch");
        let (bare, dir) = with_remote(&tmp);
        other_pushes(&tmp, &bare, "de-fora.txt");
        fetch(&dir).unwrap();
        assert_eq!(git_status_of(&dir).behind, 1);
    }

    #[test]
    fn pull_traz_o_commit_do_remoto() {
        let tmp = Tmp::new("orq-git", "pull");
        let (bare, dir) = with_remote(&tmp);
        other_pushes(&tmp, &bare, "de-fora.txt");
        pull(&dir).unwrap();
        assert!(dir.join("de-fora.txt").is_file());
        assert_eq!(git_status_of(&dir).behind, 0);
    }

    #[test]
    fn pull_com_historia_dividida_faz_merge_sem_abrir_editor() {
        let tmp = Tmp::new("orq-git", "pull-merge");
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
        let tmp = Tmp::new("orq-git", "push");
        let (bare, dir) = with_remote(&tmp);
        write(&dir, "b.txt", "b");
        commit_all(&dir, "b");
        push(&dir).unwrap();
        assert_eq!(git(&bare, &["rev-parse", "main"]), git(&dir, &["rev-parse", "HEAD"]));
    }

    #[test]
    fn push_de_branch_sem_upstream_publica_em_origin_e_configura_o_upstream() {
        let tmp = Tmp::new("orq-git", "push-novo");
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
        let tmp = Tmp::new("orq-git", "push-sem-origin");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        commit_all(&dir, "a");
        let outro = tmp.0.join("outro.git");
        git(&tmp.0, &["init", "-q", "--bare", "outro.git"]);
        git(&dir, &["remote", "add", "outro", outro.to_str().unwrap()]);
        assert!(push(&dir).unwrap_err().contains("origin"));
    }

    #[test]
    fn branches_lista_troca_e_cria() {
        let tmp = Tmp::new("orq-git", "branches");
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
        let tmp = Tmp::new("orq-git", "branch-erro");
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
        let tmp = Tmp::new("orq-git", "stderr");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        commit_all(&dir, "a");
        let why = switch(&dir, "nao-existe").unwrap_err();
        assert!(why.contains("nao-existe"), "o erro traz o que o Git escreveu: {why}");
        assert_eq!(branches(&dir).unwrap().current, "main");
    }

    #[test]
    fn repositorio_sem_commit_lista_branch_e_troca_pelo_nome() {
        let tmp = Tmp::new("orq-git", "branches-unborn");
        let dir = repo(&tmp, "r");
        let b = branches(&dir).unwrap();
        assert_eq!((b.current.as_str(), b.local.is_empty()), ("main", true));
        create_branch(&dir, "trabalho").unwrap();
        assert_eq!(branches(&dir).unwrap().current, "trabalho");
    }

    /// Texto com `n` linhas numeradas, para o Git separar blocos que estão longe um do outro.
    fn texto(n: usize) -> String {
        (1..=n).map(|i| format!("linha {i}\n")).collect()
    }

    /// Quantos blocos o diff de `rel` tem, lidos do patch que a revisão devolve.
    fn quantos_blocos(dir: &Path, rel: &str, staged: bool) -> usize {
        split_hunks(&review_diff(dir, rel, staged).unwrap().diff.patch).1.len()
    }

    /// Um bloco como a UI o receberia: da revisão lida agora do diff.
    fn bloco(dir: &Path, rel: &str, staged: bool, index: usize) -> HunkRef {
        let r = review_diff(dir, rel, staged).unwrap();
        let (_, blocks) = split_hunks(&r.diff.patch);
        let block = blocks.get(index).unwrap_or_else(|| panic!("sem bloco {index}: {r:?}"));
        HunkRef { path: rel.into(), index, header: block_header(block).into(), revision: r.revision.clone() }
    }

    /// Duas alterações longe uma da outra: dois blocos no diff não preparado.
    fn dois_blocos(dir: &Path, rel: &str) {
        write(dir, rel, texto(30).replace("linha 2\n", "dois\n").replace("linha 28\n", "vinte e oito\n"));
    }

    #[test]
    fn bloco_prepara_tira_e_descarta_sem_mexer_nos_outros() {
        let tmp = Tmp::new("orq-git", "bloco");
        let dir = repo(&tmp, "r");
        write(&dir, "f.txt", texto(30));
        commit_all(&dir, "base");
        dois_blocos(&dir, "f.txt");
        assert_eq!(quantos_blocos(&dir, "f.txt", false), 2);

        stage_hunk(&dir, &bloco(&dir, "f.txt", false, 0)).unwrap();
        let cached = git(&dir, &["diff", "--cached"]);
        assert!(cached.contains("+dois") && !cached.contains("vinte e oito"), "só o primeiro bloco foi preparado: {cached}");
        let st = git_status_of(&dir);
        assert_eq!((find(&st, "f.txt").staged, find(&st, "f.txt").unstaged), (Some(Kind::Modified), Some(Kind::Modified)));
        assert_eq!(quantos_blocos(&dir, "f.txt", false), 1, "sobrou só o bloco não preparado");

        // Descarta o bloco que sobrou na pasta: o preparado continua preparado e a linha volta ao original.
        discard_hunk(&dir, &bloco(&dir, "f.txt", false, 0)).unwrap();
        let conteudo = fs::read_to_string(dir.join("f.txt")).unwrap();
        assert!(conteudo.contains("dois\n") && conteudo.contains("linha 28\n") && !conteudo.contains("vinte e oito"), "{conteudo}");
        assert!(git(&dir, &["diff", "--cached"]).contains("+dois"), "o descarte não tocou o índice");
        assert_eq!(quantos_blocos(&dir, "f.txt", false), 0);

        // Tira do preparo: o índice volta ao original e a pasta mantém a mudança, que aparece como não preparada.
        unstage_hunk(&dir, &bloco(&dir, "f.txt", true, 0)).unwrap();
        assert_eq!(git(&dir, &["diff", "--cached", "--name-only"]).trim(), "");
        let solto = review_diff(&dir, "f.txt", false).unwrap();
        assert!(solto.diff.patch.contains("+dois"), "{}", solto.diff.patch);
        assert_eq!(fs::read_to_string(dir.join("f.txt")).unwrap().matches("dois\n").count(), 1);
    }

    #[test]
    fn bloco_com_snapshot_obsoleto_e_recusado_sem_tocar_o_indice() {
        let tmp = Tmp::new("orq-git", "obsoleto");
        let dir = repo(&tmp, "r");
        write(&dir, "f.txt", texto(30));
        commit_all(&dir, "base");
        dois_blocos(&dir, "f.txt");
        let velho = bloco(&dir, "f.txt", false, 0);
        write(&dir, "f.txt", texto(30).replace("linha 2\n", "outro\n"));

        let why = stage_hunk(&dir, &velho).unwrap_err();
        assert!(why.contains("mudou desde que o diff foi lido"), "{why}");
        assert!(git(&dir, &["diff", "--cached", "--name-only"]).trim().is_empty());
        let why = discard_hunk(&dir, &velho).unwrap_err();
        assert!(why.contains("mudou desde"), "{why}");
        assert!(fs::read_to_string(dir.join("f.txt")).unwrap().contains("outro\n"), "o descarte obsoleto não mexe na pasta");
    }

    #[test]
    fn bloco_com_indice_caminho_ou_cabecalho_invalido_e_recusado() {
        let tmp = Tmp::new("orq-git", "bloco-invalido");
        let dir = repo(&tmp, "r");
        write(&dir, "f.txt", texto(30));
        write(&dir, "[a] -x.txt", "x\n");
        commit_all(&dir, "base");
        dois_blocos(&dir, "f.txt");
        let ok = bloco(&dir, "f.txt", false, 0);

        let mut r = ok.clone();
        r.index = 9;
        assert!(stage_hunk(&dir, &r).unwrap_err().contains("não existe mais"));
        let mut r = ok.clone();
        r.header = "@@ -9,9 +9,9 @@".into();
        assert!(stage_hunk(&dir, &r).unwrap_err().contains("não existe mais"));
        for path in ["../f.txt", "/etc/passwd", "a/../../f.txt", ""] {
            let mut r = ok.clone();
            r.path = path.into();
            assert!(stage_hunk(&dir, &r).is_err(), "{path} deveria ser recusado");
        }
        let mut r = ok.clone();
        r.path = "naoexiste.txt".into();
        assert!(stage_hunk(&dir, &r).unwrap_err().contains("não está rastreado"));
        // Nome com colchete e hífen inicial é caminho literal: a revisão dele é aceita.
        write(&dir, "[a] -x.txt", "x\ny\n");
        let literal = bloco(&dir, "[a] -x.txt", false, 0);
        stage_hunk(&dir, &literal).unwrap();
        assert_eq!(find(&git_status_of(&dir), "[a] -x.txt").staged, Some(Kind::Modified));
        assert!(git(&dir, &["diff", "--cached", "--name-only"]).contains("[a] -x.txt"));
    }

    #[test]
    fn bloco_preserva_fim_de_linha_crlf_do_arquivo() {
        let tmp = Tmp::new("orq-git", "bloco-crlf");
        let dir = repo(&tmp, "r");
        write(&dir, "f.txt", texto(30).replace('\n', "\r\n"));
        commit_all(&dir, "base");
        write(&dir, "f.txt", texto(30).replace('\n', "\r\n").replace("linha 2\r\n", "dois\r\n").replace("linha 28\r\n", "vinte e oito\r\n"));
        stage_hunk(&dir, &bloco(&dir, "f.txt", false, 0)).unwrap();
        let blob = git(&dir, &["show", ":f.txt"]);
        assert!(blob.contains("dois\r\n") && blob.contains("linha 3\r\n"), "o índice ficou com CRLF: {blob:?}");
        assert!(!blob.contains("vinte e oito"));
        assert!(fs::read_to_string(dir.join("f.txt")).unwrap().contains("vinte e oito\r\n"), "a pasta não mudou no outro bloco");
    }

    #[test]
    fn bloco_de_arquivo_apagado_e_de_repositorio_sem_commit() {
        let tmp = Tmp::new("orq-git", "bloco-apagado");
        let dir = repo(&tmp, "r");
        write(&dir, "apagar.txt", "x\ny\n");
        commit_all(&dir, "base");
        fs::remove_file(dir.join("apagar.txt")).unwrap();
        stage_hunk(&dir, &bloco(&dir, "apagar.txt", false, 0)).unwrap();
        assert_eq!(find(&git_status_of(&dir), "apagar.txt").staged, Some(Kind::Deleted));
        unstage_hunk(&dir, &bloco(&dir, "apagar.txt", true, 0)).unwrap();
        assert_eq!(find(&git_status_of(&dir), "apagar.txt").unstaged, Some(Kind::Deleted));
        discard_hunk(&dir, &bloco(&dir, "apagar.txt", false, 0)).unwrap();
        assert_eq!(fs::read_to_string(dir.join("apagar.txt")).unwrap(), "x\ny\n");

        // Sem commit: o bloco preparado (arquivo novo) sai do índice sem apagar nada da pasta.
        let vazio = repo(&tmp, "vazio");
        write(&vazio, "a.txt", "a\n");
        git(&vazio, &["add", "a.txt"]);
        unstage_hunk(&vazio, &bloco(&vazio, "a.txt", true, 0)).unwrap();
        assert_eq!(find(&git_status_of(&vazio), "a.txt").staged, None);
        assert!(vazio.join("a.txt").is_file());
    }

    #[test]
    fn bloco_de_binario_e_de_nao_rastreado_e_recusado_com_motivo() {
        let tmp = Tmp::new("orq-git", "bloco-binario");
        let dir = repo(&tmp, "r");
        write(&dir, "dado.bin", [0u8, 1, 2]);
        commit_all(&dir, "base");
        write(&dir, "dado.bin", [9u8, 0, 8]);
        let r = review_diff(&dir, "dado.bin", false).unwrap();
        assert!(r.diff.binary);
        let why = stage_hunk(&dir, &HunkRef { path: "dado.bin".into(), index: 0, header: String::new(), revision: r.revision }).unwrap_err();
        assert!(why.contains("binário"), "{why}");

        write(&dir, "novo.txt", "n\n");
        let r = review_diff(&dir, "novo.txt", false).unwrap();
        assert!(r.untracked);
        let why = stage_hunk(&dir, &HunkRef { path: "novo.txt".into(), index: 0, header: String::new(), revision: r.revision }).unwrap_err();
        assert!(why.contains("não está rastreado"), "{why}");
    }

    #[test]
    fn descarte_por_arquivo_mantem_o_que_esta_preparado() {
        let tmp = Tmp::new("orq-git", "descarte-arquivo");
        let dir = repo(&tmp, "r");
        write(&dir, "f.txt", "um\n");
        commit_all(&dir, "base");
        write(&dir, "f.txt", "dois\n");
        git(&dir, &["add", "f.txt"]);
        write(&dir, "f.txt", "tres\n");
        let rev = review_diff(&dir, "f.txt", false).unwrap().revision;

        assert!(discard_file(&dir, "f.txt", "obsoleto").unwrap_err().contains("mudou desde"));
        discard_file(&dir, "f.txt", &rev).unwrap();
        assert_eq!(fs::read_to_string(dir.join("f.txt")).unwrap(), "dois\n", "a pasta volta ao que está no índice");
        assert!(git(&dir, &["diff", "--cached"]).contains("+dois"), "o preparado não foi tocado");

        write(&dir, "novo.txt", "n\n");
        let why = discard_file(&dir, "novo.txt", "x").unwrap_err();
        assert!(why.contains("não está rastreado") && why.contains("Lixeira"), "{why}");
        assert!(dir.join("novo.txt").is_file(), "não rastreado nunca é apagado pelo descarte");
    }

    #[test]
    fn comparador_recebe_o_arquivo_inteiro_e_os_blocos_seguem_o_contexto_de_tres() {
        let tmp = Tmp::new("orq-git", "display");
        let dir = repo(&tmp, "r");
        write(&dir, "f.txt", &texto(30));
        commit_all(&dir, "base");
        write(&dir, "f.txt", &texto(30).replace("linha 2
", "dois
").replace("linha 28
", "vinte e oito
"));
        let r = review_diff(&dir, "f.txt", false).unwrap();
        // O arquivo inteiro: as linhas antes, entre e depois das duas mudanças, num bloco só.
        for linha in ["linha 1
", "linha 15
", "linha 30
"] {
            assert!(r.display.patch.contains(linha), "{linha} sumiu do arquivo inteiro");
        }
        assert_eq!(split_hunks(&r.display.patch).1.len(), 1, "o arquivo inteiro é um bloco só");
        // Os blocos seguem o -U3: as duas mudanças são blocos separados e a linha 15 fica fora deles.
        assert_eq!(split_hunks(&r.diff.patch).1.len(), 2);
        assert!(!r.diff.patch.contains("linha 15
"));
        // Não rastreado: o arquivo inteiro já é o patch, e não há bloco.
        write(&dir, "novo.txt", "n
");
        let novo = review_diff(&dir, "novo.txt", false).unwrap();
        assert!(novo.untracked && novo.display == novo.diff);
    }

    #[test]
    fn historia_pagina_filtra_por_caminho_e_segue_renomeacao() {
        let tmp = Tmp::new("orq-git", "historia");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a\n");
        commit_all(&dir, "c1 raiz");
        write(&dir, "-x.txt", "x\n");
        write(&dir, "[b] c.txt", "b\n");
        commit_all(&dir, "c2 com hifen");
        git(&dir, &["mv", "a.txt", "renomeado [r] x.txt"]);
        git(&dir, &["commit", "-q", "-m", "c3 renomeia"]);

        let todos = history(&dir, 10, 0, None).unwrap();
        let assuntos: Vec<&str> = todos.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(assuntos, ["c3 renomeia", "c2 com hifen", "c1 raiz"]);
        assert_eq!(todos[0].author, "teste");
        assert_eq!(todos[0].short.len(), 7);
        assert!(todos[0].date.contains('T'), "data ISO: {}", todos[0].date);
        assert_eq!(history(&dir, 2, 0, None).unwrap().len(), 2);
        assert_eq!(history(&dir, 2, 2, None).unwrap()[0].subject, "c1 raiz");
        assert!(history(&dir, 2, 9, None).unwrap().is_empty());

        let do_arquivo: Vec<String> = history(&dir, 10, 0, Some("renomeado [r] x.txt")).unwrap().into_iter().map(|c| c.subject).collect();
        assert_eq!(do_arquivo, ["c3 renomeia", "c1 raiz"], "acompanha a renomeação");
        assert_eq!(history(&dir, 10, 0, Some("-x.txt")).unwrap()[0].subject, "c2 com hifen");
        assert_eq!(history(&dir, 10, 0, Some("[b] c.txt")).unwrap()[0].subject, "c2 com hifen");
        assert!(history(&dir, 10, 0, Some("../fora")).is_err());

        let vazio = repo(&tmp, "vazio");
        assert!(history(&vazio, 5, 0, None).unwrap().is_empty());
    }

    fn assuntos(todos: &[GraphCommit]) -> Vec<&str> {
        todos.iter().map(|c| c.commit.subject.as_str()).collect()
    }

    #[test]
    fn graph_linear_traz_pais_datas_e_a_branch_atual() {
        let tmp = Tmp::new("orq-git", "graph-linear");
        let dir = repo(&tmp, "r");
        for i in 1..=3 {
            write(&dir, "a.txt", i.to_string());
            commit_all(&dir, &format!("c{i}"));
        }

        let todos = graph(&dir, 10, 0).unwrap();
        assert_eq!(assuntos(&todos), ["c3", "c2", "c1"]);
        assert_eq!(todos[0].commit.hash, git(&dir, &["rev-parse", "HEAD"]).trim());
        assert_eq!(todos[0].commit.short.len(), 7);
        assert_eq!(todos[0].commit.author, "teste");
        assert!(todos[0].commit.date.contains('T'), "data ISO: {}", todos[0].commit.date);
        assert_eq!(todos[0].parents, [todos[1].commit.hash.as_str()]);
        assert_eq!(todos[1].parents, [todos[2].commit.hash.as_str()]);
        assert!(todos[2].parents.is_empty(), "commit raiz não tem pai");
        assert_eq!(todos[0].refs, [GitRef { name: "main".into(), kind: RefKind::Branch, current: true }]);
        assert!(todos[1].refs.is_empty());
        // O JSON segue plano: os campos do `Commit` saem no mesmo nível que `parents` e `refs`.
        let json = serde_json::to_value(&todos[0]).unwrap();
        assert_eq!(json["hash"], todos[0].commit.hash.as_str());
        assert!(json.get("commit").is_none() && json["parents"].is_array() && json["refs"].is_array(), "{json}");
    }

    #[test]
    fn graph_mostra_branch_lateral_mesclada_com_merge_de_dois_pais() {
        let tmp = Tmp::new("orq-git", "graph-merge");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "1");
        commit_all(&dir, "base");
        git(&dir, &["switch", "-q", "-c", "lateral"]);
        write(&dir, "l.txt", "1");
        commit_all(&dir, "lateral 1");
        write(&dir, "l.txt", "2");
        commit_all(&dir, "lateral 2");
        git(&dir, &["switch", "-q", "main"]);
        write(&dir, "m.txt", "1");
        commit_all(&dir, "main 1");
        git(&dir, &["merge", "-q", "--no-ff", "-m", "mescla lateral", "lateral"]);

        let todos = graph(&dir, 10, 0).unwrap();
        assert_eq!(todos.len(), 5, "{:?}", assuntos(&todos));
        for subject in ["base", "lateral 1", "lateral 2", "main 1"] {
            assert!(assuntos(&todos).contains(&subject), "{subject} some do gráfico");
        }
        let merge = &todos[0];
        assert_eq!(merge.commit.subject, "mescla lateral");
        assert_eq!(merge.parents, [git(&dir, &["rev-parse", "HEAD^1"]).trim(), git(&dir, &["rev-parse", "HEAD^2"]).trim()]);
        let lateral = todos.iter().find(|c| c.commit.subject == "lateral 2").unwrap();
        assert_eq!(lateral.refs, [GitRef { name: "lateral".into(), kind: RefKind::Branch, current: false }]);
    }

    #[test]
    fn graph_separa_branch_atual_de_branch_e_tag_e_ignora_o_resto() {
        let tmp = Tmp::new("orq-git", "graph-refs");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "1");
        commit_all(&dir, "c1");
        git(&dir, &["branch", "outra"]);
        git(&dir, &["tag", "v1"]);
        write(&dir, "a.txt", "2");
        commit_all(&dir, "c2");

        let todos = graph(&dir, 10, 0).unwrap();
        assert_eq!(todos[0].refs, [GitRef { name: "main".into(), kind: RefKind::Branch, current: true }]);
        // A branch atual vem primeiro, depois as branches, as remotas e as tags.
        assert_eq!(
            todos[1].refs,
            [GitRef { name: "outra".into(), kind: RefKind::Branch, current: false }, GitRef { name: "v1".into(), kind: RefKind::Tag, current: false }]
        );
    }

    #[test]
    fn graph_com_head_destacado_marca_o_head_como_atual() {
        let tmp = Tmp::new("orq-git", "graph-destacado");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "1");
        commit_all(&dir, "c1");
        git(&dir, &["checkout", "-q", "--detach"]);

        let todos = graph(&dir, 10, 0).unwrap();
        assert_eq!(
            todos[0].refs,
            [GitRef { name: "HEAD".into(), kind: RefKind::Head, current: true }, GitRef { name: "main".into(), kind: RefKind::Branch, current: false }]
        );
    }

    #[test]
    fn graph_pagina_com_skip_e_limita_o_limite_entre_1_e_200() {
        let tmp = Tmp::new("orq-git", "graph-pagina");
        let dir = repo(&tmp, "r");
        for i in 1..=5 {
            write(&dir, "a.txt", i.to_string());
            commit_all(&dir, &format!("c{i}"));
        }

        assert_eq!(assuntos(&graph(&dir, 2, 0).unwrap()), ["c5", "c4"]);
        assert_eq!(assuntos(&graph(&dir, 2, 2).unwrap()), ["c3", "c2"]);
        assert_eq!(assuntos(&graph(&dir, 2, 4).unwrap()), ["c1"]);
        assert!(graph(&dir, 2, 9).unwrap().is_empty());
        assert_eq!(graph(&dir, 0, 0).unwrap().len(), 1, "limite 0 vira 1");
        assert_eq!(graph(&dir, 1000, 0).unwrap().len(), 5, "o total cabe no teto");
    }

    #[test]
    fn graph_de_repositorio_sem_commit_e_vazio() {
        let tmp = Tmp::new("orq-git", "graph-vazio");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a");
        assert!(graph(&dir, 5, 0).unwrap().is_empty());
    }

    #[test]
    fn graph_traz_o_commit_do_upstream_com_ref_remota_e_ignora_origin_head() {
        let tmp = Tmp::new("orq-git", "graph-upstream");
        let (bare, dir) = with_remote(&tmp);
        // Cria `refs/remotes/origin/HEAD` localmente; ele não pode aparecer no gráfico.
        git(&dir, &["remote", "set-head", "origin", "main"]);
        other_pushes(&tmp, &bare, "c.txt");
        git(&dir, &["fetch", "-q"]);

        let todos = graph(&dir, 10, 0).unwrap();
        assert_eq!(assuntos(&todos), ["c.txt", "a"], "o commit do upstream sai antes do HEAD");
        assert_eq!(todos[0].parents, [todos[1].commit.hash.as_str()]);
        assert_eq!(todos[0].refs, [GitRef { name: "origin/main".into(), kind: RefKind::Remote, current: false }]);
        assert_eq!(todos[1].refs, [GitRef { name: "main".into(), kind: RefKind::Branch, current: true }]);
        assert!(todos.iter().flat_map(|c| &c.refs).all(|r| r.name != "origin/HEAD"));
    }

    #[test]
    fn github_commit_url_aceita_remotos_https_e_ssh() {
        let hash = "a1b2c3d4";
        let expected = "https://github.com/dona/repo/commit/a1b2c3d4";
        for remote in [
            "https://github.com/dona/repo.git",
            "https://token@github.com/dona/repo",
            "ssh://git@github.com/dona/repo.git",
            "git@github.com:dona/repo.git",
        ] {
            assert_eq!(github_commit_url_from_remote(remote, hash).as_deref(), Some(expected), "{remote}");
        }
        assert_eq!(github_commit_url_from_remote("https://example.com/dona/repo.git", hash), None);
        assert_eq!(github_commit_url_from_remote("git@github.com:dona/../repo", hash), None);
    }

    #[test]
    fn commit_hover_devolve_mensagem_estatisticas_e_url_no_repo_temporario() {
        let tmp = Tmp::new("orq-git", "commit-hover");
        let dir = repo(&tmp, "r");
        write(&dir, "edit.txt", "antes\n");
        write(&dir, "removed.txt", "remover\n");
        commit_all(&dir, "base");

        write(&dir, "edit.txt", "depois\nextra\n");
        fs::remove_file(dir.join("removed.txt")).unwrap();
        write(&dir, "added.txt", "novo\narquivo\n");
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "Assunto completo", "-m",
            "Corpo com detalhes.\n\nCo-Authored-By: Ana Exemplo <ana@example.com>"]);
        git(&dir, &["remote", "add", "origin", "https://github.com/dona/repo.git"]);

        let hash = git(&dir, &["rev-parse", "HEAD"]).trim().to_string();
        let detail = commit_hover(&dir, &hash).unwrap();
        assert!(!detail.author.is_empty());
        assert!(detail.date.contains('T'));
        assert!(detail.message.starts_with("Assunto completo\n\nCorpo com detalhes."));
        assert!(detail.message.contains("Co-Authored-By: Ana Exemplo <ana@example.com>"));
        assert_eq!((detail.files_changed, detail.insertions, detail.deletions), (3, 4, 2));
        let expected_url = format!("https://github.com/dona/repo/commit/{hash}");
        assert_eq!(detail.github_url.as_deref(), Some(expected_url.as_str()));
        assert!(commit_hover(&dir, "HEAD").unwrap_err().contains("Commit inválido"));
    }

    #[test]
    fn commit_invalido_e_recusado_antes_de_chegar_ao_git() {
        let tmp = Tmp::new("orq-git", "commit-invalido");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a\n");
        commit_all(&dir, "a");
        for id in ["-x", "--output=../x", "--all", "HEAD", "main", "zz12", "ab", "", "HEAD~1"] {
            let why = commit_detail(&dir, id).unwrap_err();
            assert!(why.starts_with("Commit inválido") || why.starts_with("Commit não encontrado"), "{id}: {why}");
            assert!(commit_diff(&dir, id, None).is_err(), "{id} no diff");
        }
        assert!(commit_detail(&dir, "0123456789abcdef0123456789abcdef01234567").unwrap_err().contains("não encontrado"));
    }

    #[test]
    fn detalhe_de_commit_raiz_renomeado_e_apagado() {
        let tmp = Tmp::new("orq-git", "commit-detalhe");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "um\ndois\ntres\nquatro\ncinco\nseis\n");
        write(&dir, "apagar.txt", "x\n");
        commit_all(&dir, "raiz");
        let raiz = git(&dir, &["rev-parse", "HEAD"]).trim().to_string();

        let d = commit_detail(&dir, &raiz[..7]).unwrap();
        assert_eq!(d.commit.subject, "raiz");
        let raiz_arquivos: Vec<(&str, Kind)> = d.files.iter().map(|f| (f.path.as_str(), f.kind)).collect();
        assert_eq!(raiz_arquivos, [("a.txt", Kind::Added), ("apagar.txt", Kind::Added)]);
        assert!(commit_diff(&dir, &raiz, Some("a.txt")).unwrap().patch.contains("+seis"));

        git(&dir, &["mv", "a.txt", "novo nome [1].txt"]);
        fs::remove_file(dir.join("apagar.txt")).unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "renomeia e apaga"]);
        let troca = git(&dir, &["rev-parse", "HEAD"]).trim().to_string();

        let d = commit_detail(&dir, &troca).unwrap();
        assert_eq!(d.files.len(), 2, "{:?}", d.files);
        let renomeado = d.files.iter().find(|f| f.kind == Kind::Renamed).expect("renomeação");
        assert_eq!((renomeado.path.as_str(), renomeado.orig_path.as_deref()), ("novo nome [1].txt", Some("a.txt")));
        assert!(d.files.iter().any(|f| f.path == "apagar.txt" && f.kind == Kind::Deleted));

        let patch = commit_diff(&dir, &troca, Some("novo nome [1].txt")).unwrap().patch;
        assert!(patch.contains("rename from a.txt") && patch.contains("rename to novo nome [1].txt"), "{patch}");
        let apagado = commit_diff(&dir, &troca, Some("apagar.txt")).unwrap().patch;
        assert!(apagado.contains("-x"), "{apagado}");
        assert!(commit_diff(&dir, &troca, Some("nao-existe.txt")).unwrap().patch.is_empty());
        assert!(commit_diff(&dir, &troca, Some("../x")).is_err());
    }

    /// Repositório com uma base em `x.txt` e um merge que conflita nele (`main` x `outra`).
    fn conflito(tmp: &Tmp, name: &str, base: impl AsRef<[u8]>, ours: impl AsRef<[u8]>, theirs: impl AsRef<[u8]>) -> std::path::PathBuf {
        conflito_em(tmp, name, "x.txt", base, ours, theirs)
    }

    /// Igual a `conflito`, mas o arquivo em conflito é `rel` (pode estar numa pasta).
    fn conflito_em(tmp: &Tmp, name: &str, rel: &str, base: impl AsRef<[u8]>, ours: impl AsRef<[u8]>, theirs: impl AsRef<[u8]>) -> std::path::PathBuf {
        let dir = repo(tmp, name);
        write(&dir, rel, base);
        commit_all(&dir, "base");
        git(&dir, &["switch", "-q", "-c", "outra"]);
        write(&dir, rel, theirs);
        commit_all(&dir, "outra");
        git(&dir, &["switch", "-q", "main"]);
        write(&dir, rel, ours);
        commit_all(&dir, "main");
        let _ = proc::git_saida(&dir, &["merge", "-q", "outra"]);
        dir
    }

    /// Repositório em que `main` muda `x.txt` e `outra` apaga: o merge para num conflito de modificado de um lado e apagado do outro.
    fn modificado_e_apagado(tmp: &Tmp, name: &str) -> std::path::PathBuf {
        let dir = repo(tmp, name);
        write(&dir, "x.txt", "base\n");
        commit_all(&dir, "base");
        git(&dir, &["switch", "-q", "-c", "outra"]);
        git(&dir, &["rm", "-q", "x.txt"]);
        git(&dir, &["commit", "-q", "-m", "outra apaga"]);
        git(&dir, &["switch", "-q", "main"]);
        write(&dir, "x.txt", "main\n");
        commit_all(&dir, "main muda");
        let _ = proc::git_saida(&dir, &["merge", "-q", "outra"]);
        dir
    }

    #[test]
    fn conflito_mostra_os_tres_lados_e_grava_resolucao_protegida() {
        let tmp = Tmp::new("orq-git", "resolve-tres");
        let dir = conflito(&tmp, "r", "base\n", "main\n", "outra\n");
        let c = conflict(&dir, "x.txt").unwrap();
        assert_eq!((c.ours.as_deref(), c.theirs.as_deref()), (Some("main\n"), Some("outra\n")));
        assert!(c.worktree.as_deref().unwrap().contains("<<<<<<<"));
        assert!(!c.binary);
        assert!(conflict(&dir, "y.txt").unwrap_err().contains("não está em conflito"));

        // Gravar com revisão antiga é recusado; com marcadores e `mark_resolved` nem grava nem prepara.
        assert!(write_resolution(&dir, "x.txt", "novo\n", "obsoleta", false).unwrap_err().contains("mudou desde"));
        let marcado = c.worktree.clone().unwrap();
        let why = write_resolution(&dir, "x.txt", &marcado, &c.revision, true).unwrap_err();
        assert!(why.contains("marcadores"), "{why}");
        assert_eq!(conflict(&dir, "x.txt").unwrap().revision, c.revision, "nada foi gravado");

        write_resolution(&dir, "x.txt", "resolvido\n", &c.revision, true).unwrap();
        assert_eq!(fs::read_to_string(dir.join("x.txt")).unwrap(), "resolvido\n");
        let st = git_status_of(&dir);
        assert!(st.files.iter().all(|f| f.unstaged != Some(Kind::Conflicted)), "{st:?}");
        assert_eq!(find(&st, "x.txt").staged, Some(Kind::Modified));
    }

    #[test]
    fn rascunho_com_marcador_grava_sem_preparar() {
        let tmp = Tmp::new("orq-git", "rascunho");
        let dir = conflito(&tmp, "r", "base\n", "main\n", "outra\n");
        let c = conflict(&dir, "x.txt").unwrap();
        write_resolution(&dir, "x.txt", "parcial\n<<<<<<< rascunho\n", &c.revision, false).unwrap();
        let st = git_status_of(&dir);
        assert_eq!(find(&st, "x.txt").unstaged, Some(Kind::Conflicted), "continua em conflito: {st:?}");
    }

    #[test]
    fn gravacao_concorrente_nao_atropela_o_que_outro_escreveu() {
        let tmp = Tmp::new("orq-git", "concorrente");
        let dir = conflito(&tmp, "r", "base\n", "main\n", "outra\n");
        let lido = conflict(&dir, "x.txt").unwrap();
        // Outro processo grava no arquivo depois da leitura da UI.
        write(&dir, "x.txt", "de fora\n");
        let why = write_resolution(&dir, "x.txt", "minha\n", &lido.revision, true).unwrap_err();
        assert!(why.contains("mudou desde"), "{why}");
        assert_eq!(fs::read_to_string(dir.join("x.txt")).unwrap(), "de fora\n");
        assert_eq!(find(&git_status_of(&dir), "x.txt").unstaged, Some(Kind::Conflicted));
    }

    #[test]
    fn resolucao_preserva_bom_e_crlf() {
        let tmp = Tmp::new("orq-git", "bom-crlf");
        let mut base = files::BOM.to_vec();
        base.extend_from_slice(b"base\r\n");
        let mut ours = files::BOM.to_vec();
        ours.extend_from_slice(b"main\r\n");
        let mut theirs = files::BOM.to_vec();
        theirs.extend_from_slice(b"outra\r\n");
        let dir = conflito(&tmp, "r", base, ours, theirs);
        let c = conflict(&dir, "x.txt").unwrap();
        assert!(!c.worktree.as_deref().unwrap().contains('\r'));
        write_resolution(&dir, "x.txt", "um\ndois\n", &c.revision, true).unwrap();
        let bytes = fs::read(dir.join("x.txt")).unwrap();
        assert!(bytes.starts_with(&files::BOM), "o BOM continua");
        assert_eq!(String::from_utf8(bytes[3..].to_vec()).unwrap(), "um\r\ndois\r\n");
    }

    #[test]
    fn conflito_de_apagado_de_um_lado_aceita_remocao_ou_conteudo() {
        let tmp = Tmp::new("orq-git", "apagado");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", texto(5));
        commit_all(&dir, "base");
        git(&dir, &["switch", "-q", "-c", "outra"]);
        write(&dir, "a.txt", texto(5).replace("linha 2\n", "mudou\n"));
        commit_all(&dir, "outra");
        git(&dir, &["switch", "-q", "main"]);
        git(&dir, &["rm", "-q", "a.txt"]);
        git(&dir, &["commit", "-q", "-m", "main apaga"]);
        let _ = proc::git_saida(&dir, &["merge", "-q", "outra"]);

        let c = conflict(&dir, "a.txt").unwrap();
        assert_eq!(c.ours, None, "o lado que apagou não tem estágio 2");
        assert!(c.theirs.as_deref().unwrap().contains("mudou"));
        assert!(accept_deletion(&dir, "a.txt", "obsoleta").unwrap_err().contains("mudou desde"));
        accept_deletion(&dir, "a.txt", &c.revision).unwrap();
        assert!(!dir.join("a.txt").exists());
        assert!(conflict(&dir, "a.txt").is_err(), "o arquivo saiu do conflito");
    }

    #[test]
    fn estagio_trocado_com_a_pasta_igual_recusa_o_snapshot_antigo() {
        let tmp = Tmp::new("orq-git", "estagio");
        let dir = conflito(&tmp, "r", "base\n", "main\n", "outra\n");
        let lido = conflict(&dir, "x.txt").unwrap();
        let antes = fs::read(dir.join("x.txt")).unwrap();
        // Troca só o estágio 3 do índice (passa a ser o blob da base): a pasta fica igual, mas o conflito já não é o que a UI leu.
        let base = git(&dir, &["rev-parse", ":1:x.txt"]);
        proc::git_entrada(&dir, &["update-index", "--index-info"], format!("100644 {} 3\tx.txt\n", base.trim()).as_bytes()).unwrap();
        assert_eq!(fs::read(dir.join("x.txt")).unwrap(), antes, "a pasta não mudou");
        assert_ne!(conflict(&dir, "x.txt").unwrap().revision, lido.revision, "a revisão cobre os estágios");
        assert!(write_resolution(&dir, "x.txt", "novo\n", &lido.revision, true).unwrap_err().contains("mudou desde"));
        assert!(accept_deletion(&dir, "x.txt", &lido.revision).unwrap_err().contains("mudou desde"));
        assert_eq!(fs::read(dir.join("x.txt")).unwrap(), antes, "a recusa não gravou nada");
    }

    #[cfg(windows)]
    #[test]
    fn junction_no_caminho_do_conflito_e_recusada() {
        let tmp = Tmp::new("orq-git", "junction");
        let dir = conflito_em(&tmp, "r", "pasta/x.txt", "base\n", "main\n", "outra\n");
        let c = conflict(&dir, "pasta/x.txt").unwrap();
        // A pasta do projeto passa a ser uma junction para uma pasta fora dela, com um arquivo do mesmo nome.
        let fora = tmp.0.join("fora");
        fs::create_dir_all(&fora).unwrap();
        fs::write(fora.join("x.txt"), "de fora\n").unwrap();
        fs::rename(dir.join("pasta"), tmp.0.join("pasta-real")).unwrap();
        assert!(junction(&dir.join("pasta"), &fora), "mklink /J falhou");
        assert!(conflict(&dir, "pasta/x.txt").unwrap_err().contains("link"));
        assert!(write_resolution(&dir, "pasta/x.txt", "x\n", &c.revision, false).unwrap_err().contains("link"));
        assert!(accept_deletion(&dir, "pasta/x.txt", &c.revision).unwrap_err().contains("link"));
        assert_eq!(fs::read_to_string(fora.join("x.txt")).unwrap(), "de fora\n", "o destino da junction não foi tocado");
        // Tira só a junction (remove_dir não segue o destino).
        fs::remove_dir(dir.join("pasta")).unwrap();
    }

    #[test]
    fn lado_que_nao_e_utf8_recusa_o_conflito_inteiro() {
        let tmp = Tmp::new("orq-git", "nao-utf8-lado");
        let dir = conflito(&tmp, "r", "base\n", b"main\n\xFF\n", "outra\n");
        assert!(conflict(&dir, "x.txt").unwrap_err().contains("não é UTF-8"));
    }

    #[test]
    fn pasta_que_nao_e_utf8_vira_so_aviso_e_nao_e_gravada() {
        let tmp = Tmp::new("orq-git", "nao-utf8-pasta");
        let dir = conflito(&tmp, "r", "base\n", "main\n", "outra\n");
        write(&dir, "x.txt", b"ok \xFF fim\n");
        let c = conflict(&dir, "x.txt").unwrap();
        assert!(c.binary && c.worktree.is_none(), "{c:?}");
        assert!(write_resolution(&dir, "x.txt", "novo\n", &c.revision, true).unwrap_err().contains("UTF-8"));
        assert_eq!(fs::read(dir.join("x.txt")).unwrap(), b"ok \xFF fim\n", "nada foi gravado");
        assert!(git_status_of(&dir).files.iter().any(|f| f.path == "x.txt" && f.unstaged == Some(Kind::Conflicted)));
    }

    #[test]
    fn conflito_grande_demais_e_recusado_sem_ler_tudo() {
        let tmp = Tmp::new("orq-git", "grande");
        let grande = "a\n".repeat(600_000);
        let dir = conflito(&tmp, "r", "base\n", "main\n", "outra\n");
        write(&dir, "x.txt", grande.as_str());
        assert!(conflict(&dir, "x.txt").unwrap_err().contains("grande demais"));
        let dir = conflito(&tmp, "lado", "base\n", grande.as_str(), "outra\n");
        assert!(conflict(&dir, "x.txt").unwrap_err().contains("grande demais"));
    }

    #[test]
    fn remocao_recusada_quando_os_dois_lados_existem() {
        let tmp = Tmp::new("orq-git", "dois-lados");
        let dir = conflito(&tmp, "r", "base\n", "main\n", "outra\n");
        let c = conflict(&dir, "x.txt").unwrap();
        assert!(accept_deletion(&dir, "x.txt", &c.revision).unwrap_err().contains("Os dois lados"));
        assert!(dir.join("x.txt").is_file(), "o arquivo continua na pasta");
        assert_eq!(conflict(&dir, "x.txt").unwrap().revision, c.revision, "nada mudou");
    }

    #[test]
    fn ausencia_validada_cria_o_arquivo_sem_sobrescrever_quem_apareceu_antes() {
        let tmp = Tmp::new("orq-git", "ausente");
        let dir = modificado_e_apagado(&tmp, "r");
        // A pasta não tem o arquivo: o usuário o tirou com o conflito aberto.
        fs::remove_file(dir.join("x.txt")).unwrap();
        let c = conflict(&dir, "x.txt").unwrap();
        assert!(c.worktree.is_none() && !c.binary && c.theirs.is_none() && c.ours.is_some(), "{c:?}");

        // Outro programa cria o arquivo antes da gravação: a revisão lida já não vale, e o arquivo dele fica.
        write(&dir, "x.txt", "de fora\n");
        assert!(write_resolution(&dir, "x.txt", "minha\n", &c.revision, true).unwrap_err().contains("mudou desde"));
        assert_eq!(fs::read_to_string(dir.join("x.txt")).unwrap(), "de fora\n");

        // Com a revisão de agora, a ausência é válida: o arquivo nasce com o texto e vai para o índice.
        fs::remove_file(dir.join("x.txt")).unwrap();
        let c = conflict(&dir, "x.txt").unwrap();
        write_resolution(&dir, "x.txt", "resolvido\n", &c.revision, true).unwrap();
        assert_eq!(fs::read_to_string(dir.join("x.txt")).unwrap(), "resolvido\n");
        assert_eq!(find(&git_status_of(&dir), "x.txt").staged, Some(Kind::Modified));
    }

    #[test]
    fn ausencia_validada_aceita_a_remocao_direto_no_git() {
        let tmp = Tmp::new("orq-git", "ausente-remove");
        let dir = modificado_e_apagado(&tmp, "r");
        fs::remove_file(dir.join("x.txt")).unwrap();
        let c = conflict(&dir, "x.txt").unwrap();
        accept_deletion(&dir, "x.txt", &c.revision).unwrap();
        assert_eq!(find(&git_status_of(&dir), "x.txt").staged, Some(Kind::Deleted));
        assert!(!dir.join("x.txt").exists());
    }

    #[test]
    fn falha_do_git_na_remocao_e_dita_com_o_que_ficou_feito() {
        let tmp = Tmp::new("orq-git", "falha-remocao");
        let dir = modificado_e_apagado(&tmp, "r");
        // Sem arquivo na pasta, nada sai do disco: o erro diz que nada foi alterado.
        fs::remove_file(dir.join("x.txt")).unwrap();
        let c = conflict(&dir, "x.txt").unwrap();
        fs::write(dir.join(".git").join("index.lock"), "").unwrap();
        let why = accept_deletion(&dir, "x.txt", &c.revision).unwrap_err();
        assert!(why.contains("não registrou a remoção") && why.contains("Nada foi alterado"), "{why}");
        fs::remove_file(dir.join(".git").join("index.lock")).unwrap();
        assert_eq!(conflict(&dir, "x.txt").unwrap().revision, c.revision, "o conflito segue como estava");

        // Com o arquivo na pasta, ele vai para a Lixeira antes; se o Git falha, a mensagem diz isso.
        write(&dir, "x.txt", "main\n");
        let c = conflict(&dir, "x.txt").unwrap();
        fs::write(dir.join(".git").join("index.lock"), "").unwrap();
        let why = accept_deletion(&dir, "x.txt", &c.revision).unwrap_err();
        assert!(why.contains("foi para a Lixeira") && why.contains("não registrou a remoção"), "{why}");
        fs::remove_file(dir.join(".git").join("index.lock")).unwrap();
    }

    #[test]
    fn caminho_de_pasta_nao_vira_patch_de_varios_arquivos() {
        let tmp = Tmp::new("orq-git", "pasta-patch");
        let dir = repo(&tmp, "r");
        write(&dir, "pasta/um.txt", texto(3));
        write(&dir, "pasta/dois.txt", texto(3));
        commit_all(&dir, "base");
        write(&dir, "pasta/um.txt", "mudou um\n");
        write(&dir, "pasta/dois.txt", "mudou dois\n");
        assert!(diff(&dir, "pasta", false).unwrap_err().contains("um arquivo por vez"));
        assert!(review_diff(&dir, "pasta", false).is_err());
        let hunk = HunkRef { path: "pasta".into(), index: 0, header: String::new(), revision: String::new() };
        assert!(stage_hunk(&dir, &hunk).unwrap_err().contains("é uma pasta"));
        assert!(git(&dir, &["diff", "--cached", "--name-only"]).trim().is_empty());
        assert_eq!(quantos_blocos(&dir, "pasta/um.txt", false), 1, "o arquivo sozinho segue normal");
    }

    #[test]
    fn modo_de_pasta_sem_repositorio_com_repositorio_e_com_repositorio_acima() {
        let tmp = Tmp::new("orq-git", "modo");
        let fora = tmp.0.join("fora");
        fs::create_dir_all(&fora).unwrap();
        assert_eq!(modo(&fora).unwrap(), Modo::NoRepo);
        let dir = repo(&tmp, "r");
        assert_eq!(modo(&dir).unwrap(), Modo::Repo);
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        match modo(&sub).unwrap() {
            Modo::RepoAbove(root) => assert!(proc::mesma_pasta(Path::new(&root), &dir), "raiz {root}"),
            outro => panic!("subpasta deveria ser repo-above: {outro:?}"),
        }
    }

    #[test]
    fn git_init_cria_o_repositorio_e_a_pasta_vira_repo() {
        let tmp = Tmp::new("orq-git", "init");
        let dir = tmp.0.join("novo");
        fs::create_dir_all(&dir).unwrap();
        write(&dir, "a.txt", "a\n");
        init(&dir).unwrap();
        assert_eq!(modo(&dir).unwrap(), Modo::Repo);
        // Sem commit e sem stage: o arquivo só aparece como não rastreado.
        assert!(proc::git_saida(&dir, &["rev-parse", "-q", "--verify", "HEAD"]).is_err());
        assert!(git(&dir, &["diff", "--cached", "--name-only"]).trim().is_empty());
        assert_eq!(status(&dir).unwrap().files, vec![GitFile { path: "a.txt".into(), orig_path: None, staged: None, unstaged: Some(Kind::Untracked) }]);
        // Repositório já criado: recusa sem rodar nada.
        assert_eq!(init(&dir).unwrap_err(), "A pasta já é um repositório Git.");
    }

    #[test]
    fn git_init_dentro_de_repositorio_acima_e_recusado() {
        let tmp = Tmp::new("orq-git", "init-acima");
        let dir = repo(&tmp, "r");
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        let erro = init(&sub).unwrap_err();
        assert!(erro.starts_with("A pasta está dentro do repositório ") && erro.ends_with("; não dá para criar outro aqui."), "{erro}");
        assert!(!sub.join(".git").exists(), "nada foi criado dentro da subpasta");
    }

    #[test]
    fn git_que_recusa_com_git_presente_da_erro_e_nao_oferece_init() {
        let tmp = Tmp::new("orq-git", "recusa");
        let dir = tmp.0.join("projeto");
        fs::create_dir_all(dir.join("sub")).unwrap();
        // Um `.git` de arquivo que aponta para lugar nenhum: o Git recusa, mas o repositório existe.
        fs::write(dir.join(".git"), "gitdir: nao-existe-aqui\n").unwrap();
        assert!(modo(&dir).is_err());
        assert!(modo(&dir.join("sub")).is_err());
        assert!(estado(&dir).is_err());
        assert!(init(&dir).is_err());
        assert!(!dir.join("sub").join(".git").exists());
    }

    #[test]
    fn estado_tem_o_formato_que_a_tela_recebe() {
        let tmp = Tmp::new("orq-git", "estado");
        let fora = tmp.0.join("fora");
        fs::create_dir_all(&fora).unwrap();
        assert_eq!(serde_json::to_value(estado(&fora).unwrap()).unwrap(), serde_json::json!({ "kind": "no-repo" }));
        assert_eq!(serde_json::to_value(Estado::NoGit).unwrap(), serde_json::json!({ "kind": "no-git" }));
        assert_eq!(serde_json::to_value(Estado::RepoAbove { root: "D:\\raiz".into() }).unwrap(), serde_json::json!({ "kind": "repo-above", "root": "D:\\raiz" }));
        let dir = repo(&tmp, "r");
        let json = serde_json::to_value(estado(&dir).unwrap()).unwrap();
        assert_eq!(json["kind"], "repo");
        assert_eq!(json["status"]["branch"], "main");
    }

    fn base(head: Option<&str>, index: Option<&str>) -> Option<FileBase> {
        Some(FileBase { head: head.map(String::from), index: index.map(String::from) })
    }

    #[test]
    fn texto_base_de_modificado_preparado_e_novo_no_indice() {
        let tmp = Tmp::new("orq-git", "base");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "um\n");
        write(&dir, "src/b.txt", "dois\n");
        commit_all(&dir, "base");

        write(&dir, "a.txt", "um mudado\n");
        assert_eq!(file_base(&dir, "a.txt").unwrap(), base(Some("um\n"), Some("um\n")), "modificado só na pasta");

        write(&dir, "src/b.txt", "dois preparado\n");
        git(&dir, &["add", "src/b.txt"]);
        write(&dir, "src/b.txt", "dois na pasta\n");
        assert_eq!(file_base(&dir, "src/b.txt").unwrap(), base(Some("dois\n"), Some("dois preparado\n")), "mudança no índice");

        write(&dir, "novo.txt", "novo\n");
        git(&dir, &["add", "novo.txt"]);
        assert_eq!(file_base(&dir, "novo.txt").unwrap(), base(None, Some("novo\n")), "novo só no índice");

        git(&dir, &["rm", "-q", "--cached", "a.txt"]);
        assert_eq!(file_base(&dir, "a.txt").unwrap(), base(Some("um\n"), None), "tirado do índice com rm --cached");
    }

    #[test]
    fn texto_base_sem_commit_e_em_conflito() {
        let tmp = Tmp::new("orq-git", "base-sem-commit");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "um\n");
        git(&dir, &["add", "a.txt"]);
        assert_eq!(file_base(&dir, "a.txt").unwrap(), base(None, Some("um\n")));

        let dir = conflito(&tmp, "c", "base\n", "nosso\n", "deles\n");
        assert_eq!(file_base(&dir, "x.txt").unwrap(), base(Some("nosso\n"), None), "em conflito não há estágio 0");
    }

    #[test]
    fn texto_base_vazio_para_nao_rastreado_pasta_binario_e_grande() {
        let tmp = Tmp::new("orq-git", "base-vazio");
        let dir = repo(&tmp, "r");
        write(&dir, "src/a.txt", "um\n");
        write(&dir, "dado.bin", [0u8, 1, 2, 0, 255]);
        write(&dir, "grande.txt", "a\n".repeat(600_000));
        commit_all(&dir, "base");
        write(&dir, "solto.txt", "solto\n");

        assert_eq!(file_base(&dir, "solto.txt").unwrap(), None, "não rastreado");
        assert_eq!(file_base(&dir, "nao-existe.txt").unwrap(), None, "não existe");
        assert_eq!(file_base(&dir, "src").unwrap(), None, "pasta");
        assert_eq!(file_base(&dir, "dado.bin").unwrap(), None, "binário");
        assert_eq!(file_base(&dir, "grande.txt").unwrap(), None, "maior que MAX_FILE");

        write(&dir, "src/a.txt", [0u8, 9]);
        git(&dir, &["add", "src/a.txt"]);
        assert_eq!(file_base(&dir, "src/a.txt").unwrap(), None, "o índice binário anula o HEAD de texto");

        assert!(file_base(&dir, "../fora.txt").is_err());
        assert!(file_base(&dir, ".git/config").is_err());
    }

    #[test]
    fn texto_base_vazio_fora_do_modo_repositorio() {
        let tmp = Tmp::new("orq-git", "base-modo");
        let fora = tmp.0.join("fora");
        write(&fora, "a.txt", "um\n");
        assert_eq!(file_base(&fora, "a.txt").unwrap(), None, "sem repositório");

        let dir = repo(&tmp, "r");
        write(&dir, "sub/a.txt", "um\n");
        commit_all(&dir, "base");
        assert_eq!(file_base(&dir.join("sub"), "a.txt").unwrap(), None, "repositório acima");
    }

    #[test]
    fn texto_base_segue_o_crlf_do_checkout_e_tira_o_bom() {
        let tmp = Tmp::new("orq-git", "base-crlf");
        let dir = repo(&tmp, "r");
        git(&dir, &["config", "core.autocrlf", "true"]);
        write(&dir, "a.txt", "\u{feff}um\r\ndois\r\n");
        commit_all(&dir, "base");
        assert_eq!(git(&dir, &["cat-file", "blob", "HEAD:a.txt"]), "\u{feff}um\ndois\n", "o commit guardou LF");

        let pasta = files::read(&dir, "a.txt").unwrap().text;
        assert_eq!(pasta, "um\r\ndois\r\n");
        assert_eq!(file_base(&dir, "a.txt").unwrap(), base(Some(&pasta), Some(&pasta)));
    }

    /// O texto do índice como o editor o vê: `file_base` com `\n`. É o `expected` de `write_index`.
    fn visto(dir: &Path, rel: &str) -> String {
        so_lf(&file_base(dir, rel).unwrap().unwrap().index.unwrap())
    }

    /// Os bytes do blob de `rel` no índice, sem filtro.
    fn blob(dir: &Path, rel: &str) -> Vec<u8> {
        proc::git_bytes(dir, &["cat-file", "blob", &format!(":{rel}")]).unwrap()
    }

    #[test]
    fn grava_no_indice_so_o_trecho_preparado() {
        let tmp = Tmp::new("orq-git", "indice-parte");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", texto(8));
        commit_all(&dir, "base");
        let pasta = texto(8).replace("linha 2\n", "dois\n").replace("linha 7\n", "sete\n");
        write(&dir, "a.txt", &pasta);

        write_index(&dir, "a.txt", &texto(8).replace("linha 2\n", "dois\n"), &visto(&dir, "a.txt")).unwrap();
        let preparado = git(&dir, &["diff", "--cached", "--", "a.txt"]);
        assert!(preparado.contains("+dois") && !preparado.contains("sete"), "{preparado}");
        let solto = git(&dir, &["diff", "--", "a.txt"]);
        assert!(solto.contains("+sete") && !solto.contains("dois"), "{solto}");
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), pasta, "a pasta de trabalho não muda");
        assert_eq!(visto(&dir, "a.txt"), texto(8).replace("linha 2\n", "dois\n"));
    }

    #[test]
    fn texto_igual_ao_head_tira_tudo_do_preparo() {
        let tmp = Tmp::new("orq-git", "indice-tira");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", texto(4));
        commit_all(&dir, "base");
        let pasta = texto(4).replace("linha 1\n", "um\n").replace("linha 4\n", "quatro\n");
        write(&dir, "a.txt", &pasta);
        git(&dir, &["add", "a.txt"]);

        write_index(&dir, "a.txt", &texto(4), &visto(&dir, "a.txt")).unwrap();
        assert_eq!(git(&dir, &["diff", "--cached", "--name-only"]), "", "nada preparado");
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), pasta, "a pasta de trabalho não muda");
    }

    #[test]
    fn indice_mudado_desde_a_leitura_recusa_sem_gravar() {
        let tmp = Tmp::new("orq-git", "indice-velho");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", texto(3));
        commit_all(&dir, "base");
        let antes = git(&dir, &["ls-files", "-s"]);
        let why = write_index(&dir, "a.txt", "outro\n", &texto(2)).unwrap_err();
        assert_eq!(why, "O índice de a.txt mudou desde que o editor o leu. Tente de novo.");
        assert_eq!(git(&dir, &["ls-files", "-s"]), antes, "o índice fica igual");
    }

    #[test]
    fn crlf_guardado_no_blob_continua_em_todas_as_linhas() {
        let tmp = Tmp::new("orq-git", "indice-crlf");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "um\r\ndois\r\ntres\r\n");
        commit_all(&dir, "base");
        assert_eq!(visto(&dir, "a.txt"), "um\ndois\ntres\n");

        write_index(&dir, "a.txt", "um\nDOIS\ntres\n", &visto(&dir, "a.txt")).unwrap();
        assert_eq!(blob(&dir, "a.txt"), b"um\r\nDOIS\r\ntres\r\n");
        assert_eq!(git(&dir, &["diff", "--cached", "--numstat"]), "1\t1\ta.txt\n", "só a linha mudada");

        // Como no `git add`, a conversão automática não mexe no blob que já tem CRLF: nem `core.autocrlf=true`...
        git(&dir, &["config", "core.autocrlf", "true"]);
        write_index(&dir, "a.txt", "um\nDOIS\nTRES\n", &visto(&dir, "a.txt")).unwrap();
        assert_eq!(blob(&dir, "a.txt"), b"um\r\nDOIS\r\nTRES\r\n");
        // ...nem `text=auto`.
        write(&dir, ".gitattributes", "* text=auto\n");
        write_index(&dir, "a.txt", "UM\nDOIS\nTRES\n", &visto(&dir, "a.txt")).unwrap();
        assert_eq!(blob(&dir, "a.txt"), b"UM\r\nDOIS\r\nTRES\r\n");
        assert_eq!(git(&dir, &["diff", "--cached", "--numstat", "--", "a.txt"]), "3\t3\ta.txt\n", "só as três linhas mudadas");
    }

    #[test]
    fn fins_de_linha_mistos_recusam_sem_converter_o_arquivo() {
        let tmp = Tmp::new("orq-git", "indice-misto");
        let dir = repo(&tmp, "r");
        write(&dir, "a.txt", "a\nb\r\nc\n");
        commit_all(&dir, "base");
        let antes = git(&dir, &["ls-files", "-s"]);
        let why = write_index(&dir, "a.txt", "A\nb\nc\n", &visto(&dir, "a.txt")).unwrap_err();
        assert!(why.contains("mistura CRLF e LF"), "{why}");
        assert_eq!(git(&dir, &["ls-files", "-s"]), antes, "o índice fica igual");

        let tmp = Tmp::new("orq-git", "conflito-misto");
        let dir = conflito(&tmp, "r", b"base\n".to_vec(), b"main\r\nx\n".to_vec(), b"outra\n".to_vec());
        let c = conflict(&dir, "x.txt").unwrap();
        let antes = fs::read(dir.join("x.txt")).unwrap();
        let why = write_resolution(&dir, "x.txt", "um\ndois\n", &c.revision, true).unwrap_err();
        assert!(why.contains("mistura CRLF e LF"), "{why}");
        assert_eq!(fs::read(dir.join("x.txt")).unwrap(), antes, "a pasta fica igual");
    }

    #[test]
    fn autocrlf_com_blob_em_lf_grava_em_lf() {
        let tmp = Tmp::new("orq-git", "indice-autocrlf");
        let dir = repo(&tmp, "r");
        git(&dir, &["config", "core.autocrlf", "true"]);
        write(&dir, "a.txt", "um\r\ndois\r\ntres\r\n");
        commit_all(&dir, "base");
        assert_eq!(blob(&dir, "a.txt"), b"um\ndois\ntres\n", "o commit guardou LF");

        write_index(&dir, "a.txt", "um\nDOIS\ntres\n", &visto(&dir, "a.txt")).unwrap();
        assert_eq!(blob(&dir, "a.txt"), b"um\nDOIS\ntres\n");
        assert_eq!(git(&dir, &["diff", "--cached", "--numstat"]), "1\t1\ta.txt\n", "só a linha mudada");
    }

    #[test]
    fn bom_do_blob_e_modo_executavel_continuam() {
        let tmp = Tmp::new("orq-git", "indice-bom");
        let dir = repo(&tmp, "r");
        write(&dir, "a.sh", "\u{feff}um\ndois\n");
        git(&dir, &["add", "a.sh"]);
        git(&dir, &["update-index", "--chmod=+x", "a.sh"]);
        git(&dir, &["commit", "-q", "-m", "base"]);
        assert_eq!(visto(&dir, "a.sh"), "um\ndois\n", "o editor não vê o BOM");

        write_index(&dir, "a.sh", "um\nDOIS\n", &visto(&dir, "a.sh")).unwrap();
        assert_eq!(blob(&dir, "a.sh"), "\u{feff}um\nDOIS\n".as_bytes());
        assert!(git(&dir, &["ls-files", "-s", "a.sh"]).starts_with("100755 "), "o modo executável fica");
        assert_eq!(git(&dir, &["diff", "--cached", "--numstat"]), "1\t1\ta.sh\n");
    }

    #[test]
    fn gravar_no_indice_recusa_fora_do_estagio_zero_e_do_projeto() {
        let tmp = Tmp::new("orq-git", "indice-recusa");
        let dir = repo(&tmp, "r");
        write(&dir, "src/a.txt", "um\n");
        write(&dir, "dado.bin", [0u8, 1, 2, 0, 255]);
        write(&dir, "tirado.txt", "t\n");
        commit_all(&dir, "base");
        write(&dir, "solto.txt", "solto\n");
        git(&dir, &["rm", "-q", "--cached", "tirado.txt"]);
        let antes = git(&dir, &["ls-files", "-s"]);

        assert!(write_index(&dir, "solto.txt", "x\n", "solto\n").unwrap_err().contains("não está no índice"), "não rastreado");
        assert!(write_index(&dir, "tirado.txt", "x\n", "t\n").unwrap_err().contains("não está no índice"), "removido do índice");
        assert!(write_index(&dir, "src", "x\n", "").unwrap_err().contains("pasta"));
        assert!(write_index(&dir, "dado.bin", "x\n", "").unwrap_err().contains("binário"));
        assert!(write_index(&dir, "../x", "x\n", "").is_err());
        assert!(write_index(&dir, ".git/config", "x\n", "").is_err());
        assert_eq!(git(&dir, &["ls-files", "-s"]), antes, "nada foi gravado");

        let dir = conflito(&tmp, "c", "base\n", "nosso\n", "deles\n");
        assert!(write_index(&dir, "x.txt", "x\n", "nosso\n").unwrap_err().contains("não está no índice"), "em conflito");
    }
}
