// Medição de desempenho do Orquestrador (Tauri 2 + WebView2, Windows): abertura, RAM por processo, latência de digitação no terminal,
// quadros, heap e perfis do renderer, num cenário fixo. Não altera o app: sobe o exe de debug com o frontend de produção embutido.
//
// Uso:   node scripts/medir-desempenho.mjs [--build] [--out <pasta>] [--abertura-novo 5] [--abertura-existente 5] [--amostras 250]
//            [--taxa 6000] [--segundos-a 70] [--segundos-b 40] [--segundos-c 20] [--limite-mb 5500] [--prazo-eco 20000]
//            [--sem-abertura] [--sem-cenario] [--sem-heap]
//        node scripts/medir-desempenho.mjs --analisar <arquivo.heapsnapshot> [saida.json] [--inspecionar <texto|prop:nome>]   (só analisa um heap snapshot)
// Saída: <out>/resultado.json (padrão %TEMP%\orq-medir-<data>\), renderer.heapsnapshot e heap-analise.json na mesma pasta; resumo no stdout.
//        Roda do zero, sem intervenção (~6 min). Sem argumentos usa os valores acima como padrão: é o cenário de base para comparar depois.
//
// Como mede (detalhes e limites em docs/desempenho-base.md):
// - Executável: src-tauri/target/debug/orquestrador.exe, compilado por `npx tauri build --debug --no-bundle` com CARGO_PROFILE_DEV_OPT_LEVEL=3
//   (Rust otimizado como o release, mas com debug_assertions: sem o plugin single-instance e sem a verificação de atualização, que atuariam
//   sobre a instância instalada). Só compila se o exe não existir ou com `--build`. O frontend é o `dist` de produção (React em modo produção).
// - Isolamento: WEBVIEW2_USER_DATA_FOLDER e LOCALAPPDATA do filho apontam para pastas temporárias (perfil do WebView2 e config.json do app); a
//   instância instalada não é lida nem escrita. O PSReadLine grava o histórico no APPDATA real (pela API do Windows): os terminais desligam a
//   gravação e a predição, e o fim do script remove do arquivo as linhas digitadas aqui. Só se encerra a árvore iniciada aqui (taskkill /PID <raiz> /T).
// - Conexão: CDP pelo --remote-debugging-port do WebView2, com o playwright-core do cache local do npx (nada é instalado).
// - Cenário: repositório git descartável em %TEMP% (40 commits, src/app.js de 3000 linhas aberto no editor, árvore suja para a tela Git) e
//   4 terminais pwsh. Cada um roda `node flood.mjs` (linhas coloridas de 120 colunas, conteúdo determinístico) a --taxa linhas/s:
//   fase A (3 terminais, --segundos-a) mede a latência de digitação no 4º; fase B (4 terminais, --segundos-b) mede RAM e quadros;
//   depois 30 s de espera, GC, heap snapshot e a fase C (--segundos-c) com os perfis de CPU e de alocações do V8.
//   A trava --limite-mb encerra tudo se a RAM privada do app passar do limite (a máquina é compartilhada).
// - Métrica do terminal com dois métodos (resultado.cenario.latencia*.metodo): 'dom' se o terminal visível tem `.xterm-rows` (renderizador DOM:
//   MutationObserver e textContent, como na base); 'buffer' se está em WebGL (sem linhas no DOM): o script injeta, antes dos scripts da página
//   (Page.addScriptToEvaluateOnNewDocument + reload; --sem-gancho desliga), `window.__orqMedir`, que src/terminal/term.ts chama ao criar cada xterm.
//   Eco "renderizado" = onRender do xterm cobre a linha do prompt e o buffer já tem a letra; saída "consumida" = a última linha do gerador está
//   nas últimas 60 linhas do buffer do terminal. Sem o gancho no app (build antigo), cai no método 'dom'.
import { spawn, spawnSync, execFile } from 'node:child_process';
import { createServer } from 'node:net';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync, createWriteStream } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const EXE = join(ROOT, 'src-tauri', 'target', 'debug', 'orquestrador.exe');

// ---------------------------------------------------------------- argumentos
const argv = process.argv.slice(2);
const flag = (name) => argv.includes(`--${name}`);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i >= 0 && argv[i + 1] !== undefined ? argv[i + 1] : fallback; };
const nopt = (name, fallback) => Number(opt(name, fallback));

const ANALISE = flag('analisar');
const ARGS = {
  build: flag('build'),
  aberturaNovo: nopt('abertura-novo', 5),
  aberturaExistente: nopt('abertura-existente', 5),
  amostras: nopt('amostras', 250),
  taxa: nopt('taxa', 6000),
  segundosA: nopt('segundos-a', 70),
  segundosB: nopt('segundos-b', 40),
  segundosC: nopt('segundos-c', 20),
  limiteMB: nopt('limite-mb', 5500),
  timeoutEcoMs: nopt('prazo-eco', 20000),
  semAbertura: flag('sem-abertura'),
  semCenario: flag('sem-cenario'),
  semHeap: flag('sem-heap'),
  semGancho: flag('sem-gancho'),
};
const STAMP = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
const OUT = opt('out', join(tmpdir(), `orq-medir-${STAMP}`));
const WORK = join(OUT, 'w');
if (!ANALISE) mkdirSync(WORK, { recursive: true });

// ---------------------------------------------------------------- utilidades
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.error(`[${((Date.now() - T_START) / 1000).toFixed(1)}s]`, ...a);
const T_START = Date.now();
const MB = 1024 * 1024;
const mb = (bytes) => (bytes == null ? null : Math.round((bytes / MB) * 10) / 10);

function percentil(sorted, p) {
  if (!sorted.length) return null;
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))];
}
function resumo(values, digits = 2) {
  const v = [...values].sort((a, b) => a - b);
  const r = (x) => (x == null ? null : Math.round(x * 10 ** digits) / 10 ** digits);
  return {
    n: v.length, min: r(v[0]), p50: r(percentil(v, 50)), p95: r(percentil(v, 95)), p99: r(percentil(v, 99)), max: r(v[v.length - 1]),
    media: v.length ? r(v.reduce((a, b) => a + b, 0) / v.length) : null,
  };
}
async function until(fn, { timeout = 30000, every = 50, what = 'condição' } = {}) {
  const limit = Date.now() + timeout;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > limit) throw new Error(`tempo esgotado esperando: ${what}`);
    await sleep(every);
  }
}
const sha = (file) => (existsSync(file) ? createHash('sha256').update(readFileSync(file)).digest('hex').slice(0, 16) : null);
function freePort() {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.once('error', reject);
    s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)); });
  });
}

function findPlaywright() {
  if (process.env.PLAYWRIGHT_CORE) return process.env.PLAYWRIGHT_CORE;
  const cache = join(process.env.LOCALAPPDATA ?? '', 'npm-cache', '_npx');
  if (existsSync(cache)) {
    for (const dir of readdirSync(cache)) {
      const candidate = join(cache, dir, 'node_modules', 'playwright-core', 'index.mjs');
      if (existsSync(candidate)) return candidate;
    }
  }
  throw new Error('playwright-core não encontrado. Defina PLAYWRIGHT_CORE com o caminho do index.mjs.');
}

// ---------------------------------------------------------------- amostrador de processos (PowerShell)
// Descendentes da raiz + qualquer msedgewebview2 cujo comando cite a pasta de dados desta execução. Com -Full: working set privado e GPU.
const PS_SAMPLER = String.raw`param([int]$RootPid, [string]$Udf, [switch]$Full)
$ErrorActionPreference = 'Stop'
$all = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine)
$set = @{ $RootPid = $true }
do {
  $grew = $false
  foreach ($p in $all) { if (-not $set.ContainsKey([int]$p.ProcessId) -and $set.ContainsKey([int]$p.ParentProcessId)) { $set[[int]$p.ProcessId] = $true; $grew = $true } }
} while ($grew)
if ($Udf) { foreach ($p in $all) { if ($p.CommandLine -and $p.CommandLine.Contains($Udf)) { $set[[int]$p.ProcessId] = $true } } }
$set.Remove([int]$PID)
$mine = @($all | Where-Object { $set.ContainsKey([int]$_.ProcessId) })
$ids = @($mine | ForEach-Object { [int]$_.ProcessId })
$gp = @{}
if ($ids.Count -gt 0) { Get-Process -Id $ids -ErrorAction SilentlyContinue | ForEach-Object { $gp[$_.Id] = $_ } }
$wsp = @{}
$gpu = @{}
if ($Full -and $ids.Count -gt 0) {
  $flt = ($ids | ForEach-Object { "IDProcess=$_" }) -join ' OR '
  Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter $flt | ForEach-Object { $wsp[[int]$_.IDProcess] = [int64]$_.WorkingSetPrivate }
  $c = Get-Counter '\GPU Process Memory(*)\Dedicated Usage','\GPU Process Memory(*)\Shared Usage' -ErrorAction SilentlyContinue
  foreach ($s in $c.CounterSamples) {
    if ($s.InstanceName -match '^pid_(\d+)_') {
      $id = [int]$Matches[1]
      if ($set.ContainsKey($id)) {
        $k = if ($s.Path -like '*dedicated*') { 'dedicated' } else { 'shared' }
        if (-not $gpu.ContainsKey($id)) { $gpu[$id] = @{} }
        $gpu[$id][$k] = [int64]$s.CookedValue
      }
    }
  }
}
$rows = foreach ($p in $mine) {
  $id = [int]$p.ProcessId; $g = $gp[$id]
  if (-not $g) { continue }
  [pscustomobject]@{ pid = $id; ppid = [int]$p.ParentProcessId; name = $p.Name; cmd = $p.CommandLine; privateBytes = [int64]$g.PrivateMemorySize64; workingSet = [int64]$g.WorkingSet64
    privateWorkingSet = $(if ($wsp.ContainsKey($id)) { $wsp[$id] } else { $null }); gpu = $(if ($gpu.ContainsKey($id)) { $gpu[$id] } else { $null }); cpuSeconds = [math]::Round($g.TotalProcessorTime.TotalSeconds, 2) }
}
@{ t = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds(); rows = @($rows) } | ConvertTo-Json -Depth 5 -Compress
`;
const SAMPLER_FILE = join(WORK, 'amostrador.ps1');
if (!ANALISE) writeFileSync(SAMPLER_FILE, PS_SAMPLER);

