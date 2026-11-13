// Espelho dos terminais fora da thread principal: um xterm headless por terminal processa toda a saída. Ao aparecer, o terminal da tela
// recebe daqui o estado serializado, e a thread principal só processa a saída dos terminais visíveis.
import './worker-window';
import { Terminal, type ITerminalAddon } from '@xterm/headless';
import { SerializeAddon } from '@xterm/addon-serialize';
import type { MirrorIn, MirrorOut } from './mirror';

type Mirror = { term: Terminal; serializer: SerializeAddon; answer: boolean; queued: boolean; extra: Extra };
/** Estado que o addon-serialize 0.14 não grava: cursor oculto (?25), codificação do mouse (?1005/?1006/?1015/?1016) e forma do cursor (DECSCUSR). */
type Extra = { hidden: boolean; encoding: number; cursor: string };
const fresh = (): Extra => ({ hidden: false, encoding: 0, cursor: '' });
const ENCODINGS = new Set([1005, 1006, 1015, 1016]);

const mirrors = new Map<string, Mirror>();

/** Observa sem consumir (`false`): o xterm segue tratando a sequência. */
function track(term: Terminal, m: { extra: Extra }) {
  const extra = () => m.extra;
  const decset = (set: boolean) => (params: (number | number[])[]) => {
    for (const p of params) {
      if (p === 25) extra().hidden = !set;
      else if (typeof p === 'number' && ENCODINGS.has(p)) extra().encoding = set ? p : extra().encoding === p ? 0 : extra().encoding;
    }
    return false;
  };
  term.parser.registerCsiHandler({ prefix: '?', final: 'h' }, decset(true));
  term.parser.registerCsiHandler({ prefix: '?', final: 'l' }, decset(false));
  term.parser.registerCsiHandler({ intermediates: ' ', final: 'q' }, (params) => {
    extra().cursor = `[${params.join(';')} q`;
    return false;
  });
  const reset = () => {
    m.extra = fresh();
    return false;
  };
  term.parser.registerEscHandler({ final: 'c' }, reset);
  term.parser.registerCsiHandler({ intermediates: '!', final: 'p' }, reset);
}

function serialize(m: Mirror) {
  const { hidden, encoding, cursor } = m.extra;
  return m.serializer.serialize() + (encoding ? `[?${encoding}h` : '') + cursor + (hidden ? '[?25l' : '');
}

self.onmessage = ({ data: msg }: MessageEvent<MirrorIn>) => {
  if (msg.type === 'data') {
    for (const { key, data, answer } of msg.items) {
      const m = mirrors.get(key);
      if (!m) continue;
      // A escrita é enfileirada: o callback vazio marca, na ordem do parser, de quem é a vez de responder.
      if (m.queued !== answer) {
        m.queued = answer;
        m.term.write('', () => { m.answer = answer; });
      }
      m.term.write(data);
    }
    return;
  }
  const mirror = mirrors.get(msg.key);
  switch (msg.type) {
    case 'open': {
      mirror?.term.dispose();
      const term = new Terminal({ cols: msg.cols, rows: msg.rows, scrollback: msg.scrollback, allowProposedApi: true });
      const serializer = new SerializeAddon();
      // O addon é tipado para o xterm de tela; no headless usa só buffer, modos e opções, que os dois têm.
      term.loadAddon(serializer as unknown as ITerminalAddon);
      const key = msg.key;
      const m: Mirror = { term, serializer, answer: false, queued: false, extra: fresh() };
      track(term, m);
      // Respostas a DSR/DA: só da saída que a tela não viu, para o programa não esperar resposta que nunca vem nem receber duas.
      term.onData((reply) => {
        if (m.answer) self.postMessage({ key, reply } satisfies MirrorOut);
      });
      mirrors.set(key, m);
      break;
    }
    case 'resize':
      mirror?.term.resize(msg.cols, msg.rows);
      break;
    case 'serialize': {
      const reply = (text: string) => self.postMessage({ id: msg.id, text } satisfies MirrorOut);
      // A escrita do xterm é assíncrona e em ordem: o callback do vazio roda depois de toda a saída que chegou antes do pedido.
      if (mirror) mirror.term.write('', () => reply(serialize(mirror)));
      else reply('');
      break;
    }
    case 'close':
      mirror?.term.dispose();
      mirrors.delete(msg.key);
      break;
  }
};
