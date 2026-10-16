// Parte pura das Issues: o texto que vai ao orquestrador, datas relativas, cor do texto das labels, labels mais usadas, busca, filtro
// e ordenação da página, o filtro, as fechadas recentes, a prévia, as seções e o teclado da faixa, o estado salvo (a leitura e a resposta
// de uma criação) e as regras da issue em andamento e da próxima.
// Sem React e sem armazenamento; só sintaxe apagável, para o check rodar direto no Node (`scripts/check-issues.mjs`).
import type { Issue, Label } from './types';

// Criação que ainda não está no GitHub: `sending` enquanto o pedido está a caminho, `failed` com a mensagem do erro.
export type PendingIssue = {
  localId: string; title: string; body: string; labels: string[]; createdAt: number; status: 'sending' | 'failed'; error?: string;
};
// A issue com o orquestrador e desde quando (`Date.now()`).
export type ActiveIssue = { number: number; since: number };
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

/** Cor do texto sobre o fundo `hex` (com ou sem `#`, 3 ou 6 dígitos): preto ou branco, pela luminosidade percebida (YIQ), como no site do GitHub.
 * O limiar 186 faz `d876e3`, `008672` e `0e8a16` ficarem com texto branco e `e4e669`, `a2eeef` e `cfd3d7` com preto. Hex inválido fica com o preto. */
export function labelTextColor(hex: string): string {
  let h = hex.replace(/^#/, '');
  if (/^[0-9a-f]{3}$/i.test(h)) h = [...h].map((c) => c + c).join('');
  if (!/^[0-9a-f]{6}$/i.test(h)) return '#000000';
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
  return (r * 299 + g * 587 + b * 114) / 1000 > 186 ? '#000000' : '#ffffff';
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

/** A descrição da prévia como texto simples (o Markdown fica como está): `\r\n` vira `\n`, as linhas em branco seguidas viram uma só,
 * e passa de `lines` linhas corta com "…" no fim da última. Sem descrição (ou só espaço) volta vazio. */
export function previewBody(body: string, lines: number): string {
  const all = body.replace(/\r\n?/g, '\n').trim().replace(/\n[ \t]*(\n[ \t]*)+/g, '\n\n').split('\n').map((l) => l.trimEnd());
  if (all.length === 1 && !all[0]) return '';
  if (all.length <= lines) return all.join('\n');
  return `${all.slice(0, Math.max(1, lines)).join('\n').trimEnd()}…`;
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
 * Uma `sending` salva foi interrompida (o app fechou antes da resposta) e volta como `failed`, exceto as de `inFlight`: o pedido delas
 * ainda está a caminho nesta execução do app (o projeto foi trocado e reaberto no meio do envio) e elas continuam `sending`. */
export function readStoredIssues(text: string | null, inFlight: ReadonlySet<string> = new Set()): StoredIssues {
  return parseStored(text, (id) => inFlight.has(id));
}

/** O estado salvo depois da resposta de uma criação: sem `error`, a criação chegou ao GitHub e sai; com `error`, fica `failed` com a mensagem.
 * As outras `sending` ficam como estão (podem estar a caminho). Criação que não está no texto (descartada no meio do envio) não volta. */
export function settleStored(text: string | null, localId: string, error?: string): StoredIssues {
  const stored = parseStored(text, () => true);
  const pending = error === undefined
    ? stored.pending.filter((p) => p.localId !== localId)
    : stored.pending.map((p) => (p.localId === localId ? { ...p, status: 'failed' as const, error } : p));
  return { ...stored, pending };
}

/** Nada a guardar: sem criações, sem em andamento e sem próxima (a chave sai do localStorage). */
export const storedEmpty = (stored: StoredIssues) => !stored.pending.length && !stored.active && stored.next === null;

/** Faz de `number` a em andamento. `since` só recomeça quando muda de issue; se ela era a próxima, deixa de ser. */
export function startTracked(t: Tracked, number: number, now: number): Tracked {
  const active = t.active?.number === number ? t.active : { number, since: now };
  return { active, next: t.next === number ? null : t.next };
}

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

const isNumber = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v > 0;

function parseStored(text: string | null, keepSending: (localId: string) => boolean): StoredIssues {
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
    const { localId, title, body, labels, createdAt, status, error } = p;
    if (typeof localId !== 'string' || !localId || seen.has(localId) || typeof title !== 'string' || typeof body !== 'string') continue;
    if (!isStrings(labels) || typeof createdAt !== 'number' || !Number.isFinite(createdAt)) continue;
    if (status !== 'sending' && status !== 'failed') continue;
    if (error !== undefined && typeof error !== 'string') continue;
    seen.add(localId);
    const base = { localId, title, body, labels: [...labels], createdAt };
    if (status === 'sending' && keepSending(localId)) {
      pending.push({ ...base, status: 'sending' });
      continue;
    }
    const message = status === 'sending' ? 'O envio foi interrompido antes da resposta do GitHub.' : error;
    pending.push({ ...base, status: 'failed', ...(message ? { error: message } : {}) });
  }
  // Em andamento e próxima inválidas valem null; a próxima igual à em andamento também.
  const { active: a, next: n } = data as { active?: unknown; next?: unknown };
  const { number, since } = (a && typeof a === 'object' ? a : {}) as { number?: unknown; since?: unknown };
  const active = isNumber(number) && typeof since === 'number' && Number.isFinite(since) ? { number, since } : null;
  const next = isNumber(n) && n !== active?.number ? n : null;
  return { v: 1, pending, active, next };
}
