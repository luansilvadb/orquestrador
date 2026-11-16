//! Regras de release sem rede (porte do antigo `check-release.mjs`).
use super::*;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const WORDS: &str = "Texto concreto sobre a mudança, com o comportamento real do app e o que muda para quem usa. A descrição diz o que o usuário vê, o que mudou por baixo e qualquer limite que continua valendo depois da atualização.";

fn section(title: &str, words: &str) -> String {
    format!("## {title}\n\n{words}\n")
}

fn valid_notes(version: &str, previous: &str) -> String {
    let cmp = format!("Comparado com v{previous}, o app agora faz isso e aquilo, com a mesma base de dados e a mesma configuração do usuário.");
    [
        format!("# Orquestrador v{version}"),
        section("Resumo", WORDS),
        section("Novidades", WORDS),
        section("Melhorias e correções", WORDS),
        section("Impacto da atualização", WORDS),
        section("Requisitos e limitações", WORDS),
        section("Instalação", WORDS),
        section("Comparação com a versão anterior", &cmp),
    ]
    .join("\n")
}

fn notes() -> String {
    valid_notes("0.1.7", "0.1.6")
}

fn erro<T: std::fmt::Debug>(r: R<T>) -> String {
    r.expect_err("devia falhar")
}

fn contem(s: &str, re: &str) -> bool {
    regex::Regex::new(re).unwrap().is_match(s)
}

// ---------------------------------------------------------------- CI

fn ci_sha() -> String {
    "a".repeat(40)
}

fn ci_run(over: Value) -> Value {
    let mut r = json!({
        "attempt": 1, "conclusion": "success", "createdAt": "2026-10-30T10:00:00Z", "databaseId": 100, "event": "push",
        "headBranch": "main", "headSha": ci_sha(), "startedAt": "2026-10-30T10:00:05Z", "status": "completed",
        "updatedAt": "2026-10-30T10:01:00Z", "workflowDatabaseId": 20, "workflowName": "ci",
    });
    for (k, v) in over.as_object().unwrap() {
        r[k] = v.clone();
    }
    r
}

fn saida(runs: &[Value]) -> String {
    serde_json::to_string(runs).unwrap()
}

/// `exec` fixo, relógio parado e sem espera.
fn ci_fixo(out: R<String>) -> Ci<'static> {
    Ci {
        exec: Box::new(move |_, _| out.clone()),
        now: Box::new(|| 0.),
        sleep: Box::new(|_| {}),
        timeout_ms: CI_WAIT_MS,
        poll_ms: 15_000.,
    }
}

fn ci_com(runs: &[Value]) -> R<String> {
    wait_for_ci(&ci_sha(), ci_fixo(Ok(saida(runs))))
}

#[test]
fn ci_so_aceita_push_de_ci_yml_em_main_para_o_sha_exato() {
    let visto: RefCell<Option<(Vec<String>, u64)>> = RefCell::new(None);
    let ci = Ci {
        exec: Box::new(|a, t| {
            *visto.borrow_mut() = Some((a.to_vec(), t));
            Ok(saida(&[ci_run(json!({}))]))
        }),
        ..ci_fixo(Ok(String::new()))
    };
    let msg = wait_for_ci(&ci_sha(), ci).unwrap();
    assert!(msg.contains(&ci_sha()));
    let (args, timeout) = visto.into_inner().unwrap();
    assert_eq!(&args[..3], ["run", "list", "--repo"]);
    let depois = |f: &str| args.iter().position(|a| a == f).map(|i| args[i + 1].clone());
    assert_eq!(depois("--workflow").as_deref(), Some("ci.yml"));
    assert_eq!(depois("--commit"), Some(ci_sha()));
    assert_eq!(depois("--branch").as_deref(), Some("main"));
    assert_eq!(depois("--event").as_deref(), Some("push"));
    assert_eq!(depois("--limit").as_deref(), Some("1000"));
    assert!(args.iter().any(|a| a == "--json"));
    assert!(!args.iter().any(|a| a == "--status") && timeout > 0 && timeout as f64 <= CI_WAIT_MS);
}

