//! Atualização automática na abertura. Antes de liberar o projeto ou qualquer terminal, o app confere
//! a última versão estável no GitHub. Se houver uma superior, baixa, valida a assinatura, instala e
//! reabre sozinho. Qualquer falha mantém o uso bloqueado, com o motivo e uma nova tentativa. O build
//! de desenvolvimento libera o uso direto e não consulta nada.
use crate::core::Emit;
use serde::Serialize;
use serde_json::json;
use std::future::Future;
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

/// De onde vêm as versões. Em produção é o plugin do Tauri; nos testes, uma falsa.
pub trait Remote: Send + Sync + 'static {
    /// Atualização anunciada e ainda não baixada.
    type Pending: Send + Sync;

    /// Consulta o endpoint. `Ok(None)` quando não há versão superior.
    fn check(&self) -> impl Future<Output = Result<Option<Self::Pending>, String>> + Send;

    fn version(&self, pending: &Self::Pending) -> String;

    /// Baixa e valida a assinatura. `on_chunk` recebe os bytes de cada pedaço e o tamanho total, se houver.
    fn download(
        &self,
        pending: &Self::Pending,
        on_chunk: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> impl Future<Output = Result<Vec<u8>, String>> + Send;

    /// Lança o instalador. No Windows o processo encerra daqui; nos outros sistemas volta `Ok`.
    fn install(&self, pending: &Self::Pending, bytes: &[u8]) -> Result<(), String>;

    /// Reabre o app, quando a instalação não o fez sozinha.
    fn relaunch(&self);
}

/// Uma rodada completa: consulta, baixa e instala se houver versão superior. Devolve o estado final.
async fn attempt<R: Remote>(gate: &Gate, remote: &R, current: &str) -> Status {
    let pending = match remote.check().await {
        Ok(Some(pending)) => pending,
        Ok(None) => return Status::Ready,
        Err(why) => return Status::Failed { message: format!("Não foi possível verificar atualizações: {why}.") },
    };
    let version = remote.version(&pending);
    // Só instala versão mais nova. O endpoint não é assinado, então a comparação fica aqui também.
    if !is_newer(&version, current) {
        return Status::Ready;
    }
    gate.set(Status::Downloading { version: version.clone(), downloaded: 0, total: None });
    let (mut downloaded, mut shown) = (0u64, 0u64);
    let bytes = remote
        .download(&pending, &mut |chunk: u64, total: Option<u64>| {
            downloaded += chunk;
            if downloaded - shown >= PROGRESS_STEP || total == Some(downloaded) {
                shown = downloaded;
                gate.set(Status::Downloading { version: version.clone(), downloaded, total });
            }
        })
        .await;
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(why) => return Status::Failed { message: format!("Não foi possível baixar a atualização {version}: {why}.") },
    };
    gate.set(Status::Installing { version: version.clone() });
    match remote.install(&pending, &bytes) {
        Ok(()) => {
            remote.relaunch();
            Status::Installing { version }
        }
        Err(why) => Status::Failed { message: format!("Não foi possível instalar a atualização {version}: {why}.") },
    }
}

/// `candidate` é mais nova que `current`, comparando como semver (0.10.0 > 0.9.0). Versão ilegível não conta.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (semver::Version::parse(candidate), semver::Version::parse(current)) {
        (Ok(candidate), Ok(current)) => candidate > current,
        _ => false,
    }
}

/// Começa a rodada se não houver outra em curso e o uso ainda não foi liberado. Devolve se começou.
pub fn start<R: Remote>(gate: &Gate, remote: R, current: String) -> bool {
    if !gate.reserve() {
        return false;
    }
    let gate = gate.clone();
    tauri::async_runtime::spawn(async move { settle(&gate, &remote, &current).await });
    true
}

/// Roda a rodada e registra o fim dela. Se algo estourar um pânico, a tela recebe a falha: o uso
/// fica bloqueado com `Tentar novamente`, e não preso em "verificando".
async fn settle<R: Remote>(gate: &Gate, remote: &R, current: &str) {
    struct Abort<'a>(Option<&'a Gate>);
    impl Drop for Abort<'_> {
        fn drop(&mut self) {
            if let Some(gate) = self.0 {
                gate.finish(Status::Failed { message: "A verificação parou inesperadamente.".into() });
            }
        }
    }
    let mut abort = Abort(Some(gate));
    let status = attempt(gate, remote, current).await;
    abort.0 = None;
    gate.finish(status);
}

/// Verificação da abertura e do "Tentar novamente", pelo plugin oficial do Tauri.
pub fn check(app: &AppHandle, gate: &Gate) {
    let current = app.package_info().version.to_string();
    start(gate, Plugin { app: app.clone() }, current);
}

