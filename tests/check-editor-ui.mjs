// QA de interface do editor central, em Chrome headless contra o app simulado (src/backend/mock.ts, só no `vite` de desenvolvimento).
// Cada cenário roda com contexto de navegador próprio (localStorage e clipboard fakes limpos) e interage pela UI. O mock só entra
// na fronteira de I/O: fixtures (`__mock.addFile`), edição externa (`__mock.external`), latência (`__mock.delay`, `__mock.lateRead`) e o disco (`__mock.disk`).
// Loader compartilhado em editor-ui-harness.mjs: Vite na porta 1421 (SMOKE_PORT), encerrado só aqui, e o browser deste teste. Não instala nada.
// Uso: node tests/check-editor-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
// Capturas em .playwright-mcp/editor-ui-*.png: evidência para leitura humana, não asserção.
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { OUT, uiSuite, openApp, expand, treeItem, tabs, tabTitles, editorText, focusEditor, columnPoint, primaryCursor } from './editor-ui-harness.mjs';


// Geometria real da faixa: área visível (scroll), ações do editor à direita, aba ativa e o X dela. Espera até 2 s a rolagem levar a aba
// ativa para dentro da faixa (sob carga, ela chega depois das pausas fixas); se não chegar, devolve a última medida.
async function strip(page, group = 'A') {
  let s;
  for (let i = 0; i < 40; i++) {
    s = await measureStrip(page, group);
    if (!s?.active || (s.active.l >= s.box.l - EPS && s.active.r <= s.box.r + EPS)) break;
    await page.waitForTimeout(50);
  }
  return s;
}
function measureStrip(page, group) {
  return page.evaluate((g) => {
    const box = document.querySelector(`[data-group="${g}"] .overflow-x-auto`);
    if (!box) return null;
    const count = document.querySelector(`[data-group="${g}"] [role="toolbar"][aria-label="Ações de editor"]`);
    const activeWrap = box.querySelector('[role="tab"][aria-selected="true"]')?.closest('[data-tab]') ?? null;
    const rect = (el) => { const b = el.getBoundingClientRect(); return { l: b.left, r: b.right, t: b.top, b: b.bottom, w: b.width }; };
    const x = activeWrap?.querySelector('button[aria-label^="Fechar"]');
    // Nome da aba ativa (o primeiro span de texto do rótulo): mede se ele tem largura útil e não cruza o X.
    const name = activeWrap?.querySelector('[role="tab"] span.min-w-0') ?? null;
    return {
      box: rect(box), count: count ? rect(count) : null, active: activeWrap ? rect(activeWrap) : null,
      activeId: activeWrap?.dataset.tab ?? null, x: x ? rect(x) : null, name: name ? rect(name) : null,
      pageOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    };
  }, group);
}
const EPS = 0.75;
// A aba ativa inteira dentro da área visível da faixa, o X dela dentro da aba e da faixa, o contador fora da faixa e sem rolagem da página.
function assertRevealed(s, label) {
  assert.ok(s, `${label}: faixa não encontrada`);
  assert.ok(s.active, `${label}: sem aba ativa`);
  assert.ok(s.active.l >= s.box.l - EPS && s.active.r <= s.box.r + EPS,
    `${label}: aba ativa ${s.activeId} fora da área visível (aba ${s.active.l.toFixed(1)}–${s.active.r.toFixed(1)}, faixa ${s.box.l.toFixed(1)}–${s.box.r.toFixed(1)})`);
  assert.ok(s.x, `${label}: X da aba ativa ausente`);
  assert.ok(s.x.l >= s.active.l - EPS && s.x.r <= s.active.r + EPS, `${label}: X da aba ativa fora da própria aba`);
  assert.ok(s.x.l >= s.box.l - EPS && s.x.r <= s.box.r + EPS, `${label}: X da aba ativa cortado pela faixa`);
  assert.ok(s.name && s.name.w > 0, `${label}: nome da aba ativa sem largura útil (${s.name ? s.name.w.toFixed(1) : 'ausente'}px)`);
  assert.ok(s.name.l >= s.active.l - EPS && s.name.r <= s.x.l + EPS, `${label}: nome da aba ativa cruza o X (nome ${s.name.l.toFixed(1)}–${s.name.r.toFixed(1)}, X ${s.x.l.toFixed(1)})`);
  if (s.count) assert.ok(s.count.l >= s.box.r - EPS, `${label}: ações do editor sobrepõem a faixa (ações começam em ${s.count.l.toFixed(1)}, faixa acaba em ${s.box.r.toFixed(1)})`);
  assert.ok(s.pageOverflow <= 0, `${label}: rolagem horizontal da página (${s.pageOverflow}px)`);
}



