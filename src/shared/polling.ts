// Atualização periódica de telas (lista de arquivos, branch, painel Git e aba de alterações).
import { useEffect, useLayoutEffect, useRef } from 'react';

const POLL_MS = 1500;
/** Intervalo com a janela visível e sem foco. */
const BLURRED_MS = 10000;

/** Grupo de pollers que `pokePolling` acorda: `files` é o status do Git e a árvore de arquivos. */
export type PollGroup = 'files';

type Poller = { wake: (now?: boolean) => void; retime: () => void; group?: PollGroup };
const pollers = new Set<Poller>();
let watching = false;

// Fonte única de foco e visibilidade: ouvintes registrados uma vez, na primeira montagem.
function watch() {
  if (watching) return;
  watching = true;
  const wakeAll = () => {
    if (!document.hidden) for (const p of pollers) p.wake();
  };
  window.addEventListener('focus', wakeAll);
  // Perder o foco estica a espera em curso para o intervalo sem foco.
  window.addEventListener('blur', () => {
    for (const p of pollers) p.retime();
  });
  document.addEventListener('visibilitychange', wakeAll);
}

/** Rodada imediata dos pollers do grupo, mesmo sem foco. Rajadas viram no máximo uma rodada pendente por poller. */
export function pokePolling(group: PollGroup) {
  if (document.hidden) return;
  for (const p of pollers) if (p.group === group) p.wake(true);
}

/**
 * Repete `task` com o componente montado: uma rodada por vez, a cada `ms` com foco, a cada 10 s sem foco e nenhuma com a janela oculta.
 * Ganhar foco ou voltar a ficar visível adianta a rodada; `group` deixa `pokePolling` adiantá-la também. `task` não lança.
 * Com `key` falsy não roda. Quando `key` muda, o laço recomeça e a primeira rodada sai na hora.
 */
export function usePolling(task: () => Promise<void>, ms = POLL_MS, key: unknown = true, group?: PollGroup) {
  const run = useRef(task);
  useLayoutEffect(() => {
    run.current = task;
  });
  useEffect(() => {
    if (!key) return;
    watch();
    let stop = false;
    let timer = 0;
    let resume: (() => void) | null = null;
    let since = 0;
    const arm = (delay: number) => {
      timer = window.setTimeout(() => {
        const r = resume;
        resume = null;
        r?.();
      }, delay);
    };
    // Pedido de rodada imediata que chegou durante uma rodada: sai logo depois dela (no máximo uma pendente).
    let pending = false;
    const poller: Poller = {
      group,
      wake: (now?: boolean) => {
        if (resume) {
          window.clearTimeout(timer);
          const r = resume;
          resume = null;
          r();
        } else if (now) pending = true;
      },
      retime: () => {
        if (!resume) return;
        window.clearTimeout(timer);
        arm(Math.max(0, BLURRED_MS - (performance.now() - since)));
      },
    };
    pollers.add(poller);
    const loop = async () => {
      while (!stop) {
        pending = false;
        if (!document.hidden) await run.current();
        if (stop) break;
        if (pending) continue;
        await new Promise<void>((resolve) => {
          resume = resolve;
          since = performance.now();
          arm(document.hasFocus() ? ms : BLURRED_MS);
        });
      }
    };
    void loop();
    return () => {
      stop = true;
      pollers.delete(poller);
      window.clearTimeout(timer);
      resume?.();
      resume = null;
    };
  }, [key, ms, group]);
}
