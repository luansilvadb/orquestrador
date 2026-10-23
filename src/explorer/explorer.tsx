// A navegação da coluna de ícones, a árvore de pastas (na tela principal) e a barra de status com a branch e o botão do Terminal.
// A árvore se atualiza sozinha (usePolling). O status do Git vem do App, que o usa no contador da navegação e na barra de status.
// Arquivo e pasta da árvore têm menu de contexto (clique direito, Shift+F10 e a tecla de contexto); a área vazia do painel tem o menu da raiz do projeto.
// Seleção com Ctrl e Shift; arrastar e soltar move para pasta ou para a raiz, com o destino mostrado e validado. As ações que mexem no App (abas,
// rascunhos, telas) vêm pelo `actions`; as que só chamam o backend a árvore chama direto.
// O visual segue o Explorer do Antigravity/VS Code: ícones do tema Symbols, decorações do Git (cor, letra e "•" na pasta; cinza no ignorado),
// barra com as ações da view e a seção recolhível com o nome do projeto.
import { type DragEvent as ReactDragEvent, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { ChevronRight, ChevronsDownUp, FilePlus, Files, FolderPlus, Info, LoaderCircle, type LucideIcon, RefreshCw, Search, SquareTerminal, X } from 'lucide-react';
import {
  ActionIcon, Alert, Badge, type ContextMenuItem, Modal, ScrollArea, showContextMenu, Text, toast, Tree, type TreeDataNode,
} from '@lobehub/ui/base-ui';
import { createPortal } from 'react-dom';
import { Flexbox } from '@lobehub/ui';
import { message } from '../backend/backend';
import { useBackend } from '../backend/backend-context';
import { decorations, IGNORED } from './explorer-decorations';
import { FileIcon } from '../shared/file-icon';
import { SourceControl } from './source-control-icon';
import { fileNameError } from '../shared/file-tab-actions';
import { action, blocked, DIVIDER, fileName, useChord } from '../shared/file-tabs';
import { join, splitPath } from '../shared/paths';
import { usePolling } from '../shared/polling';
import type { Batch, DirEntry, GitStatus, Moved } from '../backend/types';

export type Route = 'arquivos' | 'pesquisa' | 'git';

/** Opções de abertura. Sem elas, o clique simples abre como prévia (a aba seguinte substitui). `permanent`: a aba fica. `side`: abre na área da direita. */
export type OpenOptions = { permanent?: boolean; side?: boolean };

/**
 * O que o menu e os atalhos da árvore pedem ao App. Excluir e Mover resolvem com o lote: `done` são os itens já feitos (não são desfeitos)
 * e `error` diz o que parou; quem chama reconcilia com `done`. Erros que impedem a ação toda chegam como exceção (Renomear, Excluir, Mover,
 * Colar) ou viram toast no App (o resto).
 */
export type FileActions = {
  /** Dos caminhos dados (arquivos ou pastas), os que têm alteração não salva, pastas incluídas. Excluir avisa antes. */
  dirtyOf: (paths: string[]) => string[];
  /** Copia os caminhos de uma lista, um por linha, no formato absoluto ou relativo. Lista com um caminho só copia esse. */
  copyPaths: (kind: 'relative' | 'absolute', paths: string[]) => void;
  openInTerminal: (dir: string) => void;
  /** Cola o caminho no rascunho do Orquestrador. `null` quando ele não está rodando (o item fica desabilitado). */
  addToChat: ((path: string) => void) | null;
  /** Renomeia um arquivo ou uma pasta na mesma pasta e devolve o caminho novo. Rejeita com a mensagem do motivo, sem mexer em nada. */
  rename: (path: string, name: string) => Promise<string>;
  /** Manda os itens (pastas incluídas, com o que está dentro) para a Lixeira e fecha as abas deles. `done` são os caminhos feitos. */
  removeMany: (paths: string[]) => Promise<Batch<string>>;
  /** Move itens para a pasta `dest` ('' é a raiz), relativos ao projeto. `done` tem o `{from, to}` de cada um. */
  move: (paths: string[], dest: string) => Promise<Batch<Moved>>;
  /** Abre o histórico Git do arquivo ou da pasta. `null` fora do modo `repo`: o item some do menu. */
  history: ((path: string) => void) | null;
  /** Vai para a pesquisa de todo o projeto (`dir` vazio ou omitido) ou da pasta `dir` (relativa, com "/"), com o campo focado. */
  search: (dir?: string) => void;
  /** Cola o clipboard na pasta `dir` ('' é a raiz). Rejeita com a mensagem. Devolve as origens que o recorte tirou do projeto (vazio na cópia). */
  paste: (dir: string) => Promise<string[]>;
};

// Caminho relativo com "/" (como o backend); a raiz é ''.
const inside = (dir: string, path: string) => path.startsWith(`${dir}/`);
const under = (base: string, path: string) => path === base || inside(base, path);
// Nó provisório ("Lendo a pasta…") tem a chave `<pasta>/`. Nenhum caminho real termina com barra.
const provisional = (key: string) => key.endsWith('/');
const where = (dest: string) => (dest ? `“${dest}”` : 'a raiz do projeto');

// Ações que só chamam o backend: o erro vira toast aqui mesmo.
const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });
// Revelar no Explorer do Windows: a pasta (a raiz, '') abre nela; o arquivo abre a pasta dele com ele marcado.
const revealPath = (backend: ReturnType<typeof useBackend>, path: string, folder: boolean) => void (folder ? backend.revealDir(path) : backend.revealFile(path)).catch(report);
// Copiar e recortar põem os itens no clipboard do Windows: colar é no Explorer. Recortar não move nada agora.
const pasted = (moved: number, place: string) => moved ? `Itens movidos ${place}` : `Itens colados ${place}`;
const copyMany = (backend: ReturnType<typeof useBackend>, paths: string[], cut: boolean) => void backend.copyFiles(paths, cut).then(
  () => toast.success({ title: cut ? `${paths.length} itens recortados. Cole no Explorer para mover.` : `${paths.length} itens copiados. Cole no Explorer.`, duration: 4000 }),
  report,
);

