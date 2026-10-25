// Check do modelo do espaço de trabalho (src/editor/workspace-model.ts): grupos, prévias, pins, fechamento por decisão externa, reabertura,
// remapeamento de pastas e estado salvo adulterado. Roda sem dependências: `node tests/check-workspace.mjs`.
// Sai com 1 e mostra a falha se alguma regra quebrar.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import {
  activate, closeTabs, createWorkspace, draftName, isDirty, isSafeFilePath, joinGroups, liveDocIds, loadWorkspace, markEdited, moveTab, navigate,
  openDraft, openPermanent, openPreview, planClose, promoteDraft, remapPath, reopenClosed, reorderTab, retargetDoc, splitTab, targetKey, togglePin,
} from '../src/editor/workspace-model.ts';

const NONE = new Set();
const file = (docId, path) => ({ kind: 'file', docId, path });
const diff = (side, path) => ({ kind: 'diff', side, path });
const commit = (hash, path) => ({ kind: 'commit', hash, path });
const conflict = (path) => ({ kind: 'conflict', path });
const draft = (docId, num) => ({ kind: 'draft', docId, num });

// Invariantes: valem depois de qualquer operação.
function assertConsistent(state) {
  assert.equal(state.version, 1);
  assert.ok(state.groups.length >= 1 && state.groups.length <= 2, 'entre 1 e 2 grupos');
  assert.equal(new Set(state.groups.map((g) => g.id)).size, state.groups.length, 'ids de grupo únicos');
  assert.ok(state.groups.some((g) => g.id === state.focus), 'foco aponta para grupo existente');
  const ids = state.groups.flatMap((g) => g.tabs.map((t) => t.id));
  assert.equal(new Set(ids).size, ids.length, 'ids de aba únicos no workspace');
  for (const g of state.groups) {
    const keys = g.tabs.map((t) => targetKey(t.target));
    assert.equal(new Set(keys).size, keys.length, `identidade única no grupo ${g.id}`);
    const firstUnpinned = g.tabs.findIndex((t) => !t.pinned);
    if (firstUnpinned >= 0) assert.ok(g.tabs.slice(firstUnpinned).every((t) => !t.pinned), `fixadas antes das demais no grupo ${g.id}`);
    assert.ok(g.tabs.filter((t) => t.preview).length <= 1, `no máximo uma prévia no grupo ${g.id}`);
    assert.ok(g.tabs.every((t) => !(t.pinned && t.preview)), 'fixada não é prévia');
    if (g.tabs.length === 0) assert.equal(g.active, null, 'grupo vazio sem ativa');
    else assert.ok(g.tabs.some((t) => t.id === g.active), `ativa do grupo ${g.id} existe nele`);
  }
  assert.ok(state.closed.length <= 20, 'pilha de fechadas limitada');
}

const groupOf = (state, id) => state.groups.find((g) => g.id === id);
const tabsOf = (state, id) => groupOf(state, id).tabs;
const label = (tab) => {
  const t = tab.target;
  if (t.kind === 'file') return t.path;
  if (t.kind === 'diff') return `${t.side}:${t.path}`;
  if (t.kind === 'commit') return `${t.hash}:${t.path}`;
  if (t.kind === 'draft') return `Sem título-${t.num}`;
  return `conflito:${t.path}`;
};
const names = (state, id) => tabsOf(state, id).map(label);
const openAll = (state, group, entries) => entries.reduce((s, [docId, path]) => openPermanent(s, group, file(docId, path)), state);
const idOf = (state, group, path) => tabsOf(state, group).find((t) => label(t) === path).id;

check('Prévia suja é preservada: a nova prévia não a substitui, ela vira permanente', () => {
  let s = openPreview(createWorkspace(), 'A', file('d1', 'a.ts'), NONE);
  s = openPreview(s, 'A', file('d2', 'b.ts'), new Set(['d1']));
  assert.deepEqual(names(s, 'A'), ['a.ts', 'b.ts']);
  const [a, b] = tabsOf(s, 'A');
  assert.equal(a.preview, false);
  assert.equal(b.preview, true);
  assertConsistent(s);
});

