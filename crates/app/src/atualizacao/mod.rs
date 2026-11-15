//! Atualização na abertura (porte do updater do 0.1.48 sem o `tauri-plugin-updater`): consulta o `latest.json`
//! da última release estável, baixa a versão superior, valida a assinatura, instala em silêncio e o instalador reabre
//! o app. Em dia, o portão segue para os CLIs. O canal, a chave e a versão podem ser trocados na compilação
//! (`ORQ_ATUALIZACAO_URL`, `ORQ_ATUALIZACAO_CHAVE`, `ORQ_VERSAO`), só para o teste de ponta a ponta.
pub mod manifesto;

use std::io::Read;
use std::time::Duration;

pub const URL_OFICIAL: &str = "https://github.com/luansilvadb/orquestrador/releases/latest/download/latest.json";
/// A chave do updater do 0.1.48 (`empacotamento/compat-tauri/updater-0.1.48.json`).
pub const PUBKEY_OFICIAL: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDJCQkRCMUY4N0Q1NUIzNTUKUldSVnMxVjkrTEc5SzFhUU1pR004UE8yUVI3VTAwQnBLaHkrNVVVUDYyVGovZGNaT0cvTmVHSkgK";

pub const URL: &str = match option_env!("ORQ_ATUALIZACAO_URL") {
    Some(u) => u,
    None => URL_OFICIAL,
};
pub const PUBKEY: &str = match option_env!("ORQ_ATUALIZACAO_CHAVE") {
    Some(k) => k,
    None => PUBKEY_OFICIAL,
};
pub const VERSAO: &str = match option_env!("ORQ_VERSAO") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// Prazo da consulta.
const PRAZO_CONSULTA: Duration = Duration::from_secs(30);
/// Prazo do download inteiro.
const PRAZO_DOWNLOAD: Duration = Duration::from_secs(15 * 60);
/// Intervalo mínimo entre dois avisos de progresso.
const PASSO_PROGRESSO: u64 = 256 * 1024;
/// Teto do instalador baixado.
const TAMANHO_MAXIMO: u64 = 1 << 30;

/// O andamento que a tela mostra antes da etapa dos CLIs.
#[derive(Clone, Debug, PartialEq)]
pub enum Fase {
    Verificando,
    Baixando { versao: String, baixado: u64, total: Option<u64> },
    Instalando { versao: String },
}

/// Uma rodada: `Ok(())` em dia; com versão nova, instala e encerra o processo (o instalador reabre o app).
pub fn rodada(mut avisar: impl FnMut(Fase)) -> Result<(), String> {
    avisar(Fase::Verificando);
    let lancamento = consultar().map_err(|e| format!("Não foi possível verificar atualizações: {e}."))?;
    let Some(l) = lancamento else { return Ok(()) };
    let versao = l.versao.to_string();
    let dados = baixar(&l.url, |baixado, total| avisar(Fase::Baixando { versao: versao.clone(), baixado, total }))
        .and_then(|d| manifesto::verificar(&d, &l.assinatura, PUBKEY, &l.versao).map(|_| d).map_err(explicar_manifesto))
        .map_err(|e| format!("Não foi possível baixar a atualização {versao}: {e}."))?;
    avisar(Fase::Instalando { versao: versao.clone() });
    instalar(&versao, &dados).map_err(|e| format!("Não foi possível instalar a atualização {versao}: {e}."))?;
    std::process::exit(0);
}

fn agente(prazo: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(prazo))
        .user_agent(concat!("orquestrador/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn consultar() -> Result<Option<manifesto::Lancamento>, String> {
    let mut r = agente(PRAZO_CONSULTA).get(URL).call().map_err(|e| explicar_rede(e, true))?;
    if r.status() == 204 {
        return Ok(None);
    }
    let texto = r.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| explicar_rede(e, true))?;
    manifesto::ler(&texto, VERSAO).map_err(explicar_manifesto)
}

