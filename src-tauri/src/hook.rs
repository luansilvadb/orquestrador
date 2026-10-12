//! Os modos `--hook`, `--guard` e `--ask` do executável, sem janela, chamados pelos CLIs dos executores.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Quanto o `--ask` espera entre uma olhada e outra na pasta da resposta.
const ASK_POLL: Duration = Duration::from_millis(200);
/// Quanto o `--ask` espera a resposta do orquestrador. Medido no Claude Code 2.1.296: o diálogo nega sozinho aos
/// ~120 s e ignora a resposta que chega depois; o `--ask` desiste antes, e o executor repete o comando.
const ASK_DEADLINE: Duration = Duration::from_secs(100);
const CANCELLED: &str = "O executor foi encerrado antes de o orquestrador responder.";
const NO_ANSWER: &str = "O orquestrador ainda não respondeu a este pedido de confirmação. Não tente outro caminho: repita exatamente o mesmo comando para continuar esperando a resposta.";

/// `orquestrador.exe --hook <pasta> [json]`: o sinal de fim de rodada. O Claude Code entrega o
/// JSON do hook `Stop` por stdin; o Codex, como último argumento do `notify`. Grava o que recebeu,
/// sem interpretar, num arquivo novo da pasta (publicado por renomeação); quem lê é o app.
/// Um arquivo por chamada: medido no Codex 0.162, o `notify` também dispara para a rodada interna
/// que gera o título da conversa, e uma chamada não pode apagar a outra.
pub fn run(args: &[String]) -> i32 {
    let Some(dir) = args.first() else { return 2 };
    let payload = match args.get(1) {
        Some(json) => json.clone().into_bytes(),
        None => {
            let mut buf = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut buf);
            buf
        }
    };
    if put(Path::new(dir), &format!("{}.json", unique()), &payload).is_err() { 1 } else { 0 }
}

/// Nome de um arquivo novo da pasta: os nanossegundos com zeros à esquerda (a ordem alfabética é a
/// cronológica) e o pid, para duas chamadas no mesmo instante não se confundirem.
fn unique() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:032}-{}", std::process::id())
}

/// Publica `bytes` como `dir\name`, por renomeação: quem lê a pasta nunca pega o arquivo pela metade.
pub fn put(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dir.join(name))
}

/// `orquestrador.exe --ask <pasta>`: o hook `PermissionRequest` do executor. O CLI entrega o pedido de
/// confirmação por stdin e espera a decisão que sai na saída padrão. Sempre sai com 0 e uma decisão,
/// porque um hook que falha deixa o diálogo do CLI na tela, e ninguém responde a ele.
pub fn ask(args: &[String]) -> i32 {
    let decision = match args.first() {
        Some(dir) => {
            let mut payload = String::new();
            let _ = std::io::stdin().read_to_string(&mut payload);
            ask_in(Path::new(dir), &payload)
        }
        None => deny("O pedido de confirmação veio sem a pasta de respostas."),
    };
    let _ = writeln!(std::io::stdout(), "{}", output(decision));
    0
}

/// Publica o pedido `payload` em `dir` e espera a decisão dele por até `ASK_DEADLINE`. Se o pedido some antes da
/// resposta (o app encerrou o executor e limpou a pasta), nega com o motivo. Se o prazo passa, nega pedindo que o
/// comando seja repetido: o pedido sai da pasta, e uma resposta que chegar depois não tem efeito.
pub fn ask_in(dir: &Path, payload: &str) -> Value {
    ask_within(dir, payload, ASK_DEADLINE)
}

/// `ask_in` com o prazo como parâmetro: os testes passam um prazo curto.
fn ask_within(dir: &Path, payload: &str, deadline: Duration) -> Value {
    let id = unique();
    if let Err(e) = put(dir, &format!("{id}.json"), payload.as_bytes()) {
        return deny(&format!("Não foi possível pedir a resposta do orquestrador: {e}"));
    }
    let request = dir.join(format!("{id}.json"));
    let reply = dir.join(format!("{id}.reply"));
    let started = Instant::now();
    let decision = loop {
        // A resposta é lida antes do prazo: quem chega no instante do prazo vale, ou é apagada abaixo junto com o
        // pedido e não tem efeito. O app só grava resposta para pedido que existe (`deliver_answers`).
        if let Ok(text) = std::fs::read_to_string(&reply) {
            break serde_json::from_str(&text).unwrap_or_else(|_| deny("A resposta do orquestrador não é JSON válido."));
        }
        if !request.is_file() {
            break deny(CANCELLED);
        }
        if started.elapsed() >= deadline {
            break deny(NO_ANSWER);
        }
        std::thread::sleep(ASK_POLL);
    };
    // Primeiro o pedido, depois a resposta: um pedido que ainda existe sem resposta conta como pendente, e a
    // espera do orquestrador o veria de novo entre as duas remoções.
    let _ = std::fs::remove_file(&request);
    let _ = std::fs::remove_file(&reply);
    decision
}