// Ponto na área vazia à direita das abas do grupo: duplo clique ali cria rascunho.
async function emptyStripPoint(page, group = 'A') {
  const b = await page.locator(`[data-group="${group}"] [role="tablist"]`).boundingBox();
  return { x: b.x + b.width - 20, y: b.y + b.height / 2 };
}

// ---------- cenários (cada um com fixtures próprias; nenhum depende de estado de outro) ----------

const scenario = uiSuite('editor-ui');

scenario('formatar JSON pelo menu cria uma edição só e Shift+Alt+F formata o editor', async ({ page }) => {
  const source = '{"z":1,"a":[1,2]}';
  await page.evaluate((text) => window.__mock.addFile('format.json', text), source);
  await treeItem(page, 'format.json').dblclick();
  await focusEditor(page);
  const before = await editorText(page);
  await page.locator('.cm-content:visible').first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Formatar Documento/ }).click();
  await page.waitForFunction((old) => document.querySelector('.cm-content')?.textContent !== old, before);
  const formatted = await page.locator('.cm-content:visible').first().evaluate((el) => el.innerText);
  assert.ok(formatted.includes('"z": 1') && formatted.includes('"a": ['), 'JSON formatado pelo menu: ' + formatted);

  await page.keyboard.press('Control+z');
  await page.waitForFunction((old) => document.querySelector('.cm-content')?.textContent === old, before);
  await page.keyboard.press('Shift+Alt+f');
  await page.waitForFunction((old) => document.querySelector('.cm-content')?.textContent !== old, before);
});

scenario('faixa de abas em 1040px: aba ativa visível com 1 e 2 grupos, nomes repetidos e longos, X e contador sem sobreposição', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFile('src/lib/index.ts', 'export {};\n');
    window.__mock.addFile('tests/index.ts', '// teste\n');
    window.__mock.addFile('docs/relatorio-com-nome-bem-longo-para-a-faixa-de-abas-estreita.md', '# relatório\n');
  });
  await expand(page, ['src', 'lib']);
  await treeItem(page, 'index.ts').waitFor();
  await expand(page, ['tests', 'docs']);
  await treeItem(page, 'index.ts', 1).waitFor();
  const long = 'relatorio-com-nome-bem-longo-para-a-faixa-de-abas-estreita.md';
  await treeItem(page, long).waitFor();
  for (const name of ['App.tsx', 'index.css', 'longo.ts', 'main.tsx', 'conflito.ts']) await treeItem(page, name).dblclick();
  await treeItem(page, 'index.ts', 0).dblclick();
  await treeItem(page, 'index.ts', 1).dblclick();
  await treeItem(page, long).dblclick();
  assert.equal(await tabs(page).count(), 8, `8 abas abertas (${JSON.stringify(await tabTitles(page))})`);
  const titles = await tabTitles(page);
  assert.equal(titles.filter((t) => t.includes('index.ts')).length, 2, 'dois index.ts abertos');
  assert.ok(titles.some((t) => t.includes('src/lib')) && titles.some((t) => t.includes('tests')), `pasta vira detalhe nos nomes repetidos (${JSON.stringify(titles)})`);

  await page.setViewportSize({ width: 1040, height: 800 });
  await page.waitForTimeout(200);
  assertRevealed(await strip(page), '1 grupo, 1040px, aba ativa (última aberta)');

  // Clicar na primeira aba (escondida) a revela e a torna ativa.
  await tabs(page).first().click();
  await page.waitForTimeout(150);
  const first = await strip(page);
  assertRevealed(first, '1 grupo, 1040px, após clicar na primeira aba');
  assert.ok(first.active.w <= 224 + EPS, `aba maior que 14rem (${first.active.w.toFixed(1)}px)`);

  // Sujar a aba ativa muda a largura dela: continua revelada.
  await focusEditor(page);
  await page.keyboard.press('End');
  await page.keyboard.type('x');
  await page.waitForTimeout(150);
  assertRevealed(await strip(page), '1 grupo, 1040px, aba ativa suja');
  await page.screenshot({ path: join(OUT, 'editor-ui-narrow-one-group.png') });

  // Dividir: dois grupos, cada um com a sua largura mínima. A aba ativa e o nome de cada um ficam inteiros na própria faixa.
  await page.keyboard.press('Control+Backslash');
  await page.locator('[data-group="B"]').waitFor();
  await page.waitForTimeout(200);
  assertRevealed(await strip(page, 'A'), '2 grupos, 1040px, grupo A');
  assertRevealed(await strip(page, 'B'), '2 grupos, 1040px, grupo B');
  // Medidas: a faixa da árvore (A.x − Nav) cede até 200 e o painel do Orquestrador até 320; cada grupo fica com mais de 200 px.
  const narrow = { A: await page.locator('[data-pane="A"]').boundingBox(), B: await page.locator('[data-pane="B"]').boundingBox() };
  assert.ok(narrow.A.width >= 200 && narrow.B.width >= 200, `2 grupos a 1040px com largura útil (A ${narrow.A.width}, B ${narrow.B.width})`);
  assert.ok(Math.abs(narrow.A.x - 44 - 200) <= 2, `faixa da árvore cedeu até 200 (A começa em ${narrow.A.x})`);
  await page.screenshot({ path: join(OUT, 'editor-ui-narrow-two-groups.png') });

  // Janela larga: a faixa volta ao tamanho preferido (280) e os dois grupos ficam mais largos.
  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.waitForTimeout(300);
  const wide = { A: await page.locator('[data-pane="A"]').boundingBox() };
  assert.ok(Math.abs(wide.A.x - 44 - 280) <= 2, `faixa volta ao preferido 280 ao crescer (A começa em ${wide.A.x})`);
  assertRevealed(await strip(page, 'A'), '2 grupos, 1920px, grupo A');
  assertRevealed(await strip(page, 'B'), '2 grupos, 1920px, grupo B');
  await page.screenshot({ path: join(OUT, 'editor-ui-wide-two-groups.png') });
});

