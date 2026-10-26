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

scenario('sincronizar permanece no projeto de origem e a árvore escondida para de consultar', async ({ page }) => {
  await twoProjects(page);
  await tabs(page).first().click();
  await page.waitForFunction(() => document.querySelector('footer button[aria-label^="Sincronizar as alterações"]') && !document.querySelector('footer button[aria-label^="Sincronizar as alterações"]').disabled);
  await page.evaluate(() => { window.__mock.calls.length = 0; window.__mock.delay.git_pull = 900; });
  await page.locator('footer:visible button[aria-label^="Sincronizar as alterações"]').first().click();
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'git_pull'));
  await tabs(page).last().click();
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'git_push'));
  await page.waitForTimeout(1700);
  const calls = await page.evaluate(() => window.__mock.calls);
  const pullIndex = calls.findIndex((c) => c.cmd === 'git_pull');
  assert.equal(calls[pullIndex].project, 'project-1', 'pull usa o primeiro projeto');
  const pullDoneAt = calls[pullIndex].completedAt;
  const postPullStatus = calls.filter((c) => c.cmd === 'git_status' && c.startedAt >= pullDoneAt).sort((a, b) => a.startedAt - b.startedAt)[0];
  assert.equal(postPullStatus?.project, 'project-1', 'status encadeado ao pull fica no projeto de origem');
  assert.ok(calls.filter((c) => c.cmd === 'git_push').every((c) => c.project === 'project-1'), 'push fica no projeto de origem');
  assert.equal(calls.filter((c) => c.cmd === 'list_dir' && c.project === 'project-1').length, 0, 'a árvore escondida não consulta list_dir');
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
  await page.getByRole('button', { name: 'Arquivo', exact: true }).click();
  await page.getByRole('menuitem', { name: 'Projetos recentes' }).hover();
  await page.getByRole('menuitem', { name: /exemplo/i }).click();
  assert.equal(await tabs(page).count(), 2);
  await selected(page).filter({ hasText: /exemplo/i }).waitFor();
  assert.match(await selected(page).textContent(), /exemplo/i);
});

