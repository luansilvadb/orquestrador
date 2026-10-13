// Backend simulado, só para desenvolvimento no navegador (nunca entra no build de produção).
// Digite no orquestrador: `lote` (6 tarefas), `sair` (termina com código 0), `sair 3` (código 3).
// Na URL: `?mock=nogit` faz a escolha da pasta falhar como "fora de um repositório Git".
// Na URL: `?update=falha` mostra a atualização bloqueada por falta de internet (a nova tentativa libera) e
// `?update=download` simula o download (`indeterminado` e `total-zero`: tamanho desconhecido). Sem parâmetro, o uso já vem liberado, como no build de desenvolvimento.
// No console: `__mock.close()` simula fechar a janela; `__mock.writes` e `__mock.sizes` registram o que chegou.
// `__mock.remove('src/App.tsx')` apaga um arquivo da árvore do projeto (a lista da tela some com ele sozinha).
// A árvore (`list_dir`) e o Git (`git_*`) são dados em memória: a árvore muda só com o executor e `__mock.remove`.
// O Git é uma lista fixa de alterações (um arquivo em duas seções, um binário, um grande demais para o diff, um não
// rastreado), com os diffs escritos à mão. Preparar, tirar, commitar, criar e trocar branch funcionam; fetch, pull e push mexem em ahead/behind (push recusa
// se o remoto tem commits novos). Um executor que termina bem cria `src/tarefa-<lote>-<n>.ts`, que aparece como não rastreado.
// Com executor não finalizado, pull, troca e criação de branch são recusados, como no backend.
import type { Transport } from './backend';
import {
  ORCHESTRATOR, type Choice, type DiffHunk, type DiffLine, type Events, type ExecutorNode, type GitDiff, type GitFile,
  type GitKind, type NodeState, type Options, type UpdateStatus,
} from './types';

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

// Árvore simulada do projeto, em memória. Pastas são `Folder`; arquivos são `{ dir: false }`.
type Folder = { dir: true; children: Record<string, Entry> };
type Entry = Folder | { dir: false };
const file = (): Entry => ({ dir: false });
const folder = (children: Record<string, Entry> = {}): Folder => ({ dir: true, children });
const tree = folder({
  '.gitignore': file(),
  'README.md': file(),
  'package.json': file(),
  assets: folder({ 'logo.png': file() }),
  dados: folder({ 'exportacao.json': file() }),
  docs: folder({ 'design-system.md': file(), 'rascunho-painel.md': file() }),
  src: folder({
    'App.tsx': file(), 'index.css': file(), 'main.tsx': file(),
    components: folder({ ui: folder({ 'button.tsx': file(), 'card.tsx': file() }) }),
    lib: folder({ 'utils.ts': file() }),
  }),
  tests: folder(),
});

// Lista do Git: só os arquivos alterados, com o lado preparado (`staged`) e o das alterações (`unstaged`).
const entry = (path: string, staged: GitKind | null, unstaged: GitKind | null): GitFile => ({ path, origPath: null, staged, unstaged });
const byPath = (a: GitFile, b: GitFile) => a.path.localeCompare(b.path, 'pt-BR');
let files: GitFile[] = [
  entry('README.md', null, 'modified'),
  entry('docs/design-system.md', 'modified', null),
  entry('docs/rascunho-painel.md', null, 'untracked'),
  entry('src/App.tsx', 'modified', 'modified'),
  entry('dados/exportacao.json', null, 'modified'),
  entry('assets/logo.png', null, 'modified'),
];
// Diffs escritos à mão, por lado e caminho: `staged:` é o preparado contra o HEAD; `unstaged:`, as alterações contra o preparado.
const ctx = (old: number, nw: number, text: string): DiffLine => ({ kind: 'context', old, new: nw, text });
const add = (nw: number, text: string): DiffLine => ({ kind: 'add', old: null, new: nw, text });
const del = (old: number, text: string): DiffLine => ({ kind: 'del', old, new: null, text });
const HUNKS: Record<string, DiffHunk[]> = {
  'unstaged:README.md': [{ header: '@@ -1 +1,3 @@', lines: [ctx(1, 1, '# Orquestrador'), add(2, ''), add(3, 'Painel Git em andamento.')] }],
  'staged:docs/design-system.md': [{ header: '@@ -1 +1,3 @@', lines: [ctx(1, 1, '# Design system'), add(2, ''), add(3, 'Tokens semânticos.')] }],
  'unstaged:docs/rascunho-painel.md': [{ header: '@@ -0,0 +1,3 @@', lines: [add(1, '# Rascunho'), add(2, ''), add(3, 'Ideias para o painel Git.')] }],
  'staged:src/App.tsx': [{
    header: '@@ -1,3 +1,3 @@',
    lines: [ctx(1, 1, 'export function App() {'), del(2, '  return null;'), add(2, '  return <main />;'), ctx(3, 3, '}')],
  }],
  'unstaged:src/App.tsx': [{
    header: '@@ -1,3 +1,5 @@',
    lines: [ctx(1, 1, 'export function App() {'), ctx(2, 2, '  return <main />;'), ctx(3, 3, '}'), add(4, ''), add(5, '// TODO: abas')],
  }],
};
const branches = ['main', 'feat/painel-git'];
let branch = 'main', ahead = 1, behind = 0;
const BUSY = 'Há executores trabalhando. Espere o lote terminar.';

