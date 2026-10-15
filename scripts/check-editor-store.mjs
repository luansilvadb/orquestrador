// Check dos buffers, rascunhos, recuperação, conflitos e blocos de diff (src/workspace-store.ts).
// Roda sem dependências e sem navegador: `node scripts/check-editor-store.mjs`. Sai com 1 se alguma regra quebrar.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import {
  blockCount, conflictKey, decodeRecovery, dirtyKeys, dropData, encodeRecovery, hasDraftAt, hasMarkers, isBufferDirty, MAX_DRAFT_CHARS,
  MAX_RECOVERY_CHARS, parseConflict, patchHunks, remapData, renderConflict, toDiskNewlines,
} from '../src/workspace-store.ts';

const buffer = (over = {}) => ({
  docId: 'doc-1', path: 'src/a.ts', raw: 'a\nb', bom: false, newline: '\n', draft: 'a\nb', readOnly: false, cursor: null, ...over,
});

// ---- Buffers compartilhados e sujeira ----
check('Buffer limpo não é sujo; editar o rascunho torna sujo', () => {
  const b = buffer();
  assert.equal(isBufferDirty(b), false);
  assert.equal(isBufferDirty({ ...b, draft: 'a\nb!' }), true);
});
check('CRLF no disco e LF no editor não contam como alteração', () => {
  assert.equal(isBufferDirty(buffer({ raw: 'a\r\nb', draft: 'a\nb', newline: '\r\n' })), false);
});
check('Salvamento atrasado: texto digitado durante o save continua sujo', () => {
  // O save manda o rascunho de antes (`sent`); quem digitou depois tem um draft novo, que ainda difere do que foi gravado.
  const sentDraft = 'a\nb';
  const b = buffer({ draft: 'a\nb\nc' });
  const afterSave = { ...b, raw: toDiskNewlines(sentDraft, b.newline) };
  assert.equal(isBufferDirty(afterSave), true);
  assert.equal(isBufferDirty({ ...afterSave, draft: 'a\nb', raw: 'a\nb' }), false);
});
check('dirtyKeys: arquivo pelo docId e conflito pela chave conflict:<path>', () => {
  const data = {
    buffers: { 'doc-1': buffer({ draft: 'x' }), 'doc-2': buffer({ docId: 'doc-2', path: 'b.ts' }) },
    conflicts: { 'src/c.ts': { path: 'src/c.ts', origin: 'o', draft: 'p', revision: 'r' } },
  };
  assert.deepEqual([...dirtyKeys(data)].sort(), ['conflict:src/c.ts', 'doc-1'].sort());
});
check('Conflito sem mudança no texto não é sujo', () => {
  const data = { buffers: {}, conflicts: { 'x.ts': { path: 'x.ts', origin: 'o', draft: 'o', revision: 'r' } } };
  assert.equal(dirtyKeys(data).size, 0);
});

// ---- Rename e movimento: o localizador muda, o buffer não ----
check('Rename de pasta troca o path e mantém docId, rascunho e revisão', () => {
  const data = { buffers: { 'doc-1': buffer({ path: 'src/a.ts', draft: 'x' }) }, conflicts: {} };
  const moved = remapData(data, 'src', 'lib');
  assert.equal(moved.buffers['doc-1'].path, 'lib/a.ts');
  assert.equal(moved.buffers['doc-1'].draft, 'x');
  assert.equal(moved.buffers['doc-1'].docId, 'doc-1');
});
check('Rename não pega pasta irmã com prefixo igual, e ignora caixa', () => {
  const data = { buffers: { 'd1': buffer({ docId: 'd1', path: 'src2/x.ts' }), 'd2': buffer({ docId: 'd2', path: 'SRC/y.ts' }) }, conflicts: {} };
  const moved = remapData(data, 'src', 'novo');
  assert.equal(moved.buffers['d1'].path, 'src2/x.ts');
  assert.equal(moved.buffers['d2'].path, 'novo/y.ts');
});
check('Rename com caminho inseguro não muda nada', () => {
  const data = { buffers: { 'd1': buffer({ docId: 'd1' }) }, conflicts: {} };
  assert.equal(remapData(data, 'src', '../fora').buffers['d1'].path, 'src/a.ts');
});
check('Conflito acompanha o rename: a chave muda para o caminho novo', () => {
  const data = { buffers: {}, conflicts: { 'src/c.ts': { path: 'src/c.ts', origin: 'o', draft: 'p', revision: 'r' } } };
  const moved = remapData(data, 'src', 'lib');
  assert.deepEqual(Object.keys(moved.conflicts), ['lib/c.ts']);
  assert.equal(moved.conflicts['lib/c.ts'].draft, 'p');
});
check('Remoção por pasta descarta buffers e conflitos dentro dela', () => {
  const data = {
    buffers: { 'd1': buffer({ docId: 'd1', path: 'src/a.ts' }), 'd2': buffer({ docId: 'd2', path: 'src2/b.ts' }) },
    conflicts: { 'src/c.ts': { path: 'src/c.ts', origin: 'o', draft: 'p', revision: 'r' } },
  };
  const left = dropData(data, ['src']);
  assert.deepEqual(Object.keys(left.buffers), ['d2']);
  assert.deepEqual(Object.keys(left.conflicts), []);
});
check('hasDraftAt: rascunho dentro da pasta bloqueia, irmã não', () => {
  const data = { buffers: { 'd1': buffer({ docId: 'd1', path: 'src/a.ts', draft: 'mudou' }) }, conflicts: {} };
  assert.equal(hasDraftAt(data, 'src'), true);
  assert.equal(hasDraftAt(data, 'src/a.ts'), true);
  assert.equal(hasDraftAt(data, 'src2'), false);
  assert.equal(hasDraftAt({ buffers: { 'd1': buffer({ docId: 'd1', path: 'src/a.ts' }) }, conflicts: {} }, 'src'), false);
});
check('Arquivo apagado com rascunho continua no conjunto (recuperável)', () => {
  const data = { buffers: { 'd1': buffer({ docId: 'd1', draft: 'nao salvo' }) }, conflicts: {} };
  assert.equal(dirtyKeys(data).has('d1'), true);
});

