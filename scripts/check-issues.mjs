// Check da parte pura das Issues (src/issue-text.ts): o texto que vai ao orquestrador, datas relativas, as labels (mais usadas, todas,
// as do filtro, a cor e o nome de uma nova), busca, filtro e ordenação da faixa, as fechadas recentes, qual issue o detalhe mostra, as
// seções e o teclado da faixa, as regras da issue em andamento e da próxima, o rascunho do orquestrador (o que foi colado e não enviado)
// e a leitura das criações que a versão com o GitHub deixou no localStorage. Roda sem dependências: `node scripts/check-issues.mjs`.
// Sai com 1 e mostra a falha se alguma regra quebrar.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import {
  allLabels, detailIssue, draftAdd, draftAsk, draftJoined, draftOnTurn, draftPaste, draftText, draftWaits, filterLabels, hasLabels, issueList,
  issuePrompt, LABEL_MAX, labelColor, legacyIssues, markNext, markTurned, matchesQuery, passesFilter, pickLabel, pruneTracked, readSections,
  recentClosed, relativeTime, rovingStep, SECTIONS_DEFAULT, sortIssues, startTracked, topLabels,
} from '../src/issue-text.ts';

const issue = (number, body, labels = []) => ({
  number, title: `Título ${number}`, body, state: 'open', labels, createdAt: '2026-10-01T00:00:00Z', updatedAt: '2026-10-01T00:00:00Z', closedAt: null,
});

check('issuePrompt: número, título, linha em branco e a descrição', () => {
  assert.equal(issuePrompt(issue(7, 'Primeira linha\n\n- item')), 'Issue #7: Título 7\n\nPrimeira linha\n\n- item');
});

check('issuePrompt: sem descrição (ou só espaço) não tem a linha em branco', () => {
  assert.equal(issuePrompt(issue(3, '')), 'Issue #3: Título 3');
  assert.equal(issuePrompt(issue(3, ' \r\n\n\t')), 'Issue #3: Título 3');
});

