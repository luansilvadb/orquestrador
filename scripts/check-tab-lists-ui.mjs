// QA das listas de abas em menu (issue #15), em Chrome headless contra o app simulado: a lista "Todas as abas" do Orquestrador numa linha
// por item, com o ícone na margem, o título cortado e o estado à direita; e o "..." do editor com o rótulo das abas alinhado aos outros itens.
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

scenario('"..." do editor: aba com ícone na margem e rótulo alinhado às ações', async ({ page }) => {
  await page.getByRole('treeitem', { name: 'README.md', exact: true }).dblclick();
  await page.getByRole('button', { name: 'Mais Ações...' }).first().click();
  await page.getByRole('menuitem').first().waitFor();
  await menuOpen(page);
  await page.screenshot({ path: join(OUT, 'tab-lists-editor.png') });

  const items = await menuItems(page);
  const tab = items.find((i) => i.text === 'README.md');
  const action = items.find((i) => i.text.startsWith('Fechar Todas'));
  assert.ok(tab.icon && tab.icon.x + tab.icon.w <= tab.item.x + 26 + 1, 'ícone da aba na margem');
  assert.equal(Math.round(tab.label.x - tab.item.x), 26);
  assert.equal(Math.round(action.label.x - action.item.x), 26);
});
