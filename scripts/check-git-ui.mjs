// QA de interface do Git (controle de versão, diff por bloco, histórico, conflitos) e da preservação do shell e do editor, em Chrome headless
// contra o app simulado (src/mock.ts, só no `vite` de desenvolvimento). Usa o loader compartilhado (editor-ui-harness.mjs): cada cenário roda
// com contexto novo, clipboard fake e a árvore carregada. O mock só entra na fronteira de I/O: fixtures (`__mock.addConflict`, `__mock.addFile`),
// edição externa (`__mock.external`), disco (`__mock.disk`), remoção pela árvore (`__mock.remove`) e o registro das chamadas Git (`__mock.calls`),
// que o check usa para conferir o payload que a UI mandou (índice, cabeçalho e revisão vistos), não só o que apareceu na tela.
// Uso: node scripts/check-git-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
// Capturas em .playwright-mcp/git-ui-*.png: evidência para leitura humana, não asserção.
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { OUT, uiSuite, expand, treeItem, tabs, editorText, focusEditor } from './editor-ui-harness.mjs';

// Diagnóstico extra da falha: as chamadas Git que o simulado recusou explicam uma falha de fluxo melhor que a espera que estourou.
const { scenario, run } = uiSuite('git-ui', {
  diagnose: async (page) => {
    const refused = await page.evaluate(() => (window.__mock?.calls ?? []).filter((c) => c.error).map((c) => `${c.cmd}: ${c.error}`)).catch(() => []);
    return [`chamadas Git recusadas: ${refused.length ? refused.join(' | ') : 'nenhuma'}`];
  },
});

// ---------- helpers ----------

const nav = (page) => page.getByRole('navigation', { name: 'Telas' });
const openRoute = (page, name) => nav(page).getByRole('button', { name, exact: true }).click();
const openGit = (page) => nav(page).getByRole('button', { name: /^Git/ }).click();
// Seção do painel Git pelo título (o botão de recolher traz o título e a contagem).
const section = (page, title) => page.locator('section').filter({ has: page.getByRole('button', { name: new RegExp(`^${title}\\b`) }) });
const SIDE = { staged: 'Preparadas', unstaged: 'Alterações' };
// Linha de um arquivo numa seção: o botão de texto tem o título "caminho: palavra. ..." (a palavra é o estado).
const row = (page, side, path) => section(page, SIDE[side]).locator(`button[title^="${path}: "]`);
// Aba pelo rótulo final (o `title` da aba): `label` casa pelo fim, como "(preparadas)" ou "src/App.tsx".
const tab = (page, label) => page.locator(`[data-tab] [role="tab"][title$="${label}"]`);
// Só o corpo da aba atual: o split copia as abas nos dois grupos, e a cópia escondida também tem o bloco.
const hunk = (page, header) => page.locator('[data-body]:visible [data-hunk]').filter({ hasText: header });
const act = (scope, name) => scope.getByRole('button', { name, exact: true });
const dialog = (page, title) => page.getByRole('dialog', { name: title });
const dialogBtn = (page, title, name) => dialog(page, title).getByRole('button', { name, exact: true });
const calls = (page, cmd) => page.evaluate((c) => window.__mock.calls.filter((x) => x.cmd === c && !x.error), cmd);
// Último diff de um lado, como a tela o viu: a revisão e o patch.
async function lastReview(page, path, staged) {
  const seen = (await calls(page, 'git_review_diff')).filter((x) => x.args.path === path && x.args.staged === staged);
  assert.ok(seen.length > 0, `nenhuma leitura do diff de ${path} (${staged ? 'preparadas' : 'alterações'})`);
  return seen.at(-1).result;
}
// Índice do bloco (0-based) pela ordem das linhas `@@` do patch, como o backend numera.
const indexOf = (review, header) => {
  const headers = review.diff.patch.split('\n').filter((l) => l.startsWith('@@'));
  const index = headers.indexOf(header);
  assert.ok(index >= 0, `bloco ${header} não está no patch: ${JSON.stringify(headers)}`);
  return index;
};
const waitCount = async (page, cmd, n) => {
  for (let i = 0; i < 100 && (await calls(page, cmd)).length < n; i++) await page.waitForTimeout(50);
};
const cancelBtn = (page, title) => dialogBtn(page, title, 'Cancelar');
// Botão dentro de um bloco do diff, pela linha de cabeçalho do bloco (texto do `@@`).
const actIn = (page, header, name) => act(hunk(page, header), name);
// Não há sobreposição: o centro do botão cai nele (ou num filho), e a caixa dele fica dentro do contêiner pedido.
async function visibleAndClear(locator, container, label) {
  const ok = await locator.evaluate((el, boxSel) => {
    const r = el.getBoundingClientRect();
    // Desabilitado tem pointer-events: none, então o ponto cai no pai: a sobreposição só vale para o que recebe clique.
    const top = el.disabled ? el : document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    const box = (boxSel === 'parent' ? el.parentElement : el.closest(boxSel))?.getBoundingClientRect();
    const inside = !box || (r.left >= box.left - 0.5 && r.right <= box.right + 0.5 && r.top >= box.top - 0.5 && r.bottom <= box.bottom + 0.5);
    return { hit: !!top && (el === top || el.contains(top)), inside, width: r.width, name: el.textContent.trim() };
  }, container);
  assert.ok(ok.width > 0, `${label}: sem largura`);
  assert.ok(ok.hit, `${label}: outro elemento cobre o botão "${ok.name}"`);
  assert.ok(ok.inside, `${label}: "${ok.name}" saiu do cabeçalho`);
}

