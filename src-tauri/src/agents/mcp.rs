//! O servidor MCP do orquestrador: `orquestrador.exe --mcp <pasta da sessão>`. O CLI do orquestrador o abre como
//! servidor stdio e fala com ele em JSON-RPC 2.0, uma mensagem por linha, pela entrada e pela saída padrão. As
//! ferramentas são `delegar` e `aguardar`, e cada uma age sobre o lote da sessão (`delegate.rs`).
use crate::agents::delegate::{self, Waited};
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
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            write(&out, &error(Value::Null, -32700, "JSON ilegível."));
            continue;
        };
        if !msg.is_object() {
            write(&out, &error(Value::Null, -32600, "A mensagem não é um objeto JSON-RPC."));
            continue;
        }
        // Sem `id` é notificação, e notificação não tem resposta.
        let Some(id) = msg.get("id").cloned() else { continue };
        if msg["method"] != "tools/call" {
            write(&out, &handle(id, &msg));
            continue;
        }
        let name = msg["params"]["name"].as_str().unwrap_or_default().to_string();
        let args = msg["params"]["arguments"].clone();
        let (out, session) = (Arc::clone(&out), session.clone());
        calls.push(std::thread::spawn(move || {
            // Erro vira resposta com `isError`; a marca do lote só vem com o resultado.
            let (text, is_error, marker) = match dispatch(&session, &name, &args, window) {
                Ok(Waited::Result { text, marker }) => (text, false, Some(marker)),
                Ok(Waited::Running) => (RUNNING.into(), false, None),
                Err(why) => (why, true, None),
            };
            let reply = json!({ "jsonrpc": "2.0", "id": id, "result": { "content": [{ "type": "text", "text": text }], "isError": is_error } });
            // A marca só entra depois que a resposta saiu inteira: se a saída falhar, o lote segue pendente.
            if write(&out, &reply) {
                if let Some(marker) = marker {
                    let _ = std::fs::write(marker, "");
                }
            }
        }));
    }
    calls
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

/// As duas ferramentas do orquestrador, com o que cada uma recebe.
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
    ])
}

/// Roda a ferramenta `name` sobre a sessão e espera o lote por até `window`.
fn dispatch(session: &Path, name: &str, args: &Value, window: Duration) -> Result<Waited, String> {
    if !matches!(name, "delegar" | "aguardar") {
        return Err(format!("Ferramenta desconhecida: {name}."));
    }
    delegate::check_session(session)?;
    let id = if name == "delegar" { delegate::publish(session, &tasks(args)?)? } else { delegate::resume(session)? };
    delegate::wait(session, &id, window)
}

fn tasks(args: &Value) -> Result<Vec<String>, String> {
    let items = args["tarefas"].as_array().ok_or("Falta a lista `tarefas`, com pelo menos uma tarefa.")?;
    items
        .iter()
        .map(|item| item.as_str().map(String::from).ok_or_else(|| "`tarefas` tem de ser uma lista de textos.".to_string()))
        .collect()
}

