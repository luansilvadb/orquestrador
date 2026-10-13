// A navegação da coluna de ícones, a árvore de pastas (na tela principal) e a barra de status com a branch.
// A árvore se atualiza sozinha (usePolling). O status do Git vem do App, que o usa no contador da navegação e na barra de status.
import { useLayoutEffect, useRef, useState } from 'react';
import { File, Files, Folder, FolderOpen, GitBranch, type LucideIcon } from 'lucide-react';
import { ActionIcon, Badge, ScrollArea, Text, Tree, type TreeDataNode } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from './backend';
import { usePolling } from './polling';
import type { DirEntry } from './types';

export type Route = 'arquivos' | 'git';

// Caminho relativo com "/" (como o backend); a raiz é ''.
const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
const inside = (dir: string, path: string) => path.startsWith(`${dir}/`);

type ExplorerTree = { children: Record<string, DirEntry[]>; expanded: string[] };
/** Tira uma pasta e tudo o que está dentro dela, aberto ou com lista guardada. */
const drop = (tree: ExplorerTree, path: string): ExplorerTree => ({
  expanded: tree.expanded.filter((p) => p !== path && !inside(path, p)),
  children: Object.fromEntries(Object.entries(tree.children).filter(([p]) => p !== path && !inside(path, p))),
});
const sameList = (a: DirEntry[] | undefined, b: DirEntry[]) =>
  !!a && a.length === b.length && a.every((e, i) => e.name === b[i].name && e.dir === b[i].dir);

// Seleção fica sempre vazia: clicar na linha abre ou fecha a pasta, sem marcar nada.
const NO_SELECTION: string[] = [];

/** Coluna de ícones: uma tela por botão, na ordem Arquivos, Git. O botão da tela aberta tem aria-current. */
export function Nav({ route, changed, onGo }: { route: Route; changed: number; onGo: (to: Route) => void }) {
  const button = (to: Route, label: string, icon: LucideIcon, title = label) => (
    <ActionIcon
      icon={icon} variant="borderless" active={route === to}
      aria-label={label} aria-current={route === to ? 'page' : undefined} title={title}
      onClick={() => onGo(to)}
    />
  );
  const gitLabel = changed ? `Git (${changed} ${changed === 1 ? 'arquivo alterado' : 'arquivos alterados'})` : 'Git';
  return (
    <Flexbox as="nav" aria-label="Telas" flex="none" width={44} align="center" gap={4} paddingBlock={8} className="border-r bg-card">
      {button('arquivos', 'Arquivos', Files)}
      <div className="relative">
        {button('git', gitLabel, GitBranch, 'Git')}
        {changed > 0 && (
          <Badge aria-hidden="true" count={changed} className="pointer-events-none absolute -top-1 -right-1" />
        )}
      </div>
    </Flexbox>
  );
}

/** Árvore de pastas do projeto, à esquerda dos terminais na tela principal. Pasta abre e fecha; clicar em arquivo não faz nada. */
export function FilesScreen() {
  const [tree, setTree] = useState<ExplorerTree>({ children: {}, expanded: [] });
  const [error, setError] = useState('');
  const expanded = useRef<string[]>([]);
  useLayoutEffect(() => {
    expanded.current = tree.expanded;
  });

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

  // O Tree só mostra seta e abre uma pasta que tem filhos. Por isso toda pasta ainda sem lista guardada recebe
  // um filho provisório (não selecionável), e a pasta não parece folha. Ele some quando a lista chega.
  const nodes = (dir: string): TreeDataNode[] =>
    (tree.children[dir] ?? []).map((entry): TreeDataNode => {
      const path = join(dir, entry.name);
      const title = <Text as="span" ellipsis title={entry.name}>{entry.name}</Text>;
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

  return (
    <ScrollArea className="h-full" disableContentFit>
      <Flexbox paddingBlock={8}>
        {error && <Text as="p" role="status" type="secondary" fontSize={12} className="px-3 py-1">{error}</Text>}
        {!tree.children[''] && !error && (
          <Text as="p" type="secondary" fontSize={12} className="px-3 py-1">Lendo a pasta…</Text>
        )}
        <Tree
          treeData={nodes('')} expandedKeys={tree.expanded} selectedKeys={NO_SELECTION}
          indent={16} size="small" showIcon style={{ paddingInline: 8 }}
          onExpand={(keys) => {
            // Abre o que entrou na lista e fecha o que saiu (a tecla * abre vários de uma vez).
            keys.filter((k) => !tree.expanded.includes(k)).forEach((k) => toggle(k, false));
            tree.expanded.filter((k) => !keys.includes(k)).forEach((k) => toggle(k, true));
          }}
          onSelect={(_, { node }) => {
            if (!node.isLeaf) toggle(node.key, tree.expanded.includes(node.key));
          }}
        />
      </Flexbox>
    </ScrollArea>
  );
}

/** Barra de status fina embaixo da tela principal: só a branch atual do projeto. */
export function StatusBar({ branch }: { branch: string | undefined }) {
  return (
    <Flexbox as="footer" horizontal align="center" gap={6} height={24} flex="none" paddingInline={12} className="border-t bg-card">
      {branch && (
        <>
          <GitBranch className="size-3.5 shrink-0 text-muted-foreground" />
          <Text as="span" code fontSize={12} ellipsis title={branch}>{branch}</Text>
        </>
      )}
    </Flexbox>
  );
}
