// Preview de Markdown (.md, .markdown, .mdx) com o texto atual do buffer. Carregado sob demanda (React.lazy) pelo editor-workspace.
// Imagem relativa: lida pelo `readMedia` (o mesmo da aba de mídia) e exibida por blob. Link http(s) abre no navegador; relativo abre no editor.
// `components` tem identidade estável por arquivo: sem isso, cada render remontaria as imagens e as leria de novo.
import { type ComponentProps, type MouseEvent, memo, useCallback, useDeferredValue, useLayoutEffect, useMemo, useRef } from 'react';
import { Markdown } from '@lobehub/ui';
import { useBackend } from '../backend/backend-context';
import { message } from '../backend/backend';
import { normalizeSegments } from '../shared/file-tab-actions';
import { splitPath } from '../shared/paths';
import { isWebUrl } from '../shared/urls';
import { useMediaBlob } from './media-view';

const isExternal = (url: string) => isWebUrl(url) || /^(data:|blob:|mailto:)/i.test(url);

/** Caminho relativo ao projeto a partir da pasta do .md; null se sair da raiz. */
function resolve(base: string, target: string): string | null {
  const dir = target.startsWith('/') ? [] : splitPath(base).dir.split('/');
  return normalizeSegments([...dir, ...decodeURI(target).split(/[?#]/)[0].split('/')])?.join('/') ?? null;
}

const slug = (text: string) => text.trim().toLowerCase().replace(/[^\p{L}\p{N}\s-]/gu, '').replace(/\s+/g, '-');

function LocalImage({ base, src, alt, ...rest }: ComponentProps<'img'> & { base: string }) {
  const raw = typeof src === 'string' ? src : '';
  const path = raw && !isExternal(raw) ? resolve(base, raw) : null;
  const source = useMediaBlob(path);
  if (!path) return <img src={raw} alt={alt} {...rest} />;
  return source && 'url' in source ? <img src={source.url} alt={alt} {...rest} /> : <span>{alt}</span>;
}

type Props = { path: string; text: string; onOpen: (path: string) => void };

export default memo(function MarkdownPreview({ path, text, onOpen }: Props) {
  const backend = useBackend();
  const box = useRef<HTMLDivElement>(null);
  const latest = useRef({ path, onOpen, backend });
  useLayoutEffect(() => { latest.current = { path, onOpen, backend }; });
  const markdown = useDeferredValue(text);
  const click = useCallback((e: MouseEvent, href: string | undefined) => {
    e.preventDefault();
    if (!href) return;
    const { path: base, onOpen: open, backend: api } = latest.current;
    if (href.startsWith('#')) {
      const want = decodeURIComponent(href.slice(1));
      const found = [...(box.current?.querySelectorAll('h1,h2,h3,h4,h5,h6') ?? [])].find((h) => h.id === want || slug(h.textContent ?? '') === want);
      found?.scrollIntoView({ block: 'start' });
    } else if (isWebUrl(href) || /^mailto:/i.test(href)) {
      void api.openUrl(href).catch((err: unknown) => console.warn(message(err)));
    } else {
      const target = resolve(base, href);
      if (target) open(target);
    }
  }, []);
  const components = useMemo(() => ({
    a: ({ href, children, node: _n, ...rest }: ComponentProps<'a'> & { node?: unknown }) => <a {...rest} href={href} onClick={(e) => click(e, href)}>{children}</a>,
    img: ({ node: _n, ...props }: ComponentProps<'img'> & { node?: unknown }) => <LocalImage base={path} {...props} />,
  }), [path, click]);
  return (
    <div ref={box} data-markdown-preview className="min-h-0 flex-1 overflow-y-auto bg-[var(--ant-color-bg-container)]">
      <div className="mx-auto max-w-[800px] px-8 py-6">
        <Markdown enableStream={false} animated={false} enableLatex={false} enableMermaid={false} fontSize={14} components={components}>
          {markdown}
        </Markdown>
      </div>
    </div>
  );
});