/// Escreve uma mensagem numa linha e a descarrega. Devolve se saiu inteira.
fn write<W: Write>(out: &Mutex<W>, msg: &Value) -> bool {
    let mut out = out.lock().unwrap();
    writeln!(out, "{msg}").and_then(|()| out.flush()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::session;
    use std::io::Cursor;

    /// A janela dos testes: curta, para a espera acabar logo.
    const SHORT: Duration = Duration::from_millis(300);

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
        let s = &session("orq-mcp", "init", true).0;
        let got = talk(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\"}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\",\"params\":{}}\n",
            &s,
        );
        assert_eq!(got[0]["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(got[1]["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(got[0]["result"]["capabilities"], json!({ "tools": {} }));
        assert_eq!(got[0]["result"]["serverInfo"], json!({ "name": "orquestrador", "version": env!("CARGO_PKG_VERSION") }));
        assert_eq!(got[0]["id"], 1);
    }

    #[test]
    fn ping_devolve_resultado_vazio() {
        let s = &session("orq-mcp", "ping", true).0;
        let got = talk("{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}\n", &s);
        assert_eq!(got, [json!({ "jsonrpc": "2.0", "id": 3, "result": {} })]);
    }

    #[test]
    fn tools_list_traz_as_duas_ferramentas_com_esquema() {
        let s = &session("orq-mcp", "list", true).0;
        let got = talk("{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/list\"}\n", &s);
        let tools = got[0]["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["delegar", "aguardar"]);
        assert!(tools.iter().all(|t| t["description"].is_string() && t["inputSchema"]["type"] == "object"));
        assert_eq!(tools[0]["inputSchema"]["required"], json!(["tarefas"]));
        assert_eq!(tools[0]["inputSchema"]["properties"]["tarefas"]["minItems"], 1);
    }

    #[test]
    fn notificacao_nao_tem_resposta() {
        let s = &session("orq-mcp", "notif", true).0;
        let got = talk(
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":7}}\n",
            &s,
        );
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn metodo_desconhecido_com_id_e_erro_32601() {
        // O Claude manda `server/discover` antes do `initialize`: o erro é a resposta certa.
        let s = &session("orq-mcp", "metodo", true).0;
        let got = talk("{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"server/discover\"}\n", &s);
        assert_eq!(got[0]["id"], 5);
        assert_eq!(got[0]["error"]["code"], -32601);
    }

    #[test]
    fn json_ilegivel_e_erro_32700_com_id_nulo() {
        let s = &session("orq-mcp", "ilegivel", true).0;
        let got = talk("{\"jsonrpc\":\"2.0\",\"id\":6,\n", &s);
        assert_eq!(got[0]["id"], Value::Null);
        assert_eq!(got[0]["error"]["code"], -32700);
    }

    #[test]
    fn delegar_com_lista_vazia_ou_tarefa_em_branco_nao_publica_nada() {
        let s = &session("orq-mcp", "invalido", true).0;
        let input = [call(7, "delegar", json!({ "tarefas": [] })), call(8, "delegar", json!({ "tarefas": ["ok", "  "] })), call(9, "delegar", json!({}))].join("\n");
        let got = talk(&input, &s);
        let by_id = |id: u64| got.iter().find(|r| r["id"] == id).unwrap();
        let (text, is_error) = text_of(by_id(7));
        assert!(is_error && text.contains("Nenhuma tarefa"), "{text}");
        let (text, is_error) = text_of(by_id(8));
        assert!(is_error && text.contains("Tarefa vazia"), "{text}");
        assert!(text_of(by_id(9)).1, "sem `tarefas` é erro");
        assert_eq!(std::fs::read_dir(s.join("requests")).unwrap().count(), 0, "nenhum lote publicado");
    }

    #[test]
    fn aguardar_sem_lote_e_erro_sem_prefixo_nem_fim() {
        let s = &session("orq-mcp", "sem-lote", true).0;
        let got = talk(&call(10, "aguardar", json!({})), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(is_error);
        assert_eq!(text, "Nenhum lote aguardando entrega.");
        assert!(!text.contains("wrapper_error") && !text.contains("--- FIM ---"));
    }

    #[test]
    fn janela_curta_devolve_em_andamento_sem_marca_e_aguardar_pega_o_resultado() {
        let s = &session("orq-mcp", "janela", true).0;
        let got = talk(&call(16, "delegar", json!({ "tarefas": ["Tarefa A"] })), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(!is_error);
        assert_eq!(text, RUNNING);
        assert!(text.ends_with("--- EM ANDAMENTO ---\n") && text.starts_with("O lote segue em andamento; chame aguardar para continuar esperando."));
        let id = std::fs::read_dir(s.join("requests")).unwrap().next().unwrap().unwrap().file_name().to_string_lossy().into_owned();
        assert!(!s.join("results").join(format!("{id}.delivered")).exists(), "em andamento não entrega nada");
        // O app grava o resultado; `aguardar` o pega, e só então o lote fica entregue.
        std::fs::write(s.join("results").join(format!("{id}.txt")), "--- EXECUTOR 1/1 ---\nTarefa A\nfeito\nexecutor_exit: 0\n--- FIM ---\n").unwrap();
        let got = talk(&call(17, "aguardar", json!({})), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(!is_error);
        assert_eq!(text, "--- EXECUTOR 1/1 ---\nTarefa A\nfeito\nexecutor_exit: 0\n--- FIM ---\n");
        assert!(s.join("results").join(format!("{id}.delivered")).exists(), "a resposta saiu, então a marca entra");
    }

    #[test]
    fn marca_de_entregue_so_entra_depois_que_a_resposta_sai() {
        let s = &session("orq-mcp", "marca", true).0;
        std::fs::create_dir_all(s.join("requests").join("B1")).unwrap();
        std::fs::write(s.join("results").join("B1.txt"), "--- EXECUTOR 1/1 ---\nok\n--- FIM ---\n").unwrap();
        // O resultado devolvido traz a marca, que quem chama grava; a função em si não grava nada.
        let Ok(Waited::Result { marker, .. }) = dispatch(&s, "aguardar", &json!({}), SHORT) else { panic!("o resultado traz a marca") };
        assert!(!marker.exists(), "devolver o resultado não grava a marca");
        // Pelo laço, a marca entra depois da resposta escrita.
        let _ = talk(&call(18, "aguardar", json!({})), &s);
        assert!(marker.exists());
    }

    #[test]
    fn sessao_sem_dono_e_erro_em_toda_chamada() {
        let s = &session("orq-mcp", "sem-dono", false).0;
        std::fs::remove_file(s.join("owner")).unwrap();
        let input = [
            call(20, "delegar", json!({ "tarefas": ["x"] })),
            call(21, "aguardar", json!({})),
        ]
        .join("\n");
        let got = talk(&input, &s);
        for reply in &got {
            let (text, is_error) = text_of(reply);
            assert!(is_error && text == "A pasta não é uma sessão ativa do orquestrador.", "{text}");
        }
        assert_eq!(std::fs::read_dir(s.join("requests")).unwrap().count(), 0, "nada publicado");
    }

    #[test]
    fn ferramenta_desconhecida_e_erro() {
        let s = &session("orq-mcp", "desconhecida", true).0;
        let got = talk(&call(23, "apagar", json!({})), &s);
        let (text, is_error) = text_of(&got[0]);
        assert!(is_error && text.contains("Ferramenta desconhecida"), "{text}");
    }
}
