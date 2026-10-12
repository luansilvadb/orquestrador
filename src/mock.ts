// Backend simulado, só para desenvolvimento no navegador (nunca entra no build de produção).
// Digite no orquestrador: `lote` (6 tarefas), `sair` (termina com código 0), `sair 3` (código 3).
// Na URL: `?mock=nogit` faz a escolha da pasta falhar como "fora de um repositório Git".
// Na URL: `?update=falha` mostra a atualização bloqueada por falta de internet (a nova tentativa libera) e
// `?update=download` simula o download (`indeterminado` e `total-zero`: tamanho desconhecido). Sem parâmetro, o uso já vem liberado, como no build de desenvolvimento.
// No console: `__mock.close()` simula fechar a janela; `__mock.writes` e `__mock.sizes` registram o que chegou.
import type { Transport } from './backend';
import { ORCHESTRATOR, type Choice, type Events, type ExecutorNode, type NodeState, type Options, type UpdateStatus } from './types';

type Handler = (payload: never) => void;
type Job = ExecutorNode & { timer?: number };

const CLAUDE = ['low', 'medium', 'high', 'xhigh', 'max'], CODEX = ['low', 'medium', 'high', 'xhigh'];
const options: Options = {
  tools: [
    { name: 'claude', efforts: CLAUDE, models: ['fable', 'opus', 'sonnet'].map((name) => ({ name, efforts: CLAUDE }))
      .concat({ name: 'haiku', efforts: ['low', 'medium', 'high'] }) },
    { name: 'codex', efforts: CODEX, models: [{ name: 'gpt-5.5', efforts: CODEX }, { name: 'gpt-5.5-mini', efforts: ['low', 'medium'] }] },
  ],
  last: { orchestrator: { tool: 'codex', model: '', effort: '' }, executor: { tool: 'claude', model: 'opus', effort: 'high' } },
  maxParallel: 4,
};
const TASKS = [
  'Extrair a validação de configuração para um módulo próprio',
  'Cobrir o analisador de argumentos com testes',
  'Atualizar o README com o fluxo novo',
  'Trocar o cliente HTTP por fetch nativo e remover a dependência antiga do projeto inteiro',
  'Migrar o esquema do banco (esta falha de propósito)',
  'Revisar mensagens de erro em português',
];

const handlers = new Map<string, Handler[]>();
const emit = <K extends keyof Events>(event: K, payload: Events[K]) =>
  handlers.get(event)?.forEach((h) => (h as (p: Events[K]) => void)(payload));

const base64 = (text: string) => {
  let binary = '';
  for (const byte of new TextEncoder().encode(text)) binary += String.fromCharCode(byte);
  return btoa(binary);
};
const terms = new Map<string, { seq: number; text: string }>();
function out(id: string, text: string) {
  const term = terms.get(id) ?? { seq: 0, text: '' };
  terms.set(id, term);
  term.text += text;
  emit('term-data', { id, seq: ++term.seq, data: base64(text) });
}

let alive = false, line = '', batches = 0, executorChoice: Choice | null = null, closeHandler: (() => Promise<boolean>) | null = null;
const jobs = new Map<string, Job>();
const debug = { writes: [] as string[], sizes: {} as Record<string, string>, close: async () => {
  const closed = closeHandler ? await closeHandler() : true;
  if (closed) document.body.innerHTML = '<p style="padding:24px">Janela fechada (simulado).</p>';
  return closed;
} };
(window as unknown as { __mock: typeof debug }).__mock = debug;

const PROMPT = '\x1b[38;5;105morquestrador>\x1b[0m ';

const MB = 1024 * 1024;
let update: UpdateStatus = { phase: 'ready' };
const setUpdate = (next: UpdateStatus) => { update = next; emit('update-status', next); };
// `total` null (tamanho desconhecido) ou 0 mostram o progresso indeterminado; o tamanho real é 40 MB.
function demoDownload(total: number | null = 40 * MB) {
  const size = 40 * MB;
  let downloaded = 0;
  const tick = () => {
    downloaded = Math.min(size, downloaded + 2 * MB);
    setUpdate({ phase: 'downloading', version: '0.2.0', downloaded, total });
    if (downloaded < size) return void setTimeout(tick, 150);
    setUpdate({ phase: 'installing', version: '0.2.0' });
    setTimeout(() => setUpdate({ phase: 'ready' }), 1500);
  };
  setTimeout(tick, 300);
}
const updateMode = new URLSearchParams(location.search).get('update');
if (updateMode === 'falha') update = { phase: 'failed', message: 'Não foi possível verificar atualizações: sem conexão com o GitHub.' };
if (updateMode === 'download') demoDownload();
if (updateMode === 'indeterminado') demoDownload(null);
if (updateMode === 'total-zero') demoDownload(0);

