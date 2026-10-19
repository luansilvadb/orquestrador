// QA dos marcadores de mudança do Git no editor (src/editor/git-gutter.ts), em Chrome headless contra o app simulado (src/backend/mock.ts).
// O mock entra só na fronteira de I/O: `git_file_base` devolve o HEAD e o índice de `src/marcadores.ts` (fixture em mock-fixtures.ts: um bloco
// modificado em stage na linha 3; fora do índice, duas linhas acrescentadas em 8–9, a `v12` apagada antes da linha 15 e a linha 22 modificada).
// Confere pela DOM o gutter (tipo, linha, cheia × vazada), a geometria do texto, as marcas na barra de rolagem, a faixa do minimap, a digitação
// ao vivo com Desfazer, a nova leitura da base depois de salvar, e a ausência de marcadores em arquivo não rastreado e em rascunho. O bloco
// aberto (src/editor/git-hunk.ts): clicar no marcador mostra as linhas antigas e destaca as novas, Esc e o X fecham, Restaurar com Desfazer,
// Preparar e Tirar do preparo (o índice que o mock recebeu em `git_write_index`), as setas, Alt+F5 e o menu de contexto.
// Uso: node scripts/check-git-gutter-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
// Capturas em .playwright-mcp/git-gutter-*.png: evidência para leitura humana, não asserção.
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { OUT, uiSuite, expand, treeItem, columnPoint } from './editor-ui-harness.mjs';
import { MARCADORES_HEAD, MARCADORES_INDEX } from '../src/backend/mock-fixtures.ts';

// Cores do VS Code Dark Modern: editorGutter.addedBackground, modifiedBackground e deletedBackground.
const RGB = { added: 'rgb(46, 160, 67)', modified: 'rgb(0, 120, 212)', deleted: 'rgb(248, 81, 73)' };
const EXPECTED = [
  { line: 3, kind: 'modified', staged: true },
  { line: 8, kind: 'added', staged: false },
  { line: 9, kind: 'added', staged: false },
  { line: 15, kind: 'deleted', staged: false },
  { line: 22, kind: 'modified', staged: false },
];

// Espera uma condição assíncrona (leitura da base, medida do CodeMirror): tenta por até 4 s.
async function eventually(fn) {
  let last;
  for (let i = 0; i < 40; i++) {
    try { return await fn(); } catch (e) { last = e; await new Promise((r) => setTimeout(r, 100)); }
  }
  throw last;
}

async function openFile(page, folder, name) {
  await expand(page, [folder]);
  await treeItem(page, name).dblclick();
  await page.locator('.cm-content:visible').first().waitFor();
}

// Marcadores do gutter do Git, cada um com a linha (pelo número na mesma altura), o tipo, se é vazado e o estilo do ::before.
function gutterMarks(page) {
  return page.evaluate(() => {
    const editor = [...document.querySelectorAll('.cm-editor')].find((e) => e.offsetParent !== null);
    if (!editor) return [];
    const numbers = [...editor.querySelectorAll('.cm-lineNumbers .cm-gutterElement')]
      .map((el) => ({ top: Math.round(el.getBoundingClientRect().top), n: Number(el.textContent) })).filter((x) => x.n > 0);
    return [...editor.querySelectorAll('.cm-gitGutter .cm-gutterElement.cm-git')].map((el) => {
      const top = Math.round(el.getBoundingClientRect().top);
      const before = getComputedStyle(el, '::before');
      const kind = ['added', 'modified', 'deleted'].find((k) => el.classList.contains(`cm-git-${k}`));
      return {
        line: numbers.find((x) => Math.abs(x.top - top) <= 1)?.n ?? null, kind, staged: el.classList.contains('cm-git-staged'),
        background: before.backgroundColor, borderLeft: before.borderLeftWidth, borderColor: before.borderLeftColor, image: before.backgroundImage,
        width: before.width, height: el.getBoundingClientRect().height, top, bottom: Math.round(el.getBoundingClientRect().bottom),
      };
    });
  });
}

