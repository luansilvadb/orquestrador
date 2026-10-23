//! Pesquisa, abertura rápida e substituição de texto nos arquivos do projeto. Os candidatos são os do `git ls-files`
//! (rastreados e não ignorados, sem `.git`), ou os da pasta quando não há repositório.
//! Busca, prévia e aplicação usam o mesmo motor (`regex`) e a mesma regra:
//! o padrão roda linha a linha, sem a quebra de linha, e o CR do fim não conta. Binário, não UTF-8, grande, o que não é
//! arquivo comum (link, junção, submódulo) e arquivo cujo fim de linha a gravação não preservaria ficam de fora, e
//! a resposta diz quais e por quê.
use crate::files;
use crate::git;
use crate::proc;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Mais linhas casadas que isto não voltam: a resposta marca `truncated` e para de procurar.
const MAX_LINES: usize = 2000;
/// Tamanho máximo do texto de uma linha, em caracteres.
const MAX_TEXT: usize = 400;
/// Quantos caracteres antes da ocorrência a janela começa, quando a linha é mais longa que a janela.
const CONTEXT_BEFORE: usize = 40;
/// Arquivos com trechos na prévia; os demais aparecem só na contagem.
const MAX_PREVIEW_FILES: usize = 100;
/// Texto (original mais resultado) que a prévia traz de uma vez; passou disso, o arquivo fica só na contagem.
const MAX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
/// Quantos arquivos ignorados entram na lista; o resto vira só a contagem `skippedOmitted`.
const MAX_SKIPPED: usize = 500;
/// Quantos caminhos a abertura rápida devolve no máximo.
const MAX_QUICK: usize = 100;

/// A consulta. `text` é o texto (literal) ou o padrão (`regex`); `include` e `exclude` são trechos de caminho, em que `*`
/// vale qualquer sequência e `?` um caractere, sem diferenciar maiúsculas. Lista de `include` vazia quer dizer todos.
/// Trecho que começa com `./` vale a partir da raiz do projeto (`./src` é a pasta `src`, não `outro/src`); sem `./`, o
/// trecho casa em qualquer ponto do caminho. `only`, quando vem, limita a busca a estes caminhos exatos (os editores
/// abertos); lista vazia não deixa nada.
/// `preserve_case` só vale para a substituição (prévia e aplicação): ajusta a caixa do substituto à ocorrência casada.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub only: Option<Vec<String>>,
    pub preserve_case: bool,
}

/// Por que um arquivo candidato ficou de fora. A busca lista o caminho com esta razão.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// Tem NUL nos primeiros bytes: não é texto.
    Binary,
    /// Não é UTF-8 válido (depois do BOM).
    NotUtf8,
    /// Passa de 1 MiB.
    TooLarge,
    /// Não é arquivo comum: link, junção, submódulo ou pasta.
    NotFile,
    /// Não existe mais na pasta ou não pôde ser lido.
    Unreadable,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    /// Relativo à raiz do projeto, com "/".
    pub path: String,
    pub reason: SkipReason,
}

/// O resultado da busca. `files` vem em ordem alfabética por caminho; `truncated` avisa que passou de 2000 linhas casadas.
/// `skipped` lista os arquivos que não entraram na busca (até 500; `skippedOmitted` conta o resto).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    pub files: Vec<FileMatches>,
    pub truncated: bool,
    pub skipped: Vec<Skipped>,
    pub skipped_omitted: usize,
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

/// Uma linha que casa. `line` começa em 1; `col` é a coluna da primeira ocorrência na linha original, em unidades UTF-16
/// (como o CodeMirror) e a partir de 1.
/// `text` é a linha sem CR no fim; linha longa vem como janela que contém a ocorrência.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub line: u32,
    pub col: u32,
    pub text: String,
}

/// Abertura rápida: caminhos que casam com os termos (todos, sem diferenciar maiúsculas), o total que casou e até 100 deles.
/// Nome do arquivo primeiro, depois o caminho mais curto.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quick {
    pub paths: Vec<String>,
    pub total: usize,
}

/// Uma prévia de substituição. Nada é gravado. `files` traz o texto original e o resultado de até 100 arquivos (e 8 MiB);
/// `omittedFiles` conta os que têm ocorrência e não entraram. Quem aplica só pode escolher entre `files`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub files: Vec<PreviewFile>,
    pub omitted_files: usize,
}

/// Um arquivo da prévia. `revision` é a impressão digital dos bytes lidos: a aplicação só grava se ela ainda for a mesma.
/// `original` e `result` vão sem BOM, com os mesmos fins de linha do arquivo.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFile {
    pub path: String,
    pub revision: String,
    pub count: usize,
    pub original: String,
    pub result: String,
}

/// Um arquivo escolhido da prévia, com a revisão que a prévia mostrou.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    pub path: String,
    pub revision: String,
}

/// O pedido de aplicação: a mesma consulta e substituição da prévia, e os arquivos escolhidos. Só esses são tocados.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    pub query: Query,
    pub replacement: String,
    pub files: Vec<Pick>,
}

