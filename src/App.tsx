import { useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState, type ReactNode } from 'react';
import { Flexbox } from '@lobehub/ui';
import { Alert, Button, DraggablePanel, Modal, Progress, setActiveScope, Text, toast } from '@lobehub/ui/base-ui';
import { backend, connect, host, message, projectBackend } from './backend/backend';
import { BackendProvider } from './backend/backend-context';
import { ChoiceForm } from './terminal/choice-form';
import { EditorWorkspace, type EditorWorkspaceHandle } from './editor/editor-workspace';
import { FilesScreen, type FileActions, type OpenOptions } from './explorer/explorer';
import { Nav, type Route } from './window/nav';
import { StatusBar } from './window/status-bar';
import { fileName } from './shared/file-tabs';
import { createGitGuard } from './git/git-guard';
import { GitSidebar, usePolled } from './git/git';
import { type HistoryRequest } from './git/git-history';
import { copy, join, splitPath } from './shared/paths';
import { QuickOpen } from './editor/quick-open';
import { SearchScreen } from './search/search';
import { SHELL_HEIGHT, SHELL_MIN, ShellPanel } from './terminal/shell-panel';
import { type ExecutorTab, FINISHED, PANEL_MIN, PANEL_WIDTH, STATES, TerminalsScreen } from './terminal/terminal-screen';
import type { TermView } from './terminal/term';
import { terminals } from './terminal/terminals';
import { folderName, TitleBar, type ProjectTab, type TitleBarMenu } from './window/titlebar';
import { SECONDARY } from './shared/text-style';
import { pathText, relativeTo } from './shared/file-tab-actions';
import { OpenProjectPrompt } from './shared/open-project';
import { isWithin, type TabTarget } from './editor/workspace-model';
import {
  ORCHESTRATOR, type Choice, type ExecutorNode, type GitState, type NodeState, type Options, type ProjectSnapshot, type UpdateStatus,
} from './backend/types';

const MB = 1024 * 1024;
// Frase do aviso da janela do Orquestrador, por modo do Git que não é `repo`: diz o que falta para o registro existir.
const NO_GIT_FIX: Record<Exclude<GitState['kind'], 'repo'>, string> = {
  'no-repo': 'Para ter isso, inicialize o repositório no painel Git.',
  'no-git': 'Para ter isso, instale o Git e abra o app de novo.',
  'repo-above': 'Para ter isso, abra a raiz do repositório como projeto.',
};
// Larguras da linha principal: o Nav, o mínimo que cada grupo do editor mantém (vezes o número de grupos), e a faixa da árvore e da pesquisa (a inicial, que só vale sem layout guardado: a largura que o usuário dá fica guardada por projeto, no layout que a recuperação do editor leva; e a mínima).
const NAV_WIDTH = 44;
const CENTER_MIN_WIDTH = 240;
const SIDE_WIDTH = 280;
const SIDE_MIN = 200;
// Limites do layout guardado (o JSON que a recuperação do editor leva junto): valor fora deles é corrigido na leitura.
const SIDE_MAX_SAVED = 960;
const PANEL_MAX_SAVED = 960;
const SHELL_MAX_SAVED = 1200;
// Objeto constante: o duplo clique na alça volta para a largura inicial (o DraggablePanel usa `defaultSize` nesse reset).
const SIDE_DEFAULT_SIZE = { width: SIDE_WIDTH };
// Altura que a área do arquivo mantém quando o painel Terminal está aberto no máximo.
const FILE_MIN_HEIGHT = 160;
const LAST_PROJECT_KEY = 'orquestrador.project.last';
const RECENT_PROJECTS_KEY = 'orquestrador.project.recent';
const OPEN_PROJECTS_KEY = 'orquestrador.project.tabs';
const ACTIVE_PROJECT_KEY = 'orquestrador.project.active';
const projectOpeners = new Map<string, (path: string) => void>();
const pendingProjectFiles = new Map<string, string[]>();
let looseFileOpener: ((path: string) => void) | null = null;
export let openAbsolutePath = async (_path: string) => {};
let windowCloseRequest: (() => Promise<boolean>) | null = null;
// Os editores montados, por projeto ('' é o modo sem projeto): o fechamento da janela consulta todos, não só o ativo.
const editorHandles = new Map<string, { name: string; editor: React.RefObject<EditorWorkspaceHandle | null> }>();
type PendingEditor = { editor: EditorWorkspaceHandle; prefix: string };
const unsavedOf = (list: PendingEditor[]) => list.flatMap(({ editor, prefix }) => editor.unsaved().map((path) => prefix + path));
// O editor de cada projeto montado; os de outro projeto levam o nome da pasta na frente dos arquivos.
const mountedEditors = (current: string): PendingEditor[] => [...editorHandles].flatMap(([id, { name, editor }]) =>
  editor.current ? [{ editor: editor.current, prefix: id === current ? '' : `${name}: ` }] : []);

function readRecentProjects(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(RECENT_PROJECTS_KEY) ?? '[]');
    return Array.isArray(value) ? value.filter((path): path is string => typeof path === 'string').slice(0, 10) : [];
  } catch {
    return [];
  }
}

type Overlay = 'start' | 'executor' | null;
// Um shell do painel Terminal: `program` nasce com o último que o backend devolveu (ou '' no primeiro) e é confirmado quando ele responde.
// `cwd` é a pasta (relativa ao projeto) em que o shell abre; sem ela, a raiz do projeto.
// `n` é o número exibido: o menor livre entre os shells abertos ao criar. O id segue único, para respostas atrasadas do backend não acharem outra aba.
type ShellItem = { id: string; n: number; program: string; cwd?: string; project: string | null };
type State = {
  update: UpdateStatus;
  projectId: string | null;
  project: string;
  projectBusy: boolean;
  options: Options | null;
  orchestrator: { choice: Choice | null; overlay: Overlay } | null;
  executorChoice: Choice | null;
  executors: ExecutorTab[];
  notice: string;
  shells: ShellItem[];
  shellActive: string;
  shellOpen: boolean;
};
type Action =
  | { type: 'update'; status: UpdateStatus }
  | { type: 'projectBusy'; busy: boolean }
  | { type: 'options'; options: Options }
  | { type: 'open'; options: Options }
  | { type: 'started'; choice: Choice }
  | { type: 'overlay'; overlay: Overlay }
  | { type: 'executorChoice'; choice: Choice }
  | { type: 'nodeAdded'; node: ExecutorNode }
  | { type: 'nodeState'; id: string; state: NodeState; choice: Choice | null }
  | { type: 'removed'; ids: string[] }
  | { type: 'reset'; notice: string }
  | { type: 'shellAdd'; id: string; program: string; cwd?: string; project: string | null }
  | { type: 'shellProgram'; id: string; program: string }
  | { type: 'shellRemove'; id: string }
  | { type: 'shellSelect'; id: string }
  | { type: 'shellMove'; id: string; index: number }
  | { type: 'shellOpen'; open: boolean };

const initial: State = {
  update: { phase: 'checking' }, projectId: null, project: '', projectBusy: false, options: null, orchestrator: null,
  executorChoice: null, executors: [], notice: '', shells: [], shellActive: '', shellOpen: false,
};

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'update':
      return { ...state, update: action.status };
    case 'projectBusy':
      return { ...state, projectBusy: action.busy };
    case 'options':
      return { ...state, options: action.options };
    case 'open':
      return {
        ...state, options: action.options, notice: '',
        orchestrator: { choice: null, overlay: 'start' },
      };
    case 'started':
      return { ...state, orchestrator: state.orchestrator && { ...state.orchestrator, choice: action.choice, overlay: null } };
    case 'overlay':
      return { ...state, orchestrator: state.orchestrator && { ...state.orchestrator, overlay: action.overlay } };
    case 'executorChoice':
      return { ...state, executorChoice: action.choice };
    case 'nodeAdded': {
      if (!state.orchestrator || state.executors.some((e) => e.id === action.node.id)) return state;
      let n = 1;
      while (state.executors.some((e) => e.n === n)) n += 1;
      return { ...state, executors: [...state.executors, { ...action.node, n }] };
    }
    case 'nodeState':
      return {
        ...state,
        executors: state.executors.map((e) => (e.id === action.id ? { ...e, state: action.state, choice: action.choice } : e)),
      };
    case 'removed':
      return { ...state, executors: state.executors.filter((e) => !action.ids.includes(e.id)) };
    case 'reset':
      return { ...state, orchestrator: null, executorChoice: null, executors: [], notice: action.notice };
    case 'shellAdd': {
      let n = 1;
      while (state.shells.some((s) => s.n === n)) n += 1;
      return { ...state, shells: [...state.shells, { id: action.id, n, program: action.program, cwd: action.cwd, project: action.project }], shellActive: action.id, shellOpen: true };
    }
    case 'shellProgram':
      return { ...state, shells: state.shells.map((s) => (s.id === action.id ? { ...s, program: action.program } : s)) };
    case 'shellRemove': {
      const index = state.shells.findIndex((s) => s.id === action.id);
      if (index < 0) return state;
      const shells = state.shells.filter((s) => s.id !== action.id);
      // Sem aba nenhuma o painel fecha; a próxima abertura cria um shell novo.
      if (shells.length === 0) return { ...state, shells, shellActive: '', shellOpen: false };
      // A aba ativa que sai passa para a vizinha: a da direita, ou a da esquerda se era a última.
      const shellActive = state.shellActive === action.id ? shells[index]?.id ?? shells[index - 1].id : state.shellActive;
      return { ...state, shells, shellActive };
    }
    case 'shellSelect':
      return { ...state, shellActive: action.id };
    case 'shellMove': {
      // `index` é a posição entre as outras abas, já sem a que se move.
      const shell = state.shells.find((s) => s.id === action.id);
      if (!shell) return state;
      const shells = state.shells.filter((s) => s.id !== action.id);
      shells.splice(action.index, 0, shell);
      return { ...state, shells };
    }
    case 'shellOpen':
      return { ...state, shellOpen: action.open };
  }
}

