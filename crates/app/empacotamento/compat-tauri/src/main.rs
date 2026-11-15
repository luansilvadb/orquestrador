//! O updater do 0.1.48 (tauri-plugin-updater 2.14.1, com a configuração do `src-tauri/tauri.conf.json`) diante dos
//! artefatos novos. Uso:
//!   compat verificar <pubkey> <latest.json> <instalador>   restore_update + verify, como o app faria após a consulta
//!   compat instalar <pubkey> <url do latest.json> [args…]  check + download + install de verdade (sai ao instalar)
use tauri_plugin_updater::UpdaterExt;

fn app(pubkey: &str, endpoint: &str) -> tauri::App<tauri::test::MockRuntime> {
    let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
    ctx.package_info_mut().version = "0.1.48".parse().unwrap();
    let conf: serde_json::Value = serde_json::from_str(include_str!("../../../../../src-tauri/tauri.conf.json")).unwrap();
    let mut updater = conf["plugins"]["updater"].clone();
    updater["pubkey"] = pubkey.into();
    updater["endpoints"] = serde_json::json!([endpoint]);
    // Só o teste local usa http.
    updater["dangerousInsecureTransportProtocol"] = endpoint.starts_with("http:").into();
    ctx.config_mut().plugins.0.insert("updater".into(), updater);
    tauri::test::mock_builder().plugin(tauri_plugin_updater::Builder::new().build()).build(ctx).unwrap()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let r = match a.first().map(String::as_str) {
        Some("verificar") if a.len() == 4 => verificar(&a[1], &a[2], &a[3]),
        Some("instalar") if a.len() >= 3 => instalar(&a[1], &a[2], &a[3..]),
        _ => Err("uso: compat verificar|instalar …".into()),
    };
    if let Err(e) = r {
        eprintln!("compat: {e}");
        std::process::exit(1);
    }
}

fn verificar(pubkey: &str, manifesto: &str, instalador: &str) -> Result<(), String> {
    let pubkey = ler_chave(pubkey)?;
    let app = app(&pubkey, "https://example.invalid/latest.json");
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(manifesto).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let up = app.updater_builder().build().map_err(|e| e.to_string())?;
    let update = up.restore_update(json).map_err(|e| e.to_string())?.ok_or("o updater do 0.1.48 não vê atualização")?;
    let dados = std::fs::read(instalador).map_err(|e| e.to_string())?;
    update.verify(&dados).map_err(|e| format!("verify recusou: {e}"))?;
    let mut alterado = dados.clone();
    *alterado.last_mut().unwrap() ^= 1;
    if update.verify(&alterado).is_ok() {
        return Err("verify aceitou um instalador alterado".into());
    }
    println!("ok: 0.1.48 aceita {} ({}) de {}", update.version, update.target, update.download_url);
    Ok(())
}

fn instalar(pubkey: &str, endpoint: &str, args: &[String]) -> Result<(), String> {
    let pubkey = ler_chave(pubkey)?;
    let app = app(&pubkey, endpoint);
    let up = app.updater_builder().installer_args(args).build().map_err(|e| e.to_string())?;
    tauri::async_runtime::block_on(async move {
        let update = up.check().await.map_err(|e| e.to_string())?.ok_or("sem atualização")?;
        println!("0.1.48 → {}: baixando e instalando como o Tauri", update.version);
        update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())
    })
}

fn ler_chave(p: &str) -> Result<String, String> {
    match std::fs::read_to_string(p) {
        Ok(s) => Ok(s.trim().to_string()),
        Err(_) => Ok(p.to_string()),
    }
}
