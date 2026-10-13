// Tipos do contrato (JSON, camelCase). '' em modelo ou esforço = padrão do CLI.
export type Tool = 'claude' | 'codex';
export type Choice = { tool: Tool; model: string; effort: string };
export type ModelOption = { name: string; efforts: string[] };
export type ToolOption = { name: Tool; efforts: string[]; models: ModelOption[] };
export type Options = {
  tools: ToolOption[];
  last: { orchestrator: Choice; executor: Choice };
  maxParallel: number;
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

// Painel Git (contrato do backend, JSON em camelCase). Um arquivo pode estar nas duas seções ao mesmo tempo.
export type GitKind = 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked' | 'conflicted';
export type GitFile = { path: string; origPath: string | null; staged: GitKind | null; unstaged: GitKind | null };
export type GitStatus = { branch: string; ahead: number; behind: number; files: GitFile[] };
// `patch` é o diff unificado do arquivo (cabeçalho e `@@`); vazio quando não há alteração.
export type GitDiff = { binary: boolean; tooLarge: boolean; patch: string };
export type GitBranches = { current: string; local: string[] };

// Pesquisa de texto (contrato do backend, JSON em camelCase). `path` é relativo à raiz, com "/"; uma entrada por linha que casa.
export type SearchMatch = { line: number; text: string };
export type SearchFile = { path: string; matches: SearchMatch[] };
export type SearchResults = { files: SearchFile[]; truncated: boolean };
export type SearchOptions = { caseSensitive: boolean; wholeWord: boolean; regex: boolean };

export type TermData = { id: string; seq: number; data: string };
export type Snapshot = { seq: number; data: string };
export type NodeStateEvent = { id: string; state: NodeState; choice: Choice | null };

export type Events = {
  'term-data': TermData;
  'node-added': ExecutorNode;
  'node-state': NodeStateEvent;
  'executor-choice': Choice;
  'orchestrator-exit': { code: number | null };
  'shell-exit': { code: number | null };
  'update-status': UpdateStatus;
};

export const ORCHESTRATOR = 'orchestrator';
// Id fixo do terminal do shell do usuário (o painel Terminal).
export const SHELL = 'shell';
