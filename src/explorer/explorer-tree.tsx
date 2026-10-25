// A árvore do explorador: as linhas à vista (as pastas abertas, achatadas na ordem da tela), numa lista virtual do virtua, como a da
// pesquisa. Só as linhas na área visível (e a do foco) ficam montadas; teclado, seleção e rolagem andam pelo índice da linha, não pelo DOM.
// Pasta abre e fecha na hora, sem animação, como no VS Code. Teclado do Tree do lobehub, que ela substitui: setas, Home/End, * abre as
// pastas irmãs, Enter e espaço selecionam; o tabIndex é itinerante. O resto (menu, atalhos, arraste) fica no explorer.tsx.
import { type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode, type Ref, type RefObject, memo, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { ChevronRight, File as FileGlyph } from 'lucide-react';
import { Virtualizer, type VirtualizerHandle } from 'virtua';
import { join } from '../shared/paths';
import type { DirEntry } from '../backend/types';

// Medidas do Explorer do Antigravity: linha de 22 px e recuo de 8 px por nível.
const ROW_HEIGHT = 22;
const INDENT = 8;

/**
 * Linha da árvore. `file` e `dir` são itens reais; `wait` é o nó provisório ("Lendo a pasta…") de pasta aberta sem lista, e `new`, a linha
 * do nome do item novo. As chaves de `wait` e `new` terminam em "/": nenhum caminho real termina com barra.
 */
export type TreeRow = {
  key: string;
  depth: number;
  /** Pasta de cima ('' na raiz). */
  parent: string;
  kind: 'file' | 'dir' | 'wait' | 'new';
  entry: DirEntry | null;
  /** Tem seta: pasta ainda sem lista ou com algo dentro. Pasta vazia já lida fica sem seta, como no Tree. */
  expandable: boolean;
};
export type Mods = { ctrl: boolean; shift: boolean };

/**
 * Linhas à vista, na ordem da tela. `adding` põe a linha do nome novo na pasta dela (já lida): pasta nova no topo, arquivo novo depois das
 * pastas (a ordem do VS Code).
 */
export function treeRows(children: Record<string, DirEntry[]>, expanded: string[], adding: { dir: string; kind: 'file' | 'dir' } | null): TreeRow[] {
  const open = new Set(expanded);
  const rows: TreeRow[] = [];
  const walk = (dir: string, depth: number) => {
    const list = children[dir] ?? [];
    const at = adding?.dir === dir ? (adding.kind === 'dir' ? 0 : list.filter((e) => e.dir).length) : -1;
    for (let i = 0; i <= list.length; i++) {
      if (i === at) rows.push({ key: `${join(dir, '\0novo')}/`, depth, parent: dir, kind: 'new', entry: null, expandable: false });
      const entry = list[i];
      if (!entry) break;
      const key = join(dir, entry.name);
      if (!entry.dir) {
        rows.push({ key, depth, parent: dir, kind: 'file', entry, expandable: false });
        continue;
      }
      const inner = children[key];
      rows.push({ key, depth, parent: dir, kind: 'dir', entry, expandable: !inner || inner.length > 0 || adding?.dir === key });
      if (!open.has(key)) continue;
      if (inner) walk(key, depth + 1);
      else rows.push({ key: `${key}/`, depth: depth + 1, parent: key, kind: 'wait', entry: null, expandable: false });
    }
  };
  walk('', 0);
  return rows;
}

const real = (row: TreeRow) => row.kind === 'file' || row.kind === 'dir';

/**
 * Linha com a largura toda: seta de 16 px a 8 px da borda (o recuo do nível entra antes), ícone, nome. `--depth` dá as guias de recuo
 * (index.css); `frame` é a moldura do destino do arraste. Memoizada: o virtua renderiza a lista de novo ao medir as linhas novas, e
 * essas não mudam.
 */
const Row = memo(function Row({ row, isOpen, selected, active, draggable, frame, title, icon, onSelect, onPrefetch, onToggle, onRightClick, onFocus }: {
  row: TreeRow;
  isOpen: boolean;
  selected: boolean;
  active: boolean;
  draggable: boolean;
  frame: 'error' | 'primary' | null;
  title: (row: TreeRow) => ReactNode;
  icon: (row: TreeRow) => ReactNode;
  onSelect: (row: TreeRow, mods: Mods) => void;
  onPrefetch: (path: string) => void;
  onToggle: (row: TreeRow) => void;
  onRightClick: (event: ReactMouseEvent<HTMLElement>, row: TreeRow) => void;
  onFocus: (key: string) => void;
}) {
  const style = {
    paddingInlineStart: row.depth * INDENT,
    '--depth': row.depth,
    boxShadow: frame ? `inset 0 0 0 1px var(--ant-color-${frame})` : undefined,
  } as CSSProperties;
  return (
    <div
      role="treeitem" data-id={row.key} aria-level={row.depth + 1} aria-selected={selected}
      aria-expanded={row.expandable ? isOpen : undefined} tabIndex={active ? 0 : -1} draggable={draggable} style={style}
      className="relative flex h-[22px] cursor-pointer items-center pr-2 text-[var(--ant-color-text)] outline-none hover:bg-[var(--ant-color-fill-tertiary)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ant-color-info)] aria-selected:bg-[var(--ant-color-fill-secondary)]"
      onMouseDown={(e) => { if (row.kind === 'file' && e.button === 0 && !e.ctrlKey && !e.metaKey && !e.shiftKey) onPrefetch(row.key); }}
      onClick={(e) => onSelect(row, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey })}
      onContextMenu={(e) => onRightClick(e, row)}
      onFocus={() => onFocus(row.key)}
    >
      <button
        type="button" aria-hidden tabIndex={-1}
        className={`ml-2 grid h-[22px] w-4 flex-none cursor-pointer place-items-center rounded-[var(--ant-border-radius-sm)] border-0 bg-transparent p-0 text-[var(--ant-color-text-secondary)] hover:bg-[var(--ant-color-fill-secondary)] hover:text-[var(--ant-color-text)]${row.expandable ? '' : ' pointer-events-none invisible'}`}
        onClick={(e) => {
          e.stopPropagation();
          onToggle(row);
        }}
      >
        {row.expandable && <ChevronRight size={14} className={isOpen ? 'rotate-90' : undefined} />}
      </button>
      {icon(row) ?? <FileGlyph size={16} className="mr-1.5 flex-none text-[var(--ant-color-text-secondary)]" />}
      <span className="inline-flex min-w-0 flex-1 items-center gap-2 overflow-hidden rounded-[var(--ant-border-radius-sm)] py-0.5 text-ellipsis whitespace-nowrap">
        {title(row)}
      </span>
    </div>
  );
});

