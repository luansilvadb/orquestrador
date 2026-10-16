// Parte pura das Issues: o texto que vai ao orquestrador, datas relativas, labels mais usadas, busca, filtro
// e ordenação da faixa, as fechadas recentes, qual issue o detalhe mostra, as seções e o teclado da faixa, o estado salvo (a leitura e a
// resposta de uma criação), o espaçamento e o motivo das criações que falharam, as regras da issue em andamento e da próxima e o rascunho do orquestrador (o que foi colado e não enviado).
// Sem React e sem armazenamento; só sintaxe apagável, para o check rodar direto no Node (`scripts/check-issues.mjs`).
import type { Issue, Label } from './types';

// Criação que ainda não está no GitHub: `sending` enquanto o pedido está a caminho, `failed` com a mensagem do erro e `waiting` guardada
// sem tentar (o GitHub não estava disponível na hora de criar; sai sozinha quando ele voltar). `failures` e `failedAt`: quantas vezes o envio
// falhou e quando foi a última (`Date.now()`); a `sending` que já tinha falhado os mantém, com o `error` da última, durante a nova tentativa.
// Ausentes: nenhuma falha.
export type PendingIssue = {
  localId: string; title: string; body: string; labels: string[]; createdAt: number; status: 'sending' | 'failed' | 'waiting'; error?: string;
  failures?: number; failedAt?: number;
};
// A issue com o orquestrador e desde quando (`Date.now()`). `turned`: uma rodada do orquestrador começou depois do Iniciar; falso é "colada,
// aguardando envio" (o Iniciar só cola). Ausente no que foi salvo antes dele existir: vale falso, o lado seguro (Concluir pergunta).
export type ActiveIssue = { number: number; since: number; turned: boolean };
// A em andamento e a próxima (a que é colada sozinha quando o orquestrador termina a rodada). Nunca a mesma issue nas duas.
export type Tracked = { active: ActiveIssue | null; next: number | null };
// O que o app guarda por projeto no localStorage. `active` e `next` entraram depois, ainda na `v: 1`: sem eles, valem null.
export type StoredIssues = { v: 1; pending: PendingIssue[] } & Tracked;

/** Texto que vai ao orquestrador ao iniciar a issue: número, título, URL e a descrição (se houver), com `\n`. */
export function issuePrompt(issue: Issue): string {
  const body = issue.body.replace(/\r\n?/g, '\n').trimEnd();
  const head = `Issue #${issue.number}: ${issue.title}\n${issue.url}`;
  return body ? `${head}\n\n${body}` : head;
}

const MINUTE = 60_000, HOUR = 60 * MINUTE, DAY = 24 * HOUR;
const plural = (n: number, one: string, many: string) => `há ${n} ${n === 1 ? one : many}`;

/** Tempo desde `iso` em português ("agora", "há 5 min", "há 2 horas", "há 1 dia", "há 2 meses", "há 1 ano"). Data futura ou inválida é "agora". */
export function relativeTime(iso: string, now: number): string {
  const ms = now - Date.parse(iso);
  if (!(ms >= MINUTE)) return 'agora';
  if (ms < HOUR) return `há ${Math.floor(ms / MINUTE)} min`;
  if (ms < DAY) return plural(Math.floor(ms / HOUR), 'hora', 'horas');
  const days = Math.floor(ms / DAY);
  if (days < 30) return plural(days, 'dia', 'dias');
  if (days < 365) return plural(Math.floor(days / 30), 'mês', 'meses');
  return plural(Math.floor(days / 365), 'ano', 'anos');
}

/** As `n` labels do repositório mais usadas em `issues`; empate (e as não usadas, para completar) na ordem do repositório. */
export function topLabels(issues: Issue[], labels: Label[], n: number): Label[] {
  const uses = new Map<string, number>();
  for (const issue of issues) for (const l of issue.labels) uses.set(l.name, (uses.get(l.name) ?? 0) + 1);
  return labels
    .map((label, i) => ({ label, i, count: uses.get(label.name) ?? 0 }))
    .sort((a, b) => b.count - a.count || a.i - b.i)
    .slice(0, Math.max(0, n))
    .map((x) => x.label);
}

// Sem acento e sem diferença de maiúsculas, para a busca.
const fold = (text: string) => text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();

/** A busca da página Issues: o título contém o texto (sem diferenciar maiúsculas e acentos), ou o texto é o número exato, com ou sem `#`.
 * Busca vazia passa tudo. `number` null (criação ainda sem número) só passa pelo título. */
