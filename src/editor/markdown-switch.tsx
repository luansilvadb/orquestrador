// Seletor "Preview | Markdown" na linha do breadcrumb, só em arquivos Markdown.
import type { KeyboardEvent } from 'react';

export type MarkdownMode = 'preview' | 'source';
const MODES: { value: MarkdownMode; label: string }[] = [{ value: 'preview', label: 'Preview' }, { value: 'source', label: 'Markdown' }];

export const isMarkdown = (path: string) => /\.(md|markdown|mdx)$/i.test(path);

export function MarkdownSwitch({ value, onChange }: { value: MarkdownMode; onChange: (mode: MarkdownMode) => void }) {
  // Setas trocam o modo, como num radiogroup.
  const key = (e: KeyboardEvent) => {
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) return;
    e.preventDefault();
    const next = value === 'preview' ? 'source' : 'preview';
    onChange(next);
    (e.currentTarget.querySelector(`[data-mode="${next}"]`) as HTMLElement | null)?.focus();
  };
  return (
    <div role="radiogroup" aria-label="Modo do Markdown" className="flex h-[18px] items-center gap-0.5" onKeyDown={key}>
      {MODES.map((m) => {
        const on = m.value === value;
        return (
          <button
            key={m.value} type="button" role="radio" aria-checked={on} tabIndex={on ? 0 : -1} data-mode={m.value}
            onClick={() => onChange(m.value)}
            className={`h-full rounded-sm px-1.5 text-[12px] leading-none outline-none ${on ? 'bg-[var(--ant-color-fill-secondary)] text-[var(--ant-color-text)]' : 'text-[var(--ant-color-text-secondary)] hover:text-[var(--ant-color-text)]'}`}
          >
            {m.label}
          </button>
        );
      })}
    </div>
  );
}