// Geometria do texto e do gutter: largura dos gutters da esquerda, x do texto (relativo ao editor) e x do ícone da dobra.
function geometry(page) {
  return page.evaluate(() => {
    const editor = [...document.querySelectorAll('.cm-editor')].find((e) => e.offsetParent !== null);
    const left = editor.getBoundingClientRect().left;
    const rect = (sel) => editor.querySelector(sel)?.getBoundingClientRect() ?? null;
    return {
      gutters: Math.round(rect('.cm-gutters-before').width * 10) / 10,
      text: Math.round((rect('.cm-content').left - left) * 10) / 10,
      numbersRight: Math.round((rect('.cm-lineNumbers').right - left) * 10) / 10,
      git: Math.round(rect('.cm-gitGutter').width * 10) / 10,
      gitLeft: Math.round((rect('.cm-gitGutter').left - left) * 10) / 10,
      foldLeft: Math.round((rect('.cm-foldGutter').left - left) * 10) / 10,
      fold: Math.round(rect('.cm-foldGutter').width * 10) / 10,
    };
  });
}

// Marcas na barra de rolagem: a faixa (posição em relação ao scroller, à barra e ao minimap) e cada marca.
function overview(page) {
  return page.evaluate(() => {
    const editor = [...document.querySelectorAll('.cm-editor')].find((e) => e.offsetParent !== null);
    const box = editor.querySelector('.cm-gitOverview');
    const el = editor.querySelector('.cm-scroller');
    const scroller = el.getBoundingClientRect();
    const minimap = editor.querySelector('.cm-minimap-gutter')?.getBoundingClientRect() ?? null;
    if (!box) return null;
    const b = box.getBoundingClientRect();
    return {
      left: b.left, right: b.right, top: b.top, bottom: b.bottom, width: b.width, pointer: getComputedStyle(box).pointerEvents,
      scrollerRight: scroller.right, bar: el.offsetWidth - el.clientWidth, scrollerTop: scroller.top, minimapRight: minimap?.right ?? null,
      marks: [...box.querySelectorAll('.cm-gitOverview-mark')].map((m) => {
        const r = m.getBoundingClientRect();
        return { kind: ['added', 'modified', 'deleted'].find((k) => m.classList.contains(`cm-gitOverview-${k}`)), top: r.top, height: r.height, color: getComputedStyle(m).backgroundColor };
      }),
    };
  });
}

// Cores presentes na faixa de git do minimap (4 px do canvas a partir do x 2, depois do respiro da esquerda).
function minimapColors(page) {
  return page.evaluate(() => {
    const editor = [...document.querySelectorAll('.cm-editor')].find((e) => e.offsetParent !== null);
    const canvas = editor.querySelector('.cm-minimap-gutter canvas');
    if (!canvas) return [];
    const data = canvas.getContext('2d').getImageData(4, 0, 1, canvas.height).data;
    const seen = new Set();
    for (let i = 0; i < data.length; i += 4) if (data[i + 3] > 200) seen.add(`rgb(${data[i]}, ${data[i + 1]}, ${data[i + 2]})`);
    return [...seen];
  });
}

const fileBaseCalls = (page, path) => page.evaluate((p) => window.__mock.calls.filter((c) => c.cmd === 'git_file_base' && c.args.path === p), path);
const simplify = (marks) => marks.map(({ line, kind, staged }) => ({ line, kind, staged }));

// Sem `--hide-scrollbars` (padrão do Playwright headless): a barra vertical ocupa os 14 px, como no WebView2, e o minimap fica à esquerda dela.
const scenario = uiSuite('git-gutter', { launch: { ignoreDefaultArgs: ['--hide-scrollbars'] } });

