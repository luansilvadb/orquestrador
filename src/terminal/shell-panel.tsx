// O painel Terminal, embaixo da área do arquivo na coluna central da tela principal: uma aba por shell do usuário, redimensionável pela borda de cima.
// Fica montado depois da primeira abertura; fechar só esconde (os shells seguem vivos no backend). Todos os TermHost ficam montados; só o da aba ativa aparece.
import { type KeyboardEvent, type PointerEvent, useLayoutEffect, useRef, useState } from 'react';
import { Check, ChevronDown, ChevronLeft, ChevronRight, ChevronsUpDown, List, Plus, SquareTerminal, X } from 'lucide-react';
import { ActionIcon, Button, DraggablePanel, Popover, TabsIndicator, TabsList, TabsRoot, TabsTab, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import type { TermView } from './term';
import { TermHost } from './terminals';
import { dragShift, revealTab, trackTabDrag, type TabDrag, wheelToScroll } from '../shared/tab-strip';

// Altura ao abrir o app (não se guarda entre aberturas); o mínimo mantém a barra e uma linha de terminal.
export const SHELL_HEIGHT = 280;
export const SHELL_MIN = 120;
// Abaixo dessa largura do painel, a faixa de abas vira um seletor do terminal ativo. Com as setas, a faixa fica ~186 px menor que o cabeçalho;
// a aba com o rótulo no máximo (número, programa de até 128 px e o X) precisa de ~274 px, então o normal só começa aqui.
const COMPACT_WIDTH = 460;
// Objeto constante: o DraggablePanel recalcula o tamanho quando `defaultSize` muda de identidade.
const DEFAULT_SIZE = { height: SHELL_HEIGHT };

// `title` é o completo (pasta e programa), para o tooltip; `program` é '' até o backend responder.
export type ShellTab = { id: string; title: string; program: string };

// Acha a aba pelo `data-shell` e a revela na faixa (a regra de rolagem está em tab-strip).
const reveal = (box: HTMLDivElement, id: string) => revealTab(box, box.querySelector<HTMLElement>(`[data-shell="${id}"]`));

// O número vem do id `shell-N`, então não muda quando outra aba fecha; o programa é o que corta quando falta espaço.
function shellNumber(tab: ShellTab) {
  return `Terminal ${tab.id.match(/(\d+)$/)?.[1] ?? ''}`;
}

function shellLabel(tab: ShellTab) {
  return `${shellNumber(tab)}${tab.program ? ` · ${tab.program}` : ''}`;
}

/** Escondido com `hidden` (display none): sai do layout e a área do arquivo cresce. Os TermHost ficam montados. */
export function ShellPanel({ open, maxHeight, height, onHeightChange, tabs, active, onSelect, onMove, onAdd, onHide, onClose, onReady }: {
  open: boolean;
  maxHeight: number;
  /** Altura que o usuário deu ao painel (o App guarda e restaura). Só controla o tamanho: não mexe nos shells. */
  height: number;
  onHeightChange: (height: number) => void;
  tabs: ShellTab[];
  active: string;
  onSelect: (id: string) => void;
  /** Leva a aba para `index`, contado entre as outras abas. */
  onMove: (id: string, index: number) => void;
  onAdd: () => void;
  onHide: () => void;
  onClose: (id: string) => void;
  onReady: (id: string, view: TermView) => void;
}) {
  const header = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [listOpen, setListOpen] = useState(false);
  const [compact, setCompact] = useState(false);
  // Se há aba escondida para cada lado: decide se as setas aparecem e se cada uma fica habilitada.
  const [edge, setEdge] = useState({ before: false, after: false });
  // Se as abas cabem sem as setas: elas só aparecem quando a faixa, sem elas, não comporta tudo.
  const [overflow, setOverflow] = useState(false);
  const current = tabs.find((tab) => tab.id === active);
  // Aba sendo arrastada (o deslocamento e o destino estão em TabDrag).
  const [drag, setDrag] = useState<({ id: string } & TabDrag) | null>(null);
  const dragFrom = drag ? tabs.findIndex((tab) => tab.id === drag.id) : -1;

  function measure() {
    const box = strip.current;
    if (!box) return;
    // Com as setas na tela, elas ocupam parte da faixa: o que cabe soma esse espaço de volta; senão as setas nunca saem.
    const header = box.parentElement!;
    const gap = parseFloat(getComputedStyle(header).columnGap) || 0;
    const room = [...header.querySelectorAll<HTMLElement>('button[aria-label^="Abas"]')]
      .reduce((sum, el) => sum + el.getBoundingClientRect().width + gap, 0);
    setOverflow(box.scrollWidth > box.clientWidth + room + 1);
    setEdge({
      before: box.scrollLeft > 0,
      after: box.scrollLeft + box.clientWidth < box.scrollWidth - 1,
    });
  }

  // Adicionar, selecionar, fechar a ativa, reabrir o painel, trocar de modo e aparecer/sumir as setas (a faixa encolhe ou cresce): a aba ativa entra na área visível.
  const activeRef = useRef(active);
  useLayoutEffect(() => {
    activeRef.current = active;
    if (strip.current) reveal(strip.current, active);
    measure();
  }, [active, tabs, open, compact, overflow]);

  // A largura do cabeçalho é a do painel: decide o modo e, no normal, recalcula o que cabe. A faixa também é observada, pois muda de largura sem o cabeçalho mudar. Escondido (largura 0), mantém o modo de antes.
  useLayoutEffect(() => {
    const box = header.current;
    if (!box) return;
    const observer = new ResizeObserver(() => {
      if (box.clientWidth === 0) return;
      setCompact(box.clientWidth < COMPACT_WIDTH);
      if (strip.current) {
        reveal(strip.current, activeRef.current);
        measure();
      }
    });
    observer.observe(box);
    // Só existe no modo normal; ao trocar de modo, o efeito roda de novo e observa a faixa montada.
    if (strip.current) observer.observe(strip.current);
    return () => observer.disconnect();
  }, [compact]);

  // Fechar um item pela lista: o foco vai para o item que ocupa o lugar dele (ou o último), em vez de se perder.
  const focusAfterClose = useRef<number | null>(null);
  useLayoutEffect(() => {
    const index = focusAfterClose.current;
    if (index === null) return;
    focusAfterClose.current = null;
    const items = listRef.current?.querySelectorAll<HTMLButtonElement>('button[data-shell-item]');
    if (items?.length) items[Math.min(index, items.length - 1)].focus();
  }, [tabs]);

  function closeFromList(index: number, id: string) {
    focusAfterClose.current = index;
    onClose(id);
  }

  function changeListOpen(next: boolean) {
    focusAfterClose.current = null;
    setListOpen(next);
  }

  // Setas: rolam a faixa em uma página (80% da largura visível).
  function page(direction: 1 | -1) {
    const box = strip.current;
    if (box) box.scrollBy({ left: direction * box.clientWidth * 0.8, behavior: 'smooth' });
  }

  // Segurar a aba e arrastar para os lados: ela segue o ponteiro, as vizinhas abrem espaço e, ao soltar, a ordem muda. Esc ou perda do ponteiro cancelam.
  function dragTab(e: PointerEvent<HTMLDivElement>, id: string) {
    const box = strip.current;
    // Só pela aba: o X não arrasta.
    if (e.button !== 0 || !box || !(e.target as HTMLElement).closest('[role="tab"]')) return;
    const nodes = [...box.querySelectorAll<HTMLElement>('[data-shell]')];
    const from = nodes.findIndex((node) => node.dataset.shell === id);
    trackTabDrag(e, box, nodes, from, {
      onStart: () => onSelect(id),
      onDrag: (d) => setDrag({ id, ...d }),
      onDrop: (target) => {
        if (target !== null && target !== from) onMove(id, target);
        setDrag(null);
      },
    });
  }

  // Lista aberta: as setas do teclado andam entre os itens (sem passar pelos X); Escape fecha (o Popover cuida disso) e devolve o foco ao gatilho.
  function moveFocus(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const items = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('button[data-shell-item]'));
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const step = e.key === 'ArrowDown' ? 1 : -1;
    // Sem foco nos itens, a primeira seta cai no começo (para baixo) ou no fim (para cima).
    const from = index < 0 ? (step > 0 ? -1 : items.length) : index;
    e.preventDefault();
    items[(from + step + items.length) % items.length]?.focus();
  }

  // A mesma lista serve aos dois modos: todos os terminais, o ativo com check e um X por item.
  const list = (
    <Flexbox ref={listRef} gap={2} padding={8} className="w-56" onKeyDown={moveFocus}>
      <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 4 }}>
        {tabs.length} {tabs.length === 1 ? 'terminal' : 'terminais'}
      </Text>
      <Flexbox gap={2} className="max-h-72 overflow-y-auto">
        {tabs.map((tab, index) => {
          const isCurrent = tab.id === active;
          return (
            <Flexbox key={tab.id} horizontal align="center" gap={2}>
              <Button
                data-shell-item type="text" size="small" className="min-w-0 flex-1 justify-start! gap-2"
                icon={<Check className={`shrink-0${isCurrent ? '' : ' invisible'}`} />}
                aria-current={isCurrent ? 'true' : undefined}
                title={tab.title}
                onClick={() => { onSelect(tab.id); setListOpen(false); }}
              >
                <Text as="span" ellipsis className="min-w-0">{shellLabel(tab)}</Text>
              </Button>
              <ActionIcon
                variant="borderless" size="small" icon={X} className="shrink-0"
                aria-label={`Fechar o ${shellLabel(tab)}`} title={`Fechar o terminal ${tab.title}`}
                onClick={() => closeFromList(index, tab.id)}
              />
            </Flexbox>
          );
        })}
      </Flexbox>
    </Flexbox>
  );

  const addButton = (
    <ActionIcon
      variant="borderless" size="small" icon={Plus} className="shrink-0 self-center"
      aria-label="Novo terminal" title="Novo terminal"
      onClick={onAdd}
    />
  );

  // `shrink-0`: a área do arquivo é que encolhe; o painel mantém a altura que o usuário deu.
  return (
    <div className={open ? 'shrink-0' : 'hidden'}>
      <DraggablePanel
        placement="bottom" expandable={false} showHandleWideArea={false}
        size={{ height }} defaultSize={DEFAULT_SIZE} minHeight={SHELL_MIN} maxHeight={Math.max(maxHeight, SHELL_MIN)}
        onSizeChange={(_, size) => typeof size?.height === 'number' && onHeightChange(size.height)}
        backgroundColor="var(--ant-color-bg-container)"
      >
        <Flexbox height="100%" className="min-h-0 overflow-hidden bg-card">
          {/* Esticado para a faixa de abas ocupar os 36 px; os controles centralizam sozinhos (self-center). O divisor de 1 px fica por baixo, como no Orquestrador. */}
          <Flexbox ref={header} horizontal align="stretch" gap={4} height={36} flex="none" paddingInline={4} className="relative">
            <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-border" />
            {compact ? (
              <>
                {/* Modo compacto: o terminal ativo, com o número sempre visível; o clique abre a mesma lista. */}
                <Popover open={listOpen} onOpenChange={changeListOpen} trigger="click" placement="bottomLeft" content={list}>
                  <Button
                    type="text" size="small" icon={<SquareTerminal className="size-3.5" />} className="min-w-0 flex-1 justify-start! gap-1.5 self-center"
                    aria-label={`Terminal ativo: ${current ? shellLabel(current) : 'nenhum'}. Abrir a lista de terminais (${tabs.length})`}
                    title={current?.title}
                  >
                    {current && <Text as="span" className="shrink-0">{shellNumber(current)}</Text>}
                    {current?.program && <Text as="span" ellipsis type="secondary" className="min-w-0">· {current.program}</Text>}
                    <ChevronsUpDown className="ml-auto size-3.5 shrink-0" />
                  </Button>
                </Popover>
                <ActionIcon
                  variant="borderless" size="small" icon={X} className="shrink-0 self-center" disabled={!current}
                  aria-label={current ? `Fechar o ${shellLabel(current)}` : 'Fechar o terminal ativo'}
                  title={current ? `Fechar o terminal ${current.title}` : 'Fechar o terminal ativo'}
                  onClick={() => current && onClose(current.id)}
                />
                {addButton}
              </>
            ) : (
              <>
                {/* Só as abas rolam na horizontal; as setas, a lista, o "+" e o botão de esconder ficam fora da rolagem. A faixa só cresce até o conteúdo, para o "+" vir logo depois da última aba. */}
                <Flexbox
                  ref={strip} horizontal align="stretch" flex="0 1 auto" className="min-w-0 overflow-x-auto [scrollbar-width:none]!"
                  onScroll={measure}
                  onWheel={wheelToScroll}
                >
                  {/* Variant square, como no Orquestrador: a faixa tem 36 px e o indicador de 2 px fica colado no divisor. `overflow-clip` recorta o indicador sem criar uma área de rolagem (o `overflow-y-hidden` fazia o X virar `auto` e aparecer uma barra). A sombra zerada e o gap 0 tiram a borda e o espaço do variant. */}
                  <TabsRoot variant="square" value={active} onValueChange={(key: string) => onSelect(key)} className="gap-0">
                    <TabsList className="h-full flex-nowrap justify-start overflow-clip" style={{ gap: 0, boxShadow: 'none' }}>
                      {/* No arrasto, a aba ativa é a arrastada: o indicador anda junto, sem a transição, e fica por cima do fundo dela. */}
                      <TabsIndicator style={drag ? { transform: `translateX(${drag.dx}px)`, transition: 'none', zIndex: 30 } : undefined} />
                      {tabs.map((tab, index) => {
                        // Vizinhas entre a origem e o destino abrem o espaço da arrastada, com transição; ao soltar, tudo volta a 0 junto com a ordem nova.
                        const shift = drag ? dragShift(drag, dragFrom, index) : 0;
                        // Aba e X são irmãos: o X fica por cima da aba, na parte direita, e não aninha um button no outro. Os dois ocupam a altura da faixa; o X centraliza na vertical.
                        return (
                          <div
                            key={tab.id} data-shell={tab.id}
                            className={`relative h-full flex-none${drag?.id === tab.id ? ' z-20 bg-card' : drag ? ' transition-transform duration-150 motion-reduce:transition-none' : ''}`}
                            style={shift ? { transform: `translateX(${shift}px)` } : undefined}
                            onPointerDown={(e) => dragTab(e, tab.id)}
                          >
                            <TabsTab value={tab.id} title={tab.title} className="h-full flex-none gap-1.5 pr-7! pl-3">
                              <SquareTerminal className="size-3.5 shrink-0 text-muted-foreground" />
                              <Text as="span" className="shrink-0">{shellNumber(tab)}</Text>
                              {tab.program && <Text as="span" ellipsis type="secondary" className="max-w-32">· {tab.program}</Text>}
                            </TabsTab>
                            <ActionIcon
                              variant="borderless" size="small" icon={X}
                              aria-label={`Fechar o ${shellLabel(tab)}`} title={`Fechar o terminal ${tab.title}`}
                              className="absolute! z-10 top-1/2 right-1 -translate-y-1/2!"
                              onClick={() => onClose(tab.id)}
                            />
                          </div>
                        );
                      })}
                    </TabsList>
                  </TabsRoot>
                </Flexbox>
                {addButton}
                <div className="flex-1" />
                {overflow && (
                  <ActionIcon
                    variant="borderless" size="small" icon={ChevronLeft} className="shrink-0 self-center"
                    aria-label="Abas anteriores" title="Mostra as abas anteriores" disabled={!edge.before}
                    onClick={() => page(-1)}
                  />
                )}
                {overflow && (
                  <ActionIcon
                    variant="borderless" size="small" icon={ChevronRight} className="shrink-0 self-center"
                    aria-label="Abas seguintes" title="Mostra as abas seguintes" disabled={!edge.after}
                    onClick={() => page(1)}
                  />
                )}
                <Popover open={listOpen} onOpenChange={changeListOpen} trigger="click" placement="bottomRight" content={list}>
                  <Button
                    type="text" size="small" icon={<List className="size-3.5" />} className="shrink-0 self-center"
                    aria-label={`Lista dos terminais: ${tabs.length}`} title={`Todos os terminais (${tabs.length})`}
                  >
                    {tabs.length}
                  </Button>
                </Popover>
              </>
            )}
            <ActionIcon
              variant="borderless" size="small" icon={ChevronDown} className="shrink-0 self-center"
              aria-label="Esconder o painel Terminal" title="Esconde o painel; os shells continuam rodando"
              onClick={onHide}
            />
          </Flexbox>
          <div className="relative min-h-0 flex-1 overflow-hidden bg-terminal">
            {tabs.map((tab) => (
              <div key={tab.id} className={`absolute inset-0${tab.id !== active ? ' invisible' : ''}`}>
                <TermHost id={tab.id} writable onReady={(view) => onReady(tab.id, view)} />
              </div>
            ))}
          </div>
        </Flexbox>
      </DraggablePanel>
    </div>
  );
}