/** Pasta pelo caminho relativo com "/" ('' é a raiz); arquivo ou caminho inexistente dá undefined. */
function folderAt(path: string): Folder | undefined {
  let node: Entry | undefined = tree;
  for (const name of path.split('/').filter(Boolean)) node = node?.dir ? node.children[name] : undefined;
  return node?.dir ? node : undefined;
}
// Pastas primeiro, depois arquivos, alfabética: a mesma ordem do backend.
const listing = (dir: Folder) => Object.entries(dir.children)
  .map(([name, entry]) => ({ name, dir: entry.dir }))
  .sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name, 'pt-BR'));
// Arquivo novo no disco, fora do índice: aparece como não rastreado no painel Git.
function addFile(dirPath: string, name: string) {
  const dir = folderAt(dirPath);
  if (!dir) return;
  dir.children[name] = file();
  files.push(entry(`${dirPath}/${name}`, null, 'untracked'));
}
// Apagar pela árvore: o arquivo some do disco (apagado, se estava no Git; some de vez, se era não rastreado).
function removePath(path: string) {
  const parts = path.split('/'), name = parts.pop()!;
  const dir = folderAt(parts.join('/'));
  const child = dir?.children[name];
  if (!dir || !child) return;
  delete dir.children[name];
  if (child.dir) return;
  const f = files.find((x) => x.path === path);
  if (f?.unstaged === 'untracked') files = files.filter((x) => x !== f);
  else if (f) f.unstaged = 'deleted';
  else files.push(entry(path, null, 'deleted'));
}

let alive = false, line = '', batches = 0, executorChoice: Choice | null = null, closeHandler: (() => Promise<boolean>) | null = null;
const jobs = new Map<string, Job>();
const debug = { writes: [] as string[], sizes: {} as Record<string, string>, close: async () => {
  const closed = closeHandler ? await closeHandler() : true;
  if (closed) document.body.innerHTML = '<p style="padding:24px">Janela fechada (simulado).</p>';
  return closed;
}, remove: (path: string) => removePath(path) };
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

// Estado vivo que ocupa vaga de simultâneos, como o `running`.
const live = (state: NodeState) => state === 'running' || state === 'awaiting-orchestrator';

