// QA do arrasto das abas do editor (issue #14), em Chrome headless contra o app simulado: clique seleciona sem mover, arrastar reordena,
// o X e o Esc não reordenam, as fixadas ficam no começo, soltar na faixa do outro grupo move a aba e a faixa rola sozinha com a aba na borda.
// Uso: node scripts/check-editor-tabs-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
import assert from 'node:assert/strict';
import { uiSuite, treeItem, tabs, focusEditor } from './editor-ui-harness.mjs';

const scenario = uiSuite('editor-tabs-ui');
// Só o nome: o texto da aba inclui o "(fixada)" e o "(prévia)" do leitor de tela.
const order = (page, group = 'A') => tabs(page, group).evaluateAll((els) => els.map((el) => el.querySelector('[role="tab"] span.min-w-0').textContent.trim()));
const active = (page, group = 'A') => page.locator(`[data-group="${group}"] [role="tab"][aria-selected="true"]`).textContent();
const tab = (page, name, group = 'A') => tabs(page, group).filter({ hasText: name }).first();
const transforms = (page) => page.locator('[data-tab]').evaluateAll((els) => els.map((el) => el.style.transform).filter(Boolean));

// Cria os arquivos e abre cada um como aba permanente (duplo clique na árvore).
async function openFiles(page, names) {
  for (const name of names) {
    await page.evaluate((n) => window.__mock.addFile(n, `${n}\n`), name);
    await treeItem(page, name).dblclick();
    await tab(page, name).waitFor();
  }
  return order(page);
}

// Segura a aba e anda `dx` px em passos; `finish` decide como termina (soltar, Esc...).
async function drag(page, locator, dx, finish = () => page.mouse.up()) {
  const box = await locator.locator('[role="tab"]').boundingBox();
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + dx, y, { steps: 12 });
  await finish();
}

scenario('clique seleciona sem mover; arrastar reordena; Esc e o X não reordenam', async ({ page }) => {
  const [a, b, c] = await openFiles(page, ['aa.md', 'bb.md', 'cc.md']);
  await tab(page, a).locator('[role="tab"]').click();
  assert.equal(await active(page), a);
  assert.deepEqual(await order(page), [a, b, c]);

  const width = (await tab(page, b).boundingBox()).width + (await tab(page, c).boundingBox()).width;
  await drag(page, tab(page, a), width);
  assert.deepEqual(await order(page), [b, c, a]);
  assert.equal(await active(page), a);
  assert.deepEqual(await transforms(page), [], 'sem deslocamento residual');

  // Para a esquerda, além da primeira: a trava no começo ainda alcança a posição 0.
  await drag(page, tab(page, c), -600);
  assert.deepEqual(await order(page), [c, b, a]);
  assert.equal(await active(page), c, 'a arrastada fica ativa');

  await drag(page, tab(page, c), 600, async () => { await page.keyboard.press('Escape'); await page.mouse.up(); });
  assert.deepEqual(await order(page), [c, b, a], 'Esc cancela');
  assert.deepEqual(await transforms(page), []);

  const close = await tab(page, b).getByRole('button', { name: /^Fechar/ }).boundingBox();
  await page.mouse.move(close.x + close.width / 2, close.y + close.height / 2);
  await page.mouse.down();
  await page.mouse.move(close.x - 400, close.y + close.height / 2, { steps: 12 });
  await page.mouse.up();
  assert.deepEqual(await order(page), [c, b, a], 'arrastar pelo X não reordena');
});

scenario('fixadas e não fixadas não se misturam no arrasto', async ({ page }) => {
  const [a, b, c] = await openFiles(page, ['aa.md', 'bb.md', 'cc.md']);
  // Fixa a primeira (Ctrl+K Shift+Enter no editor dela).
  await tab(page, a).locator('[role="tab"]').click();
  await focusEditor(page);
  await page.keyboard.press('Control+k');
  await page.keyboard.press('Shift+Enter');
  await tab(page, a).locator('.sr-only', { hasText: '(fixada)' }).waitFor();

  await drag(page, tab(page, c), -600);
  assert.deepEqual(await order(page), [a, c, b], 'a não fixada para depois das fixadas');
  await drag(page, tab(page, a), 600);
  assert.deepEqual(await order(page), [a, c, b], 'a fixada não sai do começo');
});

scenario('soltar na faixa do outro grupo move a aba para lá, na posição do ponteiro', async ({ page }) => {
  const [a] = await openFiles(page, ['aa.md']);
  // Dividir abre a cópia de `aa` no grupo B; as outras abas ficam só no A.
  await page.getByRole('button', { name: 'Dividir o Editor à Direita' }).click();
  await page.locator('[data-group="B"]').waitFor();
  await tab(page, a, 'A').locator('[role="tab"]').click();
  await page.evaluate(() => { window.__mock.addFile('bb.md', 'bb\n'); window.__mock.addFile('cc.md', 'cc\n'); });
  for (const name of ['bb.md', 'cc.md']) {
    await treeItem(page, name).dblclick();
    await tab(page, name, 'A').waitFor();
  }
  assert.deepEqual(await order(page, 'A'), [a, 'bb.md', 'cc.md']);
  assert.deepEqual(await order(page, 'B'), [a]);

  // `cc` vai para antes de `aa` no B: solta na metade esquerda da aba do B.
  const source = await tab(page, 'cc.md', 'A').locator('[role="tab"]').boundingBox();
  const dest = await tab(page, a, 'B').boundingBox();
  await page.mouse.move(source.x + source.width / 2, source.y + source.height / 2);
  await page.mouse.down();
  await page.mouse.move(dest.x + 4, dest.y + dest.height / 2, { steps: 20 });
  await page.waitForFunction(() => document.querySelector('[data-group="B"]').className.includes('primary-bg'));
  await page.mouse.up();
  await tab(page, 'cc.md', 'B').waitFor();
  assert.deepEqual(await order(page, 'A'), [a, 'bb.md']);
  assert.deepEqual(await order(page, 'B'), ['cc.md', a]);
  assert.equal(await active(page, 'B'), 'cc.md');
  assert.deepEqual(await transforms(page), []);
});

scenario('com abas escondidas, a faixa rola sozinha com a aba arrastada na borda', async ({ page }) => {
  const names = Array.from({ length: 16 }, (_, i) => `arquivo-${String(i).padStart(2, '0')}.md`);
  const ids = await openFiles(page, names);
  const strip = page.locator('[data-group="A"] [role="tablist"]');
  await tab(page, ids[0]).locator('[role="tab"]').click();
  await page.waitForFunction(() => document.querySelector('[data-group="A"] [role="tablist"]').scrollLeft === 0);
  const edge = await strip.boundingBox();
  await drag(page, tab(page, ids[0]), 0, async () => {
    const box = await tab(page, ids[0]).boundingBox();
    await page.mouse.move(edge.x + edge.width - 4, box.y + box.height / 2, { steps: 12 });
    await page.waitForFunction(() => {
      const box = document.querySelector('[data-group="A"] [role="tablist"]');
      return box.scrollLeft + box.clientWidth >= box.scrollWidth - 1;
    });
    await page.mouse.up();
  });
  const after = await order(page);
  assert.equal(after.at(-1), ids[0]);
  assert.equal(await active(page), ids[0]);
});
