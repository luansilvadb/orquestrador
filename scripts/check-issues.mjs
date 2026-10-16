// Check da parte pura das Issues (src/issue-text.ts): o texto que vai ao orquestrador, datas relativas,
// labels mais usadas, busca, filtro e ordenação da faixa, as fechadas recentes, qual issue o detalhe mostra, as seções e o
// teclado da faixa, a leitura do estado salvo no localStorage, a resposta de uma criação, o espaçamento e o motivo das que falharam, as regras da issue em andamento e da próxima e o
// rascunho do orquestrador (o que foi colado e não enviado). Roda sem dependências: `node scripts/check-issues.mjs`.
// Sai com 1 e mostra a falha se alguma regra quebrar.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import {
  commandIn, detailIssue, draftAdd, draftAsk, draftJoined, draftOnTurn, draftPaste, draftText, draftWaits, failedReason, failPending, filterLabels,
  hasLabels, issueList, issuePrompt, markNext, markTurned, matchesQuery, passesFilter, pendingRetrying, pruneTracked, readSections, readStoredIssues,
  recentClosed, relativeTime, retryDue, rovingStep, SECTIONS_DEFAULT, settleStored, sortIssues, splitCode, startTracked, storedEmpty, topLabels,
} from '../src/issue-text.ts';

const issue = (number, body, labels = []) => ({
  number, title: `Título ${number}`, body, state: 'open', labels: labels.map((name) => ({ name, color: 'ededed' })), author: 'luansilvadb',
  comments: 0, createdAt: '2026-10-01T00:00:00Z', updatedAt: '2026-10-01T00:00:00Z', closedAt: null,
  url: `https://github.com/luansilvadb/orquestrador/issues/${number}`,
});
const label = (name) => ({ name, color: 'ededed', description: '' });

check('issuePrompt: número, título, URL, linha em branco e a descrição', () => {
  assert.equal(
    issuePrompt(issue(7, 'Primeira linha\n\n- item')),
    'Issue #7: Título 7\nhttps://github.com/luansilvadb/orquestrador/issues/7\n\nPrimeira linha\n\n- item',
  );
});

check('issuePrompt: sem descrição (ou só espaço) não tem a linha em branco', () => {
  const head = 'Issue #3: Título 3\nhttps://github.com/luansilvadb/orquestrador/issues/3';
  assert.equal(issuePrompt(issue(3, '')), head);
  assert.equal(issuePrompt(issue(3, ' \r\n\n\t')), head);
});

check('issuePrompt: \\r\\n vira \\n e o espaço do fim sai', () => {
  assert.equal(
    issuePrompt(issue(1, 'a\r\nb\r\n\r\nc  \r\n\r\n')),
    'Issue #1: Título 1\nhttps://github.com/luansilvadb/orquestrador/issues/1\n\na\nb\n\nc',
  );
});

check('relativeTime: as faixas em português', () => {
  const now = Date.parse('2026-10-16T12:00:00Z');
  const ago = (ms) => new Date(now - ms).toISOString();
  const MIN = 60_000, HOUR = 60 * MIN, DAY = 24 * HOUR;
  assert.equal(relativeTime(ago(0), now), 'agora');
  assert.equal(relativeTime(ago(59_000), now), 'agora');
  assert.equal(relativeTime(ago(-5 * MIN), now), 'agora', 'data futura');
  assert.equal(relativeTime('não é data', now), 'agora', 'data inválida');
  assert.equal(relativeTime(ago(MIN), now), 'há 1 min');
  assert.equal(relativeTime(ago(5 * MIN + 30_000), now), 'há 5 min');
  assert.equal(relativeTime(ago(59 * MIN), now), 'há 59 min');
  assert.equal(relativeTime(ago(HOUR), now), 'há 1 hora');
  assert.equal(relativeTime(ago(2 * HOUR + 10 * MIN), now), 'há 2 horas');
  assert.equal(relativeTime(ago(23 * HOUR), now), 'há 23 horas');
  assert.equal(relativeTime(ago(DAY), now), 'há 1 dia');
  assert.equal(relativeTime(ago(3 * DAY), now), 'há 3 dias');
  assert.equal(relativeTime(ago(29 * DAY), now), 'há 29 dias');
  assert.equal(relativeTime(ago(30 * DAY), now), 'há 1 mês');
  assert.equal(relativeTime(ago(65 * DAY), now), 'há 2 meses');
  assert.equal(relativeTime(ago(364 * DAY), now), 'há 12 meses');
  assert.equal(relativeTime(ago(365 * DAY), now), 'há 1 ano');
  assert.equal(relativeTime(ago(800 * DAY), now), 'há 2 anos');
});

