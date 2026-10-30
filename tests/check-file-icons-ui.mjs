import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { uiSuite, ROOT, treeItem } from './editor-ui-harness.mjs';
import theme from '../src/assets/material-icons/theme.json' with { type: 'json' };

const scenario = uiSuite('file-icons-ui');

async function icon(page, row, key) {
  const img = row.locator('img[data-icon]');
  await img.waitFor();
  await page.waitForFunction((el) => el.complete && el.naturalWidth > 0, await img.elementHandle());
  assert.equal(await img.getAttribute('data-icon'), key);
  const svg = await img.evaluate(async (el) => (await fetch(el.src)).text());
  assert.match(svg, /<svg\b[\s\S]*<\/svg>/i);
  return { src: await img.getAttribute('src'), svg };
}

scenario('ícones SVG da árvore, pasta e abas', async ({ page }) => {
  const failedAssets = [];
  page.on('requestfailed', (request) => failedAssets.push(`${request.url()}: ${request.failure()?.errorText}`));
  page.on('response', (response) => {
    if (response.status() >= 400 && /\.svg(?:\?|$)/.test(response.url())) failedAssets.push(`${response.status()} ${response.url()}`);
  });

  await icon(page, treeItem(page, 'src'), theme.folderNames.src);
  await treeItem(page, 'src').click();
  await icon(page, treeItem(page, 'src'), theme.folderNamesExpanded.src);
  await treeItem(page, 'src').click();
  await icon(page, treeItem(page, 'src'), theme.folderNames.src);

  for (const name of ['script.gd', 'scene.tscn', 'resource.tres', 'project.godot', 'arquivo.inventado']) {
    await page.evaluate((n) => window.__mock.addFile(n, ''), name);
    const key = theme.fileNames[name] ?? theme.fileExtensions[name.split('.').pop()] ?? theme.file;
    const fromTree = await icon(page, treeItem(page, name), key);
    await treeItem(page, name).dblclick();
    const tab = page.locator('[data-group="A"] [data-tab]').filter({ hasText: name });
    await tab.waitFor();
    const fromTab = await icon(page, tab, key);
    assert.equal(fromTab.src, fromTree.src, `${name}: aba e árvore usam o mesmo SVG`);
  }
  assert.deepEqual(failedAssets, []);
  mkdirSync(join(ROOT, '.playwright-mcp'), { recursive: true });
  await page.screenshot({ path: join(ROOT, '.playwright-mcp', 'material-icons-ui.png') });
});
