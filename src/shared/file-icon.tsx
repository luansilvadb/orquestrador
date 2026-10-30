// Ícone decorativo do Material Icon Theme.
import { iconFile, iconKey } from './file-icons';

// URLs dos SVGs, pela posição relativa a icons/ ("typescript.svg"). O Vite embute os pequenos como data URI; os outros carregam quando aparecem.
const URLS = Object.fromEntries(
  Object.entries(import.meta.glob<string>('../assets/material-icons/icons/*.svg', { query: '?url', import: 'default', eager: true }))
    .map(([path, url]) => [path.replace('../assets/material-icons/icons/', ''), url]),
);

export function FileIcon({ path, dir = false, expanded = false, className = 'size-4' }: { path: string; dir?: boolean; expanded?: boolean; className?: string }) {
  const key = iconKey(path, dir, expanded);
  return <img src={URLS[iconFile(key)]} data-icon={key} alt="" aria-hidden="true" draggable={false} className={`shrink-0 ${className}`} />;
}
