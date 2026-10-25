// Realce do comparador fora da thread principal: o motor desenha o diff sem cores e o worker (shiki WASM) devolve o realce em seguida.
// Com o pool, tema e `lineDiffType` valem daqui; os passados por componente são ignorados.
import type { ReactNode } from 'react';
import { WorkerPoolContextProvider, type WorkerInitializationRenderOptions } from '@pierre/diffs/react';
import DiffsWorkerUrl from '@pierre/diffs/worker/worker.js?worker&url';

const poolOptions = { poolSize: 2, workerFactory: () => new Worker(DiffsWorkerUrl, { type: 'module' }) };
const highlighterOptions: WorkerInitializationRenderOptions = { langs: ['typescript'], theme: { dark: 'dark-plus', light: 'dark-plus' }, lineDiffType: 'word-alt', preferredHighlighter: 'shiki-wasm' };

export function DiffWorkerPool({ children }: { children: ReactNode }) {
  return <WorkerPoolContextProvider poolOptions={poolOptions} highlighterOptions={highlighterOptions}>{children}</WorkerPoolContextProvider>;
}