// Cabeçalho de cada bloco visível numa linha legível: não picado em vertical (altura de uma linha de 12px, com folga) e com largura útil.
async function headersReadable(page, label) {
  const headers = await page.locator('[data-body]:visible [data-hunk]').evaluateAll((els) => els.map((el) => {
    const code = el.querySelector('code');
    const r = code?.getBoundingClientRect();
    return { text: code?.textContent.trim() ?? '', width: r?.width ?? 0, height: r?.height ?? 0, fontSize: parseFloat(getComputedStyle(code ?? el).fontSize) };
  }));
  assert.ok(headers.length > 0, `${label}: nenhum bloco visível`);
  for (const h of headers) {
    assert.ok(h.width >= 100, `${label}: cabeçalho "${h.text}" estreito demais (${h.width}px)`);
    assert.ok(h.height <= h.fontSize * 2, `${label}: cabeçalho "${h.text}" picado em várias linhas (${h.height}px de altura)`);
  }
}

// Texto de conflito de 2 trechos, com texto fora dos trechos (topo, meio e fim).
const DOIS = 'topo\n<<<<<<< ours\natual-1\n=======\nrecebida-1\n>>>>>>> theirs\nmeio\n<<<<<<< ours\natual-2\n=======\nrecebida-2\n>>>>>>> theirs\nfim\n';
const DOIS_SIDES = { base: 'topo\nmeio\nfim\n', ours: 'topo\natual-1\nmeio\natual-2\nfim\n', theirs: 'topo\nrecebida-1\nmeio\nrecebida-2\nfim\n' };
const addDois = (page) => page.evaluate(([t, s]) => window.__mock.addConflict('src/dois.ts', s, t), [DOIS, DOIS_SIDES]);
const conflictTab = (page, path = 'src/dois.ts') => tab(page, `${path} (conflito)`);
// Escolhe o lado de um trecho na lista (Atual, Recebida ou Ambas), pelo rótulo do trecho.
const choose = (page, n, label) => page.getByText(`Trecho ${n}`, { exact: true }).locator('..').getByRole('button', { name: label, exact: true }).click();
const trechos = (page, n) => page.getByText(n === 1 ? /^1 trecho com conflito/ : new RegExp(`^${n} trechos com conflito`));

// ---------- Git: diff por bloco ----------

scenario('diff: mesmo arquivo nas duas seções abre duas abas; preparar, tirar e descartar um bloco com payload do patch visto; Cancelar preserva; descarte bloqueado com rascunho; não rastreado só prepara o arquivo', async ({ page }) => {
  await openGit(page);
  await row(page, 'staged', 'src/App.tsx').dblclick();
  await row(page, 'unstaged', 'src/App.tsx').dblclick();
  const titles = await page.locator('[data-tab] [role="tab"]').evaluateAll((els) => els.map((el) => el.getAttribute('title')));
  assert.ok(titles.includes('src/App.tsx (preparadas)') && titles.includes('src/App.tsx (alterações)'), `abas: ${titles}`);

  // Preparar o primeiro bloco das alterações. O payload é o bloco visto, com a revisão lida.
  const seen = await lastReview(page, 'src/App.tsx', false);
  await hunk(page, '@@ -1 +1,2 @@').waitFor();
  await actIn(page, '@@ -1 +1,2 @@', 'Preparar').click();
  await waitCount(page, 'git_stage_hunk', 1);
  const stage = (await calls(page, 'git_stage_hunk')).at(-1);
  assert.deepEqual(stage.args.hunk, { path: 'src/App.tsx', index: indexOf(seen, '@@ -1 +1,2 @@'), header: '@@ -1 +1,2 @@', revision: seen.revision });
  await hunk(page, '@@ -1 +1,2 @@').waitFor({ state: 'detached' });
  await hunk(page, '@@ -7 +8 @@').waitFor();

  // Nas preparadas, o bloco aparece; tirar do preparo devolve às alterações.
  await tab(page, '(preparadas)').click();
  await hunk(page, '@@ -1 +1,2 @@').waitFor();
  const staged = await lastReview(page, 'src/App.tsx', true);
  await actIn(page, '@@ -1 +1,2 @@', 'Tirar do stage').click();
  await waitCount(page, 'git_unstage_hunk', 1);
  const unstage = (await calls(page, 'git_unstage_hunk')).at(-1);
  assert.deepEqual(unstage.args.hunk, { path: 'src/App.tsx', index: indexOf(staged, '@@ -1 +1,2 @@'), header: '@@ -1 +1,2 @@', revision: staged.revision });
  await hunk(page, '@@ -1 +1,2 @@').waitFor({ state: 'detached' });

  // Descartar o bloco das alterações: Cancelar não muda nada; Descartar só manda aquele bloco.
  await tab(page, '(alterações)').click();
  const target = hunk(page, '@@ -7 +8 @@');
  await target.waitFor();
  const before = await lastReview(page, 'src/App.tsx', false);
  await actIn(page, '@@ -7 +8 @@', 'Descartar bloco').click();
  await dialog(page, 'Descartar este bloco?').waitFor();
  await cancelBtn(page, 'Descartar este bloco?').click();
  await dialog(page, 'Descartar este bloco?').waitFor({ state: 'detached' });
  assert.equal((await calls(page, 'git_discard_hunk')).length, 0, 'Cancelar não chama o descarte');
  await target.waitFor();
  await actIn(page, '@@ -7 +8 @@', 'Descartar bloco').click();
  await dialogBtn(page, 'Descartar este bloco?', 'Descartar').click();
  await target.waitFor({ state: 'detached' });
  const discard = (await calls(page, 'git_discard_hunk')).at(-1);
  assert.deepEqual(discard.args.hunk, { path: 'src/App.tsx', index: indexOf(before, '@@ -7 +8 @@'), header: '@@ -7 +8 @@', revision: before.revision });
  // O outro bloco das alterações continua no lugar.
  await hunk(page, '@@ -1 +1,2 @@').waitFor();
  assert.equal(await hunk(page, '@@ -7 +8 @@').count(), 0);

  // Rascunho aberto no arquivo: descartar (bloco e arquivo) fica desabilitado.
  await openRoute(page, 'Arquivos');
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type('// rascunho');
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor();
  await openGit(page);
  await tab(page, '(alterações)').click();
  await hunk(page, '@@ -1 +1,2 @@').waitFor();
  assert.ok(await actIn(page, '@@ -1 +1,2 @@', 'Descartar bloco').isDisabled(), 'descartar bloco bloqueado com rascunho');
  assert.ok(await act(page, 'Descartar arquivo').isDisabled(), 'descartar arquivo bloqueado com rascunho');
  assert.equal((await calls(page, 'git_discard_hunk')).length, 1, 'nenhum descarte novo com rascunho');
  await page.screenshot({ path: join(OUT, 'git-ui-bloco-rascunho.png') });

  // Não rastreado: o diff mostra o arquivo inteiro, sem bloco, e a ação é preparar o arquivo.
  await row(page, 'unstaged', 'docs/rascunho-painel.md').dblclick();
  await page.getByText('Arquivo novo, ainda não rastreado').waitFor();
  assert.equal(await hunk(page, '@@').count(), 0, 'não rastreado não tem ação por bloco');
  const untracked = await lastReview(page, 'docs/rascunho-painel.md', false);
  assert.ok(untracked.diff.patch.includes('new file mode'), 'o patch do não rastreado é o arquivo inteiro');
  assert.equal(untracked.untracked, true);
  await act(page, 'Preparar arquivo').click();
  await waitCount(page, 'git_stage', 1);
  assert.deepEqual((await calls(page, 'git_stage')).at(-1).args.paths, ['docs/rascunho-painel.md']);
  await row(page, 'staged', 'docs/rascunho-painel.md').waitFor();
});

