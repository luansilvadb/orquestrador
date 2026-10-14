// Faixa de abas com rolagem horizontal, compartilhada pelo editor, pelo painel do Orquestrador e pelo painel de terminais.
// Quem chama acha o próprio elemento da aba; aqui só se mexe no scroll da faixa (sem scrollIntoView, que rola os ancestrais).
import type { WheelEvent } from 'react';

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
