import { useEffect, useLayoutEffect, useReducer, useRef, useState, type ReactNode } from 'react';
import { toast } from 'sonner';
import { CircleCheck, CircleHelp, CircleX, Clock, LoaderCircle, Maximize, Square, ZoomIn, ZoomOut } from 'lucide-react';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Progress } from '@/components/ui/progress';
import { Toaster } from '@/components/ui/sonner';
import { backend, connect, message } from './backend';
import { Block, Canvas, freeSlot, moved, resized, type CanvasHandle, type Rect } from './canvas';
import { ChoiceForm } from './choice-form';
import { attachExecutor, TermHost, terminals } from './terminals';
import {
  describeChoice, ORCHESTRATOR, type Choice, type ExecutorNode, type NodeState, type Options, type UpdateStatus,
} from './types';

const STATES: Record<NodeState, { label: string; icon: typeof Clock; variant: 'outline' | 'secondary' | 'destructive' | 'default' }> = {
  'awaiting-choice': { label: 'aguardando escolha', icon: CircleHelp, variant: 'outline' },
  queued: { label: 'na fila', icon: Clock, variant: 'outline' },
  running: { label: 'executando', icon: LoaderCircle, variant: 'default' },
  done: { label: 'concluído', icon: CircleCheck, variant: 'secondary' },
  stopped: { label: 'interrompido', icon: Square, variant: 'outline' },
  failed: { label: 'falhou', icon: CircleX, variant: 'destructive' },
};
const FINISHED: NodeState[] = ['done', 'stopped', 'failed'];
const MB = 1024 * 1024;
const EXECUTOR_BLOCK = { w: 660, h: 380 };
const ORCHESTRATOR_RECT: Rect = { x: 0, y: 0, w: 800, h: 520 };

