// Modelo puro do espaço de trabalho do editor: um workspace com até dois grupos lado a lado, cada um com abas e prévia.
// Sem React, sem armazenamento e sem drafts: quem chama guarda os buffers e diz quais documentos estão sujos.
// Só usa sintaxe apagável (sem enum/namespace), para o check rodar direto no Node.
//
// Contratos (para quem integrar na UI):
// - Cada aba tem `id` (próprio da aba, único no workspace) e `target` (a identidade). A aba de arquivo é identificada por `docId`,
//   escolhido pelo integrador e nunca trocado. O `path` é só onde o arquivo está agora: `remapPath` o atualiza sem mexer no docId.
//   Por isso renomear/mover não desmonta o buffer, e duas abas com o mesmo docId (em grupos diferentes) compartilham o buffer.
// - Diff é identificado por lado + path (`staged`/`unstaged`); histórico, por hash + path; conflito, por path.
// - Dentro de um grupo, uma identidade aparece no máximo uma vez: abrir de novo só ativa a que já existe.
// - Ordem do grupo: fixadas primeiro, depois as demais. No máximo uma prévia por grupo. Fixada nunca é prévia.
// - Cada operação devolve um estado novo; o argumento não muda. Operação inválida (grupo ou aba inexistente) devolve o mesmo estado.
// - Fechamento em duas etapas: `planClose` diz o que sairia e quais alvos estão sujos (`unsaved`, ids de aba). O integrador decide
//   (confirmar ou cancelar). Só `closeTabs` fecha: cancelar é simplesmente não chamá-lo. Se o estado mudou durante a confirmação,
//   chame `planClose` de novo.
// - Ativa: `active` é null só quando o grupo não tem abas; senão aponta para uma aba do grupo. `focus` é o grupo com o foco.
// - Fechar a ativa passa a ativa para a vizinha da direita entre as que ficam; sem ela, a da esquerda.
// - `loadWorkspace(texto)` valida o JSON salvo (versão, paths relativos seguros, até 2 grupos, ids, prévias, ativas) e conserta o
//   que for inválido, descartando o que não couber. Nunca abre nem lê path nenhum: só confere texto.

export const WORKSPACE_VERSION = 1;
export const MAX_GROUPS = 2;
const MAX_CLOSED = 20;

export type GroupId = 'A' | 'B';
export type DiffSide = 'staged' | 'unstaged';
export type TabTarget =
  | { kind: 'file'; docId: string; path: string }
  | { kind: 'diff'; side: DiffSide; path: string }
  | { kind: 'commit'; hash: string; path: string }
  | { kind: 'conflict'; path: string };
export type EditorTab = { id: string; target: TabTarget; pinned: boolean; preview: boolean };
export type EditorGroup = { id: GroupId; tabs: EditorTab[]; active: string | null };
export type ClosedTab = { group: GroupId; target: TabTarget };
export type WorkspaceState = {
  version: typeof WORKSPACE_VERSION;
  groups: EditorGroup[]; // ordem = esquerda → direita
  focus: GroupId; // grupo com o foco do teclado
  recent: string[]; // ids das abas, a usada por último primeiro
  closed: ClosedTab[]; // a última fechada é a última da lista
  nextId: number; // próximo número para `tab-<n>`
};
export type CloseScope =
  | { kind: 'one'; tabId: string } // X / Fechar / Ctrl+F4: fecha inclusive fixada
  | { kind: 'others' | 'right'; tabId: string } // lote: nunca alcança fixada
  | { kind: 'saved' | 'all'; group: GroupId }; // lote: nunca alcança fixada
export type ClosePlan = { tabIds: string[]; unsaved: string[] };
export type Loaded = { state: WorkspaceState; notes: string[] };

const GROUP_IDS: GroupId[] = ['A', 'B'];
const NO_DIRTY: ReadonlySet<string> = new Set();

export function createWorkspace(): WorkspaceState {
  return { version: WORKSPACE_VERSION, groups: [{ id: 'A', tabs: [], active: null }], focus: 'A', recent: [], closed: [], nextId: 1 };
}

