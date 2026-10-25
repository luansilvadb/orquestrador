// Histórico do Git na barra lateral: os commits do projeto, ou os que mexeram num arquivo (`request.path`), em páginas.
// Um commit abre a lista dos arquivos dele; um arquivo abre a alteração daquele commit no editor central.
// Os caminhos são os do commit (imutáveis): um arquivo renomeado depois aparece com o nome que tinha ali.
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { ActionIcon, Button, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { ArrowLeft, Cloud, GitBranch, RefreshCw, Tag, Target } from 'lucide-react';
import { FileIcon } from '../shared/file-icon';
import { splitPath } from '../shared/paths';
import { spinIcon } from '../shared/spin';
import { KIND_COLOR } from '../explorer/explorer-decorations';
import { message } from '../backend/backend';
import { useBackend } from '../backend/backend-context';
import type { TabTarget } from '../editor/workspace-model';
import type { CommitDetail, CommitFile, GitKind, GitRef } from '../backend/types';

/** Pedido de histórico: `path` null é o projeto inteiro. Um pedido novo (outro objeto) sempre refaz a lista. */
export type HistoryRequest = { path: string | null };

export const KIND: Record<GitKind, { letter: string; title: string }> = {
  modified: { letter: 'M', title: 'modificado' },
  added: { letter: 'A', title: 'novo' },
  deleted: { letter: 'D', title: 'apagado' },
  renamed: { letter: 'R', title: 'renomeado' },
  untracked: { letter: 'U', title: 'não rastreado' },
  conflicted: { letter: 'C', title: 'em conflito' },
};

/** Commits por página, no histórico e no gráfico. */
export const PAGE = 50;
export const when = (iso: string, style: Intl.DateTimeFormatOptions) => {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleString('pt-BR', style);
};

const commitDetails = new Map<string, CommitDetail>();
const commitDetailRequests = new Map<string, Promise<CommitDetail>>();
// Cores das etiquetas: branch local em azul, remota em lilás, tag em laranja (historyItemRefColor, RemoteRefColor e BaseRefColor).
const REF_COLOR = { local: '#59a4f9', remote: '#b180d7', tag: '#ea5c00' } as const;

/**
 * Etiqueta de uma ref, como a do grafo do Antigravity: pílula de 18 px e raio de 10 px, na cor do tipo (branch azul, remota lilás,
 * tag laranja), com o texto escuro de 12 px. A branch atual tem o alvo e sempre mostra o nome; as outras, na linha estreita, viram
 * só o ícone (o nome fica no title e aparece quando a linha tem espaço).
 */
export function RefBadge({ item }: { item: GitRef }) {
  const Icon = item.current ? Target : item.kind === 'remote' ? Cloud : item.kind === 'tag' ? Tag : GitBranch;
  const kind = item.kind === 'remote' ? 'remota' : item.kind === 'tag' ? 'tag' : 'branch';
  const color = item.kind === 'remote' ? REF_COLOR.remote : item.kind === 'tag' ? REF_COLOR.tag : REF_COLOR.local;
  return (
    <span
      title={`${item.name} (${item.current ? `${kind} atual` : kind})`}
      className="flex h-[18px] min-w-0 max-w-[9rem] shrink items-center rounded-[10px] text-[#181818]"
      style={{ backgroundColor: color }}
    >
      <Icon className="size-4 shrink-0 p-px" aria-hidden />
      <span className={`min-w-0 truncate pr-1 text-[12px] leading-[18px]${item.current ? '' : ' hidden @[22rem]:inline'}`}>{item.name}</span>
    </span>
  );
}

const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 31_536_000], ['month', 2_592_000], ['week', 604_800], ['day', 86_400], ['hour', 3_600], ['minute', 60], ['second', 1],
];
export function relativeDate(iso: string) {
  const timestamp = Date.parse(iso);
  if (!Number.isFinite(timestamp)) return iso;
  const seconds = (timestamp - Date.now()) / 1_000;
  const [unit, amount] = RELATIVE_UNITS.find(([, size]) => Math.abs(seconds) >= size) ?? ['second', 1];
  return new Intl.RelativeTimeFormat('pt-BR', { numeric: 'auto' }).format(Math.round(seconds / amount), unit);
}

/** Linha de status no lugar da lista (lendo, vazio, erro). */
export const note = (text: string, danger = false) => (
  <Text as="p" role="status" fontSize={12} type={danger ? 'danger' : 'secondary'} style={{ paddingInline: 12, paddingBlock: 6 }}>{text}</Text>
);

export type OpenCommitTarget = (target: TabTarget, options?: { permanent?: boolean }) => void;