export function matchesQuery(title: string, number: number | null, query: string): boolean {
  const q = query.trim();
  if (!q) return true;
  const exact = /^#?(\d+)$/.exec(q);
  if (exact && number === Number(exact[1])) return true;
  return fold(title).includes(fold(q));
}

/** Passa se tem todas as labels marcadas (como o filtro do GitHub). Sem nenhuma marcada, passa tudo. */
export const hasLabels = (names: readonly string[], wanted: readonly string[]) => wanted.every((w) => names.includes(w));

/** O filtro da faixa: a busca da página (`matchesQuery`) e todas as labels marcadas (`hasLabels`). */
export const passesFilter = (issue: Issue, query: string, wanted: readonly string[]) =>
  matchesQuery(issue.title, issue.number, query) && hasLabels(issue.labels.map((l) => l.name), wanted);

/** As labels do repositório (na ordem dele) em uso nas issues abertas, para os chips do filtro da faixa. As marcadas ficam mesmo sem uso,
 * para poderem ser desmarcadas. */
export function filterLabels(issues: readonly Issue[], labels: readonly Label[], selected: readonly string[]): Label[] {
  const used = new Set(issues.filter((i) => i.state === 'open').flatMap((i) => i.labels.map((l) => l.name)));
  return labels.filter((l) => used.has(l.name) || selected.includes(l.name));
}

/** As `n` fechadas mais recentes (pelo `closedAt`; sem ele, pelo `updatedAt`), da mais recente para a mais antiga; empate pelo número, maior primeiro. */
export function recentClosed(issues: readonly Issue[], n: number): Issue[] {
  const key = (i: Issue) => Date.parse(i.closedAt ?? i.updatedAt) || 0;
  return issues
    .filter((i) => i.state === 'closed')
    .sort((a, b) => key(b) - key(a) || b.number - a.number)
    .slice(0, Math.max(0, n));
}

/** Qual issue o detalhe mostra: a selecionada, se ainda existe (aberta ou fechada); senão a em andamento, a próxima e a primeira aberta
 * (a mais nova, que é a primeira da faixa depois da próxima), cada uma só se está aberta. Nenhuma aberta e nada selecionado: null. */
export function detailIssue(issues: readonly Issue[], selected: number | null, active: number | null, next: number | null): Issue | null {
  const byNumber = (n: number | null) => (n === null ? undefined : issues.find((i) => i.number === n));
  const open = (n: number | null) => {
    const issue = byNumber(n);
    return issue?.state === 'open' ? issue : undefined;
  };
  return byNumber(selected) ?? open(active) ?? open(next) ?? sortIssues(issues.filter((i) => i.state === 'open'), 'newest')[0] ?? null;
}

// As seções da faixa e se estão abertas. "Fechadas recentes" começa recolhida.
export type IssueSections = { active: boolean; queue: boolean; closed: boolean };
export const SECTIONS_DEFAULT: IssueSections = { active: true, queue: true, closed: false };

/** Lê o estado de recolher das seções (`localStorage`) com desconfiança: o que não é booleano fica com o padrão. */
export function readSections(text: string | null): IssueSections {
  let data: unknown;
  try {
    data = JSON.parse(text ?? '');
  } catch {
    return { ...SECTIONS_DEFAULT };
  }
  const saved = (data && typeof data === 'object' ? data : {}) as Record<string, unknown>;
  const pick = (k: keyof IssueSections) => (typeof saved[k] === 'boolean' ? saved[k] : SECTIONS_DEFAULT[k]);
  return { active: pick('active'), queue: pick('queue'), closed: pick('closed') };
}

/** Navegação por teclado numa lista de `length` linhas, a partir de `at`: ↑/↓ andam uma (sem dar a volta), Home e End vão às pontas.
 * Outra tecla, ou lista vazia, devolve null. */
export function rovingStep(at: number, length: number, key: string): number | null {
  if (length <= 0) return null;
  if (key === 'Home') return 0;
  if (key === 'End') return length - 1;
  if (key === 'ArrowDown') return Math.min(length - 1, at + 1);
  if (key === 'ArrowUp') return Math.max(0, at - 1);
  return null;
}

export type IssueOrder = 'newest' | 'oldest' | 'updated';
/** Ordena uma cópia: mais novas, mais antigas (pela criação) ou atualizadas recentemente; empate pelo número, maior primeiro. */
export function sortIssues(issues: readonly Issue[], order: IssueOrder): Issue[] {
  const key = (i: Issue) => Date.parse(order === 'updated' ? i.updatedAt : i.createdAt) || 0;
  const dir = order === 'oldest' ? 1 : -1;
  return [...issues].sort((a, b) => dir * (key(a) - key(b)) || dir * (a.number - b.number));
}

