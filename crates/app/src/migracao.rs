//! Migração única do `localStorage` do WebView2 (versão React, até a 0.1.48) para os arquivos do GPUI.
//! Lê uma cópia do LevelDB em %TEMP%; só preenche o que o GPUI ainda não tem; marca `migracaoReact` no `interface.json`.
use crate::estado::{Estado, Projetos, Secao, RECENTES_MAX};
use orquestrador_editor::armazem::{chave_recuperacao, ArquivoChaves, Chaves, SEM_PROJETO};
use orquestrador_pesquisa::PreferenciaPesquisa;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const PREFIXO_EDITOR: &str = "orquestrador.editor:";

/// Marca da migração feita.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MigracaoReact {
    pub feita: bool,
}

impl Secao for MigracaoReact {
    const NOME: &'static str = "migracaoReact";
    const VERSAO: u32 = 1;
}

/// O que a migração achou e gravou.
#[derive(Debug, Default)]
pub struct Relatorio {
    pub origem: Option<String>,
    pub chaves: Vec<String>,
    pub gravadas: Vec<String>,
    pub mantidas: Vec<String>,
}

/// Pasta do LevelDB do WebView2 do app React.
pub fn pasta_webview(local: &Path) -> PathBuf {
    local.join("dev.luansilvadb.orquestrador").join("EBWebView").join("Default").join("Local Storage").join("leveldb")
}

/// Arquivo-marcador: a verificação na abertura é um `metadata`, sem ler JSON nem varrer pastas.
pub const MARCADOR: &str = "migracao-react.feita";

/// Chamada na abertura: erro vira log, nunca impede o app.
pub fn executar() {
    let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(PathBuf::from) else { return };
    executar_em(&pasta_webview(&local), &local.join("orquestrador"));
}

/// `true` quando migrou agora.
pub fn executar_em(webview: &Path, destino: &Path) -> bool {
    let marcador = destino.join(MARCADOR);
    if marcador.exists() {
        return false;
    }
    let t = std::time::Instant::now();
    let r = migrar(webview, destino);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    match &r {
        Ok(Some(r)) => eprintln!("migração do React: {} chaves, {} gravadas, {ms:.1} ms", r.chaves.len(), r.gravadas.len()),
        Ok(None) => {}
        Err(e) => eprintln!("migração do React falhou ({ms:.1} ms): {e}"),
    }
    // Falha não se repete a cada abertura; o original fica intacto para nova tentativa manual.
    let _ = std::fs::create_dir_all(destino).and_then(|_| std::fs::write(&marcador, b""));
    true
}

/// Migra para `destino` (a pasta `orquestrador`). `None` quando já foi feita.
pub fn migrar(webview: &Path, destino: &Path) -> Result<Option<Relatorio>, String> {
    let mut estado = Estado::abrir(Some(destino.join("interface.json")));
    if estado.ler::<MigracaoReact>().feita {
        return Ok(None);
    }
    let mut r = Relatorio::default();
    if webview.is_dir() {
        let (origem, itens) = ler_local_storage(webview)?;
        r.origem = origem;
        r.chaves = itens.keys().cloned().collect();
        let mut editor = ArquivoChaves::abrir(Some(destino.join("editor-recuperacao.json")));
        converter(&itens, &mut estado, &mut editor, &mut r)?;
    }
    estado.gravar(&MigracaoReact { feita: true });
    Ok(Some(r))
}

