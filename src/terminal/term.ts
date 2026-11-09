// Um xterm ligado a um terminal do backend: retrato inicial + fluxo numerado, sem perda nem repetição.
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Terminal } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import { toast } from '@lobehub/ui/base-ui';
import { backend, message } from '../backend/backend';
import { decodeTermFrame } from '../backend/term-frame';
import { mirror } from './mirror';
import { termKey } from './term-key';
import { clipboardFiles, clipboardPaste, type Cli, type Paste } from './paste';

const theme = {
  background: '#0b0c0d', foreground: '#d4d7dc', cursor: '#d4d7dc', cursorAccent: '#0b0c0d',
  selectionBackground: '#3a4180', selectionInactiveBackground: '#2a2d3a',
  black: '#1b1d21', red: '#e5706b', green: '#6cc28a', yellow: '#d9b35c', blue: '#6f9fe8',
  magenta: '#b48be0', cyan: '#5bbdc4', white: '#c5c8ce',
  brightBlack: '#6b7079', brightRed: '#f08d88', brightGreen: '#8bd6a4', brightYellow: '#e8c97c',
  brightBlue: '#8fb6f2', brightMagenta: '#c9a6ee', brightCyan: '#7dd3d9', brightWhite: '#f2f3f5',
};

type RenderServiceInternals = {
  _isPaused: boolean;
  _needsFullRefresh: boolean;
  _needsSelectionRefresh: boolean;
  _selectionState: { start: unknown; end: unknown; columnSelectMode: boolean };
  handleSelectionChanged(start: unknown, end: unknown, columnSelectMode: boolean): void;
};

/**
 * No xterm 6, a rolagem agenda `SelectionService.refresh` a cada quadro e ele chega a `RenderService.handleSelectionChanged`, que ignora a
 * pausa do IntersectionObserver e repinta todas as linhas no renderizador DOM: um terminal escondido com saída contínua continuaria
 * renderizando. Pausado, só guarda a seleção e marca o repintar para quando o terminal voltar a intersectar (é o que `_renderRows` já faz
 * com `_needsSelectionRefresh`). API interna do xterm: se a forma mudar, nada é trocado e o teste de terminal escondido acusa.
 */
function deferSelectionRedraw(term: Terminal) {
  const service = (term as unknown as { _core?: { _renderService?: RenderServiceInternals } })._core?._renderService;
  if (!service || typeof service.handleSelectionChanged !== 'function' || typeof service._isPaused !== 'boolean') return;
  const redraw = service.handleSelectionChanged;
  service.handleSelectionChanged = function (this: RenderServiceInternals, start, end, columnSelectMode) {
    if (!this._isPaused) return redraw.call(this, start, end, columnSelectMode);
    this._selectionState = { start, end, columnSelectMode };
    this._needsSelectionRefresh = true;
    this._needsFullRefresh = true;
  };
}

/**
 * Terminais com WebGL ao mesmo tempo, somando painéis e projetos. O Chromium perde contextos acima de ~16. Escondido, o terminal fica com o
 * seu (pausado pelo IntersectionObserver, sem custo de GPU), e voltar a ele não recria contexto nem atlas; passou do teto, o escondido usado
 * há mais tempo solta o dele, já pausado, então o renderizador DOM que entra no lugar não pinta nada.
 */
export const WEBGL_MAX = 4;
/** Linhas de histórico do espelho de um terminal: é o que o terminal recebe ao aparecer. Menos linhas, troca de aba mais rápida. */
const MIRROR_SCROLLBACK = 2000;

