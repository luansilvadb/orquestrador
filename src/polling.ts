// Atualização periódica de telas (lista de arquivos, branch, painel Git e aba de alterações).
import { useEffect, useLayoutEffect, useRef } from 'react';

const POLL_MS = 1500;

/** Repete `task` com o componente montado: uma rodada por vez, e nenhuma com a janela oculta. `task` não lança. */
export function usePolling(task: () => Promise<void>) {
  const run = useRef(task);
  useLayoutEffect(() => {
    run.current = task;
  });
  useEffect(() => {
    let stop = false;
    const loop = async () => {
      while (!stop) {
        if (!document.hidden) await run.current();
        await new Promise((resolve) => setTimeout(resolve, POLL_MS));
      }
    };
    void loop();
    return () => {
      stop = true;
    };
  }, []);
}