/// Aplica as chaves do React sem sobrescrever o que o GPUI já tem.
pub fn converter(
    itens: &BTreeMap<String, String>, estado: &mut Estado, editor: &mut dyn Chaves, r: &mut Relatorio,
) -> Result<(), String> {
    let get = |k: &str| itens.get(k).map(String::as_str);
    let lista = |k: &str| -> Option<Vec<String>> {
        let v: Value = serde_json::from_str(get(k)?).ok()?;
        Some(v.as_array()?.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
    };
    let abertas = lista("orquestrador.project.tabs");
    let ultima = get("orquestrador.project.last").map(str::to_string);
    let recentes = lista("orquestrador.project.recent");
    if abertas.is_some() || ultima.is_some() || recentes.is_some() {
        if estado.tem::<Projetos>() {
            r.mantidas.push(Projetos::NOME.into());
        } else {
            let mut p = Projetos {
                abertas: abertas.unwrap_or_else(|| ultima.clone().into_iter().collect()),
                ativa: get("orquestrador.project.active").map(str::to_string).or(ultima),
                recentes: recentes.unwrap_or_default().into_iter().take(RECENTES_MAX).collect(),
            };
            p.validar();
            estado.gravar(&p);
            r.gravadas.push(Projetos::NOME.into());
        }
    }
    let historico = lista("orquestrador.search.history");
    let visao = get("orquestrador.search.view");
    if historico.is_some() || visao.is_some() {
        if estado.tem::<PreferenciaPesquisa>() {
            r.mantidas.push(PreferenciaPesquisa::NOME.into());
        } else {
            let mut p = PreferenciaPesquisa { historico: historico.unwrap_or_default(), ..Default::default() };
            if visao == Some("tree") {
                p.visao = orquestrador_pesquisa::logica::Visao::Arvore;
            }
            p.validar();
            estado.gravar(&p);
            r.gravadas.push(PreferenciaPesquisa::NOME.into());
        }
    }
    for (k, texto) in itens {
        let Some(projeto) = k.strip_prefix(PREFIXO_EDITOR) else { continue };
        let chave = if projeto == SEM_PROJETO { chave_recuperacao("") } else { chave_recuperacao(projeto) };
        if editor.ler(&chave).is_some() {
            r.mantidas.push(chave);
        } else {
            editor.gravar(&chave, texto)?;
            r.gravadas.push(chave);
        }
    }
    Ok(())
}

/// Lê o Local Storage de uma cópia da pasta. Devolve a origem do Tauri e as chaves dela.
pub fn ler_local_storage(pasta: &Path) -> Result<(Option<String>, BTreeMap<String, String>), String> {
    let copia = std::env::temp_dir().join(format!("orquestrador-migracao-{}-{}", std::process::id(), agora()));
    let resultado = copiar(pasta, &copia).and_then(|_| ler_copia(&copia));
    let _ = std::fs::remove_dir_all(&copia);
    resultado
}

fn agora() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

fn copiar(de: &Path, para: &Path) -> Result<(), String> {
    std::fs::create_dir_all(para).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(de).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        if e.file_name() == "LOCK" || !e.path().is_file() {
            continue;
        }
        std::fs::copy(e.path(), para.join(e.file_name())).map_err(|x| format!("{}: {x}", e.path().display()))?;
    }
    Ok(())
}

fn ler_copia(pasta: &Path) -> Result<(Option<String>, BTreeMap<String, String>), String> {
    let opcoes = rusty_leveldb::Options { create_if_missing: false, ..Default::default() };
    let mut db = rusty_leveldb::DB::open(pasta, opcoes).map_err(|e| e.to_string())?;
    let mut it = db.new_iter().map_err(|e| e.to_string())?;
    let mut por_origem: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    while let Some((k, v)) = rusty_leveldb::LdbIterator::next(&mut it) {
        let Some((origem, chave)) = chave_local_storage(&k) else { continue };
        let Some(valor) = texto_chromium(&v) else { continue };
        por_origem.entry(origem).or_default().insert(chave, valor);
    }
    drop(it);
    let _ = db.close();
    let escolhida = por_origem.keys().find(|o| o.contains("tauri.localhost")).cloned();
    Ok(match escolhida {
        Some(o) => {
            let itens = por_origem.remove(&o).unwrap_or_default();
            (Some(o), itens)
        }
        None => (None, BTreeMap::new()),
    })
}

/// `_<origem>\x00<codificação><chave>` → (origem, chave).
pub fn chave_local_storage(k: &[u8]) -> Option<(String, String)> {
    let resto = k.strip_prefix(b"_")?;
    let fim = resto.iter().position(|&b| b == 0)?;
    let origem = String::from_utf8(resto[..fim].to_vec()).ok()?;
    Some((origem, texto_chromium(&resto[fim + 1..])?))
}

