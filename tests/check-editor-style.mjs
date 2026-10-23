// Paridade visual do editor central com o Antigravity (Dark Modern), em Chrome headless contra o app simulado (src/backend/mock.ts).
// Os valores esperados vêm dos arquivos do Antigravity (ver docs/interface.md, "Cores e fontes do editor"). Cada cenário abre um
// arquivo pela árvore, espera a linguagem carregar (é assíncrona) e confere estilos computados e geometria, não aparência: a
// comparação pixel a pixel não é feita aqui. Capturas em .playwright-mcp/editor-style-*.png para leitura humana.
// Uso: node tests/check-editor-style.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
import { join } from 'node:path';
import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { OUT, ROOT, uiSuite, treeItem } from './editor-ui-harness.mjs';

// Fixtures equivalentes a docs/design-system.md (heading, negrito, código inline, lista), TSX e JSON com aninhamento,
// texto dentro de string, comentário, `${}` de template e um `)` sem par.
const MD = '# Design system\n\nA interface usa **Lobe UI** com `npm run build` e `Consolas`.\n\n- item com `código`\n- outro item\n';
const TSX = [
  'const config = { list: [1, (2 + f({ a: "(" })) ], nome: `${(x)}` }; // ( comentário {',
  'if (a) { g(`${b}`, [c]); }',
  'x = 1); // ok',
].join('\n');
const JSON_TEXT = '{"a": [1, {"b": [true]}], "c": {}}\n';
// Documento longo com `{` e `(` na primeira tela e os fechamentos só no fim (fora da primeira tela). Parênteses com expressão válida,
// sem pares dentro, para o parser não errar a árvore no meio.
const LONG_TSX = [
  'const obj = {',
  '  alvo: (',
  ...Array.from({ length: 200 }, (_, i) => `    ${i} +`),
  '    0',
  '  ),',
  '};',
  '',
].join('\n');

// Cores do Antigravity: editorBracketHighlight 1 a 3 (ouro, orquídea, azul) e unexpectedBracket (vermelho translúcido).
const LEVEL = { 'cm-bracket-1': 'g', 'cm-bracket-2': 'o', 'cm-bracket-3': 'b', 'cm-bracket-unexpected': 'u' };
const COLOR = { g: 'rgb(255, 215, 0)', o: 'rgb(218, 112, 214)', b: 'rgb(23, 159, 255)', u: 'rgba(255, 18, 18, 0.8)' };

// Sequência esperada: um código por (, ), [, ], {, } em ordem do documento, inclusive os de string e comentário, que saem '-'.
// Derivada à mão da ordem dos delimitadores e do aninhamento (profundidade 0 = g, 1 = o, 2 = b, 3 = g...):
//  linha 1: { g · [ o · ( b · ( g · { o · "(" - · } o · ) g · ) b · ] o · ${ o (abre, profundidade 1) · ( b · ) b · } o (fecha ${)
//           · } g · ( - e { - (comentário). São 17 caracteres.
//  linha 2: if ( g ) g · { g · g( o · ${ b · } b (fecha ${) · [ b · ] b · ) o · } g.
//  linha 3: `)` sobra sem abertura: u.
const EXPECTED_TSX = 'gobgo-ogboobbog--' + 'gggobbbbog' + 'u';
const EXPECTED_JSON = 'gobggbooog';
const EXPECTED_LONG_TOP = 'go'; // `{` e `(` na primeira tela
const EXPECTED_LONG_END = 'og'; // `)` (profundidade 1, o) e `}` (profundidade 0, g) depois de Ctrl+End

// Espera um cenário assíncrono (linguagem carregando, parser em partes) se resolver: tenta por até 3 s.
async function eventually(fn) {
  let last;
  for (let i = 0; i < 30; i++) {
    try { return await fn(); } catch (e) { last = e; await new Promise((r) => setTimeout(r, 100)); }
  }
  throw last;
}