const updateText = (status: UpdateStatus, fatal: boolean) => {
  if (fatal) return 'Não foi possível iniciar o app.';
  switch (status.phase) {
    case 'checking':
      return 'Verificando se há uma versão nova…';
    case 'downloading': {
      const { version, downloaded, total } = status;
      const size = total ? `${Math.round((downloaded / total) * 100)}%` : `${(downloaded / MB).toLocaleString('pt-BR', { maximumFractionDigits: 1 })} MB`;
      return `Baixando a versão ${version}… ${size}`;
    }
    case 'installing':
      return `Instalando a versão ${status.version}. O app reabre sozinho.`;
    case 'failed':
      return 'Não foi possível atualizar. O uso fica bloqueado até a atualização dar certo.';
    case 'ready':
      return '';
  }
};

// Ação destrutiva do diálogo de fechar: texto vermelho com borda vermelha escura, mais leve que a borda `colorError` do `danger`.
const DANGER_SOFT = { borderColor: 'var(--ant-color-error-border)' };
const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });

// Layout da janela guardado por projeto (texto opaco na recuperação do editor). Lê com desconfiança: o que não for número vira o padrão,
// e o número é limitado às faixas aceitas. Texto inválido devolve null, e nada muda.
type Layout = { sideWidth: number; panelWidth: number; shellHeight: number };
function readLayout(text: string): Partial<Layout> | null {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof raw !== 'object' || raw === null) return null;
  const pick = (value: unknown, min: number, max: number) => (typeof value === 'number' && Number.isFinite(value)
    ? Math.min(max, Math.max(min, Math.round(value)))
    : undefined);
  return {
    sideWidth: pick((raw as Layout).sideWidth, SIDE_MIN, SIDE_MAX_SAVED),
    panelWidth: pick((raw as Layout).panelWidth, PANEL_MIN, PANEL_MAX_SAVED),
    shellHeight: pick((raw as Layout).shellHeight, SHELL_MIN, SHELL_MAX_SAVED),
  };
}

// Largura ou altura de um elemento, medida com ResizeObserver. O elemento entra por callback ref: ele só existe em algumas telas,
// então a medida segue o elemento que está montado agora, e não o primeiro que apareceu.
function useSize(axis: 'width' | 'height') {
  const [el, setEl] = useState<HTMLElement | null>(null);
  const [size, setSize] = useState(0);
  useLayoutEffect(() => {
    if (!el) return;
    const measure = () => setSize(axis === 'width' ? el.clientWidth : el.clientHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [el, axis]);
  return [setEl, size] as const;
}

// Resposta do fechamento: cancelar, descartar as alterações, salvar tudo, ou (só com terminais) encerrar mesmo assim.
type CloseChoice = 'cancel' | 'discard' | 'save' | 'close';
type CloseAsk = { files: string[]; terminals: boolean; projectWork?: ProjectSnapshot };

type ManagedProject = { id: string; project: string };

const hasWork = (work: ProjectSnapshot) => work.orchestratorOpen || work.executorsRunning > 0 || work.executorsQueued > 0;

function readOpenProjects(): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(OPEN_PROJECTS_KEY) ?? 'null');
    if (Array.isArray(raw)) return raw.filter((path): path is string => typeof path === 'string');
    const last = localStorage.getItem(LAST_PROJECT_KEY);
    return last ? [last] : [];
  } catch { return []; }
}

function readActiveProject(): string | null {
  try {
    return localStorage.getItem(ACTIVE_PROJECT_KEY) ?? localStorage.getItem(LAST_PROJECT_KEY);
  } catch { /* Sem preferência ativa, usa a primeira pasta. */ return null; }
}

export function App() {
  const [projects, setProjects] = useState<ManagedProject[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [mounted, setMounted] = useState<string[]>([]);
  const [busyByProject, setBusyByProject] = useState<Record<string, number>>({});
  const [recentProjects, setRecentProjects] = useState(readRecentProjects);
  const [ready, setReady] = useState(false);
  // Etapa da abertura, mostrada até os projetos salvos voltarem.
  const [stage, setStage] = useState('Conectando ao backend…');
  const projectsRef = useRef(projects);
  const activeRef = useRef(active);
  useLayoutEffect(() => {
    projectsRef.current = projects;
    activeRef.current = active;
  });

  const persist = (next: ManagedProject[], activeId: string | null) => {
    setProjects(next);
    setActive(activeId);
    if (activeId) setMounted((ids) => ids.includes(activeId) ? ids : [...ids, activeId]);
    const activePath = next.find((item) => item.id === activeId)?.project;
    try {
      localStorage.setItem(OPEN_PROJECTS_KEY, JSON.stringify(next.map((item) => item.project)));
      if (activePath) {
        localStorage.setItem(ACTIVE_PROJECT_KEY, activePath);
        localStorage.setItem(LAST_PROJECT_KEY, activePath);
      } else {
        localStorage.removeItem(ACTIVE_PROJECT_KEY);
        localStorage.removeItem(LAST_PROJECT_KEY);
      }
    } catch { /* A preferência local não impede abrir a pasta. */ }
  };
  // Põe a pasta no topo dos recentes e como a última aberta.
  const rememberProject = (project: string) => {
    const recents = [project, ...readRecentProjects().filter((item) => item.toLowerCase() !== project.toLowerCase())].slice(0, 10);
    try {
      localStorage.setItem(RECENT_PROJECTS_KEY, JSON.stringify(recents));
      localStorage.setItem(LAST_PROJECT_KEY, project);
    } catch { /* A preferência local não impede abrir a pasta. */ }
    setRecentProjects(recents);
  };
  const openNewProject = async (path?: string) => {
    await connect();
    const opened = path ? await backend.openProject(path) : await backend.chooseProject();
    if (!opened) return;
    const current = projectsRef.current;
    const found = current.find((item) => item.id === opened.id);
    const next = found ? current : [...current, { id: opened.id, project: opened.project }];
    rememberProject(opened.project);
    persist(next, opened.id);
    if (opened.subfolder) toast.info({ title: `Esta pasta está dentro do repositório ${opened.project}. O projeto aberto é a raiz dele.`, duration: 6000 });
  };
  const addProject = (path?: string) => openNewProject(path).catch((e: unknown) => {
    if (path && /não existe|não é uma pasta/i.test(message(e))) {
      const next = readRecentProjects().filter((item) => item.toLowerCase() !== path.toLowerCase());
      try { localStorage.setItem(RECENT_PROJECTS_KEY, JSON.stringify(next)); } catch { /* A preferência local é opcional. */ }
      setRecentProjects(next);
      toast.warning({ title: `Projeto removido dos recentes: ${path}`, duration: 6000 });
    } else report(e);
  });
  const dropManagedProject = async (id: string) => {
    await backend.closeProject(id);
    const next = projectsRef.current.filter((item) => item.id !== id);
    setMounted((ids) => ids.filter((item) => item !== id));
    const nextActive = activeRef.current === id ? next[Math.max(0, projectsRef.current.findIndex((item) => item.id === id) - 1)]?.id ?? next[0]?.id ?? null : activeRef.current;
    persist(next, nextActive);
    setBusyByProject((items) => { const rest = { ...items }; delete rest[id]; return rest; });
  };
  const moveProject = (id: string, target: number) => {
    const next = [...projectsRef.current];
    const from = next.findIndex((item) => item.id === id);
    if (from < 0) return;
    const [item] = next.splice(from, 1);
    next.splice(target, 0, item);
    persist(next, activeRef.current);
  };
  const selectProject = (id: string) => persist(projectsRef.current, id);

  async function openAbsolute(path: string) {
    const opened = await backend.openProject(path).catch(() => null);
    if (opened) {
      const current = projectsRef.current;
      const next = current.some((item) => item.id === opened.id) ? current : [...current, { id: opened.id, project: opened.project }];
      rememberProject(opened.project);
      persist(next, opened.id);
      return;
    }
    // Arquivo: procurar primeiro uma pasta já aberta.
    const containing = projectsRef.current.find((item) => relativeTo(item.project, path.replace(/\\/g, '/')) !== null);
    const target = containing ?? projectsRef.current.find((item) => item.id === activeRef.current);
    if (!target) { looseFileOpener?.(path); return; }
    persist(projectsRef.current, target.id);
    const open = projectOpeners.get(target.id);
    if (open) open(path);
    else pendingProjectFiles.set(target.id, [...(pendingProjectFiles.get(target.id) ?? []), path]);
  }
  useLayoutEffect(() => {
    openAbsolutePath = openAbsolute;
  });

  useEffect(() => {
    let live = true;
    void (async () => {
      await connect();
      await host().onCloseRequested(async () => windowCloseRequest ? windowCloseRequest() : true);
      const restored: ManagedProject[] = [];
      const params = new URLSearchParams(window.location.search);
      const mockCount = Number(params.get('projects'));
      const saved = params.get('project') === 'none' ? [] : mockCount > 1
        ? ['C:\\projetos\\exemplo', ...Array.from({ length: Math.min(8, mockCount) - 1 }, (_, i) => `C:\\projetos\\simulado-${i + 2}`)]
        : readOpenProjects();
      for (const [index, path] of saved.entries()) {
        if (live) setStage(`Reabrindo ${folderName(path)}${saved.length > 1 ? ` (${index + 1} de ${saved.length})` : ''}…`);
        try {
          const opened = await backend.openProject(path);
          if (!restored.some((item) => item.id === opened.id)) restored.push({ id: opened.id, project: opened.project });
        } catch { /* Pastas removidas não são reabertas. */ }
      }
      const selectedPath = readActiveProject();
      const selected = restored.find((item) => item.project.toLowerCase() === selectedPath?.toLowerCase()) ?? restored[0];
      if (live) { persist(restored, selected?.id ?? null); setReady(true); }
    })().catch((e) => { report(e); if (live) setReady(true); });
    return () => { live = false; };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      for (const item of projectsRef.current) void backend.projectSnapshot(item.id).then((snapshot) => {
        const busy = snapshot.executorsRunning + snapshot.executorsQueued;
        setBusyByProject((current) => (current[item.id] === busy ? current : { ...current, [item.id]: busy }));
      }).catch(() => {});
    }, 1500);
    return () => window.clearInterval(timer);
  }, []);

  if (!ready) return <Screen connected={false} description={(
    <Text as="p" fontSize={14} style={SECONDARY} role="status" aria-live="polite">{stage}</Text>
  )}>{null}</Screen>;
  const tabs: ProjectTab[] = projects.map((item) => ({ id: item.id, path: item.project, busy: busyByProject[item.id] }));
  if (projects.length === 0) return <ProjectApp projects={tabs} onSelect={selectProject} onClose={dropManagedProject} onMove={moveProject} onOpen={addProject} recentProjects={recentProjects} />;
  return <div className="relative h-dvh w-full overflow-hidden">
    {projects.map((item) => <div key={item.id} className={`absolute inset-0 ${active === item.id ? '' : 'invisible'}`}>
      {mounted.includes(item.id) && <ProjectApp initialProject={item} active={active === item.id} projects={tabs} onSelect={selectProject} onClose={dropManagedProject} onMove={moveProject} onOpen={addProject} recentProjects={recentProjects} />}
    </div>)}
  </div>;
}

