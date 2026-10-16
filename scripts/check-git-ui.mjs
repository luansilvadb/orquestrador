// QA de interface do Git (controle de versão, comparador do arquivo inteiro com blocos, histórico, conflitos) e da preservação do shell e do editor, em Chrome headless
// contra o app simulado (src/mock.ts, só no `vite` de desenvolvimento). Usa o loader compartilhado (editor-ui-harness.mjs): cada cenário roda
// com contexto novo, clipboard fake e a árvore carregada. O mock só entra na fronteira de I/O: fixtures (`__mock.addConflict`, `__mock.addFile`),
// edição externa (`__mock.external`), disco (`__mock.disk`), remoção pela árvore (`__mock.remove`) e o registro das chamadas Git (`__mock.calls`),
// que o check usa para conferir o payload que a UI mandou (índice, cabeçalho e revisão vistos), não só o que apareceu na tela.
// Uso: node scripts/check-git-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
// Capturas em .playwright-mcp/git-ui-*.png: evidência para leitura humana, não asserção.
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { BASE, OUT, uiSuite, expand, treeItem, tabs, editorText, focusEditor } from './editor-ui-harness.mjs';

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
const dialog = (page, title) => page.getByRole('dialog', { name: title });
const dialogBtn = (page, title, name) => dialog(page, title).getByRole('button', { name, exact: true });
const calls = (page, cmd) => page.evaluate((c) => window.__mock.calls.filter((x) => x.cmd === c && !x.error), cmd);
// Último diff de um lado, como a tela o viu: a revisão e o patch.
async function lastReview(page, path, staged) {
  const seen = (await calls(page, 'git_review_diff')).filter((x) => x.args.path === path && x.args.staged === staged);
  assert.ok(seen.length > 0, `nenhuma leitura do diff de ${path} (${staged ? 'preparadas' : 'alterações'})`);
  return seen.at(-1).result;
}
const waitCount = async (page, cmd, n) => {
  for (let i = 0; i < 100 && (await calls(page, cmd)).length < n; i++) await page.waitForTimeout(50);
};
const cancelBtn = (page, title) => dialogBtn(page, title, 'Cancelar');
// Reabre o app com a URL dada (o modo do Git e a pasta escolhida vêm dela) e escolhe a pasta de novo: a página recomeça do zero.
async function reopen(page, query = '') {
  await page.goto(`${BASE}${query}`);
  await page.getByRole('button', { name: 'Escolher pasta do projeto' }).click();
  await page.getByRole('navigation', { name: 'Telas' }).waitFor();
}
const NO_REPO_TEXT = /^A pasta aberta não tem um repositório Git\./;
const initBtn = (page) => page.getByRole('button', { name: 'Inicializar repositório', exact: true });
const historyItem = (page) => page.getByRole('menuitem', { name: /Histórico Git/ });
// Menu da árvore de um arquivo de docs (docs/design-system.md), aberto pelo clique direito.
async function openFileMenu(page) {
  await openRoute(page, 'Arquivos');
  await expand(page, ['docs']);
  await treeItem(page, 'design-system.md').click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Abrir com/ }).waitFor();
}
// Comparador do Git: o arquivo inteiro numa visão só (um PatchDiff) e a unidade atual (bloco do Git, ou trecho no commit) marcada.
// As linhas vêm do motor (shadow DOM do PatchDiff): `data-line` é o número da linha e o topo é a posição real na tela.
const view = (page) => page.locator('[data-body]:visible .compare-view');
const bar = (page) => page.locator('[data-body]:visible .compare-toolbar').first();
// Ação da barra pelo nome. A barra é o escopo, para o nome não casar com outro botão do comparador, como o do divisor.
const tb = (page, name) => bar(page).getByRole('button', { name, exact: true });
// Texto do contador da barra: "bloco 1 de 2", "mudança 1 de 3", "arquivo novo, não rastreado", "preparadas" ou "alterações".
const counterOf = async (page) => (await bar(page).locator('span').filter({ hasText: /^(bloco|mudança|arquivo novo|preparadas$|alterações$)/ }).first().textContent()).trim();
const waitCounter = (page, text) => bar(page).locator('span').filter({ hasText: new RegExp(`^${text}$`) }).first().waitFor();
// Linhas do motor: número, tipo e topo relativo ao corpo do comparador (um item por lado na visão lado a lado).
const engineRows = (page) => page.evaluate(() => {
  const stage = [...document.querySelectorAll('[data-body] .compare-stage')].find((el) => el.getClientRects().length > 0);
  const root = [...stage.querySelectorAll('*')].find((e) => e.shadowRoot).shadowRoot;
  const box = stage.getBoundingClientRect();
  const area = stage.parentElement.getBoundingClientRect();
  return [...root.querySelectorAll('[data-line]')].map((el) => {
    const r = el.getBoundingClientRect();
    // inView: a linha cai na área visível do corpo (não só existe no motor).
    return { line: Number(el.getAttribute('data-line')), type: el.getAttribute('data-line-type'), top: r.top - box.top, height: r.height, x: r.left, y: r.top + r.height / 2, inView: r.top >= area.top && r.bottom <= area.bottom };
  });
});
// A unidade atual: o `data-unit` da marca azul e a linha do motor que está exatamente no topo dela (a linha real da mudança).
// A marca aparece depois da medida do motor (um render depois do bloco mudar), então o helper espera por ela.
const current = async (page) => {
  await page.locator('[data-body]:visible .compare-current').waitFor();
  return page.evaluate(() => {
    const stage = [...document.querySelectorAll('[data-body] .compare-stage')].find((el) => el.getClientRects().length > 0);
    const cur = stage.querySelector('.compare-current');
    if (!cur) return null;
    const box = stage.getBoundingClientRect();
    const top = cur.getBoundingClientRect().top - box.top;
    const root = [...stage.querySelectorAll('*')].find((e) => e.shadowRoot).shadowRoot;
    const at = [...root.querySelectorAll('[data-line]')].filter((el) => Math.abs(el.getBoundingClientRect().top - box.top - top) < 1);
    return { unit: Number(cur.dataset.unit), top: Math.round(top), line: at.length ? Number(at[0].getAttribute('data-line')) : null, rowType: at[0]?.getAttribute('data-line-type') ?? null };
  });
};
// Rola o corpo do comparador visível até `top` (pixels): a linha do motor precisa estar na tela para o clique cair nela.
const scrollBody = (page, top) => page.evaluate((t) => {
  const body = [...document.querySelectorAll('[data-body] .compare-body')].find((el) => el.getClientRects().length > 0);
  body.scrollTop = t;
}, top);
const visibleTopLine = (page) => page.evaluate(() => {
  const body = [...document.querySelectorAll('[data-body] .compare-body')].find((el) => el.getClientRects().length > 0);
  const stage = body.querySelector('.compare-stage');
  const root = stage.querySelector('diffs-container').shadowRoot;
  const top = body.getBoundingClientRect().top;
  const rows = [...root.querySelectorAll('[data-line]')].map((el) => ({
    line: Number(el.getAttribute('data-line')), type: el.getAttribute('data-line-type'), top: el.getBoundingClientRect().top,
  })).filter((r) => r.top >= top - 19 && r.top <= top + 19);
  return rows[0] ? { line: rows[0].line, type: rows[0].type, offset: rows[0].top - top } : null;
});
const assertSameTopLine = async (page, expected, message) => {
  const actual = await visibleTopLine(page);
  assert.ok(actual && expected && actual.line === expected.line && actual.type === expected.type && Math.abs(actual.offset - expected.offset) <= 19,
    `${message}: topo antes ${JSON.stringify(expected)}, depois ${JSON.stringify(actual)}`);
};
// Cabeçalhos `@@` do patch lido: a ordem é a numeração da revisão (o índice do bloco).
const headersOf = (review) => review.diff.patch.split('\n').filter((l) => l.startsWith('@@'));
// Não há sobreposição: o centro do botão cai nele (ou num filho), e a caixa dele fica dentro do contêiner pedido.
async function visibleAndClear(locator, container, label) {
  const ok = await locator.evaluate((el, boxSel) => {
    const r = el.getBoundingClientRect();
    // Desabilitado tem pointer-events: none, então o ponto cai no pai: a sobreposição só vale para o que recebe clique.
    const top = el.disabled ? el : document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    const box = (boxSel === 'parent' ? el.parentElement : el.closest(boxSel))?.getBoundingClientRect();
    const inside = !box || (r.left >= box.left - 0.5 && r.right <= box.right + 0.5 && r.top >= box.top - 0.5 && r.bottom <= box.bottom + 0.5);
    return { hit: !!top && (el === top || el.contains(top)), inside, width: r.width, name: el.getAttribute('aria-label') || el.textContent.trim() };
  }, container);
  assert.ok(ok.width > 0, `${label}: sem largura`);
  assert.ok(ok.hit, `${label}: outro elemento cobre o botão "${ok.name}"`);
  assert.ok(ok.inside, `${label}: "${ok.name}" saiu da barra`);
}

