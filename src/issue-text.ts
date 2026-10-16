// Parte pura das Issues: o texto que vai ao orquestrador, datas relativas, as labels (mais usadas, todas, as do filtro, a cor e o nome de
// uma nova), busca, filtro e ordenação da faixa, as fechadas recentes, qual issue o detalhe mostra, as seções e o teclado da faixa, as
// regras da issue em andamento e da próxima, o rascunho do orquestrador (o que foi colado e não enviado) e a leitura das criações que a
// versão com o GitHub deixou no localStorage.
// Sem React e sem armazenamento; só sintaxe apagável, para o check rodar direto no Node (`scripts/check-issues.mjs`).
import type { ActiveIssue, Issue } from './types';

// A em andamento e a próxima (a que é colada sozinha quando o orquestrador termina a rodada). Nunca a mesma issue nas duas.
export type Tracked = { active: ActiveIssue | null; next: number | null };

/** Texto que vai ao orquestrador ao colar a issue: número e título e, depois de uma linha em branco, a descrição (se houver), com `\n`. */
export function issuePrompt(issue: Issue): string {
  const body = issue.body.replace(/\r\n?/g, '\n').trimEnd();
  const head = `Issue #${issue.number}: ${issue.title}`;
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

/** O maior nome de label, em caracteres (o mesmo limite do backend). */
export const LABEL_MAX = 50;

// Ordem alfabética das labels, sem diferenciar maiúsculas e acentos.
const byName = (a: string, b: string) => a.localeCompare(b, 'pt-BR', { sensitivity: 'base' }) || a.localeCompare(b);

/** Todas as labels em uso nas issues (abertas e fechadas), em ordem alfabética: a lista do "+". */
export const allLabels = (issues: readonly Issue[]): string[] => [...new Set(issues.flatMap((i) => i.labels))].sort(byName);

/** As `n` labels mais usadas em `issues`; empate em ordem alfabética. */
export function topLabels(issues: readonly Issue[], n: number): string[] {
  const uses = new Map<string, number>();
  for (const issue of issues) for (const name of issue.labels) uses.set(name, (uses.get(name) ?? 0) + 1);
  return [...uses.keys()].sort((a, b) => uses.get(b)! - uses.get(a)! || byName(a, b)).slice(0, Math.max(0, n));
}

/**
 * O nome de uma label digitada: espaços juntados; vazio é null. Se já existe em `known` (sem diferenciar maiúsculas), vale a grafia de lá,
 * como o backend faz ao gravar.
 */
export function pickLabel(known: readonly string[], typed: string): string | null {
  const name = typed.split(/\s+/).filter(Boolean).join(' ');
  if (!name) return null;
  return known.find((k) => k.toLowerCase() === name.toLowerCase()) ?? name;
}

// A paleta das labels: cores claras o bastante para a bolinha aparecer no fundo escuro, longe das cores de estado das issues.
const LABEL_COLORS = ['f47067', 'f69d50', 'daaa3f', '57ab5a', '39c5cf', '539bf5', '986ee2', 'e275ad'];

/** A cor da bolinha de uma label (hex sem `#`): uma da paleta, sempre a mesma para o mesmo nome (sem diferenciar maiúsculas). */
export function labelColor(name: string): string {
  let hash = 0x811c9dc5;
  for (const ch of name.toLowerCase()) hash = Math.imul(hash ^ ch.codePointAt(0)!, 0x01000193) >>> 0;
  return LABEL_COLORS[hash % LABEL_COLORS.length];
}

// Sem acento e sem diferença de maiúsculas, para a busca.
const fold = (text: string) => text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();

/** A busca da faixa: o título contém o texto (sem diferenciar maiúsculas e acentos), ou o texto é o número exato, com ou sem `#`.
 * Busca vazia passa tudo. */
export function matchesQuery(title: string, number: number, query: string): boolean {
  const q = query.trim();
  if (!q) return true;
  const exact = /^#?(\d+)$/.exec(q);
  if (exact && number === Number(exact[1])) return true;
  return fold(title).includes(fold(q));
}

/** Passa se tem todas as labels marcadas. Sem nenhuma marcada, passa tudo. */
export const hasLabels = (names: readonly string[], wanted: readonly string[]) => wanted.every((w) => names.includes(w));

/** O filtro da faixa: a busca (`matchesQuery`) e todas as labels marcadas (`hasLabels`). */
export const passesFilter = (issue: Issue, query: string, wanted: readonly string[]) =>
  matchesQuery(issue.title, issue.number, query) && hasLabels(issue.labels, wanted);

/** As labels em uso nas issues abertas, em ordem alfabética, para os chips do filtro da faixa. As marcadas ficam mesmo sem uso, para
 * poderem ser desmarcadas. */
export function filterLabels(issues: readonly Issue[], selected: readonly string[]): string[] {
  return [...new Set([...issues.filter((i) => i.state === 'open').flatMap((i) => i.labels), ...selected])].sort(byName);
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

const isStrings = (v: unknown): v is string[] => Array.isArray(v) && v.every((x) => typeof x === 'string');

/**
 * As criações que a versão com o GitHub guardou no localStorage do projeto e nunca chegaram lá (`{ v: 1, pending: [...] }`), na ordem
 * em que estão: título, descrição e labels de cada uma. Texto ilegível, formato errado e criação sem título ou com campo inválido ficam de
 * fora, sem lançar. A em andamento e a próxima de lá eram números do GitHub e não são lidas.
 */
export function legacyIssues(text: string | null): { title: string; body: string; labels: string[] }[] {
  let data: unknown;
  try {
    data = JSON.parse(text ?? '');
  } catch {
    return [];
  }
  const list = data && typeof data === 'object' ? (data as { pending?: unknown }).pending : undefined;
  if (!Array.isArray(list)) return [];
  return list.flatMap((p: unknown) => {
    if (!p || typeof p !== 'object') return [];
    const { title, body = '', labels = [] } = p as Record<string, unknown>;
    if (typeof title !== 'string' || !title.trim() || typeof body !== 'string' || !isStrings(labels)) return [];
    return [{ title: title.trim(), body, labels: [...labels] }];
  });
}