/// O plugin oficial, com a fonte de `plugins.updater` em `tauri.conf.json`.
struct Plugin {
    app: AppHandle,
}

impl Remote for Plugin {
    type Pending = tauri_plugin_updater::Update;

    fn check(&self) -> impl Future<Output = Result<Option<Self::Pending>, String>> + Send {
        let app = self.app.clone();
        async move {
            let updater = app.updater_builder().timeout(CHECK_TIMEOUT).build().map_err(explain)?;
            let update = updater.check().await.map_err(explain)?;
            Ok(update.map(|mut update| {
                update.timeout = Some(DOWNLOAD_TIMEOUT);
                update
            }))
        }
    }

    fn version(&self, pending: &Self::Pending) -> String {
        pending.version.clone()
    }

    fn download(
        &self,
        pending: &Self::Pending,
        on_chunk: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> impl Future<Output = Result<Vec<u8>, String>> + Send {
        async move { pending.download(|chunk, total| on_chunk(chunk as u64, total), || {}).await.map_err(explain) }
    }

    fn install(&self, pending: &Self::Pending, bytes: &[u8]) -> Result<(), String> {
        pending.install(bytes).map_err(explain)
    }

    fn relaunch(&self) {
        self.app.restart();
    }
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
    use std::collections::VecDeque;

    /// Fonte falsa: cada consulta devolve a próxima resposta da fila, e as chamadas ficam registradas.
    struct Fake {
        checks: Mutex<VecDeque<Result<Option<String>, String>>>,
        download: Result<Vec<u8>, String>,
        install: Result<(), String>,
        calls: Mutex<Vec<&'static str>>,
    }

    fn fake(checks: Vec<Result<Option<&str>, &str>>) -> Fake {
        Fake {
            checks: Mutex::new(checks.into_iter().map(|c| c.map(|v| v.map(String::from)).map_err(String::from)).collect()),
            download: Ok(b"instalador".to_vec()),
            install: Ok(()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(fake: &Fake) -> Vec<&'static str> {
        fake.calls.lock().unwrap().clone()
    }

    impl Remote for Fake {
        type Pending = String;

        fn check(&self) -> impl Future<Output = Result<Option<String>, String>> + Send {
            self.calls.lock().unwrap().push("check");
            let next = self.checks.lock().unwrap().pop_front().expect("consulta sem resposta prevista");
            async move { next }
        }

        fn version(&self, pending: &String) -> String {
            pending.clone()
        }

        fn download(
            &self,
            _pending: &String,
            on_chunk: &mut (dyn FnMut(u64, Option<u64>) + Send),
        ) -> impl Future<Output = Result<Vec<u8>, String>> + Send {
            self.calls.lock().unwrap().push("download");
            on_chunk(300_000, Some(600_000));
            on_chunk(300_000, Some(600_000));
            let result = self.download.clone();
            async move { result }
        }

        fn install(&self, _pending: &String, bytes: &[u8]) -> Result<(), String> {
            self.calls.lock().unwrap().push("install");
            assert_eq!(bytes, b"instalador", "o instalador instalado é o que foi baixado");
            self.install.clone()
        }

        fn relaunch(&self) {
            self.calls.lock().unwrap().push("relaunch");
        }
    }

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

    fn phases(log: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
        log.lock().unwrap().iter().map(|v| v["phase"].as_str().unwrap().to_string()).collect()
    }

    /// Uma rodada completa, como `start` faria, e o estado final.
    fn run(gate: &Gate, fake: &Fake, current: &str) -> Status {
        assert!(gate.reserve(), "a rodada deveria começar");
        tauri::async_runtime::block_on(settle(gate, fake, current));
        gate.status()
    }

    /// Fonte que estoura logo na consulta: nada pode deixar a tela presa em "verificando".
    struct Exploding;

    impl Remote for Exploding {
        type Pending = String;

        async fn check(&self) -> Result<Option<String>, String> {
            panic!("falha inesperada na consulta")
        }

        fn version(&self, pending: &String) -> String {
            pending.clone()
        }

        fn download(
            &self,
            _pending: &String,
            _on_chunk: &mut (dyn FnMut(u64, Option<u64>) + Send),
        ) -> impl Future<Output = Result<Vec<u8>, String>> + Send {
            async { Ok(Vec::new()) }
        }

        fn install(&self, _pending: &String, _bytes: &[u8]) -> Result<(), String> {
            Ok(())
        }

        fn relaunch(&self) {}
    }

    #[test]
    fn versao_em_dia_libera_o_uso_sem_baixar_nada() {
        let (gate, log) = new_gate(false);
        let fake = fake(vec![Ok(None)]);
        assert_eq!(run(&gate, &fake, "0.1.0"), Status::Ready);
        assert!(gate.ensure_open().is_ok());
        assert_eq!(calls(&fake), ["check"]);
        assert_eq!(phases(&log), ["ready"]);
    }

    #[test]
    fn versao_superior_baixa_valida_instala_e_reabre() {
        let (gate, log) = new_gate(false);
        let fake = Fake { download: Ok(b"instalador".to_vec()), ..fake(vec![Ok(Some("0.2.0"))]) };
        assert_eq!(run(&gate, &fake, "0.1.0"), Status::Installing { version: "0.2.0".into() });
        assert_eq!(calls(&fake), ["check", "download", "install", "relaunch"]);
        assert!(gate.ensure_open().is_err(), "enquanto instala, o uso continua fechado");
        let log = log.lock().unwrap();
        let progress: Vec<(u64, Option<u64>)> = log
            .iter()
            .filter(|v| v["phase"] == "downloading")
            .map(|v| (v["downloaded"].as_u64().unwrap(), v["total"].as_u64()))
            .collect();
        assert_eq!(progress, [(0, None), (300_000, Some(600_000)), (600_000, Some(600_000))]);
        assert_eq!(log.last().unwrap(), &json!({"phase": "installing", "version": "0.2.0"}));
    }

    #[test]
    fn sem_conexao_bloqueia_com_o_erro_e_a_nova_tentativa_libera() {
        let (gate, log) = new_gate(false);
        let fake = fake(vec![Err("sem conexão com o GitHub"), Ok(None)]);
        assert_eq!(
            run(&gate, &fake, "0.1.0"),
            Status::Failed { message: "Não foi possível verificar atualizações: sem conexão com o GitHub.".into() }
        );
        assert!(gate.ensure_open().is_err(), "a falha mantém o uso bloqueado");
        assert_eq!(run(&gate, &fake, "0.1.0"), Status::Ready, "a nova tentativa liberou o uso");
        assert_eq!(phases(&log), ["failed", "checking", "ready"]);
    }

    #[test]
    fn falha_inesperada_nao_deixa_a_tela_presa_em_verificando() {
        let (gate, log) = new_gate(false);
        assert!(gate.reserve());
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tauri::async_runtime::block_on(settle(&gate, &Exploding, "0.1.0"));
        }));
        assert!(outcome.is_err(), "o pânico continua sendo um pânico");
        assert_eq!(gate.status(), Status::Failed { message: "A verificação parou inesperadamente.".into() });
        assert_eq!(phases(&log).last().map(String::as_str), Some("failed"), "a tela recebe a falha");
        assert!(gate.ensure_open().is_err());
        assert!(gate.reserve(), "a nova tentativa fica disponível");
    }

