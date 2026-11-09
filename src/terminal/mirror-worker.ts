// Espelho dos executores fora da thread principal: um xterm headless por executor processa toda a saída. Ao aparecer, o terminal da tela
// recebe daqui o estado serializado, e a thread principal só processa a saída do executor visível.
import './worker-window';
import { Terminal, type ITerminalAddon } from '@xterm/headless';
import { SerializeAddon } from '@xterm/addon-serialize';
import type { MirrorIn, MirrorOut } from './mirror';

const mirrors = new Map<string, { term: Terminal; serializer: SerializeAddon }>();

self.onmessage = ({ data: msg }: MessageEvent<MirrorIn>) => {
  const mirror = mirrors.get(msg.key);
  switch (msg.type) {
    case 'open': {
      mirror?.term.dispose();
      const term = new Terminal({ cols: msg.cols, rows: msg.rows, scrollback: msg.scrollback, allowProposedApi: true });
      const serializer = new SerializeAddon();
      // O addon é tipado para o xterm de tela; no headless usa só buffer, modos e opções, que os dois têm.
      term.loadAddon(serializer as unknown as ITerminalAddon);
      mirrors.set(msg.key, { term, serializer });
      break;
    }
    case 'data':
      mirror?.term.write(msg.data);
      break;
    case 'resize':
      mirror?.term.resize(msg.cols, msg.rows);
      break;
    case 'serialize': {
      const reply = (text: string) => self.postMessage({ id: msg.id, text } satisfies MirrorOut);
      // A escrita do xterm é assíncrona e em ordem: o callback do vazio roda depois de toda a saída que chegou antes do pedido.
      if (mirror) mirror.term.write('', () => reply(mirror.serializer.serialize()));
      else reply('');
      break;
    }
    case 'close':
      mirror?.term.dispose();
      mirrors.delete(msg.key);
      break;
  }
};