#[test]
fn workflow_permissoes_e_gates_precedem_alteracoes_remotas_e_publicacao() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let texto = std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
    let wf: serde_yaml::Value = serde_yaml::from_str(&texto).unwrap();
    assert_eq!(wf["permissions"]["actions"].as_str(), Some("read"));
    let steps = wf["jobs"]["release"]["steps"].as_sequence().unwrap();
    let run = |s: &serde_yaml::Value| s["run"].as_str().unwrap_or("").to_string();
    let gate = |r: &str| r.rfind("cargo xtask release wait-ci $sha");
    let gates: Vec<usize> =
        (0..steps.len()).filter(|&i| run(&steps[i]).contains("git rev-parse HEAD") && gate(&run(&steps[i])).is_some()).collect();
    assert_eq!(gates.len(), 2);
    let escrita = regex::Regex::new(r"\bgh release (?:create|edit|upload|delete)\b").unwrap();
    let writes: Vec<usize> = (0..steps.len())
        .filter(|&i| escrita.is_match(&run(&steps[i])))
        .collect();
    assert!(!writes.is_empty());
    assert!(writes.iter().all(|&i| gates[0] < i));
    let publish = run(&steps[gates[1]]);
    let cmd = publish.find("--draft=false --latest").unwrap();
    assert!(cmd > gate(&publish).unwrap());
}

#[test]
fn ci_aguarda_execucao_ausente_ou_pendente_e_so_retorna_apos_sucesso() {
    let previous = ci_run(json!({}));
    let queued = ci_run(json!({"databaseId": 101, "createdAt": "2026-10-30T11:00:00Z", "updatedAt": "2026-10-30T11:00:01Z", "status": "queued", "conclusion": "", "startedAt": "0001-01-01T00:00:00Z"}));
    let mut queued_success = queued.clone();
    for (k, v) in json!({"startedAt": "2026-10-30T11:00:05Z", "updatedAt": "2026-10-30T11:01:00Z", "status": "completed", "conclusion": "success"}).as_object().unwrap() {
        queued_success[k] = v.clone();
    }
    let in_progress = ci_run(json!({"databaseId": 102, "createdAt": "2026-10-30T10:59:00Z", "startedAt": "2026-10-30T11:00:05Z", "updatedAt": "2026-10-30T11:00:10Z", "status": "in_progress", "conclusion": ""}));
    let mut in_progress_success = in_progress.clone();
    in_progress_success["updatedAt"] = "2026-10-30T11:01:00Z".into();
    in_progress_success["status"] = "completed".into();
    in_progress_success["conclusion"] = "success".into();
    let casos = vec![
        (vec![], vec![ci_run(json!({}))]),
        (vec![queued.clone()], vec![queued_success.clone()]),
        (vec![previous.clone(), queued], vec![previous, queued_success]),
        (vec![in_progress], vec![in_progress_success]),
    ];
    for (first, second) in casos {
        let calls = Cell::new(0);
        let clock = Rc::new(Cell::new(0.));
        let (c1, c2) = (clock.clone(), clock.clone());
        let ci = Ci {
            exec: Box::new(|_, _| {
                let n = calls.get();
                calls.set(n + 1);
                Ok(saida(if n == 0 { &first } else { &second }))
            }),
            now: Box::new(move || c1.get()),
            sleep: Box::new(move |ms| c2.set(c2.get() + ms)),
            timeout_ms: 100.,
            poll_ms: 20.,
        };
        wait_for_ci(&ci_sha(), ci).unwrap();
        assert_eq!(calls.get(), 2);
    }
}

#[test]
fn ci_conclusao_terminal_incoerente_ou_ausente_bloqueia() {
    assert!(erro(ci_com(&[ci_run(json!({"status": "in_progress", "conclusion": "success"}))])).contains("conclusão inesperada"));
    assert!(erro(ci_com(&[ci_run(json!({"conclusion": ""}))])).contains("conclusão ausente"));
}

#[test]
fn ci_sha_branch_evento_ou_workflow_diferentes_bloqueiam() {
    for over in [json!({"headSha": "b".repeat(40)}), json!({"headBranch": "release"}), json!({"event": "pull_request"}), json!({"workflowName": "outro"})] {
        assert!(erro(ci_com(&[ci_run(over)])).contains("diferente do esperado"));
    }
}

#[test]
fn ci_falha_da_tentativa_mais_recente_nao_e_mascarada_por_sucesso_anterior() {
    let previous = ci_run(json!({"databaseId": 101, "createdAt": "2026-10-30T09:00:00Z", "startedAt": "2026-10-30T09:00:05Z", "updatedAt": "2026-10-30T09:01:00Z"}));
    for conclusion in ["failure", "cancelled", "skipped"] {
        let latest = ci_run(json!({"attempt": 2, "databaseId": 102, "createdAt": "2026-10-30T11:00:00Z", "startedAt": "2026-10-30T11:00:05Z", "updatedAt": "2026-10-30T11:01:00Z", "conclusion": conclusion}));
        assert!(erro(ci_com(&[previous.clone(), latest])).contains("tentativa 2"));
    }
}

