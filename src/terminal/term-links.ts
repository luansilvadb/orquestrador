// Ctrl+clique em URL e caminho de arquivo nos terminais, como no VS Code: com Ctrl, o link sob o mouse ganha sublinhado e mão; o clique abre
// a URL no navegador e o arquivo no editor (na linha, se houver). Vale também para os hiperlinks OSC 8 (`linkHandler`).
import type { IBufferRange, IDisposable, ILink, ILinkDecorations, Terminal } from '@xterm/xterm';
import { createContext } from 'react';
import { backend } from '../backend/backend';
import { relativeTo } from '../shared/file-tab-actions';
import { isWebUrl } from '../shared/urls';
import { findTermLinks, resolveTermPath } from './term-link-parse';

export type PathKind = 'file' | 'dir';
/** O que abre os links de um projeto, lido na hora do uso: `root()` resolve os relativos. */
export type LinkTarget = {
  root(): string | null;
  openFile(path: string, line?: number, col?: number): void;
  revealDir(path: string): void;
};
/** Dado pelo app (ver App.tsx) aos TermHost do projeto. Sem ele, só URLs abrem. */
export const LinkTargetContext = createContext<LinkTarget | null>(null);

// Existência por caminho absoluto, guardada por pouco tempo: o hover pede a mesma linha várias vezes.
const TTL = 5000;
const cache = new Map<string, { at: number; kind: Promise<PathKind | null> }>();
/** Tipo de cada caminho, na ordem; os que não estão no cache vão ao backend numa chamada só. */
function pathKinds(paths: readonly string[]): Promise<(PathKind | null)[]> {
  const now = Date.now();
  const missing = new Map<string, string>();
  for (const abs of paths) {
    const key = abs.toLowerCase();
    const hit = cache.get(key);
    if (!(hit && now - hit.at < TTL)) missing.set(key, abs);
  }
  if (missing.size) {
    const keys = [...missing.keys()];
    const batch = backend.pathKinds([...missing.values()]).catch(() => keys.map(() => null));
    keys.forEach((key, i) => cache.set(key, { at: now, kind: batch.then((kinds) => kinds[i] ?? null) }));
    if (cache.size > 500) for (const [k, v] of cache) if (now - v.at >= TTL) cache.delete(k);
  }
  return Promise.all(paths.map((abs) => cache.get(abs.toLowerCase())!.kind));
}
const pathKind = (abs: string) => pathKinds([abs]).then(([kind]) => kind);

// Ctrl seguido na janela enquanto houver terminal com links: os ouvintes entram no primeiro e saem no último.
let ctrl = false;
const views = new Set<() => void>();
function setCtrl(next: boolean) {
  if (next === ctrl) return;
  ctrl = next;
  for (const refresh of views) refresh();
}
const onKey = (e: KeyboardEvent | MouseEvent) => setCtrl(e.ctrlKey);
const onBlur = () => setCtrl(false);
function watchCtrl(refresh: () => void) {
  if (!views.size) {
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('keyup', onKey, true);
    window.addEventListener('mousemove', onKey, true);
    window.addEventListener('blur', onBlur);
  }
  views.add(refresh);
  return () => {
    views.delete(refresh);
    if (views.size) return;
    window.removeEventListener('keydown', onKey, true);
    window.removeEventListener('keyup', onKey, true);
    window.removeEventListener('mousemove', onKey, true);
    window.removeEventListener('blur', onBlur);
    ctrl = false;
  };
}

type Open = () => void;

/** Texto da linha lógica que contém a linha `y` (1-based) e a posição (x, y, 1-based) de cada unidade UTF-16. */
function logicalLine(term: Terminal, y: number) {
  const buf = term.buffer.active;
  let first = y - 1;
  while (first > 0 && buf.getLine(first)?.isWrapped) first--;
  let last = y - 1;
  while (buf.getLine(last + 1)?.isWrapped) last++;
  let text = '';
  const cells: { x: number; y: number; w: number }[] = [];
  const cell = buf.getNullCell();
  for (let row = first; row <= last; row++) {
    const line = buf.getLine(row);
    if (!line) break;
    for (let x = 0; x < line.length; x++) {
      line.getCell(x, cell);
      const w = cell.getWidth();
      if (!w) continue;
      const chars = cell.getChars() || ' ';
      for (let i = 0; i < chars.length; i++) cells.push({ x: x + 1, y: row + 1, w });
      text += chars;
    }
  }
  return { text, cells };
}