scenario('só o grupo B (aba do A movida/fechada) + Abrir à Direita e Dividir: o novo editor entra à direita, sem perder o arquivo', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+Backslash');
  await page.locator('[data-pane="B"] .cm-content').waitFor();
  // Fecha a aba do A: sobra só o grupo B.
  await page.locator('[data-group="A"] [data-tab]').first().getByRole('button', { name: /^Fechar/ }).click();
  await page.waitForFunction(() => !document.querySelector('[data-group="A"]'));
  assert.equal(await page.locator('[data-pane="A"]').count(), 0, 'só o grupo B existe');

  // Dividir com o foco no B: o segundo editor (A) entra à direita, com o mesmo arquivo e o texto dele.
  await focusEditor(page);
  await page.keyboard.press('Control+Backslash');
  await page.locator('[data-pane="A"] .cm-content').waitFor();
  const leftB = (await page.locator('[data-pane="B"]').boundingBox()).x;
  const rightA = (await page.locator('[data-pane="A"]').boundingBox()).x;
  assert.ok(leftB < rightA, `dividir com só o B: B à esquerda, A à direita (B ${leftB}, A ${rightA})`);
  assert.deepEqual(await tabTitles(page, 'A'), await tabTitles(page, 'B'), 'Dividir: os dois grupos têm a mesma aba');
  assert.equal(await page.locator('[data-pane="A"] .cm-content').textContent(), await page.locator('[data-pane="B"] .cm-content').textContent(), 'Dividir: mesmo texto');

  // Volta a um grupo só (fecha o A) e abre main.tsx à direita, com o foco no B: a árvore põe o arquivo no segundo grupo, à direita.
  await page.locator('[data-group="A"] [data-tab]').first().getByRole('button', { name: /^Fechar/ }).click();
  await page.waitForFunction(() => !document.querySelector('[data-group="A"]'));
  await treeItem(page, 'main.tsx').click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Abrir à Direita' }).click();
  await page.locator('[data-pane="A"]').waitFor();
  assert.deepEqual(await tabTitles(page, 'B'), ['App.tsx'], 'o grupo B continua com App.tsx');
  // Clique simples na árvore abre prévia: o texto da aba traz "(prévia)" junto, por isso compara só o começo.
  const newTabs = await tabTitles(page, 'A');
  assert.equal(newTabs.length, 1, 'Abrir à Direita: uma aba no grupo novo');
  assert.ok(newTabs[0].startsWith('main.tsx'), `Abrir à Direita: main.tsx no grupo novo (${newTabs[0]})`);
  const pb = await page.locator('[data-pane="B"]').boundingBox();
  const pa = await page.locator('[data-pane="A"]').boundingBox();
  assert.ok(pb.x < pa.x, `Abrir à Direita com só o B: novo grupo à direita (B ${pb.x}, A ${pa.x})`);
  assertRevealed(await strip(page, 'A'), 'após Abrir à Direita, grupo A');
  assertRevealed(await strip(page, 'B'), 'após Abrir à Direita, grupo B');

  // Dois grupos com o foco no da direita (A): Abrir à Direita usa o segundo na ordem visual, que é o próprio A, e não o da esquerda.
  await page.locator('[data-pane="A"] .cm-content').click();
  await treeItem(page, 'README.md').click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Abrir à Direita' }).click();
  await page.waitForFunction(() => [...document.querySelectorAll('[data-group="A"] [data-tab]')].some((t) => t.textContent.includes('README.md')));
  assert.ok((await tabTitles(page, 'A')).some((t) => t.startsWith('README.md')), 'Abrir à Direita com foco na direita: README entra no grupo da direita');
  assert.deepEqual(await tabTitles(page, 'B'), ['App.tsx'], 'Abrir à Direita com foco na direita: o grupo da esquerda fica como estava');
});