// Layout: 1920 e 1040 com dois grupos. As ações do cabeçalho e dos blocos são acessíveis, inteiras e sem sobreposição.
scenario('diff em 1920px e em 1040px com 2 grupos: ações do cabeçalho e dos blocos com nome inteiro, dentro da caixa e sem sobreposição', async ({ page }) => {
  await openGit(page);
  await row(page, 'unstaged', 'src/App.tsx').dblclick();
  await hunk(page, '@@ -7 +8 @@').waitFor();
  await page.screenshot({ path: join(OUT, 'git-ui-diff-1920.png') });
  for (const name of ['Bloco anterior', 'Próximo bloco', 'Descartar arquivo']) await visibleAndClear(act(page, name), 'parent', `1920 ${name}`);
  for (const name of ['Abrir no arquivo', 'Preparar', 'Descartar bloco']) {
    await visibleAndClear(act(hunk(page, '@@ -7 +8 @@'), name), '[data-hunk]', `1920 bloco ${name}`);
  }
  await headersReadable(page, '1920px');

  // Dois grupos: o arquivo abre à esquerda e se divide para a direita; o diff entra no grupo ativo.
  await openRoute(page, 'Arquivos');
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await tab(page, 'src/App.tsx').first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Dividir à Direita/ }).click();
  await page.locator('[data-group="B"]').waitFor();
  await openGit(page);
  await row(page, 'unstaged', 'src/App.tsx').dblclick();
  await hunk(page, '@@ -7 +8 @@').waitFor();
  await page.setViewportSize({ width: 1040, height: 800 });
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(OUT, 'git-ui-diff-1040.png') });
  for (const name of [/^(Unificado|Lado a lado)$/, 'Bloco anterior', 'Próximo bloco', 'Descartar arquivo']) {
    await visibleAndClear(act(page, name), 'parent', `1040 ${String(name)}`);
  }
  for (const name of ['Abrir no arquivo', 'Preparar', 'Descartar bloco']) {
    await visibleAndClear(act(hunk(page, '@@ -7 +8 @@'), name), '[data-hunk]', `1040 bloco ${name}`);
  }
  await headersReadable(page, '1040px, 2 grupos');
  const groups = await page.locator('[data-group]').count();
  assert.equal(groups, 2, 'os dois grupos estão visíveis em 1040px');
});

// ---------- Git: histórico ----------