/** Identidade estável da aba: a mesma chave significa o mesmo documento/diff/commit/conflito. Diff e histórico diferenciam lado e hash. Sem diferença de caixa, como no Windows. */
export function targetKey(target: TabTarget): string {
  switch (target.kind) {
    case 'file': return `file:${target.docId}`;
    case 'diff': return `diff:${target.side}:${target.path.toLowerCase()}`;
    case 'commit': return `commit:${target.hash.toLowerCase()}:${target.path.toLowerCase()}`;
    case 'conflict': return `conflict:${target.path.toLowerCase()}`;
  }
}

/** Abre (ou ativa) uma prévia no grupo. Substitui a prévia limpa; se a prévia atual está suja (`dirty`, docIds), ela vira permanente e a nova entra depois. */
export function openPreview(state: WorkspaceState, group: GroupId, target: TabTarget, dirty: ReadonlySet<string>): WorkspaceState {
  return openAt(state, group, target, true, dirty);
}

/** Abre de vez (duplo clique, ou abrir a partir de outra ação). Se a identidade já é prévia no grupo, ela vira permanente. */
export function openPermanent(state: WorkspaceState, group: GroupId, target: TabTarget): WorkspaceState {
  return openAt(state, group, target, false, NO_DIRTY);
}

/** Ativa a aba, no grupo dela, e leva o foco para lá. */
export function activate(state: WorkspaceState, tabId: string): WorkspaceState {
  const found = locate(state, tabId);
  if (!found) return state;
  return tidy({
    ...setGroup(state, { ...found.group, active: tabId }),
    focus: found.group.id,
    recent: [tabId, ...state.recent.filter((id) => id !== tabId)],
  });
}

/** Editar torna a aba permanente. */
export function markEdited(state: WorkspaceState, tabId: string): WorkspaceState {
  return updateTab(state, tabId, (tab) => ({ ...tab, preview: false }));
}

/** Fixa ou desafixa. Fixar torna a aba permanente; a fixada vai para o começo do grupo. */
export function togglePin(state: WorkspaceState, tabId: string): WorkspaceState {
  return updateTab(state, tabId, (tab) => ({ ...tab, pinned: !tab.pinned, preview: false }));
}

/** O que um fechamento sairia, sem fechar nada. `unsaved` são os alvos sujos (ids de aba) para o integrador confirmar. */
export function planClose(state: WorkspaceState, scope: CloseScope, dirty: ReadonlySet<string>): ClosePlan {
  const group = 'tabId' in scope
    ? locate(state, scope.tabId)?.group
    : state.groups.find((g) => g.id === scope.group);
  if (!group) return { tabIds: [], unsaved: [] };
  const tabs = group.tabs;
  const at = tabs.findIndex((t) => t.id === ('tabId' in scope ? scope.tabId : ''));
  let picked: EditorTab[] = [];
  switch (scope.kind) {
    case 'one': picked = tabs.filter((t) => t.id === scope.tabId); break;
    case 'others': picked = tabs.filter((t, i) => i !== at && !t.pinned); break;
    case 'right': picked = tabs.filter((t, i) => i > at && !t.pinned); break;
    case 'saved': picked = tabs.filter((t) => !t.pinned && !isDirty(t.target, dirty)); break;
    case 'all': picked = tabs.filter((t) => !t.pinned); break;
  }
  return {
    tabIds: picked.map((t) => t.id),
    unsaved: picked.filter((t) => isDirty(t.target, dirty)).map((t) => t.id),
  };
}

/** Fecha as abas (só as que existem). Guarda cada uma na pilha de fechadas e escolhe a vizinha ativa. Quem chama já decidiu sobre os rascunhos. */
export function closeTabs(state: WorkspaceState, tabIds: readonly string[]): WorkspaceState {
  const closing = new Set(tabIds);
  const closed: ClosedTab[] = [];
  const groups = state.groups.map((group) => {
    for (const tab of group.tabs) if (closing.has(tab.id)) closed.push({ group: group.id, target: tab.target });
    return dropFromGroup(group, closing);
  });
  return tidy({
    ...state,
    groups,
    closed: [...state.closed, ...closed],
    recent: state.recent.filter((id) => !closing.has(id)),
  });
}

