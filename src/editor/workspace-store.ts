// Buffers, rascunhos, conflitos e recuperação do editor. Sem React e sem disco: quem chama lê e grava.
// Um buffer é do arquivo (`docId`), não do caminho: renomear só troca `path`, e as duas vistas usam o mesmo texto.
// O texto gravado é `raw` (como veio do disco, sem BOM); `draft` é o que o CodeMirror mostra, com `\n`.
// Sujo é `draft` diferente do `raw` normalizado. Conflito é sujo quando o texto difere do que foi carregado (`origin`).
// Só usa sintaxe apagável (sem enum/namespace), para o check rodar direto no Node.
import { asArray, isRecord, isSafeFilePath, isSafePath, isWithin } from './workspace-model.ts';

export const RECOVERY_VERSION = 1;
// Limites da recuperação no localStorage: texto de um rascunho e total. Passou disso, rascunho grande fica de fora (com aviso).
export const MAX_DRAFT_CHARS = 2_000_000;
export const MAX_RECOVERY_CHARS = 4_000_000;
const MAX_BUFFERS = 50;

export type Newline = '\n' | '\r\n' | '\r';
export type Cursor = { line: number; col: number };
// Rolagem do editor (pixels, como o scrollTop/scrollLeft do DOM). Guardada junto do cursor, por arquivo.
export type Scroll = { top: number; left: number };
// `path` é '' para rascunho (sem arquivo no disco ainda): raw é '' e a gravação vai pelo "Salvar como".
export type Buffer = {
  docId: string; path: string; raw: string; bom: boolean; newline: Newline; draft: string; readOnly: boolean;
};
// `revision` é a da pasta do Git que a tela leu; `origin` é o texto carregado (para saber se o rascunho mudou).
export type ConflictDraft = { path: string; origin: string; draft: string; revision: string };
export type EditorData = { buffers: Record<string, Buffer>; conflicts: Record<string, ConflictDraft> };

export const normalizeNewlines = (text: string) => (text.includes('\r') ? text.replace(/\r\n|\r/g, '\n') : text);
export const newlineOf = (text: string): Newline => (text.includes('\r\n') ? '\r\n' : text.includes('\r') ? '\r' : '\n');
export const toDiskNewlines = (text: string, newline: Newline) => (newline === '\n' ? text : text.replace(/\n/g, newline));
export const conflictKey = (path: string) => `conflict:${path.toLowerCase()}`;
// Compara com o `raw` normalizado sem copiá-lo: roda a cada render, para todos os buffers. Buffer é imutável, então o resultado
// fica guardado por objeto: só o buffer editado é recalculado a cada tecla.
const dirtyMemo = new WeakMap<Buffer, boolean>();
export function isBufferDirty(buffer: Buffer): boolean {
  let dirty = dirtyMemo.get(buffer);
  if (dirty === undefined) {
    dirty = differsFromRaw(buffer);
    dirtyMemo.set(buffer, dirty);
  }
  return dirty;
}
function differsFromRaw({ raw, draft }: Buffer): boolean {
  if (!raw.includes('\r')) return draft !== raw;
  let j = 0;
  for (let i = 0; i < raw.length; i++, j++) {
    let code = raw.charCodeAt(i);
    if (code === 13) {
      if (raw.charCodeAt(i + 1) === 10) i++;
      code = 10;
    }
    if (draft.charCodeAt(j) !== code) return true;
  }
  return j !== draft.length;
}
export const isConflictDirty = (draft: ConflictDraft) => draft.draft !== draft.origin;

/** Chaves sujas, no formato que o workspace-model usa: docId para arquivo, `conflict:<path>` para conflito. */
export function dirtyKeys(data: EditorData): Set<string> {
  const keys = new Set<string>();
  for (const buffer of Object.values(data.buffers)) if (isBufferDirty(buffer)) keys.add(buffer.docId);
  for (const draft of Object.values(data.conflicts)) if (isConflictDirty(draft)) keys.add(conflictKey(draft.path));
  return keys;
}

/** Há rascunho (de arquivo aberto ou de conflito) neste caminho, ou dentro dele se for pasta? Bloqueia descarte no Git. */
// Rascunho (path '') não está no projeto: nunca bloqueia operação Git.
export function hasDraftAt(data: EditorData, path: string): boolean {
  return Object.values(data.buffers).some((b) => b.path !== '' && isBufferDirty(b) && isWithin(b.path, path))
    || Object.values(data.conflicts).some((c) => isConflictDirty(c) && isWithin(c.path, path));
}

