// Rota por hash (#/arquivos, #/git), sem biblioteca. Sem hash, hash desconhecido ou o antigo #/terminais cai em arquivos.
import { useEffect, useState } from 'react';

export type Route = 'arquivos' | 'git';
const ROUTES: Route[] = ['arquivos', 'git'];

const read = (): Route => {
  const to = location.hash.replace(/^#\//, '');
  return ROUTES.find((r) => r === to) ?? 'arquivos';
};

/** Rota atual; acompanha o hash, seja pelo botão da navegação, pelo voltar do navegador ou por digitação. */
export function useRoute(): Route {
  const [route, setRoute] = useState(read);
  useEffect(() => {
    const sync = () => setRoute(read());
    window.addEventListener('hashchange', sync);
    return () => window.removeEventListener('hashchange', sync);
  }, []);
  return route;
}

export const go = (to: Route) => {
  location.hash = `/${to}`;
};