check('Prévia limpa é substituída: uma só prévia, no mesmo lugar, e a permanente fica', () => {
  let s = openPermanent(createWorkspace(), 'A', file('d0', 'z.ts'));
  s = openPreview(s, 'A', file('d1', 'a.ts'), NONE);
  s = openPreview(s, 'A', file('d2', 'b.ts'), NONE);
  assert.deepEqual(names(s, 'A'), ['z.ts', 'b.ts']);
  assert.equal(tabsOf(s, 'A')[1].preview, true);
  assertConsistent(s);
});

check('Prévia da mesma identidade só ativa; editar, fixar e duplo clique tornam permanente', () => {
  let s = openPreview(createWorkspace(), 'A', file('d1', 'a.ts'), NONE);
  s = openPreview(s, 'A', file('d1', 'a.ts'), NONE);
  assert.equal(tabsOf(s, 'A').length, 1);
  s = markEdited(s, tabsOf(s, 'A')[0].id);
  assert.equal(tabsOf(s, 'A')[0].preview, false);
  s = openPreview(s, 'A', file('d2', 'b.ts'), NONE);
  s = openPermanent(s, 'A', file('d2', 'b.ts'));
  assert.equal(tabsOf(s, 'A')[1].preview, false, 'duplo clique sobre a prévia a torna permanente');
  s = openPreview(s, 'A', file('d3', 'c.ts'), NONE);
  s = togglePin(s, idOf(s, 'A', 'c.ts'));
  assert.equal(tabsOf(s, 'A')[0].preview === false && tabsOf(s, 'A')[0].pinned, true, 'fixar a prévia a torna permanente e a põe no começo');
  assertConsistent(s);
});

check('Fixar: a fixada vai para antes das demais e a desafixada continua na faixa', () => {
  let s = openAll(createWorkspace(), 'A', [['d1', 'a.ts'], ['d2', 'b.ts'], ['d3', 'c.ts']]);
  s = togglePin(s, idOf(s, 'A', 'c.ts'));
  assert.deepEqual(names(s, 'A'), ['c.ts', 'a.ts', 'b.ts']);
  s = togglePin(s, idOf(s, 'A', 'c.ts'));
  assert.equal(tabsOf(s, 'A').length, 3, 'desafixar não perde a aba');
  assert.ok(tabsOf(s, 'A').every((t) => !t.pinned));
  assertConsistent(s);
});

check('Mesma doc em dois grupos: split copia a identidade com id próprio; abrir de novo só ativa', () => {
  let s = openAll(createWorkspace(), 'A', [['d1', 'a.ts'], ['d2', 'b.ts']]);
  const original = idOf(s, 'A', 'a.ts');
  s = splitTab(s, original);
  assert.deepEqual(names(s, 'A'), ['a.ts', 'b.ts']);
  assert.deepEqual(names(s, 'B'), ['a.ts']);
  const copy = tabsOf(s, 'B')[0];
  assert.notEqual(copy.id, original);
  assert.equal(copy.target.docId, 'd1', 'mesmo docId: o buffer é compartilhado');
  assert.equal(s.focus, 'B');
  assertConsistent(s);

  const again = openPermanent(s, 'A', file('d1', 'a.ts'));
  assert.equal(tabsOf(again, 'A').length, 2, 'não duplica no mesmo grupo');
  assert.equal(groupOf(again, 'A').active, original, 'só ativa a aba que já estava lá');
  assert.equal(again.focus, 'A');
  assertConsistent(again);
});

check('Mover aba entre grupos mantém o id e escolhe a vizinha ativa na origem', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts'], ['dc', 'c.ts']]);
  const b = idOf(s, 'A', 'b.ts');
  s = activate(s, b);
  s = moveTab(s, b, 'B', 0);
  assert.deepEqual(names(s, 'A'), ['a.ts', 'c.ts']);
  assert.deepEqual(names(s, 'B'), ['b.ts']);
  assert.equal(tabsOf(s, 'B')[0].id, b);
  assert.equal(groupOf(s, 'A').active, idOf(s, 'A', 'c.ts'), 'vizinha da direita vira ativa na origem');
  assert.equal(groupOf(s, 'B').active, b);
  assert.equal(s.focus, 'B');
  assertConsistent(s);
});