type ExplorerTree = { children: Record<string, DirEntry[]>; expanded: string[] };
/** Tira uma pasta e tudo o que está dentro dela, aberto ou com lista guardada. */
const drop = (tree: ExplorerTree, path: string): ExplorerTree => ({
  expanded: tree.expanded.filter((p) => !under(path, p)),
  children: Object.fromEntries(Object.entries(tree.children).filter(([p]) => !under(path, p))),
});
const sameList = (a: DirEntry[] | undefined, b: DirEntry[]) =>
  !!a && a.length === b.length && a.every((e, i) => e.name === b[i].name && e.dir === b[i].dir && e.ignored === b[i].ignored);

/** Leva um caminho para onde o item (ou a pasta dele) foi parar. Caminho que não foi movido fica como está. */
const remapPath = (path: string, moves: Moved[]) => {
  const hit = moves.find((m) => under(m.from, path));
  return hit ? hit.to + path.slice(hit.from.length) : path;
};

/** Motivo para soltar `paths` em `dest`, ou null se pode. Pasta não vai para si nem para dentro dela; item já na pasta não tem o que mover. */
function dropProblem(paths: string[], dest: string): string | null {
  const into = paths.find((p) => under(p, dest));
  if (into) return `Não dá para mover “${fileName(into)}” para dentro dele mesmo.`;
  if (paths.every((p) => splitPath(p).dir === dest)) return dest ? 'Os itens já estão nesta pasta.' : 'Os itens já estão na raiz do projeto.';
  return null;
}

// Sem arquivo aberto nem seleção, nada fica marcado. Pasta abre e fecha sem ficar marcada.
const NONE: string[] = [];
const NO_MODS = { ctrl: false, shift: false };