scenario('histórico do projeto abre commit, arquivo e a aba de diff do commit (só leitura); histórico do arquivo pelo menu; rename no disco não troca o caminho da revisão', async ({ page }) => {
  await openGit(page);
  await page.getByRole('button', { name: 'Ver o histórico do projeto', exact: true }).click();
  const commit = page.locator('button[title^="4b2e8d0 · "]');
  await commit.waitFor();
  assert.equal(await page.locator('button[title*=" · "]').count(), 4, 'quatro commits do projeto');
  await commit.click();
  const renamed = page.locator('button[title^="src/lib/utils.ts (renomeado de src/utils.ts)"]');
  await renamed.waitFor();
  await renamed.click();
  await tab(page, 'src/lib/utils.ts @ 4b2e8d0').waitFor();
  await page.getByText('commit 4b2e8d0 · somente leitura').waitFor();
  // A aba abre antes do conteúdo chegar, e o diff pode estar em shadow DOM (innerText não atravessa): espera pelo texto do locator.
  await page.getByText('depois', { exact: true }).first().waitFor();
  const detail = (await calls(page, 'git_commit_diff')).at(-1);
  assert.deepEqual(detail.args, { id: '4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11', path: 'src/lib/utils.ts' }, 'o diff pede o caminho do commit');

  // Sem botões de mutação na aba de commit nem na lista do histórico.
  const mutation = /^(Preparar|Tirar|Descartar|Salvar|Marcar|Aceitar|Restaurar|Recarregar|Commit)/;
  assert.equal(await page.locator('[data-group="A"] button').filter({ hasText: mutation }).count(), 0, 'aba de commit sem mutação');
  assert.equal(await page.getByRole('button', { name: mutation }).count(), 0, 'histórico sem mutação');
  await page.screenshot({ path: join(OUT, 'git-ui-historico-commit.png') });

  // Histórico de um arquivo pelo menu da árvore: docs/design-system.md tem um commit.
  await openRoute(page, 'Arquivos');
  await expand(page, ['docs']);
  await treeItem(page, 'design-system.md').click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Histórico Git/ }).click();
  await openGit(page);
  await page.locator('code', { hasText: 'design-system.md' }).waitFor();
  assert.equal(await page.locator('button[title^="d7e1a9c · "]').count(), 1, 'um commit do arquivo');
  await page.locator('button[title^="d7e1a9c · "]').click();
  await page.locator('button[title^="docs/design-system.md: modificado. Ver a alteração"]').click();
  await tab(page, 'docs/design-system.md @ d7e1a9c').waitFor();

  // Renomear o arquivo no disco: a aba do commit e o diff continuam com o caminho do commit.
  await openRoute(page, 'Arquivos');
  await treeItem(page, 'design-system.md').click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Renomear/ }).click();
  await page.getByRole('textbox', { name: 'Novo nome' }).fill('sistema.md');
  await dialogBtn(page, 'Renomear arquivo', 'Renomear').click();
  await treeItem(page, 'sistema.md').waitFor();
  assert.equal(await page.evaluate(() => window.__mock.disk('docs/design-system.md')), '', 'o caminho antigo saiu do disco');
  // A barra ainda está no commit aberto (o estado do histórico se mantém): volta à lista e abre o commit de novo.
  await openGit(page);
  await page.getByRole('button', { name: 'Voltar à lista de commits', exact: true }).click();
  await page.locator('button[title^="d7e1a9c · "]').click();
  await page.locator('button[title^="docs/design-system.md: modificado. Ver a alteração"]').click();
  const last = (await calls(page, 'git_commit_diff')).at(-1);
  assert.equal(last.args.path, 'docs/design-system.md', 'a revisão histórica continua no caminho do commit');
  await tab(page, 'docs/design-system.md @ d7e1a9c').waitFor();
});

// ---------- Git: conflitos ----------

scenario('conflito de 2 trechos: escolher Atual e Recebida, texto fora preservado; Salvar atualiza a revisão e mantém em conflito; Marcar como resolvido tira do conflito sem ler o conflito de novo', async ({ page }) => {
  await addDois(page);
  await openGit(page);
  await row(page, 'unstaged', 'src/dois.ts').dblclick();
  await conflictTab(page).waitFor();
  await trechos(page, 2).waitFor();
  const first = (await calls(page, 'git_conflict')).at(-1).result;

  await choose(page, 1, 'Atual');
  await trechos(page, 1).waitFor();
  await choose(page, 1, 'Recebida');
  await page.getByText('Trecho 1', { exact: true }).waitFor({ state: 'detached' });
  // Texto final esperado: trecho 1 = Atual, trecho 2 = Recebida, o resto (topo, meio, fim) como estava.
  const text = 'topo\natual-1\nmeio\nrecebida-2\nfim\n';
  assert.ok(!/<<<<<<<|=======|>>>>>>>/.test(await editorText(page)), 'sem marcadores no editor');

  // Salvar: grava com a revisão lida, relê o conflito e continua em conflito.
  await page.getByRole('button', { name: 'Salvar', exact: true }).click();
  await waitCount(page, 'git_write_resolution', 1);
  const save = (await calls(page, 'git_write_resolution')).at(-1);
  assert.deepEqual(save.args, { path: 'src/dois.ts', content: text, revision: first.revision, markResolved: false });
  assert.equal(await page.evaluate(() => window.__mock.disk('src/dois.ts')), text, 'o disco recebeu a resolução');
  const after = (await calls(page, 'git_conflict')).at(-1).result;
  assert.notEqual(after.revision, first.revision, 'a revisão mudou depois de salvar');
  assert.equal(await page.getByRole('button', { name: 'Salvar', exact: true }).isDisabled(), true, 'sem alteração pendente');
  await row(page, 'unstaged', 'src/dois.ts').waitFor();
  assert.equal(await row(page, 'staged', 'src/dois.ts').count(), 0, 'ainda em conflito: o arquivo não está preparado');

  // Marcar como resolvido: o conflito sai, o arquivo fica preparado, e a aba fecha sem nova leitura do conflito.
  const conflictReads = (await calls(page, 'git_conflict')).length;
  await page.getByRole('button', { name: 'Marcar como resolvido', exact: true }).click();
  await waitCount(page, 'git_write_resolution', 2);
  const mark = (await calls(page, 'git_write_resolution')).at(-1);
  assert.equal(mark.args.markResolved, true);
  await row(page, 'staged', 'src/dois.ts').waitFor();
  assert.equal(await row(page, 'unstaged', 'src/dois.ts').count(), 0, 'saiu das alterações');
  assert.equal((await calls(page, 'git_conflict')).length, conflictReads, 'nenhuma leitura de conflito depois de marcar');
  assert.equal(await conflictTab(page).count(), 0, 'a aba do conflito fechou');
  assert.equal(await page.evaluate(() => window.__mock.disk('src/dois.ts')), text, 'o texto resolvido ficou no disco');
});