/** Liga os links ao terminal. `host` recebe o mousedown antes do xterm: o Ctrl+clique abre e não chega à seleção nem ao PTY. */
export function attachTermLinks(term: Terminal, host: HTMLElement, target: LinkTarget | null): IDisposable {
  let hovered: { open: Open; link?: ILink } | null = null;
  // O xterm acompanha os campos de `decorations` (troca o objeto por um com setters): muda os campos, não o objeto.
  const paint = (d: ILinkDecorations = { underline: false, pointerCursor: false }) => Object.assign(d, { underline: ctrl, pointerCursor: ctrl });
  const unwatch = watchCtrl(() => { if (hovered?.link?.decorations) paint(hovered.link.decorations); });

  const openPath = (abs: string, kind: PathKind, line?: number, col?: number) => {
    if (!target) return;
    const inside = relativeTo(target.root() ?? '', abs);
    if (kind === 'dir') {
      if (inside) target.revealDir(inside);
    } else target.openFile(inside ?? abs, line, col);
  };
  const openUrl = (url: string) => void backend.openUrl(url).catch(() => {});

  const provider = term.registerLinkProvider({
    provideLinks(y, callback) {
      const { text, cells } = logicalLine(term, y);
      const matches = findTermLinks(text);
      if (!matches.length) return callback(undefined);
      const root = target?.root() ?? null;
      const abs = matches.map((m) => (m.kind === 'path' ? resolveTermPath(m.path, root) : null));
      void pathKinds(abs.filter((a): a is string => !!a)).then((kinds) => {
        let k = 0;
        const found: ILink[] = [];
        matches.forEach((m, i) => {
          let open: Open;
          if (m.kind === 'url') open = () => openUrl(m.url);
          else {
            const path = abs[i];
            if (!path) return;
            const kind = kinds[k++];
            if (!kind) return;
            open = () => openPath(path, kind, m.line, m.col);
          }
          const a = cells[m.start], b = cells[m.end - 1];
          if (!a || !b) return;
          const range: IBufferRange = { start: { x: a.x, y: a.y }, end: { x: b.x + b.w - 1, y: b.y } };
          if (range.start.y > y || range.end.y < y) return;
          const link: ILink = {
            range, text: text.slice(m.start, m.end), decorations: paint(),
            activate: () => {},
            hover: () => { hovered = { open, link }; if (link.decorations) paint(link.decorations); },
            leave: () => { if (hovered?.link === link) hovered = null; },
          };
          found.push(link);
        });
        callback(found.length ? found : undefined);
      });
    },
  });

  const linkOpen = (uri: string): Open | null => {
    if (isWebUrl(uri)) return () => openUrl(uri);
    const m = /^file:\/\/(?:localhost)?\/?([^?#]*)/i.exec(uri);
    if (!m) return null;
    const abs = resolveTermPath(decodeURIComponent(m[1]), null);
    return abs ? () => void pathKind(abs).then((kind) => { if (kind) openPath(abs, kind); }) : null;
  };
  // OSC 8: o xterm pinta o hover dele (sem controle de decoração); aqui só registra o alvo para o Ctrl+clique.
  term.options.linkHandler = {
    allowNonHttpProtocols: true,
    activate: () => {},
    hover: (_e, uri) => {
      const open = linkOpen(uri);
      hovered = open ? { open } : null;
    },
    leave: () => { if (hovered && !hovered.link) hovered = null; },
  };

  const onDown = (e: MouseEvent) => {
    if (!e.ctrlKey || e.button !== 0 || !hovered) return;
    e.preventDefault();
    e.stopPropagation();
    hovered.open();
  };
  host.addEventListener('mousedown', onDown, true);
  return {
    dispose() {
      unwatch();
      provider.dispose();
      host.removeEventListener('mousedown', onDown, true);
    },
  };
}