function setState(job: Job, state: NodeState) {
  job.state = state;
  emit('node-state', { id: job.id, state, choice: job.choice });
}

function schedule() {
  const count = () => [...jobs.values()].filter((j) => j.state === 'running').length;
  for (const job of jobs.values()) if (job.state === 'queued' && executorChoice && count() < options.maxParallel) run(job);
  for (const batch of new Set([...jobs.values()].map((j) => j.batch))) {
    const all = [...jobs.values()].filter((j) => j.batch === batch);
    if (!alive || !all.length || all.some((j) => !['done', 'stopped', 'failed'].includes(j.state)) || reported.has(batch)) continue;
    reported.add(batch);
    out(ORCHESTRATOR, `\r\n\x1b[2mlote ${batch} devolvido:\x1b[0m\r\n` +
      all.map((j) => `  #${j.index + 1} ${j.state === 'done' ? '\x1b[32mok\x1b[0m' : j.state === 'stopped' ? '\x1b[33minterrompido pelo usuário\x1b[0m' : '\x1b[31mfalhou\x1b[0m'}  ${j.title}`).join('\r\n') +
      `\r\n${PROMPT}`);
  }
}
const reported = new Set<string>();

function run(job: Job) {
  job.choice = executorChoice;
  setState(job, 'running');
  const steps = 18 + job.index * 9, fails = job.index === 4;
  let step = 0;
  out(job.id, `\x1b[1m${job.choice!.tool}\x1b[0m \x1b[2m${job.choice!.model || 'modelo padrão'} · ${job.choice!.effort || 'esforço padrão'}\x1b[0m\r\n\x1b[36m>\x1b[0m ${job.title}\r\n\r\n`);
  job.timer = window.setInterval(() => {
    step++;
    out(job.id, `\x1b[2m${String(step).padStart(3)}\x1b[0m  \x1b[32m●\x1b[0m ${['Read', 'Grep', 'Edit', 'Bash'][step % 4]}(src/modulo-${step}.ts)  \x1b[2m${(step * 37) % 900} linhas\x1b[0m\r\n`);
    if (fails && step === 12) {
      out(job.id, '\r\n\x1b[31mErro: o processo terminou com código 1.\x1b[0m\r\n');
      finish(job, 'failed');
    } else if (step >= steps) {
      out(job.id, `\r\n\x1b[1mConcluído.\x1b[0m ${steps} passos, nenhuma pendência.\r\n`);
      finish(job, 'done');
    }
  }, 140);
}

function finish(job: Job, state: NodeState) {
  clearInterval(job.timer);
  setState(job, state);
  schedule();
}

function startBatch() {
  const batch = String(++batches);
  TASKS.forEach((title, index) => {
    const state: NodeState = executorChoice ? 'queued' : index < options.maxParallel ? 'awaiting-choice' : 'queued';
    const job: Job = { id: `exec-${batch}-${index}`, batch, index, title: title.slice(0, 80), state, choice: null };
    setTimeout(() => {
      if (!alive) return;
      jobs.set(job.id, job);
      emit('node-added', { ...job });
      schedule();
    }, 90 * index);
  });
}

function killAll() {
  for (const job of jobs.values()) clearInterval(job.timer);
  jobs.clear();
  terms.clear();
  reported.clear();
  alive = false;
  executorChoice = null;
}

function typed(data: string) {
  for (const char of data) {
    if (char === '\r') {
      const [command, arg] = line.trim().split(/\s+/);
      line = '';
      out(ORCHESTRATOR, '\r\n');
      if (command === 'lote') { out(ORCHESTRATOR, 'delegando 6 tarefas; aguardando todas voltarem...\r\n'); startBatch(); continue; }
      if (command === 'sair') { killAll(); emit('orchestrator-exit', { code: arg ? Number(arg) : 0 }); return; }
      if (command) out(ORCHESTRATOR, `eco: ${command}${arg ? ' ' + arg : ''}\r\n`);
      out(ORCHESTRATOR, PROMPT);
    } else if (char === '\x7f') {
      if (line) { line = line.slice(0, -1); out(ORCHESTRATOR, '\b \b'); }
    } else if (char >= ' ') {
      line += char;
      out(ORCHESTRATOR, char);
    }
  }
}