#[test]
fn ci_tentativa_reexecutada_mais_recente_vence_run_criado_depois() {
    let newer = ci_run(json!({"databaseId": 103, "createdAt": "2026-10-30T10:00:00Z", "startedAt": "2026-10-30T10:00:05Z", "updatedAt": "2026-10-30T10:01:00Z"}));
    let rerun = ci_run(json!({"attempt": 2, "databaseId": 104, "createdAt": "2026-10-30T09:00:00Z", "startedAt": "2026-10-30T11:00:05Z", "updatedAt": "2026-10-30T11:01:00Z", "conclusion": "failure"}));
    assert!(erro(ci_com(&[newer, rerun])).contains("tentativa 2"));
}

#[test]
fn ci_api_json_e_campos_invalidos_bloqueiam_sem_aguardar() {
    assert!(erro(wait_for_ci(&ci_sha(), ci_fixo(Err("gh indisponível".into())))).contains("gh indisponível"));
    assert!(erro(wait_for_ci(&ci_sha(), ci_fixo(Ok("{".into())))).contains("JSON inválido"));
    assert!(erro(wait_for_ci(&ci_sha(), ci_fixo(Ok("{}".into())))).contains("não retornou uma lista"));
    assert!(erro(ci_com(&[ci_run(json!({"updatedAt": "inválido"}))])).contains("updatedAt inválido"));
    assert!(erro(ci_com(&[ci_run(json!({"conclusion": 0}))])).contains("conclusion inválida"));
    assert!(erro(ci_com(&[ci_run(json!({"headSha": 42}))])).contains("diferente do esperado"));
}

#[test]
fn ci_ausencia_ou_pendencia_persistente_e_sucesso_depois_do_prazo_expiram() {
    for out in ["[]".to_string(), saida(&[ci_run(json!({"status": "in_progress", "conclusion": ""}))])] {
        let clock = Rc::new(Cell::new(0.));
        let (c1, c2) = (clock.clone(), clock.clone());
        let ci = Ci {
            exec: Box::new(move |_, _| Ok(out.clone())),
            now: Box::new(move || c1.get()),
            sleep: Box::new(move |ms| c2.set(c2.get() + ms)),
            timeout_ms: 50.,
            poll_ms: 25.,
        };
        assert!(erro(wait_for_ci(&ci_sha(), ci)).contains("30 minutos"));
    }
    let clock = Rc::new(Cell::new(0.));
    let calls = Cell::new(0);
    let (c1, c2) = (clock.clone(), clock.clone());
    let ci = Ci {
        exec: Box::new(|_, timeout| {
            calls.set(calls.get() + 1);
            c1.set(c1.get() + timeout as f64 + 1.);
            Ok(saida(&[ci_run(json!({}))]))
        }),
        now: Box::new(move || c2.get()),
        sleep: Box::new(|_| {}),
        timeout_ms: 50.,
        poll_ms: 15_000.,
    };
    assert!(erro(wait_for_ci(&ci_sha(), ci)).contains("30 minutos"));
    assert_eq!(calls.get(), 1);
}

// ---------------------------------------------------------------- notas, versões e manifesto

/// Assinatura no formato do Tauri: base64 do .sig do minisign, com o comentário confiável em tabs.
fn sig_for(version: &str, extra: &str) -> String {
    STANDARD.encode(format!(
        "untrusted comment: signature from tauri secret key\nRUSIG\ntrusted comment: timestamp:1700000000\tfile:Orquestrador_{version}_x64-setup.exe{extra}\tversion:{version}\nGLOBALSIG\n"
    ))
}

fn manifest(version: &str, sig: &str) -> String {
    serde_json::to_string_pretty(&json!({
        "version": version, "notes": "antigas", "pub_date": "2026-10-14T00:00:00Z",
        "platforms": { "windows-x86_64": { "signature": sig, "url": "https://api.github.com/repos/x/assets/1" } },
    }))
    .unwrap()
}

/// latest.json já com a URL pública e as notas, como o fluxo de release publica.
fn manifest_pronto(version: &str, sig: &str, notes: &str) -> String {
    let mut m: Value = serde_json::from_str(&manifest(version, sig)).unwrap();
    m["platforms"]["windows-x86_64"]["url"] = installer_url(version, REPO).into();
    m["notes"] = canonical_text(notes.trim()).into();
    serde_json::to_string_pretty(&m).unwrap()
}