// ---- Recuperação ----
const dirtyRecovery = {
  workspace: '{"version":1}', layout: 'layout-x', split: 0.4,
  buffers: [
    { docId: 'd1', path: 'src/a.ts', cursor: { line: 3, col: 2 }, dirty: { raw: 'a', bom: false, newline: '\n', draft: 'a!', readOnly: false } },
    { docId: 'd2', path: 'src/b.ts', cursor: null, dirty: null },
  ],
  conflicts: [{ path: 'src/c.ts', origin: '<<<<<<< x', draft: 'final', revision: 'rev-1' }],
};
check('Recuperação: ida e volta preserva rascunhos, cursor, layout e proporção', () => {
  const { text, notes } = encodeRecovery('C:/p', dirtyRecovery);
  assert.deepEqual(notes, []);
  const back = decodeRecovery(text, 'C:/p');
  assert.equal(back.recovery.layout, 'layout-x');
  assert.equal(back.recovery.split, 0.4);
  assert.equal(back.recovery.buffers[0].dirty.draft, 'a!');
  assert.deepEqual(back.recovery.buffers[0].cursor, { line: 3, col: 2 });
  assert.equal(back.recovery.buffers[1].dirty, null);
  assert.equal(back.recovery.conflicts[0].revision, 'rev-1');
});
check('Recuperação de outro projeto começa vazia', () => {
  const { text } = encodeRecovery('C:/p', dirtyRecovery);
  const back = decodeRecovery(text, 'D:/outro');
  assert.equal(back.recovery.buffers.length, 0);
  assert.equal(back.recovery.conflicts.length, 0);
});
check('Recuperação quebrada ou de outra versão começa vazia, com aviso', () => {
  assert.equal(decodeRecovery('{não é json', 'C:/p').recovery.buffers.length, 0);
  assert.equal(decodeRecovery('{"version":99,"project":"C:/p"}', 'C:/p').notes.length, 1);
});
check('Recuperação descarta caminho inseguro, .git e id duplicado', () => {
  const raw = JSON.stringify({
    version: 1, project: 'C:/p', workspace: null, layout: null, split: null, conflicts: [],
    buffers: [
      { docId: 'd1', path: '../fora.ts', cursor: null, dirty: null },
      { docId: 'd2', path: '.git/config', cursor: null, dirty: null },
      { docId: 'd3', path: 'ok.ts', cursor: null, dirty: null },
      { docId: 'd3', path: 'ok2.ts', cursor: null, dirty: null },
    ],
  });
  const back = decodeRecovery(raw, 'C:/p');
  assert.deepEqual(back.recovery.buffers.map((b) => b.path), ['ok.ts']);
  assert.ok(back.notes.length >= 3);
});
check('Recuperação valida cursor e proporção fora da faixa', () => {
  const raw = JSON.stringify({
    version: 1, project: 'P', workspace: null, layout: null, split: 2, conflicts: [],
    buffers: [{ docId: 'd1', path: 'a.ts', cursor: { line: 0, col: 1 }, dirty: null }],
  });
  const back = decodeRecovery(raw, 'P');
  assert.equal(back.recovery.split, null);
  assert.equal(back.recovery.buffers[0].cursor, null);
});
check('Rascunho grande demais não entra na recuperação, e o aviso aparece', () => {
  const big = 'x'.repeat(MAX_DRAFT_CHARS + 1);
  const { text, notes } = encodeRecovery('C:/p', {
    ...dirtyRecovery, buffers: [{ docId: 'd1', path: 'a.ts', cursor: null, dirty: { raw: 'a', bom: false, newline: '\n', draft: big, readOnly: false } }], conflicts: [],
  });
  assert.ok(notes.length >= 1);
  assert.equal(decodeRecovery(text, 'C:/p').recovery.buffers[0].dirty, null);
});
check('Estouro do total: a recuperação fica dentro do limite e avisa', () => {
  const chunk = 'y'.repeat(MAX_DRAFT_CHARS);
  const buffers = Array.from({ length: 3 }, (_, i) => ({ docId: `d${i}`, path: `f${i}.ts`, cursor: null, dirty: { raw: 'a', bom: false, newline: '\n', draft: chunk, readOnly: false } }));
  const { text, notes } = encodeRecovery('C:/p', { ...dirtyRecovery, buffers, conflicts: [] });
  assert.ok(text.length <= MAX_RECOVERY_CHARS);
  assert.ok(notes.length >= 1);
});

