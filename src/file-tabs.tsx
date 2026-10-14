// Faixa de abas dos arquivos abertos, no topo da área central: uma aba por arquivo, com o nome, o ● de alteração não salva e um X.
// Só a faixa rola na horizontal (roda e setas do teclado); a aba ativa é revelada, e uma lista atende quando não cabem todas.
// Clique direito (ou Shift+F10 e a tecla de contexto) abre o menu da aba. Ação sem alvo fica desabilitada, com o motivo escrito.
import { useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { Check, File, List, Pin, X } from 'lucide-react';
import { ActionIcon, Button, ContextMenuTrigger, type ContextMenuItem, Popover, TabsIndicator, TabsList, TabsRoot, TabsTab, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { batchTargets, type BatchKind, type CopyKind } from './file-tab-actions';

export const fileName = (path: string) => path.slice(path.lastIndexOf('/') + 1);
const folderOf = (path: string) => path.slice(0, Math.max(path.lastIndexOf('/'), 0)) || 'raiz';

// Mesmo nome em pastas diferentes: a pasta vira o detalhe da aba, para as duas não ficarem iguais. Sem repetição, não há detalhe.
function detailsOf(paths: string[]) {
  const count = new Map<string, number>();
  for (const path of paths) count.set(fileName(path), (count.get(fileName(path)) ?? 0) + 1);
  return new Map(paths.map((path) => [path, (count.get(fileName(path)) ?? 0) > 1 ? folderOf(path) : '']));
}

// Mantém a aba dentro da área visível da faixa, mexendo só no scroll dela (não no scrollIntoView, que rola os ancestrais).
function reveal(box: HTMLElement, path: string) {
  const tab = [...box.querySelectorAll<HTMLElement>('[data-file]')].find((el) => el.dataset.file === path);
  if (!tab || box.clientWidth === 0) return;
  // Medidas com subpixel, sem arredondar: arredondar para cima passava do início da aba em até 1 px.
  const edge = box.getBoundingClientRect();
  const rect = tab.getBoundingClientRect();
  const left = rect.left - edge.left + box.scrollLeft;
  const right = left + rect.width;
  const visible = box.clientWidth;
  // Aba que ocupa a faixa inteira (ou maior): mostra o começo dela (o nome), não o fim.
  if (left < box.scrollLeft || rect.width >= visible) box.scrollLeft = left;
  else if (right > box.scrollLeft + visible) box.scrollLeft = right - visible;
}

// Item de ação. O atalho aparece só nas ações que o app executa de fato.
const hint = (text: string) => <Text as="span" type="secondary" fontSize={12}>{text}</Text>;
export const action = (key: string, label: string, run: () => void, shortcut?: string): ContextMenuItem => ({
  key, label, extra: shortcut ? hint(shortcut) : undefined, onClick: run,
});
// Ação sem alvo: desabilitada, com o motivo embaixo do nome (texto comum, lido por leitor de tela).
export const blocked = (key: string, label: string, reason: string): ContextMenuItem => ({
  key, disabled: true, label: <Flexbox gap={0}><span>{label}</span>{hint(reason)}</Flexbox>,
});
export const DIVIDER: ContextMenuItem = { type: 'divider' };

/** Faixa de 36 px no topo da área central. Montada só com arquivo aberto. `dirty` traz os caminhos com alteração não salva; `pinned`, os fixados (sempre antes das outras abas). */
export function FileTabs({ paths, active, dirty, pinned, hasCursor, onSelect, onClose, onBatch, onTogglePin, onCopy, onReveal }: {
  paths: string[];
  active: string | null;
  dirty: ReadonlySet<string>;
  pinned: readonly string[];
  /** Diz se a aba já tem a posição do cursor (o editor carregou). Sem ela, copiar o caminho de navegação fica desabilitado. */
  hasCursor: (path: string) => boolean;
  onSelect: (path: string) => void;
  onClose: (path: string) => void;
  onBatch: (kind: BatchKind, path: string) => void;
  onTogglePin: (path: string) => void;
  onCopy: (kind: CopyKind, path: string) => void;
  onReveal: (path: string) => void;
}) {
  const strip = useRef<HTMLDivElement>(null);
  const activeRef = useRef(active);
  const [listOpen, setListOpen] = useState(false);
  const details = detailsOf(paths);

  // Aba ativa entra na área visível ao abrir, trocar, fechar ou sujar/salvar (o ● muda a largura da aba, não a da faixa).
  useLayoutEffect(() => {
    activeRef.current = active;
    if (active && strip.current) reveal(strip.current, active);
  }, [active, paths, dirty]);
  useLayoutEffect(() => {
    const box = strip.current;
    if (!box) return;
    const observer = new ResizeObserver(() => {
      if (activeRef.current) reveal(box, activeRef.current);
    });
    observer.observe(box);
    return () => observer.disconnect();
  }, []);

  // Menu da aba clicada (ou da aba focada). Monta na hora do clique, então lê o estado atual; a ação age sobre `path`, ativa ou não.
  const menuFor = (path: string): ContextMenuItem[] => {
    const others = batchTargets(paths, pinned, dirty, 'others', path);
    const every = batchTargets(paths, pinned, dirty, 'all');
    const isPinned = pinned.includes(path);
    const sections: ContextMenuItem[][] = [
      [
        action('close', 'Fechar', () => onClose(path), 'Ctrl+F4'),
        others.length > 0
          ? action('close-others', 'Fechar Outros', () => onBatch('others', path))
          : blocked('close-others', 'Fechar Outros', 'Não há outras abas para fechar. As fixadas ficam abertas.'),
        every.length > 0
          ? action('close-all', 'Fechar Tudo', () => onBatch('all', path), 'Ctrl+K W')
          : blocked('close-all', 'Fechar Tudo', 'Não há abas para fechar. As fixadas ficam abertas.'),
      ],
      [
        action('copy-relative', 'Copiar Caminho Relativo', () => onCopy('relative', path), 'Ctrl+K Ctrl+Shift+C'),
        action('copy-path', 'Copiar o Caminho', () => onCopy('absolute', path), 'Shift+Alt+C'),
        hasCursor(path)
          ? action('copy-navigation', 'Copiar Caminho de Navegação', () => onCopy('navigation', path))
          : blocked('copy-navigation', 'Copiar Caminho de Navegação', 'Abra o arquivo para ter a posição do cursor.'),
      ],
      [action('reveal', 'Revelar no Explorer', () => onReveal(path))],
      [action('pin', isPinned ? 'Desafixar' : 'Fixar', () => onTogglePin(path), 'Ctrl+K Shift+Enter')],
    ];
    return sections.flatMap((items, i) => (i ? [DIVIDER, ...items] : items));
  };

  // Shift+F10 e a tecla de contexto: disparam o clique direito da própria aba, então o menu é o mesmo, ancorado no canto dela.
  const openFromKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!((e.key === 'F10' && e.shiftKey) || e.key === 'ContextMenu')) return;
    e.preventDefault();
    const rect = e.currentTarget.getBoundingClientRect();
    e.currentTarget.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rect.left + 16, clientY: rect.bottom }));
  };

  // A lista serve para achar um arquivo que caiu fora da faixa: mostra todos os abertos, com a pasta de cada um (caminhos diferentes ficam distintos), o ativo com check e os sujos com ●.
  const list = (
    <Flexbox gap={2} padding={8} className="w-64 max-h-72 overflow-y-auto">
      {paths.map((path) => {
        const isActive = path === active;
        return (
          <Button
            key={path} type="text" size="small" title={path}
            icon={<Check className={`shrink-0${isActive ? '' : ' invisible'}`} />}
            aria-current={isActive ? 'true' : undefined}
            className="min-w-0 justify-start! gap-2"
            onClick={() => { onSelect(path); setListOpen(false); }}
          >
            <Text as="span" ellipsis className="min-w-0">{fileName(path)}</Text>
            <Text as="span" ellipsis type="secondary" fontSize={12} className="min-w-0">{folderOf(path)}</Text>
            {pinned.includes(path) && <Pin aria-label="Fixada" className="size-3 shrink-0 text-muted-foreground" />}
            {dirty.has(path) && <Text as="span" aria-label="Alteração não salva" className="shrink-0">●</Text>}
          </Button>
        );
      })}
    </Flexbox>
  );

  return (
    <Flexbox horizontal align="center" gap={4} height={36} flex="none" paddingInline={4} className="border-b bg-card">
      {/* Só as abas rolam na horizontal; a lista fica fora da rolagem. `overflow-clip` recorta o indicador sem criar outra área de rolagem. */}
      {/* `@container`: a faixa é o contêiner das abas, então uma aba nunca passa da largura visível (cqw), e o X fica dentro dela. */}
      <Flexbox
        ref={strip} horizontal align="center" flex={1} className="@container min-w-0 overflow-x-auto [scrollbar-width:none]!"
        onWheel={(e) => { if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY; }}
      >
        <TabsRoot value={active ?? ''} onValueChange={(key: string) => onSelect(key)} className="gap-0">
          <TabsList className="h-8 flex-nowrap justify-start overflow-clip rounded-md">
            <TabsIndicator />
            {paths.map((path) => {
              const name = fileName(path);
              const isPinned = pinned.includes(path);
              return (
                // Aba e X são irmãos: o X fica por cima da aba, na parte direita, e não aninha um button no outro.
                // O menu é do wrapper (aba e X), então o clique direito em qualquer dos dois abre o mesmo menu.
                <ContextMenuTrigger key={path} items={() => menuFor(path)}>
                  <div data-file={path} className="relative flex-none max-w-[min(14rem,100cqw)]" onKeyDown={openFromKey}>
                    <TabsTab value={path} title={path} className="max-w-full flex-none gap-1.5 pr-7! pl-3">
                      {isPinned
                        ? <Pin className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
                        : <File className="size-3.5 shrink-0 text-muted-foreground" />}
                      <Text as="span" ellipsis className="min-w-0">{name}</Text>
                      {details.get(path) && (
                        <Text as="span" ellipsis type="secondary" fontSize={12} className="min-w-0">{details.get(path)}</Text>
                      )}
                      {isPinned && <span className="sr-only">(fixada)</span>}
                      {dirty.has(path) && (
                        <Text as="span" aria-label="Alteração não salva" title="Alteração não salva" className="shrink-0">●</Text>
                      )}
                    </TabsTab>
                    <ActionIcon
                      variant="borderless" size="small" icon={X}
                      aria-label={`Fechar ${path}`} title={`Fechar ${name}`}
                      className="absolute! z-10 top-1/2 right-1 -translate-y-1/2!"
                      onClick={() => onClose(path)}
                    />
                  </div>
                </ContextMenuTrigger>
              );
            })}
          </TabsList>
        </TabsRoot>
      </Flexbox>
      <Popover open={listOpen} onOpenChange={setListOpen} trigger="click" placement="bottomRight" content={list}>
        <Button
          type="text" size="small" icon={<List className="size-3.5" />} className="shrink-0"
          aria-label={`Arquivos abertos: ${paths.length}`} title={`Todos os arquivos abertos (${paths.length})`}
        >
          {paths.length}
        </Button>
      </Popover>
    </Flexbox>
  );
}
