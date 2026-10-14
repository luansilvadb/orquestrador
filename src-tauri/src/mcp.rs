//! O servidor MCP do orquestrador: `orquestrador.exe --mcp <pasta da sessão>`. O CLI do orquestrador o abre como
//! servidor stdio e fala com ele em JSON-RPC 2.0, uma mensagem por linha, pela entrada e pela saída padrão. As
//! ferramentas são `delegar`, `aguardar` e `responder`, e cada uma age sobre o lote da sessão (`delegate.rs`).
use crate::delegate::{self, Waited};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// A versão do protocolo que o servidor responde quando o cliente não pede uma.
const PROTOCOL: &str = "2025-06-18";
const RUNNING: &str = "O lote segue em andamento; chame aguardar para continuar esperando.\n--- EM ANDAMENTO ---\n";

/// Ponto de entrada: `args` são os argumentos depois de `--mcp`, e o primeiro é a pasta da sessão. Responde até a
/// entrada padrão fechar, e sai com 0. Uma chamada que ainda espera o lote morre junto com o processo, sem marca de
/// entregue: `aguardar` a pega na próxima sessão do CLI.
pub fn serve(args: &[String]) -> i32 {
    let session = PathBuf::from(args.first().map(String::as_str).unwrap_or_default());
    serve_io(std::io::stdin().lock(), std::io::stdout(), session, delegate::WINDOW);
    0
}

/// O laço de leitura: cada linha é uma mensagem. `tools/call` roda numa thread própria, para a leitura não parar
/// enquanto uma chamada espera o lote; as respostas saem uma de cada vez, com o lock da saída. Devolve as threads
/// das chamadas: o `serve` não as espera, e os testes esperam por elas.
pub fn serve_io<W: Write + Send + 'static>(input: impl BufRead, output: W, session: PathBuf, window: Duration) -> Vec<JoinHandle<()>> {
    let out = Arc::new(Mutex::new(output));
    let mut calls = Vec::new();
    for line in input.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        match route(&line) {
            Route::Silent => {}
            Route::Reply(msg) => {
                write(&out, &msg);
            }
            Route::Call { id, name, args } => {
                let (out, session) = (Arc::clone(&out), session.clone());
                calls.push(std::thread::spawn(move || {
                    let reply = call_tool(&session, &name, &args, window);
                    // A marca só entra depois que a resposta saiu inteira: se a saída falhar, o lote segue pendente.
                    if write(&out, &result(&id, &reply)) {
                        if let Some(marker) = reply.delivered {
                            let _ = std::fs::write(marker, "");
                        }
                    }
                }));
            }
        }
    }
    calls
}

/// O que uma linha da entrada pede: nada (notificação), uma resposta pronta, ou uma chamada de ferramenta.
enum Route {
    Silent,
    Reply(Value),
    Call { id: Value, name: String, args: Value },
}

fn route(line: &str) -> Route {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Route::Reply(error(Value::Null, -32700, "JSON ilegível."));
    };
    if !msg.is_object() {
        return Route::Reply(error(Value::Null, -32600, "A mensagem não é um objeto JSON-RPC."));
    }
    // Sem `id` é notificação, e notificação não tem resposta.
    let Some(id) = msg.get("id").cloned() else { return Route::Silent };
    if msg["method"] == "tools/call" {
        let name = msg["params"]["name"].as_str().unwrap_or_default().to_string();
        return Route::Call { id, name, args: msg["params"]["arguments"].clone() };
    }
    Route::Reply(handle(id, &msg))
}