/// A decisão no formato que o Claude Code lê do hook `PermissionRequest`.
fn output(decision: Value) -> Value {
    json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision } })
}

fn deny(message: &str) -> Value {
    json!({ "behavior": "deny", "message": message })
}

/// Os pedidos de confirmação de `dir` que ainda esperam decisão, pelo id, na ordem em que chegaram. Um
/// pedido já tem resposta quando há `<id>.reply` (o `--ask` ainda não a leu) ou `<id>.answer.json` (o
/// orquestrador respondeu e o app ainda não a entregou).
pub fn pending_asks(dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json").filter(|id| !id.ends_with(".answer")).map(String::from))
        .filter(|id| !dir.join(format!("{id}.reply")).exists() && !dir.join(format!("{id}.answer.json")).exists())
        .collect();
    ids.sort();
    ids
}

/// O app entrega as respostas: cada `<id>.answer.json` vira `<id>.reply`, que o `--ask` espera. Um
/// answer cujo pedido já não existe (o nó terminou, ou o `--ask` saiu no prazo) só some.
pub fn deliver_answers(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".answer.json") else { continue };
        if dir.join(format!("{id}.json")).is_file() {
            if let Ok(decision) = std::fs::read(entry.path()) {
                let _ = put(dir, &format!("{id}.reply"), &decision);
                // O `--ask` pode ter saído entre a checagem e a gravação: a resposta sem pedido não fica na pasta.
                if !dir.join(format!("{id}.json")).is_file() {
                    let _ = std::fs::remove_file(dir.join(format!("{id}.reply")));
                }
            }
        }
        let _ = std::fs::remove_file(entry.path());
    }
}

/// Tira tudo o que a pasta tem. Um nó que termina leva consigo os pedidos sem resposta: o `--ask` que
/// ainda espera vê o pedido sumir e nega com `CANCELLED`.
pub fn clear_asks(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
}

/// O que não tem volta e fica barrado aos executores (item 8 do contrato).
pub const BARRED: [&str; 5] = ["git push", "git reset --hard", "git clean", "npm publish", "pnpm publish"];

/// O comando barrado que aparece em `command`, em qualquer ponto dele (depois de `cd x &&`,
/// dentro de `pwsh -Command "..."`). Compara texto: é uma rede, não uma parede.
pub fn barred(command: &str) -> Option<&'static str> {
    let flat = command.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    BARRED.into_iter().find(|b| flat.contains(b))
}

