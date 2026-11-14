// Preview de Markdown (.md, .markdown, .mdx) com o texto atual do buffer. Carregado sob demanda (React.lazy) pelo editor-workspace.
// Imagem relativa: lida pelo `readMedia` (o mesmo da aba de mídia) e exibida por blob. Link http(s) abre no navegador; relativo abre no editor.
import { type ComponentProps, type MouseEvent, useEffect, useRef, useState } from 'react';
import { Markdown } from '@lobehub/ui';
import { useBackend } from '../backend/backend-context';
import { message } from '../backend/backend';
import { splitPath } from '../shared/paths';
import { mediaOf } from './media-view';

const EXTERNAL = /^(https?:|data:|blob:|mailto:)/i;

/** Caminho relativo ao projeto a partir da pasta do .md; null se sair da raiz. */
function resolve(base: string, target: string): string | null {
  const parts = target.startsWith('/') ? [] : splitPath(base).dir.split('/').filter(Boolean);
  for (const seg of decodeURI(target).split(/[?#]/)[0].split('/')) {
    if (!seg || seg === '.') continue;
    if (seg === '..') { if (!parts.pop()) return null; } else parts.push(seg);
  }
  return parts.join('/');
}

const slug = (text: string) => text.trim().toLowerCase().replace(/[^\p{L}\p{N}\s-]/gu, '').replace(/\s+/g, '-');

function LocalImage({ base, src, alt, ...rest }: ComponentProps<'img'> & { base: string }) {
  const backend = useBackend();
  const raw = typeof src === 'string' ? src : '';
  const path = raw && !EXTERNAL.test(raw) ? resolve(base, raw) : null;
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    const media = path ? mediaOf(path) : null;
    if (!path || media?.kind !== 'image') return;
    let live = true;
    let made: string | null = null;
    backend.readMedia(path).then((bytes) => {
      if (!live) return;
      made = URL.createObjectURL(new Blob([bytes], { type: media.mime }));
      setUrl(made);
    }, (e) => console.warn(message(e)));
    return () => { live = false; if (made) URL.revokeObjectURL(made); };
  }, [path]);
  if (!path) return <img src={raw} alt={alt} {...rest} />;
  return url ? <img src={url} alt={alt} {...rest} /> : <span>{alt}</span>;
}

export default function MarkdownPreview({ path, text, onOpen }: { path: string; text: string; onOpen: (path: string) => void }) {
  const backend = useBackend();
  const box = useRef<HTMLDivElement>(null);
  const click = (e: MouseEvent, href: string | undefined) => {
    e.preventDefault();
    if (!href) return;
    if (href.startsWith('#')) {
      const want = decodeURIComponent(href.slice(1));
      const found = [...(box.current?.querySelectorAll('h1,h2,h3,h4,h5,h6') ?? [])].find((h) => h.id === want || slug(h.textContent ?? '') === want);
      found?.scrollIntoView({ block: 'start' });
    } else if (/^(https?:|mailto:)/i.test(href)) {
      void backend.openUrl(href).catch((err: unknown) => console.warn(message(err)));
    } else {
      const target = resolve(path, href);
      if (target) onOpen(target);
    }
  };
  return (
    <div ref={box} data-markdown-preview className="min-h-0 flex-1 overflow-y-auto bg-[var(--ant-color-bg-container)]">
      <div className="mx-auto max-w-[800px] px-8 py-6">
        <Markdown
          enableStream={false} animated={false} enableLatex={false} enableMermaid={false} fontSize={14}
          components={{
            a: ({ href, children, node: _n, ...rest }: ComponentProps<'a'> & { node?: unknown }) => <a {...rest} href={href} onClick={(e) => click(e, href)}>{children}</a>,
            img: ({ node: _n, ...props }: ComponentProps<'img'> & { node?: unknown }) => <LocalImage base={path} {...props} />,
          }}
        >
          {text}
        </Markdown>
      </div>
    </div>
  );
}
