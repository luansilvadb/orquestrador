// Ctrl+clique em URL e caminho de arquivo nos terminais, como no VS Code: com Ctrl, o link sob o mouse ganha sublinhado e mão; o clique abre
// a URL no navegador e o arquivo no editor (na linha, se houver). Vale também para os hiperlinks OSC 8 (`linkHandler`).
import type { IBufferRange, IDisposable, ILink, Terminal } from '@xterm/xterm';
import { backend } from '../backend/backend';
import { findTermLinks, resolveTermPath } from './term-link-parse';

export type PathKind = 'file' | 'dir';
/** O que abre os links de um projeto: registrado pelo app (ver App.tsx); `root` resolve os relativos. */
export type LinkTarget = {
  root: string | null;
  openFile(path: string, line?: number, col?: number): void;
  revealDir(path: string): void;
};

const targets = new Map<string | null, LinkTarget>();
export function setLinkTarget(project: string | null, target: LinkTarget) {
  targets.set(project, target);
  return () => { if (targets.get(project) === target) targets.delete(project); };
}

// Existência por caminho absoluto, guardada por pouco tempo: o hover pede a mesma linha várias vezes.
const TTL = 5000;
const cache = new Map<string, { at: number; kind: Promise<PathKind | null> }>();
function pathKind(abs: string): Promise<PathKind | null> {
  const key = abs.toLowerCase();
  const hit = cache.get(key);
  if (hit && Date.now() - hit.at < TTL) return hit.kind;
  const kind = backend.pathKinds([abs]).then(([k]) => k ?? null, () => null);
  cache.set(key, { at: Date.now(), kind });
  if (cache.size > 500) for (const [k, v] of cache) if (Date.now() - v.at >= TTL) cache.delete(k);
  return kind;
}

let ctrl = false;
const views = new Set<() => void>();
function setCtrl(next: boolean) {
  if (next === ctrl) return;
  ctrl = next;
  for (const refresh of views) refresh();
}
if (typeof window !== 'undefined') {
  window.addEventListener('keydown', (e) => setCtrl(e.ctrlKey), true);
  window.addEventListener('keyup', (e) => setCtrl(e.ctrlKey), true);
  window.addEventListener('mousemove', (e) => setCtrl(e.ctrlKey), true);
  window.addEventListener('blur', () => setCtrl(false));
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
export function attachTermLinks(term: Terminal, host: HTMLElement, project: string | null): IDisposable {
  let hovered: { open: Open; link?: ILink } | null = null;
  const deco = () => ({ underline: ctrl, pointerCursor: ctrl });
  // O xterm acompanha os campos de `decorations` (troca o objeto por um com setters): muda os campos, não o objeto.
  const paint = (link: ILink) => {
    if (!link.decorations) return;
    link.decorations.underline = ctrl;
    link.decorations.pointerCursor = ctrl;
  };
  const refresh = () => { if (hovered?.link) paint(hovered.link); };
  views.add(refresh);

  const openPath = (abs: string, kind: PathKind, line?: number, col?: number) => {
    const target = targets.get(project);
    if (!target) return;
    const root = target.root ? resolveTermPath('', target.root) : null;
    const inside = root && abs.toLowerCase().startsWith(`${root.toLowerCase()}/`) ? abs.slice(root.length + 1) : null;
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
      const root = targets.get(project)?.root ?? null;
      void Promise.all(matches.map(async (m): Promise<ILink | null> => {
        let open: Open;
        if (m.kind === 'url') open = () => openUrl(m.url);
        else {
          const abs = resolveTermPath(m.path, root);
          const kind = abs ? await pathKind(abs) : null;
          if (!abs || !kind) return null;
          open = () => openPath(abs, kind, m.line, m.col);
        }
        const a = cells[m.start], b = cells[m.end - 1];
        if (!a || !b) return null;
        const range: IBufferRange = { start: { x: a.x, y: a.y }, end: { x: b.x + b.w - 1, y: b.y } };
        const link: ILink = {
          range, text: text.slice(m.start, m.end), decorations: deco(),
          activate: () => {},
          hover: () => { hovered = { open, link }; paint(link); },
          leave: () => { if (hovered?.link === link) hovered = null; },
        };
        return link;
      })).then((links) => {
        const found = links.filter((l): l is ILink => !!l);
        callback(found.length ? found.filter((l) => l.range.start.y <= y && l.range.end.y >= y) : undefined);
      });
    },
  });

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
  const linkOpen = (uri: string): Open | null => {
    if (/^https?:\/\//i.test(uri)) return () => openUrl(uri);
    const m = /^file:\/\/(?:localhost)?\/?([^?#]*)/i.exec(uri);
    if (!m) return null;
    const abs = resolveTermPath(decodeURIComponent(m[1]), null);
    return abs ? () => void pathKind(abs).then((kind) => { if (kind) openPath(abs, kind); }) : null;
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
      views.delete(refresh);
      provider.dispose();
      host.removeEventListener('mousedown', onDown, true);
    },
  };
}