check('Mover para grupo que já tem a mesma identidade é recusado; nada muda', () => {
  let s = openAll(createWorkspace(), 'A', [['d1', 'a.ts']]);
  s = splitTab(s, idOf(s, 'A', 'a.ts'));
  const before = structuredClone(s);
  assert.deepEqual(moveTab(s, idOf(s, 'A', 'a.ts'), 'B'), before);
});

check('Reordenar: a aba muda de posição e a fixada continua no começo', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts'], ['dc', 'c.ts']]);
  s = togglePin(s, idOf(s, 'A', 'a.ts'));
  s = reorderTab(s, idOf(s, 'A', 'c.ts'), 0);
  assert.deepEqual(names(s, 'A'), ['a.ts', 'c.ts', 'b.ts'], 'fixada segue na frente mesmo com a ordem pedida');
  s = reorderTab(s, idOf(s, 'A', 'b.ts'), 1);
  assert.deepEqual(names(s, 'A'), ['a.ts', 'b.ts', 'c.ts']);
  assertConsistent(s);
});

check('Fechamento em lote só gera alvos; cancelar (não chamar closeTabs) não fecha nada', () => {
  const s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts'], ['dc', 'c.ts']]);
  const before = structuredClone(s);
  const plan = planClose(s, { kind: 'all', group: 'A' }, new Set(['db']));
  assert.deepEqual(plan.tabIds, [idOf(s, 'A', 'a.ts'), idOf(s, 'A', 'b.ts'), idOf(s, 'A', 'c.ts')]);
  assert.deepEqual(plan.unsaved, [idOf(s, 'A', 'b.ts')]);
  assert.deepEqual(s, before, 'planejar não muda o estado');
});

check('Fechar Tudo e Fechar Salvos: fixadas ficam; confirmar fecha as não fixadas', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts'], ['dc', 'c.ts']]);
  s = togglePin(s, idOf(s, 'A', 'a.ts'));
  const all = planClose(s, { kind: 'all', group: 'A' }, new Set(['dc']));
  assert.deepEqual(all.tabIds, [idOf(s, 'A', 'b.ts'), idOf(s, 'A', 'c.ts')]);
  const saved = planClose(s, { kind: 'saved', group: 'A' }, new Set(['dc']));
  assert.deepEqual(saved.tabIds, [idOf(s, 'A', 'b.ts')]);
  const closed = closeTabs(s, all.tabIds);
  assert.deepEqual(names(closed, 'A'), ['a.ts']);
  assertConsistent(closed);
});

check('Fechar Outros e à Direita: a referência fica e as fixadas não entram no lote', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts'], ['dc', 'c.ts'], ['dd', 'd.ts']]);
  s = togglePin(s, idOf(s, 'A', 'd.ts'));
  const others = planClose(s, { kind: 'others', tabId: idOf(s, 'A', 'b.ts') }, NONE);
  assert.deepEqual(others.tabIds, [idOf(s, 'A', 'a.ts'), idOf(s, 'A', 'c.ts')]);
  const right = planClose(s, { kind: 'right', tabId: idOf(s, 'A', 'b.ts') }, NONE);
  assert.deepEqual(right.tabIds, [idOf(s, 'A', 'c.ts')]);
  const one = planClose(s, { kind: 'one', tabId: idOf(s, 'A', 'd.ts') }, NONE);
  assert.deepEqual(one.tabIds, [idOf(s, 'A', 'd.ts')], 'fechar explícito alcança a fixada');
});

check('Fechar a ativa escolhe a vizinha da direita; sem ela, a da esquerda', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts'], ['dc', 'c.ts']]);
  s = activate(s, idOf(s, 'A', 'b.ts'));
  s = closeTabs(s, [idOf(s, 'A', 'b.ts')]);
  assert.equal(groupOf(s, 'A').active, idOf(s, 'A', 'c.ts'));
  s = closeTabs(s, [idOf(s, 'A', 'c.ts')]);
  assert.equal(groupOf(s, 'A').active, idOf(s, 'A', 'a.ts'));
  assertConsistent(s);
});

