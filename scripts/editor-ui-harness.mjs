// Loader e helpers compartilhados pelos checks de interface (editor-ui e arquivos/pesquisa): Chrome headless contra o app simulado
// (src/mock.ts, só no `vite` de desenvolvimento). Sobe o próprio Vite (porta SMOKE_PORT, padrão 1421, com --strictPort) e encerra só esse
// processo. Cada cenário ganha um contexto de navegador novo, com clipboard fake (`window.__copied`); o clipboard do usuário não é tocado.
// Não instala nada: usa o playwright-core e o Chromium já presentes no cache local.
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
import { test, before, after } from 'node:test';
import { existsSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
export const OUT = join(ROOT, '.playwright-mcp');
export const PORT = Number(process.env.SMOKE_PORT ?? 1421);
export const BASE = `http://localhost:${PORT}/`;
// Erros de console que o React já emite no modo de desenvolvimento e que não são do app.
export const KNOWN = [/Download the React DevTools/, /Accessing element\.ref was removed in React 19/];

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
// Chromium já baixado no cache do Playwright (o da versão do playwright-core pode não estar instalado).
function findChrome() {
  if (process.env.SMOKE_CHROME) return process.env.SMOKE_CHROME;
  const cache = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (existsSync(cache)) {
    for (const dir of readdirSync(cache).filter((d) => d.startsWith('chromium-')).sort().reverse()) {
      const candidate = join(cache, dir, 'chrome-win64', 'chrome.exe');
      if (existsSync(candidate)) return candidate;
    }
  }
  return undefined;
}

/** Sobe o Vite de desenvolvimento nesta porta. `stop()` encerra só este processo. */
export function startVite() {
  const vite = spawn(process.execPath, [join(ROOT, 'node_modules', 'vite', 'bin', 'vite.js'), '--port', String(PORT), '--strictPort'], {
    cwd: ROOT, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
  });
  const stop = () => { if (!vite.killed) vite.kill(); };
  process.on('exit', stop);
  return { stop };
}

export async function waitForServer() {
  for (let i = 0; i < 120; i++) {
    try {
      if ((await fetch(BASE)).ok) return;
    } catch { /* ainda subindo */ }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`Vite não respondeu em ${BASE}`);
}

/** Browser headless do Chromium local; quem chama fecha. */
export async function launchBrowser() {
  const { chromium } = await import(pathToFileURL(findPlaywright()).href);
  return chromium.launch({ headless: true, executablePath: findChrome() });
}

/** Contexto novo (sem nada da sessão anterior), com o app já escolhido e a árvore pronta. */
export async function session(browser) {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 } });
  await context.addInitScript(() => {
    window.__copied = [];
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (t) => { window.__copied.push(String(t)); } } });
  });
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') errors.push(`${m.type()}: ${m.text()}`); });
  await openApp(page);
  return { context, page, errors };
}

/** Abre o app na URL dada (o simulado lê a query) e escolhe a pasta: a página recomeça do zero. */
export async function openApp(page, query = '') {
  await page.goto(`${BASE}${query}`);
  await page.getByRole('button', { name: 'Escolher pasta do projeto' }).click();
  await page.getByRole('navigation', { name: 'Telas' }).waitFor();
}

// Árvore: pasta só é aberta se estiver fechada (clicar de novo fecharia).
export async function expand(page, folders) {
  for (const name of folders) {
    const node = page.getByRole('treeitem', { name, exact: true }).first();
    await node.waitFor();
    if ((await node.getAttribute('aria-expanded')) !== 'true') await node.click();
  }
}
export const treeItem = (page, name, nth = 0) => page.getByRole('treeitem', { name, exact: true }).nth(nth);
export const tabs = (page, group = 'A') => page.locator(`[data-group="${group}"] [data-tab]`);
export const tabTitles = async (page, group = 'A') => (await tabs(page, group).allTextContents()).map((t) => t.trim());
export const editorText = (page) => page.locator('.cm-content:visible').first().evaluate((el) => el.textContent);
// Atalhos de arquivo só valem com o foco dentro da área do editor: clica no texto antes.
export async function focusEditor(page) { await page.locator('.cm-content:visible').first().click(); }

// Posição (x, y) da coluna de uma linha (1-based, UTF-16), pelos nós de texto reais do CodeMirror.
export async function columnPoint(page, line, col) {
  return page.evaluate(([ln, cl]) => {
    const content = document.querySelector('.cm-content:not([aria-hidden])');
    const el = content?.querySelectorAll('.cm-line')[ln - 1];
    if (!el) return { error: `linha ${ln} ausente` };
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let remaining = cl - 1;
    let node;
    while ((node = walker.nextNode())) {
      if (remaining <= node.length) {
        const range = document.createRange();
        range.setStart(node, remaining);
        range.setEnd(node, remaining);
        const b = range.getBoundingClientRect();
        return { x: b.left, top: el.getBoundingClientRect().top, bottom: el.getBoundingClientRect().bottom };
      }
      remaining -= node.length;
    }
    return { error: `coluna ${cl} além do fim da linha ${ln}` };
  }, [line, col]);
}
export const primaryCursor = (page) => page.evaluate(() => {
  const c = document.querySelector('.cm-cursor-primary, .cm-cursor');
  if (!c) return null;
  const b = c.getBoundingClientRect();
  return { x: b.left, y: b.top };
});

/**
 * Cenários de um check de interface em `node:test`: o `before` sobe o Vite e o Chromium, o `after` fecha os dois, e cada cenário roda num
 * contexto novo. `prefix` nomeia as capturas de falha (`<prefix>-falha-N.png`, N = posição do cenário). `diagnose(page)`, opcional,
 * devolve linhas extras que saem, com o caminho da captura, como diagnóstico da falha. Devolve `scenario(name, fn)`.
 */
export function uiSuite(prefix, { diagnose } = {}) {
  let vite, browser, count = 0;
  before(async () => { vite = startVite(); await waitForServer(); browser = await launchBrowser(); });
  after(async () => { await browser?.close(); vite?.stop(); });
  return (name, fn) => {
    const index = ++count;
    test(name, async (t) => {
      const { context, page, errors } = await session(browser);
      try {
        await fn({ page, errors });
        const bad = errors.filter((e) => !KNOWN.some((k) => k.test(e)));
        assert.deepEqual(bad, [], 'erros/avisos no console');
      } catch (e) {
        const shot = join(OUT, `${prefix}-falha-${index}.png`);
        await page.screenshot({ path: shot }).catch(() => {});
        for (const line of diagnose ? await diagnose(page) : []) t.diagnostic(line);
        t.diagnostic(`captura da falha: ${shot}`);
        throw e;
      } finally {
        await context.close();
      }
    });
  };
}
