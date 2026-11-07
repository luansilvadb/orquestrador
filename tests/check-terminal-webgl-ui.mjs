// QA do renderizador WebGL do terminal, em Chrome headless contra o app simulado: só o terminal visível tem canvas WebGL (os escondidos
// voltam ao DOM, por causa do limite de ~16 contextos), e sem WebGL2 o terminal segue no DOM. Uso: node tests/check-terminal-webgl-ui.mjs
import assert from 'node:assert/strict';
import { openApp, uiSuite } from './editor-ui-harness.mjs';

const webgl = uiSuite('terminal-webgl-ui', { launch: { args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] } });
const dom = uiSuite('terminal-dom-ui', { launch: { args: ['--disable-3d-apis'] } });

// `.xterm-link-layer` é o canvas que o addon WebGL põe em cada terminal que o usa; o renderizador DOM não cria nenhum.
const GL = '.xterm-link-layer';
const count = (page) => page.locator(GL).count();
const waitCount = (page, n) => page.waitForFunction(([sel, c]) => document.querySelectorAll(sel).length === c, [GL, n]);

// Um terminal por vez, esperando o anterior largar o contexto (como o usuário abre): o Chromium avisa no console acima de ~16 contextos vivos.
async function openShells(page, n, gl = true) {
  await page.keyboard.press('Control+Backquote');
  await page.locator('.xterm').first().waitFor();
  for (let i = 1; i < n; i++) {
    await page.getByRole('button', { name: 'Novo terminal' }).click();
    await page.waitForFunction((c) => document.querySelectorAll('.xterm').length === c, i + 1);
    if (gl && n > 2) await waitCount(page, 1);
  }
}

webgl('WebGL: só o terminal visível tem canvas; trocar de aba passa o contexto; muitos terminais não passam de um contexto', async ({ page }) => {
  await openShells(page, 20);
  await waitCount(page, 1);
  assert.equal(await page.locator('.xterm').count(), 20, '20 terminais montados');
  const tabs = page.locator('[data-shell] [role="tab"]');
  await tabs.first().click();
  await waitCount(page, 1);
  const visible = await page.evaluate(() => [...document.querySelectorAll('.xterm')].filter((el) => getComputedStyle(el).visibility === 'visible').length);
  assert.equal(visible, 1, 'um terminal visível');
  assert.equal(await page.locator('.xterm').evaluateAll((els) => els.filter((el) => getComputedStyle(el).visibility === 'visible' && el.querySelector('.xterm-link-layer')).length), 1, 'o visível é o que tem o canvas');
  // Esconde o painel (Ctrl+`): nenhum contexto fica preso; reabrir devolve o WebGL ao terminal visível.
  await page.keyboard.press('Control+Backquote');
  await waitCount(page, 0);
  await page.keyboard.press('Control+Backquote');
  await waitCount(page, 1);
});

dom('sem WebGL2: o terminal segue no DOM, com o texto em .xterm-rows', async ({ page }) => {
  await openShells(page, 2);
  await page.waitForFunction(() => [...document.querySelectorAll('.xterm-rows')].some((rows) => rows.textContent.includes('PS ')));
  assert.equal(await count(page), 0, 'nenhum canvas');
});

// Terminal escondido não renderiza: o xterm só pausa o RenderService quando o IntersectionObserver da tela dele diz que não intersecta, e
// `visibility: hidden` sozinho não muda isso. A aba inativa fica deslocada para fora do contêiner (que corta o overflow), com o mesmo tamanho.
dom('terminal escondido não gera mutação no DOM, nem com saída intensa; ao voltar, mostra o conteúdo atual sem mudar de tamanho', async ({ page }) => {
  // __orqMedir: gancho do TermView (term.ts) que entrega cada xterm; aqui conta as mutações de `.xterm-rows` (as linhas pintadas pelo renderizador
  // DOM; a barra de rolagem e o elemento de medição ficam de fora).
  await page.addInitScript(() => {
    window.__terms = [];
    window.__orqMedir = (id, term) => {
      const entry = { id, term, mutations: 0 };
      window.__terms.push(entry);
      new MutationObserver((records) => { entry.mutations += records.length; })
        .observe(term.element.querySelector('.xterm-rows'), { subtree: true, childList: true, characterData: true, attributes: true });
    };
  });
  await openApp(page);
  await openShells(page, 4, false);
  const state = () => page.evaluate(() => window.__terms.map(({ term, mutations }) => ({
    mutations, cols: term.cols, rows: term.rows,
    shown: getComputedStyle(term.element).visibility === 'visible',
  })));
  await page.waitForTimeout(500); // o IntersectionObserver avisa o xterm de forma assíncrona
  const before = await state();
  assert.equal(before.length, 4);
  assert.equal(before.filter((t) => t.shown).length, 1, 'um terminal visível');
  // Saída intensa em todos, o visível e os escondidos; a última linha de cada um é a marca.
  await page.evaluate(async () => {
    const chunk = Array.from({ length: 200 }, (_, i) => `linha ${i} ${'x'.repeat(60)}

`).join('');
    await Promise.all(window.__terms.map(({ term }, n) => new Promise((resolve) => {
      for (let i = 0; i < 20; i++) term.write(chunk);
      term.write(`MARCA-${n}

`, resolve);
    })));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
  const after = await state();
  const counts = after.map((t, i) => `${t.shown ? 'visível' : 'escondido'}=${t.mutations - before[i].mutations}`).join(' ');
  console.log(`mutações no DOM com saída intensa: ${counts}`);
  after.forEach((t, i) => {
    const grew = t.mutations - before[i].mutations;
    if (t.shown) assert.ok(grew > 0, 'o visível renderiza');
    else assert.equal(grew, 0, `o escondido ${i} não renderiza (${counts})`);
  });
  // Trocar de aba: o conteúdo atual aparece (a marca do terminal), sem mudar cols/rows (nenhum reflow).
  const tabs = page.locator('[data-shell] [role="tab"]');
  for (let i = 0; i < 4; i++) {
    await tabs.nth(i).click();
    await page.waitForFunction((k) => {
      const t = window.__terms[k];
      return getComputedStyle(t.term.element).visibility === 'visible' && t.term.element.querySelector('.xterm-rows').textContent.includes(`MARCA-${k}`);
    }, i);
  }
  const end = await state();
  assert.deepEqual(end.map((t) => [t.cols, t.rows]), after.map((t) => [t.cols, t.rows]), 'trocar de aba não redimensiona');
});