scenario('arquivo modificado: os três tipos nas linhas certas, cheia × vazada, geometria igual, barra de rolagem e minimap', async ({ page }) => {
  await openFile(page, 'src', 'marcadores.ts');
  const marks = await eventually(async () => {
    const found = await gutterMarks(page);
    assert.deepEqual(simplify(found), EXPECTED);
    return found;
  });
  for (const m of marks) {
    if (m.kind === 'deleted') {
      assert.match(m.image, /svg/, `linha ${m.line}: triângulo do apagado`);
      assert.match(decodeURIComponent(m.image), /F85149/i, `linha ${m.line}: triângulo vermelho (#F85149)`);
      continue;
    }
    assert.equal(m.width, '3px', `linha ${m.line}: barra de 3 px`);
    if (m.staged) {
      assert.equal(m.background, 'rgba(0, 0, 0, 0)', `linha ${m.line}: preparado é vazado`);
      assert.equal(m.borderLeft, '1px', `linha ${m.line}: contorno de 1 px`);
      assert.equal(m.borderColor, RGB[m.kind], `linha ${m.line}: contorno na cor do tipo`);
    } else {
      assert.equal(m.background, RGB[m.kind], `linha ${m.line}: não preparado é cheio, na cor do tipo`);
    }
  }
  // As duas linhas acrescentadas formam uma barra contínua: a 9 começa onde a 8 termina.
  const [l8, l9] = marks.filter((m) => m.kind === 'added');
  assert.equal(l9.top, l8.bottom, 'barra contínua entre linhas seguidas');

  // Geometria do texto: 50 px dos números + 26 px (6 do Git e 20 da dobra), o texto começa em 76 px, como antes do gutter novo.
  const geo = await geometry(page);
  assert.deepEqual(geo, { gutters: 76, text: 76, numbersRight: 50, git: 6, gitLeft: 50, foldLeft: 56, fold: 20 }, 'geometria do gutter');

  // Barra de rolagem: quatro marcas (uma por bloco), na faixa da barra vertical, à direita do minimap, sem receber o ponteiro, em ordem.
  const ov = await eventually(async () => {
    const found = await overview(page);
    assert.ok(found, 'faixa das marcas presente');
    assert.equal(found.marks.length, 4, `quatro marcas: ${JSON.stringify(found.marks)}`);
    return found;
  });
  assert.equal(ov.pointer, 'none', 'as marcas não recebem o ponteiro');
  // A largura é a da barra de verdade: o tema pede 14 px, mas o `scrollbar-width: thin` global do app vence o ::-webkit-scrollbar no Chromium.
  assert.ok(ov.bar > 0, 'barra vertical ocupa espaço');
  assert.equal(Math.round(ov.width), ov.bar, `faixa com a largura da barra vertical (${ov.bar} px)`);
  assert.ok(Math.abs(ov.right - ov.scrollerRight) <= 0.5, 'faixa encostada na direita do scroller');
  assert.ok(ov.minimapRight !== null && ov.left >= ov.minimapRight - 0.5, `marcas fora do minimap (faixa em ${ov.left}, minimap até ${ov.minimapRight})`);
  assert.deepEqual(ov.marks.map((m) => m.kind), ['modified', 'added', 'deleted', 'modified'], 'tipos na ordem do documento');
  for (let i = 1; i < ov.marks.length; i++) assert.ok(ov.marks[i].top > ov.marks[i - 1].top, 'marcas em ordem de cima para baixo');
  for (const m of ov.marks) {
    assert.ok(m.height >= 3, 'altura mínima de 3 px');
    assert.ok(m.top >= ov.top - 0.5 && m.top + m.height <= ov.bottom + 0.5, 'marca dentro da faixa');
  }
  assert.equal(ov.marks[1].color, 'rgba(46, 160, 67, 0.6)', 'overview ruler com a cor do gutter a 60%');

  // Minimap: a faixa de git com as três cores.
  const colors = await eventually(async () => {
    const found = await minimapColors(page);
    for (const kind of ['added', 'modified', 'deleted']) assert.ok(found.includes(RGB[kind]), `minimap com ${kind} (${found.join(' ')})`);
    return found;
  });
  assert.ok(colors.length >= 3);
  await page.locator('.cm-editor:visible').first().screenshot({ path: join(OUT, 'git-gutter-marcadores.png') });
});