/** Rename ou movimento de pasta: troca o `path` dos buffers e dos conflitos dentro de `from`, sem tocar no texto nem no `docId`. */
export function remapData(data: EditorData, from: string, to: string): EditorData {
  if (!isSafeFilePath(from) || !isSafeFilePath(to)) return data;
  const move = (path: string) => (isWithin(path, from) ? to + path.slice(from.length) : path);
  const buffers: Record<string, Buffer> = {};
  for (const [id, buffer] of Object.entries(data.buffers)) buffers[id] = { ...buffer, path: move(buffer.path) };
  const conflicts: Record<string, ConflictDraft> = {};
  for (const draft of Object.values(data.conflicts)) {
    const path = move(draft.path);
    conflicts[conflictKey(path)] = { ...draft, path };
  }
  return { buffers, conflicts };
}

// ---- Recuperação (localStorage, por projeto). Nunca grava no disco. ----

export type RecoveryBuffer = { docId: string; path: string; cursor: Cursor | null; scroll: Scroll | null; dirty: Pick<Buffer, 'raw' | 'bom' | 'newline' | 'draft' | 'readOnly'> | null };
// `split` é a proporção do grupo A (0,15 a 0,85) e `layout` é o texto opaco do layout externo (o App decide o formato).
export type Recovery = { workspace: string | null; layout: string | null; split: number | null; buffers: RecoveryBuffer[]; conflicts: ConflictDraft[] };
export type Recovered = { recovery: Recovery; notes: string[] };

