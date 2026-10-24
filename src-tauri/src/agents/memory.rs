//! A memória de contexto do orquestrador: o texto literal que o app guarda fora do contexto do modelo, para
//! depois de uma compactação. Fica em `<sessão>\memoria\`: `atual` com o id da conversa corrente e uma pasta por
//! conversa, com uma entrada por arquivo (`<hook::unique()>.json`, `{"tipo", "texto"}`, publicada por renomeação) e
//! `transcript`, com o último `transcript_path` que o CLI informou. O transcript é lido onde está, nunca
//! copiado. Quem grava é o modo `--memoria` (hooks do Claude e do Codex) e o servidor MCP (lotes e resultados); quem lê é a
//! ferramenta `memoria` e a lembrança que o `SessionStart` devolve depois de uma compactação.
use crate::agents::delegate;
use crate::agents::hook;
use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

/// Tipos de entrada.
pub const PEDIDO: &str = "pedido";
pub const RESPOSTA: &str = "resposta";
pub const LOTE: &str = "lote";
pub const RESULTADO: &str = "resultado";
pub const COMPACTACAO: &str = "compactacao";

/// A conversa de quem não informou `session_id`, e a que a ferramenta lê quando nenhum hook disparou.
const DEFAULT: &str = "sessao";
/// Teto, em caracteres, da lembrança devolvida depois de uma compactação e do índice da ferramenta.
pub const REMINDER_LIMIT: usize = 4000;
/// Teto, em caracteres, da saída de uma busca.
pub const SEARCH_LIMIT: usize = 4000;
/// Tamanho, em caracteres, de cada parte do texto integral de uma entrada.
pub const PART: usize = 20000;
const RESULTS: usize = 8;
const SNIPPET: usize = 300;
const PREVIEW: usize = 100;
/// O que o índice reserva no fim para dizer que há entradas mais antigas.
const MORE_ROOM: usize = 80;

const HEADER: &str = "Memória do app: o texto literal que ele guardou desta conversa, fora do resumo. A ferramenta `memoria` busca detalhes (`consulta`) e abre entradas pelo id (`entrada`, como e12).\n";

/// Uma entrada da conversa, na ordem em que foi gravada. O id mostrado ao modelo é `e<posição>`, a partir de 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: String,
    pub text: String,
}

/// `orquestrador.exe --memoria <pasta da sessão>`: o `--hook` da memória, chamado pelos hooks `SessionStart`,
/// `UserPromptSubmit`, `Stop` e `PreCompact` do Claude Code e do Codex, que entregam o JSON por stdin. Imprime a
/// resposta do hook em ASCII (`ascii`) e sai sempre com 0: a memória nunca atrapalha o CLI.
pub fn run(args: &[String]) -> i32 {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    let answer = args.first().map_or_else(|| json!({}), |session| handle(Path::new(session), &buf));
    println!("{}", ascii(&answer));
    0
}

/// O JSON só com bytes ASCII: todo caractere não ASCII vira `\uXXXX` (par substituto acima do BMP). Medido no Codex
/// 0.163 (Windows): o stdout do hook passa pelo `pwsh`, que o lê na codepage ibm850, e texto não ASCII chega
/// corrompido; o stdin do hook chega correto. Os dois CLIs decodificam o escape igual, então a saída é a mesma.
/// Fora de strings o JSON do `serde_json` já é ASCII, então o escape não altera a estrutura.
fn ascii(value: &Value) -> String {
    let mut out = String::new();
    for c in value.to_string().chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

/// Trata um evento de hook. Pasta que não é sessão ativa, JSON ilegível e evento desconhecido não gravam nada e
/// respondem `{}`. A única resposta com conteúdo é a lembrança do `SessionStart` depois de uma compactação; nada é
/// injetado por mensagem.
pub fn handle(session: &Path, payload: &[u8]) -> Value {
    let Ok(input) = serde_json::from_slice::<Value>(payload) else { return json!({}) };
    if delegate::check_session(session).is_err() {
        return json!({});
    }
    let conversation = conversation_id(input["session_id"].as_str().unwrap_or_default());
    let dir = session.join("memoria").join(&conversation);
    let text = |key: &str| input[key].as_str().filter(|t| !t.is_empty());
    let mark = || {
        let _ = hook::put(&session.join("memoria"), "atual", conversation.as_bytes());
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return json!({});
    }
    let mut reminder = None;
    match input["hook_event_name"].as_str() {
        Some("SessionStart") => {
            mark();
            if input["source"] == "compact" {
                reminder = Some(remind(&entries(&dir)));
            }
        }
        Some("UserPromptSubmit") => {
            mark();
            if let Some(prompt) = text("prompt") {
                let _ = save(&dir, PEDIDO, prompt);
            }
        }
        Some("Stop") => {
            if let Some(message) = text("last_assistant_message") {
                let _ = save(&dir, RESPOSTA, message);
            }
        }
        Some("PreCompact") => {
            let _ = save(&dir, COMPACTACAO, input["trigger"].as_str().unwrap_or("desconhecido"));
        }
        _ => return json!({}),
    }
    if let Some(path) = text("transcript_path") {
        let _ = hook::put(&dir, "transcript", path.as_bytes());
    }
    match reminder {
        Some(context) => json!({ "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": context } }),
        None => json!({}),
    }
}