function psSample(rootPid, udf, full = false) {
  return new Promise((resolve, reject) => {
    const args = ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', SAMPLER_FILE, '-RootPid', String(rootPid), '-Udf', udf];
    if (full) args.push('-Full');
    execFile('powershell.exe', args, { maxBuffer: 64 * MB, windowsHide: true }, (error, stdout, stderr) => {
      if (error) return reject(new Error(`amostrador falhou: ${error.message}\n${stderr}`));
      try { const j = JSON.parse(stdout); j.rows = (j.rows ?? []).filter(Boolean); resolve(j); } catch (e) { reject(new Error(`saída do amostrador ilegível: ${stdout.slice(0, 300)}`)); }
    });
  });
}

// Classifica cada processo: grupos do app (orquestrador, webview2 por tipo) contam no total; shells e filhos ficam à parte.
function classificar(row) {
  const name = row.name.toLowerCase();
  if (name === 'orquestrador.exe') return { side: 'app', group: 'orquestrador.exe' };
  if (name === 'msedgewebview2.exe') {
    const type = /--type=([\w-]+)/.exec(row.cmd ?? '')?.[1];
    if (!type) return { side: 'app', group: 'webview2-browser' };
    if (type === 'utility') {
      const sub = /--utility-sub-type=([\w.]+)/.exec(row.cmd ?? '')?.[1]?.split('.').pop()?.replace(/service$/i, '').toLowerCase();
      // O serviço de tracing existe só por causa dos dumps de memória deste script: não conta no total do app.
      if (sub === 'tracing') return { side: 'medicao', group: 'webview2-utility-tracing', detail: sub };
      return { side: 'app', group: 'webview2-utility', detail: sub };
    }
    return { side: 'app', group: type === 'gpu-process' ? 'webview2-gpu' : type === 'crashpad-handler' ? 'webview2-crashpad' : `webview2-${type}` };
  }
  return { side: 'shells', group: name };
}

function agregarMomento(sample) {
  const rows = sample.rows.map((r) => ({ ...r, ...classificar(r), cmd: undefined }));
  const sum = (list, key) => list.reduce((a, r) => a + (r[key] ?? 0), 0);
  const grupos = {};
  for (const r of rows.filter((x) => x.side === 'app')) {
    const g = (grupos[r.group] ??= { processos: 0, privateMB: 0, workingSetMB: 0, privateWorkingSetMB: 0, gpuDedicatedMB: 0, gpuSharedMB: 0, cpuSeconds: 0 });
    g.processos++;
    g.privateMB += r.privateBytes / MB;
    g.workingSetMB += r.workingSet / MB;
    g.privateWorkingSetMB += (r.privateWorkingSet ?? 0) / MB;
    g.gpuDedicatedMB += (r.gpu?.dedicated ?? 0) / MB;
    g.gpuSharedMB += (r.gpu?.shared ?? 0) / MB;
    g.cpuSeconds += r.cpuSeconds;
  }
  for (const g of Object.values(grupos)) for (const k of Object.keys(g)) if (k !== 'processos') g[k] = Math.round(g[k] * 10) / 10;
  const app = rows.filter((x) => x.side === 'app');
  const sh = rows.filter((x) => x.side === 'shells');
  const shells = {};
  for (const r of sh) { const g = (shells[r.group] ??= { processos: 0, privateMB: 0, workingSetMB: 0 }); g.processos++; g.privateMB += r.privateBytes / MB; g.workingSetMB += r.workingSet / MB; }
  for (const g of Object.values(shells)) { g.privateMB = Math.round(g.privateMB * 10) / 10; g.workingSetMB = Math.round(g.workingSetMB * 10) / 10; }
  return {
    t: sample.t,
    totalApp: {
      privateMB: mb(sum(app, 'privateBytes')), workingSetMB: mb(sum(app, 'workingSet')),
      privateWorkingSetMB: app.some((r) => r.privateWorkingSet != null) ? mb(sum(app, 'privateWorkingSet')) : null,
    },
    grupos,
    processos: app.map((r) => ({ pid: r.pid, nome: r.name, grupo: r.group, detalhe: r.detail, privateMB: mb(r.privateBytes), workingSetMB: mb(r.workingSet), privateWorkingSetMB: mb(r.privateWorkingSet), gpu: r.gpu ?? null, cpuSeconds: r.cpuSeconds })),
    instrumentoDeMedicao: rows.filter((x) => x.side === 'medicao').map((r) => ({ nome: r.group, privateMB: mb(r.privateBytes) })),
    shells,
    totalShells: { privateMB: mb(sum(sh, 'privateBytes')), workingSetMB: mb(sum(sh, 'workingSet')) },
  };
}

// ---------------------------------------------------------------- agente dentro da página (serializado pelo Playwright)
function instalarAgente() {
  if (window.__medir) return;
  const A = (window.__medir = { state: null, obs: null, lastMut: 0, fps: null });
  const norm = (s) => s.replace(/\u00a0/g, ' ').replace(/\s+$/, '');
  A.visibleRows = () => [...document.querySelectorAll('.xterm-rows')].find((el) => getComputedStyle(el).visibility !== 'hidden') ?? null;
  // Dois métodos. 'dom': o terminal visível tem `.xterm-rows` (renderizador DOM); mutações do DOM e textContent. 'buffer': o visível está em WebGL
  // (sem linhas no DOM); lê o buffer do xterm e o evento onRender pelo gancho `window.__orqMedir` (GANCHO_JS, injetado pelo script).
  A.modo = 'dom';
  A.rec = null;
  A.recs = () => { const l = window.__orqMedirTerms ?? []; return [...document.querySelectorAll('.xterm')].map((el) => l.find((r) => r.term.element === el)).filter(Boolean); };
  A.recVisivel = () => A.recs().find((r) => r.term.element.offsetParent && getComputedStyle(r.term.element).visibility !== 'hidden') ?? null;
  A.bufLinha = (rec, y) => norm(rec.term.buffer.active.getLine(y)?.translateToString(true) ?? '');
  // Última linha não vazia da tela do terminal: { y: linha dentro da tela, texto }.
  A.bufUltima = (rec) => {
    const base = rec.term.buffer.active.baseY;
    for (let y = rec.term.rows - 1; y >= 0; y--) { const t = A.bufLinha(rec, base + y); if (t) return { y, texto: t }; }
    return { y: -1, texto: '' };
  };
  A.lastLine = () => {
    if (A.modo === 'buffer') { const rec = A.rec ?? A.recVisivel(); return rec ? A.bufUltima(rec).texto : ''; }
    const r = A.visibleRows();
    if (!r) return '';
    for (let i = r.children.length - 1; i >= 0; i--) { const t = norm(r.children[i].textContent ?? ''); if (t) return t; }
    return '';
  };
  // A última linha do gerador já está no terminal `i` (posição no DOM)? 'buffer': nas últimas 60 linhas do buffer (processado); 'dom': no DOM.
  A.consumido = (needles) => needles.map(([i, n]) => {
    if (A.modo === 'buffer') {
      const rec = A.recs()[i];
      if (!rec) return false;
      const b = rec.term.buffer.active;
      let txt = '';
      for (let y = Math.max(0, b.baseY + rec.term.rows - 60); y < b.baseY + rec.term.rows; y++) txt += b.getLine(y)?.translateToString(true) ?? '';
      return txt.replace(/\s+/g, '').includes(n);
    }
    return ([...document.querySelectorAll('.xterm-rows')][i]?.textContent ?? '').replace(/\s+/g, '').includes(n);
  });
  // Gancho do modo 'buffer': escrita processada = atividade; render que cobre a linha do prompt com a letra esperada = "renderizado".
  A.onWrite = (rec) => { if (rec === A.rec) A.lastMut = performance.now(); };
  A.onRender = (rec, e) => {
    if (rec !== A.rec) return;
    const s = A.state;
    if (!s || s.t0 === null || s.tMut !== null) return;
    const u = A.bufUltima(rec);
    if (u.y < e.start || u.y > e.end || !u.texto.replace(/\s+/g, '').startsWith(s.expect)) return;
    s.tMut = performance.now();
    requestAnimationFrame(() => { s.tRaf = performance.now(); s.done = true; });
  };
  A.onMut = () => {
    A.lastMut = performance.now();
    const s = A.state;
    if (s && s.t0 !== null && s.tMut === null && A.lastLine().replace(/\s+/g, '').startsWith(s.expect)) {
      s.tMut = performance.now();
      requestAnimationFrame(() => { s.tRaf = performance.now(); s.done = true; });
    }
  };
  A.watch = () => {
    A.obs?.disconnect();
    A.obs = null;
    A.rec = null;
    const r = A.visibleRows();
    if (!r) {
      const rec = A.recVisivel();
      if (!rec) return false;
      A.modo = 'buffer';
      A.rec = rec;
      A.lastMut = performance.now();
      return true;
    }
    A.modo = 'dom';
    A.obs = new MutationObserver(A.onMut);
    A.obs.observe(r, { subtree: true, childList: true, characterData: true, attributes: true });
    A.lastMut = performance.now();
    return true;
  };
  A.arm = (expect) => { A.state = { expect, t0: null, tMut: null, tRaf: null, done: false }; };
  A.take = () => { const s = A.state; A.state = null; return s && s.done ? { eco: s.tMut - s.t0, quadro: s.tRaf - s.t0 } : null; };
  A.quiet = (ms, cap) => new Promise((resolve) => {
    const start = performance.now();
    const tick = () => {
      const now = performance.now();
      if (now - A.lastMut >= ms || now - start >= cap) resolve(now - start);
      else setTimeout(tick, 10);
    };
    tick();
  });
  window.addEventListener('keydown', (e) => { const s = A.state; if (s && s.t0 === null) s.t0 = e.timeStamp; }, true);
  A.fpsStart = () => {
    const f = { last: performance.now(), dts: [], run: true, long: [], po: null };
    A.fps = f;
    try { f.po = new PerformanceObserver((l) => { for (const e of l.getEntries()) f.long.push(e.duration); }); f.po.observe({ type: 'longtask' }); } catch { /* sem longtask */ }
    const tick = (t) => { if (!f.run) return; f.dts.push(t - f.last); f.last = t; requestAnimationFrame(tick); };
    requestAnimationFrame(tick);
  };
  A.fpsStop = () => { const f = A.fps; f.run = false; f.po?.disconnect(); A.fps = null; return { dts: f.dts, long: f.long }; };
  A.mem = () => {
    const m = performance.memory;
    return {
      performanceMemory: m ? { usedJSHeapSize: m.usedJSHeapSize, totalJSHeapSize: m.totalJSHeapSize, jsHeapSizeLimit: m.jsHeapSizeLimit } : null,
      domNodes: document.getElementsByTagName('*').length,
      xtermHosts: document.querySelectorAll('.xterm').length,
      xtermRowDivs: document.querySelectorAll('.xterm-rows > div').length,
      cmEditors: document.querySelectorAll('.cm-editor').length,
    };
  };
}

