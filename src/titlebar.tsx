// Barra de título própria: a janela não tem mais a barra nativa do Windows. Arrastar e o duplo clique vêm do
// atributo data-tauri-drag-region, que vale só para o elemento que o tem; por isso botões e campos não o levam.
import { useEffect, useState, type ReactNode } from 'react';
import { Copy, Minus, Square, X, type LucideIcon } from 'lucide-react';
import { Flexbox } from '@lobehub/ui';
import { Text } from '@lobehub/ui/base-ui';
import { host } from './backend';

function WindowButton({ label, icon: Icon, danger, disabled, onClick }: {
  label: string; icon: LucideIcon; danger?: boolean; disabled: boolean; onClick: () => void;
}) {
  return (
    <button
      type="button" aria-label={label} title={label} disabled={disabled} onClick={onClick}
      className={`flex h-full w-[46px] shrink-0 items-center justify-center text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50 disabled:opacity-40 ${
        danger ? 'hover:bg-destructive hover:text-white' : 'hover:bg-muted hover:text-foreground'
      }`}
    >
      <Icon className="size-4" />
    </button>
  );
}

/** `children` vão depois do nome; os botões da janela ficam colados à borda direita. */
export function TitleBar({ connected, children }: { connected: boolean; children?: ReactNode }) {
  const [maximized, setMaximized] = useState(false);

  // Antes de connect() não há janela para consultar: a consulta e a escuta só começam com a conexão pronta.
  useEffect(() => {
    if (!connected) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    const refresh = () => void host().isMaximized().then((value) => {
      if (active) setMaximized(value);
    });
    refresh();
    void host().onResized(refresh).then((off) => {
      if (active) unlisten = off;
      else off();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [connected]);

  return (
    // data-tauri-drag-region: o arrasto e o duplo clique valem só para estes três elementos, não para os filhos.
    <Flexbox as="header" data-tauri-drag-region horizontal align="center" height={36} flex="none" className="whitespace-nowrap border-b bg-card">
      <Text as="span" data-tauri-drag-region className="px-3" fontSize={14} weight={600}>Orquestrador</Text>
      <Flexbox data-tauri-drag-region horizontal align="center" gap={12} flex={1} allowShrink style={{ paddingRight: 12 }}>{children}</Flexbox>
      <Flexbox horizontal height="100%" flex="none">
        <WindowButton label="Minimizar" icon={Minus} disabled={!connected} onClick={() => host().minimize()} />
        <WindowButton
          label={maximized ? 'Restaurar' : 'Maximizar'} icon={maximized ? Copy : Square}
          disabled={!connected} onClick={() => host().toggleMaximize()}
        />
        <WindowButton label="Fechar" icon={X} danger disabled={!connected} onClick={() => host().close()} />
      </Flexbox>
    </Flexbox>
  );
}
