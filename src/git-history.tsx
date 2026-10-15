// Histórico do Git na barra lateral: os commits do projeto, ou os que mexeram num arquivo (`request.path`), em páginas.
// Um commit abre a lista dos arquivos dele; um arquivo abre a alteração daquele commit no editor central.
// Os caminhos são os do commit (imutáveis): um arquivo renomeado depois aparece com o nome que tinha ali.
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { ActionIcon, Button, Text, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { ArrowLeft, RefreshCw } from 'lucide-react';
import { FileIcon } from './file-icon';
import { backend, message } from './backend';
import type { TabTarget } from './workspace-model';
import type { CommitDetail, CommitFile, GitKind } from './types';

/** Pedido de histórico: `path` null é o projeto inteiro. Um `id` novo sempre refaz a lista. */
export type HistoryRequest = { path: string | null; id: number };

// Um só contador para todo pedido de histórico (do menu da árvore e do cabeçalho da barra): dois pedidos nunca dividem o id,
// senão o painel acha que o segundo já foi carregado e mostra a lista anterior.
let historyIds = 0;
export const nextHistoryId = () => ++historyIds;

export const KIND: Record<GitKind, { letter: string; title: string }> = {
  modified: { letter: 'M', title: 'modificado' },
  added: { letter: 'A', title: 'novo' },
  deleted: { letter: 'D', title: 'apagado' },
  renamed: { letter: 'R', title: 'renomeado' },
  untracked: { letter: 'U', title: 'não rastreado' },
  conflicted: { letter: 'C', title: 'em conflito' },
};

/** Pasta e nome de um caminho relativo ao projeto. */
export function splitPath(path: string) {
  const slash = path.lastIndexOf('/');
  return { name: path.slice(slash + 1), dir: slash >= 0 ? path.slice(0, slash) : '' };
}

/** Commits por página, no histórico e no gráfico. */
export const PAGE = 50;
export const when = (iso: string, style: Intl.DateTimeFormatOptions) => {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleString('pt-BR', style);
};
/** Linha de status no lugar da lista (lendo, vazio, erro). */
export const note = (text: string, danger = false) => (
  <Text as="p" role="status" fontSize={12} type={danger ? 'danger' : 'secondary'} style={{ paddingInline: 12, paddingBlock: 6 }}>{text}</Text>
);

/** Copia para a área de transferência. Falha vira toast de erro; sucesso, toast curto. */
export async function copy(text: string, done: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast.success({ title: done, duration: 2000 });
  } catch (e) {
    toast.error({ title: message(e), duration: 5000 });
  }
}

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

  function reload(next: string | null) {
    const seq = ++listSeq.current;
    setScope(next);
    if (emptyOnReload) {
      setItems(null);
      setMore(false);
      setError('');
    }
    setLoading(true);
    fetchPage(0, next).then(
      (list) => {
        if (seq === listSeq.current) {
          setItems(list);
          setMore(list.length === PAGE);
          setError('');
        }
      },
      (e: unknown) => {
        if (seq === listSeq.current) setError(message(e));
      },
    ).finally(() => {
      if (seq === listSeq.current) setLoading(false);
    });
  }

  function loadMore() {
    if (!items || loading) return;
    const seq = listSeq.current;
    setLoading(true);
    fetchPage(items.length, scope).then(
      (list) => {
        if (seq === listSeq.current) {
          setItems((prev) => [...(prev ?? []), ...list]);
          setMore(list.length === PAGE);
        }
      },
      (e: unknown) => {
        if (seq === listSeq.current) setError(message(e));
      },
    ).finally(() => {
      if (seq === listSeq.current) setLoading(false);
    });
  }

  return { scope, items, more, loading, error, reload, loadMore };
}

/**
 * Detalhe do commit aberto: o hash, os dados e o erro. Abrir outro troca o detalhe; a resposta de um pedido já trocado é descartada.
 */
export function useCommitDetail() {
  const [openHash, setOpenHash] = useState<string | null>(null);
  const [detail, setDetail] = useState<CommitDetail | null>(null);
  const [detailError, setDetailError] = useState('');
  // Mesma regra da lista: só a resposta do pedido mais novo vale.
  const detailSeq = useRef(0);

  function open(hash: string) {
    const seq = ++detailSeq.current;
    setOpenHash(hash);
    setDetail(null);
    setDetailError('');
    backend.gitCommitDetail(hash).then(
      (next) => {
        if (seq === detailSeq.current) setDetail(next);
      },
      (e: unknown) => {
        if (seq === detailSeq.current) setDetailError(message(e));
      },
    );
  }

  function close() {
    ++detailSeq.current;
    setOpenHash(null);
    setDetail(null);
    setDetailError('');
  }

  return { openHash, detail, detailError, open, close };
}

