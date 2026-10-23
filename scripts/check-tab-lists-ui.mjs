// QA das listas "Todas as abas" (issues #15, #16 e #17), em Chrome headless contra o app simulado: o mesmo menu no Orquestrador, no Terminal
// e no editor, numa linha por item, com o ícone na margem, o título cortado, o estado à direita e o check na ativa; no Terminal e no editor,
// o X na linha destacada e o Delete fecham a aba sem fechar o menu; o "..." do editor fica só com as ações.
// Uso: node scripts/check-tab-lists-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { uiSuite, OUT } from './editor-ui-harness.mjs';

const scenario = uiSuite('tab-lists-ui');
// Espera o fim da animação de abertura do menu, para a captura sair opaca.
const menuOpen = (page) => page.waitForFunction(() => getComputedStyle(document.querySelector('[role="menu"]')).opacity === '1');
const LONG = 'Projeto: D:\\orquestrador (app Tauri 2 para Windows, instalador NSIS por usuário), abrir pelo Explorer na janela existente';

// Geometria de cada item do menu aberto: caixa do item, do ícone (slot com aria-hidden) e do rótulo, e se o rótulo cortou.
const menuItems = (page) => page.locator('[role="menu"] > [role="menuitem"]').evaluateAll((items) => items.map((item) => {
  const box = (el) => { const r = el?.getBoundingClientRect(); return r && { x: r.x, y: r.y, w: r.width, h: r.height }; };
  const icon = item.querySelector(':scope > div > span[aria-hidden]:first-child');
  const label = item.querySelector(':scope > div > span:not([aria-hidden])');
  const inner = label?.querySelector('.truncate') ?? label;
  return {
    text: item.textContent, item: box(item), icon: icon?.childElementCount ? box(icon) : null, label: box(label),
    cut: inner.scrollWidth > inner.clientWidth, checks: [...item.querySelectorAll('.lucide-check')].map((c) => getComputedStyle(c).visibility),
  };
}));

