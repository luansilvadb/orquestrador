// Trava das operações Git que a UI dispara (barra lateral e editor central): uma por vez, mesmo entre telas diferentes.
// O App cria uma só e passa para os dois. Não cobre `locked` (executores vivos): isso é regra do backend, que recusa a mutação.
// Stage e commit legados seguem como antes durante um executor; só a operação de outra tela da UI bloqueia.
import { toast } from '@lobehub/ui/base-ui';

export type GitGuard = {
  /** Começa uma operação. Devolve false (e avisa) se outra já está rodando. */
  enter(): boolean;
  /** Termina a operação que `enter` começou. Sempre chamar no `finally`. */
  leave(): void;
};

export function createGitGuard(): GitGuard {
  // Há operação em andamento (quem a começou é quem a termina).
  let busy = false;
  return {
    enter() {
      if (busy) {
        toast.warning({ title: 'Espere a operação do Git em andamento terminar.', duration: 4000 });
        return false;
      }
      busy = true;
      return true;
    },
    leave() {
      busy = false;
    },
  };
}