#[test]
fn notas_completas_passam_com_a_comparacao_citando_a_anterior() {
    assert!(note_problems(&notes(), "0.1.7", Some("0.1.6")).is_empty());
    assert!(check_notes(&notes(), "0.1.7", Some("0.1.6")).unwrap().starts_with("# Orquestrador v0.1.7"));
}

#[test]
fn notas_ausentes_vazias_ou_so_com_titulo_sao_recusadas() {
    for t in ["", "   \n", "# Orquestrador v0.1.7\n"] {
        assert!(erro(check_notes(t, "0.1.7", None)).contains("Notas da 0.1.7 recusadas"));
    }
}

#[test]
fn titulo_com_outra_versao_e_recusado() {
    assert!(note_problems(&valid_notes("0.1.6", "0.1.6"), "0.1.7", None).join("\n").contains("primeira linha"));
}

#[test]
fn texto_generico_e_de_modelo_e_recusado() {
    let generic = notes().replacen("## Resumo", "## Resumo\n\nTODO escrever depois", 1);
    assert!(note_problems(&generic, "0.1.7", Some("0.1.6")).join("\n").contains("genérico: \"TODO\""));
    let auto = notes().replacen("Texto concreto", "Instalador para Windows, por usuário. O app se atualiza sozinho ao abrir.", 1);
    assert!(note_problems(&auto, "0.1.7", Some("0.1.6")).join("\n").contains("genérico"));
}

#[test]
fn secao_obrigatoria_faltando_ou_vazia_e_recusada() {
    let n = notes();
    let (a, b) = (n.find("## Requisitos e limitações").unwrap(), n.find("## Instalação").unwrap());
    let missing = format!("{}{}", &n[..a], &n[b..]);
    assert!(note_problems(&missing, "0.1.7", Some("0.1.6")).join("\n").contains("falta a seção \"## Requisitos e limitações\""));
    let empty = n.replacen(&section("Resumo", WORDS), "## Resumo\n\n", 1);
    assert!(note_problems(&empty, "0.1.7", Some("0.1.6")).join("\n").contains("\"## Resumo\" está vazia ou curta demais"));
}

#[test]
fn notas_curtas_demais_sao_recusadas_mesmo_com_todas_as_secoes() {
    let short = ["# Orquestrador v0.1.7", "## Resumo\n\nCurto.", "## Novidades\n\nCurto."].join("\n");
    assert!(note_problems(&short, "0.1.7", None).join("\n").contains("curtas demais"));
}

#[test]
fn comparacao_que_nao_cita_a_anterior_e_recusada() {
    assert!(note_problems(&valid_notes("0.1.7", "0.1.5"), "0.1.7", Some("0.1.6")).join("\n").contains("não cita v0.1.6"));
}

#[test]
fn versoes_iguais_passam_e_divergente_e_apontado() {
    let m = |p: &[(&str, Option<&str>)]| p.iter().map(|(k, v)| (k.to_string(), v.map(String::from))).collect::<BTreeMap<_, _>>();
    let same = m(&[("a", Some("0.1.7")), ("b", Some("0.1.7"))]);
    assert_eq!(check_versions(&same, "0.1.7").unwrap(), "0.1.7");
    let e = erro(check_versions(&m(&[("a", Some("0.1.7")), ("b", Some("0.1.6")), ("c", None)]), "0.1.7"));
    assert!(contem(&e, r"b: 0\.1\.6[\s\S]*c: ausente"));
    assert!(erro(check_versions(&same, "v0.1.7")).contains("não é uma versão X.Y.Z"));
}

#[test]
fn cargo_le_a_versao_do_package_e_as_entradas_no_lock() {
    let toml = "[package]\nname = \"orquestrador\"\nversion = \"0.1.7\"\n\n[dependencies]\nserde = { version = \"1\" }\n";
    assert_eq!(cargo_package_version(toml, "[package]").as_deref(), Some("0.1.7"));
    assert_eq!(cargo_package_version("[dependencies]\nversion = \"9.9.9\"\n", "[package]"), None);
    assert_eq!(
        cargo_package_version("[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.8\"\n", "[workspace.package]").as_deref(),
        Some("0.1.8")
    );
    let lock = "[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"orquestrador\"\nversion = \"0.1.7\"\ndependencies = []\n";
    assert_eq!(cargo_lock_versions(lock, "orquestrador"), ["0.1.7"]);
    assert!(cargo_lock_versions("name = \"x\"\n", "orquestrador").is_empty());
}