/** Reabre a última aba fechada no grupo de origem (ou no primeiro, se ele sumiu), como permanente e ativa. Se a identidade já está aberta lá, só ativa. */
export function reopenClosed(state: WorkspaceState): WorkspaceState {
  const last = state.closed[state.closed.length - 1];
  if (!last) return state;
  const rest: WorkspaceState = { ...state, closed: state.closed.slice(0, -1) };
  const group = rest.groups.find((g) => g.id === last.group) ?? rest.groups[0];
  return openAt(rest, group.id, last.target, false, NO_DIRTY);
}

/** Anterior (-1) ou próxima (1) aba do grupo focado; dá a volta no fim e no começo. */
export function navigate(state: WorkspaceState, step: 1 | -1): WorkspaceState {
  const group = state.groups.find((g) => g.id === state.focus);
  if (!group || group.tabs.length === 0) return state;
  const n = group.tabs.length;
  const at = group.tabs.findIndex((t) => t.id === group.active);
  const from = at < 0 ? (step > 0 ? -1 : n) : at;
  return activate(state, group.tabs[(from + step + n) % n].id);
}

/** MRU: volta para a aba usada antes da atual (a segunda da lista de recentes). */
export function activateRecent(state: WorkspaceState): WorkspaceState {
  const previous = state.recent[1];
  return previous === undefined ? state : activate(state, previous);
}

/** Move a aba para outro grupo (mesmo id, mesma identidade) e a torna ativa lá. Cria o grupo se ele ainda não existe e couber. Recusa se a identidade já está no destino. */
export function moveTab(state: WorkspaceState, tabId: string, to: GroupId, index?: number): WorkspaceState {
  const found = locate(state, tabId);
  if (!found) return state;
  if (found.group.id === to) return index === undefined ? state : reorderTab(state, tabId, index);
  let base = state;
  if (!base.groups.some((g) => g.id === to)) {
    if (base.groups.length >= MAX_GROUPS) return state;
    base = { ...base, groups: [...base.groups, { id: to, tabs: [], active: null }] };
  }
  const dest = base.groups.find((g) => g.id === to);
  const key = targetKey(found.tab.target);
  if (!dest || dest.tabs.some((t) => targetKey(t.target) === key)) return state;
  const tabs = [...dest.tabs];
  tabs.splice(clamp(index ?? tabs.length, tabs.length), 0, found.tab);
  const next = setGroup(base, dropFromGroup(found.group, new Set([tabId])));
  return tidy(activate(setGroup(next, { ...dest, tabs, active: tabId }), tabId));
}

/** Muda a aba de posição dentro do grupo. As fixadas continuam no começo. */
export function reorderTab(state: WorkspaceState, tabId: string, index: number): WorkspaceState {
  const found = locate(state, tabId);
  if (!found) return state;
  const tabs = found.group.tabs.filter((t) => t.id !== tabId);
  tabs.splice(clamp(index, tabs.length), 0, found.tab);
  return tidy(setGroup(state, { ...found.group, tabs }));
}

/** Abre uma cópia da aba no outro grupo (criando-o se preciso), mesma identidade e id novo. A aba original fica onde está. */
export function splitTab(state: WorkspaceState, tabId: string): WorkspaceState {
  const found = locate(state, tabId);
  const otherId = GROUP_IDS.find((id) => id !== found?.group.id);
  if (!found || !otherId) return state;
  const base = state.groups.some((g) => g.id === otherId)
    ? state
    : { ...state, groups: [...state.groups, { id: otherId, tabs: [], active: null }] };
  return openAt(base, otherId, found.tab.target, false, NO_DIRTY);
}

/** Une o segundo grupo ao primeiro. Identidade repetida fica uma vez (a do primeiro); fixada ou permanente vence. Mantém a ativa do primeiro. */
export function joinGroups(state: WorkspaceState): WorkspaceState {
  if (state.groups.length < 2) return state;
  const [first, second] = state.groups;
  const tabs = [...first.tabs];
  for (const tab of second.tabs) {
    const same = tabs.findIndex((t) => targetKey(t.target) === targetKey(tab.target));
    if (same < 0) {
      tabs.push(tab);
    } else {
      tabs[same] = { ...tabs[same], pinned: tabs[same].pinned || tab.pinned, preview: tabs[same].preview && tab.preview };
    }
  }
  return tidy({ ...state, groups: [{ ...first, tabs }], focus: first.id });
}