const isStrings = (v: unknown): v is string[] => Array.isArray(v) && v.every((x) => typeof x === 'string');

/** Lê o estado salvo com desconfiança. JSON ilegível ou formato errado volta vazio; criação inválida é descartada sozinha.
 * Uma `sending` salva foi interrompida (o app fechou antes da resposta) e volta como `failed`, com mais uma falha em `now`, exceto as de
 * `inFlight`: o pedido delas ainda está a caminho nesta execução do app (o projeto foi trocado e reaberto no meio do envio) e elas continuam
 * `sending`. Uma `failed` salva antes da contagem (sem `failures`) vale uma falha, já vencida: sai na próxima atualização boa. */
export function readStoredIssues(text: string | null, inFlight: ReadonlySet<string> = new Set(), now = Date.now()): StoredIssues {
  return parseStored(text, (id) => inFlight.has(id), now);
}

/** A criação depois de mais uma falha em `at`: `failed`, com a mensagem, a contagem somada e a hora. */
export const failPending = (p: PendingIssue, error: string, at: number): PendingIssue =>
  ({ ...p, status: 'failed', error, failures: (p.failures ?? 0) + 1, failedAt: at });

/** O estado salvo depois da resposta de uma criação: sem `error`, a criação chegou ao GitHub e sai; com `error`, fica `failed` com a mensagem
 * e mais uma falha em `at` (`failPending`). As outras `sending` ficam como estão (podem estar a caminho). Criação que não está no texto
 * (descartada no meio do envio) não volta. */
export function settleStored(text: string | null, localId: string, error?: string, at = Date.now()): StoredIssues {
  const stored = parseStored(text, () => true, at);
  const pending = error === undefined
    ? stored.pending.filter((p) => p.localId !== localId)
    : stored.pending.map((p) => (p.localId === localId ? failPending(p, error, at) : p));
  return { ...stored, pending };
}

// A espera antes da tentativa automática seguinte, pela contagem de falhas: depois da 1ª, 15 s; da 2ª, 1 min; da 3ª, 5 min. Da 4ª em
// diante, nenhuma tentativa automática: só o Tentar de novo.
const RETRY_WAIT: readonly { ms: number; text: string }[] = [{ ms: 15_000, text: '15 s' }, { ms: MINUTE, text: '1 min' }, { ms: 5 * MINUTE, text: '5 min' }];
const retryWait = (p: PendingIssue): { ms: number; text: string } | undefined => RETRY_WAIT[Math.max(1, p.failures ?? 1) - 1];

/** A criação que falhou já pode ser reenviada sozinha em `now` (numa atualização boa)? Só a `failed`, e só depois da espera da contagem dela. */
export function retryDue(p: PendingIssue, now: number): boolean {
  const wait = p.status === 'failed' ? retryWait(p) : undefined;
  return wait !== undefined && now - (p.failedAt ?? 0) >= wait.ms;
}

/** Uma criação que já tinha falhado e está sendo enviada de novo (a automática ou o Tentar de novo): a linha mantém o motivo e as ações. */
export const pendingRetrying = (p: PendingIssue) => p.status === 'sending' && (p.failures ?? 0) > 0;

/** O motivo de uma criação que falhou, com o que vai acontecer: a próxima tentativa automática ("Nova tentativa automática em 15 s."),
 * nenhuma depois da 4ª falha ("Falhou 4 vezes: … Sem novas tentativas automáticas.") ou, durante uma nova tentativa, "Tentando de novo…".
 * `failed` falso dá o texto da tentativa; verdadeiro, o da falha, mesmo durante a tentativa (a linha reserva o espaço dele, para a altura não mudar). */
export function failedReason(p: PendingIssue, failed = p.status === 'failed'): string {
  const raw = (p.error ?? '').trim() || 'erro desconhecido';
  // A mensagem do GitHub costuma terminar com ponto: não dobra.
  const said = /[.!?…]$/.test(raw) ? raw : `${raw}.`;
  if (!failed) return `Falhou: ${said} Tentando de novo…`;
  const wait = retryWait(p), n = Math.max(1, p.failures ?? 1);
  return wait ? `Falhou: ${said} Nova tentativa automática em ${wait.text}.` : `Falhou ${n} vezes: ${said} Sem novas tentativas automáticas.`;
}