// Gancho injetado antes dos scripts da página (Page.addScriptToEvaluateOnNewDocument + reload). O app (src/terminal/term.ts) chama
// `window.__orqMedir(id, term)` ao criar cada xterm, se existir; sem esta injeção o app não faz nada extra.
const GANCHO_JS = `window.__orqMedirTerms = [];
window.__orqMedir = (id, term) => {
  const rec = { id, term };
  window.__orqMedirTerms.push(rec);
  term.onRender((e) => window.__medir?.onRender(rec, e));
  term.onWriteParsed(() => window.__medir?.onWrite(rec));
};`;

// Espera o marco "interface principal interativa": navegação lateral, aba do projeto selecionada, árvore de arquivos e botão do painel Terminal.
function esperarMarco() {
  return new Promise((resolve) => {
    const ok = () => !!(
      document.querySelector('nav[aria-label="Telas"]') && document.querySelector('header [role="tab"][aria-selected="true"]')
      && document.querySelector('[role="treeitem"]') && [...document.querySelectorAll('button')].some((b) => (b.title ?? '').startsWith('Painel Terminal'))
    );
    const nav = () => performance.getEntriesByType('navigation')[0];
    const paint = () => performance.getEntriesByName('first-contentful-paint')[0]?.startTime ?? null;
    const fim = (jaPronto) => resolve({ t: Date.now(), jaPronto, timeOrigin: performance.timeOrigin, fcp: paint(), dcl: nav()?.domContentLoadedEventEnd ?? null, load: nav()?.loadEventEnd ?? null });
    if (ok()) return fim(true);
    new MutationObserver((_, o) => { if (ok()) { o.disconnect(); fim(false); } }).observe(document, { subtree: true, childList: true, attributes: true });
  });
}

// ---------------------------------------------------------------- app: subir, conectar, encerrar
const FLOOD_JS = String.raw`// Gerador de saída intensa e determinístico: linhas de 120 colunas com cor ANSI. Uso: node flood.mjs <linhas> <tag> [linhas por segundo; 0 = o máximo].
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
const lines = Number(process.argv[2]);
const tag = process.argv[3];
const rate = Number(process.argv[4] ?? 0);
const dir = process.env.ORQ_DONE_DIR;
const ESC = String.fromCharCode(27);
const alphabet = 'abcdefghijklmnopqrstuvwxyz0123456789 _-./:=+';
const colors = [31, 32, 33, 34, 35, 36, 91, 92, 93, 94, 95, 96];
let seed = 12345 + tag.charCodeAt(0);
const rnd = () => (seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296;
function line(i) {
  let body = '[' + tag + '] ' + String(i).padStart(7, '0') + ' ';
  while (body.length < 118) body += alphabet[(rnd() * alphabet.length) | 0];
  return ESC + '[' + colors[i % colors.length] + 'm' + body.slice(0, 118) + ESC + '[0m' + ESC + '[1;3' + ((i % 7) + 1) + 'mOK' + ESC + '[0m\r\n';
}
writeFileSync(join(dir, 'inicio-' + tag), String(Date.now()));
const t0 = Date.now();
for (let i = 0; i < lines; i += 100) {
  if (rate > 0) { const wait = t0 + (i / rate) * 1000 - Date.now(); if (wait > 0) await new Promise((r) => setTimeout(r, wait)); }
  let chunk = '';
  for (let j = i; j < Math.min(lines, i + 100); j++) chunk += line(j);
  await new Promise((r) => process.stdout.write(chunk, r));
}
writeFileSync(join(dir, 'fim-' + tag), String(Date.now()));
`;

function criarProjeto(dir) {
  mkdirSync(join(dir, 'src'), { recursive: true });
  const git = (...a) => { const r = spawnSync('git', ['-c', 'user.name=medir', '-c', 'user.email=medir@example.invalid', '-c', 'commit.gpgsign=false', ...a], { cwd: dir, encoding: 'utf8' }); if (r.status !== 0) throw new Error(`git ${a.join(' ')}: ${r.stderr}`); };
  git('init', '-q', '-b', 'main');
  writeFileSync(join(dir, 'README.md'), '# Projeto de medição\n\nRepositório descartável gerado por scripts/medir-desempenho.mjs.\n');
  const arquivo = [];
  for (let i = 0; i < 3000; i++) {
    arquivo.push(i % 40 === 0 ? `// ---- bloco ${i / 40} ----` : `export function f${i}(a, b) { const total = a * ${i} + b; return total > ${i * 3} ? 'alto ${i}' : \`baixo \${total}\`; }`);
  }
  writeFileSync(join(dir, 'src', 'app.js'), arquivo.join('\n') + '\n');
  git('add', '.');
  git('commit', '-q', '-m', 'inicio');
  for (let c = 1; c <= 40; c++) {
    writeFileSync(join(dir, 'src', `mod${c % 8}.js`), `export const v${c} = ${c};\n`.repeat(c % 5 + 1));
    git('add', '.');
    git('commit', '-q', '-m', `commit ${c}: ajuste do módulo ${c % 8}`);
  }
  // Árvore suja para a tela Git: um arquivo modificado e um novo.
  writeFileSync(join(dir, 'src', 'app.js'), arquivo.map((l, i) => (i % 97 === 0 ? l + ' // editado' : l)).join('\n') + '\n');
  writeFileSync(join(dir, 'novo.txt'), 'arquivo novo\n');
}

let playwright;
async function conectar(port, limite = 60000) {
  playwright ??= await import(pathToFileURL(findPlaywright()).href);
  await until(() => fetch(`http://127.0.0.1:${port}/json/version`).then((r) => r.ok, () => false), { timeout: limite, every: 5, what: 'porta CDP do WebView2' });
  const browser = await playwright.chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const page = await until(() => browser.contexts().flatMap((c) => c.pages()).find((p) => p.url().includes('tauri.localhost')), { timeout: limite, every: 5, what: 'página do app' });
  return { browser, page };
}

/** Sobe o exe com o ambiente isolado. `state` = { udf, local, roam } (pastas temporárias). */
async function subir(state, projectDir) {
  for (const d of Object.values(state)) mkdirSync(d, { recursive: true });
  const port = await freePort();
  const doneDir = join(WORK, 'flood');
  mkdirSync(doneDir, { recursive: true });
  const env = {
    ...process.env,
    WEBVIEW2_USER_DATA_FOLDER: state.udf,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    LOCALAPPDATA: state.local,
    APPDATA: state.roam,
    ORQ_FLOOD: join(WORK, 'flood.mjs'),
    ORQ_DONE_DIR: doneDir,
  };
  const t0 = Date.now();
  const child = spawn(EXE, [projectDir], { cwd: WORK, stdio: 'ignore', windowsHide: true, env });
  child.on('error', (e) => log('erro ao iniciar o exe:', e.message));
  return { child, pid: child.pid, port, t0, udf: state.udf, doneDir };
}

async function encerrar(app) {
  if (!app?.pid) return [];
  // Só a árvore iniciada aqui; depois, o que restar com a pasta de dados desta execução (processos do WebView2 que se desligaram da árvore).
  spawnSync('taskkill', ['/PID', String(app.pid), '/T', '/F'], { windowsHide: true });
  await sleep(1500);
  let resto = [];
  for (let i = 0; i < 5; i++) {
    const s = await psSample(app.pid, app.udf).catch(() => ({ rows: [] }));
    resto = s.rows;
    if (!resto.length) return [];
    for (const r of resto) spawnSync('taskkill', ['/PID', String(r.pid), '/T', '/F'], { windowsHide: true });
    await sleep(1000);
  }
  const s = await psSample(app.pid, app.udf).catch(() => ({ rows: [] }));
  return s.rows.map((r) => ({ pid: r.pid, nome: r.name }));
}

// Dump de memória do Chromium (memory-infra) de todos os processos do WebView2, pela sessão CDP do navegador: separa heap do V8, blink (DOM, layout),
// malloc/partition_alloc, skia/gpu e o resto. Devolve, por pid, o footprint privado e os alocadores até o 2º nível (3º para v8 e blink).
async function dumpMemoriaChromium(browser) {
  const bs = await browser.newBrowserCDPSession();
  const events = [];
  bs.on('Tracing.dataCollected', (e) => events.push(...e.value));
  const done = new Promise((r) => bs.once('Tracing.tracingComplete', r));
  try {
    await bs.send('Tracing.start', { traceConfig: { includedCategories: ['-*', 'disabled-by-default-memory-infra'], memoryDumpConfig: { triggers: [] } }, transferMode: 'ReportEvents' });
    const r = await bs.send('Tracing.requestMemoryDump', { deterministic: true, levelOfDetail: 'light' });
    await bs.send('Tracing.end');
    await done;
    if (!r.success) return { erro: 'requestMemoryDump sem sucesso' };
  } finally {
    await bs.detach().catch(() => {});
  }
  const hex = (v) => (v == null ? null : parseInt(v, 16));
  const porPid = {};
  for (const e of events.filter((x) => x.ph === 'v')) {
    const p = (porPid[e.pid] ??= { totals: {}, alloc: {} });
    const d = e.args?.dumps ?? {};
    if (d.process_totals) Object.assign(p.totals, d.process_totals);
    for (const [nome, v] of Object.entries(d.allocators ?? {})) {
      const size = hex(v.attrs?.size?.value ?? v.attrs?.effective_size?.value);
      if (size != null) p.alloc[nome] = Math.max(p.alloc[nome] ?? 0, size);
    }
  }
  const saida = {};
  for (const [pid, p] of Object.entries(porPid)) {
    const lista = Object.entries(p.alloc)
      .filter(([nome]) => { const n = nome.split('/').length; const raiz = nome.split('/')[0]; return n <= 2 || (n === 3 && ['v8', 'blink_objects', 'malloc', 'partition_alloc'].includes(raiz)); })
      .map(([nome, bytes]) => ({ nome, MB: Math.round((bytes / MB) * 10) / 10 }))
      .filter((x) => x.MB >= 0.5)
      .sort((a, b) => b.MB - a.MB);
    saida[pid] = { privateFootprintMB: mb(hex(p.totals.private_footprint_bytes)), residentMB: mb(hex(p.totals.resident_set_bytes)), alocadores: lista };
  }
  return saida;
}

