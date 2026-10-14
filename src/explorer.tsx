// A navegação da coluna de ícones, a árvore de pastas (na tela principal) e a barra de status com a branch e o botão do Terminal.
// A árvore se atualiza sozinha (usePolling). O status do Git vem do App, que o usa no contador da navegação e na barra de status.
// Arquivo da árvore tem menu de contexto (clique direito em qualquer ponto da linha, Shift+F10 e a tecla de contexto); a área vazia do painel tem o menu da raiz do projeto. As ações reais vêm do App pelo `actions`.
import { type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { File, Files, Folder, FolderOpen, GitBranch, type LucideIcon, Search, SquareTerminal } from 'lucide-react';
import {
  ActionIcon, Alert, Badge, Button, type ContextMenuItem, Input, Modal, ScrollArea, showContextMenu, Text, toast, Tree, type TreeDataNode,
} from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from './backend';
import { fileNameError, isWithin } from './file-tab-actions';
import { action, blocked, DIVIDER } from './file-tabs';
import { usePolling } from './polling';
import type { DirEntry } from './types';

export type Route = 'arquivos' | 'pesquisa' | 'git';

/** O que o menu do arquivo pede ao App. Erros chegam como exceção (Renomear e Excluir) ou viram toast no App (o resto). */
export type FileActions = {
  /** Diz se o arquivo tem alteração não salva no editor. Renomear recusa; Excluir avisa antes. */
  dirty: (path: string) => boolean;
  copyPath: (kind: 'relative' | 'absolute', path: string) => void;
  reveal: (path: string) => void;
  openWith: (path: string) => void;
  openInTerminal: (dir: string) => void;
  /** Cola o caminho no rascunho do Orquestrador. `null` quando ele não está rodando (o item fica desabilitado). */
  addToChat: ((path: string) => void) | null;
  copy: (path: string, cut: boolean) => void;
  /** Renomeia na mesma pasta e devolve o caminho novo. Rejeita com a mensagem do motivo, sem mexer em nada. */
  rename: (path: string, name: string) => Promise<string>;
  /** Manda o arquivo para a Lixeira e fecha a aba dele. Rejeita com a mensagem, sem fechar nada. */
  remove: (path: string) => Promise<void>;
  /** Abre a pasta (a raiz, '') no Explorer do Windows. */
  revealDir: (path: string) => void;
  /** Vai para a pesquisa de todo o projeto, com o campo focado. */
  search: () => void;
  /** Cola o clipboard na pasta `dir` (a raiz, por ora). Rejeita com a mensagem. Devolve as origens que o recorte tirou do projeto (vazio na cópia). */
  paste: (dir: string) => Promise<string[]>;
};

// Caminho relativo com "/" (como o backend); a raiz é ''.
const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
const inside = (dir: string, path: string) => path.startsWith(`${dir}/`);
const dirOf = (path: string) => (path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '');
const nameOf = (path: string) => path.slice(path.lastIndexOf('/') + 1);

type ExplorerTree = { children: Record<string, DirEntry[]>; expanded: string[] };
/** Tira uma pasta e tudo o que está dentro dela, aberto ou com lista guardada. */
const drop = (tree: ExplorerTree, path: string): ExplorerTree => ({
  expanded: tree.expanded.filter((p) => p !== path && !inside(path, p)),
  children: Object.fromEntries(Object.entries(tree.children).filter(([p]) => p !== path && !inside(path, p))),
});
const sameList = (a: DirEntry[] | undefined, b: DirEntry[]) =>
  !!a && a.length === b.length && a.every((e, i) => e.name === b[i].name && e.dir === b[i].dir);

// Sem arquivo aberto, nada fica marcado. Pasta abre e fecha sem ficar marcada.
const NO_SELECTION: string[] = [];

/** Coluna de ícones: uma tela por botão, na ordem Arquivos, Pesquisar, Git. O botão da tela aberta tem aria-current e um filete à esquerda. */
export function Nav({ route, changed, onGo }: { route: Route; changed: number; onGo: (to: Route) => void }) {
  // Botão de 36 px com ícone de 20 px (o size numérico já dá essa proporção). A linha ocupa a faixa toda, para o filete encostar na borda.
  const button = (to: Route, label: string, icon: LucideIcon, title = label, badge?: ReactNode) => (
    <Flexbox width="100%" align="center" className="relative">
      {route === to && <span aria-hidden="true" className="absolute inset-y-2 left-0 w-0.5 rounded-full bg-[var(--ant-color-primary)]" />}
      <div className="relative">
        <ActionIcon
          icon={icon} size={20} variant="borderless" active={route === to}
          aria-label={label} aria-current={route === to ? 'page' : undefined} title={title}
          onClick={() => onGo(to)}
        />
        {badge}
      </div>
    </Flexbox>
  );
  const gitLabel = changed ? `Git (${changed} ${changed === 1 ? 'arquivo alterado' : 'arquivos alterados'})` : 'Git';
  // Contador pequeno no canto de baixo à direita do botão. Sem filhos, o Badge não se põe em absoluto sozinho, então a posição vem aqui, sem o deslocamento para fora do canto.
  const gitBadge = changed > 0 && (
    <Badge
      aria-hidden="true" count={changed} size="small" className="pointer-events-none absolute"
      style={{ top: 'auto', bottom: 0, right: 0 }}
    />
  );
  return (
    <Flexbox as="nav" aria-label="Telas" flex="none" width={44} align="center" gap={4} paddingBlock={8} className="border-r bg-card">
      {button('arquivos', 'Arquivos', Files)}
      {button('pesquisa', 'Pesquisar', Search, 'Pesquisar (Ctrl+Shift+F)')}
      {button('git', gitLabel, GitBranch, 'Git', gitBadge)}
    </Flexbox>
  );
}

// Rola só o viewport do ScrollArea (o elemento com overflow: scroll), sem scrollIntoView: ele rolaria também o painel e os ancestrais.
function scrollToNode(box: HTMLElement | null, path: string) {
  const node = box?.querySelector<HTMLElement>(`[data-tree-path="${CSS.escape(path)}"]`);
  if (!box || !node) return;
  const item = node.getBoundingClientRect();
  const area = box.getBoundingClientRect();
  if (item.top < area.top) box.scrollTop -= area.top - item.top;
  else if (item.bottom > area.bottom) box.scrollTop += item.bottom - area.bottom;
}

/** Árvore de pastas do projeto, na faixa à esquerda da tela principal. Pasta abre e fecha; arquivo abre na área do meio e fica marcado. `reveal` abre as pastas de um arquivo e o marca (`id` novo a cada pedido). */
export function FilesScreen({ openPath, onOpen, reveal, actions }: {
  openPath: string | null;
  onOpen: (path: string) => void;
  reveal?: { path: string; id: number } | null;
  actions: FileActions;
}) {
  const [tree, setTree] = useState<ExplorerTree>({ children: {}, expanded: [] });
  const [error, setError] = useState('');
  // Arquivo marcado por "Revelar": pode não ser o aberto. Some quando a aba ativa muda ou quando se abre um arquivo pela árvore.
  const [marked, setMarked] = useState<string | null>(null);
  // Diálogos de Renomear e Excluir: o caminho alvo, o texto digitado (ou o motivo de recusa) e se a ação está rodando.
  const [renaming, setRenaming] = useState<{ path: string; name: string; error: string; busy: boolean } | null>(null);
  const [deleting, setDeleting] = useState<{ path: string; error: string; busy: boolean } | null>(null);
  // Novo arquivo ou nova pasta na raiz: o tipo, o nome digitado, o motivo de recusa e se a criação está rodando.
  const [creating, setCreating] = useState<{ kind: 'file' | 'dir'; name: string; error: string; busy: boolean } | null>(null);
  // Viewport do ScrollArea (o elemento que rola de fato): o Revelar rola este, não os ancestrais.
  const viewport = useRef<HTMLDivElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const expanded = useRef<string[]>([]);
  // Acorde Ctrl+K, como nas abas: vale por 1,5 s, cancela ao sair da árvore e some com o componente.
  const chordPending = useRef(false);
  const chordTimer = useRef(0);
  useLayoutEffect(() => {
    expanded.current = tree.expanded;
  });
  useEffect(() => setMarked(null), [openPath]);
  useEffect(() => () => window.clearTimeout(chordTimer.current), []);

  // Uma rodada consulta a raiz e cada pasta aberta. O estado só muda quando a resposta é diferente.
  // Pasta aberta que não lista mais sai da árvore sem erro na tela; falha na raiz aparece como texto, não como toast.
  usePolling(async () => {
    const paths = ['', ...expanded.current];
    const results = await Promise.all(paths.map((path) =>
      backend.listDir(path).then((list) => ({ list }), (e: unknown) => ({ error: message(e) }))));
    const root = results[0];
    setError('error' in root ? root.error : '');
    setTree((prev) => {
      let next = prev;
      paths.forEach((path, i) => {
        const r = results[i];
        if (!path) {
          if ('list' in r && !sameList(next.children[''], r.list)) next = { ...next, children: { ...next.children, '': r.list } };
        } else if (prev.expanded.includes(path)) {
          // Pasta fechada enquanto a consulta ia: a resposta não se aplica.
          if ('error' in r) next = drop(next, path);
          else if (!sameList(next.children[path], r.list)) next = { ...next, children: { ...next.children, [path]: r.list } };
        }
      });
      return next;
    });
  });

  async function toggle(path: string, isOpen: boolean) {
    // Fechar só tira a pasta (e as de dentro) da lista de abertas: o conteúdo fica até a animação terminar.
    if (isOpen) return setTree((prev) => ({ ...prev, expanded: prev.expanded.filter((p) => p !== path && !inside(path, p)) }));
    await openDir(path);
  }

  async function openDir(path: string) {
    // A lista chega antes da pasta abrir, para o painel medir a altura já com o conteúdo.
    try {
      const list = await backend.listDir(path);
      setTree((prev) => ({
        children: { ...prev.children, [path]: list },
        expanded: prev.expanded.includes(path) ? prev.expanded : [...prev.expanded, path],
      }));
    } catch {
      // A pasta sumiu antes de abrir: sai da árvore sem erro na tela, como na atualização.
      setTree((prev) => drop(prev, path));
    }
  }

  // Relê uma pasta depois de uma ação dela (renomear, excluir), sem abrir a pasta se ela estava fechada.
  async function reloadFolder(dir: string) {
    try {
      const list = await backend.listDir(dir);
      setTree((prev) => (dir === '' || prev.expanded.includes(dir)
        ? { ...prev, children: { ...prev.children, [dir]: list } }
        : prev));
    } catch {
      // O polling mostra o que sumiu.
    }
  }

  // Depois de colar: relê a raiz e as pastas abertas, porque um recorte tira itens de onde estavam.
  async function reloadAll() {
    await Promise.all(['', ...expanded.current].map(reloadFolder));
  }

  // Pedido de revelar: abre as pastas de cima para baixo (cada uma precisa da lista antes), marca o arquivo e rola até ele.
  useEffect(() => {
    if (!reveal) return;
    let live = true;
    void (async () => {
      const parts = reveal.path.split('/');
      for (let i = 1; i < parts.length; i++) {
        if (!live) return;
        await openDir(parts.slice(0, i).join('/'));
      }
      if (!live) return;
      setMarked(reveal.path);
      // Depois da renderização que trouxe os nós abertos.
      requestAnimationFrame(() => scrollToNode(viewport.current, reveal.path));
    })();
    return () => {
      live = false;
    };
  }, [reveal]);

  // Arquivo de verdade na lista atual. Pasta, nó provisório ("Lendo a pasta…") e caminho que sumiu ficam de fora.
  const isFile = (path: string) => {
    const dir = dirOf(path);
    return (tree.children[dir] ?? []).some((entry) => !entry.dir && entry.name === nameOf(path));
  };

  // ---------- menu do arquivo e atalhos ----------

  const startRename = (path: string) => setRenaming({ path, name: nameOf(path), error: '', busy: false });
  const startDelete = (path: string) => setDeleting({ path, error: '', busy: false });
  const startCreate = (kind: 'file' | 'dir') => setCreating({ kind, name: '', error: '', busy: false });

  // Um item por ação real. Chat sem Orquestrador fica desabilitado, em uma linha: o motivo vai no title e no aria-description.
  const fileMenu = (path: string): ContextMenuItem[] => {
    const chatReason = 'Abra o Orquestrador para mandar o arquivo ao chat.';
    const sections: ContextMenuItem[][] = [
      [
        action('open-with', 'Abrir com...', () => actions.openWith(path)),
        action('reveal', 'Revelar no Explorador', () => actions.reveal(path), 'Shift+Alt+R'),
        action('terminal', 'Abrir no Terminal', () => actions.openInTerminal(dirOf(path))),
      ],
      [actions.addToChat
        ? action('chat', 'Adicionar ao Chat', () => actions.addToChat?.(path))
        : { key: 'chat', disabled: true, label: <span title={chatReason} aria-description={chatReason}>Adicionar ao Chat</span> }],
      [
        action('cut', 'Recortar', () => actions.copy(path, true), 'Ctrl+X'),
        action('copy', 'Copiar', () => actions.copy(path, false), 'Ctrl+C'),
      ],
      [
        action('copy-path', 'Copiar Caminho', () => actions.copyPath('absolute', path), 'Shift+Alt+C'),
        action('copy-relative', 'Copiar Caminho Relativo', () => actions.copyPath('relative', path), 'Ctrl+K Ctrl+Shift+C'),
      ],
      [
        action('rename', 'Renomear...', () => startRename(path), 'F2'),
        action('delete', 'Excluir', () => startDelete(path), 'Del'),
      ],
    ];
    return sections.flatMap((items, i) => (i ? [DIVIDER, ...items] : items));
  };

  const beginChord = () => {
    chordPending.current = true;
    window.clearTimeout(chordTimer.current);
    chordTimer.current = window.setTimeout(() => { chordPending.current = false; }, 1500);
  };
  const endChord = () => {
    chordPending.current = false;
    window.clearTimeout(chordTimer.current);
  };

  // Ações da raiz (área vazia do painel, abaixo das linhas): sempre na raiz do projeto, nunca no arquivo marcado.
  // Em quatro grupos, com separador. Colar sem arquivo no clipboard e Adicionar Pasta ao Chat sem o Orquestrador ficam desabilitados.
  const rootMenu = (hasFiles: boolean): ContextMenuItem[] => {
    const chatReason = 'Abra o Orquestrador para mandar a pasta ao chat.';
    return [
      action('root-new-file', 'Novo Arquivo...', () => startCreate('file')),
      action('root-new-dir', 'Nova Pasta...', () => startCreate('dir')),
      DIVIDER,
      action('root-reveal', 'Revelar no Explorador de Arquivos', () => actions.revealDir(''), 'Shift+Alt+R'),
      action('root-search', 'Localizar na Pasta...', () => actions.search(), 'Shift+Alt+F'),
      DIVIDER,
      actions.addToChat
        ? action('root-chat', 'Adicionar Pasta ao Chat', () => actions.addToChat?.(''))
        : { key: 'root-chat', disabled: true, label: <span title={chatReason} aria-description={chatReason}>Adicionar Pasta ao Chat</span> },
      hasFiles
        ? action('root-paste', 'Colar', () => void pasteInto(''), 'Ctrl+V')
        : blocked('root-paste', 'Colar', 'Não há arquivo ou pasta copiado.'),
      DIVIDER,
      action('root-copy-path', 'Copiar o Caminho', () => actions.copyPath('absolute', ''), 'Shift+Alt+C'),
      action('root-copy-relative', 'Copiar Caminho Relativo', () => actions.copyPath('relative', ''), 'Ctrl+K Ctrl+Shift+C'),
    ];
  };

  // Abre o menu da raiz. O Colar só fica habilitado se o clipboard tem arquivo: confere agora, ao abrir o menu, sem escuta nem consulta permanente.
  const openRootMenu = async () => {
    const hasFiles = await backend.clipboardSnapshot().then((s) => s.paths.length > 0, () => false);
    showContextMenu(rootMenu(hasFiles));
  };

  // Colar na raiz: a árvore relê depois do sucesso; erro vira toast. Recorte tira o item da árvore de onde estava, então a marcação sai se era dele.
  async function pasteInto(dir: string) {
    try {
      const moved = await actions.paste(dir);
      setMarked((prev) => (prev && moved.some((m) => isWithin(prev, m)) ? null : prev));
      await reloadAll();
      toast.success({ title: moved.length ? 'Itens movidos para a raiz do projeto' : 'Itens colados na raiz do projeto', duration: 3000 });
    } catch (e) {
      // Um recorte que falhou no meio pode ter movido alguns itens: a árvore relê mesmo assim.
      await reloadAll();
      toast.error({ title: message(e), duration: 7000 });
    }
  }

  // Criar: o nome é checado aqui para mostrar o motivo; o backend confere de novo e nunca sobrescreve. Erro fica no diálogo.
  // Arquivo novo abre no editor; pasta nova só aparece na árvore, sem fechar as abas.
  async function submitCreate() {
    if (!creating || creating.busy) return;
    const { kind, name } = creating;
    const problem = fileNameError(name);
    if (problem) return setCreating({ ...creating, error: problem });
    setCreating({ ...creating, error: '', busy: true });
    try {
      const created = kind === 'file' ? await backend.createFile('', name) : await backend.createDir('', name);
      setCreating(null);
      await reloadFolder('');
      if (kind === 'file') onOpen(created);
      toast.success({ title: kind === 'file' ? 'Arquivo criado' : 'Pasta criada', duration: 3000 });
    } catch (e) {
      setCreating((prev) => prev && { ...prev, busy: false, error: message(e) });
    }
  }

  // Clique direito no painel inteiro: suprime o menu do WebView. Linha da árvore (arquivo ou pasta) fica com o `onRightClick` do Tree;
  // pasta não recebe menu da raiz. Só a área vazia abre o menu da raiz.
  const onPanelContextMenu = (e: ReactMouseEvent<HTMLElement>) => {
    e.preventDefault();
    if ((e.target as HTMLElement).closest('[role="treeitem"]')) return;
    void openRootMenu();
  };

  // Teclas só com a linha de arquivo focada na árvore. Fora da árvore (campos, terminais, modais) nada é capturado.
  // Shift+F10 e a tecla de contexto abrem o menu disparando o `contextmenu` na linha: o mesmo caminho do clique direito.
  // Com o foco na área vazia do painel (sem linha), a mesma tecla abre o menu da raiz.
  const onTreeKey = (e: ReactKeyboardEvent<HTMLElement>) => {
    const row = (e.target as HTMLElement).closest<HTMLElement>('[role="treeitem"]');
    if (!row && ((e.shiftKey && e.key === 'F10') || e.key === 'ContextMenu')) {
      const rect = e.currentTarget.getBoundingClientRect();
      e.currentTarget.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rect.left + 24, clientY: rect.top + 24 }));
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    // Linha de pasta ou nó provisório não tem tecla. Linha de arquivo tem as dela. Sem linha (área vazia do painel), as da raiz: `path` vazio.
    if (row && (!row.dataset.id || !isFile(row.dataset.id))) return;
    // Modificador sozinho não conta como tecla: Ctrl+K, soltar, e depois Ctrl+Shift+C ainda completa o acorde (como no onFileKey do App).
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
    const path = row ? row.dataset.id! : '';
    const ctrl = e.ctrlKey && !e.altKey && !e.metaKey;
    if (chordPending.current) {
      if (ctrl && !e.shiftKey && e.code === 'KeyK') {
        e.preventDefault();
        e.stopPropagation();
        beginChord();
        return;
      }
      endChord();
      if (ctrl && e.shiftKey && e.code === 'KeyC') {
        e.preventDefault();
        e.stopPropagation();
        actions.copyPath('relative', path);
      }
      return;
    }
    const plain = !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey;
    const altShift = e.shiftKey && e.altKey && !e.ctrlKey && !e.metaKey;
    if (ctrl && !e.shiftKey && e.code === 'KeyK') beginChord();
    else if (!row) {
      // Área vazia: só as ações da raiz. Outras teclas (e Ctrl+V com foco numa linha) seguem sem ser capturadas.
      if (ctrl && !e.shiftKey && e.code === 'KeyV') void pasteInto('');
      else if (altShift && e.code === 'KeyR') actions.revealDir('');
      else if (altShift && e.code === 'KeyF') actions.search();
      else if (altShift && e.code === 'KeyC') actions.copyPath('absolute', '');
      else return;
    } else if (ctrl && !e.shiftKey && e.code === 'KeyX') actions.copy(path, true);
    else if (ctrl && !e.shiftKey && e.code === 'KeyC') actions.copy(path, false);
    else if (plain && e.key === 'F2') startRename(path);
    else if (plain && e.key === 'Delete') startDelete(path);
    else if (altShift && e.code === 'KeyR') actions.reveal(path);
    else if (altShift && e.code === 'KeyC') actions.copyPath('absolute', path);
    else if ((e.shiftKey && e.key === 'F10') || e.key === 'ContextMenu') {
      const rect = row.getBoundingClientRect();
      row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rect.left + 24, clientY: rect.top + rect.height / 2 }));
    } else return;
    e.preventDefault();
    e.stopPropagation();
  };

  // Renomear: valida aqui para mostrar o motivo; o App e o backend conferem de novo (dirty, colisão, nome).
  async function submitRename() {
    if (!renaming || renaming.busy) return;
    const { path, name } = renaming;
    if (name === nameOf(path)) return setRenaming(null);
    const problem = fileNameError(name);
    if (problem) return setRenaming({ ...renaming, error: problem });
    setRenaming({ ...renaming, error: '', busy: true });
    try {
      const next = await actions.rename(path, name);
      setRenaming(null);
      setMarked((prev) => (prev === path ? next : prev));
      await reloadFolder(dirOf(path));
      toast.success({ title: 'Arquivo renomeado', duration: 3000 });
    } catch (e) {
      setRenaming((prev) => prev && { ...prev, busy: false, error: message(e) });
    }
  }

  async function submitDelete() {
    if (!deleting || deleting.busy) return;
    const { path } = deleting;
    setDeleting({ ...deleting, busy: true, error: '' });
    try {
      await actions.remove(path);
      setDeleting(null);
      setMarked((prev) => (prev === path ? null : prev));
      await reloadFolder(dirOf(path));
      toast.success({ title: 'Arquivo enviado para a Lixeira', duration: 3000 });
    } catch (e) {
      setDeleting((prev) => prev && { ...prev, busy: false, error: message(e) });
    }
  }

  // O Tree só mostra seta e abre uma pasta que tem filhos. Por isso toda pasta ainda sem lista guardada recebe
  // um filho provisório (não selecionável), e a pasta não parece folha. Ele some quando a lista chega.
  const nodes = (dir: string): TreeDataNode[] =>
    (tree.children[dir] ?? []).map((entry): TreeDataNode => {
      const path = join(dir, entry.name);
      const title = <Text as="span" ellipsis title={entry.name} data-tree-path={path}>{entry.name}</Text>;
      if (!entry.dir) {
        return { key: path, title, isLeaf: true, icon: <File className="size-4 text-muted-foreground" /> };
      }
      const list = tree.children[path];
      return {
        key: path,
        title,
        icon: tree.expanded.includes(path) ? <FolderOpen className="size-4" /> : <Folder className="size-4" />,
        children: list ? nodes(path) : [{
          key: `${path}/`,
          title: <Text as="span" type="secondary" fontSize={12}>Lendo a pasta…</Text>,
          isLeaf: true,
          selectable: false,
        }],
      };
    });

  const selected = marked ?? openPath;
  const renameProblem = renaming && (renaming.error || fileNameError(renaming.name));
  return (
    <>
      {/* Cobre o painel inteiro: tabIndex deixa o clique na área vazia focar o painel (sem anel, para não parecer um campo). */}
      <Flexbox
        height="100%"
        tabIndex={-1}
        className="outline-none"
        onKeyDown={onTreeKey}
        onContextMenu={onPanelContextMenu}
        onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) endChord(); }}
      >
        <ScrollArea className="h-full" disableContentFit viewportProps={{ ref: viewport }}>
          <Flexbox ref={root} paddingBlock={8}>
            {error && <Text as="p" role="status" type="secondary" fontSize={12} className="px-3 py-1">{error}</Text>}
            {!tree.children[''] && !error && (
              <Text as="p" type="secondary" fontSize={12} className="px-3 py-1">Lendo a pasta…</Text>
            )}
            <Tree
              treeData={nodes('')} expandedKeys={tree.expanded} selectedKeys={selected ? [selected] : NO_SELECTION}
              indent={16} size="small" showIcon blockNode style={{ paddingInline: 8 }}
              onExpand={(keys) => {
                // Abre o que entrou na lista e fecha o que saiu (a tecla * abre vários de uma vez).
                keys.filter((k) => !tree.expanded.includes(k)).forEach((k) => toggle(k, false));
                tree.expanded.filter((k) => !keys.includes(k)).forEach((k) => toggle(k, true));
              }}
              onSelect={(_, { node }) => {
                if (node.isLeaf) {
                  setMarked(null);
                  onOpen(node.key);
                } else {
                  toggle(node.key, tree.expanded.includes(node.key));
                }
              }}
              onRightClick={({ event, node }) => {
                // Sem o menu do navegador: pasta e nó provisório não abrem nenhum; arquivo abre o do app, sem abrir o arquivo.
                event.preventDefault();
                if (isFile(node.key)) showContextMenu(fileMenu(node.key));
              }}
            />
          </Flexbox>
        </ScrollArea>
      </Flexbox>

      {/* Renomear: só o nome dentro da pasta. Enter confirma; Cancelar e Esc não mexem em nada. */}
      <Modal
        open={!!renaming}
        title="Renomear arquivo"
        closable={false}
        keyboard
        maskClosable
        onCancel={() => !renaming?.busy && setRenaming(null)}
        footer={(
          <Flexbox horizontal justify="flex-end" gap={8}>
            <Button onClick={() => setRenaming(null)} disabled={renaming?.busy}>Cancelar</Button>
            <Button type="primary" onClick={submitRename} disabled={!!renameProblem || renaming?.busy} loading={renaming?.busy}>
              Renomear
            </Button>
          </Flexbox>
        )}
      >
        {renaming && (
          <Flexbox gap={8}>
            <Text as="p" type="secondary" fontSize={12} code ellipsis title={renaming.path}>{renaming.path}</Text>
            <Input
              autoFocus aria-label="Novo nome do arquivo" value={renaming.name} disabled={renaming.busy}
              onChange={(e) => setRenaming((prev) => prev && { ...prev, name: e.target.value, error: '' })}
              onFocus={(e) => e.currentTarget.select()}
              onPressEnter={submitRename}
            />
            {renameProblem && <Text as="p" type="danger" fontSize={12} role="alert">{renameProblem}</Text>}
          </Flexbox>
        )}
      </Modal>

      {/* Novo arquivo e Nova pasta, na raiz. Enter cria; Cancelar e Esc não criam nada. Nome que existe é recusado pelo backend, sem sobrescrever. */}
      <Modal
        open={!!creating}
        title={creating?.kind === 'dir' ? 'Nova pasta' : 'Novo arquivo'}
        closable={false}
        keyboard
        maskClosable
        onCancel={() => !creating?.busy && setCreating(null)}
        footer={(
          <Flexbox horizontal justify="flex-end" gap={8}>
            <Button onClick={() => setCreating(null)} disabled={creating?.busy}>Cancelar</Button>
            <Button type="primary" onClick={submitCreate} disabled={creating?.busy} loading={creating?.busy}>
              Criar
            </Button>
          </Flexbox>
        )}
      >
        {creating && (
          <Flexbox gap={8}>
            <Text as="p" type="secondary" fontSize={12}>Será criado na raiz do projeto.</Text>
            <Input
              autoFocus aria-label={creating.kind === 'dir' ? 'Nome da nova pasta' : 'Nome do novo arquivo'} value={creating.name} disabled={creating.busy}
              placeholder={creating.kind === 'dir' ? 'Nome da pasta' : 'Nome do arquivo, com extensão'}
              onChange={(e) => setCreating((prev) => prev && { ...prev, name: e.target.value, error: '' })}
              onPressEnter={submitCreate}
            />
            {creating.error && <Text as="p" type="danger" fontSize={12} role="alert">{creating.error}</Text>}
          </Flexbox>
        )}
      </Modal>

      {/* Excluir: vai para a Lixeira do Windows. Com alteração não salva, o aviso diz que ela se perde e a aba fecha. */}
      <Modal
        open={!!deleting}
        title="Excluir arquivo?"
        closable={false}
        keyboard
        maskClosable
        onCancel={() => !deleting?.busy && setDeleting(null)}
        footer={(
          <Flexbox horizontal justify="flex-end" gap={8}>
            <Button onClick={() => setDeleting(null)} disabled={deleting?.busy}>Cancelar</Button>
            <Button type="primary" danger onClick={submitDelete} disabled={deleting?.busy} loading={deleting?.busy}>
              Mover para a Lixeira
            </Button>
          </Flexbox>
        )}
      >
        {deleting && (
          <Flexbox gap={8}>
            <Text as="p" fontSize={14}>O arquivo vai para a Lixeira do Windows:</Text>
            <Text as="p" code fontSize={12} ellipsis title={deleting.path}>{deleting.path}</Text>
            {actions.dirty(deleting.path) && (
              <Alert type="warning" description="Há alterações não salvas neste arquivo. Elas se perdem ao excluir, e a aba dele fecha." />
            )}
            {deleting.error && <Alert type="error" role="alert" description={deleting.error} className="select-text" />}
          </Flexbox>
        )}
      </Modal>
    </>
  );
}

/** Barra de status fina embaixo da tela principal: a branch atual do projeto e o botão do painel Terminal (marcado quando aberto). */
export function StatusBar({ branch, terminalOpen, onToggleTerminal }: {
  branch: string | undefined;
  terminalOpen: boolean;
  onToggleTerminal: () => void;
}) {
  return (
    <Flexbox as="footer" horizontal align="center" gap={6} height={25} flex="none" paddingInline={12} className="border-t bg-card">
      {branch && (
        <>
          <GitBranch className="size-3.5 shrink-0 text-muted-foreground" />
          <Text as="span" code fontSize={12} ellipsis title={branch}>{branch}</Text>
        </>
      )}
      <Button
        type="text" size="small" icon={SquareTerminal} aria-pressed={terminalOpen}
        title="Painel Terminal (Ctrl+`)" className={`ml-auto shrink-0${terminalOpen ? ' bg-muted' : ''}`}
        onClick={onToggleTerminal}
      >
        Terminal
      </Button>
    </Flexbox>
  );
}
