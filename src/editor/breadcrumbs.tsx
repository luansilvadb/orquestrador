// O caminho do arquivo embaixo das abas, como os breadcrumbs do Antigravity/VS Code: 22 px, as pastas e o arquivo separados por `›`.
// Clicar num item abre o seletor com o conteúdo da pasta em que ele está (o item marcado). Pasta no seletor desce um nível; arquivo
// abre como prévia. Sem os símbolos do arquivo: o último item é o próprio arquivo.
import { useState } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { Popover, ScrollArea } from '@lobehub/ui/base-ui';
import { backend, message } from '../backend/backend';
import { FileIcon } from '../shared/file-icon';
import { join, splitPath } from '../shared/paths';
import type { DirEntry } from '../backend/types';

export function Breadcrumbs({ path, onOpen }: { path: string; onOpen: (path: string) => void }) {
  const parts = path.split('/').filter(Boolean);
  // Item com o seletor aberto (índice em `parts`) e a pasta que o seletor mostra agora.
  const [open, setOpen] = useState<number | null>(null);
  const [dir, setDir] = useState('');
  const [entries, setEntries] = useState<DirEntry[] | null>(null);
  const [error, setError] = useState('');

  const load = (folder: string) => {
    setDir(folder);
    setEntries(null);
    setError('');
    backend.listDir(folder).then(setEntries, (e) => setError(message(e)));
  };
  const show = (index: number, value: boolean) => {
    setOpen(value ? index : null);
    if (value) load(parts.slice(0, index).join('/'));
  };
  const marked = open === null ? '' : parts.slice(0, open + 1).join('/');

  const picker = (
    <div className="w-64">
      {/* Dentro de uma pasta, a linha de cima diz qual é e volta um nível. */}
      {dir && (
        <div className="flex h-[22px] items-center gap-1 border-b px-1.5 text-[12px] text-[var(--ant-color-text-secondary)]">
          <button
            type="button" aria-label="Pasta de cima" title="Pasta de cima" onClick={() => load(splitPath(dir).dir)}
            className="flex size-4 items-center justify-center rounded-sm outline-none hover:bg-[var(--ant-color-fill-tertiary)] focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <ChevronLeft className="size-3.5" aria-hidden />
          </button>
          <span className="truncate">{dir}</span>
        </div>
      )}
      <ScrollArea style={{ maxHeight: 300 }} disableContentFit>
        <div role="listbox" aria-label={`Conteúdo de ${dir || 'projeto'}`} className="py-0.5">
          {error && <p role="alert" className="px-2 py-1 text-[12px] text-[var(--ant-color-error)]">{error}</p>}
          {!entries && !error && <p className="px-2 py-1 text-[12px] text-[var(--ant-color-text-secondary)]">Lendo a pasta…</p>}
          {entries?.map((e) => {
            const full = join(dir, e.name);
            return (
              <button
                key={full} type="button" role="option" aria-selected={full === marked} title={full}
                onClick={() => {
                  if (e.dir) return load(full);
                  setOpen(null);
                  onOpen(full);
                }}
                className={`flex h-[22px] w-full items-center gap-1.5 px-2 text-left text-[13px] outline-none hover:bg-[var(--ant-color-fill-tertiary)] focus-visible:bg-[var(--ant-color-fill-tertiary)] ${full === marked ? 'bg-[var(--ant-color-fill-secondary)]' : ''} ${e.ignored ? 'text-[var(--ant-color-text-quaternary)]' : ''}`}
              >
                <FileIcon path={full} dir={e.dir} className="size-4" />
                <span className="min-w-0 flex-1 truncate">{e.name}</span>
                {e.dir && <ChevronRight className="size-3.5 shrink-0 text-[var(--ant-color-text-secondary)]" aria-hidden />}
              </button>
            );
          })}
        </div>
      </ScrollArea>
    </div>
  );

  return (
    <nav aria-label="Caminho do arquivo" className="flex h-[22px] flex-none items-center overflow-hidden bg-[var(--ant-color-bg-container)] pl-1 text-[13px] text-[var(--ant-color-text-secondary)]">
      {parts.map((name, i) => {
        const last = i === parts.length - 1;
        const full = parts.slice(0, i + 1).join('/');
        return (
          <span key={full} className={`flex h-full items-center ${last ? 'min-w-0' : 'shrink-0'}`}>
            <Popover
              trigger="click" placement="bottomLeft" open={open === i} onOpenChange={(v: boolean) => show(i, v)}
              content={picker} styles={{ content: { padding: 4 } }}
            >
              <button
                type="button" title={full} aria-haspopup="listbox" aria-expanded={open === i}
                className="flex h-full min-w-0 items-center gap-1.5 rounded-sm px-1 outline-none hover:text-[var(--ant-color-text)] focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                {last && <FileIcon path={full} className="size-4" />}
                <span className="truncate">{name}</span>
              </button>
            </Popover>
            {!last && <ChevronRight className="size-4 shrink-0" aria-hidden />}
          </span>
        );
      })}
    </nav>
  );
}
