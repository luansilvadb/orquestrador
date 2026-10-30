//! O lote do orquestrador: publica as tarefas na pasta da sessão e espera o resultado que o app grava. Quem chama
//! é o servidor MCP (`mcp.rs`): `delegar` publica e espera, e `aguardar` volta a esperar o lote mais recente ainda
//! não entregue.
//! O app lê `requests\` e grava `results\<lote>.txt`; este módulo nunca escreve o resultado. A marca
//! `results\<lote>.delivered` é gravada por quem chama, só depois que a resposta saiu: sem ela, `aguardar`
//! pega o lote de novo.
use crate::agents::hook;
use crate::agents::session::owner_alive;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Quanto uma chamada espera o lote antes de devolver `--- EM ANDAMENTO ---`.
pub const WINDOW: Duration = Duration::from_secs(840);
const POLL: Duration = Duration::from_millis(250);
const NOT_ACTIVE: &str = "A pasta não é uma sessão ativa do orquestrador.";
const DEAD: &str = "A sessão do orquestrador terminou antes do fim do lote.";

/// O que a espera devolve: o resultado do lote, com a marca que só deve ser gravada depois da entrega, ou o prazo
/// sem resultado.
#[derive(Debug)]
pub enum Waited {
    Result { text: String, marker: PathBuf },
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

/// O manifesto de um lote: as tarefas, na ordem, num arquivo só. O app lê este arquivo de uma vez, então um lote
/// não pode ser aceito pela metade.
pub const MANIFEST: &str = "tarefas.json";

/// Publica as tarefas como um lote novo, na ordem em que vieram, exatamente como vieram. O manifesto é o único
/// arquivo do lote e vai junto com a pasta, publicada por renomeação.
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
    let manifest = serde_json::to_vec(tasks).map_err(|e| format!("Não foi possível publicar o lote: {e}"))?;
    std::fs::create_dir(&tmp).map_err(io)?;
    std::fs::write(tmp.join(MANIFEST), manifest).map_err(io)?;
    std::fs::rename(&tmp, requests.join(&id)).map_err(io)?;
    Ok(id)
}

/// Lê o manifesto de um lote publicado. Devolve a razão quando o arquivo não abre, não é uma lista de textos ou tem
/// tarefa vazia: quem chama recusa o lote inteiro, porque nenhuma tarefa roda sem o lote completo.
pub fn read_manifest(batch: &Path) -> Result<Vec<String>, String> {
    let file = batch.join(MANIFEST);
    let bytes = std::fs::read(&file).map_err(|e| format!("não foi possível abrir {}: {e}", file.display()))?;
    let tasks: Vec<String> =
        serde_json::from_slice(&bytes).map_err(|e| format!("o manifesto não é uma lista de textos: {e}"))?;
    if tasks.is_empty() {
        return Err("o lote chegou sem tarefas".into());
    }
    if tasks.iter().any(|task| task.trim().is_empty()) {
        return Err("o lote tem tarefa vazia".into());
    }
    Ok(tasks)
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

/// Espera o resultado do lote por até `window`. A espera acaba com o resultado, que segue inteiro ao orquestrador,
/// ou com o prazo. Se a sessão morrer antes, falha em vez de esperar para sempre.
pub fn wait(session: &Path, id: &str, window: Duration) -> Result<Waited, String> {
    let result = session.join("results").join(format!("{id}.txt"));
    let started = Instant::now();
    loop {
        if result.is_file() {
            let text = std::fs::read_to_string(&result).map_err(|e| format!("Não foi possível ler {}: {e}", result.display()))?;
            let marker = session.join("results").join(format!("{id}.delivered"));
            return Ok(Waited::Result { text, marker });
        }
        if !owner_alive(session) {
            return Err(DEAD.into());
        }
        if started.elapsed() >= window {
            return Ok(Waited::Running);
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::session;

    #[test]
    fn lote_sai_inteiro_e_renomeado_com_o_texto_exato() {
        let dir = &session("orq-delegar", "publica", true).0;
        let tasks = ["Tarefa A\nlinha 2 d'água", "Diga \"oi\" e \\\"tchau\\\" e a barra \\", "{\"a\":\"b\\\"c\"}"];
        let id = publish(&dir, &tasks.map(String::from)).unwrap();
        let batch = dir.join("requests").join(&id);
        let manifest: Vec<String> = serde_json::from_slice(&std::fs::read(batch.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(manifest, tasks, "o texto exato, na ordem, no manifesto");
        assert_eq!(read_manifest(&batch).unwrap(), tasks, "o leitor devolve o que o escritor gravou");
        let names: Vec<String> = std::fs::read_dir(&batch).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, [MANIFEST], "o manifesto é o único arquivo do lote");
        let names: Vec<String> = std::fs::read_dir(dir.join("requests")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, [id]);
    }

    #[test]
    fn tarefa_invalida_nao_publica_nada() {
        let dir = &session("orq-delegar", "invalida", true).0;
        assert!(publish(&dir, &[]).unwrap_err().contains("Nenhuma tarefa"));
        assert!(publish(&dir, &["ok".into(), "  \n".into()]).unwrap_err().contains("Tarefa vazia"));
        assert_eq!(std::fs::read_dir(dir.join("requests")).unwrap().count(), 0);
    }

    #[test]
    fn aguardar_pega_o_lote_mais_recente_que_nao_foi_entregue() {
        let dir = &session("orq-delegar", "retoma", true).0;
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
    fn espera_devolve_o_resultado_inteiro_e_a_marca() {
        let dir = &session("orq-delegar", "espera", true).0;
        std::fs::write(dir.join("results").join("x.txt"), "--- EXECUTOR 1/1 ---\nok\n").unwrap();
        let Waited::Result { text, marker } = wait(&dir, "x", WINDOW).unwrap() else { panic!("o resultado devia sair") };
        assert_eq!(text, "--- EXECUTOR 1/1 ---\nok\n");
        assert!(marker.ends_with("x.delivered"));
    }

    #[test]
    fn sessao_morta_com_lote_pendente_falha_em_vez_de_esperar() {
        let dir = &session("orq-delegar", "morta", false).0;
        assert_eq!(wait(&dir, "nunca", WINDOW).unwrap_err(), DEAD);
    }

    #[test]
    fn pasta_sem_dono_nao_e_sessao_ativa() {
        let dir = &session("orq-delegar", "ativa", true).0;
        assert!(check_session(&dir).is_ok());
        let nowhere = std::env::temp_dir().join(format!("orq-delegar-{}-inexistente", std::process::id()));
        assert_eq!(check_session(&nowhere).unwrap_err(), NOT_ACTIVE);
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