scenario('caminho externo abre pasta, arquivo do projeto e arquivo solto', async ({ page }) => {
  await page.evaluate(() => window.__mock.openPath('C:\\projetos\\simulado-3'));
  await page.waitForFunction(() => [...document.querySelectorAll('header [role="tab"]')].some((el) => el.textContent.includes('simulado-3')));
  assert.match(await selected(page).textContent(), /simulado-3/i);

  await page.evaluate(() => window.__mock.openPath('C:\\projetos\\exemplo\\src\\App.tsx'));
  await page.locator('[data-tab]').filter({ hasText: 'App.tsx' }).waitFor();
  assert.match(await page.locator('.cm-content:visible').first().textContent(), /Orquestrador/);
  assert.match(await selected(page).textContent(), /exemplo/i);

  await page.evaluate(() => window.__mock.openPath('C:\\arquivos\\solto.ts'));
  await page.locator('[data-tab]').filter({ hasText: 'solto.ts' }).waitFor();
  assert.match(await page.locator('.cm-content:visible').first().textContent(), /solto = true/);
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

// Edita o README do projeto ativo (simulado-2, o último aberto).
async function dirtyReadme(page) {
  await page.getByRole('treeitem', { name: 'README.md', exact: true }).click();
  await page.locator('.cm-content:visible').first().click();
  await page.keyboard.press('Control+End');
  await page.keyboard.type(' sujo');
}
const closeAsk = (page) => page.getByRole('dialog').filter({ hasText: 'alterações não salvas' });

scenario('fechar a janela pergunta pelas edições do projeto que não está ativo', async ({ page }) => {
  await twoProjects(page);
  await dirtyReadme(page);
  await tabs(page).first().click();
  await page.evaluate(() => { void window.__mock.close(); });
  await closeAsk(page).getByText('simulado-2: README.md', { exact: false }).waitFor();
  await closeAsk(page).getByRole('button', { name: 'Cancelar' }).click();
  await closeAsk(page).waitFor({ state: 'hidden' });
  assert.equal(await page.evaluate(() => document.body.textContent.includes('Janela fechada')), false, 'Cancelar não fecha');
});

scenario('fechar a aba de projeto com edição suja pergunta antes', async ({ page }) => {
  await twoProjects(page);
  await dirtyReadme(page);
  await page.getByRole('button', { name: 'Fechar simulado-2' }).click();
  await closeAsk(page).getByText('README.md', { exact: false }).waitFor();
  await closeAsk(page).getByRole('button', { name: 'Cancelar' }).click();
  assert.equal(await tabs(page).count(), 2, 'Cancelar mantém a aba');
  await page.getByRole('button', { name: 'Fechar simulado-2' }).click();
  await closeAsk(page).getByRole('button', { name: 'Descartar' }).click();
  await page.waitForFunction(() => [...document.querySelectorAll('header [role="tab"]')].filter((el) => getComputedStyle(el).visibility === 'visible').length === 1);
});

scenario('fechar a última aba volta ao modo sem projeto', async ({ page }) => {
  await twoProjects(page);
  await page.getByRole('button', { name: 'Fechar simulado-2' }).click();
  await page.getByRole('button', { name: 'Fechar exemplo' }).click();
  await page.locator('[data-home]').waitFor();
  assert.equal(await tabs(page).count(), 0);
});

// Issue #36: a versão é conferida só na abertura do app; o "+" abre o projeto sem passar pela tela de atualização.
scenario('abrir pasta pelo "+" não volta à tela de atualização', async ({ page }) => {
  await tabs(page).first().waitFor();
  await page.evaluate(() => {
    window.__bootSeen = false;
    new MutationObserver(() => { if (document.querySelector('.boot-main')) window.__bootSeen = true; })
      .observe(document.body, { childList: true, subtree: true });
  });
  await twoProjects(page);
  await page.locator('[data-group]:visible').first().waitFor();
  assert.equal(await page.evaluate(() => window.__bootSeen), false, 'tela de atualização apareceu');
  assert.equal(await page.getByRole('button', { name: 'Minimizar' }).isEnabled(), true, 'barra conectada');
});

// Issue #36: do modo sem projeto, a primeira pasta também abre direto.
scenario('primeira pasta pelo "+" não volta à tela de atualização', async ({ page }) => {
  await page.goto(`${BASE}?project=none`);
  await page.locator('[data-home]').waitFor();
  await page.evaluate(() => {
    window.__bootSeen = false;
    new MutationObserver(() => { if (document.querySelector('.boot-main')) window.__bootSeen = true; })
      .observe(document.body, { childList: true, subtree: true });
  });
  await openFolder(page);
  await selected(page).waitFor();
  assert.equal(await page.evaluate(() => window.__bootSeen), false, 'tela de atualização apareceu');
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

// Issue #64: o shell aberto sem projeto morre no backend quando a primeira pasta abre; depois de fechá-la, o novo `shell-1` não colide com ele.
scenario('shell sem projeto, abrir e fechar a pasta, e o shell abre de novo', async ({ page }) => {
  await page.goto(`${BASE}?project=none`);
  await page.locator('[data-home]').waitFor();
  const startShell = () => page.evaluate(() => window.__mock.calls.filter((c) => c.cmd === 'start_shell' && c.completedAt !== null).at(-1));
  await page.keyboard.press('Control+Backquote');
  await page.locator('[data-shell]').first().waitFor();
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'start_shell' && c.completedAt !== null));
  assert.equal((await startShell()).error, null);
  await openFolder(page);
  await selected(page).waitFor();
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'close_shell' && c.project === null && c.args.id === 'shell-1'));
  await page.getByRole('button', { name: 'Fechar exemplo' }).click();
  await page.locator('[data-home]').waitFor();
  await page.evaluate(() => { window.__mock.calls.length = 0; });
  await page.keyboard.press('Control+Backquote');
  await page.locator('[data-shell]').first().waitFor();
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'start_shell' && c.completedAt !== null));
  assert.equal((await startShell()).error, null, 'o shell novo abre sem erro de id');
  await page.waitForFunction(() => [...document.querySelectorAll('.xterm-rows')].some((rows) => rows.textContent.includes('PS C:\\Users\\Luan>')));
  assert.equal(await page.locator('[data-shell]').count(), 1, 'a aba do shell novo continua');
});

// Issue #64: `close_project` que falha mostra o erro e não trava o fechamento; o X da aba volta a responder.
scenario('fechar a pasta com erro do backend mostra o erro e o X volta a responder', async ({ page }) => {
  await twoProjects(page);
  await page.evaluate(() => { window.__mock.failCloseProject = true; });
  await page.getByRole('button', { name: 'Fechar simulado-2' }).click();
  await page.getByText('Não foi possível fechar o projeto (simulado).').waitFor();
  assert.equal(await tabs(page).count(), 2, 'a aba fica');
  await page.evaluate(() => { window.__mock.failCloseProject = false; });
  await page.getByRole('button', { name: 'Fechar simulado-2' }).click();
  await page.waitForFunction(() => [...document.querySelectorAll('header [role="tab"]')].filter((el) => getComputedStyle(el).visibility === 'visible').length === 1);
});
