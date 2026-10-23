// Lista "Todas as abas" das faixas do Orquestrador, do Terminal e do editor: um DropdownMenu com uma linha de 26 px por aba (medidas em
// src/index.css), o ícone na margem, o rótulo que corta com reticências, o estado à direita e o check da ativa na margem direita, no
// canto da linha. Com `onClose`, o X toma o lugar do check na linha destacada (ponteiro ou teclado) e Delete fecha a aba
// destacada; o menu continua aberto, menos quando a aba tem alteração não salva (fechar pede confirmação num diálogo) ou quando a lista
// esvazia: aí o menu desmonta na hora, pois o painel Terminal se esconde e a animação de saída, sem âncora, pulava para o canto da janela.
import { type KeyboardEvent, type ReactNode, useState } from 'react';
import { Check, List, X } from 'lucide-react';
import { Button, DropdownMenu } from '@lobehub/ui/base-ui';

export type TabListItem = {
  key: string;
  icon: ReactNode;
  label: ReactNode;
  /** Texto completo, para o tooltip e o leitor de tela. */
  title: string;
  state?: string;
  dirty?: boolean;
  onSelect: () => void;
  onClose?: () => void;
};

/** Sem `children`, o gatilho é o botão de lista com a contagem, o mesmo nas três faixas. */
export function TabListMenu({ items, active, placement = 'bottomRight', children }: {
  items: TabListItem[];
  active: string | null;
  placement?: 'bottomLeft' | 'bottomRight';
  children?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  if (open && items.length === 0) setOpen(false);
  const close = (item: TabListItem) => {
    if (item.dirty) setOpen(false);
    item.onClose?.();
  };
  // O item destacado é o que tem o foco; o rótulo carrega a chave da aba.
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'Delete') return;
    const key = (document.activeElement?.querySelector('[data-tab-key]') as HTMLElement | null)?.dataset.tabKey;
    const item = items.find((i) => i.key === key);
    if (!item?.onClose) return;
    e.preventDefault();
    close(item);
  };
  const menu = items.map((item) => ({
    key: item.key, title: item.title, icon: item.icon,
    label: <span data-tab-key={item.key} className="block max-w-72 truncate">{item.label}</span>,
    extra: (
      // Altura da linha (26 px, src/index.css) e itens centrados: em `inline-flex`, a coluna alinhava pela linha de base e ficava 2 px acima.
      <span className="flex h-[26px] items-center gap-1.5 whitespace-nowrap">
        {item.state}
        {item.dirty && <span aria-label="Alteração não salva">●</span>}
        {/* Na margem direita de 26 px, espelhando o ícone da esquerda: o X fica no canto da linha. */}
        <span className="absolute top-1/2 right-[5px] flex size-4 -translate-y-1/2 items-center justify-center">
          <Check className={`tab-list-check size-3.5${active === item.key ? '' : ' invisible'}`} />
          {item.onClose && (
            <button
              // Só para o ponteiro: no teclado, Delete fecha a linha destacada, e o nome do item fica só com o rótulo.
              type="button" tabIndex={-1} aria-hidden title="Fechar (Delete)"
              className="tab-list-close absolute inset-0 flex items-center justify-center rounded-sm hover:bg-[var(--ant-color-fill-secondary)]"
              onClick={(e) => { e.stopPropagation(); close(item); }}
            >
              <X className="size-3.5" />
            </button>
          )}
        </span>
      </span>
    ),
    onClick: item.onSelect,
  }));
  const trigger = children ?? (
    <Button
      type="text" size="small" icon={<List className="size-3.5" />} className="shrink-0 self-center"
      aria-label={`Lista de abas: ${items.length}`} title={`Todas as abas (${items.length})`}
    >
      {items.length}
    </Button>
  );
  if (items.length === 0) return trigger;
  return (
    <DropdownMenu items={menu} placement={placement} open={open} onOpenChange={setOpen} popupProps={{ onKeyDown }}>
      {trigger}
    </DropdownMenu>
  );
}
