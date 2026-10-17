// QA da aba de mídia (imagem e áudio), em Chrome headless contra o app simulado (src/backend/mock.ts, só no `vite` de desenvolvimento).
// O simulado entra só na fronteira de I/O: `read_media` devolve um PNG 32×32 para `.png` e um WAV de 2 s em silêncio para
// `.wav` (`__mock.addFile`); `__mock.failRead` faz a leitura falhar e `__mock.corruptRead` devolve bytes que o navegador não decodifica.
// Tudo o mais é o app de verdade. O áudio usa o `<audio controls>` nativo do WebView2/Chromium: o check clica nos controles
// (gesto), e usa a API HTMLMediaElement só onde o controle não expõe o ajuste (posição e volume).
// Loader compartilhado em editor-ui-harness.mjs: Vite na porta 1421 (SMOKE_PORT), encerrado só aqui. Não instala nada.
// Uso: node scripts/check-media-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
// Capturas em .playwright-mcp/media-ui-*.png só quando um cenário falha: evidência para leitura humana, não asserção.
// Limite de codec: o que toca depende do WebView2/Windows; o simulado só prova WAV (PCM) e PNG.
import assert from 'node:assert/strict';
import { uiSuite, openApp, expand, treeItem, tabs } from './editor-ui-harness.mjs';

const scenario = uiSuite('media-ui');

// Renomeia pelo menu da árvore (mesmo caminho do usuário: botão direito, Renomear..., campo Novo nome).
async function rename(page, from, to) {
  await treeItem(page, from).click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Renomear...' }).click();
  const field = page.getByRole('tree').getByRole('textbox', { name: 'Novo nome' });
  await field.waitFor();
  await field.fill(to);
  await field.press('Enter');
  await field.waitFor({ state: 'detached' });
}

// Imagem: abre na aba (sem editor), decodifica no tamanho natural, cabe no painel, e alterna Ajustar / Tamanho real.
scenario('imagem abre na aba, decodifica e alterna ajustar e tamanho real, sem editor de texto', async ({ page }) => {
  await expand(page, ['assets']);
  await treeItem(page, 'logo.png').click();
  const img = page.locator('img[alt="logo.png"]');
  await img.waitFor();
  await page.waitForFunction(() => {
    const i = document.querySelector('img[alt="logo.png"]');
    return !!i && i.complete && i.naturalWidth > 0;
  });
  const natural = await img.evaluate((i) => ({ w: i.naturalWidth, h: i.naturalHeight }));
  assert.deepEqual(natural, { w: 32, h: 32 }, 'PNG decodificado com o tamanho do arquivo');

  const fit = await img.evaluate((i) => {
    const b = i.getBoundingClientRect();
    const p = i.parentElement.getBoundingClientRect();
    return { w: b.width, h: b.height, pw: p.width, ph: p.height };
  });
  assert.ok(fit.w <= fit.pw + 0.5 && fit.h <= fit.ph + 0.5, `imagem maior que o painel: ${JSON.stringify(fit)}`);
  assert.equal(await page.locator('.cm-content').count(), 0, 'imagem não abre editor de texto');

  await page.getByRole('button', { name: 'Tamanho real' }).click();
  const real = await img.evaluate((i) => i.getBoundingClientRect().width);
  assert.equal(real, 32, 'tamanho real: 32 px na tela');
  await page.getByRole('button', { name: 'Ajustar' }).click();
  await page.getByRole('button', { name: 'Tamanho real' }).waitFor();
});