check('matchesQuery: título sem diferenciar maiúsculas e acentos, ou o número exato com ou sem #', () => {
  assert.equal(matchesQuery('Configuração do Orquestrador', 12, ''), true, 'busca vazia passa tudo');
  assert.equal(matchesQuery('Configuração do Orquestrador', 12, '  '), true);
  assert.equal(matchesQuery('Configuração do Orquestrador', 12, 'CONFIGURACAO'), true);
  assert.equal(matchesQuery('Configuracao do orquestrador', 12, 'configuração'), true);
  assert.equal(matchesQuery('Configuração do Orquestrador', 12, 'git'), false);
  assert.equal(matchesQuery('Configuração', 12, '12'), true, 'número sem #');
  assert.equal(matchesQuery('Configuração', 12, '#12'), true, 'número com #');
  assert.equal(matchesQuery('Configuração', 12, '#1'), false, 'número exato, não prefixo');
  assert.equal(matchesQuery('Versão 12', 7, '12'), true, 'número que aparece no título também passa');
  assert.equal(matchesQuery('Ideia nova', null, '#3'), false, 'criação sem número só passa pelo título');
});

check('hasLabels: passa com todas as marcadas; sem marcadas passa tudo', () => {
  assert.equal(hasLabels(['bug', 'ui'], []), true);
  assert.equal(hasLabels(['bug', 'ui'], ['bug']), true);
  assert.equal(hasLabels(['bug', 'ui'], ['bug', 'ui']), true);
  assert.equal(hasLabels(['bug'], ['bug', 'ui']), false);
});

check('sortIssues: mais novas, mais antigas e atualizadas recentemente, sem mudar a lista de entrada', () => {
  const at = (number, created, updated) => ({ ...issue(number, ''), createdAt: created, updatedAt: updated });
  const list = [at(1, '2026-01-01T00:00:00Z', '2026-09-01T00:00:00Z'), at(2, '2026-03-01T00:00:00Z', '2026-03-01T00:00:00Z'), at(3, '2026-02-01T00:00:00Z', '2026-10-01T00:00:00Z')];
  assert.deepEqual(sortIssues(list, 'newest').map((i) => i.number), [2, 3, 1]);
  assert.deepEqual(sortIssues(list, 'oldest').map((i) => i.number), [1, 3, 2]);
  assert.deepEqual(sortIssues(list, 'updated').map((i) => i.number), [3, 1, 2]);
  assert.deepEqual(list.map((i) => i.number), [1, 2, 3], 'a entrada não muda');
});

check('topLabels: mais usadas primeiro, empate pela ordem do repositório', () => {
  const repo = ['bug', 'documentation', 'enhancement', 'question'].map(label);
  const issues = [issue(1, '', ['question', 'enhancement']), issue(2, '', ['enhancement', 'documentation']), issue(3, '', ['question', 'fora-do-repo'])];
  // question 2, enhancement 2 (empate: enhancement vem antes no repositório), documentation 1.
  assert.deepEqual(topLabels(issues, repo, 3).map((l) => l.name), ['enhancement', 'question', 'documentation']);
});

check('topLabels: completa com as não usadas, na ordem do repositório', () => {
  const repo = ['bug', 'documentation', 'enhancement', 'question', 'wontfix'].map(label);
  assert.deepEqual(topLabels([issue(1, '', ['question'])], repo, 3).map((l) => l.name), ['question', 'bug', 'documentation']);
  assert.deepEqual(topLabels([], repo, 2).map((l) => l.name), ['bug', 'documentation']);
  assert.equal(topLabels([], repo, 10).length, 5, 'n maior que o total devolve todas');
  assert.deepEqual(topLabels([], repo, 0), []);
});

check('passesFilter: a busca da página (título sem acento e maiúsculas, ou #número) e todas as labels marcadas', () => {
  const i = { ...issue(12, '', ['bug', 'ui']), title: 'Configuração do Orquestrador' };
  assert.equal(passesFilter(i, '', []), true, 'filtro vazio passa');
  assert.equal(passesFilter(i, 'CONFIGURACAO', []), true);
  assert.equal(passesFilter(i, '#12', []), true);
  assert.equal(passesFilter(i, '#1', []), false);
  assert.equal(passesFilter(i, '', ['bug']), true);
  assert.equal(passesFilter(i, '', ['bug', 'ui']), true);
  assert.equal(passesFilter(i, '', ['bug', 'docs']), false, 'precisa de todas as marcadas');
  assert.equal(passesFilter(i, 'git', ['bug']), false, 'texto e labels valem juntos');
});

check('filterLabels: só as labels do repositório em uso nas abertas, na ordem do repositório; as marcadas ficam', () => {
  const repo = ['bug', 'documentation', 'enhancement', 'question', 'wontfix'].map(label);
  const closed = { ...issue(3, '', ['wontfix']), state: 'closed' };
  const issues = [issue(1, '', ['question', 'bug']), issue(2, '', ['question', 'fora-do-repo']), closed];
  assert.deepEqual(filterLabels(issues, repo, []).map((l) => l.name), ['bug', 'question'], 'fechada e label fora do repositório não contam');
  assert.deepEqual(filterLabels(issues, repo, ['wontfix']).map((l) => l.name), ['bug', 'question', 'wontfix'], 'a marcada sem uso fica, para desmarcar');
  assert.deepEqual(filterLabels([], repo, []), []);
});