/// A resposta de um método que não é `tools/call`. Método desconhecido com `id` é erro -32601.
fn handle(id: Value, msg: &Value) -> Value {
    let result = match msg["method"].as_str() {
        Some("initialize") => json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "orquestrador", "version": env!("CARGO_PKG_VERSION") },
        }),
        Some("ping") => json!({}),
        Some("tools/list") => json!({ "tools": tools() }),
        _ => return error(id, -32601, "Método desconhecido."),
    };
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// As três ferramentas do orquestrador, com o que cada uma recebe.
fn tools() -> Value {
    json!([
        {
            "name": "delegar",
            "description": "Delega um lote de tarefas aos executores e espera o resultado.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tarefas": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "description": "Uma tarefa por item, com o texto completo dela. Os executores do lote trabalham ao mesmo tempo.",
                    },
                },
                "required": ["tarefas"],
            },
        },
        {
            "name": "aguardar",
            "description": "Espera de novo o lote mais recente que ainda não foi entregue, sem publicar nada.",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "responder",
            "description": "Responde à pergunta de confirmação de um executor e volta a esperar o lote.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "executor": { "type": "string", "description": "O executor da pergunta, como exec-7." },
                    "decisao": { "type": "string", "enum": ["aceitar", "recusar"] },
                    "motivo": { "type": "string", "description": "Obrigatório ao recusar, e não é usado ao aceitar." },
                },
                "required": ["executor", "decisao"],
            },
        },
    ])
}

/// O que uma ferramenta devolve ao CLI. `delivered` é a marca do lote, que o laço grava só depois que a resposta sai.
#[derive(Debug)]
struct Reply {
    text: String,
    is_error: bool,
    delivered: Option<PathBuf>,
}

/// Roda a ferramenta `name` sobre a sessão e espera o lote por até `window`. Erro vira resposta com `isError`.
fn call_tool(session: &Path, name: &str, args: &Value, window: Duration) -> Reply {
    match dispatch(session, name, args, window) {
        Ok(Waited::Result { text, marker }) => Reply { text, is_error: false, delivered: Some(marker) },
        Ok(Waited::Asking(text)) => Reply { text, is_error: false, delivered: None },
        Ok(Waited::Running) => Reply { text: RUNNING.into(), is_error: false, delivered: None },
        Err(why) => Reply { text: why, is_error: true, delivered: None },
    }
}

fn dispatch(session: &Path, name: &str, args: &Value, window: Duration) -> Result<Waited, String> {
    if !matches!(name, "delegar" | "aguardar" | "responder") {
        return Err(format!("Ferramenta desconhecida: {name}."));
    }
    delegate::check_session(session)?;
    match name {
        "delegar" => {
            let id = delegate::publish(session, &tasks(args)?)?;
            delegate::wait(session, &id, window)
        }
        "aguardar" => {
            let id = delegate::resume(session)?;
            delegate::wait(session, &id, window)
        }
        _ => {
            let node = text_arg(args, "executor")?;
            let choice = text_arg(args, "decisao")?;
            // Motivo em branco é o mesmo que motivo nenhum: `recusar` continua exigindo um texto.
            let reason = args["motivo"].as_str().filter(|m| !m.trim().is_empty());
            delegate::respond(session, &node, &choice, reason)?;
            let id = delegate::resume(session)?;
            delegate::wait(session, &id, window)
        }
    }
}

fn tasks(args: &Value) -> Result<Vec<String>, String> {
    let items = args["tarefas"].as_array().ok_or("Falta a lista `tarefas`, com pelo menos uma tarefa.")?;
    items
        .iter()
        .map(|item| item.as_str().map(String::from).ok_or_else(|| "`tarefas` tem de ser uma lista de textos.".to_string()))
        .collect()
}

fn text_arg(args: &Value, key: &str) -> Result<String, String> {
    args[key].as_str().map(String::from).ok_or_else(|| format!("Falta o argumento `{key}`, um texto."))
}

fn result(id: &Value, reply: &Reply) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": { "content": [{ "type": "text", "text": reply.text }], "isError": reply.is_error } })
}

