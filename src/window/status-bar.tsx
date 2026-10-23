// A barra de status com a branch, o Sincronizar e o botão do Terminal.
import { RefreshCw, SquareTerminal } from 'lucide-react';
import { Flexbox } from '@lobehub/ui';

// Item da barra de status, nas medidas do Antigravity: 22 px de altura, 5 px de respiro dos lados e 12 px de texto da interface.
const STATUS_ITEM = 'flex h-full shrink-0 items-center gap-1 px-[5px] text-[12px] outline-none hover:bg-[var(--ant-color-fill)] focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset disabled:cursor-default disabled:hover:bg-transparent';

/**
 * Barra de status embaixo da tela principal, como a do Antigravity: à esquerda, a branch atual (abre o Controle do Código-Fonte) e
 * o Sincronizar, que recebe e envia (com os commits a receber e a enviar ao lado do ícone, quando há); à direita, o painel Terminal.
 */
export function StatusBar({ git, terminalOpen, onToggleTerminal, onOpenGit, onSync }: {
  /** Branch e contadores do repositório; sem repositório, a barra só tem o Terminal. */
  git: { branch: string; ahead: number; behind: number; syncing: boolean } | undefined;
  terminalOpen: boolean;
  onToggleTerminal: () => void;
  onOpenGit: () => void;
  onSync: () => void;
}) {
  const counts = git && (git.ahead || git.behind) ? `${git.behind}↓ ${git.ahead}↑` : '';
  return (
    <Flexbox as="footer" horizontal align="stretch" height={22} flex="none" className="border-t bg-card px-[3px]">
      {git && (
        <>
          <button
            type="button" className={`${STATUS_ITEM} min-w-0 shrink`} onClick={onOpenGit}
            aria-label={`${git.branch}, branch atual. Abrir o Controle do Código-Fonte`} title={`${git.branch}, branch atual`}
          >
            <span className="truncate">{git.branch}</span>
          </button>
          <button
            type="button" className={STATUS_ITEM} onClick={onSync} disabled={git.syncing}
            aria-label={`Sincronizar as alterações${counts ? ` (${git.behind} a receber, ${git.ahead} a enviar)` : ''}`}
            title={git.syncing ? 'Sincronizando…' : `Sincronizar as alterações${counts ? ` (${counts})` : ''}`}
          >
            <RefreshCw className={`size-3.5 shrink-0${git.syncing ? ' animate-spin' : ''}`} aria-hidden />
            {counts && <span>{counts}</span>}
          </button>
        </>
      )}
      <button
        type="button" aria-pressed={terminalOpen} title="Painel Terminal (Ctrl+`)" onClick={onToggleTerminal}
        className={`${STATUS_ITEM} ml-auto${terminalOpen ? ' bg-[var(--ant-color-fill-secondary)]' : ''}`}
      >
        <SquareTerminal className="size-3.5 shrink-0" aria-hidden />
        Terminal
      </button>
    </Flexbox>
  );
}
