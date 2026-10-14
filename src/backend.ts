// Único ponto de contato com o Tauri. Fora do app (navegador, em desenvolvimento) entra o simulado.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Choice, ClipboardSnapshot, DirEntry, Events, FileContent, GitBranches, GitDiff, GitStatus, Options, SearchOptions, SearchResults, Snapshot, UpdateStatus } from './types';

export interface Transport {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  listen<K extends keyof Events>(event: K, handler: (payload: Events[K]) => void): Promise<void>;
  /** `mayClose` responde se a janela pode fechar; o encerramento do backend é feito dentro dela. */
  onCloseRequested(mayClose: () => Promise<boolean>): Promise<void>;
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  /** Passa pelo `onCloseRequested`: a confirmação de fechamento vale também para o botão da barra. */
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
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
  onResized: (handler) => getCurrentWindow().onResized(() => handler()),
};

let transport: Transport;

export async function connect(): Promise<void> {
  if ('__TAURI_INTERNALS__' in window) transport = tauri;
  else if (import.meta.env.DEV) transport = (await import('./mock')).mock;
  else throw new Error('Esta página só funciona dentro do app Orquestrador.');
}

const call = <T>(cmd: string, args?: Record<string, unknown>) => transport.invoke(cmd, args) as Promise<T>;

/** Acesso direto à janela e aos eventos (escuta, fechamento, barra de título). */
export const host = (): Transport => transport;

export const backend = {
  updateStatus: () => call<UpdateStatus>('update_status'),
  retryUpdate: () => call<UpdateStatus>('retry_update'),

  chooseProject: () => call<string | null>('choose_project'),
  getOptions: () => call<Options>('get_options'),
  startOrchestrator: (choice: Choice, cols: number, rows: number) =>
    call<void>('start_orchestrator', { choice, cols, rows }),
  // Shell do usuário no terminal de `id` (gerado pelo frontend, "shell-<n>"), na pasta do projeto ou em `cwd` (relativa ao projeto, com "/"; '' é a raiz). Devolve o nome do programa. Erro vem como mensagem em português.
  startShell: (id: string, cols: number, rows: number, cwd?: string) => call<string>('start_shell', { id, cols, rows, cwd }),
  // Mata o shell de `id` sem gerar evento de saída.
  closeShell: (id: string) => call<void>('close_shell', { id }),
  setExecutorChoice: (choice: Choice) => call<void>('set_executor_choice', { choice }),
  termWrite: (id: string, data: string) => call<void>('term_write', { id, data }),
  termResize: (id: string, cols: number, rows: number) => call<void>('term_resize', { id, cols, rows }),
  termSnapshot: (id: string) => call<Snapshot>('term_snapshot', { id }),
  clipboardFiles: () => call<string[]>('clipboard_files'),
  stopExecutor: (id: string) => call<void>('stop_executor', { id }),
  clearDone: () => call<string[]>('clear_done'),
  hasRunning: () => call<boolean>('has_running'),
  shutdown: () => call<void>('shutdown'),
  // Caminho relativo à pasta do projeto, com "/"; '' é a raiz.
  listDir: (path: string) => call<DirEntry[]>('list_dir', { path }),
  // Conteúdo de um arquivo, com o mesmo caminho de `listDir`. Erro chega como mensagem em português.
  readFile: (path: string) => call<FileContent>('read_file', { path }),
  writeFile: (path: string, text: string) => call<void>('write_file', { path, text }),
  // Ações de um arquivo da árvore, com o caminho relativo ao projeto. Erro vem como mensagem em português.
  // Renomear devolve o caminho novo; nunca sobrescreve. Excluir manda à Lixeira do Windows.
  renameFile: (path: string, name: string) => call<string>('rename_file', { path, name }),
  deleteFile: (path: string) => call<void>('delete_file', { path }),
  revealFile: (path: string) => call<void>('reveal_file', { path }),
  openWith: (path: string) => call<void>('open_with', { path }),
  copyFile: (path: string, cut: boolean) => call<void>('copy_file', { path, cut }),
  // Criação na pasta `dir` (relativa, '' é a raiz). Devolve o caminho relativo criado; nunca sobrescreve (nome que existe rejeita com a mensagem).
  createFile: (dir: string, name: string) => call<string>('create_file', { dir, name }),
  createDir: (dir: string, name: string) => call<string>('create_dir', { dir, name }),
  // Abre no Explorer a pasta `path` (relativa, '' é a raiz do projeto).
  revealDir: (path: string) => call<void>('reveal_dir', { path }),
  // Clipboard lido na hora da ação. Colar recebe essa mesma lista: a checagem de alterações e a operação usam o que foi lido aqui.
  clipboardSnapshot: () => call<ClipboardSnapshot>('clipboard_snapshot'),
  pasteFiles: (dest: string, snap: ClipboardSnapshot) => call<string[]>('paste_files', { dest, snap }),
  // Painel Git. Caminhos relativos à raiz do projeto, com "/"; lista vazia em stage/unstage vale para tudo.
  gitStatus: () => call<GitStatus>('git_status'),
  gitDiff: (path: string, staged: boolean) => call<GitDiff>('git_diff', { path, staged }),
  gitStage: (paths: string[]) => call<void>('git_stage', { paths }),
  gitUnstage: (paths: string[]) => call<void>('git_unstage', { paths }),
  gitCommit: (message: string) => call<void>('git_commit', { message }),
  gitFetch: () => call<string>('git_fetch'),
  gitPull: () => call<string>('git_pull'),
  gitPush: () => call<string>('git_push'),
  gitBranches: () => call<GitBranches>('git_branches'),
  gitSwitch: (name: string) => call<void>('git_switch', { name }),
  gitCreateBranch: (name: string) => call<void>('git_create_branch', { name }),
  // Pesquisa de texto nos arquivos do projeto. Regex inválida rejeita com a mensagem em português; consulta vazia devolve lista vazia.
  search: (query: string, options: SearchOptions) => call<SearchResults>('search', { query, ...options }),
};

/** Erros do backend já chegam como mensagem em português. */
export const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