check('recentClosed: as n fechadas mais recentes pelo closedAt, sem mudar a entrada', () => {
  const at = (number, closedAt, updatedAt = '2026-01-01T00:00:00Z') => ({ ...issue(number, ''), state: 'closed', closedAt, updatedAt });
  const list = [
    at(1, '2026-03-01T00:00:00Z'), issue(2, ''), at(3, '2026-09-01T00:00:00Z'), at(4, '2026-05-01T00:00:00Z'),
    at(5, null, '2026-10-01T00:00:00Z'), at(6, '2026-01-01T00:00:00Z'), at(7, '2026-02-01T00:00:00Z'), at(8, '2026-09-01T00:00:00Z'),
  ];
  assert.deepEqual(recentClosed(list, 5).map((i) => i.number), [5, 8, 3, 4, 1], 'sem closedAt vale o updatedAt; empate pelo número maior; aberta não entra');
  assert.deepEqual(recentClosed(list, 2).map((i) => i.number), [5, 8]);
  assert.deepEqual(recentClosed(list, 0), []);
  assert.deepEqual(recentClosed([issue(1, '')], 5), [], 'sem fechadas');
  assert.deepEqual(list.map((i) => i.number), [1, 2, 3, 4, 5, 6, 7, 8], 'a entrada não muda');
});

check('detailIssue: a escolhida (aberta ou fechada); sem escolha, a em andamento, a próxima e a primeira aberta, só se abertas', () => {
  const at = (number, createdAt, state = 'open') => ({ ...issue(number, ''), createdAt, state });
  const list = [at(1, '2026-10-01T00:00:00Z'), at(2, '2026-10-03T00:00:00Z'), at(3, '2026-10-02T00:00:00Z'), at(4, '2026-10-04T00:00:00Z', 'closed')];
  const n = (x) => x?.number ?? null;
  assert.equal(n(detailIssue(list, 3, 2, 1)), 3, 'a escolhida vence a em andamento e a próxima');
  assert.equal(n(detailIssue(list, 4, 2, 1)), 4, 'a escolhida pode ser uma fechada');
  assert.equal(n(detailIssue(list, null, 3, 1)), 3, 'sem escolha: a em andamento');
  assert.equal(n(detailIssue(list, null, null, 1)), 1, 'sem em andamento: a próxima');
  assert.equal(n(detailIssue(list, null, null, null)), 2, 'sem nenhuma das duas: a primeira aberta (a mais nova)');
  assert.equal(n(detailIssue(list, 99, 3, null)), 3, 'a escolhida sumiu do GitHub: cai na regra sem escolha');
  assert.equal(n(detailIssue(list, null, 4, 1)), 1, 'a em andamento fechada não vale: vai para a próxima');
  assert.equal(n(detailIssue(list, null, null, 4)), 2, 'a próxima fechada não vale: vai para a primeira aberta');
  assert.equal(detailIssue([at(4, '2026-10-04T00:00:00Z', 'closed')], null, null, null), null, 'nenhuma aberta e nada escolhido');
  assert.equal(detailIssue([], null, null, null), null);
});

check('readSections: Na fila e Em andamento abertas e Fechadas recentes recolhida por padrão; o que não é booleano fica com o padrão', () => {
  const def = { active: true, queue: true, closed: false };
  for (const text of [null, '', '{', 'null', '42', '[]', '{}', '"x"']) assert.deepEqual(readSections(text), def, String(text));
  assert.deepEqual(readSections('{"active":false,"queue":false,"closed":true}'), { active: false, queue: false, closed: true });
  assert.deepEqual(readSections('{"closed":true,"queue":"no","extra":1}'), { active: true, queue: true, closed: true });
  assert.deepEqual(SECTIONS_DEFAULT, def);
});

check('rovingStep: ↑/↓ andam uma sem dar a volta, Home e End vão às pontas, outra tecla não faz nada', () => {
  assert.equal(rovingStep(0, 5, 'ArrowDown'), 1);
  assert.equal(rovingStep(4, 5, 'ArrowDown'), 4, 'para na última');
  assert.equal(rovingStep(2, 5, 'ArrowUp'), 1);
  assert.equal(rovingStep(0, 5, 'ArrowUp'), 0, 'para na primeira');
  assert.equal(rovingStep(3, 5, 'Home'), 0);
  assert.equal(rovingStep(1, 5, 'End'), 4);
  assert.equal(rovingStep(1, 5, 'Enter'), null);
  assert.equal(rovingStep(0, 0, 'ArrowDown'), null, 'lista vazia');
});

