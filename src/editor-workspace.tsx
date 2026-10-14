// Área central do editor: até dois grupos de abas lado a lado. Cada aba é um arquivo (CodeMirror), um diff staged/unstaged,
// um commit do histórico (só leitura) ou um conflito de merge. Buffers e rascunhos são do arquivo (`docId`), não da aba.
//
// API para o App (ref do componente, `EditorWorkspaceHandle`). Começa do zero: não substitui o FileView/FileTabs antigos
// até o App trocar a área do arquivo por este componente.
//   openFile(path, { permanent?, line?, col?, side? })  Abre o arquivo. Sem `permanent`, vira prévia (um clique). `line`/`col` (1-based, UTF-16) põem o cursor.
//                                                        `side` é o grupo 'A' | 'B' (cria B se couber).
//   openTarget(target, { permanent?, side? })           Abre um TabTarget do modelo (diff, commit, conflito ou arquivo com docId).
//   remap(from, to)                                     Depois de renomear/mover: troca o caminho de abas e rascunhos. Não remonta buffer.
//   remove(paths)                                       Depois de excluir: fecha as abas e descarta os buffers. Quem chama já decidiu o descarte.
//   hasDraft(path): boolean                             Há rascunho neste caminho (ou dentro, se for pasta). Bloqueia descarte no Git.
//   unsaved(): string[]                                 Os mesmos caminhos, para o integrador perguntar antes de fechar a janela.
//   saveAll(): Promise<boolean>                         Salva arquivos e rascunhos de conflito (conflito é gravado sem marcar resolvido; marcadores podem ficar). false se algo falhou.
//   discardAll(): void                                  Descarta todos os rascunhos (volta ao texto do disco/Git) e limpa a recuperação deles.
//   selectedText(): string                              Texto da seleção principal no arquivo da aba ativa do grupo focado. Sem seleção, a palavra sob o cursor.
//                                                        Vazio se não há aba de arquivo, se a seleção tem quebra de linha ou se não há palavra. Não mexe no editor.
// Props: `project` (raiz absoluta, chave da recuperação), `active` (área visível: sem ela, não lê o disco), `locked` (executores vivos:
// bloqueia mutações Git), `refreshGit` (chamado após mutação), `onActivePath`/`onDirtyPaths` (avisos), `layout` (texto opaco do layout
// externo, salvo na recuperação), `onRestoreLayout` (chamado uma vez na montagem com o layout salvo), `onReveal` (revelar no Explorer).
// `onSaved(paths)` (prop): chamada depois de cada gravação bem-sucedida no disco pelo editor (Ctrl+S, salvar tudo, comparação,
// resolução de conflito). `paths` são relativos com "/", um por gravação.
// Recuperação: localStorage por projeto (`orquestrador.editor:<project>`), validada ao ler; nunca grava no disco.
import {
  forwardRef, Fragment, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type DragEvent, type FocusEvent, type KeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode,
} from 'react';
import { ActionIcon, Button, ContextMenuTrigger, type ContextMenuItem, Modal, Popover, TabsIndicator, TabsList, TabsRoot, TabsTab, Text, toast } from '@lobehub/ui/base-ui';
import { Flexbox, PatchDiff } from '@lobehub/ui';
import { File, List, Pin, X } from 'lucide-react';
import { backend, message } from './backend';
import { CodeEditor, forgetEditorState, liveEditorView, type Reveal, type StartCursor } from './file-view';
import { action, blocked, DIVIDER, fileName } from './file-tabs';
import { pathText } from './file-tab-actions';
import {
  activate, closeTabs, createWorkspace, isDirty, isWithin, joinGroups, loadWorkspace, markEdited, moveTab, navigate, openPermanent, openPreview,
  planClose, remapPath, reopenClosed, splitTab, targetKey, togglePin, type CloseScope, type EditorTab, type GroupId, type TabTarget, type WorkspaceState,
} from './workspace-model';
import { revealTab, wheelToScroll } from './tab-strip';
import {
  blockCount, type Buffer, type Choice, type ConflictDraft, decodeRecovery, dirtyKeys, dropData, encodeRecovery, hasDraftAt,
  hasMarkers, isBufferDirty, isConflictDirty, newlineOf, normalizeNewlines, parseConflict, patchHunks, type PatchHunk, type Recovery,
  remapData, renderConflict, toDiskNewlines, conflictKey, type EditorData, type Cursor, type Scroll,
} from './workspace-store';
import type { GitGuard } from './git-guard';
import type { Conflict, FileContent, GitDiff, HunkRef, ReviewDiff } from './types';

export type OpenOptions = { permanent?: boolean; line?: number; col?: number; side?: GroupId };
export type EditorWorkspaceHandle = {
  openFile(path: string, options?: OpenOptions): void;
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
  /** Texto da seleção principal (ou a palavra sob o cursor) no arquivo da aba ativa do grupo focado. Vazio se não há o que pesquisar. */
  selectedText(): string;
};
export type EditorWorkspaceProps = {
  project: string;
  active: boolean;
  locked: boolean;
  refreshGit: () => void;
  /** Trava compartilhada com a barra do Git: as operações Git desta área e da barra não rodam juntas. */
  guard: GitGuard;
  onActivePath?: (path: string | null) => void;
  // Alvo da aba ativa no grupo focado (diff, commit, conflito ou arquivo): a sidebar marca o diff que está aberto.
  onActiveTarget?: (target: TabTarget | null) => void;
  onDirtyPaths?: (paths: string[]) => void;
  layout?: string | null;
  onRestoreLayout?: (layout: string) => void;
  onReveal?: (path: string) => void;
  /** Quantos grupos existem (1 ou 2): o App reserva a largura central de cada um. */
  onGroupCount?: (count: number) => void;
  /** Chamada após cada gravação bem-sucedida no disco feita pelo editor, com o caminho relativo ("/") do arquivo. */
  onSaved?: (paths: string[]) => void;
};

type Status = { error?: string; binary?: boolean; large?: boolean };
// Versão do disco que difere de um rascunho sujo: a tela oferece usar o disco ou salvar sobre ele, nunca troca sozinha.
type DiskVersion = { text: string; bom: boolean; readOnly: boolean };
type Ask = { title: string; body: ReactNode; actions: { key: string; label: string; danger?: boolean }[] };
// Comparação antes de gravar sobre o disco (ou de usar o disco): as duas versões são as que a tela tinha quando o usuário pediu.
type Compare = { docId: string; path: string; mode: 'keep' | 'use'; disk: DiskVersion; draft: string };
type Common = {
  active: boolean;
  locked: boolean;
  guard: GitGuard;
  refreshGit: () => void;
  openLine: (path: string, line: number) => void;
  hasDraft: (path: string) => boolean;
  // Só o buffer do arquivo (não o rascunho de conflito): é o que impede gravar a resolução por cima de uma edição ainda não salva.
  fileDirty: (path: string) => boolean;
  ask: (ask: Ask) => Promise<string | null>;
};