/** Fecha um grupo vazio. Não fecha grupo com abas (feche ou mova antes) e nunca o último. O foco vai para o grupo que sobra. */
export function closeGroup(state: WorkspaceState, groupId: GroupId): WorkspaceState {
  const group = state.groups.find((g) => g.id === groupId);
  if (!group || group.tabs.length > 0 || state.groups.length < 2) return state;
  const groups = state.groups.filter((g) => g.id !== groupId);
  return tidy({ ...state, groups, focus: state.focus === groupId ? groups[0].id : state.focus });
}

/**
 * Remapeia um prefixo de path (arquivo ou pasta) depois de renomear/mover: vale para abas de arquivo, diff, histórico e conflito,
 * abertas ou fechadas. Compara por pasta inteira e sem diferença de caixa: `src` não pega `src2`, e `SRC/x` vira `novo/x`.
 */
export function remapPath(state: WorkspaceState, from: string, to: string): WorkspaceState {
  if (!isSafePath(from) || !isSafePath(to)) return state;
  const move = (path: string) => (within(path, from) ? to + path.slice(from.length) : path);
  // Histórico é imutável: o path de um commit é o do próprio commit e não muda. Abrir o arquivo atual sai de outro localizador.
  const retarget = (target: TabTarget): TabTarget => (target.kind === 'commit' ? target : { ...target, path: move(target.path) });
  return tidy({
    ...state,
    groups: state.groups.map((g) => ({ ...g, tabs: g.tabs.map((t) => ({ ...t, target: retarget(t.target) })) })),
    closed: state.closed.map((c) => ({ ...c, target: retarget(c.target) })),
  });
}

/** Lê o JSON salvo e devolve um estado seguro. Versão desconhecida ou JSON quebrado começam vazios. `notes` diz o que foi descartado. */
export function loadWorkspace(text: string): Loaded {
  const fresh = (note: string): Loaded => ({ state: createWorkspace(), notes: [note] });
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return fresh('JSON inválido: o espaço de trabalho começou vazio.');
  }
  if (!isRecord(raw) || raw.version !== WORKSPACE_VERSION) return fresh('Versão desconhecida: o espaço de trabalho começou vazio.');
  const notes: string[] = [];
  const groups: EditorGroup[] = [];
  const groupIds = new Set<GroupId>();
  const tabIds = new Set<string>();
  for (const group of asArray(raw.groups)) {
    if (!isRecord(group) || !isGroupId(group.id) || groupIds.has(group.id) || groups.length === MAX_GROUPS) {
      notes.push('Grupo inválido ou excedente descartado.');
      continue;
    }
    groupIds.add(group.id);
    const tabs: EditorTab[] = [];
    for (const tab of asArray(group.tabs)) {
      if (!isRecord(tab) || typeof tab.id !== 'string' || tab.id === '' || tab.id.length > 64 || tabIds.has(tab.id) || !isSafeTarget(tab.target)) {
        notes.push('Aba inválida descartada.');
        continue;
      }
      tabIds.add(tab.id);
      tabs.push({ id: tab.id, target: tab.target, pinned: tab.pinned === true, preview: tab.preview === true });
    }
    groups.push({ id: group.id, tabs, active: typeof group.active === 'string' ? group.active : null });
  }
  const recent = asArray(raw.recent).filter((id): id is string => typeof id === 'string');
  const closed: ClosedTab[] = [];
  for (const entry of asArray(raw.closed)) {
    if (isRecord(entry) && isGroupId(entry.group) && isSafeTarget(entry.target)) closed.push({ group: entry.group, target: entry.target });
    else notes.push('Aba fechada inválida descartada.');
  }
  const nextId = typeof raw.nextId === 'number' && Number.isSafeInteger(raw.nextId) && raw.nextId >= 1 && raw.nextId < Number.MAX_SAFE_INTEGER ? raw.nextId : 1;
  const state = tidy({ version: WORKSPACE_VERSION, groups, focus: isGroupId(raw.focus) ? raw.focus : 'A', recent, closed, nextId });
  return { state, notes };
}

