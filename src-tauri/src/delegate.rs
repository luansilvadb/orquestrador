//! O programa de delegação do orquestrador, `orquestrador-delegar.exe`, que vai junto com o app. Quem o
//! chama é o adaptador `.ps1` da sessão (`adapter`), pelo nome que o orquestrador digita:
//!   orq-<id>.ps1 'tarefa A' 'tarefa B'
//!   orq-<id>.ps1 '--aguardar'
//!   orq-<id>.ps1 '--responder' 'exec-7' 'aceitar'
//!   orq-<id>.ps1 '--responder' 'exec-7' 'recusar' 'motivo'
//! O adaptador só transporta: manda ao programa, pela entrada padrão, uma linha com a pasta da sessão e cada
//! texto em base64 (`decode`), separados por `;`. Nenhum argumento leva o lote, porque a linha de comando do
//! Windows tem limite de 32767 caracteres e o base64 cresce um terço. O lote vai para a pasta da sessão de
//! uma vez: a pasta é montada com outro nome e renomeada no fim, então o app nunca pega um lote pela metade.
//! O app grava o resultado em `results\<lote>.txt`; este programa espera esse arquivo e imprime o texto em
//! UTF-8. `--aguardar` volta a esperar o lote mais recente que ainda não foi entregue, sem publicar nada. Toda
//! falha do próprio programa imprime `wrapper_error:`, fecha com `--- FIM ---` e sai com 4.
//! Quando um executor do lote para num pedido de confirmação, a espera volta na hora com código 3 e as perguntas
//! (`--- AGUARDANDO RESPOSTA ---` fecha a saída, sem `--- FIM ---`). `--responder` grava a decisão e volta a
//! esperar o lote; a pergunta respondida não volta.
use crate::core::title;
use crate::hook;
use crate::session::{command_for, owner_alive};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// O nome do programa: fica ao lado de `orquestrador.exe`, instalado ou em desenvolvimento.
pub const PROGRAM: &str = "orquestrador-delegar.exe";
/// Volta a esperar o lote mais recente ainda não entregue.
const RESUME: &str = "--aguardar";
/// Responde à pergunta mais antiga de um executor e volta a esperar o lote.
const RESPOND: &str = "--responder";
const RESPOND_USAGE: &str = "Uso: '--responder' '<executor>' 'aceitar', ou '--responder' '<executor>' 'recusar' 'motivo'.";
/// Código de saída quando a espera para por causa de uma pergunta, e não por causa do fim do lote.
const ASKING: i32 = 3;
const USAGE: &str = "Uso: o comando de delegação da sessão, com uma tarefa entre aspas simples por argumento.";
/// Código de saída quando o próprio comando falha, antes ou depois do lote.
const WRAPPER_ERROR: i32 = 4;
const POLL: Duration = Duration::from_millis(250);
const NOT_ACTIVE: &str = "A pasta não é uma sessão ativa do orquestrador.";
const DEAD: &str = "A sessão do orquestrador terminou antes do fim do lote.";

/// Ponto de entrada do programa: `args` são os argumentos depois do nome dele, que devem vir vazios, porque o
/// lote vem pela entrada padrão. Devolve o código de saída.
pub fn run(args: &[OsString]) -> i32 {
    let (code, text, marker) = match execute(args, std::io::stdin().lock()) {
        Ok(done) => (done.code, done.text, done.marker),
        Err(why) => (WRAPPER_ERROR, format!("wrapper_error: {why}\n--- FIM ---\n"), None),
    };
    let mut out = std::io::stdout().lock();
    if out.write_all(text.as_bytes()).and_then(|()| out.flush()).is_err() {
        // Sem a saída no orquestrador, o lote não conta como entregue: `--aguardar` o devolve depois.
        return WRAPPER_ERROR;
    }
    if let Some(marker) = marker {
        let _ = std::fs::write(marker, "");
    }
    code
}

/// O que sai de um lote: o código de saída, o texto para o orquestrador e a marca de entregue. A marca só
/// existe quando o texto é o resultado do lote; uma pergunta não entrega nada.
#[derive(Debug)]
struct Done {
    code: i32,
    text: String,
    marker: Option<PathBuf>,
}