// ---- Conflito ----
const plain = ['antes', '<<<<<<< HEAD', 'minha 1', 'minha 2', '=======', 'deles 1', '>>>>>>> feature', 'depois'].join('\n');
const diff3 = ['<<<<<<< HEAD', 'atual', '||||||| base', 'original', '=======', 'recebida', '>>>>>>> topic'].join('\n');
check('Conflito normal: um bloco, com os dois lados', () => {
  const { pieces, malformed } = parseConflict(plain);
  assert.equal(malformed, false);
  assert.equal(blockCount(pieces), 1);
  const block = pieces.find((p) => 'block' in p).block;
  assert.deepEqual(block.ours, ['minha 1', 'minha 2']);
  assert.deepEqual(block.theirs, ['deles 1']);
  assert.equal(block.base, null);
});
check('Escolher atual, recebida ou ambas produz o texto certo', () => {
  const { pieces } = parseConflict(plain);
  assert.equal(renderConflict(pieces, ['ours']), ['antes', 'minha 1', 'minha 2', 'depois'].join('\n'));
  assert.equal(renderConflict(pieces, ['theirs']), ['antes', 'deles 1', 'depois'].join('\n'));
  assert.equal(renderConflict(pieces, ['both']), ['antes', 'minha 1', 'minha 2', 'deles 1', 'depois'].join('\n'));
});
check('Pendente devolve o texto original exato (ida e volta sem perda)', () => {
  assert.equal(renderConflict(parseConflict(plain).pieces, ['pending']), plain);
  assert.equal(renderConflict(parseConflict(diff3).pieces, ['pending']), diff3);
});
check('diff3: a base fica no bloco e a escolha não a mantém no texto', () => {
  const { pieces } = parseConflict(diff3);
  const block = pieces.find((p) => 'block' in p).block;
  assert.deepEqual(block.base, ['original']);
  assert.equal(renderConflict(pieces, ['ours']), 'atual');
});
check('Dois blocos: cada um recebe a escolha na sua ordem', () => {
  const two = [plain, 'meio', diff3].join('\n');
  const { pieces } = parseConflict(two);
  assert.equal(blockCount(pieces), 2);
  assert.equal(renderConflict(pieces, ['theirs', 'ours']), ['antes', 'deles 1', 'depois', 'meio', 'atual'].join('\n'));
});
check('Bloco que não fecha é malformado (sem ação por trecho)', () => {
  assert.equal(parseConflict('<<<<<<< x\nalgo\n=======\nmeio').malformed, true);
  assert.equal(parseConflict('<<<<<<< x\nalgo').malformed, true);
});
check('Bloco vazio de um lado (lado apagou só o texto) funciona', () => {
  const { pieces } = parseConflict('<<<<<<< HEAD\n=======\nnovo\n>>>>>>> t');
  assert.equal(renderConflict(pieces, ['ours']), '');
  assert.equal(renderConflict(pieces, ['theirs']), 'novo');
});
check('hasMarkers: pega marcador, não pega sete sinais colados em texto', () => {
  assert.equal(hasMarkers(plain), true);
  assert.equal(hasMarkers('=======\n'), true);
  assert.equal(hasMarkers('========\n'), false);
  assert.equal(hasMarkers('a=======b'), false);
  assert.equal(hasMarkers('texto limpo'), false);
});
check('conflictKey ignora caixa, como o Windows', () => {
  assert.equal(conflictKey('Src/A.ts'), conflictKey('src/a.ts'));
});

