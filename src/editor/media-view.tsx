// Aba de mídia do editor central: imagem (png, jpg, gif, webp, bmp) e áudio (mp3, wav, ogg, flac, m4a). Os bytes vêm de `read_media`
// e entram num blob local (nada de asset protocol nem HTML). Mídia não lê texto, não cria rascunho e não grava: a aba é só de leitura.
// O áudio não toca sozinho e para ao sair da vista (troca de aba, grupo oculto, app em segundo plano) e ao desmontar.
// A lista de extensões espelha `MEDIA_EXTS` em `files.rs`: o backend recusa o que não estiver nela.
import { useEffect, useRef, useState } from 'react';
import { Button, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from '../backend/backend';
import { fileName } from '../shared/file-tabs';

export type MediaKind = 'image' | 'audio';
const TYPES: Record<string, { kind: MediaKind; mime: string }> = {
  png: { kind: 'image', mime: 'image/png' },
  jpg: { kind: 'image', mime: 'image/jpeg' },
  jpeg: { kind: 'image', mime: 'image/jpeg' },
  gif: { kind: 'image', mime: 'image/gif' },
  webp: { kind: 'image', mime: 'image/webp' },
  bmp: { kind: 'image', mime: 'image/bmp' },
  mp3: { kind: 'audio', mime: 'audio/mpeg' },
  wav: { kind: 'audio', mime: 'audio/wav' },
  ogg: { kind: 'audio', mime: 'audio/ogg' },
  oga: { kind: 'audio', mime: 'audio/ogg' },
  flac: { kind: 'audio', mime: 'audio/flac' },
  m4a: { kind: 'audio', mime: 'audio/mp4' },
};

/** Tipo da mídia pelo nome do arquivo; null se não é mídia (texto e binário desconhecido seguem o caminho de sempre). */
export function mediaOf(path: string): { kind: MediaKind; mime: string } | null {
  const name = fileName(path);
  const dot = name.lastIndexOf('.');
  if (dot <= 0) return null;
  return TYPES[name.slice(dot + 1).toLowerCase()] ?? null;
}

type Source = { url: string } | { error: string } | null;

export function MediaView({ path, shown }: { path: string; shown: boolean }) {
  const media = mediaOf(path);
  const [source, setSource] = useState<Source>(null);
  // Uma leitura por caminho: a troca de caminho (renomear) descarta o blob anterior, e a resposta tardia de um caminho velho não entra.
  useEffect(() => {
    if (!media) return;
    let live = true;
    let url: string | null = null;
    setSource(null);
    backend.readMedia(path).then(
      (bytes) => {
        if (!live) return;
        url = URL.createObjectURL(new Blob([bytes], { type: media.mime }));
        setSource({ url });
      },
      (e) => { if (live) setSource({ error: message(e) }); },
    );
    return () => {
      live = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [path]);

  if (!media) return null;
  if (!source) return <Note text="Lendo o arquivo…" />;
  if ('error' in source) return <Note text={source.error} danger />;
  return media.kind === 'image'
    ? <ImageBody url={source.url} name={fileName(path)} />
    : <AudioBody url={source.url} name={fileName(path)} shown={shown} />;
}

// Aviso no lugar do corpo da aba (lendo, erro, sem conteúdo). Também usado pelas abas do editor central (src/editor/editor-workspace.tsx).
export function Note({ text, danger, onRetry }: { text: string; danger?: boolean; onRetry?: () => void }) {
  return (
    <Flexbox padding={16} gap={8}>
      <Text as="p" role="status" fontSize={14} type={danger ? 'danger' : 'secondary'}>{text}</Text>
      {onRetry && <Button size="small" onClick={onRetry} className="self-start">Tentar de novo</Button>}
    </Flexbox>
  );
}

// Ajustar (padrão): cabe no painel sem cortar. Tamanho real: pixels naturais, com rolagem quando passa do painel.
function ImageBody({ url, name }: { url: string; name: string }) {
  const [natural, setNatural] = useState(false);
  const [size, setSize] = useState<{ w: number; h: number } | null>(null);
  const [failed, setFailed] = useState(false);
  return (
    <Flexbox flex={1} className="min-h-0">
      <Flexbox horizontal align="center" gap={8} paddingInline={12} height={32} flex="none" className="border-b">
        <Text as="span" role="status" fontSize={12} type="secondary" ellipsis className="min-w-0">
          {size ? `${size.w} × ${size.h} px` : 'Imagem'}
        </Text>
        <span className="flex-1" />
        <Button size="small" aria-pressed={natural} onClick={() => setNatural((n) => !n)}>
          {natural ? 'Ajustar' : 'Tamanho real'}
        </Button>
      </Flexbox>
      {failed ? (
        <Note text="Não foi possível exibir esta imagem." danger />
      ) : natural ? (
        <div className="flex min-h-0 flex-1 overflow-auto">
          <div className="flex min-h-full min-w-full">
            <img src={url} alt={name} draggable={false} onLoad={(e) => setSize({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })} onError={() => setFailed(true)} className="m-auto max-w-none" />
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center overflow-hidden p-4">
          <img src={url} alt={name} draggable={false} onLoad={(e) => setSize({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })} onError={() => setFailed(true)} className="max-h-full max-w-full object-contain" />
        </div>
      )}
    </Flexbox>
  );
}

// Controles nativos do WebView2 (tocar, pausar, posição, volume), sem pacote novo. Não toca sozinho (sem `autoplay`).
function AudioBody({ url, name, shown }: { url: string; name: string; shown: boolean }) {
  const audio = useRef<HTMLAudioElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);

  // Fora da vista, para: troca de aba (`shown`), grupo oculto ou outra tela (a interseção cai a zero), app em segundo plano.
  useEffect(() => {
    if (!shown) audio.current?.pause();
  }, [shown]);
  useEffect(() => {
    const el = audio.current;
    const frame = box.current;
    if (!el || !frame) return;
    const observer = new IntersectionObserver(([entry]) => { if (!entry.isIntersecting) el.pause(); });
    observer.observe(frame);
    const hidden = () => { if (document.hidden) el.pause(); };
    document.addEventListener('visibilitychange', hidden);
    return () => {
      observer.disconnect();
      document.removeEventListener('visibilitychange', hidden);
      el.pause();
    };
  }, []);

  return (
    <Flexbox ref={box} flex={1} align="center" justify="center" gap={16} padding={24} className="min-h-0">
      <Text as="p" fontSize={14} ellipsis className="max-w-full">{name}</Text>
      <audio
        ref={audio} src={url} controls preload="metadata" aria-label={name} style={{ width: '100%', maxWidth: 560 }}
        onPlay={() => setError(null)}
        onError={() => setError('Não foi possível tocar este áudio. O formato pode não ser suportado pelo Windows.')}
      />
      {error && <Text as="p" role="alert" fontSize={12} type="danger">{error}</Text>}
    </Flexbox>
  );
}
