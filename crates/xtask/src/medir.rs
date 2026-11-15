//! Medição de desempenho do exe GPUI (`orquestrador`), sem intervenção. Ver docs/desempenho-gpui.md.
//! Cenário isolado: LOCALAPPDATA, TEMP e repositório em pasta descartável; build de desenvolvimento otimizado
//! (`debug_assertions` ligado: sem instância única e sem atualização de CLIs). O app grava os marcos pelo gancho
//! `crates/app/src/medir.rs` (`ORQ_MEDIR_DIR`) e a latência por `ORQ_MEDIR` (`orq-latencia.txt` no TEMP do filho).
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MB: f64 = 1024. * 1024.;
/// Comando que desliga histórico e predição do PSReadLine. `# secret` faz o PSReadLine não gravar a própria linha.
const SEM_HISTORICO: &str = "Set-PSReadLineOption -HistorySaveStyle SaveNothing -PredictionSource None";

struct Opcoes {
    exe: Option<PathBuf>,
    out: Option<PathBuf>,
    aberturas: usize,
    amostras: usize,
    taxa: usize,
    despejo_s: usize,
    so_abertura: bool,
}

fn opcoes(args: &[String]) -> Result<Opcoes, String> {
    let mut o = Opcoes { exe: None, out: None, aberturas: 5, amostras: 250, taxa: 6000, despejo_s: 70, so_abertura: false };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().cloned().ok_or(format!("{a} sem valor"));
        let n = |s: String| s.parse::<usize>().map_err(|_| format!("{a}: número inválido"));
        match a.as_str() {
            "--exe" => o.exe = Some(v()?.into()),
            "--out" => o.out = Some(v()?.into()),
            "--aberturas" => o.aberturas = n(v()?)?,
            "--amostras" => o.amostras = n(v()?)?,
            "--taxa" => o.taxa = n(v()?)?,
            "--despejo" => o.despejo_s = n(v()?)?,
            "--so-abertura" => o.so_abertura = true,
            _ => return Err(format!("opção desconhecida: {a}. Opções: --exe, --out, --aberturas, --amostras, --taxa, --despejo, --so-abertura")),
        }
    }
    Ok(o)
}

fn agora_us() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_micros()).unwrap_or(0)
}

fn esperar(limite: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < limite {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn git(repo: &Path, args: &[&str]) -> Result<(), String> {
    let s = Command::new("git")
        .args(["-c", "user.name=medir", "-c", "user.email=medir@exemplo.invalid", "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if s.status.success() {
        Ok(())
    } else {
        Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&s.stderr)))
    }
}

/// Repositório de 40 commits, `src/app.js` de 3000 linhas modificado e `novo.txt` não rastreado.
fn repositorio(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;
    git(dir, &["init", "-q", "-b", "main"])?;
    let js = |rev: usize| -> String {
        (0..3000).map(|i| format!("export function f{i}(x) {{ return x * {} + {i}; }} // revisão {rev}\n", (i + rev) % 97)).collect()
    };
    for c in 0..40 {
        std::fs::write(dir.join("src/app.js"), js(c)).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("src/m{}.txt", c % 8)), format!("commit {c}\n")).map_err(|e| e.to_string())?;
        git(dir, &["add", "-A"])?;
        git(dir, &["commit", "-q", "-m", &format!("commit {c}")])?;
    }
    std::fs::write(dir.join("src/app.js"), js(40)).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("novo.txt"), "não rastreado\n").map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------- Windows: processos, memória e teclas

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_KEYDOWN, WM_KEYUP,
    };

    pub struct Proc {
        pub pid: u32,
        pub pai: u32,
        pub nome: String,
    }

    pub fn processos() -> Vec<Proc> {
        let mut v = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap as isize == -1 {
                return v;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                let fim = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                v.push(Proc { pid: e.th32ProcessID, pai: e.th32ParentProcessID, nome: String::from_utf16_lossy(&e.szExeFile[..fim]) });
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
        }
        v
    }

    /// Private Bytes (commit) e working set, em bytes.
    pub fn memoria(pid: u32) -> Option<(u64, u64)> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut c: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
            c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
            let ok = K32GetProcessMemoryInfo(h, &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb);
            CloseHandle(h);
            (ok != 0).then_some((c.PrivateUsage as u64, c.WorkingSetSize as u64))
        }
    }

    pub fn janela(pid: u32) -> Option<HWND> {
        struct Busca {
            pid: u32,
            achada: HWND,
        }
        unsafe extern "system" fn cada(h: HWND, l: LPARAM) -> BOOL {
            let b = &mut *(l as *mut Busca);
            let mut p = 0;
            GetWindowThreadProcessId(h, &mut p);
            if p == b.pid && IsWindowVisible(h) != 0 {
                b.achada = h;
                return 0;
            }
            1
        }
        let mut b = Busca { pid, achada: std::ptr::null_mut() };
        unsafe { EnumWindows(Some(cada), &mut b as *mut Busca as LPARAM) };
        (!b.achada.is_null()).then_some(b.achada)
    }

    /// Tecla posta na fila da janela (não exige a janela em primeiro plano nem mexe no teclado do usuário).
    pub fn tecla(h: HWND, vk: u16) {
        unsafe {
            let sc = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as isize;
            PostMessageW(h, WM_KEYDOWN, vk as usize, 1 | (sc << 16));
            PostMessageW(h, WM_KEYUP, vk as usize, 1 | (sc << 16) | (1 << 30) | (1 << 31));
        }
    }
}

