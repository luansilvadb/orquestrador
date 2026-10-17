import { toast } from '@lobehub/ui/base-ui';
import { message } from '../backend/backend';

/** Pasta e nome de um caminho relativo ao projeto. */
export function splitPath(path: string) {
  const slash = path.lastIndexOf('/');
  return { name: path.slice(slash + 1), dir: slash >= 0 ? path.slice(0, slash) : '' };
}
/** Junta pasta e nome num caminho relativo ao projeto (pasta '' é a raiz). */
export const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);

/** Copia para a área de transferência. Falha vira toast de erro; sucesso, toast curto. */
export async function copy(text: string, done: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast.success({ title: done, duration: 2000 });
  } catch (e) {
    toast.error({ title: message(e), duration: 5000 });
  }
}