check('Reabrir a última fechada: volta ao grupo, ativa e permanente; depois a anterior', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts']]);
  s = closeTabs(s, [idOf(s, 'A', 'b.ts')]);
  s = closeTabs(s, [idOf(s, 'A', 'a.ts')]);
  s = reopenClosed(s);
  assert.deepEqual(names(s, 'A'), ['a.ts']);
  assert.equal(groupOf(s, 'A').active, idOf(s, 'A', 'a.ts'));
  s = reopenClosed(s);
  assert.deepEqual(names(s, 'A'), ['a.ts', 'b.ts']);
  assert.equal(s.closed.length, 0);
  assert.equal(reopenClosed(s), s, 'sem nada fechado, não muda');
  assertConsistent(s);
});

check('Reabrir com resolve: usa o alvo resolvido e só ativa se a identidade já está no grupo', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts']]);
  s = closeTabs(s, [idOf(s, 'A', 'a.ts')]);
  s = openPermanent(s, 'A', file('da2', 'a.ts'));
  const resolve = (t) => (t.kind === 'file' && t.path === 'a.ts' ? file('da2', 'a.ts') : t);
  s = activate(s, idOf(s, 'A', 'b.ts'));
  s = reopenClosed(s, resolve);
  assert.deepEqual(names(s, 'A'), ['b.ts', 'a.ts'], 'sem aba duplicada');
  assert.equal(groupOf(s, 'A').active, idOf(s, 'A', 'a.ts'));
  assert.equal(s.closed.length, 0);
  assertConsistent(s);
});

check('Anterior/próxima dão a volta no grupo focado', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts'], ['dc', 'c.ts']]);
  assert.equal(groupOf(s, 'A').active, idOf(s, 'A', 'c.ts'));
  s = navigate(s, 1);
  assert.equal(groupOf(s, 'A').active, idOf(s, 'A', 'a.ts'), 'próxima dá a volta');
  s = navigate(s, -1);
  assert.equal(groupOf(s, 'A').active, idOf(s, 'A', 'c.ts'), 'anterior dá a volta');
  assertConsistent(s);
});

check('Grupo que fica vazio some sozinho e nenhuma aba se perde', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts']]);
  s = splitTab(s, idOf(s, 'A', 'a.ts'));
  const copy = tabsOf(s, 'B')[0].id;
  // Fechar a última aba de um lado: o grupo vazio some sozinho e a área inteira volta ao outro (nada fica sem área).
  s = closeTabs(s, [copy]);
  assert.equal(s.groups.length, 1);
  assert.equal(s.focus, 'A');
  assert.deepEqual(names(s, 'A'), ['a.ts', 'b.ts']);
  assertConsistent(s);
});

check('Unir grupos: identidade repetida fica uma vez, fixada/permanente vence, ativa do primeiro é mantida', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts']]);
  s = splitTab(s, idOf(s, 'A', 'b.ts'));
  s = openPermanent(s, 'B', file('dc', 'c.ts'));
  s = joinGroups(s);
  assert.equal(s.groups.length, 1);
  assert.deepEqual(names(s, 'A'), ['a.ts', 'b.ts', 'c.ts']);
  assertConsistent(s);
});

