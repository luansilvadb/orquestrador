//! O programa de delegação do orquestrador, `orquestrador-delegar.exe`, que vai junto com o app. Quem o
//! chama é o adaptador `.ps1` da sessão (`adapter`), pelo nome que o orquestrador digita:
//!   orq-<id>.ps1 'tarefa A' 'tarefa B'
//!   orq-<id>.ps1 '--aguardar'
//! O adaptador só transporta: manda ao programa, pela entrada padrão, uma linha com a pasta da sessão e cada
//! texto em base64 (`decode`), separados por `;`. Nenhum argumento leva o lote, porque a linha de comando do
//! Windows tem limite de 32767 caracteres e o base64 cresce um terço. O lote vai para a pasta da sessão de
//! uma vez: a pasta é montada com outro nome e renomeada no fim, então o app nunca pega um lote pela metade.
//! O app grava o resultado em `results\<lote>.txt`; este programa espera esse arquivo e imprime o texto em
//! UTF-8. `--aguardar` volta a esperar o lote mais recente que ainda não foi entregue, sem publicar nada. Toda
//! falha do próprio programa imprime `wrapper_error:`, fecha com `--- FIM ---` e sai com 4.
use crate::session::owner_alive;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE};

/// O nome do programa: fica ao lado de `orquestrador.exe`, instalado ou em desenvolvimento.
pub const PROGRAM: &str = "orquestrador-delegar.exe";
/// Volta a esperar o lote mais recente ainda não entregue.
const RESUME: &str = "--aguardar";
const USAGE: &str = "Uso: o comando de delegação da sessão, com uma tarefa entre aspas simples por argumento.";
/// Código de saída quando o próprio comando falha, antes ou depois do lote.
const WRAPPER_ERROR: i32 = 4;
const POLL: Duration = Duration::from_millis(250);
const NOT_ACTIVE: &str = "A pasta não é uma sessão ativa do orquestrador.";
const DEAD: &str = "A sessão do orquestrador terminou antes do fim do lote.";

/// Ponto de entrada do programa: `args` são os argumentos depois do nome dele, que devem vir vazios, porque o
/// lote vem pela entrada padrão. Devolve o código de saída.
pub fn run(args: &[OsString]) -> i32 {
    if !has_stdout() {
        // Em modo janela o stdout herdado pode não existir. Sem ele ninguém receberia o resultado,
        // então o lote nem chega a ser publicado.
        let _ = writeln!(std::io::stderr(), "wrapper_error: não há saída padrão para devolver o resultado.");
        return WRAPPER_ERROR;
    }
    let (code, text, marker) = match execute(args, std::io::stdin().lock()) {
        Ok(done) => (done.code, done.text, Some(done.marker)),
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

/// O que sai de um lote: o código de saída, o texto para o orquestrador e a marca de entregue.
#[derive(Debug)]
struct Done {
    code: i32,
    text: String,
    marker: PathBuf,
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
        _ => publish(&session, rest)?,
    };
    let (code, text) = wait(&session, &id)?;
    Ok(Done { code, text, marker: session.join("results").join(format!("{id}.delivered")) })
}

/// A linha que o adaptador manda: a pasta da sessão e as tarefas, cada texto em base64 do UTF-8, separados por
/// `;`. Base64 não tem espaço nem aspas, então nada se perde no transporte; e um texto vazio continua sendo um
/// pedaço, porque o separador o segura.
pub fn decode(line: &str) -> Result<Vec<String>, String> {
    if line.is_empty() {
        return Ok(Vec::new());
    }
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
    let id = batch_id();
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

/// Nome do lote. Os nanossegundos com zeros à esquerda fazem a ordem alfabética ser a cronológica,
/// que `--aguardar` usa para achar o mais recente; o sufixo separa dois lotes no mesmo instante.
fn batch_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    format!("{nanos:020}-{}", &suffix[..8])
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
/// morrer antes, falha em vez de esperar para sempre.
fn wait(session: &Path, id: &str) -> Result<(i32, String), String> {
    let result = session.join("results").join(format!("{id}.txt"));
    while !result.is_file() {
        if !owner_alive(session) {
            return Err(DEAD.into());
        }
        std::thread::sleep(POLL);
    }
    let text = std::fs::read_to_string(&result).map_err(|e| format!("Não foi possível ler {}: {e}", result.display()))?;
    let (code, body) = text.split_once('\n').ok_or("o resultado do lote não traz o código de saída.")?;
    let code: i32 = code.trim().parse().map_err(|_| format!("código de saída ilegível no resultado do lote: {code:?}"))?;
    Ok((code, body.to_string()))
}

/// Se há um stdout para devolver o resultado. Processo de janela pode não ter um.
fn has_stdout() -> bool {
    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    !handle.is_null() && handle != INVALID_HANDLE_VALUE
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
        assert_eq!(wait(&dir, "x").unwrap(), (1, "--- EXECUTOR 1/1 ---\nok\n".to_string()));
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
        let first = batch_id();
        std::thread::sleep(Duration::from_millis(2));
        let second = batch_id();
        assert!(first < second, "{first} < {second}");
        assert_eq!(first.as_bytes()[20], b'-');
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