function schedule() {
  const count = () => [...jobs.values()].filter((j) => live(j.state)).length;
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
  // Uma tarefa (índice 2) pede confirmação ao orquestrador no passo 5 e fica 3 s em `awaiting-orchestrator`.
  let pauseAt = job.index === 2 ? 5 : -1;
  let step = 0;
  out(job.id, `\x1b[1m${job.choice!.tool}\x1b[0m \x1b[2m${job.choice!.model || 'modelo padrão'} · ${job.choice!.effort || 'esforço padrão'}\x1b[0m\r\n\x1b[36m>\x1b[0m ${job.title}\r\n\r\n`);
  job.timer = window.setInterval(() => {
    if (job.state === 'awaiting-orchestrator') return;
    if (step === pauseAt) {
      pauseAt = -1;
      setState(job, 'awaiting-orchestrator');
      out(job.id, '\x1b[33mPedido de confirmação: aguardando o orquestrador.\x1b[0m\r\n');
      setTimeout(() => { if (job.state === 'awaiting-orchestrator') setState(job, 'running'); }, 3000);
      return;
    }
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
  // Um executor que termina bem deixa um arquivo: a árvore muda e a lista do explorador tem de acompanhar.
  if (state === 'done') addFile('src', `tarefa-${job.batch}-${job.index + 1}.ts`);
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

// ---------- Git simulado ----------

// Executor ou orquestrador com tarefa aberta: a mesma trava do backend para pull, troca e criação de branch.
const busy = () => [...jobs.values()].some((j) => !['done', 'stopped', 'failed'].includes(j.state));

// Lista vazia vale para tudo, como no backend.
const known = (path: string) => {
  const f = files.find((x) => x.path === path);
  if (!f) throw `Arquivo não encontrado no repositório: ${path}`;
  return f;
};
function stage(paths: string[]) {
  const targets = paths.length ? paths : files.filter((f) => f.unstaged).map((f) => f.path);
  for (const path of targets) {
    const f = known(path);
    if (!f.unstaged) continue;
    // O que estava nas alterações vai para o preparado; arquivo novo continua novo.
    f.staged = f.staged === 'added' || f.unstaged === 'untracked' ? 'added' : f.unstaged;
    f.unstaged = null;
  }
}
function unstage(paths: string[]) {
  const targets = paths.length ? paths : files.filter((f) => f.staged).map((f) => f.path);
  for (const path of targets) {
    const f = known(path);
    if (!f.staged) continue;
    // Tirado do preparo, o que era novo volta a não rastreado.
    f.unstaged = f.staged === 'added' ? 'untracked' : f.staged;
    f.staged = null;
  }
}
function commit(message: string) {
  if (!message.trim()) throw 'A mensagem do commit está vazia.';
  if (!files.some((f) => f.staged)) throw 'Nada preparado para commitar.';
  for (const f of files) f.staged = null;
  // O que não sobrou em alterações sai da lista.
  files = files.filter((f) => f.unstaged);
  ahead++;
}
// Remoto simulado: fetch traz 2 commits de fora; pull os recebe; push só envia se nada novo chegou do remoto.
const fetchRemote = () => {
  behind = Math.max(behind, 2);
  return `From github.com:luansilvadb/orquestrador\n   1a2b3c4..5d6e7f8  ${branch} -> origin/${branch}\n`;
};
const pullRemote = () => {
  if (busy()) throw BUSY;
  if (!behind) return 'Already up to date.\n';
  const n = behind;
  behind = 0;
  return `Atualizando de origin/${branch}\nFast-forward\n ${n} commits recebidos\n`;
};
const pushRemote = () => {
  if (behind) throw 'O envio foi recusado: o remoto tem commits que você não recebeu. Receba antes e tente de novo.';
  if (!ahead) return 'Everything up-to-date\n';
  const n = ahead;
  ahead = 0;
  return `Enviando ${n} commit(s) para origin/${branch}\n   1a2b3c4..5d6e7f8  ${branch} -> ${branch}\n`;
};

// Diff: o que não tem hunk escrito aqui sai sem trechos ("Sem alterações neste arquivo").
function diffOf(path: string, staged: boolean): GitDiff {
  known(path);
  if (path.endsWith('.png')) return { binary: true, tooLarge: false, hunks: [] };
  if (path === 'dados/exportacao.json') return { binary: false, tooLarge: true, hunks: [] };
  return { binary: false, tooLarge: false, hunks: HUNKS[`${staged ? 'staged' : 'unstaged'}:${path}`] ?? [] };
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
  list_dir: ({ path }: { path: string }) => {
    const dir = folderAt(path);
    if (!dir) throw `Pasta não encontrada: ${path}`;
    return listing(dir);
  },
  git_status: () => ({ branch, ahead, behind, files: [...files].sort(byPath) }),
  git_diff: ({ path, staged }: { path: string; staged: boolean }) => diffOf(path, staged),
  git_stage: ({ paths }: { paths: string[] }) => { stage(paths); return null; },
  git_unstage: ({ paths }: { paths: string[] }) => { unstage(paths); return null; },
  git_commit: ({ message }: { message: string }) => { commit(message); return null; },
  git_fetch: () => fetchRemote(),
  git_pull: () => pullRemote(),
  git_push: () => pushRemote(),
  git_branches: () => ({ current: branch, local: [...branches] }),
  git_switch: ({ name }: { name: string }) => {
    if (busy()) throw BUSY;
    if (!branches.includes(name)) throw `Branch não encontrada: ${name}`;
    branch = name;
    return null;
  },
  git_create_branch: ({ name }: { name: string }) => {
    if (busy()) throw BUSY;
    const next = name.trim();
    if (!next) throw 'Informe o nome da branch.';
    if (branches.includes(next)) throw `A branch ${next} já existe.`;
    branches.push(next);
    branch = next;
    return null;
  },
  term_resize: ({ id, cols, rows }: { id: string; cols: number; rows: number }) => { debug.sizes[id] = `${cols}x${rows}`; },
  term_snapshot: ({ id }: { id: string }) => {
    const term = terms.get(id);
    return { seq: term?.seq ?? 0, data: base64(term?.text ?? '') };
  },
  stop_executor: ({ id }: { id: string }) => {
    const job = jobs.get(id);
    if (job && (job.state === 'queued' || live(job.state))) finish(job, 'stopped');
  },
  clear_done: () => {
    const ids = [...jobs.values()].filter((j) => ['done', 'stopped', 'failed'].includes(j.state)).map((j) => j.id);
    ids.forEach((id) => { jobs.delete(id); terms.delete(id); });
    return ids;
  },
  has_running: () => alive || [...jobs.values()].some((j) => live(j.state)),
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
  // Janela simulada: minimizar e maximizar não fazem nada; fechar segue o mesmo caminho de `__mock.close()`.
  minimize: async () => {},
  toggleMaximize: async () => {},
  close: async () => void (await debug.close()),
  isMaximized: async () => false,
  onResized: async () => () => {},
};
