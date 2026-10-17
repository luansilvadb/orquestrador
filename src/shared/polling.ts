// Atualização periódica de telas (lista de arquivos, branch, painel Git e aba de alterações).
import { useEffect, useLayoutEffect, useRef } from 'react';

const POLL_MS = 1500;

/**
 * Repete `task` com o componente montado: uma rodada por vez, e nenhuma com a janela oculta. `task` não lança.
 * Com `key` falsy não roda. Quando `key` muda, o laço recomeça e a primeira rodada sai na hora.
 */
export function usePolling(task: () => Promise<void>, ms = POLL_MS, key: unknown = true) {
  const run = useRef(task);
  useLayoutEffect(() => {
    run.current = task;
  });
  useEffect(() => {
    if (!key) return;
    let stop = false;
    const loop = async () => {
      while (!stop) {
        if (!document.hidden) await run.current();
        await new Promise((resolve) => setTimeout(resolve, ms));
      }
    };
    void loop();
    return () => {
      stop = true;
    };
  }, [key, ms]);
}
