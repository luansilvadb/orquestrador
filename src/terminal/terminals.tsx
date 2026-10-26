// Os terminais xterm: cada um é um TermView criado quando o TermHost monta e descartado quando ele sai.
import { useEffect, useLayoutEffect, useRef } from 'react';
import { TermView } from './term';
import type { TermData } from '../backend/types';

const key = (id: string, project?: string | null) => `${project ?? ''}\0${id}`;
const views = new Map<string, TermView>();
/** Dados que chegam antes de o terminal montar; sem isso o começo do fluxo se perderia. */
const early = new Map<string, TermData[]>();
// Só IDs de executor são monotônicos; shells e o orquestrador podem reutilizar a chave.
const forgotten = new Set<string>();

export const terminals = {
  get: (id: string, project?: string | null) => views.get(key(id, project)),
  push(data: TermData) {
    const k = key(data.id, data.project);
    if (forgotten.has(k)) return;
    const view = views.get(k);
    if (view) view.push(data.seq, data.data);
    else if (early.has(k)) early.get(k)!.push(data);
    else early.set(k, [data]);
  },
  forget(id: string, project?: string | null, ended = false) {
    const k = key(id, project);
    early.delete(k);
    if (ended) forgotten.add(k);
  },
};

export function TermHost({ id, project = null, writable, onReady }: { id: string; project?: string | null; writable: boolean; onReady?: (view: TermView) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const ready = useRef(onReady);
  useLayoutEffect(() => {
    ready.current = onReady;
  });

  useEffect(() => {
    const view = new TermView(id, host.current!, writable, project);
    const k = key(id, project);
    views.set(k, view);
    for (const data of early.get(k) ?? []) view.push(data.seq, data.data);
    early.delete(k);
    ready.current?.(view);
    return () => {
      if (views.get(k) === view) views.delete(k);
      view.dispose();
    };
  }, [id, project, writable]);

  // 12 px à esquerda: o texto do terminal começa na coluna dos cabeçalhos dos painéis (paddingInline 12).
  return <div ref={host} className="term absolute top-2 right-0.5 bottom-1 left-3 select-text" />;
}