scenario('digitação ao vivo: linha nova vira acrescentada na hora; Desfazer tira; salvar relê a base', async ({ page }) => {
  await openFile(page, 'src', 'marcadores.ts');
  await eventually(async () => assert.deepEqual(simplify(await gutterMarks(page)), EXPECTED));
  const original = await page.locator('.cm-content:visible').first().evaluate((el) => el.textContent);
  // Fim da linha 30 (`v29`), Enter e uma linha nova: a 31 vira acrescentada, sem esperar nenhuma consulta.
  const p = await columnPoint(page, 30, 1);
  await page.mouse.click(p.x + 1, (p.top + p.bottom) / 2);
  await page.keyboard.press('End');
  const before = (await fileBaseCalls(page, 'src/marcadores.ts')).length;
  await page.keyboard.press('Enter');
  await page.keyboard.type('export const nova = 0;');
  await eventually(async () => assert.deepEqual(simplify(await gutterMarks(page)), [...EXPECTED, { line: 31, kind: 'added', staged: false }]));
  assert.ok((await fileBaseCalls(page, 'src/marcadores.ts')).length - before <= 1, 'o marcador não dependeu de nova leitura da base');
  await page.locator('.cm-editor:visible').first().screenshot({ path: join(OUT, 'git-gutter-digitando.png') });
  // Desfazer até o texto voltar ao original: o marcador some e os outros ficam como estavam (a base não entrou no histórico).
  for (let i = 0; i < 5; i++) {
    if ((await page.locator('.cm-content:visible').first().evaluate((el) => el.textContent)) === original) break;
    await page.keyboard.press('Control+z');
  }
  assert.equal(await page.locator('.cm-content:visible').first().evaluate((el) => el.textContent), original, 'Desfazer voltou ao texto original');
  await eventually(async () => assert.deepEqual(simplify(await gutterMarks(page)), EXPECTED));
  // Um Desfazer a mais não mexe no documento (a troca da base não é desfazível).
  await page.keyboard.press('Control+z');
  assert.equal(await page.locator('.cm-content:visible').first().evaluate((el) => el.textContent), original, 'nada além da edição para desfazer');

  // Salvar: a base é lida de novo logo depois da gravação, antes da próxima consulta periódica (2,5 s).
  await page.keyboard.press('Enter');
  await page.keyboard.type('export const salva = 1;');
  const saved = (await fileBaseCalls(page, 'src/marcadores.ts')).length;
  await page.keyboard.press('Control+s');
  await eventually(async () => assert.ok((await fileBaseCalls(page, 'src/marcadores.ts')).length > saved, 'nova leitura da base depois de salvar'));
});

scenario('sem marcadores: arquivo não rastreado e rascunho "Sem título"; a geometria é a mesma', async ({ page }) => {
  await openFile(page, 'docs', 'rascunho-painel.md');
  await eventually(async () => {
    const calls = await fileBaseCalls(page, 'docs/rascunho-painel.md');
    assert.ok(calls.length > 0 && calls.every((c) => c.result === null), 'o backend respondeu sem base');
  });
  await page.waitForTimeout(300);
  assert.deepEqual(await gutterMarks(page), [], 'não rastreado sem marcador');
  assert.equal((await geometry(page)).text, 76, 'texto no mesmo x sem marcadores');

  await page.locator('.cm-content:visible').first().click();
  await page.keyboard.press('Control+n');
  await page.getByRole('tab', { name: /Sem título-1/ }).waitFor();
  await page.keyboard.type('linha de rascunho\noutra');
  await page.waitForTimeout(300);
  assert.deepEqual(await gutterMarks(page), [], 'rascunho sem marcador');
  assert.equal((await fileBaseCalls(page, '')).length, 0, 'rascunho não consulta o Git');
  assert.equal((await geometry(page)).text, 76, 'rascunho com o texto no mesmo x');
  assert.equal(await page.locator('.cm-gitOverview-mark').count(), 0, 'rascunho sem marcas na barra de rolagem');
  await page.locator('.cm-editor:visible').first().screenshot({ path: join(OUT, 'git-gutter-rascunho.png') });
  // Sem alterações, os itens de navegação do menu ficam bloqueados, com o motivo; Alt+F5 não abre nada.
  await page.keyboard.press('Alt+F5');
  assert.equal(await page.locator('.cm-gitHunk').count(), 0, 'Alt+F5 sem alterações não abre bloco');
  await page.locator('.cm-content:visible').first().click({ button: 'right' });
  const next = page.getByRole('menuitem', { name: /Próxima Alteração/ });
  await next.waitFor();
  assert.equal(await next.getAttribute('aria-disabled'), 'true', 'Próxima Alteração bloqueada sem alterações');
  assert.match(await next.textContent(), /Sem alterações em relação ao Git/, 'motivo do bloqueio');
  await page.keyboard.press('Escape');
});

