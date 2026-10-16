// Check do modelo do espaço de trabalho (src/workspace-model.ts): grupos, prévias, pins, fechamento por decisão externa, reabertura,
// remapeamento de pastas e estado salvo adulterado. Roda sem dependências: `node scripts/check-workspace.mjs`.
// Sai com 1 e mostra a falha se alguma regra quebrar.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import {
  activate, closeTabs, createWorkspace, joinGroups, loadWorkspace, markEdited, moveTab, navigate,
  openPermanent, openPreview, planClose, remapPath, reopenClosed, reorderTab, splitTab, targetKey, togglePin,
} from '../src/workspace-model.ts';

const NONE = new Set();
const file = (docId, path) => ({ kind: 'file', docId, path });
const diff = (side, path) => ({ kind: 'diff', side, path });
const commit = (hash, path) => ({ kind: 'commit', hash, path });
const conflict = (path) => ({ kind: 'conflict', path });

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

check('Estado adulterado: paths fora do projeto são descartados e o resto é consertado', () => {
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
  for (const g of state.groups) for (const t of g.tabs) assert.ok(/^(?!\/)(?!.*[\\:])(?!(.*\/)?\.\.?(\/|$))(?!.*\/\/)[^\u0000-\u001f]+$/.test(t.target.path), t.target.path);
  assert.deepEqual(names(state, 'A'), ['src/a.ts', 'src/b.ts', 'src/c.ts']);
  assert.deepEqual(tabsOf(state, 'A').map((t) => [t.pinned, t.preview]), [[true, false], [false, true], [false, false]]);
  assert.equal(tabsOf(state, 'A')[0].id, 't6', 'id repetido: fica o primeiro');
  assert.equal(groupOf(state, 'A').active, 't6', 'ativa inexistente cai na primeira');
  // B vazio some (o A tem abas), então sobra um grupo só.
  assert.equal(state.groups.length, 1);
  assert.equal(state.focus, 'A');
  assert.deepEqual(state.closed, []);
  assert.equal(state.nextId, 1);
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

// As Issues não são abas do editor: recuperações de versões antigas podem trazer as abas `issues` e `new-issue`, que saem em silêncio.
const legacyTab = (id, kind) => ({ id, target: { kind }, pinned: false, preview: false });

check('Recuperação antiga com abas issues e new-issue: lida sem elas e sem perder as outras', () => {
  let s = openPermanent(createWorkspace(), 'A', file('d1', 'a.ts'));
  s = openPermanent(s, 'A', file('d2', 'b.ts'));
  const [a, b] = tabsOf(s, 'A');
  const old = {
    ...s,
    groups: [{ id: 'A', tabs: [a, legacyTab('tab-90', 'issues'), b, legacyTab('tab-91', 'new-issue')], active: 'tab-90' }],
    closed: [{ group: 'A', target: { kind: 'issues' } }, { group: 'A', target: { kind: 'new-issue' } }, { group: 'A', target: file('d3', 'c.ts') }],
  };
  const { state, notes } = loadWorkspace(JSON.stringify(old));
  assertConsistent(state);
  assert.deepEqual(notes, [], 'sai sem aviso');
  assert.deepEqual(names(state, 'A'), ['a.ts', 'b.ts'], 'as outras abas ficam, na ordem');
  assert.equal(state.groups[0].active, b.id, 'a ativa era a Issues: passa para a vizinha da direita');
  assert.deepEqual(state.closed.map((c) => c.target.path), ['c.ts'], 'nem pela pilha de fechadas');
});

check('Recuperação antiga: ativa antiga no fim passa para a da esquerda; grupo só com Issues fica vazio', () => {
  let s = openPermanent(createWorkspace(), 'A', file('d1', 'a.ts'));
  const [a] = tabsOf(s, 'A');
  const old = {
    ...s,
    groups: [
      { id: 'A', tabs: [a, legacyTab('tab-90', 'new-issue')], active: 'tab-90' },
      { id: 'B', tabs: [legacyTab('tab-91', 'issues')], active: 'tab-91' },
    ],
  };
  const { state, notes } = loadWorkspace(JSON.stringify(old));
  assertConsistent(state);
  assert.deepEqual(notes, []);
  assert.equal(state.groups.length, 1, 'o grupo que ficou vazio some, como o modelo já trata');
  assert.equal(state.groups[0].active, a.id);
});

check('Abas issues e new-issue não são mais alvos válidos fora da leitura', () => {
  const { state, notes } = loadWorkspace(JSON.stringify({
    ...createWorkspace(), groups: [{ id: 'A', tabs: [legacyTab('tab-1', 'issues')], active: 'tab-1' }],
  }));
  assert.deepEqual(notes, []);
  assert.deepEqual(tabsOf(state, 'A'), []);
});
