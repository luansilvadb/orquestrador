// Os terminais xterm: cada um é um TermView criado quando o TermHost monta e descartado quando ele sai.
import { useEffect, useLayoutEffect, useRef } from 'react';
import { TermView } from './term';
import type { TermData } from './types';

const views = new Map<string, TermView>();
/** Dados que chegam antes de o terminal montar; sem isso o começo do fluxo se perderia. */
const early = new Map<string, TermData[]>();

export const terminals = {
  get: (id: string) => views.get(id),
  push(data: TermData) {
    const view = views.get(data.id);
    if (view) view.push(data.seq, data.data);
    else early.set(data.id, [...(early.get(data.id) ?? []), data]);
  },
  forget(id: string) {
    early.delete(id);
  },
};

export function TermHost({ id, writable, onReady }: { id: string; writable: boolean; onReady?: (view: TermView) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const ready = useRef(onReady);
  useLayoutEffect(() => {
    ready.current = onReady;
  });

  useEffect(() => {
    const view = new TermView(id, host.current!, writable);
    views.set(id, view);
    for (const data of early.get(id) ?? []) view.push(data.seq, data.data);
    early.delete(id);
    ready.current?.(view);
    return () => {
      if (views.get(id) === view) views.delete(id);
      view.dispose();
    };
  }, [id, writable]);

  return <div ref={host} className="term absolute top-1.5 right-0.5 bottom-1 left-2 select-text" />;
}
