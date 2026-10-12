// O canvas: um "mundo" com transform (pan + zoom), blocos arrastáveis e as linhas que os ligam.
export type Rect = { x: number; y: number; w: number; h: number };
export type Block = { id: string; rect: Rect; el: HTMLElement; head: HTMLElement; body: HTMLElement };

const MIN_ZOOM = 0.2, MAX_ZOOM = 2, MIN_W = 340, MIN_H = 180, GAP = 28;
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const overlaps = (a: Rect, b: Rect) =>
  a.x < b.x + b.w + GAP / 2 && b.x < a.x + a.w + GAP / 2 && a.y < b.y + b.h + GAP / 2 && b.y < a.y + a.h + GAP / 2;

export class Canvas {
  readonly blocks = new Map<string, Block>();
  private view = { x: 0, y: 0, z: 1 };
  private links = new Map<string, { from: string; path: SVGPathElement }>();
  private top = 1;
  onZoom: (zoom: number) => void = () => {};

  constructor(private viewport: HTMLElement, private world: HTMLElement, private svg: SVGSVGElement) {
    // Pan: arrastar o fundo (qualquer ponto fora de um bloco).
    viewport.addEventListener('pointerdown', (e) => {
      if (e.button !== 0 || (e.target as Element).closest('.block, .empty')) return;
      (document.activeElement as HTMLElement | null)?.blur();
      this.drag(e, viewport, (dx, dy) => this.setView(this.view.x + dx, this.view.y + dy, this.view.z));
    });
    // Zoom com a roda, ancorado no cursor. Dentro do corpo de um bloco a roda é do terminal (Ctrl força o zoom).
    viewport.addEventListener('wheel', (e) => {
      if (!e.ctrlKey && (e.target as Element).closest('.block-body')) return;
      e.preventDefault();
      const box = viewport.getBoundingClientRect();
      this.zoomAt(this.view.z * Math.exp(-e.deltaY * 0.0015), e.clientX - box.left, e.clientY - box.top);
    }, { passive: false });
    this.fixTerminalMouse();
  }

  get zoom() { return this.view.z; }

  /** Zoom mantendo fixo o ponto (cx, cy) da área visível; sem ponto, o centro. */
  zoomAt(zoom: number, cx = this.viewport.clientWidth / 2, cy = this.viewport.clientHeight / 2) {
    const z = clamp(zoom, MIN_ZOOM, MAX_ZOOM), k = z / this.view.z;
    this.setView(cx - (cx - this.view.x) * k, cy - (cy - this.view.y) * k, z);
  }

  /** Enquadra todos os blocos (sem ampliar além de 100%). */
  fitAll() {
    const rects = [...this.blocks.values()].map((b) => b.rect);
    if (!rects.length) return this.setView(0, 0, 1);
    const x0 = Math.min(...rects.map((r) => r.x)), y0 = Math.min(...rects.map((r) => r.y));
    const x1 = Math.max(...rects.map((r) => r.x + r.w)), y1 = Math.max(...rects.map((r) => r.y + r.h));
    const vw = this.viewport.clientWidth, vh = this.viewport.clientHeight, pad = 32;
    const z = clamp(Math.min((vw - 2 * pad) / (x1 - x0), (vh - 2 * pad) / (y1 - y0), 1), MIN_ZOOM, MAX_ZOOM);
    this.setView((vw - (x1 - x0) * z) / 2 - x0 * z, (vh - (y1 - y0) * z) / 2 - y0 * z, z);
  }

