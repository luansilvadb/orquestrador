// Faixa de abas com rolagem horizontal, compartilhada pelo editor, pelo painel do Orquestrador e pelo painel de terminais.
// Quem chama acha o próprio elemento da aba; aqui só se mexe no scroll da faixa (sem scrollIntoView, que rola os ancestrais).
import type { PointerEvent, WheelEvent } from 'react';

// Arrasto de aba: começa depois de DRAG_START px, para o clique seguir só selecionando; a menos de EDGE px da borda da faixa, ela rola sozinha.
const DRAG_START = 4;
const EDGE = 24;

/** Aba em arrasto: desloca `dx` px da posição original; `target` é onde ela entra entre as outras; `width` é o espaço que as vizinhas abrem. */
export type TabDrag = { dx: number; target: number; width: number };

/** Deslocamento de cada aba durante o arrasto: a arrastada segue `dx`; as vizinhas entre a origem e o destino abrem o espaço dela. */
export function dragShift(drag: TabDrag, from: number, index: number) {
  return index === from ? drag.dx
    : from < index && index <= drag.target ? -drag.width
    : drag.target <= index && index < from ? drag.width : 0;
}

/**
 * Segurar a aba `nodes[from]` e arrastar para os lados: `onDrag` recebe a posição a cada passo e `onDrop` o destino ao soltar (null se Esc,
 * perda do ponteiro ou da janela cancelam). Clique sem arrasto não chama nenhum dos dois. `range` limita as posições alcançáveis (índices em `nodes`).
 */
export function trackTabDrag(e: PointerEvent<HTMLElement>, box: HTMLElement, nodes: HTMLElement[], from: number, { range = [0, nodes.length - 1], onStart, onDrag, onDrop }: {
  range?: [number, number];
  onStart: () => void;
  onDrag: (drag: TabDrag, x: number, y: number) => void;
  onDrop: (target: number | null, x: number, y: number) => void;
}) {
  const [lo, hi] = range;
  // Posições de antes do arrasto (offset ignora as transformações): o destino é medido contra elas.
  const slots = nodes.map((node) => ({ left: node.offsetLeft, width: node.offsetWidth }));
  const self = slots[from];
  const min = slots[lo].left - self.left;
  const max = slots[hi].left + slots[hi].width - self.left - self.width;
  const startX = e.clientX;
  const startScroll = box.scrollLeft;
  const pointer = e.pointerId;
  let x = startX;
  let y = e.clientY;
  let moving = false;
  let target = from;
  let frame = 0;

  const update = () => {
    // A rolagem da faixa conta no deslocamento: a aba fica sob o ponteiro enquanto a faixa anda.
    const dx = Math.min(max, Math.max(min, x - startX + box.scrollLeft - startScroll));
    // Passa uma vizinha quando a borda da frente cruza o meio dela: com a aba travada no fim da faixa, ainda alcança a primeira e a última posição.
    const left = self.left + dx;
    target = lo + slots.filter((slot, i) => i !== from && i >= lo && i <= hi && slot.left + slot.width / 2 < (i < from ? left : left + self.width)).length;
    onDrag({ dx, target, width: self.width }, x, y);
  };
  const edgeScroll = () => {
    const rect = box.getBoundingClientRect();
    const before = box.scrollLeft;
    box.scrollLeft += x < rect.left + EDGE ? -8 : x > rect.right - EDGE ? 8 : 0;
    if (box.scrollLeft !== before) update();
    frame = requestAnimationFrame(edgeScroll);
  };
  const move = (ev: WindowEventMap['pointermove']) => {
    if (ev.pointerId !== pointer) return;
    x = ev.clientX;
    y = ev.clientY;
    if (!moving) {
      if (Math.abs(x - startX) < DRAG_START) return;
      moving = true;
      onStart();
      frame = requestAnimationFrame(edgeScroll);
    }
    update();
  };
  const finish = (commit: boolean) => {
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    window.removeEventListener('pointercancel', cancel);
    window.removeEventListener('blur', cancel);
    window.removeEventListener('keydown', escape, true);
    cancelAnimationFrame(frame);
    if (moving) onDrop(commit ? target : null, x, y);
  };
  const up = (ev: WindowEventMap['pointerup']) => { if (ev.pointerId === pointer) finish(true); };
  const cancel = () => finish(false);
  // Em captura, para o Esc não chegar ao shell nem ao editor.
  const escape = (ev: WindowEventMap['keydown']) => {
    if (ev.key !== 'Escape' || !moving) return;
    ev.preventDefault();
    ev.stopPropagation();
    finish(false);
  };
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
  window.addEventListener('pointercancel', cancel);
  window.addEventListener('blur', cancel);
  window.addEventListener('keydown', escape, true);
}

/** Mantém a aba dentro da área visível da faixa. Faixa escondida (largura 0) não tem o que mostrar; ao reabrir, o efeito roda de novo. */
export function revealTab(box: HTMLElement, tab: HTMLElement | null | undefined) {
  if (!tab || box.clientWidth === 0) return;
  // Medidas com subpixel (rect), não offsetWidth/clientWidth: a borda da aba não pode passar da faixa nem por fração de pixel.
  const edge = box.getBoundingClientRect();
  const rect = tab.getBoundingClientRect();
  const left = rect.left - edge.left + box.scrollLeft;
  const right = left + rect.width;
  // Aba maior que a faixa: mostra o começo dela (o rótulo), não o fim. Esquerda arredonda para baixo e direita para cima.
  if (left < box.scrollLeft || rect.width > edge.width) box.scrollLeft = Math.floor(left);
  else if (right > box.scrollLeft + edge.width) box.scrollLeft = Math.ceil(right - edge.width);
}

/** Roda do mouse rola a faixa na horizontal, quando o gesto é mais vertical que horizontal. */
export function wheelToScroll(e: WheelEvent<HTMLElement>) {
  if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY;
}