async function open(page, name, content) {
  await page.evaluate(([n, c]) => window.__mock.addFile(n, c), [name, content]);
  await treeItem(page, name).dblclick();
  await page.locator('.cm-content:visible').first().waitFor();
}

// Cada ( ) [ ] { } do DOM, em ordem, com o código de cor e a cor calculada do texto que o contém. O `${` de template conta só o `{`
// (o `$` não é par); o caractere fica dentro do mesmo marcador do par, então recebe o nível dele.
function brackets(page) {
  return page.evaluate((levels) => {
    const content = document.querySelector('.cm-content:not([aria-hidden])');
    const walker = document.createTreeWalker(content, NodeFilter.SHOW_TEXT);
    const out = [];
    let node;
    while ((node = walker.nextNode())) {
      for (const ch of node.textContent) {
        if (!'()[]{}'.includes(ch)) continue;
        const el = node.parentElement;
        const mark = el.closest('[class*="cm-bracket-"]');
        const cls = mark ? [...mark.classList].find((c) => c.startsWith('cm-bracket-')) : null;
        out.push({ ch, code: levels[cls] ?? '-', color: getComputedStyle(el).color });
      }
    }
    return out;
  }, LEVEL);
}

async function assertBrackets(page, expected) {
  await eventually(async () => {
    const report = await brackets(page);
    assert.equal(report.map((r) => r.code).join(''), expected, 'sequência de níveis (ordem do documento)');
  });
  const report = await brackets(page);
  for (const r of report) {
    if (r.code !== '-') assert.equal(r.color, COLOR[r.code], `cor de "${r.ch}" no nível ${r.code}`);
  }
  return report;
}

// Cor e peso do nó de texto que contém o trecho `t`.
function styleOf(page, t) {
  return page.evaluate((text) => {
    const content = document.querySelector('.cm-content:not([aria-hidden])');
    const walker = document.createTreeWalker(content, NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) {
      if (node.textContent.includes(text)) {
        const cs = getComputedStyle(node.parentElement);
        return { color: cs.color, weight: cs.fontWeight, background: cs.backgroundColor };
      }
    }
    return null;
  }, t);
}

// Marcadores de lista (`.cm-list-mark`): texto e cor de cada um, na ordem do documento.
function listMarks(page) {
  return page.evaluate(() => [...document.querySelectorAll('.cm-list-mark')].map((el) => ({
    text: el.textContent, color: getComputedStyle(el).color,
  })));
}

// Posição e largura de cada caractere da primeira linha (arredondadas). Se o destaque do par mudar a geometria do texto, muda aqui.
function glyphGeometry(page) {
  return page.evaluate(() => {
    const line = document.querySelector('.cm-content:not([aria-hidden]) .cm-line');
    const walker = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
    const out = [];
    let node;
    const r2 = (n) => Math.round(n * 100) / 100;
    while ((node = walker.nextNode())) {
      for (let i = 0; i < node.length; i++) {
        const range = document.createRange();
        range.setStart(node, i);
        range.setEnd(node, i + 1);
        const b = range.getBoundingClientRect();
        out.push([r2(b.left), r2(b.width)]);
      }
    }
    return out;
  });
}

const scenario = uiSuite('editor-style');