/// O que a aplicação fez: cada arquivo gravado com a contagem, e cada um recusado com o motivo. Um erro não para os outros.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub applied: Vec<Done>,
    pub errors: Vec<Failed>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Done {
    pub path: String,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Failed {
    pub path: String,
    pub error: String,
}

/// Um arquivo lido como texto: sem BOM, com a revisão dos bytes e se tinha BOM.
struct Doc {
    bom: bool,
    revision: String,
    text: String,
}

/// Busca `query.text` nos arquivos do projeto. Consulta vazia ou só de espaços não roda e devolve vazio. Padrão inválido
/// ou quebra de linha na consulta são recusados com mensagem.
pub fn search(dir: &Path, query: &Query) -> Result<Results, String> {
    let mut out = Results { files: Vec::new(), truncated: false, skipped: Vec::new(), skipped_omitted: 0 };
    if query.text.trim().is_empty() {
        return Ok(out);
    }
    let re = compile(query)?;
    let filter = filters(query)?;
    let mut hits = 0usize;
    for path in candidates(dir, &filter)? {
        let doc = match load(dir, &path) {
            Ok(doc) => doc,
            Err(reason) => {
                skip(&mut out.skipped, &mut out.skipped_omitted, path, reason);
                continue;
            }
        };
        let mut matches = Vec::new();
        let mut stop = false;
        for (line, body) in lines(&doc.text) {
            let Some(at) = first_hit(&re, body) else { continue };
            if hits == MAX_LINES {
                out.truncated = true;
                stop = true;
                break;
            }
            hits += 1;
            let col = body[..at].encode_utf16().count() + 1;
            matches.push(Match { line, col: col as u32, text: window(body, at) });
        }
        if !matches.is_empty() {
            out.files.push(FileMatches { path, matches });
        }
        if stop {
            break;
        }
    }
    Ok(out)
}

/// Os caminhos (rastreados ou não ignorados, sem o que é só pasta ou link) que casam com `terms`. Sem termo, a lista
/// começa pelos mais curtos. Devolve até 100 e o total que casou.
pub fn quick_open(dir: &Path, term: &str) -> Result<Quick, String> {
    let words: Vec<String> = term.to_lowercase().split_whitespace().map(str::to_string).collect();
    let mut hits: Vec<String> = candidates(dir, &Filter::default())?
        .into_iter()
        .filter(|p| {
            let low = p.to_lowercase();
            words.iter().all(|w| low.contains(w))
        })
        .collect();
    let name_of = |p: &str| p.rsplit('/').next().unwrap_or(p).to_lowercase();
    hits.sort_by_key(|p| (!words.iter().all(|w| name_of(p).contains(w)), p.len(), p.clone()));
    // `plain` toca o disco, então só confere os que entram na lista. O total desconta os recusados entre os conferidos.
    let mut total = hits.len();
    let mut paths = Vec::new();
    for p in hits {
        if paths.len() == MAX_QUICK {
            break;
        }
        if files::plain(dir, &p, false).is_ok() { paths.push(p) } else { total -= 1 }
    }
    Ok(Quick { paths, total })
}

/// A prévia da substituição de `query` por `replacement`. Lê tudo e não grava nada. Consulta vazia devolve prévia vazia.
pub fn preview(dir: &Path, query: &Query, replacement: &str) -> Result<Preview, String> {
    let mut out = Preview { files: Vec::new(), omitted_files: 0 };
    if query.text.trim().is_empty() {
        return Ok(out);
    }
    let re = compile(query)?;
    let rep = template(query, &re, replacement)?;
    let filter = filters(query)?;
    let mut shown = 0usize;
    for path in candidates(dir, &filter)? {
        let Ok(doc) = load(dir, &path) else { continue };
        let (result, count) = replace_text(&re, &doc.text, &rep, query.preserve_case);
        if count == 0 {
            continue;
        }
        let size = doc.text.len() + result.len();
        if out.files.len() >= MAX_PREVIEW_FILES || shown + size > MAX_PREVIEW_BYTES {
            out.omitted_files += 1;
            continue;
        }
        shown += size;
        out.files.push(PreviewFile { path, revision: doc.revision, count, original: doc.text, result });
    }
    Ok(out)
}

/// Grava a substituição nos arquivos escolhidos. Cada um é relido e conferido: se a revisão mudou desde a prévia, se não
/// é mais texto, se não há mais ocorrência ou se não está entre os candidatos da pesquisa, ele é recusado e os outros
/// seguem. A gravação passa por `write_checked`, que recusa se o arquivo mudou entre a leitura e a escrita.
pub fn apply(dir: &Path, request: &ApplyRequest) -> Result<Applied, String> {
    let query = &request.query;
    if query.text.trim().is_empty() {
        return Err("Digite o texto a substituir.".into());
    }
    let re = compile(query)?;
    let rep = template(query, &re, &request.replacement)?;
    let listed = candidates(dir, &filters(query)?)?;
    let mut out = Applied { applied: Vec::new(), errors: Vec::new() };
    for pick in &request.files {
        match apply_one(dir, &listed, &re, &rep, query.preserve_case, pick) {
            Ok(count) => out.applied.push(Done { path: pick.path.clone(), count }),
            Err(error) => out.errors.push(Failed { path: pick.path.clone(), error }),
        }
    }
    Ok(out)
}

fn apply_one(dir: &Path, listed: &[String], re: &Regex, rep: &str, preserve: bool, pick: &Pick) -> Result<usize, String> {
    if listed.binary_search(&pick.path).is_err() {
        return Err("Este arquivo não está na pesquisa atual (filtro, ignorado ou apagado). Nada foi gravado.".into());
    }
    let doc = load(dir, &pick.path).map_err(|reason| format!("{} Nada foi gravado.", reason_text(reason)))?;
    if doc.revision != pick.revision {
        return Err("O arquivo mudou desde a prévia. Nada foi gravado; faça a prévia de novo.".into());
    }
    let (result, count) = replace_text(re, &doc.text, rep, preserve);
    if count == 0 {
        return Err("Não há mais o que substituir neste arquivo. Nada foi gravado.".into());
    }
    files::write_checked(dir, &pick.path, &result, &doc.text, doc.bom)?;
    Ok(count)
}

/// Monta o padrão da consulta. Literal escapa o texto; palavra inteira prende em `\b`. Sem diferença de caixa por padrão.
/// Consulta com quebra de linha ou que não compila é recusada com a razão em português.
fn compile(query: &Query) -> Result<Regex, String> {
    if query.text.contains(['\n', '\r', '\0']) {
        return Err("A consulta não pode ter quebra de linha.".into());
    }
    let source = if query.regex { query.text.clone() } else { regex::escape(&query.text) };
    let source = if query.whole_word { format!(r"\b(?:{source})\b") } else { source };
    let flags = if query.case_sensitive { "" } else { "(?i)" };
    Regex::new(&format!("{flags}{source}")).map_err(|e| format!("Expressão regular inválida: {}", regex_reason(&e)))
}

/// A linha de erro do `regex` (a última que começa com "error: "), para não mostrar o desenho do padrão.
fn regex_reason(e: &regex::Error) -> String {
    let text = e.to_string();
    text.lines().rev().find_map(|l| l.strip_prefix("error: ")).map(str::to_string).unwrap_or_else(|| text.lines().next().unwrap_or("").to_string())
}

/// Checa o texto de substituição e o devolve no formato que `replace_text` expande. Literal vira template com cada `$`
/// dobrado (`$$`), então sai como está. Regex aceita só `$$`, `$n` (grupo que existe), `$nome` (grupo nomeado que
/// existe) e `${...}` com um desses; qualquer outro `$` é recusado, nunca aplicado errado. Quebra de linha ou CR na
/// substituição também é recusada.
fn template(query: &Query, re: &Regex, replacement: &str) -> Result<String, String> {
    if replacement.contains(['\n', '\r', '\0']) {
        return Err("A substituição precisa ser de uma linha só.".into());
    }
    if !query.regex {
        return Ok(replacement.replace('$', "$$"));
    }
    let bytes = replacement.as_bytes();
    let unsupported = |what: &str| format!("Sintaxe de substituição não suportada ({what}). Use $1, $2 ou ${{nome}} para grupos, e $$ para um $ literal.");
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            i += 1;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'$') {
            i += 2;
            continue;
        }
        let (name, next) = if bytes.get(i + 1) == Some(&b'{') {
            match replacement[i + 2..].find('}') {
                Some(end) => (&replacement[i + 2..i + 2 + end], i + 3 + end),
                None => return Err(unsupported("$ sem }")),
            }
        } else {
            let len = bytes[i + 1..].iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'_').count();
            (&replacement[i + 1..i + 1 + len], i + 1 + len)
        };
        let well_formed = !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        let exists = match name.parse::<usize>() {
            Ok(n) => n < re.captures_len(),
            Err(_) => re.capture_names().flatten().any(|c| c == name),
        };
        if !well_formed || !exists {
            return Err(unsupported(&format!("${name}")));
        }
        i = next;
    }
    Ok(replacement.to_string())
}