const RECOVERY_PREFIX = 'orquestrador.editor:';
export const NO_PROJECT = '__sem-projeto__';
/** Grafia única do caminho do projeto: `D:\Proj`, `d:/proj` e `D:\Proj\.` dão o mesmo texto. */
export const normalizeProject = (project: string) => project.toLowerCase().replace(/\//g, '\\').replace(/(\\\.?)+$/, '');
/** Chave da recuperação no localStorage; sem projeto, a chave fixa. */
export const recoveryKey = (project: string) => RECOVERY_PREFIX + (project ? normalizeProject(project) : NO_PROJECT);

export type RecoveryStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem' | 'key' | 'length'>;

/**
 * Lê a recuperação pela chave normalizada. Sem ela, procura uma chave antiga (caminho cru, em qualquer grafia; a do
 * caminho atual primeiro), copia para a nova e apaga a antiga. Erro de leitura sobe; erro na cópia mantém a antiga.
 */
export function readRecovery(storage: RecoveryStorage, project: string): string | null {
  const key = recoveryKey(project);
  const text = storage.getItem(key);
  if (text !== null || !project) return text;
  const target = normalizeProject(project);
  const old = [RECOVERY_PREFIX + project];
  for (let i = 0; i < storage.length; i++) {
    const k = storage.key(i);
    if (k?.startsWith(RECOVERY_PREFIX) && k !== key && normalizeProject(k.slice(RECOVERY_PREFIX.length)) === target) old.push(k);
  }
  for (const k of old) {
    if (k === key) continue;
    const found = storage.getItem(k);
    if (found === null) continue;
    try {
      storage.setItem(key, found);
      storage.removeItem(k);
    } catch {
      // Sem espaço para copiar: usa o texto e deixa a chave antiga como está.
    }
    return found;
  }
  return null;
}

/**
 * Monta o texto salvo. Só guarda texto de rascunho sujo; arquivo limpo volta do disco. Se passar do limite total,
 * descarta rascunhos a partir do fim e diz quantos ficaram de fora (`notes`).
 */
export function encodeRecovery(project: string, recovery: Recovery): { text: string; notes: string[] } {
  const notes: string[] = [];
  // Abas além do limite não entram na recuperação: se alguma tinha rascunho, o aviso diz quantas.
  const omitted = recovery.buffers.slice(MAX_BUFFERS).filter((b) => b.dirty).length;
  if (omitted > 0) notes.push(`Mais de ${MAX_BUFFERS} abas: rascunhos de ${omitted} não foram guardados.`);
  const buffers = recovery.buffers.slice(0, MAX_BUFFERS).map((b) => {
    if (b.dirty && b.dirty.draft.length > MAX_DRAFT_CHARS) {
      notes.push(`Rascunho de ${b.path} grande demais para guardar.`);
      return { ...b, dirty: null };
    }
    return b;
  });
  const conflicts = recovery.conflicts.filter((c) => {
    if (c.draft.length > MAX_DRAFT_CHARS) notes.push(`Rascunho do conflito de ${c.path} grande demais para guardar.`);
    return c.draft.length <= MAX_DRAFT_CHARS;
  });
  let keepBuffers = buffers;
  let keepConflicts = conflicts;
  const build = () => JSON.stringify({
    version: RECOVERY_VERSION, project, workspace: recovery.workspace, layout: recovery.layout, split: recovery.split,
    buffers: keepBuffers, conflicts: keepConflicts,
  });
  let text = build();
  // Estouro: tira primeiro os rascunhos de conflito, depois os de arquivo, de trás para frente, até caber.
  while (text.length > MAX_RECOVERY_CHARS && (keepConflicts.length > 0 || keepBuffers.some((b) => b.dirty))) {
    if (keepConflicts.length > 0) keepConflicts = keepConflicts.slice(0, -1);
    else {
      const at = keepBuffers.findLastIndex((b) => b.dirty !== null);
      keepBuffers = keepBuffers.map((b, i) => (i === at ? { ...b, dirty: null } : b));
    }
    notes.push('Espaço de recuperação cheio: alguns rascunhos não foram guardados.');
    text = build();
  }
  if (text.length > MAX_RECOVERY_CHARS) {
    text = JSON.stringify({ version: RECOVERY_VERSION, project, workspace: recovery.workspace, layout: recovery.layout, split: recovery.split, buffers: [], conflicts: [] });
  }
  return { text, notes: [...new Set(notes)] };
}

/** Lê o texto salvo com desconfiança: versão, projeto, caminhos seguros, tamanhos e tipos. O que não passa é descartado com aviso. */
export function decodeRecovery(text: string, project: string): Recovered {
  const empty = (note?: string): Recovered => ({
    recovery: { workspace: null, layout: null, split: null, buffers: [], conflicts: [] },
    notes: note ? [note] : [],
  });
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return empty('Recuperação ilegível: começou vazia.');
  }
  if (!isRecord(raw) || raw.version !== RECOVERY_VERSION) return empty('Recuperação de outra versão: começou vazia.');
  if (typeof raw.project !== 'string' || normalizeProject(raw.project) !== normalizeProject(project)) return empty();
  const notes: string[] = [];
  const buffers: RecoveryBuffer[] = [];
  const docIds = new Set<string>();
  for (const entry of asArray(raw.buffers).slice(0, MAX_BUFFERS)) {
    if (!isRecord(entry) || typeof entry.docId !== 'string' || entry.docId === '' || entry.docId.length > 128 || docIds.has(entry.docId)
      || (entry.path !== '' && !isSafeFilePath(entry.path)) || typeof entry.path !== 'string') {
      notes.push('Aba de arquivo inválida descartada.');
      continue;
    }
    docIds.add(entry.docId);
    const dirty = parseDirty(entry.dirty);
    if (entry.dirty !== undefined && entry.dirty !== null && !dirty) notes.push(`Rascunho de ${entry.path} inválido descartado.`);
    buffers.push({ docId: entry.docId, path: entry.path, cursor: parseCursor(entry.cursor), scroll: parseScroll(entry.scroll), dirty });
  }
  const conflicts: ConflictDraft[] = [];
  const paths = new Set<string>();
  for (const entry of asArray(raw.conflicts)) {
    if (!isRecord(entry) || !isSafePath(entry.path) || paths.has(entry.path.toLowerCase())
      || typeof entry.origin !== 'string' || typeof entry.draft !== 'string' || typeof entry.revision !== 'string'
      || entry.draft.length > MAX_DRAFT_CHARS || entry.origin.length > MAX_DRAFT_CHARS) {
      notes.push('Rascunho de conflito inválido descartado.');
      continue;
    }
    paths.add(entry.path.toLowerCase());
    conflicts.push({ path: entry.path, origin: entry.origin, draft: entry.draft, revision: entry.revision });
  }
  return {
    recovery: {
      workspace: typeof raw.workspace === 'string' && raw.workspace.length <= 1_000_000 ? raw.workspace : null,
      layout: typeof raw.layout === 'string' && raw.layout.length <= 10_000 ? raw.layout : null,
      split: typeof raw.split === 'number' && raw.split >= 0.15 && raw.split <= 0.85 ? raw.split : null,
      buffers,
      conflicts,
    },
    notes,
  };
}

function parseDirty(value: unknown): RecoveryBuffer['dirty'] {
  if (!isRecord(value)) return null;
  if (typeof value.raw !== 'string' || typeof value.draft !== 'string' || typeof value.bom !== 'boolean' || typeof value.readOnly !== 'boolean') return null;
  if (value.raw.length > MAX_DRAFT_CHARS || value.draft.length > MAX_DRAFT_CHARS) return null;
  if (value.newline !== '\n' && value.newline !== '\r\n' && value.newline !== '\r') return null;
  return { raw: value.raw, bom: value.bom, newline: value.newline, draft: value.draft, readOnly: value.readOnly };
}