check('Renomear pasta: troca o prefixo em arquivo, diff, histórico e conflito, abertos ou fechados', () => {
  let s = openAll(createWorkspace(), 'A', [['d1', 'src/App.tsx'], ['d2', 'src2/a.ts']]);
  s = openPermanent(s, 'A', diff('staged', 'src/x.ts'));
  s = openPermanent(s, 'A', diff('unstaged', 'src/x.ts'));
  s = openPermanent(s, 'A', commit('abc1234', 'src/y.ts'));
  s = openPermanent(s, 'A', conflict('SRC/z.ts'));
  s = closeTabs(s, [idOf(s, 'A', 'unstaged:src/x.ts')]);
  s = remapPath(s, 'src', 'lib/core');
  // Histórico é imutável: o commit mantém o path da revisão; só diff, arquivo e conflito seguem a pasta.
  assert.deepEqual(names(s, 'A'), ['lib/core/App.tsx', 'src2/a.ts', 'staged:lib/core/x.ts', 'abc1234:src/y.ts', 'conflito:lib/core/z.ts']);
  assert.equal(s.closed[0].target.path, 'lib/core/x.ts', 'a aba fechada também é remapeada');
  assert.equal(tabsOf(s, 'A')[0].target.docId, 'd1', 'o docId não muda com o path');
  assertConsistent(s);
});

check('Renomear arquivo mantém a aba, o id e o docId (o buffer continua o mesmo)', () => {
  let s = openAll(createWorkspace(), 'A', [['d1', 'src/App.tsx']]);
  const before = tabsOf(s, 'A')[0];
  s = remapPath(s, 'src/App.tsx', 'src/Main.tsx');
  const after = tabsOf(s, 'A')[0];
  assert.equal(after.id, before.id);
  assert.equal(after.target.docId, 'd1');
  assert.equal(after.target.path, 'src/Main.tsx');
});

check('Remapeamento: src não pega src2 nem pasta de nome parecido; caixa do Windows não importa', () => {
  let s = openAll(createWorkspace(), 'A', [['d1', 'src2/a.ts'], ['d2', 'SRC/b.ts'], ['d3', 'srcx/c.ts']]);
  s = remapPath(s, 'src', 'novo');
  assert.deepEqual(names(s, 'A'), ['src2/a.ts', 'novo/b.ts', 'srcx/c.ts']);
});

check('Identidade histórica diferencia commit e lado do diff', () => {
  assert.notEqual(targetKey(commit('abc1234', 'a.ts')), targetKey(commit('def5678', 'a.ts')));
  assert.notEqual(targetKey(diff('staged', 'a.ts')), targetKey(diff('unstaged', 'a.ts')));
  assert.equal(targetKey(diff('staged', 'SRC/a.ts')), targetKey(diff('staged', 'src/a.ts')));
});

check('Estado salvo volta igual pelo JSON', () => {
  let s = openAll(createWorkspace(), 'A', [['da', 'a.ts'], ['db', 'b.ts']]);
  s = togglePin(s, idOf(s, 'A', 'a.ts'));
  s = splitTab(s, idOf(s, 'A', 'b.ts'));
  s = openPreview(s, 'B', diff('staged', 'c.ts'), NONE);
  s = closeTabs(s, [idOf(s, 'A', 'b.ts')]);
  const loaded = loadWorkspace(JSON.stringify(s));
  assert.deepEqual(loaded.notes, []);
  assert.deepEqual(loaded.state, s);
});

check('JSON quebrado, versão desconhecida ou não-objeto começam vazios, sem abrir nada', () => {
  for (const text of ['{nope', JSON.stringify({ version: 2, groups: [] }), 'null', '[]', '"A"']) {
    const loaded = loadWorkspace(text);
    assert.deepEqual(loaded.state, createWorkspace(), text);
    assert.equal(loaded.notes.length, 1, text);
  }
});

