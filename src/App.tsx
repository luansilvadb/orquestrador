import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from 'react';
import { Block, Center, Flexbox } from '@lobehub/ui';
import { Alert, Button, DraggablePanel, Modal, Progress, Text, toast } from '@lobehub/ui/base-ui';
import { backend, connect, host, message } from './backend';
import { ChoiceForm } from './choice-form';
import { FileView, type Reveal } from './file-view';
import { FileTabs } from './file-tabs';
import { batchTargets, dropFrom, isWithin, type BatchKind, type CopyKind, type OpenFiles, pathText, pinFrom, relativeTo, renamedFiles, settleBatch } from './file-tab-actions';
import { FilesScreen, Nav, StatusBar, type FileActions, type Route } from './explorer';
import { GitScreen, usePolled } from './git';
import { SearchScreen } from './search';
import { ShellPanel } from './shell-panel';
import { FINISHED, PANEL_MIN, PANEL_WIDTH, STATES, TerminalsScreen } from './terminal-screen';
import type { TermView } from './term';
import { terminals } from './terminals';
import { TitleBar } from './titlebar';
import {
  ORCHESTRATOR, type Choice, type ExecutorNode, type NodeState, type Options, type UpdateStatus,
} from './types';

const MB = 1024 * 1024;
// Larguras da linha principal: o Nav, o mínimo que a coluna central mantém, e a faixa da árvore e da pesquisa (a inicial, que não se guarda entre aberturas, e a mínima).
const NAV_WIDTH = 44;
const CENTER_MIN_WIDTH = 240;
const SIDE_WIDTH = 280;
const SIDE_MIN = 200;
// Objeto constante: o duplo clique na alça volta para a largura inicial (o DraggablePanel usa `defaultSize` nesse reset).
const SIDE_DEFAULT_SIZE = { width: SIDE_WIDTH };
// Altura que a área do arquivo mantém quando o painel Terminal está aberto no máximo.
const FILE_MIN_HEIGHT = 160;

type Overlay = 'start' | 'executor' | null;
// Um shell do painel Terminal: `program` fica '' até o backend responder com o nome do programa.
// `cwd` é a pasta (relativa ao projeto) em que o shell abre; sem ela, a raiz do projeto.
type ShellItem = { id: string; program: string; cwd?: string };
type State = {
  update: UpdateStatus;
  project: string;
  projectBusy: boolean;
  options: Options | null;
  orchestrator: { choice: Choice | null; started: boolean; overlay: Overlay } | null;
  executorChoice: Choice | null;
  executors: ExecutorNode[];
  notice: string;
  shells: ShellItem[];
  shellActive: string;
  shellOpen: boolean;
};
type Action =
  | { type: 'update'; status: UpdateStatus }
  | { type: 'project'; project: string; options: Options }
  | { type: 'options'; options: Options }
  | { type: 'open'; options: Options }
  | { type: 'started'; choice: Choice }
  | { type: 'overlay'; overlay: Overlay }
  | { type: 'executorChoice'; choice: Choice }
  | { type: 'nodeAdded'; node: ExecutorNode }
  | { type: 'nodeState'; id: string; state: NodeState; choice: Choice | null }
  | { type: 'removed'; ids: string[] }
  | { type: 'reset'; notice: string }
  | { type: 'shellAdd'; id: string; cwd?: string }
  | { type: 'shellProgram'; id: string; program: string }
  | { type: 'shellRemove'; id: string }
  | { type: 'shellSelect'; id: string }
  | { type: 'shellOpen'; open: boolean };

const initial: State = {
  update: { phase: 'checking' }, project: '', projectBusy: false, options: null, orchestrator: null,
  executorChoice: null, executors: [], notice: '', shells: [], shellActive: '', shellOpen: false,
};

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'update':
      return { ...state, update: action.status };
    case 'project':
      return { ...state, project: action.project, options: action.options, projectBusy: false };
    case 'options':
      return { ...state, options: action.options };
    case 'open':
      return {
        ...state, options: action.options, notice: '',
        orchestrator: { choice: null, started: false, overlay: 'start' },
      };
    case 'started':
      return { ...state, orchestrator: state.orchestrator && { ...state.orchestrator, choice: action.choice, started: true, overlay: null } };
    case 'overlay':
      return { ...state, orchestrator: state.orchestrator && { ...state.orchestrator, overlay: action.overlay } };
    case 'executorChoice':
      return { ...state, executorChoice: action.choice };
    case 'nodeAdded': {
      if (!state.orchestrator || state.executors.some((e) => e.id === action.node.id)) return state;
      return { ...state, executors: [...state.executors, action.node] };
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
    case 'shellAdd':
      return { ...state, shells: [...state.shells, { id: action.id, program: '', cwd: action.cwd }], shellActive: action.id, shellOpen: true };
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

const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });

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