/// Grava uma entrada na conversa corrente da sessão. Falha em silêncio: a memória nunca muda a resposta de quem
/// chama.
pub fn record(session: &Path, kind: &str, text: &str) {
    let dir = session.join("memoria").join(current(session));
    let _ = std::fs::create_dir_all(&dir).and_then(|()| save(&dir, kind, text));
}

/// O texto de um lote de tarefas como entrada `lote`: cada tarefa sob o seu marcador, literal.
pub fn batch_text(tasks: &[String]) -> String {
    let total = tasks.len();
    tasks.iter().enumerate().map(|(n, task)| format!("--- TAREFA {}/{total} ---\n{task}\n", n + 1)).collect()
}

/// As entradas da conversa corrente da sessão, da mais antiga à mais recente.
pub fn list(session: &Path) -> Vec<Entry> {
    entries(&session.join("memoria").join(current(session)))
}

fn save(dir: &Path, kind: &str, text: &str) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(&json!({ "tipo": kind, "texto": text }))?;
    hook::put(dir, &format!("{}.json", hook::unique()), &bytes)
}

/// O id da conversa como nome de pasta: só `[A-Za-z0-9_-]`, até 64 caracteres. Vazio vira `sessao`.
fn conversation_id(raw: &str) -> String {
    let id: String = raw.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).take(64).collect();
    if id.is_empty() { DEFAULT.into() } else { id }
}

/// A conversa corrente, a de `atual`. Sem o arquivo, `sessao`.
fn current(session: &Path) -> String {
    conversation_id(std::fs::read_to_string(session.join("memoria").join("atual")).unwrap_or_default().trim())
}

/// As entradas de uma pasta de conversa, na ordem dos nomes. Arquivo ilegível é pulado.
fn entries(dir: &Path) -> Vec<Entry> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json") && p.is_file())
        .collect();
    names.sort();
    names
        .iter()
        .filter_map(|p| {
            let v: Value = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
            Some(Entry { kind: v["tipo"].as_str()?.to_string(), text: v["texto"].as_str()?.to_string() })
        })
        .collect()
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// Os `n` primeiros caracteres de `s`, sem cortar um caractere no meio.
fn take(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

/// `s` com até `max` caracteres; o que passa vira `…` no fim.
fn clip(s: &str, max: usize) -> String {
    if chars(s) <= max { s.to_string() } else { format!("{}…", take(s, max - 1)) }
}

/// Uma linha do índice: `eN tipo: <primeira linha não vazia, até 100 caracteres>`.
fn index_line(n: usize, e: &Entry) -> String {
    let first = e.text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    format!("e{n} {}: {}\n", e.kind, clip(first, PREVIEW))
}

/// O índice das entradas que `skip` não marca, mais recentes primeiro, em até `room` caracteres. Devolve também
/// quantas ficaram de fora.
fn index(entries: &[Entry], skip: &[bool], room: usize) -> (String, usize) {
    let (mut out, mut left, mut rest) = (String::new(), room, 0);
    for (i, e) in entries.iter().enumerate().rev().filter(|(i, _)| !skip[*i]) {
        let line = index_line(i + 1, e);
        if rest == 0 && chars(&line) <= left {
            left -= chars(&line);
            out.push_str(&line);
        } else {
            rest += 1;
        }
    }
    (out, rest)
}

/// A lembrança do `SessionStart` depois de uma compactação, em até `REMINDER_LIMIT` caracteres: o cabeçalho, os
/// pedidos do usuário literais, mais recentes primeiro (o que não cabe inteiro é cortado com a indicação da íntegra, e
/// os mais antigos ficam de fora), e, com o espaço que sobrar, o índice das demais entradas.
fn remind(entries: &[Entry]) -> String {
    let mut out = HEADER.to_string();
    let mut shown = vec![false; entries.len()];
    let heading = "\nPedidos do usuário, mais recentes primeiro:\n";
    let mut room = REMINDER_LIMIT.saturating_sub(chars(&out) + chars(heading));
    let mut requests = String::new();
    for (i, e) in entries.iter().enumerate().rev().filter(|(_, e)| e.kind == PEDIDO) {
        let id = format!("e{}", i + 1);
        let block = format!("{id}: {}\n", e.text);
        if chars(&block) <= room {
            room -= chars(&block);
            requests.push_str(&block);
            shown[i] = true;
            continue;
        }
        let (prefix, cut) = (format!("{id}: "), format!("… (íntegra: memoria entrada {id})\n"));
        let fixed = chars(&prefix) + chars(&cut);
        if room > fixed {
            requests.push_str(&format!("{prefix}{}{cut}", take(&e.text, room - fixed)));
            shown[i] = true;
        }
        break;
    }
    if !requests.is_empty() {
        out.push_str(heading);
        out.push_str(&requests);
    }
    let heading = "\nOutras entradas, mais recentes primeiro:\n";
    let (lines, _) = index(entries, &shown, REMINDER_LIMIT.saturating_sub(chars(&out) + chars(heading)));
    if !lines.is_empty() {
        out.push_str(heading);
        out.push_str(&lines);
    }
    out
}

/// A ferramenta `memoria`. `entrada` abre o texto integral (em partes, com `parte`); `consulta` busca por termos nas
/// entradas e no transcript; sem nenhum dos dois, devolve o índice. Lê a conversa de `atual`, só da sessão recebida.
pub fn tool(session: &Path, args: &Value) -> Result<String, String> {
    let text = |key: &str| match &args[key] {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.trim()).filter(|s| !s.is_empty())),
        _ => Err(format!("`{key}` tem de ser um texto.")),
    };
    let dir = session.join("memoria").join(current(session));
    if let Some(id) = text("entrada")? {
        let part = match &args["parte"] {
            Value::Null => 1,
            v => v.as_u64().filter(|p| *p >= 1).ok_or("`parte` tem de ser um inteiro a partir de 1.")? as usize,
        };
        return open(&dir, id, part);
    }
    match text("consulta")? {
        Some(query) => search(&dir, query),
        None => overview(&dir),
    }
}

