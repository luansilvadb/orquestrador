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

export async function connect(): Promise<void> {
  if ('__TAURI_INTERNALS__' in window) transport = tauri;
  else if (import.meta.env.DEV) transport = (await import('./mock')).mock;
  else throw new Error('Esta página só funciona dentro do app Orquestrador.');
}

// Chamada tipada pelo mapa `Commands`: o nome, os argumentos e o resultado vêm de lá. Sem argumento, não se passa nada.
type CallArgs<K extends CommandName> = Commands[K]['args'] extends NoArgs ? [] : [args: Commands[K]['args']];
const call = <K extends CommandName>(cmd: K, ...rest: CallArgs<K>) =>
  transport.invoke(cmd, rest[0] as Record<string, unknown> | undefined) as Promise<Commands[K]['result']>;

/** Acesso direto à janela e aos eventos (escuta, fechamento, barra de título). */
export const host = (): Transport => transport;

export const backend = {
  updateStatus: () => call('update_status'),
  retryUpdate: () => call('retry_update'),

  // Pasta escolhida: `subfolder` quer dizer que ela está dentro de um repositório e o projeto é a raiz dele.
  chooseProject: () => call('choose_project'),
  getOptions: () => call('get_options'),
  startOrchestrator: (choice: Choice, cols: number, rows: number) =>
    call('start_orchestrator', { choice, cols, rows }),
  // Shell do usuário no terminal de `id` (gerado pelo frontend, "shell-<n>"), na pasta do projeto ou em `cwd` (relativa ao projeto, com "/"; '' é a raiz). Devolve o nome do programa. Erro vem como mensagem em português.
  startShell: (id: string, cols: number, rows: number, cwd?: string) => call('start_shell', { id, cols, rows, cwd }),
  // Mata o shell de `id` sem gerar evento de saída.
  closeShell: (id: string) => call('close_shell', { id }),
  setExecutorChoice: (choice: Choice) => call('set_executor_choice', { choice }),
  termWrite: (id: string, data: string) => call('term_write', { id, data }),
  // O CLI que roda agora no terminal `id`, pelo processo (num shell, muda quando ele abre e quando sai); `null` é shell comum.
  termCli: (id: string) => call('term_cli', { id }),
  termResize: (id: string, cols: number, rows: number) => call('term_resize', { id, cols, rows }),
  termSnapshot: (id: string) => call('term_snapshot', { id }),
  stopExecutor: (id: string) => call('stop_executor', { id }),
  hasRunning: () => call('has_running'),
  // Caminho relativo à pasta do projeto, com "/"; '' é a raiz.
  listDir: (path: string) => call('list_dir', { path }),
  // Conteúdo de um arquivo, com o mesmo caminho de `listDir`. Erro chega como mensagem em português.
  readFile: (path: string) => call('read_file', { path }),
  // Bytes de uma imagem ou áudio (extensão na lista de `files.rs`), como `ArrayBuffer`. Erro chega como mensagem em português; limite de 32 MiB.
  readMedia: (path: string) => call('read_media', { path }),
  // Grava só se o arquivo ainda é `base` (o texto lido, sem BOM) e ainda tem o BOM `bom`; senão rejeita sem gravar.
  writeFileChecked: (path: string, text: string, base: string, bom: boolean) => call('write_file_checked', { path, text, base, bom }),
  // Ações de um arquivo da árvore, com o caminho relativo ao projeto. Erro vem como mensagem em português.
  // Renomear devolve o caminho novo; nunca sobrescreve. Excluir manda à Lixeira do Windows.
  renameFile: (path: string, name: string) => call('rename_file', { path, name }),
  // Lote: `done` é o que foi feito e `error` o que parou (null se tudo deu certo). Nada é desfeito em caso de erro.
  deleteFiles: (paths: string[]) => call('delete_files', { paths }),
  moveFiles: (paths: string[], dest: string) => call('move_files', { paths, dest }),
  copyFiles: (paths: string[], cut: boolean) => call('copy_files', { paths, cut }),
  revealFile: (path: string) => call('reveal_file', { path }),
  openWith: (path: string) => call('open_with', { path }),
  // Criação na pasta `dir` (relativa, '' é a raiz). Devolve o caminho relativo criado; nunca sobrescreve (nome que existe rejeita com a mensagem).
  createFile: (dir: string, name: string) => call('create_file', { dir, name }),
  createDir: (dir: string, name: string) => call('create_dir', { dir, name }),
  // Abre no Explorer a pasta `path` (relativa, '' é a raiz do projeto).
  revealDir: (path: string) => call('reveal_dir', { path }),
  // Clipboard lido na hora da ação. Colar recebe essa mesma lista: a checagem de alterações e a operação usam o que foi lido aqui.
  clipboardSnapshot: () => call('clipboard_snapshot'),
  // Colagem, com o detalhe: `from` absoluto como estava no clipboard, `to` relativo ao projeto; `error` diz o que parou.
  pasteFilesDetailed: (dest: string, snap: ClipboardSnapshot) => call('paste_files_detailed', { dest, snap }),
  // Painel Git. Caminhos relativos à raiz do projeto, com "/"; lista vazia em stage/unstage vale para tudo.
  // Estado do Git da pasta (ver `GitState`). Erro chega como mensagem em português.
  gitStatus: () => call('git_status'),
  // Cria o repositório na pasta do projeto; só no modo `no-repo`, nos outros rejeita com a mensagem.
  gitInit: () => call('git_init'),
  gitStage: (paths: string[]) => call('git_stage', { paths }),
  gitUnstage: (paths: string[]) => call('git_unstage', { paths }),
  gitCommit: (message: string) => call('git_commit', { message }),
  gitFetch: () => call('git_fetch'),
  gitPull: () => call('git_pull'),
  gitPush: () => call('git_push'),
  gitBranches: () => call('git_branches'),
  gitSwitch: (name: string) => call('git_switch', { name }),
  gitCreateBranch: (name: string) => call('git_create_branch', { name }),
  // Revisão por bloco. `revision` do diff e de cada bloco (`HunkRef`) é o que a tela leu; mudança depois disso é recusada.
  gitReviewDiff: (path: string, staged: boolean) => call('git_review_diff', { path, staged }),
  gitStageHunk: (hunk: HunkRef) => call('git_stage_hunk', { hunk }),
  gitUnstageHunk: (hunk: HunkRef) => call('git_unstage_hunk', { hunk }),
  gitDiscardHunk: (hunk: HunkRef) => call('git_discard_hunk', { hunk }),
  gitDiscardFile: (path: string, revision: string) => call('git_discard_file', { path, revision }),
  // Histórico: `limit` até 200 e `skip` os já mostrados; com `path`, só os commits que tocaram o arquivo.
  gitHistory: (limit: number, skip: number, path?: string) => call('git_history', { limit, skip, path: path ?? null }),
  // Gráfico: HEAD e o upstream da branch atual, mais novo primeiro; `limit` até 200 e `skip` os já mostrados. Sem commit, lista vazia.
  gitGraph: (limit: number, skip: number) => call('git_graph', { limit, skip }),
  gitCommitDetail: (id: string) => call('git_commit_detail', { id }),
  gitCommitDiff: (id: string, path: string) => call('git_commit_diff', { id, path }),
  // Conflito: os lados, o texto atual e a revisão. A gravação da resolução só vale se a pasta ainda está nessa revisão.
  gitConflict: (path: string) => call('git_conflict', { path }),
  gitWriteResolution: (path: string, content: string, revision: string, markResolved: boolean) =>
    call('git_write_resolution', { path, content, revision, markResolved }),
  gitAcceptDeletion: (path: string, revision: string) => call('git_accept_deletion', { path, revision }),
  // Pesquisa de texto nos arquivos do projeto. Regex inválida rejeita com a mensagem em português; consulta vazia devolve lista vazia.
  search: (query: string, options: SearchOptions, filter: SearchFilter) =>
    call('search', { query: { text: query, ...options, ...filter } }),
  // Abertura rápida pelo nome ou caminho (termos separados por espaço). Só rastreados e não ignorados.
  quickOpen: (term: string) => call('quick_open', { term }),
  // Prévia da substituição: não grava. A aplicação só toca nos arquivos escolhidos da prévia, com a revisão que ela mostrou; erros voltam por arquivo.
  replacePreview: (query: ReplaceQuery, replacement: string) => call('replace_preview', { query, replacement }),
  replaceApply: (request: ReplaceRequest) => call('replace_apply', { request }),
};

/** Erros do backend já chegam como mensagem em português. */
export const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
