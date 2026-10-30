// Associações do Material Icon Theme geradas no ciclo de dev/build.
// A ordem é a do VS Code: nome do arquivo, extensão (da mais longa para a mais curta, "d.ts" antes de "ts"), linguagem pelo
// nome, linguagem pela extensão e o padrão. Pasta: pelo nome, ou a pasta padrão. Tudo sem diferenciar maiúsculas.
import theme from '../assets/material-icons/theme.json' with { type: 'json' };

type Map = Record<string, string>;
const { fileNames, fileExtensions, langNames, langExtensions, folderNames, folderNamesExpanded } = theme as unknown as Record<string, Map>;

// "a.b.c" -> ["b.c", "c"]. Arquivo que começa com ponto (".gitignore") não tem o nome como extensão.
function extensionsOf(name: string) {
  const parts = name.split('.');
  return parts.slice(name.startsWith('.') ? 2 : 1).map((_, i, rest) => rest.slice(i).join('.'));
}

/** A chave do ícone (ex.: "ts", "folder-github") do último segmento do caminho. */
export function iconKey(path: string, dir: boolean, expanded = false): string {
  const name = (path.split(/[\\/]/).pop() ?? path).toLowerCase();
  if (dir) return (expanded ? folderNamesExpanded[name] : folderNames[name]) ?? (expanded ? theme.folderExpanded : theme.folder);
  const exts = extensionsOf(name);
  return fileNames[name]
    ?? exts.map((e) => fileExtensions[e]).find(Boolean)
    ?? langNames[name]
    ?? exts.map((e) => langExtensions[e]).find(Boolean)
    ?? theme.file;
}

/** Nome do SVG gerado em src/assets/material-icons/icons. */
export const iconFile = (key: string): string => (theme.icons as Map)[key];
