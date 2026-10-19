// QA dos marcadores de mudança do Git no editor (src/editor/git-gutter.ts), em Chrome headless contra o app simulado (src/backend/mock.ts).
// O mock entra só na fronteira de I/O: `git_file_base` devolve o HEAD e o índice de `src/marcadores.ts` (fixture em mock-fixtures.ts: um bloco
// modificado em stage na linha 3; fora do índice, duas linhas acrescentadas em 8–9, a `v12` apagada antes da linha 15 e a linha 22 modificada).
// Confere pela DOM o gutter (tipo, linha, cheia × vazada), a geometria do texto, as marcas na barra de rolagem, a faixa do minimap, a digitação
// ao vivo com Desfazer, a nova leitura da base depois de salvar, e a ausência de marcadores em arquivo não rastreado e em rascunho.
// Uso: node scripts/check-git-gutter-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
// Capturas em .playwright-mcp/git-gutter-*.png: evidência para leitura humana, não asserção.
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { OUT, uiSuite, expand, treeItem, columnPoint } from './editor-ui-harness.mjs';

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
});