/// Memória da árvore do app: o exe e, à parte, shells (pwsh, conhost) e o gerador.
#[cfg(windows)]
fn memoria_arvore(raiz: u32) -> Value {
    let ps = win::processos();
    let mut arvore = vec![raiz];
    let mut i = 0;
    while i < arvore.len() {
        let p = arvore[i];
        arvore.extend(ps.iter().filter(|x| x.pai == p && x.pid != p).map(|x| x.pid));
        i += 1;
    }
    let mut grupos: std::collections::BTreeMap<String, (u32, f64, f64)> = Default::default();
    let mut por_processo = Vec::new();
    for pid in &arvore {
        let Some(p) = ps.iter().find(|x| x.pid == *pid) else { continue };
        let Some((pv, ws)) = win::memoria(*pid) else { continue };
        let nome = p.nome.to_lowercase();
        let grupo = if *pid == raiz {
            "app"
        } else if nome.starts_with("pwsh") || nome.starts_with("powershell") {
            "pwsh"
        } else if nome == "conhost.exe" || nome == "openconsole.exe" {
            "conhost"
        } else if nome == "xtask.exe" {
            "gerador"
        } else {
            "outros"
        };
        let g = grupos.entry(grupo.into()).or_default();
        g.0 += 1;
        g.1 += pv as f64 / MB;
        g.2 += ws as f64 / MB;
        por_processo.push(json!({"pid": pid, "nome": p.nome, "grupo": grupo, "privada_mb": pv as f64 / MB, "ws_mb": ws as f64 / MB}));
    }
    let grupos: serde_json::Map<String, Value> = grupos
        .into_iter()
        .map(|(k, (n, pv, ws))| (k, json!({"processos": n, "privada_mb": pv, "ws_mb": ws})))
        .collect();
    json!({"grupos": grupos, "processos": por_processo})
}

fn mb(v: &Value, grupo: &str, campo: &str) -> f64 {
    v["grupos"][grupo][campo].as_f64().unwrap_or(0.)
}

// ---------------------------------------------------------------- execução do app

struct App {
    filho: Child,
    dir: PathBuf,
    t0_us: u128,
}

