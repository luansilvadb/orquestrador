// Barra de título própria: a janela não tem mais a barra nativa do Windows. Arrastar e o duplo clique vêm do
// atributo data-tauri-drag-region, que vale só para o elemento que o tem; por isso botões e campos não o levam.
import { Children, useEffect, useState, type ComponentType, type ReactNode } from 'react';
import { Flexbox } from '@lobehub/ui';
import { ActionIcon, DropdownMenu, Text, Tooltip, type ContextMenuItem } from '@lobehub/ui/base-ui';
import { Menu } from 'lucide-react';
import { SECONDARY } from './text-style';
import { host } from '../backend/backend';
import { CloseIcon, MaximizeIcon, MinimizeIcon, RestoreIcon } from './window-icons';

// O Tooltip clona o botão e entrega o ref; bottomRight alinha a dica à borda direita, então ela não corta na janela.
// Referência do Antigravity: hover branco a 10% nos três, e o Fechar vai ao vermelho com o X branco. Sem transição nem estado ativo.
function WindowButton({ label, icon: Icon, danger, disabled, onClick }: {
  label: string; icon: ComponentType<{ className?: string }>; danger?: boolean; disabled: boolean; onClick: () => void;
}) {
  return (
    <Tooltip title={label} placement="bottomRight">
      <button
        type="button" aria-label={label} disabled={disabled} onClick={onClick}
        className={`flex h-full w-[46px] shrink-0 items-center justify-center text-[#cccccc] outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50 disabled:opacity-40 ${danger ? 'hover:bg-[#e81123e6] hover:text-white' : 'hover:bg-[#ffffff1a]'}`}
      >
        <Icon className="size-4" />
      </button>
    </Tooltip>
  );
}

/** Último segmento do caminho, com \ ou / (o caminho todo fica no title). */
export function folderName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export type TitleBarMenu = {
  recentProjects: string[];
  projectOpen: boolean;
  onNewFile: () => void;
  onOpenFile: () => void;
  onOpenProject: () => void;
  onOpenRecent: (path: string) => void;
  onSave: () => void;
  onSaveAs: () => void;
  onCloseProject: () => void;
  onExit: () => void;
};

/**
 * Identidade à esquerda; a pasta do projeto no meio (nome em destaque, caminho completo em secundário, copiável);
 * `children` são os contadores, agrupados à direita; os botões da janela ficam colados à borda direita.
 */
export function TitleBar({ connected, project, children, menu }: { connected: boolean; project?: string; children?: ReactNode; menu?: TitleBarMenu }) {
  const [maximized, setMaximized] = useState(false);
  const menuItems: ContextMenuItem[] = menu ? [
    { key: 'new-file', label: 'Novo arquivo', extra: <span>Ctrl+N</span>, onClick: menu.onNewFile },
    { key: 'open-file', label: 'Abrir arquivo…', extra: <span>Ctrl+O</span>, onClick: menu.onOpenFile },
    { key: 'open-project', label: 'Abrir pasta…', extra: <span>Ctrl+K Ctrl+O</span>, onClick: menu.onOpenProject },
    {
      key: 'recent-projects', type: 'submenu', label: 'Projetos recentes', openOnHover: true,
      children: menu.recentProjects.length > 0
        ? menu.recentProjects.slice(0, 10).map((path) => ({ key: path, label: folderName(path), desc: path, onClick: () => menu.onOpenRecent(path) }))
        : [{ key: 'empty', label: 'Nenhum projeto recente', disabled: true }],
    },
    { type: 'divider' },
    { key: 'save', label: 'Salvar', extra: <span>Ctrl+S</span>, onClick: menu.onSave },
    { key: 'save-as', label: 'Salvar como…', extra: <span>Ctrl+Shift+S</span>, onClick: menu.onSaveAs },
    { key: 'close-project', label: 'Fechar pasta', disabled: !menu.projectOpen, onClick: menu.onCloseProject },
    { type: 'divider' },
    { key: 'exit', label: 'Sair', onClick: menu.onExit },
  ] : [];

  // O título nativo distingue as janelas na barra de tarefas.
  useEffect(() => {
    if (!connected) return;
    void host().setTitle(project ? `${folderName(project)} - Orquestrador` : 'Orquestrador');
  }, [connected, project]);

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
    // data-tauri-drag-region: o arrasto e o duplo clique valem só nos elementos passivos e nas áreas vazias. O caminho não leva o atributo, para ficar selecionável.
    <Flexbox as="header" data-tauri-drag-region horizontal align="center" height={36} flex="none" className="whitespace-nowrap border-b bg-card">
      {menu && (
        <DropdownMenu items={menuItems} placement="bottomLeft">
          <ActionIcon variant="borderless" size="small" icon={Menu} aria-label="Abrir menu" title="Menu" className="mx-1 shrink-0" />
        </DropdownMenu>
      )}
      <Flexbox data-tauri-drag-region horizontal align="center" paddingInline={12} flex="none">
        <Text as="span" data-tauri-drag-region fontSize={13} weight={600}>Orquestrador</Text>
      </Flexbox>
      <Flexbox data-tauri-drag-region horizontal align="center" gap={8} flex={1} allowShrink className="min-w-0">
        {project && (
          <>
            <Text as="span" data-tauri-drag-region fontSize={13} weight={500} ellipsis title={project} className="min-w-0 shrink">
              {folderName(project)}
            </Text>
            <Text as="span" fontSize={12} code ellipsis title={project} className="min-w-0 shrink select-text" style={SECONDARY}>
              {project}
            </Text>
          </>
        )}
      </Flexbox>
      {/* Sem contadores, o grupo nem existe: não ocupa espaço nem padding. */}
      {Children.toArray(children).length > 0 && (
        <Flexbox data-tauri-drag-region horizontal align="center" gap={12} paddingInline={12} flex="0 1 auto" allowShrink className="min-w-0 max-w-1/2">{children}</Flexbox>
      )}
      <Flexbox horizontal height="100%" flex="none">
        <WindowButton label="Minimizar" icon={MinimizeIcon} disabled={!connected} onClick={() => host().minimize()} />
        <WindowButton
          label={maximized ? 'Restaurar' : 'Maximizar'} icon={maximized ? RestoreIcon : MaximizeIcon}
          disabled={!connected} onClick={() => host().toggleMaximize()}
        />
        <WindowButton label="Fechar" icon={CloseIcon} danger disabled={!connected} onClick={() => host().close()} />
      </Flexbox>
    </Flexbox>
  );
}