export type FileTreeHandle = {
  /** Foca a linha; fora da tela, a lista rola até ela e o foco vem quando ela monta. false se ela não está na lista. */
  focus: (key: string) => boolean;
  /** Rola até a linha (a mais perto possível). Se ela ainda não está na lista, espera alguns quadros por ela. */
  scrollTo: (key: string) => void;
};

/**
 * `selected` é a marcação exibida; `onSelect` recebe a seleção nova (Ctrl alterna, Shift pega o intervalo desde a âncora) com os
 * modificadores. `onToggle` abre ou fecha a pasta. `renaming` é a linha que não arrasta; `drop`, a pasta que recebe o arraste (moldura
 * vermelha se não pode). `title` e `icon` desenham o conteúdo da linha; o ícone vem com a margem dele (6 px) e, sem ele, a linha põe o de
 * arquivo genérico.
 */
export function FileTree({ ref, viewport, rows, expanded, selected, renaming, drop, title, icon, onSelect, onPrefetch, onToggle, onRightClick }: {
  ref: Ref<FileTreeHandle>;
  /** O elemento que rola (o viewport do ScrollArea). */
  viewport: RefObject<HTMLElement | null>;
  rows: TreeRow[];
  expanded: string[];
  selected: string[];
  renaming: string | null;
  drop: { dest: string; problem: string | null } | null;
  title: (row: TreeRow) => ReactNode;
  icon: (row: TreeRow) => ReactNode;
  onSelect: (keys: string[], row: TreeRow, mods: Mods) => void;
  onPrefetch: (path: string) => void;
  onToggle: (row: TreeRow) => void;
  onRightClick: (event: ReactMouseEvent<HTMLElement>, row: TreeRow) => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const virtualizer = useRef<VirtualizerHandle>(null);
  const indexOf = useMemo(() => new Map(rows.map((r, i) => [r.key, i])), [rows]);
  const open = useMemo(() => new Set(expanded), [expanded]);
  const marked = useMemo(() => new Set(selected), [selected]);
  // Linha do tabIndex 0: a última com foco, ou a primeira marcada, ou a primeira. Fica montada fora da tela, para o Tab achar a árvore.
  const [active, setActive] = useState<string | null>(null);
  const activeKey = active !== null && indexOf.has(active) ? active : selected.find((k) => indexOf.has(k)) ?? rows[0]?.key ?? null;
  const activeIndex = activeKey === null ? -1 : indexOf.get(activeKey)!;
  const anchor = useRef<string | null>(null);

  // Linha que a lista rola até mostrar e, com `focus`, que recebe o foco assim que estiver montada (o virtua a mostra só depois de medir).
  const pending = useRef<{ key: string; focus: boolean; tries: number } | null>(null);
  const [, setTick] = useState(0);
  const request = (key: string, focus: boolean) => {
    pending.current = { key, focus, tries: 0 };
    if (focus) setActive(key);
    setTick((n) => n + 1);
  };
  useEffect(() => {
    const p = pending.current;
    if (!p) return;
    const index = indexOf.get(p.key);
    if (index !== undefined) {
      virtualizer.current?.scrollToIndex(index, { align: 'nearest' });
      if (!p.focus) return void (pending.current = null);
      const el = [...(box.current?.querySelectorAll<HTMLElement>('[role="treeitem"]') ?? [])].find((r) => r.dataset.id === p.key);
      el?.focus({ preventScroll: true });
      if (el && document.activeElement === el) return void (pending.current = null);
    }
    if (p.tries++ < 30) requestAnimationFrame(() => setTick((n) => n + 1));
    else pending.current = null;
  });
  const rowsNow = useRef(indexOf);
  rowsNow.current = indexOf;
  useImperativeHandle(ref, () => ({
    focus: (key) => {
      if (!rowsNow.current.has(key)) return false;
      request(key, true);
      return true;
    },
    scrollTo: (key) => request(key, false),
  }), []);

  const select = (row: TreeRow, mods: Mods) => {
    if (!real(row)) return;
    let next: string[];
    const from = anchor.current === null ? undefined : indexOf.get(anchor.current);
    if (mods.shift && from !== undefined) {
      const to = indexOf.get(row.key)!;
      next = rows.slice(Math.min(from, to), Math.max(from, to) + 1).filter(real).map((r) => r.key);
    } else if (mods.ctrl) {
      next = marked.has(row.key) ? selected.filter((k) => k !== row.key) : [...selected, row.key];
      anchor.current = row.key;
    } else {
      next = [row.key];
      anchor.current = row.key;
    }
    setActive(row.key);
    onSelect(next, row, mods);
  };
  // As linhas são memoizadas: os eventos delas leem as funções da hora do evento, e só a linha que mudou redesenha.
  const latest = useRef({ select, onPrefetch, onToggle, onRightClick });
  useLayoutEffect(() => {
    latest.current = { select, onPrefetch, onToggle, onRightClick };
  });
  const events = useMemo(() => ({
    select: (row: TreeRow, mods: Mods) => latest.current.select(row, mods),
    prefetch: (path: string) => latest.current.onPrefetch(path),
    toggle: (row: TreeRow) => latest.current.onToggle(row),
    menu: (event: ReactMouseEvent<HTMLElement>, row: TreeRow) => latest.current.onRightClick(event, row),
  }), []);

  // As teclas não saem daqui: o explorer.tsx ainda vê Enter (abre fixo), F2, Delete e os atalhos. Campo de texto na linha fica de fora.
  const onKeyDown = (e: ReactKeyboardEvent<HTMLElement>) => {
    const target = e.target as HTMLElement;
    if (e.ctrlKey || e.metaKey || e.altKey || target.isContentEditable || ['INPUT', 'SELECT', 'TEXTAREA'].includes(target.tagName)) return;
    const at = indexOf.get(target.closest<HTMLElement>('[role="treeitem"]')?.dataset.id ?? '') ?? -1;
    const row = rows[at] ?? rows[activeIndex];
    const last = rows.length - 1;
    const go = (index: number) => rows[index] && request(rows[index].key, true);
    const keys: Record<string, () => void> = {
      // Setas dão a volta na lista, como no Tree.
      ArrowDown: () => go(at < 0 ? 0 : (at + 1) % rows.length),
      ArrowUp: () => go(at < 0 ? last : (at - 1 + rows.length) % rows.length),
      Home: () => go(0),
      End: () => go(last),
      ' ': () => row && select(row, { ctrl: false, shift: e.shiftKey }),
      Enter: () => row && select(row, { ctrl: false, shift: e.shiftKey }),
      '*': () => row && rows.filter((r) => r.parent === row.parent && r.expandable && !open.has(r.key)).forEach(onToggle),
      ArrowLeft: () => {
        if (!row) return;
        if (row.expandable && open.has(row.key)) onToggle(row);
        else if (row.parent) request(row.parent, true);
      },
      ArrowRight: () => {
        if (!row?.expandable) return;
        if (open.has(row.key)) go(indexOf.get(row.key)! + 1);
        else onToggle(row);
      },
    };
    const run = keys[e.key];
    if (!run || !rows.length) return;
    e.preventDefault();
    run();
  };

  const renderRow = (row: TreeRow) => (
    <Row
      key={row.key} row={row} isOpen={row.expandable && open.has(row.key)} selected={marked.has(row.key)} active={row.key === activeKey}
      draggable={real(row) && renaming !== row.key} frame={!!drop && real(row) && drop.dest === row.key ? (drop.problem ? 'error' : 'primary') : null}
      title={title} icon={icon} onSelect={events.select} onPrefetch={events.prefetch} onToggle={events.toggle} onRightClick={events.menu} onFocus={setActive}
    />
  );

  return (
    <div ref={box} role="tree" tabIndex={-1} className="flex flex-col py-0.5 outline-none select-none" onKeyDown={onKeyDown}>
      <Virtualizer
        ref={virtualizer} scrollRef={viewport} data={rows} itemSize={ROW_HEIGHT} startMargin={2}
        keepMounted={activeIndex >= 0 ? [activeIndex] : undefined}
      >
        {renderRow}
      </Virtualizer>
    </div>
  );
}
