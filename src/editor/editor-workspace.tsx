// Área central do editor: até dois grupos de abas lado a lado. Cada aba é um arquivo (CodeMirror), uma imagem ou um áudio (só leitura,
// `media-view.tsx`), um diff staged/unstaged, um commit do histórico (só leitura; ambos em `diff-view.tsx`) ou um conflito de merge (`conflict-view.tsx`). Buffers e rascunhos são do arquivo (`docId`), não da aba;
// mídia não tem buffer.
//
// API para o App (ref do componente, `EditorWorkspaceHandle`):
//   openFile(path, { permanent?, line?, col?, side? })  Abre o arquivo. Sem `permanent`, vira prévia (um clique). `line`/`col` (1-based, UTF-16) põem o cursor.
//                                                        `side` é o grupo 'A' | 'B' (cria B se couber).
//   openTarget(target, { permanent?, side? })           Abre um TabTarget do modelo (diff, commit, conflito ou arquivo com docId).
//   openBeside(path, { permanent?, line?, col? })       Abre à direita, no segundo grupo (ou no outro, se só há um).
//   remap(from, to)                                     Depois de renomear/mover: troca o caminho de abas e rascunhos. Não remonta buffer.
//   remove(paths)                                       Depois de excluir: fecha as abas e descarta os buffers. Quem chama já decidiu o descarte.
//   hasDraft(path): boolean                             Há rascunho neste caminho (ou dentro, se for pasta). Bloqueia descarte no Git.
//   unsaved(): string[]                                 Os mesmos caminhos, para o integrador perguntar antes de fechar a janela.
//   saveAll(): Promise<boolean>                         Salva arquivos e rascunhos de conflito (conflito é gravado sem marcar resolvido; marcadores podem ficar). Rascunho com texto
//                                                        abre o "Salvar como" um por um. false se algo falhou ou se cancelou.
//   discardAll(): void                                  Descarta todos os rascunhos (volta ao texto do disco/Git) e limpa a recuperação deles.
//   openPaths(): string[]                               Caminhos de todas as abas, para checar colisão antes de criar ou mover.
//   refresh(): void                                     Relê do disco os arquivos abertos que estão limpos.
//   flushRecovery(): void                               Grava a recuperação agora (antes de fechar o app).
//   selectedText(): string                              Texto da seleção principal no arquivo (ou rascunho) da aba ativa do grupo focado. Sem seleção, a palavra sob o cursor.
//                                                        Vazio se não há aba de arquivo ou rascunho, se a seleção tem quebra de linha ou se não há palavra. Não mexe no editor.
// Props: `project` (raiz absoluta, chave da recuperação), `locked` (executores vivos: bloqueia mutações Git), `guard` (trava compartilhada
// com a barra do Git), `refreshGit` (chamado após mutação), `onActiveTarget` (aviso da aba ativa), `layout` (texto opaco do
// layout externo, salvo na recuperação), `onRestoreLayout` (chamado uma vez na montagem com o layout salvo), `onReveal` (revelar no Explorer),
// `onGroupCount` (1 ou 2 grupos), `onSaved()` (chamada depois de cada gravação bem-sucedida no disco pelo editor: Ctrl+S, salvar tudo,
// comparação, resolução de conflito).
// Rascunho ("Sem título-N"): duplo clique na faixa vazia, na área do grupo sem abas ou Ctrl+N abrem um buffer em memória, sem caminho. Salvar
// abre o diálogo "Salvar como"; depois de salvo, vira arquivo do projeto (caminho relativo) ou solto (absoluto).
// Recuperação: localStorage por projeto (`orquestrador.editor:<project normalizado>`) e uma chave fixa no modo sem projeto.
import {
  forwardRef, Fragment, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type FocusEvent, type KeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode,
} from 'react';
import { ActionIcon, Button, ContextMenuTrigger, type ContextMenuItem, DropdownMenu, Modal, Text, toast } from '@lobehub/ui/base-ui';
import { Center, Flexbox } from '@lobehub/ui';
import { Columns2, Ellipsis, FileText, Pin, X } from 'lucide-react';
import { EditorView } from '@codemirror/view';
import { FileIcon } from '../shared/file-icon';
import { AppMark } from '../window/app-mark';
import { Breadcrumbs } from './breadcrumbs';
import { message } from '../backend/backend';
import { useBackend } from '../backend/backend-context';
import { useWorkerPool } from '@pierre/diffs/react';
import { usePolling } from '../shared/polling';
import { CodeEditor, forgetEditorState, preloadLanguage, type EditorRestore, type Reveal } from './file-view';
import { MediaView, mediaOf, Note } from './media-view';
import { canHighlightGitTabDiff, CommitTab, CompareView, DiffTab, gitTabFileDiff, POLL_MS, Reading, type Ask, type Common, type GitTabRead, type GitTabReadResult } from './diff-view';
import { ConflictTab } from './conflict-view';
import { action, blocked, DIVIDER, fileName, useChord } from '../shared/file-tabs';
import { isAbsolutePath, pathText, relativeTo } from '../shared/file-tab-actions';
import { copy, splitPath } from '../shared/paths';
import {
  activate, closeTabs, createWorkspace, draftName, isDirty, isSafePath, isWithin, joinGroups, liveDocIds, loadWorkspace, markEdited, moveTab, navigate,
  openDraft, openPermanent, openPreview, planClose, promoteDraft, remapPath, reopenClosed, retargetDoc, splitTab, targetKey, togglePin, type CloseScope, type EditorTab, type GroupId, type TabTarget, type WorkspaceState,
} from './workspace-model';
import { dragShift, revealTab, trackTabDrag, type TabDrag, wheelToScroll } from '../shared/tab-strip';
import { TabListMenu, type TabListItem } from '../shared/tab-list-menu';
import {
  type Buffer, type ConflictDraft, decodeRecovery, NO_PROJECT, readRecovery, recoveryKey, dirtyKeys, encodeRecovery, hasDraftAt,
  hasMarkers, isBufferDirty, isConflictDirty, newlineOf, normalizeNewlines, type Recovery,
  remapData, toDiskNewlines, conflictKey, type EditorData, type Cursor, type Newline, type Scroll,
} from './workspace-store';
import type { GitGuard } from '../git/git-guard';
import type { FileBase, FileContent } from '../backend/types';

export type OpenOptions = { permanent?: boolean; line?: number; col?: number; side?: GroupId };
export type EditorWorkspaceHandle = {
  newFile(): void;
  saveActive(): Promise<boolean>;
  saveActiveAs(): Promise<boolean>;
  openFile(path: string, options?: OpenOptions): void;
  prefetch(path: string): void;
  prefetchTarget(target: TabTarget, hover?: boolean): void;
  openTarget(target: TabTarget, options?: Pick<OpenOptions, 'permanent' | 'side'>): void;
  /** Abre à direita: com dois grupos, o segundo na ordem visual (mesmo com o foco nele); com um grupo só, o outro id, que entra depois dele (à direita). */
  openBeside(path: string, options?: Pick<OpenOptions, 'permanent' | 'line' | 'col'>): void;
  remap(from: string, to: string): void;
  remove(paths: string[]): void;
  hasDraft(path: string): boolean;
  unsaved(): string[];
  saveAll(): Promise<boolean>;
  discardAll(): void;
  /** Caminhos de todas as abas (arquivo, diff, commit, conflito). Quem move ou cria um item confere colisão com elas antes. */
  openPaths(): string[];
  /** Relê do disco os arquivos abertos que estão limpos (sujos ficam como estão). */
  refresh(): void;
  /** Grava a recuperação agora, sem esperar o atraso. Usado antes de fechar o app. */
  flushRecovery(): void;
  /** Texto da seleção principal (ou a palavra sob o cursor) no arquivo ou rascunho da aba ativa do grupo focado. Vazio se não há o que pesquisar. */
  selectedText(): string;
};
export type EditorWorkspaceProps = {
  project: string;
  active?: boolean;
  locked: boolean;
  refreshGit: () => void;
  /** Trava compartilhada com a barra do Git: as operações Git desta área e da barra não rodam juntas. */
  guard: GitGuard;
  // Alvo da aba ativa no grupo focado (diff, commit, conflito ou arquivo): a sidebar marca o diff que está aberto.
  onActiveTarget?: (target: TabTarget | null) => void;
  layout?: string | null;
  onRestoreLayout?: (layout: string) => void;
  onReveal: (path: string) => void;
  /** Quantos grupos existem (1 ou 2): o App reserva a largura central de cada um. */
  onGroupCount?: (count: number) => void;
  /** Chamada após cada gravação bem-sucedida no disco feita pelo editor. */
  onSaved?: () => void;
};

type Status = { error?: string; binary?: boolean; large?: boolean };
type CachedRead = { promise: Promise<FileContent>; language?: Promise<void>; timer: number };
// Versão do disco que difere de um rascunho sujo: a tela oferece usar o disco ou salvar sobre ele, nunca troca sozinha.
type DiskVersion = { text: string; bom: boolean; readOnly: boolean };
// Comparação antes de gravar sobre o disco (ou de usar o disco): as duas versões são as que a tela tinha quando o usuário pediu.
type Compare = { docId: string; path: string; mode: 'keep' | 'use'; disk: DiskVersion; draft: string };

const DEFAULT_SPLIT = 0.5;
const newDocId = () => `doc-${crypto.randomUUID()}`;
// Mesmo nome em pastas diferentes: a pasta vira o detalhe da aba, para as duas não ficarem iguais. Sem repetição, não há detalhe.
function detailsOf(paths: string[]) {
  const count = new Map<string, number>();
  for (const path of paths) count.set(fileName(path), (count.get(fileName(path)) ?? 0) + 1);
  return new Map(paths.map((path) => [path, (count.get(fileName(path)) ?? 0) > 1 ? splitPath(path).dir || 'raiz' : '']));
}
// Nome curto da aba nos avisos.
const nameOf = (target: TabTarget) => (target.kind === 'draft' ? draftName(target.num) : fileName(target.path));

// Texto de uma aba para a lista e os avisos: arquivo e pasta, ou o rótulo do diff/commit/conflito/rascunho.
function labelOf(target: TabTarget): string {
  switch (target.kind) {
    case 'file': return target.path;
    case 'diff': return `${target.path} (${target.side === 'staged' ? 'preparadas' : 'alterações'})`;
    case 'commit': return `${target.path} @ ${target.hash.slice(0, 7)}`;
    case 'conflict': return `${target.path} (conflito)`;
    case 'draft': return draftName(target.num);
  }
}

// Ícone da aba: o do tipo de arquivo; rascunho não tem tipo ainda, então usa o de texto.
const tabIcon = (target: TabTarget) => (target.kind === 'draft'
  ? <FileText className="size-4" />
  : <FileIcon path={target.path} className="size-4" />);