scenario('atalhos de arquivo: Ctrl+K W, Ctrl+K Shift+Enter, Ctrl+K Ctrl+Shift+C, Shift+Alt+C, prefixo de 1,5 s, tecla sem acorde vai ao editor', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await treeItem(page, 'main.tsx').dblclick();
  assert.equal(await tabs(page).count(), 2, 'duas abas permanentes');
  const pinnedOf = (i) => tabs(page).nth(i).locator('.sr-only', { hasText: '(fixada)' }).count();

  // Fixa a App (aba 0) com Ctrl+K Shift+Enter, focando o editor dela.
  await tabs(page).nth(0).click();
  await focusEditor(page);
  await page.keyboard.press('Control+k');
  await page.keyboard.press('Shift+Enter');
  await page.waitForFunction(() => document.querySelector('[data-group="A"] [data-tab] .sr-only')?.textContent === '(fixada)');
  assert.equal(await pinnedOf(0), 1, 'App fixada por Ctrl+K Shift+Enter');

  // Ctrl+K W fecha só a não fixada (main); a fixada fica.
  await tabs(page).nth(0).click();
  await focusEditor(page);
  await page.keyboard.press('Control+k');
  await page.keyboard.press('w');
  await page.waitForFunction(() => document.querySelectorAll('[data-group="A"] [data-tab]').length === 1);
  assert.ok((await tabTitles(page))[0].includes('App.tsx'), 'Ctrl+K W deixou só a fixada (App.tsx)');

  // Copiar relativo e absoluto, pelo clipboard fake.
  await focusEditor(page);
  await page.keyboard.press('Control+k');
  await page.keyboard.press('Control+Shift+C');
  await page.waitForFunction(() => window.__copied.length >= 1);
  assert.equal(await page.evaluate(() => window.__copied.at(-1)), 'src\\App.tsx', 'Ctrl+K Ctrl+Shift+C copia o relativo');
  await focusEditor(page);
  await page.keyboard.press('Shift+Alt+C');
  await page.waitForFunction(() => window.__copied.length >= 2);
  assert.equal(await page.evaluate(() => window.__copied.at(-1)), 'C:\\projetos\\exemplo\\src\\App.tsx', 'Shift+Alt+C copia o absoluto');

  // Prefixo expira em 1,5 s: depois da espera, Shift+Enter não desafixa (o acorde já caiu).
  await focusEditor(page);
  await page.keyboard.press('Control+k');
  await page.waitForTimeout(1700);
  await page.keyboard.press('Shift+Enter');
  await page.waitForTimeout(150);
  assert.equal(await pinnedOf(0), 1, 'prefixo expirado não desafixa');

  // Ctrl+K seguido de tecla sem ação: a tecla vai para o editor (entra o Q) e o texto da linha não é apagado pelo Ctrl+K.
  const before = await editorText(page);
  await focusEditor(page);
  await page.keyboard.press('Home');
  await page.keyboard.press('Control+k');
  await page.keyboard.press('Q');
  await page.waitForTimeout(150);
  const after = await editorText(page);
  assert.ok(after.includes('Q'), 'a tecla após Ctrl+K chegou ao editor');
  assert.ok(after.length > before.length - 1, `Ctrl+K não apagou o texto da linha (antes ${before.length}, depois ${after.length})`);
});

scenario('grupo vazio: fechar a última aba de um lado mostra o outro; recuperação com só o grupo B', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await focusEditor(page);
  // Dividir cria a cópia no B; fechar a cópia deixa só o A, que ocupa a área inteira.
  await page.keyboard.press('Control+Backslash');
  await page.locator('[data-pane="B"] .cm-content').waitFor();
  await page.locator('[data-group="B"] [data-tab]').first().getByRole('button', { name: /^Fechar/ }).click();
  await page.locator('[data-group="B"]').waitFor({ state: 'detached' });
  const alone = await page.locator('.cm-editor:visible').first().boundingBox();
  assert.ok(alone && alone.width > 300 && alone.height > 100, `grupo A ocupa a área inteira (${JSON.stringify(alone)})`);

  // Dividir de novo e fechar a aba do A: o A fica vazio e some. Só o B sobra, e ele mostra o editor.
  await focusEditor(page);
  await page.keyboard.press('Control+Backslash');
  await page.locator('[data-pane="B"] .cm-content').waitFor();
  await page.locator('[data-group="A"] [data-tab]').first().getByRole('button', { name: /^Fechar/ }).click();
  await page.waitForFunction(() => !document.querySelector('[data-group="A"]'));
  const onlyB = await page.locator('.cm-editor:visible').first().boundingBox();
  assert.ok(onlyB && onlyB.width > 300 && onlyB.height > 100, `só o grupo B: editor visível e largo (${JSON.stringify(onlyB)})`);
  assert.equal(await page.locator('[data-pane="A"]').count(), 0, 'sem área do grupo A');

  // Recuperação com só o B: depois de recarregar, o editor continua visível.
  await openApp(page);
  await page.locator('[data-group="B"] [data-tab]').first().waitFor();
  assert.equal(await page.locator('[data-group="A"]').count(), 0, 'após recarregar, sem grupo A vazio');
  const reloaded = await page.locator('.cm-editor:visible').first().boundingBox();
  assert.ok(reloaded && reloaded.width > 300 && reloaded.height > 100, `recuperação com só B: editor visível (${JSON.stringify(reloaded)})`);
});

