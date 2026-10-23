// Barra de título própria: a janela não tem mais a barra nativa do Windows. Arrastar e o duplo clique vêm do
// atributo data-tauri-drag-region, que vale só para o elemento que o tem; por isso botões e campos não o levam.
import { Children, useEffect, useRef, useState, type ComponentType, type ReactNode, type PointerEvent } from 'react';
import { Flexbox } from '@lobehub/ui';
import { Folder, Plus, X } from 'lucide-react';
import { DropdownMenu, Text, Tooltip, type ContextMenuItem } from '@lobehub/ui/base-ui';
import { SECONDARY } from './text-style';
import { AppMark } from './app-mark';
import { host } from '../backend/backend';
import { CloseIcon, MaximizeIcon, MinimizeIcon, RestoreIcon } from './window-icons';
import { dragShift, revealTab, trackTabDrag, wheelToScroll, type TabDrag } from '../shared/tab-strip';

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

export type ProjectTab = { id: string; path: string; busy?: number };
export type ProjectTabsProps = {
  projects: ProjectTab[];
  active: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onMove: (id: string, index: number) => void;
  onAdd: () => void;
};

/**
 * Marca e menu Arquivo à esquerda; a pasta do projeto no meio (nome em destaque, caminho completo em secundário, copiável);
 * `children` são os contadores, agrupados à direita; os botões da janela ficam colados à borda direita.
 */
export function TitleBar({ connected, project, children, menu, projectTabs, titleActive = true }: { connected: boolean; project?: string; children?: ReactNode; menu?: TitleBarMenu; projectTabs?: ProjectTabsProps; titleActive?: boolean }) {
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
    if (!connected || !titleActive) return;
    void host().setTitle(project ? `${folderName(project)} - Orquestrador` : 'Orquestrador');
  }, [connected, project, titleActive]);

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
      <AppMark className="pointer-events-none mx-2.5 size-4 shrink-0" />
      {menu && (
        <DropdownMenu items={menuItems} placement="bottomLeft">
          <button type="button" className="mr-1 h-[22px] shrink-0 rounded px-2 text-[13px] text-[#cccccc] outline-none hover:bg-[#ffffff1a] focus-visible:ring-2 focus-visible:ring-ring/50">Arquivo</button>
        </DropdownMenu>
      )}
      {projectTabs && <ProjectTabs {...projectTabs} />}
      {!projectTabs && <Flexbox data-tauri-drag-region horizontal align="center" gap={8} flex={1} allowShrink className="min-w-0">
        {project && <>
          <Text as="span" data-tauri-drag-region fontSize={13} weight={500} ellipsis title={project} className="min-w-0 shrink">
            {folderName(project)}
          </Text>
          <Text as="span" fontSize={12} code ellipsis title={project} className="min-w-0 shrink select-text" style={SECONDARY}>
            {project}
          </Text>
        </>}
      </Flexbox>}
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

function ProjectTabs({ projects, active, onSelect, onClose, onMove, onAdd }: ProjectTabsProps) {
  const strip = useRef<HTMLDivElement>(null);
  const tabs = useRef(new Map<string, HTMLDivElement>());
  const [drag, setDrag] = useState<TabDrag | null>(null);
  const [from, setFrom] = useState(-1);
  useEffect(() => { revealTab(strip.current!, tabs.current.get(active ?? '')); }, [active, projects]);
  const begin = (e: PointerEvent<HTMLDivElement>, index: number) => {
    if ((e.target as HTMLElement).closest('button')) return;
    const box = strip.current;
    if (!box) return;
    const nodes = projects.map((item) => tabs.current.get(item.id)!).filter(Boolean);
    trackTabDrag(e, box, nodes, index, {
      onStart: () => setFrom(index), onDrag: setDrag,
      onDrop: (target) => { if (target !== null && target !== index) onMove(projects[index].id, target); setDrag(null); setFrom(-1); },
    });
  };
  // Pílulas nas medidas do botão Arquivo ao lado: 26 px, raio de 6 px, hover branco a 10%. A ativa fica com o fundo e o texto claros;
  // o X aparece nela, no hover e com o foco no botão.
  return (
    <div ref={strip} role="tablist" aria-label="Projetos" data-tauri-drag-region onWheel={wheelToScroll} className="flex h-full min-w-0 flex-1 items-center gap-1 overflow-x-auto px-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      {projects.map((item, index) => {
        const selected = active === item.id;
        return (
          <div key={item.id} ref={(el) => { if (el) tabs.current.set(item.id, el); else tabs.current.delete(item.id); }}
            onPointerDown={(e) => begin(e, index)} onClick={() => onSelect(item.id)} title={item.path}
            role="tab" aria-selected={selected}
            className={`group flex h-[26px] max-w-52 min-w-24 shrink-0 cursor-default items-center gap-1.5 rounded-md pr-1 pl-2 text-[12px] ${selected ? 'bg-[#ffffff1a] text-[#ececec]' : 'text-[#999999] hover:bg-[#ffffff0f] hover:text-[#cccccc]'} ${from === index ? 'z-10 opacity-80' : ''}`}
            style={drag ? { transform: `translateX(${dragShift(drag, from, index)}px)` } : undefined}>
            <Folder className="size-3.5 shrink-0 opacity-70" aria-hidden />
            <span className="min-w-0 flex-1 truncate">{folderName(item.path)}</span>
            {!!item.busy && <span className="size-1.5 shrink-0 rounded-full bg-[var(--ant-color-warning)]" aria-label={`${item.busy} executores ativos`} title={`${item.busy} executores ativos`} />}
            <button
              type="button" aria-label={`Fechar ${folderName(item.path)}`} title="Fechar pasta" onPointerDown={(e) => e.stopPropagation()} onClick={(e) => { e.stopPropagation(); onClose(item.id); }}
              className={`flex size-[18px] shrink-0 items-center justify-center rounded text-[#cccccc] outline-none hover:bg-[#ffffff1a] focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring/50 ${selected ? '' : 'opacity-0 group-hover:opacity-100'}`}
            >
              <X className="size-3" aria-hidden />
            </button>
          </div>
        );
      })}
      <button type="button" aria-label="Abrir pasta" title="Abrir pasta" onClick={onAdd} className="flex size-[26px] shrink-0 items-center justify-center rounded-md text-[#cccccc] outline-none hover:bg-[#ffffff1a] focus-visible:ring-2 focus-visible:ring-ring/50">
        <Plus className="size-4" aria-hidden />
      </button>
    </div>
  );
}