// ---------------------------------------------------------------- abertura
async function medirAbertura(state, projectDir, rotulo) {
  const app = await subir(state, projectDir);
  let r;
  try {
    const { browser, page } = await conectar(app.port);
    const tConectado = Date.now();
    const marco = await page.evaluate(esperarMarco);
    r = {
      rotulo, marcoMs: marco.t - app.t0, jaProntoAoConectar: marco.jaPronto, cdpConectadoMs: tConectado - app.t0,
      navegacaoWebviewMs: Math.round(marco.timeOrigin - app.t0), fcpMs: marco.fcp == null ? null : Math.round(marco.timeOrigin + marco.fcp - app.t0),
      domContentLoadedMs: marco.dcl == null ? null : Math.round(marco.timeOrigin + marco.dcl - app.t0),
      loadMs: marco.load == null ? null : Math.round(marco.timeOrigin + marco.load - app.t0),
    };
    await browser.close().catch(() => {});
  } finally {
    const sobras = await encerrar(app);
    if (sobras?.length) log('ATENÇÃO: processos restantes após encerrar:', sobras);
  }
  await sleep(1000);
  return r;
}

const HISTORICO_OFF = 'Set-PSReadLineOption -HistorySaveStyle SaveNothing -PredictionSource None';
const HISTORICO_ARQ = join(process.env.APPDATA ?? '', 'Microsoft', 'Windows', 'PowerShell', 'PSReadLine', 'ConsoleHost_history.txt');
// Tira do histórico do usuário só o que este script digitou (as linhas do flood e a que desliga a gravação). Devolve quantas linhas removeu.
function limparHistorico() {
  if (!existsSync(HISTORICO_ARQ)) return 0;
  const bruto = readFileSync(HISTORICO_ARQ, 'utf8');
  const eol = bruto.includes('\r\n') ? '\r\n' : '\n';
  const linhas = bruto.split(eol);
  let off = 0;
  const mantidas = [];
  for (let i = linhas.length - 1; i >= 0; i--) {
    const l = linhas[i];
    if (/^node \$env:ORQ_FLOOD /.test(l) || (l === HISTORICO_OFF && off < 8 && ++off)) continue;
    mantidas.push(l);
  }
  const novo = mantidas.reverse().join(eol);
  if (novo === bruto) return 0;
  writeFileSync(HISTORICO_ARQ, novo);
  return linhas.length - mantidas.length;
}

// ---------------------------------------------------------------- teclado via CDP
const VK = { ' ': 32, '$': 52, ':': 186, '_': 189, '.': 190, '-': 189, '/': 191, '\\': 220 };
async function tecla(cdp, ch) {
  const special = { Enter: { key: 'Enter', code: 'Enter', vk: 13, text: '\r' }, Escape: { key: 'Escape', code: 'Escape', vk: 27 } }[ch];
  if (special) {
    await cdp.send('Input.dispatchKeyEvent', { type: special.text ? 'keyDown' : 'rawKeyDown', key: special.key, code: special.code, windowsVirtualKeyCode: special.vk, nativeVirtualKeyCode: special.vk, ...(special.text ? { text: special.text, unmodifiedText: special.text } : {}) });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: special.key, code: special.code, windowsVirtualKeyCode: special.vk, nativeVirtualKeyCode: special.vk });
    return;
  }
  const upper = ch.toUpperCase();
  const vk = /[a-z0-9]/i.test(ch) ? upper.charCodeAt(0) : VK[ch] ?? 0;
  const code = /[a-z]/i.test(ch) ? `Key${upper}` : /[0-9]/.test(ch) ? `Digit${ch}` : '';
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyDown', key: ch, code, text: ch, unmodifiedText: ch, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk });
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: ch, code, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk });
}
async function digitar(cdp, texto) { for (const ch of texto) { await tecla(cdp, ch); await sleep(4); } }

// PRNG fixo: os intervalos entre teclas são os mesmos em toda execução.
function prng(seed) { let s = seed >>> 0; return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296); }

async function medirLatencia(page, cdp, n, rotulo, aindaCarregando) {
  const rnd = prng(20260101);
  const eco = [], quadro = [], censuradas = [], cargaAoLongo = [];
  const letras = 'abcdefghijklmnopqrstuvwxyz';
  const tInicio = Date.now();
  await page.evaluate(() => { window.__medir.watch(); return window.__medir.quiet(300, 3000); });
  const metodo = await page.evaluate(() => window.__medir.modo);
  // A linha do prompt sem nada digitado (sem espaços): o ponto de partida de toda amostra.
  const prompt = (await page.evaluate(() => window.__medir.lastLine())).replace(/\s+/g, '');
  // Antes de cada amostra a linha precisa estar de volta ao prompt (um eco atrasado da amostra anterior ainda pode chegar): Esc a limpa.
  const limpar = async () => {
    const limite = Date.now() + 30000;
    for (;;) {
      const linha = (await page.evaluate(() => window.__medir.lastLine())).replace(/\s+/g, '');
      if (linha === prompt) return true;
      if (Date.now() > limite) return false;
      await tecla(cdp, 'Escape');
      await sleep(200);
    }
  };
  for (let i = 0; i < n; i++) {
    const limpo = await limpar();
    const ch = letras[i % letras.length];
    await page.evaluate((expect) => window.__medir.arm(expect), prompt + ch);
    await tecla(cdp, ch);
    const ok = await page.waitForFunction(() => window.__medir.state?.done === true, null, { polling: 'raf', timeout: ARGS.timeoutEcoMs }).then(() => true, () => false);
    const s = await page.evaluate(() => window.__medir.take());
    if (ok && s) { eco.push(s.eco); quadro.push(s.quadro); }
    else { censuradas.push({ i, limpo }); eco.push(ARGS.timeoutEcoMs); quadro.push(ARGS.timeoutEcoMs); } // sem eco no prazo: entra como o prazo (limite inferior)
    await tecla(cdp, 'Escape');
    await page.evaluate(() => window.__medir.quiet(60, 3000));
    await sleep(Math.round(20 + rnd() * 60));
    if (aindaCarregando && (i === 0 || i === Math.floor(n / 2) || i === n - 1)) cargaAoLongo.push({ amostra: i, tSeg: Math.round((Date.now() - tInicio) / 100) / 10, terminais: await aindaCarregando() });
  }
  const arr = (a) => a.map((x) => Math.round(x * 100) / 100);
  return {
    rotulo, metodo, amostras: eco.length, semEcoNoPrazo: censuradas.length, prazoMs: ARGS.timeoutEcoMs, duracaoS: Math.round((Date.now() - tInicio) / 100) / 10, cargaAoLongo,
    eco_dom_ms: resumo(eco), eco_quadro_ms: resumo(quadro), brutas: { eco: arr(eco), quadro: arr(quadro) },
  };
}

// ---------------------------------------------------------------- perfis do V8 (CDP) resumidos
const nomeFrame = (cf) => `${cf.functionName || '(anônima)'} ${(cf.url || '').split('/').pop().split('?')[0]}:${cf.lineNumber + 1}`;
function resumirPerfilCPU(p) {
  const porId = new Map(p.nodes.map((n) => [n.id, n]));
  const us = new Map();
  for (let i = 0; i < p.samples.length; i++) us.set(p.samples[i], (us.get(p.samples[i]) ?? 0) + (p.timeDeltas[i + 1] ?? p.timeDeltas[i] ?? 0));
  const agg = new Map();
  let total = 0;
  for (const [id, t] of us) { const k = nomeFrame(porId.get(id).callFrame); agg.set(k, (agg.get(k) ?? 0) + t); total += t; }
  const lista = [...agg.entries()].sort((a, b) => b[1] - a[1]).slice(0, 25).map(([funcao, t]) => ({ funcao, msProprios: Math.round(t / 1000), pct: Math.round((t / total) * 1000) / 10 }));
  return { duracaoMs: Math.round(total / 1000), amostras: p.samples.length, topPorTempoProprio: lista };
}
function resumirPerfilAlocacoes(p) {
  const agg = new Map();
  let total = 0;
  const pilha = [p.head];
  while (pilha.length) {
    const n = pilha.pop();
    if (n.selfSize) { const k = nomeFrame(n.callFrame); agg.set(k, (agg.get(k) ?? 0) + n.selfSize); total += n.selfSize; }
    for (const c of n.children ?? []) pilha.push(c);
  }
  const lista = [...agg.entries()].sort((a, b) => b[1] - a[1]).slice(0, 20).map(([funcao, bytes]) => ({ funcao, MB: Math.round((bytes / MB) * 10) / 10, pct: Math.round((bytes / total) * 1000) / 10 }));
  return { totalAlocadoMB: Math.round((total / MB) * 10) / 10, incluiColetados: true, topPorBytes: lista };
}

