// Tipos do contrato (JSON, camelCase). '' em modelo ou esforço = padrão do CLI.
export type Tool = 'claude' | 'codex';
// Nome de cada ferramenta na tela; o valor técnico (`Tool`) é o que vai ao backend.
export const TOOL_NAME: Record<Tool, string> = { claude: 'Claude', codex: 'Codex' };
export type Choice = { tool: Tool; model: string; effort: string };
export type ModelOption = { name: string; efforts: string[] };
export type ToolOption = { name: Tool; efforts: string[]; models: ModelOption[] };
export type Options = {
  tools: ToolOption[];
  last: { orchestrator: Choice; executor: Choice };
};
export type NodeState = 'awaiting-choice' | 'queued' | 'running' | 'done' | 'stopped' | 'failed';
export type ExecutorNode = {
  id: string;
  batch: string;
  index: number;
  title: string;
  state: NodeState;
  choice: Choice | null;
};

// Atualização na abertura: o uso só é liberado quando o estado chega a `ready`.
export type UpdateStatus =
  | { phase: 'checking' }
  | { phase: 'downloading'; version: string; downloaded: number; total: number | null }
  | { phase: 'installing'; version: string }
  | { phase: 'ready' }
  | { phase: 'failed'; message: string };

// Uma entrada de `list_dir`: um nível só. A ordem é a do backend (pastas primeiro, depois arquivos, alfabética).
// `ignored`: o `.gitignore` ignora a entrada (a árvore a mostra apagada); sempre falso fora de um repositório.
export type DirEntry = { name: string; dir: boolean; ignored: boolean };
// Arquivos do clipboard do Windows: caminhos absolutos, na ordem, e se foram recortados (mover) ou copiados.
export type ClipboardSnapshot = { paths: string[]; cut: boolean };

// Uma leitura de `read_file`: o texto como está no disco, sem o BOM (que vem em `bom`). Binário e arquivo acima de 1 MiB voltam sem texto.
// `bom` indica o BOM UTF-8 do disco; `text` vem sem ele. A gravação checada manda o texto cru e o `bom` de volta.
export type FileContent = { binary: boolean; tooLarge: boolean; readOnly: boolean; bom: boolean; text: string };

// Painel Git (contrato do backend, JSON em camelCase). Um arquivo pode estar nas duas seções ao mesmo tempo.
export type GitKind = 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked' | 'conflicted';
export type GitFile = { path: string; origPath: string | null; staged: GitKind | null; unstaged: GitKind | null };
export type GitStatus = { branch: string; ahead: number; behind: number; head: string; files: GitFile[] };
// `head` é o hash completo do HEAD; '' sem commit.
// `patch` é o diff unificado do arquivo (cabeçalho e `@@`); vazio quando não há alteração.
export type GitDiff = { binary: boolean; tooLarge: boolean; patch: string };
// Estado do Git da pasta do projeto (retorno de `git_status`). Só `repo` tem o painel completo; os outros dizem por que não há.
export type GitState =
  // A raiz do projeto é um repositório: o status é o de sempre.
  | { kind: 'repo'; status: GitStatus }
  // A pasta não está em nenhum repositório; `git_init` cria um.
  | { kind: 'no-repo' }
  // O Git não foi encontrado neste computador.
  | { kind: 'no-git' }
  // A pasta está dentro de um repositório cuja raiz (`root`) não é o projeto.
  | { kind: 'repo-above'; root: string };
export type GitBranches = { current: string; local: string[] };