// ---- Bloco aberto (src/editor/git-hunk.ts) ----

// Cores do diff do VS Code (diffEditor.removedLineBackground e insertedLineBackground, tema escuro).
const REMOVED = 'rgba(255, 0, 0, 0.2)';
const INSERTED = 'rgba(155, 185, 85, 0.2)';
// O índice depois de preparar as duas linhas acrescentadas (8–9): o índice da fixture com elas depois da `v6`.
const INDEX_WITH_EXTRA = MARCADORES_INDEX.replace('export const v6 = 6;\n', 'export const v6 = 6;\nexport const extra1 = 1;\nexport const extra2 = 2;\n');

// Clica no marcador da linha (no meio do gutter do Git, na altura dela).
async function clickMarker(page, line) {
  const mark = (await gutterMarks(page)).find((m) => m.line === line);
  assert.ok(mark, `marcador na linha ${line}`);
  const box = await page.locator('.cm-gitGutter:visible').first().boundingBox();
  await page.mouse.click(box.x + box.width / 2, (mark.top + mark.bottom) / 2);
}

// Bloco aberto: título, botões, linhas antigas (texto, fundo, cores de sintaxe, fonte), linhas novas destacadas (pelo número na mesma
// altura) e números de linha dentro da altura do widget (devem ser zero). `null` sem bloco aberto.
function openBlock(page) {
  return page.evaluate(() => {
    const editor = [...document.querySelectorAll('.cm-editor')].find((e) => e.offsetParent !== null);
    const w = editor.querySelector('.cm-gitHunk');
    if (!w) return null;
    const numbers = [...editor.querySelectorAll('.cm-lineNumbers .cm-gutterElement')]
      .map((el) => ({ top: Math.round(el.getBoundingClientRect().top), n: Number(el.textContent) })).filter((x) => x.n > 0);
    const lineOf = (el) => numbers.find((x) => Math.abs(x.top - Math.round(el.getBoundingClientRect().top)) <= 1)?.n ?? null;
    const style = (el) => { const s = getComputedStyle(el); return { font: s.fontFamily, size: s.fontSize, height: s.lineHeight }; };
    const box = w.getBoundingClientRect();
    const old = w.querySelector('.cm-gitHunk-old');
    const first = w.querySelector('.cm-gitHunk-old .cm-gitHunk-line');
    const bar = w.querySelector('.cm-gitHunk-bar').getBoundingClientRect();
    const minimap = editor.querySelector('.cm-minimap-gutter')?.getBoundingClientRect();
    const lastButton = [...w.querySelectorAll('.cm-gitHunk-button')].pop()?.getBoundingClientRect();
    return {
      title: w.querySelector('.cm-gitHunk-title').textContent,
      buttons: [...w.querySelectorAll('.cm-gitHunk-button')].map((b) => b.getAttribute('aria-label')),
      old: [...w.querySelectorAll('.cm-gitHunk-old .cm-gitHunk-line')].map((l) => l.textContent),
      oldBackground: old ? getComputedStyle(old).backgroundColor : null,
      oldColors: first ? [...first.querySelectorAll('span')].map((s) => getComputedStyle(s).color) : [],
      oldStyle: first ? style(first) : null,
      oldHeight: first ? first.getBoundingClientRect().height : null,
      lineStyle: style(editor.querySelector('.cm-content .cm-line')),
      newLines: [...editor.querySelectorAll('.cm-content .cm-line.cm-gitHunk-new')]
        .map((l) => ({ line: lineOf(l), text: l.textContent, background: getComputedStyle(l).backgroundColor })),
      numbersInside: numbers.filter((x) => x.top >= Math.round(box.top) && x.top < Math.round(box.bottom) - 1).length,
      bottom: Math.round(box.bottom),
      barRight: bar.right, closeRight: lastButton?.right ?? null, minimapLeft: minimap?.left ?? null,
    };
  });
}

