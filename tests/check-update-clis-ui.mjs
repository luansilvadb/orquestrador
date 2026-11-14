// Portão de abertura na fase `clis` (simulado com `?update=clis`): as linhas dos dois CLIs com nome, versão e passo, sem
// `Tentar novamente` e sem o resto do app.
// Uso: node tests/check-update-clis-ui.mjs   (saída 0 = passou; 1 = falhou)
import assert from 'node:assert/strict';
import { uiSuite, BASE } from './editor-ui-harness.mjs';

const scenario = uiSuite('update-clis-ui');

scenario('fase clis: uma linha por CLI, sem botão de repetir nem acesso ao app', async ({ page }) => {
  await page.goto(`${BASE}?update=clis`, { waitUntil: 'domcontentloaded', timeout: 30000 });
  const status = page.getByRole('status');
  await status.getByText('Preparando os CLIs…').waitFor();
  const lines = (await status.getByRole('list', { name: 'CLIs' }).getByRole('listitem').allTextContents()).map((t) => t.trim());
  assert.deepEqual(lines, ['Claude Code 2.1.325: atualizando…', 'Codex: instalando…']);
  assert.equal(await page.getByRole('button', { name: 'Tentar novamente' }).count(), 0);
  assert.equal(await page.getByRole('progressbar').count(), 0);
  assert.equal(await page.getByRole('navigation', { name: 'Telas' }).count(), 0);
  await page.keyboard.press('Control+P');
  assert.equal(await page.getByRole('navigation', { name: 'Telas' }).count(), 0);
});
