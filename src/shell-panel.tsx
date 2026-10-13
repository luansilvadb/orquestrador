// O painel Terminal, embaixo dos terminais da tela principal: o shell do usuário, redimensionável pela borda de cima.
// Fica montado depois da primeira abertura; fechar só esconde (o shell segue vivo no backend).
import { SquareTerminal, X } from 'lucide-react';
import { ActionIcon, DraggablePanel, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import type { TermView } from './term';
import { TermHost } from './terminals';
import { SHELL } from './types';

// Altura ao abrir o app (não se guarda entre aberturas); o mínimo mantém a barra e uma linha de terminal.
const SHELL_HEIGHT = 280;
const SHELL_MIN = 120;
// Objeto constante: o DraggablePanel recalcula o tamanho quando `defaultSize` muda de identidade.
const DEFAULT_SIZE = { height: SHELL_HEIGHT };

/** Escondido com `hidden` (display none): sai do layout e o cartão de cima cresce. O TermHost fica montado. */
export function ShellPanel({ open, mounted, maxHeight, onClose, onReady }: {
  open: boolean;
  mounted: boolean;
  maxHeight: number;
  onClose: () => void;
  onReady: (view: TermView) => void;
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
          <Flexbox horizontal align="center" gap={8} height={36} flex="none" paddingInline={12} className="border-b">
            <SquareTerminal className="size-3.5 text-muted-foreground" />
            <Text as="span" weight={500}>Terminal</Text>
            <Flexbox flex={1} />
            <ActionIcon
              variant="borderless" size="small" icon={X} aria-label="Fechar o painel Terminal"
              title="Fecha o painel; o shell continua rodando"
              onClick={onClose}
            />
          </Flexbox>
          <div className="relative mx-1.5 mb-1.5 min-h-0 flex-1 overflow-hidden rounded-lg bg-terminal">
            {mounted && <TermHost id={SHELL} writable onReady={onReady} />}
          </div>
        </Flexbox>
      </DraggablePanel>
    </div>
  );
}