// ---- internos ----

function openAt(state: WorkspaceState, groupId: GroupId, target: TabTarget, preview: boolean, dirty: ReadonlySet<string>): WorkspaceState {
  const group = state.groups.find((g) => g.id === groupId);
  if (!group) return state;
  const key = targetKey(target);
  const same = group.tabs.find((t) => targetKey(t.target) === key);
  if (same) {
    const kept = !preview && same.preview ? markEdited(state, same.id) : state;
    return activate(kept, same.id);
  }
  const { state: minted, id } = mint(state);
  const tab: EditorTab = { id, target, pinned: false, preview };
  const previewTab = group.tabs.find((t) => t.preview);
  let tabs: EditorTab[];
  if (preview && previewTab && !isDirty(previewTab.target, dirty)) {
    tabs = group.tabs.map((t) => (t === previewTab ? tab : t));
  } else {
    // Prévia suja (ou abertura permanente): a prévia atual, se suja, fica permanente e a nova vem depois.
    const promote = preview ? previewTab : undefined;
    tabs = [...group.tabs.map((t) => (t === promote ? { ...t, preview: false } : t)), tab];
  }
  return activate(setGroup(minted, { ...group, tabs }), id);
}

function mint(state: WorkspaceState): { state: WorkspaceState; id: string } {
  const used = new Set(state.groups.flatMap((g) => g.tabs.map((t) => t.id)));
  // No limite do inteiro seguro volta ao 1; há poucas abas, então sempre sobra um número livre e o laço termina.
  let n = nextSafe(state.nextId);
  while (used.has(`tab-${n}`)) n = nextSafe(n);
  return { state: { ...state, nextId: nextSafe(n) }, id: `tab-${n}` };
}

/** Próximo número de id depois de `n`, dentro do inteiro seguro: no limite, recomeça em 1. */
const nextSafe = (n: number) => (n >= Number.MAX_SAFE_INTEGER - 1 ? 1 : n + 1);

function locate(state: WorkspaceState, tabId: string): { group: EditorGroup; tab: EditorTab } | undefined {
  for (const group of state.groups) {
    const tab = group.tabs.find((t) => t.id === tabId);
    if (tab) return { group, tab };
  }
  return undefined;
}

function setGroup(state: WorkspaceState, next: EditorGroup): WorkspaceState {
  return { ...state, groups: state.groups.map((g) => (g.id === next.id ? next : g)) };
}

function updateTab(state: WorkspaceState, tabId: string, change: (tab: EditorTab) => EditorTab): WorkspaceState {
  const found = locate(state, tabId);
  if (!found) return state;
  return tidy(setGroup(state, { ...found.group, tabs: found.group.tabs.map((t) => (t.id === tabId ? change(t) : t)) }));
}

/** Tira as abas `closing` do grupo; se a ativa sai, a vizinha (direita, depois esquerda) é escolhida antes da remoção. */
function dropFromGroup(group: EditorGroup, closing: ReadonlySet<string>): EditorGroup {
  const active = group.active !== null && closing.has(group.active) ? neighborAfter(group.tabs, group.active, closing) : group.active;
  return { ...group, tabs: group.tabs.filter((t) => !closing.has(t.id)), active };
}

function neighborAfter(tabs: readonly EditorTab[], tabId: string, closing: ReadonlySet<string>): string | null {
  const at = tabs.findIndex((t) => t.id === tabId);
  const right = tabs.slice(at + 1).find((t) => !closing.has(t.id));
  if (right) return right.id;
  return tabs.slice(0, Math.max(at, 0)).reverse().find((t) => !closing.has(t.id))?.id ?? null;
}

