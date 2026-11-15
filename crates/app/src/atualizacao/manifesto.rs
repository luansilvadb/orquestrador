//! O `latest.json` e a assinatura, com as regras do `tauri-plugin-updater` 2.14 (o mesmo `minisign-verify`):
//! plataforma `windows-x86_64-nsis` ou `windows-x86_64`, versão só maior e `version:` assinada igual à anunciada.
use base64::{engine::general_purpose::STANDARD, Engine};
use semver::Version;
use serde::Deserialize;
use std::collections::HashMap;

/// Chaves de plataforma na ordem em que o Tauri procura.
pub const ALVOS: [&str; 2] = ["windows-x86_64-nsis", "windows-x86_64"];

#[derive(Debug, Clone, PartialEq)]
pub struct Lancamento {
    pub versao: Version,
    pub url: String,
    pub assinatura: String,
}

#[derive(Debug, PartialEq)]
pub enum Erro {
    /// JSON fora do formato.
    Formato(String),
    /// Versão superior sem entrada para o Windows.
    SemPlataforma,
    /// Assinatura inválida, de outra chave ou de outra versão.
    Assinatura(String),
}

#[derive(Deserialize)]
struct Entrada {
    url: String,
    signature: String,
}

#[derive(Deserialize)]
struct Bruto {
    #[serde(alias = "name")]
    version: String,
    platforms: Option<HashMap<String, Entrada>>,
    url: Option<String>,
    signature: Option<String>,
}

fn versao(s: &str) -> Result<Version, Erro> {
    Version::parse(s.trim().trim_start_matches('v')).map_err(|e| Erro::Formato(format!("versão {s:?}: {e}")))
}

/// `None` quando não há versão maior que `atual` (sem downgrade, como `allowDowngrades: false`).
pub fn ler(json: &str, atual: &str) -> Result<Option<Lancamento>, Erro> {
    let b: Bruto = serde_json::from_str(json).map_err(|e| Erro::Formato(e.to_string()))?;
    let v = versao(&b.version)?;
    if v <= versao(atual)? {
        return Ok(None);
    }
    let (url, assinatura) = match b.platforms {
        Some(mut p) => {
            let e = ALVOS.iter().find_map(|a| p.remove(*a)).ok_or(Erro::SemPlataforma)?;
            (e.url, e.signature)
        }
        None => match (b.url, b.signature) {
            (Some(u), Some(s)) => (u, s),
            _ => return Err(Erro::Formato("sem `platforms` nem `url`/`signature`".into())),
        },
    };
    Ok(Some(Lancamento { versao: v, url, assinatura }))
}

fn b64_texto(s: &str) -> Result<String, Erro> {
    let bytes = STANDARD.decode(s.trim()).map_err(|e| Erro::Assinatura(format!("base64: {e}")))?;
    String::from_utf8(bytes).map_err(|_| Erro::Assinatura("base64 não é texto".into()))
}

/// Confere `dados` contra a assinatura com a chave pública (ambas em base64, como no `tauri.conf.json`) e exige a
/// versão assinada igual à anunciada (`requireSignedVersion`).
pub fn verificar(dados: &[u8], assinatura: &str, pubkey: &str, anunciada: &Version) -> Result<(), Erro> {
    let pk = minisign_verify::PublicKey::decode(&b64_texto(pubkey)?).map_err(|e| Erro::Assinatura(e.to_string()))?;
    let sig = minisign_verify::Signature::decode(&b64_texto(assinatura)?).map_err(|e| Erro::Assinatura(e.to_string()))?;
    pk.verify(dados, &sig, true).map_err(|e| Erro::Assinatura(e.to_string()))?;
    let assinada = sig.trusted_comment().split('\t').find_map(|c| c.strip_prefix("version:"));
    match assinada.map(versao) {
        Some(Ok(v)) if &v == anunciada => Ok(()),
        Some(_) => Err(Erro::Assinatura(format!("versão assinada {} difere da anunciada {anunciada}", assinada.unwrap()))),
        None => Err(Erro::Assinatura("assinatura sem versão".into())),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    // Par minisign gerado só para estes testes (senha vazia); a assinatura cobre `DADOS` com `version:0.2.0`.
    const PUB: &str = include_str!("teste/chave.pub");
    const SIG: &str = include_str!("teste/dados.sig");
    const SIG_OUTRA_VERSAO: &str = include_str!("teste/dados-0.1.0.sig");
    const DADOS: &[u8] = include_bytes!("teste/dados.bin");

    fn manifesto(versao: &str, plataforma: &str) -> String {
        serde_json::json!({
            "version": versao, "notes": "n", "pub_date": "2026-11-15T00:00:00Z",
            "platforms": { plataforma: { "url": "https://x/Orquestrador_0.2.0_x64-setup.exe", "signature": SIG.trim() } }
        })
        .to_string()
    }

    #[test]
    fn so_versao_maior_e_atualizacao() {
        assert!(ler(&manifesto("0.2.0", "windows-x86_64"), "0.1.48").unwrap().is_some());
        assert_eq!(ler(&manifesto("0.1.48", "windows-x86_64"), "0.1.48"), Ok(None));
        assert_eq!(ler(&manifesto("0.1.9", "windows-x86_64"), "0.1.48"), Ok(None), "sem downgrade");
        assert!(ler(&manifesto("v0.2.0", "windows-x86_64"), "0.2.0-teste.2").unwrap().is_some());
        assert!(ler(&manifesto("0.2.0-teste.2", "windows-x86_64"), "0.2.0-teste.1").unwrap().is_some());
    }

    #[test]
    fn escolhe_a_plataforma_do_windows() {
        assert_eq!(ler(&manifesto("0.2.0", "linux-x86_64"), "0.1.48"), Err(Erro::SemPlataforma));
        let l = ler(&manifesto("0.2.0", "windows-x86_64-nsis"), "0.1.48").unwrap().unwrap();
        assert!(l.url.ends_with("setup.exe"));
        assert!(matches!(ler("{\"versao\":1}", "0.1.48"), Err(Erro::Formato(_))));
    }

    #[test]
    fn manifesto_dinamico_tambem_vale() {
        let j = serde_json::json!({ "version": "0.2.0", "url": "https://x/a.exe", "signature": "s" }).to_string();
        assert_eq!(ler(&j, "0.1.48").unwrap().unwrap().url, "https://x/a.exe");
    }

    #[test]
    fn assinatura_confere_e_amarra_a_versao() {
        let v = Version::parse("0.2.0").unwrap();
        assert_eq!(verificar(DADOS, SIG, PUB, &v), Ok(()));
        assert!(matches!(verificar(b"outro", SIG, PUB, &v), Err(Erro::Assinatura(_))), "dados trocados");
        assert!(matches!(verificar(DADOS, SIG, PUB, &Version::parse("0.3.0").unwrap()), Err(Erro::Assinatura(_))));
        assert!(matches!(verificar(DADOS, SIG_OUTRA_VERSAO, PUB, &v), Err(Erro::Assinatura(_))), "assinatura antiga reaproveitada");
        assert!(matches!(verificar(DADOS, SIG, super::super::PUBKEY_OFICIAL, &v), Err(Erro::Assinatura(_))), "outra chave");
    }
}