function parseCursor(value: unknown): Cursor | null {
  if (!isRecord(value)) return null;
  const ok = (n: unknown): n is number => typeof n === 'number' && Number.isSafeInteger(n) && n >= 1;
  return ok(value.line) && ok(value.col) ? { line: value.line, col: value.col } : null;
}

function parseScroll(value: unknown): Scroll | null {
  if (!isRecord(value)) return null;
  const ok = (n: unknown): n is number => typeof n === 'number' && Number.isFinite(n) && n >= 0 && n <= 1e8;
  return ok(value.top) && ok(value.left) ? { top: value.top, left: value.left } : null;
}

// ---- Conflitos: marcadores normais e diff3. Trabalha com linhas `\n`, como o texto que o backend devolve. ----

export type ConflictBlock = {
  ours: string[]; base: string[] | null; theirs: string[];
  // Linhas de marcador como estavam, para devolver um bloco pendente sem perder o texto original.
  head: string; baseHead: string | null; mid: string; tail: string;
};
export type ConflictPiece = { line: string } | { block: ConflictBlock };
export type Choice = 'ours' | 'theirs' | 'both' | 'pending';
const MARKER = /^(<{7}|\|{7}|={7}|>{7})(\s|$)/;
// Exatamente sete caracteres: `=========` (sublinhado de Markdown) é texto. `\s` aceita o `\r` de linhas CRLF.
const isMarker = (line: string, char: string) => MARKER.test(line) && line.startsWith(char.repeat(7));

/** Quebra o texto em linhas e blocos. `malformed` quando um bloco não fecha: aí a tela não oferece ação por trecho. */
export function parseConflict(text: string): { pieces: ConflictPiece[]; malformed: boolean } {
  const lines = text.split('\n');
  const pieces: ConflictPiece[] = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!isMarker(line, '<')) {
      pieces.push({ line });
      continue;
    }
    const head = line;
    const ours: string[] = [];
    let baseHead: string | null = null;
    let base: string[] | null = null;
    let j = i + 1;
    while (j < lines.length && !isMarker(lines[j], '=') && !isMarker(lines[j], '|')) ours.push(lines[j++]);
    if (j < lines.length && isMarker(lines[j], '|')) {
      baseHead = lines[j++];
      base = [];
      while (j < lines.length && !isMarker(lines[j], '=')) base.push(lines[j++]);
    }
    if (j >= lines.length) return { pieces, malformed: true };
    const mid = lines[j++];
    const theirs: string[] = [];
    while (j < lines.length && !isMarker(lines[j], '>')) theirs.push(lines[j++]);
    if (j >= lines.length) return { pieces, malformed: true };
    pieces.push({ block: { ours, base, theirs, head, baseHead, mid, tail: lines[j] } });
    i = j;
  }
  return { pieces, malformed: false };
}

/** Texto final: cada bloco vira o lado escolhido; `pending` devolve o bloco com os marcadores originais. */
export function renderConflict(pieces: ConflictPiece[], choices: readonly Choice[]): string {
  const out: string[] = [];
  let n = 0;
  for (const piece of pieces) {
    if ('line' in piece) {
      out.push(piece.line);
      continue;
    }
    const block = piece.block;
    const choice = choices[n++] ?? 'pending';
    if (choice === 'ours') out.push(...block.ours);
    else if (choice === 'theirs') out.push(...block.theirs);
    else if (choice === 'both') out.push(...block.ours, ...block.theirs);
    else {
      out.push(block.head, ...block.ours);
      if (block.baseHead !== null) out.push(block.baseHead, ...(block.base ?? []));
      out.push(block.mid, ...block.theirs, block.tail);
    }
  }
  return out.join('\n');
}

/** Quantos blocos há, na ordem em que `renderConflict` os espera. */
export const blockCount = (pieces: ConflictPiece[]) => pieces.filter((p) => 'block' in p).length;

/** Marcador de conflito ainda no texto (linha que começa com 7 `<`, `|`, `=` ou `>` seguido de espaço ou fim). */
export const hasMarkers = (text: string) => text.split('\n').some((line) => MARKER.test(line));

// ---- Blocos de diff (um patch de um arquivo: cabeçalho e `@@`). ----

export type PatchHunk = { header: string; text: string };

/** Separa o patch nos blocos `@@`, na ordem do backend. O texto de cada bloco é o que a tela mostra ao confirmar um descarte. */
export function patchHunks(patch: string): PatchHunk[] {
  const lines = patch.split('\n');
  const hunks: PatchHunk[] = [];
  let current: string[] | null = null;
  const flush = () => {
    if (current) hunks.push({ header: current[0], text: current.join('\n') });
  };
  for (const line of lines) {
    if (line.startsWith('@@')) {
      flush();
      current = [line];
    } else if (current) {
      current.push(line);
    }
  }
  flush();
  return hunks;
}