/// Os filtros de caminho de uma consulta, já compilados. O padrão deixa passar tudo.
#[derive(Default)]
struct Filter {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
    only: Option<Vec<String>>,
}

/// Os filtros de caminho de `query`: cada trecho vira um padrão sem diferença de caixa; `only` vai com "/".
fn filters(query: &Query) -> Result<Filter, String> {
    let compile_all = |terms: &[String]| -> Result<Vec<Regex>, String> {
        terms.iter().map(|t| t.trim().replace('\\', "/")).filter(|t| !t.is_empty()).map(|t| glob(&t)).collect()
    };
    let only = query.only.as_ref().map(|paths| paths.iter().map(|p| p.replace('\\', "/")).collect());
    Ok(Filter { include: compile_all(&query.include)?, exclude: compile_all(&query.exclude)?, only })
}

/// Trecho de caminho com `*` e `?`; o resto é literal. Sem `./` na frente, acha o trecho em qualquer lugar do caminho;
/// com `*` ou `?`, o trecho vai até o fim de um nome: `*.js` não casa `package.json`.
/// Com `./`, o trecho começa na raiz e vai até o fim de um nome: `./src` casa `src` e o que está dentro, mas não
/// `src-tauri` nem `outro/src`. `./` sozinho (ou `.`) é o projeto inteiro.
fn glob(term: &str) -> Result<Regex, String> {
    // `escape` põe `\` antes de `*` e de `?`; trocar esses pares pelo equivalente do `regex` dá o glob.
    let body = |t: &str| regex::escape(t).replace(r"\*", ".*").replace(r"\?", ".");
    let rooted = if term == "." { Some("") } else { term.strip_prefix("./") };
    let pattern = match rooted.map(|rest| rest.trim_end_matches('/')) {
        Some("") => "^".to_string(),
        Some(rest) => format!("(?i)^{}(?:/|$)", body(rest)),
        None if term.contains(['*', '?']) => format!("(?i){}(?:/|$)", body(term)),
        None => format!("(?i){}", body(term)),
    };
    Regex::new(&pattern).map_err(|e| format!("Filtro de caminho inválido: {}", regex_reason(&e)))
}

