//! Memória de contexto (#79) com os CLIs reais (chama modelo, por isso `#[ignore]`): um orquestrador compacta três
//! vezes e tem de lembrar de um fato e de uma delegação anteriores; a memória some com a sessão e não vaza para outra.
//! Roda num repositório Git descartável em %TEMP%, apagado no fim. Uso:
//!   $env:ORQ_REAL = 'claude'      # orquestrador: claude ou codex
//!   $env:ORQ_REAL_EXEC = 'claude' # executor (padrão: claude)
//!   $env:ORQ_REAL_CODEX_MODEL = 'gpt-5.6-luna' # modelo barato do Codex (padrão: este)
//!   cargo test --test memoria_real -- --ignored --nocapture
//! Os logs ficam em %TEMP%\orq-memoria-real-<orquestrador>-*: etapas (telas e entradas), tela final, bytes do
//! terminal, cópia do transcript e as entradas da memória.
use base64::Engine;
use orquestrador_lib::agents::cli::Exes;
use orquestrador_lib::agents::session::Places;
use orquestrador_lib::config::Choice;
use orquestrador_lib::core::{Core, Emit, ORCHESTRATOR};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Os fatos únicos da conversa: o do passo 2, o do passo 4 e o texto da tarefa delegada.
const CODINOME: &str = "GIRASSOL-4821";
const PRAZO: &str = "SETEMBRO-3307";
const TEXTO: &str = "LANTERNA-9035";
/// Trecho da pergunta final, para achar a rodada dela no transcript.
const MARCA: &str = "texto exato que mandei gravar";

#[derive(Clone, Debug)]
struct Entrada {
    conversa: String,
    tipo: String,
    texto: String,
}

/// As entradas da memória de uma sessão, da conversa corrente (`atual`) ou de todas, na ordem dos nomes.
fn le_entradas(session: &Path, so_atual: bool) -> Vec<Entrada> {
    let memoria = session.join("memoria");
    let atual = fs::read_to_string(memoria.join("atual")).unwrap_or_default().trim().to_string();
    let mut pastas: Vec<PathBuf> = fs::read_dir(&memoria).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    pastas.sort();
    let mut out = Vec::new();
    for pasta in pastas {
        let conversa = pasta.file_name().unwrap().to_string_lossy().to_string();
        if so_atual && conversa != atual {
            continue;
        }
        let mut arquivos: Vec<PathBuf> = fs::read_dir(&pasta).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        arquivos.sort();
        for arquivo in arquivos {
            let Ok(v) = serde_json::from_slice::<Value>(&fs::read(&arquivo).unwrap_or_default()) else { continue };
            out.push(Entrada { conversa: conversa.clone(), tipo: v["tipo"].as_str().unwrap_or("?").into(), texto: v["texto"].as_str().unwrap_or("").into() });
        }
    }
    out
}

fn conta(entradas: &[Entrada], tipo: &str) -> usize {
    entradas.iter().filter(|e| e.tipo == tipo).count()
}

/// Os nomes de ferramenta chamados num JSON de transcript, em qualquer profundidade (Claude: `tool_use`; Codex:
/// `function_call` e as chamadas MCP).
fn coleta(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            let tipo = m.get("type").and_then(Value::as_str).unwrap_or("");
            if let Some(nome) = m.get("name").and_then(Value::as_str) {
                if tipo == "tool_use" || tipo.contains("call") {
                    out.push(nome.to_string());
                }
            }
            if let (Some(servidor), Some(ferramenta)) = (m.get("server").and_then(Value::as_str), m.get("tool").and_then(Value::as_str)) {
                out.push(format!("{servidor}.{ferramenta}"));
            }
            m.values().for_each(|x| coleta(x, out));
        }
        Value::Array(items) => items.iter().for_each(|x| coleta(x, out)),
        _ => {}
    }
}

/// As ferramentas chamadas nas linhas do transcript depois da primeira que contém `marca`, com a contagem.
fn ferramentas_apos(linhas: &[String], marca: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let Some(inicio) = linhas.iter().position(|l| l.contains(marca)) else { return out };
    for linha in &linhas[inicio..] {
        let Ok(v) = serde_json::from_str::<Value>(linha) else { continue };
        let mut nomes = Vec::new();
        coleta(&v, &mut nomes);
        for nome in nomes {
            *out.entry(nome).or_insert(0) += 1;
        }
    }
    out
}