// Toolbar do comparador numa linha: 28 px de altura, cada item dentro da faixa e nada passando da largura (a sobra vai para o caminho).
async function toolbarOneLine(page, label) {
  const bar = await page.locator('[data-body]:visible .compare-toolbar').first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    const inside = [...el.children].every((c) => {
      const k = c.getBoundingClientRect();
      return k.top >= r.top - 0.5 && k.bottom <= r.bottom + 0.5 && k.right <= r.right + 0.5;
    });
    return { height: r.height, inside, overflow: el.scrollWidth - el.clientWidth };
  });
  assert.ok(bar.height <= 30, `${label}: toolbar com ${bar.height}px de altura`);
  assert.ok(bar.inside, `${label}: algum item da toolbar quebrou para fora da linha`);
  assert.ok(bar.overflow <= 0.5, `${label}: toolbar estoura ${bar.overflow}px`);
}

// Texto de conflito de 2 trechos, com texto fora dos trechos (topo, meio e fim).
const DOIS = 'topo\n<<<<<<< ours\natual-1\n=======\nrecebida-1\n>>>>>>> theirs\nmeio\n<<<<<<< ours\natual-2\n=======\nrecebida-2\n>>>>>>> theirs\nfim\n';
const DOIS_SIDES = { base: 'topo\nmeio\nfim\n', ours: 'topo\natual-1\nmeio\natual-2\nfim\n', theirs: 'topo\nrecebida-1\nmeio\nrecebida-2\nfim\n' };
const addDois = (page) => page.evaluate(([t, s]) => window.__mock.addConflict('src/dois.ts', s, t), [DOIS, DOIS_SIDES]);
const conflictTab = (page, path = 'src/dois.ts') => tab(page, `${path} (conflito)`);
// Escolhe o lado de um trecho na lista (Atual, Recebida ou Ambas), pelo rótulo do trecho.
const choose = (page, n, label) => page.getByText(`Trecho ${n}`, { exact: true }).locator('..').getByRole('button', { name: label, exact: true }).click();
const trechos = (page, n) => page.getByText(n === 1 ? /^1 trecho com conflito/ : new RegExp(`^${n} trechos com conflito`));

// ---------- Git: diff (arquivo inteiro com os blocos do Git) ----------

// Arquivo longo do simulado (mock.ts, GIT_TEXT): na pasta, a linha 28 e a 32 formam o bloco 1 (com 29 a 31 no meio) e a 96 é o bloco 2.
// No preparado, a linha 62 é um bloco. O arquivo tem 200 linhas: há linhas de contexto antes, entre e depois das mudanças.
const EDIT = 'src/editor-workspace.tsx';
const LINES_OF_INTEREST = [1, 27, 29, 45, 60, 95, 97, 150, 200];

