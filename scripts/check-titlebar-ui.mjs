// QA dos controles da janela na barra de título (minimizar, maximizar/restaurar, fechar), em Chrome headless contra o app simulado
// (src/mock.ts, só no `vite` de desenvolvimento). Confere medidas, cores, hover, ícone centralizado, ausência de arrasto nos botões
// e o handler de fechar (o simulado troca o corpo por "Janela fechada (simulado)", sem fechar nada de verdade).
// Minimizar, maximizar/restaurar e o estado maximizado usam o TitleBar real com o transporte da janela substituído só neste teste
// (`windowHost`): contador, estado simulado e resize emitido. Não prova o efeito da janela nativa do Tauri (minimizar, maximizar, redimensionar).
// Uso: node scripts/check-titlebar-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
// Capturas em .playwright-mcp/titlebar-controls-*.png a cada execução: recorte para revisão visual, não asserção.
import assert from 'node:assert/strict';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { uiSuite, OUT } from './editor-ui-harness.mjs';

const { scenario, run } = uiSuite('titlebar-ui');
const WINDOW_LABELS = ['Minimizar', 'Maximizar', 'Restaurar', 'Fechar'];
const button = (page, name) => page.getByRole('button', { name, exact: true });

// Medidas, cores e arrasto dos controles e do cabeçalho, lidos do DOM real (sem assumir o código).
function measure(page) {
  return page.evaluate((labels) => {
    const header = document.querySelector('header');
    const hr = header.getBoundingClientRect();
    const border = parseFloat(getComputedStyle(header).borderBottomWidth);
    const buttons = labels.map((n) => document.querySelector(`button[aria-label="${n}"]`)).filter(Boolean);
    return {
      header: { top: hr.top, right: hr.right, height: hr.height, border },
      buttons: buttons.map((b) => {
        const r = b.getBoundingClientRect();
        const svg = b.querySelector('svg');
        const s = svg.getBoundingClientRect();
        const cs = getComputedStyle(b);
        return {
          label: b.getAttribute('aria-label'),
          top: r.top, left: r.left, right: r.right, width: r.width, height: r.height,
          svgWidth: s.width, svgHeight: s.height,
          svgCenter: { x: s.left + s.width / 2, y: s.top + s.height / 2 },
          btnCenter: { x: r.left + r.width / 2, y: r.top + r.height / 2 },
          color: cs.color, background: cs.backgroundColor, transition: cs.transitionDuration,
          drag: [b, svg, b.parentElement].some((el) => el.hasAttribute('data-tauri-drag-region')),
        };
      }),
    };
  }, WINDOW_LABELS);
}

// "rgba(r, g, b, a)" ou "rgb(r, g, b)" → [r, g, b, a]
const rgba = (css) => { const n = css.match(/[\d.]+/g).map(Number); return n.length === 3 ? [...n, 1] : n; };
function assertColor(actual, expected, what) {
  const a = rgba(actual);
  const e = rgba(expected);
  assert.deepEqual(a.slice(0, 3), e.slice(0, 3), `${what}: cor ${actual}, esperado ${expected}`);
  assert.ok(Math.abs(a[3] - e[3]) < 0.005, `${what}: alfa ${actual}, esperado ${expected}`);
}

// Clip em volta dos três controles (com margem), para o recorte de revisão.
async function controlsClip(page) {
  const { buttons } = await measure(page);
  const x = Math.min(...buttons.map((b) => b.left)) - 8;
  const y = Math.min(...buttons.map((b) => b.top)) - 8;
  const right = Math.max(...buttons.map((b) => b.right)) + 8;
  const bottom = Math.max(...buttons.map((b) => b.top + b.height)) + 8;
  return { x, y, width: right - x, height: bottom - y };
}

// Só neste teste: a resposta de src/mock.ts ganha o fim com a captura do handler de resize. O mock assina onResized com um no-op,
// e o TitleBar assina no mount; a captura precisa estar no módulo antes de a página montar a barra (por isso o reload abaixo).
// Começa com quebra de linha: o Vite termina o módulo com um comentário `//# sourceMappingURL`, e sem ela o trecho ficaria comentado.
const RESIZE_CAPTURE = '\nwindow.__resizeHandlers = [];\nmock.onResized = async (handler) => { window.__resizeHandlers.push(handler); return () => {}; };\n';
const icon = (page, label) => page.locator(`button[aria-label="${label}"] svg`).evaluate((el) => el.outerHTML);

