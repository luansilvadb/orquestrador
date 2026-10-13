import { useEffect, useLayoutEffect, useReducer, useRef, useState, type ReactNode } from 'react';
import { Block, Center, Flexbox } from '@lobehub/ui';
import { Alert, Button, InputNumber, Modal, Progress, Text, toast } from '@lobehub/ui/base-ui';
import { backend, connect, host, message } from './backend';
import { ChoiceForm } from './choice-form';
import { FilesScreen, Nav, StatusBar, type Route } from './explorer';
import { GitScreen, usePolled } from './git';
import { SearchScreen } from './search';
import { FINISHED, STATES, TerminalsScreen } from './terminal-screen';
import type { TermView } from './term';
import { terminals } from './terminals';
import { TitleBar } from './titlebar';
import {
  ORCHESTRATOR, type Choice, type ExecutorNode, type NodeState, type Options, type UpdateStatus,
} from './types';

const MB = 1024 * 1024;

type Overlay = 'start' | 'executor' | null;
// Um shell do painel Terminal: `program` fica '' até o backend responder com o nome do programa.
type ShellItem = { id: string; program: string };
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
  | { type: 'maxParallel'; value: number }
  | { type: 'open'; options: Options }
  | { type: 'started'; choice: Choice }
  | { type: 'overlay'; overlay: Overlay }
  | { type: 'executorChoice'; choice: Choice }
  | { type: 'nodeAdded'; node: ExecutorNode }
  | { type: 'nodeState'; id: string; state: NodeState; choice: Choice | null }
  | { type: 'removed'; ids: string[] }
  | { type: 'reset'; notice: string }
  | { type: 'shellAdd'; id: string }
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
    case 'maxParallel':
      return { ...state, options: state.options && { ...state.options, maxParallel: action.value } };
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
      return { ...state, shells: [...state.shells, { id: action.id, program: '' }], shellActive: action.id, shellOpen: true };
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