scenario('diff: arquivo inteiro num comparador só; preparar, tirar e descartar um bloco com o payload visto; Cancelar preserva; descarte bloqueado com rascunho; não rastreado só prepara o arquivo', async ({ page }) => {
  await openGit(page);
  await row(page, 'staged', EDIT).dblclick();
  await row(page, 'unstaged', EDIT).dblclick();
  const titles = await page.locator('[data-tab] [role="tab"]').evaluateAll((els) => els.map((el) => el.getAttribute('title')));
  assert.ok(titles.includes(`${EDIT} (preparadas)`) && titles.includes(`${EDIT} (alterações)`), `abas: ${titles}`);

  // Alterações: dois blocos; o primeiro é o atual ao abrir, e a linha dele é a 28 (a mudança real, não o início do bloco).
  await view(page).waitFor();
  assert.equal(await counterOf(page), 'bloco 1 de 2');
  const seen = await lastReview(page, EDIT, false);
  const heads = headersOf(seen);
  assert.equal(heads.length, 2, `dois blocos na pasta: ${heads}`);
  // A marca do bloco 1 está no topo da linha 28 que o motor desenhou (a medida real, não uma conta).
  const first = await current(page);
  assert.deepEqual({ unit: first.unit, line: first.line, rowType: first.rowType }, { unit: 0, line: 28, rowType: 'change-deletion' }, 'o bloco 1 começa na linha 28');
  assert.ok(Math.abs(first.top - (await engineLine(page, 28, 'change-deletion')).top) <= 1, `a marca do bloco 1 em ${first.top}px fora da linha 28 do motor`);

  // Preparar o bloco 1: o payload é o bloco visto, com a revisão lida; sobra o bloco 2, que vira o atual, na linha 96.
  await tb(page, 'Preparar bloco 1 de 2').click();
  await waitCount(page, 'git_stage_hunk', 1);
  const stage = (await calls(page, 'git_stage_hunk')).at(-1);
  assert.deepEqual(stage.args.hunk, { path: EDIT, index: 0, header: heads[0], revision: seen.revision });
  await waitCounter(page, 'bloco 1 de 1');
  assert.equal((await current(page)).line, 96, 'o bloco que sobrou é o da linha 96');

  // Preparadas: o bloco da linha 28 e o da 62 (que já estava preparado). Tirar do preparo o primeiro.
  await tab(page, '(preparadas)').click();
  await waitCounter(page, 'bloco 1 de 2');
  const staged = await lastReview(page, EDIT, true);
  const sheads = headersOf(staged);
  assert.equal((await current(page)).line, 28);
  await tb(page, 'Tirar bloco 1 de 2 do preparo').click();
  await waitCount(page, 'git_unstage_hunk', 1);
  assert.deepEqual((await calls(page, 'git_unstage_hunk')).at(-1).args.hunk, { path: EDIT, index: 0, header: sheads[0], revision: staged.revision });
  await waitCounter(page, 'bloco 1 de 1');

  // Alterações: de volta, dois blocos. Próximo vai ao segundo; Cancelar não muda nada; Descartar manda só esse bloco visto.
  await tab(page, '(alterações)').click();
  await waitCounter(page, 'bloco 1 de 2');
  await tb(page, 'Próximo bloco').click();
  await waitCounter(page, 'bloco 2 de 2');
  const before = await lastReview(page, EDIT, false);
  await tb(page, 'Descartar bloco 2 de 2').click();
  await dialog(page, 'Descartar este bloco?').waitFor();
  await cancelBtn(page, 'Descartar este bloco?').click();
  await dialog(page, 'Descartar este bloco?').waitFor({ state: 'detached' });
  assert.equal((await calls(page, 'git_discard_hunk')).length, 0, 'Cancelar não chama o descarte');
  await tb(page, 'Descartar bloco 2 de 2').click();
  await dialogBtn(page, 'Descartar este bloco?', 'Descartar').click();
  await waitCount(page, 'git_discard_hunk', 1);
  const discard = (await calls(page, 'git_discard_hunk')).at(-1);
  assert.deepEqual(discard.args.hunk, { path: EDIT, index: 1, header: headersOf(before)[1], revision: before.revision });
  await waitCounter(page, 'bloco 1 de 1');
  assert.equal((await current(page)).line, 28, 'o bloco da linha 28 segue na pasta e é o atual');

  // Rascunho aberto no arquivo: descartar (bloco e arquivo) fica desabilitado.
  await openRoute(page, 'Arquivos');
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type('// rascunho');
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor();
  await openGit(page);
  await row(page, 'unstaged', 'src/App.tsx').dblclick();
  await waitCounter(page, 'bloco 1 de 1');
  assert.ok(await tb(page, 'Descartar bloco 1 de 1').isDisabled(), 'descartar bloco bloqueado com rascunho');
  assert.ok(await tb(page, 'Descartar arquivo').isDisabled(), 'descartar arquivo bloqueado com rascunho');
  assert.equal((await calls(page, 'git_discard_hunk')).length, 1, 'nenhum descarte novo com rascunho');
  await page.screenshot({ path: join(OUT, 'git-ui-bloco-rascunho.png') });

  // Não rastreado: o comparador mostra o arquivo novo inteiro, sem blocos; a ação é preparar o arquivo.
  await page.evaluate(() => window.__mock.external('docs/rascunho-painel.md', 'primeira\nsegunda\n'));
  await row(page, 'unstaged', 'docs/rascunho-painel.md').dblclick();
  await page.getByText('arquivo novo, não rastreado').waitFor();
  assert.equal(await bar(page).getByRole('button', { name: /bloco/ }).count(), 0, 'não rastreado não tem ação por bloco');
  const untracked = await lastReview(page, 'docs/rascunho-painel.md', false);
  assert.ok(untracked.diff.patch.includes('new file mode'), 'o patch do não rastreado é o arquivo inteiro');
  assert.equal(untracked.untracked, true);
  assert.ok((await engineRows(page)).some((r) => r.line === 1), 'o arquivo novo aparece desde a linha 1');
  await tb(page, 'Preparar arquivo').click();
  await waitCount(page, 'git_stage', 1);
  assert.deepEqual((await calls(page, 'git_stage')).at(-1).args.paths, ['docs/rascunho-painel.md']);
  await row(page, 'staged', 'docs/rascunho-painel.md').waitFor();
});

// Layout: 1920 e 1040 com dois grupos. As ações da barra são acessíveis, inteiras e sem sobreposição; o arquivo entra no grupo ativo.
scenario('diff em 1920px e em 1040px com 2 grupos: ações da barra com nome inteiro, dentro da barra e sem sobreposição', async ({ page }) => {
  await openGit(page);
  await row(page, 'unstaged', EDIT).dblclick();
  await view(page).waitFor();
  const actions = ['Bloco anterior', 'Próximo bloco', 'Preparar bloco 1 de 2', 'Abrir bloco 1 de 2 no arquivo', 'Descartar bloco 1 de 2', 'Descartar arquivo'];
  await page.screenshot({ path: join(OUT, 'git-ui-diff-1920.png') });
  for (const name of [...actions, /^(Unificado|Lado a lado)$/]) await visibleAndClear(tb(page, name), 'parent', `1920 ${String(name)}`);

  // Dois grupos: o arquivo abre à esquerda e se divide para a direita; o diff entra no grupo ativo.
  await openRoute(page, 'Arquivos');
  await expand(page, ['src']);
  await treeItem(page, 'editor-workspace.tsx').dblclick();
  await tab(page, 'src/editor-workspace.tsx').first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Dividir à Direita/ }).click();
  await page.locator('[data-group="B"]').waitFor();
  await openGit(page);
  await row(page, 'unstaged', EDIT).dblclick();
  await view(page).waitFor();
  await page.setViewportSize({ width: 1040, height: 800 });
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(OUT, 'git-ui-diff-1040.png') });
  for (const name of [...actions, /^(Unificado|Lado a lado)$/]) await visibleAndClear(tb(page, name), 'parent', `1040 ${String(name)}`);
  await toolbarOneLine(page, '1040px, 2 grupos');
  const groups = await page.locator('[data-group]').count();
  assert.equal(groups, 2, 'os dois grupos estão visíveis em 1040px');
});