impl App {
    /// Sobe o exe com LOCALAPPDATA e TEMP próprios em `dir`; `roteiro` vira um shell por linha.
    fn subir(exe: &Path, repo: &Path, dir: &Path, roteiro: Option<&[String]>) -> Result<App, String> {
        let _ = std::fs::remove_dir_all(dir);
        for d in ["local", "temp", "sinais"] {
            std::fs::create_dir_all(dir.join(d)).map_err(|e| e.to_string())?;
        }
        if let Some(r) = roteiro {
            std::fs::write(dir.join("roteiro.txt"), r.join("\n")).map_err(|e| e.to_string())?;
        }
        let t0_us = agora_us();
        let filho = Command::new(exe)
            .arg(repo)
            .current_dir(repo)
            .env("LOCALAPPDATA", dir.join("local"))
            .env("TEMP", dir.join("temp"))
            .env("TMP", dir.join("temp"))
            .env("ORQ_MEDIR", "1")
            .env("ORQ_MEDIR_DIR", dir)
            .env("ORQ_DONE_DIR", dir.join("sinais"))
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(dir.join("stderr.txt")).map_err(|e| e.to_string())?)
            .spawn()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        Ok(App { filho, dir: dir.to_path_buf(), t0_us })
    }

    /// Spawn até o primeiro quadro, em ms.
    fn abertura(&mut self) -> Result<f64, String> {
        let arq = self.dir.join("abertura.txt");
        let mut t = None;
        esperar(Duration::from_secs(60), || {
            t = std::fs::read_to_string(&arq).ok().and_then(|s| s.trim().parse::<u128>().ok());
            t.is_some()
        });
        if let Ok(Some(s)) = self.filho.try_wait() {
            return Err(format!("o app saiu ({s}); veja {}", self.dir.join("stderr.txt").display()));
        }
        let t = t.ok_or("o app não marcou o primeiro quadro em 60 s")?;
        Ok((t.saturating_sub(self.t0_us)) as f64 / 1000.)
    }

    fn sinal(&self, nome: &str) -> bool {
        self.dir.join("sinais").join(nome).exists()
    }

    /// Última linha de `orq-latencia.txt`: `n`, e p50/p95/máx de total, pty e janela.
    fn latencia(&self) -> Option<Value> {
        let t = std::fs::read_to_string(self.dir.join("temp/orq-latencia.txt")).ok()?;
        let l = t.lines().last()?;
        // Compilada uma vez: o xtask é de dev, e compilar a cada par de teclas atrasava o eco do pwsh em ~5,5 ms.
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| regex::Regex::new(r"n=(\d+) total\[p50=([\d.]+) p95=([\d.]+) max=([\d.]+)\] pty\[p50=([\d.]+) p95=([\d.]+) max=([\d.]+)\] janela\[p50=([\d.-]+) p95=([\d.-]+) max=([\d.-]+)\]").unwrap());
        let c = re.captures(l)?;
        let f = |i: usize| c[i].parse::<f64>().unwrap_or(f64::NAN);
        Some(json!({
            "n": c[1].parse::<u64>().unwrap_or(0),
            "total": {"p50": f(2), "p95": f(3), "max": f(4)},
            "pty": {"p50": f(5), "p95": f(6), "max": f(7)},
            "janela": {"p50": f(8), "p95": f(9), "max": f(10)},
        }))
    }

    fn encerrar(mut self) {
        let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &self.filho.id().to_string()]).output();
        let _ = self.filho.wait();
    }
}

/// Teclas no shell ativo (o último do roteiro): letra, pausa, Backspace, pausa, até `orq-latencia.txt` somar
/// `alvo` amostras (cada tecla que muda a linha do cursor conta uma).
#[cfg(windows)]
fn digitar(app: &App, alvo: u64) -> Result<Value, String> {
    let h = win::janela(app.filho.id()).ok_or("janela do app não encontrada")?;
    let mut seed = 7u32;
    let mut pausa = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        std::thread::sleep(Duration::from_millis(20 + (seed >> 26) as u64));
    };
    let n = || app.latencia().and_then(|v| v["n"].as_u64()).unwrap_or(0);
    let inicio = n();
    if inicio != 0 {
        return Err("orq-latencia.txt já tinha amostras".into());
    }
    let mut i = 0u16;
    while n() < alvo {
        if i as u64 > alvo * 2 {
            return Err(format!("só {} amostras depois de {i} pares de teclas: a janela não recebe as teclas?", n()));
        }
        win::tecla(h, 0x41 + i % 26);
        pausa();
        win::tecla(h, 0x08);
        pausa();
        i += 1;
    }
    let mut v = app.latencia().unwrap();
    v["teclas"] = json!(i as u64 * 2);
    Ok(v)
}

/// Histórico do PSReadLine (a pasta vem da API do Windows; o `APPDATA` do filho não vale).
fn historico() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(r"Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt"))
}

/// Shells 1 a 4: geradores esperando `go`; o 5º só avisa que está pronto e recebe as teclas.
fn roteiro(gerador: &Path, sinais: &Path, linhas: usize, taxa: usize) -> Vec<String> {
    let mut r: Vec<String> = (1..=4)
        .map(|t| {
            format!(
                "{SEM_HISTORICO}; & '{}' despejar {linhas} {t} {taxa} --esperar '{}' # secret",
                gerador.display(),
                sinais.join("go").display()
            )
        })
        .collect();
    r.push(format!("{SEM_HISTORICO}; Set-Content -LiteralPath '{}' 1; Clear-Host # secret", sinais.join("pronto-5").display()));
    r
}

