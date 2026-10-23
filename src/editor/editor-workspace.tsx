// Área central do editor: até dois grupos de abas lado a lado. Cada aba é um arquivo (CodeMirror), uma imagem ou um áudio (só leitura,
// `media-view.tsx`), um diff staged/unstaged, um commit do histórico (só leitura) ou um conflito de merge. Buffers e rascunhos são do arquivo (`docId`), não da aba;
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
// Recuperação: localStorage por projeto (`orquestrador.editor:<project>`) e uma chave fixa no modo sem projeto.
import {
  forwardRef, Fragment, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type ComponentProps, type FocusEvent, type KeyboardEvent, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode,
} from 'react';
import { ActionIcon, Button, ContextMenuTrigger, type ContextMenuItem, DropdownMenu, Modal, Text, toast } from '@lobehub/ui/base-ui';
import { Center, Flexbox, PatchDiff } from '@lobehub/ui';
import { ArrowRight, ChevronDown, ChevronUp, Columns2, Ellipsis, FileText, Minus, Pin, Plus, Rows2, Undo2, X } from 'lucide-react';
import { EditorView } from '@codemirror/view';
import { FileIcon } from '../shared/file-icon';
import { AppMark } from '../window/app-mark';
import { Breadcrumbs } from './breadcrumbs';
import { backend, message } from '../backend/backend';
import { usePolling } from '../shared/polling';
import { CodeEditor, forgetEditorState, type Reveal, type StartCursor } from './file-view';
import { MediaView, mediaOf, Note } from './media-view';
import { action, blocked, DIVIDER, fileName, useChord } from '../shared/file-tabs';
import { pathText } from '../shared/file-tab-actions';
import { copy, splitPath } from '../shared/paths';
import {
  activate, closeTabs, createWorkspace, draftName, isDirty, isSafePath, isWithin, joinGroups, liveDocIds, loadWorkspace, markEdited, moveTab, navigate,
  openDraft, openPermanent, openPreview, planClose, promoteDraft, remapPath, reopenClosed, splitTab, targetKey, togglePin, type CloseScope, type EditorTab, type GroupId, type TabTarget, type WorkspaceState,
} from './workspace-model';
import { dragShift, revealTab, trackTabDrag, type TabDrag, wheelToScroll } from '../shared/tab-strip';
import { TabListMenu, type TabListItem } from '../shared/tab-list-menu';
import {
  blockCount, type Buffer, type Choice, type ConflictDraft, decodeRecovery, dirtyKeys, encodeRecovery, hasDraftAt,
  hasMarkers, isBufferDirty, isConflictDirty, newlineOf, normalizeNewlines, parseConflict, patchHunks, type PatchHunk, type Recovery,
  remapData, renderConflict, toDiskNewlines, conflictKey, type EditorData, type Cursor, type Newline, type Scroll,
} from './workspace-store';
import type { GitGuard } from '../git/git-guard';
import type { Conflict, FileBase, FileContent, GitDiff, HunkRef, ReviewDiff } from '../backend/types';
import { blockRanges, changeKeys, compareOf, type CompareModel } from './compare-rows';