const pending = (localId, extra = {}) => ({ localId, title: 'Ideia', body: 'texto', labels: ['bug'], createdAt: 1_700_000_000_000, status: 'failed', error: 'Sem conexão com o GitHub.', ...extra });
const EMPTY = { v: 1, pending: [], active: null, next: null };

check('readStoredIssues: texto ilegível ou formato errado volta vazio, sem lançar', () => {
  for (const text of [null, '', '{', 'null', '42', '"texto"', '[]', '{}', '{"v":2,"pending":[]}', '{"v":"1","pending":[]}', '{"v":1}', '{"v":1,"pending":{}}']) {
    assert.deepEqual(readStoredIssues(text), EMPTY, String(text));
  }
});

check('readStoredIssues: a falha guardada volta igual, com a contagem e a hora', () => {
  const saved = {
    v: 1, pending: [pending('a', { failures: 2, failedAt: 1_700_000_100_000 }), pending('b', { labels: [], body: '', failures: 1, failedAt: 5 })], active: null, next: null,
  };
  assert.deepEqual(readStoredIssues(JSON.stringify(saved)), saved);
});

check('readStoredIssues: falha salva sem contagem (dado antigo) vale uma falha já vencida; contagem e hora inválidas também', () => {
  const read = (extra) => readStoredIssues(JSON.stringify({ v: 1, pending: [pending('a', extra)] }), new Set(), 1_800_000_000_000).pending[0];
  const old = read({});
  assert.equal(old.failures, 1);
  assert.equal(old.failedAt, 0);
  assert.equal(retryDue(old, 1_800_000_000_000), true, 'sai na primeira atualização boa');
  for (const failures of [0, -1, 1.5, '2', null]) assert.equal(read({ failures, failedAt: 9 }).failures, 1, JSON.stringify(failures));
  for (const failedAt of ['ontem', null, true]) assert.equal(read({ failures: 2, failedAt }).failedAt, 0, JSON.stringify(failedAt));
  assert.equal(read({ failures: 3, failedAt: 9 }).failures, 3);
});

check('readStoredIssues: envio interrompido conta como mais uma falha, na hora da leitura', () => {
  const now = 1_800_000_000_000;
  const read = (extra) => readStoredIssues(JSON.stringify({ v: 1, pending: [pending('a', { status: 'sending', ...extra })] }), new Set(), now).pending[0];
  const first = read({ error: undefined });
  assert.deepEqual([first.status, first.failures, first.failedAt], ['failed', 1, now], 'a primeira tentativa (sem falha antes)');
  const again = read({ failures: 2, failedAt: 5 });
  assert.deepEqual([again.status, again.failures, again.failedAt], ['failed', 3, now], 'uma nova tentativa depois de duas falhas');
  assert.match(again.error, /interrompido/);
});

check('readStoredIssues: a nova tentativa ainda a caminho mantém o erro, a contagem e a hora da última falha', () => {
  const text = JSON.stringify({ v: 1, pending: [pending('a', { status: 'sending', failures: 2, failedAt: 5 })] });
  const [p] = readStoredIssues(text, new Set(['a'])).pending;
  assert.deepEqual(p, pending('a', { status: 'sending', failures: 2, failedAt: 5 }));
  assert.equal(pendingRetrying(p), true);
});

check('settleStored e failPending: a falha soma a contagem e guarda a hora', () => {
  const text = JSON.stringify({ v: 1, pending: [pending('a', { status: 'sending', failures: 1, failedAt: 5 }), pending('b', { status: 'sending', error: undefined })] });
  const [a] = settleStored(text, 'a', 'O GitHub respondeu 502.', 1_000).pending;
  assert.deepEqual([a.status, a.error, a.failures, a.failedAt], ['failed', 'O GitHub respondeu 502.', 2, 1_000]);
  const b = settleStored(text, 'b', 'Sem conexão com o GitHub.', 2_000).pending[1];
  assert.deepEqual([b.status, b.failures, b.failedAt], ['failed', 1, 2_000], 'a primeira falha');
  assert.deepEqual(failPending(pending('c', { status: 'sending', error: undefined }), 'x', 7), pending('c', { error: 'x', failures: 1, failedAt: 7 }));
});

check('retryDue: a próxima tentativa automática depois de 15 s, 1 min e 5 min; da 4ª falha em diante, nenhuma', () => {
  const failed = (failures) => pending('a', { failures, failedAt: 1_000 });
  for (const [failures, wait] of [[1, 15_000], [2, 60_000], [3, 300_000]]) {
    assert.equal(retryDue(failed(failures), 1_000 + wait - 1), false, `${failures}: antes da espera`);
    assert.equal(retryDue(failed(failures), 1_000 + wait), true, `${failures}: na espera`);
  }
  for (const failures of [4, 5, 12]) assert.equal(retryDue(failed(failures), 1e15), false, `${failures}: só o Tentar de novo`);
  assert.equal(retryDue(pending('a', { status: 'sending', failures: 1, failedAt: 0 }), 1e15), false, 'a caminho');
  assert.equal(retryDue(pending('a', { status: 'waiting', error: undefined }), 1e15), false, 'a guardada vai por outra regra');
});