fn overview(dir: &Path) -> Result<String, String> {
    let entries = entries(dir);
    if entries.is_empty() {
        return Ok("Nenhuma entrada guardada nesta conversa.".into());
    }
    let heading = "Entradas desta conversa, mais recentes primeiro (`entrada` abre o texto integral, `consulta` busca por termos):\n";
    let (lines, rest) = index(&entries, &vec![false; entries.len()], REMINDER_LIMIT - chars(heading) - MORE_ROOM);
    let more = if rest > 0 { format!("… mais {rest} entradas antigas; busque com `consulta`.\n") } else { String::new() };
    Ok(format!("{heading}{lines}{more}"))
}

/// Minúscula sem acento (os do português), um caractere por caractere, para as posições do texto original valerem
/// no texto dobrado.
fn fold(c: char) -> char {
    match c.to_lowercase().next().unwrap_or(c) {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        c => c,
    }
}

/// Os tokens alfanuméricos de 3 ou mais caracteres de `text`, dobrados, cada um com a posição (em caracteres) em
/// que começa no texto original. Acento solto (forma decomposta) não separa nem entra no token.
fn tokens(text: &str) -> Vec<(String, usize)> {
    let (mut out, mut word, mut start) = (Vec::new(), String::new(), 0);
    let mut end = |word: &mut String, start: usize| {
        if chars(word) >= 3 {
            out.push((std::mem::take(word), start));
        }
        word.clear();
    };
    for (i, c) in text.chars().enumerate() {
        if ('\u{300}'..='\u{36f}').contains(&c) {
            continue;
        }
        let c = fold(c);
        if c.is_alphanumeric() {
            if word.is_empty() {
                start = i;
            }
            word.push(c);
        } else {
            end(&mut word, start);
        }
    }
    end(&mut word, start);
    out
}

/// Quantos termos distintos `text` tem, quantas vezes eles aparecem e onde (em caracteres) está o primeiro.
fn score(text: &str, terms: &[String]) -> Option<(usize, usize, usize)> {
    let mut seen = vec![0usize; terms.len()];
    let mut first = usize::MAX;
    for (token, at) in tokens(text) {
        if let Some(t) = terms.iter().position(|t| *t == token) {
            seen[t] += 1;
            first = first.min(at);
        }
    }
    let distinct = seen.iter().filter(|n| **n > 0).count();
    (distinct > 0).then(|| (distinct, seen.iter().sum(), first))
}

/// Até `SNIPPET` caracteres de `text` em volta da posição `at`.
fn snippet(text: &str, at: usize) -> String {
    let from = at.saturating_sub(SNIPPET / 3);
    let rest = text.char_indices().nth(from).map_or("", |(i, _)| &text[i..]);
    let body = take(rest, SNIPPET);
    let (before, after) = (if from > 0 { "…" } else { "" }, if body.len() < rest.len() { "…" } else { "" });
    format!("{before}{}{after}", body.trim())
}