// Áudio: controles nativos com nome; não toca sozinho; o gesto no controle toca e pausa.
scenario('áudio usa o controle nativo com nome, não toca sozinho e toca e pausa pelo gesto', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFolder('midia');
    window.__mock.addFile('midia/tom.wav');
  });
  await expand(page, ['midia']);
  // Duplo clique abre aba permanente: a troca de aba abaixo não a substitui pela prévia.
  await treeItem(page, 'tom.wav').dblclick();
  const audio = page.locator('audio');
  await audio.waitFor({ state: 'attached' });
  await page.waitForFunction(() => document.querySelector('audio')?.readyState >= 1);
  const attrs = await audio.evaluate((a) => ({ controls: a.controls, preload: a.getAttribute('preload'), label: a.getAttribute('aria-label'), paused: a.paused, duration: a.duration }));
  assert.equal(attrs.controls, true, 'controles nativos ligados');
  assert.equal(attrs.preload, 'metadata', 'preload metadata');
  assert.equal(attrs.label, 'tom.wav', 'nome do áudio no rótulo acessível');
  assert.equal(await page.locator('audio[aria-label="tom.wav"]').count(), 1, 'o elemento de áudio tem o nome do arquivo');
  assert.equal(attrs.paused, true, 'áudio não toca sozinho');
  assert.ok(Math.abs(attrs.duration - 2) < 0.05, `duração do WAV: ${attrs.duration}`);
  await page.waitForTimeout(300);
  assert.equal(await audio.evaluate((a) => a.paused), true, 'continua parado depois de um instante');

  // Gesto no controle nativo: o botão de tocar fica no começo da barra (x ≈ 20 px, centro vertical).
  const box = await audio.boundingBox();
  await page.mouse.click(box.x + 20, box.y + box.height / 2);
  await page.waitForFunction(() => { const a = document.querySelector('audio'); return !!a && !a.paused && a.currentTime > 0.05; }, null, { timeout: 5000 });
  await page.mouse.click(box.x + 20, box.y + box.height / 2);
  await page.waitForFunction(() => document.querySelector('audio')?.paused === true, null, { timeout: 5000 });
  const at = await audio.evaluate((a) => a.currentTime);
  await page.waitForTimeout(250);
  assert.equal(await audio.evaluate((a) => a.currentTime), at, 'pausado não avança');
});

// Áudio: seek e volume (API do elemento, o controle nativo não é endereçável aqui); pausa ao trocar de aba, ao ficar oculto e ao fechar.
scenario('áudio: seek, volume, pausa ao trocar de aba, ao ficar oculto e ao fechar a aba', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFolder('midia');
    window.__mock.addFile('midia/tom.wav');
  });
  await expand(page, ['midia']);
  await treeItem(page, 'tom.wav').dblclick();
  const audio = page.locator('audio');
  await audio.waitFor({ state: 'attached' });
  await page.waitForFunction(() => document.querySelector('audio')?.readyState >= 1);

  await audio.evaluate((a) => { a.currentTime = 1; });
  await page.waitForFunction(() => Math.abs(document.querySelector('audio').currentTime - 1) < 0.1);
  await audio.evaluate((a) => { a.volume = 0.25; });
  assert.ok(Math.abs((await audio.evaluate((a) => a.volume)) - 0.25) < 0.001, 'volume 0,25');

  await audio.evaluate((a) => a.play());
  await page.waitForFunction(() => document.querySelector('audio')?.paused === false);
  await treeItem(page, 'README.md').click();
  await page.waitForFunction(() => document.querySelector('audio')?.paused === true);
  await page.getByRole('tab', { name: 'tom.wav' }).click();
  await page.waitForTimeout(200);
  assert.equal(await audio.evaluate((a) => a.paused), true, 'voltar à aba não retoma sozinho');

  // Aba oculta (troca de app/janela) pausa mesmo com a aba visível.
  await audio.evaluate((a) => a.play());
  await page.waitForFunction(() => document.querySelector('audio')?.paused === false);
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => true });
    document.dispatchEvent(new Event('visibilitychange'));
    delete document.hidden;
  });
  await page.waitForFunction(() => document.querySelector('audio')?.paused === true);

  // Fechar a aba com o áudio tocando: o elemento que tocava fica pausado.
  await audio.evaluate((a) => { window.__audioAntigo = a; a.play(); });
  await page.waitForFunction(() => window.__audioAntigo.paused === false);
  await tabs(page).filter({ hasText: 'tom.wav' }).getByRole('button', { name: /^Fechar/ }).click();
  await page.waitForFunction(() => window.__audioAntigo.paused === true);
  assert.equal(await page.locator('audio').count(), 0, 'aba fechada sem elemento de áudio');
});

// Erro de leitura: a aba mostra a mensagem do backend, sem elemento de mídia nem editor.
scenario('falha de leitura da mídia aparece como erro na aba', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFolder('midia');
    window.__mock.addFile('midia/quebrado.wav');
    window.__mock.failRead = ['midia/quebrado.wav'];
  });
  await expand(page, ['midia']);
  await treeItem(page, 'quebrado.wav').dblclick();
  await page.getByRole('status').filter({ hasText: 'Falha de leitura simulada' }).waitFor();
  assert.equal(await page.locator('audio').count(), 0, 'sem elemento de áudio quando a leitura falha');
  assert.equal(await page.locator('.cm-content').count(), 0, 'sem editor de texto');
});