// Transporte de janela de teste sobre o objeto `host()` real (o mesmo `mock`): contador de minimizar, estado de maximizar e
// resize emitido para o TitleBar, que relê isMaximized. Não mexe no código de produção.
async function windowHost(page) {
  await page.route((url) => url.pathname === '/src/mock.ts', async (route) => {
    const response = await route.fetch();
    await route.fulfill({ status: 200, contentType: 'text/javascript', body: (await response.text()) + RESIZE_CAPTURE });
  });
  await page.reload();
  await page.getByRole('button', { name: 'Escolher pasta do projeto' }).click();
  await page.getByRole('navigation', { name: 'Telas' }).waitFor();
  await page.evaluate(async () => {
    const { host } = await import('/src/backend.ts');
    const h = host();
    window.__win = { minimize: 0, toggle: 0, maximized: false };
    h.minimize = async () => { window.__win.minimize += 1; };
    h.toggleMaximize = async () => {
      window.__win.toggle += 1;
      window.__win.maximized = !window.__win.maximized;
      window.__resizeHandlers.forEach((handler) => handler());
    };
    h.isMaximized = async () => window.__win.maximized;
  });
}

scenario('três controles: 46 px de largura, 35 px úteis, colados à direita, ícone de 16 px centralizado, sem arrasto', async ({ page }) => {
  const m = await measure(page);
  assert.equal(m.buttons.length, 3, 'minimizar, maximizar ou restaurar, e fechar');
  assert.equal(m.header.height, 36, 'barra com 36 px no total');
  assert.equal(m.header.border, 1, 'borda inferior de 1 px');
  const useful = m.header.height - m.header.border;
  m.buttons.forEach((b, i) => {
    assert.ok(Math.abs(b.width - 46) < 0.5, `${b.label}: largura ${b.width}, esperado 46`);
    assert.ok(Math.abs(b.height - useful) < 0.5, `${b.label}: altura ${b.height}, esperado ${useful}`);
    assert.ok(Math.abs(b.top - m.header.top) < 0.5, `${b.label}: topo colado no cabeçalho`);
    assert.ok(Math.abs(b.svgWidth - 16) < 0.5 && Math.abs(b.svgHeight - 16) < 0.5, `${b.label}: ícone ${b.svgWidth}×${b.svgHeight}, esperado 16×16`);
    assert.ok(Math.abs(b.svgCenter.x - b.btnCenter.x) < 0.5 && Math.abs(b.svgCenter.y - b.btnCenter.y) < 0.5, `${b.label}: ícone fora do centro`);
    assert.equal(b.drag, false, `${b.label}: não pode ser região de arrasto`);
    assert.equal(b.transition, '0s', `${b.label}: sem transição, como na referência`);
    if (i > 0) assert.equal(m.buttons[i - 1].right, b.left, `${b.label}: sem espaço entre os controles`);
  });
  assert.equal(m.buttons.at(-1).right, m.header.right, 'Fechar colado à borda direita da janela');
  // Área passiva da barra segue arrastável: o título leva o atributo.
  assert.equal(await page.locator('header').getByText('Orquestrador', { exact: true }).evaluate((el) => el.hasAttribute('data-tauri-drag-region')), true, 'título arrastável');
});

scenario('estado normal: cor #CCCCCC, fundo transparente', async ({ page }) => {
  const m = await measure(page);
  for (const b of m.buttons) {
    assertColor(b.color, 'rgb(204, 204, 204)', `${b.label} (texto)`);
    assertColor(b.background, 'rgba(0, 0, 0, 0)', `${b.label} (fundo)`);
  }
});