/// A linha de um transcript JSONL como texto: todos os valores string do JSON, em qualquer profundidade e sem olhar
/// nomes de campo, um por linha. O que não é JSON entra cru.
fn line_text(raw: &str) -> String {
    fn collect(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.push(s.clone()),
            Value::Array(items) => items.iter().for_each(|i| collect(i, out)),
            Value::Object(map) => map.values().for_each(|i| collect(i, out)),
            _ => {}
        }
    }
    let raw = raw.trim_end_matches(['\r', '\n']);
    match serde_json::from_str::<Value>(raw) {
        Ok(v) => {
            let mut out = Vec::new();
            collect(&v, &mut out);
            out.join("\n")
        }
        Err(_) => raw.to_string(),
    }
}

/// As linhas do transcript da conversa, com o número (a partir de 1), sem decodificar. Sem arquivo gravado ou
/// legível, nenhuma.
fn transcript(dir: &Path) -> impl Iterator<Item = (usize, String)> {
    let path = std::fs::read_to_string(dir.join("transcript")).unwrap_or_default();
    let mut reader = Some(path.trim()).filter(|p| !p.is_empty()).and_then(|p| File::open(p).ok()).map(BufReader::new);
    let mut n = 0;
    std::iter::from_fn(move || {
        let mut buf = Vec::new();
        match reader.as_mut()?.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                n += 1;
                Some((n, String::from_utf8_lossy(&buf).into_owned()))
            }
        }
    })
}

/// Um resultado da busca. `rank`: termos distintos, frequência, origem (1 entrada, 0 transcript) e posição.
struct Hit {
    rank: (usize, usize, u8, usize),
    id: String,
    kind: String,
    snippet: String,
}

/// A busca: as entradas e as linhas do transcript que têm termos da consulta, melhores primeiro. A pontuação é o
/// número de termos distintos presentes, depois a frequência deles e depois o mais recente (entre as duas origens, a
/// entrada vem antes da linha do transcript).
fn search(dir: &Path, query: &str) -> Result<String, String> {
    let mut terms: Vec<String> = Vec::new();
    for (token, _) in tokens(query) {
        if !terms.contains(&token) {
            terms.push(token);
        }
    }
    if terms.is_empty() {
        return Err("A consulta não tem termos: use palavras de 3 ou mais letras ou números.".into());
    }
    let mut hits: Vec<Hit> = Vec::new();
    for (i, e) in entries(dir).iter().enumerate() {
        if let Some((distinct, freq, at)) = score(&e.text, &terms) {
            hits.push(Hit { rank: (distinct, freq, 1, i), id: format!("e{}", i + 1), kind: e.kind.clone(), snippet: snippet(&e.text, at) });
        }
    }
    for (n, raw) in transcript(dir) {
        let text = line_text(&raw);
        if let Some((distinct, freq, at)) = score(&text, &terms) {
            hits.push(Hit { rank: (distinct, freq, 0, n), id: format!("t{n}"), kind: "transcript".into(), snippet: snippet(&text, at) });
        }
    }
    if hits.is_empty() {
        return Ok(format!("Nada encontrado para: {}. Tente outros termos (sinônimos, nomes de arquivo, trechos do que foi dito).", terms.join(" ")));
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.rank));
    let mut out = String::from("Resultados, melhores primeiro (`entrada` abre o texto integral):\n");
    for Hit { id, kind, snippet: text, .. } in hits.into_iter().take(RESULTS) {
        let (prefix, room) = (format!("\n{id} {kind}: "), SEARCH_LIMIT - chars(&out));
        let block = format!("{prefix}{text}\n");
        if chars(&block) <= room {
            out.push_str(&block);
        } else {
            if room > chars(&prefix) + 40 {
                out.push_str(&format!("{prefix}{}…\n", take(&text, room - chars(&prefix) - 2)));
            }
            break;
        }
    }
    Ok(out)
}