scenario('dividir: mesmo texto nos dois grupos; edição feita num grupo aparece no outro', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type('ZZ-A');
  await page.keyboard.press('Control+Backslash');
  await page.locator('[data-pane="B"] .cm-content').waitFor();
  assert.ok((await page.locator('[data-pane="B"] .cm-content').textContent()).includes('ZZ-A'), 'grupo B mostra o texto digitado no A');
  await page.locator('[data-pane="B"] .cm-content').click();
  await page.keyboard.press('Control+End');
  await page.keyboard.type('YY-B');
  await page.waitForTimeout(150);
  assert.ok((await page.locator('[data-pane="A"] .cm-content').textContent()).includes('YY-B'), 'grupo A mostra a edição feita no B');
});

scenario('edição externa: modal mostra disco e rascunho; Cancelar não grava; Salvar grava sobre a base revista', async ({ page }) => {
  await treeItem(page, 'README.md').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type(' local');
  await page.evaluate(() => window.__mock.external('README.md', 'EXTERNO do disco\n'));
  await focusEditor(page);
  // A gravação recusa (a base mudou) e a faixa do arquivo oferece a comparação.
  await page.keyboard.press('Control+s');
  await page.getByText('O arquivo mudou no disco e você tem alterações.').waitFor();
  await page.getByRole('button', { name: 'Comparar e salvar minha edição' }).click();
  const dialog = page.getByRole('dialog', { name: 'Salvar sua edição sobre o disco?' });
  await dialog.waitFor();
  const text = await dialog.textContent();
  assert.ok(text.includes('EXTERNO do disco'), 'modal mostra a versão do disco');
  assert.ok(text.includes('local'), 'modal mostra a sua edição');
  await page.screenshot({ path: join(OUT, 'editor-ui-compare.png') });
  await dialog.getByRole('button', { name: 'Cancelar' }).click();
  await dialog.waitFor({ state: 'detached' });
  assert.equal(await page.evaluate(() => window.__mock.disk('README.md')), 'EXTERNO do disco\n', 'Cancelar não gravou');

  await page.getByRole('button', { name: 'Comparar e salvar minha edição' }).click();
  await dialog.waitFor();
  await dialog.getByRole('button', { name: 'Salvar minha edição sobre o disco' }).click();
  await dialog.waitFor({ state: 'detached' });
  const onDisk = await page.evaluate(() => window.__mock.disk('README.md'));
  assert.ok(onDisk.endsWith(' local') && !onDisk.includes('EXTERNO'), `Salvar gravou o rascunho sobre a base revista (${JSON.stringify(onDisk.slice(-40))})`);
  assert.equal(await page.locator('[aria-label="Alteração não salva"]').count(), 0, 'sem alteração pendente depois de gravar');
});

scenario('disco revertido por fora ao texto que já passou pelo editor: o editor mostra o disco e não grava por cima', async ({ page }) => {
  const original = await page.evaluate(() => window.__mock.disk('README.md'));
  await treeItem(page, 'README.md').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type('x');
  await page.keyboard.press('Backspace');
  await page.keyboard.type('y');
  await page.keyboard.press('Control+s');
  await page.waitForFunction((o) => window.__mock.disk('README.md') === `${o}y`, original);
  await page.evaluate((o) => window.__mock.external('README.md', o), original);
  // A sincronização do disco roda a cada 2,5 s.
  await page.waitForFunction(() => !document.querySelector('.cm-content:not([aria-hidden])')?.textContent.endsWith('y'), null, { timeout: 7000 });
  assert.equal(await page.locator('[aria-label="Alteração não salva"]').count(), 0, 'sem alteração pendente depois da reversão');
  assert.equal(await page.evaluate(() => window.__mock.disk('README.md')), original, 'o disco segue revertido');
});