scenario('lista do Orquestrador: uma linha por item, ícone na margem, título cortado e estado à direita', async ({ page }) => {
  await page.getByRole('button', { name: 'Abrir terminal' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Abrir terminal' }).click();
  await page.getByRole('tab', { name: 'Orquestrador' }).waitFor();
  await page.evaluate((title) => {
    const choice = { tool: 'claude', model: 'opus', effort: 'high' };
    [[title, 'running'], ['Revisar docs', 'queued'], ['Corrigir teste', 'done']].forEach(([t, state], index) =>
      window.__mock.addNode({ project: 'project-1', id: `e${index}`, batch: 'b1', index, title: t, state, choice }));
  }, LONG);
  await page.getByRole('tab', { name: /^Executor #2/ }).click();
  await page.getByRole('button', { name: /^Lista de abas/ }).click();
  await page.getByRole('menuitem').first().waitFor();
  await menuOpen(page);
  await page.screenshot({ path: join(OUT, 'tab-lists-orquestrador.png') });

  const items = await menuItems(page);
  assert.equal(items.length, 4);
  for (const [i, { item, icon, label }] of items.entries()) {
    assert.equal(Math.round(item.h), 26, `altura do item ${i}`);
    if (i > 0) assert.equal(Math.round(item.y - items[i - 1].item.y), 26, `item ${i} sem sobrepor o anterior`);
    assert.ok(icon && icon.x - item.x >= 4 && icon.x + icon.w <= item.x + 26, `ícone do item ${i} na margem de 26 px`);
    assert.ok(Math.abs(icon.y + icon.h / 2 - (item.y + item.h / 2)) <= 1, `ícone do item ${i} centrado na linha`);
    assert.equal(Math.round(label.x - item.x), 26, `rótulo do item ${i} a 26 px`);
  }
  assert.match(items[1].text, /^#1\s+Projeto/);
  assert.ok(items[1].cut, 'título longo corta com reticências');
  assert.ok(items[1].item.w <= 520, `menu estreito (${items[1].item.w} px)`);
  assert.match(items[1].text, /executando$/);
  assert.match(items[2].text, /na fila$/);
  assert.match(items[3].text, /concluído$/);
  // Só a aba ativa (#2) tem o check visível.
  assert.deepEqual(items.map((i) => i.checks), [['hidden'], ['hidden'], ['visible'], ['hidden']]);

  await page.getByRole('menuitem', { name: /#3/ }).click();
  assert.equal(await page.getByRole('tab', { name: /^Executor #3/ }).getAttribute('aria-selected'), 'true');
});

// Confere a geometria comum das linhas: 26 px cada, sem sobrepor, ícone na margem e rótulo a 26 px.
function assertRows(items) {
  for (const [i, { item, icon, label }] of items.entries()) {
    assert.equal(Math.round(item.h), 26, `altura do item ${i}`);
    if (i > 0) assert.equal(Math.round(item.y - items[i - 1].item.y), 26, `item ${i} sem sobrepor o anterior`);
    assert.ok(icon && icon.x - item.x >= 4 && icon.x + icon.w <= item.x + 26 + 1, `ícone do item ${i} na margem de 26 px`);
    assert.equal(Math.round(label.x - item.x), 26, `rótulo do item ${i} a 26 px`);
  }
}
// Visibilidade do X de cada linha.
const closes = (page) => page.locator('[role="menu"] > [role="menuitem"]').evaluateAll((items) =>
  items.map((item) => getComputedStyle(item.querySelector('.tab-list-close')).visibility));

scenario('lista do Terminal: o mesmo menu, sem cabeçalho; X na linha destacada e Delete fecham sem fechar o menu', async ({ page }) => {
  await page.keyboard.press('Control+Backquote');
  await page.locator('[data-shell]').first().waitFor();
  for (let n = 1; n < 3; n++) await page.getByRole('button', { name: 'Novo terminal' }).click();
  await page.waitForFunction(() => document.querySelectorAll('[data-shell]').length === 3);
  await page.getByRole('button', { name: /^Lista de abas: 3/ }).click();
  await page.getByRole('menuitem').first().waitFor();
  await menuOpen(page);

  let items = await menuItems(page);
  assert.equal(items.length, 3);
  assertRows(items);
  assert.ok(items.every((i) => i.text.startsWith('Terminal ')), 'sem cabeçalho, só os terminais');
  // A ativa é a última aberta.
  assert.deepEqual(items.map((i) => i.checks), [['hidden'], ['hidden'], ['visible']]);

  // Com o ponteiro na primeira, o X aparece só nela; na ativa, o X toma o lugar do check.
  await page.getByRole('menuitem').nth(0).hover();
  await page.waitForFunction(() => document.querySelector('[role="menu"] > [role="menuitem"]').hasAttribute('data-highlighted'));
  assert.deepEqual(await closes(page), ['visible', 'hidden', 'hidden']);
  await page.screenshot({ path: join(OUT, 'tab-lists-terminal.png') });
  await page.getByRole('menuitem').nth(2).hover();
  await page.waitForFunction(() => document.querySelectorAll('[role="menu"] > [role="menuitem"]')[2].hasAttribute('data-highlighted'));
  assert.deepEqual((await menuItems(page))[2].checks, ['hidden']);

  // O X fecha aquele terminal; o menu continua aberto e a ativa não muda.
  await page.getByRole('menuitem').nth(0).hover();
  await page.getByRole('menuitem').nth(0).locator('.tab-list-close').click();
  await page.waitForFunction(() => document.querySelectorAll('[data-shell]').length === 2);
  assert.equal(await page.getByRole('menuitem').count(), 2);
  items = await menuItems(page);
  assert.deepEqual(items.map((i) => i.checks), [['hidden'], ['visible']]);

  // Delete fecha a linha destacada pelo teclado.
  await page.getByRole('menuitem').nth(0).focus();
  await page.keyboard.press('Delete');
  await page.waitForFunction(() => document.querySelectorAll('[data-shell]').length === 1);
  assert.equal(await page.getByRole('menuitem').count(), 1);

  // Clicar no item troca de aba e fecha o menu.
  await page.getByRole('menuitem').first().click();
  await page.locator('[role="menu"]').waitFor({ state: 'detached' });
});

// Issue #25: o número exibido é o menor livre; fechar o último terminal pela lista esconde o painel e fecha o menu, sem sobrar menu vazio.
scenario('lista do Terminal: número reaproveitado e menu fechado ao fechar o último', async ({ page }) => {
  await page.keyboard.press('Control+Backquote');
  await page.locator('[data-shell]').first().waitFor();
  for (let n = 1; n < 3; n++) await page.getByRole('button', { name: 'Novo terminal' }).click();
  await page.waitForFunction(() => document.querySelectorAll('[data-shell]').length === 3);
  await page.getByRole('button', { name: 'Fechar o Terminal 2', exact: false }).click();
  await page.waitForFunction(() => document.querySelectorAll('[data-shell]').length === 2);
  await page.getByRole('button', { name: 'Novo terminal' }).click();
  await page.waitForFunction(() => document.querySelectorAll('[data-shell]').length === 3);
  const numbers = await page.locator('[data-shell] [role="tab"]').evaluateAll((tabs) => tabs.map((t) => t.textContent.match(/Terminal (\d+)/)[1]));
  assert.deepEqual(numbers, ['1', '3', '2']);

  for (let left = 3; left > 0; left--) {
    if (!await page.getByRole('menu').isVisible()) {
      await page.getByRole('button', { name: /^Lista de abas/ }).click();
      await menuOpen(page);
    }
    await page.getByRole('menuitem').nth(0).hover();
    await page.getByRole('menuitem').nth(0).locator('.tab-list-close').click();
    await page.waitForFunction((n) => document.querySelectorAll('[data-shell]').length === n, left - 1);
  }
  await page.locator('[role="menu"]').waitFor({ state: 'detached' });
  await page.screenshot({ path: join(OUT, 'tab-lists-terminal-vazio.png') });

  await page.keyboard.press('Control+Backquote');
  await page.locator('[data-shell]').first().waitFor();
  assert.match(await page.locator('[data-shell] [role="tab"]').first().textContent(), /^Terminal 1\b/);
});

scenario('lista do editor: botão antes de Dividir, mesmo menu, X fecha a aba; o "..." só com ações', async ({ page }) => {
  await page.getByRole('treeitem', { name: 'README.md', exact: true }).dblclick();
  await page.getByRole('treeitem', { name: 'package.json', exact: true }).dblclick();
  const toolbar = page.getByRole('toolbar', { name: 'Ações de editor' }).first();
  const list = toolbar.getByRole('button', { name: /^Lista de abas: 2/ });
  const split = toolbar.getByRole('button', { name: 'Dividir o Editor à Direita' });
  assert.ok((await list.boundingBox()).x < (await split.boundingBox()).x, 'lista antes de Dividir');

  await list.click();
  await page.getByRole('menuitem').first().waitFor();
  await menuOpen(page);
  await page.screenshot({ path: join(OUT, 'tab-lists-editor.png') });
  const items = await menuItems(page);
  assertRows(items);
  assert.deepEqual(items.map((i) => i.text), ['README.md', 'package.json']);
  assert.deepEqual(items.map((i) => i.checks), [['hidden'], ['visible']]);

  await page.getByRole('menuitem').nth(0).hover();
  await page.getByRole('menuitem').nth(0).locator('.tab-list-close').click();
  await page.locator('[data-group="A"] [role="tab"]', { hasText: 'README.md' }).waitFor({ state: 'detached' });
  assert.equal(await page.getByRole('menuitem').count(), 1);
  await page.keyboard.press('Escape');

  await toolbar.getByRole('button', { name: 'Mais Ações...' }).click();
  await page.getByRole('menuitem').first().waitFor();
  await menuOpen(page);
  const actions = await menuItems(page);
  assert.ok(actions[0].text.startsWith('Fechar Todas'), `"..." começa nas ações (${actions[0].text})`);
  assert.ok(!actions.some((i) => i.text === 'package.json'), 'sem as abas no "..."');
  assert.equal(Math.round(actions[0].label.x - actions[0].item.x), 26);
});