// ---------------------------------------------------------------- cenário principal
async function cenario(projectDir) {
  const state = { udf: join(WORK, 'cenario', 'udf'), local: join(WORK, 'cenario', 'local'), roam: join(WORK, 'cenario', 'roam') };
  const app = await subir(state, projectDir);
  const out = { momentos: {}, serieDurante: [], erros: [] };
  let browser, guarda;
  try {
    const conn = await conectar(app.port);
    browser = conn.browser;
    const { page } = conn;
    page.setDefaultTimeout(30000);
    page.on('pageerror', (e) => out.erros.push(`pageerror: ${e.message}`));
    const marco = await page.evaluate(esperarMarco);
    out.aberturaMs = marco.t - app.t0;
    out.abertura = { marcoMs: out.aberturaMs, jaProntoAoConectar: marco.jaPronto, fcpMs: marco.fcp == null ? null : Math.round(marco.timeOrigin + marco.fcp - app.t0) };
    log('interface interativa em', out.aberturaMs, 'ms');
    // Gancho do xterm (ver GANCHO_JS): injetado no próximo documento e a página recarregada (a abertura acima é de antes do reload).
    const cdp = await page.context().newCDPSession(page);
    if (!ARGS.semGancho) {
      await cdp.send('Page.enable');
      await cdp.send('Page.addScriptToEvaluateOnNewDocument', { source: GANCHO_JS });
      await page.reload();
      await page.evaluate(esperarMarco);
      log('página recarregada com o gancho');
    }
    await page.evaluate(instalarAgente);
    await cdp.send('Performance.enable');
    await cdp.send('HeapProfiler.enable');
    await cdp.send('Profiler.enable');
    // Trava de segurança: a saída intensa pode inflar o renderer; acima do limite, encerra a árvore e sai (a máquina é compartilhada).
    let guardaAtiva = false;
    guarda = setInterval(async () => {
      if (!guardaAtiva) return;
      try {
        const s = agregarMomento(await psSample(app.pid, app.udf, false));
        if (s.totalApp.privateMB > ARGS.limiteMB) {
          log(`ABORTADO: RAM privada do app ${s.totalApp.privateMB} MB acima do limite de ${ARGS.limiteMB} MB`);
          await encerrar(app);
          process.exit(2);
        }
      } catch { /* a próxima volta tenta de novo */ }
    }, 5000);

    // --- montar o cenário: editor, Git, 4 terminais
    const pasta = page.getByRole('treeitem', { name: 'src', exact: true }).first();
    await pasta.waitFor();
    if ((await pasta.getAttribute('aria-expanded')) !== 'true') await pasta.click();
    await page.getByRole('treeitem', { name: 'app.js', exact: true }).first().dblclick();
    await page.locator('.cm-content:visible').first().waitFor();
    await sleep(1500);
    await page.getByRole('button', { name: /^Git/ }).first().click();
    await sleep(2500);
    await page.getByRole('button', { name: 'Arquivos', exact: true }).first().click();
    await sleep(1000);
    out.aberto = ['editor: src/app.js (3000 linhas, CodeMirror)', 'tela Git visitada (40 commits, árvore suja) e retorno a Arquivos', '4 terminais pwsh no painel Terminal'];

    await page.locator('button[title^="Painel Terminal"]').click();
    await page.locator('[data-shell]').first().waitFor();
    for (let n = 1; n < 4; n++) {
      await page.getByRole('button', { name: 'Novo terminal' }).click();
      await page.waitForFunction((c) => document.querySelectorAll('[data-shell]').length === c, n + 1);
    }
    const ids = await page.locator('[data-shell]').evaluateAll((els) => els.map((e) => e.dataset.shell));
    const selecionar = async (id) => {
      await page.locator(`[data-shell="${id}"] [role="tab"]`).click();
      await page.waitForFunction((i) => document.querySelector(`[data-shell="${i}"] [role="tab"]`)?.getAttribute('aria-selected') === 'true', id);
      await sleep(150);
      await page.evaluate(() => window.__medir.watch());
      await page.evaluate(() => window.__medir.quiet(800, 20000));
    };
    for (const id of ids) await selecionar(id); // espera cada shell mostrar o prompt
    // O PSReadLine grava o histórico real do usuário (a pasta vem da API do Windows, não do %APPDATA% do ambiente) e sugere comandos do histórico.
    // Nos 4 terminais: sem gravar histórico e sem predição, para o cenário ser igual em toda execução. A linha que liga isso é removida do arquivo no fim.
    for (const id of ids) {
      await selecionar(id);
      await digitar(cdp, HISTORICO_OFF);
      await tecla(cdp, 'Enter');
      await page.evaluate(() => window.__medir.quiet(800, 20000));
    }
    log('4 terminais prontos:', ids.join(','));

    const linhasDe = {};
    const linhasA = ARGS.taxa * ARGS.segundosA, linhasB = ARGS.taxa * ARGS.segundosB, linhasC = ARGS.taxa * ARGS.segundosC;
    const flood = async (id, tag, n) => {
      linhasDe[tag] = n;
      await selecionar(id);
      await digitar(cdp, `node $env:ORQ_FLOOD ${n} ${tag} ${ARGS.taxa}`);
      await tecla(cdp, 'Enter');
    };
    const inicioDe = (tag) => existsSync(join(app.doneDir, `inicio-${tag}`));
    const inicioMs = (tag) => Number(readFileSync(join(app.doneDir, `inicio-${tag}`), 'utf8'));
    // O gerador acaba assim que o ConPTY aceita a saída; o app ainda a consome depois. "Consumido" = a última linha do gerador já está
    // renderizada no xterm daquele terminal (as linhas do DOM são atualizadas mesmo com a aba escondida). `alvos` = [[posição do terminal, tag]].
    const consumido = (alvos) => page.evaluate((needles) => window.__medir.consumido(needles), alvos.map(([i, tag]) => [i, `[${tag}]${String(linhasDe[tag] - 1).padStart(7, '0')}`]));
    // Espera todos consumirem; devolve, por tag, o tempo (ms) do início do gerador até a última linha aparecer no terminal.
    const esperarConsumo = async (alvos, { timeout = 600000, aCada = async () => {} } = {}) => {
      const feito = {};
      const limite = Date.now() + timeout;
      for (;;) {
        const flags = await consumido(alvos);
        alvos.forEach(([, tag], k) => { if (flags[k] && feito[tag] === undefined) feito[tag] = Date.now() - inicioMs(tag); });
        if (flags.every(Boolean)) return feito;
        if (Date.now() > limite) throw new Error('tempo esgotado esperando o consumo da saída');
        await aCada();
        await sleep(500);
      }
    };

    const mem = async (nome, completo = true) => {
      const proc = agregarMomento(await psSample(app.pid, app.udf, completo));
      const heap = await cdp.send('Runtime.getHeapUsage');
      const perf = (await cdp.send('Performance.getMetrics')).metrics.reduce((a, m) => ({ ...a, [m.name]: m.value }), {});
      const dom = await cdp.send('Memory.getDOMCounters').catch(() => null);
      const pagina = await page.evaluate(() => window.__medir.mem());
      const dumpChromium = nome === 'durante' ? null : await dumpMemoriaChromium(browser).catch((e) => ({ erro: e.message }));
      out.momentos[nome] = { ...proc, dumpChromium, heapJS: { usedMB: mb(heap.usedSize), totalMB: mb(heap.totalSize) }, performanceMetrics: { JSHeapUsedSizeMB: mb(perf.JSHeapUsedSize), JSHeapTotalSizeMB: mb(perf.JSHeapTotalSize), Nodes: perf.Nodes, Documents: perf.Documents, JSEventListeners: perf.JSEventListeners }, domCounters: dom, pagina };
      return out.momentos[nome];
    };

    // --- 1. ocioso
    await selecionar(ids[3]);
    await page.locator('.xterm-helper-textarea').last().focus().catch(() => {});
    log('ocioso: aguardando 15 s');
    await sleep(15000);
    await mem('ocioso');

    // --- 2. latência sem carga (terminal 4 ativo)
    log('latência sem carga');
    out.latenciaSemCarga = await medirLatencia(page, cdp, ARGS.amostras, 'sem saída concorrente');

    // --- 3. latência com os outros 3 terminais despejando
    log('floods nos terminais 1-3');
    guardaAtiva = true;
    const tagsA = ['a', 'b', 'c'];
    for (let i = 0; i < 3; i++) await flood(ids[i], tagsA[i], linhasA);
    await selecionar(ids[3]);
    await page.locator('.xterm-helper-textarea').last().focus().catch(() => {});
    await until(() => tagsA.every(inicioDe), { timeout: 30000, what: 'início dos floods' });
    await sleep(3000);
    const alvosA = tagsA.map((t, i) => [i, t]);
    out.latenciaComCarga = await medirLatencia(page, cdp, ARGS.amostras, '3 terminais despejando saída intensa', async () => (await consumido(alvosA)).map((feito, k) => ({ tag: tagsA[k], consumido: feito })));
    log('aguardando o app consumir a saída dos terminais 1-3');
    out.consumoFaseA = { linhasPorTerminal: linhasA, taxaPorTerminal: ARGS.taxa, msDoInicioDoGeradorAteUltimaLinhaNoTerminal: await esperarConsumo(alvosA) };
    // Recomeça de um heap limpo: a fase B não herda o lixo da fase A.
    await cdp.send('HeapProfiler.collectGarbage');
    await sleep(3000);

    // --- 4. todos os 4 despejando (o 4º visível): RAM durante, quadros
    // Taxa de quadros do monitor em repouso (referência: o rAF acompanha a taxa de atualização da tela).
    await page.evaluate(() => window.__medir.fpsStart());
    await sleep(4000);
    const fpsOcioso = await page.evaluate(() => window.__medir.fpsStop());
    const dOc = fpsOcioso.dts.slice(1);
    out.quadrosOcioso = { fpsMedio: Math.round((dOc.length / (dOc.reduce((a, b) => a + b, 0) / 1000)) * 10) / 10, tempoDeQuadroMs: resumo(dOc) };
    log('floods nos 4 terminais');
    await page.evaluate(() => window.__medir.fpsStart());
    const tagsB = ['d', 'e', 'f', 'g'];
    const tB0 = Date.now();
    for (let i = 0; i < 4; i++) await flood(ids[i], tagsB[i], linhasB);
    await until(() => tagsB.every(inicioDe), { timeout: 30000, what: 'início dos floods B' });
    const amostras = [];
    let durante = false;
    const alvosB = tagsB.map((t, i) => [i, t]);
    const consumoB = await esperarConsumo(alvosB, {
      aCada: async () => {
        // "durante": amostra completa 15 s depois do início; entre elas, amostras leves a cada ~2,5 s para a série e o pico.
        if (!durante && Date.now() - tB0 > 15000) { durante = true; await mem('durante'); return; }
        if (amostras.length && Date.now() - amostras[amostras.length - 1].t < 2500) return;
        const s = agregarMomento(await psSample(app.pid, app.udf, false));
        amostras.push({ t: s.t, tSeg: Math.round((s.t - tB0) / 100) / 10, appPrivateMB: s.totalApp.privateMB, appWorkingSetMB: s.totalApp.workingSetMB, grupos: Object.fromEntries(Object.entries(s.grupos).map(([k, g]) => [k, { privateMB: g.privateMB, cpuSeconds: g.cpuSeconds }])) });
      },
    });
    const tFimB = Date.now();
    const fpsRaw = await page.evaluate(() => window.__medir.fpsStop());
    if (!durante) out.erros.push('a saída foi consumida em menos de 15 s: momento "durante" não medido; aumente --segundos-b');
    out.serieDurante = amostras.map(({ t, ...resto }) => resto);
    out.consumoFaseB = { linhasPorTerminal: linhasB, taxaPorTerminal: ARGS.taxa, msDoInicioDoGeradorAteUltimaLinhaNoTerminal: consumoB, msDoPrimeiroComandoAteTodosConsumidos: tFimB - tB0 };
    // "durante": amostra completa (com working set privado e GPU) no começo do período e a série para o pico
    const dts = fpsRaw.dts.slice(1);
    out.quadrosDurante = { quadros: dts.length, duracaoS: Math.round(dts.reduce((a, b) => a + b, 0) / 100) / 10, fpsMedio: Math.round((dts.length / (dts.reduce((a, b) => a + b, 0) / 1000)) * 10) / 10, tempoDeQuadroMs: resumo(dts), quadrosAcima50ms: dts.filter((x) => x > 50).length, tarefasLongas: { n: fpsRaw.long.length, totalMs: Math.round(fpsRaw.long.reduce((a, b) => a + b, 0)), maxMs: Math.round(Math.max(0, ...fpsRaw.long)) } };
    const picoApp = amostras.reduce((a, s) => (s.appPrivateMB > (a?.appPrivateMB ?? 0) ? s : a), null);
    out.picoDurante = picoApp;

    // --- 5. depois: 30 s após o fim da saída
    log('saída terminou; aguardando 30 s');
    await sleep(30000);
    await mem('depois30s');
    // "durante" completo: repete um flood curto não é determinístico; usa-se a série acima (privado e working set) e o pico.

    // --- 6. diagnóstico de memória: GC + heap snapshot
    const antes = await cdp.send('Runtime.getHeapUsage');
    await cdp.send('HeapProfiler.collectGarbage');
    await sleep(1500);
    const depoisGc = await cdp.send('Runtime.getHeapUsage');
    const procGc = agregarMomento(await psSample(app.pid, app.udf, true));
    const dumpGc = await dumpMemoriaChromium(browser).catch((e) => ({ erro: e.message }));
    out.momentos.depoisDoGC = { ...procGc, dumpChromium: dumpGc, heapJS: { usedMB: mb(depoisGc.usedSize), totalMB: mb(depoisGc.totalSize) }, heapAntesDoGC: { usedMB: mb(antes.usedSize), totalMB: mb(antes.totalSize) } };
    if (!ARGS.semHeap) {
      const file = join(OUT, 'renderer.heapsnapshot');
      const stream = createWriteStream(file);
      const onChunk = ({ chunk }) => stream.write(chunk);
      cdp.on('HeapProfiler.addHeapSnapshotChunk', onChunk);
      log('heap snapshot…');
      await cdp.send('HeapProfiler.takeHeapSnapshot', { reportProgress: false, captureNumericValue: false });
      cdp.off('HeapProfiler.addHeapSnapshotChunk', onChunk);
      await new Promise((r) => stream.end(r));
      out.heapSnapshot = { arquivo: file, MB: mb(statSync(file).size) };
      log('snapshot salvo:', file, out.heapSnapshot.MB, 'MB');
    }

    // --- 7. diagnóstico: de onde vêm CPU e alocações. Os 4 terminais despejam de novo com os perfiladores do V8 ligados (as medidas acima já foram tomadas).
    log('diagnóstico: perfil de CPU e de alocações');
    await cdp.send('Profiler.setSamplingInterval', { interval: 1000 });
    await cdp.send('Profiler.start');
    await cdp.send('HeapProfiler.startSampling', { samplingInterval: 65536, includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true });
    const tagsC = ['h', 'i', 'j', 'k'];
    for (let i = 0; i < 4; i++) await flood(ids[i], tagsC[i], linhasC);
    await esperarConsumo(tagsC.map((t, i) => [i, t]));
    const cpu = (await cdp.send('Profiler.stop')).profile;
    const alloc = (await cdp.send('HeapProfiler.stopSampling')).profile;
    out.perfilCPU = { linhasPorTerminal: linhasC, ...resumirPerfilCPU(cpu) };
    out.perfilAlocacoes = { linhasPorTerminal: linhasC, ...resumirPerfilAlocacoes(alloc) };
    guardaAtiva = false;
    await browser.close().catch(() => {});
    browser = null;
  } finally {
    clearInterval(guarda);
    const sobras = await encerrar(app);
    out.processosRestantes = sobras;
    if (sobras?.length) log('ATENÇÃO: processos restantes após encerrar:', sobras);
  }
  return out;
}

