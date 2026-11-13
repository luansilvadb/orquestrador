// QA do renderizador WebGL do terminal, em Chrome headless contra o app simulado: o terminal visível tem canvas WebGL, e só os últimos
// usados mantêm o seu (WEBGL_MAX em term.ts, por causa do limite de ~16 contextos); sem WebGL2 o terminal segue no DOM. Uso: node tests/check-terminal-webgl-ui.mjs
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { openApp, uiSuite } from './editor-ui-harness.mjs';

const webgl = uiSuite('terminal-webgl-ui', { launch: { args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] } });
const dom = uiSuite('terminal-dom-ui', { launch: { args: ['--disable-3d-apis'] } });

// `.xterm-link-layer` é o canvas que o addon WebGL põe em cada terminal que o usa; o renderizador DOM não cria nenhum.
const GL = '.xterm-link-layer';
const count = (page) => page.locator(GL).count();
const waitCount = (page, n) => page.waitForFunction(([sel, c]) => document.querySelectorAll(sel).length === c, [GL, n]);
const MAX = Number(readFileSync(new URL('../src/terminal/term.ts', import.meta.url), 'utf8').match(/export const WEBGL_MAX = (\d+)/)?.[1]);
if (!MAX) throw new Error('WEBGL_MAX não encontrado em src/terminal/term.ts');

