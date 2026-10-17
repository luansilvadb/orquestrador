import { useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState, type ReactNode } from 'react';
import { Block, Center, Flexbox } from '@lobehub/ui';
import { Alert, Button, DraggablePanel, Modal, Progress, setActiveScope, Text, toast } from '@lobehub/ui/base-ui';
import { backend, connect, host, message } from './backend';
import { ChoiceForm } from './choice-form';
import { EditorWorkspace, type EditorWorkspaceHandle } from './editor-workspace';
import { BatchError, FilesScreen, Nav, StatusBar, type FileActions, type Route } from './explorer';
import { createGitGuard } from './git-guard';
import { GitSidebar, usePolled } from './git';
import { copy, type HistoryRequest, nextHistoryId, splitPath } from './git-history';
import { QuickOpen } from './quick-open';
import { SearchScreen } from './search';
import { SHELL_HEIGHT, SHELL_MIN, ShellPanel } from './shell-panel';
import { FINISHED, PANEL_MIN, PANEL_WIDTH, STATES, TerminalsScreen } from './terminal-screen';
import type { TermView } from './term';
import { terminals } from './terminals';
import { folderName, TitleBar } from './titlebar';
import { SECONDARY } from './text-style';
import { pathText, relativeTo } from './file-tab-actions';
import { isWithin, type TabTarget } from './workspace-model';
import {
  ORCHESTRATOR, type Choice, type ExecutorNode, type GitState, type Moved, type NodeState, type Options, type UpdateStatus,
} from './types';

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