check('failedReason: o motivo diz o que vai acontecer', () => {
  const failed = (failures, error = 'Sem conexão com o GitHub.') => pending('a', { failures, failedAt: 0, error });
  assert.equal(failedReason(failed(1)), 'Falhou: Sem conexão com o GitHub. Nova tentativa automática em 15 s.');
  assert.equal(failedReason(failed(2)), 'Falhou: Sem conexão com o GitHub. Nova tentativa automática em 1 min.');
  assert.equal(failedReason(failed(3, 'O GitHub respondeu 502')), 'Falhou: O GitHub respondeu 502. Nova tentativa automática em 5 min.', 'sem ponto, ganha um');
  assert.equal(failedReason(failed(4)), 'Falhou 4 vezes: Sem conexão com o GitHub. Sem novas tentativas automáticas.');
  assert.equal(failedReason(failed(6)), 'Falhou 6 vezes: Sem conexão com o GitHub. Sem novas tentativas automáticas.');
  assert.equal(failedReason(failed(1, '')), 'Falhou: erro desconhecido. Nova tentativa automática em 15 s.');
  const retrying = pending('a', { status: 'sending', failures: 2, failedAt: 0 });
  assert.equal(failedReason(retrying), 'Falhou: Sem conexão com o GitHub. Tentando de novo…');
  assert.equal(failedReason(retrying, true), 'Falhou: Sem conexão com o GitHub. Nova tentativa automática em 1 min.', 'o texto da falha, para reservar o espaço');
});

check('pendingRetrying: só a que já falhou e está a caminho de novo', () => {
  assert.equal(pendingRetrying(pending('a', { status: 'sending', failures: 1, failedAt: 0 })), true);
  assert.equal(pendingRetrying(pending('a', { status: 'sending', error: undefined })), false, 'a primeira tentativa (a otimista da captura)');
  assert.equal(pendingRetrying(pending('a', { failures: 1, failedAt: 0 })), false, 'falhou e espera');
});

check('readStoredIssues: envio interrompido (sending) volta como failed, com aviso', () => {
  const { pending: [p] } = readStoredIssues(JSON.stringify({ v: 1, pending: [pending('a', { status: 'sending', error: undefined })] }));
  assert.equal(p.status, 'failed');
  assert.match(p.error, /interrompido/);
});

check('readStoredIssues: sending com o pedido ainda a caminho nesta execução continua sending', () => {
  const text = JSON.stringify({ v: 1, pending: [pending('a', { status: 'sending', error: undefined }), pending('b', { status: 'sending', error: undefined })] });
  const read = readStoredIssues(text, new Set(['a']));
  assert.deepEqual(read.pending.map((p) => [p.localId, p.status]), [['a', 'sending'], ['b', 'failed']]);
  assert.equal('error' in read.pending[0], false);
});

check('settleStored: a resposta tira a criação que deu certo e marca a que falhou, sem mexer nas outras', () => {
  const text = JSON.stringify({ v: 1, pending: [pending('ok', { status: 'sending', error: undefined }), pending('outra', { status: 'sending', error: undefined }), pending('velha')] });
  const done = settleStored(text, 'ok');
  assert.deepEqual(done.pending.map((p) => [p.localId, p.status]), [['outra', 'sending'], ['velha', 'failed']], 'as outras a caminho continuam sending');
  const failed = settleStored(text, 'ok', 'Sem conexão com o GitHub.');
  assert.deepEqual(failed.pending.map((p) => [p.localId, p.status, p.error]), [
    ['ok', 'failed', 'Sem conexão com o GitHub.'], ['outra', 'sending', undefined], ['velha', 'failed', 'Sem conexão com o GitHub.'],
  ]);
  assert.deepEqual(settleStored(text, 'descartada').pending.length, 3, 'descartada no meio do envio não volta nem mexe no resto');
  assert.deepEqual(settleStored(null, 'ok'), EMPTY, 'sem nada salvo (outro projeto limpo) continua vazio');
});

check('Troca de projeto no meio do envio: a resposta grava no projeto de origem e a criação não volta como falha', () => {
  // Projeto A: criação a caminho, salva como sending. O usuário vai para B e volta para A antes da resposta.
  const savedA = JSON.stringify({ v: 1, pending: [pending('x', { status: 'sending', error: undefined })] });
  const reopened = readStoredIssues(savedA, new Set(['x']));
  assert.equal(reopened.pending[0].status, 'sending', 'a sessão nova de A não a trata como interrompida (nem a reenvia)');
  // A resposta chega (com B aberto ou com A de novo): o localStorage de A perde a pendência.
  assert.deepEqual(settleStored(savedA, 'x'), EMPTY);
  // Depois, sem nada a caminho, o que sobrou de A volta como falha só se o app fechou antes da resposta.
  assert.equal(readStoredIssues(savedA).pending[0].status, 'failed');
});