const STORAGE = (project: string) => `orquestrador.editor:${project}`;
const POLL_MS = 2500;
const DEFAULT_SPLIT = 0.5;
const newDocId = () => `doc-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
const folderOf = (path: string) => path.slice(0, Math.max(path.lastIndexOf('/'), 0)) || 'raiz';
// Mesmo nome em pastas diferentes: a pasta vira o detalhe da aba, para as duas não ficarem iguais. Sem repetição, não há detalhe.
function detailsOf(paths: string[]) {
  const count = new Map<string, number>();
  for (const path of paths) count.set(fileName(path), (count.get(fileName(path)) ?? 0) + 1);
  return new Map(paths.map((path) => [path, (count.get(fileName(path)) ?? 0) > 1 ? folderOf(path) : '']));
}
const tabPath = (target: TabTarget) => target.path;

// Texto de uma aba para a lista e os avisos: arquivo e pasta, ou o rótulo do diff/commit/conflito.
function labelOf(target: TabTarget): string {
  switch (target.kind) {
    case 'file': return target.path;
    case 'diff': return `${target.path} (${target.side === 'staged' ? 'preparadas' : 'alterações'})`;
    case 'commit': return `${target.path} @ ${target.hash.slice(0, 7)}`;
    case 'conflict': return `${target.path} (conflito)`;
  }
}

function emptyData(): EditorData { return { buffers: {}, conflicts: {} }; }

export const EditorWorkspace = forwardRef<EditorWorkspaceHandle, EditorWorkspaceProps>(function EditorWorkspace(props, handle) {
  const { project, active, locked, guard, refreshGit, onActivePath, onActiveTarget, onDirtyPaths, layout, onRestoreLayout, onReveal, onGroupCount, onSaved } = props;
  // Ref para que as gravações antigas (ainda em await) avisem com a prop atual.
  const savedRef = useRef(onSaved);
  savedRef.current = onSaved;
  const notifySaved = (path: string) => savedRef.current?.([path]);

  // Restaura a recuperação uma vez, na montagem (lê o localStorage de forma síncrona e valida tudo).
  const [boot] = useState(() => {
    let text: string | null = null;
    const notes: string[] = [];
    try {
      text = localStorage.getItem(STORAGE(project));
    } catch (e) {
      // Erro de leitura não é primeira abertura: avisa, e a edição começa vazia sem apagar a recuperação gravada.
      notes.push(`Não foi possível ler a recuperação (${message(e)}). Começou vazia.`);
      text = null;
    }
    if (text === null) return { ws: createWorkspace(), data: emptyData(), split: DEFAULT_SPLIT, layout: null as string | null, notes, cursors: {} as Record<string, Cursor>, scrolls: {} as Record<string, Scroll> };
    const decoded = decodeRecovery(text, project);
    notes.push(...decoded.notes);
    const ws = decoded.recovery.workspace ? loadWorkspace(decoded.recovery.workspace) : { state: createWorkspace(), notes: [] };
    notes.push(...ws.notes);
    const data = emptyData();
    const cursors: Record<string, Cursor> = {};
    const scrolls: Record<string, Scroll> = {};
    for (const b of decoded.recovery.buffers) {
      if (b.cursor) cursors[b.docId] = b.cursor;
      if (b.scroll) scrolls[b.docId] = b.scroll;
      if (b.dirty) data.buffers[b.docId] = { docId: b.docId, path: b.path, cursor: b.cursor, ...b.dirty };
    }
    for (const c of decoded.recovery.conflicts) data.conflicts[conflictKey(c.path)] = { ...c };
    return { ws: ws.state, data, split: decoded.recovery.split ?? DEFAULT_SPLIT, layout: decoded.recovery.layout, notes, cursors, scrolls };
  });

  const [ws, setWsState] = useState<WorkspaceState>(boot.ws);
  const [data, setDataState] = useState<EditorData>(boot.data);
  const [status, setStatus] = useState<Record<string, Status>>({});
  const [disk, setDisk] = useState<Record<string, DiskVersion>>({});
  const [split, setSplit] = useState(boot.split);
  const [reveal, setReveal] = useState<(Reveal & { docId: string }) | null>(null);
  const [notice, setNotice] = useState(boot.notes.join(' '));
  const [askState, setAskState] = useState<(Ask & { resolve: (key: string | null) => void }) | null>(null);
  const [layoutSaved] = useState(boot.layout);
  // Mudança de cursor não passa pelo estado do workspace; este contador a põe na fila de gravação da recuperação.
  const [cursorTick, setCursorTick] = useState(0);
  const wsRef = useRef(ws);
  const dataRef = useRef(data);
  const cursors = useRef<Record<string, Cursor>>(boot.cursors);
  // Rolagem por arquivo (docId): a vista que monta de novo (aba movida, app reaberto) volta para o mesmo ponto.
  const scrolls = useRef<Record<string, Scroll>>(boot.scrolls);
  const scrollTimer = useRef(0);
  const [compare, setCompare] = useState<Compare | null>(null);
  const saving = useRef(new Set<string>());
  const askOpen = useRef(false);
  const revealSeq = useRef(0);
  const layoutRef = useRef(layout ?? layoutSaved);
  const splitRef = useRef(split);
  const dirtyPrev = useRef('');
  const dragId = useRef<string | null>(null);
  const splitDrag = useRef<{ startX: number; start: number; width: number } | null>(null);

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
    setData({ ...dataRef.current, conflicts: { ...dataRef.current.conflicts, [conflictKey(draft.path)]: draft } });
  };
  const dropConflict = (path: string) => {
    const { [conflictKey(path)]: _gone, ...rest } = dataRef.current.conflicts;
    setData({ ...dataRef.current, conflicts: rest });
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
    const { [docId]: _gone, ...rest } = d;
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

  const openTarget = (target: TabTarget, options: Pick<OpenOptions, 'permanent' | 'side'> = {}) => {
    const dirty = dirtyKeys(dataRef.current);
    const group: GroupId = options.side ?? wsRef.current.focus;
    const base = groupWithDefault(wsRef.current, group);
    setWs(options.permanent ? openPermanent(base, group, target) : openPreview(base, group, target, dirty));
  };

  const openFile = (path: string, options: OpenOptions = {}) => {
    const docId = docIdFor(path);
    openTarget({ kind: 'file', docId, path }, options);
    if (options.line !== undefined) setReveal({ docId, path, line: options.line, col: options.col, id: ++revealSeq.current });
  };
  // À direita, pela ordem visual: com dois grupos, o segundo (groups[1]), mesmo quando o foco já está nele. Com um só,
  // vai para o outro id, que entra depois do existente (à direita dele).
  const openBeside = (path: string, options: Pick<OpenOptions, 'permanent' | 'line' | 'col'> = {}) => {
    const state = wsRef.current;
    const side: GroupId = state.groups.length >= 2 ? state.groups[1].id : state.groups[0]?.id === 'A' ? 'B' : 'A';
    openFile(path, { ...options, side });
  };

  // ---- Fechar (decisão de rascunho antes de qualquer mudança) ----
  const closeFlow = async (scope: CloseScope): Promise<boolean> => {
    const plan = planClose(wsRef.current, scope, dirtyKeys(dataRef.current));
    if (plan.tabIds.length === 0) return true;
    const closing = new Set(plan.tabIds);
    const stay = new Set(allTabs().filter((t) => !closing.has(t.id)).map((t) => targetKey(t.target)));
    const byId = new Map(allTabs().map((t) => [t.id, t] as const));
    // Vista que continua aberta (no mesmo grupo ou no outro) não perde o buffer: essas abas não entram na pergunta.
    const unsavedTabs = plan.unsaved.map((id) => byId.get(id)).filter((t): t is EditorTab => !!t && !stay.has(targetKey(t.target)));
    let decision: string | null = null;
    if (unsavedTabs.length > 0) {
      const names = unsavedTabs.map((t) => fileName(tabPath(t.target))).join(', ');
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
            setNotice(`Não foi possível salvar ${fileName(tabPath(tab.target))}. Nada foi fechado.`);
            return false;
          }
        }
      }
    }
    // Estado pode ter mudado durante a pergunta e o salvamento (digitação nova): a lista é refeita e o que ainda está sujo não fecha sem decisão.
    const again = planClose(wsRef.current, scope, dirtyKeys(dataRef.current));
    // Descartar já foi decidido para as abas de `unsavedTabs`; salvar não garante nada se houve digitação durante o save.
    const dirtyNow = dirtyKeys(dataRef.current);
    const discarded = new Set(decision === 'discard' ? unsavedTabs.map((t) => t.id) : []);
    const pendingNew = again.tabIds
      .map((id) => allTabs().find((t) => t.id === id))
      .filter((t): t is EditorTab => !!t && isDirty(t.target, dirtyNow) && !discarded.has(t.id) && !stay.has(targetKey(t.target)));
    if (pendingNew.length > 0) {
      setNotice(`Ainda há alterações não salvas em ${pendingNew.map((t) => fileName(tabPath(t.target))).join(', ')}. Nada foi fechado.`);
      return false;
    }
    const next = closeTabs(wsRef.current, again.tabIds);
    setWs(next);
    pruneData(next);
    return true;
  };

  // Buffers e conflitos sem aba: a aba foi fechada e o rascunho já foi decidido (salvo ou descartado).
  const pruneData = (state: WorkspaceState) => {
    const tabs = state.groups.flatMap((g) => g.tabs);
    const docIds = new Set(tabs.flatMap((t) => (t.target.kind === 'file' ? [t.target.docId] : [])));
    const keys = new Set(tabs.flatMap((t) => (t.target.kind === 'conflict' ? [conflictKey(t.target.path)] : [])));
    const current = dataRef.current;
    for (const id of Object.keys(current.buffers)) if (!docIds.has(id)) forgetEditorState(id);
    const buffers = Object.fromEntries(Object.entries(current.buffers).filter(([id]) => docIds.has(id)));
    const conflicts = Object.fromEntries(Object.entries(current.conflicts).filter(([key]) => keys.has(key)));
    for (const id of Object.keys(scrolls.current)) if (!docIds.has(id)) delete scrolls.current[id];
    setData({ buffers, conflicts });
  };

  // ---- Salvar ----
  const saveBuffer = async (docId: string): Promise<boolean> => {
    const buffer = dataRef.current.buffers[docId];
    if (!buffer || !isBufferDirty(buffer)) return true;
    if (buffer.readOnly || saving.current.has(docId)) return false;
    saving.current.add(docId);
    const sent = toDiskNewlines(buffer.draft, buffer.newline);
    try {
      // Grava só se o disco ainda é o `raw` lido (base crua, sem normalizar). Conflito é tratado abaixo, não há força silenciosa.
      await backend.writeFileChecked(buffer.path, sent, buffer.raw, buffer.bom);
      patchBuffer(docId, (b) => ({ ...b, raw: sent }));
      clearDisk(docId);
      setStatusOf(docId, {});
      refreshGit();
      notifySaved(buffer.path);
      return true;
    } catch (e) {
      const reason = message(e);
      try {
        const file = await backend.readFile(buffer.path);
        if (normalizeNewlines(file.text) !== normalizeNewlines(buffer.raw) || file.bom !== buffer.bom) {
          setDisk((d) => ({ ...d, [docId]: { text: file.text, bom: file.bom, readOnly: file.readOnly } }));
          setStatusOf(docId, {});
        } else {
          setStatusOf(docId, { error: reason });
        }
      } catch (readError) {
        setStatusOf(docId, { error: `${reason} ${message(readError)}` });
      }
      return false;
    } finally {
      saving.current.delete(docId);
    }
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
    if (!guard.enter('resolução do conflito')) return false;
    conflictSaving.current.add(key);
    const sent = current.draft;
    try {
      await backend.gitWriteResolution(path, sent, current.revision, markResolved);
      notifySaved(path);
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
    } catch (e) {
      toast.error({ title: message(e), duration: 7000 });
      return false;
    } finally {
      conflictSaving.current.delete(key);
      guard.leave();
    }
  };

  const saveTab = (target: TabTarget): Promise<boolean> => {
    if (target.kind === 'file') return saveBuffer(target.docId);
    if (target.kind === 'conflict') return saveConflict(target.path, false);
    return Promise.resolve(true);
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
    let file: FileContent;
    try {
      file = await backend.readFile(path);
    } catch (e) {
      // Arquivo apagado ou inacessível: o buffer (e o rascunho) continuam; a aba mostra o erro.
      setStatusOf(docId, { error: message(e) });
      return;
    }
    if (!allTabs().some((t) => t.target.kind === 'file' && t.target.docId === docId)) return;
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
            readOnly: file.readOnly, cursor: cursors.current[docId] ?? null,
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
  useEffect(() => {
    if (!active) return;
    const ids = activeKey === '' ? [] : activeKey.split('|');
    ids.forEach((id) => void syncFile(id));
    const timer = window.setInterval(() => ids.forEach((id) => void syncFile(id)), POLL_MS);
    return () => window.clearInterval(timer);
    // syncFile lê refs; o efeito só precisa refazer quando a aba ativa ou a visibilidade mudam.
  }, [active, activeKey]);

  // ---- Avisos e recuperação ----
  const dirtyList = (): string[] => {
    const out: string[] = [];
    for (const b of Object.values(dataRef.current.buffers)) if (isBufferDirty(b)) out.push(b.path);
    for (const c of Object.values(dataRef.current.conflicts)) if (isConflictDirty(c)) out.push(c.path);
    return out;
  };
  useEffect(() => {
    const list = dirtyList();
    const text = JSON.stringify(list);
    if (text === dirtyPrev.current) return;
    dirtyPrev.current = text;
    onDirtyPaths?.(list);
  }, [data]);
  useEffect(() => {
    const group = ws.groups.find((g) => g.id === ws.focus);
    const tab = group?.tabs.find((t) => t.id === group.active);
    onActivePath?.(tab ? tabPath(tab.target) : null);
    onActiveTarget?.(tab ? tab.target : null);
  }, [ws]);

  useEffect(() => {
    if (layoutSaved !== null) onRestoreLayout?.(layoutSaved);
    // Só na montagem: depois disso o App manda o layout de volta pela prop `layout`.
  }, []);
  layoutRef.current = layout ?? layoutRef.current;
  splitRef.current = split;

  // Grava a recuperação: com atraso (edição rápida não escreve a cada tecla) e de imediato ao sair da página ou desmontar.
  // Falha de quota vira aviso e não derruba a edição. Layout e divisor ficam mesmo sem abas; só some quando não há nada.
  const persist = () => {
    const open = wsRef.current.groups.flatMap((g) => g.tabs);
    const conflicts = Object.values(dataRef.current.conflicts).filter(isConflictDirty);
    const hasState = open.length > 0 || conflicts.length > 0 || layoutRef.current !== null;
    const recovery: Recovery = {
      workspace: JSON.stringify(wsRef.current),
      layout: layoutRef.current,
      split: splitRef.current,
      buffers: [...new Map(open.flatMap((t) => (t.target.kind === 'file' ? [[t.target.docId, t.target] as const] : []))).values()].map((target) => {
        const docId = target.docId;
        const b = dataRef.current.buffers[docId];
        return {
          docId, path: target.path, cursor: cursors.current[docId] ?? null, scroll: scrolls.current[docId] ?? null,
          dirty: b && isBufferDirty(b) ? { raw: b.raw, bom: b.bom, newline: b.newline, draft: b.draft, readOnly: b.readOnly } : null,
        };
      }),
      conflicts: conflicts.map((c) => ({ path: c.path, origin: c.origin, draft: c.draft, revision: c.revision })),
    };
    try {
      if (!hasState) {
        localStorage.removeItem(STORAGE(project));
        return;
      }
      const encoded = encodeRecovery(project, recovery);
      localStorage.setItem(STORAGE(project), encoded.text);
      if (encoded.notes.length > 0) setNotice(encoded.notes.join(' '));
    } catch (e) {
      setNotice(`Não foi possível guardar a recuperação (${message(e)}). A edição continua, mas não será recuperada.`);
    }
  };
  const persistRef = useRef(persist);
  persistRef.current = persist;
  useEffect(() => {
    const timer = window.setTimeout(() => persistRef.current(), 400);
    return () => window.clearTimeout(timer);
  }, [ws, data, layout, split, cursorTick, project]);
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
  const copy = async (kind: 'relative' | 'absolute' | 'navigation', target: TabTarget) => {
    const cursor = target.kind === 'file' ? cursors.current[target.docId] : undefined;
    const text = pathText(kind, target.path, project, cursor);
    if (text === null) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch (e) {
      toast.error({ title: message(e), duration: 5000 });
    }
  };
  const reveal_ = (path: string) => {
    if (onReveal) onReveal(path);
    else void backend.revealFile(path).catch((e) => toast.error({ title: message(e), duration: 5000 }));
  };
  const reopen = () => setWs(reopenClosed(wsRef.current));

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
    const ids = allTabs().filter((t) => t.target.kind !== 'commit' && hit(tabPath(t.target))).map((t) => t.id);
    const next = closeTabs(wsRef.current, ids);
    setWs(next);
    setData(dropData(dataRef.current, paths));
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

  // ---- Arrastar abas ----
  const dropOn = (groupId: GroupId, index?: number) => {
    const id = dragId.current;
    dragId.current = null;
    if (!id) return;
    setWs(moveTab(wsRef.current, id, groupId, index));
  };

  // ---- Teclado (só dentro da área; terminal e campos fora dela não passam por aqui) ----
  // Captura (não bolha): o CodeMirror trata Ctrl+K (apagar até o fim da linha) na fase de bolha, então este handler vê a tecla antes
  // e pode parar a propagação. Só alcança o que está dentro desta área; o xterm e os campos ficam fora dela ou são pulados abaixo.
  // Acordes: Ctrl+K e depois W (fecha as não fixadas do grupo ativo), Ctrl+Shift+C (caminho relativo), Ctrl+Shift+Enter não existe:
  // Shift+Enter (fixa/desafixa). O prefixo vale 1,5 s e cai ao sair da área.
  const chordPending = useRef(false);
  const chordTimer = useRef(0);
  const beginChord = () => {
    chordPending.current = true;
    window.clearTimeout(chordTimer.current);
    chordTimer.current = window.setTimeout(() => { chordPending.current = false; }, 1500);
  };
  const endChord = () => {
    chordPending.current = false;
    window.clearTimeout(chordTimer.current);
  };
  useEffect(() => () => window.clearTimeout(chordTimer.current), []);
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
    if (chordPending.current) {
      // Ctrl+K de novo reinicia o acorde, sem chegar ao editor.
      if (e.ctrlKey && !e.shiftKey && !e.altKey && e.code === 'KeyK') {
        e.preventDefault();
        e.stopPropagation();
        beginChord();
        return;
      }
      endChord();
      // Acorde sem ação (ou sem alvo) não é capturado: a tecla segue para o editor, como qualquer outra.
      let run: (() => void) | null = null;
      if (!mod && !e.altKey && !e.shiftKey && e.code === 'KeyW') run = () => void closeFlow({ kind: 'all', group: wsRef.current.focus });
      else if (e.ctrlKey && e.shiftKey && !e.altKey && e.code === 'KeyC' && focused) run = () => void copy('relative', focused.target);
      else if (!mod && !e.altKey && e.shiftKey && e.code === 'Enter' && focused) run = () => setWs(togglePin(wsRef.current, focused.id));
      if (!run) return;
      e.preventDefault();
      e.stopPropagation();
      run();
      return;
    }
    if (mod && !e.shiftKey && !e.altKey && e.code === 'KeyK') {
      e.preventDefault();
      e.stopPropagation();
      beginChord();
    } else if (mod && !e.shiftKey && !e.altKey && (e.key === 'w' || e.key === 'F4') && focused) {
      e.preventDefault();
      e.stopPropagation();
      void closeFlow({ kind: 'one', tabId: focused.id });
    } else if (e.altKey && e.shiftKey && !mod && e.code === 'KeyC' && focused) {
      e.preventDefault();
      e.stopPropagation();
      void copy('absolute', focused.target);
    } else if (e.ctrlKey && e.key === 'Tab') {
      e.preventDefault();
      e.stopPropagation();
      setWs(navigate(wsRef.current, e.shiftKey ? -1 : 1));
    } else if (mod && e.shiftKey && (e.key === 'T' || e.key === 't')) {
      e.preventDefault();
      e.stopPropagation();
      reopen();
    } else if (mod && e.key === '\\') {
      e.preventDefault();
      e.stopPropagation();
      if (focused) split_(focused.id);
    }
  };
  // Sair da área (foco foi para fora, p. ex. o terminal) cancela o acorde pendente.
  const onKeyLeave = (e: FocusEvent<HTMLDivElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) endChord();
  };

  // Avisa o App quando o número de grupos muda (ele reserva a largura central de cada um).
  useEffect(() => { onGroupCount?.(ws.groups.length); }, [ws.groups.length]);

  useImperativeHandle(handle, () => ({
    openFile,
    openTarget,
    openBeside,
    remap,
    remove,
    hasDraft: (path: string) => hasDraftAt(dataRef.current, path),
    unsaved: () => dirtyList(),
    saveAll,
    openPaths: () => [...new Set(allTabs().map((t) => tabPath(t.target)))],
    refresh: () => {
      const ids = new Set(allTabs().flatMap((t) => (t.target.kind === 'file' ? [t.target.docId] : [])));
      ids.forEach((id) => void syncFile(id));
    },
    flushRecovery: () => persistRef.current(),
    selectedText: () => {
      const tab = focusedTab();
      if (!tab || tab.target.kind !== 'file') return '';
      const view = liveEditorView(tab.target.docId);
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
    const sent = toDiskNewlines(choice.draft, newlineOf(choice.disk.text));
    let failed: unknown = null;
    try {
      await backend.writeFileChecked(buffer.path, sent, choice.disk.text, choice.disk.bom);
      // Quem digitou durante o await continua sujo: o `raw` vira o texto gravado, e o `draft` atual fica como está.
      patchBuffer(choice.docId, (b) => ({ ...b, raw: sent, bom: choice.disk.bom, newline: newlineOf(choice.disk.text) }));
      clearDisk(choice.docId);
      setStatusOf(choice.docId, {});
      refreshGit();
      notifySaved(buffer.path);
    } catch (e) {
      failed = e;
    } finally {
      saving.current.delete(choice.docId);
    }
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
    const dirtySet = dirtyKeys(dataRef.current);
    const dirty = isDirty(target, dirtySet);
    const buffer = target.kind === 'file' ? dataRef.current.buffers[target.docId] : undefined;
    const groupOf = wsRef.current.groups.find((g) => g.tabs.some((t) => t.id === tab.id));
    const others = groupOf ? groupOf.tabs.filter((t) => t.id !== tab.id && !t.pinned).length : 0;
    const right = groupOf ? groupOf.tabs.slice(groupOf.tabs.findIndex((t) => t.id === tab.id) + 1).filter((t) => !t.pinned).length : 0;
    const saved = groupOf ? groupOf.tabs.filter((t) => !t.pinned && !isDirty(t.target, dirtySet)).length : 0;
    const all = groupOf ? groupOf.tabs.filter((t) => !t.pinned).length : 0;
    const closeBatch = (scope: CloseScope) => void closeFlow(scope);
    const saveBlock = target.kind === 'diff' || target.kind === 'commit'
      ? blocked('save', 'Salvar', 'Esta aba não tem texto para salvar.')
      : !dirty ? blocked('save', 'Salvar', 'Não há alterações não salvas nesta aba.')
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
        action('copy-relative', 'Copiar Caminho Relativo', () => void copy('relative', target), 'Ctrl+K Ctrl+Shift+C'),
        action('copy-path', 'Copiar o Caminho', () => void copy('absolute', target), 'Shift+Alt+C'),
        target.kind === 'file' && cursors.current[target.docId]
          ? action('copy-navigation', 'Copiar Caminho de Navegação', () => void copy('navigation', target))
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
        action('reveal', 'Revelar no Explorer', () => reveal_(target.path)),
      ],
    ];
    return sections.flatMap((items, i) => (i ? [DIVIDER, ...items] : items));
  };

  const bodyOf = (tab: EditorTab, groupActive: boolean, shown: boolean): ReactNode => {
    const target = tab.target;
    if (target.kind === 'diff') return <DiffTab target={target} common={common} shown={shown} />;
    if (target.kind === 'commit') return <CommitTab target={target} common={common} />;
    if (target.kind === 'conflict') {
      return (
        <ConflictTab
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
    return (
      <FileBody
        active={groupActive && active}
        buffer={data.buffers[target.docId]}
        status={status[target.docId]}
        disk={disk[target.docId]}
        reveal={reveal && reveal.docId === target.docId && groupActive ? reveal : null}
        cursor={cursors.current[target.docId] ?? null}
        onChange={(value) => {
          patchBuffer(target.docId, (b) => ({ ...b, draft: value }));
          // Editar torna a aba permanente (a prévia vira aba de verdade). Só as abas com este docId são tocadas.
          const current = wsRef.current;
          let next = current;
          for (const g of current.groups) for (const t of g.tabs) if (t.target.kind === 'file' && t.target.docId === target.docId && t.preview) next = markEdited(next, t.id);
          if (next !== current) setWs(next);
        }}
        onSave={() => void saveBuffer(target.docId)}
        onCursor={(line, col) => {
          const prev = cursors.current[target.docId];
          if (prev && prev.line === line && prev.col === col) return;
          cursors.current[target.docId] = { line, col };
          setCursorTick((n) => n + 1);
        }}
        scroll={scrolls.current[target.docId] ?? null}
        onScroll={(value) => {
          scrolls.current[target.docId] = value;
          // Rolar não re-renderiza a área: só agenda a gravação da recuperação (o atraso junta as rolagens).
          window.clearTimeout(scrollTimer.current);
          scrollTimer.current = window.setTimeout(() => persistRef.current(), 400);
        }}
        onCompare={(mode) => openCompare(target.docId, target.path, mode)}
        onRetry={() => void syncFile(target.docId)}
      />
    );
  };

  // ---- Grupos ----
  const dirty = dirtyKeys(data);
  const activeTabOf = (g: { active: string | null; tabs: EditorTab[] }) => g.tabs.find((t) => t.id === g.active) ?? null;
  const titleOf = (target: TabTarget) => {
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
  const stripRef = (groupId: GroupId) => (el: HTMLDivElement | null) => {
    strips.current.get(groupId)?.observer.disconnect();
    strips.current.delete(groupId);
    if (!el) return;
    const observer = new ResizeObserver(() => revealActive(groupId, true));
    observer.observe(el);
    strips.current.set(groupId, { el, observer });
  };
  const stripRefs = useRef(new Map<GroupId, (el: HTMLDivElement | null) => void>());
  const stripRefFor = (groupId: GroupId) => {
    let fn = stripRefs.current.get(groupId);
    if (!fn) stripRefs.current.set(groupId, fn = stripRef(groupId));
    return fn;
  };
  useLayoutEffect(() => {
    for (const group of ws.groups) revealActive(group.id);
  });

  const renderStrip = (groupId: GroupId) => {
    const group = ws.groups.find((g) => g.id === groupId);
    if (!group) return null;
    const current = activeTabOf(group);
    const focusedHere = ws.focus === groupId;
    const details = detailsOf(ws.groups.flatMap((g) => g.tabs.map((t) => t.target.path)));
    return (
      // Faixa de 36 px, como a do Orquestrador: abas em variant square, com o divisor por baixo. O foco do grupo aparece na cor do indicador.
      <Flexbox
        horizontal align="stretch" gap={4} height={36} flex="none" paddingInline={4} className="relative bg-card"
        onDragOver={(e: DragEvent) => { if (dragId.current) e.preventDefault(); }}
        onDrop={(e: DragEvent) => { e.preventDefault(); dropOn(groupId); }}
        data-group={groupId}
      >
        <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-border" />
        <Flexbox
          ref={stripRefFor(groupId)} horizontal align="stretch" flex={1} className="@container min-w-0 overflow-x-auto [scrollbar-width:none]!"
          onWheel={wheelToScroll}
        >
          <TabsRoot variant="square" value={current?.id ?? ''} onValueChange={(id: string) => activateTab(id)} className="gap-0">
            {/* Sem gap e sem sombra nativa: o indicador de 2 px fica colado no divisor. Grupo sem foco usa cor apagada. */}
            <TabsList className="h-full flex-nowrap justify-start overflow-clip" style={{ gap: 0, boxShadow: 'none' }}>
              <TabsIndicator style={focusedHere ? undefined : { background: 'var(--ant-color-text-quaternary)' }} />
              {group.tabs.map((tab, index) => (
                <ContextMenuTrigger key={tab.id} items={() => tabMenu(tab)}>
                  <div
                    data-tab={tab.id}
                    draggable
                    onDragStart={(e) => { dragId.current = tab.id; e.dataTransfer.setData('text/plain', tab.id); }}
                    onDragEnd={() => { dragId.current = null; }}
                    onDragOver={(e) => { if (dragId.current) e.preventDefault(); }}
                    onDrop={(e) => { e.preventDefault(); e.stopPropagation(); dropOn(groupId, index); }}
                    className="relative h-full flex-none max-w-[min(14rem,100cqw)]"
                    onKeyDown={(e) => {
                      if (!((e.key === 'F10' && e.shiftKey) || e.key === 'ContextMenu')) return;
                      e.preventDefault();
                      const rect = e.currentTarget.getBoundingClientRect();
                      e.currentTarget.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rect.left + 16, clientY: rect.bottom }));
                    }}
                  >
                    <TabsTab value={tab.id} title={labelOf(tab.target)} className={`h-full max-w-full flex-none gap-1.5 pr-7! pl-3${tab.preview ? ' italic' : ''}`}>
                      {tab.pinned
                        ? <Pin className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
                        : <File className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />}
                      <Text as="span" ellipsis className="min-w-0">{titleOf(tab.target)}</Text>
                      {details.get(tab.target.path) && (
                        <Text as="span" ellipsis type="secondary" fontSize={12} className="min-w-0">{details.get(tab.target.path)}</Text>
                      )}
                      {tab.pinned && <span className="sr-only">(fixada)</span>}
                      {tab.preview && <span className="sr-only">(prévia)</span>}
                      {isDirtyTab(tab.target) && <Text as="span" aria-label="Alteração não salva" title="Alteração não salva" className="shrink-0">●</Text>}
                    </TabsTab>
                    <ActionIcon
                      variant="borderless" size="small" icon={X}
                      aria-label={`Fechar ${labelOf(tab.target)}`} title="Fechar"
                      className="absolute! z-10 top-1/2 right-1 -translate-y-1/2!"
                      onClick={() => void closeFlow({ kind: 'one', tabId: tab.id })}
                    />
                  </div>
                </ContextMenuTrigger>
              ))}
            </TabsList>
          </TabsRoot>
        </Flexbox>
        <Popover
          trigger="click" placement="bottomRight"
          content={(
            <Flexbox gap={2} padding={8} className="w-64 max-h-72 overflow-y-auto">
              {group.tabs.map((tab) => (
                <Button key={tab.id} type="text" size="small" title={labelOf(tab.target)} className="min-w-0 justify-start! gap-2" aria-current={tab.id === current?.id ? 'true' : undefined} onClick={() => activateTab(tab.id)}>
                  <Text as="span" ellipsis className="min-w-0">{labelOf(tab.target)}</Text>
                </Button>
              ))}
            </Flexbox>
          )}
        >
          <Button type="text" size="small" icon={<List className="size-3.5" />} className="shrink-0 self-center" aria-label={`Abas do grupo ${groupId}: ${group.tabs.length}`} title="Todas as abas deste grupo">
            {group.tabs.length}
          </Button>
        </Popover>
      </Flexbox>
    );
  };

  const renderGroup = (groupId: GroupId) => {
    const group = ws.groups.find((g) => g.id === groupId);
    if (!group) return null;
    const current = activeTabOf(group);
    return (
      <Flexbox flex={1} height="100%" className="min-w-0 min-h-0" data-pane={groupId} onMouseDown={() => { if (ws.focus !== groupId) setWs({ ...wsRef.current, focus: groupId }); }}>
        {renderStrip(groupId)}
        <Flexbox flex={1} className="min-h-0 relative">
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
  return (
    <Flexbox height="100%" className="min-h-0" onKeyDownCapture={onKey} onBlurCapture={onKeyLeave}>
      {notice && (
        <Flexbox horizontal align="center" gap={8} paddingInline={12} height={28} flex="none" className="border-b">
          <Text as="span" role="status" type="secondary" fontSize={12} ellipsis className="min-w-0">{notice}</Text>
          <ActionIcon variant="borderless" size="small" icon={X} className="ml-auto shrink-0" aria-label="Fechar o aviso" onClick={() => setNotice('')} />
        </Flexbox>
      )}
      <Flexbox horizontal flex={1} className="min-h-0" height="100%">
        {ws.groups.map((group, index) => (
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

// ---- Arquivo ----

function FileBody({ active, buffer, status, disk, reveal, cursor, onChange, onSave, onCursor, onCompare, onRetry, scroll, onScroll }: {
  active: boolean; buffer: Buffer | undefined; status: Status | undefined; disk: DiskVersion | undefined;
  reveal: (Reveal & { docId: string }) | null; cursor: StartCursor | null; onChange: (value: string) => void; onSave: () => void;
  onCursor: (line: number, col: number) => void; onCompare: (mode: 'keep' | 'use') => void; onRetry: () => void;
  scroll: Scroll | null; onScroll: (scroll: Scroll) => void;
}) {
  if (status?.binary) return <Note text="Arquivo binário: não há texto para mostrar aqui." />;
  if (status?.large) return <Note text="Arquivo grande demais para mostrar aqui." />;
  if (!buffer) return <Note text={status?.error ?? 'Lendo o arquivo…'} danger={!!status?.error} onRetry={status?.error ? onRetry : undefined} />;
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
      <Flexbox flex={1} className="min-h-0 pr-1 pb-1">
        <Flexbox flex={1} className="min-h-0 overflow-hidden">
          <CodeEditor
            text={buffer.draft}
            readOnly={buffer.readOnly}
            path={buffer.path}
            reveal={active && reveal ? { path: buffer.path, line: reveal.line, col: reveal.col, id: reveal.id } : null}
            onChange={onChange}
            onSave={onSave}
            onCursor={onCursor}
            onScroll={onScroll}
            scroll={scroll}
            cacheKey={buffer.docId}
            start={cursor}
          />
        </Flexbox>
      </Flexbox>
    </Flexbox>
  );
}

// Acima disso a comparação não é desenhada (duas áreas de texto gigantes travam a tela). A confirmação continua valendo.
const COMPARE_CHARS = 200_000;

// As duas versões lado a lado, como estavam quando o usuário pediu. A contagem compara linha a linha pela posição: é um indicador,
// não um diff. O texto do disco é o que a confirmação grava de volta como base, então ele precisa estar à vista.
function CompareView({ path, disk, draft, mode }: { path: string; disk: string; draft: string; mode: 'keep' | 'use' }) {
  const big = disk.length > COMPARE_CHARS || draft.length > COMPARE_CHARS;
  const diskText = normalizeNewlines(disk);
  const diskLines = diskText.split('\n');
  const draftLines = draft.split('\n');
  let differ = 0;
  for (let i = 0; i < Math.max(diskLines.length, draftLines.length); i++) if (diskLines[i] !== draftLines[i]) differ++;
  return (
    <Flexbox gap={12}>
      <Text as="p" fontSize={14} type="secondary" style={{ margin: 0, overflowWrap: 'anywhere' }}>
        {mode === 'use'
          ? `${path}: sua edição será descartada e o texto do disco entra no lugar.`
          : `${path}: o disco passa a ter a sua edição. Se o arquivo mudar de novo antes de você confirmar, nada é gravado.`}
      </Text>
      {big ? (
        <Text as="p" role="status" fontSize={14} type="secondary" style={{ margin: 0 }}>
          Arquivo grande demais para comparar aqui. Confirme só se tem certeza do que está no disco.
        </Text>
      ) : (
        <>
          <Text as="p" role="status" fontSize={12} type="secondary" style={{ margin: 0 }}>
            {differ === 0 ? 'As duas versões têm o mesmo texto linha a linha.' : `${differ} linha(s) diferente(s) na mesma posição.`}
          </Text>
          <div className="grid min-w-0 gap-3 md:grid-cols-2">
            <section aria-label="Versão do disco" className="flex min-w-0 flex-col gap-1">
              <Text as="p" fontSize={12} weight={600} style={{ margin: 0 }}>Disco (atual)</Text>
              <pre className="max-h-[50vh] overflow-auto rounded border p-2 text-xs whitespace-pre select-text">{diskText}</pre>
            </section>
            <section aria-label="Sua edição" className="flex min-w-0 flex-col gap-1">
              <Text as="p" fontSize={12} weight={600} style={{ margin: 0 }}>Sua edição</Text>
              <pre className="max-h-[50vh] overflow-auto rounded border p-2 text-xs whitespace-pre select-text">{draft}</pre>
            </section>
          </div>
        </>
      )}
    </Flexbox>
  );
}

function Note({ text, danger, onRetry }: { text: string; danger?: boolean; onRetry?: () => void }) {
  return (
    <Flexbox padding={16} gap={8}>
      <Text as="p" role="status" fontSize={14} type={danger ? 'danger' : 'secondary'}>{text}</Text>
      {onRetry && <Button size="small" onClick={onRetry} className="self-start">Tentar de novo</Button>}
    </Flexbox>
  );
}

// ---- Diff (preparadas e alterações), por bloco ----

// Cabeçalho do arquivo no patch (tudo antes do primeiro `@@`). Cada bloco mostra com ele, para o PatchDiff ler o caminho.
const fileHead = (patch: string) => {
  const at = patch.indexOf('\n@@');
  return at < 0 ? patch : patch.slice(0, at);
};

// `shown`: a aba é a atual do grupo. As abas escondidas ficam montadas e não consultam o Git; ao voltar, leem o diff de novo,
// porque a aba irmã pode ter mudado o índice (a revisão velha seria recusada pelo Git).
function DiffTab({ target, common, shown }: { target: Extract<TabTarget, { kind: 'diff' }>; common: Common; shown: boolean }) {
  const staged = target.side === 'staged';
  const [review, setReview] = useState<ReviewDiff | null>(null);
  const [error, setError] = useState('');
  const [mode, setMode] = useState<'split' | 'unified'>('unified');
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const holder = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState(0);
  const load = async () => {
    try {
      setReview(await backend.gitReviewDiff(target.path, staged));
      setError('');
    } catch (e) {
      setError(message(e));
    }
  };
  useEffect(() => {
    if (!common.active || !shown) return;
    void load();
    const timer = window.setInterval(() => void load(), POLL_MS);
    return () => window.clearInterval(timer);
  }, [common.active, shown, target.path, target.side]);

  // Uma operação por vez: o clique duplo não dispara duas (o ref não espera o re-render).
  const run = async (work: () => Promise<void>) => {
    if (busyRef.current || common.locked) return;
    if (!common.guard.enter('alteração do diff')) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await work();
      await load();
      common.refreshGit();
    } catch (e) {
      toast.error({ title: message(e), duration: 7000 });
      await load();
    } finally {
      busyRef.current = false;
      setBusy(false);
      common.guard.leave();
    }
  };

  const hunks: PatchHunk[] = review ? patchHunks(review.diff.patch) : [];
  const head = review ? fileHead(review.diff.patch) : '';
  const blocked_ = common.locked || busy;
  const draftOpen = common.hasDraft(target.path);
  // Não rastreado: o Git não separa em blocos (lista vazia), mas o patch existe. A tela mostra o arquivo inteiro e oferece preparar tudo.
  const untracked = review !== null && review.hunks.length === 0 && review.diff.patch !== '' && !review.diff.binary && !review.diff.tooLarge;
  // Ação por bloco só vale se os blocos da tela batem, um a um, com a lista oficial do backend (índice 0-based).
  const aligned = review !== null && review.hunks.length === hunks.length && hunks.every((h, i) => review.hunks[i].header === h.header);
  const blockOff = blocked_ || !aligned;
  const jump = (step: number) => {
    if (hunks.length === 0) return;
    const next = Math.min(Math.max(at + step, 0), hunks.length - 1);
    setAt(next);
    holder.current?.querySelector<HTMLElement>(`[data-hunk="${next}"]`)?.scrollIntoView({ block: 'start' });
  };

  const note = (text: string, danger?: boolean) => <Flexbox padding={16}><Text as="p" role="status" fontSize={14} type={danger ? 'danger' : 'secondary'}>{text}</Text></Flexbox>;
  if (error && !review) return note(error, true);
  if (!review) return note('Lendo as alterações…');
  if (review.diff.binary) return note('Arquivo binário: o diff não é mostrado.');
  if (review.diff.tooLarge) return note('Diff grande demais para mostrar aqui.');
  if (review.diff.patch === '') return note('Não há alterações neste lado.');

  return (
    <Flexbox flex={1} className="min-h-0">
      <Flexbox horizontal align="center" wrap="wrap" gap={6} paddingInline={12} paddingBlock={4} flex="none" className="border-b">
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={target.path} className="min-w-0">{target.path}</Text>
        <Text as="span" type="secondary" fontSize={12} className="shrink-0">{staged ? 'preparadas' : 'alterações'}</Text>
        <Button size="small" className="ml-auto shrink-0" onClick={() => setMode(mode === 'split' ? 'unified' : 'split')}>
          {mode === 'split' ? 'Unificado' : 'Lado a lado'}
        </Button>
        <Button size="small" disabled={hunks.length === 0 || at <= 0} onClick={() => jump(-1)} aria-label="Bloco anterior">Anterior</Button>
        <Button size="small" disabled={hunks.length === 0 || at >= hunks.length - 1} onClick={() => jump(1)} aria-label="Próximo bloco">Próximo</Button>
        {!staged && (
          <Button
            size="small" danger disabled={blocked_ || draftOpen}
            title={draftOpen ? 'Resolva o rascunho aberto deste arquivo antes de descartar.' : 'Descartar todas as alterações deste arquivo'}
            onClick={async () => {
              const ok = await common.ask({
                title: 'Descartar as alterações do arquivo?',
                body: <Text as="pre" fontSize={12} className="max-h-64 overflow-auto whitespace-pre">{review.diff.patch}</Text>,
                actions: [{ key: 'ok', label: 'Descartar', danger: true }],
              });
              if (ok !== 'ok') return;
              // Rascunho aberto pode ter surgido durante a confirmação: a checagem vale agora, não só no render anterior.
              if (common.hasDraft(target.path)) {
                toast.error({ title: 'Há um rascunho aberto deste arquivo. Salve ou descarte antes de descartar as alterações.', duration: 6000 });
                return;
              }
              const captured = review.revision;
              await run(() => backend.gitDiscardFile(target.path, captured));
            }}
          >
            Descartar arquivo
          </Button>
        )}
      </Flexbox>
      <Flexbox ref={holder} flex={1} gap={8} padding={8} className="min-h-0 overflow-y-auto">
        {untracked && (
          <Flexbox gap={6}>
            <Flexbox horizontal align="center" wrap="wrap" gap={6}>
              <Text as="span" type="secondary" fontSize={12} className="min-w-0 flex-1">Arquivo novo, ainda não rastreado: o Git não separa em blocos.</Text>
              {!staged && <Button size="small" disabled={blocked_} onClick={() => run(() => backend.gitStage([target.path]))}>Preparar arquivo</Button>}
            </Flexbox>
            <PatchDiff patch={review.diff.patch} viewMode={mode} showHeader={false} variant="borderless" />
          </Flexbox>
        )}
        {!untracked && hunks.length > 0 && !aligned && (
          <Text as="p" role="status" type="danger" fontSize={12}>Os blocos mudaram desde a leitura. Atualize a tela antes de agir sobre um bloco.</Text>
        )}
        {!untracked && hunks.map((h, i) => {
          // Índice e cabeçalho vêm da lista oficial (`review.hunks[i]`), não da contagem local de `@@`.
          const official = aligned ? review.hunks[i] : null;
          const ref: HunkRef = { path: target.path, index: official?.index ?? -1, header: h.header, revision: review.revision };
          return (
            <Flexbox key={h.index} gap={4} data-hunk={h.index}>
              <Flexbox horizontal align="center" wrap="wrap" gap={6}>
                <Text as="code" code fontSize={12} type="secondary" className="min-w-32 flex-1 break-all">{h.header}</Text>
                <Button size="small" onClick={() => common.openLine(target.path, h.newLine)}>Abrir no arquivo</Button>
                {staged
                  ? <Button size="small" disabled={blockOff} onClick={() => run(() => backend.gitUnstageHunk(ref))}>Tirar do stage</Button>
                  : <Button size="small" disabled={blockOff} onClick={() => run(() => backend.gitStageHunk(ref))}>Preparar</Button>}
                {!staged && (
                  <Button
                    size="small" danger disabled={blockOff || draftOpen}
                    title={draftOpen ? 'Resolva o rascunho aberto deste arquivo antes de descartar.' : 'Descartar este bloco'}
                    onClick={async () => {
                      // Confirma mostrando o patch capturado agora; o clique em confirmar usa este mesmo `ref`, sem reler outro patch.
                      const ok = await common.ask({
                        title: 'Descartar este bloco?',
                        body: <Text as="pre" fontSize={12} className="max-h-64 overflow-auto whitespace-pre">{`${head}\n${h.text}`}</Text>,
                        actions: [{ key: 'ok', label: 'Descartar', danger: true }],
                      });
                      if (ok !== 'ok') return;
                      if (common.hasDraft(target.path)) {
                        toast.error({ title: 'Há um rascunho aberto deste arquivo. Salve ou descarte antes de descartar o bloco.', duration: 6000 });
                        return;
                      }
                      await run(() => backend.gitDiscardHunk(ref));
                    }}
                  >
                    Descartar bloco
                  </Button>
                )}
              </Flexbox>
              <PatchDiff patch={`${head}\n${h.text}`} viewMode={mode} showHeader={false} variant="borderless" />
            </Flexbox>
          );
        })}
      </Flexbox>
    </Flexbox>
  );
}

// ---- Histórico (commit): leitura imutável, sem botões de mutação ----

function CommitTab({ target, common }: { target: Extract<TabTarget, { kind: 'commit' }>; common: Common }) {
  const [diff, setDiff] = useState<GitDiff | null>(null);
  const [error, setError] = useState('');
  const [mode, setMode] = useState<'split' | 'unified'>('unified');
  useEffect(() => {
    if (!common.active) return;
    if (diff) return; // histórico é imutável: lê uma vez
    backend.gitCommitDiff(target.hash, target.path).then(setDiff, (e) => setError(message(e)));
  }, [common.active, target.hash, target.path]);
  const note = (text: string, danger?: boolean) => <Flexbox padding={16}><Text as="p" role="status" fontSize={14} type={danger ? 'danger' : 'secondary'}>{text}</Text></Flexbox>;
  if (error) return note(error, true);
  if (!diff) return note('Lendo o commit…');
  if (diff.binary) return note('Arquivo binário: o diff não é mostrado.');
  if (diff.tooLarge) return note('Diff grande demais para mostrar aqui.');
  if (diff.patch === '') return note('Este commit não altera o texto deste arquivo.');
  return (
    <Flexbox flex={1} className="min-h-0">
      <Flexbox horizontal align="center" gap={6} height={32} flex="none" paddingInline={12} className="border-b">
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={target.path} className="min-w-0">{target.path}</Text>
        <Text as="span" type="secondary" fontSize={12} className="shrink-0">commit {target.hash.slice(0, 7)} · somente leitura</Text>
        <Button size="small" className="ml-auto shrink-0" onClick={() => setMode(mode === 'split' ? 'unified' : 'split')}>
          {mode === 'split' ? 'Unificado' : 'Lado a lado'}
        </Button>
      </Flexbox>
      <Flexbox flex={1} className="min-h-0 overflow-y-auto">
        <PatchDiff patch={diff.patch} viewMode={mode} showHeader={false} variant="borderless" />
      </Flexbox>
    </Flexbox>
  );
}

// ---- Conflito: blocos por trecho (marcadores normais e diff3), texto final editável ----
// O rascunho é a fonte: os trechos são lidos do texto dele a cada render. O que já foi escolhido sai da lista, o que ainda tem
// marcador continua com ação, e edição à mão não precisa ser descartada. Gravar (`save`) fica com o editor central.

function ConflictTab({ path, common, shown, draft, putConflict, dropConflict, save, closeTab }: {
  path: string; common: Common; shown: boolean; draft: ConflictDraft | undefined;
  putConflict: (draft: ConflictDraft) => void; dropConflict: (path: string) => void;
  save: (markResolved: boolean) => Promise<boolean>; closeTab: () => void;
}) {
  const [conflict, setConflict] = useState<Conflict | null>(null);
  const [error, setError] = useState('');
  const [stale, setStale] = useState(false);
  const busyRef = useRef(false);
  const [busy, setBusy] = useState(false);
  // `reset` troca o rascunho pelo texto atual da pasta; só é chamado depois da confirmação. Sem ele, respeita o rascunho.
  const load = async (reset = false) => {
    try {
      const fresh = await backend.gitConflict(path);
      setConflict(fresh);
      setError('');
      if (!draft || reset) {
        const text = fresh.worktree ?? '';
        putConflict({ path, origin: text, draft: text, revision: fresh.revision });
        setStale(false);
      } else {
        setStale(draft.revision !== fresh.revision);
      }
    } catch (e) {
      setError(message(e));
    }
  };
  // Aba escondida não relê: ao voltar a ser a atual, relê a pasta (a revisão do rascunho pode ter ficado velha).
  useEffect(() => {
    if (!common.active || !shown) return;
    void load();
  }, [common.active, shown, path]);

  const text = draft?.draft ?? '';
  // Trechos lidos do texto atual do rascunho (inclusive o que já foi resolvido à mão ou por trecho).
  const parsed = conflict ? parseConflict(text) : null;
  const pieces = parsed?.pieces ?? [];
  const count = blockCount(pieces);
  const resolvable = !!parsed && !parsed.malformed;
  const markersLeft = hasMarkers(text);
  const deletion = conflict !== null && (conflict.ours === null || conflict.theirs === null);
  const absent = conflict !== null && conflict.worktree === null;
  const fileOpen = common.fileDirty(path);
  const canMark = resolvable && !markersLeft && !common.locked && !busy && !fileOpen;

  const run = async (work: () => Promise<unknown>) => {
    if (busyRef.current || common.locked) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await work();
    } catch (e) {
      toast.error({ title: message(e), duration: 7000 });
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  // Escolhe o lado de um trecho do rascunho atual; os demais trechos ficam com marcadores.
  const choose = (index: number, value: Choice) => {
    if (!draft || !resolvable) return;
    const picks: Choice[] = Array.from({ length: count }, (_, i) => (i === index ? value : 'pending'));
    putConflict({ ...draft, draft: renderConflict(pieces, picks) });
  };
  // Restaura a versão de um lado no rascunho. Não grava: quem grava é o botão Salvar ou Marcar.
  const restore = (value: string) => {
    if (draft) putConflict({ ...draft, draft: value });
  };

  const note = (message_: string, danger?: boolean) => <Flexbox padding={16}><Text as="p" role="status" fontSize={14} type={danger ? 'danger' : 'secondary'}>{message_}</Text></Flexbox>;
  if (error && !conflict) return note(error, true);
  if (!conflict || !draft) return note('Lendo o conflito…');
  if (conflict.binary) return note('Conflito em arquivo binário: escolha um lado pelo Git; não há texto para resolver aqui.');

  return (
    <Flexbox flex={1} className="min-h-0">
      <Flexbox horizontal align="center" wrap="wrap" gap={6} paddingInline={12} paddingBlock={4} flex="none" className="border-b">
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={path} className="min-w-0">{path}</Text>
        <Text as="span" type="secondary" fontSize={12} className="shrink-0">conflito</Text>
        {stale && <Text as="span" role="alert" type="danger" fontSize={12} className="min-w-0">A pasta mudou desde o seu rascunho.</Text>}
        {error && <Text as="span" role="status" type="danger" fontSize={12} className="min-w-0">{error}</Text>}
        <span className="ml-auto flex flex-wrap items-center gap-1.5">
          <Button
            size="small"
            onClick={async () => {
              if (stale) {
                const ok = await common.ask({
                  title: 'Recarregar do Git?',
                  body: 'O rascunho deste conflito será trocado pelo texto atual da pasta. As escolhas feitas aqui se perdem.',
                  actions: [{ key: 'ok', label: 'Recarregar' }],
                });
                if (ok !== 'ok') return;
                await load(true);
                return;
              }
              await load();
            }}
          >
            {stale ? 'Recarregar do Git' : 'Atualizar'}
          </Button>
          <Button size="small" disabled={!isConflictDirty(draft) || common.locked || busy || fileOpen} onClick={() => run(() => save(false))}>Salvar</Button>
          <Button size="small" disabled={!canMark} onClick={() => run(() => save(true))}>Marcar como resolvido</Button>
        </span>
      </Flexbox>
      {fileOpen && (
        <Flexbox horizontal align="center" paddingInline={12} height={28} flex="none" className="border-b">
          <Text as="span" role="status" type="danger" fontSize={12}>Este arquivo também está aberto no editor com alterações. Salve ou descarte antes de gravar a resolução.</Text>
        </Flexbox>
      )}
      {absent && (
        <Flexbox gap={6} paddingInline={12} paddingBlock={6} flex="none" className="border-b">
          <Text as="span" role="status" type="secondary" fontSize={12}>
            O arquivo não existe na pasta. {deletion ? 'Um lado apagou o arquivo: aceite a remoção ou restaure a versão de um lado.' : 'Restaure a versão de um lado para editar e salvar.'}
          </Text>
          <Flexbox horizontal align="center" wrap="wrap" gap={6}>
            {conflict.ours !== null && (
              <Button size="small" disabled={common.locked || busy} onClick={() => restore(conflict.ours ?? '')}>Restaurar versão atual</Button>
            )}
            {conflict.theirs !== null && (
              <Button size="small" disabled={common.locked || busy} onClick={() => restore(conflict.theirs ?? '')}>Restaurar versão recebida</Button>
            )}
          </Flexbox>
        </Flexbox>
      )}
      {deletion && !absent && (
        <Flexbox horizontal align="center" wrap="wrap" gap={8} paddingInline={12} paddingBlock={4} flex="none" className="border-b">
          <Text as="span" type="secondary" fontSize={12}>Um lado apagou o arquivo. Marcar como resolvido mantém o arquivo como está.</Text>
        </Flexbox>
      )}
      {deletion && (
        <Flexbox horizontal align="center" wrap="wrap" gap={8} paddingInline={12} paddingBlock={4} flex="none" className="border-b">
          <Button
            size="small" danger disabled={common.locked || busy}
            onClick={async () => {
              const ok = await common.ask({
                title: 'Aceitar a remoção do arquivo?',
                body: `O arquivo ${path} sai do projeto, conforme o lado que o apagou.`,
                actions: [{ key: 'ok', label: 'Aceitar remoção', danger: true }],
              });
              if (ok !== 'ok') return;
              // Revisão de quando a tela carregou o conflito; se a pasta mudou, o Git recusa e nada é removido.
              const revision = conflict.revision;
              await run(async () => {
                await backend.gitAcceptDeletion(path, revision);
                dropConflict(path);
                common.refreshGit();
                closeTab();
              });
            }}
          >
            Aceitar a remoção
          </Button>
        </Flexbox>
      )}
      {!absent && !resolvable && (
        <Flexbox paddingInline={12} paddingBlock={6} flex="none" className="border-b">
          <Text as="span" type="secondary" fontSize={12}>{parsed?.malformed ? 'Marcadores de conflito incompletos: edite o texto final à mão.' : 'Edite o texto final à mão.'}</Text>
        </Flexbox>
      )}
      {!absent && resolvable && count > 0 && (
        <Flexbox gap={6} padding={8} className="max-h-40 overflow-y-auto border-b">
          <Text as="span" type="secondary" fontSize={12}>{count === 1 ? '1 trecho com conflito' : `${count} trechos com conflito`} no texto atual:</Text>
          {Array.from({ length: count }, (_, i) => (
            <Flexbox key={i} horizontal align="center" wrap="wrap" gap={6}>
              <Text as="span" fontSize={12} className="min-w-0 flex-1">Trecho {i + 1}</Text>
              {(['ours', 'theirs', 'both'] as const).map((value) => (
                <Button key={value} size="small" disabled={common.locked || busy} onClick={() => choose(i, value)}>
                  {CHOICE_LABEL[value]}
                </Button>
              ))}
            </Flexbox>
          ))}
        </Flexbox>
      )}
      <Flexbox flex={1} className="min-h-0 pr-1 pb-1">
        <Flexbox flex={1} className="min-h-0 overflow-hidden">
          <CodeEditor
            text={text}
            readOnly={common.locked}
            path={path}
            reveal={null}
            onChange={(value) => putConflict({ ...draft, draft: value })}
            onSave={() => void run(() => save(false))}
            onCursor={() => undefined}
          />
        </Flexbox>
      </Flexbox>
    </Flexbox>
  );
}

const CHOICE_LABEL: Record<Choice, string> = { ours: 'Atual', theirs: 'Recebida', both: 'Ambas', pending: 'Pendente' };