type Overlay = 'start' | 'executor' | null;
// Um shell do painel Terminal: `program` fica '' até o backend responder com o nome do programa.
// `cwd` é a pasta (relativa ao projeto) em que o shell abre; sem ela, a raiz do projeto.
type ShellItem = { id: string; program: string; cwd?: string };
type State = {
  update: UpdateStatus;
  project: string;
  projectBusy: boolean;
  options: Options | null;
  orchestrator: { choice: Choice | null; overlay: Overlay } | null;
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
type CloseAsk = { files: string[]; terminals: boolean };

export function App() {
  const [state, dispatch] = useReducer(reducer, initial);
  const stateRef = useRef(state);
  useLayoutEffect(() => {
    stateRef.current = state;
  });
  const [route, setRoute] = useState<Route>('arquivos');
  // Área do editor central (grupos, abas, rascunhos e recuperação ficam nele). O App só pede coisas por este handle.
  const editor = useRef<EditorWorkspaceHandle>(null);
  // Trava das operações Git da UI: a barra lateral e o editor dividem a mesma.
  const [gitGuard] = useState(createGitGuard);
  // Sincronizar da barra de status em curso: o ícone gira e o botão fica parado.
  const [syncing, setSyncing] = useState(false);
  // Alvo da aba ativa do editor: a barra Git marca o diff (ou conflito) que está aberto.
  const [activeTarget, setActiveTarget] = useState<TabTarget | null>(null);
  const [activePath, setActivePath] = useState<string | null>(null);
  // Caminho com a árvore revelada (o Explorer abre as pastas e marca o arquivo). O `id` é novo a cada pedido.
  const [treeReveal, setTreeReveal] = useState<{ path: string; id: number } | null>(null);
  const treeSeq = useRef(0);
  // Pedido de histórico Git (arquivo ou projeto). Um `id` novo sempre refaz a lista.
  const [historyRequest, setHistoryRequest] = useState<HistoryRequest>({ path: null, id: 0 });
  // Pesquisa com pasta: vazio é o projeto inteiro; o `id` novo refaz o escopo mesmo se a pasta é a mesma.
  const [scope, setScope] = useState<{ path: string; id: number } | null>(null);
  const scopeSeq = useRef(0);
  // Sobe a cada Ctrl+Shift+F: a pesquisa refoca o campo mesmo se já estiver na tela.
  const [searchFocus, setSearchFocus] = useState(0);
  // Seleção do editor que vira a consulta da pesquisa (Ctrl+Shift+F com texto selecionado). O `id` é novo a cada pedido.
  const [seed, setSeed] = useState<{ text: string; id: number } | null>(null);
  const seedSeq = useRef(0);
  // Sobe a cada gravação do editor: a pesquisa refaz a busca atual para não mostrar linhas velhas.
  const [saveTick, setSaveTick] = useState(0);
  // F4 e Shift+F4: próximo ou anterior resultado da pesquisa. O `id` é novo a cada tecla.
  const [step, setStep] = useState<{ dir: 1 | -1; id: number } | null>(null);
  const stepSeq = useRef(0);
  const [quickOpen, setQuickOpen] = useState(false);
  // O status do Git roda com o projeto aberto, em qualquer tela: o contador da navegação precisa dele.
  // Status do Git, atualizado a cada 1,5 s. O estado só muda quando a resposta é diferente, para a lista não piscar. Sem projeto (`enabled` falso), não consulta.
  const git = usePolled(() => backend.gitStatus(), state.update.phase === 'ready' && !!state.project);
  const [connected, setConnected] = useState(false);
  const [fatal, setFatal] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [projectError, setProjectError] = useState('');
  // Fechamento da janela: alterações não salvas (salvar, descartar ou cancelar) e terminais trabalhando (encerrar ou cancelar).
  const [closeAsk, setCloseAsk] = useState<CloseAsk | null>(null);
  const closeAnswer = useRef<((choice: CloseChoice) => void) | null>(null);
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

  // ---------- ações ----------

  const resetOrchestrator = (notice = '') => {
    for (const e of stateRef.current.executors) terminals.forget(e.id);
    dispatch({ type: 'reset', notice });
  };

  // Com uma confirmação já na tela, o pedido novo não sobrescreve a resposta pendente: recusa, e a primeira continua valendo.
  const askClose = (files: string[] = [], terminalsRunning = false) => {
    if (closeAnswer.current) return Promise.resolve<CloseChoice>('cancel');
    return new Promise<CloseChoice>((resolve) => {
      closeAnswer.current = resolve;
      setCloseAsk({ files, terminals: terminalsRunning });
    });
  };
  const settleClose = (choice: CloseChoice) => {
    setCloseAsk(null);
    closeAnswer.current?.(choice);
    closeAnswer.current = null;
  };

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
  const targetOf = (dir: string, path: string) => {
    const name = splitPath(path).name;
    return dir ? `${dir}/${name}` : name;
  };
  // Renomear: o backend troca o nome (arquivo ou pasta) e o editor troca só o caminho das abas e dos rascunhos. Nada é recusado por rascunho.
  async function renameFile(path: string, name: string) {
    const next = await backend.renameFile(path, name);
    editor.current?.remap(path, next);
    return next;
  }
  // Excluir em lote: o backend manda à Lixeira e devolve o que foi feito. Só o feito sai das abas; uma parada no meio vira BatchError com o feito.
  async function removeMany(paths: string[]) {
    const batch = await backend.deleteFiles(paths);
    editor.current?.remove(batch.done);
    if (batch.error) throw new BatchError(batch.error, batch.done);
    return batch.done;
  }
  // Mover: confere colisão com as abas antes de qualquer mudança; depois troca o caminho só do que o backend moveu.
  async function moveItems(paths: string[], dest: string): Promise<Moved[]> {
    const clash = clashAt(paths.map((p) => targetOf(dest, p)));
    if (clash) throw clash;
    const batch = await backend.moveFiles(paths, dest);
    for (const m of batch.done) editor.current?.remap(m.from, m.to);
    if (batch.error) throw new BatchError(batch.error, batch.done);
    return batch.done;
  }
  // Colar na pasta `dir`. O clipboard é lido aqui, e essa mesma lista vai para o backend. Recorte troca o caminho do que foi movido
  // (inclusive numa parada no meio); cópia não mexe nas abas. Devolve as origens que o recorte tirou do projeto.
  async function pasteInto(dir: string): Promise<string[]> {
    const snap = await backend.clipboardSnapshot();
    if (snap.paths.length === 0) throw new Error('Não há arquivo ou pasta copiado para colar.');
    if (snap.cut) {
      const sources = snap.paths.map((p) => relativeTo(stateRef.current.project, p)).filter((p): p is string => p !== null);
      const clash = clashAt(sources.map((s) => targetOf(dir, s)));
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
    if (batch.error) throw new BatchError(batch.error, moved);
    return moved;
  }
  // Cola o caminho absoluto no rascunho do Orquestrador, sem Enter. O foco vai para o terminal, para a mensagem continuar dali.
  function addToChat(path: string) {
    const view = terminals.get(ORCHESTRATOR);
    const text = pathText('absolute', path, stateRef.current.project);
    if (!view || !text) return report(new Error('O Orquestrador não está aberto.'));
    void view.pasteFiles([text]).then(() => view.term.focus()).catch(report);
  }

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
          // O estado vivo: o PTY já existe, então setLive só reenvia o tamanho. Não recria nem zera o terminal.
          if (next === 'running') terminals.get(id)?.setLive();
          // Executor que terminou fecha a própria aba; a resposta dele já seguiu para o orquestrador.
          if (FINISHED.includes(next)) {
            terminals.forget(id);
            dispatch({ type: 'removed', ids: [id] });
          }
        }),
        host().listen('orchestrator-exit', ({ code }) => {
          resetOrchestrator(`O orquestrador terminou (${code === null ? 'sem código de saída' : `código de saída ${code}`}).`);
        }),
        // `exit` num shell: o backend já o encerrou, então só a aba dele sai (a última fecha o painel).
        host().listen('shell-exit', ({ id }) => dropShell(id)),
        host().onCloseRequested(async () => {
          try {
            const running = await backend.hasRunning();
            const unsaved = editor.current?.unsaved() ?? [];
            if (unsaved.length > 0) {
              // Três respostas: salvar tudo (só fecha se tudo gravou e nada novo ficou sujo), descartar (a recuperação é gravada
              // antes do encerramento), ou cancelar, que mantém a janela e tudo como está.
              const choice = await askClose(unsaved, running);
              if (choice === 'cancel') return false;
              if (choice === 'save') {
                const saved = await editor.current!.saveAll();
                if (!saved || editor.current!.unsaved().length > 0) {
                  report(new Error('Não foi possível salvar tudo. Nada foi fechado.'));
                  return false;
                }
              } else {
                editor.current!.discardAll();
              }
              editor.current?.flushRecovery();
            } else if (running && (await askClose([], true)) !== 'close') {
              return false;
            }
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
      const picked = await backend.chooseProject();
      if (!picked) return;
      const options = await backend.getOptions();
      dispatch({ type: 'project', project: picked.project, options });
      // Pasta dentro de um repositório: o projeto aberto é a raiz dele. Aviso informativo, não erro.
      if (picked.subfolder) toast.info({ title: `Esta pasta está dentro do repositório ${picked.project}. O projeto aberto é a raiz dele.`, duration: 6000 });
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
    terminals.get(ORCHESTRATOR)?.term.focus();
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

  // Atalhos de projeto (pesquisa, F4, arquivo rápido) valem só com o projeto aberto e o app pronto.
  const canShortcut = () => !!stateRef.current.project && stateRef.current.update.phase === 'ready';

  // Atalhos de janela num listener só, no window em captura (antes do xterm, que não recebe a tecla). Chama sempre o da última renderização.
  // Cada tecla tem a sua condição e são excludentes; a que casa consome o evento.
  const windowKey = useRef<(e: KeyboardEvent) => void>(() => {});
  useLayoutEffect(() => {
    windowKey.current = (e) => {
      const target = e.target as HTMLElement | null;
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
        if (text) setSeed({ text, id: ++seedSeq.current });
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
        setStep({ dir: e.shiftKey ? -1 : 1, id: ++stepSeq.current });
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
    const listener = (e: KeyboardEvent) => windowKey.current(e);
    window.addEventListener('keydown', listener, true);
    return () => window.removeEventListener('keydown', listener, true);
  }, []);

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
      if (now.kind === 'repo' && now.status.ahead > 0) await backend.gitPush();
      toast.success({ title: 'Alterações sincronizadas', duration: 3000 });
    } catch (e) {
      report(e);
    } finally {
      gitGuard.leave();
      setSyncing(false);
      void git.refresh();
    }
  }

  // ---------- derivados ----------

  const { update, orchestrator, executors, options } = state;
  const ready = update.phase === 'ready';
  // Título da aba: nome da pasta do projeto (último segmento do caminho) e, depois que o backend responde, o programa.
  const folder = folderName(state.project);
  const shellTabs = state.shells.map((s) => ({ id: s.id, title: s.program ? `${folder} — ${s.program}` : folder, program: s.program }));
  // Enquanto algum executor não terminou, pull, troca e criação de branch ficam travados (o backend recusa do mesmo jeito).
  const busy = executors.some((e) => !FINISHED.includes(e.state));
  const count = (state: NodeState) => executors.filter((e) => e.state === state).length;
  const counts = (['running', 'queued', 'awaiting-choice'] as const)
    .map((state) => [count(state), STATES[state].label] as const)
    .filter(([n]) => n);
  // Contador da navegação, branch e Histórico Git só existem no modo `repo`.
  const changed = git.data?.kind === 'repo' ? git.data.status.files.length : 0;
  const repoStatus = git.data?.kind === 'repo' ? git.data.status : undefined;
  const noGitFix = git.data && git.data.kind !== 'repo' ? NO_GIT_FIX[git.data.kind] : null;
  // Texto do fechamento: a lista de arquivos (o caminho desambigua nomes iguais) e a menção aos terminais, quando há.
  const closeFiles = closeAsk?.files ?? [];
  const closeUnsaved = closeFiles.length === 1
    ? `em ${closeFiles[0]}`
    : `em ${closeFiles.length} arquivos: ${closeFiles.join(', ')}`;
  // Ações do menu de arquivo da árvore (src/explorer.tsx). Ação sem resposta do backend avisa por toast; o que pede confirmação fica no modal.
  const hasDraft = (path: string) => editor.current?.hasDraft(path) ?? false;
  const fileActions: FileActions = {
    dirtyOf: (paths) => paths.filter(hasDraft),
    copyPaths,
    reveal: (path) => void backend.revealFile(path).catch(report),
    openWith: (path) => void backend.openWith(path).catch(report),
    openInTerminal: (dir) => openShellAt(dir),
    addToChat: orchestrator?.choice ? addToChat : null,
    // Copiar e recortar põem os itens no clipboard do Windows: colar é no Explorer. Recortar não move nada agora.
    copyMany: (paths, cut) => {
      backend.copyFiles(paths, cut).then(
        () => toast.success({ title: cut ? `${paths.length} itens recortados. Cole no Explorer para mover.` : `${paths.length} itens copiados. Cole no Explorer.`, duration: 4000 }),
        report,
      );
    },
    rename: renameFile,
    removeMany,
    move: moveItems,
    revealDir: (dir) => void backend.revealDir(dir).catch(report),
    history: git.data?.kind === 'repo' ? (path) => {
      setHistoryRequest({ path, id: nextHistoryId() });
      setRoute('git');
    } : null,
    // Localizar na pasta: a pasta vai para o campo de incluir da pesquisa (vazio é o projeto inteiro), com o campo focado.
    search: (dir = '') => {
      setScope({ path: dir, id: ++scopeSeq.current });
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
        <Text as="p" fontSize={14} style={SECONDARY}>
          Escolha a pasta do projeto. O orquestrador e os executores trabalham nela.
        </Text>
      )}>
        {projectError && (
          <Alert type="error" role="alert" description={projectError} className="select-text" />
        )}
        <Button type="primary" onClick={chooseProject} className="self-start">Escolher pasta do projeto</Button>
      </Screen>
    );
  }

  // Este é o layout de uma tela só: a faixa da esquerda muda de conteúdo (Arquivos, Pesquisa, Git) e a área central fica montada.
  // A faixa é a mesma nas três rotas: a largura é uma só.
  const shown = (to: Route) => `absolute inset-0${route === to ? '' : ' invisible'}`;
  return (
    <Flexbox height="100dvh">
      <TitleBar connected={connected} project={state.project}>
        {counts.map(([n, label]) => (
          <Text key={label} as="span" fontSize={12} style={SECONDARY} noWrap ellipsis title={`${n} ${label}`} data-tauri-drag-region>
            {`${n} ${label}`}
          </Text>
        ))}
      </TitleBar>

      <Flexbox ref={rowRef} horizontal flex={1} className="min-h-0">
        <Nav route={route} changed={changed} onGo={setRoute} />
        <main className="relative min-h-0 min-w-0 flex-1">
          <Flexbox horizontal height="100%">
            {/* Faixa da esquerda: árvore, pesquisa ou Git, todas montadas; só a da rota aparece. A borda direita arrasta a largura. */}
            <DraggablePanel
              placement="left" expandable={false} showHandleWideArea={false}
              size={{ width: shownSideWidth }} defaultSize={SIDE_DEFAULT_SIZE}
              onSizeChange={(_, size) => typeof size?.width === 'number' && setSideWidth(size.width)}
              minWidth={SIDE_MIN} maxWidth={sideMaxWidth}
              backgroundColor="var(--ant-color-bg-layout)"
            >
              <div className="relative h-full">
                <div className={shown('arquivos')}>
                  <FilesScreen openPath={activePath} onOpen={(path, options) => (options?.side ? editor.current?.openBeside(path, { permanent: options.permanent }) : editor.current?.openFile(path, { permanent: options?.permanent }))} reveal={treeReveal} actions={fileActions}
                    project={state.project.split(/[\\/]/).filter(Boolean).pop() ?? state.project} git={git.data?.kind === 'repo' ? git.data.status : null}
                  />
                </div>
                <div className={shown('pesquisa')}>
                  <SearchScreen
                    active={route === 'pesquisa'} focusKey={searchFocus} folder={scope} seed={seed} refreshKey={saveTick} step={step} hasDraft={hasDraft}
                    openPaths={() => editor.current?.openPaths() ?? []}
                    onOpen={(path, line, col, options) => editor.current?.openFile(path, { permanent: options?.permanent, line, col })}
                    onChanged={() => { git.refresh(); editor.current?.refresh(); }}
                  />
                </div>
                <div className={shown('git')}>
                  <GitSidebar
                    git={git} locked={busy} guard={gitGuard} active={route === 'git'} selected={activeTarget} historyRequest={historyRequest}
                    onOpenTarget={(target, options) => editor.current?.openTarget(target, options)}
                    onOpenFile={(path, options) => editor.current?.openFile(path, options)}
                  />
                </div>
              </div>
            </DraggablePanel>
            {/* Coluna central: o editor (grupos, abas, diffs e conflitos) em cima e o painel Terminal embaixo. Sempre montada. */}
            <Flexbox ref={centerRef} height="100%" flex={1} allowShrink>
              <div className="relative min-h-0 flex-1">
                <div className="absolute inset-0">
                  <Flexbox height="100%" flex={1} className="min-h-0">
                    {/* key por projeto: outro projeto remonta o editor (lê a recuperação dele); o anterior grava a sua ao sair. */}
                    <EditorWorkspace
                      key={state.project} ref={editor} project={state.project} locked={busy} guard={gitGuard}
                      refreshGit={() => git.refresh()} onSaved={() => setSaveTick((n) => n + 1)}
                      onActivePath={setActivePath} onActiveTarget={setActiveTarget}
                      onGroupCount={setGroupCount}
                      layout={layoutText}
                      onRestoreLayout={(text) => {
                        const saved = readLayout(text);
                        if (!saved) return;
                        if (saved.sideWidth !== undefined) setSideWidth(saved.sideWidth);
                        if (saved.panelWidth !== undefined) setPanelWidth(saved.panelWidth);
                        if (saved.shellHeight !== undefined) setShellHeight(saved.shellHeight);
                      }}
                      onReveal={(path) => { setTreeReveal({ path, id: ++treeSeq.current }); setRoute('arquivos'); }}
                    />
                  </Flexbox>
                </div>
              </div>
              <ShellPanel
                open={state.shellOpen} maxHeight={centerHeight - FILE_MIN_HEIGHT}
                height={shellHeight} onHeightChange={setShellHeight}
                tabs={shellTabs} active={state.shellActive}
                onSelect={(id) => dispatch({ type: 'shellSelect', id })} onAdd={addShell}
                onHide={() => dispatch({ type: 'shellOpen', open: false })} onClose={closeShell} onReady={startShell}
              />
            </Flexbox>
          </Flexbox>
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
          onOpen={(path, choice) => {
            setQuickOpen(false);
            if (choice.side) editor.current?.openBeside(path, { permanent: choice.permanent });
            else editor.current?.openFile(path, { permanent: choice.permanent });
          }}
        />
      )}
      {/* Esc, clique fora e Cancelar recusam o fechamento; só o botão de confirmação fecha. */}
      {/* Borda e sombra neutras do tema. */}
      <Modal
        open={closeAsk !== null}
        title={closeFiles.length > 0 ? 'Fechar com alterações não salvas?' : 'Fechar o Orquestrador?'}
        width={480}
        closable={false}
        keyboard
        maskClosable
        onCancel={() => settleClose('cancel')}
        style={{ border: '1px solid var(--ant-color-border)', boxShadow: 'var(--ant-box-shadow-secondary)' }}
        styles={{
          header: { padding: '24px 24px 0', minHeight: 0 },
          title: { fontSize: 16, fontWeight: 600, lineHeight: 1.4 },
          body: { padding: '8px 24px 24px' },
          footer: { padding: '16px 24px', borderTop: '1px solid var(--ant-color-border)' },
        }}
        footer={(
          <Flexbox horizontal wrap="wrap" justify="flex-end" gap={8} width="100%">
            <Button size="large" onClick={() => settleClose('cancel')}>Cancelar</Button>
            {closeFiles.length > 0 ? (
              <>
                <Button size="large" danger onClick={() => settleClose('discard')}>
                  {closeAsk?.terminals ? 'Descartar e encerrar todos' : 'Descartar'}
                </Button>
                <Button size="large" type="fill" onClick={() => settleClose('save')}>
                  {closeAsk?.terminals ? 'Salvar e encerrar todos' : 'Salvar'}
                </Button>
              </>
            ) : (
              <Button size="large" type="fill" onClick={() => settleClose('close')}>Fechar e encerrar todos</Button>
            )}
          </Flexbox>
        )}
      >
        <Text as="p" fontSize={14} lineHeight={1.6} style={{ ...SECONDARY, margin: 0, overflowWrap: 'anywhere' }}>
          {closeFiles.length > 0
            ? `Há alterações não salvas ${closeUnsaved}. Salvar grava tudo antes de fechar; descartar joga a edição fora.${closeAsk?.terminals ? ' Os terminais trabalhando também serão encerrados.' : ''}`
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
