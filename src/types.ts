// Tipos do contrato (JSON, camelCase). '' em modelo ou esforço = padrão do CLI.
export type Tool = 'claude' | 'codex';
export type Choice = { tool: Tool; model: string; effort: string };
export type ModelOption = { name: string; efforts: string[] };
export type ToolOption = { name: Tool; efforts: string[]; models: ModelOption[] };
export type Options = {
  tools: ToolOption[];
  last: { orchestrator: Choice; executor: Choice };
};
export type NodeState = 'awaiting-choice' | 'queued' | 'running' | 'awaiting-orchestrator' | 'done' | 'stopped' | 'failed';
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
export type DirEntry = { name: string; dir: boolean };
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
export type GitBranches = { current: string; local: string[] };

// Revisão por bloco (contrato do backend). `revision` é a impressão digital do diff que a tela viu; um bloco é identificado por `index` e `header`.
export type HunkRef = { path: string; index: number; header: string; revision: string };
export type HunkInfo = { index: number; header: string };
export type ReviewDiff = { diff: GitDiff; revision: string; hunks: HunkInfo[] };
// Histórico e commits. `date` vem em ISO 8601; a tela formata.
export type Commit = { hash: string; short: string; author: string; date: string; subject: string };
export type CommitFile = { path: string; origPath: string | null; kind: GitKind };
export type CommitDetail = { commit: Commit; files: CommitFile[] };
// Gráfico: HEAD e o upstream da branch atual. `parents` são hashes completos (vazio na raiz); `refs` são as que apontam para o commit.
// `current` só na branch em uso (ou no HEAD destacado); ordem: current, branches, remotes, tags.
export type GitRef = { name: string; kind: 'branch' | 'remote' | 'tag' | 'head'; current: boolean };
export type GraphCommit = Commit & { parents: string[]; refs: GitRef[] };
// Conflito de merge: `base`, `ours` e `theirs` são os lados (`null` se o lado apagou o arquivo); `worktree` é o texto atual, sem BOM e com `\n`.
// `revision` é a da pasta: a gravação da resolução manda de volta a que a tela leu.
export type Conflict = {
  path: string; base: string | null; ours: string | null; theirs: string | null; worktree: string | null;
  binary: boolean; crlf: boolean; bom: boolean; revision: string;
};

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
export type SearchFilter = { include: string[]; exclude: string[] };
// Abertura rápida: caminhos que casam com os termos (até 100) e o total que casou.
export type QuickOpen = { paths: string[]; total: number };

// Substituição em vários arquivos. A prévia não grava; a aplicação só toca nos arquivos escolhidos, com a revisão que a prévia mostrou.
// `preserveCase`: a substituição segue a caixa da ocorrência (toda maiúscula, toda minúscula ou inicial maiúscula). Só vale na substituição.
export type ReplaceQuery = SearchOptions & SearchFilter & { text: string; preserveCase?: boolean };
export type PreviewFile = { path: string; revision: string; bom: boolean; count: number; original: string; result: string };
export type ReplacePreview = {
  files: PreviewFile[];
  totalFiles: number;
  totalMatches: number;
  // Arquivos com ocorrência que não entraram em `files` (limite de arquivos ou de texto): não são aplicáveis por esta prévia.
  omittedFiles: number;
  skipped: Skipped[];
  skippedOmitted: number;
};
export type ReplacePick = { path: string; revision: string };
export type ReplaceRequest = { query: ReplaceQuery; replacement: string; files: ReplacePick[] };
export type ReplaceApplied = { applied: { path: string; count: number }[]; errors: { path: string; error: string }[] };

// Resultado de ação em lote (mover, mandar à Lixeira, colar): o que foi feito e o erro do que parou (`null` se tudo deu certo).
// `done` vem mesmo com erro: o que já foi feito não é desfeito.
export type Batch<T> = { done: T[]; error: string | null };
// Mover: `from` e `to` relativos ao projeto; em pasta, o que está dentro acompanha o prefixo.
export type Moved = { from: string; to: string };
// Colar: `from` é o caminho absoluto que estava no clipboard; `to` é o relativo ao projeto do que foi criado.
export type Pasted = { from: string; to: string };

export type TermData = { id: string; seq: number; data: string };
export type Snapshot = { seq: number; data: string };
export type NodeStateEvent = { id: string; state: NodeState; choice: Choice | null };

export type Events = {
  'term-data': TermData;
  'node-added': ExecutorNode;
  'node-state': NodeStateEvent;
  'executor-choice': Choice;
  'orchestrator-exit': { code: number | null };
  'shell-exit': { id: string; code: number | null };
  'update-status': UpdateStatus;
};

export const ORCHESTRATOR = 'orchestrator';