/** Coluna de ícones: uma tela por botão, na ordem Arquivos, Pesquisar, Git. O botão da tela aberta tem aria-current e um filete à esquerda. */
export function Nav({ route, changed, onGo }: { route: Route | null; changed: number; onGo: (to: Route) => void }) {
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
  // Contador no canto de baixo à direita do botão, com as medidas do badge do Antigravity (pílula de 12 px, fonte de 10 px, preta para dar contraste sobre o vermelho) e a posição 19/5 px dentro do botão de 36 px. Sem filhos, o Badge é a própria pílula: o `style` troca o tamanho e a sombra do tema, e não há deslocamento para fora do canto.
  const gitBadge = changed > 0 && (
    <Badge
      aria-hidden="true" count={changed} size="small" className="pointer-events-none absolute"
      style={{ top: 19, right: 5, minWidth: 12, height: 12, padding: '0 3px', fontSize: 10, fontWeight: 600, color: '#000', lineHeight: '12px', borderRadius: 4, boxShadow: 'none' }}
    />
  );
  return (
    <Flexbox as="nav" aria-label="Telas" flex="none" width={44} align="center" gap={4} paddingBlock={8} className="border-r bg-card">
      {button('arquivos', 'Arquivos', Files)}
      {button('pesquisa', 'Pesquisar', Search, 'Pesquisar (Ctrl+Shift+F)')}
      {button('git', gitLabel, SourceControl, 'Git', gitBadge)}
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

// Botões do diálogo, nas medidas do Antigravity: 28 px, raio de 2 px, 10 px de respiro e 13 px de texto; o principal na cor primária do
// tema (texto escuro do fundo), o secundário no preenchimento.
const DIALOG_BUTTON = 'flex h-7 min-w-[64px] items-center justify-center gap-1.5 rounded-[2px] border border-[var(--ant-color-fill-tertiary)] px-2.5 text-[13px] outline-none focus:ring-1 focus:ring-[var(--ant-color-primary)] focus:ring-offset-2 focus:ring-offset-[var(--ant-color-bg-elevated)] disabled:cursor-default disabled:opacity-40';

/**
 * Diálogo de Excluir, no formato do diálogo do Antigravity/VS Code (Renomear e Criar são inline): caixa de 522 px com raio de 3 px e
 * borda de 1 px, o X no canto, o ícone de 48 px à esquerda, a pergunta em 18 px, o detalhe em 13 px e os botões à direita,
 * o "Mover para a Lixeira" primeiro e o Cancelar depois. Esc, o X e Cancelar não fecham com o pedido rodando; clicar fora não fecha.
 * O foco começa no confirmar, como no VS Code: Enter confirma.
 */
function FormDialog({ open, title, detail, busy, onClose, onConfirm, children }: {
  open: boolean;
  /** A pergunta: é também o nome acessível do diálogo. */
  title: string;
  detail: ReactNode;
  busy: boolean;
  onClose: () => void;
  onConfirm: () => void;
  children?: ReactNode;
}) {
  const close = () => !busy && onClose();
  const body = useRef<HTMLDivElement>(null);
  const confirm = useRef<HTMLButtonElement>(null);
  // O foco fica no diálogo enquanto ele está aberto: o menu de contexto, ao fechar, devolve o foco à árvore logo depois de o diálogo
  // abrir; aqui ele volta ao confirmar, para Enter ir ao diálogo.
  useEffect(() => {
    if (!open) return;
    const keep = (e: FocusEvent) => {
      const dialog = body.current?.closest('[role="dialog"]');
      if (!dialog || !(e.target instanceof Node) || dialog.contains(e.target)) return;
      confirm.current?.focus();
    };
    document.addEventListener('focusin', keep);
    return () => document.removeEventListener('focusin', keep);
  }, [open]);
  return (
    <Modal
      open={open} title={title} closable={false} draggable={false} keyboard maskClosable={false} width={522} onCancel={close}
      className="rounded-[3px]! border border-[var(--ant-color-border-secondary)] p-2.5 shadow-[0_0_8px_rgba(0,0,0,0.36)]!"
      classNames={{ header: 'sr-only', body: 'p-0!', footer: 'gap-2.5! px-[15px]! pt-6! pb-[14px]!' }}
      footer={(
        <>
          <button
            ref={confirm} type="button" autoFocus disabled={busy} onClick={onConfirm}
            className={`${DIALOG_BUTTON} bg-[var(--ant-color-primary)] text-[var(--ant-color-bg-layout)] enabled:hover:bg-[var(--ant-color-primary-hover)]`}
          >
            {busy && <LoaderCircle className="size-3.5 animate-spin" aria-hidden />}
            Mover para a Lixeira
          </button>
          <button
            type="button" disabled={busy} onClick={onClose}
            className={`${DIALOG_BUTTON} bg-[var(--ant-color-fill-secondary)] text-[var(--ant-color-text)] enabled:hover:bg-[var(--ant-color-fill)]`}
          >
            Cancelar
          </button>
        </>
      )}
    >
      <div ref={body} />
      <Flexbox horizontal justify="flex-end" className="h-[26px] pb-1">
        <button
          type="button" aria-label="Fechar" title="Fechar" disabled={busy} onClick={close}
          className="flex size-[22px] items-center justify-center rounded-[5px] outline-none hover:bg-[var(--ant-color-fill)] focus-visible:ring-2 focus-visible:ring-[var(--ant-color-primary)]"
        >
          <X className="size-4" aria-hidden />
        </button>
      </Flexbox>
      <Flexbox horizontal className="px-2.5 text-[13px] leading-[22px]">
        <Info className="size-12 shrink-0 text-[var(--ant-color-info)]" strokeWidth={1.5} aria-hidden />
        <Flexbox className="min-w-0 flex-1 pl-6">
          <p className="mb-2 flex min-h-12 items-center text-[18px] font-normal leading-[22px] [overflow-wrap:anywhere]">{title}</p>
          <p>{detail}</p>
          {children}
        </Flexbox>
      </Flexbox>
    </Modal>
  );
}

/**
 * Campo do nome, dentro da linha da árvore, como o do Explorer do Antigravity: 22 px, borda de 1 px, raio de 4 px e o contorno de foco
 * de 1 px; com nome inválido, a borda fica vermelha e o motivo aparece embaixo. Serve a Criar e a Renomear. Enter confirma; Esc cancela;
 * sair do campo confirma se o nome é válido e cancela se não é. As teclas não vão para a árvore.
 */
function NewItemInput({ kind, label, selectBase = false, name, busy, error, onChange, onSubmit, onCancel }: {
  kind: 'file' | 'dir';
  /** Nome acessível do campo; sem ele, o do campo de criar. */
  label?: string;
  /** Seleciona só o nome sem a extensão (Renomear de arquivo); senão, o nome todo. */
  selectBase?: boolean;
  name: string;
  busy: boolean;
  error: string;
  onChange: (name: string) => void;
  /** `leaving`: saiu do campo (clique fora), então o foco não volta para a linha. */
  onSubmit: (leaving?: boolean) => void;
  onCancel: (leaving?: boolean) => void;
}) {
  const stop = (e: { stopPropagation: () => void }) => e.stopPropagation();
  // O menu de contexto, ao fechar, devolve o foco à árvore logo depois de o campo aparecer: essa saída do campo não conta, e o foco volta.
  const born = useRef(performance.now());
  // O motivo vai fora da linha (ela corta o que passa dela), preso embaixo do campo, no painel (onde valem as cores do tema);
  // acompanha a rolagem e o tamanho.
  const field = useRef<HTMLInputElement>(null);
  const [at, setAt] = useState<{ rect: DOMRect; host: Element } | null>(null);
  useLayoutEffect(() => {
    if (!error) return setAt(null);
    const place = () => {
      if (field.current) setAt({ rect: field.current.getBoundingClientRect(), host: field.current.closest('.explorer-tree') ?? document.body });
    };
    place();
    window.addEventListener('scroll', place, true);
    window.addEventListener('resize', place);
    return () => {
      window.removeEventListener('scroll', place, true);
      window.removeEventListener('resize', place);
    };
  }, [error]);
  // Foco e seleção: no arquivo, o nome sem a última extensão (".gitignore" fica inteiro). Também vale quando o menu devolve o foco.
  const place = (input: HTMLInputElement) => {
    input.focus({ preventScroll: true });
    const dot = selectBase ? name.lastIndexOf('.') : -1;
    input.setSelectionRange(0, dot > 0 ? dot : name.length);
  };
  useLayoutEffect(() => {
    if (field.current) place(field.current);
  }, []);
  // Depois de um pedido que falhou (o campo fica desabilitado enquanto roda), o foco volta para ele se ninguém o pegou.
  useEffect(() => {
    if (!busy && (!document.activeElement || document.activeElement === document.body)) field.current?.focus({ preventScroll: true });
  }, [busy]);
  const blur = (input: HTMLInputElement) => {
    if (busy) return;
    if (performance.now() - born.current < 300) return void requestAnimationFrame(() => place(input));
    if (error || !name.trim()) onCancel(true);
    else onSubmit(true);
  };
  return (
    <span className="relative flex min-w-0 flex-1" onClick={stop} onDoubleClick={stop} onContextMenu={stop} onMouseDown={stop}>
      <input
        ref={field} autoFocus spellCheck={false} value={name} disabled={busy}
        aria-label={label ?? (kind === 'dir' ? 'Nome da nova pasta' : 'Nome do novo arquivo')} aria-invalid={!!error || undefined}
        className={`h-[22px] min-w-0 flex-1 rounded-[4px] border bg-[var(--ant-color-fill-tertiary)] px-1 text-[13px] text-[var(--ant-color-text)] outline-none ${error
          ? 'border-[var(--ant-color-error)]'
          : 'border-[var(--ant-color-border)] focus:border-[var(--ant-color-primary)]'}`}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          // Com composição de IME (acentos, ideogramas), Enter e Esc são do IME.
          if (e.nativeEvent.isComposing) return;
          if (e.key === 'Enter') {
            e.preventDefault();
            onSubmit();
          } else if (e.key === 'Escape') {
            e.preventDefault();
            onCancel();
          }
        }}
        onBlur={(e) => blur(e.currentTarget)}
      />
      {error && at && createPortal(
        <span
          role="alert" style={{ left: at.rect.left, top: at.rect.bottom, width: at.rect.width }}
          className="fixed z-[1000] rounded-b-[4px] border border-t-0 border-[var(--ant-color-error)] bg-[color-mix(in_srgb,var(--ant-color-error)_25%,var(--ant-color-bg-elevated))] px-1.5 py-1 text-[12px] leading-[17px] text-[var(--ant-color-text)]"
        >
          {error}
        </span>,
        at.host,
      )}
    </span>
  );
}

