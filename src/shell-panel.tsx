// O painel Terminal, embaixo dos terminais da tela principal: uma aba por shell do usuário, redimensionável pela borda de cima.
// Fica montado depois da primeira abertura; fechar só esconde (os shells seguem vivos no backend). Todos os TermHost ficam montados; só o da aba ativa aparece.
import { ChevronDown, Plus, SquareTerminal, X } from 'lucide-react';
import { ActionIcon, DraggablePanel, TabsIndicator, TabsList, TabsRoot, TabsTab, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import type { TermView } from './term';
import { TermHost } from './terminals';

// Altura ao abrir o app (não se guarda entre aberturas); o mínimo mantém a barra e uma linha de terminal.
const SHELL_HEIGHT = 280;
const SHELL_MIN = 120;
// Objeto constante: o DraggablePanel recalcula o tamanho quando `defaultSize` muda de identidade.
const DEFAULT_SIZE = { height: SHELL_HEIGHT };

export type ShellTab = { id: string; title: string };

/** Escondido com `hidden` (display none): sai do layout e o cartão de cima cresce. Os TermHost ficam montados. */
export function ShellPanel({ open, maxHeight, tabs, active, onSelect, onAdd, onHide, onClose, onReady }: {
  open: boolean;
  maxHeight: number;
  tabs: ShellTab[];
  active: string;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onHide: () => void;
  onClose: (id: string) => void;
  onReady: (id: string, view: TermView) => void;
}) {
  // `shrink-0`: o cartão de cima é que encolhe; o painel mantém a altura que o usuário deu.
  return (
    <div className={open ? 'shrink-0' : 'hidden'}>
      <DraggablePanel
        placement="bottom" expandable={false} showBorder={false}
        defaultSize={DEFAULT_SIZE} minHeight={SHELL_MIN} maxHeight={Math.max(maxHeight, SHELL_MIN)}
        backgroundColor="var(--ant-color-bg-container)"
      >
        <Flexbox height="100%" className="min-h-0 overflow-hidden rounded-xl border bg-card">
          <Flexbox horizontal align="center" gap={4} height={36} flex="none" paddingInline={4} className="border-b">
            {/* Só as abas e o "+" rolam juntos na horizontal; o botão de esconder fica fora da rolagem. */}
            <Flexbox horizontal align="center" gap={4} flex={1} className="min-w-0 overflow-x-auto [scrollbar-width:none]">
              <TabsRoot value={active} onValueChange={(key: string) => onSelect(key)} className="gap-0">
                <TabsList className="h-8 flex-nowrap justify-start overflow-y-hidden rounded-full">
                  <TabsIndicator />
                  {tabs.map((tab) => (
                    // Aba e X são irmãos: o X fica por cima da aba, na parte direita, e não aninha um button no outro.
                    <div key={tab.id} className="relative flex-none">
                      <TabsTab value={tab.id} title={tab.title} className="flex-none gap-1.5 pr-7! pl-3">
                        <SquareTerminal className="size-3.5 shrink-0 text-muted-foreground" />
                        <Text as="span" ellipsis className="max-w-48">{tab.title}</Text>
                      </TabsTab>
                      <ActionIcon
                        variant="borderless" size="small" icon={X}
                        aria-label={`Fechar o terminal ${tab.title}`} title={`Fechar o terminal ${tab.title}`}
                        className="absolute! z-10 top-1/2 right-1 -translate-y-1/2!"
                        onClick={() => onClose(tab.id)}
                      />
                    </div>
                  ))}
                </TabsList>
              </TabsRoot>
              <ActionIcon
                variant="borderless" size="small" icon={Plus} className="shrink-0"
                aria-label="Novo terminal" title="Novo terminal"
                onClick={onAdd}
              />
            </Flexbox>
            <ActionIcon
              variant="borderless" size="small" icon={ChevronDown} className="shrink-0"
              aria-label="Esconder o painel Terminal" title="Esconde o painel; os shells continuam rodando"
              onClick={onHide}
            />
          </Flexbox>
          <div className="relative mx-1.5 mb-1.5 min-h-0 flex-1 overflow-hidden rounded-lg bg-terminal">
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