#[cfg(windows)]
fn cenario(exe: &Path, repo: &Path, dir: &Path, opc: &Opcoes, com_carga: bool) -> Result<Value, String> {
    let gerador = std::env::current_exe().map_err(|e| e.to_string())?;
    let linhas = opc.taxa * opc.despejo_s;
    let r = roteiro(&gerador, &dir.join("sinais"), linhas, opc.taxa);
    let mut app = App::subir(exe, repo, dir, Some(&r))?;
    let abertura = app.abertura()?;
    let prontos = || app.sinal("pronto-5") && (1..=4).all(|t| app.sinal(&format!("pronto-{t}")));
    if !esperar(Duration::from_secs(90), prontos) {
        app.encerrar();
        return Err("os 5 shells não ficaram prontos em 90 s".into());
    }
    let pid = app.filho.id();
    std::thread::sleep(Duration::from_secs(15));
    let ocioso = memoria_arvore(pid);
    let alvo = opc.amostras as u64 * 2;
    let mut out = json!({"abertura_ms": abertura, "ocioso": ocioso});
    if !com_carga {
        let lat = digitar(&app, alvo);
        app.encerrar();
        out["latencia"] = lat?;
        return Ok(out);
    }
    std::fs::write(dir.join("sinais/go"), "1").map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    // Série de memória a cada 3 s até o fim do despejo.
    let parar = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let p2 = parar.clone();
    let serie = std::thread::spawn(move || {
        let mut s = Vec::new();
        let t = Instant::now();
        while !p2.load(std::sync::atomic::Ordering::Acquire) {
            let m = memoria_arvore(pid);
            s.push(json!({"t_s": t.elapsed().as_secs_f64(), "app_privada_mb": mb(&m, "app", "privada_mb"), "app_ws_mb": mb(&m, "app", "ws_mb")}));
            std::thread::sleep(Duration::from_secs(3));
        }
        s
    });
    if !esperar(Duration::from_secs(10), || (1..=4).all(|t| app.sinal(&format!("inicio-{t}")))) {
        app.encerrar();
        return Err("os geradores não começaram".into());
    }
    std::thread::sleep(Duration::from_secs(15).saturating_sub(t0.elapsed()));
    out["durante"] = memoria_arvore(pid);
    let lat = digitar(&app, alvo);
    let ativo_no_fim = !(1..=4).all(|t| app.sinal(&format!("fim-{t}")));
    let limite = Duration::from_secs(opc.despejo_s as u64 * 3 + 60);
    let terminou = esperar(limite, || (1..=4).all(|t| app.sinal(&format!("fim-{t}"))));
    out["despejo_s_real"] = json!(t0.elapsed().as_secs_f64());
    // A saída precisa ser consumida pelo app; um instante para o último pedaço.
    std::thread::sleep(Duration::from_secs(2));
    out["fim"] = memoria_arvore(pid);
    std::thread::sleep(Duration::from_secs(30));
    out["depois_30s"] = memoria_arvore(pid);
    parar.store(true, std::sync::atomic::Ordering::Release);
    out["serie"] = Value::Array(serie.join().unwrap_or_default());
    app.encerrar();
    out["latencia"] = lat?;
    out["carga_ativa_durante_latencia"] = json!(ativo_no_fim);
    if !terminou {
        return Err("os geradores não terminaram no prazo".into());
    }
    Ok(out)
}

fn build(root: &Path) -> Result<PathBuf, String> {
    let base = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or(root.join("target"));
    let dir = base.join("medir");
    println!("build: orquestrador (dev com opt-level 3, dependências sem debug_assertions) em {}", dir.display());
    let s = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .args(["build", "-p", "orquestrador-app", "--bin", "orquestrador"])
        // Dependências sem `debug_assertions`, como no release: o GPUI compilaria os shaders HLSL em tempo de execução
        // (~200 ms na abertura) e ligaria a camada de depuração do DXGI. O app continua com elas.
        .args(["--config", "profile.dev.package.\"*\".debug-assertions=false"])
        .env("CARGO_TARGET_DIR", &dir)
        .env("CARGO_PROFILE_DEV_OPT_LEVEL", "3")
        .current_dir(root)
        .status()
        .map_err(|e| e.to_string())?;
    if !s.success() {
        return Err(format!("build falhou ({s})"));
    }
    Ok(dir.join("debug/orquestrador.exe"))
}