export function App() {
  const [state, dispatch] = useReducer(reducer, initial);
  const stateRef = useRef(state);
  useLayoutEffect(() => {
    stateRef.current = state;
  });
  const [route, setRoute] = useState<Route>('arquivos');
  // Sobe a cada Ctrl+Shift+F: a pesquisa refoca o campo mesmo se já estiver na tela.
  const [searchFocus, setSearchFocus] = useState(0);
  // O status do Git roda com o projeto aberto, em qualquer tela: o contador da navegação precisa dele.
  // Status do Git, atualizado a cada 1,5 s. O estado só muda quando a resposta é diferente, para a lista não piscar. Sem projeto (`enabled` falso), não consulta.
  const git = usePolled(() => backend.gitStatus(), state.update.phase === 'ready' && !!state.project, true);
  const [connected, setConnected] = useState(false);
  const [fatal, setFatal] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [projectError, setProjectError] = useState('');
  const [confirmOpen, setConfirmOpen] = useState(false);
  const closeAnswer = useRef<((ok: boolean) => void) | null>(null);
  const updateEvents = useRef(0);
  const [maxValue, setMaxValue] = useState<number | null>(null);
  const maxParallel = state.options?.maxParallel;
  useEffect(() => setMaxValue(maxParallel ?? null), [maxParallel]);
  // Contador dos shells do painel Terminal: só cresce, então um id nunca se repete enquanto o app está aberto.
  const shellSeq = useRef(0);

  // ---------- ações ----------

  const resetOrchestrator = (notice = '') => {
    for (const e of stateRef.current.executors) terminals.forget(e.id);
    dispatch({ type: 'reset', notice });
  };

  const askClose = () => new Promise<boolean>((resolve) => {
    closeAnswer.current = resolve;
    setConfirmOpen(true);
  });
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
            if (await backend.hasRunning() && !(await askClose())) return false;
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

  async function commitMax() {
    const current = stateRef.current.options!.maxParallel;
    const value = Math.min(16, Math.max(1, Math.round(maxValue ?? 0) || current));
    try {
      await backend.setMaxParallel(value);
      dispatch({ type: 'maxParallel', value });
      setMaxValue(value);
    } catch (e) {
      report(e);
      setMaxValue(current);
    }
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

  // Nova aba: id novo e ativo. O TermHost dela monta e chama startShell.
  function addShell() {
    shellSeq.current += 1;
    dispatch({ type: 'shellAdd', id: `shell-${shellSeq.current}` });
  }

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
      const program = await backend.startShell(id, view.term.cols, view.term.rows);
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
  const shellTabs = state.shells.map((s) => ({ id: s.id, title: s.program ? `${folder} — ${s.program}` : folder }));
  // Enquanto algum executor não terminou, pull, troca e criação de branch ficam travados (o backend recusa do mesmo jeito).
  const busy = executors.some((e) => !FINISHED.includes(e.state));
  const count = (state: NodeState) => executors.filter((e) => e.state === state).length;
  const counts = (['running', 'queued', 'awaiting-choice', 'awaiting-orchestrator'] as const)
    .map((state) => [count(state), STATES[state].label] as const)
    .filter(([n]) => n)
    .map(([n, label]) => `${n} ${label}`)
    .join(' · ');
  const changed = git.data?.files.length ?? 0;
  // As telas ficam montadas; a rota só escolhe qual aparece. Inativa vai para invisible, nunca para display none.
  const shown = (...to: Route[]) => `absolute inset-0${to.includes(route) ? '' : ' invisible'}`;

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
      <TitleBar connected={connected}>
        <Text as="code" code fontSize={12} ellipsis title="Pasta do projeto" className="min-w-0 rounded-md border bg-muted px-2 py-0.5 select-text">
          {state.project}
        </Text>
        <Text as="span" type="secondary" fontSize={12} noWrap data-tauri-drag-region>{counts}</Text>
        <Flexbox horizontal align="center" className="ml-auto">
          <Flexbox
            as="label" horizontal align="center" gap={8}
            title="Quantos executores rodam ao mesmo tempo (1 a 16); os demais ficam na fila"
          >
            <Text as="span" type="secondary" fontSize={12} noWrap>simultâneos</Text>
            {/* O blur do wrapper confirma (o foco sai do campo); o Enter tira o foco. Valor vazio vira null. */}
            <InputNumber
              size="small" min={1} max={16} step={1}
              value={maxValue}
              onChange={(value) => setMaxValue(value)}
              onBlur={commitMax}
              onPressEnter={(e) => (e.target as HTMLInputElement).blur()}
              controls={false}
              className="w-16! font-mono!"
            />
          </Flexbox>
        </Flexbox>
      </TitleBar>

      <Flexbox horizontal flex={1} className="min-h-0">
        <Nav route={route} changed={changed} onGo={setRoute} />
        <main className="relative min-h-0 min-w-0 flex-1">
          <div className={shown('arquivos', 'pesquisa')}>
            <Flexbox horizontal height="100%">
              {/* Árvore e pesquisa ocupam a mesma faixa: as duas montadas, a inativa só invisível (os terminais não mudam de tamanho). */}
              <div className="relative h-full w-[280px] shrink-0 border-r bg-background">
                <div className={shown('arquivos')}>
                  <FilesScreen />
                </div>
                <div className={shown('pesquisa')}>
                  <SearchScreen active={route === 'pesquisa'} focusKey={searchFocus} />
                </div>
              </div>
              <Flexbox height="100%" flex={1} allowShrink>
                <TerminalsScreen
                  orchestrator={orchestrator}
                  executors={executors}
                  executorChoice={state.executorChoice}
                  notice={state.notice}
                  shellOpen={state.shellOpen}
                  shellTabs={shellTabs}
                  shellActive={state.shellActive}
                  onOpen={openOrchestrator}
                  onExecutorChoice={toggleExecutorChoice}
                  onStop={(id) => backend.stopExecutor(id).catch(report)}
                  onShellSelect={(id) => dispatch({ type: 'shellSelect', id })}
                  onShellAdd={addShell}
                  onShellHide={() => dispatch({ type: 'shellOpen', open: false })}
                  onShellClose={closeShell}
                  onShellReady={startShell}
                />
              </Flexbox>
            </Flexbox>
          </div>
          <div className={shown('git')}>
            <GitScreen git={git} locked={busy} active={route === 'git'} />
          </div>
        </main>
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
      {/* Esc, clique fora e Cancelar recusam o fechamento; só o botão de perigo confirma. */}
      <Modal
        open={confirmOpen}
        title="Fechar o Orquestrador?"
        closable={false}
        keyboard
        maskClosable
        onCancel={() => settleClose(false)}
        footer={(
          <Flexbox horizontal justify="flex-end" gap={8}>
            <Button onClick={() => settleClose(false)}>Cancelar</Button>
            <Button type="primary" danger onClick={() => settleClose(true)}>Fechar e encerrar todos</Button>
          </Flexbox>
        )}
      >
        <Text as="p" type="secondary" fontSize={14}>Há terminais trabalhando; fechar encerra todos.</Text>
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
