// Check da parte pura das Issues (src/issue-text.ts): o texto que vai ao orquestrador, datas relativas, cor do texto das labels,
// labels mais usadas, busca, filtro e ordenação da página, o filtro da faixa, as fechadas recentes, a descrição da prévia, as seções e o
// teclado da faixa, a leitura do estado salvo no localStorage, a resposta de uma criação e as regras da issue em andamento e da próxima. Roda sem dependências: `node scripts/check-issues.mjs`.
// Sai com 1 e mostra a falha se alguma regra quebrar.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import {
  filterLabels, hasLabels, issuePrompt, labelTextColor, markNext, matchesQuery, passesFilter, previewBody, pruneTracked, readSections,
  readStoredIssues, recentClosed, relativeTime, rovingStep, SECTIONS_DEFAULT, settleStored, sortIssues, startTracked, storedEmpty, topLabels,
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

check('labelTextColor: como no site do GitHub (labels padrão), preto no fundo claro e branco no escuro', () => {
  for (const light of ['ffffff', 'e4e669', 'a2eeef', 'cfd3d7', '#fff', 'ededed']) assert.equal(labelTextColor(light), '#000000', light);
  for (const dark of ['d73a4a', '0075ca', '7057ff', '008672', '0e8a16', 'd876e3', '000000', '1f2328', '5319e7', '#123']) {
    assert.equal(labelTextColor(dark), '#ffffff', dark);
  }
  assert.equal(labelTextColor('xyz'), '#000000', 'hex inválido');
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

check('previewBody: texto simples, \\r\\n vira \\n, linhas em branco seguidas viram uma, corte em n linhas com …', () => {
  assert.equal(previewBody('', 8), '');
  assert.equal(previewBody(' \r\n\n\t ', 8), '', 'só espaço é sem descrição');
  assert.equal(previewBody('# Título\r\n\r\n- **item**', 8), '# Título\n\n- **item**', 'o Markdown fica como está');
  assert.equal(previewBody('a\n\n\n\n  \nb', 8), 'a\n\nb');
  assert.equal(previewBody('a  \nb\t', 8), 'a\nb', 'espaço no fim da linha sai');
  const nine = Array.from({ length: 9 }, (_, k) => `linha ${k + 1}`).join('\n');
  assert.equal(previewBody(nine, 8), `${nine.split('\n').slice(0, 8).join('\n')}…`);
  assert.equal(previewBody(nine.split('\n').slice(0, 8).join('\n'), 8), nine.split('\n').slice(0, 8).join('\n'), 'oito linhas cabem sem corte');
  assert.equal(previewBody('a\nb\n\nc', 3), 'a\nb…', 'a linha em branco do corte não fica antes das reticências');
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

check('readStoredIssues: a falha guardada volta igual', () => {
  const saved = { v: 1, pending: [pending('a'), pending('b', { labels: [], body: '' })], active: null, next: null };
  assert.deepEqual(readStoredIssues(JSON.stringify(saved)), saved);
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
  const saved = { v: 1, pending: [], active: { number: 12, since: 1_700_000_000_000 }, next: 13 };
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
  assert.deepEqual([same.active, same.next], [{ number: 12, since }, null], 'a próxima igual à em andamento não vale');
});

check('settleStored: a resposta de uma criação não mexe na em andamento nem na próxima', () => {
  const text = JSON.stringify({ v: 1, pending: [pending('ok', { status: 'sending', error: undefined })], active: { number: 4, since: 10 }, next: 5 });
  const done = settleStored(text, 'ok');
  assert.deepEqual(done, { v: 1, pending: [], active: { number: 4, since: 10 }, next: 5 });
  assert.equal(storedEmpty(done), false, 'com em andamento ou próxima a chave fica no localStorage');
  assert.equal(storedEmpty({ ...done, active: null }), false);
  assert.equal(storedEmpty({ ...done, active: null, next: null }), true);
});

check('startTracked: vira a em andamento; o since só recomeça ao trocar de issue; deixa de ser a próxima', () => {
  const none = { active: null, next: null };
  assert.deepEqual(startTracked(none, 7, 100), { active: { number: 7, since: 100 }, next: null });
  const running = { active: { number: 7, since: 100 }, next: 8 };
  assert.deepEqual(startTracked(running, 7, 500), running, 'a mesma issue mantém o since');
  assert.deepEqual(startTracked(running, 9, 500), { active: { number: 9, since: 500 }, next: 8 }, 'a anterior só deixa de ser a em andamento');
  assert.deepEqual(startTracked(running, 8, 500), { active: { number: 8, since: 500 }, next: null }, 'a próxima que começa deixa de ser a próxima');
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