// O comparador é um arquivo só: as linhas antes, entre e depois das mudanças estão lá, lado a lado por padrão, e o Unificado troca e volta.
scenario('comparador: o arquivo inteiro numa visão só, com linhas antes, entre e depois das mudanças; lado a lado por padrão; Unificado troca e volta', async ({ page }) => {
  await openGit(page);
  await row(page, 'unstaged', EDIT).dblclick();
  await view(page).waitFor();
  assert.equal(await page.locator('[data-body]:visible .compare-stage diffs-container').count(), 1, 'um só motor de diff para o arquivo');
  const modes = () => page.locator('[data-body]:visible pre[data-diff-type]').evaluateAll((els) => els.map((el) => el.getAttribute('data-diff-type')));
  const initial = await modes();
  assert.ok(initial.length > 0 && initial.every((m) => m === 'split'), `padrão lado a lado: ${initial}`);
  const split = await engineRows(page);
  for (const n of LINES_OF_INTEREST) assert.ok(split.some((r) => r.line === n), `linha ${n} sumiu do arquivo inteiro (lado a lado)`);
  assert.equal(split.length, 400, '200 linhas nos dois lados, sem nenhuma linha escondida');
  assert.equal(await page.locator('[data-body]:visible .compare-stage [data-separator], [data-body]:visible .compare-stage [data-separator-wrapper]').count(), 0, 'sem separador entre as mudanças');
  await visibleAndClear(tb(page, 'Unificado'), 'parent', '1920 Unificado');
  await page.screenshot({ path: join(OUT, 'git-ui-comparador-lado-a-lado.png') });
  await tb(page, 'Unificado').click();
  assert.ok((await modes()).every((m) => m === 'single'), 'Unificado (single)');
  const unified = await engineRows(page);
  for (const n of LINES_OF_INTEREST) assert.ok(unified.some((r) => r.line === n), `linha ${n} sumiu do arquivo inteiro (unificado)`);
  await page.screenshot({ path: join(OUT, 'git-ui-comparador-unificado.png') });
  await tb(page, 'Lado a lado').click();
  assert.ok((await modes()).every((m) => m === 'split'), 'Lado a lado volta ao split');
});

// Navegação: Próximo e Anterior vão à linha real da mudança (não ao começo de um container); F8 e Shift+F8; um clique numa mudança e a marca
// da faixa também escolhem a unidade. O limite desabilita os dois lados.
scenario('comparador: Próximo e Anterior vão à linha real da mudança; F8 e Shift+F8; clique na mudança e marca da faixa escolhem o bloco', async ({ page }) => {
  await openGit(page);
  await row(page, 'unstaged', EDIT).dblclick();
  await view(page).waitFor();
  assert.equal(await counterOf(page), 'bloco 1 de 2');
  const prev = tb(page, 'Bloco anterior');
  const next = tb(page, 'Próximo bloco');
  assert.ok(await prev.isDisabled(), 'Anterior desabilitado no primeiro bloco');
  await next.click();
  await waitCounter(page, 'bloco 2 de 2');
  assert.deepEqual(await current(page), { unit: 1, top: (await current(page)).top, line: 96, rowType: 'change-deletion' }, 'Próximo vai à linha 96');
  assert.ok(await next.isDisabled(), 'Próximo desabilitado no último bloco');
  await page.keyboard.press('Shift+F8');
  await waitCounter(page, 'bloco 1 de 2');
  assert.equal((await current(page)).line, 28, 'Shift+F8 volta à linha 28');
  await page.keyboard.press('F8');
  await waitCounter(page, 'bloco 2 de 2');
  // Com o bloco 1 atual, um clique na linha 96 (do bloco 2) escolhe o bloco 2; depois, um clique na 28 escolhe o bloco 1.
  await page.keyboard.press('Shift+F8');
  await waitCounter(page, 'bloco 1 de 2');
  await reveal(page, 96, 'change-deletion');
  const r96 = (await engineRows(page)).find((r) => r.line === 96 && r.type === 'change-deletion');
  assert.ok(r96 && r96.inView, `a linha 96 aparece na tela: ${JSON.stringify(r96)}`);
  await page.mouse.click(r96.x + 60, r96.y);
  await waitCounter(page, 'bloco 2 de 2');
  await reveal(page, 28, 'change-deletion');
  const r28 = (await engineRows(page)).find((r) => r.line === 28 && r.type === 'change-deletion');
  assert.ok(r28 && r28.inView, `a linha 28 aparece na tela: ${JSON.stringify(r28)}`);
  await page.mouse.click(r28.x + 60, r28.y);
  await waitCounter(page, 'bloco 1 de 2');
  // A segunda marca da faixa leva ao bloco 2, e o atual passa a ser o da linha 96.
  await view(page).locator('.compare-mark').nth(1).click();
  await waitCounter(page, 'bloco 2 de 2');
  assert.equal((await current(page)).line, 96, 'a marca 2 leva à linha 96');
  // Unificado: o bloco atual vai à linha da remoção, que é a primeira linha dele nesse modo.
  await tb(page, 'Unificado').click();
  await tb(page, 'Bloco anterior').click();
  await waitCounter(page, 'bloco 1 de 2');
  assert.deepEqual(await current(page), { unit: 0, top: (await current(page)).top, line: 28, rowType: 'change-deletion' }, 'unificado: o bloco 1 começa na remoção da linha 28');
  await page.screenshot({ path: join(OUT, 'git-ui-comparador-navegacao.png') });
});