// Revisão por bloco (contrato do backend). `revision` é a impressão digital do diff que a tela viu; um bloco é identificado por `index` e `header`.
export type HunkRef = { path: string; index: number; header: string; revision: string };
// `untracked`: o arquivo é novo para o Git, então o patch é o arquivo inteiro e não há blocos a agir.
// `diff` é o bloco (-U3), base da revisão e das ações; `display` é o arquivo inteiro, só para o comparador.
export type ReviewDiff = { diff: GitDiff; display: GitDiff; revision: string; untracked: boolean };
// Histórico e commits. `date` vem em ISO 8601; a tela formata.
export type Commit = { hash: string; short: string; author: string; date: string; subject: string };
export type CommitFile = { path: string; origPath: string | null; kind: GitKind };
export type CommitDetail = { commit: Commit; files: CommitFile[] };
export type GitCommitHoverDetail = {
  author: string;
  date: string;
  message: string;
  filesChanged: number;
  insertions: number;
  deletions: number;
  githubUrl: string | null;
};
// Gráfico: HEAD e o upstream da branch atual. `parents` são hashes completos (vazio na raiz); `refs` são as que apontam para o commit.
// `current` só na branch em uso (ou no HEAD destacado); ordem: current, branches, remotes, tags.
export type GitRef = { name: string; kind: 'branch' | 'remote' | 'tag' | 'head'; current: boolean };
export type GraphCommit = Commit & { parents: string[]; refs: GitRef[] };
// Conflito de merge: `ours` e `theirs` são os lados (`null` se o lado apagou o arquivo); `worktree` é o texto atual, sem BOM e com `\n`.
// `revision` é a da pasta: a gravação da resolução manda de volta a que a tela leu.
export type Conflict = { ours: string | null; theirs: string | null; worktree: string | null; binary: boolean; revision: string };
// Texto-base do Git de um arquivo, para os marcadores de mudança do editor. `head` é o texto no HEAD (`null` se não está lá) e `index` o do
// índice, estágio 0 (`null` se não está lá, ex. em conflito). Vêm com o fim de linha do checkout e sem BOM, comparáveis com o `text` de `read_file`.
export type FileBase = { head: string | null; index: string | null };
export type FormatterInfo =
  | { kind: 'prettier'; config: string | null; configKind: 'json' | 'yaml' | 'auto'; options: Record<string, unknown> }
  | { kind: 'cli'; formatter: 'ruff' | 'black' | 'rustfmt' | 'gofmt' }
  | { kind: 'unavailable'; reason: string };

// Pesquisa de texto (contrato do backend, JSON em camelCase). `path` é relativo à raiz, com "/"; uma entrada por linha que casa.
// `col` é a coluna da primeira ocorrência, em unidades UTF-16 (como o CodeMirror) e a partir de 1; emoji ocupa 2. `skipped` lista os arquivos que ficaram de fora e por quê
// (até 500; `skippedOmitted` conta o resto). `truncated` avisa que passou de 2000 linhas casadas.
export type SearchMatch = { line: number; col: number; text: string };
export type SearchFile = { path: string; matches: SearchMatch[] };
export type SkipReason = 'binary' | 'notUtf8' | 'tooLarge' | 'notFile' | 'unreadable';
export type Skipped = { path: string; reason: SkipReason };
export type SearchResults = { files: SearchFile[]; truncated: boolean; skipped: Skipped[]; skippedOmitted: number };
export type SearchOptions = { caseSensitive: boolean; wholeWord: boolean; regex: boolean };
// Filtros de caminho: trechos separados por vírgula na tela, `*` qualquer sequência e `?` um caractere, sem diferenciar caixa. Lista vazia de `include` quer dizer todos.
// Trecho com `./` na frente vale a partir da raiz (`./src` é a pasta `src`); sem ele, casa em qualquer ponto do caminho.
// `only`: só estes caminhos exatos (os editores abertos); ausente é sem esse recorte, lista vazia não deixa nada.
export type SearchFilter = { include: string[]; exclude: string[]; only?: string[] };
// Abertura rápida: caminhos que casam com os termos (até 100) e o total que casou.
export type QuickOpen = { paths: string[]; total: number };

// Substituição em vários arquivos. A prévia não grava; a aplicação só toca nos arquivos escolhidos, com a revisão que a prévia mostrou.
// `preserveCase`: a substituição segue a caixa da ocorrência (toda maiúscula, toda minúscula ou inicial maiúscula). Só vale na substituição.
export type ReplaceQuery = SearchOptions & SearchFilter & { text: string; preserveCase?: boolean };
export type PreviewFile = { path: string; revision: string; count: number; original: string; result: string };
export type ReplacePreview = {
  files: PreviewFile[];
  // Arquivos com ocorrência que não entraram em `files` (limite de arquivos ou de texto): não são aplicáveis por esta prévia.
  omittedFiles: number;
};
export type ReplacePick = { path: string; revision: string };
export type ReplaceRequest = { query: ReplaceQuery; replacement: string; files: ReplacePick[] };
export type ReplaceApplied = { applied: { path: string; count: number }[]; errors: { path: string; error: string }[] };