// ---------------------------------------------------------------- análise do heap snapshot (V8) sem dependências
// Lê nodes/edges direto do arquivo para typed arrays, calcula dominadores (Cooper-Harvey-Kennedy) e o tamanho retido de cada nó,
// agrega por construtor (como a visão "Summary" do DevTools) e atribui a memória a partes do app pela cadeia de dominadores.
// O bundle de produção é minificado (construtores viram `UOt`, `e`...), então a atribuição usa nomes de propriedades, que sobrevivem à minificação.
// Cada regra vê as propriedades do objeto (bits abaixo) e vale para tudo que ele domina.
const PROPS_INTERESSANTES = ['_bufferService', '_optionsService', '_maxLength', '_array', '_data', '_combined', 'HEAPU8', 'createOnigScanner', 'memoizedProps', 'lastRenderedReducer', 'viewState', 'docView', 'contentDOM', 'facets'];
const CATEGORIAS = [
  ['xterm (buffers e estado dos terminais)', (b) => b & 0b11 || (b & 0b1100) === 0b1100 || (b & 0b110000) === 0b110000],
  ['shiki / oniguruma (memória do WASM)', (b) => b & 0b11000000],
  ['React (fibras, estado e filas de atualização)', (b) => b & 0b1100000000],
  ['CodeMirror (editor)', (b) => b & 0b11110000000000],
];

