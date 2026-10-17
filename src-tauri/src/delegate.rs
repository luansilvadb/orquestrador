//! O lote do orquestrador: publica as tarefas na pasta da sessão, espera o resultado que o app grava e responde
//! às perguntas dos executores. Quem chama é o servidor MCP (`mcp.rs`): `delegar` publica e espera, `aguardar`
//! volta a esperar o lote mais recente ainda não entregue, e `responder` grava a decisão e volta a esperar.
//! O app lê `requests\` e grava `results\<lote>.txt`; este módulo nunca escreve o resultado. A marca
//! `results\<lote>.delivered` é gravada por quem chama, só depois que a resposta saiu: sem ela, `aguardar`
//! pega o lote de novo.
use crate::core::title;
use crate::hook;
use crate::session::owner_alive;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Quanto uma chamada espera o lote antes de devolver `--- EM ANDAMENTO ---`.
pub const WINDOW: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_millis(250);
const NOT_ACTIVE: &str = "A pasta não é uma sessão ativa do orquestrador.";
const DEAD: &str = "A sessão do orquestrador terminou antes do fim do lote.";

/// O que a espera devolve: o resultado do lote, com a marca que só deve ser gravada depois da entrega; as
/// perguntas de um executor do lote, que param a espera e não entregam nada; ou o prazo sem resultado.
#[derive(Debug)]
pub enum Waited {
    Result { text: String, marker: PathBuf },
    Asking(String),
    Running,
}

/// A pasta é uma sessão ativa: o app a criou e declara o dono em `owner`. Sem isso, nenhuma ferramenta age.
pub fn check_session(session: &Path) -> Result<(), String> {
    if session.join("owner").is_file() {
        Ok(())
    } else {
        Err(NOT_ACTIVE.into())
    }
}

