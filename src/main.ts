import './style.css';
import { backend, connect, message } from './backend';
import { Canvas, type Block } from './canvas';
import { createSelector } from './selector';
import { TermView } from './term';
import { describeChoice, ORCHESTRATOR, type Choice, type ExecutorNode, type NodeState, type Options } from './types';

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

const STATES: Record<NodeState, string> = {
  'awaiting-choice': '? aguardando escolha',
  queued: '◷ na fila',
  running: '▶ executando',
  done: '✓ concluído',
  stopped: '■ interrompido',
  failed: '✕ falhou',
};
const FINISHED: NodeState[] = ['done', 'stopped', 'failed'];

type Panel = { block: Block; view: TermView; overlay: HTMLElement };
type Orchestrator = Panel & { label: HTMLButtonElement };
type Executor = Panel & { node: ExecutorNode };

const canvas = new Canvas($('viewport'), $('world'), $('links') as unknown as SVGSVGElement);
const executors = new Map<string, Executor>();
let orchestrator: Orchestrator | null = null;
let executorChoice: Choice | null = null;
let options: Options;

// ---------- avisos ----------

function toast(text: string) {
  const el = document.createElement('div');
  el.className = 'toast';
  el.textContent = text;
  $('toasts').append(el);
  setTimeout(() => el.remove(), 7000);
}
const report = (e: unknown) => toast(message(e));

function confirmDialog(text: string, okLabel: string): Promise<boolean> {
  const dialog = $<HTMLDialogElement>('confirm');
  dialog.querySelector('p')!.textContent = text;
  dialog.querySelector('button[value="ok"]')!.textContent = okLabel;
  dialog.returnValue = 'cancel';
  dialog.showModal();
  return new Promise((resolve) => dialog.addEventListener('close', () => resolve(dialog.returnValue === 'ok'), { once: true }));
}

// ---------- blocos ----------

/** Corpo comum dos blocos: o terminal e, por cima, uma camada para o seletor ou um aviso. */
function mountPanel(block: Block, writable: boolean): Panel {
  block.body.innerHTML = '<div class="term"></div><div class="overlay" hidden></div>';
  const [host, overlay] = block.body.children as unknown as HTMLElement[];
  return { block, overlay, view: new TermView(block.id, host, writable) };
}

function setOverlay(panel: Panel, kind: string, content?: HTMLElement) {
  panel.overlay.replaceChildren(...(content ? [content] : []));
  panel.overlay.dataset.kind = kind;
  panel.overlay.hidden = !content;
}

function executorSelector(panel: Panel, title: string, hint: string, onCancel?: () => void) {
  return createSelector({
    title, hint, options, onCancel, confirmLabel: 'Usar nos executores',
    initial: executorChoice ?? options.last.executor,
    onConfirm: (choice) => backend.setExecutorChoice(choice).then(() => panel === orchestrator && closeLabelSelector()),
  });
}

function closeLabelSelector() {
  if (!orchestrator) return;
  setOverlay(orchestrator, '');
  orchestrator.view.term.focus();
}

async function openOrchestrator() {
  if (orchestrator) return;
  options = await backend.getOptions();
  $('empty').hidden = true;
  const block = canvas.add(ORCHESTRATOR, { x: 0, y: 0, w: 800, h: 520 });
  block.el.classList.add('orchestrator');
  block.head.innerHTML = `<span class="badge">orquestrador</span><span class="title"></span><span class="spacer"></span>
    <button class="tag" hidden title="Escolha dos executores; a troca vale para os próximos"></button>`;
  const self: Orchestrator = { ...mountPanel(block, true), label: block.head.querySelector('button')! };
  orchestrator = self;
  renderLabel();

  self.label.onclick = async () => {
    if (self.overlay.dataset.kind === 'executor') return closeLabelSelector();
    options = await backend.getOptions().catch(() => options);
    setOverlay(self, 'executor', executorSelector(self, 'Executores',
      'Ferramenta, modelo e esforço dos próximos executores. Os que já estão rodando não mudam.', closeLabelSelector));
  };

  setOverlay(self, 'start', createSelector({
    title: 'Orquestrador', hint: 'Só lê o projeto e delega. É o único terminal onde você digita.',
    options, initial: options.last.orchestrator, confirmLabel: 'Abrir terminal',
    onCancel: () => resetCanvas(),
    onConfirm: async (choice) => {
      await backend.startOrchestrator(choice, self.view.term.cols, self.view.term.rows);
      block.head.querySelector('.title')!.textContent = describeChoice(choice);
      self.label.hidden = false;
      setOverlay(self, '');
      self.view.setLive();
      await self.view.attach();
      self.view.term.focus();
    },
  }));
  canvas.fitAll();
}

function renderLabel() {
  if (!orchestrator) return;
  orchestrator.label.textContent = `executores: ${executorChoice ? describeChoice(executorChoice) : 'ainda não escolhido'}`;
  orchestrator.label.classList.toggle('unset', !executorChoice);
}

function addExecutor(node: ExecutorNode) {
  if (!orchestrator || executors.has(node.id)) return;
  const block = canvas.add(node.id, canvas.freeSlot(orchestrator.block.rect, 660, 380), ORCHESTRATOR);
  block.head.innerHTML = `<span class="badge"></span><span class="index"></span><span class="title"></span>
    <span class="choice"></span><button class="stop" title="Interrompe só este executor">parar</button>`;
  block.head.querySelector('.index')!.textContent = `#${node.index + 1}`;
  const title = block.head.querySelector<HTMLElement>('.title')!;
  title.textContent = title.title = node.title;
  const executor: Executor = { ...mountPanel(block, false), node };
  executors.set(node.id, executor);
  block.head.querySelector<HTMLButtonElement>('.stop')!.onclick = () => backend.stopExecutor(node.id).catch(report);
  executor.view.setLive();
  void executor.view.attach();
  renderExecutor(executor);
  if (!canvas.isVisible(block.rect)) canvas.fitAll();
}

