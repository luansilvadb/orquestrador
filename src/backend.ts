// Único ponto de contato com o Tauri. Fora do app (navegador, em desenvolvimento) entra o simulado.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Choice, Events, Options, Snapshot } from './types';

export interface Transport {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>;
  listen<K extends keyof Events>(event: K, handler: (payload: Events[K]) => void): Promise<void>;
  /** `mayClose` responde se a janela pode fechar; o encerramento do backend é feito dentro dela. */
  onCloseRequested(mayClose: () => Promise<boolean>): Promise<void>;
}

const tauri: Transport = {
  invoke,
  listen: async (event, handler) => void (await listen(event, (e) => handler(e.payload as never))),
  onCloseRequested: async (mayClose) =>
    void (await getCurrentWindow().onCloseRequested(async (event) => {
      if (!(await mayClose())) event.preventDefault();
    })),
};

let transport: Transport;

export async function connect(): Promise<void> {
  if ('__TAURI_INTERNALS__' in window) transport = tauri;
  else if (import.meta.env.DEV) transport = (await import('./mock')).mock;
  else throw new Error('Esta página só funciona dentro do app Orquestrador.');
}

const call = <T>(cmd: string, args?: Record<string, unknown>) => transport.invoke(cmd, args) as Promise<T>;

export const backend = {
  listen: <K extends keyof Events>(event: K, handler: (payload: Events[K]) => void) => transport.listen(event, handler),
  onCloseRequested: (mayClose: () => Promise<boolean>) => transport.onCloseRequested(mayClose),

  chooseProject: () => call<string | null>('choose_project'),
  getOptions: () => call<Options>('get_options'),
  startOrchestrator: (choice: Choice, cols: number, rows: number) =>
    call<string>('start_orchestrator', { choice, cols, rows }),
  setExecutorChoice: (choice: Choice) => call<void>('set_executor_choice', { choice }),
  setMaxParallel: (value: number) => call<void>('set_max_parallel', { value }),
  termWrite: (id: string, data: string) => call<void>('term_write', { id, data }),
  termResize: (id: string, cols: number, rows: number) => call<void>('term_resize', { id, cols, rows }),
  termSnapshot: (id: string) => call<Snapshot>('term_snapshot', { id }),
  stopExecutor: (id: string) => call<void>('stop_executor', { id }),
  clearDone: () => call<string[]>('clear_done'),
  hasRunning: () => call<boolean>('has_running'),
  shutdown: () => call<void>('shutdown'),
};

/** Erros do backend já chegam como mensagem em português. */
export const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