check('readStoredIssues: criação inválida sai sozinha, as válidas ficam', () => {
  const list = [
    pending('ok-1'),
    null, 'texto', 3,
    pending(''),
    pending('sem-titulo', { title: 5 }),
    pending('labels-erradas', { labels: ['bug', 1] }),
    pending('data-errada', { createdAt: 'ontem' }),
    pending('status-errado', { status: 'done' }),
    pending('erro-errado', { error: 404 }),
    pending('ok-1', { title: 'repetida' }),
    pending('ok-2', { error: undefined }),
  ];
  const read = readStoredIssues(JSON.stringify({ v: 1, pending: list, active: 12, next: 13 }));
  assert.deepEqual(read.pending.map((p) => p.localId), ['ok-1', 'ok-2']);
  assert.equal(read.pending[0].title, 'Ideia');
  assert.equal('error' in read.pending[1], false, 'failed sem mensagem fica sem `error`');
});

check('readStoredIssues: a leitura antiga (sem active e next) continua valendo, com os dois null', () => {
  const read = readStoredIssues(JSON.stringify({ v: 1, pending: [pending('a')] }));
  assert.deepEqual(read.pending.map((p) => p.localId), ['a']);
  assert.equal(read.active, null);
  assert.equal(read.next, null);
});

check('readStoredIssues: em andamento e próxima voltam iguais', () => {
  const saved = { v: 1, pending: [], active: { number: 12, since: 1_700_000_000_000, turned: true }, next: 13 };
  assert.deepEqual(readStoredIssues(JSON.stringify(saved)), saved);
  assert.deepEqual(readStoredIssues(JSON.stringify({ ...saved, active: null })), { ...saved, active: null }, 'só a próxima');
  assert.deepEqual(readStoredIssues(JSON.stringify({ ...saved, next: null })), { ...saved, next: null }, 'só a em andamento');
});

check('readStoredIssues: em andamento ou próxima inválidas valem null, sem derrubar o resto', () => {
  const since = 1_700_000_000_000;
  for (const active of [12, '12', [], { number: 12 }, { since }, { number: '12', since }, { number: 0, since }, { number: -3, since },
    { number: 1.5, since }, { number: 12, since: '2026' }, { number: 12, since: null }]) {
    const read = readStoredIssues(JSON.stringify({ v: 1, pending: [pending('a')], active, next: 13 }));
    assert.equal(read.active, null, JSON.stringify(active));
    assert.equal(read.next, 13, 'a próxima válida fica');
    assert.equal(read.pending.length, 1);
  }
  for (const next of ['13', 0, -1, 2.5, {}, [13], true]) {
    assert.equal(readStoredIssues(JSON.stringify({ v: 1, pending: [], active: { number: 12, since }, next })).next, null, JSON.stringify(next));
  }
  const same = readStoredIssues(JSON.stringify({ v: 1, pending: [], active: { number: 12, since }, next: 12 }));
  assert.deepEqual([same.active, same.next], [{ number: 12, since, turned: false }, null], 'a próxima igual à em andamento não vale');
});

check('settleStored: a resposta de uma criação não mexe na em andamento nem na próxima', () => {
  const text = JSON.stringify({ v: 1, pending: [pending('ok', { status: 'sending', error: undefined })], active: { number: 4, since: 10, turned: true }, next: 5 });
  const done = settleStored(text, 'ok');
  assert.deepEqual(done, { v: 1, pending: [], active: { number: 4, since: 10, turned: true }, next: 5 });
  assert.equal(storedEmpty(done), false, 'com em andamento ou próxima a chave fica no localStorage');
  assert.equal(storedEmpty({ ...done, active: null }), false);
  assert.equal(storedEmpty({ ...done, active: null, next: null }), true);
});

check('startTracked: vira a em andamento; o since só recomeça ao trocar de issue; deixa de ser a próxima', () => {
  const none = { active: null, next: null };
  assert.deepEqual(startTracked(none, 7, 100), { active: { number: 7, since: 100, turned: false }, next: null });
  const running = { active: { number: 7, since: 100, turned: true }, next: 8 };
  assert.deepEqual(startTracked(running, 7, 500), running, 'a mesma issue mantém o since');
  assert.deepEqual(startTracked(running, 9, 500), { active: { number: 9, since: 500, turned: false }, next: 8 }, 'a anterior só deixa de ser a em andamento');
  assert.deepEqual(startTracked(running, 8, 500), { active: { number: 8, since: 500, turned: false }, next: null }, 'a próxima que começa deixa de ser a próxima');
});

