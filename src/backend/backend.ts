// Único ponto de contato com o Tauri. Fora do app (navegador, em desenvolvimento) entra o simulado.
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type {
  Choice, ClipboardSnapshot, CommandName, Commands, Events, HunkRef, NoArgs, ReplaceQuery, ReplaceRequest, SearchFilter, SearchOptions,
} from './types';

export interface Transport {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  /** Responde o cancelamento da escuta. */
  listen<K extends keyof Events>(event: K, handler: (payload: Events[K]) => void): Promise<() => void>;
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
  /** URL de vídeo para o `<video>`: no app, o protocolo `orq-media` (streaming com Range); `project` null é caminho absoluto. */
  mediaUrl(path: string, project: string | null): string;
}

const tauri: Transport = {
  invoke,
  listen: (event, handler) => listen(event, (e) => handler(e.payload as never)),
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
  mediaUrl: (path, project) => `${convertFileSrc('video', 'orq-media')}?path=${encodeURIComponent(path)}`
    + (project === null ? '' : `&project=${encodeURIComponent(project)}`),
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
type ProjectCommand = { [K in CommandName]: Commands[K]['args'] extends { project: string } ? K : never }[CommandName];
const requireProjectId = (id: string | null) => {
  if (!id) throw new Error('Escolha a pasta do projeto primeiro.');
  return id;
};
const projectCall = <K extends ProjectCommand>(id: string | null, cmd: K, args: Omit<Commands[K]['args'], 'project'>) =>
  transport.invoke(cmd, { ...args, project: requireProjectId(id) } as Record<string, unknown>) as Promise<Commands[K]['result']>;
const shellProject = (id: string | null, project?: string | null) => project === undefined ? id : project;
const absolutePath = (path: string) => /^(?:[a-z]:[\\/]|\\\\|\/)/i.test(path);

/** Acesso direto à janela e aos eventos (escuta, fechamento, barra de título). */
export const host = (): Transport => transport;

export const projectBackend = (projectId: string | null) => ({
  updateStatus: () => call('update_status'),
  retryUpdate: () => call('retry_update'),
  takeOpenPaths: () => call('take_open_paths'),

  // Pasta escolhida: `subfolder` quer dizer que ela está dentro de um repositório e o projeto é a raiz dele.
  chooseProject: () => call('choose_project'),
  openProject: (path: string) => call('open_project', { path }),
  closeProject: (id = requireProjectId(projectId)) => call('close_project', { project: id }),
  listProjects: () => call('list_projects'),
  projectSnapshot: (id = requireProjectId(projectId)) => call('project_snapshot', { project: id }),
  pickFile: () => call('pick_file'),
  pickSavePath: (directory: string | null, fileName: string) => call('pick_save_path', { directory, fileName }),
  getOptions: () => call('get_options'),
  startOrchestrator: (choice: Choice, cols: number, rows: number) =>
    projectCall(projectId, 'start_orchestrator', { choice, cols, rows }),
  // Shell do usuário no terminal de `id` (gerado pelo frontend, "shell-<n>"), na pasta do projeto ou em `cwd` (relativa ao projeto, com "/"; '' é a raiz). Devolve o nome do programa. Erro vem como mensagem em português.
  startShell: (id: string, cols: number, rows: number, cwd?: string, project?: string | null) => call('start_shell', { project: shellProject(projectId, project), id, cols, rows, cwd }),
  // Mata o shell de `id` sem gerar evento de saída.
  closeShell: (id: string, project?: string | null) => call('close_shell', { project: shellProject(projectId, project), id }),
  setExecutorChoice: (choice: Choice) => call('set_executor_choice', { choice }),
  termWrite: (id: string, data: string, project?: string | null) => call('term_write', { project: shellProject(projectId, project), id, data }),
  // O CLI que roda agora no terminal `id`, pelo processo (num shell, muda quando ele abre e quando sai); `null` é shell comum.
  termCli: (id: string, project?: string | null) => call('term_cli', { project: shellProject(projectId, project), id }),
  termResize: (id: string, cols: number, rows: number, project?: string | null) => call('term_resize', { project: shellProject(projectId, project), id, cols, rows }),
  termSnapshot: (id: string, project?: string | null) => call('term_snapshot', { project: shellProject(projectId, project), id }),
  stopExecutor: (id: string) => projectCall(projectId, 'stop_executor', { id }),
  hasRunning: () => call('has_running'),
  // Caminho relativo à pasta do projeto, com "/"; '' é a raiz.
  listDir: (path: string) => projectCall(projectId, 'list_dir', { path }),
  // Conteúdo de um arquivo, com o mesmo caminho de `listDir`. Erro chega como mensagem em português.
  readFile: (path: string) => absolutePath(path) ? call('read_file_at', { path }) : projectCall(projectId, 'read_file', { path }),
  readFileAt: (path: string) => call('read_file_at', { path }),
  // Bytes de uma imagem ou áudio (extensão na lista de `files.rs`), como `ArrayBuffer`. Erro chega como mensagem em português; limite de 32 MiB.
  readMedia: (path: string) => absolutePath(path) ? call('read_media_at', { path }) : projectCall(projectId, 'read_media', { path }),
  readMediaAt: (path: string) => call('read_media_at', { path }),
  // Vídeo não passa pelo IPC: o `<video>` lê a URL em pedaços.
  mediaUrl: (path: string) => transport.mediaUrl(path, absolutePath(path) ? null : requireProjectId(projectId)),
  formatterInfo: (path: string) => call('formatter_info', { path }),
  formatCli: (path: string, text: string) => call('format_cli', { path, text }),
  // Grava só se o arquivo ainda é `base` (o texto lido, sem BOM) e ainda tem o BOM `bom`; senão rejeita sem gravar.
  writeFileChecked: (path: string, text: string, base: string, bom: boolean) => absolutePath(path)
    ? call('write_file_at_checked', { path, text, base, bom })
    : projectCall(projectId, 'write_file_checked', { path, text, base, bom }),
  writeFileAtChecked: (path: string, text: string, base: string, bom: boolean) => call('write_file_at_checked', { path, text, base, bom }),
  // Ações de um arquivo da árvore, com o caminho relativo ao projeto. Erro vem como mensagem em português.
  // Renomear devolve o caminho novo; nunca sobrescreve. Excluir manda à Lixeira do Windows.
  renameFile: (path: string, name: string) => projectCall(projectId, 'rename_file', { path, name }),
  // Lote: `done` é o que foi feito e `error` o que parou (null se tudo deu certo). Nada é desfeito em caso de erro.
  deleteFiles: (paths: string[]) => projectCall(projectId, 'delete_files', { paths }),
  moveFiles: (paths: string[], dest: string) => projectCall(projectId, 'move_files', { paths, dest }),
  copyFiles: (paths: string[], cut: boolean) => projectCall(projectId, 'copy_files', { paths, cut }),
  revealFile: (path: string) => projectCall(projectId, 'reveal_file', { path }),
  openWith: (path: string) => projectCall(projectId, 'open_with', { path }),
  openUrl: (url: string) => call('open_url', { url }),
  // Criação na pasta `dir` (relativa, '' é a raiz). Devolve o caminho relativo criado; nunca sobrescreve (nome que existe rejeita com a mensagem).
  createFile: (dir: string, name: string) => projectCall(projectId, 'create_file', { dir, name }),
  createDir: (dir: string, name: string) => projectCall(projectId, 'create_dir', { dir, name }),
  // Abre no Explorer a pasta `path` (relativa, '' é a raiz do projeto).
  revealDir: (path: string) => projectCall(projectId, 'reveal_dir', { path }),
  // Clipboard lido na hora da ação. Colar recebe essa mesma lista: a checagem de alterações e a operação usam o que foi lido aqui.
  clipboardSnapshot: () => call('clipboard_snapshot'),
  // Colagem, com o detalhe: `from` absoluto como estava no clipboard, `to` relativo ao projeto; `error` diz o que parou.
  pasteFilesDetailed: (dest: string, snap: ClipboardSnapshot) => projectCall(projectId, 'paste_files_detailed', { dest, snap }),
  // Painel Git. Caminhos relativos à raiz do projeto, com "/"; lista vazia em stage/unstage vale para tudo.
  // Estado do Git da pasta (ver `GitState`). Erro chega como mensagem em português.
  gitStatus: () => projectCall(projectId, 'git_status', {}),
  // Cria o repositório na pasta do projeto; só no modo `no-repo`, nos outros rejeita com a mensagem.
  gitInit: () => projectCall(projectId, 'git_init', {}),
  gitStage: (paths: string[]) => projectCall(projectId, 'git_stage', { paths }),
  gitUnstage: (paths: string[]) => projectCall(projectId, 'git_unstage', { paths }),
  gitCommit: (message: string) => projectCall(projectId, 'git_commit', { message }),
  gitFetch: () => projectCall(projectId, 'git_fetch', {}),
  gitPull: () => projectCall(projectId, 'git_pull', {}),
  gitPush: () => projectCall(projectId, 'git_push', {}),
  gitBranches: () => projectCall(projectId, 'git_branches', {}),
  gitSwitch: (name: string) => projectCall(projectId, 'git_switch', { name }),
  gitCreateBranch: (name: string) => projectCall(projectId, 'git_create_branch', { name }),
  // Revisão por bloco. `revision` do diff e de cada bloco (`HunkRef`) é o que a tela leu; mudança depois disso é recusada.
  gitReviewDiff: (path: string, staged: boolean) => projectCall(projectId, 'git_review_diff', { path, staged }),
  gitStageHunk: (hunk: HunkRef) => projectCall(projectId, 'git_stage_hunk', { hunk }),
  gitUnstageHunk: (hunk: HunkRef) => projectCall(projectId, 'git_unstage_hunk', { hunk }),
  gitDiscardHunk: (hunk: HunkRef) => projectCall(projectId, 'git_discard_hunk', { hunk }),
  gitDiscardFile: (path: string, revision: string) => projectCall(projectId, 'git_discard_file', { path, revision }),
  // Histórico: `limit` até 200 e `skip` os já mostrados; com `path`, só os commits que tocaram o arquivo.
  gitHistory: (limit: number, skip: number, path?: string) => projectCall(projectId, 'git_history', { limit, skip, path: path ?? null }),
  // Gráfico: HEAD e o upstream da branch atual, mais novo primeiro; `limit` até 200 e `skip` os já mostrados. Sem commit, lista vazia.
  gitGraph: (limit: number, skip: number) => projectCall(projectId, 'git_graph', { limit, skip }),
  gitCommitDetail: (id: string) => projectCall(projectId, 'git_commit_detail', { id }),
  gitCommitHover: (hash: string) => projectCall(projectId, 'git_commit_hover', { hash }),
  gitCommitDiff: (id: string, path: string) => projectCall(projectId, 'git_commit_diff', { id, path }),
  // Conflito: os lados, o texto atual e a revisão. A gravação da resolução só vale se a pasta ainda está nessa revisão.
  gitConflict: (path: string) => projectCall(projectId, 'git_conflict', { path }),
  gitWriteResolution: (path: string, content: string, revision: string, markResolved: boolean) =>
    projectCall(projectId, 'git_write_resolution', { path, content, revision, markResolved }),
  gitAcceptDeletion: (path: string, revision: string) => projectCall(projectId, 'git_accept_deletion', { path, revision }),
  // Texto do arquivo no HEAD e no índice, base dos marcadores de mudança do editor. `null` sem marcadores (fora de repositório, não rastreado, binário ou acima de 1 MiB).
  gitFileBase: (path: string) => projectCall(projectId, 'git_file_base', { path }),
  // Grava `text` (com `\n`) como o arquivo no índice: preparar e tirar do preparo um bloco no editor. `expected` é o índice que a tela
  // viu (normalizado); se mudou, ou se há executores trabalhando, rejeita com a mensagem em português.
  gitWriteIndex: (path: string, text: string, expected: string) => projectCall(projectId, 'git_write_index', { path, text, expected }),
  // Pesquisa de texto nos arquivos do projeto. Regex inválida rejeita com a mensagem em português; consulta vazia devolve lista vazia.
  search: (query: string, options: SearchOptions, filter: SearchFilter) =>
    projectCall(projectId, 'search', { query: { text: query, ...options, ...filter } }),
  // Abertura rápida pelo nome ou caminho (termos separados por espaço). Só rastreados e não ignorados.
  quickOpen: (term: string) => projectCall(projectId, 'quick_open', { term }),
  // Prévia da substituição: não grava. A aplicação só toca nos arquivos escolhidos da prévia, com a revisão que ela mostrou; erros voltam por arquivo.
  replacePreview: (query: ReplaceQuery, replacement: string) => projectCall(projectId, 'replace_preview', { query, replacement }),
  replaceApply: (request: ReplaceRequest) => projectCall(projectId, 'replace_apply', { request }),
});

export const backend = projectBackend(null);

/** Erros do backend já chegam como mensagem em português. */
export const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
