// Único ponto de contato com o Tauri. Fora do app (navegador, em desenvolvimento) entra o simulado.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type {
  Batch, Choice, ClipboardSnapshot, Commit, CommitDetail, Conflict, DirEntry, Events, FileContent, GitBranches, GitDiff, GitState, GraphCommit, HunkRef, Moved, Options,
  Pasted, QuickOpen, ReplaceApplied, ReplacePreview, ReplaceQuery, ReplaceRequest, ReviewDiff, SearchFilter, SearchOptions, SearchResults, Snapshot, UpdateStatus,
} from './types';

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

  // Pasta escolhida: `subfolder` quer dizer que ela está dentro de um repositório e o projeto é a raiz dele.
  chooseProject: () => call<{ project: string; subfolder: boolean } | null>('choose_project'),
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
  stopExecutor: (id: string) => call<void>('stop_executor', { id }),
  hasRunning: () => call<boolean>('has_running'),
  shutdown: () => call<void>('shutdown'),
  // Caminho relativo à pasta do projeto, com "/"; '' é a raiz.
  listDir: (path: string) => call<DirEntry[]>('list_dir', { path }),
  // Conteúdo de um arquivo, com o mesmo caminho de `listDir`. Erro chega como mensagem em português.
  readFile: (path: string) => call<FileContent>('read_file', { path }),
  // Bytes de uma imagem ou áudio (extensão na lista de `files.rs`), como `ArrayBuffer`. Erro chega como mensagem em português; limite de 32 MiB.
  readMedia: (path: string) => call<ArrayBuffer>('read_media', { path }),
  // Grava só se o arquivo ainda é `base` (o texto lido, sem BOM) e ainda tem o BOM `bom`; senão rejeita sem gravar.
  writeFileChecked: (path: string, text: string, base: string, bom: boolean) => call<void>('write_file_checked', { path, text, base, bom }),
  // Ações de um arquivo da árvore, com o caminho relativo ao projeto. Erro vem como mensagem em português.
  // Renomear devolve o caminho novo; nunca sobrescreve. Excluir manda à Lixeira do Windows.
  renameFile: (path: string, name: string) => call<string>('rename_file', { path, name }),
  // Lote: `done` é o que foi feito e `error` o que parou (null se tudo deu certo). Nada é desfeito em caso de erro.
  deleteFiles: (paths: string[]) => call<Batch<string>>('delete_files', { paths }),
  moveFiles: (paths: string[], dest: string) => call<Batch<Moved>>('move_files', { paths, dest }),
  copyFiles: (paths: string[], cut: boolean) => call<void>('copy_files', { paths, cut }),
  revealFile: (path: string) => call<void>('reveal_file', { path }),
  openWith: (path: string) => call<void>('open_with', { path }),
  // Criação na pasta `dir` (relativa, '' é a raiz). Devolve o caminho relativo criado; nunca sobrescreve (nome que existe rejeita com a mensagem).
  createFile: (dir: string, name: string) => call<string>('create_file', { dir, name }),
  createDir: (dir: string, name: string) => call<string>('create_dir', { dir, name }),
  // Abre no Explorer a pasta `path` (relativa, '' é a raiz do projeto).
  revealDir: (path: string) => call<void>('reveal_dir', { path }),
  // Clipboard lido na hora da ação. Colar recebe essa mesma lista: a checagem de alterações e a operação usam o que foi lido aqui.
  clipboardSnapshot: () => call<ClipboardSnapshot>('clipboard_snapshot'),
  // Colagem, com o detalhe: `from` absoluto como estava no clipboard, `to` relativo ao projeto; `error` diz o que parou.
  pasteFilesDetailed: (dest: string, snap: ClipboardSnapshot) => call<Batch<Pasted>>('paste_files_detailed', { dest, snap }),
  // Painel Git. Caminhos relativos à raiz do projeto, com "/"; lista vazia em stage/unstage vale para tudo.
  // Estado do Git da pasta (ver `GitState`). Erro chega como mensagem em português.
  gitStatus: () => call<GitState>('git_status'),
  // Cria o repositório na pasta do projeto; só no modo `no-repo`, nos outros rejeita com a mensagem.
  gitInit: () => call<void>('git_init'),
  gitStage: (paths: string[]) => call<void>('git_stage', { paths }),
  gitUnstage: (paths: string[]) => call<void>('git_unstage', { paths }),
  gitCommit: (message: string) => call<void>('git_commit', { message }),
  gitFetch: () => call<string>('git_fetch'),
  gitPull: () => call<string>('git_pull'),
  gitPush: () => call<string>('git_push'),
  gitBranches: () => call<GitBranches>('git_branches'),
  gitSwitch: (name: string) => call<void>('git_switch', { name }),
  gitCreateBranch: (name: string) => call<void>('git_create_branch', { name }),
  // Revisão por bloco. `revision` do diff e de cada bloco (`HunkRef`) é o que a tela leu; mudança depois disso é recusada.
  gitReviewDiff: (path: string, staged: boolean) => call<ReviewDiff>('git_review_diff', { path, staged }),
  gitStageHunk: (hunk: HunkRef) => call<void>('git_stage_hunk', { hunk }),
  gitUnstageHunk: (hunk: HunkRef) => call<void>('git_unstage_hunk', { hunk }),
  gitDiscardHunk: (hunk: HunkRef) => call<void>('git_discard_hunk', { hunk }),
  gitDiscardFile: (path: string, revision: string) => call<void>('git_discard_file', { path, revision }),
  // Histórico: `limit` até 200 e `skip` os já mostrados; com `path`, só os commits que tocaram o arquivo.
  gitHistory: (limit: number, skip: number, path?: string) => call<Commit[]>('git_history', { limit, skip, path: path ?? null }),
  // Gráfico: HEAD e o upstream da branch atual, mais novo primeiro; `limit` até 200 e `skip` os já mostrados. Sem commit, lista vazia.
  gitGraph: (limit: number, skip: number) => call<GraphCommit[]>('git_graph', { limit, skip }),
  gitCommitDetail: (id: string) => call<CommitDetail>('git_commit_detail', { id }),
  gitCommitDiff: (id: string, path?: string) => call<GitDiff>('git_commit_diff', { id, path: path ?? null }),
  // Conflito: os lados, o texto atual e a revisão. A gravação da resolução só vale se a pasta ainda está nessa revisão.
  gitConflict: (path: string) => call<Conflict>('git_conflict', { path }),
  gitWriteResolution: (path: string, content: string, revision: string, markResolved: boolean) =>
    call<void>('git_write_resolution', { path, content, revision, markResolved }),
  gitAcceptDeletion: (path: string, revision: string) => call<void>('git_accept_deletion', { path, revision }),
  // Pesquisa de texto nos arquivos do projeto. Regex inválida rejeita com a mensagem em português; consulta vazia devolve lista vazia.
  // Filtros de caminho são opcionais; sem eles, busca em todos os arquivos.
  search: (query: string, options: SearchOptions, filter: SearchFilter = { include: [], exclude: [] }) =>
    call<SearchResults>('search', { query: { text: query, ...options, ...filter } }),
  // Abertura rápida pelo nome ou caminho (termos separados por espaço). Só rastreados e não ignorados.
  quickOpen: (term: string) => call<QuickOpen>('quick_open', { term }),
  // Prévia da substituição: não grava. A aplicação só toca nos arquivos escolhidos da prévia, com a revisão que ela mostrou; erros voltam por arquivo.
  replacePreview: (query: ReplaceQuery, replacement: string) => call<ReplacePreview>('replace_preview', { query, replacement }),
  replaceApply: (request: ReplaceRequest) => call<ReplaceApplied>('replace_apply', { request }),
};

/** Erros do backend já chegam como mensagem em português. */
export const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
