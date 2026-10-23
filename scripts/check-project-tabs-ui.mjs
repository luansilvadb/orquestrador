// Interface das abas de projeto: workspaces preservados, foco de pasta já aberta, encerramento e restauração.
import assert from 'node:assert/strict';
import { uiSuite, BASE } from './editor-ui-harness.mjs';

const scenario = uiSuite('project-tabs-ui');
const tabs = (page) => page.locator('header [role="tab"]:visible');
const selected = (page) => page.locator('header [role="tab"][aria-selected="true"]:visible');
const openFolder = async (page) => page.locator('header button[aria-label="Abrir pasta"]').click();

async function twoProjects(page) {
  await tabs(page).first().waitFor();
  await openFolder(page);
  await page.waitForFunction(() => [...document.querySelectorAll('header [role="tab"]')].filter((el) => getComputedStyle(el).visibility === 'visible').length >= 2);
}

scenario('abrir duas pastas cria duas abas', async ({ page }) => {
  await twoProjects(page);
  assert.equal(await tabs(page).count(), 2);
  assert.match(await selected(page).textContent(), /simulado-2/i);
});

scenario('trocar de projeto preserva árvore, editor e texto digitado', async ({ page }) => {
  await twoProjects(page);
  await page.getByRole('treeitem', { name: 'README.md', exact: true }).click();
  const editor = page.locator('.cm-content:visible').first();
  await editor.click();
  await page.keyboard.press('Control+A');
  await page.keyboard.type('texto do primeiro projeto');
  await tabs(page).first().click();
  assert.match(await selected(page).textContent(), /exemplo/i);
  await page.getByRole('treeitem', { name: 'README.md', exact: true }).click();
  assert.match(await page.locator('.cm-content:visible').first().textContent(), /Orquestrador/);
  await tabs(page).last().click();
  assert.equal(await page.locator('.cm-content:visible').first().textContent(), 'texto do primeiro projeto');
});

scenario('abrir pasta recente já aberta só foca a aba existente', async ({ page }) => {
  await twoProjects(page);
  await page.getByRole('button', { name: 'Abrir menu' }).click();
  await page.getByRole('menuitem', { name: 'Projetos recentes' }).hover();
  await page.getByRole('menuitem', { name: /exemplo/i }).click();
  assert.equal(await tabs(page).count(), 2);
  await selected(page).filter({ hasText: /exemplo/i }).waitFor();
  assert.match(await selected(page).textContent(), /exemplo/i);
});

scenario('fechar projeto com orquestrador pede confirmação', async ({ page }) => {
  await twoProjects(page);
  await page.getByRole('button', { name: 'Abrir terminal', exact: true }).last().click();
  await page.getByRole('button', { name: 'Abrir terminal', exact: true }).last().click();
  await page.getByRole('button', { name: 'Fechar simulado-2' }).click();
  await page.getByText('O orquestrador aberto será encerrado.', { exact: false }).waitFor();
  await page.getByRole('button', { name: 'Fechar projeto e encerrar' }).click();
  await page.waitForFunction(() => [...document.querySelectorAll('header [role="tab"]')].filter((el) => getComputedStyle(el).visibility === 'visible').length === 1);
  assert.equal(await tabs(page).count(), 1);
});

scenario('fechar a última aba volta ao modo sem projeto', async ({ page }) => {
  await twoProjects(page);
  await page.getByRole('button', { name: 'Fechar simulado-2' }).click();
  await page.getByRole('button', { name: 'Fechar exemplo' }).click();
  await page.getByText('Abra uma pasta para explorar os arquivos.', { exact: true }).waitFor();
  assert.equal(await tabs(page).count(), 0);
});

scenario('recarregar restaura ordem e aba ativa', async ({ page }) => {
  await twoProjects(page);
  await tabs(page).first().click();
  await page.evaluate(() => history.replaceState(null, '', '/'));
  await page.goto(BASE);
  await page.getByRole('navigation', { name: 'Telas' }).waitFor();
  await tabs(page).nth(1).waitFor();
  assert.match(await tabs(page).nth(0).textContent(), /exemplo/i);
  assert.match(await tabs(page).nth(1).textContent(), /simulado-2/i);
  assert.match(await selected(page).textContent(), /exemplo/i);
});
