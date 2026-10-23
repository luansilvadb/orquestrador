// Único ponto de contato com o Tauri. Fora do app (navegador, em desenvolvimento) entra o simulado.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type {
  Choice, ClipboardSnapshot, CommandName, Commands, Events, HunkRef, NoArgs, ReplaceQuery, ReplaceRequest, SearchFilter, SearchOptions,
} from './types';

export interface Transport {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  listen<K extends keyof Events>(event: K, handler: (payload: Events[K]) => void): Promise<void>;
  /** `mayClose` responde se a janela pode fechar. */
  onCloseRequested(mayClose: () => Promise<boolean>): Promise<void>;
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  /** Passa pelo `onCloseRequested`: a confirmação de fechamento vale também para o botão da barra. */
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
  setTitle(title: string): Promise<void>;
  /** Chama `handler` a cada mudança de tamanho (maximizar e restaurar entram aqui); responde o cancelamento da escuta. */
  onResized(handler: () => void): Promise<() => void>;
}

const tauri: Transport = {
  invoke,
  listen: async (event, handler) => void (await listen(event, (e) => handler(e.payload as never))),
  onCloseRequested: async (mayClose) =>
    void (await getCurrentWindow().onCloseRequested(async (event) => {
      if (!(await mayClose())) event.preventDefault();
    })),
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
  isMaximized: () => getCurrentWindow().isMaximized(),
  setTitle: (title) => getCurrentWindow().setTitle(title),
  onResized: (handler) => getCurrentWindow().onResized(() => handler()),
};

let transport: Transport;
let activeProject: string | null = null;

export function setProjectId(id: string | null): void {
  activeProject = id;
}

export async function connect(): Promise<void> {
  if ('__TAURI_INTERNALS__' in window) transport = tauri;
  else if (import.meta.env.DEV) transport = (await import('./mock')).mock;
  else throw new Error('Esta página só funciona dentro do app Orquestrador.');
}

// Chamada tipada pelo mapa `Commands`: o nome, os argumentos e o resultado vêm de lá. Sem argumento, não se passa nada.
type CallArgs<K extends CommandName> = Commands[K]['args'] extends NoArgs ? [] : [args: Commands[K]['args']];
const call = <K extends CommandName>(cmd: K, ...rest: CallArgs<K>) =>
  transport.invoke(cmd, rest[0] as Record<string, unknown> | undefined) as Promise<Commands[K]['result']>;
type ProjectCommand = { [K in CommandName]: Commands[K]['args'] extends { project: string } ? K : never }[CommandName];
const projectId = () => {
  if (!activeProject) throw new Error('Escolha a pasta do projeto primeiro.');
  return activeProject;
};
const projectCall = <K extends ProjectCommand>(cmd: K, args: Omit<Commands[K]['args'], 'project'>) =>
  transport.invoke(cmd, { ...args, project: projectId() } as Record<string, unknown>) as Promise<Commands[K]['result']>;
const shellProject = (project?: string | null) => project === undefined ? activeProject : project;
const absolutePath = (path: string) => /^(?:[a-z]:[\\/]|\\\\|\/)/i.test(path);

/** Acesso direto à janela e aos eventos (escuta, fechamento, barra de título). */
export const host = (): Transport => transport;

