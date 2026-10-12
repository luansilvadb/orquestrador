// A navegação da coluna de ícones, a árvore de pastas (na tela principal) e a barra de status com a branch.
// A árvore se atualiza sozinha (usePolling). O status do Git vem do App, que o usa no contador da navegação e na barra de status.
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { ChevronDown, ChevronRight, File, Files, Folder, FolderOpen, GitBranch, GitCommitHorizontal } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { backend, message } from './backend';
import { go, type Route } from './route';
import { usePolling } from './polling';
import type { DirEntry } from './types';

// Recuo por nível: a seta (ou o espaço dela, nos arquivos) fica sempre na mesma coluna de cada nível.
const indent = (depth: number) => 8 + depth * 16;

// Caminho relativo com "/" (como o backend); a raiz é ''.
const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
const inside = (dir: string, path: string) => path.startsWith(`${dir}/`);

type Tree = { children: Record<string, DirEntry[]>; expanded: string[] };
/** Tira uma pasta e tudo o que está dentro dela, aberto ou com lista guardada. */
const drop = (tree: Tree, path: string): Tree => ({
  expanded: tree.expanded.filter((p) => p !== path && !inside(path, p)),
  children: Object.fromEntries(Object.entries(tree.children).filter(([p]) => p !== path && !inside(path, p))),
});
const sameList = (a: DirEntry[] | undefined, b: DirEntry[]) =>
  !!a && a.length === b.length && a.every((e, i) => e.name === b[i].name && e.dir === b[i].dir);

/** Coluna de ícones: uma tela por botão, na ordem Arquivos, Git. O botão da tela aberta tem aria-current. */
export function Nav({ route, changed }: { route: Route; changed: number }) {
  const button = (to: Route, label: string, icon: ReactNode, title = label) => (
    <Button
      variant={route === to ? 'secondary' : 'ghost'} size="icon-sm"
      aria-label={label} aria-current={route === to ? 'page' : undefined} title={title}
      onClick={() => go(to)}
    >
      {icon}
    </Button>
  );
  const gitLabel = changed ? `Git (${changed} ${changed === 1 ? 'arquivo alterado' : 'arquivos alterados'})` : 'Git';
  return (
    <nav aria-label="Telas" className="flex w-11 shrink-0 flex-col items-center gap-1 border-r bg-card py-2">
      {button('arquivos', 'Arquivos', <Files />)}
      <div className="relative">
        {button('git', gitLabel, <GitCommitHorizontal />, 'Git')}
        {changed > 0 && (
          <Badge aria-hidden="true" className="pointer-events-none absolute -top-1 -right-1 h-4 min-w-4 px-1 text-[10px]">
            {changed}
          </Badge>
        )}
      </div>
    </nav>
  );
}

/** Árvore de pastas do projeto, à esquerda dos terminais na tela principal. Pasta abre e fecha; clicar em arquivo não faz nada. */
export function FilesScreen() {
  const [tree, setTree] = useState<Tree>({ children: {}, expanded: [] });
  const [error, setError] = useState('');
  const expanded = useRef<string[]>([]);
  useLayoutEffect(() => {
    expanded.current = tree.expanded;
  });

  // Uma rodada consulta a raiz e cada pasta aberta. O estado só muda quando a resposta é diferente.
  // Pasta aberta que não lista mais sai da árvore sem erro na tela; falha na raiz aparece como texto, não como toast.
  usePolling(async () => {
    const paths = ['', ...expanded.current];
    const results = await Promise.all(paths.map((path) =>
      backend.listDir(path).then((list) => ({ list }), (e: unknown) => ({ error: message(e) }))));
    const root = results[0];
    setError('error' in root ? root.error : '');
    setTree((prev) => {
      let next = prev;
      paths.forEach((path, i) => {
        const r = results[i];
        if (!path) {
          if ('list' in r && !sameList(next.children[''], r.list)) next = { ...next, children: { ...next.children, '': r.list } };
        } else if (prev.expanded.includes(path)) {
          // Pasta fechada enquanto a consulta ia: a resposta não se aplica.
          if ('error' in r) next = drop(next, path);
          else if (!sameList(next.children[path], r.list)) next = { ...next, children: { ...next.children, [path]: r.list } };
        }
      });
      return next;
    });
  });

  async function toggle(path: string, isOpen: boolean) {
    if (isOpen) return setTree((prev) => drop(prev, path));
    setTree((prev) => ({ ...prev, expanded: [...prev.expanded, path] }));
    try {
      const list = await backend.listDir(path);
      setTree((prev) => (prev.expanded.includes(path) ? { ...prev, children: { ...prev.children, [path]: list } } : prev));
    } catch {
      // A pasta sumiu antes de abrir: sai da árvore sem erro na tela, como na atualização.
      setTree((prev) => drop(prev, path));
    }
  }

  const rows = (dir: string, depth: number): ReactNode[] =>
    (tree.children[dir] ?? []).flatMap((entry) => {
      const path = join(dir, entry.name);
      const style = { paddingLeft: indent(depth) };
      if (!entry.dir) {
        return [
          <div key={path} title={entry.name} style={style} className="flex h-6 items-center gap-1 pr-2 text-sm">
            <span className="size-3.5 shrink-0" />
            <File className="size-4 shrink-0 text-muted-foreground" />
            <span className="min-w-0 truncate">{entry.name}</span>
          </div>,
        ];
      }
      const isOpen = tree.expanded.includes(path);
      return [
        <button
          key={path} type="button" aria-expanded={isOpen} title={entry.name} style={style}
          onClick={() => toggle(path, isOpen)}
          className="flex h-6 w-full items-center gap-1 rounded-sm pr-2 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          {isOpen
            ? <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
            : <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />}
          {isOpen ? <FolderOpen className="size-4 shrink-0" /> : <Folder className="size-4 shrink-0" />}
          <span className="min-w-0 truncate">{entry.name}</span>
        </button>,
        ...(isOpen ? rows(path, depth + 1) : []),
      ];
    });

  return (
    <div className="h-full overflow-y-auto overflow-x-hidden py-2">
      {error && <p role="status" className="px-3 py-1 text-xs text-muted-foreground">{error}</p>}
      {!tree.children[''] && !error && <p className="px-3 py-1 text-xs text-muted-foreground">Lendo a pasta…</p>}
      {rows('', 0)}
    </div>
  );
}

/** Barra de status fina embaixo da tela principal: só a branch atual do projeto. */
export function StatusBar({ branch }: { branch: string | undefined }) {
  return (
    <footer className="flex h-6 shrink-0 items-center gap-1.5 border-t bg-card px-3 text-xs text-muted-foreground">
      {branch && (
        <>
          <GitBranch className="size-3.5 shrink-0" />
          <span title={branch} className="min-w-0 truncate font-mono text-foreground">{branch}</span>
        </>
      )}
    </footer>
  );
}