// Fixture src/sem-quebra.ts (mock): 150 linhas em três blocos — 4 linhas a mais no começo, uma linha trocada por duas no meio e a última
// trocada, sem quebra de linha no fim (o aviso do motor). A marca, o clique e a rolagem vão à linha real que o motor desenhou, não a uma
// proporção: o topo da marca bate com o topo da linha do motor.
const SEM_QUEBRA = 'src/sem-quebra.ts';
// Linha de mudança como o motor a desenha: o número (antigo na remoção, novo na inclusão), o tipo e a posição real na tela do comparador.
const engineLine = async (page, line, type) => {
  const found = (await engineRows(page)).find((r) => r.line === line && r.type === type);
  assert.ok(found, `a linha ${line} (${type}) está no motor`);
  return found;
};
// Rola até a linha (com uma folga acima) e devolve a posição dela depois da rolagem.
async function reveal(page, line, type) {
  await scrollBody(page, (await engineLine(page, line, type)).top - 100);
  return engineLine(page, line, type);
}
// A unidade atual é a `n`, a marca está no topo da linha real (tolerância de 1 px) e a linha aparece inteira.
async function atRealLine(page, n, line, type) {
  await waitCounter(page, `bloco ${n} de 3`);
  const cur = await current(page);
  const real = await engineLine(page, line, type);
  assert.equal(cur.unit, n - 1, `unidade atual é o bloco ${n}`);
  assert.ok(Math.abs(cur.top - real.top) <= 1, `bloco ${n}: marca em ${cur.top}px, linha real ${line} em ${real.top}px`);
  assert.ok(real.inView, `bloco ${n}: a linha ${line} está visível`);
}
scenario('comparador com inserção desigual e sem quebra de linha no fim: marca, Próximo, F8, clique e faixa na linha real; polling igual não rola', async ({ page }) => {
  await openGit(page);
  await row(page, 'unstaged', SEM_QUEBRA).dblclick();
  await view(page).waitFor();
  // O motor desenha depois do primeiro render: o aviso de fim sem quebra é uma linha auxiliar que aparece quando ele termina.
  const notes = page.locator('[data-body]:visible .compare-stage [data-no-newline]');
  await notes.waitFor();
  assert.equal(await notes.count(), 1, 'o motor mostra o aviso de fim sem quebra de linha');
  // Lado a lado (o padrão): o primeiro bloco é o atual ao abrir, e Próximo, F8 e Shift+F8 vão às linhas reais.
  await atRealLine(page, 1, 3, 'change-addition');
  await tb(page, 'Próximo bloco').click();
  await atRealLine(page, 2, 140, 'change-deletion');
  await tb(page, 'Próximo bloco').click();
  await atRealLine(page, 3, 150, 'change-deletion');
  await page.keyboard.press('Shift+F8');
  await atRealLine(page, 2, 140, 'change-deletion');
  await page.keyboard.press('F8');
  await atRealLine(page, 3, 150, 'change-deletion');
  // Clique no lado da inclusão do bloco 1 escolhe o bloco 1; a marca 2 da faixa leva ao bloco 2.
  const r3 = await reveal(page, 3, 'change-addition');
  await page.mouse.click(r3.x + 60, r3.y);
  await atRealLine(page, 1, 3, 'change-addition');
  await view(page).locator('.compare-mark').nth(1).click();
  await atRealLine(page, 2, 140, 'change-deletion');
  // Unificado: as mesmas unidades nas linhas desse modo. Clique numa remoção do bloco 2 escolhe o bloco 2 a partir do bloco 1.
  await tb(page, 'Unificado').click();
  await atRealLine(page, 2, 140, 'change-deletion');
  await tb(page, 'Bloco anterior').click();
  await atRealLine(page, 1, 3, 'change-addition');
  const r140 = await reveal(page, 140, 'change-deletion');
  await page.mouse.click(r140.x + 60, r140.y);
  await atRealLine(page, 2, 140, 'change-deletion');
  // Polling: a consulta do diff roda a cada 2,5 s. Uma resposta igual não move a rolagem.
  await scrollBody(page, 300);
  const polls = (await calls(page, 'git_review_diff')).length;
  await page.waitForTimeout(3200);
  assert.ok((await calls(page, 'git_review_diff')).length > polls, 'a consulta do diff rodou durante a espera');
  assert.equal(await page.evaluate(() => [...document.querySelectorAll('[data-body] .compare-body')].find((el) => el.getClientRects().length > 0).scrollTop), 300, 'polling igual não mexe na rolagem');
});