/// Byte 0: UTF-16LE; byte 1: Latin-1.
pub fn texto_chromium(b: &[u8]) -> Option<String> {
    match b.split_first()? {
        (0, r) if r.len() % 2 == 0 => {
            String::from_utf16(&r.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>()).ok()
        }
        (1, r) => Some(r.iter().map(|&c| c as char).collect()),
        _ => None,
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn latin1(s: &str) -> Vec<u8> {
        std::iter::once(1).chain(s.chars().map(|c| c as u8)).collect()
    }
    fn utf16(s: &str) -> Vec<u8> {
        std::iter::once(0).chain(s.encode_utf16().flat_map(u16::to_le_bytes)).collect()
    }
    fn chave(origem: &str, k: Vec<u8>) -> Vec<u8> {
        [format!("_{origem}\0").into_bytes(), k].concat()
    }

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(nome: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!("orq-migracao-teste-{nome}-{}", agora()));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }

    const ORIGEM: &str = "http://tauri.localhost";
    const RASCUNHO: &str = r#"{"version":1,"project":"D:\\Proj","workspace":null,"layout":null,"split":null,"buffers":[],"conflicts":[]}"#;

    fn gerar(pasta: &Path) {
        let mut db = rusty_leveldb::DB::open(pasta, rusty_leveldb::Options { create_if_missing: true, ..Default::default() }).unwrap();
        db.put(b"VERSION", b"1").unwrap();
        db.put(format!("META:{ORIGEM}").as_bytes(), b"\x08\x01").unwrap();
        let par = |db: &mut rusty_leveldb::DB, k: Vec<u8>, v: Vec<u8>| db.put(&chave(ORIGEM, k), &v).unwrap();
        par(&mut db, latin1("orquestrador.project.tabs"), latin1(r#"["D:\\A","D:\\B"]"#));
        par(&mut db, latin1("orquestrador.project.active"), utf16("D:\\B"));
        par(&mut db, latin1("orquestrador.project.last"), latin1("D:\\B"));
        par(&mut db, latin1("orquestrador.project.recent"), utf16(r#"["D:\\B","D:\\Ação"]"#));
        par(&mut db, latin1("orquestrador.search.history"), utf16(r#"["fn ação","x","x"]"#));
        par(&mut db, latin1("orquestrador.search.view"), latin1("tree"));
        par(&mut db, utf16("orquestrador.editor:D:/Proj"), utf16(RASCUNHO));
        par(&mut db, latin1("orquestrador.editor:__sem-projeto__"), latin1("{}"));
        db.put(&chave("https://outro.site", latin1("orquestrador.project.tabs")), &latin1("[]")).unwrap();
        db.flush().unwrap();
        db.close().unwrap();
    }

    #[test]
    fn decodifica_chaves() {
        assert_eq!(chave_local_storage(&chave(ORIGEM, utf16("ç"))), Some((ORIGEM.into(), "ç".into())));
        assert_eq!(texto_chromium(&latin1("é")).as_deref(), Some("é"));
        assert_eq!(texto_chromium(&[0, 1]), None);
        assert_eq!(chave_local_storage(b"META:x"), None);
    }

    #[test]
    fn migra_uma_vez_sem_sobrescrever() {
        let web = tmp("web");
        let dest = tmp("dest");
        gerar(&web.0);
        // Dado GPUI mais novo na pesquisa: fica.
        let mut e = Estado::abrir(Some(dest.0.join("interface.json")));
        e.gravar(&PreferenciaPesquisa { historico: vec!["novo".into()], ..Default::default() });
        drop(e);
        let r = migrar(&web.0, &dest.0).unwrap().unwrap();
        assert_eq!(r.origem.as_deref(), Some(ORIGEM));
        assert_eq!(r.chaves.len(), 8);
        assert_eq!(r.mantidas, vec!["pesquisa".to_string()]);
        let e = Estado::abrir(Some(dest.0.join("interface.json")));
        let p: Projetos = e.ler();
        assert_eq!(p.abertas, vec!["D:\\A", "D:\\B"]);
        assert_eq!(p.ativa.as_deref(), Some("D:\\B"));
        assert_eq!(p.recentes, vec!["D:\\B", "D:\\Ação"]);
        assert_eq!(e.ler::<PreferenciaPesquisa>().historico, vec!["novo"]);
        let ed = ArquivoChaves::abrir(Some(dest.0.join("editor-recuperacao.json")));
        assert_eq!(ed.ler(&chave_recuperacao("d:\\proj")).as_deref(), Some(RASCUNHO));
        assert_eq!(ed.ler(&chave_recuperacao("")).as_deref(), Some("{}"));
        assert!(web.0.join("CURRENT").exists(), "original intacto");
        assert!(migrar(&web.0, &dest.0).unwrap().is_none(), "repetiu");
    }

    #[test]
    fn marcador_evita_segunda_leitura() {
        let web = tmp("web4");
        let dest = tmp("dest4");
        gerar(&web.0);
        assert!(executar_em(&web.0, &dest.0));
        assert!(dest.0.join(MARCADOR).exists());
        let t = std::time::Instant::now();
        assert!(!executar_em(&web.0, &dest.0));
        assert!(t.elapsed().as_millis() < 2, "verificação lenta");
    }

    #[test]
    fn pesquisa_vira_arvore_sem_pasta_marca() {
        let web = tmp("web2");
        let dest = tmp("dest2");
        gerar(&web.0);
        migrar(&web.0, &dest.0).unwrap();
        let p: PreferenciaPesquisa = Estado::abrir(Some(dest.0.join("interface.json"))).ler();
        assert_eq!(p.historico, vec!["fn ação", "x"]);
        assert_eq!(p.visao, orquestrador_pesquisa::logica::Visao::Arvore);
        let dest3 = tmp("dest3");
        assert!(migrar(&dest3.0.join("nada"), &dest3.0).unwrap().unwrap().chaves.is_empty());
        assert!(migrar(&dest3.0.join("nada"), &dest3.0).unwrap().is_none());
    }

    /// Conferência manual contra o perfil real: `MIGRACAO_WEBVIEW=<pasta> cargo test -- --ignored perfil_real --nocapture`.
    #[test]
    #[ignore]
    fn perfil_real() {
        let web = PathBuf::from(std::env::var("MIGRACAO_WEBVIEW").unwrap());
        let dest = tmp("real");
        let t = std::time::Instant::now();
        assert!(executar_em(&web, &dest.0));
        let pendente = t.elapsed().as_secs_f64() * 1000.0;
        let t = std::time::Instant::now();
        assert!(!executar_em(&web, &dest.0));
        let feita = t.elapsed().as_secs_f64() * 1000.0;
        println!("abertura: pendente {pendente:.1} ms; já migrada {feita:.3} ms");
        let (origem, itens) = ler_local_storage(&web).unwrap();
        println!("origem: {origem:?}; {} chaves", itens.len());
        for (k, v) in &itens {
            let nome = if k.starts_with(PREFIXO_EDITOR) { "orquestrador.editor:<projeto>" } else { k };
            println!("  {nome}: {} bytes", v.len());
        }
        let p: Projetos = Estado::abrir(Some(dest.0.join("interface.json"))).ler();
        let q: PreferenciaPesquisa = Estado::abrir(Some(dest.0.join("interface.json"))).ler();
        println!("projetos: {} abertas, ativa {}, {} recentes; pesquisa: {} no histórico", p.abertas.len(), p.ativa.is_some(), p.recentes.len(), q.historico.len());
        for a in ["interface.json", "editor-recuperacao.json"] {
            println!("{a}: {} bytes", std::fs::metadata(dest.0.join(a)).map(|m| m.len()).unwrap_or(0));
        }
    }
}