/** Nada a guardar: sem criações, sem em andamento e sem próxima (a chave sai do localStorage). */
export const storedEmpty = (stored: StoredIssues) => !stored.pending.length && !stored.active && stored.next === null;

/** Faz de `number` a em andamento. `since` só recomeça quando muda de issue; se ela era a próxima, deixa de ser. */
export function startTracked(t: Tracked, number: number, now: number): Tracked {
  const active = t.active?.number === number ? t.active : { number, since: now, turned: false };
  return { active, next: t.next === number ? null : t.next };
}

/** Uma rodada do orquestrador começou: a em andamento deixa de ser só colada. Sem em andamento, ou já marcada, devolve o mesmo `t`. */
export const markTurned = (t: Tracked): Tracked => (!t.active || t.active.turned ? t : { ...t, active: { ...t.active, turned: true } });

/** Marca `number` como a próxima (null tira a marca). A em andamento não pode ser a próxima: fica como está. */
export const markNext = (t: Tracked, number: number | null): Tracked =>
  (number !== null && t.active?.number === number ? t : { ...t, next: number });

/** Zera a em andamento e a próxima que não estão mais abertas em `issues` (fechadas ou fora da lista). Sem mudança, devolve o mesmo `t`. */
export function pruneTracked(t: Tracked, issues: readonly Issue[]): Tracked {
  const open = (n: number) => issues.some((i) => i.number === n && i.state === 'open');
  const active = t.active && !open(t.active.number) ? null : t.active;
  const next = t.next !== null && !open(t.next) ? null : t.next;
  return active === t.active && next === t.next ? t : { active, next };
}

// O rascunho do orquestrador: as issues coladas nele desde o último envio, na ordem da colagem. Só em memória, por projeto (o rascunho morre
// com o processo do orquestrador): zera no envio, quando o orquestrador abre ou termina e ao abrir o app.
export type Draft = readonly number[];

/** Colar `number` com `draft` no rascunho: `already` (já está lá; colar de novo duplicaria), `paste` (rascunho vazio) ou `ask` (há outras lá:
 * juntar no mesmo envio ou trocar é do usuário). */
export const draftPaste = (draft: Draft, number: number): 'already' | 'paste' | 'ask' =>
  (draft.includes(number) ? 'already' : draft.length ? 'ask' : 'paste');

/** O rascunho depois de colar `number`: `join` (Colar junto) a põe depois das que estavam; senão (rascunho vazio ou "Já apaguei") fica só ela.
 * Já lá, não muda. */
export const draftAdd = (draft: Draft, number: number, join: boolean): Draft =>
  (draft.includes(number) ? draft : join ? [...draft, number] : [number]);

/** O que vai ao terminal: junto do que já está no rascunho, depois de uma linha em branco. */
export const draftText = (prompt: string, join: boolean) => (join ? `\n\n${prompt}` : prompt);

/** O rascunho quando uma rodada começa: o envio (`prompt` com o texto, ou `undefined` de um CLI antigo) leva a linha inteira e o deixa vazio,
 * tenha a issue ido ou não; `null` (a leitura inicial do estado, sem envio) não muda. Sem mudança, devolve o mesmo `draft`. */
export const draftOnTurn = (draft: Draft, prompt: string | null | undefined): Draft => (prompt === null || !draft.length ? draft : []);

/** As issues numa frase: "a #11", "a #11 e a #12", "a #11, a #12 e a #13". */
export function issueList(numbers: readonly number[]): string {
  const items = numbers.map((n) => `a #${n}`);
  return items.length < 2 ? (items[0] ?? '') : `${items.slice(0, -1).join(', ')} e ${items[items.length - 1]}`;
}
const capital = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);
const TOGETHER = ['as duas', 'as três', 'as quatro', 'as cinco', 'as seis', 'as sete', 'as oito', 'as nove', 'as dez'];

/** A pergunta antes de colar `number` com `others` (as do rascunho, nenhuma igual a ela) ainda não enviadas. */
export function draftAsk(others: readonly number[], number: number): { title: string; body: string } {
  const list = issueList(others), one = others.length === 1;
  return {
    title: `${capital(list)} ainda ${one ? 'está' : 'estão'} no rascunho`,
    body: `${capital(list)} ${one ? 'foi colada' : 'foram coladas'} no rascunho do orquestrador e não ${one ? 'foi enviada' : 'foram enviadas'}. `
      + `Colar a #${number} agora junta ${TOGETHER[others.length - 1] ?? 'todas'} no mesmo envio. `
      + `${one ? 'Para trocar uma pela outra' : 'Para trocar'}, apague ${list} no terminal do orquestrador antes.`,
  };
}