scenario('conflito: edição externa depois da leitura deixa o rascunho e mostra erro claro; Recarregar do Git pede confirmação e só então troca o rascunho', async ({ page }) => {
  await addDois(page);
  await openGit(page);
  await row(page, 'unstaged', 'src/dois.ts').dblclick();
  await conflictTab(page).waitFor();
  await trechos(page, 2).waitFor();
  await choose(page, 1, 'Atual');
  await trechos(page, 1).waitFor();

  // Outro programa grava o arquivo: a revisão de quem lê não bate mais.
  const external = DOIS.replace('recebida-2', 'externo-2');
  await page.evaluate((t) => window.__mock.external('src/dois.ts', t), external);
  await page.getByRole('button', { name: 'Atualizar', exact: true }).click();
  await page.getByText('A pasta mudou desde o seu rascunho.', { exact: true }).waitFor();
  const draft = await editorText(page);
  assert.ok(draft.includes('atual-1') && !draft.includes('externo-2'), 'o rascunho com a escolha feita fica como está');

  // Salvar com revisão velha: o Git recusa, nada é gravado e o erro aparece.
  await page.getByRole('button', { name: 'Salvar', exact: true }).click();
  await page.getByText('O arquivo mudou desde que foi lido. Nada foi gravado; atualize a tela.', { exact: false }).waitFor();
  assert.equal(await page.evaluate(() => window.__mock.disk('src/dois.ts')), external, 'o disco ficou com a edição externa');
  assert.equal(await editorText(page).then((t) => t.includes('atual-1')), true, 'o rascunho continua depois do erro');

  // Recarregar: pede confirmação; Cancelar mantém o rascunho.
  await page.getByRole('button', { name: 'Recarregar do Git', exact: true }).click();
  await dialog(page, 'Recarregar do Git?').waitFor();
  await cancelBtn(page, 'Recarregar do Git?').click();
  assert.ok((await editorText(page)).includes('atual-1'), 'Cancelar mantém o rascunho');
  await page.getByRole('button', { name: 'Recarregar do Git', exact: true }).click();
  await dialogBtn(page, 'Recarregar do Git?', 'Recarregar').click();
  await page.getByText('A pasta mudou desde o seu rascunho.', { exact: true }).waitFor({ state: 'detached' });
  const reloaded = await editorText(page);
  // O marcador de conflito repete "atual-1", então a prova é o texto da pasta (externo-2) e os dois trechos de volta.
  await trechos(page, 2).waitFor();
  assert.ok(reloaded.includes('externo-2'), 'recarregar troca o rascunho pelo texto do Git');
});

scenario('conflito: rascunho parcial sobrevive a recarregar a janela (trecho resolvido preservado) e o restante resolve e marca', async ({ page }) => {
  await addDois(page);
  await openGit(page);
  await row(page, 'unstaged', 'src/dois.ts').dblclick();
  await conflictTab(page).waitFor();
  await trechos(page, 2).waitFor();
  await choose(page, 2, 'Recebida');
  await trechos(page, 1).waitFor();
  // Rascunho não salvo: a recuperação (localStorage do projeto) guarda o que já foi escolhido.
  await page.waitForTimeout(400);
  await page.reload();
  // A janela voltou na tela de escolher a pasta. O simulado começa do zero: o disco volta igual ao de antes (mesma revisão)
  // antes de o projeto abrir e a aba restaurada ler o conflito.
  const picker = page.getByRole('button', { name: 'Escolher pasta do projeto' });
  await picker.waitFor();
  await addDois(page);
  await picker.click();
  await nav(page).waitFor();
  await openGit(page);
  if (await conflictTab(page).count() === 0) await row(page, 'unstaged', 'src/dois.ts').dblclick();
  await conflictTab(page).waitFor();
  await trechos(page, 1).waitFor();
  // O trecho já resolvido volta como texto (recebida-2 sem marcador); o outro continua com marcadores.
  const restoredText = await editorText(page);
  assert.ok(restoredText.includes('recebida-2') && restoredText.includes('<<<<<<<'), `o trecho resolvido voltou: ${restoredText}`);
  await choose(page, 1, 'Atual');
  await page.getByRole('button', { name: 'Marcar como resolvido', exact: true }).click();
  await row(page, 'staged', 'src/dois.ts').waitFor();
  assert.equal(await conflictTab(page).count(), 0, 'a aba fechou depois de marcar');
});

scenario('conflito modify/delete (um lado apagou, arquivo presente): Cancelar não faz nada; Aceitar remoção confirmada apaga e prepara a remoção', async ({ page }) => {
  await openGit(page);
  await row(page, 'unstaged', 'docs/apagado.md').dblclick();
  await conflictTab(page, 'docs/apagado.md').waitFor();
  await page.getByText('Um lado apagou o arquivo. Marcar como resolvido mantém o arquivo como está.', { exact: true }).waitFor();
  await page.getByRole('button', { name: 'Aceitar a remoção', exact: true }).click();
  await dialog(page, 'Aceitar a remoção do arquivo?').waitFor();
  await cancelBtn(page, 'Aceitar a remoção do arquivo?').click();
  await dialog(page, 'Aceitar a remoção do arquivo?').waitFor({ state: 'detached' });
  assert.equal((await calls(page, 'git_accept_deletion')).length, 0, 'Cancelar não remove');
  await row(page, 'unstaged', 'docs/apagado.md').waitFor();
  assert.equal(await page.evaluate(() => window.__mock.disk('docs/apagado.md')), '# Rascunho\n\nAjuste.\n', 'o arquivo continua como estava');

  const seen = (await calls(page, 'git_conflict')).at(-1).result;
  await page.getByRole('button', { name: 'Aceitar a remoção', exact: true }).click();
  await dialogBtn(page, 'Aceitar a remoção do arquivo?', 'Aceitar remoção').click();
  await row(page, 'staged', 'docs/apagado.md').waitFor();
  const accept = (await calls(page, 'git_accept_deletion')).at(-1);
  assert.deepEqual(accept.args, { path: 'docs/apagado.md', revision: seen.revision });
  await page.getByRole('button', { name: /^(Preparadas|Alterações)/ }).first().waitFor().catch(() => {});
  assert.equal(await conflictTab(page, 'docs/apagado.md').count(), 0, 'a aba do conflito fechou');
  assert.ok((await row(page, 'staged', 'docs/apagado.md').getAttribute('title')).includes('apagado'), 'a remoção aparece como apagada');
});