// Abre o terminal na aba Orquestrador e cria `n` executores rodando (e0..e{n-1}).
async function openOrchestrator(page, n) {
  await page.getByRole('button', { name: 'Abrir terminal' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Abrir terminal' }).click();
  await page.getByRole('tab', { name: 'Orquestrador' }).waitFor();
  await page.evaluate((k) => { for (let i = 0; i < k; i++) window.__mock.addNode({
    project: 'project-1', id: `e${i}`, batch: 'b', index: i, title: `T${i}`, state: 'running', choice: { tool: 'claude', model: 'opus', effort: 'high' },
  }); }, n);
}

// Um terminal por vez (como o usuário abre): o Chromium avisa no console acima de ~16 contextos vivos.
async function openShells(page, n, gl = true) {
  await page.keyboard.press('Control+Backquote');
  await page.locator('.xterm').first().waitFor();
  for (let i = 1; i < n; i++) {
    await page.getByRole('button', { name: 'Novo terminal' }).click();
    await page.waitForFunction((c) => document.querySelectorAll('.xterm').length === c, i + 1);
    if (gl) await waitCount(page, Math.min(i + 1, MAX));
  }
}

webgl('WebGL: o terminal visível tem canvas; trocar de aba mantém os últimos usados; muitos terminais não passam do teto', async ({ page }) => {
  await openShells(page, 20);
  await waitCount(page, MAX);
  assert.equal(await page.locator('.xterm').count(), 20, '20 terminais montados');
  const tabs = page.locator('[data-shell] [role="tab"]');
  await tabs.first().click();
  await waitCount(page, MAX);
  const visible = await page.evaluate(() => [...document.querySelectorAll('.xterm')].filter((el) => getComputedStyle(el).visibility === 'visible').length);
  assert.equal(visible, 1, 'um terminal visível');
  assert.equal(await page.locator('.xterm').evaluateAll((els) => els.filter((el) => getComputedStyle(el).visibility === 'visible' && el.querySelector('.xterm-link-layer')).length), 1, 'o visível é o que tem o canvas');
  // Esconde o painel (Ctrl+`): os contextos ficam, sem passar do teto; reabrir não cria outro.
  await page.keyboard.press('Control+Backquote');
  await page.keyboard.press('Control+Backquote');
  await waitCount(page, MAX);
  assert.equal(await page.locator('.xterm').evaluateAll((els) => els.filter((el) => getComputedStyle(el).visibility === 'visible' && el.querySelector('.xterm-link-layer')).length), 1, 'o visível segue com o canvas');
});

dom('sem WebGL2: o terminal segue no DOM, com o texto em .xterm-rows', async ({ page }) => {
  await openShells(page, 2, false);
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
    // Pelo fluxo do backend: escondido, a saída vai ao espelho no worker, não ao xterm da tela.
    window.__terms.forEach(({ id }, n) => {
      for (let i = 0; i < 20; i++) window.__mock.termOut(id, chunk, 'project-1');
      window.__mock.termOut(id, `MARCA-${n}
`, 'project-1');
    });
    await new Promise((resolve) => setTimeout(resolve, 500));
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

// Executor escondido: a saída vai ao espelho no worker, não ao xterm da tela (zerado ao esconder). Ao entrar na aba, o retrato serializado
// mostra o que saiu, com cores, e o fluxo segue; voltar ao orquestrador zera o xterm do executor de novo.
dom('executor escondido não processa saída na tela; ao entrar na aba, mostra o que saiu e segue o fluxo', async ({ page }) => {
  await openApp(page);
  await openOrchestrator(page, 1);
  const tab = page.locator('[role="tab"][data-tab="e0"]');
  await tab.waitFor();
  const rows = () => page.evaluate(() => document.querySelector('[data-tab="e0"]') && [...document.querySelectorAll('.xterm')]
    .find((el) => el.closest('[inert]') === null && el.closest('.bg-terminal') && getComputedStyle(el).visibility === 'visible')
    ?.querySelector('.xterm-rows').textContent);
  const hidden = () => page.evaluate(() => [...document.querySelectorAll('.bg-terminal [inert] .xterm-rows')].map((r) => r.textContent).join(''));
  await page.evaluate(() => {
    for (let i = 0; i < 50; i++) window.__mock.termOut('e0', `linha ${i}\r\n`, 'project-1');
    window.__mock.termOut('e0', '\x1b[31mVERMELHO\x1b[0m\r\n', 'project-1');
  });
  await page.waitForTimeout(300);
  assert.ok(!(await hidden()).includes('VERMELHO'), 'o xterm do executor escondido não recebeu a saída');
  await tab.click();
  await page.waitForFunction(() => [...document.querySelectorAll('.bg-terminal .xterm-rows')].some((r) => r.textContent.includes('VERMELHO')));
  const red = await page.evaluate(() => [...document.querySelectorAll('.bg-terminal .xterm-rows span')].some((el) => el.textContent.includes('VERMELHO') && /xterm-fg-1\b/.test(el.className)));
  assert.ok(red, 'a cor vem no retrato');
  await page.evaluate(() => window.__mock.termOut('e0', 'DEPOIS\r\n', 'project-1'));
  await page.waitForFunction(() => [...document.querySelectorAll('.bg-terminal .xterm-rows')].some((r) => r.textContent.includes('DEPOIS')));
  assert.ok((await rows())?.includes('linha 49'), 'o histórico está no visível');
  await page.getByRole('tab', { name: 'Orquestrador' }).click();
  await page.waitForFunction(() => ![...document.querySelectorAll('.bg-terminal [inert] .xterm-rows')].some((r) => r.textContent.includes('DEPOIS')));
});

// Spinner em passos (src/index.css, --animate-spin): com executores rodando, todo `animate-spin` gira em 30 passos por 1,5 s (20 quadros/s),
// e nada contínuo (linear infinito) anima dentro da faixa de abas dos executores, que faria o compositor redesenhar na taxa do monitor.
dom('executores rodando: o ícone gira em passos e a faixa de abas não tem animação linear infinita', async ({ page }) => {
  await openApp(page);
  await openOrchestrator(page, 4);
  await page.locator('[role="tab"][data-tab="e3"]').waitFor();
  await page.waitForTimeout(300);
  const anims = await page.evaluate(() => {
    const strip = document.querySelector('[data-tab="e0"]').closest('[role="tablist"]') ?? document.querySelector('[data-tab="e0"]').parentElement;
    return document.getAnimations().filter((a) => a.playState === 'running').map((a) => {
      const t = a.effect.getTiming();
      const el = a.effect.target;
      // Em animação CSS, o animation-timing-function vai para cada keyframe; a timing do efeito fica `linear`.
      const easing = t.easing !== 'linear' ? t.easing : (a.effect.getKeyframes()[0]?.easing ?? 'linear');
      return { name: a.animationName ?? a.constructor.name, easing, duration: t.duration, iterations: t.iterations,
        spin: el.classList.contains('animate-spin'), inStrip: strip.contains(el) };
    });
  });
  const spins = anims.filter((a) => a.spin);
  assert.ok(spins.length >= 4, `um spinner por executor (${spins.length})`);
  for (const a of spins) {
    assert.match(a.easing, /^steps\(30(, ?end)?\)$/, `easing do spin: ${a.easing}`);
    assert.equal(a.duration, 1500, 'duração do spin');
  }
  assert.deepEqual(anims.filter((a) => a.inStrip && a.iterations === Infinity && a.easing === 'linear'), [], 'linear infinito na faixa');
});