// ---- Blocos de diff ----
const patch = [
  'diff --git a/src/a.ts b/src/a.ts',
  '--- a/src/a.ts',
  '+++ b/src/a.ts',
  '@@ -1,2 +1,2 @@',
  '-old',
  '+new',
  ' keep',
  '@@ -10,1 +10,2 @@',
  ' x',
  '+y',
  '',
].join('\n');
check('patchHunks: índice, cabeçalho, linha do novo e texto sem o cabeçalho do arquivo', () => {
  const hunks = patchHunks(patch);
  assert.equal(hunks.length, 2);
  assert.equal(hunks[0].index, 0);
  assert.equal(hunks[0].header, '@@ -1,2 +1,2 @@');
  assert.equal(hunks[0].newLine, 1);
  assert.equal(hunks[1].newLine, 10);
  assert.ok(!hunks[0].text.includes('diff --git'));
  assert.ok(hunks[0].text.includes('+new'));
});
check('patchHunks: patch sem bloco devolve lista vazia', () => {
  assert.deepEqual(patchHunks('diff --git a/x b/x\n'), []);
});

// ---- Correções de QA: recuperação com aviso de limite, proporção e layout sem abas; conflito parcial; resolução ----
check('Recuperação: mais de 50 abas com rascunho avisa quantos ficaram de fora', () => {
  const buffers = Array.from({ length: 52 }, (_, i) => ({
    docId: `d${i}`, path: `f${i}.ts`, cursor: null, dirty: { raw: 'a', bom: false, newline: '\n', draft: 'b', readOnly: false },
  }));
  const { text, notes } = encodeRecovery('/p', { workspace: null, layout: null, split: null, buffers, conflicts: [] });
  assert.ok(notes.some((n) => n.includes('Mais de 50 abas') && n.includes('2 não foram guardados')), notes.join(' | '));
  assert.equal(JSON.parse(text).buffers.length, 50);
});
check('Recuperação: aba sem rascunho além de 50 não avisa (nada se perde)', () => {
  const buffers = Array.from({ length: 51 }, (_, i) => ({ docId: `d${i}`, path: `f${i}.ts`, cursor: null, dirty: null }));
  const { notes } = encodeRecovery('/p', { workspace: null, layout: null, split: null, buffers, conflicts: [] });
  assert.deepEqual(notes, []);
});
check('Recuperação: a proporção do divisor volta na leitura (não cai para 0,5)', () => {
  const { text } = encodeRecovery('/p', { workspace: null, layout: null, split: 0.3, buffers: [], conflicts: [] });
  assert.equal(decodeRecovery(text, '/p').recovery.split, 0.3);
});
check('Recuperação: layout sobrevive sem nenhuma aba (ida e volta)', () => {
  const { text } = encodeRecovery('/p', { workspace: null, layout: '{"x":1}', split: null, buffers: [], conflicts: [] });
  assert.equal(decodeRecovery(text, '/p').recovery.layout, '{"x":1}');
});
check('Conflito parcial: o texto recuperado ainda tem um trecho com marcador; a resolução do outro fica', () => {
  const partial = ['antes', '<<<<<<< ours', 'A', '=======', 'B', '>>>>>>> theirs', 'meio', '<<<<<<< ours', 'C', '=======', 'D', '>>>>>>> theirs', 'fim'].join('\n');
  const { pieces, malformed } = parseConflict(partial);
  assert.equal(malformed, false);
  assert.equal(blockCount(pieces), 2);
  // Escolhe o primeiro trecho (atual) e deixa o segundo com marcadores: o texto final tem só um bloco.
  const next = renderConflict(pieces, ['ours', 'pending']);
  assert.equal(blockCount(parseConflict(next).pieces), 1);
  assert.ok(next.includes('A') && !next.includes('>>>>>>> theirs\nmeio') && next.includes('<<<<<<< ours\nC'));
  // Sem escolhas, o texto volta igual ao recuperado.
  assert.equal(renderConflict(pieces, ['pending', 'pending']), partial);
});
check('Conflito parcial sem marcador restante não tem bloco (resolvido por completo)', () => {
  assert.equal(blockCount(parseConflict('só texto\nfinal').pieces), 0);
  assert.equal(hasMarkers('só texto\nfinal'), false);
});
