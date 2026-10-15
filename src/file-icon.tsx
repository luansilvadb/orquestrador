// O ícone do tema Symbols de um arquivo ou pasta, como no Explorer do Antigravity. Decorativo: o nome ao lado já diz o que é.
import { iconFile, iconKey } from './file-icons';

// URLs dos SVGs, pela posição relativa a icons/ ("files/ts.svg"). O Vite embute os pequenos como data URI; os outros carregam quando aparecem.
const URLS = Object.fromEntries(
  Object.entries(import.meta.glob<string>('./assets/symbols/icons/**/*.svg', { query: '?url', import: 'default', eager: true }))
    .map(([path, url]) => [path.replace('./assets/symbols/icons/', ''), url]),
);

export function FileIcon({ path, dir = false, className = 'size-4' }: { path: string; dir?: boolean; className?: string }) {
  const key = iconKey(path, dir);
  return <img src={URLS[iconFile(key)]} data-icon={key} alt="" aria-hidden="true" draggable={false} className={`shrink-0 ${className}`} />;
}
