//! Assinatura e manifesto da atualização, no formato do `tauri-plugin-updater` 2 (o 0.1.48 instalado o exige):
//! `.sig` em base64 de uma caixa minisign com `version:` no comentário confiável e `latest.json` com `windows-x86_64`.
//! Uso:
//!   assinador chaves <arquivo>                              par sem senha (<arquivo> e <arquivo>.pub), só para testes
//!   assinador assinar <chave> <instalador> <versão>         grava <instalador>.sig; <chave> é caminho ou conteúdo
//!   assinador manifesto <versão> <url> <sig> <notas|-> <saída>
//!   assinador verificar <pubkey> <versão> <latest.json> <instalador>
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::path::Path;
use std::process::ExitCode;

const PLATAFORMAS: [&str; 2] = ["windows-x86_64", "windows-x86_64-nsis"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let r = match a.as_slice() {
        ["chaves", arquivo] => chaves(Path::new(arquivo)),
        ["assinar", chave, instalador, versao] => assinar(chave, Path::new(instalador), versao),
        ["manifesto", versao, url, sig, notas, saida] => manifesto(versao, url, Path::new(sig), notas, Path::new(saida)),
        ["verificar", pubkey, versao, manifesto, instalador] => verificar(pubkey, versao, Path::new(manifesto), Path::new(instalador)),
        _ => Err("uso: assinador chaves|assinar|manifesto|verificar … (ver src/main.rs)".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("assinador: {e}");
            ExitCode::FAILURE
        }
    }
}

type R<T = ()> = Result<T, String>;

fn ler(p: &Path) -> R<String> {
    std::fs::read_to_string(p).map(|s| s.trim().to_string()).map_err(|e| format!("{}: {e}", p.display()))
}

fn b64_texto(s: &str) -> R<String> {
    let bytes = STANDARD.decode(s.trim()).map_err(|e| format!("base64 inválido: {e}"))?;
    String::from_utf8(bytes).map_err(|_| "base64 não é texto".into())
}

fn chaves(arquivo: &Path) -> R {
    let par = minisign::KeyPair::generate_encrypted_keypair(Some(String::new())).map_err(|e| e.to_string())?;
    let sk = STANDARD.encode(par.sk.to_box(None).map_err(|e| e.to_string())?.to_string());
    let pk = STANDARD.encode(par.pk.to_box().map_err(|e| e.to_string())?.to_string());
    std::fs::write(arquivo, sk).map_err(|e| e.to_string())?;
    std::fs::write(arquivo.with_extension("pub"), &pk).map_err(|e| e.to_string())?;
    println!("{pk}");
    Ok(())
}

fn assinar(chave: &str, instalador: &Path, versao: &str) -> R {
    let conteudo = if Path::new(chave).is_file() { ler(Path::new(chave))? } else { chave.trim().to_string() };
    let senha = std::env::var("ORQ_CHAVE_SENHA").unwrap_or_default();
    let sk = minisign::SecretKeyBox::from_string(&b64_texto(&conteudo)?)
        .and_then(|b| b.into_secret_key(Some(senha)))
        .map_err(|e| format!("chave privada: {e}"))?;
    let nome = instalador.file_name().ok_or("instalador sem nome")?.to_string_lossy();
    let agora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    // Mesmo comentário confiável do Tauri CLI: o updater confere `version:` contra a versão anunciada.
    let confiavel = format!("timestamp:{agora}\tfile:{nome}\tversion:{versao}");
    let dados = std::fs::File::open(instalador).map_err(|e| format!("{}: {e}", instalador.display()))?;
    let caixa = minisign::sign(None, &sk, std::io::BufReader::new(dados), Some(&confiavel), Some("signature from tauri secret key"))
        .map_err(|e| e.to_string())?;
    let mut sig = instalador.as_os_str().to_owned();
    sig.push(".sig");
    std::fs::write(&sig, STANDARD.encode(caixa.to_string())).map_err(|e| e.to_string())
}