check('Estado salvo aceita arquivos absolutos e descarta caminhos inválidos', () => {
  const tampered = {
    version: 1,
    groups: [
      {
        id: 'A',
        active: 't9',
        tabs: [
          { id: 't1', target: file('x', '../../secret.env'), pinned: false, preview: false },
          { id: 't2', target: file('y', 'C:\\Windows\\win.ini'), pinned: false, preview: false },
          { id: 't3', target: file('z', '/etc/passwd'), pinned: false, preview: false },
          { id: 't4', target: file('w', 'src\\a.ts'), pinned: false, preview: false },
          { id: 't5', target: file('v', 'src//a.ts'), pinned: false, preview: false },
          { id: 't6', target: file('u', 'src/a.ts'), pinned: true, preview: true },
          { id: 't7', target: file('s', 'src/b.ts'), pinned: false, preview: true },
          { id: 't8', target: file('r', 'src/c.ts'), pinned: false, preview: true },
          { id: 't6', target: file('dup', 'x.ts'), pinned: false, preview: false },
          { id: 't10', target: commit('zz', 'a.ts'), pinned: false, preview: false },
        ],
      },
      { id: 'C', tabs: [] },
      { id: 'A', tabs: [] },
      { id: 'B', tabs: [] },
    ],
    focus: 'Z',
    recent: ['t99', 't7'],
    closed: [{ group: 'Q', target: file('k', '../x') }],
    nextId: -4,
  };
  const { state, notes } = loadWorkspace(JSON.stringify(tampered));
  assertConsistent(state);
  assert.ok(notes.length > 0);
  for (const g of state.groups) for (const t of g.tabs) if (t.target.kind === 'file') assert.equal(isSafeFilePath(t.target.path), true, t.target.path);
  assert.deepEqual(names(state, 'A'), ['src/a.ts', 'C:\\Windows\\win.ini', '/etc/passwd', 'src/b.ts', 'src/c.ts']);
  assert.deepEqual(tabsOf(state, 'A').map((t) => [t.pinned, t.preview]), [[true, false], [false, false], [false, false], [false, true], [false, false]]);
  assert.equal(tabsOf(state, 'A')[0].id, 't6', 'id repetido: fica o primeiro');
  assert.equal(groupOf(state, 'A').active, 't6', 'ativa inexistente cai na primeira');
  // B vazio some (o A tem abas), então sobra um grupo só.
  assert.equal(state.groups.length, 1);
  assert.equal(state.focus, 'A');
  assert.deepEqual(state.closed, []);
  assert.equal(state.nextId, 1);
});

check('Rascunho volta no JSON do workspace', () => {
  const state = openDraft(createWorkspace(), 'A', 'draft-1');
  const loaded = loadWorkspace(JSON.stringify(state), true);
  assert.deepEqual(loaded.notes, []);
  assert.deepEqual(loaded.state, state);
});

check('Estado adulterado com 3 grupos ou grupo repetido: no máximo dois, sem id repetido', () => {
  const tampered = {
    version: 1,
    focus: 'B',
    groups: [
      { id: 'A', active: null, tabs: [{ id: 'a1', target: file('d1', 'a.ts'), pinned: false, preview: false }] },
      { id: 'A', active: 'a1', tabs: [{ id: 'a2', target: file('d2', 'b.ts'), pinned: false, preview: false }] },
      { id: 'B', active: null, tabs: [] },
      { id: 'B', active: null, tabs: [] },
    ],
    recent: [],
    closed: [],
    nextId: 3,
  };
  const { state } = loadWorkspace(JSON.stringify(tampered));
  assertConsistent(state);
  // Grupo B vazio some (o A tem abas): a área inteira fica com o A, e o foco que apontava para B cai nele.
  assert.deepEqual(state.groups.map((g) => g.id), ['A']);
  assert.equal(groupOf(state, 'A').active, 'a1');
  assert.equal(state.focus, 'A');
});

check('Histórico não muda de path ao mover pasta; o arquivo atual e o conflito mudam', () => {
  let s = openPermanent(createWorkspace(), 'A', commit('abcdef1', 'src/a.ts'));
  s = openPermanent(s, 'A', file('d1', 'src/a.ts'));
  s = openPermanent(s, 'A', conflict('src/a.ts'));
  s = remapPath(s, 'src', 'novo');
  const targets = groupOf(s, 'A').tabs.map((t) => t.target);
  assert.deepEqual(targets.find((t) => t.kind === 'commit'), commit('abcdef1', 'src/a.ts'));
  assert.deepEqual(targets.find((t) => t.kind === 'file'), file('d1', 'novo/a.ts'));
  assert.deepEqual(targets.find((t) => t.kind === 'conflict'), conflict('novo/a.ts'));
  assertConsistent(s);
});