/// Escreve uma mensagem numa linha e a descarrega. Devolve se saiu inteira.
fn write<W: Write>(out: &Mutex<W>, msg: &Value) -> bool {
    let mut out = out.lock().unwrap();
    writeln!(out, "{msg}").and_then(|()| out.flush()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::start_time;
    use std::io::Cursor;

    /// A janela dos testes: curta, para a espera acabar logo.
    const SHORT: Duration = Duration::from_millis(300);

    /// Uma sessão cujo dono é este processo (viva) ou um processo que já não existe.
    fn session(tag: &str, alive: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orq-mcp-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["requests", "results", "exec"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let me = std::process::id();
        let started = start_time(me).unwrap() + u64::from(!alive);
        std::fs::write(dir.join("owner"), format!("{me} {started} strategist-{tag}")).unwrap();
        dir
    }

    /// Um lote com o executor `node` na tarefa `index` e um pedido de confirmação sem resposta com `payload`.
    fn batch_with_ask(dir: &Path, batch: &str, node: &str, index: usize, size: usize, payload: &str) {
        let req = dir.join("requests").join(batch);
        std::fs::create_dir_all(&req).unwrap();
        for n in 1..=size {
            std::fs::write(req.join(format!("{n}.txt")), format!("Tarefa {n}\nescopo")).unwrap();
        }
        let node_dir = dir.join("exec").join(node);
        std::fs::create_dir_all(node_dir.join("ask")).unwrap();
        std::fs::write(node_dir.join("batch.json"), json!({ "batch": batch, "index": index, "size": size }).to_string()).unwrap();
        std::fs::write(node_dir.join("ask").join("00000000000000000001-aaaaaaaa.json"), payload).unwrap();
    }

    const BASH: &str = r#"{"tool_name":"Bash","tool_input":{"command":"rm /d/a.log","description":"x"}}"#;

    /// Uma saída que o teste lê depois: o servidor escreve nela e o teste vê as linhas.
    #[derive(Clone, Default)]
    struct Output(Arc<Mutex<Vec<u8>>>);

    impl Write for Output {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Output {
        fn lines(&self) -> Vec<Value> {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
        }
    }

    /// Manda `input` ao servidor, uma linha por mensagem, e espera as chamadas que ficaram em curso.
    fn talk(input: &str, session: &Path) -> Vec<Value> {
        let out = Output::default();
        for handle in serve_io(Cursor::new(input.to_string()), out.clone(), session.to_path_buf(), SHORT) {
            handle.join().unwrap();
        }
        out.lines()
    }

    /// Uma chamada de ferramenta, como uma linha JSON-RPC.
    fn call(id: u32, name: &str, arguments: Value) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": arguments } }).to_string()
    }

    /// Texto e erro da resposta a uma chamada.
    fn text_of(reply: &Value) -> (String, bool) {
        (reply["result"]["content"][0]["text"].as_str().unwrap().to_string(), reply["result"]["isError"].as_bool().unwrap())
    }

    #[test]
    fn initialize_devolve_a_versao_pedida_ou_a_padrao() {
        let s = session("init", true);
        let got = talk(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\"}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\",\"params\":{}}\n",
            &s,
        );
        assert_eq!(got[0]["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(got[1]["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(got[0]["result"]["capabilities"], json!({ "tools": {} }));
        assert_eq!(got[0]["result"]["serverInfo"], json!({ "name": "orquestrador", "version": env!("CARGO_PKG_VERSION") }));
        assert_eq!(got[0]["id"], 1);
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn ping_devolve_resultado_vazio() {
        let s = session("ping", true);
        let got = talk("{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}\n", &s);
        assert_eq!(got, [json!({ "jsonrpc": "2.0", "id": 3, "result": {} })]);
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn tools_list_traz_as_tres_ferramentas_com_esquema() {
        let s = session("list", true);
        let got = talk("{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/list\"}\n", &s);
        let tools = got[0]["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["delegar", "aguardar", "responder"]);
        assert!(tools.iter().all(|t| t["description"].is_string() && t["inputSchema"]["type"] == "object"));
        assert_eq!(tools[0]["inputSchema"]["required"], json!(["tarefas"]));
        assert_eq!(tools[0]["inputSchema"]["properties"]["tarefas"]["minItems"], 1);
        assert_eq!(tools[2]["inputSchema"]["required"], json!(["executor", "decisao"]));
        assert_eq!(tools[2]["inputSchema"]["properties"]["decisao"]["enum"], json!(["aceitar", "recusar"]));
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn notificacao_nao_tem_resposta() {
        let s = session("notif", true);
        let got = talk(
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":7}}\n",
            &s,
        );
        assert!(got.is_empty(), "{got:?}");
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn metodo_desconhecido_com_id_e_erro_32601() {
        // O Claude manda `server/discover` antes do `initialize`: o erro é a resposta certa.
        let s = session("metodo", true);
        let got = talk("{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"server/discover\"}\n", &s);
        assert_eq!(got[0]["id"], 5);
        assert_eq!(got[0]["error"]["code"], -32601);
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn json_ilegivel_e_erro_32700_com_id_nulo() {
        let s = session("ilegivel", true);
        let got = talk("{\"jsonrpc\":\"2.0\",\"id\":6,\n", &s);
        assert_eq!(got[0]["id"], Value::Null);
        assert_eq!(got[0]["error"]["code"], -32700);
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn delegar_com_lista_vazia_ou_tarefa_em_branco_nao_publica_nada() {
        let s = session("invalido", true);
        let input = [call(7, "delegar", json!({ "tarefas": [] })), call(8, "delegar", json!({ "tarefas": ["ok", "  "] })), call(9, "delegar", json!({}))].join("\n");
        let got = talk(&input, &s);
        let by_id = |id: u64| got.iter().find(|r| r["id"] == id).unwrap();
        let (text, is_error) = text_of(by_id(7));
        assert!(is_error && text.contains("Nenhuma tarefa"), "{text}");
        let (text, is_error) = text_of(by_id(8));
        assert!(is_error && text.contains("Tarefa vazia"), "{text}");
        assert!(text_of(by_id(9)).1, "sem `tarefas` é erro");
        assert_eq!(std::fs::read_dir(s.join("requests")).unwrap().count(), 0, "nenhum lote publicado");
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn aguardar_sem_lote_e_erro_sem_prefixo_nem_fim() {
        let s = session("sem-lote", true);
        let got = talk(&call(10, "aguardar", json!({})), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(is_error);
        assert_eq!(text, "Nenhum lote aguardando entrega.");
        assert!(!text.contains("wrapper_error") && !text.contains("--- FIM ---"));
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn responder_valida_a_resposta_antes_de_gravar() {
        let s = session("validacao", true);
        batch_with_ask(&s, "B1", "exec-1", 0, 1, BASH);
        let input = [
            call(11, "responder", json!({ "executor": "exec-1", "decisao": "recusar" })),
            call(12, "responder", json!({ "executor": "exec-1", "decisao": "aceitar", "motivo": "por quê" })),
            call(13, "responder", json!({ "executor": "exec-7", "decisao": "aceitar" })),
            call(14, "responder", json!({ "executor": "exec-1", "decisao": "talvez" })),
        ]
        .join("\n");
        // As chamadas rodam em threads: a ordem da saída não é fixa, então cada resposta é achada pelo id.
        let got = talk(&input, &s);
        let by_id = |id: u64| got.iter().find(|r| r["id"] == id).unwrap();
        for (id, expected) in [
            (11, "recusar pede um motivo"),
            (12, "aceitar não recebe motivo"),
            (13, "não tem pergunta pendente"),
            (14, "resposta desconhecida"),
        ] {
            let (text, is_error) = text_of(by_id(id));
            assert!(is_error && text.contains(expected), "{id}: {text}");
        }
        assert!(!s.join("exec/exec-1/ask/00000000000000000001-aaaaaaaa.reply").exists(), "erro não grava nada");
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn responder_recusar_com_motivo_grava_e_volta_a_esperar_o_lote() {
        let s = session("recusa", true);
        batch_with_ask(&s, "B1", "exec-1", 0, 1, BASH);
        let got = talk(&call(15, "responder", json!({ "executor": "exec-1", "decisao": "recusar", "motivo": "use só o arquivo que você criou" })), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(!is_error);
        assert_eq!(text, RUNNING, "o lote não tem resultado, então a espera acaba no prazo");
        let answer = std::fs::read_to_string(s.join("exec/exec-1/ask/00000000000000000001-aaaaaaaa.reply")).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&answer).unwrap(), json!({ "behavior": "deny", "message": "use só o arquivo que você criou" }));
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn janela_curta_devolve_em_andamento_sem_marca_e_aguardar_pega_o_resultado() {
        let s = session("janela", true);
        let got = talk(&call(16, "delegar", json!({ "tarefas": ["Tarefa A"] })), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(!is_error);
        assert_eq!(text, RUNNING);
        assert!(text.ends_with("--- EM ANDAMENTO ---\n") && text.starts_with("O lote segue em andamento; chame aguardar para continuar esperando."));
        let id = std::fs::read_dir(s.join("requests")).unwrap().next().unwrap().unwrap().file_name().to_string_lossy().into_owned();
        assert!(!s.join("results").join(format!("{id}.delivered")).exists(), "em andamento não entrega nada");
        // O app grava o resultado; `aguardar` o pega, e só então o lote fica entregue.
        std::fs::write(s.join("results").join(format!("{id}.txt")), "0\n--- EXECUTOR 1/1 ---\nTarefa A\nfeito\nexecutor_exit: 0\n--- FIM ---\n").unwrap();
        let got = talk(&call(17, "aguardar", json!({})), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(!is_error);
        assert_eq!(text, "--- EXECUTOR 1/1 ---\nTarefa A\nfeito\nexecutor_exit: 0\n--- FIM ---\n");
        assert!(s.join("results").join(format!("{id}.delivered")).exists(), "a resposta saiu, então a marca entra");
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn marca_de_entregue_so_entra_depois_que_a_resposta_sai() {
        let s = session("marca", true);
        batch_with_ask(&s, "B1", "exec-1", 0, 1, BASH);
        std::fs::write(s.join("results").join("B1.txt"), "0\n--- EXECUTOR 1/1 ---\nok\n--- FIM ---\n").unwrap();
        // O resultado devolvido traz a marca, que quem chama grava; a função em si não grava nada.
        let reply = call_tool(&s, "aguardar", &json!({}), SHORT);
        assert!(!reply.is_error);
        let marker = reply.delivered.clone().expect("o resultado traz a marca");
        assert!(!marker.exists(), "devolver o resultado não grava a marca");
        // Pelo laço, a marca entra depois da resposta escrita.
        let _ = talk(&call(18, "aguardar", json!({})), &s);
        assert!(marker.exists());
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn pergunta_interrompe_a_espera_e_nao_entrega() {
        let s = session("pergunta", true);
        batch_with_ask(&s, "B1", "exec-1", 0, 2, BASH);
        let got = talk(&call(19, "aguardar", json!({})), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(!is_error);
        assert!(text.contains("--- PERGUNTA exec-1 (1/2) ---") && text.contains("Bash: rm /d/a.log"), "{text}");
        assert!(text.contains("chame responder com o executor, aceitar ou recusar e, ao recusar, o motivo"), "{text}");
        assert!(text.ends_with("--- AGUARDANDO RESPOSTA ---\n") && !text.contains("--- FIM ---"));
        assert!(!s.join("results").join("B1.delivered").exists());
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn sessao_sem_dono_e_erro_em_toda_chamada() {
        let s = session("sem-dono", false);
        std::fs::remove_file(s.join("owner")).unwrap();
        let input = [
            call(20, "delegar", json!({ "tarefas": ["x"] })),
            call(21, "aguardar", json!({})),
            call(22, "responder", json!({ "executor": "exec-1", "decisao": "aceitar" })),
        ]
        .join("\n");
        let got = talk(&input, &s);
        for reply in &got {
            let (text, is_error) = text_of(reply);
            assert!(is_error && text == "A pasta não é uma sessão ativa do orquestrador.", "{text}");
        }
        assert_eq!(std::fs::read_dir(s.join("requests")).unwrap().count(), 0, "nada publicado");
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn ferramenta_desconhecida_e_erro() {
        let s = session("desconhecida", true);
        let got = talk(&call(23, "apagar", json!({})), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(is_error && text.contains("Ferramenta desconhecida"), "{text}");
        let _ = std::fs::remove_dir_all(&s);
    }
}
