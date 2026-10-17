//! Atualização automática na abertura. Antes de liberar o projeto ou qualquer terminal, o app confere
//! a última versão estável no GitHub. Se houver uma superior, baixa, valida a assinatura, instala e
//! reabre sozinho. Qualquer falha mantém o uso bloqueado, com o motivo e uma nova tentativa. O build
//! de desenvolvimento libera o uso direto e não consulta nada.
use crate::core::Emit;
use serde::Serialize;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Tempo máximo da consulta ao GitHub; passado disso, é falha.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Tempo máximo do download do instalador (o plugin aplica esse limite à requisição inteira).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Intervalo mínimo entre dois avisos de progresso, para a janela não ser inundada.
const PROGRESS_STEP: u64 = 256 * 1024;

/// O estado da verificação, como a tela o mostra. Vai ao front por evento e por `update_status`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum Status {
    /// Consultando o GitHub.
    Checking,
    /// Baixando a versão superior; `total` é `None` quando o servidor não informa o tamanho.
    Downloading { version: String, downloaded: u64, total: Option<u64> },
    /// Baixada e com a assinatura validada; instalando. No Windows o app encerra e o instalador o reabre.
    Installing { version: String },
    /// Versão em dia: o uso está liberado.
    Ready,
    /// Falhou: o uso continua bloqueado até uma nova tentativa dar certo.
    Failed { message: String },
}

/// O portão do uso: guarda o estado e diz se o app já pode ser usado.
#[derive(Clone)]
pub struct Gate {
    state: Arc<Mutex<State>>,
    emit: Emit,
}

struct State {
    status: Status,
    /// Uma verificação ou instalação em curso: nenhuma outra começa.
    busy: bool,
}

impl Gate {
    /// `bypass` libera o uso direto (build de desenvolvimento); nesse caso a verificação nem começa.
    pub fn new(bypass: bool, emit: Emit) -> Gate {
        let status = if bypass { Status::Ready } else { Status::Checking };
        Gate { state: Arc::new(Mutex::new(State { status, busy: false })), emit }
    }

    pub fn status(&self) -> Status {
        self.state.lock().unwrap().status.clone()
    }

    /// Erro para qualquer comando de trabalho enquanto a versão não estiver em dia.
    pub fn ensure_open(&self) -> Result<(), String> {
        match self.status() {
            Status::Ready => Ok(()),
            _ => Err("O app ainda não pode ser usado: a atualização precisa estar concluída.".into()),
        }
    }

    /// Reserva a rodada. Só uma por vez, e nunca depois que o uso foi liberado.
    fn reserve(&self) -> bool {
        {
            let mut state = self.state.lock().unwrap();
            if state.busy || state.status == Status::Ready {
                return false;
            }
            state.busy = true;
        }
        self.change(None, Status::Checking);
        true
    }

    fn set(&self, status: Status) {
        self.change(None, status);
    }

    /// Encerra a rodada: o estado final e a liberação da reserva saem juntos.
    fn finish(&self, status: Status) {
        self.change(Some(false), status);
    }

    /// Troca o estado e avisa a tela, se ele mudou.
    fn change(&self, busy: Option<bool>, status: Status) {
        let changed = {
            let mut state = self.state.lock().unwrap();
            if let Some(busy) = busy {
                state.busy = busy;
            }
            let changed = state.status != status;
            state.status = status.clone();
            changed
        };
        if changed {
            (self.emit)("update-status", json!(status));
        }
    }
}

/// Uma rodada completa pelo plugin oficial do Tauri, com a fonte de `plugins.updater` em `tauri.conf.json`:
/// consulta, baixa e instala se houver versão superior. Devolve o estado final.
async fn attempt(app: &AppHandle, gate: &Gate) -> Status {
    let checked = async { app.updater_builder().timeout(CHECK_TIMEOUT).build()?.check().await }.await;
    let mut update = match checked {
        Ok(Some(update)) => update,
        Ok(None) => return Status::Ready,
        Err(why) => return Status::Failed { message: format!("Não foi possível verificar atualizações: {}.", explain(why)) },
    };
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    let version = update.version.clone();
    gate.set(Status::Downloading { version: version.clone(), downloaded: 0, total: None });
    let (mut downloaded, mut shown) = (0u64, 0u64);
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if downloaded - shown >= PROGRESS_STEP || total == Some(downloaded) {
                    shown = downloaded;
                    gate.set(Status::Downloading { version: version.clone(), downloaded, total });
                }
            },
            || {},
        )
        .await;
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(why) => return Status::Failed { message: format!("Não foi possível baixar a atualização {version}: {}.", explain(why)) },
    };
    gate.set(Status::Installing { version: version.clone() });
    // No Windows o processo encerra no `install`; nos outros sistemas, o app reabre aqui.
    match update.install(&bytes) {
        Ok(()) => app.restart(),
        Err(why) => Status::Failed { message: format!("Não foi possível instalar a atualização {version}: {}.", explain(why)) },
    }
}