export function App() {
  const [state, dispatch] = useReducer(reducer, initial);
  const stateRef = useRef(state);
  useLayoutEffect(() => {
    stateRef.current = state;
  });
  const [route, setRoute] = useState<Route>('arquivos');
  // Arquivos abertos na área do meio. Cada um fica montado; só o ativo aparece e interage.
  const [files, setFiles] = useState<OpenFiles>({ paths: [], active: null, pinned: [] });
  const openFile = files.active;
  const filesRef = useRef(files);
  filesRef.current = files;
  // Caminhos com alteração não salva, de qualquer arquivo aberto (ativo ou não). Protege o fechamento da janela e o de cada aba.
  const [dirtyFiles, setDirtyFiles] = useState<ReadonlySet<string>>(() => new Set());
  const dirtyRef = useRef(dirtyFiles);
  dirtyRef.current = dirtyFiles;
  // Linha e coluna do cursor de cada arquivo aberto, informadas pelo editor (sem render: só o menu lê, na hora do clique).
  const cursors = useRef(new Map<string, { line: number; col: number }>());
  const reportCursor = useCallback((path: string, line: number, col: number) => {
    cursors.current.set(path, { line, col });
  }, []);
  // Acorde Ctrl+K: vale por 1,5 s; o timer é o único recurso dele, então sai com o componente.
  const chordPending = useRef(false);
  const chordTimer = useRef(0);
  // Pedido de revelar no explorador: o `id` é novo a cada pedido, então o mesmo arquivo revela de novo.
  const [treeReveal, setTreeReveal] = useState<{ path: string; id: number } | null>(null);
  const treeSeq = useRef(0);
  // Sobe a cada Ctrl+Shift+F: a pesquisa refoca o campo mesmo se já estiver na tela.
  const [searchFocus, setSearchFocus] = useState(0);
  // O status do Git roda com o projeto aberto, em qualquer tela: o contador da navegação precisa dele.
  // Status do Git, atualizado a cada 1,5 s. O estado só muda quando a resposta é diferente, para a lista não piscar. Sem projeto (`enabled` falso), não consulta.
  const git = usePolled(() => backend.gitStatus(), state.update.phase === 'ready' && !!state.project, true);
  const [connected, setConnected] = useState(false);
  const [fatal, setFatal] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [projectError, setProjectError] = useState('');
  // A confirmação combina descarte de edição (os caminhos com alteração não salva) e encerramento de terminais conforme a ação pedida.
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [confirmFiles, setConfirmFiles] = useState<string[]>([]);
  const [confirmTerminals, setConfirmTerminals] = useState(false);
  const closeAnswer = useRef<((ok: boolean) => void) | null>(null);
  const updateEvents = useRef(0);
  // Contador dos shells do painel Terminal: só cresce, então um id nunca se repete enquanto o app está aberto.
  const shellSeq = useRef(0);
  // Largura da janela (a faixa e o painel do Orquestrador cabem nela) e altura da coluna central (o painel Terminal não come a área do arquivo).
  const [rowRef, rowWidth] = useSize('width');
  const [centerRef, centerHeight] = useSize('height');
  // Larguras que o usuário deu à faixa e ao painel do Orquestrador. As exibidas são elas limitadas ao que cabe na janela agora.
  const [sideWidth, setSideWidth] = useState(SIDE_WIDTH);
  const [panelWidth, setPanelWidth] = useState(PANEL_WIDTH);
  // O que a faixa e o painel dividem: a janela menos o Nav e o mínimo da coluna central. Indefinido enquanto a janela não foi medida: os dois nascem na largura inicial, sem limite e sem animação.
  const sharedWidth = rowWidth ? rowWidth - NAV_WIDTH - CENTER_MIN_WIDTH : undefined;
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

  // ---------- ações ----------

  const resetOrchestrator = (notice = '') => {
    for (const e of stateRef.current.executors) terminals.forget(e.id);
    dispatch({ type: 'reset', notice });
  };

  // Com uma confirmação já na tela, o pedido novo não sobrescreve a resposta pendente: recusa, e a primeira continua valendo.
  const askClose = (unsaved: string[] = [], running = false) => {
    if (closeAnswer.current) return Promise.resolve(false);
    return new Promise<boolean>((resolve) => {
      closeAnswer.current = resolve;
      setConfirmFiles(unsaved);
      setConfirmTerminals(running);
      setConfirmOpen(true);
    });
  };
  const setFileDirty = useCallback((path: string, dirty: boolean) => {
    setDirtyFiles((prev) => {
      if (prev.has(path) === dirty) return prev;
      const next = new Set(prev);
      if (dirty) next.add(path);
      else next.delete(path);
      return next;
    });
  }, []);
  // Pedido de navegação a uma linha de um resultado da pesquisa. Um pedido só vale enquanto o usuário não abre ou ativa outro arquivo pelo caminho normal.
  const [reveal, setReveal] = useState<Reveal | null>(null);
  const revealSeq = useRef(0);
  // Abrir um arquivo que já está aberto só o ativa (sem duplicar a aba). Trocar de aba não pede nada: cada arquivo guarda o próprio rascunho.
  const openFileAt = (path: string) => {
    setReveal(null);
    setFiles((prev) => (prev.paths.includes(path)
      ? prev.active === path ? prev : { ...prev, active: path }
      : { ...prev, paths: [...prev.paths, path], active: path }));
  };
  const selectFile = (path: string) => {
    setReveal(null);
    setFiles((prev) => (prev.active === path ? prev : { ...prev, active: path }));
  };
  // Clique num resultado da pesquisa: abre ou ativa o arquivo (sem duplicar aba) e pede a linha. Cada clique gera um id novo, então a mesma linha navega de novo.
  const openResult = (path: string, line: number) => {
    openFileAt(path);
    setReveal({ path, line, id: ++revealSeq.current });
  };
  // Tira as abas `closing` do estado, com o rascunho, o cursor e o pedido de cada uma. Quem chama já decidiu sobre as alterações não salvas.
  // A aba ativa que sai passa para a vizinha (direita, ou esquerda se não há à direita entre as que ficam).
  const dropFiles = (closing: ReadonlySet<string>) => {
    for (const path of closing) {
      setFileDirty(path, false);
      cursors.current.delete(path);
    }
    setReveal((prev) => (prev && closing.has(prev.path) ? null : prev));
    setFiles((prev) => dropFrom(prev, closing));
  };
  // Fechar explícito (X, Fechar, Ctrl+F4) pede confirmação só se a própria aba tem alteração não salva. Fecha inclusive fixada.
  const closeFile = async (path: string) => {
    if (dirtyRef.current.has(path) && !(await askClose([path]))) return;
    dropFiles(new Set([path]));
  };
  // Fechamento em lote, de uma vez só: uma confirmação para todos os rascunhos que ele descarta. Cancelar não fecha nenhuma aba.
  // Os alvos são recalculados a cada rodada: uma aba limpa que fica suja durante a confirmação não some, e uma já fechada por outro caminho não entra.
  const closeFiles = async (kind: BatchKind, from?: string) => {
    const targets = await settleBatch(
      kind, from,
      () => ({ ...filesRef.current, dirty: dirtyRef.current }),
      (unsaved) => askClose(unsaved),
    );
    if (targets?.length) dropFiles(new Set(targets));
  };
  // Fixar alterna (as fixadas vão para o começo da faixa).
  const togglePin = (path: string) => setFiles((prev) => pinFrom(prev, path));
  // Copia o caminho da aba clicada. Erro do clipboard vira toast; a navegação sem cursor já vem desabilitada no menu.
  const copyPath = async (kind: CopyKind, path: string) => {
    const text = pathText(kind, path, state.project, cursors.current.get(path));
    if (text === null) return;
    try {
      await navigator.clipboard.writeText(text);
      toast.success({ title: 'Caminho copiado', duration: 3000 });
    } catch (e) {
      toast.error({ title: 'Não foi possível copiar o caminho', description: message(e), duration: 7000 });
    }
  };
  // Revelar no Explorer: a árvore abre as pastas do arquivo e o marca; a tela vai para Arquivos. Os editores seguem montados.
  const revealInTree = (path: string) => {
    setTreeReveal({ path, id: ++treeSeq.current });
    setRoute('arquivos');
  };
  // Renomear um arquivo com alteração não salva é recusado: trocar o caminho de um rascunho exigiria reestruturar as abas, e perder o texto
  // é pior. Arquivo limpo muda de caminho na aba, na ativa, nos fixados e no cursor; o editor remonta no caminho novo e relê o disco.
  // Erro do backend sai como exceção, e o modal da árvore mostra; nada do estado muda.
  async function renameFile(path: string, name: string) {
    if (dirtyRef.current.has(path)) throw new Error('Salve as alterações deste arquivo antes de renomear.');
    const next = await backend.renameFile(path, name);
    const cursor = cursors.current.get(path);
    cursors.current.delete(path);
    if (cursor) cursors.current.set(next, cursor);
    setReveal((prev) => (prev?.path === path ? null : prev));
    setFiles((prev) => renamedFiles(prev, path, next));
    return next;
  }
  // Excluir: vai para a Lixeira; só depois de o backend confirmar a aba do arquivo fecha. Se falhar, nada sai do lugar.
  async function removeFile(path: string) {
    await backend.deleteFile(path);
    dropFiles(new Set([path]));
  }
  // Cola o caminho absoluto no rascunho do Orquestrador, sem Enter. O foco vai para o terminal, para a mensagem continuar dali.
  function addToChat(path: string) {
    const view = terminals.get(ORCHESTRATOR);
    const text = pathText('absolute', path, state.project);
    if (!view || !text) return report(new Error('O Orquestrador não está aberto.'));
    void view.pasteFiles([text]).then(() => view.term.focus()).catch(report);
  }
  // Copiar e recortar põem o arquivo no clipboard do Windows: colar é no Explorer. Recortar não move nada agora.
  function copyFile(path: string, cut: boolean) {
    backend.copyFile(path, cut).then(
      () => toast.success({ title: cut ? 'Arquivo recortado. Cole no Explorer para mover.' : 'Arquivo copiado. Cole no Explorer.', duration: 4000 }),
      report,
    );
  }
  // Caminhos (relativos) que não estão mais no disco. Pasta sumida leva junto os arquivos de dentro: a listagem da pasta falha.
  async function missing(paths: string[]) {
    const gone: string[] = [];
    for (const path of paths) {
      const cut = path.lastIndexOf('/');
      const here = await backend.listDir(cut < 0 ? '' : path.slice(0, cut))
        .then((list) => list.some((e) => e.name === path.slice(cut + 1)), () => false);
      if (!here) gone.push(path);
    }
    return gone;
  }
  // Colar na raiz. O clipboard é lido aqui, e essa mesma lista vale para a checagem de alterações não salvas e para o backend.
  // Recorte recusa mover qualquer origem que tenha arquivo sujo. Depois de mover, fecha as abas limpas cujo arquivo sumiu;
  // uma aba que ficou (colagem recusada, ou suja durante a operação) continua aberta. Devolve as origens que o recorte tirou; vazio na cópia.
  async function pasteInto(dir: string): Promise<string[]> {
    const snap = await backend.clipboardSnapshot();
    if (snap.paths.length === 0) throw new Error('Não há arquivo ou pasta copiado para colar.');
    const sources = snap.cut
      ? snap.paths.map((p) => relativeTo(stateRef.current.project, p)).filter((p): p is string => p !== null)
      : [];
    const under = (path: string) => sources.some((s) => isWithin(path, s));
    const unsaved = [...dirtyRef.current].filter(under);
    if (unsaved.length > 0) throw new Error(`Há alterações não salvas em ${unsaved.join(', ')}. Salve ou descarte antes de recortar.`);
    const candidates = filesRef.current.paths.filter(under);
    let failure: unknown = null;
    try {
      await backend.pasteFiles(dir, snap);
    } catch (e) {
      failure = e;
    }
    // Recortado, mesmo com falha no meio: o que sumiu do disco é fechado; o resto segue como estava.
    if (snap.cut) dropFiles(new Set((await missing(candidates)).filter((p) => !dirtyRef.current.has(p))));
    if (failure) throw failure;
    return sources;
  }

  // Atalhos da área do arquivo (o XTerm e os campos ficam fora dela). Só o que o app faz: Ctrl+F4 fecha a ativa; Ctrl+K W
  // fecha todas; Ctrl+K Ctrl+Shift+C copia o relativo; Shift+Alt+C o absoluto; Ctrl+K Shift+Enter fixa. Os outros acordes não fazem nada.
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
  // Há alvo para o fechamento em lote agora? Usa os refs, então vale no instante da tecla.
  const hasTargets = (kind: BatchKind) =>
    batchTargets(filesRef.current.paths, filesRef.current.pinned, dirtyRef.current, kind).length > 0;
  // Captura (não bubbling): o keymap do CodeMirror trata Ctrl+K (apagar até o fim da linha) na fase de bolha, então este handler
  // vê a tecla antes do editor e pode parar a propagação. Só alcança o que está dentro da área do arquivo; o xterm fica fora dela.
  const onFileKey = (e: ReactKeyboardEvent<HTMLElement>) => {
    const target = e.target as HTMLElement;
    if (target.closest('input, textarea, [role="menu"], [role="listbox"], [role="dialog"]')) return;
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
    const active = filesRef.current.active;
    const ctrl = e.ctrlKey && !e.altKey && !e.metaKey;
    const plain = !e.altKey && !e.metaKey && !e.shiftKey;
    if (chordPending.current) {
      // Ctrl+K de novo reinicia o acorde, sem chegar ao editor.
      if (ctrl && !e.shiftKey && e.code === 'KeyK') {
        e.preventDefault();
        e.stopPropagation();
        beginChord();
        return;
      }
      endChord();
      // Atalho desabilitado (sem alvo ou sem arquivo) não é capturado: a tecla segue para o editor, como qualquer outra.
      let run: (() => void) | null = null;
      if (plain && e.code === 'KeyW' && hasTargets('all')) run = () => void closeFiles('all');
      else if (ctrl && e.shiftKey && e.code === 'KeyC' && active) run = () => void copyPath('relative', active);
      // Shift+Enter basta: o rótulo é Ctrl+K, soltar Ctrl, Shift+Enter. Ctrl mantido também vale.
      else if (!e.altKey && !e.metaKey && e.shiftKey && e.code === 'Enter' && active) run = () => togglePin(active);
      if (!run) return;
      e.preventDefault();
      e.stopPropagation();
      run();
      return;
    }
    if (ctrl && !e.shiftKey && e.code === 'KeyK') {
      e.preventDefault();
      e.stopPropagation();
      beginChord();
    } else if (ctrl && !e.shiftKey && e.code === 'F4' && active) {
      e.preventDefault();
      e.stopPropagation();
      void closeFile(active);
    } else if (e.shiftKey && e.altKey && !e.ctrlKey && !e.metaKey && e.code === 'KeyC' && active) {
      e.preventDefault();
      e.stopPropagation();
      void copyPath('absolute', active);
    }
  };
  const settleClose = (ok: boolean) => {
    setConfirmOpen(false);
    closeAnswer.current?.(ok);
    closeAnswer.current = null;
  };

  async function boot() {
    try {
      await connect();
      setConnected(true);
      await Promise.all([
        host().listen('update-status', (status) => {
          updateEvents.current++;
          dispatch({ type: 'update', status });
        }),
        host().listen('term-data', (data) => terminals.push(data)),
        host().listen('node-added', (node) => dispatch({ type: 'nodeAdded', node })),
        host().listen('node-state', ({ id, state: next, choice }) => {
          dispatch({ type: 'nodeState', id, state: next, choice });
          // Os dois estados vivos: o PTY já existe, então setLive só reenvia o tamanho. Não recria nem zera o terminal.
          if (next === 'running' || next === 'awaiting-orchestrator') terminals.get(id)?.setLive();
          // Executor que terminou fecha a própria aba; a resposta dele já seguiu para o orquestrador.
          if (FINISHED.includes(next)) void clearDone();
        }),
        host().listen('executor-choice', (choice) => dispatch({ type: 'executorChoice', choice })),
        host().listen('orchestrator-exit', ({ code }) =>
          resetOrchestrator(`O orquestrador terminou (${code === null ? 'sem código de saída' : `código de saída ${code}`}).`)),
        // `exit` num shell: o backend já o encerrou, então só a aba dele sai (a última fecha o painel).
        host().listen('shell-exit', ({ id }) => dropShell(id)),
        host().onCloseRequested(async () => {
          try {
            const running = await backend.hasRunning();
            // Qualquer arquivo com rascunho protege a janela, ativo ou não.
            const unsaved = [...dirtyRef.current];
            if ((running || unsaved.length > 0) && !(await askClose(unsaved, running))) return false;
            await backend.shutdown();
          } catch { /* sem backend para responder não há o que proteger: deixa fechar */ }
          return true;
        }),
      ]);
      // Depois de escutar os eventos: o estado atual cobre o que saiu antes do registro. Se um evento chegou
      // enquanto ele vinha, o evento é mais novo e a resposta não se aplica.
      // Se a resposta falha depois de um evento novo, a falha é da resposta velha e o evento vale.
      const seen = updateEvents.current;
      try {
        const status = await backend.updateStatus();
        if (updateEvents.current === seen) dispatch({ type: 'update', status });
      } catch (e) {
        if (updateEvents.current === seen) throw e;
      }
    } catch (e) {
      setFatal(true);
      dispatch({ type: 'update', status: { phase: 'failed', message: message(e) } });
    }
  }

  useEffect(() => {
    void boot();
  }, []);

  async function retryUpdate() {
    setRetrying(true);
    const seen = updateEvents.current;
    try {
      const status = await backend.retryUpdate();
      if (updateEvents.current === seen) dispatch({ type: 'update', status });
    } catch (e) {
      if (updateEvents.current === seen) dispatch({ type: 'update', status: { phase: 'failed', message: message(e) } });
    } finally {
      setRetrying(false);
    }
  }

  async function chooseProject() {
    setProjectError('');
    setRetrying(false);
    try {
      const project = await backend.chooseProject();
      if (!project) return;
      const options = await backend.getOptions();
      dispatch({ type: 'project', project, options });
    } catch (e) {
      setProjectError(message(e));
    }
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
    const view = terminals.get(ORCHESTRATOR)!;
    await backend.startOrchestrator(choice, view.term.cols, view.term.rows);
    dispatch({ type: 'started', choice });
    view.imagePaste = choice.tool === 'codex' ? '\x16' : '\x1bv';
    view.pastePerFile = choice.tool === 'codex';
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
    dispatch({ type: 'overlay', overlay: null });
  }

  // Foco das janelas de escolha: ao fechar, a janela não devolve o foco ao botão que as abriu.
  function focusOrchestrator() {
    terminals.get(ORCHESTRATOR)?.term.focus();
  }

  async function clearDone() {
    try {
      const ids = await backend.clearDone();
      ids.forEach((id) => terminals.forget(id));
      dispatch({ type: 'removed', ids });
    } catch (e) {
      report(e);
    }
  }

  // Tira a aba do estado e descarta o terminal dela. Não fala com o backend: quem chama decide se o shell morre.
  function dropShell(id: string) {
    terminals.forget(id);
    dispatch({ type: 'shellRemove', id });
  }

  // Nova aba: id novo e ativo. O TermHost dela monta e chama startShell. `cwd` (relativo ao projeto) é a pasta em que ela abre.
  function openShellAt(cwd?: string) {
    shellSeq.current += 1;
    dispatch({ type: 'shellAdd', id: `shell-${shellSeq.current}`, cwd });
  }
  // Sem argumento: o botão "+" passa o evento de clique para `onAdd`, então esta função não recebe nada.
  const addShell = () => openShellAt();

  // X da aba: mata o shell no backend e só depois tira a aba. Se o backend recusar, a aba fica para tentar de novo.
  async function closeShell(id: string) {
    try {
      await backend.closeShell(id);
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
    try {
      const cwd = stateRef.current.shells.find((s) => s.id === id)?.cwd;
      const program = await backend.startShell(id, view.term.cols, view.term.rows, cwd);
      // A aba foi fechada enquanto o shell abria: o terminal já saiu, e o shell que acabou de nascer no backend fica sem aba. Mata de novo.
      if (!stateRef.current.shells.some((s) => s.id === id)) {
        await backend.closeShell(id).catch(() => {});
        return;
      }
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
    terminals.get(state.shellActive)?.fit();
    terminals.get(state.shellActive)?.term.focus();
  }, [state.shellOpen, state.shellActive]);

  // Ctrl+` abre e fecha o painel de qualquer lugar. A captura no window vem antes do xterm, que não recebe a tecla.
  // Sem dependências: lê o estado pelo stateRef, então o listener nunca usa uma lista de abas antiga.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey || e.metaKey || e.code !== 'Backquote') return;
      e.preventDefault();
      e.stopPropagation();
      toggleShell();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  // Ctrl+Shift+F abre a pesquisa e foca o campo, de qualquer lugar (com projeto aberto). Mesma captura no window do Ctrl+`.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || !e.shiftKey || e.altKey || e.metaKey || e.code !== 'KeyF') return;
      if (!stateRef.current.project || stateRef.current.update.phase !== 'ready') return;
      e.preventDefault();
      e.stopPropagation();
      setRoute('pesquisa');
      setSearchFocus((n) => n + 1);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  // ---------- derivados ----------

  const { update, orchestrator, executors, options } = state;
  const ready = update.phase === 'ready';
  // Título da aba: nome da pasta do projeto (último segmento do caminho) e, depois que o backend responde, o programa.
  const folder = state.project.split(/[\\/]/).filter(Boolean).pop() ?? '';
  const shellTabs = state.shells.map((s) => ({ id: s.id, title: s.program ? `${folder} — ${s.program}` : folder, program: s.program }));
  // Texto do modal: um arquivo pelo caminho; vários, pelo número e pela lista (o caminho desambigua nomes iguais).
  const unsavedText = confirmFiles.length === 1
    ? `em ${confirmFiles[0]}`
    : `em ${confirmFiles.length} arquivos: ${confirmFiles.join(', ')}`;
  // Enquanto algum executor não terminou, pull, troca e criação de branch ficam travados (o backend recusa do mesmo jeito).
  const busy = executors.some((e) => !FINISHED.includes(e.state));
  const count = (state: NodeState) => executors.filter((e) => e.state === state).length;
  const counts = (['running', 'queued', 'awaiting-choice', 'awaiting-orchestrator'] as const)
    .map((state) => [count(state), STATES[state].label] as const)
    .filter(([n]) => n);
  const changed = git.data?.files.length ?? 0;
  // As telas ficam montadas; a rota só escolhe qual aparece. Inativa vai para invisible, nunca para display none.
  const shown = (...to: Route[]) => `absolute inset-0${to.includes(route) ? '' : ' invisible'}`;
  // Ações do menu de arquivo da árvore (src/explorer.tsx). Ação sem resposta do backend avisa por toast; o que pede confirmação fica no modal.
  const fileActions: FileActions = {
    dirty: (path) => dirtyRef.current.has(path),
    copyPath: (kind, path) => void copyPath(kind, path),
    reveal: (path) => void backend.revealFile(path).catch(report),
    openWith: (path) => void backend.openWith(path).catch(report),
    openInTerminal: (dir) => openShellAt(dir),
    addToChat: orchestrator?.started ? addToChat : null,
    copy: copyFile,
    rename: renameFile,
    remove: removeFile,
    revealDir: (dir) => void backend.revealDir(dir).catch(report),
    // Localizar na pasta: a pesquisa de todo o projeto, com o campo focado (mesmo efeito do Ctrl+Shift+F).
    search: () => {
      setRoute('pesquisa');
      setSearchFocus((n) => n + 1);
    },
    paste: pasteInto,
  };

  // ---------- telas ----------

  if (!ready) {
    const retry = update.phase === 'failed';
    return (
      <Screen connected={connected} description={(
        <Text as="p" type="secondary" fontSize={14} role="status" aria-live="polite">{updateText(update, fatal)}</Text>
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
          <Button type="primary" onClick={retryUpdate} disabled={retrying} loading={retrying} className="self-start">
            Tentar novamente
          </Button>
        )}
      </Screen>
    );
  }

  if (!state.project) {
    return (
      <Screen connected={connected} description={(
        <Text as="p" type="secondary" fontSize={14}>
          Escolha a pasta do projeto (um repositório Git). O orquestrador e os executores trabalham nela.
        </Text>
      )}>
        {projectError && (
          <Alert type="error" role="alert" description={projectError} className="select-text" />
        )}
        <Button type="primary" onClick={chooseProject} className="self-start">Escolher pasta do projeto</Button>
      </Screen>
    );
  }

  return (
    <Flexbox height="100dvh">
      <TitleBar connected={connected} project={state.project}>
        {counts.map(([n, label]) => (
          <Text key={label} as="span" type="secondary" fontSize={12} noWrap ellipsis title={`${n} ${label}`} data-tauri-drag-region>
            {`${n} ${label}`}
          </Text>
        ))}
      </TitleBar>

      <Flexbox ref={rowRef} horizontal flex={1} className="min-h-0">
        <Nav route={route} changed={changed} onGo={setRoute} />
        <main className="relative min-h-0 min-w-0 flex-1">
          <div className={shown('arquivos', 'pesquisa')}>
            <Flexbox horizontal height="100%">
              {/* Árvore e pesquisa ocupam a mesma faixa: as duas montadas, a inativa só invisível (a coluna central não muda de tamanho na troca). A borda direita da faixa arrasta para mudar a largura. */}
              <DraggablePanel
                placement="left" expandable={false} showHandleWideArea={false}
                size={{ width: shownSideWidth }} defaultSize={SIDE_DEFAULT_SIZE}
                onSizeChange={(_, size) => typeof size?.width === 'number' && setSideWidth(size.width)}
                minWidth={SIDE_MIN} maxWidth={sideMaxWidth}
                backgroundColor="var(--ant-color-bg-layout)"
              >
                <div className="relative h-full">
                  <div className={shown('arquivos')}>
                    <FilesScreen openPath={openFile} onOpen={openFileAt} reveal={treeReveal} actions={fileActions} />
                  </div>
                  <div className={shown('pesquisa')}>
                    <SearchScreen active={route === 'pesquisa'} focusKey={searchFocus} onOpen={openResult} />
                  </div>
                </div>
              </DraggablePanel>
              {/* Coluna central: a área do arquivo em cima e o painel Terminal embaixo. */}
              <Flexbox ref={centerRef} height="100%" flex={1} allowShrink>
                {/* Saiu da área (foco em terminal, formulário ou fora do app): o acorde Ctrl+K pendente é cancelado, para não disparar depois. */}
                <Flexbox
                  height="100%" flex={1} className="min-h-0" onKeyDownCapture={onFileKey}
                  onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) endChord(); }}
                >
                  {files.paths.length > 0 && (
                    <FileTabs
                      paths={files.paths} active={openFile} dirty={dirtyFiles} pinned={files.pinned}
                      hasCursor={(path) => cursors.current.has(path)}
                      onSelect={selectFile} onClose={(path) => void closeFile(path)}
                      onBatch={(kind: BatchKind, path) => void closeFiles(kind, path)}
                      onTogglePin={togglePin} onCopy={(kind: CopyKind, path) => void copyPath(kind, path)}
                      onReveal={revealInTree}
                    />
                  )}
                  {/* Todos os arquivos abertos ficam montados, em camadas: o ativo aparece e recebe o teclado; os outros ficam invisíveis, com a edição e a rolagem. */}
                  <Flexbox flex={1} className="relative min-h-0">
                    {files.paths.map((path) => (
                      <div key={path} className={`absolute inset-0${path === openFile ? '' : ' invisible'}`}>
                        <FileView path={path} active={path === openFile && route !== 'git'} reveal={reveal} onDirtyChange={setFileDirty} onCursor={reportCursor} />
                      </div>
                    ))}
                  </Flexbox>
                </Flexbox>
                <ShellPanel
                  open={state.shellOpen} maxHeight={centerHeight - FILE_MIN_HEIGHT}
                  tabs={shellTabs} active={state.shellActive}
                  onSelect={(id) => dispatch({ type: 'shellSelect', id })} onAdd={addShell}
                  onHide={() => dispatch({ type: 'shellOpen', open: false })} onClose={closeShell} onReady={startShell}
                />
              </Flexbox>
            </Flexbox>
          </div>
          <div className={shown('git')}>
            <GitScreen
              git={git} locked={busy} active={route === 'git'}
              width={shownSideWidth} minWidth={SIDE_MIN} maxWidth={sideMaxWidth} onWidthChange={setSideWidth}
              onOpenFile={(path) => { openFileAt(path); setRoute('arquivos'); }}
            />
          </div>
        </main>
        {/* Painel do Orquestrador, à direita de tudo e em todas as telas: nenhuma rota o desmonta nem muda o tamanho dele. */}
        <TerminalsScreen
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
      </Flexbox>

      <StatusBar branch={git.data?.branch} terminalOpen={state.shellOpen} onToggleTerminal={toggleShell} />

      {/* As duas escolhas são janelas: a do orquestrador (início) e uma só para os executores. Com um executor aguardando escolha, a janela dele é a única e não fecha sem confirmar: nunca duas empilhadas. */}
      {options && (
        <>
          <ChoiceForm
            open={orchestrator?.overlay === 'start'}
            title="Orquestrador" hint="Só lê o projeto e delega. Você conversa com ele neste terminal."
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
      {/* Esc, clique fora e Cancelar recusam o fechamento; só o botão de confirmação fecha. */}
      {/* Borda e sombra neutras do tema. */}
      <Modal
        open={confirmOpen}
        title={confirmFiles.length > 0 && !confirmTerminals
          ? 'Descartar alterações?'
          : 'Fechar o Orquestrador?'}
        width={480}
        closable={false}
        keyboard
        maskClosable
        onCancel={() => settleClose(false)}
        style={{ border: '1px solid var(--ant-color-border)', boxShadow: 'var(--ant-box-shadow-secondary)' }}
        styles={{
          header: { padding: '24px 24px 0', minHeight: 0 },
          title: { fontSize: 16, fontWeight: 600, lineHeight: 1.4 },
          body: { padding: '8px 24px 24px' },
          footer: { padding: '16px 24px', borderTop: '1px solid var(--ant-color-border)' },
        }}
        footer={(
          <Flexbox horizontal wrap="wrap" justify="flex-end" gap={8} width="100%">
            <Button size="large" onClick={() => settleClose(false)}>Cancelar</Button>
            <Button size="large" type="fill" onClick={() => settleClose(true)}>
              {confirmFiles.length > 0 && confirmTerminals
                ? 'Descartar e encerrar todos'
                : confirmFiles.length > 0
                  ? 'Descartar'
                  : 'Fechar e encerrar todos'}
            </Button>
          </Flexbox>
        )}
      >
        <Text as="p" type="secondary" fontSize={14} lineHeight={1.6} style={{ margin: 0, overflowWrap: 'anywhere' }}>
          {confirmFiles.length > 0 && confirmTerminals
            ? `Há alterações não salvas ${unsavedText} e terminais trabalhando; fechar descarta a edição e encerra os terminais.`
            : confirmFiles.length > 0
              ? `Há alterações não salvas ${unsavedText}; descartá-las?`
              : 'Há terminais trabalhando; fechar encerra todos.'}
        </Text>
      </Modal>
    </Flexbox>
  );
}

/** Atualização e escolha de projeto: barra no topo e o cartão centralizado no espaço que sobra. */
function Screen({ connected, description, children }: { connected: boolean; description: ReactNode; children: ReactNode }) {
  return (
    <Flexbox height="100dvh">
      <TitleBar connected={connected} />
      <Center flex={1} padding={24} className="min-h-0">
        <Block variant="outlined" direction="vertical" gap={16} padding={24} className="w-full max-w-md">
          <Flexbox gap={6}>
            <Text as="h1" fontSize={16} weight={600} lineHeight={1}>Orquestrador</Text>
            {description}
          </Flexbox>
          <Flexbox gap={16}>{children}</Flexbox>
        </Block>
      </Center>
    </Flexbox>
  );
}