#[test]
fn repositorio_tem_as_versoes_sincronizadas() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let v = manifest_versions(&root).unwrap();
    let ws = v["Cargo.toml ([workspace.package])"].clone().unwrap();
    check_versions(&v, &ws).unwrap();
}

#[test]
fn manifesto_url_de_rascunho_assinatura_trocada_ou_notes_antigas_sao_recusados() {
    let sig = sig_for("0.1.7", "");
    let n = notes();
    let fixed = manifest_pronto("0.1.7", &sig, &n);
    verify_manifest(&fixed, "0.1.7", &sig, Some(&n), REPO).unwrap();
    assert!(erro(verify_manifest(&manifest("0.1.7", &sig), "0.1.7", &sig, Some(&n), REPO)).contains("URL https://api.github"));
    assert!(erro(verify_manifest(&fixed, "0.1.7", &sig_for("0.1.7", "\tother"), Some(&n), REPO)).contains("não é a do .sig"));
    assert!(erro(verify_manifest(&fixed, "0.1.7", &sig, Some("outra coisa"), REPO)).contains("notes do latest.json"));
    assert!(erro(verify_manifest(&fixed, "0.1.8", &sig, None, REPO)).contains("anuncia 0.1.7, não 0.1.8"));
}

#[test]
fn notes_do_latest_crlf_e_espaco_final_sao_o_mesmo_texto() {
    let sig = sig_for("0.1.7", "");
    let lf = notes();
    let crlf = lf.replace('\n', "\r\n");
    let mut m: Value = serde_json::from_str(&manifest_pronto("0.1.7", &sig, &lf)).unwrap();
    m["notes"] = crlf.clone().into();
    let crlf_manifest = serde_json::to_string_pretty(&m).unwrap();
    verify_manifest(&crlf_manifest, "0.1.7", &sig, Some(&lf), REPO).unwrap();
    verify_manifest(&manifest_pronto("0.1.7", &sig, &crlf), "0.1.7", &sig, Some(&crlf), REPO).unwrap();
    verify_manifest(&crlf_manifest, "0.1.7", &sig, Some(&format!("{crlf}\r\n")), REPO).unwrap();
    assert!(erro(verify_manifest(&crlf_manifest, "0.1.7", &sig, Some(&lf.replacen("Resumo", "Resumo alterado", 1)), REPO)).contains("notes do latest.json"));
    assert!(erro(verify_manifest(&crlf_manifest, "0.1.7", &sig, Some(&format!("{lf}x")), REPO)).contains("notes do latest.json"));
    let out: Value = serde_json::from_str(&manifest_pronto("0.1.7", &sig, &crlf)).unwrap();
    assert_eq!(out["notes"], lf.trim());
}

// ---------------------------------------------------------------- corpo

#[test]
fn corpo_da_release_sem_titulo_com_todas_as_secoes_na_ordem() {
    let body = release_body(&notes());
    assert!(body.starts_with("## Resumo\n"));
    assert!(!contem(&body, r"(?m)^# "));
    let titulos: Vec<&str> = secoes(&body).iter().map(|(t, _)| *t).collect();
    assert_eq!(titulos, REQUIRED_SECTIONS);
    assert_eq!(release_body(&format!("\r\n\r\n{}\r\n", notes().replace('\n', "\r\n"))), body);
}

#[test]
fn corpo_da_release_igual_as_notas_e_alteracoes_recusadas() {
    let n = notes();
    let body = release_body(&n);
    let ok = "corpo da release igual às notas, sem o título.";
    assert_eq!(verify_body(&format!("{body}\n\n"), &n).unwrap(), ok);
    assert_eq!(verify_body(&body.replace('\n', "\r\n"), &n).unwrap(), ok);
    let (a, b) = (body.find("## Instalação").unwrap(), body.find("## Comparação").unwrap());
    for ruim in [
        n.clone(),
        format!("# Orquestrador v0.1.7\r\n\r\n{body}"),
        format!("{}{}", &body[..a], &body[b..]),
        format!("{body}x"),
        body.replacen("Resumo", "Resumo alterado", 1),
        String::new(),
    ] {
        assert!(erro(verify_body(&ruim, &n)).contains("sem o título"));
    }
}

#[test]
fn data_iso_como_date_parse() {
    assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0.));
    assert_eq!(parse_iso("2026-10-30T10:00:05Z"), Some(1_793_354_405_000.));
    assert_eq!(parse_iso("2026-10-30T10:00:05.5+01:00"), Some(1_793_354_405_500. - 3_600_000.));
    assert_eq!(parse_iso("inválido"), None);
}