check('markNext: só uma próxima, a nova substitui; a em andamento não pode ser a próxima; null tira a marca', () => {
  const t = { active: { number: 7, since: 100 }, next: 8 };
  assert.deepEqual(markNext(t, 9), { active: t.active, next: 9 });
  assert.equal(markNext(t, 7), t, 'marcar a em andamento não vale');
  assert.deepEqual(markNext(t, null), { active: t.active, next: null });
  assert.deepEqual(markNext({ active: null, next: null }, 3), { active: null, next: 3 });
});

check('pruneTracked: zera a em andamento e a próxima fechadas ou fora da lista; abertas ficam', () => {
  const at = (number, state) => ({ ...issue(number, ''), state });
  const t = { active: { number: 1, since: 100 }, next: 2 };
  assert.equal(pruneTracked(t, [at(1, 'open'), at(2, 'open')]), t, 'as duas abertas: o mesmo objeto, sem mudança');
  assert.deepEqual(pruneTracked(t, [at(1, 'closed'), at(2, 'open')]), { active: null, next: 2 }, 'em andamento fechada (Closes #1 ou pelo site)');
  assert.deepEqual(pruneTracked(t, [at(1, 'open'), at(2, 'closed')]), { active: t.active, next: null }, 'próxima fechada');
  assert.deepEqual(pruneTracked(t, [at(3, 'open')]), { active: null, next: null }, 'nenhuma das duas na lista');
  assert.deepEqual(pruneTracked(t, []), { active: null, next: null });
  const none = { active: null, next: null };
  assert.equal(pruneTracked(none, [at(1, 'open')]), none);
});

check('markTurned: a rodada marca a em andamento; sem em andamento ou já marcada devolve o mesmo objeto', () => {
  const sent = { active: { number: 7, since: 100, turned: false }, next: 8 };
  assert.deepEqual(markTurned(sent), { active: { number: 7, since: 100, turned: true }, next: 8 });
  assert.equal(sent.active.turned, false, 'não muda o de entrada');
  const done = markTurned(sent);
  assert.equal(markTurned(done), done);
  const none = { active: null, next: 8 };
  assert.equal(markTurned(none), none);
});

check('Rodada desde o Iniciar: colada → trabalhando; um Iniciar de outra issue recomeça; o mesmo Iniciar mantém', () => {
  let t = startTracked({ active: null, next: null }, 7, 100);
  assert.equal(t.active.turned, false, 'recém-colada: aguardando envio');
  t = markTurned(t);
  assert.equal(t.active.turned, true, 'rodada começou');
  assert.equal(startTracked(t, 7, 900).active.turned, true, 'o mesmo Iniciar não zera');
  assert.equal(startTracked(t, 9, 900).active.turned, false, 'outra issue começa colada de novo');
});

check('readStoredIssues: turned ausente (dado antigo) ou inválido vale false; só true vale true', () => {
  const since = 1_700_000_000_000;
  const turned = (v) => readStoredIssues(JSON.stringify({ v: 1, pending: [], active: { number: 12, since, ...(v === undefined ? {} : { turned: v }) }, next: null })).active.turned;
  assert.equal(turned(undefined), false);
  for (const v of ['true', 1, null, [], {}]) assert.equal(turned(v), false, JSON.stringify(v));
  assert.equal(turned(false), false);
  assert.equal(turned(true), true);
});

check('readStoredIssues: criação guardada sem o GitHub (waiting) continua waiting; sem erro inventado', () => {
  const read = readStoredIssues(JSON.stringify({ v: 1, pending: [pending('w', { status: 'waiting', error: undefined })], active: null, next: null }));
  assert.equal(read.pending.length, 1);
  assert.equal(read.pending[0].status, 'waiting');
  assert.equal('error' in read.pending[0], false);
  assert.equal(storedEmpty(read), false, 'ainda há o que guardar');
});

check('rascunho: colar com ele vazio cola; a mesma de novo já está lá; outra pergunta', () => {
  assert.equal(draftPaste([], 11), 'paste');
  assert.equal(draftPaste([11], 11), 'already');
  assert.equal(draftPaste([11, 13], 11), 'already', 'vale para qualquer uma do rascunho, não só a em andamento');
  assert.equal(draftPaste([11], 13), 'ask');
});

check('rascunho: colar (ou Já apaguei) deixa só ela; Colar junto a põe depois das que estavam', () => {
  assert.deepEqual(draftAdd([], 11, false), [11]);
  assert.deepEqual(draftAdd([11, 12], 13, false), [13], 'Já apaguei tira as anteriores');
  assert.deepEqual(draftAdd([11], 13, true), [11, 13]);
  assert.deepEqual(draftAdd([11, 13], 12, true), [11, 13, 12], 'na ordem da colagem');
  const draft = [11, 13];
  assert.equal(draftAdd(draft, 13, true), draft, 'já lá: o mesmo rascunho');
});

