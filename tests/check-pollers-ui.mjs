// QA de interface dos pollers das telas Git e Arquivos: poll com resposta igual não pode deixar updates na fila dos hooks.
// `setX(valor igual)` ou `setX(fn)` devolvendo o mesmo valor o React descarta sem renderizar, mas mantém o update na fila do hook até o
// próximo render; com o app aberto o dia todo isso acumula memória. O check anda pelas fibers a partir de `#root` e acusa hook com 2+ updates
// na fila (um update isolado espera o próximo render, normal). Chrome headless contra o app simulado (editor-ui-harness.mjs).
// Uso: node tests/check-pollers-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
import assert from 'node:assert/strict';
import { uiSuite, openApp } from './editor-ui-harness.mjs';

const scenario = uiSuite('pollers-ui');

const nav = (page) => page.getByRole('navigation', { name: 'Telas' });
const count = (page, cmd) => page.evaluate((c) => window.__mock.calls.filter((x) => x.cmd === c).length, cmd);
// Espera ao menos `more` consultas novas de `cmd` (o poll é de 1,5 s) e uma folga para o render assentar.
async function waitPolls(page, cmd, more) {
  const start = await count(page, cmd);
  await page.waitForFunction(([c, n]) => window.__mock.calls.filter((x) => x.cmd === c).length >= n, [cmd, start + more], { timeout: 20000 });
  await page.waitForTimeout(300);
}

// Hooks com 2+ updates na fila, na subárvore do componente `scope` (o hook de `usePolled` mora no ProjectApp, então o escopo do Git é ele todo).
function pendingHooks(page, scope) {
  return page.evaluate((scopeName) => {
    // `#root` guarda a fiber do HostRoot em `__reactContainer$*`; `stateNode.current` é a árvore atual. Anda por child/sibling.
    const root = document.getElementById('root');
    const key = Object.keys(root).find((k) => k.startsWith('__reactContainer$'));
    if (!key) throw new Error('container do React não encontrado em #root');
    const nameOf = (f) => f.type?.displayName ?? f.type?.name ?? f.type?.render?.name ?? '';
    const found = [];
    let inside = 0;
    const walk = (fiber, scoped) => {
      for (let f = fiber; f; f = f.sibling) {
        const here = scoped || nameOf(f) === scopeName;
        if (here) inside++;
        if (here && (f.tag === 0 || f.tag === 11 || f.tag === 14 || f.tag === 15)) {
          let hook = 0;
          for (let h = f.memoizedState; h && typeof h === 'object' && 'baseQueue' in h; h = h.next, hook++) {
            const last = h.queue?.pending;
            let n = last ? 1 : 0;
            for (let u = last?.next; last && u !== last; u = u.next) n++;
            if (n >= 2) found.push(`${nameOf(f)}#${hook} (${n} updates)`);
          }
        }
        walk(f.child, here);
      }
    };
    walk(root[key].stateNode?.current ?? root[key], false);
    if (inside < 5) throw new Error(`subárvore de ${scopeName} não encontrada (${inside} fibers)`);
    return found;
  }, scope);
}

scenario('tela Git parada: o poll do status não enfileira updates equivalentes', async ({ page }) => {
  await openApp(page);
  await nav(page).getByRole('button', { name: /^Git/ }).click();
  await page.getByRole('tab', { name: /^Alterações/ }).waitFor();
  await waitPolls(page, 'git_status', 4);
  const pending = await pendingHooks(page, 'ProjectApp');
  assert.deepEqual(pending, [], `updates retidos na fila dos hooks: ${pending.join(', ')}`);
});

scenario('tela Arquivos parada: o poll da árvore não enfileira updates equivalentes', async ({ page }) => {
  await openApp(page);
  await page.getByRole('treeitem').first().waitFor();
  await waitPolls(page, 'list_dirs', 4);
  const pending = await pendingHooks(page, 'FilesScreen');
  assert.deepEqual(pending, [], `updates retidos na fila dos hooks: ${pending.join(', ')}`);
});