fn manifesto(versao: &str, url: &str, sig: &Path, notas: &str, saida: &Path) -> R {
    let assinatura = ler(sig)?;
    let notas = if notas == "-" { String::new() } else { ler(Path::new(notas))?.replace("\r\n", "\n") };
    let data = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| e.to_string())?;
    let entrada = json!({ "signature": assinatura, "url": url });
    let platforms: serde_json::Map<String, Value> = PLATAFORMAS.iter().map(|p| (p.to_string(), entrada.clone())).collect();
    let m = json!({ "version": versao, "notes": notas, "pub_date": data, "platforms": platforms });
    std::fs::write(saida, serde_json::to_string_pretty(&m).unwrap() + "\n").map_err(|e| e.to_string())
}

/// As mesmas conferências do `tauri-plugin-updater` 2.14 com `requireSignedVersion`, mais URL e versão esperadas.
fn verificar(pubkey: &str, versao: &str, manifesto: &Path, instalador: &Path) -> R {
    let m: Value = serde_json::from_str(&ler(manifesto)?).map_err(|e| format!("latest.json: {e}"))?;
    if m["version"].as_str() != Some(versao) {
        return Err(format!("latest.json anuncia {}, não {versao}", m["version"]));
    }
    let nome = instalador.file_name().ok_or("instalador sem nome")?.to_string_lossy().to_string();
    let dados = std::fs::read(instalador).map_err(|e| e.to_string())?;
    let pk = minisign_verify::PublicKey::decode(&b64_texto(pubkey)?).map_err(|e| format!("pubkey: {e}"))?;
    for p in PLATAFORMAS {
        let e = &m["platforms"][p];
        let url = e["url"].as_str().ok_or(format!("{p} sem url"))?;
        if !url.ends_with(&format!("/{nome}")) {
            return Err(format!("{p}: a url {url} não aponta para {nome}"));
        }
        let sig = minisign_verify::Signature::decode(&b64_texto(e["signature"].as_str().ok_or(format!("{p} sem signature"))?)?)
            .map_err(|e| format!("{p}: assinatura: {e}"))?;
        pk.verify(&dados, &sig, true).map_err(|e| format!("{p}: a assinatura não confere: {e}"))?;
        let assinada = sig.trusted_comment().split('\t').find_map(|c| c.strip_prefix("version:"));
        if assinada != Some(versao) {
            return Err(format!("{p}: versão assinada {assinada:?}, anunciada {versao}"));
        }
    }
    println!("ok: {nome} {versao}");
    Ok(())
}

#[cfg(test)]
mod testes {
    // Par descartável com senha vazia gerado por `chaves` (scrypt com senha vazia) (mesmo formato do Tauri CLI).
    const SK: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5ClJXUlRZMEl5T0V4NUw1MWljaDZDOEkwU1RocnI3NkYzdEtKQmpDS3RURC9JcndJNTEzc0FBQkFBQUFBQUFBQUFBQUlBQUFBQU1GWEp0ZlNRaW5YUFdMRFErUUI5QnJEYXM3bmlsSXVjMUNrdTl1NVpDRVlZMDlNcWZZTElTbi95ZlFZQWlWYU45TEg5dnhRVHpubEI1VUdvS0I1VzI0SWFqR0FBVlVmZjl3dUREMCsrMTA5WWtjVVR0aVhpMGRKNlVrbDc4UndySDQwNFNrME9xa0E9Cg==";
    const PK: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDM3NzQ1RTBEQjY2QTc2MkQKUldRdGRtcTJEVjUwTjh0RGpGSmc5ZHZmYzFzc290bitlbml5cEdYa0Z0aUVlL0NCNXJHajRDbDEK";

    #[test]
    fn abre_chave_sem_senha_do_tauri() {
        let dir = std::env::temp_dir().join(format!("assinador-teste-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let arq = dir.join("instalador.exe");
        std::fs::write(&arq, b"conteudo").unwrap();
        let r = super::assinar(SK, &arq, "0.0.0");
        let sig = std::fs::read_to_string(dir.join("instalador.exe.sig"));
        std::fs::remove_dir_all(&dir).unwrap();
        r.unwrap();
        let pk = minisign_verify::PublicKey::decode(&super::b64_texto(PK).unwrap()).unwrap();
        let sig = minisign_verify::Signature::decode(&super::b64_texto(&sig.unwrap()).unwrap()).unwrap();
        pk.verify(b"conteudo", &sig, true).unwrap();
    }
}