/// Os caminhos que a busca enxerga, em ordem, depois dos filtros. Com repositório, são os do `git ls-files`
/// (rastreados e não ignorados); sem ele, a pasta percorrida (`caminhos_da_pasta`).
fn candidates(dir: &Path, filter: &Filter) -> Result<Vec<String>, String> {
    let mut paths = match git::modo(dir)? {
        git::Modo::Repo => {
            let out = proc::git_saida(dir, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"])?;
            out.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect()
        }
        _ => caminhos_da_pasta(dir)?,
    };
    if let Some(only) = &filter.only {
        paths.retain(|p| only.contains(p));
    }
    paths.retain(|p| filter.include.is_empty() || filter.include.iter().any(|r| r.is_match(p)));
    paths.retain(|p| !filter.exclude.iter().any(|r| r.is_match(p)));
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Os arquivos da pasta do projeto, como caminhos com "/" a partir dela, para quando não há repositório. Segue a
/// mesma regra do `git ls-files --exclude-standard`: respeita `.gitignore` e `.ignore` desta pasta e das subpastas
/// (mesmo sem repositório), mas não os de pastas acima, inclui arquivos ocultos, não entra em link nem em junção, e
/// nunca desce em `.git` nem em `node_modules`.
fn caminhos_da_pasta(dir: &Path) -> Result<Vec<String>, String> {
    let mut walk = ignore::WalkBuilder::new(dir);
    walk.hidden(false).parents(false).require_git(false).follow_links(false);
    walk.filter_entry(|e| !matches!(e.file_name().to_str(), Some(".git" | "node_modules")));
    // Só a raiz conta como erro: se ela não abre, a pasta não pode ser listada. Entrada ilegível é pulada.
    std::fs::read_dir(dir).map_err(|e| format!("Não foi possível ler a pasta do projeto: {e}"))?;
    let mut paths = Vec::new();
    for entry in walk.build() {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry.path().strip_prefix(dir).map_err(|e| e.to_string())?;
        let parts: Vec<&str> = rel.components().filter_map(|c| c.as_os_str().to_str()).collect();
        paths.push(parts.join("/"));
    }
    Ok(paths)
}

/// Lê `path` como texto para a busca ou a substituição. Diz o motivo quando não entra. Nada é gravado aqui.
fn load(dir: &Path, path: &str) -> Result<Doc, SkipReason> {
    let meta = std::fs::symlink_metadata(dir.join(path)).map_err(|_| SkipReason::Unreadable)?;
    if !meta.is_file() {
        return Err(SkipReason::NotFile);
    }
    files::plain(dir, path, false).map_err(|_| SkipReason::NotFile)?;
    if meta.len() > files::MAX_FILE {
        return Err(SkipReason::TooLarge);
    }
    let bytes = std::fs::read(dir.join(path)).map_err(|_| SkipReason::Unreadable)?;
    if files::is_binary(&bytes) {
        return Err(SkipReason::Binary);
    }
    let body = bytes.strip_prefix(&files::BOM).unwrap_or(&bytes[..]);
    let text = std::str::from_utf8(body).map_err(|_| SkipReason::NotUtf8)?.to_string();
    Ok(Doc { bom: body.len() < bytes.len(), revision: files::fingerprint(&bytes), text })
}

/// As linhas do texto como a busca as vê: número a partir de 1, sem CR no fim.
fn lines(text: &str) -> impl Iterator<Item = (u32, &str)> {
    text.split('\n').enumerate().map(|(i, line)| (i as u32 + 1, line.strip_suffix('\r').unwrap_or(line)))
}

/// Posição (em bytes) da primeira ocorrência não vazia na linha, ou `None`.
fn first_hit(re: &Regex, body: &str) -> Option<usize> {
    re.find_iter(body).find(|m| !m.is_empty()).map(|m| m.start())
}

/// Substitui em `text`, linha a linha, e conta as ocorrências trocadas. Ocorrência vazia não conta e não muda nada.
/// `rep` é o template de `template`, expandido em cada ocorrência. Os fins de linha de `text` ficam como estão.
/// Com `preserve`, a caixa do substituto segue a ocorrência (`keep_case`).
fn replace_text(re: &Regex, text: &str, rep: &str, preserve: bool) -> (String, usize) {
    let mut count = 0;
    let mut out = String::with_capacity(text.len());
    for piece in text.split_inclusive('\n') {
        let rest = piece.strip_suffix('\n').unwrap_or(piece);
        let body = rest.strip_suffix('\r').unwrap_or(rest);
        let replaced = re.replace_all(body, |caps: &Captures| {
            if caps[0].is_empty() {
                return String::new();
            }
            count += 1;
            let mut sub = String::new();
            caps.expand(rep, &mut sub);
            if preserve {
                keep_case(&caps[0], &sub)
            } else {
                sub
            }
        });
        out.push_str(&replaced);
        out.push_str(&piece[body.len()..]);
    }
    (out, count)
}

/// Ajusta a caixa do substituto `rep` à ocorrência `matched`, como o VS Code: toda maiúscula, toda minúscula, ou só a
/// primeira letra maiúscula (com o resto como foi digitado). Qualquer outro caso, ou ocorrência sem letra, fica como está.
fn keep_case(matched: &str, rep: &str) -> String {
    let letters: Vec<char> = matched.chars().filter(|c| c.is_alphabetic()).collect();
    let Some((first, rest)) = letters.split_first() else {
        return rep.to_string();
    };
    if letters.iter().all(|c| c.is_uppercase()) {
        rep.to_uppercase()
    } else if letters.iter().all(|c| c.is_lowercase()) {
        rep.to_lowercase()
    } else if first.is_uppercase() && rest.iter().all(|c| c.is_lowercase()) {
        let mut chars = rep.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => String::new(),
        }
    } else {
        rep.to_string()
    }
}

/// Registra um arquivo ignorado na lista da busca, até o limite; o resto só entra na contagem `omitted`.
fn skip(list: &mut Vec<Skipped>, omitted: &mut usize, path: String, reason: SkipReason) {
    if list.len() < MAX_SKIPPED {
        list.push(Skipped { path, reason });
    } else {
        *omitted += 1;
    }
}

/// Frase em português para o motivo, usada nas recusas da aplicação.
fn reason_text(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::Binary => "É um arquivo binário.",
        SkipReason::NotUtf8 => "Não é um arquivo UTF-8.",
        SkipReason::TooLarge => "O arquivo passa de 1 MiB.",
        SkipReason::NotFile => "Não é um arquivo comum (link, junção ou pasta).",
        SkipReason::Unreadable => "Não foi possível ler o arquivo.",
    }
}