scenario('Markdown: fonte, medidas, fundo, medianiz, seleção e cores de markup como no Antigravity', async ({ page }) => {
  await open(page, 'estilo-editor.md', MD);

  const base = await page.evaluate(() => {
    const s = getComputedStyle(document.querySelector('.cm-scroller'));
    const ed = getComputedStyle(document.querySelector('.cm-editor'));
    const gutter = document.querySelector('.cm-lineNumbers .cm-gutterElement');
    const active = document.querySelector('.cm-activeLineGutter');
    return {
      family: s.fontFamily, size: s.fontSize, line: s.lineHeight, weight: s.fontWeight, ligatures: s.fontVariantLigatures,
      editorBg: ed.backgroundColor, editorColor: ed.color,
      number: gutter ? getComputedStyle(gutter).color : null,
      activeNumber: active ? getComputedStyle(active).color : null,
      lineH: document.querySelector('.cm-line').getBoundingClientRect().height,
    };
  });
  assert.match(base.family, /^Consolas\b/, `fonte: ${base.family}`);
  assert.equal(base.size, '14px');
  assert.equal(base.line, '19px');
  assert.equal(base.weight, '400');
  assert.equal(base.lineH, 19, 'altura da linha');
  assert.equal(base.editorBg, 'rgb(31, 31, 31)', 'fundo do editor (#1F1F1F)');
  assert.equal(base.editorColor, 'rgb(204, 204, 204)', 'texto (#CCCCCC)');
  assert.equal(base.number, 'rgb(110, 118, 129)', 'números (#6E7681)');
  assert.equal(base.activeNumber, 'rgb(204, 204, 204)', 'número da linha ativa (#CCCCCC)');

  const heading = await eventually(async () => {
    const s = await styleOf(page, 'Design system');
    assert.equal(s?.color, 'rgb(86, 156, 214)', 'heading (#569CD6)');
    return s;
  });
  assert.equal(heading.weight, '700', 'heading em negrito');
  assert.equal((await styleOf(page, 'Lobe UI')).color, 'rgb(86, 156, 214)', 'negrito de texto (#569CD6)');
  assert.equal((await styleOf(page, 'Lobe UI')).weight, '700', 'negrito de texto');
  assert.equal((await styleOf(page, 'npm run build')).color, 'rgb(206, 145, 120)', 'código inline (#CE9178)');
  assert.equal((await styleOf(page, 'A interface usa')).color, 'rgb(204, 204, 204)', 'texto comum (#CCCCCC)');

  // Lista: o marcador (`-`) é azul-claro (#6796E6) e é o único que ganha a cor; o texto do item e o código dentro dele não.
  const marks = await eventually(async () => {
    const found = await listMarks(page);
    assert.deepEqual(found.map((m) => m.text), ['-', '-'], 'um marcador por item da lista');
    return found;
  });
  for (const m of marks) assert.equal(m.color, 'rgb(103, 150, 230)', `marcador "${m.text}" (#6796E6)`);
  assert.equal((await styleOf(page, 'outro item')).color, 'rgb(204, 204, 204)', 'corpo do item da lista (#CCCCCC), não o marcador');
  assert.equal((await styleOf(page, 'item com')).color, 'rgb(204, 204, 204)', 'corpo do item com código (#CCCCCC)');
  assert.equal((await styleOf(page, 'código')).color, 'rgb(206, 145, 120)', 'código inline dentro do item da lista (#CE9178)');

  // Seleção: com foco, #264F78; sem foco, #3A3D41 (inactiveSelectionBackground). Medido pelo fundo do .cm-selectionBackground.
  await page.locator('.cm-content:visible').first().click();
  await page.keyboard.press('Control+Home');
  await page.keyboard.press('Shift+End');
  const focused = await page.evaluate(() => getComputedStyle(document.querySelector('.cm-selectionBackground')).backgroundColor);
  assert.equal(focused, 'rgb(38, 79, 120)', 'seleção com foco (#264F78)');
  await page.evaluate(() => document.activeElement?.blur());
  await page.waitForFunction(() => !document.querySelector('.cm-focused'), null, { timeout: 3000 });
  const unfocused = await page.evaluate(() => getComputedStyle(document.querySelector('.cm-selectionBackground')).backgroundColor);
  assert.equal(unfocused, 'rgb(58, 61, 65)', 'seleção sem foco (#3A3D41)');
  await page.screenshot({ path: join(OUT, 'editor-style-markdown.png') });
});

