// Decorações do Git na árvore de arquivos, como no Explorer do VS Code: cor e letra no arquivo alterado, cor e "•" na pasta
// que tem algo alterado dentro, cinza no que o `.gitignore` ignora. As cores são as do tema escuro do VS Code (gitDecoration.*).
import type { GitFile, GitKind } from './types';

export type Decoration = { color: string; letter: string; title: string };

// Da mais forte para a mais fraca: a pasta fica com a cor da alteração mais forte de dentro dela.
const ORDER: GitKind[] = ['conflicted', 'deleted', 'modified', 'added', 'renamed', 'untracked'];
const STYLE: Record<GitKind, { color: string; letter: string; title: string }> = {
  conflicted: { color: '#e4676b', letter: '!', title: 'Em conflito' },
  deleted: { color: '#c74e39', letter: 'D', title: 'Apagado' },
  modified: { color: '#E2C08D', letter: 'M', title: 'Modificado' },
  added: { color: '#81b88b', letter: 'A', title: 'Novo no índice' },
  renamed: { color: '#73C991', letter: 'R', title: 'Renomeado' },
  untracked: { color: '#73C991', letter: 'U', title: 'Não rastreado' },
};
export const IGNORED: Decoration = { color: '#8C8C8C', letter: '', title: 'Ignorado pelo .gitignore' };

const stronger = (a: GitKind | undefined, b: GitKind | null): GitKind | undefined =>
  (!b ? a : !a || ORDER.indexOf(b) < ORDER.indexOf(a) ? b : a);

/** Caminho (arquivo ou pasta, relativo com "/") -> decoração. Pasta leva "•" no lugar da letra. */
export function decorations(files: GitFile[]): Map<string, Decoration> {
  const kinds = new Map<string, GitKind>();
  const dirs = new Map<string, GitKind>();
  for (const f of files) {
    const kind = stronger(stronger(undefined, f.staged), f.unstaged);
    if (!kind) continue;
    kinds.set(f.path, kind);
    const parts = f.path.split('/');
    for (let i = 1; i < parts.length; i++) {
      const dir = parts.slice(0, i).join('/');
      dirs.set(dir, stronger(dirs.get(dir), kind)!);
    }
  }
  const out = new Map<string, Decoration>();
  dirs.forEach((kind, dir) => out.set(dir, { ...STYLE[kind], letter: '•', title: `Contém itens: ${STYLE[kind].title.toLowerCase()}` }));
  kinds.forEach((kind, path) => out.set(path, STYLE[kind]));
  return out;
}