function absoluteFilePath(project: string, path: string): string {
  if (!path) return path;
  if (isAbsolutePath(path)) return path.replace(/\//g, '\\');
  return project.replace(/[\\/]+$/, '') + '\\' + path.replace(/\//g, '\\');
}

// Grava `text` em `path` (relativo, dentro do projeto). Arquivo novo é criado; se já existe (o diálogo confirmou a sobrescrita), o texto
// vai por cima da versão que está no disco agora, sem gravar às cegas.
async function writeNewFile(backend: ReturnType<typeof useBackend>, path: string, text: string): Promise<void> {
  if (isAbsolutePath(path)) {
    let base = '';
    let bom = false;
    try {
      const current = await backend.readFileAt(path);
      if (current.binary || current.tooLarge) throw new Error(`${fileName(path)} não é texto: não dá para salvar por cima dele.`);
      base = current.text;
      bom = current.bom;
    } catch (e) {
      if (e instanceof Error && e.message.includes('não é texto')) throw e;
      // Caminho que ainda não existe: o backend cria só com base vazia e recusa se alguém o criar antes da gravação.
    }
    await backend.writeFileAtChecked(path, text, base, bom);
    return;
  }
  const slash = path.lastIndexOf('/');
  let base = '';
  let bom = false;
  try {
    await backend.createFile(slash < 0 ? '' : path.slice(0, slash), path.slice(slash + 1));
  } catch (e) {
    // Só "já existe" segue para a leitura; outro erro (permissão, pasta ausente) sobe com o motivo real.
    if (!message(e).startsWith('Já existe um item')) throw e;
    const current = await backend.readFile(path);
    if (current.binary || current.tooLarge) throw new Error(`${fileName(path)} não é texto: não dá para salvar por cima dele.`);
    base = current.text;
    bom = current.bom;
  }
  await backend.writeFileChecked(path, text, base, bom);
}

function emptyData(): EditorData { return { buffers: {}, conflicts: {} }; }

// Só os docIds vivos; devolve o mesmo objeto se nada saiu (o estado não re-renderiza à toa).
function keepDocs<T>(record: Record<string, T>, ids: ReadonlySet<string>): Record<string, T> {
  const entries = Object.entries(record);
  const kept = entries.filter(([id]) => ids.has(id));
  return kept.length === entries.length ? record : Object.fromEntries(kept);
}

export const EditorWorkspace = forwardRef<EditorWorkspaceHandle, EditorWorkspaceProps>(function EditorWorkspace(props, handle) {
  const backend = useBackend();
  const workerPool = useWorkerPool();
  const { project, active = true, locked, guard, refreshGit, onActiveTarget, layout, onRestoreLayout, onReveal, onGroupCount, onSaved } = props;
  const storageProject = project || NO_PROJECT;
  // Ref para que as gravações antigas (ainda em await) avisem com a prop atual.
  const savedRef = useRef(onSaved);
  useLayoutEffect(() => {
    savedRef.current = onSaved;
  });
  const notifySaved = () => savedRef.current?.();

  // Restaura a recuperação uma vez, na montagem (lê o localStorage de forma síncrona e valida tudo).
  const [boot] = useState(() => {
    let text: string | null = null;
    let readFailed = false;
    const notes: string[] = [];
    try {
      text = readRecovery(localStorage, project);
    } catch (e) {
      // Erro de leitura não é primeira abertura: avisa, e a edição começa vazia sem apagar a recuperação gravada.
      notes.push(`Não foi possível ler a recuperação (${message(e)}). Começou vazia.`);
      text = null;
      readFailed = true;
    }
    if (text === null) return { ws: createWorkspace(), data: emptyData(), split: DEFAULT_SPLIT, layout: null as string | null, notes, cursors: {} as Record<string, Cursor>, scrolls: {} as Record<string, Scroll>, readFailed };
    const decoded = decodeRecovery(text, storageProject);
    notes.push(...decoded.notes);
    const ws = decoded.recovery.workspace ? loadWorkspace(decoded.recovery.workspace, true) : { state: createWorkspace(), notes: [] };
    notes.push(...ws.notes);
    const data = emptyData();
    const draftIds = new Set(ws.state.groups.flatMap((g) => g.tabs.flatMap((tab) => tab.target.kind === 'draft' ? [tab.target.docId] : [])));
    for (const docId of draftIds) data.buffers[docId] = { docId, path: '', raw: '', bom: false, newline: '\n', draft: '', readOnly: false };
    const cursors: Record<string, Cursor> = {};
    const scrolls: Record<string, Scroll> = {};
    for (const b of decoded.recovery.buffers) {
      if (b.cursor) cursors[b.docId] = b.cursor;
      if (b.scroll) scrolls[b.docId] = b.scroll;
      if (b.dirty && (b.path !== '' || draftIds.has(b.docId))) data.buffers[b.docId] = { docId: b.docId, path: b.path, ...b.dirty };
    }
    for (const c of decoded.recovery.conflicts) data.conflicts[conflictKey(c.path)] = c;
    return { ws: ws.state, data, split: decoded.recovery.split ?? DEFAULT_SPLIT, layout: decoded.recovery.layout, notes, cursors, scrolls, readFailed };
  });

  const [ws, setWsState] = useState<WorkspaceState>(boot.ws);
  const [data, setDataState] = useState<EditorData>(boot.data);
  const [status, setStatus] = useState<Record<string, Status>>({});
  const [disk, setDisk] = useState<Record<string, DiskVersion>>({});
  const [split, setSplit] = useState(boot.split);
  const [reveal, setReveal] = useState<(Reveal & { docId: string }) | null>(null);
  const [notice, setNotice] = useState(boot.notes.join(' '));
  const [askState, setAskState] = useState<(Ask & { resolve: (key: string | null) => void }) | null>(null);
  const wsRef = useRef(ws);
  const dataRef = useRef(data);
  const cursors = useRef<Record<string, Cursor>>(boot.cursors);
  // Rolagem por arquivo (docId): a vista que monta de novo (aba movida, app reaberto) volta para o mesmo ponto.
  const scrolls = useRef<Record<string, Scroll>>(boot.scrolls);
  // Cursor e rolagem não passam pelo estado: este atraso os põe na fila de gravação da recuperação sem re-renderizar a área.
  const persistTimer = useRef(0);
  // Leitura da recuperação falhou: nada é gravado por cima dela até o usuário mexer nas abas ou nos textos.
  const untouched = useRef(boot.readFailed);
  const [compare, setCompare] = useState<Compare | null>(null);
  const saving = useRef(new Set<string>());
  const reads = useRef(new Map<string, CachedRead>());
  const gitReads = useRef(new Map<string, GitTabRead>());
  const openSequence = useRef(0);
  const askOpen = useRef(false);
  const revealSeq = useRef(0);
  const layoutRef = useRef(layout ?? boot.layout);
  const splitRef = useRef(split);
  // Aba em arrasto na faixa de `group`; `into` é o outro grupo, com o ponteiro sobre a faixa dele.
  const [tabDrag, setTabDrag] = useState<({ id: string; group: GroupId; from: number; into: GroupId | null } & TabDrag) | null>(null);
  const splitDrag = useRef<{ startX: number; start: number; width: number } | null>(null);
  // Sujos: rascunhos de arquivo e de conflito.
  const dirtyNow = (d?: EditorData) => dirtyKeys(d ?? dataRef.current);

  const setWs = (next: WorkspaceState) => {
    wsRef.current = next;
    setWsState(next);
  };
  const setData = (next: EditorData) => {
    dataRef.current = next;
    setDataState(next);
  };
  const patchBuffer = (docId: string, change: (b: Buffer) => Buffer) => {
    const current = dataRef.current.buffers[docId];
    if (!current) return;
    setData({ ...dataRef.current, buffers: { ...dataRef.current.buffers, [docId]: change(current) } });
  };
  const putConflict = (draft: ConflictDraft) => {
    const conflicts = { ...dataRef.current.conflicts };
    conflicts[conflictKey(draft.path)] = draft;
    setData({ ...dataRef.current, conflicts });
  };
  const dropConflict = (path: string) => {
    const conflicts = { ...dataRef.current.conflicts };
    delete conflicts[conflictKey(path)];
    setData({ ...dataRef.current, conflicts });
  };
  const allTabs = () => wsRef.current.groups.flatMap((g) => g.tabs);
  // Só troca quando o estado muda: o polling chama isto a cada 2,5 s e não pode re-renderizar a área à toa.
  const setStatusOf = (docId: string, value: Status) => setStatus((s) => {
    const cur = s[docId];
    if (cur && cur.error === value.error && !!cur.binary === !!value.binary && !!cur.large === !!value.large) return s;
    return { ...s, [docId]: value };
  });
  const clearDisk = (docId: string) => setDisk((d) => {
    if (!(docId in d)) return d;
    const rest = { ...d };
    delete rest[docId];
    return rest;
  });

  // Confirmação genérica (Modal): devolve a chave escolhida, ou null se cancelou.
  // Uma confirmação por vez: se outra ação pede modal enquanto ele está aberto, recusa esta (sem deixar promise pendurada).
  const ask = (options: Ask) => new Promise<string | null>((resolve) => {
    if (askOpen.current) {
      resolve(null);
      return;
    }
    askOpen.current = true;
    setAskState({ ...options, resolve: (key) => { askOpen.current = false; resolve(key); } });
  });

  const groupWithDefault = (state: WorkspaceState, id: GroupId): WorkspaceState => {
    if (state.groups.some((g) => g.id === id) || state.groups.length >= 2) return state;
    return { ...state, groups: [...state.groups, { id, tabs: [], active: null }] };
  };

  // ---- Abrir ----
  const docIdFor = (path: string): string => {
    const lower = path.toLowerCase();
    const buffer = Object.values(dataRef.current.buffers).find((b) => b.path.toLowerCase() === lower);
    if (buffer) return buffer.docId;
    const tab = allTabs().find((t) => t.target.kind === 'file' && t.target.path.toLowerCase() === lower);
    return tab && tab.target.kind === 'file' ? tab.target.docId : newDocId();
  };

  const readFor = (path: string): CachedRead => {
    const key = path.toLowerCase();
    const cached = reads.current.get(key);
    if (cached) return cached;
    const entry: CachedRead = { promise: Promise.resolve().then(() => backend.readFile(path)), timer: 0 };
    const expire = () => {
      if (reads.current.get(key) === entry) reads.current.delete(key);
    };
    entry.timer = window.setTimeout(expire, 5000);
    reads.current.set(key, entry);
    const keepBriefly = () => {
      window.clearTimeout(entry.timer);
      entry.timer = window.setTimeout(expire, 3000);
    };
    void entry.promise.then(keepBriefly, keepBriefly);
    return entry;
  };

  const prefetch = (path: string) => {
    if (mediaOf(path) || Object.values(dataRef.current.buffers).some((buffer) => buffer.path.toLowerCase() === path.toLowerCase())) return;
    const entry = readFor(path);
    entry.language ??= preloadLanguage(path);
  };

  const openTargetNow = (target: TabTarget, options: Pick<OpenOptions, 'permanent' | 'side'> = {}, background = false) => {
    const dirty = dirtyNow();
    const group: GroupId = options.side ?? wsRef.current.focus;
    if (background && options.permanent) {
      const current = wsRef.current;
      const focus = current.focus;
      const active = current.groups.find((item) => item.id === focus)?.active;
      let next = openPermanent(groupWithDefault(current, group), group, target);
      if (active) next = activate(next, active);
      if (next.focus !== focus) next = { ...next, focus };
      setWs(next);
      return;
    }
    const base = groupWithDefault(wsRef.current, group);
    setWs(options.permanent ? openPermanent(base, group, target) : openPreview(base, group, target, dirty));
  };

  const gitReadFor = (target: Extract<TabTarget, { kind: 'diff' | 'commit' }>): GitTabRead => {
    const key = targetKey(target);
    const cached = gitReads.current.get(key);
    if (cached) return cached;
    const request = target.kind === 'diff'
      ? backend.gitReviewDiff(target.path, target.side === 'staged').then((review) => ({ review } as const))
      : backend.gitCommitDiff(target.hash, target.path).then((diff) => ({ diff } as const));
    let consumed = false;
    const promise = request.then((result): GitTabReadResult => {
      entry.result = result;
      const patch = 'review' in result ? result.review.display.patch : result.diff.patch;
      const invalid = 'review' in result
        ? result.review.diff.binary || result.review.diff.tooLarge || result.review.display.binary || result.review.display.tooLarge
        : result.diff.binary || result.diff.tooLarge;
      entry.patch = patch;
      if (patch && !invalid) {
        entry.fileDiff = gitTabFileDiff(target, patch, 'review' in result ? result.review.revision : undefined) ?? undefined;
        if (entry.fileDiff && workerPool && canHighlightGitTabDiff(entry.fileDiff)) workerPool.primeDiffHighlightCache(entry.fileDiff);
      }
      return result;
    }).catch((e: unknown) => ({ error: message(e) } as const));
    const entry: GitTabRead = {
      promise,
      timer: 0,
      consume: () => {
        consumed = true;
        window.clearTimeout(entry.timer);
        if (gitReads.current.get(key) === entry) gitReads.current.delete(key);
      },
    };
    const expire = () => { if (gitReads.current.get(key) === entry) gitReads.current.delete(key); };
    entry.timer = window.setTimeout(expire, 5000);
    gitReads.current.set(key, entry);
    void entry.promise.then(() => {
      if (consumed || gitReads.current.get(key) !== entry) return;
      window.clearTimeout(entry.timer);
      entry.timer = window.setTimeout(expire, 3000);
    });
    return entry;
  };

  // `hover` só aquece a leitura e o realce: o prazo de 500 ms do openTarget conta do mousedown (ou do clique), não do hover.
  const prefetchTarget = (target: TabTarget, hover = false) => {
    if (target.kind === 'diff' || target.kind === 'commit') {
      const entry = gitReadFor(target);
      if (!hover) entry.prefetchAt = performance.now();
    }
  };

  const openTarget = (target: TabTarget, options: Pick<OpenOptions, 'permanent' | 'side'> = {}) => {
    const sequence = ++openSequence.current;
    const open = () => {
      if (sequence === openSequence.current) openTargetNow(target, options);
      else if (options.permanent) openTargetNow(target, options, true);
    };
    if (target.kind !== 'diff' && target.kind !== 'commit') { open(); return; }
    const key = targetKey(target);
    if (allTabs().some((tab) => targetKey(tab.target) === key)) { gitReads.current.get(key)?.consume(); open(); return; }
    const entry = gitReadFor(target);
    const startedAt = entry.prefetchAt ?? performance.now();
    entry.prefetchAt = undefined;
    void (async () => {
      const deadline = startedAt + 500;
      let timer = 0;
      const timeout = new Promise<void>((resolve) => { timer = window.setTimeout(resolve, Math.max(0, deadline - performance.now())); });
      const waitForHighlight = (fileDiff: NonNullable<GitTabRead['fileDiff']>) => new Promise<void>((resolve) => {
        let frame = 0;
        const finish = () => {
          window.clearTimeout(highlightTimer);
          if (frame) window.cancelAnimationFrame(frame);
          resolve();
        };
        const highlightTimer = window.setTimeout(finish, Math.max(0, deadline - performance.now()));
        const poll = () => {
          if (!workerPool || !workerPool.isWorkingPool() || workerPool.getDiffResultCache(fileDiff) || performance.now() >= deadline) finish();
          else frame = window.requestAnimationFrame(poll);
        };
        poll();
      });
      try {
        await Promise.race([entry.promise, timeout]);
        if (entry.fileDiff && canHighlightGitTabDiff(entry.fileDiff) && performance.now() < deadline) {
          await Promise.race([waitForHighlight(entry.fileDiff), timeout]);
        }
      } finally {
        window.clearTimeout(timer);
        open();
      }
    })();
  };

  const openFile = (path: string, options: OpenOptions = {}) => {
    const sequence = ++openSequence.current;
    const docId = docIdFor(path);
    const target = { kind: 'file' as const, docId, path };
    const revealLine = () => {
      if (options.line !== undefined) setReveal({ docId, line: options.line, col: options.col, id: ++revealSeq.current });
    };
    const open = () => {
      if (sequence !== openSequence.current) return;
      openTarget(target, options);
      revealLine();
    };
    const buffer = dataRef.current.buffers[docId];
    if (mediaOf(path) || buffer) {
      if (!mediaOf(path)) {
        const entry = reads.current.get(path.toLowerCase());
        if (entry) entry.language ??= preloadLanguage(path);
        else void preloadLanguage(path);
      }
      open();
      return;
    }

    const entry = readFor(path);
    entry.language ??= preloadLanguage(path);
    const result = entry.promise.then((file) => ({ file } as const), (error: unknown) => ({ error } as const));
    const ready = Promise.all([result, entry.language]).then(([read]) => read);
    let timedOut = false;
    let lateReadHandled = false;
    const applyRead = (read: Awaited<typeof result>, opening: boolean) => {
      if (!opening && !allTabs().some((tab) => tab.target.kind === 'file' && tab.target.docId === docId && tab.target.path.toLowerCase() === path.toLowerCase())) return;
      if ('error' in read) {
        setStatusOf(docId, { error: message(read.error) });
        return;
      }
      const file = read.file;
      if (file.binary || file.tooLarge) {
        setStatusOf(docId, file.binary ? { binary: true } : { large: true });
        return;
      }
      if (saving.current.has(docId) || dataRef.current.buffers[docId]) return;
      setStatusOf(docId, {});
      setData({
        ...dataRef.current,
        buffers: {
          ...dataRef.current.buffers,
          [docId]: {
            docId, path, raw: file.text, bom: file.bom, newline: newlineOf(file.text), draft: normalizeNewlines(file.text),
            readOnly: file.readOnly,
          },
        },
      });
    };
    const applyLateRead = (read: Awaited<typeof result>) => {
      if (!timedOut || lateReadHandled) return;
      lateReadHandled = true;
      applyRead(read, false);
    };
    void result.then(applyLateRead);
    void (async () => {
      let timer = 0;
      const timeout = Symbol('timeout');
      const outcome = await Promise.race([
        ready,
        new Promise<typeof timeout>((resolve) => { timer = window.setTimeout(() => resolve(timeout), 150); }),
      ]);
      window.clearTimeout(timer);
      if (sequence !== openSequence.current) return;
      if (outcome === timeout) {
        timedOut = true;
        open();
        void result.then(applyLateRead);
        return;
      }
      applyRead(outcome, true);
      open();
    })();
  };
  // À direita, pela ordem visual: com dois grupos, o segundo (groups[1]), mesmo quando o foco já está nele. Com um só,
  // vai para o outro id, que entra depois do existente (à direita dele).
  const openBeside = (path: string, options: Pick<OpenOptions, 'permanent' | 'line' | 'col'> = {}) => {
    const state = wsRef.current;
    const side: GroupId = state.groups.length >= 2 ? state.groups[1].id : state.groups[0]?.id === 'A' ? 'B' : 'A';
    openFile(path, { ...options, side });
  };

  // ---- Rascunho novo (duplo clique na faixa, Ctrl+N): buffer vazio em memória, aba permanente e ativa, com foco no texto ----
  // `freshDraft` é o rascunho que acabou de nascer: a vista dele foca ao montar. Some logo depois da renderização (effect abaixo).
  const freshDraft = useRef<string | null>(null);
  const newDraft = (side: GroupId) => {
    const docId = newDocId();
    const base = groupWithDefault(wsRef.current, side);
    setData({
      ...dataRef.current,
      buffers: { ...dataRef.current.buffers, [docId]: { docId, path: '', raw: '', bom: false, newline: '\n', draft: '', readOnly: false } },
    });
    freshDraft.current = docId;
    setWs(openDraft(base, side, docId));
  };
  useEffect(() => { freshDraft.current = null; });

  // ---- Fechar (decisão de rascunho antes de qualquer mudança) ----
  const closeFlow = async (scope: CloseScope): Promise<boolean> => {
    const plan = planClose(wsRef.current, scope, dirtyNow());
    if (plan.tabIds.length === 0) return true;
    const closing = new Set(plan.tabIds);
    const stay = new Set(allTabs().filter((t) => !closing.has(t.id)).map((t) => targetKey(t.target)));
    const byId = new Map(allTabs().map((t) => [t.id, t] as const));
    // Vista que continua aberta (no mesmo grupo ou no outro) não perde o buffer: essas abas não entram na pergunta.
    const unsavedAll = plan.unsaved.map((id) => byId.get(id)).filter((t): t is EditorTab => !!t && !stay.has(targetKey(t.target)));
    const unsavedTabs = unsavedAll;
    let decision: string | null = null;
    if (unsavedTabs.length > 0) {
      const names = unsavedTabs.map((t) => nameOf(t.target)).join(', ');
      const choice = await ask({
        title: 'Fechar com alterações não salvas?',
        body: `Há alterações não salvas em ${names}. Salvar antes de fechar, descartar ou cancelar?`,
        actions: [{ key: 'discard', label: 'Descartar', danger: true }, { key: 'save', label: 'Salvar' }],
      });
      if (choice === null) return false;
      decision = choice;
      if (choice === 'save') {
        for (const tab of unsavedTabs) {
          if (!(await saveTab(tab.target))) {
            setNotice(`Não foi possível salvar ${nameOf(tab.target)}. Nada foi fechado.`);
            return false;
          }
        }
      }
    }
    // Estado pode ter mudado durante a pergunta e o salvamento (digitação nova): a lista é refeita e o que ainda está sujo não fecha sem decisão.
    const again = planClose(wsRef.current, scope, dirtyNow());
    // Descartar já foi decidido para as abas de `unsavedTabs`; salvar não garante nada se houve digitação durante o save.
    const dirtyAfter = dirtyNow();
    const discarded = new Set(decision === 'discard' ? unsavedTabs.map((t) => t.id) : []);
    const pendingNew = again.tabIds
      .map((id) => allTabs().find((t) => t.id === id))
      .filter((t): t is EditorTab => !!t && isDirty(t.target, dirtyAfter) && !discarded.has(t.id) && !stay.has(targetKey(t.target)));
    if (pendingNew.length > 0) {
      setNotice(`Ainda há alterações não salvas em ${pendingNew.map((t) => nameOf(t.target)).join(', ')}. Nada foi fechado.`);
      return false;
    }
    const next = closeTabs(wsRef.current, again.tabIds);
    setWs(next);
    pruneData(next);
    return true;
  };

  // Buffers e conflitos sem aba: a aba foi fechada e o rascunho já foi decidido (salvo ou descartado). Rascunho aberto mantém o buffer.
  const pruneData = (state: WorkspaceState) => {
    const tabs = state.groups.flatMap((g) => g.tabs);
    const docIds = liveDocIds(state);
    const keys = new Set(tabs.flatMap((t) => (t.target.kind === 'conflict' ? [conflictKey(t.target.path)] : [])));
    const current = dataRef.current;
    for (const id of Object.keys(current.buffers)) if (!docIds.has(id)) forgetEditorState(id);
    const buffers = Object.fromEntries(Object.entries(current.buffers).filter(([id]) => docIds.has(id)));
    const conflicts = Object.fromEntries(Object.entries(current.conflicts).filter(([key]) => keys.has(key)));
    for (const id of Object.keys(scrolls.current)) if (!docIds.has(id)) delete scrolls.current[id];
    for (const id of Object.keys(cursors.current)) if (!docIds.has(id)) delete cursors.current[id];
    setDisk((d) => keepDocs(d, docIds));
    setStatus((st) => keepDocs(st, docIds));
    setData({ buffers, conflicts });
  };

  // ---- Salvar ----
  const saveBuffer = async (docId: string): Promise<boolean> => {
    const buffer = dataRef.current.buffers[docId];
    if (!buffer || !isBufferDirty(buffer)) return true;
    if (allTabs().some((t) => t.target.kind === 'draft' && t.target.docId === docId)) return saveDraft(docId);
    if (buffer.readOnly || saving.current.has(docId)) return false;
    saving.current.add(docId);
    const sent = toDiskNewlines(buffer.draft, buffer.newline);
    // Grava só se o disco ainda é o `raw` lido (base crua, sem normalizar). Conflito é tratado abaixo, não há força silenciosa.
    return backend.writeFileChecked(buffer.path, sent, buffer.raw, buffer.bom)
      .then(() => {
        patchBuffer(docId, (b) => ({ ...b, raw: sent }));
        clearDisk(docId);
        setStatusOf(docId, {});
        refreshGit();
        notifySaved();
        return true;
      })
      .catch((e: unknown) => {
        const reason = message(e);
        return backend.readFile(buffer.path)
          .then((file) => {
            if (normalizeNewlines(file.text) !== normalizeNewlines(buffer.raw) || file.bom !== buffer.bom) {
              setDisk((d) => ({ ...d, [docId]: { text: file.text, bom: file.bom, readOnly: file.readOnly } }));
              setStatusOf(docId, {});
            } else {
              setStatusOf(docId, { error: reason });
            }
          }, (readError: unknown) => {
            setStatusOf(docId, { error: `${reason} ${message(readError)}` });
          })
          .then(() => false);
      })
      .finally(() => {
        saving.current.delete(docId);
      });
  };

  // Grava a resolução. Com `markResolved` o Git já tirou o conflito (o arquivo vai para o stage): não relê o conflito, solta o
  // rascunho e fecha a aba. Sem ele, relê a revisão mas só troca `origin`/`revision`, preservando o texto digitado durante o await.
  const conflictSaving = useRef(new Set<string>());
  const saveConflict = async (path: string, markResolved: boolean): Promise<boolean> => {
    const key = conflictKey(path);
    const current = dataRef.current.conflicts[key];
    if (!current || (!isConflictDirty(current) && !markResolved)) return true;
    if (conflictSaving.current.has(key)) return false;
    if (markResolved && hasMarkers(current.draft)) {
      toast.error({ title: 'Ainda há marcadores de conflito no texto. Remova-os antes de marcar como resolvido.', duration: 7000 });
      return false;
    }
    if (Object.values(dataRef.current.buffers).some((b) => isBufferDirty(b) && b.path.toLowerCase() === path.toLowerCase())) {
      toast.error({ title: 'Salve ou descarte a edição aberta deste arquivo antes de gravar a resolução.', duration: 7000 });
      return false;
    }
    // Gravar a resolução é operação Git da UI: a trava compartilhada vale aqui também (a da barra lateral).
    if (!guard.enter()) return false;
    conflictSaving.current.add(key);
    const sent = current.draft;
    return backend.gitWriteResolution(path, sent, current.revision, markResolved)
      .then(async () => {
        notifySaved();
        if (markResolved) {
          dropConflict(path);
          refreshGit();
          const found = allTabs().find((t) => t.target.kind === 'conflict' && t.target.path.toLowerCase() === path.toLowerCase());
          if (found) void closeFlow({ kind: 'one', tabId: found.id });
          return true;
        }
        const fresh = await backend.gitConflict(path);
        // Quem chama pode ter digitado durante o await: lê o rascunho atual e só troca a base e a revisão.
        const now = dataRef.current.conflicts[key];
        if (now) putConflict({ ...now, origin: sent, revision: fresh.revision });
        refreshGit();
        return true;
      })
      .catch((e: unknown) => {
        toast.error({ title: message(e), duration: 7000 });
        return false;
      })
      .finally(() => {
        conflictSaving.current.delete(key);
        guard.leave();
      });
  };

  // Rascunho sem caminho: o diálogo começa na pasta do projeto quando há uma; fora dela, o caminho segue absoluto.
  const saveDraft = async (docId: string): Promise<boolean> => {
    const tab = allTabs().find((t) => t.target.kind === 'draft' && t.target.docId === docId);
    const buffer = dataRef.current.buffers[docId];
    if (!tab || tab.target.kind !== 'draft' || !buffer || saving.current.has(docId)) return false;
    saving.current.add(docId);
    const name = draftName(tab.target.num);
    const attempt = async (): Promise<boolean> => {
      const picked = await backend.pickSavePath(project || null, name);
      if (picked === null) return false;
      const absolute = picked.replace(/\\/g, '/');
      const path = relativeTo(project, absolute) ?? absolute;
      if (!isAbsolutePath(path) && !isSafePath(path)) return false;
      const lower = path.toLowerCase();
      const existing = allTabs().find((t) => t.target.kind === 'file' && t.target.path.toLowerCase() === lower);
      const existingBuffer = existing?.target.kind === 'file' ? dataRef.current.buffers[existing.target.docId] : undefined;
      if (existingBuffer && isBufferDirty(existingBuffer)) {
        toast.error({ title: `Salve ou descarte a edição aberta de ${fileName(path)} antes de salvar por cima dele.`, duration: 7000 });
        return false;
      }
      const sent = toDiskNewlines(buffer.draft, buffer.newline);
      await writeNewFile(backend, path, sent);
      const saved = await backend.readFile(path);
      if (existing) {
        const next = activate(closeTabs(wsRef.current, [tab.id]), existing.id);
        setWs(next);
        pruneData(next);
        if (existing.target.kind === 'file') void syncFile(existing.target.docId);
      } else {
        patchBuffer(docId, (b) => ({ ...b, path, raw: saved.text, bom: saved.bom, newline: newlineOf(saved.text) }));
        setWs(promoteDraft(wsRef.current, tab.id, path));
      }
      refreshGit();
      notifySaved();
      return true;
    };
    return attempt()
      .catch((e: unknown) => {
        toast.error({ title: message(e), duration: 7000 });
        return false;
      })
      .finally(() => {
        saving.current.delete(docId);
      });
  };

  const saveTab = (target: TabTarget): Promise<boolean> => {
    if (target.kind === 'file') return saveBuffer(target.docId);
    if (target.kind === 'draft') return saveDraft(target.docId);
    if (target.kind === 'conflict') return saveConflict(target.path, false);
    return Promise.resolve(true);
  };

  const saveAs = async (target: TabTarget): Promise<boolean> => {
    if (target.kind === 'draft') return saveDraft(target.docId);
    if (target.kind !== 'file') return false;
    let buffer = dataRef.current.buffers[target.docId];
    if (!buffer) {
      await syncFile(target.docId);
      buffer = dataRef.current.buffers[target.docId];
    }
    if (!buffer) return false;
    const slash = buffer.path.lastIndexOf('/');
    const directory = project || (isAbsolutePath(buffer.path) && slash > 0 ? buffer.path.slice(0, slash) : null);
    const docId = target.docId;
    const attempt = async (current: Buffer): Promise<boolean> => {
      const picked = await backend.pickSavePath(directory, fileName(current.path));
      if (!picked) return false;
      const absolute = picked.replace(/\\/g, '/');
      const path = relativeTo(project, absolute) ?? absolute;
      if (path.toLowerCase() === current.path.toLowerCase()) return saveBuffer(docId);
      const existing = allTabs().find((t) => t.target.kind === 'file' && t.target.path.toLowerCase() === path.toLowerCase());
      const existingBuffer = existing?.target.kind === 'file' ? dataRef.current.buffers[existing.target.docId] : undefined;
      if (existingBuffer && isBufferDirty(existingBuffer)) {
        toast.error({ title: `Salve ou descarte a edição aberta de ${fileName(path)} antes de salvar por cima dele.`, duration: 7000 });
        return false;
      }
      await writeNewFile(backend, path, toDiskNewlines(current.draft, current.newline));
      const saved = await backend.readFile(path);
      if (existing) {
        const source = allTabs().filter((t) => t.target.kind === 'file' && t.target.docId === docId).map((t) => t.id);
        const next = activate(closeTabs(wsRef.current, source), existing.id);
        setWs(next);
        pruneData(next);
        void syncFile(existing.target.kind === 'file' ? existing.target.docId : '');
      } else {
        // Cópia, não renomeação: o arquivo antigo continua no disco, então diff e conflito dele ficam onde estão. Só o documento muda de caminho.
        setWs(retargetDoc(wsRef.current, docId, path));
        patchBuffer(docId, (b) => ({ ...b, path, raw: saved.text, bom: saved.bom, newline: newlineOf(saved.text) }));
        clearDisk(docId);
        setStatusOf(docId, {});
      }
      refreshGit();
      notifySaved();
      return true;
    };
    return attempt(buffer).catch((e: unknown) => {
      toast.error({ title: message(e), duration: 7000 });
      return false;
    });
  };

  const saveActive = () => {
    const tab = focusedTab();
    return tab ? saveTab(tab.target) : Promise.resolve(false);
  };
  const saveActiveAs = () => {
    const tab = focusedTab();
    return tab ? saveAs(tab.target) : Promise.resolve(false);
  };

  const saveAll = async (): Promise<boolean> => {
    let ok = true;
    for (const docId of Object.keys(dataRef.current.buffers)) if (!(await saveBuffer(docId))) ok = false;
    for (const draft of Object.values(dataRef.current.conflicts)) {
      if (isConflictDirty(draft) && !(await saveConflict(draft.path, false))) ok = false;
    }
    // Rascunho que apareceu ou mudou durante os awaits continua sujo: não diz que pode fechar.
    return ok && dirtyKeys(dataRef.current).size === 0;
  };

  // ---- Sincronizar com o disco (abas ativas, em intervalo) ----
  const syncFile = async (docId: string) => {
    const tab = allTabs().find((t) => t.target.kind === 'file' && t.target.docId === docId);
    if (!tab || tab.target.kind !== 'file' || saving.current.has(docId)) return;
    const path = tab.target.path;
    // Mídia não tem texto: a aba lê o próprio arquivo (`MediaView`) e não cria buffer, então nada aqui a grava ou a torna suja.
    if (mediaOf(path)) return;
    const raw = dataRef.current.buffers[docId]?.raw;
    let file: FileContent;
    const entry = readFor(path);
    try {
      file = await entry.promise;
    } catch (e) {
      // Arquivo apagado ou inacessível: o buffer (e o rascunho) continuam; a aba mostra o erro.
      setStatusOf(docId, { error: message(e) });
      return;
    } finally {
      if (reads.current.get(path.toLowerCase()) === entry) {
        window.clearTimeout(entry.timer);
        reads.current.delete(path.toLowerCase());
      }
    }
    if (!allTabs().some((t) => t.target.kind === 'file' && t.target.docId === docId)) return;
    // Gravação durante a leitura: a resposta pode ser de antes dela e trocaria o buffer recém-salvo.
    if (saving.current.has(docId) || dataRef.current.buffers[docId]?.raw !== raw) return;
    if (file.binary || file.tooLarge) {
      setStatusOf(docId, file.binary ? { binary: true } : { large: true });
      return;
    }
    setStatusOf(docId, {});
    const buffer = dataRef.current.buffers[docId];
    if (!buffer) {
      setData({
        ...dataRef.current,
        buffers: {
          ...dataRef.current.buffers,
          [docId]: {
            docId, path, raw: file.text, bom: file.bom, newline: newlineOf(file.text), draft: normalizeNewlines(file.text),
            readOnly: file.readOnly,
          },
        },
      });
      return;
    }
    if (normalizeNewlines(buffer.raw) === normalizeNewlines(file.text) && buffer.bom === file.bom) {
      clearDisk(docId);
      return;
    }
    if (!isBufferDirty(buffer)) {
      patchBuffer(docId, () => ({ ...buffer, raw: file.text, bom: file.bom, newline: newlineOf(file.text), draft: normalizeNewlines(file.text), readOnly: file.readOnly }));
      return;
    }
    setDisk((d) => ({ ...d, [docId]: { text: file.text, bom: file.bom, readOnly: file.readOnly } }));
  };

  const activeFileIds = (state: WorkspaceState) => state.groups.flatMap((g) => {
    const tab = g.tabs.find((t) => t.id === g.active);
    return tab && tab.target.kind === 'file' ? [tab.target.docId] : [];
  });
  const activeKey = activeFileIds(ws).join('|');
  // Sincroniza só as abas de arquivo ativas; sem nenhuma (chave vazia), o polling fica desligado.
  usePolling(async () => {
    await Promise.all(activeKey.split('|').map((id) => syncFile(id)));
  }, POLL_MS, active && activeKey);

  // ---- Avisos e recuperação ----
  const dirtyList = (): string[] => {
    const out: string[] = [];
    for (const b of Object.values(dataRef.current.buffers)) if (isBufferDirty(b) && b.path !== '') out.push(b.path);
    for (const tab of allTabs()) {
      const b = tab.target.kind === 'draft' ? dataRef.current.buffers[tab.target.docId] : undefined;
      if (tab.target.kind === 'draft' && b && isBufferDirty(b)) out.push(draftName(tab.target.num));
    }
    for (const c of Object.values(dataRef.current.conflicts)) if (isConflictDirty(c)) out.push(c.path);
    return out;
  };
  useEffect(() => {
    const group = ws.groups.find((g) => g.id === ws.focus);
    const tab = group?.tabs.find((t) => t.id === group.active);
    onActiveTarget?.(tab ? tab.target : null);
  }, [ws]);

  useEffect(() => {
    if (boot.layout !== null) onRestoreLayout?.(boot.layout);
    // Só na montagem: depois disso o App manda o layout de volta pela prop `layout`.
  }, []);
  useLayoutEffect(() => {
    layoutRef.current = layout ?? layoutRef.current;
    splitRef.current = split;
  });

  // Grava a recuperação: com atraso (edição rápida não escreve a cada tecla) e de imediato ao sair da página ou desmontar.
  // Falha de quota vira aviso e não derruba a edição. Layout e divisor ficam mesmo sem abas; só some quando não há nada.
  const persist = () => {
    if (untouched.current) {
      if (wsRef.current === boot.ws && dataRef.current === boot.data) return;
      untouched.current = false;
    }
    const open = wsRef.current.groups.flatMap((g) => g.tabs);
    const savedWorkspace = wsRef.current;
    const savedTabs = savedWorkspace.groups.flatMap((g) => g.tabs);
    const conflicts = Object.values(dataRef.current.conflicts).filter(isConflictDirty);
    const hasState = savedTabs.length > 0 || conflicts.length > 0 || layoutRef.current !== null;
    const recovery: Recovery = {
      workspace: JSON.stringify(savedWorkspace),
      layout: layoutRef.current,
      split: splitRef.current,
      buffers: [...new Map(open.flatMap((t) => (t.target.kind === 'file' || t.target.kind === 'draft' ? [[t.target.docId, t.target] as const] : []))).values()].map((target) => {
        const docId = target.docId;
        const b = dataRef.current.buffers[docId];
        const path = target.kind === 'file' ? target.path : '';
        return {
          docId, path, cursor: cursors.current[docId] ?? null, scroll: scrolls.current[docId] ?? null,
          dirty: b && (target.kind === 'draft' || isBufferDirty(b)) ? { raw: b.raw, bom: b.bom, newline: b.newline, draft: b.draft, readOnly: b.readOnly } : null,
        };
      }),
      conflicts,
    };
    try {
      if (!hasState) {
        localStorage.removeItem(recoveryKey(project));
        return;
      }
      const encoded = encodeRecovery(storageProject, recovery);
      localStorage.setItem(recoveryKey(project), encoded.text);
      if (encoded.notes.length > 0) setNotice(encoded.notes.join(' '));
    } catch (e) {
      // Sem apagar, a gravação anterior ficaria e poderia restaurar um texto que o usuário já descartou.
      try { localStorage.removeItem(recoveryKey(project)); } catch { /* Sem acesso ao armazenamento: só o aviso. */ }
      setNotice(`Não foi possível guardar a recuperação (${message(e)}). A edição continua, mas não será recuperada.`);
    }
  };
  const persistRef = useRef(persist);
  useLayoutEffect(() => {
    persistRef.current = persist;
  });
  const persistSoon = () => {
    window.clearTimeout(persistTimer.current);
    persistTimer.current = window.setTimeout(() => persistRef.current(), 400);
  };
  useEffect(() => {
    const timer = window.setTimeout(() => persistRef.current(), 400);
    return () => window.clearTimeout(timer);
  }, [ws, data, layout, split, project]);
  useEffect(() => {
    const flush = () => persistRef.current();
    window.addEventListener('pagehide', flush);
    return () => {
      window.removeEventListener('pagehide', flush);
      flush();
    };
  }, []);

  // ---- Operações de aba (menu, teclado) ----
  const activateTab = (tabId: string) => setWs(activate(wsRef.current, tabId));
  // Copia o caminho pedido (relativo, absoluto ou de navegação); sucesso e erro vêm do helper `copy`.
  const copyPath = (kind: 'relative' | 'absolute' | 'navigation', target: TabTarget) => {
    // Rascunho não tem caminho: o menu já mostra o item bloqueado; o atalho só não faz nada.
    if (target.kind === 'draft') return;
    const cursor = target.kind === 'file' ? cursors.current[target.docId] : undefined;
    const text = pathText(kind, target.path, project, cursor);
    if (text === null) return;
    void copy(text, 'Caminho copiado');
  };
  // Arquivo reabre pelo docId atual do caminho: o guardado pode ser de um buffer que já saiu, e a aba duplicaria.
  const reopen = () => setWs(reopenClosed(wsRef.current, (t) => (t.kind === 'file' ? { ...t, docId: docIdFor(t.path) } : t)));

  const moveToOther = (tabId: string) => {
    const found = wsRef.current.groups.find((g) => g.tabs.some((t) => t.id === tabId));
    const to: GroupId = found?.id === 'A' ? 'B' : 'A';
    setWs(moveTab(groupWithDefault(wsRef.current, to), tabId, to));
  };
  const split_ = (tabId: string) => setWs(splitTab(wsRef.current, tabId));
  const join_ = () => setWs(joinGroups(wsRef.current));

  // ---- Renomear e excluir (chamados pelo App depois da operação de arquivo) ----
  const remap = (from: string, to: string) => {
    setWs(remapPath(wsRef.current, from, to));
    setData(remapData(dataRef.current, from, to));
    setStatus({});
  };
  const remove = (paths: string[]) => {
    const hit = (p: string) => paths.some((base) => isWithin(p, base));
    // Abas de commit (histórico) leem blobs imutáveis: continuam abertas mesmo quando o arquivo some da pasta.
    // Rascunho não tem caminho no projeto: nunca some com a exclusão de um arquivo.
    const ids = allTabs().filter((t) => t.target.kind !== 'commit' && t.target.kind !== 'draft' && hit(t.target.path)).map((t) => t.id);
    const next = closeTabs(wsRef.current, ids);
    setWs(next);
    pruneData(next);
  };

  // ---- Lado a lado: arrastar o divisor e guardar a proporção ----
  const splitMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = splitDrag.current;
    if (!d) return;
    const next = Math.min(0.85, Math.max(0.15, d.start + (e.clientX - d.startX) / d.width));
    splitRef.current = next;
    setSplit(next);
  };

  // ---- Arrastar abas: como no painel Terminal (src/shared/tab-strip.ts); soltar na faixa do outro grupo move a aba para lá ----
  const dragTab = (e: ReactPointerEvent<HTMLDivElement>, groupId: GroupId, tabId: string) => {
    const box = strips.current.get(groupId)?.el;
    const group = wsRef.current.groups.find((g) => g.id === groupId);
    // Só pela aba: o X não arrasta.
    if (e.button !== 0 || !box || !group || !(e.target as HTMLElement).closest('[role="tab"]')) return;
    const nodes = [...box.querySelectorAll<HTMLElement>('[data-tab]')];
    const from = nodes.findIndex((node) => node.dataset.tab === tabId);
    if (from < 0) return;
    // As fixadas ficam no começo: a aba só anda entre as do mesmo tipo.
    const pinned = group.tabs.filter((t) => t.pinned).length;
    const range: [number, number] = group.tabs[from].pinned ? [0, pinned - 1] : [pinned, nodes.length - 1];
    const otherStrip = (x: number, y: number) => {
      const id = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-group]')?.dataset.group as GroupId | undefined;
      return id && id !== groupId ? id : null;
    };
    trackTabDrag(e, box, nodes, from, {
      range,
      onStart: () => activateTab(tabId),
      // Sobre o outro grupo, as vizinhas voltam ao lugar: a aba sai desta faixa.
      onDrag: (d, x, y) => {
        const into = otherStrip(x, y);
        setTabDrag({ id: tabId, group: groupId, from, into, ...d, target: into ? from : d.target });
      },
      onDrop: (target, x, y) => {
        setTabDrag(null);
        if (target === null) return;
        const into = otherStrip(x, y);
        if (!into) {
          if (target !== from) setWs(moveTab(wsRef.current, tabId, groupId, target));
          return;
        }
        // Entra antes da primeira aba do destino cujo meio fica à direita do ponteiro.
        const dest = strips.current.get(into)?.el;
        const index = dest ? [...dest.querySelectorAll<HTMLElement>('[data-tab]')].filter((node) => {
          const rect = node.getBoundingClientRect();
          return rect.left + rect.width / 2 < x;
        }).length : undefined;
        setWs(moveTab(wsRef.current, tabId, into, index));
      },
    });
  };

  // ---- Teclado (só dentro da área; terminal e campos fora dela não passam por aqui) ----
  // Captura (não bolha): o CodeMirror trata Ctrl+K (apagar até o fim da linha) na fase de bolha, então este handler vê a tecla antes
  // e pode parar a propagação. Só alcança o que está dentro desta área; o xterm e os campos ficam fora dela ou são pulados abaixo.
  // Acordes: Ctrl+K e depois W (fecha as não fixadas do grupo ativo), Ctrl+Shift+C (caminho relativo), Ctrl+Shift+Enter não existe:
  // Shift+Enter (fixa/desafixa). O prefixo vale 1,5 s e cai ao sair da área.
  const { pending: chordPending, begin: beginChord, end: endChord } = useChord();
  const focusedTab = (): EditorTab | null => {
    const group = wsRef.current.groups.find((g) => g.id === wsRef.current.focus);
    return group ? activeTabOf(group) : null;
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    const inField = (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) && !target.closest('.cm-editor');
    if (inField || target.closest('[role="menu"], [role="listbox"], [role="dialog"]')) return;
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
    const mod = e.ctrlKey || e.metaKey;
    const focused = focusedTab();
    // Tecla sem ação (ou sem alvo) não é capturada: segue para o editor, como qualquer outra.
    let run: (() => void) | null = null;
    if (chordPending.current) {
      // Ctrl+K de novo reinicia o acorde, sem chegar ao editor.
      if (e.ctrlKey && !e.shiftKey && !e.altKey && e.code === 'KeyK') run = beginChord;
      else {
        endChord();
        if (!mod && !e.altKey && !e.shiftKey && e.code === 'KeyW') run = () => void closeFlow({ kind: 'all', group: wsRef.current.focus });
        else if (e.ctrlKey && e.shiftKey && !e.altKey && e.code === 'KeyC' && focused) run = () => void copyPath('relative', focused.target);
        else if (!mod && !e.altKey && e.shiftKey && e.code === 'Enter' && focused) run = () => setWs(togglePin(wsRef.current, focused.id));
      }
    } else if (mod && !e.shiftKey && !e.altKey && e.code === 'KeyK') run = beginChord;
    // Ctrl+N: rascunho novo no grupo com foco. Ctrl+N não tem ação no editor (CodeMirror) nem no resto do app.
    else if (mod && !e.shiftKey && !e.altKey && e.code === 'KeyN') run = () => newDraft(wsRef.current.focus);
    else if (mod && !e.shiftKey && !e.altKey && (e.key === 'w' || e.key === 'F4') && focused) run = () => void closeFlow({ kind: 'one', tabId: focused.id });
    else if (e.altKey && e.shiftKey && !mod && e.code === 'KeyC' && focused) run = () => void copyPath('absolute', focused.target);
    else if (e.ctrlKey && e.key === 'Tab') run = () => setWs(navigate(wsRef.current, e.shiftKey ? -1 : 1));
    else if (mod && e.shiftKey && (e.key === 'T' || e.key === 't')) run = reopen;
    else if (mod && e.key === '\\') run = () => { if (focused) split_(focused.id); };
    if (!run) return;
    e.preventDefault();
    e.stopPropagation();
    run();
  };
  // Sair da área (foco foi para fora, p. ex. o terminal) cancela o acorde pendente.
  const onKeyLeave = (e: FocusEvent<HTMLDivElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) endChord();
  };

  // Avisa o App quando o número de grupos muda (ele reserva a largura central de cada um).
  useEffect(() => { onGroupCount?.(ws.groups.length); }, [ws.groups.length]);

  useImperativeHandle(handle, () => ({
    newFile: () => newDraft(wsRef.current.focus),
    saveActive,
    saveActiveAs,
    openFile,
    prefetch,
    prefetchTarget,
    openTarget,
    openBeside,
    remap,
    remove,
    hasDraft: (path: string) => hasDraftAt(dataRef.current, path),
    unsaved: () => dirtyList(),
    saveAll,
    openPaths: () => [...new Set(allTabs().flatMap((t) => (t.target.kind === 'draft' ? [] : [t.target.path])))],
    refresh: () => {
      const ids = new Set(allTabs().flatMap((t) => (t.target.kind === 'file' ? [t.target.docId] : [])));
      ids.forEach((id) => void syncFile(id));
    },
    flushRecovery: () => persistRef.current(),
    selectedText: () => {
      const tab = focusedTab();
      if (!tab || (tab.target.kind !== 'file' && tab.target.kind !== 'draft')) return '';
      // A vista sai do corpo da aba (`data-body`, um por aba): o id de aba é único no workspace.
      const body = document.querySelector<HTMLElement>(`[data-body="${tab.id}"]`);
      const view = body && EditorView.findFromDOM(body);
      if (!view) return '';
      const { state } = view;
      const main = state.selection.main;
      // Sem seleção, a palavra sob o cursor (como o Ctrl+Shift+F do VS Code). Fora de palavra, nada.
      const range = main.empty ? state.wordAt(main.head) : main;
      if (!range) return '';
      const text = state.sliceDoc(range.from, range.to);
      return text.includes('\n') ? '' : text;
    },
    discardAll: () => {
      const current = dataRef.current;
      const buffers: Record<string, Buffer> = {};
      for (const [id, b] of Object.entries(current.buffers)) buffers[id] = isBufferDirty(b) ? { ...b, draft: normalizeNewlines(b.raw) } : b;
      const conflicts: Record<string, ConflictDraft> = {};
      for (const [key, c] of Object.entries(current.conflicts)) conflicts[key] = { ...c, draft: c.origin };
      setData({ buffers, conflicts });
      // Aba de conflito sem rascunho continua aberta; o recovery segue o estado acima na próxima gravação.
    },
  }));

  // ---- Comparar antes de gravar sobre o disco (ou de descartar a edição pelo disco) ----
  // Abre o modal com as duas versões como estão agora. Sem versão do disco (já resolvida), não há o que comparar.
  // Gravar a edição sobre o disco exige arquivo gravável: sem isso, o modal nem abre.
  const openCompare = (docId: string, path: string, mode: 'keep' | 'use') => {
    const buffer = dataRef.current.buffers[docId];
    const version = disk[docId];
    if (!buffer || !version) return;
    if (mode === 'keep' && version.readOnly) {
      toast.error({ title: 'Arquivo somente leitura: a edição não pode ser gravada aqui.', duration: 5000 });
      return;
    }
    setCompare({ docId, path, mode, disk: version, draft: buffer.draft });
  };
  // Confirmação do modal. "Usar o disco" troca o rascunho pela versão comparada. "Salvar" grava a edição comparada sobre a base que
  // o usuário viu: se o arquivo mudou de novo depois da comparação, writeFileChecked recusa, nada é gravado e a comparação é refeita.
  const confirmCompare = async () => {
    const choice = compare;
    if (!choice) return;
    setCompare(null);
    const buffer = dataRef.current.buffers[choice.docId];
    if (!buffer) return;
    if (choice.mode === 'use') {
      const version = choice.disk;
      patchBuffer(choice.docId, (b) => ({ ...b, raw: version.text, bom: version.bom, readOnly: version.readOnly, newline: newlineOf(version.text), draft: normalizeNewlines(version.text) }));
      clearDisk(choice.docId);
      return;
    }
    if (saving.current.has(choice.docId)) return;
    saving.current.add(choice.docId);
    const sent = toDiskNewlines(choice.draft, buffer.newline);
    const failed = await backend.writeFileChecked(buffer.path, sent, choice.disk.text, choice.disk.bom)
      .then(() => {
        // Quem digitou durante o await continua sujo: o `raw` vira o texto gravado, e o `draft` atual fica como está.
        patchBuffer(choice.docId, (b) => ({ ...b, raw: sent, bom: choice.disk.bom }));
        clearDisk(choice.docId);
        setStatusOf(choice.docId, {});
        refreshGit();
        notifySaved();
        return null;
      })
      .catch((e: unknown) => e)
      .finally(() => {
        saving.current.delete(choice.docId);
      });
    if (failed) {
      toast.error({ title: message(failed), duration: 7000 });
      // Refaz a versão do disco: a próxima comparação mostra o que mudou, não a versão antiga.
      void syncFile(choice.docId);
    }
  };

  // ---- Corpo das abas ----
  const common: Common = {
    active, locked, guard, refreshGit,
    openLine: (path, line) => openFile(path, { permanent: true, line }),
    hasDraft: (path) => hasDraftAt(dataRef.current, path),
    fileDirty: (path) => Object.values(dataRef.current.buffers).some((b) => isBufferDirty(b) && b.path.toLowerCase() === path.toLowerCase()),
    ask,
  };

  const tabMenu = (tab: EditorTab): ContextMenuItem[] => {
    const target = tab.target;
    const noPath = 'O rascunho ainda não tem caminho. Salve para ter um.';
    const dirtySet = dirtyNow();
    const dirty = isDirty(target, dirtySet);
    const buffer = target.kind === 'file' ? dataRef.current.buffers[target.docId] : undefined;
    const groupOf = wsRef.current.groups.find((g) => g.tabs.some((t) => t.id === tab.id));
    const others = groupOf ? groupOf.tabs.filter((t) => t.id !== tab.id && !t.pinned).length : 0;
    const right = groupOf ? groupOf.tabs.slice(groupOf.tabs.findIndex((t) => t.id === tab.id) + 1).filter((t) => !t.pinned).length : 0;
    const saved = groupOf ? groupOf.tabs.filter((t) => !t.pinned && !isDirty(t.target, dirtySet)).length : 0;
    const all = groupOf ? groupOf.tabs.filter((t) => !t.pinned).length : 0;
    const closeBatch = (scope: CloseScope) => void closeFlow(scope);
    // Rascunho salva sempre (abre o "Salvar como", mesmo vazio); os outros só com alteração.
    const saveBlock = target.kind !== 'file' && target.kind !== 'conflict' && target.kind !== 'draft'
      ? blocked('save', 'Salvar', 'Esta aba não tem texto para salvar.')
      : target.kind !== 'draft' && !dirty ? blocked('save', 'Salvar', 'Não há alterações não salvas nesta aba.')
        : buffer?.readOnly ? blocked('save', 'Salvar', 'Arquivo somente leitura.')
          : action('save', 'Salvar', () => void saveTab(target), 'Ctrl+S');
    const sections: ContextMenuItem[][] = [
      [
        saveBlock,
        action('save-all', 'Salvar Tudo', () => void saveAll()),
      ],
      [
        action('close', 'Fechar', () => closeBatch({ kind: 'one', tabId: tab.id }), 'Ctrl+F4'),
        others > 0 ? action('close-others', 'Fechar Outras', () => closeBatch({ kind: 'others', tabId: tab.id }))
          : blocked('close-others', 'Fechar Outras', 'Não há outras abas para fechar. As fixadas ficam abertas.'),
        right > 0 ? action('close-right', 'Fechar à Direita', () => closeBatch({ kind: 'right', tabId: tab.id }))
          : blocked('close-right', 'Fechar à Direita', 'Não há abas à direita para fechar.'),
        saved > 0 ? action('close-saved', 'Fechar Salvas', () => closeBatch({ kind: 'saved', group: groupOf?.id ?? 'A' }))
          : blocked('close-saved', 'Fechar Salvas', 'Não há abas salvas para fechar.'),
        all > 0 ? action('close-all', 'Fechar Todas', () => closeBatch({ kind: 'all', group: groupOf?.id ?? 'A' }), 'Ctrl+K W')
          : blocked('close-all', 'Fechar Todas', 'Não há abas para fechar. As fixadas ficam abertas.'),
      ],
      [
        target.kind === 'draft' ? blocked('copy-relative', 'Copiar Caminho Relativo', noPath)
          : action('copy-relative', 'Copiar Caminho Relativo', () => void copyPath('relative', target), 'Ctrl+K Ctrl+Shift+C'),
        target.kind === 'draft' ? blocked('copy-path', 'Copiar o Caminho', noPath)
          : action('copy-path', 'Copiar o Caminho', () => void copyPath('absolute', target), 'Shift+Alt+C'),
        target.kind === 'file' && cursors.current[target.docId]
          ? action('copy-navigation', 'Copiar Caminho de Navegação', () => void copyPath('navigation', target))
          : blocked('copy-navigation', 'Copiar Caminho de Navegação', 'Abra o arquivo para ter a posição do cursor.'),
      ],
      [
        action('split', 'Dividir à Direita', () => split_(tab.id), 'Ctrl+\\'),
        action('move', 'Mover para o Outro Grupo', () => moveToOther(tab.id)),
        groupOf && wsRef.current.groups.length === 2 ? action('join', 'Unir Grupos', join_) : blocked('join', 'Unir Grupos', 'Só há um grupo.'),
        wsRef.current.closed.length > 0 ? action('reopen', 'Reabrir Fechada', reopen, 'Ctrl+Shift+T') : blocked('reopen', 'Reabrir Fechada', 'Não há aba fechada para reabrir.'),
      ],
      [
        action('pin', tab.pinned ? 'Desafixar' : 'Fixar', () => setWs(togglePin(wsRef.current, tab.id)), 'Ctrl+K Shift+Enter'),
        target.kind === 'draft' ? blocked('reveal', 'Revelar no Explorer', noPath)
          : target.kind === 'file' && isAbsolutePath(target.path) && relativeTo(project, target.path) === null
            ? blocked('reveal', 'Revelar no Explorer', 'Este arquivo está fora da pasta do projeto.')
          : action('reveal', 'Revelar no Explorer', () => onReveal(target.path)),
      ],
    ];
    return sections.flatMap((items, i) => (i ? [DIVIDER, ...items] : items));
  };

  const bodyOf = (tab: EditorTab, groupActive: boolean, shown: boolean): ReactNode => {
    const target = tab.target;
    if (target.kind === 'diff') return <DiffTab key={targetKey(target)} target={target} common={common} shown={shown} read={gitReads.current.get(targetKey(target))} />;
    if (target.kind === 'commit') return <CommitTab key={targetKey(target)} target={target} read={gitReads.current.get(targetKey(target))} />;
    if (target.kind === 'conflict') {
      return (
        <ConflictTab
          key={targetKey(target)}
          path={target.path}
          common={common}
          shown={shown}
          draft={data.conflicts[conflictKey(target.path)]}
          putConflict={putConflict}
          dropConflict={dropConflict}
          save={(markResolved) => saveConflict(target.path, markResolved)}
          closeTab={() => { const found = allTabs().find((t) => t.target.kind === 'conflict' && t.target.path.toLowerCase() === target.path.toLowerCase()); if (found) void closeFlow({ kind: 'one', tabId: found.id }); }}
        />
      );
    }
    if (target.kind === 'file' && mediaOf(target.path)) return <MediaView path={target.path} shown={shown} />;
    return (
      <FileBody
        project={project}
        active={active}
        buffer={data.buffers[target.docId]}
        shown={shown}
        status={status[target.docId]}
        disk={disk[target.docId]}
        reveal={reveal && reveal.docId === target.docId && groupActive ? reveal : null}
        restore={() => ({
          cursor: cursors.current[target.docId] ?? null,
          scroll: scrolls.current[target.docId] ?? null,
          focus: target.kind === 'draft' && freshDraft.current === target.docId,
        })}
        onChange={(value) => {
          patchBuffer(target.docId, (b) => {
            const next = { ...b, draft: value };
            // Igual ao disco: volta ao fim de linha do arquivo. `isBufferDirty` compara sem normalizar o `raw` a cada tecla.
            return isBufferDirty(next) ? next : { ...next, newline: newlineOf(b.raw) };
          });
          // Editar torna a aba permanente (a prévia vira aba de verdade). Só as abas com este docId são tocadas.
          const current = wsRef.current;
          let next = current;
          for (const g of current.groups) for (const t of g.tabs) if (t.target.kind === 'file' && t.target.docId === target.docId && t.preview) next = markEdited(next, t.id);
          if (next !== current) setWs(next);
        }}
        onFormatEol={(newline) => patchBuffer(target.docId, (b) => ({ ...b, newline }))}
        onSave={() => void (target.kind === 'draft' ? saveDraft(target.docId) : saveBuffer(target.docId))}
        onCursor={(line, col) => {
          const prev = cursors.current[target.docId];
          if (prev && prev.line === line && prev.col === col) return;
          cursors.current[target.docId] = { line, col };
          persistSoon();
        }}
        onScroll={(value) => {
          scrolls.current[target.docId] = value;
          persistSoon();
        }}
        onCompare={(mode) => openCompare(target.docId, target.kind === 'file' ? target.path : '', mode)}
        onRetry={() => void syncFile(target.docId)}
      />
    );
  };

  // ---- Grupos ----
  const dirty = dirtyKeys(data);
  const activeTabOf = (g: { active: string | null; tabs: EditorTab[] }) => g.tabs.find((t) => t.id === g.active) ?? null;
  const titleOf = (target: TabTarget) => {
    if (target.kind === 'draft') return draftName(target.num);
    if (target.kind === 'file') return fileName(target.path);
    if (target.kind === 'diff') return `${fileName(target.path)} · ${target.side === 'staged' ? 'preparadas' : 'alterações'}`;
    if (target.kind === 'commit') return `${target.hash.slice(0, 7)} · ${fileName(target.path)}`;
    return `${fileName(target.path)} · conflito`;
  };
  const isDirtyTab = (target: TabTarget) => isDirty(target, dirty);

  // Faixa de cada grupo: a aba ativa entra na área visível quando muda, abre, fica suja/limpa ou a faixa muda de largura.
  // Mexe só no scroll da faixa (sem scrollIntoView, que rola os ancestrais); rolar à mão não é desfeito por re-renderização.
  const strips = useRef(new Map<GroupId, { el: HTMLDivElement; observer: ResizeObserver }>());
  const revealKey = useRef(new Map<GroupId, string>());
  const revealActive = (groupId: GroupId, force = false) => {
    const strip = strips.current.get(groupId);
    const group = wsRef.current.groups.find((g) => g.id === groupId);
    const active = group ? activeTabOf(group) : null;
    if (!strip || !active) return;
    const key = `${active.id}|${group?.tabs.length}|${isDirtyTab(active.target)}|${active.pinned}`;
    if (!force && revealKey.current.get(groupId) === key) return;
    revealKey.current.set(groupId, key);
    revealTab(strip.el, [...strip.el.querySelectorAll<HTMLElement>('[data-tab]')].find((el) => el.dataset.tab === active.id));
  };
  // O observer roda fora do render: chama a versão atual de `revealActive` (ela lê `dirty` e o estado).
  const revealLatest = useRef(revealActive);
  useLayoutEffect(() => {
    revealLatest.current = revealActive;
  });
  const stripRef = (groupId: GroupId) => (el: HTMLDivElement | null) => {
    strips.current.get(groupId)?.observer.disconnect();
    strips.current.delete(groupId);
    if (!el) return;
    const observer = new ResizeObserver(() => revealLatest.current(groupId, true));
    observer.observe(el);
    strips.current.set(groupId, { el, observer });
  };
  // Um ref de callback por grupo, criado uma vez: a identidade fica estável e o observer não é refeito a cada render.
  const [stripRefs] = useState(() => ({ A: stripRef('A'), B: stripRef('B') }));
  useLayoutEffect(() => {
    for (const group of ws.groups) revealActive(group.id);
  });

  // Faixa de abas de cada grupo, nas medidas do Antigravity: abas de 30 px com divisor à direita; a ativa tem o fundo do editor e uma
  // borda de 1 px em cima (na cor de destaque com o grupo em foco). À direita, a lista de abas, Dividir à Direita e Mais Ações (`...`).
  const renderStrip = (groupId: GroupId) => {
    const group = ws.groups.find((g) => g.id === groupId);
    if (!group) return null;
    const current = activeTabOf(group);
    const focusedHere = ws.focus === groupId;
    // Rascunho não tem pasta para diferenciar: fica fora do cálculo de detalhe.
    const details = detailsOf(ws.groups.flatMap((g) => g.tabs.flatMap((t) => (t.target.kind === 'draft' ? [] : [t.target.path]))));
    // Setas andam entre as abas da faixa (a ativação é no clique, Enter ou Espaço), como uma lista de abas.
    const onTabKey = (e: KeyboardEvent<HTMLButtonElement>) => {
      if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight' && e.key !== 'Home' && e.key !== 'End') return;
      const items = [...(e.currentTarget.closest('[role="tablist"]')?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? [])];
      const at = items.indexOf(e.currentTarget);
      const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 : at + (e.key === 'ArrowRight' ? 1 : -1);
      const target = items[Math.min(Math.max(next, 0), items.length - 1)];
      if (!target) return;
      e.preventDefault();
      target.focus();
    };
    const pinnedFree = group.tabs.filter((t) => !t.pinned);
    const tabList: TabListItem[] = group.tabs.map((tab) => {
      const detail = tab.target.kind === 'draft' ? '' : (details.get(tab.target.path) ?? '');
      return {
        key: tab.id, title: labelOf(tab.target), dirty: isDirtyTab(tab.target),
        icon: tab.pinned ? <Pin className="size-4 text-[var(--ant-color-text-secondary)]" /> : tabIcon(tab.target),
        label: <>{titleOf(tab.target)}{detail && <span className="text-[12px] text-[var(--ant-color-text-secondary)]">&ensp;{detail}</span>}</>,
        onSelect: () => activateTab(tab.id), onClose: () => void closeFlow({ kind: 'one', tabId: tab.id }),
      };
    });
    const groupMenu = (): ContextMenuItem[] => [
      pinnedFree.length > 0 ? action('close-all', 'Fechar Todas', () => void closeFlow({ kind: 'all', group: groupId }), 'Ctrl+K W')
        : blocked('close-all', 'Fechar Todas', 'Não há abas para fechar. As fixadas ficam abertas.'),
      pinnedFree.some((t) => !isDirtyTab(t.target)) ? action('close-saved', 'Fechar Salvas', () => void closeFlow({ kind: 'saved', group: groupId }))
        : blocked('close-saved', 'Fechar Salvas', 'Não há abas salvas para fechar.'),
      DIVIDER,
      action('save-all', 'Salvar Tudo', () => void saveAll()),
      wsRef.current.closed.length > 0 ? action('reopen', 'Reabrir Fechada', reopen, 'Ctrl+Shift+T') : blocked('reopen', 'Reabrir Fechada', 'Não há aba fechada para reabrir.'),
      wsRef.current.groups.length === 2 ? action('join', 'Unir Grupos', join_) : blocked('join', 'Unir Grupos', 'Só há um grupo.'),
    ];
    return (
      <div
        className={`relative flex h-[30px] flex-none ${tabDrag?.into === groupId ? 'bg-[var(--ant-color-primary-bg)]' : 'bg-[var(--ant-color-bg-layout)]'}`}
        data-group={groupId}
      >
        <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-border" />
        <div
          ref={stripRefs[groupId]} role="tablist" aria-label={`Abas do grupo ${groupId}`}
          className="@container relative flex min-w-0 flex-1 items-stretch overflow-x-auto [scrollbar-width:none]!"
          onWheel={wheelToScroll}
          // Duplo clique na faixa, fora das abas: rascunho novo neste grupo (como no VS Code). Em cima de uma aba, não faz nada.
          onDoubleClick={(e) => { if (!(e.target as HTMLElement).closest('[data-tab]')) newDraft(groupId); }}
        >
          {group.tabs.map((tab, index) => {
            const active = tab.id === current?.id;
            const dirtyTab = isDirtyTab(tab.target);
            const detail = tab.target.kind === 'draft' ? '' : (details.get(tab.target.path) ?? '');
            // Vizinhas entre a origem e o destino abrem o espaço da arrastada, com transição; ao soltar, tudo volta a 0 junto com a ordem nova.
            const dragHere = tabDrag?.group === groupId ? tabDrag : null;
            const shift = dragHere ? dragShift(dragHere, dragHere.from, index) : 0;
            return (
              <ContextMenuTrigger key={tab.id} items={() => tabMenu(tab)}>
                <div
                  data-tab={tab.id}
                  style={shift ? { transform: `translateX(${shift}px)` } : undefined}
                  onPointerDown={(e) => dragTab(e, groupId, tab.id)}
                  className={`group/tab relative ${dragHere?.id === tab.id ? 'z-20' : dragHere ? 'z-[1] transition-transform duration-150 motion-reduce:transition-none' : 'z-[1]'} flex h-full max-w-[min(14rem,100cqw)] flex-none items-center border-r border-border ${active ? 'bg-[var(--ant-color-bg-container)]' : 'hover:bg-[var(--ant-color-bg-container)]'}`}
                  onKeyDown={(e) => {
                    if (!((e.key === 'F10' && e.shiftKey) || e.key === 'ContextMenu')) return;
                    e.preventDefault();
                    const rect = e.currentTarget.getBoundingClientRect();
                    e.currentTarget.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rect.left + 16, clientY: rect.bottom }));
                  }}
                >
                  {/* A aba ativa cobre a linha de baixo da faixa (fica "aberta" para o editor) e tem a borda de cima. */}
                  {active && <span aria-hidden className="pointer-events-none absolute inset-x-0 -bottom-px h-px bg-[var(--ant-color-bg-container)]" />}
                  {active && <span aria-hidden className="pointer-events-none absolute inset-x-0 top-0 h-px" style={{ background: focusedHere ? 'var(--ant-color-primary)' : 'var(--ant-color-text-quaternary)' }} />}
                  <button
                    type="button" role="tab" aria-selected={active} tabIndex={active ? 0 : -1} title={labelOf(tab.target)}
                    onClick={() => activateTab(tab.id)} onKeyDown={onTabKey}
                    className={`flex h-full min-w-0 items-center gap-1.5 pr-7 pl-2.5 text-left text-[13px] outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset ${tab.preview ? 'italic' : ''} ${active ? 'text-[var(--ant-color-text)]' : 'text-[var(--ant-color-text-secondary)]'}`}
                  >
                    {tab.pinned
                      ? <Pin className="size-4 shrink-0 text-[var(--ant-color-text-secondary)]" aria-hidden />
                      : tabIcon(tab.target)}
                    <span className={`min-w-0 truncate ${tab.preview ? 'pr-0.5' : ''}`}>{titleOf(tab.target)}</span>
                    {detail && (
                      <span className={`min-w-0 truncate text-[12px] text-[var(--ant-color-text-secondary)] ${tab.preview ? 'pr-0.5' : ''}`}>{detail}</span>
                    )}
                    {tab.pinned && <span className="sr-only">(fixada)</span>}
                    {tab.preview && <span className="sr-only">(prévia)</span>}
                  </button>
                  {/* Como no VS Code: o X aparece na aba ativa e com o ponteiro; com alteração não salva, o ● fica no lugar dele até o ponteiro chegar. */}
                  {dirtyTab && (
                    <span
                      aria-label="Alteração não salva" title="Alteração não salva"
                      className="pointer-events-none absolute top-1/2 right-1 flex size-5 -translate-y-1/2 items-center justify-center text-[13px] group-hover/tab:opacity-0"
                    >
                      ●
                    </span>
                  )}
                  <ActionIcon
                    variant="borderless" icon={X} size={{ size: 16, blockSize: 20, borderRadius: 5 }}
                    aria-label={`Fechar ${labelOf(tab.target)}`} title="Fechar (Ctrl+F4)"
                    className={`absolute! top-1/2 right-1 z-10 -translate-y-1/2! focus-visible:opacity-100 group-hover/tab:opacity-100 ${active && !dirtyTab ? 'opacity-100' : 'opacity-0'}`}
                    onClick={() => void closeFlow({ kind: 'one', tabId: tab.id })}
                  />
                </div>
              </ContextMenuTrigger>
            );
          })}
        </div>
        <div className="flex flex-none items-center gap-0.5 pr-2 pl-1" role="toolbar" aria-label="Ações de editor">
          {group.tabs.length > 0 && <TabListMenu items={tabList} active={current?.id ?? null} />}
          <ActionIcon
            variant="borderless" size="small" icon={Columns2} aria-label="Dividir o Editor à Direita" title="Dividir o Editor à Direita (Ctrl+\)"
            disabled={!current} onClick={() => { if (current) split_(current.id); }}
          />
          <DropdownMenu items={groupMenu} placement="bottomRight">
            <ActionIcon variant="borderless" size="small" icon={Ellipsis} aria-label="Mais Ações..." title="Mais Ações..." />
          </DropdownMenu>
        </div>
      </div>
    );
  };

  const renderGroup = (groupId: GroupId) => {
    const group = ws.groups.find((g) => g.id === groupId);
    if (!group) return null;
    const current = activeTabOf(group);
    return (
      <Flexbox flex={1} height="100%" className="min-w-0 min-h-0" data-pane={groupId} onMouseDown={() => { if (ws.focus !== groupId) setWs({ ...wsRef.current, focus: groupId }); }}>
        {renderStrip(groupId)}
        {/* Breadcrumbs só nas abas de arquivo: diff, commit e conflito já têm a faixa própria com o caminho. */}
        {current?.target.kind === 'file' && <Breadcrumbs path={current.target.path} onOpen={(path) => openFile(path)} />}
        <Flexbox flex={1} className="min-h-0 relative">
          {!current && <EmptyGroup onNewDraft={() => newDraft(groupId)} />}
          {group.tabs.map((tab) => (
            <div key={tab.id} className="absolute inset-0 flex flex-col min-h-0" style={{ display: tab.id === current?.id ? 'flex' : 'none' }} data-body={tab.id}>
              {bodyOf(tab, tab.id === current?.id && ws.focus === groupId, tab.id === current?.id)}
            </div>
          ))}
        </Flexbox>
      </Flexbox>
    );
  };

  // Renderiza os grupos que existem (1 ou 2), não A/B fixos: um grupo só ocupa a área inteira, mesmo que seja o B.
  const twoGroups = ws.groups.length === 2;
  const showHome = !project && ws.groups.every((group) => group.tabs.length === 0);
  return (
    <Flexbox height="100%" className="min-h-0" onKeyDownCapture={onKey} onBlurCapture={onKeyLeave}>
      {notice && (
        <Flexbox horizontal align="center" gap={8} paddingInline={12} height={28} flex="none" className="border-b">
          <Text as="span" role="status" type="secondary" fontSize={12} ellipsis className="min-w-0">{notice}</Text>
          <ActionIcon variant="borderless" size="small" icon={X} className="ml-auto shrink-0" aria-label="Fechar o aviso" onClick={() => setNotice('')} />
        </Flexbox>
      )}
      <Flexbox horizontal flex={1} className="min-h-0" height="100%">
        {showHome ? (
          <HomeScreen onNewFile={() => newDraft(wsRef.current.focus)} />
        ) : ws.groups.map((group, index) => (
          <Fragment key={group.id}>
            {index === 1 && (
              <div
                role="separator" aria-orientation="vertical" aria-label="Divisor dos grupos" title="Arrastar para redimensionar"
                className="w-1 shrink-0 cursor-col-resize bg-transparent hover:bg-border"
                onPointerDown={(e) => {
                  const box = e.currentTarget.parentElement;
                  if (!box) return;
                  e.currentTarget.setPointerCapture(e.pointerId);
                  splitDrag.current = { startX: e.clientX, start: splitRef.current, width: box.clientWidth };
                }}
                onPointerMove={splitMove}
                onPointerUp={() => { splitDrag.current = null; }}
              />
            )}
            <div className="flex min-w-0 min-h-0" style={{ flex: twoGroups ? (index === 0 ? split : 1 - split) : 1 }}>{renderGroup(group.id)}</div>
          </Fragment>
        ))}
      </Flexbox>
      <Modal
        open={compare !== null}
        title={compare?.mode === 'use' ? 'Usar a versão do disco?' : 'Salvar sua edição sobre o disco?'}
        width={960}
        closable={false}
        keyboard
        onCancel={() => setCompare(null)}
        footer={compare && (
          <Flexbox horizontal wrap="wrap" justify="flex-end" gap={8} width="100%">
            <Button size="large" onClick={() => setCompare(null)}>Cancelar</Button>
            <Button size="large" type={compare.mode === 'use' ? undefined : 'fill'} danger={compare.mode === 'use'} onClick={() => void confirmCompare()}>
              {compare.mode === 'use' ? 'Descartar minha edição e usar o disco' : 'Salvar minha edição sobre o disco'}
            </Button>
          </Flexbox>
        )}
      >
        {compare && <CompareView path={compare.path} disk={compare.disk.text} draft={compare.draft} mode={compare.mode} />}
      </Modal>
      <Modal
        open={askState !== null}
        title={askState?.title}
        width={480}
        closable={false}
        keyboard
        onCancel={() => { askState?.resolve(null); setAskState(null); }}
        footer={askState && (
          <Flexbox horizontal wrap="wrap" justify="flex-end" gap={8} width="100%">
            <Button onClick={() => { askState.resolve(null); setAskState(null); }}>Cancelar</Button>
            {askState.actions.map((item) => (
              <Button key={item.key} type={item.danger ? undefined : 'fill'} danger={item.danger} onClick={() => { askState.resolve(item.key); setAskState(null); }}>
                {item.label}
              </Button>
            ))}
          </Flexbox>
        )}
      >
        {/* `div`, não `p`: o corpo pode ser um bloco (o patch do descarte é um `pre`, e `p` não aceita bloco dentro). */}
        <Text as="div" type="secondary" fontSize={14} lineHeight={1.6} style={{ margin: 0, overflowWrap: 'anywhere' }}>{askState?.body}</Text>
      </Modal>
    </Flexbox>
  );
});