function analisarSnapshot(file, inspecionar) {
  const buf = readFileSync(file);
  const iN = buf.indexOf('"nodes":['), iE = buf.indexOf('"edges":['), iS = buf.indexOf('"strings":[');
  if (iN < 0 || iE < 0 || iS < 0) throw new Error('arquivo não parece um .heapsnapshot do V8');
  const head = JSON.parse(buf.toString('utf8', 0, iN).replace(/,\s*$/, '') + '}').snapshot;
  const meta = head.meta, nf = meta.node_fields, ef = meta.edge_fields;
  const NF = nf.length, EF = ef.length, N = head.node_count, E = head.edge_count;
  const parse = (start, count) => {
    const out = new Uint32Array(count);
    let n = 0, v = 0, inNum = false;
    for (let i = start; n < count; i++) {
      const c = buf[i];
      if (c >= 48 && c <= 57) { v = v * 10 + (c - 48); inNum = true; }
      else { if (inNum) { out[n++] = v; v = 0; inNum = false; } if (c === 93) break; }
    }
    return out;
  };
  const nodes = parse(iN + 9, N * NF);
  const edges = parse(iE + 9, E * EF);
  const endStrings = buf.lastIndexOf(']');
  const strings = JSON.parse(buf.toString('utf8', iS + 10, endStrings + 1));
  const iType = nf.indexOf('type'), iName = nf.indexOf('name'), iSelf = nf.indexOf('self_size'), iEc = nf.indexOf('edge_count');
  const jType = ef.indexOf('type'), jTo = ef.indexOf('to_node'), jNameIdx = ef.indexOf('name_or_index');
  const nodeTypes = meta.node_types[0], edgeTypes = meta.edge_types[0];
  const T = { weak: edgeTypes.indexOf('weak'), shortcut: edgeTypes.indexOf('shortcut') };
  const tObject = nodeTypes.indexOf('object'), tClosure = nodeTypes.indexOf('closure'), tString = nodeTypes.indexOf('string'), tArray = nodeTypes.indexOf('array'),
    tHidden = nodeTypes.indexOf('hidden'), tCode = nodeTypes.indexOf('code'), tNative = nodeTypes.indexOf('native'), tCons = nodeTypes.indexOf('concatenated string'),
    tSliced = nodeTypes.indexOf('sliced string'), tNumber = nodeTypes.indexOf('number'), tSynth = nodeTypes.indexOf('synthetic'), tRegexp = nodeTypes.indexOf('regexp'), tShape = nodeTypes.indexOf('object shape');

  const type = new Uint8Array(N), name = new Uint32Array(N), self = new Float64Array(N), first = new Uint32Array(N + 1);
  let acc = 0;
  for (let i = 0; i < N; i++) {
    type[i] = nodes[i * NF + iType]; name[i] = nodes[i * NF + iName]; self[i] = nodes[i * NF + iSelf];
    first[i] = acc; acc += nodes[i * NF + iEc];
  }
  first[N] = acc;
  const edgeTo = (e) => edges[e * EF + jTo] / NF;
  const edgeOk = (e) => { const t = edges[e * EF + jType]; return t !== T.weak && t !== T.shortcut; };

  // DFS iterativa a partir da raiz (nó 0): ordem pós-fixa.
  const po = new Int32Array(N).fill(-1), order = new Uint32Array(N), stackN = new Uint32Array(N), stackE = new Uint32Array(N);
  const seen = new Uint8Array(N);
  let sp = 0, count = 0;
  stackN[0] = 0; stackE[0] = first[0]; seen[0] = 1; sp = 1;
  while (sp) {
    const v = stackN[sp - 1];
    let e = stackE[sp - 1];
    if (e < first[v + 1]) {
      stackE[sp - 1] = e + 1;
      if (!edgeOk(e)) continue;
      const w = edgeTo(e);
      if (!seen[w]) { seen[w] = 1; stackN[sp] = w; stackE[sp] = first[w]; sp++; }
    } else { po[v] = count; order[count++] = v; sp--; }
  }
  // Predecessores (CSR) só dos alcançáveis.
  const pc = new Uint32Array(N + 1);
  for (let v = 0; v < N; v++) if (seen[v]) for (let e = first[v]; e < first[v + 1]; e++) if (edgeOk(e)) pc[edgeTo(e) + 1]++;
  for (let i = 0; i < N; i++) pc[i + 1] += pc[i];
  const preds = new Uint32Array(pc[N]), fill = pc.slice(0, N);
  for (let v = 0; v < N; v++) if (seen[v]) for (let e = first[v]; e < first[v + 1]; e++) if (edgeOk(e)) preds[fill[edgeTo(e)]++] = v;
  // Dominadores.
  const idom = new Int32Array(N).fill(-1);
  idom[0] = 0;
  for (let changed = true; changed;) {
    changed = false;
    for (let k = count - 2; k >= 0; k--) {
      const b = order[k];
      let nd = -1;
      for (let j = pc[b]; j < pc[b + 1]; j++) {
        const p = preds[j];
        if (idom[p] === -1) continue;
        if (nd === -1) { nd = p; continue; }
        let f1 = p, f2 = nd;
        while (f1 !== f2) { while (po[f1] < po[f2]) f1 = idom[f1]; while (po[f2] < po[f1]) f2 = idom[f2]; }
        nd = f1;
      }
      if (nd !== idom[b]) { idom[b] = nd; changed = true; }
    }
  }
  const retained = Float64Array.from(self);
  for (let k = 0; k < count - 1; k++) { const v = order[k]; retained[idom[v]] += retained[v]; }

  // Agrupamento por construtor.
  const groupKey = (v) => {
    const t = type[v], nm = strings[name[v]];
    if (t === tObject) return nm;
    if (t === tClosure) return '(closure)';
    if (t === tString || t === tCons || t === tSliced) return '(string)';
    if (t === tArray) return '(array)';
    if (t === tHidden) return '(system)';
    if (t === tCode) return '(compiled code)';
    if (t === tNumber) return '(number)';
    if (t === tShape) return '(object shape)';
    if (t === tRegexp) return 'RegExp';
    if (t === tNative) return `native: ${nm}`;
    if (t === tSynth) return `synthetic: ${nm}`;
    return `${nodeTypes[t]}: ${nm}`;
  };
  const gid = new Uint32Array(N), gmap = new Map(), gnames = [];
  for (let v = 0; v < N; v++) {
    if (!seen[v]) continue;
    const k = groupKey(v);
    let id = gmap.get(k);
    if (id === undefined) { id = gnames.length; gmap.set(k, id); gnames.push(k); }
    gid[v] = id;
  }
  const G = gnames.length;
  const gCount = new Uint32Array(G), gShallow = new Float64Array(G), gRetained = new Float64Array(G), gActive = new Uint32Array(G);
  // Filhos na árvore de dominadores (CSR) e DFS: o retido do grupo só soma nós sem ancestral do mesmo grupo.
  const cc = new Uint32Array(N + 1);
  for (let v = 1; v < N; v++) if (seen[v] && idom[v] >= 0) cc[idom[v] + 1]++;
  for (let i = 0; i < N; i++) cc[i + 1] += cc[i];
  const kids = new Uint32Array(cc[N]), cf = cc.slice(0, N);
  for (let v = 1; v < N; v++) if (seen[v] && idom[v] >= 0) kids[cf[idom[v]]++] = v;
  const st = new Uint32Array(N + 1), si = new Uint32Array(N + 1);
  sp = 0; st[0] = 0; si[0] = cc[0]; sp = 1;
  gCount[gid[0]]++; gShallow[gid[0]] += self[0]; gRetained[gid[0]] += retained[0]; gActive[gid[0]]++;
  while (sp) {
    const v = st[sp - 1];
    if (si[sp - 1] < cc[v + 1]) {
      const w = kids[si[sp - 1]++];
      const g = gid[w];
      gCount[g]++; gShallow[g] += self[w];
      if (gActive[g] === 0) gRetained[g] += retained[w];
      gActive[g]++;
      st[sp] = w; si[sp] = cc[w]; sp++;
    } else { gActive[gid[v]]--; sp--; }
  }
  // Categorias por dono: a regra do ancestral mais próximo na árvore de dominadores; sem ancestral com regra, o tipo do nó decide.
  const bitOf = new Map();
  strings.forEach((str, i) => { const k = PROPS_INTERESSANTES.indexOf(str); if (k >= 0) bitOf.set(i, 1 << k); });
  const cat = new Int16Array(N).fill(-1);
  const catNames = [...CATEGORIAS.map((c) => c[0]), 'V8: código compilado e fonte dos scripts', 'strings sem dono identificado', 'nativos do Blink (DOM, CSS, estilos)', 'outros objetos (app, antd/lobehub, libs)'];
  const K = CATEGORIAS.length, K_CODE = K, K_STR = K + 1, K_BLINK = K + 2, K_OTHER = K + 3;
  const catSelf = new Float64Array(catNames.length);
  for (let k = count - 1; k >= 0; k--) {
    const v = order[k];
    let c = -1;
    if (type[v] === tObject) {
      let mask = 0;
      for (let e = first[v]; e < first[v + 1]; e++) if (edgeTypes[edges[e * EF + jType]] === 'property') mask |= bitOf.get(edges[e * EF + jNameIdx]) ?? 0;
      if (mask) for (let i = 0; i < K; i++) if (CATEGORIAS[i][1](mask)) { c = i; break; }
    }
    cat[v] = c >= 0 ? c : v === 0 ? -1 : cat[idom[v]];
  }
  for (let v = 0; v < N; v++) {
    if (!seen[v]) continue;
    let c = cat[v];
    if (c < 0) {
      const t = type[v], nm = strings[name[v]] ?? '';
      c = t === tCode || nm.startsWith('system / ExternalStringData') || nm.startsWith('system / Script') ? K_CODE
        : t === tString || t === tCons || t === tSliced ? K_STR
          : t === tNative && !nm.startsWith('system /') ? K_BLINK : K_OTHER;
    }
    catSelf[c] += self[v];
  }

  // Maiores nós individuais por tamanho retido (sem o próprio raiz e nós "sintéticos" gigantes), com a cadeia de dominadores.
  const idxs = [];
  for (let v = 1; v < N; v++) if (seen[v]) idxs.push(v);
  idxs.sort((a, b) => retained[b] - retained[a]);
  const label = (v) => `${nodeTypes[type[v]]}:${(strings[name[v]] ?? '').slice(0, 60)}`;
  const jName = ef.indexOf('name_or_index');
  const eName = (e) => { const t = edgeTypes[edges[e * EF + jType]]; const x = edges[e * EF + jName]; return t === 'element' || t === 'hidden' ? `[${x}]` : strings[x]; };
  const props = (v) => { const r = []; for (let e = first[v]; e < first[v + 1] && r.length < 14; e++) if (edgeTypes[edges[e * EF + jType]] === 'property') r.push(eName(e)); return r; };
  const via = (v) => { const p = idom[v]; for (let e = first[p]; e < first[p + 1]; e++) if (edgeOk(e) && edgeTo(e) === v) return eName(e); return null; };
  const topNos = idxs.slice(0, 80).map((v) => {
    const chain = [];
    for (let p = idom[v], d = 0; d < 10 && p > 0; p = idom[p], d++) chain.push(`${label(p)} (via ${via(p)})`);
    const filhos = Array.from(kids.subarray(cc[v], cc[v + 1])).sort((x, y) => retained[y] - retained[x]).slice(0, 4)
      .map((w) => ({ no: label(w), via: via(w), retidoMB: Math.round(retained[w] / MB * 100) / 100 }));
    return { no: label(v), via: via(v), props: props(v), retidoMB: Math.round(retained[v] / MB * 100) / 100, rasoMB: Math.round(self[v] / MB * 100) / 100, dominadores: chain, filhosDominados: filhos };
  });
  // --inspecionar <texto>: a subárvore de dominadores do maior nó cujo rótulo ("tipo:nome") contém o texto, ou `prop:<nome>` para quem tem essa propriedade (6 níveis, 3 filhos por nível).
  let inspecao = null;
  if (inspecionar) {
    let melhor = -1;
    const porPropriedade = inspecionar.startsWith('prop:') ? inspecionar.slice(5) : null;
    const casa = (v) => (porPropriedade ? props(v).includes(porPropriedade) : label(v).includes(inspecionar));
    for (let v = 1; v < N; v++) if (seen[v] && casa(v) && (melhor < 0 || retained[v] > retained[melhor])) melhor = v;
    const desc = (v, d) => ({
      no: label(v), via: via(v), props: props(v), retidoMB: Math.round(retained[v] / MB * 100) / 100,
      filhos: d >= 6 ? [] : Array.from(kids.subarray(cc[v], cc[v + 1])).sort((x, y) => retained[y] - retained[x]).slice(0, 3).filter((w) => retained[w] > 0.01 * MB).map((w) => desc(w, d + 1)),
    });
    if (melhor >= 0) inspecao = desc(melhor, 0);
  }
  const grupos = [...Array(G).keys()].sort((a, b) => gRetained[b] - gRetained[a]).slice(0, 60)
    .map((g) => ({ construtor: gnames[g], contagem: gCount[g], rasoMB: Math.round(gShallow[g] / MB * 100) / 100, retidoMB: Math.round(gRetained[g] / MB * 100) / 100 }));
  const gruposPorRaso = [...Array(G).keys()].sort((a, b) => gShallow[b] - gShallow[a]).slice(0, 25)
    .map((g) => ({ construtor: gnames[g], contagem: gCount[g], rasoMB: Math.round(gShallow[g] / MB * 100) / 100, retidoMB: Math.round(gRetained[g] / MB * 100) / 100 }));
  let totalRaso = 0;
  for (let v = 0; v < N; v++) if (seen[v]) totalRaso += self[v];
  const porTipo = {};
  for (let v = 0; v < N; v++) if (seen[v]) { const k = nodeTypes[type[v]]; (porTipo[k] ??= { contagem: 0, rasoMB: 0 }); porTipo[k].contagem++; porTipo[k].rasoMB += self[v] / MB; }
  for (const k of Object.keys(porTipo)) porTipo[k].rasoMB = Math.round(porTipo[k].rasoMB * 100) / 100;
  return {
    arquivo: file, nos: N, arestas: E, alcancaveis: count, totalRasoMB: Math.round(totalRaso / MB * 100) / 100,
    porTipoDeNo: porTipo,
    categoriasPorDono: catNames.map((n, i) => ({ categoria: n, rasoMB: Math.round(catSelf[i] / MB * 100) / 100 })).sort((a, b) => b.rasoMB - a.rasoMB),
    inspecao,
    topConstrutoresPorRetido: grupos, topConstrutoresPorRaso: gruposPorRaso, topNosPorRetido: topNos,
  };
}