check('issuePrompt: \\r\\n vira \\n e o espaço do fim sai', () => {
  assert.equal(issuePrompt(issue(1, 'a\r\nb\r\n\r\nc  \r\n\r\n')), 'Issue #1: Título 1\n\na\nb\n\nc');
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

check('topLabels: mais usadas primeiro, empate em ordem alfabética; abertas e fechadas contam', () => {
  const closed = { ...issue(4, '', ['bug', 'ui']), state: 'closed' };
  const issues = [issue(1, '', ['question', 'enhancement']), issue(2, '', ['enhancement', 'documentation']), issue(3, '', ['question', 'ui']), closed];
  // enhancement 2, question 2, ui 2 (empate: ordem alfabética), bug 1, documentation 1.
  assert.deepEqual(topLabels(issues, 3), ['enhancement', 'question', 'ui']);
  assert.deepEqual(topLabels(issues, 10), ['enhancement', 'question', 'ui', 'bug', 'documentation'], 'n maior que o total devolve todas');
  assert.deepEqual(topLabels(issues, 0), []);
  assert.deepEqual(topLabels([], 3), [], 'projeto sem labels');
});

check('allLabels: todas as labels em uso, sem repetir, em ordem alfabética sem diferenciar maiúsculas e acentos', () => {
  const closed = { ...issue(3, '', ['zeta']), state: 'closed' };
  assert.deepEqual(allLabels([issue(1, '', ['ui', 'Bug', 'ação']), issue(2, '', ['ui', 'acesso']), closed]), ['ação', 'acesso', 'Bug', 'ui', 'zeta']);
  assert.deepEqual(allLabels([issue(1, '')]), []);
});

check('pickLabel: espaços juntados, vazio é null, e a grafia de uma que já existe vale', () => {
  assert.equal(pickLabel([], '  nova   label  '), 'nova label');
  assert.equal(pickLabel([], ' \t '), null);
  assert.equal(pickLabel(['Bug', 'ui'], 'bug'), 'Bug');
  assert.equal(pickLabel(['Bug'], 'BUG '), 'Bug');
  assert.equal(pickLabel(['Bug'], 'bugs'), 'bugs');
  assert.equal(LABEL_MAX, 50, 'o mesmo limite do backend');
});

check('labelColor: uma cor da paleta, a mesma para o mesmo nome sem diferenciar maiúsculas', () => {
  const colors = new Set(['bug', 'ui', 'documentation', 'enhancement', 'question', 'ação', 'help wanted', 'x', '', '🙂'].map(labelColor));
  for (const color of colors) assert.match(color, /^[0-9a-f]{6}$/);
  assert.ok(colors.size > 1, 'nomes diferentes não caem todos na mesma cor');
  assert.equal(labelColor('Bug'), labelColor('bug'));
  assert.equal(labelColor('bug'), labelColor('bug'), 'estável');
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

check('filterLabels: só as labels em uso nas abertas, em ordem alfabética; as marcadas ficam', () => {
  const closed = { ...issue(3, '', ['wontfix']), state: 'closed' };
  const issues = [issue(1, '', ['question', 'bug']), issue(2, '', ['question', 'docs']), closed];
  assert.deepEqual(filterLabels(issues, []), ['bug', 'docs', 'question'], 'a da fechada não conta');
  assert.deepEqual(filterLabels(issues, ['wontfix']), ['bug', 'docs', 'question', 'wontfix'], 'a marcada sem uso fica, para desmarcar');
  assert.deepEqual(filterLabels(issues, ['bug']), ['bug', 'docs', 'question'], 'sem repetir');
  assert.deepEqual(filterLabels([], []), []);
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
  assert.equal(n(detailIssue(list, 99, 3, null)), 3, 'a escolhida foi excluída: cai na regra sem escolha');
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
  assert.deepEqual(pruneTracked(t, [at(1, 'closed'), at(2, 'open')]), { active: null, next: 2 }, 'em andamento fechada (em outra janela do app)');
  assert.deepEqual(pruneTracked(t, [at(1, 'open'), at(2, 'closed')]), { active: t.active, next: null }, 'próxima fechada');
  assert.deepEqual(pruneTracked(t, [at(3, 'open')]), { active: null, next: null }, 'nenhuma das duas na lista (excluídas)');
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

check('Rodada desde o Colar: colada → trabalhando; um Colar de outra issue recomeça; o mesmo Colar mantém', () => {
  let t = startTracked({ active: null, next: null }, 7, 100);
  assert.equal(t.active.turned, false, 'recém-colada: aguardando envio');
  t = markTurned(t);
  assert.equal(t.active.turned, true, 'rodada começou');
  assert.equal(startTracked(t, 7, 900).active.turned, true, 'o mesmo Colar não zera');
  assert.equal(startTracked(t, 9, 900).active.turned, false, 'outra issue começa colada de novo');
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
  assert.deepEqual(draftOnTurn(draft, 'Issue #11: Título 11\n\ntexto'), []);
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

check('legacyIssues: as criações guardadas pela versão com o GitHub, na ordem; o resto fica de fora sem lançar', () => {
  for (const text of [null, '', '{', 'null', '42', '"texto"', '[]', '{}', '{"v":1}', '{"v":1,"pending":{}}']) assert.deepEqual(legacyIssues(text), [], String(text));
  const saved = {
    v: 1, active: { number: 12, since: 1, turned: true }, next: 13,
    pending: [
      { localId: 'a', title: ' Ideia ', body: 'texto', labels: ['bug'], createdAt: 1, status: 'failed', error: 'Sem conexão com o GitHub.', failures: 2 },
      null, 'texto', 3,
      { localId: 'b', title: '   ', body: '', labels: [] },
      { localId: 'c', title: 5, body: '', labels: [] },
      { localId: 'd', title: 'labels erradas', body: '', labels: ['bug', 1] },
      { localId: 'e', title: 'corpo errado', body: 7, labels: [] },
      { localId: 'f', title: 'Guardada', body: '', labels: [], status: 'waiting' },
      { title: 'Só o essencial' },
    ],
  };
  assert.deepEqual(legacyIssues(JSON.stringify(saved)), [
    { title: 'Ideia', body: 'texto', labels: ['bug'] },
    { title: 'Guardada', body: '', labels: [] },
    { title: 'Só o essencial', body: '', labels: [] },
  ]);
  // O que sobra da migração interrompida (gravado no mesmo formato) é lido do mesmo jeito.
  const rest = JSON.stringify({ v: 1, pending: [{ title: 'Guardada', body: '', labels: [] }] });
  assert.deepEqual(legacyIssues(rest), [{ title: 'Guardada', body: '', labels: [] }]);
});