scenario('conflito com arquivo ausente na pasta: restaurar o lado existente, marcar como resolvido grava esse lado; Aceitar remoção só com o lado ausente', async ({ page }) => {
  // Ausente: o arquivo some do disco (conflito continua no Git). Só o lado atual existe.
  await page.evaluate(() => window.__mock.remove('docs/apagado.md'));
  await openGit(page);
  await row(page, 'unstaged', 'docs/apagado.md').dblclick();
  await conflictTab(page, 'docs/apagado.md').waitFor();
  await page.getByText('O arquivo não existe na pasta.', { exact: false }).waitFor();
  assert.equal(await page.getByRole('button', { name: 'Restaurar versão recebida', exact: true }).count(), 0, 'o lado recebido não existe');
  await page.getByRole('button', { name: 'Restaurar versão atual', exact: true }).click();
  await page.waitForFunction(() => [...document.querySelectorAll('.cm-content')].some((el) => el.offsetParent && el.textContent.includes('Ajuste.')));
  assert.ok((await editorText(page)).includes('Ajuste.'), 'o texto do lado atual entrou no rascunho');
  await page.getByRole('button', { name: 'Marcar como resolvido', exact: true }).click();
  await row(page, 'staged', 'docs/apagado.md').waitFor();
  assert.ok((await page.evaluate(() => window.__mock.disk('docs/apagado.md'))).includes('Ajuste.'), 'gravou o lado atual no arquivo');
  const write = (await calls(page, 'git_write_resolution')).at(-1);
  assert.equal(write.args.revision, 'ausente', 'a revisão de arquivo ausente foi a lida');
  assert.equal(write.args.markResolved, true);
});

// ---------- Git: gráfico ----------

// Hashes completos dos quatro commits do simulado (src/mock.ts), do mais novo para o mais antigo.
const H9F = '9f3a1c2b7d4e5f60718293a4b5c6d7e8f9a0b1c2';
const H4B = '4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11';
const HD7 = 'd7e1a9c3b5f7081a2c4e6f8a0b2d4f6e8a0c2e4b';
const HA2 = 'a2c4e6f8b0d2e4f6a8c0b2d4e6f8a0c2e4b6d8f0';
// Seção Gráfico: a que tem o botão de recolher com esse nome.
const graph = (page) => page.locator('section').filter({ has: page.getByRole('button', { name: /^Gráfico\b/ }) });
const graphToggle = (page) => graph(page).getByRole('button', { name: /^Gráfico\b/ });
const graphRows = (page) => graph(page).locator('li[data-commit]');
const graphCommit = (page, hash) => graph(page).locator(`li[data-commit="${hash}"]`);
const graphHeight = (page) => graph(page).evaluate((el) => el.getBoundingClientRect().height);
const divider = (page) => page.getByRole('separator', { name: 'Divisor do gráfico' });

scenario('gráfico: os 4 commits do simulado na ordem, com as etiquetas (main atual, origin/main, v0.1.7, feat/painel-git) e o HEAD marcado', async ({ page }) => {
  await openGit(page);
  await graphRows(page).first().waitFor();
  const order = await graphRows(page).evaluateAll((els) => els.map((el) => el.getAttribute('data-commit')));
  assert.deepEqual(order, [H9F, H4B, HD7, HA2], 'mais novo primeiro');
  assert.equal(await graphCommit(page, H9F).getAttribute('data-head'), 'true', 'o HEAD é o 9f3a1c2');
  assert.equal(await graphCommit(page, H4B).getAttribute('data-head'), null, 'só o HEAD é marcado');
  const badge = (hash, title) => graphCommit(page, hash).locator(`span[title="${title}"]`);
  assert.equal(await badge(H9F, 'main (branch atual)').count(), 1, 'main é a branch atual, no 9f3a1c2');
  assert.equal(await badge(H9F, 'v0.1.7 (tag)').count(), 1, 'a tag v0.1.7 no 9f3a1c2');
  assert.equal(await badge(H4B, 'origin/main (remota)').count(), 1, 'origin/main no 4b2e8d0');
  assert.equal(await badge(HD7, 'feat/painel-git (branch)').count(), 1, 'feat/painel-git no d7e1a9c');
  assert.equal(await graph(page).locator('span[title="main (branch atual)"]').count(), 1, 'main só aparece uma vez');
  assert.equal(await graph(page).locator('button[title*="Ver a alteração neste commit"]').count(), 0, 'nenhum commit aberto no começo');
  assert.ok((await graphCommit(page, H9F).innerText()).includes('Prepara v0.1.7 com notas completas e publicação validada'), 'assunto na linha');
});