check('Rascunho de conflito conta como sujo no fechamento', () => {
  const s = openPermanent(createWorkspace(), 'A', conflict('src/a.ts'));
  assert.equal(planClose(s, { kind: 'all', group: 'A' }, new Set(['conflict:src/a.ts'])).unsaved.length, 1);
  assert.equal(planClose(s, { kind: 'all', group: 'A' }, NONE).unsaved.length, 0);
});

check('Caminho .git em qualquer caixa ou com ponto/espaço final é recusado ao carregar', () => {
  for (const bad of ['.git/config', '.GIT/HEAD', 'x/.Git./y', 'x/.git ']) {
    const text = JSON.stringify({ version: 1, focus: 'A', recent: [], closed: [], nextId: 2,
      groups: [{ id: 'A', active: 'a', tabs: [{ id: 'a', target: file('d', bad), pinned: false, preview: false }] }] });
    const { state, notes } = loadWorkspace(text);
    assert.equal(groupOf(state, 'A').tabs.length, 0, `aceitou ${bad}`);
    assert.ok(notes.length > 0);
  }
});

check('Id no limite do inteiro seguro não trava o mint e não repete aba', () => {
  const max = Number.MAX_SAFE_INTEGER;
  let s = { ...createWorkspace(), nextId: max - 1 };
  s = openPermanent(s, 'A', file('d1', 'a.ts'));
  s = openPermanent(s, 'A', file('d2', 'b.ts'));
  assertConsistent(s);
  const { state } = loadWorkspace(JSON.stringify({ ...s, nextId: max }));
  assert.equal(state.nextId, 1);
  const ids = state.groups.flatMap((g) => g.tabs.map((t) => t.id));
  assert.equal(new Set(ids).size, ids.length);
});

check('Rascunho: numera pelo menor número livre, fica ativo e com o foco', () => {
  let s = openDraft(createWorkspace(), 'A', 'x1');
  s = openDraft(s, 'A', 'x2');
  assert.deepEqual(names(s, 'A'), ['Sem título-1', 'Sem título-2']);
  assert.ok(tabsOf(s, 'A').every((t) => !t.preview), 'rascunho nunca é prévia');
  assert.equal(groupOf(s, 'A').active, tabsOf(s, 'A')[1].id);
  s = closeTabs(s, [tabsOf(s, 'A')[0].id]);
  s = openDraft(s, 'A', 'x3');
  assert.deepEqual(names(s, 'A'), ['Sem título-2', 'Sem título-1'], 'reaproveita o 1 livre');
  assert.equal(draftName(7), 'Sem título-7');
  assertConsistent(s);
});

check('Docs vivos: arquivo e rascunho têm buffer; diff, histórico e conflito não; fechar outra aba mantém o rascunho', () => {
  let s = openPermanent(createWorkspace(), 'A', file('d1', 'a.ts'));
  s = openPermanent(s, 'A', diff('staged', 'a.ts'));
  s = openDraft(s, 'A', 'x1');
  s = openPermanent(s, 'A', conflict('b.ts'));
  assert.deepEqual([...liveDocIds(s)].sort(), ['d1', 'x1']);
  const closed = closeTabs(s, [tabsOf(s, 'A')[0].id]);
  assert.deepEqual([...liveDocIds(closed)].sort(), ['x1'], 'o rascunho sobrevive ao fechamento de outra aba');
  assertConsistent(closed);
});

check('Rascunho em outro grupo: vai para ele e leva o foco', () => {
  let s = openPermanent(createWorkspace(), 'A', file('d1', 'a.ts'));
  s = splitTab(s, tabsOf(s, 'A')[0].id);
  s = openDraft(s, 'B', 'x1');
  assert.equal(tabsOf(s, 'B').at(-1).target.num, 1);
  assert.equal(s.focus, 'B');
  assertConsistent(s);
});

