// O canvas: um "mundo" com transform (pan + zoom), blocos arrastáveis e as linhas que os ligam.
// Pan, zoom, arrasto e redimensionamento são listeners nativos do DOM: um Select ou Dialog em portal
// (fora do bloco no DOM) não dispara arrasto nem pan, mesmo que o React propague o evento pela árvore.
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import type { NodeState } from './types';

export type Rect = { x: number; y: number; w: number; h: number };
export type CanvasHandle = { zoom: () => number; zoomAt: (zoom: number) => void; fitAll: () => void };
export type LinkModel = { id: string; from: string; state: NodeState | '' };

export const MIN_ZOOM = 0.2, MAX_ZOOM = 2, MIN_W = 340, MIN_H = 180, GAP = 28;
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const overlaps = (a: Rect, b: Rect) =>
  a.x < b.x + b.w + GAP / 2 && b.x < a.x + a.w + GAP / 2 && a.y < b.y + b.h + GAP / 2 && b.y < a.y + a.h + GAP / 2;

/** Primeira vaga livre numa grade à direita de `anchor`, três blocos por coluna. */
export function freeSlot(rects: Rect[], anchor: Rect, w: number, h: number): Rect {
  for (let col = 0; ; col++) {
    for (let row = 0; row < 3; row++) {
      const slot = { x: anchor.x + anchor.w + 2 * GAP + col * (w + GAP), y: anchor.y + row * (h + GAP), w, h };
      if (!rects.some((r) => overlaps(r, slot))) return slot;
    }
  }
}

/** Movimento em pixels de tela; o zoom converte para o espaço do mundo. */
export const moved = (r: Rect, dx: number, dy: number, z: number): Rect => ({ ...r, x: r.x + dx / z, y: r.y + dy / z });
export const resized = (r: Rect, dx: number, dy: number, z: number): Rect =>
  ({ ...r, w: Math.max(MIN_W, r.w + dx / z), h: Math.max(MIN_H, r.h + dy / z) });

/** Sai do lado do orquestrador que olha para o executor. */
function linkPath(a: Rect, b: Rect) {
  const right = b.x + b.w / 2 >= a.x + a.w / 2;
  const x1 = right ? a.x + a.w : a.x, y1 = a.y + a.h / 2;
  const x2 = right ? b.x : b.x + b.w, y2 = b.y + b.h / 2;
  const bend = Math.max(40, Math.abs(x2 - x1) / 2) * (right ? 1 : -1);
  return `M${x1},${y1} C${x1 + bend},${y1} ${x2 - bend},${y2} ${x2},${y2}`;
}

const LINK_STYLE: Record<string, { stroke: string; opacity: number; dash?: string }> = {
  '': { stroke: 'var(--status-queued)', opacity: 0.75 },
  running: { stroke: 'var(--status-running)', opacity: 1 },
  'awaiting-choice': { stroke: 'var(--status-awaiting)', opacity: 0.75, dash: '5 4' },
  queued: { stroke: 'var(--status-queued)', opacity: 0.75, dash: '2 5' },
  done: { stroke: 'var(--status-done)', opacity: 0.45 },
  stopped: { stroke: 'var(--status-stopped)', opacity: 0.45 },
  failed: { stroke: 'var(--status-failed)', opacity: 0.7 },
};