// Linha do cursor (pelo número destacado no gutter) e as linhas do documento (todas desenhadas: o arquivo tem 81 linhas).
const cursorLine = (page) => page.evaluate(() => {
  const editor = [...document.querySelectorAll('.cm-editor')].find((e) => e.offsetParent !== null);
  return Number(editor.querySelector('.cm-lineNumbers .cm-activeLineGutter')?.textContent);
});
const docLines = (page) => page.evaluate(() => {
  const editor = [...document.querySelectorAll('.cm-editor')].find((e) => e.offsetParent !== null);
  return [...editor.querySelectorAll('.cm-content .cm-line')].map((l) => l.textContent);
});
const writeCalls = (page) => page.evaluate(() => window.__mock.calls.filter((c) => c.cmd === 'git_write_index'));
const blockButton = (page, name) => page.locator('.cm-gitHunk-bar').getByRole('button', { name, exact: true });

scenario('bloco aberto: clicar no marcador mostra as linhas antigas e destaca as novas; clicar de novo e Esc fecham; fechado, nada muda', async ({ page }) => {
  await openFile(page, 'src', 'marcadores.ts');
  await eventually(async () => assert.deepEqual(simplify(await gutterMarks(page)), EXPECTED));
  const geoBefore = await geometry(page);
  const at30 = await columnPoint(page, 30, 5);

  await clickMarker(page, 22);
  const block = await eventually(async () => {
    const found = await openBlock(page);
    assert.ok(found, 'bloco aberto');
    // A cor de sintaxe chega com a linguagem (assíncrona): `export` em rosa (#C586C0), como no texto do arquivo.
    assert.ok(found.oldColors.includes('rgb(197, 134, 192)'), `linha antiga colorida (${found.oldColors.join(' ')})`);
    return found;
  });
  assert.equal(block.title, 'Alteração 4 de 4');
  assert.deepEqual(block.buttons, ['Preparar', 'Restaurar', 'Alteração anterior', 'Próxima alteração', 'Fechar']);
  assert.deepEqual(block.old, ['export const v20 = 20;'], 'linha antiga do HEAD');
  assert.equal(block.oldBackground, REMOVED, 'linhas antigas em vermelho translúcido');
  assert.deepEqual(block.oldStyle, block.lineStyle, 'linhas antigas com a fonte, o tamanho e a altura de linha do editor');
  assert.equal(block.oldHeight, 19, 'linha antiga com 19 px');
  assert.deepEqual(block.newLines, [{ line: 22, text: 'export const v20 = 200;', background: INSERTED }], 'linha nova em verde translúcido');
  assert.equal(block.numbersInside, 0, 'as linhas antigas não ganham número');
  const line22 = await columnPoint(page, 22, 1);
  assert.equal(block.bottom, Math.round(line22.top), 'o widget fica logo acima da primeira linha do bloco');
  // A barra fica na área visível do texto, à esquerda do minimap (que fica por cima do conteúdo), com o X encostado à direita.
  assert.ok(block.minimapLeft !== null && block.barRight <= block.minimapLeft + 0.5, `barra até o minimap (${block.barRight} × ${block.minimapLeft})`);
  assert.ok(block.closeRight <= block.barRight && block.barRight - block.closeRight <= 10, 'botões alinhados à direita da barra');
  assert.equal(await page.locator('.cm-gitGutter .cm-git').first().evaluate((el) => getComputedStyle(el).cursor), 'pointer', 'cursor de mão no marcador');
  await page.locator('.cm-editor:visible').first().screenshot({ path: join(OUT, 'git-gutter-bloco-aberto.png') });

  // Clicar de novo fecha; Esc com o foco no editor também.
  await clickMarker(page, 22);
  assert.equal(await openBlock(page), null, 'segundo clique fecha');
  await clickMarker(page, 15);
  const deleted = await openBlock(page);
  assert.deepEqual(deleted.old, ['export const v12 = 12;'], 'apagado mostra a linha que sumiu');
  assert.deepEqual(deleted.newLines, [], 'apagado não destaca linha nova');
  assert.equal(deleted.bottom, Math.round((await columnPoint(page, 15, 1)).top), 'apagado: o widget fica na fronteira, acima da linha 15');
  await page.keyboard.press('Escape');
  assert.equal(await openBlock(page), null, 'Esc fecha');

  // Fechado: a geometria e a posição do texto são as de antes.
  assert.deepEqual(await geometry(page), geoBefore, 'geometria igual com o bloco fechado');
  assert.deepEqual(await columnPoint(page, 30, 5), at30, 'texto no mesmo lugar com o bloco fechado');
});