scenario('cursor na coluna 0 inteiro à direita da medianiz, sem mover o texto', async ({ page }) => {
  await open(page, 'cursor-coluna-0.md', '# x\n\nweb\n');
  const line = await page.locator('.cm-content:visible .cm-line').nth(2).boundingBox();
  await page.mouse.click(line.x + 1, line.y + 5);
  await page.locator('.cm-cursor-primary').waitFor();
  const g = await page.evaluate(() => ({
    cursor: document.querySelector('.cm-cursor-primary').getBoundingClientRect().left,
    gutter: document.querySelector('.cm-gutters').getBoundingClientRect().right,
    content: document.querySelector('.cm-content').getBoundingClientRect().left,
  }));
  assert.ok(g.cursor >= g.gutter, `cursor em x ${g.cursor} sob a medianiz (até x ${g.gutter})`);
  assert.equal(g.content, g.gutter, 'texto colado à medianiz, sem recuo');
});

scenario('TSX: pares coloridos pelo nível, fora de strings e comentários, `${` como par, e `)` sem par em vermelho', async ({ page }) => {
  await open(page, 'estilo-editor.tsx', TSX);
  await assertBrackets(page, EXPECTED_TSX);
  await page.screenshot({ path: join(OUT, 'editor-style-tsx.png') });
});

scenario('JSON: pares aninhados coloridos pelo nível', async ({ page }) => {
  await open(page, 'estilo-editor.json', JSON_TEXT);
  await assertBrackets(page, EXPECTED_JSON);
  await page.screenshot({ path: join(OUT, 'editor-style-json.png') });
});

scenario('par sob o cursor: caixa do Antigravity sem mudar a geometria do texto; digitar recolore o arquivo', async ({ page }) => {
  await open(page, 'estilo-editor.tsx', TSX);
  await assertBrackets(page, EXPECTED_TSX);
  await page.locator('.cm-content:visible').first().click();
  await page.keyboard.press('Control+Home');
  const before = await glyphGeometry(page); // cursor no começo da linha, sem par destacado
  assert.ok(before.length > 40, `medida da linha 1 tem os caracteres (${before.length})`);
  for (let i = 0; i < 16; i++) await page.keyboard.press('ArrowRight'); // logo antes do `{` da linha 1
  const pair = await eventually(async () => {
    const found = await page.evaluate(() => [...document.querySelectorAll('.cm-matchingBracket')].map((el) => {
      const cs = getComputedStyle(el);
      return {
        text: el.textContent, bg: cs.backgroundColor, shadow: cs.boxShadow, outline: cs.outlineStyle,
        borderW: cs.borderTopWidth, borderS: cs.borderTopStyle,
      };
    }));
    assert.equal(found.length, 2, 'par destacado');
    return found;
  });
  assert.deepEqual(pair.map((p) => p.text).sort(), ['{', '}'], 'par é { e }');
  for (const p of pair) {
    const [r, g, b, a] = p.bg.match(/[\d.]+/g).map(Number);
    assert.deepEqual([r, g, b], [0, 100, 0], `fundo do par (${p.bg})`);
    assert.ok(a > 0.09 && a < 0.11, `opacidade do fundo do par (${p.bg})`);
    // A caixa #888 é uma sombra interna: borda real aumentaria a largura inline do caractere em 2 px.
    assert.equal(p.borderW, '0px', 'sem borda real no par');
    // Chrome serializa a sombra com `inset` no fim: é a mesma sombra, só a ordem do texto.
    assert.match(p.shadow, /^rgb\(136, 136, 136\) 0px 0px 0px 1px inset$/, `caixa do par por dentro (#888): ${p.shadow}`);
    assert.equal(p.outline, 'none', 'sem contorno');
  }
  // Geometria: a posição e a largura de cada caractere da linha 1 são as mesmas com o par destacado.
  const after = await glyphGeometry(page);
  assert.deepEqual(after, before, 'geometria do texto muda com o par destacado');
  // Digitar no fim do arquivo: o `insertText` não aciona o fechamento automático de pares.
  await page.keyboard.press('Control+End');
  await page.keyboard.insertText('\nif (q) {}');
  await eventually(async () => {
    const report = await brackets(page);
    assert.equal(report.map((r) => r.code).join(''), EXPECTED_TSX + 'gggg', 'recoloração depois de digitar');
  });
  await page.screenshot({ path: join(OUT, 'editor-style-cursor.png') });
});