scenario('gráfico: o commit de merge desenha duas faixas (SVG mais largo que o de um commit linear)', async ({ page }) => {
  await openGit(page);
  await graphRows(page).first().waitFor();
  const lanes = (hash) => graphCommit(page, hash).locator('svg').first();
  const linear = Number(await lanes(H9F).getAttribute('width'));
  const merge = Number(await lanes(H4B).getAttribute('width'));
  assert.equal(linear, 12, 'commit linear: uma faixa de 12 px');
  assert.equal(merge, 24, 'merge: duas faixas de 12 px');
  assert.ok(merge > linear, 'o merge é mais largo que o linear');
  assert.ok((await graphCommit(page, H4B).locator('svg').first().locator('path').count()) >= 2, 'o merge tem as curvas das duas faixas');
});

scenario('gráfico: clicar no commit abre os arquivos ali; clicar no arquivo abre a aba do commit e o marca; clicar de novo recolhe', async ({ page }) => {
  await openGit(page);
  await graphRows(page).first().waitFor();
  await graphCommit(page, H4B).locator('button').first().click();
  const renamed = graph(page).locator('button[title^="src/lib/utils.ts (renomeado de src/utils.ts)"]');
  await renamed.waitFor();
  assert.equal(await graphCommit(page, H4B).locator('button[aria-expanded="true"]').count(), 1, 'o commit está aberto');
  await renamed.click();
  await tab(page, 'src/lib/utils.ts @ 4b2e8d0').waitFor();
  await page.getByText('commit 4b2e8d0 · somente leitura').waitFor();
  assert.equal(await renamed.getAttribute('aria-current'), 'true', 'o arquivo aberto fica marcado');
  await page.screenshot({ path: join(OUT, 'git-ui-grafico.png') });
  await graphCommit(page, H4B).locator('button').first().click();
  await renamed.waitFor({ state: 'detached' });
});

scenario('gráfico: recolher esconde a lista e deixa o cabeçalho com as ações; abrir de novo mostra os commits', async ({ page }) => {
  await openGit(page);
  await graphRows(page).first().waitFor();
  const open = await graphHeight(page);
  await graphToggle(page).click();
  await graphRows(page).first().waitFor({ state: 'detached' });
  assert.equal(await graphToggle(page).getAttribute('aria-expanded'), 'false', 'recolhido');
  assert.ok((await graphHeight(page)) < open / 2, 'recolhida, a seção fica só com o cabeçalho');
  await graph(page).getByRole('button', { name: 'Atualizar o gráfico', exact: true }).waitFor();
  await graphToggle(page).click();
  await graphRows(page).first().waitFor();
  assert.equal(await graphToggle(page).getAttribute('aria-expanded'), 'true', 'aberto de novo');
});

scenario('gráfico: o divisor muda a altura (arrastar e setas) e para no mínimo', async ({ page }) => {
  await openGit(page);
  await graphRows(page).first().waitFor();
  const h0 = await graphHeight(page);
  // Arrastar 40 px para cima: a seção cresce o mesmo tanto.
  const box = await divider(page).boundingBox();
  const x = box.x + box.width / 2, y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x, y - 40, { steps: 6 });
  await page.mouse.up();
  const h1 = await graphHeight(page);
  assert.ok(Math.abs(h1 - (h0 + 40)) <= 2, `arrastar 40 px: ${h0} -> ${h1}`);
  // Setas: para baixo encolhe, e nunca passa do mínimo (96 px).
  await divider(page).focus();
  await page.keyboard.press('ArrowDown');
  assert.ok((await graphHeight(page)) < h1, 'seta para baixo encolhe a seção');
  for (let i = 0; i < 30; i++) await page.keyboard.press('ArrowDown');
  assert.equal(await divider(page).getAttribute('aria-valuenow'), '96', 'o valor do divisor para no mínimo');
  assert.equal(Math.round(await graphHeight(page)), 96, 'a seção fica no mínimo');
});

scenario('gráfico: commit pela UI põe o commit novo no topo com a etiqueta main e tira a main do commit anterior', async ({ page }) => {
  await openGit(page);
  await graphRows(page).first().waitFor();
  await row(page, 'unstaged', 'README.md').hover();
  await act(page, 'Preparar README.md').click();
  await section(page, 'Preparadas').locator('button[title^="README.md: "]').waitFor();
  await page.getByRole('textbox', { name: /^Mensagem do commit/ }).fill('Commit pelo gráfico');
  await act(page, 'Commit').click();
  const top = graphRows(page).first();
  await top.filter({ hasText: 'Commit pelo gráfico' }).waitFor();
  const newHash = await top.getAttribute('data-commit');
  assert.notEqual(newHash, H9F, 'o commit novo não é o antigo');
  assert.equal(await top.getAttribute('data-head'), 'true', 'o commit novo é o HEAD');
  assert.equal(await top.locator('span[title="main (branch atual)"]').count(), 1, 'main está no commit novo');
  assert.equal(await graphCommit(page, H9F).locator('span[title="main (branch atual)"]').count(), 0, 'main saiu do commit anterior');
  assert.equal(await graphCommit(page, H9F).locator('span[title="v0.1.7 (tag)"]').count(), 1, 'a tag v0.1.7 continua no anterior');
  assert.equal((await calls(page, 'git_commit')).at(-1).args.message, 'Commit pelo gráfico');
});

// ---------- Shell e editor (sem tocar no clipboard do usuário) ----------