fn mediana(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

#[cfg(windows)]
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let opc = opcoes(args)?;
    let exe = match &opc.exe {
        Some(e) => e.clone(),
        None => build(root)?,
    };
    let out = opc.out.clone().unwrap_or_else(|| std::env::temp_dir().join(format!("orq-medir-gpui-{}", agora_us() / 1_000_000)));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let repo = out.join("repo");
    if !repo.join(".git").exists() {
        repositorio(&repo)?;
    }
    let hist = historico();
    let hist_antes = hist.as_ref().and_then(|h| std::fs::read(h).ok());
    println!("pasta: {}", out.display());

    // Abertura: perfil novo a cada execução, sem roteiro; RAM do app ocioso 5 s depois do primeiro quadro.
    let mut aberturas = Vec::new();
    let mut ram_vazia = Vec::new();
    for k in 0..opc.aberturas {
        let mut app = App::subir(&exe, &repo, &out.join(format!("abertura-{k}")), None)?;
        let ms = app.abertura();
        std::thread::sleep(Duration::from_secs(5));
        let m = memoria_arvore(app.filho.id());
        app.encerrar();
        let ms = ms?;
        println!("abertura {k}: {ms:.0} ms; app ocioso sem shells {:.0} MB privados", mb(&m, "app", "privada_mb"));
        aberturas.push(ms);
        ram_vazia.push(m);
    }
    if opc.so_abertura {
        println!("abertura (ms): {aberturas:.0?}; mediana {:.0}", mediana(&mut aberturas.clone()));
        return Ok(());
    }
    println!("cenário sem carga (5 shells, {} amostras)...", opc.amostras);
    let sem = cenario(&exe, &repo, &out.join("sem-carga"), &opc, false)?;
    println!("cenário com carga (4 despejando {} linhas/s por {} s)...", opc.taxa, opc.despejo_s);
    let com = cenario(&exe, &repo, &out.join("com-carga"), &opc, true)?;

    let hist_ok = match (&hist, &hist_antes) {
        (Some(h), Some(a)) => std::fs::read(h).map(|d| d == *a).unwrap_or(false),
        _ => true,
    };
    let abertura_med = mediana(&mut aberturas.clone());
    let total = |v: &Value| mb(v, "app", "privada_mb");
    let ocioso = total(&com["ocioso"]).max(total(&sem["ocioso"]));
    let pico = com["serie"].as_array().map(|s| s.iter().filter_map(|x| x["app_privada_mb"].as_f64()).fold(0., f64::max)).unwrap_or(0.);
    let carga = total(&com["durante"]).max(total(&com["fim"])).max(pico);
    let cresceu = total(&com["depois_30s"]) - total(&com["fim"]);
    let p95 = com["latencia"]["total"]["p95"].as_f64().unwrap_or(f64::NAN);
    let metas = json!({
        "abertura_mediana_ms": {"valor": abertura_med, "meta": "< 400", "ok": abertura_med < 400.},
        "ram_ociosa_mb": {"valor": ocioso, "meta": "< 150", "ok": ocioso < 150.},
        "ram_com_carga_mb": {"valor": carga, "meta": "< 500", "ok": carga < 500.},
        "crescimento_30s_mb": {"valor": cresceu, "meta": "<= 5 (não cresce)", "ok": cresceu <= 5.},
        "digitacao_p95_carga_ms": {"valor": p95, "meta": "< 16", "ok": p95 < 16.},
    });
    let resultado = json!({
        "exe": exe, "aberturas_ms": aberturas, "app_sem_shells": ram_vazia, "sem_carga": sem, "com_carga": com,
        "historico_psreadline_intacto": hist_ok, "metas": metas,
    });
    let arq = out.join("resultado.json");
    std::fs::write(&arq, serde_json::to_string_pretty(&resultado).unwrap()).map_err(|e| e.to_string())?;
    println!("\nabertura (ms): {aberturas:.0?}; mediana {abertura_med:.0}");
    for (nome, v) in [("ocioso (sem carga)", &sem["ocioso"]), ("ocioso", &com["ocioso"]), ("durante (15 s)", &com["durante"]), ("fim do despejo", &com["fim"]), ("30 s depois", &com["depois_30s"])] {
        println!(
            "RAM {nome}: app {:.0} / {:.0} MB (privada / working set); pwsh {:.0}, conhost {:.0}, gerador {:.0} MB privados",
            mb(v, "app", "privada_mb"), mb(v, "app", "ws_mb"), mb(v, "pwsh", "privada_mb"), mb(v, "conhost", "privada_mb"), mb(v, "gerador", "privada_mb")
        );
    }
    println!("pico da série: {pico:.0} MB");
    for (nome, v) in [("sem carga", &sem["latencia"]), ("com carga", &com["latencia"])] {
        println!("digitação {nome}: n={} total {} | pty {} | janela {}", v["n"], v["total"], v["pty"], v["janela"]);
    }
    println!("histórico do PSReadLine intacto: {hist_ok}");
    for (k, m) in metas.as_object().unwrap() {
        println!("meta {k}: {:.1} ({}) {}", m["valor"].as_f64().unwrap_or(f64::NAN), m["meta"].as_str().unwrap(), if m["ok"] == true { "ok" } else { "FALHOU" });
    }
    println!("resultado: {}", arq.display());
    Ok(())
}

#[cfg(not(windows))]
pub fn run(_: &Path, _: &[String]) -> Result<(), String> {
    Err("medir só roda no Windows.".into())
}
