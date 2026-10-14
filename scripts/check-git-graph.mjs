// Check do modelo do gráfico de commits (src/git-graph.ts): linear, branch e merge, convergência, merge com 3 pais, raiz,
// primeira página com pai não carregado, reaproveitamento de faixa livre, cor estável e lista vazia.
// Roda sem dependências: `node scripts/check-git-graph.mjs`. Sai com 1 e mostra a falha se alguma regra quebrar.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import { layoutGraph } from '../src/git-graph.ts';
const c = (hash, ...parents) => ({ hash, parents });
// Segmentos de uma linha em forma comparável e independente da ordem: "top 0>1 c2".
const segs = (row) => row.lines.map((l) => `${l.half} ${l.from}>${l.to} c${l.color}`).sort();

check('linear: uma faixa, cor 0 do começo ao fim', () => {
  const rows = layoutGraph([c('a', 'b'), c('b', 'c'), c('c')]);
  assert.equal(rows.length, 3);
  assert.deepEqual(rows.map((r) => r.col), [0, 0, 0]);
  assert.deepEqual(rows.map((r) => r.color), [0, 0, 0]);
  assert.deepEqual(segs(rows[0]), ['bottom 0>0 c0']);
  assert.deepEqual(segs(rows[1]), ['bottom 0>0 c0', 'top 0>0 c0']);
  assert.deepEqual(segs(rows[2]), ['top 0>0 c0']);
  assert.deepEqual(rows.map((r) => r.width), [1, 1, 1]);
});

check('branch e merge: ramo lateral na faixa 1 volta a convergir na faixa 0', () => {
  // M é merge de A e B; B e A têm o mesmo pai R.
  const rows = layoutGraph([c('M', 'A', 'B'), c('B', 'R'), c('A', 'R'), c('R')]);
  assert.deepEqual(rows.map((r) => r.col), [0, 1, 0, 0]);
  assert.deepEqual(segs(rows[0]), ['bottom 0>0 c0', 'bottom 0>1 c1']);
  assert.equal(rows[0].width, 2);
  assert.deepEqual(segs(rows[1]), ['bottom 0>0 c0', 'bottom 1>1 c1', 'top 0>0 c0', 'top 1>1 c1'].sort());
  assert.deepEqual(segs(rows[2]), ['bottom 0>0 c0', 'bottom 1>1 c1', 'top 0>0 c0', 'top 1>1 c1'].sort());
  assert.equal(rows[1].color, 1);
  assert.equal(rows[2].color, 0);
  // Convergência: as duas faixas que esperavam R se juntam no ponto de R.
  assert.deepEqual(segs(rows[3]), ['top 0>0 c0', 'top 1>0 c1']);
  assert.equal(rows[3].width, 2);
});

check('merge: pai extra que já tem faixa converge para ela', () => {
  // X espera Q na faixa 0; M é merge de A (novo) e Q (já esperado).
  const rows = layoutGraph([c('X', 'Q'), c('M', 'A', 'Q')]);
  assert.equal(rows[1].col, 1);
  assert.ok(segs(rows[1]).includes('bottom 1>0 c1'), 'merge desce até a faixa de Q');
  assert.ok(segs(rows[1]).includes('bottom 1>1 c1'), 'primeiro pai segue na faixa do commit');
});

check('dois filhos do mesmo pai: as duas faixas convergem no pai com top {i, col}', () => {
  const rows = layoutGraph([c('A', 'P'), c('B', 'P'), c('P')]);
  assert.equal(rows[1].col, 1);
  assert.equal(rows[1].color, 1);
  assert.deepEqual(segs(rows[2]), ['top 0>0 c0', 'top 1>0 c1']);
  assert.equal(rows[2].col, 0);
  assert.equal(rows[2].width, 2);
});

check('merge com 3 pais: três faixas com cores novas, todas convergem no pai comum', () => {
  const rows = layoutGraph([c('M', 'A', 'B', 'C'), c('A', 'R'), c('B', 'R'), c('C', 'R'), c('R')]);
  assert.deepEqual(segs(rows[0]), ['bottom 0>0 c0', 'bottom 0>1 c1', 'bottom 0>2 c2']);
  assert.equal(rows[0].width, 3);
  assert.deepEqual(segs(rows[4]), ['top 0>0 c0', 'top 1>0 c1', 'top 2>0 c2']);
  assert.equal(rows[4].width, 3);
});

check('commit raiz: sem bottom próprio e cortando a faixa', () => {
  const rows = layoutGraph([c('r')]);
  assert.deepEqual(rows, [{ col: 0, color: 0, lines: [], width: 1 }]);
  const chain = layoutGraph([c('a', 'r'), c('r')]);
  assert.deepEqual(segs(chain[1]), ['top 0>0 c0']);
});

check('primeira página: pai não carregado segue até a borda de baixo', () => {
  const rows = layoutGraph([c('a', 'Z')]);
  assert.deepEqual(segs(rows[0]), ['bottom 0>0 c0']);
  assert.equal(rows[0].width, 1);
});

check('faixa livre é reaproveitada pela esquerda e cores novas continuam sequenciais', () => {
  // a e b esperam c e d em faixas 0 e 1; c fecha a faixa 0; e entra na faixa 0 de novo, com cor nova.
  const rows = layoutGraph([c('a', 'c'), c('b', 'd'), c('c'), c('e', 'f')]);
  assert.deepEqual(rows.map((r) => r.col), [0, 1, 0, 0]);
  assert.deepEqual(rows.map((r) => r.color), [0, 1, 0, 2]);
  assert.deepEqual(segs(rows[3]), ['bottom 0>0 c2', 'bottom 1>1 c1', 'top 1>1 c1'].sort());
  assert.equal(rows[3].width, 2);
});

check('faixas vazias no fim são cortadas: a linha seguinte volta a ter largura 1', () => {
  const rows = layoutGraph([c('a', 'b'), c('b'), c('z')]);
  assert.equal(rows[2].col, 0);
  assert.equal(rows[2].width, 1);
});

check('cor estável: cada faixa mantém a cor de nascimento enquanto existir', () => {
  const rows = layoutGraph([c('M', 'A', 'B'), c('B', 'R'), c('A', 'R'), c('R')]);
  // A faixa 1 nasceu com cor 1 no merge e continua com cor 1 até convergir.
  assert.equal(rows[1].color, 1);
  assert.ok(rows[2].lines.some((l) => l.from === 1 && l.half === 'top' && l.color === 1));
  // A faixa principal mantém cor 0 do início ao fim.
  assert.equal(rows[0].color, 0);
  assert.equal(rows[2].color, 0);
  assert.equal(rows[3].color, 0);
});

check('lista vazia: nenhuma linha', () => {
  assert.deepEqual(layoutGraph([]), []);
});