// Ações da barra do painel: aparecem com o ponteiro ou o foco no painel, como no VS Code.
function HeaderAction({ icon, label, onClick }: { icon: LucideIcon; label: string; onClick: () => void }) {
  return <ActionIcon icon={icon} size="small" variant="borderless" aria-label={label} title={label} onClick={onClick} />;
}

/** Árvore de pastas do projeto, na faixa à esquerda da tela principal. Pasta abre e fecha; arquivo abre na área do meio (prévia no clique, permanente no duplo clique e no Enter).
 * `reveal` abre as pastas de um arquivo e o seleciona (um objeto novo a cada pedido). `project` é o nome da pasta do projeto, título da seção.
 * `git` é o status do repositório (null fora do modo `repo`): dá as cores e letras das linhas. */
export function FilesScreen({
  openPath, onOpen, reveal, actions, project, git, active }: {
  openPath: string | null;
  onOpen: (path: string, options?: OpenOptions) => void;
  reveal?: { path: string } | null;
  actions: FileActions;
  project: string;
  git: GitStatus | null;
  active: boolean;
}) {
  const backend = useBackend();
  const [tree, setTree] = useState<ExplorerTree>({ children: {}, expanded: [] });
  const [error, setError] = useState('');
  // Seleção própria da árvore. Vazia, o arquivo aberto aparece marcado.
  const [selection, setSelection] = useState<string[]>([]);
  // Estados do formulário inline de Renomear e Criar, e do diálogo de Excluir. Cada um guarda o que o usuário digitou (ou o motivo de recusa) e se a ação está rodando.
  const [renaming, setRenaming] = useState<Renaming | null>(null);
  const [deleting, setDeleting] = useState<Deleting | null>(null);
  const [creating, setCreating] = useState<Creating | null>(null);
  // Arraste em curso: o destino sob o ponteiro e o motivo de não poder soltar ali.
  const [dragOver, setDrop] = useState<Drop | null>(null);
  // Seção do projeto recolhida: a árvore some, a barra fica.
  const [collapsed, setCollapsed] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  // Viewport do ScrollArea (o elemento que rola de fato): o Revelar rola este, não os ancestrais.
  const viewport = useRef<HTMLDivElement>(null);
  const expanded = useRef<string[]>([]);
  const dragging = useRef<string[] | null>(null);
  // Modificadores do último clique ou tecla. O Tree não entrega o evento ao `onSelect`, então ele é lido aqui.
  const mods = useRef(NO_MODS);
  // Acorde Ctrl+K, como nas abas: vale por 1,5 s, cancela ao sair da árvore e some com o componente.
  const { pending: chordPending, begin: beginChord, end: endChord } = useChord();
  useLayoutEffect(() => {
    expanded.current = tree.expanded;
  });
  // Trocar de aba limpa a seleção: a marcação volta a ser a do arquivo aberto.
  useEffect(() => setSelection([]), [openPath]);
  // Linhas: arrastáveis quando há como mover, e a pasta que recebe o arraste ganha a moldura (vermelha se não pode). Roda a cada render, porque o Tree recria as linhas.
  useLayoutEffect(() => {
    panel.current?.querySelectorAll<HTMLElement>('[role="treeitem"][data-id]').forEach((row) => {
      const key = row.dataset.id ?? '';
      const hit = !!dragOver && !provisional(key) && dragOver.dest === key;
      row.draggable = !provisional(key) && renaming?.path !== key;
      row.style.setProperty('--depth', String(Number(row.getAttribute('aria-level') ?? 1) - 1));
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
  }, 1500, active);

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

  async function toggle(path: string, isOpen: boolean) {
    // Fechar só tira a pasta (e as de dentro) da lista de abertas: o conteúdo fica até a animação terminar.
    if (isOpen) return setTree((prev) => ({ ...prev, expanded: prev.expanded.filter((p) => !under(path, p)) }));
    await openDir(path);
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
    const { dir, name } = splitPath(path);
    return (tree.children[dir] ?? []).some((entry) => !entry.dir && entry.name === name);
  };

  // Marcação exibida: a seleção, ou o arquivo aberto quando não há seleção.
  const shown = selection.length ? selection : openPath ? [openPath] : NONE;

  // ---------- menus e atalhos ----------

  const startRename = (path: string) => {
    if (selection.length > 1) return toast.warning({ title: 'Renomeie um item de cada vez.', duration: 3000 });
    setRenaming({ path, name: fileName(path), error: '', busy: false });
  };
  const startDelete = (paths: string[]) => setDeleting({ paths, error: '', busy: false });
  // Criar abre a pasta de destino (a linha do nome entra nela) e põe ali a linha editável, como no VS Code.
  const startCreate = (kind: 'file' | 'dir', dir: string) => {
    setCollapsed(false);
    setCreating({ kind, dir, name: '', error: '', busy: false });
    if (dir && !tree.expanded.includes(dir)) void openDir(dir);
  };

  // Colar em `dir`: a árvore relê depois do sucesso; erro vira toast. Recorte tira o item da árvore de onde estava, então a seleção sai se era dele.
  async function pasteInto(dir: string) {
    const place = dir ? `em “${dir}”` : 'na raiz do projeto';
    try {
      const moved = await actions.paste(dir);
      setSelection((prev) => prev.filter((p) => !moved.some((m) => under(m, p))));
      await reloadAll();
      if (dir) await openDir(dir);
      toast.success({ title: pasted(moved.length, place), duration: 3000 });
    } catch (e) {
      // Um recorte que falhou no meio pode ter movido alguns itens: a árvore relê mesmo assim.
      await reloadAll();
      toast.error({ title: message(e), duration: 7000 });
    }
  }

  // Item (ou seleção) da árvore: uma ação real por item de menu. Vários itens: as que só servem a um ficam desabilitadas com o motivo.
  const itemMenu = (paths: string[], hasFiles: boolean): ContextMenuItem[] => {
    const many = paths.length > 1;
    const one = paths[0];
    const folder = !many && !isFile(one);
    const single = (key: string, label: string, run: () => void, shortcut?: string) =>
      (many ? blocked(key, label, 'Escolha só um item para esta ação.') : action(key, label, run, shortcut));
    const chat = actions.addToChat;
    const history = actions.history;
    const chatLabel = folder ? 'Adicionar Pasta ao Chat' : 'Adicionar ao Chat';
    const chatReason = 'Abra o Orquestrador para mandar ao chat.';
    const pasteDir = folder ? one : splitPath(one).dir;
    const sections: ContextMenuItem[][] = [];
    if (folder) {
      sections.push([
        action('new-file', 'Novo Arquivo...', () => startCreate('file', one)),
        action('new-dir', 'Nova Pasta...', () => startCreate('dir', one)),
      ]);
    }
    sections.push([
      ...(folder ? [] : [
        single('open-with', 'Abrir com...', () => void backend.openWith(one).catch(report)),
        single('open-side', 'Abrir à Direita', () => onOpen(one, { side: true })),
      ]),
      single('reveal', 'Revelar no Explorador', () => revealPath(backend, one, folder), 'Shift+Alt+R'),
      single('terminal', 'Abrir no Terminal', () => actions.openInTerminal(pasteDir)),
      ...(history ? [single('history', 'Histórico Git', () => history(one))] : []),
      ...(folder ? [single('find-in', 'Localizar na Pasta...', () => actions.search(one))] : []),
    ]);
    sections.push([
      chat ? action('chat', chatLabel, () => paths.forEach((p) => chat(p))) : blocked('chat', chatLabel, chatReason),
    ]);
    sections.push([
      action('cut', 'Recortar', () => copyMany(backend, paths, true), 'Ctrl+X'),
      action('copy', 'Copiar', () => copyMany(backend, paths, false), 'Ctrl+C'),
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

  // Ações da raiz (área vazia do painel, abaixo das linhas): sempre na raiz do projeto, nunca no arquivo marcado.
  // Colar sem arquivo no clipboard e Adicionar Pasta ao Chat sem o Orquestrador ficam desabilitados, com o motivo.
  const rootMenu = (hasFiles: boolean): ContextMenuItem[] => {
    const chat = actions.addToChat;
    const chatReason = 'Abra o Orquestrador para mandar a pasta ao chat.';
    return [
      action('root-new-file', 'Novo Arquivo...', () => startCreate('file', '')),
      action('root-new-dir', 'Nova Pasta...', () => startCreate('dir', '')),
      DIVIDER,
      action('root-reveal', 'Revelar no Explorador de Arquivos', () => revealPath(backend, '', true), 'Shift+Alt+R'),
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


  // Mover (arrastar ou soltar): só o que passou na checagem vai ao App. Parada no meio mostra o erro e reconcilia com o que foi feito.
  async function moveInto(paths: string[], dest: string) {
    const problem = dropProblem(paths, dest);
    if (problem) return toast.warning({ title: problem, duration: 4000 });
    const moving = paths.filter((p) => splitPath(p).dir !== dest);
    const { done, error: failure } = await actions.move(moving, dest).catch((e: unknown): Batch<Moved> => ({ done: [], error: message(e) }));
    settle([...moving.map((p) => splitPath(p).dir), dest], done.map((m) => m.from), done);
    if (done.length) await openDir(dest);
    const count = `${done.length} ${done.length === 1 ? 'item' : 'itens'}`;
    if (failure) toast.error({ title: done.length ? `${failure} Antes disso, ${count} foi movido.` : failure, duration: 7000 });
    else toast.success({ title: `${count} movido${done.length === 1 ? '' : 's'} para ${where(dest)}`, duration: 3000 });
  }

  // Criar: o nome é checado aqui para mostrar o motivo; o backend confere de novo e nunca sobrescreve. Erro fica embaixo do campo.
  // Arquivo novo abre no editor; pasta nova só aparece na árvore, sem fechar as abas.
  async function submitCreate() {
    if (!creating || creating.busy) return;
    const { kind, dir } = creating;
    const name = creating.name.trim();
    if (!name) return setCreating(null);
    const problem = fileNameError(name);
    if (problem) return setCreating({ ...creating, error: problem });
    setCreating({ ...creating, error: '', busy: true });
    const request = kind === 'file' ? backend.createFile(dir, name) : backend.createDir(dir, name);
    const title = kind === 'file' ? 'Arquivo criado' : 'Pasta criada';
    try {
      const created = await request;
      setCreating(null);
      if (dir) await openDir(dir);
      else await reloadFolder('');
      if (kind === 'file') onOpen(created, { permanent: true });
      toast.success({ title, duration: 3000 });
    } catch (e) {
      const error = message(e);
      setCreating((prev) => prev && { ...prev, busy: false, error });
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
    const dirFor = real ? (folder ? path : splitPath(path).dir) : '';
    let handled = true;
    if (ctrl && !e.shiftKey && e.code === 'KeyK') beginChord();
    else if (ctrl && !e.shiftKey && e.code === 'KeyV') void pasteInto(dirFor);
    else if (real && ctrl && !e.shiftKey && e.code === 'KeyX') copyMany(backend, targets, true);
    else if (real && ctrl && !e.shiftKey && e.code === 'KeyC') copyMany(backend, targets, false);
    else if (real && plain && e.key === 'F2') startRename(path);
    else if (real && plain && e.key === 'Delete') startDelete(targets);
    else if (real && plain && e.key === 'Enter' && !folder) onOpen(path, { permanent: true });
    else if (altShift && e.code === 'KeyR') revealPath(backend, path, !real || folder);
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
    return isFile(key) ? splitPath(key).dir : key;
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

  // Renomear é inline: a própria linha vira o campo (nome base selecionado). Motivo de recusa ('' se pode), com as mensagens do Antigravity:
  // vazio, nome inválido (sem trim: espaço na borda recusa, como lá), ou nome que já existe na pasta (sem contar o próprio item, sem
  // diferença de caixa). Mostra junto ao campo; o App e o backend conferem de novo.
  const renameIssue = (r: Renaming) => {
    if (!r.name.trim()) return 'É necessário fornecer um nome de arquivo ou pasta.';
    const clash = (tree.children[splitPath(r.path).dir] ?? []).some((e) => e.name !== fileName(r.path) && e.name.toLowerCase() === r.name.toLowerCase());
    return fileNameError(r.name) || (clash ? `Um arquivo ou pasta ${r.name} já existe nesta localização. Escolha um nome diferente.` : '');
  };
  // Foco na linha da árvore (ou no painel, se a linha não aceita foco): depois do menu, do Enter ou do Esc, a tecla volta ao item.
  const focusRow = (path: string) => {
    const row = panel.current?.querySelector<HTMLElement>(`[role="treeitem"][data-id="${CSS.escape(path)}"]`);
    (row ?? panel.current)?.focus({ preventScroll: true });
  };
  // Sai do campo sem renomear. `leaving`: foi clique fora, então o foco não volta à linha.
  const endRename = (path: string, leaving: boolean) => {
    setRenaming(null);
    if (!leaving) requestAnimationFrame(() => focusRow(path));
  };
  // Pasta renomeia sem checar alterações: o App cuida dos rascunhos de dentro dela. `renamingNow` barra o segundo envio já no mesmo
  // instante (o `busy` do estado só chega no redesenho): Enter repetido ou clique fora durante o pedido não renomeiam duas vezes.
  const renamingNow = useRef(false);
  async function submitRename(leaving = false) {
    if (!renaming || renaming.busy || renamingNow.current) return;
    const { path, name } = renaming;
    if (name === fileName(path)) return endRename(path, leaving);
    const problem = renameIssue(renaming);
    if (problem) return setRenaming({ ...renaming, error: problem });
    renamingNow.current = true;
    setRenaming({ ...renaming, error: '', busy: true });
    const folder = !isFile(path);
    const reopen = folder && tree.expanded.includes(path);
    const dirs = folder ? [path] : [];
    const title = folder ? 'Pasta renomeada' : 'Arquivo renomeado';
    try {
      const next = await actions.rename(path, name);
      setRenaming(null);
      settle([splitPath(path).dir], dirs, [{ from: path, to: next }]);
      setSelection([next]);
      if (!leaving) requestAnimationFrame(() => focusRow(next));
      if (reopen) await openDir(next);
      toast.success({ title, duration: 3000 });
    } catch (e) {
      const error = message(e);
      setRenaming((prev) => prev && { ...prev, busy: false, error });
    }
    renamingNow.current = false;
  }

  // Excluir: vai para a Lixeira. Uma parada no meio mantém o diálogo com o que sobrou e o erro; o que foi feito sai da árvore mesmo assim.
  async function submitDelete() {
    if (!deleting || deleting.busy) return;
    const { paths } = deleting;
    setDeleting({ ...deleting, busy: true, error: '' });
    const { done: gone, error: failure } = await actions.removeMany(paths).catch((e: unknown): Batch<string> => ({ done: [], error: message(e) }));
    settle(paths.map((p) => splitPath(p).dir), gone);
    if (!failure) {
      setDeleting(null);
      toast.success({ title: gone.length === 1 ? 'Enviado para a Lixeira' : `${gone.length} itens enviados para a Lixeira`, duration: 3000 });
    } else {
      setDeleting({ paths: paths.filter((p) => !gone.some((g) => under(g, p))), error: failure, busy: false });
    }
  }

  // O Tree só mostra seta e abre uma pasta que tem filhos. Por isso toda pasta ainda sem lista guardada recebe
  // um filho provisório (não selecionável), e a pasta não parece folha. Ele some quando a lista chega.
  // Linha: nome na cor da decoração e, à direita, a letra (arquivo) ou "•" (pasta). Ignorado fica cinza e sem letra.
  const decos = decorations(git?.files ?? []);
  const nodes = (dir: string): TreeDataNode[] => withNewItem(dir, (tree.children[dir] ?? []).map((entry): TreeDataNode => {
      const path = join(dir, entry.name);
      const deco = decos.get(path) ?? (entry.ignored ? IGNORED : null);
      const title = renaming?.path === path ? (
        <NewItemInput
          kind={entry.dir ? 'dir' : 'file'} label="Novo nome" selectBase={!entry.dir}
          name={renaming.name} busy={renaming.busy} error={renaming.error || renameIssue(renaming)}
          onChange={(name) => setRenaming((prev) => prev && { ...prev, name, error: '' })}
          onSubmit={(leaving) => void submitRename(leaving)}
          onCancel={(leaving) => endRename(path, !!leaving)}
        />
      ) : (
        <span className="flex min-w-0 flex-1 items-center gap-2" style={deco ? { color: deco.color } : undefined}>
          <Text as="span" ellipsis title={deco ? `${entry.name} · ${deco.title}` : entry.name} data-tree-path={path} className="min-w-0 flex-1" style={{ color: 'inherit' }}>{entry.name}</Text>
          {/* Fora do nome acessível da linha (o estado vai no title do nome), para a linha se chamar só pelo nome do item. */}
          {deco?.letter && <span aria-hidden="true" className="shrink-0 pr-1 text-xs font-medium tabular-nums">{deco.letter}</span>}
        </span>
      );
      const icon = <FileIcon path={entry.name} dir={entry.dir} />;
      if (!entry.dir) {
        return { key: path, title, isLeaf: true, icon };
      }
      const list = tree.children[path];
      return {
        key: path,
        title,
        icon,
        children: list ? nodes(path) : [{
          key: `${path}/`,
          title: <Text as="span" type="secondary" fontSize={12}>Lendo a pasta…</Text>,
          isLeaf: true,
          selectable: false,
        }],
      };
    }));

  // A linha do nome do item novo, na pasta de destino já lida: pasta nova no topo, arquivo novo depois das pastas (a ordem do VS Code).
  // A chave termina em "/", como a do nó provisório: sem seleção, menu nem atalhos da árvore.
  function withNewItem(dir: string, list: TreeDataNode[]): TreeDataNode[] {
    if (!creating || creating.dir !== dir || (dir && !tree.children[dir])) return list;
    const row: TreeDataNode = {
      key: `${join(dir, ' novo')}/`,
      isLeaf: true,
      selectable: false,
      icon: <FileIcon path={creating.name.trim() || (creating.kind === 'dir' ? 'pasta' : 'arquivo')} dir={creating.kind === 'dir'} />,
      title: (
        <NewItemInput
          kind={creating.kind} name={creating.name} busy={creating.busy} error={creating.error || (creating.name.trim() && fileNameError(creating.name.trim())) || ''}
          onChange={(name) => setCreating((prev) => prev && { ...prev, name, error: '' })}
          onSubmit={() => void submitCreate()}
          onCancel={() => setCreating(null)}
        />
      ),
    };
    const at = creating.kind === 'dir' ? 0 : (tree.children[dir] ?? []).filter((e) => e.dir).length;
    return [...list.slice(0, at), row, ...list.slice(at)];
  }

  // Novo Arquivo e Nova Pasta da barra: na pasta selecionada (ou na do arquivo selecionado); sem seleção única, na raiz.
  const barDir = () => {
    const one = selection.length === 1 ? selection[0] : '';
    return one && isFile(one) ? splitPath(one).dir : one;
  };

  const deleteDrafts = deleting ? actions.dirtyOf(deleting.paths) : [];
  const deleteFolders = deleting ? deleting.paths.some((p) => !isFile(p)) : false;
  // Textos do VS Code: a pergunta com o nome (ou a contagem) e o detalhe de que dá para restaurar da Lixeira.
  const deleteOne = deleting?.paths.length === 1 ? splitPath(deleting.paths[0]).name : '';
  const deleteQuestion = !deleting ? '' : deleteOne
    ? `Tem certeza de que deseja excluir '${deleteOne}'${deleteFolders ? ' e o conteúdo dela' : ''}?`
    : `Tem certeza de que deseja excluir os ${deleting.paths.length} ${deleteFolders ? 'arquivos/pastas' : 'arquivos'} a seguir${deleteFolders ? ' e o conteúdo deles' : ''}?`;
  const deleteDetail = deleteOne
    ? `Você pode restaurar ${deleteFolders ? 'essa pasta' : 'esse arquivo'} da Lixeira.`
    : 'Você pode restaurar esses itens da Lixeira.';
  return (
    <Flexbox height="100%" className="group/explorer min-h-0">
      <Flexbox horizontal align="center" gap={2} height={36} paddingInline={12} flex="none">
        <Text as="h2" ellipsis weight={500} fontSize={12} type="secondary" className="min-w-0 flex-1">Arquivos</Text>
        <Flexbox horizontal gap={2} className="opacity-0 transition-opacity group-hover/explorer:opacity-100 group-focus-within/explorer:opacity-100">
          <HeaderAction icon={FilePlus} label="Novo Arquivo..." onClick={() => startCreate('file', barDir())} />
          <HeaderAction icon={FolderPlus} label="Nova Pasta..." onClick={() => startCreate('dir', barDir())} />
          <HeaderAction icon={RefreshCw} label="Atualizar" onClick={() => void reloadAll()} />
          <HeaderAction icon={ChevronsDownUp} label="Recolher Tudo" onClick={() => setTree((prev) => ({ ...prev, expanded: [] }))} />
        </Flexbox>
      </Flexbox>
      {/* Seção do projeto, como a da pasta no VS Code: recolhe a árvore inteira. */}
      <button
        type="button" aria-expanded={!collapsed} onClick={() => setCollapsed((v) => !v)}
        className="flex h-[22px] flex-none cursor-pointer items-center gap-1 px-1 text-left text-xs font-semibold text-foreground outline-none hover:bg-[var(--ant-color-fill-tertiary)] focus-visible:ring-2 focus-visible:ring-ring/50"
      >
        <ChevronRight aria-hidden="true" className={`size-4 shrink-0 transition-transform ${collapsed ? '' : 'rotate-90'}`} />
        <span className="truncate" title={project}>{project}</span>
      </button>
      {/* Cobre o resto do painel: tabIndex deixa o clique na área vazia focar o painel (sem anel, para não parecer um campo). */}
      <Flexbox
        ref={panel}
        flex={1}
        tabIndex={-1}
        style={collapsed ? { display: 'none' } : undefined}
        className="explorer-tree relative min-h-0 outline-none"
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
          <Flexbox paddingBlock={2}>
            {error && <Text as="p" role="status" type="secondary" fontSize={12} className="px-3 py-1">{error}</Text>}
            {!tree.children[''] && !error && (
              <Text as="p" type="secondary" fontSize={12} className="px-3 py-1">Lendo a pasta…</Text>
            )}
            <Tree
              treeData={nodes('')} expandedKeys={tree.expanded} selectedKeys={shown} multiple
              indent={8} size="small" showIcon blockNode
              styles={{ node: { height: 22, borderRadius: 0 }, switcher: { width: 16, height: 22 }, title: { paddingInline: 0 } }}
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

      {/* Excluir: vai para a Lixeira do Windows. A pergunta e o detalhe são os do VS Code; com mais de um item, a lista do que vai; e o
          aviso das alterações não salvas (que se perdem e fecham as abas). */}
      <FormDialog
        open={!!deleting}
        title={deleteQuestion}
        detail={deleteDetail}
        busy={!!deleting?.busy}
        onClose={() => setDeleting(null)}
        onConfirm={submitDelete}
      >
        {deleting && (
          <Flexbox gap={8} className="mt-2 empty:hidden">
            {deleting.paths.length > 1 && (
              <Flexbox style={{ maxHeight: 180, overflow: 'auto' }}>
                {deleting.paths.slice(0, LIST_MAX).map((p) => (
                  <span key={p} title={p} className="truncate">{p}</span>
                ))}
                {deleting.paths.length > LIST_MAX && <span className="opacity-70">e mais {deleting.paths.length - LIST_MAX}</span>}
              </Flexbox>
            )}
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
    </Flexbox>
  );
}

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