/** Arrasto com o ponteiro capturado: `move` recebe o deslocamento desde o último evento. */
function drag(start: PointerEvent, target: HTMLElement, move: (dx: number, dy: number) => void) {
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
function relayTerminalMouse(zoom: () => number) {
  let dragging: Element | null = null;
  const relay = (e: MouseEvent) => {
    if (!e.isTrusted || zoom() === 1) return;
    const screen = dragging ?? (e.target as Element).closest?.('.xterm-screen');
    if (e.type === 'mousedown') dragging = screen ?? null;
    if (e.type === 'mouseup') dragging = null;
    if (!screen || !e.target) return;
    e.stopImmediatePropagation();
    const box = screen.getBoundingClientRect(), z = zoom();
    const copy = new MouseEvent(e.type, {
      bubbles: true, cancelable: true, composed: true, view: window, detail: e.detail,
      screenX: e.screenX, screenY: e.screenY, button: e.button, buttons: e.buttons,
      ctrlKey: e.ctrlKey, shiftKey: e.shiftKey, altKey: e.altKey, metaKey: e.metaKey,
      clientX: box.left + (e.clientX - box.left) / z, clientY: box.top + (e.clientY - box.top) / z,
    });
    if (!e.target.dispatchEvent(copy)) e.preventDefault();
  };
  const types = ['mousedown', 'mousemove', 'mouseup'] as const;
  for (const type of types) window.addEventListener(type, relay, true);
  return () => { for (const type of types) window.removeEventListener(type, relay, true); };
}

type CanvasProps = {
  rects: Record<string, Rect>;
  links: LinkModel[];
  /** Enquadra tudo quando `seq` muda; com `reveal`, só se esse bloco estiver fora da área visível. */
  fit: { seq: number; reveal?: string };
  onZoom: (zoom: number) => void;
  children: ReactNode;
};

export const Canvas = forwardRef<CanvasHandle, CanvasProps>(function Canvas({ rects, links, fit, onZoom, children }, ref) {
  const viewport = useRef<HTMLDivElement>(null);
  const world = useRef<HTMLDivElement>(null);
  const view = useRef({ x: 0, y: 0, z: 1 });
  const rectsRef = useRef(rects);
  const onZoomRef = useRef(onZoom);
  useLayoutEffect(() => {
    rectsRef.current = rects;
    onZoomRef.current = onZoom;
  });

  // Aplica o transform direto no DOM: pan e zoom não passam pelo React.
  const setView = (x: number, y: number, z: number) => {
    const previous = view.current.z;
    view.current = { x, y, z };
    world.current!.style.transform = `translate(${x}px, ${y}px) scale(${z})`;
    viewport.current!.style.backgroundPosition = `${x}px ${y}px`;
    viewport.current!.style.backgroundSize = `${24 * z}px ${24 * z}px`;
    if (z !== previous) onZoomRef.current(z);
  };

  /** Zoom mantendo fixo o ponto (cx, cy) da área visível; sem ponto, o centro. */
  const zoomAt = (zoom: number, cx = viewport.current!.clientWidth / 2, cy = viewport.current!.clientHeight / 2) => {
    const { x, y, z: old } = view.current;
    const z = clamp(zoom, MIN_ZOOM, MAX_ZOOM), k = z / old;
    setView(cx - (cx - x) * k, cy - (cy - y) * k, z);
  };

  /** Enquadra todos os blocos (sem ampliar além de 100%). */
  const fitAll = () => {
    const list = Object.values(rectsRef.current);
    if (!list.length) return setView(0, 0, 1);
    const x0 = Math.min(...list.map((r) => r.x)), y0 = Math.min(...list.map((r) => r.y));
    const x1 = Math.max(...list.map((r) => r.x + r.w)), y1 = Math.max(...list.map((r) => r.y + r.h));
    const vw = viewport.current!.clientWidth, vh = viewport.current!.clientHeight, pad = 32;
    const z = clamp(Math.min((vw - 2 * pad) / (x1 - x0), (vh - 2 * pad) / (y1 - y0), 1), MIN_ZOOM, MAX_ZOOM);
    setView((vw - (x1 - x0) * z) / 2 - x0 * z, (vh - (y1 - y0) * z) / 2 - y0 * z, z);
  };

  useImperativeHandle(ref, () => ({ zoom: () => view.current.z, zoomAt: (z) => zoomAt(z), fitAll }));

  useEffect(() => {
    const vp = viewport.current!;
    // Pan: arrastar o fundo (qualquer ponto fora de um bloco).
    const onDown = (e: PointerEvent) => {
      if (e.button !== 0 || (e.target as Element).closest('[data-block]')) return;
      (document.activeElement as HTMLElement | null)?.blur();
      drag(e, vp, (dx, dy) => setView(view.current.x + dx, view.current.y + dy, view.current.z));
    };
    // Zoom com a roda, ancorado no cursor. Dentro do corpo de um bloco a roda é do terminal (Ctrl força o zoom).
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && (e.target as Element).closest('.block-body')) return;
      e.preventDefault();
      const box = vp.getBoundingClientRect();
      zoomAt(view.current.z * Math.exp(-e.deltaY * 0.0015), e.clientX - box.left, e.clientY - box.top);
    };
    vp.addEventListener('pointerdown', onDown);
    vp.addEventListener('wheel', onWheel, { passive: false });
    const stopRelay = relayTerminalMouse(() => view.current.z);
    return () => {
      vp.removeEventListener('pointerdown', onDown);
      vp.removeEventListener('wheel', onWheel);
      stopRelay();
    };
  }, []);

  useEffect(() => {
    const r = fit.reveal ? rectsRef.current[fit.reveal] : undefined;
    const { x, y, z } = view.current;
    const visible = !!r && x + r.x * z >= 0 && y + r.y * z >= 0 &&
      x + (r.x + r.w) * z <= viewport.current!.clientWidth && y + (r.y + r.h) * z <= viewport.current!.clientHeight;
    if (!visible) fitAll();
  }, [fit.seq]);

  return (
    <div ref={viewport} className="canvas-grid absolute inset-0 touch-none cursor-grab overflow-hidden">
      <div ref={world} className="absolute top-0 left-0 h-0 w-0 origin-top-left">
        <svg className="pointer-events-none absolute top-0 left-0 h-px w-px overflow-visible">
          {links.map(({ id, from, state }) => {
            const a = rects[from], b = rects[id];
            if (!a || !b) return null;
            const style = LINK_STYLE[state] ?? LINK_STYLE[''];
            return (
              <path
                key={id}
                d={linkPath(a, b)}
                fill="none"
                strokeWidth={1.5}
                vectorEffect="non-scaling-stroke"
                style={{ stroke: style.stroke, opacity: style.opacity, strokeDasharray: style.dash }}
              />
            );
          })}
        </svg>
        {children}
      </div>
    </div>
  );
});