// Divisor do lado a lado (`.compare-sash`): um separador focável no vão de 40 px entre as colunas do motor; a área de pegar tem 8 px e ocupa
// o palco todo, então pega em qualquer ponto da rolagem. Medido nas colunas que o motor desenhou (o grid do split).
const GAP = 40;
const sash = (page) => view(page).locator('.compare-sash');
const scrollOf = (page) => page.evaluate(() => [...document.querySelectorAll('[data-body] .compare-body')].find((el) => el.getClientRects().length > 0).scrollTop);
// Contraste WCAG entre duas cores `rgb(r, g, b)`: (mais clara + 0,05) / (mais escura + 0,05), com a luminância relativa de cada uma.
const luminance = (rgb) => {
  const [r, g, b] = rgb.match(/\d+(\.\d+)?/g).slice(0, 3).map((v) => {
    const c = Number(v) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};
// Mede a proporção nas caixas de texto (as calhas numéricas são trilhas separadas); o vão começa após o texto esquerdo e termina na calha direita.
const splitGeo = (page) => page.evaluate(() => {
  const stage = [...document.querySelectorAll('[data-body] .compare-stage')].find((el) => el.getClientRects().length > 0);
  const root = stage.querySelector('diffs-container').shadowRoot;
  const pre = root.querySelector('pre[data-diff-type]');
  const delText = pre.querySelector(':scope > [data-deletions] > [data-content]').getBoundingClientRect();
  const addGutter = pre.querySelector(':scope > [data-additions] > [data-gutter]').getBoundingClientRect();
  const addText = pre.querySelector(':scope > [data-additions] > [data-content]').getBoundingClientRect();
  const sash = stage.querySelector('.compare-sash').getBoundingClientRect();
  const s = stage.getBoundingClientRect();
  const textWidth = delText.width + addText.width;
  return {
    pct: delText.width / textWidth * 100,
    gap: addGutter.left - delText.right,
    textWidth,
    center: sash.left + sash.width / 2,
    mid: (delText.right + addGutter.left) / 2,
    coversTop: sash.top <= s.top + 0.5,
    coversBottom: sash.bottom >= s.bottom - 0.5,
    stageLeft: s.left,
    stageWidth: s.width,
  };
});
// Ponto do divisor no meio da parte visível do corpo, e se nele o clique cai no divisor (e não no texto do motor).
const sashPoint = (page) => page.evaluate(() => {
  const body = [...document.querySelectorAll('[data-body] .compare-body')].find((el) => el.getClientRects().length > 0).getBoundingClientRect();
  const sash = [...document.querySelectorAll('[data-body] .compare-sash')].find((el) => el.getClientRects().length > 0).getBoundingClientRect();
  const x = sash.left + sash.width / 2;
  const y = Math.max(body.top + 1, Math.min(body.bottom - 1, sash.top + sash.height / 2));
  return { x, y, hit: document.elementFromPoint(x, y)?.classList.contains('compare-sash') === true };
});
// Arrasta o divisor com o mouse até `x` (pixels da tela), em passos, a partir do ponto dele.
async function dragSashTo(page, x) {
  const at = await sashPoint(page);
  assert.ok(at.hit, 'no meio da parte visível, o ponto do divisor é o divisor (não o texto)');
  await page.mouse.move(at.x, at.y);
  await page.mouse.down();
  await page.mouse.move(x, at.y, { steps: 8 });
  await page.mouse.up();
}

scenario('divisor: lado a lado tem um pipe fino no vão de 40 px entre as colunas, na altura do arquivo, em 1920 e 1040; unificado não tem divisor', async ({ page }) => {
  await openGit(page);
  await row(page, 'unstaged', EDIT).dblclick();
  await view(page).waitFor();
  await sash(page).waitFor();
  assert.equal(await sash(page).count(), 1, 'um divisor no lado a lado');
  const aria = await sash(page).evaluate((el) => ({
    role: el.getAttribute('role'), orientation: el.getAttribute('aria-orientation'), label: el.getAttribute('aria-label'), now: el.getAttribute('aria-valuenow'),
  }));
  assert.deepEqual(aria, { role: 'separator', orientation: 'vertical', label: 'Divisor entre o original e o modificado', now: '50' }, 'separador focável, 50% de início');
  const line = await sash(page).evaluate((el) => ({ width: getComputedStyle(el, '::after').width, bg: getComputedStyle(el, '::after').backgroundColor }));
  assert.deepEqual(line, { width: '1px', bg: 'rgb(110, 110, 110)' }, 'pipe de 1 px #6E6E6E em repouso');
  // O pipe tem de se ver contra o fundo do editor (#1F1F1F): contraste WCAG de pelo menos 1,8:1, para não sumir na tela.
  const ratio = contrast(line.bg, 'rgb(31, 31, 31)');
  assert.ok(ratio >= 1.8, `pipe em repouso ${line.bg} contra #1F1F1F: contraste ${ratio.toFixed(2)}:1, não 1,8:1 ou mais`);
  for (const [width, height] of [[1920, 1080], [1040, 800]]) {
    await page.setViewportSize({ width, height });
    await page.waitForTimeout(150);
    const g = await splitGeo(page);
    assert.ok(g.gap >= GAP - 0.5 && g.gap <= GAP + 0.5, `${width}px: vão de ${g.gap}px entre as colunas, não ${GAP}`);
    assert.ok(Math.abs(g.center - g.mid) <= 1, `${width}px: o divisor (${g.center}) fica no vão (${g.mid})`);
    assert.ok(g.coversTop && g.coversBottom, `${width}px: o divisor ocupa a altura do arquivo`);
    const cur = await current(page);
    const real = await engineLine(page, 28, 'change-deletion');
    assert.ok(Math.abs(cur.top - real.top) <= 1, `${width}px: a marca do bloco atual (${cur.top}px) acompanha a linha real (${real.top}px)`);
  }
  await tb(page, 'Unificado').click();
  assert.equal(await sash(page).count(), 0, 'unificado não tem divisor');
  await tb(page, 'Lado a lado').click();
  await sash(page).waitFor();
  assert.equal(await sash(page).count(), 1, 'lado a lado de volta tem o divisor');
});

// Arrastar mantém a mesma linha no topo apesar da reflow e sem refazer o diff (o mesmo elemento do motor); o mínimo de 15% segura; o
// duplo clique volta a 50/50; as setas movem com o foco no divisor, e a medida da linha da mudança atual continua certa.
scenario('divisor: arrastar muda a proporção e mantém a mesma linha no topo; limite de 15% nos dois lados; duplo clique volta a 50/50; setas com foco', async ({ page }) => {
  await openGit(page);
  await row(page, 'unstaged', EDIT).dblclick();
  await view(page).waitFor();
  await waitCounter(page, 'bloco 1 de 2');
  const start = await splitGeo(page);
  assert.ok(Math.abs(start.pct - 50) <= 0.5, `começa em 50/50 (${start.pct})`);
  // Rolado para o meio do arquivo, o divisor continua pegando (ele cobre o palco todo).
  await scrollBody(page, 900);
  const topBeforeDrag = await visibleTopLine(page);
  await page.evaluate(() => { window.__motor = [...document.querySelectorAll('[data-body] .compare-stage')].find((el) => el.getClientRects().length > 0).querySelector('diffs-container'); });
  await dragSashTo(page, start.center + 200);
  const right = await splitGeo(page);
  assert.ok(right.pct > start.pct + 3, `arrastar para a direita aumenta o lado esquerdo (${start.pct} → ${right.pct})`);
  await assertSameTopLine(page, topBeforeDrag, 'arrastar mantém a linha no topo');
  assert.equal(await page.evaluate(() => window.__motor === [...document.querySelectorAll('[data-body] .compare-stage')].find((el) => el.getClientRects().length > 0).querySelector('diffs-container')), true, 'o motor do diff não foi refeito');
  // Mínimo e máximo: o arraste além do limite para no 15% e no 85%.
  await dragSashTo(page, right.stageLeft + 1);
  const topBeforeMax = await visibleTopLine(page);
  const min = await splitGeo(page);
  assert.ok(Math.abs(min.pct - 15) <= 0.5, `limite mínimo: lado esquerdo em ${min.pct}, não 15`);
  await dragSashTo(page, min.stageLeft + min.stageWidth - 1);
  const max = await splitGeo(page);
  assert.ok(Math.abs(max.pct - 85) <= 0.5, `limite máximo: lado esquerdo em ${max.pct}, não 85`);
  for (const g of [min, max]) assert.ok(Math.abs(g.center - g.mid) <= 1, `em ${g.pct.toFixed(1)}%, o pipe (${g.center}) fica no vão (${g.mid})`);
  await assertSameTopLine(page, topBeforeMax, 'arraste até os limites mantém a linha no topo');
  // A largura nova muda a altura das linhas; a marca tem de acompanhar a linha real.
  await scrollBody(page, 0);
  const cur = await current(page);
  const real = await engineLine(page, 28, 'change-deletion');
  assert.equal(cur.unit, 0, 'o bloco 1 segue o atual');
  assert.ok(Math.abs(cur.top - real.top) <= 1, `com o divisor em 85%, a marca em ${cur.top}px e a linha 28 em ${real.top}px`);
  // Duplo clique volta a 50/50 (pelo ponto do divisor, que está no meio da tela).
  const dbl = await sashPoint(page);
  await page.mouse.dblclick(dbl.x, dbl.y);
  await page.waitForTimeout(100);
  const reset = await splitGeo(page);
  assert.ok(Math.abs(reset.pct - 50) <= 0.5, `duplo clique volta a 50/50 (${reset.pct})`);
  // Setas: com o divisor focado, a direita e a esquerda movem 2 pontos percentuais, e o aria acompanha.
  await sash(page).focus();
  const topBeforeKeys = await visibleTopLine(page);
  for (let i = 0; i < 3; i++) await page.keyboard.press('ArrowRight');
  await page.waitForTimeout(100);
  const keyed = await splitGeo(page);
  assert.ok(Math.abs(keyed.pct - 56) <= 0.5, `três setas à direita: ${keyed.pct}, não 56`);
  assert.equal(await sash(page).getAttribute('aria-valuenow'), '56', 'aria-valuenow acompanha a seta');
  await page.keyboard.press('ArrowLeft');
  await page.waitForTimeout(100);
  assert.ok(Math.abs((await splitGeo(page)).pct - 54) <= 0.5, 'seta à esquerda volta 2 pontos');
  await assertSameTopLine(page, topBeforeKeys, 'as setas mantêm a linha no topo');
  // Captura com o divisor arrastado para fora do meio (35%), sem o foco nem o pairar do mouse marcando a linha.
  const to35 = start.center + (35 - start.pct) / 100 * start.textWidth;
  await dragSashTo(page, to35);
  await sash(page).evaluate((el) => el.blur());
  await page.mouse.move(2, 2);
  await page.waitForTimeout(400);
  assert.ok(Math.abs((await splitGeo(page)).pct - 35) <= 0.5, 'o divisor fica em 35% para a captura');
  await page.screenshot({ path: join(OUT, 'git-ui-divisor.png') });
  // Zoom do divisor: 200×300 px em volta dele, na primeira mudança (a unidade atual, com a barra azul) para ver as barras das bordas internas.
  const sashBox = await sash(page).boundingBox();
  const curBox = await view(page).locator('.compare-current').boundingBox();
  const clipY = Math.max(0, Math.min(curBox.y - 120, 1080 - 300));
  await page.screenshot({ path: join(OUT, 'git-ui-divisor-zoom.png'), clip: { x: sashBox.x + sashBox.width / 2 - 100, y: clipY, width: 200, height: 300 } });
});

// No commit, o divisor existe e arrastar muda a proporção (o commit é só leitura, mas a proporção é a mesma).
scenario('divisor: o commit também tem o divisor entre as colunas, e arrastar nele muda a proporção', async ({ page }) => {
  await openGit(page);
  await page.getByRole('button', { name: 'Ver o histórico do projeto', exact: true }).click();
  await page.locator('button[title^="4b2e8d0 · "]').click();
  await page.locator('button[title^="src/lib/utils.ts (renomeado de src/utils.ts)"]').click();
  await tab(page, 'src/lib/utils.ts @ 4b2e8d0').waitFor();
  await view(page).waitFor();
  await sash(page).waitFor();
  assert.equal(await sash(page).count(), 1, 'commit tem o divisor');
  const g = await splitGeo(page);
  assert.ok(g.gap >= GAP - 0.5 && g.gap <= GAP + 0.5 && Math.abs(g.center - g.mid) <= 1, 'o divisor do commit fica no vão entre as colunas');
  await dragSashTo(page, g.center - 200);
  assert.ok((await splitGeo(page)).pct < g.pct - 3, 'arrastar para a esquerda diminui o lado esquerdo no commit');
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
  await page.getByRole('textbox', { name: 'Novo nome' }).press('Enter');
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
// Seção Grafo: a que tem o botão de recolher com esse nome.
const graph = (page) => page.locator('section').filter({ has: page.getByRole('button', { name: /^Grafo\b/ }) });
const graphToggle = (page) => graph(page).getByRole('button', { name: /^Grafo\b/ });
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
  assert.equal(linear, 22, 'commit linear: uma faixa de 11 px mais a folga de 11, como no VS Code');
  assert.equal(merge, 33, 'merge: duas faixas de 11 px mais a folga');
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
  await page.getByRole('button', { name: 'Preparar README.md', exact: true }).click();
  await section(page, 'Preparadas').locator('button[title^="README.md: "]').waitFor();
  await page.getByRole('textbox', { name: /^Mensagem do commit/ }).fill('Commit pelo gráfico');
  await page.getByRole('button', { name: 'Commit', exact: true }).click();
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


// ---------- Git: pasta sem repositório ----------

scenario('pasta sem repositório: o painel traz o texto e o botão, sem contador, branch, gráfico nem Histórico Git; Inicializar chama git_init uma vez e o painel volta ao normal', async ({ page }) => {
  await reopen(page, '?git=no-repo');
  await openGit(page);
  await page.getByText(NO_REPO_TEXT).waitFor();
  await initBtn(page).waitFor();
  await page.screenshot({ path: join(OUT, 'git-ui-sem-repositorio.png') });
  assert.equal(await page.getByRole('heading', { name: 'Controle do Código-Fonte', exact: true }).count(), 1, 'título do painel');
  assert.equal(await page.getByRole('button', { name: 'Atualizar status' }).count(), 0, 'sem ações do painel cheio');
  assert.equal(await page.getByText('Publicar no GitHub').count(), 0, 'sem Publicar no GitHub');
  assert.equal(await nav(page).getByRole('button', { name: 'Git', exact: true }).count(), 1, 'Git na navegação sem contador');
  assert.equal(await page.locator('footer').getByText('main', { exact: true }).count(), 0, 'barra de status sem branch');
  await openFileMenu(page);
  assert.equal(await historyItem(page).count(), 0, 'menu da árvore sem Histórico Git');
  await page.keyboard.press('Escape');
  await openGit(page);
  assert.equal((await calls(page, 'git_graph')).length + (await calls(page, 'git_history')).length, 0, 'gráfico e histórico não consultam o Git');
  await initBtn(page).click();
  await waitCount(page, 'git_init', 1);
  assert.equal((await calls(page, 'git_init')).length, 1, 'git_init chamado uma vez');
  await section(page, 'Alterações').waitFor();
  await page.getByText(NO_REPO_TEXT).waitFor({ state: 'detached' });
  assert.equal(await nav(page).getByRole('button', { name: /^Git \(/ }).count(), 1, 'contador da navegação volta');
  assert.equal(await page.locator('footer').getByText('main', { exact: true }).count(), 1, 'branch na barra de status');
  await openFileMenu(page);
  await historyItem(page).waitFor();
  await page.keyboard.press('Escape');
});

scenario('barra de status: 22 px, branch abre o Controle do Código-Fonte; Sincronizar mostra 0↓ 1↑, recebe e envia uma vez', async ({ page }) => {
  const footer = page.locator('footer');
  const branch = footer.getByRole('button', { name: /^main, branch atual/ });
  await branch.waitFor();
  assert.equal(Math.round((await footer.boundingBox()).height), 22, 'altura da barra');
  const sync = footer.getByRole('button', { name: /^Sincronizar as alterações/ });
  assert.equal((await sync.innerText()).trim(), '0↓ 1↑', 'contadores ao lado do ícone');
  await branch.click();
  await page.getByRole('heading', { name: 'Controle do Código-Fonte', exact: true }).waitFor();
  await sync.click();
  await waitCount(page, 'git_push', 1);
  assert.equal((await calls(page, 'git_pull')).length, 1, 'git_pull uma vez');
  assert.equal((await calls(page, 'git_push')).length, 1, 'git_push uma vez, porque havia commit a enviar');
  await page.screenshot({ path: join(OUT, 'git-ui-barra-de-status.png'), clip: { x: 0, y: (await footer.boundingBox()).y - 10, width: 400, height: 32 } });
});

scenario('Git ausente e repositório acima: cada caso diz o motivo certo, sem botão e sem ações do painel', async ({ page }) => {
  await reopen(page, '?git=no-git');
  await openGit(page);
  await page.getByText('O Git não foi encontrado neste computador. Instale o Git e abra o app de novo.').waitFor();
  await page.screenshot({ path: join(OUT, 'git-ui-sem-git.png') });
  assert.equal(await initBtn(page).count(), 0, 'sem Inicializar');
  assert.equal(await page.getByRole('button', { name: 'Atualizar status' }).count(), 0, 'sem ações do painel');
  await reopen(page, '?git=repo-above');
  await openGit(page);
  await page.getByText('Esta pasta está dentro do repositório C:\\projetos, que não é a raiz do projeto. O painel Git só funciona com a raiz do repositório aberta como projeto.').waitFor();
  assert.equal(await initBtn(page).count(), 0, 'sem Inicializar');
  assert.equal(await page.getByRole('button', { name: 'Atualizar status' }).count(), 0, 'sem ações do painel');
  assert.equal(await page.getByText('Publicar no GitHub').count(), 0, 'sem Publicar no GitHub');
});

scenario('janela do Orquestrador: o aviso sem Git aparece nos modos sem repositório (com a frase de cada um) e não aparece no modo repo', async ({ page }) => {
  const cases = [
    ['?git=no-repo', 'Para ter isso, inicialize o repositório no painel Git.'],
    ['?git=no-git', 'Para ter isso, instale o Git e abra o app de novo.'],
    ['?git=repo-above', 'Para ter isso, abra a raiz do repositório como projeto.'],
  ];
  for (const [query, fix] of cases) {
    await reopen(page, query);
    await page.getByRole('button', { name: 'Abrir terminal', exact: true }).click();
    await dialog(page, 'Orquestrador').getByText(`Esta pasta não tem Git: o que os executores mudarem não fica registrado e não pode ser desfeito pelo Git. ${fix}`).waitFor();
    if (query === '?git=no-repo') await page.screenshot({ path: join(OUT, 'git-ui-aviso-orquestrador.png') });
    await page.keyboard.press('Escape');
    await dialog(page, 'Orquestrador').waitFor({ state: 'detached' });
  }
  await reopen(page);
  await openGit(page);
  await section(page, 'Alterações').waitFor();
  await page.getByRole('button', { name: 'Abrir terminal', exact: true }).click();
  await dialog(page, 'Orquestrador').getByText('Só lê o projeto e delega.').waitFor();
  assert.equal(await dialog(page, 'Orquestrador').getByText(/Esta pasta não tem Git/).count(), 0, 'modo repo sem aviso');
});

scenario('pasta escolhida dentro de um repositório: toast informativo com o caminho do projeto aberto', async ({ page }) => {
  await reopen(page, '?subfolder=1');
  await page.getByText('Esta pasta está dentro do repositório C:\\projetos\\exemplo. O projeto aberto é a raiz dele.').waitFor();
  await openGit(page);
  await section(page, 'Alterações').waitFor();
});

scenario('troca em tempo de execução: o painel vai do estado vazio ao normal (e de volta) sem recarregar a página', async ({ page }) => {
  await openGit(page);
  await section(page, 'Alterações').waitFor();
  await page.evaluate(() => { window.__semRecarregar = 1; window.__mock.gitMode = 'no-repo'; });
  await page.getByText(NO_REPO_TEXT).waitFor();
  assert.equal(await page.locator('footer').getByText('main', { exact: true }).count(), 0, 'sem branch fora do repo');
  await page.evaluate(() => { window.__mock.gitMode = 'repo'; });
  await section(page, 'Alterações').waitFor();
  assert.equal(await page.evaluate(() => window.__semRecarregar), 1, 'a página não recarregou');
});

process.exitCode = await run();
