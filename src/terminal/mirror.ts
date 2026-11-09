// Lado da thread principal do espelho dos terminais (ver mirror-worker.ts). Um worker para todos, criado no primeiro executor.

export type MirrorIn =
  | { type: 'open'; key: string; cols: number; rows: number; scrollback: number }
  | { type: 'data'; key: string; data: Uint8Array; answer: boolean }
  | { type: 'resize'; key: string; cols: number; rows: number }
  | { type: 'serialize'; key: string; id: number }
  | { type: 'close'; key: string };
/** `text` responde a um `serialize`; `reply` é a resposta do terminal a uma consulta (DSR, DA) na saída marcada com `answer`. */
export type MirrorOut = { id: number; text: string } | { key: string; reply: string };

let worker: Worker | null = null;
const waiting = new Map<number, (text: string) => void>();
const replies = new Map<string, (reply: string) => void>();
let nextId = 0;

function post(msg: MirrorIn, transfer: Transferable[] = []) {
  if (!worker) {
    worker = new Worker(new URL('./mirror-worker.ts', import.meta.url), { type: 'module' });
    worker.onmessage = ({ data }: MessageEvent<MirrorOut>) => {
      if ('reply' in data) return replies.get(data.key)?.(data.reply);
      waiting.get(data.id)?.(data.text);
      waiting.delete(data.id);
    };
  }
  worker.postMessage(msg, transfer);
}

export const mirror = {
  /** `onReply` recebe as respostas do espelho às consultas da saída escrita com `answer`. */
  open(key: string, cols: number, rows: number, scrollback: number, onReply?: (reply: string) => void) {
    if (onReply) replies.set(key, onReply);
    post({ type: 'open', key, cols, rows, scrollback });
  },
  /** Copia: `data` costuma ser uma vista do buffer do canal, que a tela ainda pode usar; a cópia vai transferida, sem outra cópia. */
  /** `answer`: o espelho responde às consultas deste pedaço, porque o xterm da tela não o verá. */
  write(key: string, data: Uint8Array, answer = false) {
    const copy = data.slice();
    post({ type: 'data', key, data: copy, answer }, [copy.buffer]);
  },
  resize: (key: string, cols: number, rows: number) => post({ type: 'resize', key, cols, rows }),
  /** O estado em sequências VT, com tudo o que foi escrito antes do pedido. */
  serialize: (key: string) => new Promise<string>((resolve) => {
    const id = nextId++;
    waiting.set(id, resolve);
    post({ type: 'serialize', key, id });
  }),
  close(key: string) {
    replies.delete(key);
    post({ type: 'close', key });
  },
};
