// Abertura rápida (Ctrl+P): um campo e a lista de arquivos do projeto que casam com o termo (backend.quickOpen, só rastreados e não ignorados).
// Setas escolhem, Enter abre como prévia, Shift+Enter abre de vez e Ctrl+Enter abre à direita. Esc fecha. Não é uma paleta de comandos.
import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { Input, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from '../backend/backend';
import { FileIcon } from '../shared/file-icon';
import { splitPath } from '../shared/paths';
import type { QuickOpen as QuickOpenResult } from '../backend/types';

// Espera a digitação parar antes de consultar; a resposta velha não vence a nova (cada consulta tem um número).
const DELAY_MS = 120;

export type QuickOpenChoice = { permanent: boolean; side: boolean };

export function QuickOpen({ onClose, onOpen }: {
  onClose: () => void;
  onOpen: (path: string, choice: QuickOpenChoice) => void;
}) {
  const [term, setTerm] = useState('');
  const [result, setResult] = useState<QuickOpenResult | null>(null);
  const [error, setError] = useState('');
  const [index, setIndex] = useState(0);
  const seq = useRef(0);
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const current = ++seq.current;
    const text = term.trim();
    if (text === '') {
      setResult(null);
      setError('');
      return;
    }
    const timer = window.setTimeout(() => {
      backend.quickOpen(text).then(
        (found) => {
          if (current !== seq.current) return;
          setResult(found);
          setError('');
          setIndex(0);
        },
        (e) => {
          if (current !== seq.current) return;
          setResult(null);
          setError(message(e));
        },
      );
    }, DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [term]);

  // O item escolhido aparece na lista rolada: sem isso, a seta para baixo some do campo visível.
  useEffect(() => {
    list.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [index, result]);

  const paths = result?.paths ?? [];
  const open = (at: number, choice: QuickOpenChoice) => {
    const path = paths[at];
    if (path) onOpen(path, choice);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (paths.length === 0) return;
      const step = e.key === 'ArrowDown' ? 1 : -1;
      setIndex((at) => (at + step + paths.length) % paths.length);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (e.ctrlKey) open(index, { permanent: true, side: true });
      else if (e.shiftKey) open(index, { permanent: true, side: false });
      else open(index, { permanent: false, side: false });
    }
  };

  return (
    <div className="fixed inset-0 z-[1000] flex items-start justify-center bg-black/30 px-4 pt-[12vh]" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <Flexbox role="dialog" aria-label="Abrir arquivo rápido" gap={8} className="w-full max-w-xl rounded-lg border bg-card p-3 shadow-lg" style={{ maxHeight: '70vh' }}>
        <Input
          autoFocus size="large" value={term} placeholder="Nome ou caminho do arquivo" aria-label="Abrir arquivo rápido"
          aria-controls="quick-open-list" aria-activedescendant={paths.length ? `quick-open-${index}` : undefined}
          onChange={(e) => setTerm(e.target.value)} onKeyDown={onKeyDown}
        />
        <Text as="p" role="status" fontSize={12} type="secondary" style={{ margin: 0 }}>
          {error
            ? <span className="text-destructive">{error}</span>
            : term.trim() === ''
              ? 'Digite parte do nome ou do caminho. Enter abre como prévia; Shift+Enter, de vez; Ctrl+Enter, à direita.'
              : result === null
                ? 'Procurando…'
                : result.total === 0
                  ? 'Nenhum arquivo casa com o termo.'
                  : `${result.total} ${result.total === 1 ? 'arquivo' : 'arquivos'}${result.total > paths.length ? `, mostrando ${paths.length}` : ''}.`}
        </Text>
        <div id="quick-open-list" role="listbox" aria-label="Arquivos" ref={list} className="min-h-0 overflow-y-auto">
          {paths.map((path, at) => {
            const { name, dir: folder } = splitPath(path);
            const selected = at === index;
            return (
              <div
                key={path} id={`quick-open-${at}`} role="option" aria-selected={selected} title={path}
                className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1 text-sm ${selected ? 'bg-muted' : ''}`}
                onMouseEnter={() => setIndex(at)}
                onClick={() => open(at, { permanent: false, side: false })}
                onDoubleClick={() => open(at, { permanent: true, side: false })}
              >
                <FileIcon path={path} />
                <span className="shrink-0">{name}</span>
                {folder && <span className="min-w-0 truncate text-xs text-muted-foreground">{folder}</span>}
              </div>
            );
          })}
        </div>
      </Flexbox>
    </div>
  );
}
