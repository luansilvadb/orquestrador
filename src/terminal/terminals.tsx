// Os terminais xterm: cada um é um TermView criado quando o TermHost monta e descartado quando ele sai.
import { memo, useEffect, useLayoutEffect, useRef } from 'react';
import { TermView } from './term';
import { termKey as key } from './term-key';
import type { TermData } from '../backend/types';

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

/**
 * Props do contêiner de cada terminal da pilha (`absolute inset-0`, dentro de um pai com overflow hidden). A aba inativa continua com o
 * tamanho (sem resize/reflow ao trocar), mas é deslocada para fora do pai: o IntersectionObserver do RenderService do xterm passa a ver "não
 * intersecta" e pausa a renderização (a saída segue sendo processada; ao voltar, o xterm repinta tudo). `visibility: hidden` sozinho não
 * muda a interseção. `invisible` e `inert` tiram o escondido do foco, dos cliques e da árvore de acessibilidade.
 */
export const termPane = (shown: boolean) => ({
  className: `absolute inset-0${shown ? '' : ' invisible -translate-x-[200%]'}`,
  inert: !shown,
});

/**
 * `shown`: a aba é a ativa e o painel e o projeto aparecem. `onReady` só é lido na montagem, por isso não entra na comparação do `memo`: o
 * pai pode passar uma função nova a cada render sem reconciliar os N terminais.
 */
type TermHostProps = { id: string; project?: string | null; writable: boolean; shown: boolean; onReady?: (view: TermView) => void };

export const TermHost = memo(function TermHost({ id, project = null, writable, shown, onReady }: TermHostProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<TermView | null>(null);
  const shownNow = useRef(shown);
  // Antes da pintura: quem aparece já pinta com WebGL, sem um quadro no renderizador DOM.
  useLayoutEffect(() => {
    shownNow.current = shown;
    view.current?.setShown(shown);
  }, [shown]);

  useEffect(() => {
    const created = new TermView(id, host.current!, writable, project, shownNow.current);
    const k = key(id, project);
    views.set(k, created);
    view.current = created;
    for (const data of early.get(k) ?? []) created.push(data.seq, data.data);
    early.delete(k);
    onReady?.(created);
    return () => {
      if (views.get(k) === created) views.delete(k);
      if (view.current === created) view.current = null;
      created.dispose();
    };
  }, [id, project, writable]);

  // 12 px à esquerda: o texto do terminal começa na coluna dos cabeçalhos dos painéis (paddingInline 12).
  return <div ref={host} className="term absolute top-2 right-0.5 bottom-1 left-3 select-text" />;
}, (a, b) => a.id === b.id && a.project === b.project && a.writable === b.writable && a.shown === b.shown);