/** O aviso do Colar junto. */
export const draftJoined = (others: readonly number[], number: number) =>
  `#${number} colada junto com ${issueList(others)}; só a #${number} fica em andamento.`;

/** O aviso da próxima que não foi colada sozinha porque o rascunho tem outras ainda não enviadas. */
export const draftWaits = (draft: Draft, next: number) =>
  `A #${next} espera: o rascunho do orquestrador tem ${issueList(draft)}, ainda não ${draft.length === 1 ? 'enviada' : 'enviadas'}.`;

const isNumber = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v > 0;

function parseStored(text: string | null, keepSending: (localId: string) => boolean, now: number): StoredIssues {
  const empty: StoredIssues = { v: 1, pending: [], active: null, next: null };
  let data: unknown;
  try {
    data = JSON.parse(text ?? '');
  } catch {
    return empty;
  }
  if (!data || typeof data !== 'object' || (data as { v?: unknown }).v !== 1) return empty;
  const list = (data as { pending?: unknown }).pending;
  if (!Array.isArray(list)) return empty;
  const pending: PendingIssue[] = [];
  const seen = new Set<string>();
  for (const p of list as Record<string, unknown>[]) {
    if (!p || typeof p !== 'object') continue;
    const { localId, title, body, labels, createdAt, status, error, failures, failedAt } = p;
    if (typeof localId !== 'string' || !localId || seen.has(localId) || typeof title !== 'string' || typeof body !== 'string') continue;
    if (!isStrings(labels) || typeof createdAt !== 'number' || !Number.isFinite(createdAt)) continue;
    if (status !== 'sending' && status !== 'failed' && status !== 'waiting') continue;
    if (error !== undefined && typeof error !== 'string') continue;
    seen.add(localId);
    const base = { localId, title, body, labels: [...labels], createdAt };
    if (status === 'waiting') {
      pending.push({ ...base, status: 'waiting' });
      continue;
    }
    // Contagem e hora inválidas ou ausentes (dado de antes delas) valem nenhuma falha e uma hora muito antiga.
    const count = isNumber(failures) ? failures : 0, at = typeof failedAt === 'number' && Number.isFinite(failedAt) ? failedAt : 0;
    const tried = count ? { failures: count, failedAt: at } : {};
    if (status === 'sending' && keepSending(localId)) {
      pending.push({ ...base, status: 'sending', ...(error ? { error } : {}), ...tried });
      continue;
    }
    // Interrompida: mais uma falha, agora. A `failed` sem contagem vale uma, já vencida.
    if (status === 'sending') pending.push(failPending({ ...base, status, ...tried }, 'O envio foi interrompido antes da resposta do GitHub.', now));
    else pending.push({ ...base, status: 'failed', ...(error ? { error } : {}), failures: count || 1, failedAt: at });
  }
  // Em andamento e próxima inválidas valem null; a próxima igual à em andamento também.
  const { active: a, next: n } = data as { active?: unknown; next?: unknown };
  const { number, since } = (a && typeof a === 'object' ? a : {}) as { number?: unknown; since?: unknown };
  const turned = (a as { turned?: unknown } | null)?.turned === true;
  const active = isNumber(number) && typeof since === 'number' && Number.isFinite(since) ? { number, since, turned } : null;
  const next = isNumber(n) && n !== active?.number ? n : null;
  return { v: 1, pending, active, next };
}

/** Um pedaço de mensagem: texto corrido ou trecho entre crases (`code`), já sem as crases. */
export type TextPart = { text: string; code: boolean };

/** Separa os trechos entre crases do resto ("rode `gh auth login`." → texto, código, texto). Crase sem par fica como texto; trecho vazio some. */
export function splitCode(message: string): TextPart[] {
  const parts: TextPart[] = [];
  const re = /`([^`\n]+)`/g;
  let last = 0;
  for (let m = re.exec(message); m; m = re.exec(message)) {
    if (m.index > last) parts.push({ text: message.slice(last, m.index), code: false });
    parts.push({ text: m[1], code: true });
    last = m.index + m[0].length;
  }
  if (last < message.length) parts.push({ text: message.slice(last), code: false });
  return parts;
}

/** O comando para rodar que a mensagem traz: o primeiro trecho entre crases que começa por `gh `; null se não há (um `origin` não é comando). */
export function commandIn(message: string): string | null {
  return splitCode(message).find((p) => p.code && /^gh\s/.test(p.text))?.text.trim() ?? null;
}