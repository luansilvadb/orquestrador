// Acha URLs e caminhos de arquivo numa linha lógica do terminal. Puro, para o check (tests/check-term-links.mjs).

export type TermLinkMatch =
  | { kind: 'url'; start: number; end: number; url: string }
  | { kind: 'path'; start: number; end: number; path: string; line?: number; col?: number };

const URL_RE = /\bhttps?:\/\/[^\s<>"'`]+/g;
// Caminho: unidade (`D:\`, `D:/`) ou `./`/`../` opcional, segmentos separados por `\` ou `/`, e o sufixo `:l`, `:l:c`, `:l-l2` ou `(l,c)`.
// O lookbehind impede começar no meio de outro token (`a:b/c`, `x/y` dentro de `/x/y`).
const SEG = String.raw`[\w.\-@+~$%#]+`;
const PATH_RE = new RegExp(
  String.raw`(?<![\w.\-@+~$%#\\/:])((?:[A-Za-z]:[\\/]|\.{1,2}[\\/])?(?:${SEG}[\\/])*${SEG})(?::(\d+)(?::(\d+)|-\d+)?|\((\d+)(?:,\s?(\d+))?\))?`,
  'g',
);
const TRAIL = /[.,;:!?'"`\])}>]+$/;

/** URL sem a pontuação final; `)` fica se fecha um `(` da própria URL. */
function trimUrl(url: string) {
  for (;;) {
    const m = TRAIL.exec(url);
    if (!m) return url;
    const last = url[url.length - 1];
    if (last === ')' && (url.match(/\(/g)?.length ?? 0) >= (url.match(/\)/g)?.length ?? 0)) return url;
    url = url.slice(0, -1);
  }
}

export function findTermLinks(text: string): TermLinkMatch[] {
  const out: TermLinkMatch[] = [];
  for (const m of text.matchAll(URL_RE)) {
    const url = trimUrl(m[0]);
    if (url.length > 'https://'.length) out.push({ kind: 'url', start: m.index, end: m.index + url.length, url });
  }
  const inUrl = (i: number) => out.some((l) => l.kind === 'url' && i >= l.start && i < l.end);
  for (const m of text.matchAll(PATH_RE)) {
    if (inUrl(m.index)) continue;
    let path = m[1];
    const suffix = m[0].slice(path.length);
    // Ponto, hífen e afins no fim são pontuação da frase, não do nome (`veja src/a.ts.`).
    if (!suffix) path = path.replace(/[.\-]+$/, '');
    // Sem separador, só nome com extensão (`README.md`); exige letra na extensão para não pegar `1.5`.
    if (!/[\\/]/.test(path) && !/\.[A-Za-z]\w{0,9}$/.test(path)) continue;
    if (/^\.{1,2}[\\/]?$/.test(path)) continue;
    const line = m[2] ?? m[4], col = m[3] ?? m[5];
    out.push({
      kind: 'path', start: m.index, end: m.index + (suffix ? m[0].length : path.length), path,
      ...(line ? { line: Number(line) } : {}), ...(col ? { col: Number(col) } : {}),
    });
  }
  return out.sort((a, b) => a.start - b.start);
}

/** Absoluto com `/`, ou relativo a `root` normalizado (`.` e `..` resolvidos). `null` se relativo sem `root` ou se sai acima da unidade. */
export function resolveTermPath(path: string, root: string | null): string | null {
  const p = path.replace(/\\/g, '/');
  const abs = /^[A-Za-z]:\//.test(p) ? p : root ? `${root.replace(/\\/g, '/').replace(/\/+$/, '')}/${p}` : null;
  if (!abs) return null;
  const [drive, ...rest] = abs.split('/');
  const parts: string[] = [];
  for (const part of rest) {
    if (!part || part === '.') continue;
    if (part === '..') {
      if (!parts.length) return null;
      parts.pop();
    } else parts.push(part);
  }
  return [drive, ...parts].join('/');
}