fn execute(args: &[OsString], input: impl Read) -> Result<Done, String> {
    if !args.is_empty() {
        return Err("O programa não recebe argumentos: o lote vem pela entrada padrão, mandado pelo adaptador da sessão.".into());
    }
    let parts = decode(&read_line(input)?)?;
    let [session, rest @ ..] = parts.as_slice() else {
        return Err(format!("Falta a pasta da sessão. {USAGE}"));
    };
    let session = PathBuf::from(session);
    if !session.join("owner").is_file() {
        return Err(NOT_ACTIVE.into());
    }
    let id = match rest {
        [only] if only.as_str() == RESUME => resume(&session)?,
        [cmd, answer @ ..] if cmd.as_str() == RESPOND => {
            respond(&session, answer)?;
            resume(&session)?
        }
        _ => publish(&session, rest)?,
    };
    wait(&session, &id)
}

/// A linha que o adaptador manda: a pasta da sessão e as tarefas, cada texto em base64 do UTF-8, separados por
/// `;`. Base64 não tem espaço nem aspas, então nada se perde no transporte; e um texto vazio continua sendo um
/// pedaço, porque o separador o segura.
pub fn decode(line: &str) -> Result<Vec<String>, String> {
    line.split(';')
        .map(|part| {
            let bytes = B64.decode(part).map_err(|_| "entrada ilegível: um texto não está em base64.")?;
            String::from_utf8(bytes).map_err(|_| "entrada ilegível: um texto não é UTF-8.".to_string())
        })
        .collect()
}

/// A linha que o adaptador manda pela entrada padrão, lida até o fim. O Windows PowerShell 5.1 põe um BOM no
/// começo, e o PowerShell termina a linha com `\r\n`. Sem o fim de linha, o adaptador parou no meio, e o lote
/// não pode ser publicado pela metade. O conteúdo não é alterado: é base64, que só tem ASCII.
fn read_line(mut input: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).map_err(|e| format!("Não foi possível ler a entrada padrão: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "entrada ilegível: não é UTF-8.".to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    if text.trim().is_empty() {
        return Err("A entrada padrão veio vazia: o adaptador não mandou o lote.".into());
    }
    let line = text.strip_suffix('\n').ok_or("entrada incompleta: o adaptador não terminou de mandar o lote.")?;
    Ok(line.trim_end_matches('\r').to_string())
}

/// Publica as tarefas como um lote novo, na ordem dos argumentos, exatamente como vieram.
fn publish(session: &Path, tasks: &[String]) -> Result<String, String> {
    if tasks.is_empty() {
        return Err(format!("Nenhuma tarefa. Cada tarefa vai como um argumento entre aspas simples, depois do comando. {USAGE}"));
    }
    if tasks.iter().any(|task| task.trim().is_empty()) {
        return Err("Tarefa vazia.".into());
    }
    let id = hook::unique();
    let requests = session.join("requests");
    let tmp = requests.join(format!("{id}.tmp"));
    let io = |e: std::io::Error| format!("Não foi possível publicar o lote em {}: {e}", requests.display());
    std::fs::create_dir(&tmp).map_err(io)?;
    for (n, task) in tasks.iter().enumerate() {
        std::fs::write(tmp.join(format!("{}.txt", n + 1)), task).map_err(io)?;
    }
    std::fs::rename(&tmp, requests.join(&id)).map_err(io)?;
    Ok(id)
}