export class TermView {
  readonly term: Terminal;
  private fitAddon = new FitAddon();
  private observer: ResizeObserver;
  private frame = 0;
  /** O React diz se o terminal aparece (`setShown`): aba ativa, painel aberto e projeto ativo. */
  private shown: boolean;
  /** O contêiner mudou de tamanho com o terminal escondido: o ajuste fica para quando ele aparecer. */
  private stale = false;
  /** Do usado há mais tempo ao mais recente (ver `WEBGL_MAX`). */
  private static webglViews: TermView[] = [];
  /**
   * O espelho no worker processa toda a saída, e o xterm daqui só enquanto aparece. Escondido, ele é zerado ('stale'); ao aparecer, recebe
   * o estado serializado do espelho ('syncing') e volta a seguir o fluxo ('live'). Escondido não recebe teclado; as respostas a consultas
   * (DSR, DA) da saída que só o espelho viu vêm dele.
   */
  private readonly key: string;
  private sync: 'live' | 'stale' | 'syncing' = 'live';
  /** Conta as trocas de `sync`: um retrato serializado que chega depois de outra troca é descartado. */
  private syncGen = 0;
  /** Saída que chegou depois do pedido de retrato ao espelho; vai ao xterm logo depois dele. */
  private backlog: Uint8Array[] = [];
  /** Enquanto o retrato não chega, os pedaços da saída esperam aqui. */
  private pending: { seq: number; data: Uint8Array }[] | null = [];
  private seq = -1;
  /** Há um PTY do outro lado para avisar do tamanho? */
  private live = false;
  /** Renderizador WebGL: o visível e, até `WEBGL_MAX`, os escondidos usados por último; os outros ficam no DOM. */
  private webgl: WebglAddon | null = null;
  /** Sem WebGL2 neste ambiente: não tenta de novo. */
  private noWebgl = false;
  /** O contexto foi perdido: só tenta de novo depois que o terminal for escondido e reaparecer. */
  private lost = false;

  /** `writable` liga o teclado ao backend; sem ele o terminal é só leitura por construção. */
  constructor(readonly id: string, host: HTMLElement, writable: boolean, readonly project: string | null, shown: boolean) {
    this.shown = shown;
    this.term = new Terminal({
      theme, fontFamily: '"Cascadia Mono", "Cascadia Code", Consolas, "Courier New", monospace',
      fontSize: 13, lineHeight: 1.15, scrollback: 10000, cursorBlink: writable, disableStdin: !writable,
    });
    this.term.loadAddon(this.fitAddon);
    this.key = termKey(id, project);
    const reply = writable ? (data: string) => void backend.termWrite(id, data, project).catch(() => {}) : undefined;
    mirror.open(this.key, this.term.cols, this.term.rows, MIRROR_SCROLLBACK, reply);
    if (!shown) this.sync = 'stale';
    this.term.open(host);
    deferSelectionRedraw(this.term);
    // Gancho de medição (scripts/medir-desempenho.mjs): só existe se o script o definir na página; sem ele, nada acontece.
    (globalThis as { __orqMedir?: (id: string, term: Terminal) => void }).__orqMedir?.(id, this.term);
    // Ctrl+C com texto selecionado copia; Ctrl+V cola pelo evento nativo (que o xterm trata).
    this.term.attachCustomKeyEventHandler((e) => {
      const key = e.ctrlKey && !e.altKey ? e.key.toLowerCase() : '';
      return !((key === 'c' && this.term.hasSelection()) || key === 'v');
    });
    if (reply) this.term.onData(reply);
    if (writable) this.term.element?.addEventListener('paste', this.onPaste, true);
    this.term.onResize(({ cols, rows }) => {
      mirror.resize(this.key, cols, rows);
      this.sendSize();
    });
    // Escondido, o terminal mantém o tamanho: redimensionar o painel com N executores não faz N reflows nem N redesenhos de CLI.
    this.observer = new ResizeObserver(() => {
      cancelAnimationFrame(this.frame);
      this.frame = requestAnimationFrame(() => {
        if (this.shown) this.fit();
        else this.stale = true;
      });
    });
    this.observer.observe(host);
    if (shown) this.acquire();
    this.fit();
  }