scenario('hover: branco a 10% nos três, Fechar vai a #E81123 (90%) com o X branco', async ({ page }) => {
  for (const name of ['Minimizar', 'Maximizar', 'Fechar']) {
    await button(page, name).hover();
    const m = await measure(page);
    const b = m.buttons.find((x) => x.label === name);
    if (name === 'Fechar') {
      assertColor(b.background, 'rgba(232, 17, 35, 0.902)', 'Fechar (fundo no hover)');
      assertColor(b.color, 'rgb(255, 255, 255)', 'Fechar (X no hover)');
    } else {
      assertColor(b.background, 'rgba(255, 255, 255, 0.102)', `${name} (fundo no hover)`);
      assertColor(b.color, 'rgb(204, 204, 204)', `${name} (ícone no hover)`);
    }
  }
  await page.mouse.move(0, 0);
  const m = await measure(page);
  for (const b of m.buttons) assertColor(b.background, 'rgba(0, 0, 0, 0)', `${b.label} (fundo sem hover)`);
});

scenario('tooltip existente mostra o rótulo do controle', async ({ page }) => {
  // O botão não tem texto visível: o rótulo "Fechar" só aparece na dica.
  await button(page, 'Fechar').hover();
  await page.getByText('Fechar', { exact: true }).waitFor();
});

scenario('Fechar chama o handler de fechar (o simulado mostra a janela fechada)', async ({ page }) => {
  await button(page, 'Fechar').click();
  await page.getByText('Janela fechada (simulado).').waitFor();
});

scenario('recorte dos controles para revisão visual (normal e Fechar em hover)', async ({ page }) => {
  mkdirSync(OUT, { recursive: true });
  const clip = await controlsClip(page);
  await page.mouse.move(0, 0);
  await page.waitForTimeout(50);
  await page.screenshot({ path: join(OUT, 'titlebar-controls-normal.png'), clip });
  await button(page, 'Fechar').hover();
  await page.waitForTimeout(50);
  await page.screenshot({ path: join(OUT, 'titlebar-controls-fechar-hover.png'), clip });
});

scenario('minimizar: clique real chama minimize uma vez e não alterna nada', async ({ page }) => {
  await windowHost(page);
  await button(page, 'Minimizar').click();
  assert.deepEqual(await page.evaluate(() => window.__win), { minimize: 1, toggle: 0, maximized: false });
});

scenario('maximizar e restaurar: clique troca rótulo e desenho, e Restaurar volta ao original', async ({ page }) => {
  await windowHost(page);
  const maximizeIcon = await icon(page, 'Maximizar');
  await button(page, 'Maximizar').click();
  await button(page, 'Restaurar').waitFor();
  assert.equal(await page.getByRole('button', { name: 'Maximizar', exact: true }).count(), 0, 'sem Maximizar enquanto maximizado');
  assert.notEqual(await icon(page, 'Restaurar'), maximizeIcon, 'desenho de Restaurar difere do de Maximizar');
  assert.deepEqual(await page.evaluate(() => window.__win), { minimize: 0, toggle: 1, maximized: true });
  await button(page, 'Restaurar').click();
  await button(page, 'Maximizar').waitFor();
  assert.equal(await icon(page, 'Maximizar'), maximizeIcon, 'Maximizar volta ao desenho original');
  assert.deepEqual(await page.evaluate(() => window.__win), { minimize: 0, toggle: 2, maximized: false });
});

scenario('estado maximizado: Restaurar e Fechar em hover, com recorte para revisão visual', async ({ page }) => {
  await windowHost(page);
  await button(page, 'Maximizar').click();
  await button(page, 'Restaurar').waitFor();
  mkdirSync(OUT, { recursive: true });
  const clip = await controlsClip(page);
  await button(page, 'Restaurar').hover();
  let m = await measure(page);
  assertColor(m.buttons.find((b) => b.label === 'Restaurar').background, 'rgba(255, 255, 255, 0.102)', 'Restaurar (fundo no hover)');
  await page.waitForTimeout(50);
  await page.screenshot({ path: join(OUT, 'titlebar-controls-restaurar-hover.png'), clip });
  await button(page, 'Fechar').hover();
  m = await measure(page);
  const close = m.buttons.find((b) => b.label === 'Fechar');
  assertColor(close.background, 'rgba(232, 17, 35, 0.902)', 'Fechar (fundo no hover, maximizado)');
  assertColor(close.color, 'rgb(255, 255, 255)', 'Fechar (X no hover, maximizado)');
  await page.waitForTimeout(50);
  await page.screenshot({ path: join(OUT, 'titlebar-controls-restaurar-fechar-hover.png'), clip });
});

const exitCode = await run();
process.exit(exitCode);