/// O lote mais recente que ainda não foi entregue.
fn resume(session: &Path) -> Result<String, String> {
    let results = session.join("results");
    let mut pending: Vec<String> = std::fs::read_dir(session.join("requests"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|id| !id.ends_with(".tmp") && !results.join(format!("{id}.delivered")).exists())
        .collect();
    pending.sort();
    pending.pop().ok_or_else(|| "Nenhum lote aguardando entrega.".to_string())
}

/// Espera o resultado do lote e separa o código de saída, que é a primeira linha. Se a sessão
/// morrer antes, falha em vez de esperar para sempre. Uma pergunta pendente de um executor do lote
/// interrompe a espera na hora, com código `ASKING` e sem marca de entregue: o resultado só sai quando
/// nenhum executor espera o orquestrador.
fn wait(session: &Path, id: &str) -> Result<Done, String> {
    let result = session.join("results").join(format!("{id}.txt"));
    loop {
        if result.is_file() {
            let text = std::fs::read_to_string(&result).map_err(|e| format!("Não foi possível ler {}: {e}", result.display()))?;
            let (code, body) = text.split_once('\n').ok_or("o resultado do lote não traz o código de saída.")?;
            let code: i32 = code.trim().parse().map_err(|_| format!("código de saída ilegível no resultado do lote: {code:?}"))?;
            let marker = session.join("results").join(format!("{id}.delivered"));
            return Ok(Done { code, text: body.to_string(), marker: Some(marker) });
        }
        if !owner_alive(session) {
            return Err(DEAD.into());
        }
        let asks: Vec<Ask> = pending_asks(session).into_iter().filter(|a| a.batch == id).collect();
        if !asks.is_empty() {
            return Ok(Done { code: ASKING, text: questions(session, &asks), marker: None });
        }
        std::thread::sleep(POLL);
    }
}

/// Um pedido de confirmação que um executor deixou sem resposta: de que lote e de que tarefa é, e o pedido como
/// o CLI o mandou ao `--ask`.
#[derive(Debug)]
struct Ask {
    node: String,
    batch: String,
    index: usize,
    size: usize,
    id: String,
    dir: PathBuf,
    payload: String,
}

/// Os pedidos sem resposta de todos os executores da sessão, por lote, por tarefa e por chegada. Um executor
/// sem `batch.json` não é de lote nenhum e fica de fora.
fn pending_asks(session: &Path) -> Vec<Ask> {
    let mut asks = Vec::new();
    for node in std::fs::read_dir(session.join("exec")).into_iter().flatten().flatten() {
        let dir = node.path();
        let Some(meta) = std::fs::read_to_string(dir.join("batch.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
        let (Some(batch), Some(index), Some(size)) = (meta["batch"].as_str(), meta["index"].as_u64(), meta["size"].as_u64()) else { continue };
        let ask_dir = dir.join("ask");
        for id in hook::pending_asks(&ask_dir) {
            // Um pedido que já foi respondido e limpado entre a listagem e a leitura não existe mais.
            let Ok(payload) = std::fs::read_to_string(ask_dir.join(format!("{id}.json"))) else { continue };
            asks.push(Ask {
                node: node.file_name().to_string_lossy().into_owned(),
                batch: batch.to_string(),
                index: index as usize,
                size: size as usize,
                id,
                dir: ask_dir.clone(),
                payload,
            });
        }
    }
    asks.sort_by(|a, b| (&a.batch, a.index, &a.id).cmp(&(&b.batch, b.index, &b.id)));
    asks
}

/// A saída de espera com perguntas: uma seção por pergunta, a linha que ensina a responder com o nome real do
/// comando da sessão, e `--- AGUARDANDO RESPOSTA ---` como última linha.
fn questions(session: &Path, asks: &[Ask]) -> String {
    let mut out = String::new();
    for ask in asks {
        let task = std::fs::read_to_string(session.join("requests").join(&ask.batch).join(format!("{}.txt", ask.index + 1))).unwrap_or_default();
        let (tool, input) = describe(&ask.payload);
        out += &format!(
            "--- PERGUNTA {} ({}/{}) ---\n{}\nO executor parou num pedido de confirmação e espera a sua resposta.\n{tool}: {input}\n",
            ask.node,
            ask.index + 1,
            ask.size,
            title(&task)
        );
    }
    let command = command_for(&session.file_name().unwrap_or_default().to_string_lossy());
    out += &format!(
        "Para responder: {command} '--responder' '<executor>' 'aceitar', ou {command} '--responder' '<executor>' 'recusar' 'motivo'. <executor> é o de cada pergunta acima.\n"
    );
    out += "--- AGUARDANDO RESPOSTA ---\n";
    out
}

/// A ferramenta e a entrada que o pedido traz: o comando inteiro quando a ferramenta é um shell, senão o JSON da
/// entrada. Um pedido que não é JSON sai como veio.
fn describe(payload: &str) -> (String, String) {
    let Ok(call) = serde_json::from_str::<Value>(payload) else {
        return ("ferramenta desconhecida".into(), payload.to_string());
    };
    let tool = call["tool_name"].as_str().unwrap_or("ferramenta desconhecida").to_string();
    let input = match &call["tool_input"]["command"] {
        Value::String(command) => command.clone(),
        _ => call["tool_input"].to_string(),
    };
    (tool, input)
}

/// `--responder <executor> aceitar` ou `--responder <executor> recusar <motivo>`: grava a decisão da pergunta
/// mais antiga desse executor como `<id>.reply`, que o `--ask` que espera lê, e o executor segue. Erro sem nada
/// gravado se a resposta não existe, se falta o motivo ou se o executor não tem pergunta pendente.
fn respond(session: &Path, args: &[String]) -> Result<(), String> {
    let [node, choice, reason @ ..] = args else { return Err(RESPOND_USAGE.into()) };
    let decision = match (choice.as_str(), reason) {
        ("aceitar", []) => json!({ "behavior": "allow" }),
        ("recusar", [why]) if !why.trim().is_empty() => json!({ "behavior": "deny", "message": why }),
        ("recusar", _) => return Err("recusar pede um motivo, não vazio: ele vai ao executor como a mensagem de erro.".into()),
        ("aceitar", _) => return Err("aceitar não recebe motivo.".into()),
        _ => return Err(format!("resposta desconhecida {choice:?}: use aceitar ou recusar.")),
    };
    let ask = pending_asks(session)
        .into_iter()
        .find(|a| a.node == *node)
        .ok_or_else(|| format!("O executor {node} não tem pergunta pendente."))?;
    hook::put(&ask.dir, &format!("{}.reply", ask.id), decision.to_string().as_bytes())
        .map_err(|e| format!("Não foi possível gravar a resposta em {}: {e}", ask.dir.display()))
}

/// O adaptador `.ps1` da sessão. Só transporta: cada texto vai ao programa como base64 do UTF-8, numa linha
/// pela entrada padrão, e o processo sai com o código que o programa devolve. Não decide nada.
///
/// Por que existe: o Windows PowerShell 5.1 altera o texto que passa a um programa nativo (medido: a aspa
/// dupla pura some, `\"` vira `"`, `a\\"b` vira `a\b`, e a barra no fim de uma tarefa com espaço vira aspa).
/// Não há grafia que o orquestrador escreva e que chegue igual nos dois shells. Argumento de script chega
/// exato nos dois. Medido nos dois shells: 17 de 17 textos iguais, com o caminho da pasta com espaço, acento
/// e apóstrofo.
///
/// Por que a entrada padrão: como argumento, o lote estoura o limite de 32767 caracteres da linha de comando do
/// Windows. Medido: três tarefas de 9000 caracteres falham com "o nome do arquivo ou a extensão é muito grande"
/// antes de o programa rodar. Pela entrada não há esse limite.
///
/// Por que `[Environment]::Exit`: com `-Command`, o `exit` do script não vira o código do processo: o código 4
/// do programa saía como 1, nos dois shells. Só o encerramento explícito entrega o código certo em `-Command` e
/// em `-File`. A preferência `$PSNativeCommandUseErrorActionPreference` fica desligada, para o perfil do usuário
/// (pwsh 7.4 em diante) não transformar um código diferente de zero em erro.
pub fn adapter(program: &Path, session: &Path) -> Result<String, String> {
    let literal = |p: &Path| {
        p.to_str()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .ok_or_else(|| format!("O caminho não é texto Unicode: {}", p.display()))
    };
    Ok(format!(
        "$ErrorActionPreference = 'Stop'\r\n\
         $PSNativeCommandUseErrorActionPreference = $false\r\n\
         $program = {}\r\n\
         $session = {}\r\n\
         $tokens = @([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($session)))\r\n\
         foreach ($arg in $args) {{ $tokens += [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string]$arg)) }}\r\n\
         ($tokens -join ';') | & $program\r\n\
         [Environment]::Exit($LASTEXITCODE)\r\n",
        literal(program)?,
        literal(session)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::start_time;

    /// Uma pasta de sessão cujo dono é este processo (vivo) ou um processo que já não existe.
    fn session(tag: &str, alive: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orq-delegar-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["requests", "results", "exec"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let me = std::process::id();
        let started = start_time(me).unwrap() + u64::from(!alive);
        std::fs::write(dir.join("owner"), format!("{me} {started} strategist-{tag}")).unwrap();
        dir
    }

    /// A linha que o adaptador manda pela entrada padrão, com o fim de linha que o PowerShell acrescenta.
    fn encode(parts: &[&str]) -> String {
        let token: Vec<String> = parts.iter().map(|p| B64.encode(p)).collect();
        format!("{}\r\n", token.join(";"))
    }

    /// Um leitor que falha: o erro de leitura vai para o `wrapper_error`, com a causa.
    struct Quebrada;

    impl Read for Quebrada {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disco sumiu"))
        }
    }

    #[test]
    fn lote_sai_inteiro_e_renomeado_com_o_texto_exato() {
        let dir = session("publica", true);
        let tasks = ["Tarefa A\nlinha 2 d'água", "Diga \"oi\" e \\\"tchau\\\" e a barra \\", "{\"a\":\"b\\\"c\"}"];
        let id = publish(&dir, &tasks.map(String::from)).unwrap();
        let batch = dir.join("requests").join(&id);
        for (n, task) in tasks.iter().enumerate() {
            assert_eq!(std::fs::read_to_string(batch.join(format!("{}.txt", n + 1))).unwrap(), *task);
        }
        let names: Vec<String> = std::fs::read_dir(dir.join("requests")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, [id]);
    }

    #[test]
    fn tarefa_invalida_nao_publica_nada() {
        let dir = session("invalida", true);
        assert!(publish(&dir, &[]).unwrap_err().contains("Nenhuma tarefa"));
        assert!(publish(&dir, &["ok".into(), "  \n".into()]).unwrap_err().contains("Tarefa vazia"));
        assert_eq!(std::fs::read_dir(dir.join("requests")).unwrap().count(), 0);
    }

    #[test]
    fn aguardar_pega_o_lote_mais_recente_que_nao_foi_entregue() {
        let dir = session("retoma", true);
        for id in ["00000000000000000001-aaaaaaaa", "00000000000000000002-bbbbbbbb", "00000000000000000003-cccccccc", "00000000000000000004-dddddddd.tmp"] {
            std::fs::create_dir_all(dir.join("requests").join(id)).unwrap();
        }
        std::fs::write(dir.join("results").join("00000000000000000003-cccccccc.delivered"), "").unwrap();
        assert_eq!(resume(&dir).unwrap(), "00000000000000000002-bbbbbbbb");
        for id in ["00000000000000000001-aaaaaaaa", "00000000000000000002-bbbbbbbb"] {
            std::fs::write(dir.join("results").join(format!("{id}.delivered")), "").unwrap();
        }
        assert_eq!(resume(&dir).unwrap_err(), "Nenhum lote aguardando entrega.");
    }

    #[test]
    fn espera_separa_o_codigo_de_saida_do_texto() {
        let dir = session("espera", true);
        std::fs::write(dir.join("results").join("x.txt"), "1\n--- EXECUTOR 1/1 ---\nok\n").unwrap();
        let done = wait(&dir, "x").unwrap();
        assert_eq!((done.code, done.text.as_str()), (1, "--- EXECUTOR 1/1 ---\nok\n"));
        assert!(done.marker.is_some_and(|m| m.ends_with("x.delivered")));
    }

    /// Um lote de `size` tarefas, com o executor `node` na tarefa `index` e um pedido sem resposta com `payload`.
    /// Devolve o id do pedido.
    fn batch_with_ask(dir: &Path, batch: &str, node: &str, index: usize, size: usize, payload: &str) -> String {
        let req = dir.join("requests").join(batch);
        std::fs::create_dir_all(&req).unwrap();
        for n in 1..=size {
            std::fs::write(req.join(format!("{n}.txt")), format!("Tarefa {n}\nescopo")).unwrap();
        }
        let node_dir = dir.join("exec").join(node);
        std::fs::create_dir_all(node_dir.join("ask")).unwrap();
        std::fs::write(node_dir.join("batch.json"), json!({ "batch": batch, "index": index, "size": size }).to_string()).unwrap();
        let id = "00000000000000000001-aaaaaaaa";
        std::fs::write(node_dir.join("ask").join(format!("{id}.json")), payload).unwrap();
        id.to_string()
    }

    const BASH: &str = r#"{"tool_name":"Bash","tool_input":{"command":"rm /d/a.log\nlinha 2","description":"x"}}"#;

    /// A pergunta de um executor do lote interrompe a espera com código 3: a seção com o título, o ponto da
    /// tarefa e o comando inteiro, a linha que ensina a responder, e `--- AGUARDANDO RESPOSTA ---` como última
    /// linha. Pergunta de outro lote fica de fora.
    #[test]
    fn pergunta_interrompe_a_espera_com_a_secao_e_a_linha_final() {
        let dir = session("pergunta", true);
        batch_with_ask(&dir, "B1", "exec-1", 0, 2, BASH);
        batch_with_ask(&dir, "B2", "exec-2", 0, 1, BASH);
        let asked = wait(&dir, "B1").unwrap();
        assert_eq!((asked.code, asked.marker.is_none()), (ASKING, true), "a pergunta devia interromper a espera");
        let text = asked.text;
        assert!(text.starts_with(
            "--- PERGUNTA exec-1 (1/2) ---\nTarefa 1\nO executor parou num pedido de confirmação e espera a sua resposta.\nBash: rm /d/a.log\nlinha 2\nPara responder: "
        ));
        assert!(text.contains("'--responder' '<executor>' 'aceitar'") && text.contains("'--responder' '<executor>' 'recusar' 'motivo'"));
        assert!(text.ends_with("\n--- AGUARDANDO RESPOSTA ---\n") && text.matches("--- AGUARDANDO RESPOSTA ---").count() == 1);
        assert!(!text.contains("--- FIM ---") && !text.contains("exec-2"));
    }

    /// A linha de resposta leva o nome real do comando da sessão, que é o da pasta `strategist-<id>`.
    #[test]
    fn linha_de_resposta_usa_o_nome_do_comando_da_sessao() {
        let dir = std::env::temp_dir().join(format!("orq-delegar-{}-nome", std::process::id())).join("strategist-0123456789abcdef0123456789abcdef");
        std::fs::create_dir_all(dir.join("exec").join("exec-1").join("ask")).unwrap();
        let id = "00000000000000000001-aaaaaaaa";
        std::fs::write(dir.join("exec").join("exec-1").join("batch.json"), json!({ "batch": "B", "index": 0, "size": 1 }).to_string()).unwrap();
        std::fs::write(dir.join("exec").join("exec-1").join("ask").join(format!("{id}.json")), BASH).unwrap();
        let asks = pending_asks(&dir);
        let text = questions(&dir, &asks);
        assert!(text.contains("Para responder: orq-0123456789abcdef0123456789abcdef.ps1 '--responder' '<executor>' 'aceitar'"), "{text}");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    /// Pedido respondido, ou nó que não é de lote, não é pergunta pendente.
    #[test]
    fn respondida_ou_sem_lote_nao_e_pergunta() {
        let dir = session("respondida", true);
        let id = batch_with_ask(&dir, "B1", "exec-1", 0, 1, BASH);
        let ask_dir = dir.join("exec").join("exec-1").join("ask");
        // Um executor sem batch.json (não é de lote nenhum) com um pedido sem resposta.
        std::fs::create_dir_all(dir.join("exec").join("exec-9").join("ask")).unwrap();
        std::fs::write(dir.join("exec").join("exec-9").join("ask").join(format!("{id}.json")), BASH).unwrap();
        std::fs::write(ask_dir.join(format!("{id}.reply")), r#"{"behavior":"allow"}"#).unwrap();
        assert!(pending_asks(&dir).is_empty(), "com resposta gravada, e sem batch.json, nada está pendente");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--responder` grava a decisão como `<id>.reply` do executor, e a pergunta deixa de estar pendente.
    /// Sem pergunta, com resposta inválida ou com `recusar` sem motivo, nada é gravado.
    #[test]
    fn responder_grava_a_decisao_ou_recusa_sem_gravar_nada() {
        let dir = session("responder", true);
        let id = batch_with_ask(&dir, "B1", "exec-1", 0, 1, BASH);
        let answer = dir.join("exec").join("exec-1").join("ask").join(format!("{id}.reply"));
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        assert!(respond(&dir, &s(&["exec-7", "aceitar"])).unwrap_err().contains("não tem pergunta pendente"));
        assert!(respond(&dir, &s(&["exec-1", "talvez"])).unwrap_err().contains("resposta desconhecida"));
        assert!(respond(&dir, &s(&["exec-1", "recusar"])).unwrap_err().contains("recusar pede um motivo"));
        assert!(respond(&dir, &s(&["exec-1", "recusar", "   "])).unwrap_err().contains("recusar pede um motivo"));
        assert!(respond(&dir, &s(&["exec-1", "aceitar", "por quê"])).unwrap_err().contains("aceitar não recebe motivo"));
        assert!(respond(&dir, &s(&["exec-1"])).is_err());
        assert!(!answer.exists(), "erro não grava nada");
        respond(&dir, &s(&["exec-1", "recusar", "use só o arquivo que você criou"])).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&answer).unwrap()).unwrap();
        assert_eq!(written, json!({ "behavior": "deny", "message": "use só o arquivo que você criou" }));
        assert!(pending_asks(&dir).is_empty(), "respondida não volta como pendente");
        assert!(respond(&dir, &s(&["exec-1", "aceitar"])).unwrap_err().contains("não tem pergunta pendente"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--aguardar` com pergunta sem resposta devolve a pergunta com código 3 e sem marca de entregue; `--responder`
    /// responde e volta a esperar o lote, que aqui já tem resultado.
    #[test]
    fn execute_devolve_a_pergunta_com_codigo_3_e_responder_volta_a_esperar_o_lote() {
        let dir = session("execute", true);
        batch_with_ask(&dir, "B1", "exec-1", 0, 1, BASH);
        let asked = execute(&[], encode(&[dir.to_str().unwrap(), "--aguardar"]).as_bytes()).unwrap();
        assert_eq!((asked.code, asked.marker.is_none()), (ASKING, true));
        assert!(asked.text.contains("--- PERGUNTA exec-1 (1/1) ---") && asked.text.ends_with("--- AGUARDANDO RESPOSTA ---\n"));
        // O lote termina enquanto o executor espera a resposta; `--responder` a grava e recebe o resultado.
        std::fs::write(dir.join("results").join("B1.txt"), "0\n--- EXECUTOR 1/1 ---\naceito\n").unwrap();
        let done = execute(&[], encode(&[dir.to_str().unwrap(), "--responder", "exec-1", "aceitar"]).as_bytes()).unwrap();
        assert_eq!((done.code, done.text.as_str()), (0, "--- EXECUTOR 1/1 ---\naceito\n"));
        assert!(done.marker.is_some());
        let answer = dir.join("exec").join("exec-1").join("ask").join("00000000000000000001-aaaaaaaa.reply");
        assert_eq!(std::fs::read_to_string(answer).unwrap(), r#"{"behavior":"allow"}"#);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--responder` com argumentos errados fecha com `wrapper_error` e código 4, como os outros erros do comando.
    #[test]
    fn responder_com_erro_fecha_com_wrapper_error() {
        let dir = session("responder-erro", true);
        let err = execute(&[], encode(&[dir.to_str().unwrap(), "--responder", "exec-1", "aceitar"]).as_bytes()).unwrap_err();
        assert!(err.contains("não tem pergunta pendente"), "{err}");
        assert!(execute(&[], encode(&[dir.to_str().unwrap(), "--responder"]).as_bytes()).unwrap_err().contains("Uso:"));
    }

    #[test]
    fn sessao_morta_com_lote_pendente_falha_em_vez_de_esperar() {
        let dir = session("morta", false);
        assert_eq!(wait(&dir, "nunca").unwrap_err(), DEAD);
    }

    #[test]
    fn pasta_que_nao_e_sessao_e_recusada_e_sem_tarefa_tambem() {
        let dir = session("sem-tarefa", true);
        assert!(execute(&[], encode(&[dir.to_str().unwrap(), "--aguardar"]).as_bytes()).unwrap_err().contains("Nenhum lote"));
        assert!(execute(&[], encode(&[dir.to_str().unwrap()]).as_bytes()).unwrap_err().contains("Nenhuma tarefa"));
        let nowhere = std::env::temp_dir().join(format!("orq-delegar-{}-inexistente", std::process::id()));
        assert_eq!(execute(&[], encode(&[nowhere.to_str().unwrap(), "x"]).as_bytes()).unwrap_err(), NOT_ACTIVE);
        assert!(execute(&[OsString::from("x")], encode(&[dir.to_str().unwrap(), "x"]).as_bytes()).unwrap_err().contains("não recebe argumentos"));
        assert!(execute(&[], &b"\r\n"[..]).unwrap_err().contains("veio vazia"));
    }

    /// Pela entrada padrão, o lote passa inteiro. O BOM do Windows PowerShell 5.1 e o fim de linha saem, e o resto
    /// volta exato. Um lote maior que a linha de comando do Windows (32767 caracteres) passa sem cortar nada.
    #[test]
    fn lote_maior_que_a_linha_de_comando_passa_pela_entrada_inteiro() {
        let mut all = vec!["sessão".to_string()];
        all.extend((1..=4).map(|n| format!("Tarefa {n}: ação, \"aspas\" e barra \\\n").repeat(1000)));
        let line = encode(&all.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(line.len() > 32767, "{}", line.len());
        let with_bom = format!("\u{feff}{line}");
        assert_eq!(decode(&read_line(with_bom.as_bytes()).unwrap()).unwrap(), all);
    }

    /// Entrada vazia, cortada ou ilegível não passa do adaptador: vira erro com o motivo, e nada é publicado.
    #[test]
    fn entrada_vazia_cortada_ou_ilegivel_nao_passa_do_adaptador() {
        assert!(read_line(&b""[..]).unwrap_err().contains("veio vazia"));
        assert!(read_line(&b"\xEF\xBB\xBF\r\n"[..]).unwrap_err().contains("veio vazia"));
        assert!(read_line(&b"c2Vzc2Fv;YQ=="[..]).unwrap_err().contains("incompleta"));
        assert!(read_line(&b"\xFF\xFE\r\n"[..]).unwrap_err().contains("não é UTF-8"));
        assert!(execute(&[], &b"c2Vzc2Fv;!!!\r\n"[..]).unwrap_err().contains("não está em base64"));
        let why = execute(&[], Quebrada).unwrap_err();
        assert!(why.starts_with("Não foi possível ler a entrada padrão") && why.contains("disco sumiu"), "{why}");
    }

    /// O que vai pela linha de comando volta igual: aspas puras, barra antes da aspa, JSON com escapes,
    /// barra final, acentos, apóstrofo, multilinha, texto vazio e separador dentro do texto.
    #[test]
    fn texto_passa_pelo_transporte_sem_perda() {
        let dificeis = [
            "Diga \"oi\"",
            "Diga \\\"oi\\\"",
            r#"{"a": "b\"c", "d": "e\\"}"#,
            "pasta C:\\Users\\Maria Silva\\",
            "acentos ação, coração, d'água, ''duplo''",
            "linha 1\nlinha 2 \"x\"\n",
            "com ; ponto e vírgula",
            "\"",
            "",
            "  espaços nas pontas  ",
        ];
        let mut all = vec!["sessão"];
        all.extend(dificeis);
        let line = read_line(encode(&all).as_bytes()).unwrap();
        assert_eq!(decode(&line).unwrap(), all);
    }

    #[test]
    fn nome_do_lote_ordena_pelo_tempo() {
        let first = hook::unique();
        std::thread::sleep(Duration::from_millis(2));
        let second = hook::unique();
        assert!(first < second, "{first} < {second}");
        assert_eq!(first.as_bytes()[32], b'-');
    }

    #[test]
    fn adaptador_leva_o_programa_e_a_pasta_com_apostrofo_e_acento() {
        let script = adapter(Path::new(r"C:\Users\Maria Silva\it's\ação\orquestrador-delegar.exe"), Path::new(r"C:\s\strategist-x")).unwrap();
        assert!(script.starts_with("$ErrorActionPreference = 'Stop'\r\n$PSNativeCommandUseErrorActionPreference = $false\r\n"));
        assert!(script.contains("$program = 'C:\\Users\\Maria Silva\\it''s\\ação\\orquestrador-delegar.exe'\r\n"), "{script}");
        assert!(script.contains("$session = 'C:\\s\\strategist-x'\r\n"));
        // O lote vai pela entrada padrão: nenhum argumento leva o texto ao programa.
        assert!(script.contains("($tokens -join ';') | & $program\r\n"), "{script}");
        assert!(!script.contains("& $program ("), "{script}");
        // O processo sai com o código do programa; o `exit` do script, dentro de `-Command`, não chega lá.
        assert!(script.ends_with("[Environment]::Exit($LASTEXITCODE)\r\n"), "{script}");
    }

    /// O contrato de distribuição: o programa vai no instalador, ao lado do app, e é compilado antes do
    /// dev e do build de release. Se um desses pedaços sumir, a delegação quebra só na instalação.
    #[test]
    fn programa_de_delegacao_vai_junto_com_o_app_e_e_compilado_antes() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let package: serde_json::Value = serde_json::from_str(include_str!("../../package.json")).unwrap();
        assert_eq!(conf["bundle"]["resources"]["target/delegar/"], "", "a pasta vai para a raiz do instalador, ao lado do app");
        assert_eq!(conf["build"]["beforeDevCommand"], "npm run dev:tauri");
        assert_eq!(conf["build"]["beforeBuildCommand"], "npm run build:tauri");
        // A cópia para a pasta do instalador acontece depois de compilar tudo, e falha se o programa de release faltar.
        // Antes disso a pasta não tem o programa, para o `tauri-build` não copiar um de outro perfil para o dev.
        assert_eq!(conf["build"]["beforeBundleCommand"], "npm run copy:delegar");
        assert!(package["scripts"]["copy:delegar"].as_str().unwrap().contains("src-tauri/target/release/orquestrador-delegar.exe"));
        assert!(package["scripts"]["dev:tauri"].as_str().unwrap().contains("cargo build --manifest-path src-tauri/Cargo.toml --bin orquestrador-delegar"));
        assert!(package["scripts"]["build:tauri"].as_str().unwrap().contains("cargo build --release --manifest-path src-tauri/Cargo.toml --bin orquestrador-delegar"));
    }
}