function renderExecutor(executor: Executor) {
  const { node, block } = executor, { state } = node;
  block.el.dataset.state = state;
  canvas.setLinkState(node.id, state);
  const badge = block.head.querySelector<HTMLElement>('.badge')!;
  badge.textContent = STATES[state];
  block.head.querySelector('.choice')!.textContent = node.choice ? describeChoice(node.choice) : '';
  block.head.querySelector<HTMLElement>('.stop')!.hidden = state !== 'queued' && state !== 'running';

  if (state === 'awaiting-choice') {
    if (executor.overlay.dataset.kind !== state) {
      setOverlay(executor, state, executorSelector(executor, 'Escolha dos executores',
        'Vale para este e para os próximos executores enquanto o orquestrador estiver aberto.'));
    }
  } else if (state === 'queued') {
    const note = document.createElement('p');
    note.className = 'waiting';
    note.textContent = 'Na fila: começa quando um executor terminar ou o teto de simultâneos subir.';
    setOverlay(executor, state, note);
  } else {
    setOverlay(executor, '');
    if (state === 'running') executor.view.setLive();
  }
  renderBar();
}

function removeExecutor(id: string) {
  executors.get(id)?.view.dispose();
  executors.delete(id);
  canvas.remove(id);
}

/** Volta ao canvas vazio, mantendo o projeto. */
function resetCanvas(notice = '') {
  for (const id of [...executors.keys()]) removeExecutor(id);
  orchestrator?.view.dispose();
  canvas.remove(ORCHESTRATOR);
  orchestrator = null;
  executorChoice = null;
  $('notice').textContent = notice;
  $('notice').hidden = !notice;
  $('empty').hidden = false;
  canvas.fitAll();
  renderBar();
}

// ---------- barra superior ----------

function renderBar() {
  const count = (...states: NodeState[]) => [...executors.values()].filter((e) => states.includes(e.node.state)).length;
  const parts = [
    [count('running'), 'executando', 'executando'], [count('queued'), 'na fila', 'na fila'],
    [count('awaiting-choice'), 'aguardando escolha', 'aguardando escolha'], [count('done'), 'concluído', 'concluídos'],
    [count('stopped'), 'interrompido', 'interrompidos'], [count('failed'), 'com falha', 'com falha'],
  ] as const;
  $('counts').textContent = parts.filter(([n]) => n).map(([n, one, many]) => `${n} ${n === 1 ? one : many}`).join(' · ');
  $<HTMLButtonElement>('clear').disabled = count(...FINISHED) === 0;
}

function wireBar() {
  const max = $<HTMLInputElement>('max');
  max.value = String(options.maxParallel);
  max.onchange = async () => {
    const value = Math.min(16, Math.max(1, Math.round(Number(max.value)) || options.maxParallel));
    try {
      await backend.setMaxParallel(value);
      options.maxParallel = value;
    } catch (e) {
      report(e);
    }
    max.value = String(options.maxParallel);
  };
  $('clear').onclick = () => backend.clearDone().then((ids) => { ids.forEach(removeExecutor); renderBar(); }, report);
  $('zoom-out').onclick = () => canvas.zoomAt(canvas.zoom / 1.2);
  $('zoom-in').onclick = () => canvas.zoomAt(canvas.zoom * 1.2);
  $('zoom-reset').onclick = () => canvas.zoomAt(1);
  $('fit').onclick = () => canvas.fitAll();
  canvas.onZoom = (zoom) => { $('zoom-reset').textContent = `${Math.round(zoom * 100)}%`; };
  $('open').onclick = () => openOrchestrator().catch(report);
  renderBar();
}

// ---------- início ----------

async function chooseProject() {
  const error = $('start-error');
  error.hidden = true;
  try {
    const project = await backend.chooseProject();
    if (!project) return;
    options = await backend.getOptions();
    $('project').textContent = project;
    $('start').hidden = true;
    $('app').hidden = false;
    wireBar();
    canvas.fitAll();
  } catch (e) {
    error.textContent = message(e);
    error.hidden = false;
  }
}

async function main() {
  await connect();
  const view = (id: string) => (id === ORCHESTRATOR ? orchestrator?.view : executors.get(id)?.view);
  await Promise.all([
    backend.listen('term-data', ({ id, seq, data }) => view(id)?.push(seq, data)),
    backend.listen('node-added', addExecutor),
    backend.listen('node-state', ({ id, state, choice }) => {
      const executor = executors.get(id);
      if (!executor) return;
      Object.assign(executor.node, { state, choice });
      renderExecutor(executor);
    }),
    backend.listen('executor-choice', (choice) => { executorChoice = choice; renderLabel(); }),
    backend.listen('orchestrator-exit', ({ code }) =>
      resetCanvas(`O orquestrador terminou (${code === null ? 'sem código de saída' : `código de saída ${code}`}).`)),
    backend.onCloseRequested(async () => {
      try {
        if (await backend.hasRunning() &&
          !(await confirmDialog('Há terminais trabalhando; fechar encerra todos.', 'Fechar e encerrar todos'))) return false;
        await backend.shutdown();
      } catch { /* sem backend para responder não há o que proteger: deixa fechar */ }
      return true;
    }),
  ]);
  $('choose').onclick = chooseProject;
}

main().catch((e) => {
  $('start-error').textContent = message(e);
  $('start-error').hidden = false;
});