// Arquivo inválido: a leitura chega, mas o navegador não decodifica. Erro na aba, sem editor, sem tela em branco.
scenario('arquivo inválido: imagem e áudio que não decodificam mostram erro na aba, sem editor', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFolder('midia');
    window.__mock.addFile('midia/ruim.png');
    window.__mock.addFile('midia/ruim.wav');
    window.__mock.corruptRead = ['midia/ruim.png', 'midia/ruim.wav'];
  });
  await expand(page, ['midia']);
  await treeItem(page, 'ruim.png').dblclick();
  await page.getByRole('status').filter({ hasText: 'Não foi possível exibir esta imagem.' }).waitFor();
  assert.equal(await page.locator('img[alt="ruim.png"]').count(), 0, 'sem imagem quando não decodifica');
  assert.equal(await page.locator('.cm-content').count(), 0, 'imagem inválida sem editor');

  await treeItem(page, 'ruim.wav').dblclick();
  await page.getByRole('alert').filter({ hasText: 'Não foi possível tocar este áudio' }).waitFor();
  assert.equal(await page.locator('audio').count(), 1, 'o elemento de áudio fica, com o erro abaixo');
  assert.equal(await page.locator('.cm-content').count(), 0, 'áudio inválido sem editor');
});

// Renomear com a aba aberta: a aba troca de caminho e recarrega o conteúdo novo, sem editor de texto.
scenario('imagem e áudio renomeados com a aba aberta: a aba troca de nome e recarrega, sem editor', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFolder('midia');
    window.__mock.addFile('midia/foto.png');
    window.__mock.addFile('midia/tom.wav');
  });
  await expand(page, ['midia']);
  await treeItem(page, 'foto.png').dblclick();
  await page.locator('img[alt="foto.png"]').waitFor();
  await page.waitForFunction(() => document.querySelector('img[alt="foto.png"]')?.naturalWidth > 0);
  await rename(page, 'foto.png', 'foto-nova.png');
  await page.waitForFunction(() => document.querySelector('img[alt="foto-nova.png"]')?.naturalWidth === 32);
  assert.equal(await page.locator('img[alt="foto.png"]').count(), 0, 'nome antigo saiu da aba');
  assert.ok((await tabs(page).allTextContents()).some((t) => t.includes('foto-nova.png')), 'a aba mostra o nome novo');
  assert.equal(await page.locator('.cm-content').count(), 0, 'imagem renomeada não abre editor');

  await treeItem(page, 'tom.wav').dblclick();
  await page.locator('audio').waitFor({ state: 'attached' });
  await page.waitForFunction(() => document.querySelector('audio')?.readyState >= 1);
  await rename(page, 'tom.wav', 'tom-novo.wav');
  await page.waitForFunction(() => document.querySelector('audio')?.getAttribute('aria-label') === 'tom-novo.wav' && document.querySelector('audio').readyState >= 1);
  const dur = await page.locator('audio').evaluate((a) => a.duration);
  assert.ok(Math.abs(dur - 2) < 0.05, `áudio renomeado recarregado (duração ${dur})`);
  assert.equal(await page.locator('.cm-content').count(), 0, 'áudio renomeado não abre editor');
});

// Restauro: a aba de imagem volta depois de recarregar o app, e mostra a imagem de novo (sem editor).
scenario('imagem aberta volta na aba depois de recarregar o app', async ({ page }) => {
  await expand(page, ['assets']);
  await treeItem(page, 'logo.png').dblclick();
  await page.locator('img[alt="logo.png"]').waitFor();
  await page.waitForFunction(() => document.querySelector('img[alt="logo.png"]')?.naturalWidth > 0);

  await openApp(page);
  await page.locator('[data-tab]').filter({ hasText: 'logo.png' }).first().waitFor();
  await page.waitForFunction(() => document.querySelector('img[alt="logo.png"]')?.naturalWidth > 0);
  assert.equal(await page.locator('.cm-content').count(), 0, 'restaurada sem editor de texto');
});
