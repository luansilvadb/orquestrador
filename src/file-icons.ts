// Qual ícone do tema Symbols (src/assets/symbols, gerado por scripts/sync-file-icons.mjs) cabe a um arquivo ou pasta.
// A ordem é a do VS Code: nome do arquivo, extensão (da mais longa para a mais curta, "d.ts" antes de "ts"), linguagem pelo
// nome, linguagem pela extensão e o padrão. Pasta: pelo nome, ou a pasta padrão. Tudo sem diferenciar maiúsculas.
import theme from './assets/symbols/theme.json' with { type: 'json' };

type Map = Record<string, string>;
const { fileNames, fileExtensions, langNames, langExtensions, folderNames } = theme as unknown as Record<string, Map>;

// "a.b.c" -> ["b.c", "c"]. Arquivo que começa com ponto (".gitignore") não tem o nome como extensão.
function extensionsOf(name: string) {
  const parts = name.split('.');
  return parts.slice(name.startsWith('.') ? 2 : 1).map((_, i, rest) => rest.slice(i).join('.'));
}

/** A chave do ícone (ex.: "ts", "folder-github") do último segmento do caminho. */
export function iconKey(path: string, dir: boolean): string {
  const name = (path.split('/').pop() ?? path).toLowerCase();
  if (dir) return folderNames[name] ?? theme.folder;
  const exts = extensionsOf(name);
  return fileNames[name]
    ?? exts.map((e) => fileExtensions[e]).find(Boolean)
    ?? langNames[name]
    ?? exts.map((e) => langExtensions[e]).find(Boolean)
    ?? theme.file;
}

/** O SVG do ícone, relativo a src/assets/symbols/icons (ex.: "files/ts.svg"). */
export const iconFile = (key: string): string => (theme.icons as Map)[key];
