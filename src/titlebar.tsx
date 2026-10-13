// Barra de título própria: a janela não tem mais a barra nativa do Windows. Arrastar e o duplo clique vêm do
// atributo data-tauri-drag-region, que vale só para o elemento que o tem; por isso botões e campos não o levam.
import { useEffect, useState, type ReactNode } from 'react';
import { Copy, Minus, Square, X, type LucideIcon } from 'lucide-react';
import { cn } from 'cn';
import { backend } from './backend';

function WindowButton({ label, icon: Icon, danger, disabled, onClick }: {
  label: string; icon: LucideIcon; danger?: boolean; disabled: boolean; onClick: () => void;
}) {
  return (
    <button
      type="button" aria-label={label} title={label} disabled={disabled} onClick={onClick}
      className={cn(
        'flex h-full w-[46px] shrink-0 items-center justify-center text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50 disabled:opacity-40',
        danger ? 'hover:bg-destructive hover:text-white' : 'hover:bg-muted hover:text-foreground',
      )}
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
    const refresh = () => void backend.isMaximized().then((value) => {
      if (active) setMaximized(value);
    });
    refresh();
    void backend.onResized(refresh).then((off) => {
      if (active) unlisten = off;
      else off();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [connected]);

  return (
    <header data-tauri-drag-region className="flex h-9 shrink-0 items-center whitespace-nowrap border-b bg-card">
      <span data-tauri-drag-region className="px-3 text-sm font-semibold">Orquestrador</span>
      <div data-tauri-drag-region className="flex min-w-0 flex-1 items-center gap-3 pr-3">{children}</div>
      <div className="flex h-full shrink-0">
        <WindowButton label="Minimizar" icon={Minus} disabled={!connected} onClick={() => backend.minimize()} />
        <WindowButton
          label={maximized ? 'Restaurar' : 'Maximizar'} icon={maximized ? Copy : Square}
          disabled={!connected} onClick={() => backend.toggleMaximize()}
        />
        <WindowButton label="Fechar" icon={X} danger disabled={!connected} onClick={() => backend.close()} />
      </div>
    </header>
  );
}
