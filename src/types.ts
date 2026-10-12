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

export type TermData = { id: string; seq: number; data: string };
export type Snapshot = { seq: number; data: string };
export type NodeStateEvent = { id: string; state: NodeState; choice: Choice | null };

export type Events = {
  'term-data': TermData;
  'node-added': ExecutorNode;
  'node-state': NodeStateEvent;
  'executor-choice': Choice;
  'orchestrator-exit': { code: number | null };
  'update-status': UpdateStatus;
};

export const ORCHESTRATOR = 'orchestrator';

export const describeChoice = (c: Choice) =>
  [c.tool, c.model, c.effort].filter(Boolean).join(' ') + (c.model || c.effort ? '' : ' (padrão do CLI)');