  setShown(shown: boolean) {
    if (shown === this.shown) return;
    this.shown = shown;
    if (!shown) {
      this.lost = false;
      // O espelho guarda o estado: zerar solta a memória do histórico daqui, e a saída deixa de passar pelo xterm da tela.
      this.syncGen++;
      this.sync = 'stale';
      this.backlog = [];
      this.term.reset();
      return;
    }
    this.acquire();
    // O ajuste vem antes do retrato: o espelho é redimensionado antes de serializar, e o retrato chega no tamanho da tela.
    if (this.stale) {
      this.stale = false;
      this.fit();
    }
    if (this.sync === 'stale') this.resync();
  }

  private resync() {
    const gen = ++this.syncGen;
    this.sync = 'syncing';
    void mirror.serialize(this.key).then((text) => {
      if (gen !== this.syncGen) return;
      this.term.reset();
      this.term.write(text);
      for (const data of this.backlog) this.term.write(data);
      this.backlog = [];
      this.sync = 'live';
    });
  }

  /** Falha ou perda de contexto cai no DOM. */
  private acquire() {
    const views = TermView.webglViews;
    if (this.webgl) {
      views.push(...views.splice(views.indexOf(this), 1));
      return;
    }
    if (this.noWebgl || this.lost) return;
    const addon = new WebglAddon();
    try {
      addon.onContextLoss(() => {
        if (this.webgl !== addon) return;
        this.release();
        this.lost = true;
      });
      this.term.loadAddon(addon);
      this.webgl = addon;
      views.push(this);
    } catch {
      addon.dispose();
      this.noWebgl = true;
    }
    // Só depois de carregar o novo: o atlas de glifos tem dono e é reaproveitado.
    while (views.length > WEBGL_MAX) {
      const oldest = views.find((view) => !view.shown);
      if (!oldest) break;
      oldest.release();
    }
  }

  /** Solta o WebGL. O addon só remove o canvas; o contexto ficaria vivo até o GC e contaria no limite do Chromium, então é perdido de propósito. */
  private release() {
    const addon = this.webgl;
    if (!addon) return;
    this.webgl = null;
    TermView.webglViews.splice(TermView.webglViews.indexOf(this), 1);
    const canvas = this.term.element?.querySelector<HTMLCanvasElement>('.xterm-screen canvas:not(.xterm-link-layer)');
    addon.dispose();
    canvas?.getContext('webgl2')?.getExtension('WEBGL_lose_context')?.loseContext();
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
    return this.cli ?? backend.termCli(this.id, this.project);
  }

  private async put(out: Paste) {
    if ('error' in out) throw new Error(out.error);
    if ('paste' in out) this.term.paste(out.paste);
    else await backend.termWrite(this.id, out.write, this.project);
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
    if (this.live) void backend.termResize(this.id, this.term.cols, this.term.rows, this.project).catch(() => {});
  }

  /** Busca o que já saiu e passa a aplicar o fluxo; o que chegou no meio do caminho está em `pending`. */
  async attach() {
    const snapshot = await backend.termSnapshot(this.id, this.project).then(decodeTermFrame, () => ({ seq: -1, data: new Uint8Array() }));
    const waiting = this.pending ?? [];
    this.pending = null;
    this.seq = snapshot.seq;
    if (snapshot.data.length) this.write(snapshot.data);
    for (const chunk of waiting) this.push(chunk.seq, chunk.data);
  }

  push(seq: number, data: Uint8Array) {
    if (this.pending) this.pending.push({ seq, data });
    else if (seq > this.seq) {
      this.seq = seq;
      this.write(data);
    }
  }

  private write(data: Uint8Array) {
    mirror.write(this.key, data, this.sync === 'stale');
    if (this.sync === 'live') this.term.write(data);
    else if (this.sync === 'syncing') this.backlog.push(data);
  }

  dispose() {
    cancelAnimationFrame(this.frame);
    this.syncGen++;
    mirror.close(this.key);
    this.release();
    this.observer.disconnect();
    this.term.element?.removeEventListener('paste', this.onPaste, true);
    this.term.dispose();
  }
}
