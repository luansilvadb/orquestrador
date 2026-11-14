// QA do seletor Preview | Markdown do editor, em Chrome headless contra o app simulado (src/backend/mock.ts).
// Uso: node tests/check-markdown-preview-ui.mjs. Com MD_SHOTS=<pasta>, grava preview.png e markdown.png nela.
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { uiSuite, openApp, expand, treeItem, focusEditor, editorText } from './editor-ui-harness.mjs';

const scenario = uiSuite('markdown-preview-ui');
const GUIA = '# Guia do Orquestrador\n\nUm parágrafo com `código inline` e [link](https://example.com).\n\n> Citação de exemplo.\n\n| Papel | CLI |\n| --- | --- |\n| Orquestrador | claude |\n| Executor | codex |\n\n- [x] feito\n- [ ] pendente\n\n![logo](../assets/logo.png)\n\n```ts\nconst x: number = 1;\n```\n';

scenario('seletor alterna Preview e Markdown, reflete o buffer não salvo e some em .ts', async ({ page }) => {
  await openApp(page);
  await page.evaluate((text) => window.__mock.addFile('docs/guia.md', text), GUIA);
  await expand(page, ['docs']);
  await treeItem(page, 'guia.md').dblclick();
  const group = page.getByRole('radiogroup', { name: 'Modo do Markdown' });
  await group.waitFor();
  assert.equal(await group.getByRole('radio', { name: 'Markdown' }).getAttribute('aria-checked'), 'true', 'abre em Markdown');
  await page.locator('.cm-content:visible').first().waitFor();
  if (process.env.MD_SHOTS) await page.screenshot({ path: join(process.env.MD_SHOTS, 'markdown.png') });

  await group.getByRole('radio', { name: 'Preview' }).click();
  const preview = page.locator('[data-markdown-preview]');
  await preview.locator('h1', { hasText: 'Guia do Orquestrador' }).waitFor();
  await preview.locator('p', { hasText: 'Um parágrafo com' }).waitFor();
  await preview.locator('table').waitFor();
  await page.waitForFunction(() => { const i = document.querySelector('[data-markdown-preview] img[alt="logo"]'); return !!i && i.complete && i.naturalWidth > 0; });
  assert.equal(await page.locator('.cm-content:visible').count(), 0, 'CodeMirror oculto no Preview');
  if (process.env.MD_SHOTS) await page.screenshot({ path: join(process.env.MD_SHOTS, 'preview.png') });

  await group.getByRole('radio', { name: 'Markdown' }).click();
  await focusEditor(page);
  await page.keyboard.press('Control+Home');
  await page.keyboard.type('Texto não salvo\n\n');
  assert.ok((await editorText(page)).includes('Texto não salvo'));
  await group.getByRole('radio', { name: 'Preview' }).click();
  await preview.locator('p', { hasText: 'Texto não salvo' }).waitFor();
  assert.equal(await page.evaluate(() => window.__mock.disk('docs/guia.md').includes('Texto não salvo')), false, 'disco intacto');

  await expand(page, ['src']);
  await treeItem(page, 'longo.ts').dblclick();
  await page.locator('.cm-content:visible').first().waitFor();
  assert.equal(await page.getByRole('radiogroup', { name: 'Modo do Markdown' }).count(), 0, 'sem seletor fora de Markdown');

  await page.locator('[data-tab]', { hasText: 'guia.md' }).click();
  await preview.locator('h1').waitFor();
  assert.equal(await group.getByRole('radio', { name: 'Preview' }).getAttribute('aria-checked'), 'true', 'modo sobrevive à troca de aba');
});