// ---- Grupos vazios ----

// Duplo clique em qualquer ponto da área abre um rascunho.
function EmptyGroup({ onNewDraft }: { onNewDraft: () => void }) {
  return (
    <Center data-empty flex={1} className="min-h-0 overflow-hidden select-none" onDoubleClick={onNewDraft}>
      <EmptyMark />
    </Center>
  );
}

// Sem projeto: a mesma marca apagada. Duplo clique abre um rascunho.
function HomeScreen({ onNewFile }: { onNewFile: () => void }) {
  return (
    <Center data-home flex={1} className="min-w-0 min-h-0 overflow-hidden select-none" onDoubleClick={onNewFile}>
      <EmptyMark />
    </Center>
  );
}

function EmptyMark() {
  return <AppMark color="#ECEAE4" className="size-60 max-h-[35vh] max-w-[35vw] opacity-[0.06]" />;
}

// ---- Arquivo ----

// Texto-base do Git do arquivo aberto, para os marcadores de mudança do editor: lido ao montar, logo depois de salvar (o `raw` do buffer
// muda) e a cada consulta com a aba visível (pega commit, stage e checkout feitos pelo painel Git, terminais e executores). Sem caminho
// (rascunho), `null` ou erro: sem marcadores e sem aviso. Os textos chegam com o fim de linha do checkout e o editor trabalha com `\n`,
// então a base passa pela mesma normalização. O estado só muda quando a base de fato mudou; `undefined` é "ainda não lida". `reload` lê
// na hora (depois de preparar um bloco no editor, o marcador fica vazado sem esperar a consulta).
function useGitBase(path: string, raw: string, shown: boolean, active: boolean): {
  base: FileBase | null | undefined; reload: () => Promise<void> } {
  const backend = useBackend();
  const [base, setBase] = useState<FileBase | null | undefined>(undefined);
  const pathRef = useRef(path);
  useLayoutEffect(() => {
    pathRef.current = path;
  });
  const load = async () => {
    // Erro de Git não interrompe a edição: só tira os marcadores.
    const found = await backend.gitFileBase(path).catch(() => null);
    const next: FileBase | null = found
      ? { head: found.head === null ? null : normalizeNewlines(found.head), index: found.index === null ? null : normalizeNewlines(found.index) }
      : null;
    if (pathRef.current !== path) return;
    setBase((prev) => (prev === next || (prev && next && prev.head === next.head && prev.index === next.index) ? prev : next));
  };
  usePolling(load, POLL_MS, active && shown && path !== '' && !isAbsolutePath(path) && path);
  const seenRaw = useRef(raw);
  useEffect(() => {
    if (seenRaw.current === raw) return;
    seenRaw.current = raw;
    if (path !== '' && !isAbsolutePath(path)) void load();
  }, [raw]);
  return { base: path === '' || isAbsolutePath(path) ? null : base, reload: load };
}