export const backend = {
  updateStatus: () => call('update_status'),
  retryUpdate: () => call('retry_update'),

  // Pasta escolhida: `subfolder` quer dizer que ela está dentro de um repositório e o projeto é a raiz dele.
  chooseProject: () => call('choose_project'),
  openProject: (path: string) => call('open_project', { path }),
  closeProject: (id = projectId()) => call('close_project', { project: id }),
  listProjects: () => call('list_projects'),
  projectSnapshot: (id = projectId()) => call('project_snapshot', { project: id }),
  pickFile: () => call('pick_file'),
  pickSavePath: (directory: string | null, fileName: string) => call('pick_save_path', { directory, fileName }),
  getOptions: () => call('get_options'),
  startOrchestrator: (choice: Choice, cols: number, rows: number) =>
    projectCall('start_orchestrator', { choice, cols, rows }),
  // Shell do usuário no terminal de `id` (gerado pelo frontend, "shell-<n>"), na pasta do projeto ou em `cwd` (relativa ao projeto, com "/"; '' é a raiz). Devolve o nome do programa. Erro vem como mensagem em português.
  startShell: (id: string, cols: number, rows: number, cwd?: string, project?: string | null) => call('start_shell', { project: shellProject(project), id, cols, rows, cwd }),
  // Mata o shell de `id` sem gerar evento de saída.
  closeShell: (id: string, project?: string | null) => call('close_shell', { project: shellProject(project), id }),
  setExecutorChoice: (choice: Choice) => call('set_executor_choice', { choice }),
  termWrite: (id: string, data: string, project?: string | null) => call('term_write', { project: shellProject(project), id, data }),
  // O CLI que roda agora no terminal `id`, pelo processo (num shell, muda quando ele abre e quando sai); `null` é shell comum.
  termCli: (id: string, project?: string | null) => call('term_cli', { project: shellProject(project), id }),
  termResize: (id: string, cols: number, rows: number, project?: string | null) => call('term_resize', { project: shellProject(project), id, cols, rows }),
  termSnapshot: (id: string, project?: string | null) => call('term_snapshot', { project: shellProject(project), id }),
  stopExecutor: (id: string) => projectCall('stop_executor', { id }),
  hasRunning: () => call('has_running'),
  // Caminho relativo à pasta do projeto, com "/"; '' é a raiz.
  listDir: (path: string) => projectCall('list_dir', { path }),
  // Conteúdo de um arquivo, com o mesmo caminho de `listDir`. Erro chega como mensagem em português.
  readFile: (path: string) => absolutePath(path) ? call('read_file_at', { path }) : projectCall('read_file', { path }),
  readFileAt: (path: string) => call('read_file_at', { path }),
  // Bytes de uma imagem ou áudio (extensão na lista de `files.rs`), como `ArrayBuffer`. Erro chega como mensagem em português; limite de 32 MiB.
  readMedia: (path: string) => absolutePath(path) ? call('read_media_at', { path }) : projectCall('read_media', { path }),
  readMediaAt: (path: string) => call('read_media_at', { path }),
  formatterInfo: (path: string) => call('formatter_info', { path }),
  formatCli: (path: string, text: string) => call('format_cli', { path, text }),
  // Grava só se o arquivo ainda é `base` (o texto lido, sem BOM) e ainda tem o BOM `bom`; senão rejeita sem gravar.
  writeFileChecked: (path: string, text: string, base: string, bom: boolean) => absolutePath(path)
    ? call('write_file_at_checked', { path, text, base, bom })
    : projectCall('write_file_checked', { path, text, base, bom }),
  writeFileAtChecked: (path: string, text: string, base: string, bom: boolean) => call('write_file_at_checked', { path, text, base, bom }),
  // Ações de um arquivo da árvore, com o caminho relativo ao projeto. Erro vem como mensagem em português.
  // Renomear devolve o caminho novo; nunca sobrescreve. Excluir manda à Lixeira do Windows.
  renameFile: (path: string, name: string) => projectCall('rename_file', { path, name }),
  // Lote: `done` é o que foi feito e `error` o que parou (null se tudo deu certo). Nada é desfeito em caso de erro.
  deleteFiles: (paths: string[]) => projectCall('delete_files', { paths }),
  moveFiles: (paths: string[], dest: string) => projectCall('move_files', { paths, dest }),
  copyFiles: (paths: string[], cut: boolean) => projectCall('copy_files', { paths, cut }),
  revealFile: (path: string) => projectCall('reveal_file', { path }),
  openWith: (path: string) => projectCall('open_with', { path }),
  openUrl: (url: string) => call('open_url', { url }),
  // Criação na pasta `dir` (relativa, '' é a raiz). Devolve o caminho relativo criado; nunca sobrescreve (nome que existe rejeita com a mensagem).
  createFile: (dir: string, name: string) => projectCall('create_file', { dir, name }),
  createDir: (dir: string, name: string) => projectCall('create_dir', { dir, name }),
  // Abre no Explorer a pasta `path` (relativa, '' é a raiz do projeto).
  revealDir: (path: string) => projectCall('reveal_dir', { path }),
  // Clipboard lido na hora da ação. Colar recebe essa mesma lista: a checagem de alterações e a operação usam o que foi lido aqui.
  clipboardSnapshot: () => call('clipboard_snapshot'),
  // Colagem, com o detalhe: `from` absoluto como estava no clipboard, `to` relativo ao projeto; `error` diz o que parou.
  pasteFilesDetailed: (dest: string, snap: ClipboardSnapshot) => projectCall('paste_files_detailed', { dest, snap }),
  // Painel Git. Caminhos relativos à raiz do projeto, com "/"; lista vazia em stage/unstage vale para tudo.
  // Estado do Git da pasta (ver `GitState`). Erro chega como mensagem em português.
  gitStatus: () => projectCall('git_status', {}),
  // Cria o repositório na pasta do projeto; só no modo `no-repo`, nos outros rejeita com a mensagem.
  gitInit: () => projectCall('git_init', {}),
  gitStage: (paths: string[]) => projectCall('git_stage', { paths }),
  gitUnstage: (paths: string[]) => projectCall('git_unstage', { paths }),
  gitCommit: (message: string) => projectCall('git_commit', { message }),
  gitFetch: () => projectCall('git_fetch', {}),
  gitPull: () => projectCall('git_pull', {}),
  gitPush: () => projectCall('git_push', {}),
  gitBranches: () => projectCall('git_branches', {}),
  gitSwitch: (name: string) => projectCall('git_switch', { name }),
  gitCreateBranch: (name: string) => projectCall('git_create_branch', { name }),
  // Revisão por bloco. `revision` do diff e de cada bloco (`HunkRef`) é o que a tela leu; mudança depois disso é recusada.
  gitReviewDiff: (path: string, staged: boolean) => projectCall('git_review_diff', { path, staged }),
  gitStageHunk: (hunk: HunkRef) => projectCall('git_stage_hunk', { hunk }),
  gitUnstageHunk: (hunk: HunkRef) => projectCall('git_unstage_hunk', { hunk }),
  gitDiscardHunk: (hunk: HunkRef) => projectCall('git_discard_hunk', { hunk }),
  gitDiscardFile: (path: string, revision: string) => projectCall('git_discard_file', { path, revision }),
  // Histórico: `limit` até 200 e `skip` os já mostrados; com `path`, só os commits que tocaram o arquivo.
  gitHistory: (limit: number, skip: number, path?: string) => projectCall('git_history', { limit, skip, path: path ?? null }),
  // Gráfico: HEAD e o upstream da branch atual, mais novo primeiro; `limit` até 200 e `skip` os já mostrados. Sem commit, lista vazia.
  gitGraph: (limit: number, skip: number) => projectCall('git_graph', { limit, skip }),
  gitCommitDetail: (id: string) => projectCall('git_commit_detail', { id }),
  gitCommitHover: (hash: string) => projectCall('git_commit_hover', { hash }),
  gitCommitDiff: (id: string, path: string) => projectCall('git_commit_diff', { id, path }),
  // Conflito: os lados, o texto atual e a revisão. A gravação da resolução só vale se a pasta ainda está nessa revisão.
  gitConflict: (path: string) => projectCall('git_conflict', { path }),
  gitWriteResolution: (path: string, content: string, revision: string, markResolved: boolean) =>
    projectCall('git_write_resolution', { path, content, revision, markResolved }),
  gitAcceptDeletion: (path: string, revision: string) => projectCall('git_accept_deletion', { path, revision }),
  // Texto do arquivo no HEAD e no índice, base dos marcadores de mudança do editor. `null` sem marcadores (fora de repositório, não rastreado, binário ou acima de 1 MiB).
  gitFileBase: (path: string) => projectCall('git_file_base', { path }),
  // Grava `text` (com `\n`) como o arquivo no índice: preparar e tirar do preparo um bloco no editor. `expected` é o índice que a tela
  // viu (normalizado); se mudou, ou se há executores trabalhando, rejeita com a mensagem em português.
  gitWriteIndex: (path: string, text: string, expected: string) => projectCall('git_write_index', { path, text, expected }),
  // Pesquisa de texto nos arquivos do projeto. Regex inválida rejeita com a mensagem em português; consulta vazia devolve lista vazia.
  search: (query: string, options: SearchOptions, filter: SearchFilter) =>
    projectCall('search', { query: { text: query, ...options, ...filter } }),
  // Abertura rápida pelo nome ou caminho (termos separados por espaço). Só rastreados e não ignorados.
  quickOpen: (term: string) => projectCall('quick_open', { term }),
  // Prévia da substituição: não grava. A aplicação só toca nos arquivos escolhidos da prévia, com a revisão que ela mostrou; erros voltam por arquivo.
  replacePreview: (query: ReplaceQuery, replacement: string) => projectCall('replace_preview', { query, replacement }),
  replaceApply: (request: ReplaceRequest) => projectCall('replace_apply', { request }),
};

/** Erros do backend já chegam como mensagem em português. */
export const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