/**
 * Lista paginada de commits: a primeira página (`reload`), as seguintes (`loadMore`) e o escopo carregado.
 * `emptyOnReload` (histórico): recarregar esvazia a lista antes de buscar. Sem ele (gráfico), a lista anterior fica na tela até a nova chegar.
 * A resposta de um pedido já trocado por outro é descartada.
 */
export function usePagedList<T>(fetchPage: (offset: number, scope: string | null) => Promise<T[]>, emptyOnReload: boolean) {
  const [scope, setScope] = useState<string | null>(null);
  const [items, setItems] = useState<T[] | null>(null);
  const [more, setMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  // Cada pedido tem o seu número: a resposta de um pedido já trocado por outro é descartada.
  const listSeq = useRef(0);

  // Busca uma página: a primeira troca a lista (e limpa o erro), as seguintes (`add`) entram no fim.
  function page(offset: number, scope: string | null, seq: number, add: boolean) {
    setLoading(true);
    fetchPage(offset, scope).then(
      (list) => {
        if (seq !== listSeq.current) return;
        setItems((p) => (add ? [...(p ?? []), ...list] : list));
        setMore(list.length === PAGE);
        if (!add) setError('');
      },
      (e: unknown) => {
        if (seq === listSeq.current) setError(message(e));
      },
    ).finally(() => {
      if (seq === listSeq.current) setLoading(false);
    });
  }

  function reload(next: string | null) {
    setScope(next);
    if (emptyOnReload) {
      setItems(null);
      setMore(false);
      setError('');
    }
    page(0, next, ++listSeq.current, false);
  }

  function loadMore() {
    if (items && !loading) page(items.length, scope, listSeq.current, true);
  }

  return { scope, items, more, loading, error, reload, loadMore };
}

/**
 * Detalhe do commit aberto: o hash, os dados e o erro. Abrir outro troca o detalhe; a resposta de um pedido já trocado é descartada.
 */
export function useCommitDetail() {
  const backend = useBackend();
  const [openHash, setOpenHash] = useState<string | null>(null);
  const [detail, setDetail] = useState<CommitDetail | null>(null);
  const [detailError, setDetailError] = useState('');
  // Mesma regra da lista: só a resposta do pedido mais novo vale.
  const detailSeq = useRef(0);
  const pendingHash = useRef<string | null>(null);

  function request(hash: string) {
    const cached = commitDetails.get(hash);
    if (cached) return Promise.resolve(cached);
    let pending = commitDetailRequests.get(hash);
    if (!pending) {
      pending = backend.gitCommitDetail(hash).then((next) => {
        commitDetails.set(hash, next);
        return next;
      }).finally(() => commitDetailRequests.delete(hash));
      commitDetailRequests.set(hash, pending);
    }
    return pending;
  }

  function prefetch(hash: string) {
    void request(hash).catch(() => {});
  }

  function open(hash: string) {
    const seq = ++detailSeq.current;
    const cached = commitDetails.get(hash);
    pendingHash.current = cached ? null : hash;
    if (cached) {
      setOpenHash(hash);
      setDetail(cached);
      setDetailError('');
      return;
    }
    const timer = window.setTimeout(() => {
      if (seq !== detailSeq.current) return;
      setOpenHash(hash);
      setDetail(null);
      setDetailError('');
    }, 150);
    request(hash).then(
      (next) => {
        window.clearTimeout(timer);
        if (seq === detailSeq.current) {
          pendingHash.current = null;
          setOpenHash(hash);
          setDetail(next);
          setDetailError('');
        }
      },
      (e: unknown) => {
        window.clearTimeout(timer);
        if (seq === detailSeq.current) {
          pendingHash.current = null;
          setOpenHash(hash);
          setDetail(null);
          setDetailError(message(e));
        }
      },
    );
  }

  function close() {
    ++detailSeq.current;
    pendingHash.current = null;
    setOpenHash(null);
    setDetail(null);
    setDetailError('');
  }

  function toggle(hash: string) {
    if (openHash === hash || pendingHash.current === hash) close();
    else open(hash);
  }

  return { openHash, detail, detailError, open, toggle, prefetch, close };
}

// Busca de uma página do histórico: do projeto (`scope` null) ou só dos commits que mexeram no caminho.

const HOVER_PREFETCH_MS = 100;

/**
 * Arquivos de um commit, uma linha de 24 px cada. Clique prévia, duplo clique abre de vez; o arquivo aberto no editor fica marcado.
 * É a lista que abre embaixo do commit, no histórico e no gráfico. `gutter` põe algo antes de cada linha (o gráfico desenha as faixas ali).
 */
export function CommitFileList({
  hash, files, selected, onOpenTarget, onPrefetchTarget, gutter }: {
  hash: string;
  files: CommitFile[];
  selected: TabTarget | null | undefined;
  onOpenTarget: OpenCommitTarget;
  onPrefetchTarget: (target: TabTarget, hover?: boolean) => void;
  gutter?: (index: number) => ReactNode;
}) {
  // Permanência curta no hover aquece leitura e realce antes do clique; varrer a lista sem parar não lê nada.
  const hoverTimer = useRef(0);
  const cancelHover = () => window.clearTimeout(hoverTimer.current);
  useEffect(() => cancelHover, []);
  // A coluna não pode crescer com o nome longo: sem o minmax(0,1fr), o texto empurra a lista para fora.
  return (
    <ul className="grid grid-cols-[minmax(0,1fr)]">
      {files.map((f, index) => {
        const { letter, title: word } = KIND[f.kind];
        const { name, dir } = splitPath(f.path);
        const isOpen = selected?.kind === 'commit' && selected.hash.toLowerCase() === hash.toLowerCase() && selected.path === f.path;
        const full = f.origPath ? `${f.path} (renomeado de ${f.origPath})` : f.path;
        return (
          <li key={f.path} className="flex min-w-0">
            {gutter?.(index)}
            <button
              type="button"
              onMouseEnter={() => { cancelHover(); hoverTimer.current = window.setTimeout(() => onPrefetchTarget({ kind: 'commit', hash, path: f.path }, true), HOVER_PREFETCH_MS); }}
              onMouseLeave={cancelHover}
              onMouseDown={(e) => { cancelHover(); if (e.button === 0 && !e.ctrlKey && !e.metaKey && !e.shiftKey) onPrefetchTarget({ kind: 'commit', hash, path: f.path }); }}
              onClick={() => onOpenTarget({ kind: 'commit', hash, path: f.path })}
              onDoubleClick={() => onOpenTarget({ kind: 'commit', hash, path: f.path }, { permanent: true })}
              aria-current={isOpen ? 'true' : undefined}
              title={`${full}: ${word}. Ver a alteração neste commit (duplo clique abre de vez)`}
              className={`flex h-[22px] min-w-0 flex-1 items-center gap-1.5 pl-5 pr-3 text-left text-[13px] outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset${isOpen ? ' bg-muted shadow-[inset_2px_0_0_var(--ant-color-primary)]' : ''}`}
            >
              {/* Como no painel do Antigravity: nome e letra na cor do tipo de alteração (gitDecoration), a pasta em tamanho menor. */}
              <FileIcon path={f.path} className="size-4" />
              <span className="max-w-[70%] shrink-0 truncate" style={{ color: KIND_COLOR[f.kind] }}>{name}</span>
              {dir && <span className="min-w-0 flex-1 truncate text-[0.9em] opacity-70">{dir}</span>}
              <span title={word} className="ml-auto shrink-0 pl-1 text-[12px]" style={{ color: KIND_COLOR[f.kind] }}>{letter}</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

export type HistoryPanelProps = {
  request: HistoryRequest;
  /** Visível e com a tela Git aberta. Só então carrega; a lista fica na memória enquanto o painel está escondido. */
  active: boolean;
  selected: TabTarget | null | undefined;
  onOpenTarget: OpenCommitTarget;
  onPrefetchTarget: (target: TabTarget, hover?: boolean) => void;
};

/** Lista paginada de commits (do projeto ou do arquivo) e, ao escolher um, os arquivos dele. */
export function HistoryPanel({
  request, active, selected, onOpenTarget, onPrefetchTarget }: HistoryPanelProps) {
  const backend = useBackend();
  // Lista e detalhe são os mesmos dos gráficos (ver os hooks acima). Aqui recarregar esvazia a lista e fecha o commit aberto.
  const { scope, items: commits, more, loading, error, reload: reloadList, loadMore } = usePagedList((offset, scope) => backend.gitHistory(PAGE, offset, scope ?? undefined), true);
  const { openHash, detail, detailError, toggle: toggleCommit, prefetch: prefetchCommit, close: closeCommit } = useCommitDetail();
  // Último pedido que já foi carregado: voltar à tela não recarrega a lista à toa.
  const loaded = useRef<HistoryRequest | null>(null);

  // Escopo da lista carregada: null é o projeto. Vem do pedido, ou do "Projeto inteiro" da barra.
  function reload(path: string | null) {
    closeCommit();
    reloadList(path);
  }

  // Pedido novo ou primeira vez visível: recarrega o escopo pedido.
  useEffect(() => {
    if (!active || loaded.current === request) return;
    loaded.current = request;
    reload(request.path);
    // `reload` só lê refs e setters: não precisa estar nas dependências.
  }, [request, active]);

  const inDetail = openHash !== null;

  let body;
  if (inDetail) {
    if (!detail) body = note(detailError || 'Lendo o commit…', !!detailError);
    else {
      const { commit, files } = detail;
      body = (
        <>
          <Flexbox gap={2} paddingInline={12} paddingBlock={6} className="border-b">
            <Text as="p" weight={600} fontSize={13} className="select-text break-words">{commit.subject || '(sem mensagem)'}</Text>
            <Text as="code" code fontSize={11} type="secondary" className="select-text break-all">{commit.hash}</Text>
            <Text as="p" fontSize={12} type="secondary" className="select-text">
              {commit.author} · {when(commit.date, { dateStyle: 'medium', timeStyle: 'short' })}
            </Text>
          </Flexbox>
          {!files.length && note('Nenhum arquivo neste commit.')}
          {files.length > 0 && (
            <>
              <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 12, paddingBlock: 4 }}>
                {files.length === 1 ? '1 arquivo' : `${files.length} arquivos`}
              </Text>
              <CommitFileList hash={commit.hash} files={files} selected={selected} onOpenTarget={onOpenTarget} onPrefetchTarget={onPrefetchTarget} />
            </>
          )}
        </>
      );
    }
  } else if (!commits) body = note(error || 'Lendo o histórico…', !!error);
  else if (!commits.length) body = note(error || (scope ? 'Nenhum commit mudou este arquivo.' : 'Nenhum commit neste repositório.'), !!error);
  else {
    body = (
      <>
        <ul className="grid grid-cols-[minmax(0,1fr)]">
          {commits.map((c) => (
            <li key={c.hash}>
              {/* Duas linhas, como no histórico do Zed: o assunto e as refs; embaixo a inicial do autor, o autor, a data relativa e o hash. */}
              <button
                type="button"
                onMouseDown={(e) => { if (e.button === 0 && !e.ctrlKey && !e.metaKey && !e.shiftKey) prefetchCommit(c.hash); }}
                onClick={() => toggleCommit(c.hash)} title={`${c.short} · ${c.subject}`}
                className="@container flex w-full min-w-0 flex-col gap-0.5 px-3 py-1.5 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
              >
                <span className="flex w-full min-w-0 items-center gap-1">
                  <Text as="span" ellipsis fontSize={13} className="min-w-[6rem] flex-1">{c.subject || '(sem mensagem)'}</Text>
                  {c.refs.map((r) => <RefBadge key={`${r.kind}:${r.name}`} item={r} />)}
                </span>
                <span className="flex w-full min-w-0 items-center gap-1.5 text-[11px] text-[var(--ant-color-text-secondary)]">
                  <span aria-hidden className="flex size-4 shrink-0 items-center justify-center rounded-full bg-[var(--ant-color-fill-secondary)] text-[10px] uppercase leading-none">
                    {c.author.trim().charAt(0) || '?'}
                  </span>
                  <span className="min-w-0 truncate">{c.author}</span>
                  <span aria-hidden>·</span>
                  <span className="shrink-0" title={when(c.date, { dateStyle: 'medium', timeStyle: 'short' })}>{relativeDate(c.date)}</span>
                  <span aria-hidden>·</span>
                  <span className="shrink-0 font-mono">{c.short}</span>
                </span>
              </button>
            </li>
          ))}
        </ul>
        {error && note(error, true)}
        {more && (
          <Flexbox paddingInline={12} paddingBlock={6}>
            <Button size="small" className="w-full" loading={loading} onClick={loadMore}>Carregar mais</Button>
          </Flexbox>
        )}
      </>
    );
  }

  return (
    <Flexbox className="min-h-0" flex={1}>
      <Flexbox horizontal align="center" gap={2} paddingInline={12} paddingBlock={6}>
        {inDetail && (
          <ActionIcon size="small" variant="borderless" icon={ArrowLeft} aria-label="Voltar à lista de commits" title="Voltar à lista" onClick={closeCommit} />
        )}
        <Text as="h2" ellipsis weight={500} fontSize={12} color="var(--ant-color-text-secondary)" className="min-w-0 flex-1">{inDetail ? 'Commit' : 'Histórico'}</Text>
        {!inDetail && <ActionIcon size="small" variant="borderless" icon={RefreshCw} aria-label="Atualizar o histórico" title="Atualizar o histórico" onClick={(e) => { spinIcon(e.currentTarget); reload(scope); }} />}
      </Flexbox>
      {!inDetail && scope && (
        <Flexbox horizontal align="center" gap={6} paddingInline={12} paddingBlock={4}>
          <FileIcon path={scope} className="size-3.5" />
          <Text as="code" code fontSize={12} ellipsis title={scope} className="min-w-0 flex-1">{splitPath(scope).name}</Text>
          <Button size="small" type="text" onClick={() => reload(null)} title="Mostrar os commits do projeto inteiro">Projeto inteiro</Button>
        </Flexbox>
      )}
      <Flexbox className="min-h-0 overflow-y-auto overflow-x-hidden" flex={1} paddingBlock={4}>
        {body}
      </Flexbox>
    </Flexbox>
  );
}
