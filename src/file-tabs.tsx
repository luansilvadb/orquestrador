// Helpers de menu, de nome e de acorde compartilhados pela árvore (src/explorer.tsx) e pelo editor central (src/editor-workspace.tsx).
// A faixa de abas antiga saiu: as abas agora são do editor central, com grupos e prévias.
import { useEffect, useRef } from 'react';
import { type ContextMenuItem } from '@lobehub/ui/base-ui';
import { splitPath } from './git-history';

export const fileName = (path: string) => splitPath(path).name;

/** Acorde Ctrl+K: vale por 1,5 s, `end` cancela (o foco saiu da área) e o timer some com o componente. */
export function useChord() {
  const pending = useRef(false);
  const timer = useRef(0);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const begin = () => {
    pending.current = true;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => { pending.current = false; }, 1500);
  };
  const end = () => {
    pending.current = false;
    window.clearTimeout(timer.current);
  };
  return { pending, begin, end };
}

// O atalho à direita do item. Fonte, tamanho e opacidade vêm do CSS dos menus (src/index.css), como no Antigravity.
export const action = (key: string, label: string, run: () => void, shortcut?: string): ContextMenuItem => ({
  key, label, extra: shortcut ? <span>{shortcut}</span> : undefined, onClick: run,
});
// Ação sem alvo: desabilitada e apagada, como no Antigravity. O motivo fica no tooltip e é lido pelo leitor de tela.
export const blocked = (key: string, label: string, reason: string): ContextMenuItem => ({
  key, disabled: true, label: <span title={reason}>{label}<span className="sr-only">. {reason}</span></span>,
});
export const DIVIDER: ContextMenuItem = { type: 'divider' };