// ---------------------------------------------------------------- principal
function resumoTexto(r) {
  const L = [];
  const f = (x) => (x == null ? '-' : x);
  if (r.abertura) {
    L.push('Abertura (ms, do spawn ao marco interativo):');
    for (const [k, v] of Object.entries(r.abertura.grupos)) L.push(`  ${k}: ${JSON.stringify(v.marcoMs)}  mediana=${v.resumo.p50} min=${v.resumo.min} max=${v.resumo.max}`);
  }
  if (r.cenario) {
    const c = r.cenario;
    for (const [nome, m] of Object.entries(c.momentos)) L.push(`RAM ${nome}: app private=${m.totalApp.privateMB} MB, ws=${m.totalApp.workingSetMB} MB, wsPrivate=${f(m.totalApp.privateWorkingSetMB)} MB; heapJS=${m.heapJS?.usedMB}/${m.heapJS?.totalMB} MB`);
    if (c.picoDurante) L.push(`RAM pico durante a saída: app private=${c.picoDurante.appPrivateMB} MB (t=${c.picoDurante.tSeg}s)`);
    for (const k of ['latenciaSemCarga', 'latenciaComCarga']) if (c[k]) L.push(`Latência ${c[k].rotulo} [${c[k].metodo}]: n=${c[k].amostras} sem-eco-no-prazo=${c[k].semEcoNoPrazo} dom p50/p95/p99/max=${c[k].eco_dom_ms.p50}/${c[k].eco_dom_ms.p95}/${c[k].eco_dom_ms.p99}/${c[k].eco_dom_ms.max} ms; até o quadro seguinte p50/p95/p99/max=${c[k].eco_quadro_ms.p50}/${c[k].eco_quadro_ms.p95}/${c[k].eco_quadro_ms.p99}/${c[k].eco_quadro_ms.max} ms`);
    if (c.quadrosDurante) L.push(`Quadros durante a saída: ${c.quadrosDurante.fpsMedio} fps, p95=${c.quadrosDurante.tempoDeQuadroMs.p95} ms, >50ms: ${c.quadrosDurante.quadrosAcima50ms}`);
  }
  return L.join('\n');
}

async function main() {
  const resultado = { geradoEm: new Date().toISOString(), argumentos: ARGS, saida: OUT, maquina: { so: process.platform, node: process.version }, protecoes: {} };
  if (process.platform !== 'win32') throw new Error('Só Windows.');
  const instaladoCfg = join(process.env.LOCALAPPDATA ?? '', 'orquestrador', 'config.json');
  if (existsSync(HISTORICO_ARQ)) { mkdirSync(OUT, { recursive: true }); writeFileSync(join(OUT, 'historico-psreadline.bak'), readFileSync(HISTORICO_ARQ)); }
  const antes = { configSha: sha(instaladoCfg), configMtime: existsSync(instaladoCfg) ? statSync(instaladoCfg).mtimeMs : null };
  const instalados = await new Promise((res) => execFile('powershell.exe', ['-NoProfile', '-Command', "(Get-Process orquestrador -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '*\\AppData\\Local\\orquestrador\\*' } | ForEach-Object { $_.Id }) -join ','"], { windowsHide: true }, (e, out) => res(out.trim())));
  resultado.protecoes.instanciaInstaladaPidsAntes = instalados;

  if (ARGS.build || !existsSync(EXE)) {
    log('compilando (npx tauri build --debug --no-bundle, opt-level 3)…');
    const r = spawnSync('npx.cmd', ['tauri', 'build', '--debug', '--no-bundle'], { cwd: ROOT, stdio: 'inherit', shell: true, env: { ...process.env, CARGO_PROFILE_DEV_OPT_LEVEL: '3' } });
    if (r.status !== 0) throw new Error(`build falhou (código ${r.status})`);
  }
  resultado.executavel = { caminho: EXE, tamanhoMB: mb(statSync(EXE).size), modificadoEm: statSync(EXE).mtime.toISOString() };
  writeFileSync(join(WORK, 'flood.mjs'), FLOOD_JS);
  const projectDir = join(WORK, 'p');
  criarProjeto(projectDir);
  resultado.projeto = projectDir;

  if (!ARGS.semAbertura) {
    const grupos = {};
    const novo = [];
    for (let i = 0; i < ARGS.aberturaNovo; i++) {
      log(`abertura, perfil novo ${i + 1}/${ARGS.aberturaNovo}`);
      novo.push(await medirAbertura({ udf: join(WORK, `abertura-novo-${i}`, 'udf'), local: join(WORK, `abertura-novo-${i}`, 'local'), roam: join(WORK, `abertura-novo-${i}`, 'roam') }, projectDir, 'perfil novo'));
    }
    const existente = [];
    if (ARGS.aberturaExistente > 0) {
      const state = { udf: join(WORK, 'abertura-existente', 'udf'), local: join(WORK, 'abertura-existente', 'local'), roam: join(WORK, 'abertura-existente', 'roam') };
      log('abertura, preparando o perfil existente (execução descartada)');
      await medirAbertura(state, projectDir, 'preparo');
      for (let i = 0; i < ARGS.aberturaExistente; i++) {
        log(`abertura, perfil existente ${i + 1}/${ARGS.aberturaExistente}`);
        existente.push(await medirAbertura(state, projectDir, 'perfil existente'));
      }
    }
    for (const [nome, lista] of [['perfilNovo', novo], ['perfilExistente', existente]]) {
      if (!lista.length) continue;
      grupos[nome] = { execucoes: lista, marcoMs: lista.map((x) => x.marcoMs), resumo: resumo(lista.map((x) => x.marcoMs), 0), fcpMs: resumo(lista.map((x) => x.fcpMs).filter((x) => x != null), 0), navegacaoWebviewMs: resumo(lista.map((x) => x.navegacaoWebviewMs), 0) };
    }
    resultado.abertura = { marco: 'nav[aria-label=Telas] + aba de projeto selecionada + treeitem + botão "Painel Terminal" presentes no DOM (Date.now() dentro da página, MutationObserver)', grupos };
  }

  if (!ARGS.semCenario) {
    resultado.cenario = await cenario(projectDir);
    if (resultado.cenario.heapSnapshot) {
      log('analisando o heap snapshot…');
      const analiseFile = join(OUT, 'heap-analise.json');
      const r = spawnSync(process.execPath, ['--max-old-space-size=12288', fileURLToPath(import.meta.url), '--analisar', resultado.cenario.heapSnapshot.arquivo, analiseFile, '--inspecionar', 'prop:lastRenderedReducer'], { stdio: 'inherit' });
      resultado.cenario.heapAnalise = r.status === 0 ? analiseFile : `falhou (código ${r.status})`;
    }
  }

  resultado.protecoes.linhasRemovidasDoHistoricoDoPSReadLine = limparHistorico();
  const depois = { configSha: sha(instaladoCfg), configMtime: existsSync(instaladoCfg) ? statSync(instaladoCfg).mtimeMs : null };
  const vivos = await new Promise((res) => execFile('powershell.exe', ['-NoProfile', '-Command', "(Get-Process orquestrador -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '*\\AppData\\Local\\orquestrador\\*' } | ForEach-Object { $_.Id }) -join ','"], { windowsHide: true }, (e, out) => res(out.trim())));
  resultado.protecoes.instanciaInstaladaPidsDepois = vivos;
  resultado.protecoes.configInstaladaIntacta = antes.configSha === depois.configSha && antes.configMtime === depois.configMtime;

  writeFileSync(join(OUT, 'resultado.json'), JSON.stringify(resultado, null, 2));
  console.log(resumoTexto(resultado));
  console.log(`\nresultado completo: ${join(OUT, 'resultado.json')}`);
  // Limpeza: a pasta de trabalho (projeto, perfis do WebView2, gerador) sai; a cópia do histórico do PSReadLine sai se o arquivo atual
  // começa por ela (nada do usuário foi perdido). O resultado e o heap snapshot ficam em OUT.
  try { rmSync(WORK, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 }); } catch (e) { log('não consegui apagar', WORK, e.message); }
  const bak = join(OUT, 'historico-psreadline.bak');
  if (existsSync(bak) && existsSync(HISTORICO_ARQ) && readFileSync(HISTORICO_ARQ, 'utf8').startsWith(readFileSync(bak, 'utf8'))) rmSync(bak, { force: true });
}

if (ANALISE) {
  const file = opt('analisar');
  const out = argv[argv.indexOf('--analisar') + 2];
  const result = analisarSnapshot(file, opt('inspecionar'));
  if (out) writeFileSync(out, JSON.stringify(result, null, 2));
  else console.log(JSON.stringify(result, null, 2));
} else {
  main().catch((e) => { console.error(e); process.exitCode = 1; }).finally(() => { try { limparHistorico(); } catch { /* sem histórico */ } });
}
