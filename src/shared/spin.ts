// Animação do ícone do botão a cada clique, pelo que o ícone mostra: atualizar dá uma volta; Enviar solta a seta para cima, Receber e
// Buscar para baixo. O retorno visual aparece mesmo quando a resposta chega antes de a tela mudar. Web Animations, sem estado: um clique
// no meio recomeça a animação, e o foco do botão fica onde está.
const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)';
const reduced = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

export function spinIcon(button: Element) {
  if (reduced()) return;
  button.querySelector('svg')?.animate([{ transform: 'rotate(0deg)' }, { transform: 'rotate(360deg)' }], { duration: 500, easing: EASE });
}

// A seta sai pelo lado da direção e volta pelo oposto. Nos ícones usados (ArrowUpFromLine, ArrowDownToLine, CloudDownload, do lucide),
// o último traço é a base ou a nuvem, que fica parada; os outros formam a seta. `dy` em unidades do viewBox (24).
export function flyIcon(button: Element, direction: 'up' | 'down') {
  if (reduced()) return;
  const parts = [...(button.querySelector('svg')?.children ?? [])].slice(0, -1);
  const dy = direction === 'up' ? -8 : 8;
  for (const part of parts) {
    part.animate([
      { transform: 'translateY(0)', opacity: 1, easing: 'ease-in' },
      { transform: `translateY(${dy}px)`, opacity: 0, offset: 0.4 },
      { transform: `translateY(${-dy}px)`, opacity: 0, offset: 0.41, easing: EASE },
      { transform: 'translateY(0)', opacity: 1 },
    ], { duration: 600 });
  }
}
