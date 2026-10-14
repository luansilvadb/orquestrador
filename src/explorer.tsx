// A navegação da coluna de ícones, a árvore de pastas (na tela principal) e a barra de status com a branch e o botão do Terminal.
// A árvore se atualiza sozinha (usePolling). O status do Git vem do App, que o usa no contador da navegação e na barra de status.
// Arquivo e pasta da árvore têm menu de contexto (clique direito, Shift+F10 e a tecla de contexto); a área vazia do painel tem o menu da raiz do projeto.
// Seleção com Ctrl e Shift; arrastar e soltar move para pasta ou para a raiz, com o destino mostrado e validado. As ações reais vêm do App pelo `actions`.
import { type DragEvent as ReactDragEvent, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { File, Files, Folder, FolderOpen, GitBranch, type LucideIcon, Search, SquareTerminal } from 'lucide-react';
import {
  ActionIcon, Alert, Badge, Button, type ContextMenuItem, Input, Modal, ScrollArea, showContextMenu, Text, toast, Tree, type TreeDataNode,
} from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from './backend';
import { fileNameError } from './file-tab-actions';
import { action, blocked, DIVIDER } from './file-tabs';
import { usePolling } from './polling';
import type { DirEntry, Moved } from './types';

export type Route = 'arquivos' | 'pesquisa' | 'git';

/** Opções de abertura. Sem elas, o clique simples abre como prévia (a aba seguinte substitui). `permanent`: a aba fica. `side`: abre na área da direita. */
export type OpenOptions = { permanent?: boolean; side?: boolean };

/** Falha de uma ação em lote: `done` são os itens que já foram feitos (não são desfeitos) e `message` diz o que parou. Quem chama reconcilia com `done`. */
export class BatchError<T> extends Error {
  readonly done: T[];
  constructor(text: string, done: T[]) {
    super(text);
    this.name = 'BatchError';
    this.done = done;
  }
}

/** O que o menu e os atalhos da árvore pedem ao App. Erros chegam como exceção (Renomear, Excluir, Mover, Colar) ou viram toast no App (o resto). */
export type FileActions = {
  /** Dos caminhos dados (arquivos ou pastas), os que têm alteração não salva, pastas incluídas. Renomear recusa; Excluir avisa antes. */
  dirtyOf: (paths: string[]) => string[];
  /** Copia os caminhos de uma lista, um por linha, no formato absoluto ou relativo. Lista com um caminho só copia esse. */
  copyPaths: (kind: 'relative' | 'absolute', paths: string[]) => void;
  reveal: (path: string) => void;
  openWith: (path: string) => void;
  openInTerminal: (dir: string) => void;
  /** Cola o caminho no rascunho do Orquestrador. `null` quando ele não está rodando (o item fica desabilitado). */
  addToChat: ((path: string) => void) | null;
  /** Copia ou recorta um ou vários itens (arquivos ou pastas), por caminho. */
  copyMany: (paths: string[], cut: boolean) => void;
  /** Renomeia um arquivo ou uma pasta na mesma pasta e devolve o caminho novo. Rejeita com a mensagem do motivo, sem mexer em nada. */
  rename: (path: string, name: string) => Promise<string>;
  /** Manda os itens (pastas incluídas, com o que está dentro) para a Lixeira e fecha as abas deles. Resolve com os caminhos feitos; se parar no meio, rejeita com `BatchError` (`done` = feitos). */
  removeMany: (paths: string[]) => Promise<string[]>;
  /** Move itens para a pasta `dest` ('' é a raiz), relativos ao projeto. Resolve com `{from, to}` de cada um; parada no meio rejeita com `BatchError<Moved>`. */
  move: (paths: string[], dest: string) => Promise<Moved[]>;
  /** Abre a pasta (a raiz, '') no Explorer do Windows. */
  revealDir: (path: string) => void;
  /** Abre o histórico Git do arquivo ou da pasta. */
  history: (path: string) => void;
  /** Vai para a pesquisa de todo o projeto (`dir` vazio ou omitido) ou da pasta `dir` (relativa, com "/"), com o campo focado. */
  search: (dir?: string) => void;
  /** Cola o clipboard na pasta `dir` ('' é a raiz). Rejeita com a mensagem. Devolve as origens que o recorte tirou do projeto (vazio na cópia). */
  paste: (dir: string) => Promise<string[]>;
};

// Caminho relativo com "/" (como o backend); a raiz é ''.
const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
const inside = (dir: string, path: string) => path.startsWith(`${dir}/`);
const under = (base: string, path: string) => path === base || inside(base, path);
const dirOf = (path: string) => (path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '');
const nameOf = (path: string) => path.slice(path.lastIndexOf('/') + 1);
// Nó provisório ("Lendo a pasta…") tem a chave `<pasta>/`. Nenhum caminho real termina com barra.
const provisional = (key: string) => key.endsWith('/');
const where = (dest: string) => (dest ? `“${dest}”` : 'a raiz do projeto');

type ExplorerTree = { children: Record<string, DirEntry[]>; expanded: string[] };
/** Tira uma pasta e tudo o que está dentro dela, aberto ou com lista guardada. */
const drop = (tree: ExplorerTree, path: string): ExplorerTree => ({
  expanded: tree.expanded.filter((p) => !under(path, p)),
  children: Object.fromEntries(Object.entries(tree.children).filter(([p]) => !under(path, p))),
});
const sameList = (a: DirEntry[] | undefined, b: DirEntry[]) =>
  !!a && a.length === b.length && a.every((e, i) => e.name === b[i].name && e.dir === b[i].dir);

/** Leva um caminho para onde o item (ou a pasta dele) foi parar. Caminho que não foi movido fica como está. */
const remapPath = (path: string, moves: Moved[]) => {
  const hit = moves.find((m) => under(m.from, path));
  return hit ? hit.to + path.slice(hit.from.length) : path;
};

/** Motivo para soltar `paths` em `dest`, ou null se pode. Pasta não vai para si nem para dentro dela; item já na pasta não tem o que mover. */
function dropProblem(paths: string[], dest: string): string | null {
  const into = paths.find((p) => under(p, dest));
  if (into) return `Não dá para mover “${nameOf(into)}” para dentro dele mesmo.`;
  if (paths.every((p) => dirOf(p) === dest)) return dest ? 'Os itens já estão nesta pasta.' : 'Os itens já estão na raiz do projeto.';
  return null;
}

// Sem arquivo aberto nem seleção, nada fica marcado. Pasta abre e fecha sem ficar marcada.
const NONE: string[] = [];
const NO_MODS = { ctrl: false, shift: false };

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

type Renaming = { path: string; name: string; error: string; busy: boolean };
type Deleting = { paths: string[]; error: string; busy: boolean };
type Creating = { kind: 'file' | 'dir'; dir: string; name: string; error: string; busy: boolean };
type Drop = { dest: string; problem: string | null };

// Quantos itens o diálogo de excluir lista antes de resumir.
const LIST_MAX = 10;

// Casco dos diálogos de formulário (Renomear, Criar e Excluir): Esc, clique fora e Cancelar não fecham com o pedido rodando, e o botão de confirmar mostra o carregamento.
function FormDialog({ open, title, busy, confirmLabel, confirmDisabled = false, danger = false, onClose, onConfirm, children }: {
  open: boolean;
  title: string;
  busy: boolean;
  confirmLabel: string;
  /** Desabilita o confirmar além de `busy` (ex.: nome inválido). */
  confirmDisabled?: boolean;
  danger?: boolean;
  onClose: () => void;
  onConfirm: () => void;
  children: ReactNode;
}) {
  return (
    <Modal
      open={open}
      title={title}
      closable={false}
      keyboard
      maskClosable
      onCancel={() => !busy && onClose()}
      footer={(
        <Flexbox horizontal justify="flex-end" gap={8}>
          <Button onClick={onClose} disabled={busy}>Cancelar</Button>
          <Button type="primary" danger={danger} onClick={onConfirm} disabled={confirmDisabled || busy} loading={busy}>
            {confirmLabel}
          </Button>
        </Flexbox>
      )}
    >
      {children}
    </Modal>
  );
}

/** Árvore de pastas do projeto, na faixa à esquerda da tela principal. Pasta abre e fecha; arquivo abre na área do meio (prévia no clique, permanente no duplo clique e no Enter).
 * `reveal` abre as pastas de um arquivo e o seleciona (`id` novo a cada pedido). */
export function FilesScreen({ openPath, onOpen, reveal, actions }: {
  openPath: string | null;
  onOpen: (path: string, options?: OpenOptions) => void;
  reveal?: { path: string; id: number } | null;
  actions: FileActions;
}) {
  const [tree, setTree] = useState<ExplorerTree>({ children: {}, expanded: [] });
  const [error, setError] = useState('');
  // Seleção própria da árvore. Vazia, o arquivo aberto aparece marcado.
  const [selection, setSelection] = useState<string[]>([]);
  // Diálogos de Renomear, Excluir e Criar. Cada um guarda o que o usuário digitou (ou o motivo de recusa) e se a ação está rodando.
  const [renaming, setRenaming] = useState<Renaming | null>(null);
  const [deleting, setDeleting] = useState<Deleting | null>(null);
  const [creating, setCreating] = useState<Creating | null>(null);
  // Arraste em curso: o destino sob o ponteiro e o motivo de não poder soltar ali.
  const [dragOver, setDrop] = useState<Drop | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  // Viewport do ScrollArea (o elemento que rola de fato): o Revelar rola este, não os ancestrais.
  const viewport = useRef<HTMLDivElement>(null);
  const expanded = useRef<string[]>([]);
  const dragging = useRef<string[] | null>(null);
  // Modificadores do último clique ou tecla. O Tree não entrega o evento ao `onSelect`, então ele é lido aqui.
  const mods = useRef(NO_MODS);
  // Acorde Ctrl+K, como nas abas: vale por 1,5 s, cancela ao sair da árvore e some com o componente.
  const chordPending = useRef(false);
  const chordTimer = useRef(0);
  useLayoutEffect(() => {
    expanded.current = tree.expanded;
  });
  // Trocar de aba limpa a seleção: a marcação volta a ser a do arquivo aberto.
  useEffect(() => setSelection([]), [openPath]);
  useEffect(() => () => window.clearTimeout(chordTimer.current), []);
  // Linhas: arrastáveis quando há como mover, e a pasta que recebe o arraste ganha a moldura (vermelha se não pode). Roda a cada render, porque o Tree recria as linhas.
  useLayoutEffect(() => {
    panel.current?.querySelectorAll<HTMLElement>('[role="treeitem"][data-id]').forEach((row) => {
      const key = row.dataset.id ?? '';
      const hit = !!dragOver && !provisional(key) && dragOver.dest === key;
      row.draggable = !provisional(key);
      row.style.boxShadow = hit ? `inset 0 0 0 1px var(--ant-color-${dragOver?.problem ? 'error' : 'primary'})` : '';
    });
  });

  // Uma rodada consulta a raiz e cada pasta aberta. O estado só muda quando a resposta é diferente.
  // Pasta aberta que não lista mais sai da árvore sem erro na tela; falha na raiz aparece como texto, não como toast.
  usePolling(async () => {
    const paths = ['', ...expanded.current];
    const results = await Promise.all(paths.map((path) =>
      backend.listDir(path).then((list) => ({ list }), (e: unknown) => ({ error: message(e) }))));
    const rootResult = results[0];
    setError('error' in rootResult ? rootResult.error : '');
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
    if (isOpen) return setTree((prev) => ({ ...prev, expanded: prev.expanded.filter((p) => !under(path, p)) }));
    await openDir(path);
  }

  async function openDir(path: string) {
    // A lista chega antes da pasta abrir, para o painel medir a altura já com o conteúdo.
    try {
      const list = await backend.listDir(path);
      setTree((prev) => ({
        children: { ...prev.children, [path]: list },
        expanded: prev.expanded.includes(path) || !path ? prev.expanded : [...prev.expanded, path],
      }));
    } catch {
      // A pasta sumiu antes de abrir: sai da árvore sem erro na tela, como na atualização.
      setTree((prev) => drop(prev, path));
    }
  }

  // Relê uma pasta depois de uma ação dela (renomear, excluir, criar), sem abrir a pasta se ela estava fechada.
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

  // Depois de uma ação em lote, com ou sem erro: tira do estado o que saiu do lugar (`gone`), leva a seleção para onde os itens foram
  // (`moves`) e relê as pastas envolvidas (`dirs`). Só o que o backend devolveu como feito entra em `gone` e `moves`.
  function settle(dirs: string[], gone: string[], moves: Moved[] = []) {
    gone.forEach((path) => setTree((prev) => drop(prev, path)));
    setSelection((prev) => prev.map((p) => remapPath(p, moves)).filter((p) => !gone.some((g) => under(g, p))));
    new Set(['', ...dirs]).forEach((dir) => void reloadFolder(dir));
  }

  // Depois de colar: relê a raiz e as pastas abertas, porque um recorte tira itens de onde estavam.
  async function reloadAll() {
    await Promise.all(['', ...expanded.current].map(reloadFolder));
  }

  // Pedido de revelar: abre as pastas de cima para baixo (cada uma precisa da lista antes), seleciona o arquivo e rola até ele.
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
      setSelection([reveal.path]);
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

  // Marcação exibida: a seleção, ou o arquivo aberto quando não há seleção.
  const shown = selection.length ? selection : openPath ? [openPath] : NONE;

  // ---------- menus e atalhos ----------

  const startRename = (path: string) => {
    if (selection.length > 1) return toast.warning({ title: 'Renomeie um item de cada vez.', duration: 3000 });
    setRenaming({ path, name: nameOf(path), error: '', busy: false });
  };
  const startDelete = (paths: string[]) => setDeleting({ paths, error: '', busy: false });
  const startCreate = (kind: 'file' | 'dir', dir: string) => setCreating({ kind, dir, name: '', error: '', busy: false });

  // Item (ou seleção) da árvore: uma ação real por item de menu. Vários itens: as que só servem a um ficam desabilitadas com o motivo.
  const itemMenu = (paths: string[], hasFiles: boolean): ContextMenuItem[] => {
    const many = paths.length > 1;
    const one = paths[0];
    const folder = !many && !isFile(one);
    const single = (key: string, label: string, run: () => void, shortcut?: string) =>
      (many ? blocked(key, label, 'Escolha só um item para esta ação.') : action(key, label, run, shortcut));
    const chat = actions.addToChat;
    const chatLabel = folder ? 'Adicionar Pasta ao Chat' : 'Adicionar ao Chat';
    const chatReason = 'Abra o Orquestrador para mandar ao chat.';
    const pasteDir = folder ? one : dirOf(one);
    const sections: ContextMenuItem[][] = [];
    if (folder) {
      sections.push([
        action('new-file', 'Novo Arquivo...', () => startCreate('file', one)),
        action('new-dir', 'Nova Pasta...', () => startCreate('dir', one)),
      ]);
    }
    sections.push([
      ...(folder ? [] : [
        single('open-with', 'Abrir com...', () => actions.openWith(one)),
        single('open-side', 'Abrir à Direita', () => onOpen(one, { side: true })),
      ]),
      single('reveal', 'Revelar no Explorador', () => (folder ? actions.revealDir(one) : actions.reveal(one)), 'Shift+Alt+R'),
      single('terminal', 'Abrir no Terminal', () => actions.openInTerminal(pasteDir)),
      single('history', 'Histórico Git', () => actions.history(one)),
      ...(folder ? [single('find-in', 'Localizar na Pasta...', () => actions.search(one))] : []),
    ]);
    sections.push([
      chat ? action('chat', chatLabel, () => paths.forEach((p) => chat(p))) : blocked('chat', chatLabel, chatReason),
    ]);
    sections.push([
      action('cut', 'Recortar', () => actions.copyMany(paths, true), 'Ctrl+X'),
      action('copy', 'Copiar', () => actions.copyMany(paths, false), 'Ctrl+C'),
      hasFiles
        ? action('paste', 'Colar', () => void pasteInto(pasteDir), 'Ctrl+V')
        : blocked('paste', 'Colar', 'Não há arquivo ou pasta copiado.'),
    ]);
    sections.push([
      action('copy-path', many ? 'Copiar Caminhos' : 'Copiar Caminho', () => actions.copyPaths('absolute', paths), 'Shift+Alt+C'),
      action('copy-relative', many ? 'Copiar Caminhos Relativos' : 'Copiar Caminho Relativo', () => actions.copyPaths('relative', paths), 'Ctrl+K Ctrl+Shift+C'),
    ]);
    sections.push([
      many
        ? blocked('rename', 'Renomear...', 'Escolha só um item para renomear.')
        : action('rename', 'Renomear...', () => startRename(one), 'F2'),
      action('delete', many ? `Excluir ${paths.length} itens...` : 'Excluir...', () => startDelete(paths), 'Del'),
    ]);
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
  // Colar sem arquivo no clipboard e Adicionar Pasta ao Chat sem o Orquestrador ficam desabilitados, com o motivo.
  const rootMenu = (hasFiles: boolean): ContextMenuItem[] => {
    const chat = actions.addToChat;
    const chatReason = 'Abra o Orquestrador para mandar a pasta ao chat.';
    return [
      action('root-new-file', 'Novo Arquivo...', () => startCreate('file', '')),
      action('root-new-dir', 'Nova Pasta...', () => startCreate('dir', '')),
      DIVIDER,
      action('root-reveal', 'Revelar no Explorador de Arquivos', () => actions.revealDir(''), 'Shift+Alt+R'),
      action('root-search', 'Pesquisar no Projeto', () => actions.search(''), 'Shift+Alt+F'),
      DIVIDER,
      chat ? action('root-chat', 'Adicionar Pasta ao Chat', () => chat(''))
        : blocked('root-chat', 'Adicionar Pasta ao Chat', chatReason),
      hasFiles
        ? action('root-paste', 'Colar', () => void pasteInto(''), 'Ctrl+V')
        : blocked('root-paste', 'Colar', 'Não há arquivo ou pasta copiado.'),
      DIVIDER,
      action('root-copy-path', 'Copiar o Caminho', () => actions.copyPaths('absolute', ['']), 'Shift+Alt+C'),
      action('root-copy-relative', 'Copiar Caminho Relativo', () => actions.copyPaths('relative', ['']), 'Ctrl+K Ctrl+Shift+C'),
    ];
  };

  // Abre o menu com a checagem do clipboard feita agora: Colar só fica habilitado se ele tem arquivo. Sem escuta nem consulta permanente.
  const clipboardHasFiles = () => backend.clipboardSnapshot().then((s) => s.paths.length > 0, () => false);
  const openRootMenu = async () => showContextMenu(rootMenu(await clipboardHasFiles()));
  const openItemMenu = async (paths: string[]) => showContextMenu(itemMenu(paths, await clipboardHasFiles()));

  // Colar em `dir`: a árvore relê depois do sucesso; erro vira toast. Recorte tira o item da árvore de onde estava, então a seleção sai se era dele.
  async function pasteInto(dir: string) {
    const place = dir ? `em “${dir}”` : 'na raiz do projeto';
    try {
      const moved = await actions.paste(dir);
      setSelection((prev) => prev.filter((p) => !moved.some((m) => under(m, p))));
      await reloadAll();
      if (dir) await openDir(dir);
      toast.success({ title: moved.length ? `Itens movidos ${place}` : `Itens colados ${place}`, duration: 3000 });
    } catch (e) {
      // Um recorte que falhou no meio pode ter movido alguns itens: a árvore relê mesmo assim.
      await reloadAll();
      toast.error({ title: message(e), duration: 7000 });
    }
  }

  // Mover (arrastar ou soltar): só o que passou na checagem vai ao App. Parada no meio mostra o erro e reconcilia com o que foi feito.
  async function moveInto(paths: string[], dest: string) {
    const problem = dropProblem(paths, dest);
    if (problem) return toast.warning({ title: problem, duration: 4000 });
    const moving = paths.filter((p) => dirOf(p) !== dest);
    let done: Moved[] = [];
    let failure = '';
    try {
      done = await actions.move(moving, dest);
    } catch (e) {
      failure = message(e);
      if (e instanceof BatchError) done = e.done as Moved[];
    }
    settle([...moving.map(dirOf), dest], done.map((m) => m.from), done);
    if (done.length) await openDir(dest);
    const count = `${done.length} ${done.length === 1 ? 'item' : 'itens'}`;
    if (failure) toast.error({ title: done.length ? `${failure} Antes disso, ${count} foi movido.` : failure, duration: 7000 });
    else toast.success({ title: `${count} movido${done.length === 1 ? '' : 's'} para ${where(dest)}`, duration: 3000 });
  }

  // Criar: o nome é checado aqui para mostrar o motivo; o backend confere de novo e nunca sobrescreve. Erro fica no diálogo.
  // Arquivo novo abre no editor; pasta nova só aparece na árvore, sem fechar as abas.
  async function submitCreate() {
    if (!creating || creating.busy) return;
    const { kind, name, dir } = creating;
    const problem = fileNameError(name);
    if (problem) return setCreating({ ...creating, error: problem });
    setCreating({ ...creating, error: '', busy: true });
    try {
      const created = kind === 'file' ? await backend.createFile(dir, name) : await backend.createDir(dir, name);
      setCreating(null);
      if (dir) await openDir(dir);
      else await reloadFolder('');
      if (kind === 'file') onOpen(created, { permanent: true });
      toast.success({ title: kind === 'file' ? 'Arquivo criado' : 'Pasta criada', duration: 3000 });
    } catch (e) {
      setCreating((prev) => prev && { ...prev, busy: false, error: message(e) });
    }
  }

  // Clique direito no painel inteiro: suprime o menu do WebView. Linha da árvore (arquivo ou pasta) fica com o `onRightClick` do Tree;
  // só a área vazia abre o menu da raiz.
  const onPanelContextMenu = (e: ReactMouseEvent<HTMLElement>) => {
    e.preventDefault();
    if ((e.target as HTMLElement).closest('[role="treeitem"]')) return;
    void openRootMenu();
  };

  // Teclas. Com o foco numa linha, valem para a seleção (ou só para a linha, se ela não está selecionada). Fora de linha, valem para a raiz.
  // Shift+F10 e a tecla de contexto abrem o menu da linha (o mesmo caminho do clique direito) ou o da raiz.
  const onTreeKey = (e: ReactKeyboardEvent<HTMLElement>) => {
    const row = (e.target as HTMLElement).closest<HTMLElement>('[role="treeitem"]');
    const key = row?.dataset.id ?? '';
    const real = !!row && !!key && !provisional(key);
    if (row && !real) return; // Nó provisório não tem tecla.
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return; // Modificador sozinho não conta: Ctrl+K, soltar, e depois Ctrl+Shift+C ainda completa o acorde.
    if ((e.shiftKey && e.key === 'F10') || e.key === 'ContextMenu') {
      e.preventDefault();
      e.stopPropagation();
      if (!real) return void openRootMenu();
      const rect = row!.getBoundingClientRect();
      row!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rect.left + 24, clientY: rect.top + rect.height / 2 }));
      return;
    }
    const targets = real ? (selection.includes(key) ? selection : [key]) : [];
    const path = real ? key : '';
    const ctrl = e.ctrlKey && !e.altKey && !e.metaKey;
    const plain = !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey;
    const altShift = e.shiftKey && e.altKey && !e.ctrlKey && !e.metaKey;
    if (chordPending.current) {
      if (ctrl && !e.shiftKey && e.code === 'KeyK') {
        e.preventDefault();
        e.stopPropagation();
        beginChord();
        return;
      }
      endChord();
      if (ctrl && e.shiftKey && e.code === 'KeyC' && targets.length) {
        e.preventDefault();
        e.stopPropagation();
        actions.copyPaths('relative', targets);
      }
      return;
    }
    const folder = real && !isFile(path);
    const dirFor = real ? (folder ? path : dirOf(path)) : '';
    let handled = true;
    if (ctrl && !e.shiftKey && e.code === 'KeyK') beginChord();
    else if (ctrl && !e.shiftKey && e.code === 'KeyV') void pasteInto(dirFor);
    else if (real && ctrl && !e.shiftKey && e.code === 'KeyX') actions.copyMany(targets, true);
    else if (real && ctrl && !e.shiftKey && e.code === 'KeyC') actions.copyMany(targets, false);
    else if (real && plain && e.key === 'F2') startRename(path);
    else if (real && plain && e.key === 'Delete') startDelete(targets);
    else if (real && plain && e.key === 'Enter' && !folder) onOpen(path, { permanent: true });
    else if (altShift && e.code === 'KeyR') (real ? (folder ? actions.revealDir(path) : actions.reveal(path)) : actions.revealDir(''));
    else if (altShift && e.code === 'KeyF') actions.search(dirFor);
    else if (altShift && e.code === 'KeyC') actions.copyPaths('absolute', real ? targets : ['']);
    else handled = false;
    if (!handled) return;
    if (real && !selection.includes(key)) setSelection([key]);
    e.preventDefault();
    e.stopPropagation();
  };

  // Mouse: o modificador do clique fica guardado para o `onSelect` saber se é seleção (Ctrl/Shift) ou abertura.
  const onPointerMods = (e: ReactMouseEvent<HTMLElement>) => {
    mods.current = { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey };
  };

  // Seleção do Tree. Ctrl ou Shift só seleciona (não abre nem recolhe). Clique simples abre prévia; pasta abre e fecha.
  const onSelect = (keys: string[], node: TreeDataNode) => {
    const m = mods.current;
    mods.current = NO_MODS;
    setSelection(keys);
    if (m.ctrl || m.shift) return;
    if (node.isLeaf) onOpen(node.key);
    else toggle(node.key, tree.expanded.includes(node.key));
  };

  // Duplo clique (e Enter, no teclado) abre o arquivo como permanente. Pasta não tem o que abrir.
  const onDoubleClick = (e: ReactMouseEvent<HTMLElement>) => {
    const key = (e.target as HTMLElement).closest<HTMLElement>('[role="treeitem"]')?.dataset.id ?? '';
    if (key && !provisional(key) && isFile(key)) onOpen(key, { permanent: true });
  };

  // Arrastar: o item (ou a seleção, se ele está nela) segue o ponteiro. Sem `move`, a linha nem é arrastável.
  const destOf = (target: EventTarget): string | null => {
    const row = (target as HTMLElement).closest<HTMLElement>('[role="treeitem"]');
    if (!row) return ''; // Área vazia do painel: a raiz do projeto.
    const key = row.dataset.id ?? '';
    if (!key) return null;
    if (provisional(key)) return key.slice(0, -1); // Nó provisório: a pasta dele.
    return isFile(key) ? dirOf(key) : key;
  };
  const onDragStart = (e: ReactDragEvent<HTMLElement>) => {
    const key = (e.target as HTMLElement).closest<HTMLElement>('[role="treeitem"]')?.dataset.id ?? '';
    if (!key || provisional(key)) return e.preventDefault();
    const paths = selection.includes(key) ? selection : [key];
    if (!selection.includes(key)) setSelection([key]);
    dragging.current = paths;
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', paths.join('\n'));
  };
  const onDragOver = (e: ReactDragEvent<HTMLElement>) => {
    const paths = dragging.current;
    if (!paths) return;
    const dest = destOf(e.target);
    if (dest === null) return;
    const problem = dropProblem(paths, dest);
    e.preventDefault();
    e.dataTransfer.dropEffect = problem ? 'none' : 'move';
    setDrop((prev) => (prev && prev.dest === dest && prev.problem === problem ? prev : { dest, problem }));
  };
  const onDragLeave = (e: ReactDragEvent<HTMLElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDrop(null);
  };
  const onDrop = (e: ReactDragEvent<HTMLElement>) => {
    e.preventDefault();
    const paths = dragging.current;
    const dest = destOf(e.target);
    dragging.current = null;
    setDrop(null);
    if (paths && dest !== null) void moveInto(paths, dest);
  };
  const onDragEnd = () => {
    dragging.current = null;
    setDrop(null);
  };

  // Renomear: valida aqui para mostrar o motivo; o App e o backend conferem de novo (alterações não salvas, colisão, nome).
  // Pasta renomeia sem checar alterações: o App cuida dos rascunhos de dentro dela.
  async function submitRename() {
    if (!renaming || renaming.busy) return;
    const { path, name } = renaming;
    if (name === nameOf(path)) return setRenaming(null);
    const problem = fileNameError(name);
    if (problem) return setRenaming({ ...renaming, error: problem });
    setRenaming({ ...renaming, error: '', busy: true });
    const folder = !isFile(path);
    const wasOpen = tree.expanded.includes(path);
    try {
      const next = await actions.rename(path, name);
      setRenaming(null);
      settle([dirOf(path)], folder ? [path] : [], [{ from: path, to: next }]);
      if (folder && wasOpen) await openDir(next);
      toast.success({ title: folder ? 'Pasta renomeada' : 'Arquivo renomeado', duration: 3000 });
    } catch (e) {
      setRenaming((prev) => prev && { ...prev, busy: false, error: message(e) });
    }
  }

  // Excluir: vai para a Lixeira. Uma parada no meio mantém o diálogo com o que sobrou e o erro; o que foi feito sai da árvore mesmo assim.
  async function submitDelete() {
    if (!deleting || deleting.busy) return;
    const { paths } = deleting;
    setDeleting({ ...deleting, busy: true, error: '' });
    let gone: string[] = [];
    let failure = '';
    try {
      gone = await actions.removeMany(paths);
    } catch (e) {
      failure = message(e);
      if (e instanceof BatchError) gone = e.done as string[];
    }
    settle(paths.map(dirOf), gone);
    if (!failure) {
      setDeleting(null);
      toast.success({ title: gone.length === 1 ? 'Enviado para a Lixeira' : `${gone.length} itens enviados para a Lixeira`, duration: 3000 });
    } else {
      setDeleting({ paths: paths.filter((p) => !gone.some((g) => under(g, p))), error: failure, busy: false });
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

  const renameProblem = renaming && (renaming.error || fileNameError(renaming.name));
  const deleteDrafts = deleting ? actions.dirtyOf(deleting.paths) : [];
  const deleteFolders = deleting ? deleting.paths.some((p) => !isFile(p)) : false;
  return (
    <>
      {/* Cobre o painel inteiro: tabIndex deixa o clique na área vazia focar o painel (sem anel, para não parecer um campo). */}
      <Flexbox
        ref={panel}
        height="100%"
        tabIndex={-1}
        className="relative outline-none"
        onKeyDown={onTreeKey}
        onKeyDownCapture={(e) => { mods.current = { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey }; }}
        onMouseDownCapture={onPointerMods}
        onContextMenu={onPanelContextMenu}
        onDoubleClick={onDoubleClick}
        onDragStart={onDragStart}
        onDragOver={onDragOver}
        onDragLeave={onDragLeave}
        onDrop={onDrop}
        onDragEnd={onDragEnd}
        onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) endChord(); }}
      >
        <ScrollArea className="h-full" disableContentFit viewportProps={{ ref: viewport }}>
          <Flexbox paddingBlock={8}>
            {error && <Text as="p" role="status" type="secondary" fontSize={12} className="px-3 py-1">{error}</Text>}
            {!tree.children[''] && !error && (
              <Text as="p" type="secondary" fontSize={12} className="px-3 py-1">Lendo a pasta…</Text>
            )}
            <Tree
              treeData={nodes('')} expandedKeys={tree.expanded} selectedKeys={shown} multiple
              indent={16} size="small" showIcon blockNode style={{ paddingInline: 8 }}
              onExpand={(keys) => {
                // Abre o que entrou na lista e fecha o que saiu (a tecla * abre vários de uma vez).
                keys.filter((k) => !tree.expanded.includes(k)).forEach((k) => toggle(k, false));
                tree.expanded.filter((k) => !keys.includes(k)).forEach((k) => toggle(k, true));
              }}
              onSelect={(keys, { node }) => onSelect(keys, node)}
              onRightClick={({ event, node }) => {
                // Sem o menu do navegador. Clique direito fora da seleção seleciona só o item; o menu age sobre a seleção.
                event.preventDefault();
                if (provisional(node.key)) return;
                const targets = shown.includes(node.key) && selection.length ? selection : [node.key];
                if (targets !== selection) setSelection(targets);
                void openItemMenu(targets);
              }}
            />
          </Flexbox>
        </ScrollArea>
        {/* Destino do arraste, visível mesmo com a árvore rolada: a pasta sob o ponteiro, ou a raiz; em vermelho, o motivo de não poder soltar. */}
        {dragOver && (
          <Text
            as="p" role="status" fontSize={12} ellipsis
            type={dragOver.problem ? 'danger' : 'secondary'}
            className="pointer-events-none absolute inset-x-0 bottom-0 truncate bg-card px-3 py-1 shadow-[0_-1px_0_var(--ant-color-border)]"
          >
            {dragOver.problem ?? `Soltar em ${dragOver.dest ? `“${dragOver.dest}”` : 'a raiz do projeto'}`}
          </Text>
        )}
      </Flexbox>

      {/* Renomear: só o nome dentro da pasta. Enter confirma; Cancelar e Esc não mexem em nada. */}
      <FormDialog
        open={!!renaming}
        title={renaming && !isFile(renaming.path) ? 'Renomear pasta' : 'Renomear arquivo'}
        busy={!!renaming?.busy}
        confirmLabel="Renomear"
        confirmDisabled={!!renameProblem}
        onClose={() => setRenaming(null)}
        onConfirm={submitRename}
      >
        {renaming && (
          <Flexbox gap={8}>
            <Text as="p" type="secondary" fontSize={12} code ellipsis title={renaming.path}>{renaming.path}</Text>
            <Input
              autoFocus aria-label="Novo nome" value={renaming.name} disabled={renaming.busy}
              onChange={(e) => setRenaming((prev) => prev && { ...prev, name: e.target.value, error: '' })}
              onFocus={(e) => e.currentTarget.select()}
              onPressEnter={submitRename}
            />
            {renameProblem && <Text as="p" type="danger" fontSize={12} role="alert">{renameProblem}</Text>}
          </Flexbox>
        )}
      </FormDialog>

      {/* Novo arquivo e Nova pasta, na pasta escolhida (ou na raiz). Enter cria; Cancelar e Esc não criam nada. Nome que existe é recusado pelo backend, sem sobrescrever. */}
      <FormDialog
        open={!!creating}
        title={creating?.kind === 'dir' ? 'Nova pasta' : 'Novo arquivo'}
        busy={!!creating?.busy}
        confirmLabel="Criar"
        onClose={() => setCreating(null)}
        onConfirm={submitCreate}
      >
        {creating && (
          <Flexbox gap={8}>
            <Text as="p" type="secondary" fontSize={12}>Será criado {creating.dir ? <>em <Text as="span" code fontSize={12}>{creating.dir}</Text></> : 'na raiz do projeto'}.</Text>
            <Input
              autoFocus aria-label={creating.kind === 'dir' ? 'Nome da nova pasta' : 'Nome do novo arquivo'} value={creating.name} disabled={creating.busy}
              placeholder={creating.kind === 'dir' ? 'Nome da pasta' : 'Nome do arquivo, com extensão'}
              onChange={(e) => setCreating((prev) => prev && { ...prev, name: e.target.value, error: '' })}
              onPressEnter={submitCreate}
            />
            {creating.error && <Text as="p" type="danger" fontSize={12} role="alert">{creating.error}</Text>}
          </Flexbox>
        )}
      </FormDialog>

      {/* Excluir: vai para a Lixeira do Windows, com a lista do que vai e das alterações não salvas (que se perdem e fecham as abas). */}
      <FormDialog
        open={!!deleting}
        title={deleting && deleting.paths.length > 1 ? `Excluir ${deleting.paths.length} itens?` : deleteFolders ? 'Excluir pasta?' : 'Excluir arquivo?'}
        busy={!!deleting?.busy}
        confirmLabel="Mover para a Lixeira"
        danger
        onClose={() => setDeleting(null)}
        onConfirm={submitDelete}
      >
        {deleting && (
          <Flexbox gap={8}>
            <Text as="p" fontSize={14}>
              {deleting.paths.length > 1 ? 'Estes itens vão para a Lixeira do Windows:' : 'Este item vai para a Lixeira do Windows:'}
              {deleteFolders && ' As pastas levam tudo o que está dentro delas.'}
            </Text>
            <Flexbox gap={2} style={{ maxHeight: 180, overflow: 'auto' }}>
              {deleting.paths.slice(0, LIST_MAX).map((p) => (
                <Text key={p} as="span" code fontSize={12} ellipsis title={p}>{p}</Text>
              ))}
              {deleting.paths.length > LIST_MAX && (
                <Text as="span" type="secondary" fontSize={12}>e mais {deleting.paths.length - LIST_MAX}</Text>
              )}
            </Flexbox>
            {deleteDrafts.length > 0 && (
              <Alert
                type="warning"
                description={`Há alterações não salvas em ${deleteDrafts.length === 1 ? 'um item' : `${deleteDrafts.length} itens`}: ${deleteDrafts.slice(0, 3).join(', ')}${deleteDrafts.length > 3 ? ' e outros' : ''}. Elas se perdem ao excluir, e as abas desses arquivos fecham.`}
              />
            )}
            {deleting.error && <Alert type="error" role="alert" description={deleting.error} className="select-text" />}
          </Flexbox>
        )}
      </FormDialog>
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