function FileBody({
  project, active, buffer, shown, status, disk, reveal, restore, onChange, onFormatEol, onSave, onCursor, onCompare, onRetry, onScroll }: {
  project: string; active: boolean;
  buffer: Buffer | undefined; shown: boolean; status: Status | undefined; disk: DiskVersion | undefined;
  reveal: Reveal | null; restore: () => EditorRestore; onChange: (value: string) => void; onSave: () => void;
  onFormatEol: (newline: Newline) => void;
  onCursor: (line: number, col: number) => void; onCompare: (mode: 'keep' | 'use') => void; onRetry: () => void;
  onScroll: (scroll: Scroll) => void;
}) {
  const backend = useBackend();
  const git = useGitBase(buffer?.path ?? '', buffer?.raw ?? '', shown, active);
  if (status?.binary) return <Note text="Arquivo binário: não há texto para mostrar aqui." />;
  if (status?.large) return <Note text="Arquivo grande demais para mostrar aqui." />;
  if (!buffer) return status?.error ? <Note text={status.error} danger onRetry={onRetry} /> : <Reading text="Lendo o arquivo…" />;
  const notes: string[] = [];
  if (buffer.readOnly) notes.push('Arquivo não está em UTF-8; somente leitura.');
  if (status?.error) notes.push(status.error);
  return (
    <Flexbox flex={1} className="min-h-0">
      {notes.length > 0 && (
        <Flexbox horizontal align="center" wrap="wrap" gap={8} paddingInline={12} height={32} flex="none" className="border-b">
          <Text as="span" role="status" type={status?.error ? 'danger' : 'secondary'} fontSize={12} ellipsis className="min-w-0">{notes.join(' ')}</Text>
        </Flexbox>
      )}
      {disk && (
        <Flexbox horizontal align="center" wrap="wrap" gap={8} paddingInline={12} height={32} flex="none" className="border-b">
          <Text as="span" role="alert" type="danger" fontSize={12}>O arquivo mudou no disco e você tem alterações.</Text>
          <Button size="small" onClick={() => onCompare('use')}>Comparar e usar o disco</Button>
          <Button size="small" onClick={() => onCompare('keep')}>Comparar e salvar minha edição</Button>
        </Flexbox>
      )}
      <Flexbox flex={1} className="min-h-0">
        <Flexbox flex={1} className="min-h-0 overflow-hidden">
          <CodeEditor
            text={buffer.draft}
            readOnly={buffer.readOnly}
            path={absoluteFilePath(project, buffer.path)}
            reveal={reveal}
            onChange={onChange}
            onFormatEol={onFormatEol}
            onSave={onSave}
            onCursor={onCursor}
            onScroll={onScroll}
            restore={restore}
            cacheKey={buffer.docId}
            gitBase={git.base}
            onWriteIndex={async (text, expected) => {
              if (isAbsolutePath(buffer.path)) return;
              await backend.gitWriteIndex(buffer.path, text, expected);
              await git.reload();
            }}
          />
        </Flexbox>
      </Flexbox>
    </Flexbox>
  );
}