check('rascunho: Colar junto vai depois de uma linha em branco; colar sozinho, o texto como é', () => {
  assert.equal(draftText('Issue #13: x', false), 'Issue #13: x');
  assert.equal(draftText('Issue #13: x', true), '\n\nIssue #13: x');
});

check('rascunho: o envio esvazia (com texto, sem a issue, ou de CLI antigo); a leitura inicial (null) não', () => {
  const draft = [11, 13];
  assert.deepEqual(draftOnTurn(draft, 'Issue #11: Título 11\nhttps://github.com/x/y/issues/11'), []);
  assert.deepEqual(draftOnTurn(draft, 'responda apenas OK'), [], 'o texto da issue pode não ter ido: a linha inteira foi');
  assert.deepEqual(draftOnTurn(draft, ''), []);
  assert.deepEqual(draftOnTurn(draft, undefined), [], 'CLI antigo, sem prompt: conta como envio');
  assert.equal(draftOnTurn(draft, null), draft, 'null não é envio');
  const empty = [];
  assert.equal(draftOnTurn(empty, 'x'), empty, 'vazio continua o mesmo');
});

check('rascunho: as issues numa frase', () => {
  assert.equal(issueList([]), '');
  assert.equal(issueList([11]), 'a #11');
  assert.equal(issueList([11, 12]), 'a #11 e a #12');
  assert.equal(issueList([11, 12, 14]), 'a #11, a #12 e a #14');
});

check('rascunho: a pergunta antes de colar, no singular e no plural', () => {
  assert.deepEqual(draftAsk([11], 13), {
    title: 'A #11 ainda está no rascunho',
    body: 'A #11 foi colada no rascunho do orquestrador e não foi enviada. Colar a #13 agora junta as duas no mesmo envio. '
      + 'Para trocar uma pela outra, apague a #11 no terminal do orquestrador antes.',
  });
  assert.deepEqual(draftAsk([11, 12], 13), {
    title: 'A #11 e a #12 ainda estão no rascunho',
    body: 'A #11 e a #12 foram coladas no rascunho do orquestrador e não foram enviadas. Colar a #13 agora junta as três no mesmo envio. '
      + 'Para trocar, apague a #11 e a #12 no terminal do orquestrador antes.',
  });
  assert.match(draftAsk([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 11).body, /junta todas no mesmo envio/);
});

check('rascunho: os avisos do Colar junto e da próxima que espera', () => {
  assert.equal(draftJoined([11], 13), '#13 colada junto com a #11; só a #13 fica em andamento.');
  assert.equal(draftJoined([11, 12], 13), '#13 colada junto com a #11 e a #12; só a #13 fica em andamento.');
  assert.equal(draftWaits([13], 9), 'A #9 espera: o rascunho do orquestrador tem a #13, ainda não enviada.');
  assert.equal(draftWaits([11, 13], 9), 'A #9 espera: o rascunho do orquestrador tem a #11 e a #13, ainda não enviadas.');
});

check('splitCode: separa os trechos entre crases do texto corrido', () => {
  assert.deepEqual(splitCode('Instale o gh e rode `gh auth login`.'), [
    { text: 'Instale o gh e rode ', code: false }, { text: 'gh auth login', code: true }, { text: '.', code: false },
  ]);
  assert.deepEqual(splitCode('Rode `a` e `b c`'), [
    { text: 'Rode ', code: false }, { text: 'a', code: true }, { text: ' e ', code: false }, { text: 'b c', code: true },
  ]);
  assert.deepEqual(splitCode('sem código'), [{ text: 'sem código', code: false }]);
  assert.deepEqual(splitCode(''), []);
});

check('splitCode: crase sem par ou vazia fica como texto; não atravessa linha', () => {
  assert.deepEqual(splitCode('uma `solta'), [{ text: 'uma `solta', code: false }]);
  assert.deepEqual(splitCode('vazio ``'), [{ text: 'vazio ``', code: false }]);
  assert.deepEqual(splitCode('`a\nb`'), [{ text: '`a\nb`', code: false }]);
});

check('commandIn: o primeiro comando gh entre crases; origin e texto sem código não são comando', () => {
  assert.equal(commandIn('O GitHub CLI não está logado. Rode `gh auth login` num terminal e tente de novo.'), 'gh auth login');
  assert.equal(commandIn('Instale o gh e rode `gh auth login`.'), 'gh auth login');
  assert.equal(commandIn('O remoto `origin` deste repositório não está no GitHub.'), null);
  assert.equal(commandIn('Sem conexão com o GitHub.'), null);
  assert.equal(commandIn('`ghost` e depois `gh repo view`'), 'gh repo view');
});