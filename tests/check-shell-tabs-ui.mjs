// QA do arrasto das abas do painel Terminal (issue #13), em Chrome headless contra o app simulado: clique seleciona sem mover, arrastar
// reordena (faixa, lista e vizinha ao fechar), o X e o Esc não reordenam, e a faixa rola sozinha com a aba na borda.
// Uso: node tests/check-shell-tabs-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
import assert from 'node:assert/strict';
import { uiSuite } from './editor-ui-harness.mjs';

const scenario = uiSuite('shell-tabs-ui');
const strip = (page) => page.locator('[data-shell]').first().locator('xpath=ancestor::*[contains(@class,"overflow-x-auto")][1]');
const order = (page) => page.locator('[data-shell]').evaluateAll((els) => els.map((el) => el.dataset.shell));
const active = (page) => page.locator('[data-shell] [role="tab"][aria-selected="true"]').evaluate((el) => el.closest('[data-shell]').dataset.shell);

// Abre o painel (Ctrl+`) e completa `count` shells pelo "+".
async function openShells(page, count) {
  await page.keyboard.press('Control+Backquote');
  await page.locator('[data-shell]').first().waitFor();
  for (let n = 1; n < count; n++) await page.getByRole('button', { name: 'Novo terminal' }).click();
  await assert.doesNotReject(page.waitForFunction((c) => document.querySelectorAll('[data-shell]').length === c, count));
  return order(page);
}

// Segura a aba `id` e anda `dx` px em passos; `finish` decide como termina (soltar, Esc...).
async function drag(page, id, dx, finish = () => page.mouse.up()) {
  const box = await page.locator(`[data-shell="${id}"] [role="tab"]`).boundingBox();
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + dx, y, { steps: 12 });
  await finish();
}

scenario('clique seleciona sem mover; arrastar reordena a faixa, a lista e a vizinha ao fechar', async ({ page }) => {
  const [a, b, c] = await openShells(page, 3);
  await page.locator(`[data-shell="${a}"] [role="tab"]`).click();
  assert.equal(await active(page), a);
  assert.deepEqual(await order(page), [a, b, c]);

  // Leva a primeira para depois da última.
  const width = (await page.locator(`[data-shell="${b}"]`).boundingBox()).width + (await page.locator(`[data-shell="${c}"]`).boundingBox()).width;
  await drag(page, a, width);
  assert.deepEqual(await order(page), [b, c, a]);
  assert.equal(await active(page), a);
  // Sem deslocamento residual depois de soltar.
  const transforms = await page.locator('[data-shell]').evaluateAll((els) => els.map((el) => el.style.transform));
  assert.deepEqual(transforms, ['', '', '']);

  // A lista segue a mesma ordem.
  await page.getByRole('button', { name: /^Lista de abas/ }).click();
  const listed = await page.locator('[role="menu"] [data-tab-key]').allTextContents();
  const labels = await Promise.all([b, c, a].map((id) => page.locator(`[data-shell="${id}"] [role="tab"]`).textContent()));
  // Sem espaços: a aba e a lista separam o programa de jeitos diferentes.
  const bare = (t) => t.replace(/\s+/g, '');
  assert.deepEqual(listed.map(bare), labels.map(bare));
  await page.keyboard.press('Escape');

  // Fechar a ativa (agora a última) passa para a vizinha da esquerda na ordem nova.
  await page.locator(`[data-shell="${a}"]`).getByRole('button', { name: /^Fechar o/ }).click();
  await page.locator(`[data-shell="${a}"]`).waitFor({ state: 'detached' });
  assert.deepEqual(await order(page), [b, c]);
  assert.equal(await active(page), c);

  // Para a esquerda, além da primeira: a trava no começo ainda alcança a posição 0.
  await drag(page, c, -400);
  assert.deepEqual(await order(page), [c, b]);
});

scenario('Esc durante o arrasto e arrastar pelo X não reordenam', async ({ page }) => {
  const ids = await openShells(page, 3);
  await drag(page, ids[0], 400, async () => { await page.keyboard.press('Escape'); await page.mouse.up(); });
  assert.deepEqual(await order(page), ids);
  const transforms = await page.locator('[data-shell]').evaluateAll((els) => els.map((el) => el.style.transform));
  assert.deepEqual(transforms, ['', '', '']);

  const close = await page.locator(`[data-shell="${ids[0]}"]`).getByRole('button', { name: /^Fechar o/ }).boundingBox();
  await page.mouse.move(close.x + close.width / 2, close.y + close.height / 2);
  await page.mouse.down();
  await page.mouse.move(close.x + 400, close.y + close.height / 2, { steps: 12 });
  await page.mouse.up();
  assert.deepEqual(await order(page), ids);
});

scenario('com abas escondidas, a faixa rola sozinha com a aba arrastada na borda', async ({ page }) => {
  const ids = await openShells(page, 12);
  await page.locator(`[data-shell="${ids[0]}"] [role="tab"]`).click();
  await page.waitForFunction(() => document.querySelector('[data-shell]').parentElement.closest('.overflow-x-auto').scrollLeft === 0);
  const edge = await strip(page).boundingBox();
  await drag(page, ids[0], 0, async () => {
    const box = await page.locator(`[data-shell="${ids[0]}"] [role="tab"]`).boundingBox();
    await page.mouse.move(edge.x + edge.width - 4, box.y + box.height / 2, { steps: 12 });
    await page.waitForFunction(() => {
      const box = document.querySelector('[data-shell]').parentElement.closest('.overflow-x-auto');
      return box.scrollLeft + box.clientWidth >= box.scrollWidth - 1;
    });
    await page.mouse.up();
  });
  const after = await order(page);
  assert.equal(after.at(-1), ids[0]);
  assert.equal(await active(page), ids[0]);
});