/// A janela de até 400 caracteres de `line` que contém a ocorrência em `at` (em bytes). Começa uns 40 caracteres antes.
fn window(line: &str, at: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= MAX_TEXT {
        return line.to_string();
    }
    let hit = line[..at].chars().count();
    let start = hit.saturating_sub(CONTEXT_BEFORE);
    chars[start..(start + MAX_TEXT).min(chars.len())].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use crate::testutil::{commit_all, git, identity, junction, write, Tmp};

    /// Pasta temporária do teste, já um repositório Git com a identidade fixa. Some quando o teste acaba.
    fn repo_tmp(tag: &str) -> Tmp {
        let tmp = Tmp::new("orq-search", tag);
        git(&tmp.0, &["init", "-q"]);
        identity(&tmp.0);
        tmp
    }

    fn read(dir: &Path, rel: &str) -> Vec<u8> {
        fs::read(dir.join(rel)).unwrap()
    }

    fn q(text: &str) -> Query {
        Query { text: text.into(), ..Query::default() }
    }

    fn buscar(dir: &Path, query: &Query) -> Results {
        search(dir, query).unwrap()
    }

    fn linhas(r: &Results, path: &str) -> Vec<(u32, String)> {
        r.files.iter().find(|f| f.path == path).map(|f| f.matches.iter().map(|m| (m.line, m.text.clone())).collect()).unwrap_or_default()
    }

    fn caminhos(r: &Results) -> Vec<&str> {
        r.files.iter().map(|f| f.path.as_str()).collect()
    }

    /// Pick para aplicar, com a revisão que a prévia deu ao arquivo.
    fn pick(prev: &Preview, path: &str) -> Pick {
        let f = prev.files.iter().find(|f| f.path == path).unwrap();
        Pick { path: path.into(), revision: f.revision.clone() }
    }

    #[test]
    fn literal_respeita_ou_nao_a_caixa() {
        let t = repo_tmp("caixa");
        write(&t.0, "a.txt", "Olá mundo\nolá de novo\n");
        commit_all(&t.0, "base");
        assert_eq!(linhas(&buscar(&t.0, &q("olá")), "a.txt").len(), 2);
        let exato = Query { case_sensitive: true, ..q("olá") };
        assert_eq!(linhas(&buscar(&t.0, &exato), "a.txt"), vec![(2, "olá de novo".to_string())]);
    }

    #[test]
    fn palavra_inteira_nao_pega_pedaco_e_regex_com_captura_funciona() {
        let t = repo_tmp("palavra");
        write(&t.0, "a.txt", "gato\ngatinho\nx.y\nxzy\n");
        commit_all(&t.0, "base");
        let inteira = Query { whole_word: true, ..q("gato") };
        assert_eq!(linhas(&buscar(&t.0, &inteira), "a.txt").len(), 1);
        // Literal trata o ponto como ponto; regex trata como qualquer caractere.
        assert_eq!(linhas(&buscar(&t.0, &q("x.y")), "a.txt").len(), 1);
        let regex = Query { regex: true, ..q("x.y") };
        assert_eq!(linhas(&buscar(&t.0, &regex), "a.txt").len(), 2);
        let captura = Query { regex: true, ..q("gat(\\w+)") };
        assert_eq!(linhas(&buscar(&t.0, &captura), "a.txt").len(), 2);
    }

    #[test]
    fn regex_invalida_e_quebra_de_linha_sao_recusadas() {
        let t = repo_tmp("invalida");
        write(&t.0, "a.txt", "x\n");
        commit_all(&t.0, "base");
        let invalida = Query { regex: true, ..q("(aberto") };
        assert!(search(&t.0, &invalida).unwrap_err().starts_with("Expressão regular inválida"));
        assert!(search(&t.0, &q("a\nb")).unwrap_err().contains("quebra de linha"));
        assert!(buscar(&t.0, &q("   ")).files.is_empty());
    }

    #[test]
    fn nao_rastreado_aparece_e_ignorado_nao() {
        let t = repo_tmp("ignorado");
        write(&t.0, ".gitignore", "escondido.txt\n");
        write(&t.0, "base.txt", "marca\n");
        commit_all(&t.0, "base");
        write(&t.0, "novo.txt", "marca\n");
        write(&t.0, "escondido.txt", "marca\n");
        let r = buscar(&t.0, &q("marca"));
        assert_eq!(caminhos(&r), vec!["base.txt", "novo.txt"]);
    }

    #[test]
    fn binario_nao_utf8_e_grande_ficam_de_fora_e_sao_dizidos() {
        let t = repo_tmp("fora");
        write(&t.0, "texto.txt", "marca\n");
        write(&t.0, "bin.dat", b"marca\0\0");
        write(&t.0, "latin.txt", b"marca \xFF\n");
        write(&t.0, "grande.txt", vec![b'a'; (files::MAX_FILE + 1) as usize]);
        commit_all(&t.0, "base");
        let r = buscar(&t.0, &q("marca"));
        assert_eq!(caminhos(&r), vec!["texto.txt"]);
        let motivos: Vec<(String, SkipReason)> = r.skipped.iter().map(|s| (s.path.clone(), s.reason)).collect();
        assert!(motivos.contains(&("bin.dat".into(), SkipReason::Binary)));
        assert!(motivos.contains(&("latin.txt".into(), SkipReason::NotUtf8)));
        assert!(motivos.contains(&("grande.txt".into(), SkipReason::TooLarge)));
    }

    #[test]
    fn coluna_conta_caracteres_e_nao_bytes() {
        let t = repo_tmp("coluna");
        write(&t.0, "a.txt", "ação ok\n");
        commit_all(&t.0, "base");
        let r = buscar(&t.0, &q("ok"));
        assert_eq!(r.files[0].matches[0].col, 6);
    }

    /// O trecho da linha a partir da coluna `col` (1-based, em UTF-16), como o CodeMirror enxerga.
    fn a_partir_da_coluna(line: &str, col: u32) -> String {
        let units: Vec<u16> = line.encode_utf16().collect();
        String::from_utf16_lossy(&units[col as usize - 1..])
    }

    #[test]
    fn coluna_e_utf16_e_cai_no_termo_com_emoji_acento_tab_e_crlf() {
        let t = repo_tmp("coluna-utf16");
        // Emoji fora do BMP (dois code units em UTF-16), acentos e tab antes do termo; a linha 3 tem emoji e espaço.
        write(&t.0, "a.txt", "😀 ação\tfoo\r\nalvo\r\n😀 alvo\r\n");
        commit_all(&t.0, "base");
        let r = buscar(&t.0, &q("foo"));
        assert_eq!(r.files[0].matches[0].col, 9, "😀(2) + espaço + ação(4) + tab = 8 unidades");
        let linha = "😀 ação\tfoo";
        assert!(a_partir_da_coluna(linha, 9).starts_with("foo"));
        let r = buscar(&t.0, &q("alvo"));
        let cols: Vec<(u32, u32)> = r.files[0].matches.iter().map(|m| (m.line, m.col)).collect();
        assert_eq!(cols, vec![(2, 1), (3, 4)]);
        assert_eq!(r.files[0].matches[1].text, "😀 alvo", "o CR do fim não entra no texto");
        assert!(a_partir_da_coluna("😀 alvo", 4).starts_with("alvo"));
    }

    #[test]
    fn coluna_e_da_linha_original_mesmo_com_janela_recortada() {
        let t = repo_tmp("coluna-janela");
        // 200 pares "é😀" (600 unidades UTF-16) antes do termo, e 300 caracteres depois: a janela recorta o começo.
        let linha = format!("{}ALVO{}", "é😀".repeat(200), "z".repeat(300));
        write(&t.0, "a.txt", format!("{linha}\n"));
        commit_all(&t.0, "base");
        let m = &buscar(&t.0, &q("ALVO")).files[0].matches[0];
        assert!(m.text.starts_with('é') && m.text.len() < linha.len(), "a janela tem que ter recortado");
        assert!(m.text.contains("ALVO"));
        assert_eq!(m.col, 601);
        assert!(a_partir_da_coluna(&linha, m.col).starts_with("ALVO"));
    }

    #[test]
    fn linha_com_crlf_sai_sem_cr_e_dois_pontos_nao_atrapalham() {
        let t = repo_tmp("crlf");
        write(&t.0, "a.txt", "um: dois\r\n");
        commit_all(&t.0, "base");
        assert_eq!(linhas(&buscar(&t.0, &q("dois")), "a.txt"), vec![(1, "um: dois".to_string())]);
    }

    #[test]
    fn filtros_incluir_e_excluir_caminhos() {
        let t = repo_tmp("filtros");
        write(&t.0, "src/a.ts", "marca\n");
        write(&t.0, "src/b.md", "marca\n");
        write(&t.0, "docs/c.ts", "marca\n");
        write(&t.0, "src/d.tsx", "marca\n");
        commit_all(&t.0, "base");
        let so_ts = Query { include: vec!["*.ts".into()], ..q("marca") };
        assert_eq!(caminhos(&buscar(&t.0, &so_ts)), vec!["docs/c.ts", "src/a.ts"], "*.ts não casa .tsx");
        let sem_docs = Query { exclude: vec!["docs/".into()], ..q("marca") };
        assert_eq!(caminhos(&buscar(&t.0, &sem_docs)), vec!["src/a.ts", "src/b.md", "src/d.tsx"]);
    }

    #[test]
    fn filtro_com_ponto_barra_vale_a_partir_da_raiz() {
        let t = repo_tmp("raiz");
        write(&t.0, "src/a.ts", "marca\n");
        write(&t.0, "src-tauri/src/b.rs", "marca\n");
        write(&t.0, "docs/src/c.md", "marca\n");
        write(&t.0, "srcx.txt", "marca\n");
        commit_all(&t.0, "base");
        let incluir = |include: &str| -> Vec<String> {
            let r = buscar(&t.0, &Query { include: vec![include.into()], ..q("marca") });
            caminhos(&r).into_iter().map(String::from).collect()
        };
        assert_eq!(incluir("./src"), vec!["src/a.ts"]);
        assert_eq!(incluir("./src/"), vec!["src/a.ts"]);
        assert_eq!(incluir(".\\SRC"), vec!["src/a.ts"], "barra invertida e caixa não importam");
        assert_eq!(incluir("./src-tauri/*.rs"), vec!["src-tauri/src/b.rs"]);
        assert_eq!(incluir("./").len(), 4);
        // Sem `./`, o trecho continua casando em qualquer ponto do caminho.
        assert_eq!(incluir("src/"), vec!["docs/src/c.md", "src-tauri/src/b.rs", "src/a.ts"]);
        let fora = Query { exclude: vec!["./src".into()], ..q("marca") };
        assert_eq!(caminhos(&buscar(&t.0, &fora)), vec!["docs/src/c.md", "src-tauri/src/b.rs", "srcx.txt"]);
    }

    #[test]
    fn so_nos_caminhos_abertos() {
        let t = repo_tmp("abertos");
        write(&t.0, "a.txt", "marca\n");
        write(&t.0, "b[1].txt", "marca\n");
        write(&t.0, "c.txt", "marca\n");
        commit_all(&t.0, "base");
        let abertos = Query { only: Some(vec!["b[1].txt".into(), "c.txt".into(), "sumiu.txt".into()]), ..q("marca") };
        assert_eq!(caminhos(&buscar(&t.0, &abertos)), vec!["b[1].txt", "c.txt"]);
        let nenhum = Query { only: Some(vec![]), ..q("marca") };
        assert!(buscar(&t.0, &nenhum).files.is_empty());
        // A aplicação usa o mesmo recorte: arquivo fora dele é recusado.
        let prev = preview(&t.0, &abertos, "x").unwrap();
        let so_c = Query { only: Some(vec!["c.txt".into()]), ..q("marca") };
        let req = ApplyRequest { query: so_c, replacement: "x".into(), files: vec![pick(&prev, "b[1].txt")] };
        assert_eq!(apply(&t.0, &req).unwrap().errors.len(), 1);
    }

    #[test]
    fn limite_de_linhas_marca_truncado() {
        let t = repo_tmp("limite");
        write(&t.0, "a.txt", "x\n".repeat(MAX_LINES + 5));
        commit_all(&t.0, "base");
        let r = buscar(&t.0, &q("x"));
        assert!(r.truncated);
        assert_eq!(r.files[0].matches.len(), MAX_LINES);
    }

    #[test]
    fn preview_nao_grava_e_traz_original_e_resultado_sem_bom() {
        let t = repo_tmp("preview");
        write(&t.0, "a.txt", b"\xEF\xBB\xBFtroca isto\r\nisto de novo\r\n");
        commit_all(&t.0, "base");
        let before = read(&t.0, "a.txt");
        let prev = preview(&t.0, &q("isto"), "aquilo").unwrap();
        assert_eq!(read(&t.0, "a.txt"), before, "a prévia não pode gravar");
        assert_eq!(prev.files.len(), 1);
        let f = &prev.files[0];
        assert_eq!(f.count, 2);
        assert_eq!(f.original, "troca isto\r\nisto de novo\r\n");
        assert_eq!(f.result, "troca aquilo\r\naquilo de novo\r\n");
        assert_eq!(prev.omitted_files, 0);
    }

    #[test]
    fn aplicacao_seletiva_preserva_bom_e_crlf() {
        let t = repo_tmp("seletiva");
        write(&t.0, "a.txt", b"\xEF\xBB\xBFfoo\r\n");
        write(&t.0, "b.txt", "foo\nfoo\n");
        write(&t.0, "c.txt", "foo\n");
        commit_all(&t.0, "base");
        let prev = preview(&t.0, &q("foo"), "bar").unwrap();
        assert_eq!(prev.files.len(), 3);
        let req = ApplyRequest { query: q("foo"), replacement: "bar".into(), files: vec![pick(&prev, "a.txt"), pick(&prev, "b.txt")] };
        let out = apply(&t.0, &req).unwrap();
        assert_eq!(out.applied, vec![Done { path: "a.txt".into(), count: 1 }, Done { path: "b.txt".into(), count: 2 }]);
        assert!(out.errors.is_empty());
        assert_eq!(read(&t.0, "a.txt"), b"\xEF\xBB\xBFbar\r\n");
        assert_eq!(read(&t.0, "b.txt"), b"bar\nbar\n");
        assert_eq!(read(&t.0, "c.txt"), b"foo\n", "arquivo não escolhido fica como estava");
    }

    #[test]
    fn stale_e_recusado_sem_gravar_e_os_outros_seguem() {
        let t = repo_tmp("stale");
        write(&t.0, "a.txt", "foo\n");
        write(&t.0, "b.txt", "foo\n");
        commit_all(&t.0, "base");
        let prev = preview(&t.0, &q("foo"), "bar").unwrap();
        write(&t.0, "a.txt", "foo mudou por fora\n");
        let req = ApplyRequest { query: q("foo"), replacement: "bar".into(), files: vec![pick(&prev, "a.txt"), pick(&prev, "b.txt")] };
        let out = apply(&t.0, &req).unwrap();
        assert_eq!(out.applied, vec![Done { path: "b.txt".into(), count: 1 }]);
        assert!(out.errors[0].error.contains("mudou desde a prévia"));
        assert_eq!(read(&t.0, "a.txt"), b"foo mudou por fora\n");
    }

    #[test]
    fn captures_funcionam_e_sintaxe_nao_suportada_e_recusada() {
        let t = repo_tmp("captures");
        write(&t.0, "a.txt", "nome=ana\n");
        commit_all(&t.0, "base");
        let regex = Query { regex: true, ..q(r"(\w+)=(?P<valor>\w+)") };
        let prev = preview(&t.0, &regex, "$valor:$1 $$").unwrap();
        assert_eq!(prev.files[0].result, "ana:nome $\n");
        // `$9` não existe: recusa antes de qualquer arquivo; `$x` também.
        assert!(preview(&t.0, &regex, "$9").unwrap_err().contains("não suportada"));
        assert!(preview(&t.0, &regex, "$x").unwrap_err().contains("não suportada"));
        assert!(preview(&t.0, &regex, "${sem-fechar").unwrap_err().contains("não suportada"));
        // Literal não interpreta nada: `$1` sai como texto.
        let literal = preview(&t.0, &q("nome"), "$1").unwrap();
        assert_eq!(literal.files[0].result, "$1=ana\n");
    }

    #[test]
    fn preservar_maiusculas_segue_a_caixa_da_ocorrencia() {
        let t = repo_tmp("preserva");
        write(&t.0, "a.txt", "foo FOO Foo fOo\n");
        commit_all(&t.0, "base");
        let sem = preview(&t.0, &q("foo"), "bar").unwrap();
        assert_eq!(sem.files[0].result, "bar bar bar bar\n", "sem a opção, a substituição vai como foi digitada");
        let com = Query { preserve_case: true, ..q("foo") };
        assert_eq!(preview(&t.0, &com, "bar").unwrap().files[0].result, "bar BAR Bar bar\n");
    }

    #[test]
    fn preservar_maiusculas_com_acento_e_regex_com_captura() {
        let t = repo_tmp("preserva-acento");
        write(&t.0, "a.txt", "AÇÃO e ação e Ação\nNOME Nome nome\n");
        commit_all(&t.0, "base");
        let acento = Query { preserve_case: true, ..q("ação") };
        assert_eq!(preview(&t.0, &acento, "revisão").unwrap().files[0].result, "REVISÃO e revisão e Revisão\nNOME Nome nome\n");
        let captura = Query { preserve_case: true, regex: true, ..q("(nome)") };
        assert_eq!(preview(&t.0, &captura, "$1 novo").unwrap().files[0].result, "AÇÃO e ação e Ação\nNOME NOVO Nome novo nome novo\n");
    }

    #[test]
    fn caminho_fora_da_pesquisa_e_recusado_na_aplicacao() {
        let t = repo_tmp("caminho");
        write(&t.0, "a.txt", "foo\n");
        write(&t.0, "dentro/b.txt", "foo\n");
        commit_all(&t.0, "base");
        let prev = preview(&t.0, &q("foo"), "bar").unwrap();
        let mut files = vec![pick(&prev, "a.txt")];
        for path in ["../fora.txt", "/abs.txt", "dentro\\b.txt", ".git/config", "nao-existe.txt"] {
            files.push(Pick { path: path.into(), revision: "x".into() });
        }
        let out = apply(&t.0, &ApplyRequest { query: q("foo"), replacement: "bar".into(), files }).unwrap();
        assert_eq!(out.applied.len(), 1);
        assert_eq!(out.errors.len(), 5);
        assert!(out.errors.iter().all(|e| e.error.contains("não está na pesquisa atual")));
    }

    #[test]
    fn crlf_sobrevive_a_substituicao_que_mexe_na_linha() {
        let t = repo_tmp("eol");
        write(&t.0, "a.txt", "a x\r\nb x\r\n");
        commit_all(&t.0, "base");
        let prev = preview(&t.0, &q("x"), "yy").unwrap();
        assert_eq!(prev.files[0].result, "a yy\r\nb yy\r\n");
    }

    #[test]
    fn apply_nao_grava_se_a_consulta_nao_tem_mais_ocorrencia() {
        let t = repo_tmp("sem-ocorrencia");
        write(&t.0, "a.txt", "foo\n");
        commit_all(&t.0, "base");
        let prev = preview(&t.0, &q("foo"), "bar").unwrap();
        write(&t.0, "a.txt", "nada aqui\n");
        let req = ApplyRequest { query: q("foo"), replacement: "bar".into(), files: vec![pick(&prev, "a.txt")] };
        let out = apply(&t.0, &req).unwrap();
        assert!(out.applied.is_empty());
        assert_eq!(read(&t.0, "a.txt"), b"nada aqui\n");
    }

    #[test]
    fn abertura_rapida_so_rastreado_e_nao_ignorado_e_ordenada() {
        let t = repo_tmp("rapida");
        write(&t.0, ".gitignore", "build/\n");
        write(&t.0, "src/app.ts", "x\n");
        write(&t.0, "docs/app-guia.md", "x\n");
        write(&t.0, "build/app.js", "x\n");
        commit_all(&t.0, "base");
        write(&t.0, "app-novo.ts", "x\n");
        let quick = quick_open(&t.0, "app").unwrap();
        assert_eq!(quick.total, 3);
        assert_eq!(quick.paths, vec!["src/app.ts", "app-novo.ts", "docs/app-guia.md"]);
        assert_eq!(quick_open(&t.0, "build").unwrap().total, 0);
        assert_eq!(quick_open(&t.0, "src app").unwrap().paths, vec!["src/app.ts"]);
    }

    #[test]
    fn abertura_rapida_nao_segue_junction_de_pasta_do_meio() {
        // Pasta de fora sem `.git` (o `repo_tmp` inicia um repositório, e isso muda o que o git lista lá dentro).
        let fora = Tmp::new("orq-search", "junction-fora");
        write(&fora.0, "segredo.txt", "x\n");
        let t = repo_tmp("junction");
        write(&t.0, "base.txt", "x\n");
        commit_all(&t.0, "base");
        // Junção não exige privilégio no Windows. Se não der para criar, o teste é pulado.
        if !junction(&t.0.join("link"), &fora.0) {
            eprintln!("sem como criar junção neste ambiente; teste pulado");
            return;
        }
        // Antes de tudo: a pesquisa enxerga o caminho pela junção, mas recusa o conteúdo que sai do projeto.
        assert!(candidates(&t.0, &Filter::default()).unwrap().contains(&"link/segredo.txt".to_string()));
        assert!(buscar(&t.0, &q("x")).skipped.iter().any(|s| s.path == "link/segredo.txt" && s.reason == SkipReason::NotFile));
        assert_eq!(quick_open(&t.0, "segredo").unwrap().total, 0);
        assert_eq!(quick_open(&t.0, "base").unwrap().paths, vec!["base.txt"]);
        // Apagar o projeto (com a junção dentro) não pode apagar o alvo da junção.
        drop(t);
        assert!(fora.0.join("segredo.txt").exists(), "a limpeza do projeto não pode seguir a junção");
    }

    #[cfg(windows)]
    #[test]
    fn link_nao_entra_na_substituicao() {
        let t = repo_tmp("link");
        write(&t.0, "real.txt", "foo\n");
        commit_all(&t.0, "base");
        if crate::platform::symlink_arquivo(&t.0.join("real.txt"), &t.0.join("atalho.txt")).is_err() {
            eprintln!("sem permissão para criar link no Windows; teste pulado");
            return;
        }
        git(&t.0, &["add", "-A"]);
        let prev = preview(&t.0, &q("foo"), "bar").unwrap();
        assert_eq!(prev.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec!["real.txt"]);
        assert!(buscar(&t.0, &q("foo")).skipped.iter().any(|s| s.path == "atalho.txt" && s.reason == SkipReason::NotFile));
    }

    #[test]
    fn sem_repositorio_a_pesquisa_respeita_gitignore_e_pula_git_e_node_modules() {
        let t = Tmp::new("orq-search", "sem-git");
        write(&t.0, ".gitignore", "ignorado.txt\nbuild/\n");
        write(&t.0, "ignorado.txt", "x");
        write(&t.0, "build/saida.txt", "x");
        write(&t.0, "a.txt", "x");
        write(&t.0, "pasta/b.txt", "x");
        write(&t.0, ".oculto", "x");
        // Repositório aninhado: o `.git` dele fica de fora, o resto da subpasta entra.
        write(&t.0, "sub/.git/HEAD", "ref: refs/heads/main\n");
        write(&t.0, "sub/x.txt", "x");
        write(&t.0, "node_modules/lib/i.txt", "x");
        write(&t.0, "pasta/node_modules/j.txt", "x");
        assert_eq!(candidates(&t.0, &Filter::default()).unwrap(), vec![".gitignore", ".oculto", "a.txt", "pasta/b.txt", "sub/x.txt"]);
        assert_eq!(quick_open(&t.0, "b.txt").unwrap().paths, vec!["pasta/b.txt"]);
    }

    #[cfg(windows)]
    #[test]
    fn sem_repositorio_a_pesquisa_nao_entra_em_juncao() {
        let fora = Tmp::new("orq-search", "sem-git-junction-fora");
        write(&fora.0, "segredo.txt", "x\n");
        let t = Tmp::new("orq-search", "sem-git-junction");
        write(&t.0, "base.txt", "x\n");
        // Junção não exige privilégio no Windows.
        assert!(junction(&t.0.join("link"), &fora.0), "mklink /J falhou");
        assert_eq!(candidates(&t.0, &Filter::default()).unwrap(), vec!["base.txt"]);
        assert_eq!(quick_open(&t.0, "segredo").unwrap().total, 0);
    }
}