/// `orquestrador.exe --guard`: hook PreToolUse (Claude Code e Codex usam o mesmo formato). Recebe
/// a chamada de ferramenta por stdin e nega a que traz um comando barrado; o resto passa em silêncio.
pub fn guard() -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let call: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
    let command = match &call["tool_input"]["command"] {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if let Some(found) = barred(&command) {
        println!("{}", serde_json::json!({ "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": format!("`{found}` é barrado nesta sessão; pare e relate a quem delegou"),
        }}));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uma pasta de teste, vazia.
    fn fresh(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("orq-hook-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// O id do primeiro pedido que aparecer em `dir`, esperando por ele.
    fn first_request(dir: &Path) -> String {
        loop {
            if let Some(id) = pending_asks(dir).first() {
                return id.clone();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn barrados_em_qualquer_ponto_do_comando() {
        for cmd in ["git push", "git  push origin main", "cd x && git push --force", "pwsh -Command \"git clean -fd\"", "GIT RESET --hard HEAD~1", "npm publish", "pnpm publish --no-git-checks"] {
            assert!(barred(cmd).is_some(), "{cmd}");
        }
        for cmd in ["git status", "git reset --soft HEAD~1", "git commit -m x", "npm run build", "git pull", "pnpm install"] {
            assert!(barred(cmd).is_none(), "{cmd}");
        }
    }

    #[test]
    fn hook_grava_um_arquivo_por_chamada() {
        let dir = fresh("hook");
        let d = dir.to_string_lossy().to_string();
        assert_eq!(run(&[d.clone(), r#"{"a":"ação"}"#.into()]), 0);
        assert_eq!(run(&[d.clone(), r#"{"b":2}"#.into()]), 0);
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        assert_eq!(files.len(), 2);
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), r#"{"a":"ação"}"#);
        assert_eq!(run(&[]), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A saída é a forma que o Claude Code lê do `PermissionRequest`, com a decisão dentro.
    #[test]
    fn saida_do_ask_tem_o_formato_do_permission_request() {
        assert_eq!(
            output(json!({ "behavior": "allow" })),
            json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } } })
        );
        assert_eq!(output(deny("não"))["hookSpecificOutput"]["decision"]["message"], "não");
    }

    /// O pedido vai à pasta como chegou, e a decisão que o app grava é a que o `--ask` devolve; depois da
    /// resposta, nem o pedido nem a resposta ficam na pasta.
    #[test]
    fn ask_publica_o_pedido_e_devolve_a_decisao_gravada() {
        let dir = fresh("ask-ok");
        let (answer_dir, payload) = (dir.clone(), r#"{"tool_name":"Bash","tool_input":{"command":"rm /d/a.log\nlinha 2"}}"#);
        let app = std::thread::spawn(move || {
            let id = first_request(&answer_dir);
            assert_eq!(std::fs::read_to_string(answer_dir.join(format!("{id}.json"))).unwrap(), payload);
            put(&answer_dir, &format!("{id}.reply"), br#"{"behavior":"allow"}"#).unwrap();
        });
        assert_eq!(ask_in(&dir, payload), json!({ "behavior": "allow" }));
        app.join().unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Uma resposta dada pelo orquestrador (`answer.json`) passa a `reply` pelo app e é lida pelo `--ask`,
    /// com o motivo de recusa inteiro.
    #[test]
    fn resposta_do_orquestrador_passa_pelo_app_e_chega_ao_ask_com_o_motivo() {
        let dir = fresh("ask-recusa");
        let app_dir = dir.clone();
        let app = std::thread::spawn(move || {
            let id = first_request(&app_dir);
            put(&app_dir, &format!("{id}.answer.json"), br#"{"behavior":"deny","message":"use o caminho do projeto"}"#).unwrap();
            while !app_dir.join(format!("{id}.reply")).is_file() {
                deliver_answers(&app_dir);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let decision = ask_in(&dir, r#"{"tool_name":"Bash"}"#);
        app.join().unwrap();
        assert_eq!(decision, json!({ "behavior": "deny", "message": "use o caminho do projeto" }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Se o pedido some antes da resposta (o nó terminou e a pasta foi limpa), o `--ask` nega com o motivo
    /// em vez de esperar para sempre.
    #[test]
    fn pedido_que_some_sem_resposta_e_negado() {
        let dir = fresh("ask-cancel");
        let app_dir = dir.clone();
        let app = std::thread::spawn(move || {
            first_request(&app_dir);
            clear_asks(&app_dir);
        });
        assert_eq!(ask_in(&dir, "{}"), deny(CANCELLED));
        app.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pedido sem resposta e pedido já respondido não contam como pendentes; o `.tmp` de uma publicação em
    /// curso também não.
    #[test]
    fn pendentes_sao_os_pedidos_sem_decisao() {
        let dir = fresh("pendentes");
        for name in ["a.json", "b.json", "c.json", "c.reply", "d.json", "d.answer.json", "e.json.tmp", "f.answer.json"] {
            std::fs::write(dir.join(name), "{}").unwrap();
        }
        assert_eq!(pending_asks(&dir), ["a", "b"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entregar_resposta_gera_reply_e_some_com_o_answer() {
        let dir = fresh("entrega");
        std::fs::write(dir.join("a.json"), "{}").unwrap();
        std::fs::write(dir.join("a.answer.json"), r#"{"behavior":"allow"}"#).unwrap();
        std::fs::write(dir.join("orfa.answer.json"), r#"{"behavior":"allow"}"#).unwrap();
        deliver_answers(&dir);
        assert_eq!(std::fs::read_to_string(dir.join("a.reply")).unwrap(), r#"{"behavior":"allow"}"#);
        assert!(!dir.join("a.answer.json").exists() && !dir.join("orfa.answer.json").exists() && !dir.join("orfa.reply").exists());
        assert!(pending_asks(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Sem resposta até o prazo, o `--ask` nega com a mensagem que manda repetir o comando, e o pedido sai da pasta.
    #[test]
    fn pedido_sem_resposta_no_prazo_e_negado_e_sai_da_pasta() {
        let dir = fresh("ask-prazo");
        assert_eq!(ask_within(&dir, "{}", Duration::from_millis(300)), deny(NO_ANSWER));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Resposta que chega antes do prazo continua valendo, com o prazo curto dos testes.
    #[test]
    fn resposta_antes_do_prazo_continua_valendo() {
        let dir = fresh("ask-antes");
        let app_dir = dir.clone();
        let app = std::thread::spawn(move || {
            let id = first_request(&app_dir);
            put(&app_dir, &format!("{id}.answer.json"), br#"{"behavior":"allow"}"#).unwrap();
            deliver_answers(&app_dir);
        });
        assert_eq!(ask_within(&dir, "{}", Duration::from_secs(10)), json!({ "behavior": "allow" }));
        app.join().unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Resposta que chega depois do prazo não tem efeito: o `--ask` já saiu com a negativa, e a entrega que vier
    /// depois não grava `reply` nem deixa nada na pasta.
    #[test]
    fn resposta_que_chega_depois_do_prazo_nao_tem_efeito() {
        let dir = fresh("ask-tarde");
        let asker_dir = dir.clone();
        let asker = std::thread::spawn(move || ask_within(&asker_dir, "{}", Duration::from_millis(200)));
        let id = first_request(&dir);
        put(&dir, &format!("{id}.answer.json"), br#"{"behavior":"allow"}"#).unwrap();
        assert_eq!(asker.join().unwrap(), deny(NO_ANSWER));
        deliver_answers(&dir);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nada sobra na pasta");
        assert!(pending_asks(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