scenario('terminal: o mesmo shell (elemento, saída e prompt) continua depois de trocar Arquivos, Pesquisar e Git; digitar depois da troca segue na linha', async ({ page }) => {
  await page.keyboard.press('Control+Backquote');
  await page.locator('.xterm').first().waitFor();
  await page.locator('.xterm').first().click();
  await page.keyboard.type('abc');
  await page.getByText('abc', { exact: false }).first().waitFor();
  await page.evaluate(() => { window.__xterm = document.querySelector('.xterm'); });
  // Só as linhas do terminal: o `.xterm` também guarda o CSS do renderizador DOM no próprio texto.
  const screen = () => page.evaluate(() => document.querySelector('.xterm-rows')?.textContent ?? '');
  assert.ok((await screen()).includes('abc'), 'o texto digitado está no shell');
  for (const name of ['Pesquisar', /^Git/, 'Arquivos']) {
    if (typeof name === 'string') await openRoute(page, name); else await openGit(page);
    await page.waitForTimeout(120);
    const same = await page.evaluate(() => window.__xterm === document.querySelector('.xterm'));
    assert.equal(same, true, `ao trocar para ${String(name)}, o xterm é o mesmo elemento`);
    assert.ok((await screen()).includes('abc'), `saída preservada em ${String(name)}`);
    assert.ok((await screen()).includes('PS C:\\projetos\\exemplo>'), `prompt preservado em ${String(name)}: tela = ${JSON.stringify((await screen()).slice(-120))}`);
  }
  await page.locator('.xterm').first().click();
  await page.keyboard.type('def');
  await page.getByText('abcdef', { exact: false }).first().waitFor();
  assert.ok((await screen()).includes('abcdef'), 'digitar depois das trocas segue na mesma linha');
  await page.screenshot({ path: join(OUT, 'git-ui-shell-rotas.png') });
});

scenario('editor: mover a aba para o outro grupo mantém texto e cursor; Ctrl+Z desfaz a última digitação depois da mudança, e depois a anterior', async ({ page }) => {
  await openRoute(page, 'Arquivos');
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type('XYZ');
  const before = await editorText(page);
  assert.ok(before.endsWith('XYZ'), 'digitou no fim');
  // Cursor levado para antes de XYZ por setas (sem clique): depois de mover, tem de continuar exatamente ali.
  for (let i = 0; i < 3; i++) await page.keyboard.press('ArrowLeft');
  await tab(page, 'src/App.tsx').first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Mover para o Outro Grupo/ }).click();
  await page.locator('[data-group="B"] [data-tab] [role="tab"][title="src/App.tsx"]').waitFor();
  assert.equal(await tabs(page, 'A').count(), 0, 'a aba saiu do grupo A');
  assert.equal(await editorText(page), before, 'o texto não mudou ao mover');
  // Foco só por locator.focus (sem clique, sem Ctrl+End): a letra tem de cair antes de XYZ, onde o cursor estava.
  await page.locator('.cm-content:visible').first().focus();
  await page.keyboard.type('W');
  assert.equal(await editorText(page), before.slice(0, -3) + 'WXYZ', 'o cursor ficou antes de XYZ depois de mover');
  await page.keyboard.press('Control+z');
  assert.equal(await editorText(page), before, 'Ctrl+Z desfaz só o W digitado depois de mover');
  await page.keyboard.press('Control+z');
  assert.equal(await editorText(page), before.slice(0, -3), 'Ctrl+Z de novo desfaz o XYZ, com o histórico da aba');
  await page.screenshot({ path: join(OUT, 'git-ui-editor-mover.png') });
});

scenario('editor: Ctrl+G vai para a linha e Ctrl+H abre a busca sem mandar a tecla ao shell; ação de linha (Ctrl+Shift+K) remove a linha e Ctrl+Z restaura', async ({ page }) => {
  await openRoute(page, 'Arquivos');
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await focusEditor(page);
  // Terminal aberto e com um texto: Ctrl+G e Ctrl+H no editor não escrevem no shell.
  await page.keyboard.press('Control+Backquote');
  await page.locator('.xterm').first().waitFor();
  await page.locator('.xterm').first().click();
  await page.keyboard.type('xyz');
  // O eco chega pelo backend: só então a linha está no terminal.
  await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('xyz'));
  const shellBefore = await page.evaluate(() => document.querySelector('.xterm-rows')?.textContent ?? '');
  await focusEditor(page);
  const textBefore = await editorText(page);
  await page.keyboard.press('Control+g');
  await page.getByRole('search', { name: 'Ir para a linha' }).waitFor();
  await page.locator('#goto-line').fill('3');
  await page.keyboard.press('Enter');
  await page.getByRole('search', { name: 'Ir para a linha' }).waitFor({ state: 'detached' });
  assert.equal(await page.locator('.cm-activeLine').first().textContent(), 'export function App() {', 'Ctrl+G foi à linha 3');
  await page.keyboard.press('Control+h');
  await page.locator('.cm-search').first().waitFor();
  // A busca do CodeMirror fecha com Escape; só depois de fechar o editor recebe foco de novo.
  await page.keyboard.press('Escape');
  await page.locator('.cm-search').first().waitFor({ state: 'detached' });
  await focusEditor(page);
  await page.keyboard.press('Control+g');
  await page.locator('#goto-line').waitFor();
  await page.keyboard.press('Escape');
  await page.locator('#goto-line').waitFor({ state: 'detached' });
  const shellAfter = await page.evaluate(() => document.querySelector('.xterm-rows')?.textContent ?? '');
  assert.equal(shellAfter, shellBefore, `o shell não recebeu as teclas do editor (antes ${JSON.stringify(shellBefore.slice(-80))}, depois ${JSON.stringify(shellAfter.slice(-80))})`);
  // Ação de linha na linha 3, depois Ctrl+Z.
  await focusEditor(page);
  await page.keyboard.press('Control+g');
  await page.locator('#goto-line').fill('3');
  await page.keyboard.press('Enter');
  await page.keyboard.press('Control+Shift+K');
  assert.ok(!(await editorText(page)).includes('export function App() {'), 'Ctrl+Shift+K removeu a linha');
  await page.keyboard.press('Control+z');
  assert.equal(await editorText(page), textBefore, 'Ctrl+Z restaurou o texto');
});


process.exitCode = await run();