scenario('bloco aberto: digitar dentro mantém aberto; Restaurar volta o HEAD, fecha e Ctrl+Z desfaz', async ({ page }) => {
  await openFile(page, 'src', 'marcadores.ts');
  await eventually(async () => assert.deepEqual(simplify(await gutterMarks(page)), EXPECTED));
  await clickMarker(page, 22);
  await eventually(async () => assert.ok(await openBlock(page)));
  // Digitar no fim da linha 22 (dentro do bloco): o bloco continua aberto e a linha nova acompanha.
  const p = await columnPoint(page, 22, 1);
  await page.mouse.click(p.x + 1, (p.top + p.bottom) / 2);
  await page.keyboard.press('End');
  await page.keyboard.type(' // x');
  await eventually(async () => assert.deepEqual((await openBlock(page))?.newLines.map((l) => l.text), ['export const v20 = 200; // x']));

  await blockButton(page, 'Restaurar').click();
  await eventually(async () => assert.equal((await docLines(page))[21], 'export const v20 = 20;', 'linha 22 com o texto do HEAD'));
  assert.equal(await openBlock(page), null, 'o bloco restaurado some e fecha');
  await eventually(async () => assert.deepEqual(simplify(await gutterMarks(page)), EXPECTED.filter((m) => m.line !== 22)));
  assert.equal((await writeCalls(page)).length, 0, 'Restaurar não grava o índice');
  assert.equal(await page.evaluate(() => window.__mock.disk('src/marcadores.ts')).then((t) => t.includes('v20 = 200;')), true, 'nem o disco');
  await page.keyboard.press('Control+z');
  await eventually(async () => assert.equal((await docLines(page))[21], 'export const v20 = 200; // x', 'Ctrl+Z desfaz o Restaurar'));
  await eventually(async () => assert.ok(simplify(await gutterMarks(page)).some((m) => m.line === 22), 'marcador de volta'));
});

scenario('bloco aberto: Preparar deixa vazado e grava o índice certo; Tirar do preparo volta a cheio', async ({ page }) => {
  await openFile(page, 'src', 'marcadores.ts');
  await eventually(async () => assert.deepEqual(simplify(await gutterMarks(page)), EXPECTED));
  await clickMarker(page, 8);
  await eventually(async () => assert.equal((await openBlock(page))?.title, 'Alteração 2 de 4'));

  const t0 = Date.now();
  await blockButton(page, 'Preparar').click();
  await eventually(async () => {
    const marks = simplify(await gutterMarks(page));
    assert.deepEqual(marks.filter((m) => m.line === 8 || m.line === 9).map((m) => m.staged), [true, true], 'linhas 8–9 vazadas');
  });
  assert.ok(Date.now() - t0 < 2000, `releu a base na hora, sem esperar a consulta de 2,5 s (${Date.now() - t0} ms)`);
  const [stage] = await writeCalls(page);
  assert.equal(stage.args.path, 'src/marcadores.ts');
  assert.equal(stage.args.expected, MARCADORES_INDEX, 'expected é o índice que a tela viu');
  assert.equal(stage.args.text, INDEX_WITH_EXTRA, 'índice novo: só as duas linhas acrescentadas entram');
  assert.equal(stage.error, null);
  const staged = await openBlock(page);
  assert.ok(staged, 'o bloco continua aberto');
  assert.equal(staged.title, 'Alteração 2 de 4 · preparada');
  assert.equal(staged.buttons[0], 'Tirar do preparo');
  await page.locator('.cm-editor:visible').first().screenshot({ path: join(OUT, 'git-gutter-bloco-preparado.png') });

  await blockButton(page, 'Tirar do preparo').click();
  await eventually(async () => assert.deepEqual(simplify(await gutterMarks(page)), EXPECTED, 'linhas 8–9 cheias de novo'));
  const unstage = (await writeCalls(page))[1];
  assert.equal(unstage.args.expected, INDEX_WITH_EXTRA);
  assert.equal(unstage.args.text, MARCADORES_INDEX, 'o índice volta ao de antes');

  // O bloco que já vinha preparado (linha 3): Tirar do preparo deixa o índice igual ao HEAD.
  await clickMarker(page, 3);
  await eventually(async () => assert.equal((await openBlock(page))?.buttons[0], 'Tirar do preparo'));
  await blockButton(page, 'Tirar do preparo').click();
  await eventually(async () => assert.equal(simplify(await gutterMarks(page)).find((m) => m.line === 3)?.staged, false, 'linha 3 cheia'));
  assert.equal((await writeCalls(page))[2].args.text, MARCADORES_HEAD);
});