check('Rascunho sujo conta como sujo pelo docId; vazio não, e fechar sujo pede decisão', () => {
  const s = openDraft(createWorkspace(), 'A', 'x1');
  const tab = tabsOf(s, 'A')[0];
  assert.equal(isDirty(tab.target, new Set(['x1'])), true);
  assert.equal(isDirty(tab.target, NONE), false);
  assert.deepEqual(planClose(s, { kind: 'one', tabId: tab.id }, new Set(['x1'])).unsaved, [tab.id]);
  assert.deepEqual(planClose(s, { kind: 'one', tabId: tab.id }, NONE).unsaved, [], 'rascunho vazio fecha sem perguntar');
});

check('Rascunho fechado não entra na pilha de fechadas (não ressuscita)', () => {
  let s = openDraft(createWorkspace(), 'A', 'x1');
  s = openPermanent(s, 'A', file('d1', 'a.ts'));
  s = closeTabs(s, tabsOf(s, 'A').map((t) => t.id));
  assert.deepEqual(s.closed, [{ group: 'A', target: file('d1', 'a.ts') }]);
  assertConsistent(s);
});

check('Rascunho volta na recuperação do projeto e na recuperação sem projeto', () => {
  const serialized = JSON.stringify(openDraft(createWorkspace(), 'A', 'x1'));
  for (const storageKey of ['orquestrador.editor:C:\\projetos', 'orquestrador.editor:__sem-projeto__']) {
    const loaded = loadWorkspace(serialized, true);
    assert.equal(loaded.state.groups[0].tabs[0].target.kind, 'draft', storageKey);
    assert.deepEqual(loaded.notes, [], storageKey);
  }
});

check('Salvar como dentro do projeto: rascunho vira aba de arquivo, mesmo id e mesmo docId', () => {
  let s = openDraft(createWorkspace(), 'A', 'x1');
  const tab = tabsOf(s, 'A')[0];
  s = promoteDraft(s, tab.id, 'pasta/novo.ts');
  assert.deepEqual(tabsOf(s, 'A')[0], { id: tab.id, target: file('x1', 'pasta/novo.ts'), pinned: false, preview: false });
  assertConsistent(s);
  assert.equal(promoteDraft(s, tab.id, 'outro.ts'), s, 'já não é rascunho: nada muda');
  const again = openDraft(createWorkspace(), 'A', 'x2');
  assert.equal(promoteDraft(again, tabsOf(again, 'A')[0].id, '../fora.ts'), again, 'caminho inseguro recusado');
});

check('Salvar como em arquivo aberto: só as abas de arquivo do docId trocam de caminho; diff, conflito e fechadas do antigo ficam', () => {
  let s = openAll(createWorkspace(), 'A', [['d1', 'a.ts']]);
  s = splitTab(s, idOf(s, 'A', 'a.ts'));
  s = openPermanent(s, 'A', diff('unstaged', 'a.ts'));
  s = openPermanent(s, 'A', conflict('a.ts'));
  s = openPermanent(s, 'A', file('d2', 'c.ts'));
  s = closeTabs(s, [idOf(s, 'A', 'c.ts')]);
  const moved = retargetDoc(s, 'd1', 'b.ts');
  assert.deepEqual(names(moved, 'A'), ['b.ts', 'unstaged:a.ts', 'conflito:a.ts']);
  assert.deepEqual(names(moved, 'B'), ['b.ts']);
  assert.deepEqual(moved.closed, s.closed, 'pilha de fechadas intacta');
  assert.equal(tabsOf(moved, 'A')[0].id, tabsOf(s, 'A')[0].id, 'a aba continua a mesma');
  assertConsistent(moved);
  assert.equal(retargetDoc(s, 'd1', '../fora.ts'), s, 'caminho inseguro recusado');
  assert.deepEqual(names(retargetDoc(s, 'nenhum', 'x.ts'), 'A'), names(s, 'A'), 'docId sem aba: nada muda');
});

check('Remapeamento não mexe no rascunho e segue movendo o arquivo ao lado', () => {
  let s = openDraft(createWorkspace(), 'A', 'x1');
  s = openPermanent(s, 'A', file('d1', 'src/a.ts'));
  s = remapPath(s, 'src', 'lib');
  assert.deepEqual(names(s, 'A'), ['Sem título-1', 'lib/a.ts']);
  assertConsistent(s);
});