  /** Cria a moldura de um bloco; `linkedTo` desenha a linha que o liga a outro bloco. */
  add(id: string, rect: Rect, linkedTo?: string): Block {
    const el = document.createElement('section');
    el.className = 'block';
    el.innerHTML = '<header class="block-head"></header><div class="block-body"></div><div class="grip" title="Redimensionar"></div>';
    const block: Block = { id, rect, el, head: el.children[0] as HTMLElement, body: el.children[1] as HTMLElement };
    this.blocks.set(id, block);
    this.world.append(el);
    this.place(block);

    el.addEventListener('pointerdown', () => { el.style.zIndex = String(++this.top); }, true);
    block.head.addEventListener('pointerdown', (e) => {
      if (e.button !== 0 || (e.target as Element).closest('button, input')) return;
      this.drag(e, block.head, (dx, dy) => {
        rect.x += dx / this.view.z;
        rect.y += dy / this.view.z;
        this.place(block);
      });
    });
    (el.children[2] as HTMLElement).addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return;
      e.stopPropagation();
      this.drag(e, e.currentTarget as HTMLElement, (dx, dy) => {
        rect.w = Math.max(MIN_W, rect.w + dx / this.view.z);
        rect.h = Math.max(MIN_H, rect.h + dy / this.view.z);
        this.place(block);
      });
    });

    if (linkedTo) {
      const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
      this.svg.append(path);
      this.links.set(id, { from: linkedTo, path });
      this.drawLinks();
    }
    return block;
  }

  remove(id: string) {
    this.blocks.get(id)?.el.remove();
    this.blocks.delete(id);
    this.links.get(id)?.path.remove();
    this.links.delete(id);
  }

  /** O estado do bloco colore a linha dele. */
  setLinkState(id: string, state: string) {
    this.links.get(id)?.path.setAttribute('data-state', state);
  }

  /** Primeira vaga livre numa grade à direita de `anchor`, três blocos por coluna. */
  freeSlot(anchor: Rect, w: number, h: number): Rect {
    for (let col = 0; ; col++) {
      for (let row = 0; row < 3; row++) {
        const slot = { x: anchor.x + anchor.w + 2 * GAP + col * (w + GAP), y: anchor.y + row * (h + GAP), w, h };
        if (![...this.blocks.values()].some((b) => overlaps(b.rect, slot))) return slot;
      }
    }
  }

  /** O bloco está inteiro dentro da área visível? */
  isVisible(rect: Rect) {
    const { x, y, z } = this.view;
    return x + rect.x * z >= 0 && y + rect.y * z >= 0 &&
      x + (rect.x + rect.w) * z <= this.viewport.clientWidth && y + (rect.y + rect.h) * z <= this.viewport.clientHeight;
  }

  private setView(x: number, y: number, z: number) {
    this.view = { x, y, z };
    this.world.style.transform = `translate(${x}px, ${y}px) scale(${z})`;
    // A grade de fundo acompanha o mundo.
    this.viewport.style.backgroundPosition = `${x}px ${y}px`;
    this.viewport.style.backgroundSize = `${24 * z}px ${24 * z}px`;
    this.onZoom(z);
  }

  private place(block: Block) {
    const { x, y, w, h } = block.rect;
    Object.assign(block.el.style, { left: `${x}px`, top: `${y}px`, width: `${w}px`, height: `${h}px` });
    this.drawLinks();
  }

  private drawLinks() {
    for (const [id, { from, path }] of this.links) {
      const a = this.blocks.get(from)?.rect, b = this.blocks.get(id)?.rect;
      if (!a || !b) continue;
      // Sai do lado do orquestrador que olha para o executor.
      const right = b.x + b.w / 2 >= a.x + a.w / 2;
      const x1 = right ? a.x + a.w : a.x, y1 = a.y + a.h / 2;
      const x2 = right ? b.x : b.x + b.w, y2 = b.y + b.h / 2;
      const bend = Math.max(40, Math.abs(x2 - x1) / 2) * (right ? 1 : -1);
      path.setAttribute('d', `M${x1},${y1} C${x1 + bend},${y1} ${x2 - bend},${y2} ${x2},${y2}`);
    }
  }

  private drag(start: PointerEvent, target: HTMLElement, move: (dx: number, dy: number) => void) {
    start.preventDefault();
    target.setPointerCapture(start.pointerId);
    let x = start.clientX, y = start.clientY;
    const onMove = (e: PointerEvent) => {
      move(e.clientX - x, e.clientY - y);
      x = e.clientX;
      y = e.clientY;
    };
    const stop = () => {
      target.removeEventListener('pointermove', onMove);
      target.removeEventListener('pointerup', stop);
      target.removeEventListener('pointercancel', stop);
      document.body.classList.remove('dragging');
    };
    target.addEventListener('pointermove', onMove);
    target.addEventListener('pointerup', stop);
    target.addEventListener('pointercancel', stop);
    document.body.classList.add('dragging');
  }

  /**
   * O xterm acha a célula sob o ponteiro dividindo a distância em tela pelo tamanho da célula SEM o
   * scale() do mundo; com zoom diferente de 100% a seleção cairia no lugar errado. Reemitimos os
   * eventos de mouse dos terminais com a posição já desfeita do zoom.
   */
  private fixTerminalMouse() {
    let dragging: Element | null = null;
    const relay = (e: MouseEvent) => {
      if (!e.isTrusted || this.view.z === 1) return;
      const screen = dragging ?? (e.target as Element).closest?.('.xterm-screen');
      if (e.type === 'mousedown') dragging = screen ?? null;
      if (e.type === 'mouseup') dragging = null;
      if (!screen || !e.target) return;
      e.stopImmediatePropagation();
      const box = screen.getBoundingClientRect(), z = this.view.z;
      const copy = new MouseEvent(e.type, {
        bubbles: true, cancelable: true, composed: true, view: window, detail: e.detail,
        screenX: e.screenX, screenY: e.screenY, button: e.button, buttons: e.buttons,
        ctrlKey: e.ctrlKey, shiftKey: e.shiftKey, altKey: e.altKey, metaKey: e.metaKey,
        clientX: box.left + (e.clientX - box.left) / z, clientY: box.top + (e.clientY - box.top) / z,
      });
      if (!e.target.dispatchEvent(copy)) e.preventDefault();
    };
    for (const type of ['mousedown', 'mousemove', 'mouseup'] as const) window.addEventListener(type, relay, true);
  }
}
