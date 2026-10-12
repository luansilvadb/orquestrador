// A moldura da tela principal: coluna de ícones, lista de arquivos do projeto e barra de status com a branch.
// A lista e a branch se atualizam sozinhas (usePolling). O canvas não sabe deste arquivo.
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { ChevronDown, ChevronRight, File, Files, Folder, FolderOpen, GitBranch } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { backend, message } from './backend';
import type { DirEntry } from './types';

const POLL_MS = 1500;
const WIDTH = { initial: 240, min: 160, max: 600 };
const clampWidth = (v: number) => Math.min(WIDTH.max, Math.max(WIDTH.min, v));
// Recuo por nível: a seta (ou o espaço dela, nos arquivos) fica sempre na mesma coluna de cada nível.
const indent = (depth: number) => 8 + depth * 16;

/** Repete `task` com o componente montado: uma rodada por vez, e nenhuma com a janela oculta. `task` não lança. */
function usePolling(task: () => Promise<void>) {
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

/** Lista de arquivos: a pasta do projeto como árvore, com a borda direita arrastável para mudar a largura. */
export function Explorer() {
  const [open, setOpen] = useState(true);
  const [width, setWidth] = useState(WIDTH.initial);
  const [tree, setTree] = useState<Tree>({ children: {}, expanded: [] });
  const [error, setError] = useState('');
  const expanded = useRef<string[]>([]);
  useLayoutEffect(() => {
    expanded.current = tree.expanded;
  });
  const resize = useRef<{ x: number; width: number } | null>(null);

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

  // A borda captura o ponteiro: o arrasto segue mesmo quando ele sai da faixa.
  const startResize = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    resize.current = { x: e.clientX, width };
  };
  const moveResize = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (resize.current) setWidth(clampWidth(resize.current.width + e.clientX - resize.current.x));
  };
  const endResize = () => {
    resize.current = null;
  };
  const keyResize = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    setWidth((w) => clampWidth(w + (e.key === 'ArrowLeft' ? -16 : 16)));
  };

  return (
    <>
      <div className="flex w-11 shrink-0 flex-col items-center border-r bg-card py-2">
        <Button
          variant={open ? 'secondary' : 'ghost'} size="icon-sm" aria-pressed={open}
          aria-label="Lista de arquivos" title="Lista de arquivos"
          onClick={() => setOpen(!open)}
        >
          <Files />
        </Button>
      </div>
      {open && (
        <aside style={{ width }} className="relative flex min-h-0 shrink-0 flex-col border-r bg-card">
          <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden py-1">
            {error && <p role="status" className="px-3 py-1 text-xs text-muted-foreground">{error}</p>}
            {!tree.children[''] && !error && <p className="px-3 py-1 text-xs text-muted-foreground">Lendo a pasta…</p>}
            {rows('', 0)}
          </div>
          <div
            role="separator" aria-orientation="vertical" aria-label="Largura da lista de arquivos"
            aria-valuemin={WIDTH.min} aria-valuemax={WIDTH.max} aria-valuenow={width}
            tabIndex={0} title="Arraste para mudar a largura"
            onPointerDown={startResize} onPointerMove={moveResize} onPointerUp={endResize} onPointerCancel={endResize}
            onKeyDown={keyResize}
            className="absolute inset-y-0 -right-1 z-10 w-2 cursor-col-resize outline-none hover:bg-ring/30 focus-visible:bg-ring/50"
          />
        </aside>
      )}
    </>
  );
}

/** Barra de status fina embaixo da tela principal: só a branch atual do projeto. */
export function StatusBar() {
  const [branch, setBranch] = useState<string | null>(null);
  usePolling(async () => {
    try {
      setBranch(await backend.gitBranch());
    } catch {
      setBranch(null);
    }
  });
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