// Resultado de ação em lote (mover, mandar à Lixeira, colar): o que foi feito e o erro do que parou (`null` se tudo deu certo).
// `done` vem mesmo com erro: o que já foi feito não é desfeito.
export type Batch<T> = { done: T[]; error: string | null };
// Mover: `from` e `to` relativos ao projeto; em pasta, o que está dentro acompanha o prefixo.
// Colar usa o mesmo tipo: `from` é o caminho absoluto que estava no clipboard; `to` é o relativo ao projeto do que foi criado.
export type Moved = { from: string; to: string };

export type TermData = { project: string | null; id: string; seq: number; data: string };
export type Snapshot = { seq: number; data: string };
export type NodeStateEvent = { project: string; id: string; state: NodeState; choice: Choice | null };
export type OpenedProject = { id: string; project: string; subfolder: boolean; existing: boolean };
export type ProjectSnapshot = { orchestratorOpen: boolean; executorsRunning: number; executorsQueued: number };

export type Events = {
  'term-data': TermData;
  'node-added': ExecutorNode & { project: string };
  'node-state': NodeStateEvent;
  'orchestrator-exit': { project: string; code: number | null };
  'shell-exit': { project: string | null; id: string; code: number | null };
  'update-status': UpdateStatus;
  'open-paths': void;
};