/// Todos os valores string de um JSON, em qualquer profundidade.
fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|x| strings(x, out)),
        Value::Object(m) => m.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

fn sem_ansi(raw: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;?<>=]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][A-Z0-9]|\x1b[=>78]").unwrap();
    re.replace_all(raw, "").replace('\r', "")
}

fn cauda(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

fn git(repo: &Path, args: &[&str]) {
    let st = Command::new("git").arg("-C").arg(repo).args(args).status().expect("git");
    assert!(st.success(), "git {args:?}");
}

/// A cena inteira: o Core real, o terminal do orquestrador e o que se observa dele.
struct Cena {
    core: Core,
    events: Receiver<(String, Value)>,
    session: PathBuf,
    bytes: Vec<u8>,
    etapas: PathBuf,
    relatorio: Vec<(String, bool)>,
    escolha: Choice,
}

impl Cena {
    fn poll(&mut self) {
        while let Ok((event, payload)) = self.events.try_recv() {
            match event.as_str() {
                "term-data" if payload["id"] == ORCHESTRATOR => {
                    self.bytes.extend(base64::engine::general_purpose::STANDARD.decode(payload["data"].as_str().unwrap()).unwrap());
                }
                "term-data" => {}
                "orchestrator-exit" => panic!("o orquestrador saiu: {payload}"),
                _ => self.nota(&format!("{event}: {payload}")),
            }
        }
    }

    fn nota(&self, texto: &str) {
        println!("{texto}");
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&self.etapas).unwrap();
        writeln!(f, "{texto}").unwrap();
    }

    fn tela(&self) -> String {
        let snap = self.core.term_snapshot(ORCHESTRATOR).unwrap();
        String::from_utf8_lossy(&base64::engine::general_purpose::STANDARD.decode(snap.data).unwrap()).to_string()
    }

    fn seq(&self) -> u64 {
        self.core.term_snapshot(ORCHESTRATOR).unwrap().seq
    }

    /// A tela do orquestrador no log de etapas (só o fim, sem as sequências de controle).
    fn foto(&self, rotulo: &str) {
        self.nota(&format!("--- tela: {rotulo} ---\n{}\n--- fim da tela ---", cauda(&sem_ansi(&self.tela()), 2500)));
    }

    fn confere(&mut self, nome: &str, ok: bool, detalhe: &str) {
        self.nota(&format!("[{}] {nome}{}", if ok { "PASSOU" } else { "FALHOU" }, if detalhe.is_empty() { String::new() } else { format!(": {detalhe}") }));
        self.relatorio.push((nome.to_string(), ok));
    }

    fn entradas(&self) -> Vec<Entrada> {
        le_entradas(&self.session, true)
    }

    fn imprime_entradas(&self, rotulo: &str) {
        let todas = le_entradas(&self.session, false);
        self.nota(&format!("=== entradas ({rotulo}): {} ===", todas.len()));
        for (i, e) in todas.iter().enumerate() {
            self.nota(&format!("#{} [{}] {}: {}", i + 1, &e.conversa[..e.conversa.len().min(8)], e.tipo, e.texto));
        }
    }

    /// Espera `cond` por até `segundos`, olhando a cada segundo. Devolve se ela valeu.
    fn espera(&mut self, segundos: u64, cond: impl Fn(&Cena) -> bool) -> bool {
        let fim = Instant::now() + Duration::from_secs(segundos);
        loop {
            self.poll();
            if cond(self) {
                return true;
            }
            if Instant::now() > fim {
                return false;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    /// Espera o terminal ficar `quieto` segundos sem saída nova, por até `segundos`.
    fn quieta(&mut self, quieto: u64, segundos: u64) -> bool {
        let fim = Instant::now() + Duration::from_secs(segundos);
        let (mut ultimo, mut desde) = (self.seq(), Instant::now());
        loop {
            self.poll();
            let agora = self.seq();
            if agora != ultimo {
                (ultimo, desde) = (agora, Instant::now());
            }
            if desde.elapsed() >= Duration::from_secs(quieto) {
                return true;
            }
            if Instant::now() > fim {
                return false;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// Digita uma linha no orquestrador como o usuário digitaria: o texto e, depois de um instante, Enter.
    fn digita(&self, texto: &str) {
        self.core.term_write(ORCHESTRATOR, texto).unwrap();
        std::thread::sleep(Duration::from_millis(800));
        self.core.term_write(ORCHESTRATOR, "\r").unwrap();
    }

    /// Espera o TUI abrir e responde o diálogo de confiança na pasta, se aparecer.
    fn abre(&mut self) {
        std::thread::sleep(Duration::from_secs(8));
        self.quieta(5, 90);
        // Aviso de versão nova do Codex ("Update available", Enter = atualizar): Esc pula, sem instalar nada.
        if sem_ansi(&self.tela()).contains("Update available") {
            self.nota("diálogo de atualização do CLI: Esc (pular)");
            self.core.term_write(ORCHESTRATOR, "\x1b").unwrap();
            self.quieta(5, 60);
        }
        if self.tela().contains("trust") {
            self.nota("diálogo de confiança no orquestrador: respondido");
            self.core.term_write(ORCHESTRATOR, "\x1b[B").unwrap();
            std::thread::sleep(Duration::from_millis(300));
            self.core.term_write(ORCHESTRATOR, "\r").unwrap();
            self.quieta(5, 60);
        }
        self.foto("TUI aberto");
    }

    /// Uma rodada: digita `prompt` e espera a entrada `resposta` da rodada (o hook `Stop`).
    fn turno(&mut self, rotulo: &str, prompt: &str, segundos: u64) -> bool {
        // Conta em todas as conversas da sessão: depois de um `/clear` a corrente só muda (no Codex) com o próximo prompt.
        let total = |c: &Cena| conta(&le_entradas(&c.session, false), "resposta");
        let antes = total(self);
        self.nota(&format!(">>> {rotulo}: {prompt}"));
        self.digita(prompt);
        let ok = self.espera(segundos, |c| total(c) > antes);
        self.quieta(3, 30);
        self.foto(&format!("depois de {rotulo}"));
        ok
    }

    /// `/compact` no TUI. Espera o `PreCompact` gravar a entrada `compactacao` e o terminal assentar.
    fn compacta(&mut self, n: usize) -> bool {
        let antes = conta(&self.entradas(), "compactacao");
        self.nota(&format!(">>> compactação {n}: /compact"));
        self.digita("/compact");
        let mut ok = self.espera(20, |c| conta(&c.entradas(), "compactacao") > antes);
        if !ok {
            // O primeiro Enter pode só ter escolhido o comando no menu.
            self.foto(&format!("compactação {n}: sem entrada após 20 s, segundo Enter"));
            self.core.term_write(ORCHESTRATOR, "\r").unwrap();
            ok = self.espera(240, |c| conta(&c.entradas(), "compactacao") > antes);
        }
        self.quieta(10, 240);
        self.foto(&format!("depois da compactação {n}"));
        ok
    }

    fn transcript(&self) -> Option<PathBuf> {
        let atual = fs::read_to_string(self.session.join("memoria").join("atual")).ok()?;
        let path = fs::read_to_string(self.session.join("memoria").join(atual.trim()).join("transcript")).ok()?;
        Some(PathBuf::from(path.trim()))
    }

    fn linhas_do_transcript(&self) -> Vec<String> {
        self.transcript().and_then(|p| fs::read_to_string(p).ok()).map(|t| t.lines().map(String::from).collect()).unwrap_or_default()
    }

    /// Dá ao Core uma sessão nova no mesmo projeto e abre o TUI.
    fn inicia(&mut self) {
        self.core.start_orchestrator(self.escolha.clone(), 120, 40).unwrap();
        self.session = self.core.session_dir().unwrap();
        self.nota(&format!("sessão: {}", self.session.display()));
        self.abre();
    }
}

/// Apaga o repositório e a configuração descartáveis e fecha o Core, mesmo quando o teste falha no meio.
struct Limpeza {
    core: Core,
    repo: PathBuf,
    cfg: PathBuf,
}

impl Drop for Limpeza {
    fn drop(&mut self) {
        self.core.shutdown();
        for _ in 0..10 {
            if fs::remove_dir_all(&self.repo).is_ok() || !self.repo.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        let _ = fs::remove_file(&self.cfg);
        println!("repositório descartável removido: {}", !self.repo.exists());
    }
}

#[test]
#[ignore]
fn memoria_real() {
    let orquestrador = std::env::var("ORQ_REAL").unwrap_or("claude".into());
    let executor = std::env::var("ORQ_REAL_EXEC").unwrap_or("claude".into());
    let modelo_codex = std::env::var("ORQ_REAL_CODEX_MODEL").unwrap_or("gpt-5.6-luna".into());
    let barato = |tool: &str| if tool == "claude" { Choice::new("claude", "haiku", "") } else { Choice::new("codex", &modelo_codex, "low") };
    let temp = std::env::temp_dir();
    let prefixo = format!("orq-memoria-real-{orquestrador}");
    let log = |nome: &str| temp.join(format!("{prefixo}-{nome}"));

    // Repositório descartável com um commit, para o `git status` do orquestrador ter o que mostrar.
    let repo = temp.join(format!("{prefixo}-repo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&repo);
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "Teste"]);
    git(&repo, &["config", "user.email", "teste@example.com"]);
    fs::write(repo.join("README.md"), "repositório descartável do teste de memória\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "inicial"]);

    let exes = Exes { app: env!("CARGO_BIN_EXE_orquestrador").to_string(), ..Exes::real() };
    let (tx, events) = channel();
    let emit: Emit = Arc::new(move |event, payload| drop(tx.send((event.to_string(), payload))));
    let cfg = temp.join(format!("{prefixo}-config.json"));
    let core = Core::new(exes, Places::real(), cfg.clone(), emit);
    let _limpeza = Limpeza { core: core.clone(), repo: repo.clone(), cfg: cfg.clone() };
    let etapas = log("etapas.log");
    let _ = fs::remove_file(&etapas);
    core.set_project(&repo).unwrap();
    core.apply_executor_choice(barato(&executor)).unwrap();
    let mut c = Cena { core, events, session: PathBuf::new(), bytes: Vec::new(), etapas, relatorio: Vec::new(), escolha: barato(&orquestrador) };
    c.nota(&format!("orquestrador {:?}, executor {:?}, repositório {}", c.escolha, barato(&executor), repo.display()));
    c.inicia();
    let sessao1 = c.session.clone();

    // 1. Sessão nova: nenhuma entrada antes do primeiro prompt.
    let todas = le_entradas(&sessao1, false);
    c.confere("1. sessão nova sem entradas antes do primeiro prompt", todas.is_empty(), &format!("{} entradas; memoria\\ existe: {}", todas.len(), sessao1.join("memoria").exists()));

    // 2. Primeiro fato.
    let pedido1 = format!("Decisão registrada: o codinome do projeto é {CODINOME}. Responda só OK.");
    let ok = c.turno("pedido 1", &pedido1, 240);
    c.confere("2. pedido e resposta do primeiro fato gravados", ok, "");

    // 3. Primeira compactação, e uma rodada curta depois (no Codex o SessionStart(compact) só dispara nela).
    let ok = c.compacta(1);
    c.confere("3. 1ª compactação gravou a entrada `compactacao`", ok, "");
    let ok = c.turno("intervalo 1", "Intervalo 1: responda só OK.", 240);
    c.confere("3b. rodada depois da 1ª compactação respondida", ok, "");

    // 4. Segundo fato e uma delegação pequena, com o executor barato.
    let pedido2 = format!("Decisão registrada: o prazo do projeto é {PRAZO}. Sem perguntas e sem skills: use a ferramenta delegar agora, numa única chamada, com uma tarefa: crie o arquivo memoria-e2e.txt na raiz do repositório contendo exatamente o texto {TEXTO} e mais nada. Quando a ferramenta voltar, responda só RECEBIDO.");
    c.nota(&format!(">>> pedido 2 (fato + delegação): {pedido2}"));
    let antes = conta(&c.entradas(), "resposta");
    c.digita(&pedido2);
    let ok = c.espera(480, |c| {
        let e = c.entradas();
        conta(&e, "resposta") > antes && conta(&e, "lote") >= 1 && conta(&e, "resultado") >= 1
    });
    c.quieta(3, 30);
    c.foto("depois da delegação");
    let entradas = c.entradas();
    let lote = entradas.iter().find(|e| e.tipo == "lote");
    let resultado = entradas.iter().find(|e| e.tipo == "resultado");
    c.confere("4a. rodada da delegação fechou (resposta, lote e resultado)", ok, "");
    c.confere("4b. entrada `lote` com o texto literal da tarefa", lote.is_some_and(|e| e.texto.contains(TEXTO) && e.texto.contains("--- TAREFA 1/1 ---")), &format!("{:?}", lote.map(|e| &e.texto)));
    c.confere("4c. entrada `resultado` com a saída entregue", resultado.is_some_and(|e| e.texto.contains("--- FIM ---") && e.texto.contains("executor_exit")), &format!("{:?}", resultado.map(|e| &e.texto)));
    c.confere("4d. pedido 2 gravado literal", entradas.iter().any(|e| e.tipo == "pedido" && e.texto == pedido2), "");
    let arquivo = fs::read_to_string(repo.join("memoria-e2e.txt")).unwrap_or_default();
    c.confere("4e. o executor criou o arquivo da tarefa", arquivo.contains(TEXTO), &format!("{arquivo:?}"));

    // 5. Mais duas compactações, com rodadas curtas entre elas.
    let ok = c.compacta(2);
    c.confere("5a. 2ª compactação gravou a entrada", ok, "");
    let ok = c.turno("intervalo 2", "Intervalo 2: responda só OK.", 240);
    c.confere("5b. rodada depois da 2ª compactação respondida", ok, "");
    let ok = c.compacta(3);
    c.confere("5c. 3ª compactação gravou a entrada", ok, "");

    // 6. A pergunta final, sem nenhuma rodada entre a última compactação e ela.
    let pergunta = format!("Pergunta final, sem delegar: qual é o codinome do projeto que registrei no começo desta conversa e qual é o {MARCA} no arquivo da tarefa delegada? Responda numa linha só, no formato CODINOME=<valor>; TEXTO=<valor>.");
    let ok = c.turno("pergunta final", &pergunta, 300);
    let resposta = c.entradas().iter().rev().find(|e| e.tipo == "resposta").map(|e| e.texto.clone()).unwrap_or_default();
    c.nota(&format!("RESPOSTA FINAL: {resposta}"));
    c.confere("6a. a rodada da pergunta fechou", ok, "");
    c.confere("6b. acertou o codinome", resposta.contains(CODINOME), &resposta);
    c.confere("6c. acertou o texto da tarefa delegada", resposta.contains(TEXTO), &resposta);
    let linhas = c.linhas_do_transcript();
    let ferramentas = ferramentas_apos(&linhas, MARCA);
    let lembranca = linhas.iter().filter(|l| l.contains("Memória do app") || l.contains("Mem\\u00f3ria do app")).count();
    let usou_ferramenta = ferramentas.keys().any(|n| n.contains("memoria"));
    c.nota(&format!("ferramentas chamadas na rodada da pergunta: {ferramentas:?}"));
    c.nota(&format!("linhas do transcript com a lembrança injetada (\"Memória do app\"): {lembranca} de {}", linhas.len()));
    c.nota(&format!("usou a ferramenta `memoria`: {usou_ferramenta}; lembrança injetada no transcript: {}", lembranca > 0));

    // 7. Em disco.
    let todas = le_entradas(&sessao1, false);
    c.imprime_entradas("passo 7");
    let atual = c.entradas();
    let conversas: Vec<&str> = todas.iter().map(|e| e.conversa.as_str()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    c.confere("7a. uma só conversa na sessão", conversas.len() == 1, &format!("{conversas:?}"));
    for tipo in ["pedido", "resposta", "lote", "resultado"] {
        c.confere(&format!("7b. entradas `{tipo}` existem"), conta(&atual, tipo) >= 1, &format!("{}", conta(&atual, tipo)));
    }
    c.confere("7c. 3 entradas `compactacao`", conta(&atual, "compactacao") == 3, &format!("{}", conta(&atual, "compactacao")));
    c.confere("7d. o primeiro pedido continua íntegro", atual.first().is_some_and(|e| e.tipo == "pedido" && e.texto == pedido1), &format!("{:?}", atual.first().map(|e| &e.texto)));
    let anterior = linhas.iter().position(|l| l.contains(CODINOME));
    c.confere("7e. o transcript ainda contém o texto anterior à 1ª compactação", anterior.is_some() && !linhas.is_empty(), &format!("{} linhas; primeira com {CODINOME}: {anterior:?}", linhas.len()));
    if let Some(origem) = c.transcript() {
        let _ = fs::copy(&origem, log("transcript.jsonl"));
        c.nota(&format!("transcript: {}", origem.display()));
    }

    // 9. Escopo (antes do shutdown, enquanto a pasta da sessão existe): o executor não recebe a memória.
    let exec = sessao1.join("exec");
    let mut settings = Vec::new();
    for no in fs::read_dir(&exec).into_iter().flatten().flatten() {
        if let Ok(texto) = fs::read_to_string(no.path().join("settings.json")) {
            settings.push((no.file_name().to_string_lossy().to_string(), texto));
        }
    }
    for (no, texto) in &settings {
        c.nota(&format!("=== exec\\{no}\\settings.json ===\n{texto}"));
    }
    if executor == "claude" {
        c.confere("9a. há settings de executor para conferir", !settings.is_empty(), "");
    }
    let vazamento = settings.iter().any(|(_, t)| t.contains("--memoria") || ["SessionStart", "UserPromptSubmit", "PreCompact"].iter().any(|e| t.contains(e)));
    c.confere("9b. settings dos executores sem `--memoria` nem hooks da memória", !vazamento, "");
    let da_tarefa = todas.iter().any(|e| e.texto.contains("Execute exatamente a tarefa abaixo"));
    c.confere("9c. nenhum prompt de executor virou entrada da memória", !da_tarefa, "");

    // Logs da primeira sessão, antes de ela sumir.
    c.poll();
    fs::write(log("tela.log"), sem_ansi(&c.tela())).unwrap();
    fs::write(log("bytes.log"), &c.bytes).unwrap();
    let entradas_txt: String = todas.iter().enumerate().map(|(i, e)| format!("#{} {}: {}\n----\n", i + 1, e.tipo, e.texto)).collect();
    fs::write(log("memoria.txt"), entradas_txt).unwrap();

    // 8. Isolamento: a sessão some com o Core, e a seguinte, no mesmo repositório, nasce sem memória.
    c.core.shutdown();
    c.poll();
    c.confere("8a. a pasta da sessão sumiu no shutdown", !sessao1.exists(), "");
    // O arquivo da tarefa é estado do repositório, não memória: a sessão nova o acharia com `git status` (medido no
    // Codex) e responderia o texto. Sai antes, para o que a sessão nova souber só poder vir da memória.
    let _ = fs::remove_file(repo.join("memoria-e2e.txt"));
    c.bytes.clear();
    c.inicia();
    let sessao2 = c.session.clone();
    c.confere("8b. sessão nova é outra pasta", sessao2 != sessao1, &format!("{}", sessao2.display()));
    let vazias = le_entradas(&sessao2, false);
    c.confere("8c. a memória da sessão nova está vazia", vazias.is_empty(), &format!("{} entradas", vazias.len()));
    let ok = c.turno("isolamento 1", "Qual é o codinome do projeto que registrei? Responda numa linha só; se não souber, responda NAO-SEI.", 240);
    let r1 = c.entradas().iter().rev().find(|e| e.tipo == "resposta").map(|e| e.texto.clone()).unwrap_or_default();
    c.nota(&format!("RESPOSTA ISOLAMENTO 1: {r1}"));
    c.confere("8d. a rodada de isolamento fechou e a resposta não traz o codinome", ok && !r1.contains("4821") && !r1.contains(CODINOME), &r1);
    // A ferramenta lida direto sobre a pasta da sessão nova, antes de qualquer prompt que cite os termos: nada a achar.
    for termo in ["GIRASSOL", "LANTERNA", "SETEMBRO", "GIRASSOL-4821 LANTERNA-9035"] {
        let direto = orquestrador_lib::agents::memory::tool(&sessao2, &serde_json::json!({ "consulta": termo }));
        c.nota(&format!("memory::tool(consulta {termo:?}) na sessão nova: {direto:?}"));
        c.confere(&format!("8e1. memory::tool direto, consulta {termo:?}, não acha nada"), direto.as_deref().is_ok_and(|t| t.starts_with("Nada encontrado")), "");
    }
    // Pelo modelo: o pedido e a chamada da ferramenta citam o termo e entram no transcript, então a consulta acha
    // esses dois (e só eles): nenhuma entrada `resposta`/`lote`/`resultado` e nenhum texto da sessão anterior.
    let ok = c.turno("isolamento 2", "Chame a ferramenta memoria com consulta GIRASSOL e responda numa linha só o que ela devolveu.", 240);
    let r2 = c.entradas().iter().rev().find(|e| e.tipo == "resposta").map(|e| e.texto.clone()).unwrap_or_default();
    c.nota(&format!("RESPOSTA ISOLAMENTO 2: {r2}"));
    let linhas2 = c.linhas_do_transcript();
    let ferramentas2 = ferramentas_apos(&linhas2, "consulta GIRASSOL");
    c.nota(&format!("ferramentas chamadas: {ferramentas2:?}"));
    let mut saidas: Vec<String> = Vec::new();
    for linha in &linhas2 {
        let Ok(v) = serde_json::from_str::<Value>(linha) else { continue };
        let mut textos = Vec::new();
        strings(&v, &mut textos);
        for t in textos {
            if (t.starts_with("Resultados, melhores primeiro") || t.starts_with("Nada encontrado para:")) && !saidas.contains(&t) {
                saidas.push(t);
            }
        }
    }
    for s in &saidas {
        c.nota(&format!("saída da ferramenta `memoria` (transcript da sessão nova):\n{s}"));
    }
    let so_proprio = |s: &String| !s.contains("4821") && !s.contains("LANTERNA") && !s.contains("SETEMBRO") && !s.lines().any(|l| ["resposta", "lote", "resultado", "compactacao"].iter().any(|k| l.split_once(' ').is_some_and(|(id, resto)| id.starts_with('e') && resto.starts_with(&format!("{k}:")))));
    c.confere("8e2. a ferramenta `memoria` foi chamada e só achou o que a sessão nova disse", ok && !saidas.is_empty() && saidas.iter().all(so_proprio) && ferramentas2.keys().any(|n| n.contains("memoria")), &r2);
    c.confere("8f. nada da sessão anterior no transcript novo", !linhas2.iter().any(|l| l.contains("4821")), "");

    // 8g (opcional). `/clear` abre outra conversa vazia na mesma sessão.
    let antes = fs::read_to_string(sessao2.join("memoria").join("atual")).unwrap_or_default();
    c.digita("/clear");
    c.espera(30, |c| fs::read_to_string(c.session.join("memoria").join("atual")).unwrap_or_default() != antes);
    c.quieta(5, 30);
    let depois = fs::read_to_string(sessao2.join("memoria").join("atual")).unwrap_or_default();
    c.nota(&format!("/clear: conversa antes {:?}, depois {:?}", antes.trim(), depois.trim()));
    let ok = c.turno("pós /clear", "Pós-clear: responda só OK.", 240);
    let nova = c.entradas();
    c.nota(&format!("/clear: conversa corrente tem {} entradas ({} pedidos)", nova.len(), conta(&nova, "pedido")));
    c.imprime_entradas("depois do /clear");
    let atual_final = fs::read_to_string(sessao2.join("memoria").join("atual")).unwrap_or_default();
    c.confere("8g. /clear abriu outra conversa, sem os pedidos da anterior", ok && atual_final.trim() != antes.trim() && conta(&nova, "pedido") == 1, &format!("{:?} -> {:?}", antes.trim(), atual_final.trim()));
    c.poll();
    fs::write(log("tela-sessao2.log"), sem_ansi(&c.tela())).unwrap();
    c.core.shutdown();

    println!("\n=== RELATÓRIO ({orquestrador}) ===");
    for (nome, ok) in &c.relatorio {
        println!("[{}] {nome}", if *ok { "PASSOU" } else { "FALHOU" });
    }
    println!("logs: {}-*", temp.join(&prefixo).display());
    let falhas: Vec<&str> = c.relatorio.iter().filter(|(_, ok)| !ok).map(|(n, _)| n.as_str()).collect();
    assert!(falhas.is_empty(), "falharam: {falhas:?}");
}