    #[test]
    fn falha_no_download_bloqueia_e_nao_instala() {
        let (gate, _) = new_gate(false);
        let fake = Fake { download: Err("conexão caiu".into()), ..fake(vec![Ok(Some("0.2.0"))]) };
        assert_eq!(
            run(&gate, &fake, "0.1.0"),
            Status::Failed { message: "Não foi possível baixar a atualização 0.2.0: conexão caiu.".into() }
        );
        assert!(gate.ensure_open().is_err());
        assert_eq!(calls(&fake), ["check", "download"]);
    }

    #[test]
    fn falha_na_instalacao_bloqueia_e_nao_reabre() {
        let (gate, _) = new_gate(false);
        let fake = Fake { install: Err("instalador bloqueado".into()), ..fake(vec![Ok(Some("0.2.0"))]) };
        assert_eq!(
            run(&gate, &fake, "0.1.0"),
            Status::Failed { message: "Não foi possível instalar a atualização 0.2.0: instalador bloqueado.".into() }
        );
        assert!(gate.ensure_open().is_err());
        assert_eq!(calls(&fake), ["check", "download", "install"]);
    }

    #[test]
    fn nunca_instala_versao_igual_ou_anterior() {
        for announced in ["0.1.0", "0.0.9"] {
            let (gate, _) = new_gate(false);
            let fake = fake(vec![Ok(Some(announced))]);
            assert_eq!(run(&gate, &fake, "0.1.0"), Status::Ready, "anunciada {announced}");
            assert_eq!(calls(&fake), ["check"], "anunciada {announced}: nada baixado");
        }
    }

    #[test]
    fn compara_versoes_como_semver() {
        assert!(is_newer("0.10.0", "0.9.0"), "0.10.0 é mais nova que 0.9.0");
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("nao-e-versao", "0.1.0"), "versão ilegível não é tratada como nova");
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