scenario('documento longo: `{` e `(` fora da tela até o fim; a rolagem atualiza parser e cores de nível', async ({ page }) => {
  await open(page, 'estilo-longo.tsx', LONG_TSX);
  // Primeira tela: a abertura está visível, o fechamento não.
  await assertBrackets(page, EXPECTED_LONG_TOP);
  // Ctrl+End rola até o fim: a abertura sai da tela e o parser precisa cobrir o trecho novo antes das cores aparecerem.
  await page.locator('.cm-content:visible').first().click();
  await page.keyboard.press('Control+End');
  await assertBrackets(page, EXPECTED_LONG_END);
  await page.locator('.cm-editor:visible').first().screenshot({ path: join(OUT, 'editor-style-scroll.png') });
});

scenario('docs/design-system.md real, aberto no mock: lista, código inline e marcador no painel', async ({ page }) => {
  const text = readFileSync(join(ROOT, 'docs', 'design-system.md'), 'utf8');
  await open(page, 'design-system.md', text);
  await page.locator('.cm-content:visible').first().click();
  await page.keyboard.press('Control+Home');
  // Desce até a lista de "Regras" (linha 47): o cursor rola a área sozinho. Com a quebra de linha, ArrowDown anda por linha
  // visual, então o laço para pelo número da linha ativa na medianiz.
  const activeLine = () => page.evaluate(() => Number(document.querySelector('.cm-activeLineGutter')?.textContent));
  for (let i = 0; i < 400 && (await activeLine()) < 47; i++) await page.keyboard.press('ArrowDown');
  assert.equal(await activeLine(), 47, 'cursor na linha 47');
  const marks = await eventually(async () => {
    const found = await listMarks(page);
    assert.ok(found.length > 0, 'marcadores de lista visíveis');
    return found;
  });
  for (const m of marks) assert.equal(m.color, 'rgb(103, 150, 230)', 'marcador (#6796E6)');
  assert.equal((await styleOf(page, 'Concorrência dos executores')).color, 'rgb(204, 204, 204)', 'corpo da lista (#CCCCCC)');
  assert.equal((await styleOf(page, 'src/window/titlebar.tsx')).color, 'rgb(206, 145, 120)', 'código inline dentro da lista (#CE9178)');
  await page.locator('.cm-editor:visible').first().screenshot({ path: join(OUT, 'editor-style-design-system.png') });
});

scenario('linha longa quebra na largura do editor, sem rolagem horizontal', async ({ page }) => {
  await open(page, 'linha-longa.txt', `${'palavra '.repeat(400)}\nfim\n`);
  const g = await page.evaluate(() => {
    const content = document.querySelector('.cm-content:not([aria-hidden])');
    const scroller = document.querySelector('.cm-scroller');
    return {
      wrapping: content.classList.contains('cm-lineWrapping'), whiteSpace: getComputedStyle(content).whiteSpace,
      overflow: scroller.scrollWidth - scroller.clientWidth,
      lineH: content.querySelector('.cm-line').getBoundingClientRect().height,
    };
  });
  assert.ok(g.wrapping, 'classe cm-lineWrapping no conteúdo');
  // O CodeMirror usa `break-spaces` onde o navegador suporta e `pre-wrap` nos demais.
  assert.match(g.whiteSpace, /^(pre-wrap|break-spaces)$/, `white-space: ${g.whiteSpace}`);
  assert.ok(g.overflow <= 0, `rolagem horizontal de ${g.overflow} px`);
  assert.ok(g.lineH > 19, `linha longa em mais de uma linha visual (${g.lineH} px)`);
  await page.locator('.cm-editor:visible').first().screenshot({ path: join(OUT, 'editor-style-wrap.png') });
});