scenario('navegação: setas, Alt+F5 e Shift+Alt+F5 vão entre as alterações com o bloco aberto e dão a volta; menu de contexto também', async ({ page }) => {
  await openFile(page, 'src', 'marcadores.ts');
  await eventually(async () => assert.deepEqual(simplify(await gutterMarks(page)), EXPECTED));
  const p = await columnPoint(page, 1, 1);
  await page.mouse.click(p.x + 1, (p.top + p.bottom) / 2);
  // O título sem o " · preparada" do bloco 1 (o da fixture que já vem no índice).
  const state = async () => ({ title: (await openBlock(page))?.title.replace(' · preparada', '') ?? null, line: await cursorLine(page) });
  await page.keyboard.press('Alt+F5');
  assert.deepEqual(await state(), { title: 'Alteração 1 de 4', line: 3 });
  await page.keyboard.press('Alt+F5');
  assert.deepEqual(await state(), { title: 'Alteração 2 de 4', line: 8 });
  await page.keyboard.press('Alt+F5');
  assert.deepEqual(await state(), { title: 'Alteração 3 de 4', line: 15 });
  await page.keyboard.press('Alt+F5');
  assert.deepEqual(await state(), { title: 'Alteração 4 de 4', line: 22 });
  await page.keyboard.press('Alt+F5');
  assert.deepEqual(await state(), { title: 'Alteração 1 de 4', line: 3 }, 'Alt+F5 no último volta ao primeiro');
  await page.keyboard.press('Shift+Alt+F5');
  assert.deepEqual(await state(), { title: 'Alteração 4 de 4', line: 22 }, 'Shift+Alt+F5 no primeiro vai ao último');
  await blockButton(page, 'Próxima alteração').click();
  assert.deepEqual(await state(), { title: 'Alteração 1 de 4', line: 3 }, 'seta para baixo dá a volta');
  await blockButton(page, 'Alteração anterior').click();
  assert.deepEqual(await state(), { title: 'Alteração 4 de 4', line: 22 }, 'seta para cima dá a volta');
  assert.equal(await page.locator('.cm-gitHunk').count(), 1, 'um bloco aberto por vez');
  await blockButton(page, 'Fechar').click();
  assert.equal(await openBlock(page), null, 'o X fecha');

  // Menu de contexto: "Próxima Alteração" com o atalho, a partir do cursor (linha 22): dá a volta para a primeira.
  await page.locator('.cm-content:visible').first().click({ button: 'right', position: { x: 200, y: 5 } });
  const next = page.getByRole('menuitem', { name: /Próxima Alteração/ });
  await next.waitFor();
  assert.match(await next.textContent(), /Alt\+F5/);
  assert.match(await page.getByRole('menuitem', { name: /Alteração Anterior/ }).textContent(), /Shift\+Alt\+F5/);
  await next.click();
  await eventually(async () => assert.equal((await state()).title, 'Alteração 1 de 4'));
});
