// Lado da thread principal do espelho dos executores (ver mirror-worker.ts). Um worker para todos, criado no primeiro executor.

export type MirrorIn =
  | { type: 'open'; key: string; cols: number; rows: number; scrollback: number }
  | { type: 'data'; key: string; data: Uint8Array }
  | { type: 'resize'; key: string; cols: number; rows: number }
  | { type: 'serialize'; key: string; id: number }
  | { type: 'close'; key: string };
export type MirrorOut = { id: number; text: string };

let worker: Worker | null = null;
const waiting = new Map<number, (text: string) => void>();
let nextId = 0;

function post(msg: MirrorIn, transfer: Transferable[] = []) {
  if (!worker) {
    worker = new Worker(new URL('./mirror-worker.ts', import.meta.url), { type: 'module' });
    worker.onmessage = ({ data }: MessageEvent<MirrorOut>) => {
      waiting.get(data.id)?.(data.text);
      waiting.delete(data.id);
    };
  }
  worker.postMessage(msg, transfer);
}

export const mirror = {
  open: (key: string, cols: number, rows: number, scrollback: number) => post({ type: 'open', key, cols, rows, scrollback }),
  /** Copia: `data` costuma ser uma vista do buffer do canal, que a tela ainda pode usar; a cópia vai transferida, sem outra cópia. */
  write(key: string, data: Uint8Array) {
    const copy = data.slice();
    post({ type: 'data', key, data: copy }, [copy.buffer]);
  },
  resize: (key: string, cols: number, rows: number) => post({ type: 'resize', key, cols, rows }),
  /** O estado em sequências VT, com tudo o que foi escrito antes do pedido. */
  serialize: (key: string) => new Promise<string>((resolve) => {
    const id = nextId++;
    waiting.set(id, resolve);
    post({ type: 'serialize', key, id });
  }),
  close: (key: string) => post({ type: 'close', key }),
};