/** Conserta o estado: grupos únicos (até 2), uma identidade por grupo, uma prévia por grupo, fixadas no começo, ativa válida, recentes existentes. */
function tidy(state: WorkspaceState): WorkspaceState {
  const seen = new Set<GroupId>();
  const groups: EditorGroup[] = [];
  for (const group of state.groups) {
    if (seen.has(group.id) || groups.length === MAX_GROUPS) continue;
    seen.add(group.id);
    groups.push(tidyGroup(group));
  }
  // Grupo vazio some quando o outro tem abas (a área inteira vai para ele). Se os dois estão vazios, fica um só (sem área morta).
  let kept = groups;
  if (groups.length === 2) {
    const filled = groups.filter((g) => g.tabs.length > 0);
    kept = filled.length === 0 ? groups.slice(0, 1) : filled.length === 1 ? filled : groups;
  }
  if (kept.length === 0) kept.push({ id: 'A', tabs: [], active: null });
  const ids = new Set(kept.flatMap((g) => g.tabs.map((t) => t.id)));
  return {
    version: WORKSPACE_VERSION,
    groups: kept,
    focus: kept.some((g) => g.id === state.focus) ? state.focus : kept[0].id,
    recent: [...new Set(state.recent.filter((id) => ids.has(id)))],
    closed: state.closed.slice(-MAX_CLOSED),
    nextId: Math.max(1, state.nextId),
  };
}

function tidyGroup(group: EditorGroup): EditorGroup {
  const keys = new Set<string>();
  const tabs: EditorTab[] = [];
  let previewSeen = false;
  for (const tab of group.tabs) {
    const key = targetKey(tab.target);
    if (keys.has(key)) continue; // mesma identidade no grupo: fica a primeira
    keys.add(key);
    const preview = tab.preview && !tab.pinned && !previewSeen;
    if (preview) previewSeen = true;
    tabs.push({ ...tab, preview });
  }
  const ordered = [...tabs.filter((t) => t.pinned), ...tabs.filter((t) => !t.pinned)];
  const active = ordered.some((t) => t.id === group.active) ? group.active : (ordered[0]?.id ?? null);
  return { id: group.id, tabs: ordered, active };
}

/** Caminho relativo seguro: sem vazio, sem barra inicial, sem `\`, sem `:` (drive ou stream), sem `.`/`..`, sem controle. */
export function isSafePath(path: unknown): path is string {
  if (typeof path !== 'string' || path.length === 0 || path.length > 1024) return false;
  if (/[\u0000-\u001f\\:]/.test(path) || path.startsWith('/')) return false;
  // `.git` em qualquer caixa, e com ponto/espaço final (o Windows ignora esses), é o metadado do repositório: fica de fora.
  return path.split('/').every((seg) => seg !== '' && seg !== '.' && seg !== '..' && !/^\.git[. ]*$/i.test(seg));
}

function isSafeTarget(value: unknown): value is TabTarget {
  if (!isRecord(value) || !isSafePath(value.path)) return false;
  switch (value.kind) {
    case 'file': return typeof value.docId === 'string' && value.docId !== '' && value.docId.length <= 128;
    case 'diff': return value.side === 'staged' || value.side === 'unstaged';
    case 'commit': return typeof value.hash === 'string' && /^[0-9a-f]{4,64}$/i.test(value.hash);
    case 'conflict': return true;
    default: return false;
  }
}

/** `path` é `base` ou está dentro dele (os dois relativos, com "/"). Sem diferença de caixa; a barra final separa `src` de `src2`. */
function within(path: string, base: string): boolean {
  const p = path.toLowerCase();
  const b = base.toLowerCase();
  return p === b || p.startsWith(`${b}/`);
}

// Sujo: buffer de arquivo com draft (`docId`) ou rascunho de resolução de conflito (`conflict:<path>`, sem diferença de caixa).
// Quem integra mantém essas chaves no conjunto `dirty`.
const isDirty = (target: TabTarget, dirty: ReadonlySet<string>) =>
  target.kind === 'file' ? dirty.has(target.docId) : target.kind === 'conflict' ? dirty.has(`conflict:${target.path.toLowerCase()}`) : false;
const isGroupId = (value: unknown): value is GroupId => value === 'A' || value === 'B';
const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const asArray = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const clamp = (index: number, max: number) => Math.min(Math.max(Math.trunc(index) || 0, 0), max);