// Busca de uma página do histórico: do projeto (`scope` null) ou só dos commits que mexeram no caminho.
const historyPage = (offset: number, scope: string | null) => backend.gitHistory(PAGE, offset, scope ?? undefined);

/**
 * Arquivos de um commit, uma linha de 24 px cada. Clique prévia, duplo clique abre de vez; o arquivo aberto no editor fica marcado.
 * É a lista que abre embaixo do commit, no histórico e no gráfico. `gutter` põe algo antes de cada linha (o gráfico desenha as faixas ali).
 */
export function CommitFileList({ hash, files, selected, onOpenTarget, gutter }: {
  hash: string;
  files: CommitFile[];
  selected: TabTarget | null | undefined;
  onOpenTarget: OpenCommitTarget;
  gutter?: (index: number) => ReactNode;
}) {
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
              onClick={() => onOpenTarget({ kind: 'commit', hash, path: f.path })}
              onDoubleClick={() => onOpenTarget({ kind: 'commit', hash, path: f.path }, { permanent: true })}
              aria-current={isOpen ? 'true' : undefined}
              title={`${full}: ${word}. Ver a alteração neste commit (duplo clique abre de vez)`}
              className={`flex h-6 min-w-0 flex-1 items-center gap-1.5 rounded-sm pl-6 pr-2 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50${isOpen ? ' bg-muted shadow-[inset_2px_0_0_var(--ant-color-primary)]' : ''}`}
            >
              <FileIcon path={f.path} className="size-3.5" />
              <Text as="span" fontSize={13} ellipsis className="max-w-[70%] shrink-0">{name}</Text>
              {dir && <Text as="span" type="secondary" fontSize={12} ellipsis className="min-w-0 flex-1">{dir}</Text>}
              <Text as="span" title={word} fontSize={12} weight={600} type="secondary" className="shrink-0 pl-1 font-mono">{letter}</Text>
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
  onOpenTarget: (target: TabTarget, options?: { permanent?: boolean }) => void;
  onShowChanges: () => void;
};

/** Lista paginada de commits (do projeto ou do arquivo) e, ao escolher um, os arquivos dele. */
export function HistoryPanel({ request, active, selected, onOpenTarget, onShowChanges }: HistoryPanelProps) {
  // Lista e detalhe são os mesmos dos gráficos (ver os hooks acima). Aqui recarregar esvazia a lista e fecha o commit aberto.
  const { scope, items: commits, more, loading, error, reload: reloadList, loadMore } = usePagedList(historyPage, true);
  const { openHash, detail, detailError, open: openCommit, close: closeCommit } = useCommitDetail();
  // Último pedido que já foi carregado: voltar à tela não recarrega a lista à toa.
  const loadedId = useRef<number | null>(null);

  // Escopo da lista carregada: null é o projeto. Vem do pedido, ou do "Projeto inteiro" da barra.
  function reload(path: string | null) {
    closeCommit();
    reloadList(path);
  }

  // Pedido novo (id diferente) ou primeira vez visível: recarrega o escopo pedido.
  useEffect(() => {
    if (!active || loadedId.current === request.id) return;
    loadedId.current = request.id;
    reload(request.path);
    // `reload` só lê refs e setters: não precisa estar nas dependências.
  }, [request.id, request.path, active]);

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
              <CommitFileList hash={commit.hash} files={files} selected={selected} onOpenTarget={onOpenTarget} />
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
              <button
                type="button" onClick={() => openCommit(c.hash)} title={`${c.short} · ${c.subject}`}
                className="flex w-full min-w-0 flex-col gap-0.5 rounded-sm px-3 py-1.5 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                <span className="flex min-w-0 items-baseline gap-2">
                  <span className="shrink-0 font-mono text-xs text-muted-foreground">{c.short}</span>
                  <Text as="span" ellipsis fontSize={13} className="min-w-0">{c.subject || '(sem mensagem)'}</Text>
                </span>
                <Text as="span" ellipsis fontSize={11} type="secondary">{c.author} · {when(c.date, { dateStyle: 'short', timeStyle: 'short' })}</Text>
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
        <Text as="h2" ellipsis weight={500} fontSize={12} type="secondary" className="min-w-0 flex-1">{inDetail ? 'Commit' : 'Histórico'}</Text>
        {!inDetail && <ActionIcon size="small" variant="borderless" icon={RefreshCw} aria-label="Atualizar o histórico" title="Atualizar o histórico" onClick={() => reload(scope)} />}
        <Button size="small" type="text" onClick={onShowChanges} title="Voltar ao controle de versão">Alterações</Button>
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