scenario('leitura do disco que chega depois do Ctrl+S não troca o texto recém-salvo', async ({ page }) => {
  await treeItem(page, 'README.md').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type(' salvo');
  // Espera uma leitura da sincronização já feita no disco e ainda a caminho, e salva antes de ela chegar.
  await page.evaluate(() => { window.__mock.lateRead = 1500; window.__readsBefore = window.__mock.calls.filter((c) => c.cmd === 'read_file').length; });
  await page.waitForFunction(() => window.__mock.calls.filter((c) => c.cmd === 'read_file' && c.completedAt !== null).length > window.__readsBefore, null, { timeout: 5000 });
  await page.keyboard.press('Control+s');
  await page.waitForFunction(() => window.__mock.disk('README.md').endsWith(' salvo'));
  await page.waitForTimeout(2000);
  assert.ok((await editorText(page)).endsWith(' salvo'), 'o editor mantém o texto salvo');
  assert.equal(await page.locator('[aria-label="Alteração não salva"]').count(), 0, 'sem alteração pendente depois de salvar');
  assert.ok((await page.evaluate(() => window.__mock.disk('README.md'))).endsWith(' salvo'), 'o disco mantém o texto salvo');
});

scenario('fechar a janela: Cancelar mantém a janela e o rascunho; nada é gravado', async ({ page }) => {
  await treeItem(page, 'README.md').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type(' cancelar');
  await page.evaluate(() => { void window.__mock.close(); }); // não espera: a promessa só resolve depois do modal
  const ask = page.getByRole('dialog').filter({ hasText: 'alterações não salvas' });
  await ask.waitFor();
  await ask.getByRole('button', { name: 'Cancelar' }).click();
  await ask.waitFor({ state: 'detached' });
  assert.ok((await editorText(page)).includes('cancelar'), 'Cancelar: rascunho preservado');
  assert.equal(await page.evaluate(() => document.body.textContent.includes('Janela fechada')), false, 'Cancelar: a janela segue aberta');
  assert.equal((await page.evaluate(() => window.__mock.disk('README.md'))).includes('cancelar'), false, 'Cancelar não gravou');
});

scenario('fechar a janela: Descartar fecha e não grava; Salvar grava antes de fechar', async ({ page }) => {
  await treeItem(page, 'README.md').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type(' descartar');
  await page.evaluate(() => { void window.__mock.close(); }); // não espera: a promessa só resolve depois do modal
  const ask = page.getByRole('dialog').filter({ hasText: 'alterações não salvas' });
  await ask.waitFor();
  await ask.getByRole('button', { name: 'Descartar' }).click();
  await page.waitForFunction(() => document.body.textContent.includes('Janela fechada'));
  assert.equal((await page.evaluate(() => window.__mock.disk('README.md'))).includes('descartar'), false, 'Descartar não gravou');
});

scenario('fechar a janela: Salvar com gravação lenta segura o diálogo até gravar, e só então fecha', async ({ page }) => {
  await treeItem(page, 'README.md').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type(' salvo');
  await page.evaluate(() => { window.__mock.delay.write_file_checked = 1500; });
  await page.evaluate(() => { void window.__mock.close(); }); // não espera: a promessa só resolve depois do modal
  const ask = page.getByRole('dialog').filter({ hasText: 'alterações não salvas' });
  await ask.waitFor();
  await ask.getByRole('button', { name: 'Salvar' }).click();
  // Durante a gravação o diálogo fica, com Cancelar desligado: não há como voltar a digitar no meio do save.
  await ask.getByRole('button', { name: 'Salvando…' }).waitFor();
  assert.equal(await ask.getByRole('button', { name: 'Cancelar' }).isDisabled(), true, 'Cancelar desligado durante a gravação');
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => document.body.textContent.includes('Janela fechada')), false, 'não fecha antes de gravar');
  await page.waitForFunction(() => document.body.textContent.includes('Janela fechada'), null, { timeout: 5000 });
  assert.ok((await page.evaluate(() => window.__mock.disk('README.md'))).includes('salvo'), 'a gravação chegou ao disco antes de fechar');
});