// A versão é conferida uma vez por abertura do app: um projeto aberto depois (o "+" das abas) nasce pronto e conectado, sem a tela de atualização.
let updateChecked = false;
// Programa do último shell aberto (vale para todos os projetos): a aba nova já nasce com ele e não muda de largura quando o backend responde.
let lastShellProgram = '';

// `onClose` fecha o projeto no backend e tira a aba; rejeita com o erro, e nada muda.
function ProjectApp({ initialProject, active = true, projects, onSelect, onClose, onMove, onOpen, recentProjects }: {
  initialProject?: ManagedProject; active?: boolean; projects?: ProjectTab[]; onSelect?: (id: string) => void;
  onClose: (id: string) => Promise<void>; onMove?: (id: string, index: number) => void; onOpen: (path?: string) => void; recentProjects: string[];
}) {
  const [state, dispatch] = useReducer(reducer, initial, () => {
    const base: State = updateChecked ? { ...initial, update: { phase: 'ready' } } : initial;
    return initialProject ? { ...base, projectId: initialProject.id, project: initialProject.project } : base;
  });
  const backend = projectBackend(state.projectId);
  const stateRef = useRef(state);
  useLayoutEffect(() => {
    stateRef.current = state;
  });
  const [route, setRoute] = useState<Route>('arquivos');
  // Sem projeto a faixa nasce fechada, como no VS Code; o ícone da tela abre e, clicado de novo, fecha. Com projeto ela fica sempre aberta.
  const [sideOpen, setSideOpen] = useState(false);
  // Área do editor central (grupos, abas, rascunhos e recuperação ficam nele). O App só pede coisas por este handle.
  const editor = useRef<EditorWorkspaceHandle>(null);
  useEffect(() => {
    const key = initialProject?.id ?? '';
    editorHandles.set(key, { name: splitPath((initialProject?.project ?? '').replace(/\\/g, '/')).name, editor });
    return () => { if (editorHandles.get(key)?.editor === editor) editorHandles.delete(key); };
  }, [initialProject?.id]);
  // Trava das operações Git da UI: a barra lateral e o editor dividem a mesma.
  const [gitGuard] = useState(createGitGuard);
  // Sincronizar da barra de status em curso: o ícone gira e o botão fica parado.
  const [syncing, setSyncing] = useState(false);
  // Alvo da aba ativa do editor: a barra Git marca o diff (ou conflito) que está aberto.
  const [activeTarget, setActiveTarget] = useState<TabTarget | null>(null);
  // Caminho com a árvore revelada (o Explorer abre as pastas e marca o arquivo). Um objeto novo a cada pedido.
  const [treeReveal, setTreeReveal] = useState<{ path: string } | null>(null);
  // Pedido de histórico Git (arquivo ou projeto). Um pedido novo sempre refaz a lista; sem pedido, a barra Git abre nas alterações.
  const [historyRequest, setHistoryRequest] = useState<HistoryRequest>();
  // Pesquisa com pasta: vazio é o projeto inteiro; um pedido novo refaz o escopo mesmo se a pasta é a mesma.
  const [scope, setScope] = useState<{ path: string } | null>(null);
  // Sobe a cada Ctrl+Shift+F: a pesquisa refoca o campo mesmo se já estiver na tela.
  const [searchFocus, setSearchFocus] = useState(0);
  // Seleção do editor que vira a consulta da pesquisa (Ctrl+Shift+F com texto selecionado). Um objeto novo a cada pedido.
  const [seed, setSeed] = useState<{ text: string } | null>(null);
  // Sobe a cada gravação do editor: a pesquisa refaz a busca atual para não mostrar linhas velhas.
  const [saveTick, setSaveTick] = useState(0);
  // F4 e Shift+F4: próximo ou anterior resultado da pesquisa. Um objeto novo a cada tecla.
  const [step, setStep] = useState<{ dir: 1 | -1 } | null>(null);
  const [quickOpen, setQuickOpen] = useState(false);
  // O status do Git roda com o projeto aberto, em qualquer tela: o contador da navegação precisa dele.
  // Status do Git, atualizado a cada 1,5 s. O estado só muda quando a resposta é diferente, para a lista não piscar. Sem projeto (`enabled` falso), não consulta.
  const git = usePolled(() => backend.gitStatus(), active && state.update.phase === 'ready' && !!state.project);
  const [connected, setConnected] = useState(updateChecked);
  const [fatal, setFatal] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const projectChordTimer = useRef(0);
  const projectChordPending = useRef(false);
  // Fechamento da janela: alterações não salvas (salvar, descartar ou cancelar) e terminais trabalhando (encerrar ou cancelar).
  const [closeAsk, setCloseAsk] = useState<CloseAsk | null>(null);
  const closeAnswer = useRef<((choice: CloseChoice) => void) | null>(null);
  // Salvar em curso: o diálogo segue aberto, com o botão carregando, até o saveAll terminar.
  const [closeSaving, setCloseSaving] = useState(false);
  const updateEvents = useRef(0);
  // Contador dos shells do painel Terminal: só cresce, então um id nunca se repete enquanto o app está aberto.
  const shellSeq = useRef(0);
  // Largura da janela (a faixa e o painel do Orquestrador cabem nela) e altura da coluna central (o painel Terminal não come a área do arquivo).
  const [rowRef, rowWidth] = useSize('width');
  const [centerRef, centerHeight] = useSize('height');
  // Larguras e altura que o usuário deu à faixa, ao painel do Orquestrador e ao Terminal. As exibidas são elas limitadas ao que cabe agora.
  const [sideWidth, setSideWidth] = useState(SIDE_WIDTH);
  const [panelWidth, setPanelWidth] = useState(PANEL_WIDTH);
  const [shellHeight, setShellHeight] = useState(SHELL_HEIGHT);
  // Grupos do editor (1 ou 2): cada um precisa da própria largura mínima dentro da coluna central.
  const [groupCount, setGroupCount] = useState(1);
  // O que a faixa e o painel dividem: a janela menos o Nav e o mínimo da coluna central (um mínimo por grupo). Indefinido enquanto a janela não foi medida: os dois nascem na largura inicial, sem limite e sem animação.
  const sharedWidth = rowWidth ? rowWidth - NAV_WIDTH - CENTER_MIN_WIDTH * groupCount : undefined;
  // Janela estreita: o painel cede primeiro, até a mínima dele, e só depois a faixa. Por isso a faixa conta o painel na mínima, e o painel conta a faixa já exibida.
  const shownSideWidth = sharedWidth === undefined
    ? sideWidth
    : Math.min(sideWidth, Math.max(SIDE_MIN, sharedWidth - PANEL_MIN));
  const panelMaxWidth = sharedWidth === undefined
    ? undefined
    : Math.max(PANEL_MIN, sharedWidth - shownSideWidth);
  const shownPanelWidth = Math.min(panelWidth, panelMaxWidth ?? Infinity);
  // No arrasto, cada borda para quando a coluna central chega ao mínimo: o máximo de um conta a largura exibida do outro, e nenhum empurra o outro.
  const sideMaxWidth = sharedWidth === undefined
    ? undefined
    : Math.max(SIDE_MIN, sharedWidth - shownPanelWidth);
  // Layout que vai para a recuperação do editor (com o projeto). Mudou, o editor grava de novo.
  const layoutText = useMemo(() => JSON.stringify({ sideWidth, panelWidth, shellHeight }), [sideWidth, panelWidth, shellHeight]);


  function openPath(path: string) {
    const absolute = path.replace(/\\/g, '/');
    const relative = stateRef.current.project ? relativeTo(stateRef.current.project, absolute) : null;
    editor.current?.openFile(relative ?? absolute, { permanent: true });
    setRoute('arquivos');
  }

  useEffect(() => {
    if (initialProject) {
      projectOpeners.set(initialProject.id, openPath);
      for (const path of pendingProjectFiles.get(initialProject.id) ?? []) openPath(path);
      pendingProjectFiles.delete(initialProject.id);
      return () => { projectOpeners.delete(initialProject.id); };
    }
    looseFileOpener = openPath;
    return () => { if (looseFileOpener === openPath) looseFileOpener = null; };
  }, [initialProject?.id]);

  useEffect(() => {
    if (!active) return;
    let live = true;
    const drain = async () => {
      if (!live || stateRef.current.update.phase !== 'ready') return;
      for (const path of await backend.takeOpenPaths()) {
        await openAbsolutePath(path);
      }
    };
    const off = host().listen('open-paths', () => { void drain(); });
    void off.then(drain);
    return () => { live = false; void off.then((unlisten) => unlisten()); };
  }, [active, state.update.phase]);

  useEffect(() => {
    if (!initialProject) return;
    let live = true;
    void backend.getOptions().then((options) => {
      if (live) dispatch({ type: 'options', options });
    }).catch(report);
    return () => { live = false; };
  }, [initialProject?.id]);

  // ---------- ações ----------

  const resetOrchestrator = (notice = '') => {
    for (const e of stateRef.current.executors) terminals.forget(e.id, stateRef.current.projectId);
    dispatch({ type: 'reset', notice });
  };

  // Com uma confirmação já na tela, o pedido novo não sobrescreve a resposta pendente: recusa, e a primeira continua valendo.
  const askClose = (files: string[] = [], terminalsRunning = false, projectWork?: ProjectSnapshot) => {
    if (closeAnswer.current) return Promise.resolve<CloseChoice>('cancel');
    return new Promise<CloseChoice>((resolve) => {
      closeAnswer.current = resolve;
      setCloseAsk({ files, terminals: terminalsRunning, projectWork });
    });
  };
  // Com o salvar em curso, nenhuma resposta vale (nem Esc ou Cancelar): quem fecha o diálogo é o fluxo de fechamento.
  const settleClose = (choice: CloseChoice) => {
    if (closeSaving) return;
    if (choice === 'save') setCloseSaving(true);
    else setCloseAsk(null);
    closeAnswer.current?.(choice);
    closeAnswer.current = null;
  };

  // Salvar, descartar ou cancelar as alterações não salvas de `list`. `true` libera o fechamento, com a recuperação já gravada.
  const settleUnsaved = async (list: PendingEditor[], running: boolean, projectWork?: ProjectSnapshot) => {
    let choice = await askClose(unsavedOf(list), running, projectWork);
    while (choice === 'save') {
      const saved = (await Promise.all(list.map(({ editor }) => editor.saveAll()))).every(Boolean);
      const left = unsavedOf(list);
      if (saved && left.length === 0) break;
      report(new Error('Não foi possível salvar tudo. Nada foi fechado.'));
      setCloseSaving(false);
      if (left.length === 0) { setCloseAsk(null); return false; }
      choice = await askClose(left, running, projectWork);
    }
    if (choice === 'cancel') return false;
    if (choice === 'discard') for (const { editor } of list) editor.discardAll();
    setCloseSaving(false);
    setCloseAsk(null);
    for (const { editor } of list) editor.flushRecovery();
    return true;
  };
  // O editor de um projeto montado, sozinho: o fechamento da aba só pergunta pelos arquivos dele.
  const editorOf = (id: string): PendingEditor[] => {
    const handle = editorHandles.get(id)?.editor.current;
    return handle ? [{ editor: handle, prefix: '' }] : [];
  };

  useEffect(() => {
    if (!active) return;
    const mayClose = async () => {
      const list = mountedEditors(initialProject?.id ?? '');
      try {
        const running = await backend.hasRunning();
        if (unsavedOf(list).length > 0) return await settleUnsaved(list, running);
        if (running) {
          const choice = await askClose([], true);
          if (choice !== 'close') return false;
        }
      } catch { /* Sem resposta do backend, deixa a janela fechar. */ }
      for (const { editor } of list) editor.flushRecovery();
      return true;
    };
    windowCloseRequest = mayClose;
    return () => { if (windowCloseRequest === mayClose) windowCloseRequest = null; };
  }, [active]);

  // Copia o caminho (ou os caminhos), um por linha. Lista vazia não copia nada.
  const copyPaths = (kind: 'relative' | 'absolute', paths: string[]) => {
    const texts = paths.map((p) => pathText(kind, p, stateRef.current.project)).filter((t): t is string => t !== null);
    if (texts.length === 0) return;
    void copy(texts.join('\n'), texts.length === 1 ? 'Caminho copiado' : `${texts.length} caminhos copiados`);
  };
  // Destino que já tem aba (ou rascunho) aberta: mover ou colar ali criaria dois documentos no mesmo caminho. Recusa antes de mexer no disco.
  const clashAt = (targets: string[]) => {
    const open = editor.current?.openPaths() ?? [];
    const hit = targets.find((t) => open.some((p) => isWithin(p, t)));
    return hit ? new Error(`Feche a aba aberta em “${hit}” antes de mover ou colar aqui.`) : null;
  };
  // Renomear: o backend troca o nome (arquivo ou pasta) e o editor troca só o caminho das abas e dos rascunhos. Nada é recusado por rascunho.
  async function renameFile(path: string, name: string) {
    const next = await backend.renameFile(path, name);
    editor.current?.remap(path, next);
    return next;
  }
  // Excluir em lote: o backend manda à Lixeira e devolve o que foi feito. Só o feito sai das abas; o lote (com o erro de uma parada no meio) vai à árvore.
  async function removeMany(paths: string[]) {
    const batch = await backend.deleteFiles(paths);
    editor.current?.remove(batch.done);
    return batch;
  }
  // Mover: confere colisão com as abas antes de qualquer mudança; depois troca o caminho só do que o backend moveu.
  async function moveItems(paths: string[], dest: string) {
    const clash = clashAt(paths.map((p) => join(dest, fileName(p))));
    if (clash) throw clash;
    const batch = await backend.moveFiles(paths, dest);
    for (const m of batch.done) editor.current?.remap(m.from, m.to);
    return batch;
  }
  // Colar na pasta `dir`. O clipboard é lido aqui, e essa mesma lista vai para o backend. Recorte troca o caminho do que foi movido
  // (inclusive numa parada no meio); cópia não mexe nas abas. Devolve as origens que o recorte tirou do projeto.
  async function pasteInto(dir: string): Promise<string[]> {
    const snap = await backend.clipboardSnapshot();
    if (snap.paths.length === 0) throw new Error('Não há arquivo ou pasta copiado para colar.');
    if (snap.cut) {
      const sources = snap.paths.map((p) => relativeTo(stateRef.current.project, p)).filter((p): p is string => p !== null);
      const clash = clashAt(sources.map((s) => join(dir, fileName(s))));
      if (clash) throw clash;
    }
    const batch = await backend.pasteFilesDetailed(dir, snap);
    const moved: string[] = [];
    for (const item of batch.done) {
      if (!snap.cut) continue;
      const from = relativeTo(stateRef.current.project, item.from);
      if (from === null) continue;
      editor.current?.remap(from, item.to);
      moved.push(from);
    }
    if (batch.error) throw new Error(batch.error);
    return moved;
  }
  // Cola o caminho absoluto no rascunho do Orquestrador, sem Enter. O foco vai para o terminal, para a mensagem continuar dali.
  function addToChat(path: string) {
    const view = terminals.get(ORCHESTRATOR, stateRef.current.projectId);
    const text = pathText('absolute', path, stateRef.current.project);
    if (!view || !text) return report(new Error('O Orquestrador não está aberto.'));
    void view.pasteFiles([text]).then(() => view.term.focus()).catch(report);
  }

  // Tira a aba do estado e descarta o terminal dela. Não fala com o backend: quem chama decide se o shell morre.
  function dropShell(id: string) {
    const shell = stateRef.current.shells.find((item) => item.id === id);
    terminals.forget(id, shell?.project);
    dispatch({ type: 'shellRemove', id });
  }

  // `scope.offs` recebe a remoção de cada escuta, que a limpeza do efeito chama; desmontado antes de conectar, não escuta nada.
  async function boot(scope: { live: boolean; offs: Promise<() => void>[] }) {
    try {
      await connect();
      if (!scope.live) return;
      setConnected(true);
      scope.offs.push(
        host().listen('update-status', (status) => {
          updateEvents.current++;
          dispatch({ type: 'update', status });
        }),
        host().listen('term-data', (data) => {
          if (data.project === stateRef.current.projectId || stateRef.current.shells.some((shell) => shell.id === data.id && shell.project === data.project)) terminals.push(data);
        }),
        host().listen('node-added', (node) => {
          if (node.project === stateRef.current.projectId) dispatch({ type: 'nodeAdded', node });
        }),
        host().listen('node-state', ({ project, id, state: next, choice }) => {
          if (project !== stateRef.current.projectId) return;
          dispatch({ type: 'nodeState', id, state: next, choice });
          // O estado vivo: o PTY já existe, então setLive só reenvia o tamanho. Não recria nem zera o terminal.
          if (next === 'running') terminals.get(id, project)?.setLive();
          // Executor que terminou fecha a própria aba; a resposta dele já seguiu para o orquestrador.
          if (FINISHED.includes(next)) {
            terminals.forget(id, project);
            dispatch({ type: 'removed', ids: [id] });
          }
        }),
        host().listen('orchestrator-exit', ({ project, code }) => {
          if (project !== stateRef.current.projectId) return;
          resetOrchestrator(`O orquestrador terminou (${code === null ? 'sem código de saída' : `código de saída ${code}`}).`);
        }),
        // `exit` num shell: o backend já o encerrou, então só a aba dele sai (a última fecha o painel).
        host().listen('shell-exit', ({ project, id }) => {
          if (stateRef.current.shells.some((shell) => shell.id === id && shell.project === project)) dropShell(id);
        }),
      );
      await Promise.all(scope.offs);
      // Depois de escutar os eventos: o estado atual cobre o que saiu antes do registro. Se um evento chegou
      // enquanto ele vinha, o evento é mais novo e a resposta não se aplica.
      // Se a resposta falha depois de um evento novo, a falha é da resposta velha e o evento vale.
      const seen = updateEvents.current;
      await backend.updateStatus().then(
        (status) => { if (updateEvents.current === seen) dispatch({ type: 'update', status }); },
        (e: unknown) => { if (updateEvents.current === seen) throw e; },
      );
    } catch (e) {
      setFatal(true);
      dispatch({ type: 'update', status: { phase: 'failed', message: message(e) } });
    }
  }

  // A árvore do Lobe UI ouve as teclas na janela inteira (Enter, setas, Home, End) enquanto é o escopo de foco ativo, e só deixa de
  // ser quando o foco vai para outro escopo. Diálogo não é escopo: sem isto, o Enter num botão do diálogo ia para a árvore.
  useEffect(() => {
    const leave = (e: FocusEvent) => {
      if (e.target instanceof Element && e.target.closest('[role="dialog"], [role="alertdialog"]')) setActiveScope(null);
    };
    document.addEventListener('focusin', leave, true);
    return () => document.removeEventListener('focusin', leave, true);
  }, []);

  useEffect(() => {
    const scope = { live: true, offs: [] as Promise<() => void>[] };
    void boot(scope);
    return () => {
      scope.live = false;
      for (const off of scope.offs) void off.then((unlisten) => unlisten());
    };
  }, []);

  // Ao desmontar (o ProjectApp sem projeto sai quando a primeira pasta abre), os shells dele morrem no backend: senão o PTY segue vivo
  // e o próximo `shell-1` colide com ele. Sem shells no estado, um `startShell` que ainda responde mata o que acabou de abrir.
  useEffect(() => () => {
    for (const shell of stateRef.current.shells) {
      terminals.forget(shell.id, shell.project);
      void backend.closeShell(shell.id, shell.project).catch(() => {});
    }
    stateRef.current = { ...stateRef.current, shells: [] };
  }, []);

  async function retryUpdate() {
    setRetrying(true);
    const seen = updateEvents.current;
    try {
      const status = await backend.retryUpdate();
      if (updateEvents.current === seen) dispatch({ type: 'update', status });
    } catch (e) {
      if (updateEvents.current === seen) dispatch({ type: 'update', status: { phase: 'failed', message: message(e) } });
    }
    setRetrying(false);
  }

  const openProject = (path?: string) => onOpen(path);
  const chooseProject = () => openProject();
  const openRecentProject = (path: string) => openProject(path);

  async function chooseFile() {
    try {
      const path = await backend.pickFile();
      if (path) await openAbsolutePath(path);
    } catch (e) {
      report(e);
    }
  }

  async function closeProject() {
    const id = stateRef.current.projectId;
    if (!id || stateRef.current.projectBusy) return;
    dispatch({ type: 'projectBusy', busy: true });
    try {
      if (await mayCloseProject(id)) await onClose(id);
    } catch (e) {
      report(e);
    }
    // Fechado, o ProjectApp já saiu; recusado ou com erro, "Fechar pasta" e o X da aba voltam a responder.
    dispatch({ type: 'projectBusy', busy: false });
  }

  // Arquivos não salvos e trabalho do projeto `id` (orquestrador, executores e fila): pergunta antes de fechar a aba.
  async function mayCloseProject(id: string) {
    const work = await backend.projectSnapshot(id);
    const busy = hasWork(work);
    const list = editorOf(id);
    if (unsavedOf(list).length > 0) return settleUnsaved(list, busy, busy ? work : undefined);
    if (busy && (await askClose([], true, work)) !== 'close') return false;
    for (const { editor } of list) editor.flushRecovery();
    return true;
  }

  async function closeTab(id: string) {
    if (id === stateRef.current.projectId) return closeProject();
    try {
      if (await mayCloseProject(id)) await onClose(id);
    } catch (e) { report(e); }
  }

  async function openOrchestrator() {
    if (stateRef.current.orchestrator) return;
    try {
      dispatch({ type: 'open', options: await backend.getOptions() });
    } catch (e) {
      report(e);
    }
  }

  async function startOrchestrator(choice: Choice) {
    const view = terminals.get(ORCHESTRATOR, stateRef.current.projectId)!;
    await backend.startOrchestrator(choice, view.term.cols, view.term.rows);
    dispatch({ type: 'started', choice });
    view.cli = choice.tool;
    view.setLive();
    await view.attach();
    view.term.focus();
  }

  async function toggleExecutorChoice() {
    if (stateRef.current.orchestrator?.overlay === 'executor') return dispatch({ type: 'overlay', overlay: null });
    const options = await backend.getOptions().catch(() => stateRef.current.options!);
    dispatch({ type: 'options', options });
    dispatch({ type: 'overlay', overlay: 'executor' });
  }

  async function saveExecutorChoice(choice: Choice) {
    await backend.setExecutorChoice(choice);
    dispatch({ type: 'executorChoice', choice });
    dispatch({ type: 'overlay', overlay: null });
  }

  // Foco das janelas de escolha: ao fechar, a janela não devolve o foco ao botão que as abriu.
  function focusOrchestrator() {
    terminals.get(ORCHESTRATOR, stateRef.current.projectId)?.term.focus();
  }

  // Nova aba: id novo e ativo. O TermHost dela monta e chama startShell. `cwd` (relativo ao projeto) é a pasta em que ela abre.
  function openShellAt(cwd?: string) {
    shellSeq.current += 1;
    dispatch({ type: 'shellAdd', id: `shell-${shellSeq.current}`, program: lastShellProgram, cwd, project: stateRef.current.projectId });
  }
  // Sem argumento: o botão "+" passa o evento de clique para `onAdd`, então esta função não recebe nada.
  const addShell = () => openShellAt();

  // X da aba: mata o shell no backend e só depois tira a aba. Se o backend recusar, a aba fica para tentar de novo.
  async function closeShell(id: string) {
    const project = stateRef.current.shells.find((item) => item.id === id)?.project;
    try {
      await backend.closeShell(id, project);
    } catch (e) {
      report(e);
      return;
    }
    dropShell(id);
  }

  // Ctrl+` e o botão da barra de status: esconde; com o painel escondido, mostra (ou cria o primeiro shell se não há nenhum).
  function toggleShell() {
    const { shellOpen, shells } = stateRef.current;
    if (shellOpen) return dispatch({ type: 'shellOpen', open: false });
    if (shells.length === 0) return addShell();
    dispatch({ type: 'shellOpen', open: true });
  }

  // Primeiro terminal de cada aba: inicia o shell com as colunas e linhas do xterm. Se falhar, mostra o erro e a aba sai.
  async function startShell(id: string, view: TermView) {
    const shell = stateRef.current.shells.find((s) => s.id === id);
    const cwd = shell?.cwd;
    const project = shell?.project;
    try {
      const program = await backend.startShell(id, view.term.cols, view.term.rows, cwd, project);
      // A aba foi fechada enquanto o shell abria: o terminal já saiu, e o shell que acabou de nascer no backend fica sem aba. Mata de novo.
      if (!stateRef.current.shells.some((s) => s.id === id)) {
        await backend.closeShell(id, project).catch(() => {});
        return;
      }
      lastShellProgram = program;
      dispatch({ type: 'shellProgram', id, program });
      view.setLive();
      await view.attach();
      if (stateRef.current.shellActive === id) view.term.focus();
    } catch (e) {
      report(e);
      dropShell(id);
    }
  }

  // Aba ativa ou painel aberto mudou (clique, X, exit, Ctrl+`, "+"): o terminal da aba ativa reajusta e recebe o foco.
  // Num só lugar, pelo efeito. Escondido, não rouba o foco; e o xterm só recalcula o tamanho com o painel visível.
  useEffect(() => {
    if (!state.shellOpen || !state.shellActive) return;
    terminals.get(state.shellActive, state.projectId)?.fit();
    terminals.get(state.shellActive, state.projectId)?.term.focus();
  }, [state.shellOpen, state.shellActive]);

  // Atalhos de projeto (pesquisa, F4, arquivo rápido) valem só com o projeto aberto e o app pronto.
  const canShortcut = () => !!stateRef.current.project && stateRef.current.update.phase === 'ready';

  // Atalhos de janela num listener só, no window em captura (antes do xterm, que não recebe a tecla). Chama sempre o da última renderização.
  // Cada tecla tem a sua condição e são excludentes; a que casa consome o evento.
  const windowKey = useRef<(e: KeyboardEvent) => void>(() => {});
  useLayoutEffect(() => {
    windowKey.current = (e) => {
      const target = e.target as HTMLElement | null;
      const inTerminal = !!target?.closest?.('.xterm');
      if (stateRef.current.update.phase === 'ready' && !inTerminal) {
        if (projectChordPending.current && !['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
          projectChordPending.current = false;
          window.clearTimeout(projectChordTimer.current);
          if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyO') {
            e.preventDefault();
            e.stopPropagation();
            void openProject();
            return;
          }
        } else if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyK') {
          projectChordPending.current = true;
          window.clearTimeout(projectChordTimer.current);
          projectChordTimer.current = window.setTimeout(() => { projectChordPending.current = false; }, 1500);
        }
        if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyN') {
          e.preventDefault();
          e.stopPropagation();
          editor.current?.newFile();
          return;
        }
        if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyO') {
          e.preventDefault();
          e.stopPropagation();
          void chooseFile();
          return;
        }
        if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyS') {
          e.preventDefault();
          e.stopPropagation();
          void editor.current?.saveActive();
          return;
        }
        if (e.ctrlKey && e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyS') {
          e.preventDefault();
          e.stopPropagation();
          void editor.current?.saveActiveAs();
          return;
        }
      }
      // Ctrl+R, Ctrl+Shift+R e F5 (com ou sem Ctrl) recarregariam a webview e voltariam à escolha de projeto, perdendo a sessão. Bloqueados sempre.
      const reload = (e.ctrlKey && !e.altKey && !e.metaKey && e.code === 'KeyR') || (e.code === 'F5' && !e.altKey && !e.metaKey);
      if (reload) {
        e.preventDefault();
        e.stopPropagation();
        return;
      }
      // Ctrl+` abre e fecha o painel de qualquer lugar.
      if (e.ctrlKey && !e.altKey && !e.metaKey && e.code === 'Backquote') {
        e.preventDefault();
        e.stopPropagation();
        toggleShell();
        return;
      }
      // Ctrl+Shift+F abre a pesquisa e foca o campo, de qualquer lugar (com projeto aberto).
      // Com texto selecionado no editor, esse texto vira a consulta e a busca roda na hora.
      if (e.ctrlKey && e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyF' && canShortcut()) {
        e.preventDefault();
        e.stopPropagation();
        const text = editor.current?.selectedText() ?? '';
        if (text) setSeed({ text });
        setRoute('pesquisa');
        setSearchFocus((n) => n + 1);
        return;
      }
      // F4 vai ao próximo resultado da pesquisa e Shift+F4 ao anterior (com volta). Dentro do terminal (xterm) a tecla segue como antes.
      // Sem resultados a pesquisa não faz nada; a tecla é consumida mesmo assim, pois o App não sabe se há resultados.
      if (e.code === 'F4' && !e.ctrlKey && !e.altKey && !e.metaKey && canShortcut() && !target?.closest?.('.xterm')) {
        e.preventDefault();
        e.stopPropagation();
        setRoute('pesquisa');
        setStep({ dir: e.shiftKey ? -1 : 1 });
        return;
      }
      // Ctrl+P abre o arquivo rápido. Não consome o atalho dentro do terminal (xterm) nem dos campos de texto: ali a tecla segue como antes.
      if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyP' && canShortcut()
        && !target?.closest?.('.xterm') && !target?.closest?.('input, textarea')) {
        e.preventDefault();
        e.stopPropagation();
        setQuickOpen(true);
      }
    };
  });
  useEffect(() => {
    if (!active) return;
    const listener = (e: KeyboardEvent) => windowKey.current(e);
    window.addEventListener('keydown', listener, true);
    return () => {
      window.removeEventListener('keydown', listener, true);
      window.clearTimeout(projectChordTimer.current);
    };
  }, [active]);

  // Sincronizar da barra de status, como o do VS Code: recebe e, havendo commits a enviar, envia. Com executores trabalhando o
  // backend recusa o recebimento, então nem começa; a trava compartilhada impede duas operações do Git ao mesmo tempo.
  async function sync() {
    if (state.executors.some((e) => !FINISHED.includes(e.state))) {
      toast.warning({ title: 'Há executores trabalhando. Espere o lote terminar.', duration: 4000 });
      return;
    }
    if (!gitGuard.enter()) return;
    setSyncing(true);
    try {
      await backend.gitPull();
      const now = await backend.gitStatus();
      if (now.kind === 'repo') {
        if (now.status.ahead > 0) await backend.gitPush();
      }
      toast.success({ title: 'Alterações sincronizadas', duration: 3000 });
    } catch (e) {
      report(e);
    }
    gitGuard.leave();
    setSyncing(false);
    void git.refresh();
  }

  // ---------- derivados ----------

  const { update, orchestrator, executors, options } = state;
  const ready = update.phase === 'ready';
  useEffect(() => {
    if (ready) updateChecked = true;
  }, [ready]);
  // Título da aba: nome da pasta do projeto (último segmento do caminho) e o programa.
  const folder = folderName(state.project);
  const shellTabs = state.shells.map((s) => ({ id: s.id, title: s.program ? (folder ? `${folder} — ${s.program}` : s.program) : folder || 'Terminal', n: s.n, program: s.program, project: s.project }));
  // Enquanto algum executor não terminou, pull, troca e criação de branch ficam travados (o backend recusa do mesmo jeito).
  const busy = executors.some((e) => !FINISHED.includes(e.state));
  const count = (state: NodeState) => executors.filter((e) => e.state === state).length;
  const counts = (['running', 'queued', 'awaiting-choice'] as const)
    .map((state) => [count(state), STATES[state].label] as const)
    .filter(([n]) => n);
  // Contador da navegação, branch e Histórico Git só existem no modo `repo`.
  const changed = state.project && git.data?.kind === 'repo' ? git.data.status.files.length : 0;
  const repoStatus = state.project && git.data?.kind === 'repo' ? git.data.status : undefined;
  const noGitFix = state.project && git.data && git.data.kind !== 'repo' ? NO_GIT_FIX[git.data.kind] : null;
  // Texto do fechamento: a lista de arquivos (o caminho desambigua nomes iguais) e a menção aos terminais, quando há.
  const closeFiles = closeAsk?.files ?? [];
  const closeUnsaved = closeFiles.length === 1
    ? `em ${closeFiles[0]}`
    : `em ${closeFiles.length} arquivos: ${closeFiles.join(', ')}`;
  // Ações do menu de arquivo da árvore (src/explorer/explorer.tsx). Ação sem resposta do backend avisa por toast; o que pede confirmação fica no modal.
  const hasDraft = (path: string) => editor.current?.hasDraft(path) ?? false;
  const fileActions: FileActions = {
    dirtyOf: (paths) => paths.filter(hasDraft),
    copyPaths,
    openInTerminal: (dir) => openShellAt(dir),
    addToChat: orchestrator?.choice ? addToChat : null,
    rename: renameFile,
    removeMany,
    move: moveItems,
    history: git.data?.kind === 'repo' ? (path) => {
      setHistoryRequest({ path });
      setRoute('git');
    } : null,
    // Localizar na pasta: a pasta vai para o campo de incluir da pesquisa (vazio é o projeto inteiro), com o campo focado.
    search: (dir = '') => {
      setScope({ path: dir });
      setRoute('pesquisa');
      setSearchFocus((n) => n + 1);
    },
    paste: pasteInto,
  };

  // Abre na área da direita ou no grupo atual (árvore e arquivo rápido).
  const openIn = (p: string, o?: OpenOptions) => (o?.side ? editor.current?.openBeside(p, { permanent: o.permanent }) : editor.current?.openFile(p, { permanent: o?.permanent }));

  const titlebarMenu: TitleBarMenu = {
    recentProjects,
    projectOpen: !!state.project,
    onNewFile: () => editor.current?.newFile(),
    onOpenFile: () => void chooseFile(),
    onOpenProject: chooseProject,
    onOpenRecent: openRecentProject,
    onSave: () => void editor.current?.saveActive(),
    onSaveAs: () => void editor.current?.saveActiveAs(),
    onCloseProject: () => void closeProject(),
    onExit: () => { void host().close().catch(report); },
  };

  // ---------- telas ----------

  if (!ready) {
    const retry = update.phase === 'failed';
    return (
      <Screen connected={connected} still={retry} description={(
        <Text as="p" fontSize={14} style={SECONDARY} role="status" aria-live="polite">{updateText(update, fatal)}</Text>
      )}>
        {update.phase === 'downloading' && (
          // Total desconhecido (ou zero): sem percentual, a barra cheia com o brilho do status active. O Progress não tem modo indeterminado de fato.
          <Progress
            aria-label="Progresso do download da atualização"
            percent={update.total ? Math.min(100, (update.downloaded / update.total) * 100) : 100}
            status={update.total ? undefined : 'active'}
            showInfo={false}
          />
        )}
        {update.phase === 'failed' && (
          <Alert type="error" role="alert" description={update.message} className="select-text" />
        )}
        {retry && (
          <Button type="primary" onClick={retryUpdate} disabled={retrying} loading={retrying} className="self-center">
            Tentar novamente
          </Button>
        )}
      </Screen>
    );
  }

  // Este é o layout de uma tela só: a faixa da esquerda muda de conteúdo (Arquivos, Pesquisa, Git) e a área central fica montada.
  // A faixa é a mesma nas três rotas: a largura é uma só.
  const shown = (to: Route) => `absolute inset-0${route === to ? '' : ' invisible'}`;
  const sideShown = !!state.project || sideOpen;
  const go = (to: Route) => {
    if (!state.project) setSideOpen(!sideOpen || to !== route);
    setRoute(to);
  };
  return (
    <BackendProvider projectId={state.projectId}>
    <Flexbox height="100dvh">
      <TitleBar connected={connected} project={state.project} titleActive={active} menu={titlebarMenu} projectTabs={projects ? {
        projects, active: initialProject?.id ?? null, onSelect: (id) => onSelect?.(id), onClose: (id) => void closeTab(id),
        onMove: (id, index) => onMove?.(id, index), onAdd: () => onOpen(),
      } : undefined}>
        {counts.map(([n, label]) => (
          <Text key={label} as="span" fontSize={12} style={SECONDARY} noWrap ellipsis title={`${n} ${label}`} data-tauri-drag-region>
            {`${n} ${label}`}
          </Text>
        ))}
      </TitleBar>

      <Flexbox ref={rowRef} horizontal flex={1} className="min-h-0">
        <Nav route={sideShown ? route : null} changed={changed} onGo={go} />
        <main className="relative min-h-0 min-w-0 flex-1">
          <Flexbox horizontal height="100%">
            {/* Faixa da esquerda: árvore, pesquisa ou Git, todas montadas; só a da rota aparece. A borda direita arrasta a largura. */}
            {sideShown && <DraggablePanel
              placement="left" expandable={false} showHandleWideArea={false}
              size={{ width: shownSideWidth }} defaultSize={SIDE_DEFAULT_SIZE}
              onSizeChange={(_, size) => typeof size?.width === 'number' && setSideWidth(size.width)}
              minWidth={SIDE_MIN} maxWidth={sideMaxWidth}
              backgroundColor="var(--ant-color-bg-layout)"
            >
              <div className="relative h-full">
                <div className={shown('arquivos')}>
                  {state.project ? (
                    <FilesScreen openPath={activeTarget && activeTarget.kind !== 'draft' ? activeTarget.path : null} onOpen={openIn} onPrefetch={(path) => editor.current?.prefetch(path)} reveal={treeReveal} actions={fileActions}
                      project={folder} git={git.data?.kind === 'repo' ? git.data.status : null} active={active && route === 'arquivos'}
                    />
                  ) : <OpenProjectPrompt onOpen={chooseProject} description="Abra uma pasta para explorar os arquivos." />}
                </div>
                <div className={shown('pesquisa')}>
                  {state.project ? <SearchScreen
                    active={active && route === 'pesquisa'} focusKey={searchFocus} folder={scope} seed={seed} refreshKey={saveTick} step={step} hasDraft={hasDraft}
                    openPaths={() => editor.current?.openPaths() ?? []}
                    onOpen={(path, line, col, options) => editor.current?.openFile(path, { permanent: options?.permanent, line, col })}
                    onChanged={() => { git.refresh(); editor.current?.refresh(); }}
                  /> : <OpenProjectPrompt onOpen={chooseProject} description="Abra uma pasta para pesquisar nos arquivos." />}
                </div>
                <div className={shown('git')}>
                  {state.project ? <GitSidebar
                    git={git} locked={busy} guard={gitGuard} active={active && route === 'git'} selected={activeTarget} historyRequest={historyRequest}
                    onOpenTarget={(target, options) => editor.current?.openTarget(target, options)}
                    onPrefetchTarget={(target) => editor.current?.prefetchTarget(target)}
                    onOpenFile={(path, options) => editor.current?.openFile(path, options)}
                  /> : <OpenProjectPrompt onOpen={chooseProject} description="Abra uma pasta para usar o Git." />}
                </div>
              </div>
            </DraggablePanel>}
            {/* Coluna central: o editor (grupos, abas, diffs e conflitos) em cima e o painel Terminal embaixo. Sempre montada. */}
            <Flexbox ref={centerRef} height="100%" flex={1} allowShrink>
              <div className="relative min-h-0 flex-1">
                <div className="absolute inset-0">
                  <Flexbox height="100%" flex={1} className="min-h-0">
                    {/* key por projeto: outro projeto remonta o editor (lê a recuperação dele); o anterior grava a sua ao sair. */}
                    <EditorWorkspace
                      key={state.project} ref={editor} project={state.project} active={active}
                      locked={busy} guard={gitGuard}
                      refreshGit={() => git.refresh()} onSaved={() => setSaveTick((n) => n + 1)}
                      onActiveTarget={setActiveTarget}
                      onGroupCount={setGroupCount}
                      layout={layoutText}
                      onRestoreLayout={(text) => {
                        const saved = readLayout(text);
                        if (!saved) return;
                        if (saved.sideWidth !== undefined) setSideWidth(saved.sideWidth);
                        if (saved.panelWidth !== undefined) setPanelWidth(saved.panelWidth);
                        if (saved.shellHeight !== undefined) setShellHeight(saved.shellHeight);
                      }}
                      onReveal={(path) => { setTreeReveal({ path }); setRoute('arquivos'); }}
                    />
                  </Flexbox>
                </div>
              </div>
              <ShellPanel
                open={state.shellOpen} maxHeight={centerHeight - FILE_MIN_HEIGHT}
                height={shellHeight} onHeightChange={setShellHeight}
                tabs={shellTabs} active={state.shellActive}
                onSelect={(id) => dispatch({ type: 'shellSelect', id })} onMove={(id, index) => dispatch({ type: 'shellMove', id, index })} onAdd={addShell}
                onHide={() => dispatch({ type: 'shellOpen', open: false })} onClose={closeShell} onReady={startShell}
              />
            </Flexbox>
          </Flexbox>
        </main>
        {/* Painel do Orquestrador, à direita de tudo e em todas as telas: nenhuma rota o desmonta nem muda o tamanho dele. Sem projeto fica oculto, sem desmontar. */}
        <div className={state.project ? 'contents' : 'hidden'}>
        <TerminalsScreen
          projectId={state.projectId} projectOpen={!!state.project} onOpenProject={chooseProject}
          orchestrator={orchestrator}
          executors={executors}
          executorChoice={state.executorChoice}
          notice={state.notice}
          width={shownPanelWidth}
          maxWidth={panelMaxWidth}
          onWidthChange={setPanelWidth}
          onOpen={openOrchestrator}
          onExecutorChoice={toggleExecutorChoice}
          onStop={(id) => backend.stopExecutor(id).catch(report)}
        />
        </div>
      </Flexbox>

      <StatusBar
        git={repoStatus && { branch: repoStatus.branch, ahead: repoStatus.ahead, behind: repoStatus.behind, syncing }}
        terminalOpen={state.shellOpen} onToggleTerminal={toggleShell} onOpenGit={() => setRoute('git')} onSync={() => void sync()}
      />

      {/* As duas escolhas são janelas: a do orquestrador (início) e uma só para os executores. Com um executor aguardando escolha, a janela dele é a única e não fecha sem confirmar: nunca duas empilhadas. */}
      {options && (
        <>
          <ChoiceForm
            open={orchestrator?.overlay === 'start'}
            title="Orquestrador"
            hint={(
              <Flexbox gap={8}>
                <Text as="p" fontSize={14} style={SECONDARY}>Só lê o projeto e delega. Você conversa com ele neste terminal.</Text>
                {noGitFix && (
                  <Text as="p" fontSize={14} style={SECONDARY}>{`Esta pasta não tem Git: o que os executores mudarem não fica registrado e não pode ser desfeito pelo Git. ${noGitFix}`}</Text>
                )}
              </Flexbox>
            )}
            options={options} initial={options.last.orchestrator} confirmLabel="Abrir terminal"
            onCancel={() => resetOrchestrator()} onConfirm={startOrchestrator} onClosed={focusOrchestrator}
          />
          <ChoiceForm
            open={orchestrator?.overlay === 'executor' || count('awaiting-choice') > 0}
            title={count('awaiting-choice') > 0 ? 'Escolha dos executores' : 'Executores'}
            hint={count('awaiting-choice') > 0
              ? 'Vale para este e para os próximos executores enquanto o orquestrador estiver aberto.'
              : 'Ferramenta, modelo e esforço dos próximos executores. Os que já estão rodando não mudam.'}
            options={options} initial={state.executorChoice ?? options.last.executor} confirmLabel="Usar nos executores"
            onCancel={count('awaiting-choice') > 0 ? undefined : () => dispatch({ type: 'overlay', overlay: null })}
            onConfirm={(choice) => saveExecutorChoice(choice)} onClosed={focusOrchestrator}
          />
        </>
      )}
      {quickOpen && (
        <QuickOpen
          onClose={() => setQuickOpen(false)}
          onOpen={(p, c) => { setQuickOpen(false); openIn(p, c); }}
        />
      )}
      {/* Esc, clique fora e Cancelar recusam o fechamento; só o botão de confirmação fecha. */}
      {/* Borda e sombra neutras do tema. */}
      <Modal
        open={closeAsk !== null}
        title={closeFiles.length > 0 ? 'Fechar com alterações não salvas?' : closeAsk?.projectWork ? 'Fechar o projeto?' : 'Fechar o Orquestrador?'}
        width={420}
        closable={false}
        keyboard
        maskClosable
        onCancel={() => settleClose('cancel')}
        style={{ border: '1px solid var(--ant-color-border)', boxShadow: 'var(--ant-box-shadow-secondary)' }}
        styles={{
          header: { padding: '20px 20px 0', minHeight: 0 },
          title: { fontSize: 16, fontWeight: 600, lineHeight: 1.4 },
          body: { padding: '6px 20px 20px' },
          footer: { padding: '0 20px 20px' },
        }}
        footer={(
          <Flexbox horizontal wrap="wrap" justify="flex-end" gap={8} width="100%">
            {/* Foco inicial no Cancelar: Enter recusa o fechamento, e nunca encerra os terminais por engano. */}
            <Button autoFocus disabled={closeSaving} onClick={() => settleClose('cancel')}>Cancelar</Button>
            {closeFiles.length > 0 ? (
              <>
                <Button danger disabled={closeSaving} style={DANGER_SOFT} onClick={() => settleClose('discard')}>
                  {closeAsk?.terminals ? 'Descartar e encerrar todos' : 'Descartar'}
                </Button>
                <Button type="fill" loading={closeSaving} onClick={() => settleClose('save')}>
                  {closeSaving ? 'Salvando…' : closeAsk?.terminals ? 'Salvar e encerrar todos' : 'Salvar'}
                </Button>
              </>
            ) : (
              <Button danger disabled={closeSaving} style={DANGER_SOFT} onClick={() => settleClose('close')}>
                {closeAsk?.projectWork ? 'Fechar projeto e encerrar' : 'Fechar e encerrar todos'}
              </Button>
            )}
          </Flexbox>
        )}
      >
        <Flexbox gap={8}>
          {closeFiles.length > 0 ? (
            <>
              <Text as="p" fontSize={14} lineHeight={1.6} style={{ ...SECONDARY, margin: 0, overflowWrap: 'anywhere' }}>
                Há alterações não salvas {closeUnsaved}.
              </Text>
              <Text as="p" fontSize={14} lineHeight={1.6} style={{ ...SECONDARY, margin: 0, overflowWrap: 'anywhere' }}>
                Salvar grava os arquivos. Descartar joga a edição fora.
              </Text>
              {closeAsk?.terminals && (
                <Text as="p" fontSize={14} lineHeight={1.6} style={{ ...SECONDARY, margin: 0, overflowWrap: 'anywhere' }}>
                  Encerrar fecha os terminais em execução, inclusive os shells: o trabalho dos executores para no meio.
                </Text>
              )}
            </>
          ) : (
            closeAsk?.projectWork ? (
              <Text as="p" fontSize={14} lineHeight={1.6} style={{ ...SECONDARY, margin: 0, overflowWrap: 'anywhere' }}>
                {closeAsk.projectWork.orchestratorOpen ? 'O orquestrador aberto será encerrado. ' : ''}
                {closeAsk.projectWork.executorsRunning > 0 && `${closeAsk.projectWork.executorsRunning} executores em execução serão encerrados. `}
                {closeAsk.projectWork.executorsQueued > 0 && `${closeAsk.projectWork.executorsQueued} executores na fila serão removidos. `}
                Os shells deste projeto também serão fechados.
              </Text>
            ) : (
              <Text as="p" fontSize={14} lineHeight={1.6} style={{ ...SECONDARY, margin: 0, overflowWrap: 'anywhere' }}>
                Todos os terminais serão encerrados, inclusive os shells. O trabalho dos executores para no meio.
              </Text>
            )
          )}
        </Flexbox>
      </Modal>
    </Flexbox>
    </BackendProvider>
  );
}

