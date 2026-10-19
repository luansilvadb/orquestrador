// Check de preparar e tirar do preparo um bloco do editor (src/editor/git-stage.ts): o texto novo do índice calculado no frontend.
// Roda sem dependências e sem navegador: `node scripts/check-git-stage.mjs`. Sai com 1 se alguma regra quebrar.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import { Chunk } from '@codemirror/merge';
import { applyChunks, DIFF, stageHunk, textOf, unstageHunk } from '../src/editor/git-stage.ts';

const lines = (...ls) => ls.map((l) => l + '\n').join('');
// Blocos buffer × HEAD, como os do gutter: o bloco `i` é o que o editor passa para as funções.
const hunks = (head, buffer) => Chunk.build(textOf(head), textOf(buffer), DIFF);

check('Aplicar todos os trechos de A × B em A dá B, com e sem quebra final', () => {
  const samples = ['', 'a', 'a\n', 'b', 'a\nb', 'a\nb\n', 'b\na', 'a\nb\nc', 'x\na\nb\nc\n', 'a\n\nb', '\n', '\n\n', 'a\nc\n', 'c\nd'];
  for (const a of samples) for (const b of samples) assert.equal(applyChunks(a, b, Chunk.build(textOf(a), textOf(b), DIFF)), b, `${JSON.stringify(a)} → ${JSON.stringify(b)}`);
});

check('Preparar o bloco do meio entre três: só ele entra no índice', () => {
  const head = lines('a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i');
  const buffer = lines('A', 'b', 'c', 'd', 'E', 'f', 'g', 'h', 'I');
  const hs = hunks(head, buffer);
  assert.equal(hs.length, 3);
  assert.equal(stageHunk(head, buffer, hs[1]), lines('a', 'b', 'c', 'd', 'E', 'f', 'g', 'h', 'i'));
  // O primeiro e o último, cada um sozinho.
  assert.equal(stageHunk(head, buffer, hs[0]), lines('A', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i'));
  assert.equal(stageHunk(head, buffer, hs[2]), lines('a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'I'));
});

check('Preparar com o índice já diferente do HEAD em outro bloco: o preparado continua lá', () => {
  const head = lines('a', 'b', 'c', 'd', 'e', 'f');
  const index = lines('a', 'B', 'c', 'd', 'e', 'f');
  const buffer = lines('a', 'B', 'c', 'd', 'e', 'F');
  const hs = hunks(head, buffer);
  assert.equal(stageHunk(index, buffer, hs[1]), buffer);
});

check('Preparar linhas apagadas e linhas acrescentadas', () => {
  const head = lines('a', 'b', 'c', 'd', 'e', 'f', 'g');
  const buffer = lines('a', 'c', 'd', 'e', 'x', 'y', 'f', 'g');
  const [deleted, added] = hunks(head, buffer);
  assert.equal(deleted.fromB, deleted.toB, 'o primeiro bloco é só apagado');
  assert.equal(added.fromA, added.toA, 'o segundo bloco é só acrescentado');
  assert.equal(stageHunk(head, buffer, deleted), lines('a', 'c', 'd', 'e', 'f', 'g'));
  assert.equal(stageHunk(head, buffer, added), lines('a', 'b', 'c', 'd', 'e', 'x', 'y', 'f', 'g'));
  // Os dois em sequência: o índice fica igual ao buffer.
  const once = stageHunk(head, buffer, deleted);
  assert.equal(stageHunk(once, buffer, hunks(head, buffer)[1]), buffer);
});

check('Tirar do preparo: o bloco volta ao HEAD e o outro preparado fica', () => {
  const head = lines('a', '1', '2', 'b', 'c', 'd', 'e', 'f', 'g', 'h');
  const index = lines('a', '1', '2', 'B', 'c', 'd', 'e', 'f', 'G', 'h');
  // O buffer tem, além do preparado, uma linha nova antes dos dois blocos (desloca as posições do buffer em relação ao índice).
  const buffer = lines('z', 'a', '1', '2', 'B', 'c', 'd', 'e', 'f', 'G', 'h');
  const hs = hunks(head, buffer);
  assert.equal(hs.length, 3);
  assert.equal(unstageHunk(head, index, buffer, hs[1]), lines('a', '1', '2', 'b', 'c', 'd', 'e', 'f', 'G', 'h'));
  assert.equal(unstageHunk(head, index, buffer, hs[2]), lines('a', '1', '2', 'B', 'c', 'd', 'e', 'f', 'g', 'h'));
  // Preparar e tirar do preparo voltam ao mesmo índice.
  const staged = stageHunk(head, buffer, hs[0]);
  assert.equal(unstageHunk(head, staged, buffer, hs[0]), head);
});

check('Tirar do preparo linhas apagadas', () => {
  const head = lines('a', 'b', 'c', 'd');
  const index = lines('a', 'c', 'd');
  const buffer = index;
  const [deleted] = hunks(head, buffer);
  assert.equal(unstageHunk(head, index, buffer, deleted), head);
});

check('Fim de arquivo sem quebra final: preparar e tirar do preparo', () => {
  const head = 'a\nb\nc';
  const buffer = 'a\nb\nc\nd';
  const [added] = hunks(head, buffer);
  assert.equal(stageHunk(head, buffer, added), buffer);
  assert.equal(unstageHunk(head, buffer, buffer, added), head);
  // Só a quebra final acrescentada, e o contrário.
  assert.equal(stageHunk('a\nb', 'a\nb\n', hunks('a\nb', 'a\nb\n')[0]), 'a\nb\n');
  assert.equal(stageHunk('a\nb\n', 'a\nb', hunks('a\nb\n', 'a\nb')[0]), 'a\nb');
  // A última linha mudada, sem quebra final, com outro bloco antes ainda fora do índice.
  const head2 = 'a\nb\nc\nd';
  const buffer2 = 'A\nb\nc\nD';
  const hs = hunks(head2, buffer2);
  assert.equal(stageHunk(head2, buffer2, hs[1]), 'a\nb\nc\nD');
  assert.equal(unstageHunk(head2, 'a\nb\nc\nD', buffer2, hunks(head2, buffer2)[1]), head2);
});