fn baixar(url: &str, mut progresso: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> {
    let mut r = agente(PRAZO_DOWNLOAD).get(url).call().map_err(|e| explicar_rede(e, false))?;
    let total = r.body().content_length();
    progresso(0, total);
    let mut leitor = r.body_mut().with_config().limit(TAMANHO_MAXIMO).reader();
    let (mut dados, mut bloco, mut mostrado) = (Vec::with_capacity(total.unwrap_or(0) as usize), vec![0u8; 64 * 1024], 0u64);
    loop {
        let n = leitor.read(&mut bloco).map_err(|e| format!("falha de rede ({e})"))?;
        if n == 0 {
            break;
        }
        dados.extend_from_slice(&bloco[..n]);
        let baixado = dados.len() as u64;
        if baixado - mostrado >= PASSO_PROGRESSO || total == Some(baixado) {
            mostrado = baixado;
            progresso(baixado, total);
        }
    }
    Ok(dados)
}

/// Grava o instalador numa pasta temporária e o roda como o Tauri (`/S /UPDATE /R`): silencioso, sem recriar
/// atalhos e reabrindo o app no fim.
fn instalar(versao: &str, dados: &[u8]) -> Result<(), String> {
    let pasta = std::env::temp_dir().join(format!("Orquestrador-{versao}-updater-{}", std::process::id()));
    std::fs::create_dir_all(&pasta).map_err(|e| e.to_string())?;
    let exe = pasta.join(format!("Orquestrador_{versao}_x64-setup.exe"));
    std::fs::write(&exe, dados).map_err(|e| e.to_string())?;
    std::process::Command::new(&exe).args(["/S", "/UPDATE", "/R"]).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// O motivo em português, como o updater do 0.1.48.
fn explicar_rede(e: ureq::Error, consulta: bool) -> String {
    match e {
        ureq::Error::Timeout(_) => "o GitHub não respondeu a tempo".into(),
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => "sem conexão com o GitHub".into(),
        ureq::Error::StatusCode(404) if consulta => "nenhuma versão estável publicada foi encontrada".into(),
        ureq::Error::StatusCode(c) => format!("o servidor recusou o download (HTTP {c})"),
        ureq::Error::Io(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => "sem conexão com o GitHub".into(),
        outro => format!("falha de rede ({outro})"),
    }
}

fn explicar_manifesto(e: manifesto::Erro) -> String {
    match e {
        manifesto::Erro::Assinatura(_) => "a assinatura da atualização não confere, então ela foi recusada".into(),
        manifesto::Erro::SemPlataforma => "a versão publicada não tem instalador para Windows".into(),
        manifesto::Erro::Formato(d) => format!("o manifesto da versão é inválido ({d})"),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn canal_e_chave_sao_os_do_tauri() {
        use base64::Engine;
        assert_eq!(URL, URL_OFICIAL, "o build de teste não troca o canal");
        assert_eq!(PUBKEY, PUBKEY_OFICIAL);
        // O mesmo canal, a mesma chave e as regras do updater do 0.1.48.
        let c: serde_json::Value =
            serde_json::from_str(include_str!("../../empacotamento/compat-tauri/updater-0.1.48.json")).unwrap();
        let u = &c["updater"];
        assert_eq!(u["endpoints"], serde_json::json!([URL_OFICIAL]), "só a última release publicada");
        assert_eq!(u["pubkey"], PUBKEY_OFICIAL);
        assert_eq!(c["identifier"], "dev.luansilvadb.orquestrador");
        assert_eq!(u["requireSignedVersion"], true);
        assert_eq!(u["allowDowngrades"], false);
        assert_eq!(u["windows"]["installMode"], "quiet");
        let chave = base64::engine::general_purpose::STANDARD.decode(PUBKEY_OFICIAL).unwrap();
        assert!(String::from_utf8(chave).unwrap().starts_with("untrusted comment: minisign public key: "));
    }

    #[test]
    fn erros_explicados() {
        assert_eq!(explicar_rede(ureq::Error::StatusCode(404), true), "nenhuma versão estável publicada foi encontrada");
        assert_eq!(explicar_rede(ureq::Error::StatusCode(404), false), "o servidor recusou o download (HTTP 404)");
        assert_eq!(explicar_rede(ureq::Error::HostNotFound, true), "sem conexão com o GitHub");
        assert!(explicar_manifesto(manifesto::Erro::Assinatura("x".into())).contains("assinatura"));
    }

    /// Servidor local de uma resposta por conexão, para a rodada sem rede.
    fn servir(respostas: Vec<(u16, Vec<u8>)>) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let end = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for (codigo, corpo) in respostas {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = [0u8; 4096];
                let _ = std::io::Read::read(&mut s, &mut buf);
                let cab = format!("HTTP/1.1 {codigo} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", corpo.len());
                let _ = std::io::Write::write_all(&mut s, cab.as_bytes());
                let _ = std::io::Write::write_all(&mut s, &corpo);
            }
        });
        end
    }

    #[test]
    fn download_avisa_progresso_e_recusa_http_de_erro() {
        let corpo = vec![7u8; 600 * 1024];
        let end = servir(vec![(200, corpo.clone()), (500, vec![])]);
        let mut avisos = vec![];
        assert_eq!(baixar(&end, |b, t| avisos.push((b, t))).unwrap(), corpo);
        assert_eq!(avisos.first(), Some(&(0, Some(600 * 1024))));
        assert_eq!(avisos.last(), Some(&(600 * 1024, Some(600 * 1024))));
        assert!(avisos.len() >= 3, "{avisos:?}");
        assert_eq!(baixar(&end, |_, _| {}).unwrap_err(), "o servidor recusou o download (HTTP 500)");
    }
}