/// Verificação da abertura e do "Tentar novamente": começa a rodada se não houver outra em curso e o uso
/// ainda não foi liberado.
pub fn check(app: &AppHandle, gate: &Gate) {
    if !gate.reserve() {
        return;
    }
    let (app, gate) = (app.clone(), gate.clone());
    tauri::async_runtime::spawn(async move { settle(&app, &gate).await });
}

/// Roda a rodada e registra o fim dela. Se algo estourar um pânico, a tela recebe a falha: o uso
/// fica bloqueado com `Tentar novamente`, e não preso em "verificando".
async fn settle(app: &AppHandle, gate: &Gate) {
    struct Abort<'a>(Option<&'a Gate>);
    impl Drop for Abort<'_> {
        fn drop(&mut self) {
            if let Some(gate) = self.0 {
                gate.finish(Status::Failed { message: "A verificação parou inesperadamente.".into() });
            }
        }
    }
    let mut abort = Abort(Some(gate));
    let status = attempt(app, gate).await;
    abort.0 = None;
    gate.finish(status);
}

/// O motivo de uma falha em português, com o detalhe técnico quando ele ajuda.
fn explain(error: tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    match error {
        Error::Reqwest(e) if e.is_timeout() => "o GitHub não respondeu a tempo".into(),
        Error::Reqwest(e) if e.is_connect() => "sem conexão com o GitHub".into(),
        Error::Reqwest(e) => format!("falha de rede ({e})"),
        Error::ReleaseNotFound => "nenhuma versão estável publicada foi encontrada".into(),
        Error::Minisign(_) | Error::SignedVersionMismatch { .. } | Error::MissingSignedVersion => {
            "a assinatura da atualização não confere, então ela foi recusada".into()
        }
        Error::Network(detail) => format!("o servidor recusou o download ({detail})"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use serde_json::{json, Value};

    /// Um portão e o registro de tudo que ele mandou para a tela.
    fn new_gate(bypass: bool) -> (Gate, Arc<Mutex<Vec<Value>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        let emit: Emit = Arc::new(move |event, payload| {
            assert_eq!(event, "update-status");
            sink.lock().unwrap().push(payload);
        });
        (Gate::new(bypass, emit), log)
    }

    #[test]
    fn desenvolvimento_fica_liberado_e_nao_consulta() {
        let (gate, log) = new_gate(true);
        assert_eq!(gate.status(), Status::Ready);
        assert!(gate.ensure_open().is_ok());
        assert!(!gate.reserve(), "com o bypass nenhuma rodada começa");
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn nao_inicia_verificacao_duplicada_enquanto_outra_roda() {
        let (gate, _) = new_gate(false);
        assert!(gate.reserve());
        assert!(!gate.reserve(), "a segunda consulta não começa");
        gate.finish(Status::Failed { message: "x".into() });
        assert!(gate.reserve(), "depois da falha, a nova tentativa começa");
    }

    #[test]
    fn estado_chega_a_tela_no_formato_que_ela_le() {
        let v = |s: Status| serde_json::to_value(s).unwrap();
        assert_eq!(v(Status::Checking), json!({"phase": "checking"}));
        assert_eq!(v(Status::Ready), json!({"phase": "ready"}));
        assert_eq!(
            v(Status::Downloading { version: "0.2.0".into(), downloaded: 5, total: None }),
            json!({"phase": "downloading", "version": "0.2.0", "downloaded": 5, "total": null})
        );
        assert_eq!(v(Status::Failed { message: "x".into() }), json!({"phase": "failed", "message": "x"}));
    }

    #[test]
    fn configuracao_do_updater_fica_na_fonte_oficial_sem_downgrade() {
        let conf: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert_eq!(
            updater["endpoints"],
            json!(["https://github.com/luansilvadb/orquestrador/releases/latest/download/latest.json"]),
            "só a última release publicada (sem drafts nem prereleases)"
        );
        // Uma chave de verdade: o arquivo `.pub` do minisign decodificado, não um texto de exemplo.
        let pubkey = base64::engine::general_purpose::STANDARD.decode(updater["pubkey"].as_str().unwrap()).unwrap();
        assert!(String::from_utf8(pubkey).unwrap().starts_with("untrusted comment: minisign public key: "));
        assert_eq!(updater["requireSignedVersion"], true, "a versão anunciada precisa ser a assinada");
        assert_eq!(updater["allowDowngrades"], false);
        assert_ne!(updater["dangerousInsecureTransportProtocol"], true);
        assert_eq!(updater["windows"]["installMode"], "quiet", "atualização silenciosa");
        assert_eq!(conf["bundle"]["createUpdaterArtifacts"], true);
        assert_eq!(conf["bundle"]["targets"], json!(["nsis"]));
        assert_eq!(conf["bundle"]["windows"]["nsis"]["installMode"], "currentUser", "instala por usuário, sem admin");
    }
}