/// O texto integral de `e<n>` ou `t<n>`, a parte `part` de `PART` caracteres.
fn open(dir: &Path, id: &str, part: usize) -> Result<String, String> {
    let id = id.to_lowercase();
    let number = id.get(1..).and_then(|n| n.parse::<usize>().ok()).filter(|n| *n >= 1);
    let (kind, text) = match (id.chars().next(), number) {
        (Some('e'), Some(n)) => {
            let e = entries(dir).into_iter().nth(n - 1).ok_or_else(|| format!("Entrada inexistente: {id}."))?;
            (e.kind, e.text)
        }
        (Some('t'), Some(n)) => {
            let (_, raw) = transcript(dir).find(|(line, _)| *line == n).ok_or_else(|| format!("Entrada inexistente: {id}."))?;
            ("transcript".to_string(), line_text(&raw))
        }
        _ => return Err(format!("Id inválido: {id}. Use e<número> ou t<número>.")),
    };
    let parts = chars(&text).div_ceil(PART).max(1);
    if part > parts {
        return Err(format!("{id} tem {parts} parte(s); a parte {part} não existe."));
    }
    let from = text.char_indices().nth((part - 1) * PART).map_or(text.len(), |(i, _)| i);
    let next = if part < parts { format!("\n--- próxima parte: entrada={id} parte={} ---\n", part + 1) } else { String::new() };
    Ok(format!("{id} {kind}, parte {part}/{parts}\n{}{next}", take(&text[from..], PART)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::session;

    fn event(s: &Path, v: Value) -> Value {
        handle(s, v.to_string().as_bytes())
    }

    fn prompt(s: &Path, id: &str, text: &str) {
        assert_eq!(event(s, json!({ "hook_event_name": "UserPromptSubmit", "session_id": id, "prompt": text })), json!({}));
    }

    fn compact(s: &Path, id: &str) -> String {
        assert_eq!(event(s, json!({ "hook_event_name": "PreCompact", "session_id": id, "trigger": "auto" })), json!({}));
        let got = event(s, json!({ "hook_event_name": "SessionStart", "session_id": id, "source": "compact" }));
        assert_eq!(got["hookSpecificOutput"]["hookEventName"], "SessionStart");
        got["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_string()
    }

    fn kinds(s: &Path) -> Vec<String> {
        list(s).into_iter().map(|e| e.kind).collect()
    }

    fn ask(s: &Path, args: Value) -> Result<String, String> {
        tool(s, &args)
    }

    #[test]
    fn sessao_nova_nao_tem_memoria() {
        let s = &session("orq-mem", "nova", true).0;
        assert!(list(s).is_empty());
        assert_eq!(ask(s, json!({})).unwrap(), "Nenhuma entrada guardada nesta conversa.");
        assert!(ask(s, json!({ "consulta": "qualquer coisa" })).unwrap().starts_with("Nada encontrado"));
        assert!(ask(s, json!({ "entrada": "e1" })).unwrap_err().contains("inexistente"));
        assert_eq!(remind(&[]), HEADER);
        assert!(!s.join("memoria").exists(), "ler não cria nada");
    }

    #[test]
    fn cada_evento_grava_o_tipo_certo_com_texto_literal() {
        let s = &session("orq-mem", "eventos", true).0;
        let pedido = "Corrija o \"cadastro\"\nlinha 2: ação d'água \\n $HOME `x`";
        let resposta = "Feito.\n\n  Com acentos: coração, não.";
        let start = event(s, json!({ "hook_event_name": "SessionStart", "session_id": "c1", "source": "startup" }));
        assert_eq!(start, json!({}), "só a compactação devolve a lembrança");
        assert_eq!(std::fs::read_to_string(s.join("memoria").join("atual")).unwrap(), "c1");
        prompt(s, "c1", pedido);
        assert_eq!(event(s, json!({ "hook_event_name": "Stop", "session_id": "c1", "last_assistant_message": resposta })), json!({}));
        assert_eq!(event(s, json!({ "hook_event_name": "Stop", "session_id": "c1", "last_assistant_message": "" })), json!({}));
        assert_eq!(event(s, json!({ "hook_event_name": "PreCompact", "session_id": "c1", "trigger": "manual" })), json!({}));
        let got = list(s);
        let got: Vec<(&str, &str)> = got.iter().map(|e| (e.kind.as_str(), e.text.as_str())).collect();
        assert_eq!(got, [(PEDIDO, pedido), (RESPOSTA, resposta), (COMPACTACAO, "manual")]);
        let names: Vec<String> = std::fs::read_dir(s.join("memoria").join("c1")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.iter().all(|n| n.ends_with(".json") && !n.ends_with(".tmp")), "{names:?}");
    }

    #[test]
    fn transcript_path_atualiza_o_arquivo_da_conversa() {
        let s = &session("orq-mem", "tpath", true).0;
        let dir = s.join("memoria").join("c1");
        event(s, json!({ "hook_event_name": "UserPromptSubmit", "session_id": "c1", "prompt": "a", "transcript_path": r"C:\x\um.jsonl" }));
        event(s, json!({ "hook_event_name": "Stop", "session_id": "c1", "last_assistant_message": "b", "transcript_path": r"C:\x\dois.jsonl" }));
        assert_eq!(std::fs::read_to_string(dir.join("transcript")).unwrap(), r"C:\x\dois.jsonl");
        let copies = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name() != "transcript").count();
        assert_eq!(copies, 2, "só as duas entradas; o transcript não é copiado");
    }

    #[test]
    fn conversa_e_saneada() {
        assert_eq!(conversation_id("abc-123_X"), "abc-123_X");
        assert_eq!(conversation_id(r"..\..\x y/z"), "xyz");
        assert_eq!(conversation_id(""), "sessao");
        assert_eq!(conversation_id("../.."), "sessao");
        assert_eq!(conversation_id(&"a".repeat(100)), "a".repeat(64));
        let s = &session("orq-mem", "sanea", true).0;
        prompt(s, r"..\fora", "oi");
        assert!(s.join("memoria").join("fora").is_dir() && !s.join("fora").exists());
        event(s, json!({ "hook_event_name": "UserPromptSubmit", "prompt": "sem id" }));
        assert!(s.join("memoria").join("sessao").is_dir());
        assert_eq!(std::fs::read_to_string(s.join("memoria").join("atual")).unwrap(), "sessao");
    }

    #[test]
    fn pasta_sem_dono_json_ilegivel_e_evento_desconhecido_nao_gravam_nada() {
        let s = &session("orq-mem", "sem-dono", true).0;
        std::fs::remove_file(s.join("owner")).unwrap();
        prompt(s, "c1", "oi");
        assert!(!s.join("memoria").exists());
        let s = &session("orq-mem", "ilegivel", true).0;
        assert_eq!(handle(s, b"{nao e json"), json!({}));
        assert_eq!(handle(s, b""), json!({}));
        assert_eq!(handle(s, b"[1]"), json!({}));
        assert_eq!(event(s, json!({ "hook_event_name": "PostToolUse", "session_id": "c1" })), json!({}));
        assert!(!s.join("memoria").join("atual").exists());
    }

    #[test]
    fn lembranca_traz_pedidos_recentes_primeiro_dentro_do_teto() {
        let s = &session("orq-mem", "lembranca", true).0;
        event(s, json!({ "hook_event_name": "SessionStart", "session_id": "c1", "source": "startup" }));
        prompt(s, "c1", "Primeiro pedido");
        event(s, json!({ "hook_event_name": "Stop", "session_id": "c1", "last_assistant_message": "\n\nResposta um\nsegunda linha" }));
        prompt(s, "c1", "Segundo pedido\ncom duas linhas");
        let text = compact(s, "c1");
        assert!(chars(&text) <= REMINDER_LIMIT);
        assert!(text.starts_with(HEADER) && text.contains("`memoria`"));
        let (b, a) = (text.find("e3: Segundo pedido\ncom duas linhas").unwrap(), text.find("e1: Primeiro pedido").unwrap());
        assert!(b < a, "o mais recente primeiro: {text}");
        assert!(text.contains("e2 resposta: Resposta um\n"), "índice: primeira linha não vazia: {text}");
        assert!(text.contains("e4 compactacao: auto"), "{text}");
        assert!(!text.contains("e1 pedido:"), "o que veio inteiro não repete no índice");
    }

    #[test]
    fn pedido_maior_que_o_teto_e_cortado_com_a_indicacao_da_integra() {
        let s = &session("orq-mem", "corte", true).0;
        prompt(s, "c1", "Antigo");
        prompt(s, "c1", &"ç".repeat(6000));
        let text = compact(s, "c1");
        assert!(chars(&text) <= REMINDER_LIMIT, "{}", chars(&text));
        assert!(text.contains("e2: çççç") && text.contains("… (íntegra: memoria entrada e2)\n"));
        assert!(!text.contains("Antigo"), "depois do corte para: o mais antigo não entra");
        assert!(ask(s, json!({ "entrada": "e2" })).unwrap().contains(&"ç".repeat(6000)), "a íntegra segue na ferramenta");
    }

    #[test]
    fn varias_compactacoes_nao_perdem_entradas() {
        let s = &session("orq-mem", "varias", true).0;
        prompt(s, "c1", "Pedido inicial sobre o cadastro");
        for n in 2..=4 {
            compact(s, "c1");
            prompt(s, "c1", &format!("Pedido {n}"));
            event(s, json!({ "hook_event_name": "Stop", "session_id": "c1", "last_assistant_message": format!("Resposta {n}") }));
        }
        let last = compact(s, "c1");
        assert_eq!(kinds(s).iter().filter(|k| *k == COMPACTACAO).count(), 4);
        assert_eq!(list(s)[0].text, "Pedido inicial sobre o cadastro");
        assert!(last.contains("e1: Pedido inicial sobre o cadastro") && last.contains("e9: Pedido 4"), "{last}");
        assert!(chars(&last) <= REMINDER_LIMIT);
    }

    #[test]
    fn indice_sem_argumentos_respeita_o_teto() {
        let s = &session("orq-mem", "indice", true).0;
        for n in 1..=200 {
            prompt(s, "c1", &format!("\n  Pedido número {n} {}", "x".repeat(150)));
        }
        let got = ask(s, json!({})).unwrap();
        assert!(chars(&got) <= REMINDER_LIMIT, "{}", chars(&got));
        assert!(got.contains("e200 pedido: Pedido número 200 xxx") && got.contains("entradas antigas"), "{got}");
        assert!(!got.contains("e1 pedido"), "{got}");
        for line in got.lines().filter(|l| l.starts_with('e')) {
            assert!(chars(line.split_once(": ").unwrap().1) <= PREVIEW, "{line}");
        }
    }

    /// Um transcript no formato do Claude Code, só no que importa: uma linha por mensagem, com um `compact_boundary`
    /// no meio, e o que veio antes dele continua no arquivo.
    fn write_transcript(s: &Path) -> PathBuf {
        let path = s.join("transcript.jsonl");
        let lines = [
            json!({ "type": "user", "message": { "role": "user", "content": "Vamos migrar o cadastro para o PostgreSQL, com índices." } }).to_string(),
            "isto não é JSON: o servidor reiniciou às 10h".to_string(),
            json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Plano: a migração usa a tabela clientes." }, { "type": "tool_use", "input": { "command": "psql -c 'select 1'" } }] } }).to_string(),
            json!({ "type": "system", "subtype": "compact_boundary", "content": "Conversation compacted" }).to_string(),
            String::new(),
            json!({ "type": "user", "message": { "content": "Depois da compactação: confira o resumo." } }).to_string(),
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    #[test]
    fn busca_acha_entrada_antiga_e_linha_do_transcript() {
        let s = &session("orq-mem", "busca", true).0;
        let transcript = write_transcript(s);
        prompt(s, "c1", "Use a convenção de nomes em português para os módulos");
        event(s, json!({ "hook_event_name": "Stop", "session_id": "c1", "last_assistant_message": "Entendido, seguirei a convenção.", "transcript_path": transcript.to_string_lossy() }));
        for n in 0..30 {
            prompt(s, "c1", &format!("pedido de enchimento {n}"));
        }
        let got = ask(s, json!({ "consulta": "CONVENÇÃO nomes" })).unwrap();
        let (e1, e2) = (got.find("e1 pedido: Use a convenção").unwrap(), got.find("e2 resposta:").unwrap());
        assert!(e1 < e2, "dois termos antes de um só: {got}");
        let got = ask(s, json!({ "consulta": "postgresql migracao" })).unwrap();
        assert!(got.contains("t1 transcript: ") && got.contains("t3 transcript: "), "{got}");
        assert!(got.find("t3 ").unwrap() < got.find("t1 ").unwrap(), "um termo cada: o mais recente primeiro: {got}");
        assert!(ask(s, json!({ "consulta": "reiniciou" })).unwrap().contains("t2 transcript: isto não é JSON"), "linha crua");
        assert!(ask(s, json!({ "consulta": "psql" })).unwrap().contains("t3 transcript:"), "valor aninhado, sem nome de campo");
        assert!(ask(s, json!({ "consulta": "compactação confira" })).unwrap().contains("t6 transcript:"), "depois da fronteira também");
        assert!(ask(s, json!({ "consulta": "zzzz inexistente" })).unwrap().starts_with("Nada encontrado para: zzzz inexistente"));
        assert!(ask(s, json!({ "consulta": "a de" })).is_err(), "sem termo de 3 letras");
        let t = ask(s, json!({ "entrada": "t3" })).unwrap();
        assert!(t.starts_with("t3 transcript, parte 1/1\n") && t.contains("Plano: a migração usa a tabela clientes.") && t.contains("psql -c 'select 1'"), "{t}");
        assert!(ask(s, json!({ "entrada": "t7" })).unwrap_err().contains("inexistente"));
    }

    #[test]
    fn busca_limita_resultados_trecho_e_saida() {
        let s = &session("orq-mem", "limites", true).0;
        for n in 0..20 {
            prompt(s, "c1", &format!("{} cadastro {n} {}", "antes ".repeat(200), "depois ".repeat(200)));
        }
        let got = ask(s, json!({ "consulta": "cadastro" })).unwrap();
        assert!(chars(&got) <= SEARCH_LIMIT, "{}", chars(&got));
        let found = got.lines().filter(|l| l.starts_with('e') && l.contains(" pedido: ")).count();
        assert!((1..=RESULTS).contains(&found), "{found}");
        assert!(got.contains("e20 pedido: ") && got.find("e20 ").unwrap() < got.find("e19 ").unwrap(), "o mais recente primeiro no empate");
        let one = got.split("\n\n").nth(1).unwrap().lines().next().unwrap();
        assert!(chars(one) < SNIPPET + 40, "{}", chars(one));
        let s2 = &session("orq-mem", "poucos", true).0;
        for n in 0..12 {
            prompt(s2, "c1", &format!("curto cadastro {n}"));
        }
        let got = ask(s2, json!({ "consulta": "cadastro" })).unwrap();
        assert_eq!(got.matches(" pedido: ").count(), RESULTS);
    }

    #[test]
    fn entrada_devolve_a_integra_e_pagina_texto_com_multibyte() {
        let s = &session("orq-mem", "paginas", true).0;
        prompt(s, "c1", "curto, com acentuação e emoji 🙂");
        let big: String = "ação 🙂 ".repeat(6000);
        event(s, json!({ "hook_event_name": "Stop", "session_id": "c1", "last_assistant_message": big }));
        let got = ask(s, json!({ "entrada": "e1" })).unwrap();
        assert_eq!(got, "e1 pedido, parte 1/1\ncurto, com acentuação e emoji 🙂");
        let n = chars(&big).div_ceil(PART);
        assert_eq!(n, 3);
        let mut joined = String::new();
        for part in 1..=n {
            let got = ask(s, json!({ "entrada": "E2", "parte": part })).unwrap();
            let (head, body) = got.split_once('\n').unwrap();
            assert_eq!(head, format!("e2 resposta, parte {part}/{n}"));
            let body = if part < n {
                let (body, next) = body.split_once("\n--- próxima parte: ").unwrap();
                assert_eq!(next, format!("entrada=e2 parte={} ---\n", part + 1));
                body
            } else {
                body
            };
            assert!(chars(body) <= PART);
            joined.push_str(body);
        }
        assert_eq!(joined, big);
        assert!(ask(s, json!({ "entrada": "e2", "parte": 4 })).unwrap_err().contains("não existe"));
        assert!(ask(s, json!({ "entrada": "e2", "parte": 0 })).is_err());
        assert!(ask(s, json!({ "entrada": "e99" })).unwrap_err().contains("inexistente"));
        assert!(ask(s, json!({ "entrada": "x1" })).unwrap_err().contains("inválido"));
        assert!(ask(s, json!({ "entrada": "e0" })).unwrap_err().contains("inválido"));
        assert!(ask(s, json!({ "consulta": 3 })).is_err());
    }

    #[test]
    fn sessoes_e_conversas_nao_se_enxergam() {
        let a = &session("orq-mem", "isola-a", true).0;
        let b = &session("orq-mem", "isola-b", true).0;
        prompt(a, "c1", "segredo da sessão A");
        prompt(b, "c1", "assunto da sessão B");
        assert!(ask(b, json!({ "consulta": "segredo" })).unwrap().starts_with("Nada encontrado"));
        assert_eq!(list(a)[0].text, "segredo da sessão A");
        assert_eq!(list(b)[0].text, "assunto da sessão B");
        // Duas conversas na mesma sessão: a ferramenta lê a de `atual`.
        prompt(a, "c2", "tema da conversa dois");
        assert_eq!(list(a).len(), 1);
        assert!(ask(a, json!({ "consulta": "segredo" })).unwrap().starts_with("Nada encontrado"));
        assert!(ask(a, json!({ "consulta": "conversa dois" })).unwrap().contains("e1 pedido:"));
        prompt(a, "c1", "de volta à primeira");
        assert_eq!(kinds(a), [PEDIDO, PEDIDO]);
        assert_eq!(list(a)[1].text, "de volta à primeira");
        assert!(a.join("memoria").join("c2").is_dir());
    }

    #[test]
    fn saida_do_hook_e_so_ascii_e_decodifica_igual() {
        let s = &session("orq-mem", "ascii", true).0;
        let pedido = "ação é ótima 🙂 \"aspas\" \\ e 日本";
        prompt(s, "c1", pedido);
        let got = event(s, json!({ "hook_event_name": "SessionStart", "session_id": "c1", "source": "compact" }));
        let text = ascii(&got);
        assert!(text.is_ascii(), "{text}");
        let esc = |hex: &str| format!("\\u{hex}");
        assert!(text.contains(&format!("a{}{}o {}", esc("00e7"), esc("00e3"), esc("00e9"))), "{text}");
        assert!(text.contains(&format!("{}{}", esc("d83d"), esc("de42"))), "par substituto acima do BMP: {text}");
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back, got);
        assert!(back["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains(pedido));
        assert_eq!(ascii(&json!({})), "{}");
    }

    #[test]
    fn grava_lote_literal_e_falha_em_silencio() {
        let s = &session("orq-mem", "lote", true).0;
        let tasks = ["Tarefa \"A\"\nlinha 2".to_string(), "B".to_string()];
        record(s, LOTE, &batch_text(&tasks));
        assert_eq!(list(s)[0], Entry { kind: LOTE.into(), text: "--- TAREFA 1/2 ---\nTarefa \"A\"\nlinha 2\n--- TAREFA 2/2 ---\nB\n".into() });
        let gone = session("orq-mem", "lote-sumiu", true);
        let path = gone.0.join("memoria");
        std::fs::write(&path, "arquivo no lugar da pasta").unwrap();
        record(&gone.0, RESULTADO, "x");
        assert!(list(&gone.0).is_empty());
    }
}