/** Abertura, atualização e reabertura dos projetos: barra no topo e a marca com o status no centro, no layout do `index.html` (classes boot-* de lá). */
function Screen({ connected, still, description, children }: { connected: boolean; still?: boolean; description: ReactNode; children: ReactNode }) {
  return (
    <Flexbox height="100dvh">
      <TitleBar connected={connected} />
      <main className="boot-main">
        <div className="boot-col">
          <BootMark still={still} />
          <Flexbox gap={6}>
            <Text as="h1" fontSize={16} weight={600} lineHeight={1}>Orquestrador</Text>
            {description}
          </Flexbox>
          <Flexbox gap={16} className="text-left empty:hidden">{children}</Flexbox>
        </div>
      </main>
    </Flexbox>
  );
}

/** Início da animação da marca no `index.html`, lido antes de o React trocar o HTML; sem animação lá (movimento reduzido), o agora. */
let bootStart: number | undefined;

/** A marca animada do `index.html`. `--t` é o tempo desde o início dela: a animação continua da mesma fase, inclusive entre telas. */
function BootMark({ still }: { still?: boolean }) {
  const [t] = useState(() => {
    if (bootStart === undefined) bootStart = Number(document.querySelector('.boot-mark .hub')?.getAnimations()[0]?.startTime ?? performance.now());
    return `${performance.now() - bootStart}ms`;
  });
  return (
    <svg className={`boot-mark${still ? ' still' : ''}`} style={{ '--t': t } as React.CSSProperties} viewBox="28 28 200 200" aria-hidden="true">
      <g stroke="#ECEAE4" strokeWidth={19} strokeLinecap="round">
        <line x1={73.9} y1={182.1} x2={73.9} y2={59.1} pathLength={1} />
        <line x1={73.9} y1={182.1} x2={156.3} y2={91.1} pathLength={1} />
        <line x1={73.9} y1={182.1} x2={196.3} y2={169.2} pathLength={1} />
      </g>
      <g fill="#ECEAE4">
        <circle className="node" cx={73.9} cy={59.1} r={23} />
        <circle className="node" cx={156.3} cy={91.1} r={23} />
        <circle className="node" cx={196.3} cy={169.2} r={23} />
      </g>
      <circle className="hub" cx={73.9} cy={182.1} r={36.5} fill="#F5A524" />
    </svg>
  );
}