type BlockProps = {
  rect: Rect;
  state?: NodeState;
  head: ReactNode;
  children: ReactNode;
  onMove: (dx: number, dy: number) => void;
  onResize: (dx: number, dy: number) => void;
};

let topZ = 1;
const FINISHED: string[] = ['done', 'stopped', 'failed'];

/** Moldura de um bloco: cabeçalho arrastável, corpo (terminal + camadas) e alça de redimensionar. */
export function Block({ rect, state, head, children, onMove, onResize }: BlockProps) {
  const frame = useRef<HTMLElement>(null);
  const headEl = useRef<HTMLElement>(null);
  const grip = useRef<HTMLDivElement>(null);
  const callbacks = useRef({ onMove, onResize });
  useLayoutEffect(() => {
    callbacks.current = { onMove, onResize };
  });

  useEffect(() => {
    const el = frame.current!, h = headEl.current!, g = grip.current!;
    const raise = () => { el.style.zIndex = String(++topZ); };
    const onHead = (e: PointerEvent) => {
      if (e.button !== 0 || (e.target as Element).closest('button, input, [role="combobox"]')) return;
      drag(e, h, (dx, dy) => callbacks.current.onMove(dx, dy));
    };
    const onGrip = (e: PointerEvent) => {
      if (e.button !== 0) return;
      e.stopPropagation();
      drag(e, g, (dx, dy) => callbacks.current.onResize(dx, dy));
    };
    el.addEventListener('pointerdown', raise, true);
    h.addEventListener('pointerdown', onHead);
    g.addEventListener('pointerdown', onGrip);
    return () => {
      el.removeEventListener('pointerdown', raise, true);
      h.removeEventListener('pointerdown', onHead);
      g.removeEventListener('pointerdown', onGrip);
    };
  }, []);

  const finished = !!state && FINISHED.includes(state);
  return (
    <section
      ref={frame}
      data-block
      data-state={state}
      className={cn(
        'absolute flex flex-col overflow-hidden rounded-lg border bg-card text-card-foreground shadow-lg',
        'data-[state=failed]:border-destructive/60',
        finished && '[&_.term]:opacity-60',
      )}
      style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}
    >
      <header ref={headEl} className="flex h-9 shrink-0 cursor-grab items-center gap-2 border-b bg-muted/40 px-2 text-xs">
        {head}
      </header>
      <div className="block-body relative min-h-0 flex-1">{children}</div>
      <div
        ref={grip}
        title="Redimensionar"
        className="absolute right-0 bottom-0 size-4 cursor-nwse-resize touch-none after:absolute after:right-[3px] after:bottom-[3px] after:size-[7px] after:border-r-2 after:border-b-2 after:border-muted-foreground/50 after:content-['']"
      />
    </section>
  );
}