export type OpenOptions = { permanent?: boolean; line?: number; col?: number; side?: GroupId };
export type EditorWorkspaceHandle = {
  newFile(): void;
  saveActive(): Promise<boolean>;
  saveActiveAs(): Promise<boolean>;
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
const NO_PROJECT = '__sem-projeto__';
const POLL_MS = 2500;
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

// Caminho do diálogo "Salvar como" (absoluto, com a raiz do projeto) como caminho relativo com "/". Null se ficou fora do projeto.
// Sem diferença de caixa, como no Windows.
function relativeToProject(project: string, picked: string): string | null {
  if (!project) return null;
  const root = project.replace(/\\/g, '/').replace(/\/+$/, '');
  const full = picked.replace(/\\/g, '/');
  if (full.slice(0, root.length + 1).toLowerCase() !== `${root}/`.toLowerCase()) return null;
  return full.slice(root.length + 1);
}

function absoluteFilePath(project: string, path: string): string {
  if (!path) return path;
  if (isAbsoluteFilePath(path)) return path.replace(/\//g, '\\');
  return project.replace(/[\\/]+$/, '') + '\\' + path.replace(/\//g, '\\');
}

const isAbsoluteFilePath = (path: string) => /^(?:[a-z]:[\\/]|\\\\|\/)/i.test(path);

// Grava `text` em `path` (relativo, dentro do projeto). Arquivo novo é criado; se já existe (o diálogo confirmou a sobrescrita), o texto
// vai por cima da versão que está no disco agora, sem gravar às cegas.
async function writeNewFile(path: string, text: string): Promise<void> {
  if (isAbsoluteFilePath(path)) {
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
  } catch {
    const current = await backend.readFile(path);
    if (current.binary || current.tooLarge) throw new Error(`${fileName(path)} não é texto: não dá para salvar por cima dele.`);
    base = current.text;
    bom = current.bom;
  }
  await backend.writeFileChecked(path, text, base, bom);
}

function emptyData(): EditorData { return { buffers: {}, conflicts: {} }; }

export const EditorWorkspace = forwardRef<EditorWorkspaceHandle, EditorWorkspaceProps>(function EditorWorkspace(props, handle) {
  const { project, active = true, locked, guard, refreshGit, onActiveTarget, layout, onRestoreLayout, onReveal, onGroupCount, onSaved } = props;
  const storageProject = project || NO_PROJECT;
  // Ref para que as gravações antigas (ainda em await) avisem com a prop atual.
  const savedRef = useRef(onSaved);
  savedRef.current = onSaved;
  const notifySaved = () => savedRef.current?.();

  // Restaura a recuperação uma vez, na montagem (lê o localStorage de forma síncrona e valida tudo).
  const [boot] = useState(() => {
    let text: string | null = null;
    let readFailed = false;
    const notes: string[] = [];
    try {
      text = localStorage.getItem(STORAGE(storageProject));
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
  const askOpen = useRef(false);
  const revealSeq = useRef(0);
  const layoutRef = useRef(layout ?? boot.layout);
  const splitRef = useRef(split);
  // Aba em arrasto na faixa de `group`; `into` é o outro grupo, com o ponteiro sobre a faixa dele.
  const [tabDrag, setTabDrag] = useState<({ id: string; group: GroupId; from: number; into: GroupId | null } & TabDrag) | null>(null);
  const splitDrag = useRef<{ startX: number; start: number; width: number } | null>(null);
  // Sujos: rascunhos de arquivo e de conflito.
  const dirtyNow = (d: EditorData = dataRef.current) => dirtyKeys(d);

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
    const dirty = dirtyNow();
    const group: GroupId = options.side ?? wsRef.current.focus;
    const base = groupWithDefault(wsRef.current, group);
    setWs(options.permanent ? openPermanent(base, group, target) : openPreview(base, group, target, dirty));
  };

  const openFile = (path: string, options: OpenOptions = {}) => {
    const docId = docIdFor(path);
    openTarget({ kind: 'file', docId, path }, options);
    if (options.line !== undefined) setReveal({ docId, line: options.line, col: options.col, id: ++revealSeq.current });
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
    try {
      // Grava só se o disco ainda é o `raw` lido (base crua, sem normalizar). Conflito é tratado abaixo, não há força silenciosa.
      await backend.writeFileChecked(buffer.path, sent, buffer.raw, buffer.bom);
      patchBuffer(docId, (b) => ({ ...b, raw: sent }));
      clearDisk(docId);
      setStatusOf(docId, {});
      refreshGit();
      notifySaved();
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
    if (!guard.enter()) return false;
    conflictSaving.current.add(key);
    const sent = current.draft;
    try {
      await backend.gitWriteResolution(path, sent, current.revision, markResolved);
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
    } catch (e) {
      toast.error({ title: message(e), duration: 7000 });
      return false;
    } finally {
      conflictSaving.current.delete(key);
      guard.leave();
    }
  };

  // Rascunho sem caminho: o diálogo começa na pasta do projeto quando há uma; fora dela, o caminho segue absoluto.
  const saveDraft = async (docId: string): Promise<boolean> => {
    const tab = allTabs().find((t) => t.target.kind === 'draft' && t.target.docId === docId);
    const buffer = dataRef.current.buffers[docId];
    if (!tab || tab.target.kind !== 'draft' || !buffer || saving.current.has(docId)) return false;
    saving.current.add(docId);
    try {
      const picked = await backend.pickSavePath(project || null, draftName(tab.target.num));
      if (picked === null) return false;
      const absolute = picked.replace(/\\/g, '/');
      const path = relativeToProject(project, absolute) ?? absolute;
      if (!isAbsoluteFilePath(path) && !isSafePath(path)) return false;
      const lower = path.toLowerCase();
      const existing = allTabs().find((t) => t.target.kind === 'file' && t.target.path.toLowerCase() === lower);
      const existingBuffer = existing?.target.kind === 'file' ? dataRef.current.buffers[existing.target.docId] : undefined;
      if (existingBuffer && isBufferDirty(existingBuffer)) {
        toast.error({ title: `Salve ou descarte a edição aberta de ${fileName(path)} antes de salvar por cima dele.`, duration: 7000 });
        return false;
      }
      const sent = toDiskNewlines(buffer.draft, buffer.newline);
      await writeNewFile(path, sent);
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
    } catch (e) {
      toast.error({ title: message(e), duration: 7000 });
      return false;
    } finally {
      saving.current.delete(docId);
    }
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
    const directory = project || (isAbsoluteFilePath(buffer.path) && slash > 0 ? buffer.path.slice(0, slash) : null);
    try {
      const picked = await backend.pickSavePath(directory, fileName(buffer.path));
      if (!picked) return false;
      const absolute = picked.replace(/\\/g, '/');
      const path = relativeToProject(project, absolute) ?? absolute;
      if (path.toLowerCase() === buffer.path.toLowerCase()) return saveBuffer(target.docId);
      const existing = allTabs().find((t) => t.target.kind === 'file' && t.target.path.toLowerCase() === path.toLowerCase());
      const existingBuffer = existing?.target.kind === 'file' ? dataRef.current.buffers[existing.target.docId] : undefined;
      if (existingBuffer && isBufferDirty(existingBuffer)) {
        toast.error({ title: `Salve ou descarte a edição aberta de ${fileName(path)} antes de salvar por cima dele.`, duration: 7000 });
        return false;
      }
      await writeNewFile(path, toDiskNewlines(buffer.draft, buffer.newline));
      const saved = await backend.readFile(path);
      if (existing) {
        const source = allTabs().filter((t) => t.target.kind === 'file' && t.target.docId === target.docId).map((t) => t.id);
        const next = activate(closeTabs(wsRef.current, source), existing.id);
        setWs(next);
        pruneData(next);
        void syncFile(existing.target.kind === 'file' ? existing.target.docId : '');
      } else {
        remap(buffer.path, path);
        patchBuffer(target.docId, (b) => ({ ...b, path, raw: saved.text, bom: saved.bom, newline: newlineOf(saved.text) }));
      }
      refreshGit();
      notifySaved();
      return true;
    } catch (e) {
      toast.error({ title: message(e), duration: 7000 });
      return false;
    }
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
  layoutRef.current = layout ?? layoutRef.current;
  splitRef.current = split;

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
        localStorage.removeItem(STORAGE(storageProject));
        return;
      }
      const encoded = encodeRecovery(storageProject, recovery);
      localStorage.setItem(STORAGE(storageProject), encoded.text);
      if (encoded.notes.length > 0) setNotice(encoded.notes.join(' '));
    } catch (e) {
      setNotice(`Não foi possível guardar a recuperação (${message(e)}). A edição continua, mas não será recuperada.`);
    }
  };
  const persistRef = useRef(persist);
  persistRef.current = persist;
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
    const sent = toDiskNewlines(choice.draft, newlineOf(choice.disk.text));
    let failed: unknown = null;
    try {
      await backend.writeFileChecked(buffer.path, sent, choice.disk.text, choice.disk.bom);
      // Quem digitou durante o await continua sujo: o `raw` vira o texto gravado, e o `draft` atual fica como está.
      patchBuffer(choice.docId, (b) => ({ ...b, raw: sent, bom: choice.disk.bom, newline: newlineOf(choice.disk.text) }));
      clearDisk(choice.docId);
      setStatusOf(choice.docId, {});
      refreshGit();
      notifySaved();
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
          : target.kind === 'file' && isAbsoluteFilePath(target.path) && relativeToProject(project, target.path) === null
            ? blocked('reveal', 'Revelar no Explorer', 'Este arquivo está fora da pasta do projeto.')
          : action('reveal', 'Revelar no Explorer', () => onReveal(target.path)),
      ],
    ];
    return sections.flatMap((items, i) => (i ? [DIVIDER, ...items] : items));
  };

  const bodyOf = (tab: EditorTab, groupActive: boolean, shown: boolean): ReactNode => {
    const target = tab.target;
    if (target.kind === 'diff') return <DiffTab target={target} common={common} shown={shown} />;
    if (target.kind === 'commit') return <CommitTab target={target} />;
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
        autoFocus={target.kind === 'draft' && freshDraft.current === target.docId}
        cursor={cursors.current[target.docId] ?? null}
        onChange={(value) => {
          patchBuffer(target.docId, (b) => ({ ...b, draft: value, newline: value === normalizeNewlines(b.raw) ? newlineOf(b.raw) : b.newline }));
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
        scroll={scrolls.current[target.docId] ?? null}
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
  const dirty = dirtyNow(data);
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
          ref={stripRefFor(groupId)} role="tablist" aria-label={`Abas do grupo ${groupId}`}
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
                    <span className="min-w-0 truncate">{titleOf(tab.target)}</span>
                    {detail && (
                      <span className="min-w-0 truncate text-[12px] text-[var(--ant-color-text-secondary)]">{detail}</span>
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
function useGitBase(path: string, raw: string, shown: boolean, active: boolean): { base: FileBase | null | undefined; reload: () => Promise<void> } {
  const [base, setBase] = useState<FileBase | null | undefined>(undefined);
  const pathRef = useRef(path);
  pathRef.current = path;
  const load = async () => {
    let next: FileBase | null = null;
    try {
      const found = await backend.gitFileBase(path);
      if (found) next = { head: found.head === null ? null : normalizeNewlines(found.head), index: found.index === null ? null : normalizeNewlines(found.index) };
    } catch {
      // Erro de Git não interrompe a edição: só tira os marcadores.
    }
    if (pathRef.current !== path) return;
    setBase((prev) => (prev === next || (prev && next && prev.head === next.head && prev.index === next.index) ? prev : next));
  };
  usePolling(load, POLL_MS, active && shown && path !== '' && !isAbsoluteFilePath(path) && path);
  const seenRaw = useRef(raw);
  useEffect(() => {
    if (seenRaw.current === raw) return;
    seenRaw.current = raw;
    if (path !== '' && !isAbsoluteFilePath(path)) void load();
  }, [raw]);
  return { base: path === '' || isAbsoluteFilePath(path) ? null : base, reload: load };
}

function FileBody({ project, active, buffer, shown, status, disk, reveal, autoFocus, cursor, onChange, onFormatEol, onSave, onCursor, onCompare, onRetry, scroll, onScroll }: {
  project: string; active: boolean;
  buffer: Buffer | undefined; shown: boolean; status: Status | undefined; disk: DiskVersion | undefined;
  reveal: Reveal | null; autoFocus: boolean; cursor: StartCursor | null; onChange: (value: string) => void; onSave: () => void;
  onFormatEol: (newline: Newline) => void;
  onCursor: (line: number, col: number) => void; onCompare: (mode: 'keep' | 'use') => void; onRetry: () => void;
  scroll: Scroll | null; onScroll: (scroll: Scroll) => void;
}) {
  const git = useGitBase(buffer?.path ?? '', buffer?.raw ?? '', shown, active);
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
      <Flexbox flex={1} className="min-h-0">
        <Flexbox flex={1} className="min-h-0 overflow-hidden">
          <CodeEditor
            text={buffer.draft}
            readOnly={buffer.readOnly}
            autoFocus={autoFocus}
            path={absoluteFilePath(project, buffer.path)}
            reveal={reveal}
            onChange={onChange}
            onFormatEol={onFormatEol}
            onSave={onSave}
            onCursor={onCursor}
            onScroll={onScroll}
            scroll={scroll}
            cacheKey={buffer.docId}
            start={cursor}
            gitBase={git.base}
            onWriteIndex={async (text, expected) => {
              if (isAbsoluteFilePath(buffer.path)) return;
              await backend.gitWriteIndex(buffer.path, text, expected);
              await git.reload();
            }}
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

// ---- Diff (preparadas e alterações) e commit: o arquivo inteiro num comparador contínuo ----

type ViewMode = 'split' | 'unified';

// Comparador: lado a lado por padrão, com a fonte e as cores do editor (docs/interface.md). Linhas longas quebram dentro das colunas;
// o motor alinha os lados. A geometria é medida nos elementos de linha do shadow DOM depois de cada render.
type DiffOptions = NonNullable<ComponentProps<typeof PatchDiff>['diffOptions']>;
// A grade tem cinco trilhas: número, texto, vão de 40 px, número e texto.
const COMPARE_DIFF_OPTIONS: DiffOptions = {
  theme: { dark: 'dark-plus', light: 'dark-plus' },
  overflow: 'wrap',
  diffIndicators: 'none',
  lineDiffType: 'word-alt',
  unsafeCSS: `
    :host {
      --diffs-dark-bg: #1F1F1F;
      --diffs-light-bg: #1F1F1F;
      --diffs-bg-context-override: #1F1F1F;
      --diffs-bg-context-gutter-override: #1F1F1F;
      --diffs-bg-buffer-override: #1F1F1F;
      --diffs-bg-separator-override: #1F1F1F;
      --diffs-bg-addition-override: #9bb95533;
      --diffs-bg-deletion-override: #ff000033;
      --diffs-bg-addition-emphasis-override: #9ccc2c33;
      --diffs-bg-deletion-emphasis-override: #ff000033;
      --diffs-fg-number: #6E7681;
      --diffs-font-family: Consolas, 'Courier New', monospace;
      --diffs-font-size: 14px;
      --diffs-line-height: 19px;
    }
    [data-separator-wrapper], [data-separator] { display: none; }
    pre[data-diff-type='split'][data-overflow='wrap'] { grid-template-columns: var(--diffs-grid-number-column-width) minmax(0, var(--compare-l, 50fr)) 40px var(--diffs-grid-number-column-width) minmax(0, var(--compare-r, 50fr)); background-color: #1f1f1f; }
    pre[data-diff-type='split'][data-overflow='wrap'] > [data-deletions] > [data-gutter] { grid-column: 1; }
    pre[data-diff-type='split'][data-overflow='wrap'] > [data-deletions] > [data-content] { grid-column: 2; border-right: 1px solid #3c3c3c; }
    pre[data-diff-type='split'][data-overflow='wrap'] > [data-additions] > [data-gutter] { grid-column: 4; border-left: 0; }
    pre[data-diff-type='split'][data-overflow='wrap'] > [data-additions] > [data-content] { grid-column: 5; }
    pre[data-diff-type='split'] [data-column-number] { padding-inline-end: 4px; }
    pre[data-diff-type='split'] [data-column-number][data-line-type='change-deletion']::after,
    pre[data-diff-type='single'] [data-column-number][data-line-type='change-deletion']::after { content: '−'; margin-inline-start: 4px; color: #6e7681; }
    pre[data-diff-type='split'] [data-column-number][data-line-type='change-addition']::after,
    pre[data-diff-type='single'] [data-column-number][data-line-type='change-addition']::after { content: '+'; margin-inline-start: 4px; color: #6e7681; }
    pre[data-background] [data-line][data-line-type='change-deletion'] { --diffs-line-bg: #ff000033; }
    pre[data-background] [data-line][data-line-type='change-addition'] { --diffs-line-bg: #9bb95533; }
    pre[data-background] [data-line][data-line-type='change-deletion'] span[data-decoration-bg] { --diffs-decoration-bg: #ff000033; }
    pre[data-background] [data-line][data-line-type='change-addition'] span[data-decoration-bg] { --diffs-decoration-bg: #9ccc2c33; }
    pre[data-diff-type='split'] [data-content-buffer] { background-image: linear-gradient(-45deg, #cccccc33 12.5%, #0000 12.5%, #0000 50%, #cccccc33 50%, #cccccc33 62.5%, #0000 62.5%, #0000 100%); background-size: 8px 8px; background-color: #1f1f1f; }
    [data-code] { padding-top: 0 !important; padding-bottom: 0 !important; }
  `,
};

// Alterna lado a lado e unificado. O rótulo é o modo para onde o clique leva (lado a lado é o padrão).
function ModeToggle({ mode, onChange }: { mode: ViewMode; onChange: (mode: ViewMode) => void }) {
  const split = mode === 'split';
  const label = split ? 'Unificado' : 'Lado a lado';
  return <ActionIcon size="small" variant="borderless" icon={split ? Rows2 : Columns2} aria-label={label} title={label} onClick={() => onChange(split ? 'unified' : 'split')} />;
}

// Cabeçalho do arquivo no patch (tudo antes do primeiro `@@`). O descarte de bloco mostra o cabeçalho com o bloco.
const fileHead = (patch: string) => {
  const at = patch.indexOf('\n@@');
  return at < 0 ? patch : patch.slice(0, at);
};
// Arquivo criado (não rastreado, preparado ou num commit): o cabeçalho traz `new file mode`, a mesma regra com que o motor desenha uma
// coluna só. Sem original para comparar, não há divisor nem troca entre lado a lado e unificado.
const isNewFile = (patch: string) => /^new file mode /m.test(fileHead(patch));

// Geometria medida no motor, em pixels do palco: cada unidade vai do topo da primeira linha de mudança dela ao fim da última. A medida
// vem do DOM depois de cada render (`onPostRender`), então as linhas auxiliares (o aviso de fim sem quebra) entram sem estimativa.
type Geo = { patch: string; mode: ViewMode; height: number; units: ([number, number] | null)[] };
const sameGeo = (a: Geo, b: Geo) => a.patch === b.patch && a.mode === b.mode && a.height === b.height && JSON.stringify(a.units) === JSON.stringify(b.units);

// Divisor do lado a lado: cada lado tem pelo menos 15%, no arraste e nas setas.
const SPLIT_MIN = 15;
const SPLIT_MAX = 85;
const clampSplit = (pct: number) => Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, pct));

// O arquivo inteiro numa só visão: nada separa as mudanças. A unidade atual (bloco, ou trecho no commit) tem contorno, cada unidade
// tem uma marca na faixa da direita, e um clique numa mudança a escolhe. A rolagem vai à primeira linha da unidade atual, e só quando
// ela, o modo ou o arquivo mudam: a atualização do diff não rola a tela. No lado a lado, a proporção das colunas de texto e a posição do vão
// são atualizadas no palco sem refazer o diff nem rolar a tela.
function Continuous({ patch, compare, mode: asked, current, unitName, onPick, center }: {
  patch: string; compare: CompareModel; mode: ViewMode; current: number; unitName: string; onPick: (unit: number) => void; center?: ReactNode;
}) {
  const mode: ViewMode = isNewFile(patch) ? 'unified' : asked;
  const body = useRef<HTMLDivElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const [geo, setGeo] = useState<Geo | null>(null);
  const [left, setLeft] = useState(50);
  const sash = useRef<HTMLDivElement>(null);
  // Chave de cada linha de mudança (ver `changeKeys`) para a unidade dela. As duas visões têm as mesmas linhas de mudança.
  const unitOfKey = useMemo(() => {
    const m = new Map<string, number>();
    for (const r of compare.unified) for (const k of changeKeys(r)) m.set(k, r.unit);
    return m;
  }, [compare]);
  // O motor chama `onPostRender` e `onLineClick` fora do render: eles leem o estado atual por esta referência. As opções do motor
  // ficam estáveis, então um diff igual não é refeito.
  const latest = useRef({ patch, compare, mode, onPick, unitOfKey });
  latest.current = { patch, compare, mode, onPick, unitOfKey };

  // Mede as linhas de mudança e as unidades no DOM do motor (o shadow DOM do diff). O modo é o que o motor desenhou (a `pre`), que pode
  // ainda ser o anterior logo depois da troca. Só troca o estado quando a geometria muda.
  const measure = () => {
    const el = stage.current;
    const root = el?.querySelector('diffs-container')?.shadowRoot;
    const pre = root?.querySelector('pre[data-diff-type]');
    // Aba escondida (outra aba ativa) não tem geometria: não mede. Ao aparecer, o `useLayoutEffect` abaixo mede.
    if (!el || !root || !pre || el.getClientRects().length === 0) return;
    const drawn: ViewMode = pre.getAttribute('data-diff-type') === 'split' ? 'split' : 'unified';
    const { patch: text, compare: model } = latest.current;
    const box = el.getBoundingClientRect();
    const at = new Map<string, [number, number]>();
    root.querySelectorAll('[data-line]').forEach((line) => {
      const t = line.getAttribute('data-line-type');
      if (t !== 'change-deletion' && t !== 'change-addition') return;
      const r = line.getBoundingClientRect();
      at.set((t === 'change-deletion' ? 'd' : 'a') + line.getAttribute('data-line'), [r.top - box.top, r.bottom - box.top]);
    });
    const rows = drawn === 'split' ? model.split : model.unified;
    const units = model.units.map((u): [number, number] | null => {
      const s = u[drawn];
      if (s[0] < 0) return null;
      const first = at.get(changeKeys(rows[s[0]])[0] ?? '');
      const last = at.get(changeKeys(rows[s[1]])[0] ?? '');
      return first && last ? [first[0], last[1]] : null;
    });
    if (drawn === 'split') {
      const content = root.querySelector('pre[data-diff-type="split"] > [data-deletions] > [data-content]');
      if (content) stage.current?.style.setProperty('--compare-center-x', `${content.getBoundingClientRect().right - box.left + 20}px`);
    }
    const next: Geo = { patch: text, mode: drawn, height: box.height, units };
    setGeo((prev) => (prev && sameGeo(prev, next) ? prev : next));
  };
  // A rolagem pede a primeira linha da unidade atual. Só é pedida quando a unidade, o modo ou o arquivo (pela `key`) mudam; a geometria
  // chega depois do render do motor, então o pedido espera por ela.
  const jump = useRef(true);
  useEffect(() => { jump.current = true; }, [current, mode]);
  useEffect(() => {
    const ready = geo && geo.patch === patch && geo.mode === mode ? geo : null;
    const s = ready?.units[current];
    if (!jump.current || !s || !body.current) return;
    jump.current = false;
    body.current.scrollTop = Math.max(0, s[0] - body.current.clientHeight / 3);
    // Um Próximo desabilitado no último bloco tira o foco da aba (depois do commit); o F8 seguinte tem de achar o corpo, não o documento.
    setTimeout(() => { if (document.activeElement === document.body) body.current?.focus({ preventScroll: true }); }, 0);
  }, [geo, current, mode, patch]);

  // A geometria vale só para o patch e o modo que estão na tela; enquanto o motor não mediu, não há marca nem faixa.
  const ready = geo && geo.patch === patch && geo.mode === mode ? geo : null;
  // Sem medida válida (o patch mudou com a aba escondida, ou ainda não houve render), mede assim que a aba estiver na tela.
  useLayoutEffect(() => { if (!ready) measure(); });
  const cur = ready?.units[current] ?? null;
  const frac = (y: number) => `${(y / (ready?.height || 1)) * 100}%`;
  // Um clique numa linha de mudança escolhe a unidade dela, pelo número e pelo lado que o motor deu.
  const engine = useMemo<DiffOptions>(() => ({
    ...COMPARE_DIFF_OPTIONS,
    onPostRender: () => measure(),
    onLineClick: (props) => {
      const t = props.lineType;
      const unit = latest.current.unitOfKey.get((t === 'change-deletion' ? 'd' : t === 'change-addition' ? 'a' : '') + props.lineNumber);
      if (unit !== undefined) latest.current.onPick(unit);
    },
  }), []);
  // O motor não renderiza de novo no arraste: o pointer só troca a variável do palco e o `aria-valuenow` do divisor, pelo DOM. O estado
  // recebe a proporção no fim. `diff` é o mesmo elemento enquanto o patch e o modo não mudam, então o React não mexe no motor.
  const diff = useMemo(() => (
    <PatchDiff patch={patch} viewMode={mode} showHeader={false} variant="borderless" diffOptions={engine} />
  ), [patch, mode, engine]);
  const drag = useRef<{ x: number; start: number; width: number; pct: number } | null>(null);
  const paint = (pct: number) => {
    const el = stage.current;
    el?.style.setProperty('--compare-l', `${pct}fr`);
    el?.style.setProperty('--compare-r', `${100 - pct}fr`);
    const root = el?.querySelector('diffs-container')?.shadowRoot;
    const content = root?.querySelector('pre[data-diff-type="split"] > [data-deletions] > [data-content]');
    if (el && content) el.style.setProperty('--compare-center-x', `${content.getBoundingClientRect().right - el.getBoundingClientRect().left + 20}px`);
    sash.current?.setAttribute('aria-valuenow', String(Math.round(pct)));
  };
  useLayoutEffect(() => {
    paint(left);
    const el = stage.current;
    if (!el) return;
    const observer = new ResizeObserver(() => { paint(left); measure(); });
    observer.observe(el);
    return () => observer.disconnect();
  }, [left]);
  const onSashDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    // Sem seleção de texto no arraste; o foco vai ao divisor, que é focável.
    e.currentTarget.focus({ preventScroll: true });
    e.currentTarget.setPointerCapture(e.pointerId);
    const root = stage.current?.querySelector('diffs-container')?.shadowRoot;
    const delText = root?.querySelector('pre[data-diff-type="split"] > [data-deletions] > [data-content]')?.getBoundingClientRect();
    const addText = root?.querySelector('pre[data-diff-type="split"] > [data-additions] > [data-content]')?.getBoundingClientRect();
    drag.current = { x: e.clientX, start: left, width: Math.max(1, (delText?.width ?? 0) + (addText?.width ?? 0)), pct: left };
    e.currentTarget.setAttribute('data-dragging', '');
    body.current?.setAttribute('data-dragging', '');
  };
  const onSashMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    d.pct = clampSplit(d.start + ((e.clientX - d.x) / d.width) * 100);
    paint(d.pct);
  };
  const onSashUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    e.preventDefault();
    drag.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    e.currentTarget.removeAttribute('data-dragging');
    body.current?.removeAttribute('data-dragging');
    setLeft(d.pct);
    measure();
  };
  // Setas esquerda e direita movem 2 pontos percentuais; a rolagem da tela fica parada.
  const onSashKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    setLeft(clampSplit(left + (e.key === 'ArrowRight' ? 2 : -2)));
    requestAnimationFrame(measure);
  };
  return (
    <Flexbox flex={1} className="min-h-0 compare-view">
      <div
        ref={body} tabIndex={0} aria-label="Arquivo inteiro: F8 vai à próxima mudança, Shift+F8 à anterior" className="compare-body"
      >
        <div ref={stage} className="compare-stage" style={{ '--compare-l': '50fr', '--compare-r': '50fr' } as CSSProperties}>
          {diff}
          {cur && <div className="compare-current" data-unit={current} style={{ top: cur[0], height: cur[1] - cur[0] }} />}
          {mode === 'split' && center && <div className="compare-center" style={cur ? { top: cur[0] + (cur[1] - cur[0]) / 2 } : undefined}>{center}</div>}
          {mode === 'split' && (
            <div
              ref={sash} role="separator" aria-orientation="vertical" aria-label="Divisor entre o original e o modificado"
              aria-valuemin={SPLIT_MIN} aria-valuemax={SPLIT_MAX} aria-valuenow={Math.round(left)} tabIndex={0} className="compare-sash"
              onPointerDown={onSashDown} onPointerMove={onSashMove} onPointerUp={onSashUp} onPointerCancel={onSashUp}
              onDoubleClick={() => setLeft(50)} onKeyDown={onSashKey}
            />
          )}
        </div>
      </div>
      <div className="compare-ruler">
        {ready && compare.units.map((unit, i) => {
          const s = ready.units[i];
          if (!s) return null;
          return (
            <button
              key={i} type="button" className={`compare-mark compare-mark-${unit.kind}`} data-current={i === current || undefined}
              style={{ top: frac(s[0]), height: `max(3px, ${frac(s[1] - s[0])})` }}
              aria-label={`Ir para ${unitName.toLowerCase()} ${i + 1}, linha ${unit.line}`} title={`${unitName} ${i + 1}, linha ${unit.line}`}
              onClick={() => onPick(i)}
            />
          );
        })}
      </div>
    </Flexbox>
  );
}

// F8 vai à próxima mudança, Shift+F8 à anterior. Vale em toda a aba, então o foco pode estar numa ação da toolbar.
const stepKey = (e: KeyboardEvent<HTMLElement>, step: (by: number) => void) => {
  if (e.key !== 'F8') return;
  e.preventDefault();
  step(e.shiftKey ? -1 : 1);
};

// `shown`: a aba é a atual do grupo. As abas escondidas ficam montadas e não consultam o Git; ao voltar, leem o diff de novo,
// porque a aba irmã pode ter mudado o índice (a revisão velha seria recusada pelo Git).
function DiffTab({ target, common, shown }: { target: Extract<TabTarget, { kind: 'diff' }>; common: Common; shown: boolean }) {
  const staged = target.side === 'staged';
  const [review, setReview] = useState<ReviewDiff | null>(null);
  const [error, setError] = useState('');
  const [mode, setMode] = useState<ViewMode>('split');
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  // Bloco atual pelo índice (os blocos são os do Git, com 3 linhas de contexto). O primeiro é o atual ao abrir; trocar de arquivo volta a ele.
  const [at, setAt] = useState(0);
  useEffect(() => setAt(0), [target.path, target.side]);
  const load = async () => {
    try {
      setReview(await backend.gitReviewDiff(target.path, staged));
      setError('');
    } catch (e) {
      setError(message(e));
    }
  };
  usePolling(load, POLL_MS, common.active && shown && `${target.path}|${target.side}`);

  // Uma operação por vez: o clique duplo não dispara duas (o ref não espera o re-render).
  const run = async (work: () => Promise<void>) => {
    if (busyRef.current || common.locked) return;
    if (!common.guard.enter()) return;
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

  // Não rastreado: o Git não separa em blocos. O comparador mostra o arquivo novo inteiro e a ação é preparar o arquivo.
  const untracked = review !== null && review.untracked;
  const hunks: PatchHunk[] = review && !untracked ? patchHunks(review.diff.patch) : [];
  // Só os campos que o comparador usa entram nas dependências: uma resposta igual à anterior (o poll) não refaz o arquivo nem a rolagem.
  const shownPatch = review ? review.display.patch : '';
  const shownBinary = review ? review.display.binary : false;
  const shownLarge = review ? review.display.tooLarge : false;
  const diffPatch = review ? review.diff.patch : '';
  const compare = useMemo(() => (
    shownPatch !== '' && !shownBinary && !shownLarge
      ? compareOf(shownPatch, !untracked && diffPatch !== '' ? blockRanges(diffPatch) : null)
      : null
  ), [shownPatch, shownBinary, shownLarge, untracked, diffPatch]);
  const blocked_ = common.locked || busy;
  const draftOpen = common.hasDraft(target.path);
  const n = hunks.length;
  // O bloco pode sumir (preparado ou descartado): o atual nunca passa do último.
  const cur = n === 0 ? -1 : Math.min(at, n - 1);
  const step = (by: number) => {
    if (n === 0) return;
    setAt(Math.min(Math.max(cur + by, 0), n - 1));
  };
  const block = cur >= 0 ? hunks[cur] : null;
  const head = review ? fileHead(review.diff.patch) : '';
  // Ações por bloco pelo índice: a barra age no bloco atual. O `HunkRef` sai do bloco visto agora.
  const refAt = (i: number): HunkRef | null => (hunks[i] && review ? { path: target.path, index: i, header: hunks[i].header, revision: review.revision } : null);
  const ref = cur >= 0 ? refAt(cur) : null;
  const stageAt = (i: number) => { const r = refAt(i); if (r) run(() => backend.gitStageHunk(r)); };
  const unstageAt = (i: number) => { const r = refAt(i); if (r) run(() => backend.gitUnstageHunk(r)); };
  const discardAt = async (i: number) => {
    const r = refAt(i);
    if (!r) return;
    // Confirma mostrando o patch capturado agora; o clique em confirmar usa este mesmo `r`, sem reler outro patch.
    const ok = await common.ask({
      title: 'Descartar este bloco?',
      body: <Text as="pre" fontSize={12} className="max-h-64 overflow-auto whitespace-pre">{`${head}\n${hunks[i].text}`}</Text>,
      actions: [{ key: 'ok', label: 'Descartar', danger: true }],
    });
    if (ok !== 'ok') return;
    if (common.hasDraft(target.path)) {
      toast.error({ title: 'Há um rascunho aberto deste arquivo. Salve ou descarte antes de descartar o bloco.', duration: 6000 });
      return;
    }
    await run(() => backend.gitDiscardHunk(r));
  };

  if (error && !review) return <Note text={error} danger />;
  if (!review) return <Note text="Lendo as alterações…" />;
  if (review.diff.binary) return <Note text="Arquivo binário: o diff não é mostrado." />;
  if (review.diff.tooLarge || review.display.tooLarge) return <Note text="Diff grande demais para mostrar aqui." />;
  if (review.diff.patch === '' || !compare) return <Note text="Não há alterações neste lado." />;

  const counter = untracked ? 'arquivo novo, não rastreado' : n === 0 ? (staged ? 'preparadas' : 'alterações') : `bloco ${cur + 1} de ${n}`;
  return (
    <Flexbox flex={1} className="min-h-0" onKeyDown={(e) => stepKey(e, step)}>
      <Flexbox horizontal align="center" gap={2} height={28} paddingInline={8} flex="none" className="border-b compare-toolbar">
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={target.path} className="min-w-0 flex-1">{target.path}</Text>
        <Text as="span" type="secondary" fontSize={11} className="shrink-0 whitespace-nowrap">{counter}</Text>
        {n > 0 && (
          <>
            <ActionIcon
              size="small" variant="borderless" icon={ChevronUp} aria-label="Bloco anterior" title="Bloco anterior (Shift+F8)"
              disabled={cur <= 0} onClick={() => step(-1)}
            />
            <ActionIcon
              size="small" variant="borderless" icon={ChevronDown} aria-label="Próximo bloco" title="Próximo bloco (F8)"
              disabled={cur >= n - 1} onClick={() => step(1)}
            />
          </>
        )}
        {ref && block && (
          <>
            <ActionIcon
              size="small" variant="borderless" icon={FileText} aria-label={`Abrir bloco ${cur + 1} de ${n} no arquivo`}
              title={`Abrir o bloco ${cur + 1} de ${n} no arquivo`} onClick={() => common.openLine(target.path, compare.units[cur].line)}
            />
            {staged
              ? <ActionIcon size="small" variant="borderless" icon={Minus} aria-label={`Tirar bloco ${cur + 1} de ${n} do preparo`} title={`Tirar o bloco ${cur + 1} de ${n} do preparo`} disabled={blocked_} onClick={() => unstageAt(cur)} />
              : <ActionIcon size="small" variant="borderless" icon={Plus} aria-label={`Preparar bloco ${cur + 1} de ${n}`} title={`Preparar o bloco ${cur + 1} de ${n}`} disabled={blocked_} onClick={() => stageAt(cur)} />}
            {!staged && (
              <ActionIcon
                size="small" variant="borderless" danger icon={ArrowRight} aria-label={`Descartar bloco ${cur + 1} de ${n}`}
                disabled={blocked_ || draftOpen}
                title={draftOpen ? 'Resolva o rascunho aberto deste arquivo antes de descartar.' : `Descartar o bloco ${cur + 1} de ${n}`}
                onClick={() => discardAt(cur)}
              />
            )}
          </>
        )}
        {untracked && !staged && <Button size="small" disabled={blocked_} onClick={() => run(() => backend.gitStage([target.path]))}>Preparar arquivo</Button>}
        {!isNewFile(review.display.patch) && <ModeToggle mode={mode} onChange={setMode} />}
        {!staged && (
          <ActionIcon
            size="small" variant="borderless" danger icon={Undo2} aria-label="Descartar arquivo"
            disabled={blocked_ || draftOpen}
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
          />
        )}
      </Flexbox>
      <Continuous key={`${target.path}|${target.side}`} patch={review.display.patch} compare={compare} mode={mode} current={cur} unitName="Bloco" onPick={setAt}
        center={ref && block && !untracked ? <div className="compare-center-actions">
          {!staged && <ActionIcon size="small" variant="borderless" danger icon={ArrowRight} aria-label={`Descartar bloco ${cur + 1} de ${n}`} title={`Descartar o bloco ${cur + 1} de ${n}`} disabled={blocked_ || draftOpen} onClick={() => discardAt(cur)} />}
          {staged
            ? <ActionIcon size="small" variant="borderless" icon={Minus} aria-label={`Tirar bloco ${cur + 1} de ${n} do preparo`} title={`Tirar o bloco ${cur + 1} de ${n} do preparo`} disabled={blocked_} onClick={() => unstageAt(cur)} />
            : <ActionIcon size="small" variant="borderless" icon={Plus} aria-label={`Preparar bloco ${cur + 1} de ${n}`} title={`Preparar o bloco ${cur + 1} de ${n}`} disabled={blocked_} onClick={() => stageAt(cur)} />}
        </div> : undefined}
      />
    </Flexbox>
  );
}

// ---- Histórico (commit): leitura imutável, sem botões de mutação ----

// O commit mostra o arquivo inteiro. Sem blocos do Git, a unidade é cada trecho de mudança; a navegação é só de leitura.
function CommitTab({ target }: { target: Extract<TabTarget, { kind: 'commit' }> }) {
  const [diff, setDiff] = useState<GitDiff | null>(null);
  const [error, setError] = useState('');
  const [mode, setMode] = useState<ViewMode>('split');
  const [at, setAt] = useState(0);
  useEffect(() => {
    if (diff) return; // histórico é imutável: lê uma vez
    backend.gitCommitDiff(target.hash, target.path).then(setDiff, (e) => setError(message(e)));
  }, [target.hash, target.path]);
  const compare = useMemo(() => (diff && diff.patch !== '' && !diff.binary && !diff.tooLarge ? compareOf(diff.patch, null) : null), [diff]);
  if (error) return <Note text={error} danger />;
  if (!diff) return <Note text="Lendo o commit…" />;
  if (diff.binary) return <Note text="Arquivo binário: o diff não é mostrado." />;
  if (diff.tooLarge) return <Note text="Diff grande demais para mostrar aqui." />;
  if (!compare) return <Note text="Este commit não altera o texto deste arquivo." />;
  const n = compare.units.length;
  const cur = n === 0 ? -1 : Math.min(at, n - 1);
  const step = (by: number) => {
    if (n === 0) return;
    setAt(Math.min(Math.max(cur + by, 0), n - 1));
  };
  return (
    <Flexbox flex={1} className="min-h-0" onKeyDown={(e) => stepKey(e, step)}>
      <Flexbox horizontal align="center" gap={6} height={28} paddingInline={8} flex="none" className="border-b compare-toolbar">
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={target.path} className="min-w-0 flex-1">{target.path}</Text>
        <Text as="span" type="secondary" fontSize={11} className="shrink-0 whitespace-nowrap">commit {target.hash.slice(0, 7)} · somente leitura</Text>
        {n > 0 && (
          <>
            <Text as="span" type="secondary" fontSize={11} className="shrink-0 whitespace-nowrap">{`mudança ${cur + 1} de ${n}`}</Text>
            <ActionIcon size="small" variant="borderless" icon={ChevronUp} aria-label="Mudança anterior" title="Mudança anterior (Shift+F8)" disabled={cur <= 0} onClick={() => step(-1)} />
            <ActionIcon size="small" variant="borderless" icon={ChevronDown} aria-label="Próxima mudança" title="Próxima mudança (F8)" disabled={cur >= n - 1} onClick={() => step(1)} />
          </>
        )}
        {!isNewFile(diff.patch) && <ModeToggle mode={mode} onChange={setMode} />}
      </Flexbox>
      <Continuous key={`${target.hash}|${target.path}`} patch={diff.patch} compare={compare} mode={mode} current={cur} unitName="Mudança" onPick={setAt} />
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
    if (!shown) return;
    void load();
  }, [shown, path]);

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

  if (error && !conflict) return <Note text={error} danger />;
  if (!conflict || !draft) return <Note text="Lendo o conflito…" />;
  if (conflict.binary) return <Note text="Conflito em arquivo binário: escolha um lado pelo Git; não há texto para resolver aqui." />;

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
      <Flexbox flex={1} className="min-h-0">
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
