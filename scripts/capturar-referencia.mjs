// Screenshots de referência da interface atual (React), contra o app simulado (src/backend/mock.ts, só no Vite de desenvolvimento).
// Sobe o próprio Vite (porta REF_PORT, padrão 1431, --strictPort) e encerra só ele. Não instala nada: playwright-core e Chromium do cache local.
// Uso: node scripts/capturar-referencia.mjs   (grava docs/referencia-visual/*.png; saída 0 = todos os estados capturados)
import { mkdirSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { join } from 'node:path';

process.env.SMOKE_PORT = process.env.REF_PORT ?? '1431';
const H = await import('../tests/editor-ui-harness.mjs');
const { ROOT, BASE, PORT, waitForServer, launchBrowser, openApp, expand, treeItem, focusEditor } = H;
const DIR = join(ROOT, 'docs', 'referencia-visual');
mkdirSync(DIR, { recursive: true });

const nav = (page) => page.getByRole('navigation', { name: 'Telas' });
const route = (page, name) => nav(page).getByRole('button', { name, exact: true }).click();
const openGit = (page) => nav(page).getByRole('button', { name: /^Git/ }).click();
const gitTab = (page, name) => page.getByRole('tab', { name: new RegExp(`^${name}`) });
const section = (page, title) => page.locator('section').filter({ has: page.getByRole('button', { name: new RegExp(`^${title}\\b`) }) });
const row = (page, side, path) => section(page, side === 'staged' ? 'Preparadas' : 'Alterações').locator(`button[title^="${path}: "]`);
const tabByTitle = (page, label) => page.locator(`[data-tab] [role="tab"][title$="${label}"]`);
const bar = (page) => page.locator('[data-body]:visible .compare-toolbar').first();
const tb = (page, name) => bar(page).getByRole('button', { name, exact: true });
const visibleEditor = (page) => page.locator('.cm-content:visible').first().waitFor();

async function openFile(page, folders, name) {
  await expand(page, folders);
  await treeItem(page, name).dblclick();
  await visibleEditor(page);
}
async function openOrchestrator(page) {
  await page.getByRole('button', { name: 'Abrir terminal', exact: true }).first().click();
  await page.getByRole('dialog').getByRole('button', { name: 'Abrir terminal' }).click();
  await page.getByRole('tab', { name: 'Orquestrador' }).waitFor();
}
const addNode = (page, id, index, state) => page.evaluate(([id, index, state]) => window.__mock.addNode({
  project: 'project-1', id, batch: 'b1', index, title: `Tarefa ${index + 1}`, state,
  choice: { tool: 'claude', model: 'opus', effort: 'high' },
}), [id, index, state]);
const DOIS = 'topo\n<<<<<<< ours\natual-1\n=======\nrecebida-1\n>>>>>>> theirs\nmeio\n<<<<<<< ours\natual-2\n=======\nrecebida-2\n>>>>>>> theirs\nfim\n';
const DOIS_SIDES = { ours: 'topo\natual-1\nmeio\natual-2\nfim\n', theirs: 'topo\nrecebida-1\nmeio\nrecebida-2\nfim\n' };
const GUIA = '# Guia do Orquestrador\n\nUm parágrafo com `código inline` e [link](https://example.com).\n\n> Citação de exemplo.\n\n| Papel | CLI |\n| --- | --- |\n| Orquestrador | claude |\n| Executor | codex |\n\n- [x] feito\n- [ ] pendente\n\n![logo](../assets/logo.png)\n\n```ts\nconst x: number = 1;\n```\n';

async function searchTermo(page) {
  await page.evaluate(() => {
    window.__mock.addFile('src/termo-a.ts', 'const termo = 1;\n');
    window.__mock.addFile('src/termo-b.ts', 'let x = termo + termo;\n');
  });
  await route(page, 'Pesquisar');
  await page.getByRole('textbox', { name: 'Pesquisar nos arquivos do projeto' }).fill('termo');
  await page.locator('[data-result]').first().waitFor();
}
async function gitDiff(page) {
  await openGit(page);
  await row(page, 'unstaged', 'src/editor-workspace.tsx').dblclick();
  await tabByTitle(page, 'src/editor-workspace.tsx (alterações)').waitFor();
  await page.locator('[data-body]:visible .compare-view').waitFor();
}

// Cada estado: arquivo, descrição, como foi alcançado, query da URL, e a ação até o estado (página já com a pasta aberta).
const STATES = [
  { file: 'inicial-sem-projeto.png', estado: 'Janela inicial sem projeto', como: '`?project=none`', query: '?project=none', run: async () => {} },
  { file: 'inicial-projeto.png', estado: 'Projeto recém-aberto (tela Arquivos, sem editor)', como: 'Arquivo > Abrir pasta', run: async () => {} },
  { file: 'titlebar-abas-projeto.png', estado: 'Titlebar com duas abas de projeto', como: 'Botão "Abrir pasta" da titlebar abre o segundo projeto (simulado-2)', run: async (page) => {
    await page.locator('header button[aria-label="Abrir pasta"]').click();
    await page.waitForFunction(() => document.querySelectorAll('header [role="tab"]').length >= 2);
  } },
  { file: 'titlebar-tooltip.png', estado: 'Tooltip de botão da titlebar', como: 'Hover em "Fechar" da titlebar', run: async (page) => {
    await page.locator('button[aria-label="Fechar"]').hover();
    await page.getByRole('tooltip').or(page.locator('[role="tooltip"], [data-popup-open] [class*="tooltip"]')).first().waitFor().catch(() => {});
  }, keepMouse: true },
  { file: 'menu-arquivo.png', estado: 'Dropdown do menu Arquivo', como: 'Clique em "Arquivo" na titlebar', run: async (page) => {
    await page.getByRole('button', { name: 'Arquivo', exact: true }).click();
    await page.getByRole('menuitem', { name: /Abrir pasta/ }).waitFor();
  } },
  { file: 'arquivos-editor.png', estado: 'Árvore expandida e código no editor com realce, git gutter e minimapa', como: 'Expande src, docs; duplo clique em src/editor-workspace.tsx (modificado no mock)', run: async (page) => {
    await openFile(page, ['src', 'docs'], 'editor-workspace.tsx');
    // Rola até a linha 22: o bloco alterado (28 a 32) aparece com o gutter do Git e código com realce.
    for (let i = 0; i < 60; i++) {
      const done = await page.evaluate(() => {
        const sc = [...document.querySelectorAll('.cm-scroller')].find((el) => el.offsetParent);
        const g = [...sc.querySelectorAll('.cm-lineNumbers .cm-gutterElement')].find((el) => el.textContent.trim() === '26');
        if (g) { sc.scrollTop += g.getBoundingClientRect().top - sc.getBoundingClientRect().top; return true; }
        sc.scrollTop += 300;
        return false;
      });
      if (done) break;
      await page.waitForTimeout(50);
    }
    await page.waitForTimeout(500);
  } },
  { file: 'arvore-hover-selecao.png', estado: 'Hover e seleção múltipla na árvore', como: 'Clique em App.tsx, Ctrl+clique em main.tsx, hover em README.md', run: async (page) => {
    await expand(page, ['src']);
    await treeItem(page, 'App.tsx').click();
    await treeItem(page, 'main.tsx').click({ modifiers: ['Control'] });
    await treeItem(page, 'README.md').hover();
  }, keepMouse: true },
  { file: 'arvore-menu-contexto.png', estado: 'Menu de contexto de arquivo na árvore', como: 'Botão direito em docs/design-system.md', run: async (page) => {
    await expand(page, ['docs']);
    await treeItem(page, 'design-system.md').click({ button: 'right' });
    await page.getByRole('menuitem', { name: /Revelar no Explorador/ }).waitFor();
  } },
  { file: 'arvore-menu-pasta.png', estado: 'Menu de contexto de pasta na árvore', como: 'Botão direito em src', run: async (page) => {
    await treeItem(page, 'src').click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Localizar na Pasta...' }).waitFor();
  } },
  { file: 'arvore-renomear.png', estado: 'Campo de renomear inline na árvore', como: 'Botão direito em README.md > Renomear...', run: async (page) => {
    await treeItem(page, 'README.md').click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Renomear...' }).click();
    await page.getByRole('tree').getByRole('textbox', { name: 'Novo nome' }).waitFor();
  } },
  { file: 'editor-varias-abas.png', estado: 'Várias abas de editor, uma suja', como: 'Abre App.tsx, main.tsx, longo.ts, README.md; digita no README', run: async (page) => {
    for (const n of ['App.tsx', 'main.tsx', 'longo.ts']) await openFile(page, ['src'], n);
    await treeItem(page, 'README.md').dblclick();
    await focusEditor(page);
    await page.keyboard.press('Control+End');
    await page.keyboard.type('\nlinha nova');
  } },
  { file: 'editor-menu-aba.png', estado: 'Menu de contexto de aba do editor', como: 'Botão direito na aba App.tsx', run: async (page) => {
    await openFile(page, ['src'], 'App.tsx');
    await openFile(page, ['src'], 'main.tsx');
    await page.locator('[data-tab]', { hasText: 'App.tsx' }).click({ button: 'right' });
    await page.getByRole('menuitem').first().waitFor();
  } },
  { file: 'editor-lista-abas.png', estado: 'Dropdown "Lista de abas" do editor', como: 'Abre 3 arquivos e clica em "Lista de abas"', run: async (page) => {
    for (const n of ['App.tsx', 'main.tsx', 'longo.ts']) await openFile(page, ['src'], n);
    await page.getByRole('button', { name: /^Lista de abas/ }).first().click();
    await page.locator('[role="menu"] [data-tab-key]').first().waitFor();
  } },
  { file: 'editor-dividido.png', estado: 'Editor dividido em dois grupos', como: 'Abre App.tsx e Ctrl+\\ (dividir)', run: async (page) => {
    await openFile(page, ['src'], 'App.tsx');
    await focusEditor(page);
    await page.keyboard.press('Control+Backslash');
    await page.locator('[data-group="B"] [data-tab]').first().waitFor({ timeout: 3000 });
  } },
  { file: 'abrir-rapido.png', estado: 'Abrir rápido (Ctrl+P)', como: 'Ctrl+P e digita "app"', run: async (page) => {
    await page.keyboard.press('Control+P');
    await page.keyboard.type('app');
    await page.waitForTimeout(300);
  } },
  { file: 'markdown-codigo.png', estado: 'Markdown em modo código com seletor Preview | Markdown', como: 'Cria docs/guia.md no mock e abre', run: async (page) => {
    await page.evaluate((t) => window.__mock.addFile('docs/guia.md', t), GUIA);
    await openFile(page, ['docs'], 'guia.md');
    await page.getByRole('radiogroup', { name: 'Modo do Markdown' }).waitFor();
  } },
  { file: 'markdown-preview.png', estado: 'Preview de Markdown renderizado', como: 'Mesmo arquivo, clique em Preview', run: async (page) => {
    await page.evaluate((t) => window.__mock.addFile('docs/guia.md', t), GUIA);
    await openFile(page, ['docs'], 'guia.md');
    await page.getByRole('radiogroup', { name: 'Modo do Markdown' }).getByRole('radio', { name: 'Preview' }).click();
    await page.locator('[data-markdown-preview] table').waitFor();
    await page.waitForFunction(() => { const i = document.querySelector('[data-markdown-preview] img[alt="logo"]'); return !!i && i.complete && i.naturalWidth > 0; });
  } },
  { file: 'conflito-merge.png', estado: 'Tela de conflito de merge com 2 trechos', como: '`__mock.addConflict(src/dois.ts)`; Git > duplo clique no arquivo', run: async (page) => {
    await page.evaluate(([t, s]) => window.__mock.addConflict('src/dois.ts', s, t), [DOIS, DOIS_SIDES]);
    await openGit(page);
    await row(page, 'unstaged', 'src/dois.ts').dblclick();
    await page.getByText(/^2 trechos com conflito/).waitFor();
  } },
  { file: 'midia-imagem.png', estado: 'Aba de imagem (assets/logo.png)', como: 'Expande assets, clique em logo.png', run: async (page) => {
    await expand(page, ['assets']);
    await treeItem(page, 'logo.png').click();
    await page.waitForFunction(() => { const i = document.querySelector('img[alt="logo.png"]'); return !!i && i.complete && i.naturalWidth > 0; });
  } },
  { file: 'midia-audio.png', estado: 'Aba de áudio (WAV)', como: '`__mock.addFile(midia/tom.wav)`; duplo clique', run: async (page) => {
    await page.evaluate(() => { window.__mock.addFolder('midia'); window.__mock.addFile('midia/tom.wav'); });
    await expand(page, ['midia']);
    await treeItem(page, 'tom.wav').dblclick();
    await page.waitForFunction(() => { const a = document.querySelector('audio'); return a && a.readyState >= 1; });
  } },
  { file: 'midia-video.png', estado: 'Aba de vídeo (WebM)', como: '`__mock.addFile(midia/clipe.webm)`; duplo clique', run: async (page) => {
    await page.evaluate(() => { window.__mock.addFolder('midia'); window.__mock.addFile('midia/clipe.webm'); });
    await expand(page, ['midia']);
    await treeItem(page, 'clipe.webm').dblclick();
    await page.waitForFunction(() => { const v = document.querySelector('video'); return v && v.readyState >= 2; });
  } },
  { file: 'midia-erro.png', estado: 'Mídia com falha de leitura', como: '`__mock.failRead` em midia/quebrado.png; duplo clique', run: async (page) => {
    await page.evaluate(() => { window.__mock.addFolder('midia'); window.__mock.addFile('midia/quebrado.png'); window.__mock.failRead = ['midia/quebrado.png']; });
    await expand(page, ['midia']);
    await treeItem(page, 'quebrado.png').dblclick();
    await page.getByRole('status').filter({ hasText: 'Falha de leitura simulada' }).waitFor();
  } },
  { file: 'arquivo-binario.png', estado: 'Arquivo grande demais para o editor', como: 'Duplo clique em dados/exportacao.json', run: async (page) => {
    await expand(page, ['dados']);
    await treeItem(page, 'exportacao.json').dblclick();
    await page.waitForTimeout(600);
  } },
  { file: 'pesquisar-resultados.png', estado: 'Tela Pesquisar com resultados', como: 'Cria src/termo-*.ts; Pesquisar "termo"', run: searchTermo },
  { file: 'pesquisar-substituir.png', estado: 'Pesquisar com substituição aberta e prévia', como: 'Idem + "Ativar/Desativar Substituição" e "novo"', run: async (page) => {
    await searchTermo(page);
    await page.getByRole('button', { name: 'Ativar/Desativar Substituição', exact: true }).click();
    await page.getByRole('textbox', { name: 'Substituir por' }).fill('novo');
    await page.waitForFunction(() => document.querySelectorAll('.search-hit-added').length >= 3);
  } },
  { file: 'pesquisar-confirmar.png', estado: 'Diálogo de confirmação do Substituir Tudo', como: 'Idem + "Substituir Tudo"', run: async (page) => {
    await searchTermo(page);
    await page.getByRole('button', { name: 'Ativar/Desativar Substituição', exact: true }).click();
    await page.getByRole('textbox', { name: 'Substituir por' }).fill('novo');
    await page.waitForFunction(() => document.querySelectorAll('.search-hit-added').length >= 3);
    await page.getByRole('button', { name: 'Substituir Tudo', exact: true }).click();
    await page.getByRole('dialog').waitFor();
  } },
  { file: 'git-alteracoes.png', estado: 'Tela Git com Preparadas e Alterações', como: 'Botão Git na navegação', run: async (page) => {
    await openGit(page);
    await row(page, 'staged', 'src/editor-workspace.tsx').waitFor();
    await row(page, 'unstaged', 'src/editor-workspace.tsx').waitFor();
  } },
  { file: 'git-diff-lado-a-lado.png', estado: 'Comparador lado a lado', como: 'Git > duplo clique em src/editor-workspace.tsx (Alterações)', run: gitDiff },
  { file: 'git-diff-unificado.png', estado: 'Comparador unificado (inline)', como: 'Idem + "Unificado" na barra', run: async (page) => {
    await gitDiff(page);
    await tb(page, 'Unificado').click();
    await page.waitForTimeout(300);
  } },
  { file: 'git-grafo.png', estado: 'Grafo do Git na tela Git', como: 'Git; seção Grafo', run: async (page) => {
    await openGit(page);
    await page.locator('section li[data-commit]').first().waitFor();
  } },
  { file: 'git-historico.png', estado: 'Aba Histórico do Git', como: 'Git > Histórico', run: async (page) => {
    await openGit(page);
    await gitTab(page, 'Histórico').click();
    await page.locator('button[title^="4b2e8d0 · "]').waitFor();
  } },
  { file: 'git-commit-diff.png', estado: 'Commit aberto e diff de arquivo do commit', como: 'Histórico > 4b2e8d0 > src/lib/utils.ts', run: async (page) => {
    await openGit(page);
    await gitTab(page, 'Histórico').click();
    await page.locator('button[title^="4b2e8d0 · "]').click();
    await page.locator('button[title^="src/lib/utils.ts (renomeado de src/utils.ts)"]').click();
    await tabByTitle(page, 'src/lib/utils.ts @ 4b2e8d0').waitFor();
    await page.locator('[data-body]:visible .compare-view').waitFor();
  } },
  { file: 'git-sem-repositorio.png', estado: 'Git em pasta sem repositório', como: '`?git=no-repo`; Git', query: '?git=no-repo', run: async (page) => {
    await openGit(page);
    await page.getByRole('button', { name: 'Inicializar repositório', exact: true }).waitFor();
  } },
  { file: 'git-ausente.png', estado: 'Git ausente no sistema', como: '`?git=no-git`; Git', query: '?git=no-git', run: async (page) => {
    await openGit(page);
    await page.waitForTimeout(500);
  } },
  { file: 'orquestrador-escolha.png', estado: 'Diálogo de escolha do Orquestrador (ferramenta, modelo, esforço)', como: 'Botão "Abrir terminal"', run: async (page) => {
    await page.getByRole('button', { name: 'Abrir terminal', exact: true }).first().click();
    await page.getByRole('dialog').getByText('Ferramenta').waitFor();
  } },
  { file: 'orquestrador-select-modelo.png', estado: 'Dropdown do Modelo no diálogo', como: 'Idem, Claude Code no segmentado e clique no Select de Modelo', run: async (page) => {
    await page.getByRole('button', { name: 'Abrir terminal', exact: true }).first().click();
    const d = page.getByRole('dialog');
    await d.getByText('Claude', { exact: false }).first().click();
    await d.getByRole('combobox').first().click();
    await page.getByRole('option').first().waitFor();
  } },
  { file: 'orquestrador-terminal.png', estado: 'Painel do Orquestrador com terminal', como: 'Abrir terminal > confirmar', run: async (page) => {
    await openOrchestrator(page);
    await page.locator('.xterm-rows').first().waitFor();
    await page.waitForTimeout(500);
  } },
  { file: 'executores-abas.png', estado: 'Abas de executores (executando e na fila)', como: 'Orquestrador aberto + `__mock.addNode` x2', run: async (page) => {
    await openOrchestrator(page);
    await addNode(page, 'e1', 0, 'running');
    await addNode(page, 'e2', 1, 'queued');
    await page.getByRole('tab', { name: /Executor #2/ }).waitFor();
  } },
  { file: 'executor-aguardando-escolha.png', estado: 'Executor aguardando escolha (diálogo dos executores)', como: 'Orquestrador aberto + `__mock.addNode` com state awaiting-choice', run: async (page) => {
    await openOrchestrator(page);
    await addNode(page, 'e1', 0, 'awaiting-choice');
    await page.getByRole('dialog').getByText('Escolha dos executores').waitFor();
  } },
  { file: 'terminal-shells.png', estado: 'Painel Terminal embaixo com 3 abas de shell', como: 'Ctrl+` e "Novo terminal" x2', run: async (page) => {
    await page.keyboard.press('Control+Backquote');
    await page.locator('[data-shell]').first().waitFor();
    for (let i = 0; i < 2; i++) await page.getByRole('button', { name: 'Novo terminal' }).click();
    await page.waitForFunction(() => document.querySelectorAll('[data-shell]').length === 3);
    await page.keyboard.type('echo ola');
    await page.waitForTimeout(400);
  } },
  { file: 'terminal-lista-abas.png', estado: 'Lista de abas do painel Terminal', como: 'Idem + "Lista de abas"', run: async (page) => {
    await page.keyboard.press('Control+Backquote');
    await page.locator('[data-shell]').first().waitFor();
    for (let i = 0; i < 2; i++) await page.getByRole('button', { name: 'Novo terminal' }).click();
    await page.waitForFunction(() => document.querySelectorAll('[data-shell]').length === 3);
    await page.getByRole('button', { name: /^Lista de abas/ }).last().click();
    await page.locator('[role="menu"] [data-tab-key]').first().waitFor();
  } },
  { file: 'fechar-confirmacao.png', estado: 'Modal de fechar com alterações não salvas', como: 'Edita README e `__mock.close()`', run: async (page) => {
    await treeItem(page, 'README.md').dblclick();
    await focusEditor(page);
    await page.keyboard.type('x');
    await page.evaluate(() => { void window.__mock.close(); });
    await page.getByRole('dialog').waitFor();
  } },
  { file: 'toast-sincronizado.png', estado: 'Toast de sucesso', como: '"Sincronizar as alterações" na barra de status', run: async (page) => {
    await page.waitForFunction(() => { const b = document.querySelector('footer button[aria-label^="Sincronizar as alterações"]'); return b && !b.disabled; });
    await page.locator('footer button[aria-label^="Sincronizar as alterações"]').click();
    await page.getByText('Alterações sincronizadas').waitFor();
  } },
  { file: 'toast-subpasta.png', estado: 'Toast informativo (pasta dentro de repositório)', como: '`?subfolder=1`', query: '?subfolder=1', run: async (page) => {
    await page.getByText(/Esta pasta está dentro do repositório/).waitFor();
  } },
  { file: 'atualizacao-clis.png', estado: 'Portão de atualização dos CLIs', como: '`?update=clis`', query: '?update=clis', noOpen: true, run: async (page) => {
    await page.getByRole('status').getByText('Preparando os CLIs…').waitFor();
  } },
];

// Fontes carregadas, dois quadros e uma pausa para transições (as do app são curtas).
async function settle(page) {
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  await page.waitForTimeout(600);
  await page.evaluate(() => document.fonts.ready);
}

// Como o startVite do harness, mas sem pipe na saída: com ~45 cargas, o pipe não lido enche e trava o Vite.
const viteProc = spawn(process.execPath, [join(ROOT, 'node_modules', 'vite', 'bin', 'vite.js'), '--port', String(PORT), '--strictPort'], {
  cwd: ROOT, stdio: 'ignore', windowsHide: true,
});
const vite = { stop: () => { if (!viteProc.killed) viteProc.kill(); } };
process.on('exit', vite.stop);
let browser;
const failed = [];
try {
  await waitForServer();
  browser = await launchBrowser();
  // Primeira carga transforma o app inteiro: feita antes, os estados não esbarram no timeout.
  await (await browser.newPage()).goto(BASE, { timeout: 120000 });
  for (const s of STATES) {
    const context = await browser.newContext({ viewport: { width: 1400, height: 900 }, deviceScaleFactor: 1 });
    const page = await context.newPage();
    page.setDefaultTimeout(10000);
    try {
      if (s.noOpen) await page.goto(`${BASE}${s.query ?? ''}`, { waitUntil: 'domcontentloaded', timeout: 30000 });
      else await openApp(page, s.query ?? '');
      await settle(page);
      // Teto por estado: nenhuma espera pendurada segura a captura inteira.
      await Promise.race([s.run(page), new Promise((_, rej) => setTimeout(() => rej(new Error('estado não alcançado em 45 s')), 45000))]);
      if (!s.keepMouse) await page.mouse.move(1399, 450);
      await settle(page);
      await page.screenshot({ path: join(DIR, s.file) });
      console.log(`ok   ${s.file}`);
    } catch (e) {
      failed.push(s.file);
      console.log(`FALHA ${s.file}: ${e.message.split('\n')[0]}`);
    } finally {
      await context.close();
    }
  }
} finally {
  await browser?.close();
  vite.stop();
}
console.log(`${STATES.length - failed.length}/${STATES.length} capturados`);
process.exit(failed.length ? 1 : 0);
