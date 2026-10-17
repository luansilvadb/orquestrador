// Um xterm ligado a um terminal do backend: retrato inicial + fluxo numerado, sem perda nem repetição.
import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import { toast } from '@lobehub/ui/base-ui';
import { backend, message } from '../backend/backend';
import { clipboardFiles, clipboardPaste, type Cli, type Paste } from './paste';

const theme = {
  background: '#0b0c0d', foreground: '#d4d7dc', cursor: '#d4d7dc', cursorAccent: '#0b0c0d',
  selectionBackground: '#3a4180', selectionInactiveBackground: '#2a2d3a',
  black: '#1b1d21', red: '#e5706b', green: '#6cc28a', yellow: '#d9b35c', blue: '#6f9fe8',
  magenta: '#b48be0', cyan: '#5bbdc4', white: '#c5c8ce',
  brightBlack: '#6b7079', brightRed: '#f08d88', brightGreen: '#8bd6a4', brightYellow: '#e8c97c',
  brightBlue: '#8fb6f2', brightMagenta: '#c9a6ee', brightCyan: '#7dd3d9', brightWhite: '#f2f3f5',
};

export class TermView {
  readonly term: Terminal;
  private fitAddon = new FitAddon();
  private observer: ResizeObserver;
  private frame = 0;
  /** Enquanto o retrato não chega, os `term-data` esperam aqui. */
  private pending: { seq: number; data: string }[] | null = [];
  private seq = -1;
  /** Há um PTY do outro lado para avisar do tamanho? */
  private live = false;

  /** `writable` liga o teclado ao backend; sem ele o terminal é só leitura por construção. */
  constructor(readonly id: string, host: HTMLElement, writable: boolean) {
    this.term = new Terminal({
      theme, fontFamily: '"Cascadia Mono", "Cascadia Code", Consolas, "Courier New", monospace',
      fontSize: 13, lineHeight: 1.15, scrollback: 10000, cursorBlink: writable, disableStdin: !writable,
    });
    this.term.loadAddon(this.fitAddon);
    this.term.open(host);
    // Ctrl+C com texto selecionado copia; Ctrl+V cola pelo evento nativo (que o xterm trata).
    this.term.attachCustomKeyEventHandler((e) => {
      const key = e.ctrlKey && !e.altKey ? e.key.toLowerCase() : '';
      return !((key === 'c' && this.term.hasSelection()) || key === 'v');
    });
    if (writable) this.term.onData((data) => void backend.termWrite(id, data).catch(() => {}));
    if (writable) this.term.element?.addEventListener('paste', this.onPaste, true);
    this.term.onResize(() => this.sendSize());
    this.observer = new ResizeObserver(() => {
      cancelAnimationFrame(this.frame);
      this.frame = requestAnimationFrame(() => this.fit());
    });
    this.observer.observe(host);
    this.fit();
  }

  /** O CLI do terminal: fixo no orquestrador (`startOrchestrator`); vazio num shell, onde o backend olha o processo a cada colagem. */
  cli: Cli | undefined;

  /**
   * Arquivo ou imagem no clipboard: o que vai ao terminal sai de `clipboardPaste`, com o CLI que roda nele agora. Arquivo copiado no
   * Explorer: o evento não traz o caminho, o backend lê do clipboard. Texto segue pelo paste do xterm, sem mudança. O que não dá para
   * colar, ou falha no caminho, aparece num aviso: nada some calado.
   */
  private onPaste = (e: ClipboardEvent) => {
    const got = clipboardFiles(e.clipboardData);
    if (!got) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    void Promise.all([backend.clipboardSnapshot(), this.which()])
      .then(([{ paths }, cli]) => this.put(clipboardPaste(cli, paths, got.image, got.text, this.term.modes.bracketedPasteMode)))
      .catch((error) => toast.error({ title: message(error), duration: 7000 }));
  };

  private async which(): Promise<Cli> {
    return this.cli ?? backend.termCli(this.id);
  }

  private async put(out: Paste) {
    if ('error' in out) throw new Error(out.error);
    if ('paste' in out) this.term.paste(out.paste);
    else await backend.termWrite(this.id, out.write);
  }

  /** Cola caminhos como os arquivos copiados no Explorer: imagem vira anexo do CLI, o resto fica como caminho. Não manda Enter. */
  async pasteFiles(paths: string[]) {
    await this.put(clipboardPaste(await this.which(), paths, false, '', this.term.modes.bracketedPasteMode));
  }

  fit() {
    if (this.term.element?.offsetParent) this.fitAddon.fit();
  }

  /** Marca que o PTY existe e manda o tamanho atual. */
  setLive() {
    this.live = true;
    this.sendSize();
  }

  private sendSize() {
    if (this.live) void backend.termResize(this.id, this.term.cols, this.term.rows).catch(() => {});
  }

  /** Busca o que já saiu e passa a aplicar o fluxo; o que chegou no meio do caminho está em `pending`. */
  async attach() {
    const snapshot = await backend.termSnapshot(this.id).catch(() => ({ seq: -1, data: '' }));
    const waiting = this.pending ?? [];
    this.pending = null;
    this.seq = snapshot.seq;
    if (snapshot.data) this.term.write(Uint8Array.fromBase64(snapshot.data));
    for (const chunk of waiting) this.push(chunk.seq, chunk.data);
  }

  push(seq: number, data: string) {
    if (this.pending) this.pending.push({ seq, data });
    else if (seq > this.seq) {
      this.seq = seq;
      this.term.write(Uint8Array.fromBase64(data));
    }
  }

  dispose() {
    cancelAnimationFrame(this.frame);
    this.observer.disconnect();
    this.term.element?.removeEventListener('paste', this.onPaste, true);
    this.term.dispose();
  }
}
