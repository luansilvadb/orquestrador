import { useEffect, useLayoutEffect, useReducer, useRef, useState, type ReactNode } from 'react';
import { toast } from 'sonner';
import { LoaderCircle } from 'lucide-react';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Progress } from '@/components/ui/progress';
import { Toaster } from '@/components/ui/sonner';
import { cn } from 'cn';
import { backend, connect, message } from './backend';
import { ChoiceForm } from './choice-form';
import { FilesScreen, Nav, StatusBar } from './explorer';
import { GitScreen, usePolled } from './git';
import { useRoute, type Route } from './route';
import { FINISHED, STATES, TerminalsScreen } from './terminal-screen';
import { terminals } from './terminals';
import {
  describeChoice, ORCHESTRATOR, type Choice, type ExecutorNode, type NodeState, type Options, type UpdateStatus,
} from './types';

const MB = 1024 * 1024;

type Overlay = 'start' | 'executor' | null;
type State = {
  update: UpdateStatus;
  project: string;
  projectBusy: boolean;
  options: Options | null;
  orchestrator: { title: string; started: boolean; overlay: Overlay } | null;
  executorChoice: Choice | null;
  executors: ExecutorNode[];
  notice: string;
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
  | { type: 'reset'; notice: string };

const initial: State = {
  update: { phase: 'checking' }, project: '', projectBusy: false, options: null, orchestrator: null,
  executorChoice: null, executors: [], notice: '',
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
        orchestrator: { title: '', started: false, overlay: 'start' },
      };
    case 'started':
      return { ...state, orchestrator: state.orchestrator && { ...state.orchestrator, title: describeChoice(action.choice), started: true, overlay: null } };
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

const report = (e: unknown) => toast.error(message(e), { duration: 7000 });

export function App() {
  const [state, dispatch] = useReducer(reducer, initial);
  const stateRef = useRef(state);
  useLayoutEffect(() => {
    stateRef.current = state;
  });
  const route = useRoute();
  // O status do Git roda com o projeto aberto, em qualquer tela: o contador da navegação precisa dele.
  // Status do Git, atualizado a cada 1,5 s. O estado só muda quando a resposta é diferente, para a lista não piscar. Sem projeto (`enabled` falso), não consulta.
  const git = usePolled(() => backend.gitStatus(), state.update.phase === 'ready' && !!state.project, true);
  const [fatal, setFatal] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [projectError, setProjectError] = useState('');
  const [confirmOpen, setConfirmOpen] = useState(false);
  const closeAnswer = useRef<((ok: boolean) => void) | null>(null);
  const updateEvents = useRef(0);
  const [maxText, setMaxText] = useState('');
  const maxParallel = state.options?.maxParallel;
  useEffect(() => setMaxText(maxParallel === undefined ? '' : String(maxParallel)), [maxParallel]);

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
      await Promise.all([
        backend.listen('update-status', (status) => {
          updateEvents.current++;
          dispatch({ type: 'update', status });
        }),
        backend.listen('term-data', (data) => terminals.push(data)),
        backend.listen('node-added', (node) => dispatch({ type: 'nodeAdded', node })),
        backend.listen('node-state', ({ id, state: next, choice }) => {
          dispatch({ type: 'nodeState', id, state: next, choice });
          // Os dois estados vivos: o PTY já existe, então setLive só reenvia o tamanho. Não recria nem zera o terminal.
          if (next === 'running' || next === 'awaiting-orchestrator') terminals.get(id)?.setLive();
          // Executor que terminou fecha a própria aba; a resposta dele já seguiu para o orquestrador.
          if (FINISHED.includes(next)) void clearDone();
        }),
        backend.listen('executor-choice', (choice) => dispatch({ type: 'executorChoice', choice })),
        backend.listen('orchestrator-exit', ({ code }) =>
          resetOrchestrator(`O orquestrador terminou (${code === null ? 'sem código de saída' : `código de saída ${code}`}).`)),
        backend.onCloseRequested(async () => {
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

  // Foco das janelas de escolha: ao fechar, o Dialog não devolve o foco ao botão que as abriu.
  function focusOrchestrator() {
    terminals.get(ORCHESTRATOR)?.term.focus();
  }

  async function commitMax() {
    const current = stateRef.current.options!.maxParallel;
    const value = Math.min(16, Math.max(1, Math.round(Number(maxText)) || current));
    try {
      await backend.setMaxParallel(value);
      dispatch({ type: 'maxParallel', value });
      setMaxText(String(value));
    } catch (e) {
      report(e);
      setMaxText(String(current));
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

  // ---------- derivados ----------

  const { update, orchestrator, executors, options } = state;
  const ready = update.phase === 'ready';
  // Enquanto algum executor não terminou, pull, troca e criação de branch ficam travados (o backend recusa do mesmo jeito).
  const busy = executors.some((e) => !FINISHED.includes(e.state));
  const count = (state: NodeState) => executors.filter((e) => e.state === state).length;
  const counts = (['running', 'queued', 'awaiting-choice', 'awaiting-orchestrator'] as const)
    .map((state) => [count(state), STATES[state].label] as const)
    .filter(([n]) => n)
    .map(([n, label]) => `${n} ${label}`)
    .join(' · ');
  const changed = git.data?.files.length ?? 0;
  // As duas telas ficam montadas; a rota só escolhe qual aparece. Inativa vai para invisible, nunca para display none.
  const shown = (to: Route) => cn('absolute inset-0', route !== to && 'invisible');

  // ---------- telas ----------

  if (!ready) {
    const retry = update.phase === 'failed';
    return (
      <Screen>
        <Card className="w-full max-w-md">
          <CardHeader>
            <CardTitle>Orquestrador</CardTitle>
            <CardDescription role="status" aria-live="polite">{updateText(update, fatal)}</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4">
            {update.phase === 'downloading' && (
              // Total desconhecido (ou zero): value null é o indeterminado do Radix; a classe mostra a barra cheia pulsando.
              <Progress
                aria-label="Progresso do download da atualização"
                value={update.total ? Math.min(100, (update.downloaded / update.total) * 100) : null}
                className="data-[state=indeterminate]:[&>[data-slot=progress-indicator]]:!transform-none data-[state=indeterminate]:[&>[data-slot=progress-indicator]]:animate-pulse"
              />
            )}
            {update.phase === 'failed' && (
              <Alert variant="destructive" role="alert">
                <AlertDescription className="select-text">{update.message}</AlertDescription>
              </Alert>
            )}
            {retry && (
              <Button onClick={retryUpdate} disabled={retrying} className="justify-self-start">
                {retrying && <LoaderCircle className="animate-spin" />}
                Tentar novamente
              </Button>
            )}
          </CardContent>
        </Card>
      </Screen>
    );
  }

  if (!state.project) {
    return (
      <Screen>
        <Card className="w-full max-w-md">
          <CardHeader>
            <CardTitle>Orquestrador</CardTitle>
            <CardDescription>
              Escolha a pasta do projeto (um repositório Git). O orquestrador e os executores trabalham nela.
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4">
            {projectError && (
              <Alert variant="destructive" role="alert">
                <AlertDescription className="select-text">{projectError}</AlertDescription>
              </Alert>
            )}
            <Button onClick={chooseProject} className="justify-self-start">Escolher pasta do projeto</Button>
          </CardContent>
        </Card>
        <Toaster position="bottom-right" />
      </Screen>
    );
  }

  return (
    <div className="flex h-dvh flex-col">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b bg-card px-3 py-2">
        <span className="text-sm font-semibold">Orquestrador</span>
        <code title="Pasta do projeto" className="max-w-[360px] truncate rounded-md border bg-muted px-2 py-0.5 text-xs select-text">
          {state.project}
        </code>
        <span className="text-xs text-muted-foreground">{counts}</span>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Label className="text-xs text-muted-foreground" title="Quantos executores rodam ao mesmo tempo (1 a 16); os demais ficam na fila">
            simultâneos
            <Input
              type="number" min={1} max={16} step={1} inputMode="numeric"
              value={maxText}
              onChange={(e) => setMaxText(e.target.value)}
              onBlur={commitMax}
              onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur(); }}
              className="h-8 w-16 font-mono"
            />
          </Label>
        </div>
      </header>

      <div className="flex min-h-0 flex-1">
        <Nav route={route} changed={changed} />
        <main className="relative min-h-0 min-w-0 flex-1">
          <div className={shown('arquivos')}>
            <div className="flex h-full">
              <div className="h-full w-[280px] shrink-0 border-r bg-background">
                <FilesScreen />
              </div>
              <div className="h-full min-w-0 flex-1">
                <TerminalsScreen
                  orchestrator={orchestrator}
                  executors={executors}
                  executorChoice={state.executorChoice}
                  notice={state.notice}
                  onOpen={openOrchestrator}
                  onExecutorChoice={toggleExecutorChoice}
                  onStop={(id) => backend.stopExecutor(id).catch(report)}
                />
              </div>
            </div>
          </div>
          <div className={shown('git')}>
            <GitScreen git={git} locked={busy} active={route === 'git'} />
          </div>
        </main>
      </div>

      <StatusBar branch={git.data?.branch} />

      <Toaster position="bottom-right" />
      {/* As três escolhas são janelas: a do orquestrador (início e executores) e uma só para os executores aguardando. */}
      {options && (
        <>
          <ChoiceForm
            open={orchestrator?.overlay === 'start'}
            title="Orquestrador" hint="Só lê o projeto e delega. É o único terminal onde você digita."
            options={options} initial={options.last.orchestrator} confirmLabel="Abrir terminal"
            onCancel={() => resetOrchestrator()} onConfirm={startOrchestrator} onClosed={focusOrchestrator}
          />
          {/* Com um executor aguardando escolha, só a janela dele fica aberta: nunca duas empilhadas. */}
          <ChoiceForm
            open={orchestrator?.overlay === 'executor' && count('awaiting-choice') === 0}
            title="Executores"
            hint="Ferramenta, modelo e esforço dos próximos executores. Os que já estão rodando não mudam."
            options={options} initial={state.executorChoice ?? options.last.executor} confirmLabel="Usar nos executores"
            onCancel={() => dispatch({ type: 'overlay', overlay: null })}
            onConfirm={(choice) => saveExecutorChoice(choice)} onClosed={focusOrchestrator}
          />
          <ChoiceForm
            open={count('awaiting-choice') > 0}
            title="Escolha dos executores"
            hint="Vale para este e para os próximos executores enquanto o orquestrador estiver aberto."
            options={options} initial={state.executorChoice ?? options.last.executor} confirmLabel="Usar nos executores"
            onConfirm={(choice) => saveExecutorChoice(choice)} onClosed={focusOrchestrator}
          />
        </>
      )}
      <AlertDialog open={confirmOpen} onOpenChange={(open) => { if (!open) settleClose(false); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Fechar o Orquestrador?</AlertDialogTitle>
            <AlertDialogDescription>Há terminais trabalhando; fechar encerra todos.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancelar</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => settleClose(true)}>Fechar e encerrar todos</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/** Tela cheia centralizada para atualização e início. */
function Screen({ children }: { children: ReactNode }) {
  return <div className="grid h-dvh place-items-center p-6">{children}</div>;
}