scenario('busca: resultado com emoji posiciona o cursor na coluna UTF-16 exata (linha 2, coluna 7)', async ({ page }) => {
  await page.getByRole('button', { name: 'Pesquisar', exact: true }).click();
  await page.getByRole('textbox', { name: 'Pesquisar nos arquivos do projeto' }).fill('join');
  const hit = page.locator('[data-result][title="src/lib/utils.ts:2:7"]');
  await hit.waitFor();
  await hit.click();
  await page.locator('.cm-content:visible').first().waitFor();
  await page.waitForTimeout(250);
  const point = await columnPoint(page, 2, 7);
  assert.ok(!point.error, point.error);
  const cursor = await primaryCursor(page);
  assert.ok(cursor, 'cursor do CodeMirror visível');
  assert.ok(Math.abs(cursor.x - point.x) <= 2, `cursor x ${cursor.x.toFixed(1)} ≠ coluna 7 da linha 2 (${point.x.toFixed(1)})`);
  assert.ok(cursor.y >= point.top - 2 && cursor.y < point.bottom, `cursor fora da linha 2 (y ${cursor.y.toFixed(1)}, linha ${point.top.toFixed(1)}–${point.bottom.toFixed(1)})`);
});



scenario('breadcrumbs e lista de abas: o caminho do arquivo abre o seletor da pasta; arquivo do seletor abre; pasta desce; a lista mostra as abas do grupo', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await focusEditor(page);
  const crumbs = page.getByRole('navigation', { name: 'Caminho do arquivo' });
  assert.deepEqual(await crumbs.getByRole('button').allTextContents(), ['src', 'App.tsx'], 'breadcrumbs: pasta e arquivo');
  await page.screenshot({ path: join(OUT, 'editor-ui-breadcrumbs.png') });

  // "src" abre o conteúdo da raiz, com src marcada; README.md abre como prévia.
  await crumbs.getByRole('button', { name: 'src', exact: true }).click();
  const raiz = page.getByRole('listbox', { name: 'Conteúdo de projeto' });
  assert.equal(await raiz.getByRole('option', { name: 'src', exact: true }).getAttribute('aria-selected'), 'true', 'a pasta do item vem marcada');
  await raiz.getByRole('option', { name: 'README.md', exact: true }).click();
  await crumbs.getByRole('button', { name: 'README.md', exact: true }).waitFor();

  // No seletor do arquivo, a pasta src desce um nível; o arquivo dela abre.
  await crumbs.getByRole('button', { name: 'README.md', exact: true }).click();
  await page.getByRole('listbox', { name: 'Conteúdo de projeto' }).getByRole('option', { name: 'src', exact: true }).click();
  await page.getByRole('listbox', { name: 'Conteúdo de src' }).getByRole('option', { name: 'App.tsx', exact: true }).click();
  await crumbs.getByRole('button', { name: 'App.tsx', exact: true }).waitFor();

  // A lista de abas mostra as abas do grupo e ativa a escolhida.
  await treeItem(page, 'README.md').dblclick();
  await crumbs.getByRole('button', { name: 'README.md', exact: true }).waitFor();
  await page.getByRole('button', { name: /^Lista de abas/ }).first().click();
  await page.getByRole('menuitem', { name: /App\.tsx/ }).click();
  await crumbs.getByRole('button', { name: 'App.tsx', exact: true }).waitFor();
});

scenario('rascunho: duplo clique na área vazia da faixa cria "Sem título-1" ativa, e o próximo é "Sem título-2"', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  const activeTitle = () => page.locator('[data-group="A"] [role="tab"][aria-selected="true"]').textContent();

  const first = await emptyStripPoint(page);
  await page.mouse.dblclick(first.x, first.y);
  await page.waitForFunction(() => [...document.querySelectorAll('[data-group="A"] [role="tab"]')].some((t) => t.textContent.startsWith('Sem título-1')));
  assert.ok((await activeTitle()).startsWith('Sem título-1'), `primeiro rascunho ativo (${await activeTitle()})`);

  const second = await emptyStripPoint(page);
  await page.mouse.dblclick(second.x, second.y);
  await page.waitForFunction(() => [...document.querySelectorAll('[data-group="A"] [role="tab"]')].some((t) => t.textContent.startsWith('Sem título-2')));
  assert.ok((await activeTitle()).startsWith('Sem título-2'), `segundo rascunho ativo (${await activeTitle()})`);
  const titles = await tabTitles(page);
  assert.equal(titles.length, 3, `App + dois rascunhos (${JSON.stringify(titles)})`);
  assert.ok(titles.some((t) => t.startsWith('Sem título-1')) && titles.some((t) => t.startsWith('Sem título-2')), `os dois rascunhos na faixa (${JSON.stringify(titles)})`);
});

scenario('rascunho: duplo clique em cima de uma aba existente não cria rascunho', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await tabs(page).first().dblclick();
  await page.waitForTimeout(150);
  assert.equal(await tabs(page).count(), 1, 'duplo clique na aba não criou aba nova');
  assert.ok(!(await tabTitles(page)).some((t) => t.startsWith('Sem título')), `nenhum rascunho (${JSON.stringify(await tabTitles(page))})`);
});

