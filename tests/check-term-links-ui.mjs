// QA do Ctrl+clique nos terminais (src/terminal/term-links.ts), em Chrome headless contra o app simulado: num shell do painel Terminal, a saída
// simulada (`__mock.termOut`) traz um caminho do projeto com linha e uma URL. Sem Ctrl não há sublinhado nem mão; com Ctrl há; Ctrl+clique no
// caminho abre o arquivo na linha, e na URL chama `open_url`. Uso: node tests/check-term-links-ui.mjs
import assert from 'node:assert/strict';
import { tabTitles, uiSuite } from './editor-ui-harness.mjs';

const scenario = uiSuite('term-links-ui');

// Centro do caractere `offset` do trecho `needle` no terminal visível (renderizador DOM: o texto está em `.xterm-rows`).
const point = (page, needle, offset = 1) => page.evaluate(([n, o]) => {
  for (const row of document.querySelectorAll('.term .xterm-rows > div')) {
    if (!row.offsetParent) continue;
    const at = row.textContent.indexOf(n);
    if (at < 0) continue;
    const walker = document.createTreeWalker(row, NodeFilter.SHOW_TEXT);
    let left = at + o, node;
    while ((node = walker.nextNode())) {
      if (left < node.length) {
        const range = document.createRange();
        range.setStart(node, left);
        range.setEnd(node, left + 1);
        const box = range.getBoundingClientRect();
        return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
      }
      left -= node.length;
    }
  }
  return null;
}, [needle, offset]);
const underlined = (page) => page.evaluate(() => [...document.querySelectorAll('.term .xterm-rows span')]
  .some((s) => s.offsetParent && getComputedStyle(s).textDecorationLine.includes('underline')));
const pointer = (page) => page.evaluate(() => [...document.querySelectorAll('.term .xterm-cursor-pointer')].some((x) => x.offsetParent));
const settle = (page) => page.waitForTimeout(300);

scenario('Ctrl sublinha caminho e URL; Ctrl+clique abre o arquivo na linha e a URL pelo open_url', async ({ page }) => {
  await page.keyboard.press('Control+Backquote');
  await page.locator('.term .xterm').first().waitFor();
  const shell = await page.locator('[data-shell]').first().getAttribute('data-shell');
  const project = await page.evaluate(() => window.__mock.calls.findLast((c) => c.cmd === 'start_shell')?.args.project ?? null);
  await page.evaluate(([id, p]) => window.__mock.termOut(id, '\r\nerro em src/App.tsx:3 veja https://example.com/doc.\r\n', p), [shell, project]);
  await page.locator('.term .xterm-rows', { hasText: 'https://example.com/doc' }).first().waitFor();

  const path = await point(page, 'src/App.tsx:3', 2);
  assert.ok(path, 'caminho na tela');
  await page.mouse.move(path.x, path.y);
  await settle(page);
  assert.equal(await underlined(page), false, 'sem Ctrl não sublinha');
  assert.equal(await pointer(page), false, 'sem Ctrl não mostra a mão');

  await page.keyboard.down('Control');
  await page.mouse.move(path.x + 1, path.y);
  await page.waitForFunction(() => [...document.querySelectorAll('.term .xterm-rows span')]
    .some((s) => s.offsetParent && getComputedStyle(s).textDecorationLine.includes('underline')));
  assert.equal(await pointer(page), true, 'com Ctrl mostra a mão');
  await page.keyboard.up('Control');
  await page.waitForFunction(() => ![...document.querySelectorAll('.term .xterm-rows span')]
    .some((s) => s.offsetParent && getComputedStyle(s).textDecorationLine.includes('underline')));

  await page.keyboard.down('Control');
  await page.mouse.move(path.x, path.y);
  await settle(page);
  await page.mouse.down();
  await page.mouse.up();
  await page.keyboard.up('Control');
  await page.waitForFunction(() => document.querySelector('.cm-content:not([aria-hidden]) .cm-activeLine')?.textContent === 'export function App() {');
  assert.ok((await tabTitles(page)).some((t) => t.includes('App.tsx')));
  assert.equal(await page.evaluate(() => window.getSelection()?.toString() ?? ''), '', 'Ctrl+clique não seleciona');

  const url = await point(page, 'https://example.com/doc', 4);
  await page.keyboard.down('Control');
  await page.mouse.move(url.x, url.y);
  await settle(page);
  await page.mouse.move(url.x + 1, url.y);
  await settle(page);
  await page.mouse.down();
  await page.mouse.up();
  await page.keyboard.up('Control');
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'open_url'));
  const opened = await page.evaluate(() => window.__mock.calls.filter((c) => c.cmd === 'open_url').map((c) => c.args.url));
  assert.deepEqual(opened, ['https://example.com/doc']);
});