type Overlay = 'start' | 'executor' | null;
type State = {
  update: UpdateStatus;
  project: string;
  projectBusy: boolean;
  options: Options | null;
  orchestrator: { title: string; started: boolean; overlay: Overlay } | null;
  executorChoice: Choice | null;
  executors: ExecutorNode[];
  layout: Record<string, Rect>;
  notice: string;
  fit: { seq: number; reveal?: string };
  zoom: number;
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
  | { type: 'move'; id: string; dx: number; dy: number; z: number }
  | { type: 'resize'; id: string; dx: number; dy: number; z: number }
  | { type: 'zoom'; zoom: number }
  | { type: 'fit' };

const initial: State = {
  update: { phase: 'checking' }, project: '', projectBusy: false, options: null, orchestrator: null,
  executorChoice: null, executors: [], layout: {}, notice: '', fit: { seq: 0 }, zoom: 1,
};

function reducer(state: State, action: Action): State {
  const fit = (reveal?: string) => ({ seq: state.fit.seq + 1, reveal });
  switch (action.type) {
    case 'update':
      return { ...state, update: action.status };
    case 'project':
      return { ...state, project: action.project, options: action.options, projectBusy: false, fit: fit() };
    case 'options':
      return { ...state, options: action.options };
    case 'maxParallel':
      return { ...state, options: state.options && { ...state.options, maxParallel: action.value } };
    case 'open':
      return {
        ...state, options: action.options, notice: '', fit: fit(),
        orchestrator: { title: '', started: false, overlay: 'start' },
        layout: { ...state.layout, [ORCHESTRATOR]: ORCHESTRATOR_RECT },
      };
    case 'started':
      return { ...state, orchestrator: state.orchestrator && { ...state.orchestrator, title: describeChoice(action.choice), started: true, overlay: null } };
    case 'overlay':
      return { ...state, orchestrator: state.orchestrator && { ...state.orchestrator, overlay: action.overlay } };
    case 'executorChoice':
      return { ...state, executorChoice: action.choice };
    case 'nodeAdded': {
      if (!state.orchestrator || state.executors.some((e) => e.id === action.node.id)) return state;
      const rect = freeSlot(Object.values(state.layout), state.layout[ORCHESTRATOR], EXECUTOR_BLOCK.w, EXECUTOR_BLOCK.h);
      return {
        ...state, executors: [...state.executors, action.node], layout: { ...state.layout, [action.node.id]: rect },
        fit: fit(action.node.id),
      };
    }
    case 'nodeState':
      return {
        ...state,
        executors: state.executors.map((e) => (e.id === action.id ? { ...e, state: action.state, choice: action.choice } : e)),
      };
    case 'removed': {
      const layout = { ...state.layout };
      for (const id of action.ids) delete layout[id];
      return { ...state, executors: state.executors.filter((e) => !action.ids.includes(e.id)), layout };
    }
    case 'reset': {
      const layout = { ...state.layout };
      for (const id of [ORCHESTRATOR, ...state.executors.map((e) => e.id)]) delete layout[id];
      return { ...state, orchestrator: null, executorChoice: null, executors: [], layout, notice: action.notice, fit: fit() };
    }
    case 'move':
      return state.layout[action.id]
        ? { ...state, layout: { ...state.layout, [action.id]: moved(state.layout[action.id], action.dx, action.dy, action.z) } }
        : state;
    case 'resize':
      return state.layout[action.id]
        ? { ...state, layout: { ...state.layout, [action.id]: resized(state.layout[action.id], action.dx, action.dy, action.z) } }
        : state;
    case 'zoom':
      return { ...state, zoom: action.zoom };
    case 'fit':
      return { ...state, fit: fit() };
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
/** Overlay de um bloco: cobre o terminal com um formulário ou um aviso, sem sair do bloco. */
const BlockOverlay = ({ children }: { children: ReactNode }) => (
  <div className="absolute inset-0 overflow-auto bg-background/95 p-4">
    <div className="flex min-h-full items-center justify-center">{children}</div>
  </div>
);

export function App() {
  const [state, dispatch] = useReducer(reducer, initial);
  const stateRef = useRef(state);
  useLayoutEffect(() => {
    stateRef.current = state;
  });
  const canvas = useRef<CanvasHandle>(null);
  const [fatal, setFatal] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [projectError, setProjectError] = useState('');
  const [confirmOpen, setConfirmOpen] = useState(false);
  const closeAnswer = useRef<((ok: boolean) => void) | null>(null);
  const updateEvents = useRef(0);
  const [maxText, setMaxText] = useState('');
  const maxParallel = state.options?.maxParallel;
  useEffect(() => setMaxText(maxParallel === undefined ? '' : String(maxParallel)), [maxParallel]);
  const booted = useRef(false);

  // ---------- ações ----------

  const resetCanvas = (notice = '') => {
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
          if (next === 'running') terminals.get(id)?.setLive();
        }),
        backend.listen('executor-choice', (choice) => dispatch({ type: 'executorChoice', choice })),
        backend.listen('orchestrator-exit', ({ code }) =>
          resetCanvas(`O orquestrador terminou (${code === null ? 'sem código de saída' : `código de saída ${code}`}).`)),
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
    if (booted.current) return;
    booted.current = true;
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

  async function saveExecutorChoice(choice: Choice, closeOverlay: boolean) {
    await backend.setExecutorChoice(choice);
    if (closeOverlay) {
      dispatch({ type: 'overlay', overlay: null });
      terminals.get(ORCHESTRATOR)?.term.focus();
    }
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

  const { update, orchestrator, executors, layout, options } = state;
  const ready = update.phase === 'ready';
  const count = (...states: NodeState[]) => executors.filter((e) => states.includes(e.state)).length;
  const parts = [
    [count('running'), 'executando', 'executando'], [count('queued'), 'na fila', 'na fila'],
    [count('awaiting-choice'), 'aguardando escolha', 'aguardando escolha'], [count('done'), 'concluído', 'concluídos'],
    [count('stopped'), 'interrompido', 'interrompidos'], [count('failed'), 'com falha', 'com falha'],
  ] as const;
  const counts = parts.filter(([n]) => n).map(([n, one, many]) => `${n} ${n === 1 ? one : many}`).join(' · ');
  const links = orchestrator ? executors.map((e) => ({ id: e.id, from: ORCHESTRATOR, state: e.state })) : [];

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
          <Button variant="outline" size="sm" onClick={clearDone} disabled={count(...FINISHED) === 0}
            title="Remove do canvas os executores concluídos, interrompidos e com falha">
            limpar concluídos
          </Button>
          <div className="flex items-center gap-1">
            <Button variant="outline" size="icon-sm" aria-label="Afastar" title="Afastar"
              onClick={() => canvas.current?.zoomAt(canvas.current.zoom() / 1.2)}>
              <ZoomOut />
            </Button>
            <Button variant="ghost" size="sm" className="w-14 font-mono" title="Voltar a 100%"
              onClick={() => canvas.current?.zoomAt(1)}>
              {Math.round(state.zoom * 100)}%
            </Button>
            <Button variant="outline" size="icon-sm" aria-label="Aproximar" title="Aproximar"
              onClick={() => canvas.current?.zoomAt(canvas.current.zoom() * 1.2)}>
              <ZoomIn />
            </Button>
            <Button variant="outline" size="sm" onClick={() => canvas.current?.fitAll()}>
              <Maximize />
              enquadrar tudo
            </Button>
          </div>
        </div>
      </header>

      <main className="relative min-h-0 flex-1">
        <Canvas
          ref={canvas}
          rects={layout}
          links={links}
          fit={state.fit}
          onZoom={(zoom) => dispatch({ type: 'zoom', zoom })}
        >
          {orchestrator && (
            <Block
              rect={layout[ORCHESTRATOR]}
              head={
                <>
                  <Badge variant="secondary">orquestrador</Badge>
                  <span className="min-w-0 truncate font-mono text-muted-foreground">{orchestrator.title}</span>
                  <span className="flex-1" />
                  {orchestrator.started && (
                    <Button
                      variant="outline" size="xs"
                      className="font-mono"
                      title="Escolha dos executores; a troca vale para os próximos"
                      onClick={toggleExecutorChoice}
                    >
                      executores: {state.executorChoice ? describeChoice(state.executorChoice) : 'ainda não escolhido'}
                    </Button>
                  )}
                </>
              }
              onMove={(dx, dy) => dispatch({ type: 'move', id: ORCHESTRATOR, dx, dy, z: canvas.current?.zoom() ?? 1 })}
              onResize={(dx, dy) => dispatch({ type: 'resize', id: ORCHESTRATOR, dx, dy, z: canvas.current?.zoom() ?? 1 })}
            >
              <TermHost id={ORCHESTRATOR} writable />
            </Block>
          )}

          {orchestrator && executors.map((node) => {
            const meta = STATES[node.state];
            const Icon = meta.icon;
            const cancellable = node.state === 'queued' || node.state === 'running';
            return (
              <Block
                key={node.id}
                rect={layout[node.id]}
                state={node.state}
                head={
                  <>
                    <Badge variant={meta.variant} className="gap-1">
                      <Icon className={node.state === 'running' ? 'animate-spin' : undefined} />
                      {meta.label}
                    </Badge>
                    <span className="font-mono text-muted-foreground">#{node.index + 1}</span>
                    <span className="min-w-0 flex-1 truncate" title={node.title}>{node.title}</span>
                    {node.choice && <span className="truncate font-mono text-muted-foreground">{describeChoice(node.choice)}</span>}
                    {cancellable && (
                      <Button variant="outline" size="xs" title="Interrompe só este executor"
                        onClick={() => backend.stopExecutor(node.id).catch(report)}>
                        parar
                      </Button>
                    )}
                  </>
                }
                onMove={(dx, dy) => dispatch({ type: 'move', id: node.id, dx, dy, z: canvas.current?.zoom() ?? 1 })}
                onResize={(dx, dy) => dispatch({ type: 'resize', id: node.id, dx, dy, z: canvas.current?.zoom() ?? 1 })}
              >
                <TermHost id={node.id} writable={false} onReady={attachExecutor} />
                {node.state === 'awaiting-choice' && (
                  <BlockOverlay>
                    <p className="max-w-[300px] text-center text-sm text-muted-foreground">Aguardando a escolha dos executores.</p>
                  </BlockOverlay>
                )}
                {node.state === 'queued' && (
                  <BlockOverlay>
                    <p className="max-w-[300px] text-center text-sm text-muted-foreground">
                      Na fila: começa quando um executor terminar ou o teto de simultâneos subir.
                    </p>
                  </BlockOverlay>
                )}
              </Block>
            );
          })}
        </Canvas>

        {!orchestrator && (
          <div className="absolute inset-0 m-auto grid h-fit w-[min(380px,calc(100%-32px))] justify-items-center gap-3 p-4 text-center">
            {state.notice && (
              <p role="status" className="rounded-md border bg-card px-3 py-1.5 text-sm">{state.notice}</p>
            )}
            <Button onClick={() => openOrchestrator()}>Abrir terminal</Button>
            <p className="text-xs text-muted-foreground">
              Abre o orquestrador: o único terminal onde você digita. Os executores aparecem ao lado quando ele delega.
            </p>
          </div>
        )}
      </main>

      <Toaster position="bottom-right" />
      {/* As três escolhas são janelas: a do orquestrador (início e executores) e uma só para os executores aguardando. */}
      {options && (
        <>
          <ChoiceForm
            open={orchestrator?.overlay === 'start'}
            title="Orquestrador" hint="Só lê o projeto e delega. É o único terminal onde você digita."
            options={options} initial={options.last.orchestrator} confirmLabel="Abrir terminal"
            onCancel={() => resetCanvas()} onConfirm={startOrchestrator} onClosed={focusOrchestrator}
          />
          {/* Com um executor aguardando escolha, só a janela dele fica aberta: nunca duas empilhadas. */}
          <ChoiceForm
            open={orchestrator?.overlay === 'executor' && count('awaiting-choice') === 0}
            title="Executores"
            hint="Ferramenta, modelo e esforço dos próximos executores. Os que já estão rodando não mudam."
            options={options} initial={state.executorChoice ?? options.last.executor} confirmLabel="Usar nos executores"
            onCancel={() => dispatch({ type: 'overlay', overlay: null })}
            onConfirm={(choice) => saveExecutorChoice(choice, true)} onClosed={focusOrchestrator}
          />
          <ChoiceForm
            open={count('awaiting-choice') > 0}
            title="Escolha dos executores"
            hint="Vale para este e para os próximos executores enquanto o orquestrador estiver aberto."
            options={options} initial={state.executorChoice ?? options.last.executor} confirmLabel="Usar nos executores"
            onConfirm={(choice) => saveExecutorChoice(choice, true)} onClosed={focusOrchestrator}
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