scenario('rascunho: logo após criar, digitar sem clicar escreve no rascunho e a aba fica suja', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  const point = await emptyStripPoint(page);
  await page.mouse.dblclick(point.x, point.y);
  const draft = tabs(page).filter({ hasText: 'Sem título-1' });
  await draft.waitFor();
  assert.equal(await draft.locator('[aria-label="Alteração não salva"]').count(), 0, 'rascunho vazio ainda não está sujo');

  await page.keyboard.type('rascunho novo');
  await page.waitForTimeout(150);
  assert.ok((await editorText(page)).includes('rascunho novo'), 'o texto digitado foi para o rascunho, sem clique no editor');
  assert.equal(await draft.locator('[aria-label="Alteração não salva"]').count(), 1, 'aba do rascunho com texto aparece como não salva');
});

scenario('rascunho: fechar outra aba mantém o rascunho com o mesmo texto', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  const point = await emptyStripPoint(page);
  await page.mouse.dblclick(point.x, point.y);
  await page.keyboard.type('texto preservado');
  await page.waitForTimeout(150);

  // Fecha a aba do App.tsx (outra aba, sem alteração): o buffer do rascunho não pode sumir.
  await tabs(page).filter({ hasText: 'App.tsx' }).getByRole('button', { name: /^Fechar/ }).click();
  await page.waitForFunction(() => [...document.querySelectorAll('[data-group="A"] [data-tab]')].every((t) => !t.textContent.includes('App.tsx')));
  assert.deepEqual((await tabTitles(page)).map((t) => t.startsWith('Sem título-1')), [true], 'só o rascunho sobrou');
  assert.ok((await editorText(page)).includes('texto preservado'), 'rascunho mantém o texto depois de fechar a outra aba');
});

scenario('rascunho: Ctrl+N com o foco no editor cria um rascunho novo', async ({ page }) => {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+n');
  await page.waitForFunction(() => [...document.querySelectorAll('[data-group="A"] [role="tab"]')].some((t) => t.textContent.startsWith('Sem título-1')));
  assert.ok((await tabTitles(page)).some((t) => t.startsWith('Sem título-1')), 'Ctrl+N criou Sem título-1');
  assert.ok((await page.locator('[data-group="A"] [role="tab"][aria-selected="true"]').textContent()).startsWith('Sem título-1'), 'o rascunho do Ctrl+N ficou ativo');
});

// Grupo sem abas: fecha a única aba e espera a área vazia (EmptyGroup) aparecer no painel do grupo.
async function emptyGroup(page, group = 'A') {
  await expand(page, ['src']);
  await treeItem(page, 'App.tsx').dblclick();
  await tabs(page).filter({ hasText: 'App.tsx' }).getByRole('button', { name: /^Fechar/ }).click();
  const area = page.locator(`[data-pane="${group}"] [data-empty]`);
  await area.waitFor();
  return area;
}

scenario('rascunho: grupo sem abas, duplo clique no centro da área vazia cria "Sem título-1" ativa', async ({ page }) => {
  const area = await emptyGroup(page);
  assert.equal(await page.locator('[data-empty] dt').count(), 0, 'grupo vazio sem atalhos');
  assert.equal(await area.locator('svg').count(), 1, 'grupo vazio com a marca');
  const b = await area.boundingBox();
  await page.mouse.dblclick(b.x + b.width / 2, b.y + b.height / 2);
  await page.waitForFunction(() => [...document.querySelectorAll('[data-group="A"] [role="tab"]')].some((t) => t.textContent.startsWith('Sem título-1')));
  assert.ok((await page.locator('[data-group="A"] [role="tab"][aria-selected="true"]').textContent()).startsWith('Sem título-1'), 'rascunho criado no grupo vazio ficou ativo');
  assert.equal(await tabs(page).count(), 1, 'uma aba só (o rascunho)');
});

scenario('rascunho: grupo sem abas, duplo clique na barra superior vazia cria "Sem título-1"; nos botões da barra não cria', async ({ page }) => {
  await emptyGroup(page);
  const point = await emptyStripPoint(page);
  await page.mouse.dblclick(point.x, point.y);
  await page.waitForFunction(() => [...document.querySelectorAll('[data-group="A"] [role="tab"]')].some((t) => t.textContent.startsWith('Sem título-1')));
  assert.ok((await tabTitles(page)).some((t) => t.startsWith('Sem título-1')), 'duplo clique na barra vazia criou Sem título-1');
});

scenario('rascunho: grupo sem abas, duplo clique nos botões da barra não cria rascunho', async ({ page }) => {
  await emptyGroup(page);
  const dividir = page.locator('[data-group="A"] [role="toolbar"] button').first();
  await dividir.dblclick({ force: true });
  await page.waitForTimeout(150);
  assert.equal(await tabs(page).count(), 0, 'duplo clique no botão da barra não criou aba');
});