// Contrato dos comandos do Tauri. A chave é o nome que vai ao `invoke` (snake_case); `args` são as chaves como o front-end envia
// (`Option` do Rust vira `| null`, ou opcional quando o front-end pode omitir) e `result` é o retorno (`void` quando não há).
// Comando sem argumento tem `args: NoArgs`. `backend.ts` e `mock.ts` são tipados por este mapa; `tests/check-commands.mjs` confere as chaves com `lib.rs`.
export type NoArgs = Record<string, never>;
export type ProjectArgs<T = NoArgs> = { project: string } & T;
export type Commands = {
  choose_project: { args: NoArgs; result: OpenedProject | null };
  open_project: { args: { path: string }; result: OpenedProject };
  close_project: { args: { project: string }; result: void };
  list_projects: { args: NoArgs; result: OpenedProject[] };
  project_snapshot: { args: { project: string }; result: ProjectSnapshot };
  pick_file: { args: NoArgs; result: string | null };
  pick_save_path: { args: { directory: string | null; fileName: string }; result: string | null };
  get_options: { args: NoArgs; result: Options };
  list_dir: { args: ProjectArgs<{ path: string }>; result: DirEntry[] };
  read_file: { args: ProjectArgs<{ path: string }>; result: FileContent };
  read_file_at: { args: { path: string }; result: FileContent };
  read_media: { args: ProjectArgs<{ path: string }>; result: ArrayBuffer };
  read_media_at: { args: { path: string }; result: ArrayBuffer };
  formatter_info: { args: { path: string }; result: FormatterInfo };
  format_cli: { args: { path: string; text: string }; result: string };
  write_file_checked: { args: ProjectArgs<{ path: string; text: string; base: string; bom: boolean }>; result: void };
  write_file_at_checked: { args: { path: string; text: string; base: string; bom: boolean }; result: void };
  rename_file: { args: ProjectArgs<{ path: string; name: string }>; result: string };
  delete_files: { args: ProjectArgs<{ paths: string[] }>; result: Batch<string> };
  move_files: { args: ProjectArgs<{ paths: string[]; dest: string }>; result: Batch<Moved> };
  copy_files: { args: ProjectArgs<{ paths: string[]; cut: boolean }>; result: void };
  reveal_file: { args: ProjectArgs<{ path: string }>; result: void };
  reveal_dir: { args: ProjectArgs<{ path: string }>; result: void };
  open_with: { args: ProjectArgs<{ path: string }>; result: void };
  open_url: { args: { url: string }; result: void };
  create_file: { args: ProjectArgs<{ dir: string; name: string }>; result: string };
  create_dir: { args: ProjectArgs<{ dir: string; name: string }>; result: string };
  clipboard_snapshot: { args: NoArgs; result: ClipboardSnapshot };
  paste_files_detailed: { args: ProjectArgs<{ dest: string; snap: ClipboardSnapshot }>; result: Batch<Moved> };
  search: { args: ProjectArgs<{ query: SearchOptions & SearchFilter & { text: string } }>; result: SearchResults };
  quick_open: { args: ProjectArgs<{ term: string }>; result: QuickOpen };
  replace_preview: { args: ProjectArgs<{ query: ReplaceQuery; replacement: string }>; result: ReplacePreview };
  replace_apply: { args: ProjectArgs<{ request: ReplaceRequest }>; result: ReplaceApplied };
  git_status: { args: ProjectArgs; result: GitState };
  git_init: { args: ProjectArgs; result: void };
  git_stage: { args: ProjectArgs<{ paths: string[] }>; result: void };
  git_unstage: { args: ProjectArgs<{ paths: string[] }>; result: void };
  git_commit: { args: ProjectArgs<{ message: string }>; result: void };
  git_fetch: { args: ProjectArgs; result: string };
  git_pull: { args: ProjectArgs; result: string };
  git_push: { args: ProjectArgs; result: string };
  git_branches: { args: ProjectArgs; result: GitBranches };
  git_switch: { args: ProjectArgs<{ name: string }>; result: void };
  git_create_branch: { args: ProjectArgs<{ name: string }>; result: void };
  git_review_diff: { args: ProjectArgs<{ path: string; staged: boolean }>; result: ReviewDiff };
  git_stage_hunk: { args: ProjectArgs<{ hunk: HunkRef }>; result: void };
  git_unstage_hunk: { args: ProjectArgs<{ hunk: HunkRef }>; result: void };
  git_discard_hunk: { args: ProjectArgs<{ hunk: HunkRef }>; result: void };
  git_discard_file: { args: ProjectArgs<{ path: string; revision: string }>; result: void };
  git_history: { args: ProjectArgs<{ limit: number; skip: number; path: string | null }>; result: GraphCommit[] };
  git_graph: { args: ProjectArgs<{ limit: number; skip: number }>; result: GraphCommit[] };
  git_commit_detail: { args: ProjectArgs<{ id: string }>; result: CommitDetail };
  git_commit_hover: { args: ProjectArgs<{ hash: string }>; result: GitCommitHoverDetail };
  git_commit_diff: { args: ProjectArgs<{ id: string; path: string }>; result: GitDiff };
  git_conflict: { args: ProjectArgs<{ path: string }>; result: Conflict };
  git_write_resolution: { args: ProjectArgs<{ path: string; content: string; revision: string; markResolved: boolean }>; result: void };
  git_accept_deletion: { args: ProjectArgs<{ path: string; revision: string }>; result: void };
  git_file_base: { args: ProjectArgs<{ path: string }>; result: FileBase | null };
  git_write_index: { args: ProjectArgs<{ path: string; text: string; expected: string }>; result: void };
  start_orchestrator: { args: ProjectArgs<{ choice: Choice; cols: number; rows: number }>; result: void };
  start_shell: { args: { project: string | null; id: string; cols: number; rows: number; cwd?: string }; result: string };
  close_shell: { args: { project: string | null; id: string }; result: void };
  set_executor_choice: { args: { choice: Choice }; result: void };
  term_write: { args: { project: string | null; id: string; data: string }; result: void };
  term_cli: { args: { project: string | null; id: string }; result: 'claude' | 'codex' | null };
  term_resize: { args: { project: string | null; id: string; cols: number; rows: number }; result: void };
  term_snapshot: { args: { project: string | null; id: string }; result: Snapshot };
  stop_executor: { args: ProjectArgs<{ id: string }>; result: void };
  has_running: { args: NoArgs; result: boolean };
  update_status: { args: NoArgs; result: UpdateStatus };
  retry_update: { args: NoArgs; result: UpdateStatus };
  take_open_paths: { args: NoArgs; result: string[] };
};
export type CommandName = keyof Commands;

export const ORCHESTRATOR = 'orchestrator';