const commands: Record<string, (args: any) => unknown> = {
  update_status: () => update,
  retry_update: () => {
    if (update.phase === 'failed') {
      setUpdate({ phase: 'checking' });
      setTimeout(() => setUpdate({ phase: 'ready' }), 400);
    }
    return update;
  },
  choose_project: () => {
    if (new URLSearchParams(location.search).get('mock') === 'nogit') throw 'A pasta C:\\Users\\Luan\\Documentos não está dentro de um repositório Git. Escolha outra.';
    return 'C:\\projetos\\exemplo';
  },
  get_options: () => structuredClone(options),
  start_orchestrator: ({ choice }: { choice: Choice }) => {
    if (alive) throw 'Já existe um orquestrador aberto.';
    if (choice.model.includes(' ')) throw `orchestrator: modelo '${choice.model}' inválido (sem espaço, aspas nem iniciar com '-')`;
    options.last.orchestrator = choice;
    alive = true;
    line = '';
    out(ORCHESTRATOR, `\x1b[1mOrquestrador simulado\x1b[0m \x1b[2m(${choice.tool} ${choice.model || 'modelo padrão'} ${choice.effort || 'esforço padrão'})\x1b[0m\r\n` +
      `Digite \x1b[33mlote\x1b[0m + Enter para delegar 6 tarefas, \x1b[33msair\x1b[0m para terminar. O resto é ecoado.\r\n\r\n${PROMPT}`);
    return ORCHESTRATOR;
  },
  set_executor_choice: ({ choice }: { choice: Choice }) => {
    if (choice.model.includes(' ')) throw `executor: modelo '${choice.model}' inválido (sem espaço, aspas nem iniciar com '-')`;
    options.last.executor = executorChoice = choice;
    emit('executor-choice', choice);
    for (const job of jobs.values()) if (job.state === 'awaiting-choice') setState(job, 'queued');
    schedule();
  },
  set_max_parallel: ({ value }: { value: number }) => { options.maxParallel = value; schedule(); },
  term_write: ({ id, data }: { id: string; data: string }) => {
    debug.writes.push(id);
    if (id === ORCHESTRATOR && alive) typed(data);
  },
  clipboard_files: () => [] as string[],
  term_resize: ({ id, cols, rows }: { id: string; cols: number; rows: number }) => { debug.sizes[id] = `${cols}x${rows}`; },
  term_snapshot: ({ id }: { id: string }) => {
    const term = terms.get(id);
    return { seq: term?.seq ?? 0, data: base64(term?.text ?? '') };
  },
  stop_executor: ({ id }: { id: string }) => {
    const job = jobs.get(id);
    if (job && (job.state === 'queued' || job.state === 'running')) finish(job, 'stopped');
  },
  clear_done: () => {
    const ids = [...jobs.values()].filter((j) => ['done', 'stopped', 'failed'].includes(j.state)).map((j) => j.id);
    ids.forEach((id) => { jobs.delete(id); terms.delete(id); });
    return ids;
  },
  has_running: () => alive || [...jobs.values()].some((j) => j.state === 'running'),
  shutdown: () => killAll(),
};

export const mock: Transport = {
  // O retrato é tirado aos 100 ms e entregue aos 200 ms: chegam eventos já contidos nele (a descartar)
  // e eventos posteriores a ele (a guardar) enquanto a resposta está a caminho.
  invoke: (cmd, args) => new Promise((resolve, reject) => setTimeout(() => {
    try {
      const result = commands[cmd](args);
      if (cmd === 'term_snapshot') setTimeout(() => resolve(result), 100);
      else resolve(result);
    } catch (e) { reject(e); }
  }, cmd === 'term_snapshot' ? 100 : 15)),
  listen: async (event, handler) => void handlers.set(event, [...(handlers.get(event) ?? []), handler as Handler]),
  onCloseRequested: async (mayClose) => { closeHandler = mayClose; },
};