/// Publica as tarefas como um lote novo, na ordem em que vieram, exatamente como vieram.
pub fn publish(session: &Path, tasks: &[String]) -> Result<String, String> {
    if tasks.is_empty() {
        return Err("Nenhuma tarefa: `tarefas` precisa ter pelo menos um texto.".into());
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
pub fn resume(session: &Path) -> Result<String, String> {
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

/// Espera o resultado do lote por até `window`. A espera acaba de um de três jeitos: com o resultado (a primeira
/// linha do arquivo é o código de saída do app, e não segue ao orquestrador), com as perguntas de um executor do
/// lote, ou com o prazo. Se a sessão morrer antes, falha em vez de esperar para sempre.
pub fn wait(session: &Path, id: &str, window: Duration) -> Result<Waited, String> {
    let result = session.join("results").join(format!("{id}.txt"));
    let started = Instant::now();
    loop {
        if result.is_file() {
            let text = std::fs::read_to_string(&result).map_err(|e| format!("Não foi possível ler {}: {e}", result.display()))?;
            let (code, body) = text.split_once('\n').ok_or("o resultado do lote não traz o código de saída.")?;
            code.trim().parse::<i32>().map_err(|_| format!("código de saída ilegível no resultado do lote: {code:?}"))?;
            let marker = session.join("results").join(format!("{id}.delivered"));
            return Ok(Waited::Result { text: body.to_string(), marker });
        }
        if !owner_alive(session) {
            return Err(DEAD.into());
        }
        let asks: Vec<Ask> = pending_asks(session).into_iter().filter(|a| a.batch == id).collect();
        if !asks.is_empty() {
            return Ok(Waited::Asking(questions(session, &asks)));
        }
        if started.elapsed() >= window {
            return Ok(Waited::Running);
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

/// A saída de espera com perguntas: uma seção por pergunta, a linha que ensina a responder, e
/// `--- AGUARDANDO RESPOSTA ---` como última linha.
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
    out += "Para responder: chame responder com o executor, aceitar ou recusar e, ao recusar, o motivo; o executor é o de cada pergunta acima.\n";
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

/// Grava a decisão da pergunta mais antiga do executor `node` como `<id>.reply`, que o `--ask` que espera lê, e
/// o executor segue. `reason` é o motivo, se veio: `recusar` o pede, `aceitar` não o recebe. Erro sem nada gravado
/// se a escolha não existe, se o motivo não bate com ela ou se o executor não tem pergunta pendente.
pub fn respond(session: &Path, node: &str, choice: &str, reason: Option<&str>) -> Result<(), String> {
    let decision = match (choice, reason) {
        ("aceitar", None) => json!({ "behavior": "allow" }),
        ("aceitar", Some(_)) => return Err("aceitar não recebe motivo.".into()),
        ("recusar", Some(why)) if !why.trim().is_empty() => json!({ "behavior": "deny", "message": why }),
        ("recusar", _) => return Err("recusar pede um motivo, não vazio: ele vai ao executor como a mensagem de erro.".into()),
        _ => return Err(format!("resposta desconhecida {choice:?}: use aceitar ou recusar.")),
    };
    let ask = pending_asks(session)
        .into_iter()
        .find(|a| a.node == node)
        .ok_or_else(|| format!("O executor {node} não tem pergunta pendente. Chame aguardar para continuar esperando o lote."))?;
    hook::put(&ask.dir, &format!("{}.reply", ask.id), decision.to_string().as_bytes())
        .map_err(|e| format!("Não foi possível gravar a resposta em {}: {e}", ask.dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{batch_with_ask, session};

    #[test]
    fn lote_sai_inteiro_e_renomeado_com_o_texto_exato() {
        let dir = session("orq-delegar", "publica", true);
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
        let dir = session("orq-delegar", "invalida", true);
        assert!(publish(&dir, &[]).unwrap_err().contains("Nenhuma tarefa"));
        assert!(publish(&dir, &["ok".into(), "  \n".into()]).unwrap_err().contains("Tarefa vazia"));
        assert_eq!(std::fs::read_dir(dir.join("requests")).unwrap().count(), 0);
    }

    #[test]
    fn aguardar_pega_o_lote_mais_recente_que_nao_foi_entregue() {
        let dir = session("orq-delegar", "retoma", true);
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
        let dir = session("orq-delegar", "espera", true);
        std::fs::write(dir.join("results").join("x.txt"), "1\n--- EXECUTOR 1/1 ---\nok\n").unwrap();
        let Waited::Result { text, marker } = wait(&dir, "x", WINDOW).unwrap() else { panic!("o resultado devia sair") };
        assert_eq!(text, "--- EXECUTOR 1/1 ---\nok\n");
        assert!(marker.ends_with("x.delivered"));
    }

    const BASH: &str = r#"{"tool_name":"Bash","tool_input":{"command":"rm /d/a.log\nlinha 2","description":"x"}}"#;

    /// A pergunta de um executor do lote interrompe a espera: a seção com o título, o ponto da tarefa e o comando
    /// inteiro, a linha que ensina a responder, e `--- AGUARDANDO RESPOSTA ---` como última linha. Pergunta de
    /// outro lote fica de fora, e nada é entregue.
    #[test]
    fn pergunta_interrompe_a_espera_com_a_secao_e_a_linha_final() {
        let dir = session("orq-delegar", "pergunta", true);
        batch_with_ask(&dir, "B1", "exec-1", 0, 2, BASH);
        batch_with_ask(&dir, "B2", "exec-2", 0, 1, BASH);
        let Waited::Asking(text) = wait(&dir, "B1", WINDOW).unwrap() else { panic!("a pergunta devia interromper a espera") };
        assert!(text.starts_with(
            "--- PERGUNTA exec-1 (1/2) ---\nTarefa 1\nO executor parou num pedido de confirmação e espera a sua resposta.\nBash: rm /d/a.log\nlinha 2\n\
             Para responder: chame responder com o executor, aceitar ou recusar e, ao recusar, o motivo; o executor é o de cada pergunta acima.\n"
        ), "{text}");
        assert!(text.ends_with("\n--- AGUARDANDO RESPOSTA ---\n") && text.matches("--- AGUARDANDO RESPOSTA ---").count() == 1);
        assert!(!text.contains("--- FIM ---") && !text.contains("exec-2") && !text.contains("orq-"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pedido respondido, ou nó que não é de lote, não é pergunta pendente.
    #[test]
    fn respondida_ou_sem_lote_nao_e_pergunta() {
        let dir = session("orq-delegar", "respondida", true);
        let id = batch_with_ask(&dir, "B1", "exec-1", 0, 1, BASH);
        let ask_dir = dir.join("exec").join("exec-1").join("ask");
        // Um executor sem batch.json (não é de lote nenhum) com um pedido sem resposta.
        std::fs::create_dir_all(dir.join("exec").join("exec-9").join("ask")).unwrap();
        std::fs::write(dir.join("exec").join("exec-9").join("ask").join(format!("{id}.json")), BASH).unwrap();
        std::fs::write(ask_dir.join(format!("{id}.reply")), r#"{"behavior":"allow"}"#).unwrap();
        assert!(pending_asks(&dir).is_empty(), "com resposta gravada, e sem batch.json, nada está pendente");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `respond` grava a decisão como `<id>.reply` do executor, e a pergunta deixa de estar pendente. Escolha
    /// desconhecida, motivo que não bate com a escolha ou executor sem pergunta não gravam nada.
    #[test]
    fn responder_grava_a_decisao_ou_recusa_sem_gravar_nada() {
        let dir = session("orq-delegar", "responder", true);
        let id = batch_with_ask(&dir, "B1", "exec-1", 0, 1, BASH);
        let answer = dir.join("exec").join("exec-1").join("ask").join(format!("{id}.reply"));
        assert!(respond(&dir, "exec-7", "aceitar", None).unwrap_err().contains("não tem pergunta pendente"));
        assert!(respond(&dir, "exec-1", "talvez", None).unwrap_err().contains("resposta desconhecida"));
        assert!(respond(&dir, "exec-1", "recusar", None).unwrap_err().contains("recusar pede um motivo"));
        assert!(respond(&dir, "exec-1", "recusar", Some("   ")).unwrap_err().contains("recusar pede um motivo"));
        assert!(respond(&dir, "exec-1", "aceitar", Some("por quê")).unwrap_err().contains("aceitar não recebe motivo"));
        assert!(!answer.exists(), "erro não grava nada");
        respond(&dir, "exec-1", "recusar", Some("use só o arquivo que você criou")).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&answer).unwrap()).unwrap();
        assert_eq!(written, json!({ "behavior": "deny", "message": "use só o arquivo que você criou" }));
        assert!(pending_asks(&dir).is_empty(), "respondida não volta como pendente");
        assert!(respond(&dir, "exec-1", "aceitar", None).unwrap_err().contains("não tem pergunta pendente"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sessao_morta_com_lote_pendente_falha_em_vez_de_esperar() {
        let dir = session("orq-delegar", "morta", false);
        assert_eq!(wait(&dir, "nunca", WINDOW).unwrap_err(), DEAD);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pasta_sem_dono_nao_e_sessao_ativa() {
        let dir = session("orq-delegar", "ativa", true);
        assert!(check_session(&dir).is_ok());
        let nowhere = std::env::temp_dir().join(format!("orq-delegar-{}-inexistente", std::process::id()));
        assert_eq!(check_session(&nowhere).unwrap_err(), NOT_ACTIVE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nome_do_lote_ordena_pelo_tempo() {
        let first = hook::unique();
        std::thread::sleep(Duration::from_millis(2));
        let second = hook::unique();
        assert!(first < second, "{first} < {second}");
        assert_eq!(first.as_bytes()[32], b'-');
    }
}
