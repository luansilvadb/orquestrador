// Seção "Gráfico" do controle de versão: os commits do HEAD e do upstream da branch atual, no estilo do Source Control do VS Code.
// Cada linha tem as faixas desenhadas à esquerda (layoutGraph), o assunto, as etiquetas das refs e o autor. Clique num commit abre
// os arquivos dele ali mesmo, e clique num arquivo abre a alteração daquele commit. Só consulta com a tela Git visível e a seção aberta.
import { useEffect, useMemo, useRef } from 'react';
import { ActionIcon, Button, ContextMenuTrigger, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { ArrowDownToLine, ArrowUpFromLine, ChevronDown, ChevronRight, Cloud, CloudDownload, GitBranch, RefreshCw, Tag } from 'lucide-react';
import { backend } from './backend';
import { CommitFileList, copy, note, type OpenCommitTarget, PAGE, useCommitDetail, usePagedList, when } from './git-history';
import { type GraphLine, type GraphRow, layoutGraph } from './git-graph';
import type { TabTarget } from './workspace-model';
import type { GitRef } from './types';

// Medidas em px: o passo horizontal de uma faixa, a linha de commit e a linha de arquivo (a mesma do painel).
const LANE = 12;
const ROW = 22;
const FILE_ROW = 24;
// Altura da seção aberta. O gráfico e a lista de arquivos não descem do mínimo ao arrastar o divisor; setas andam de 16 em 16.
export const GRAPH_DEFAULT = 260;
export const GRAPH_MIN = 96;
export const FILES_MIN = 96;
const KEY_STEP = 16;
// Cores das faixas: variáveis do tema do antd, então acompanham o tema claro e o escuro.
const PALETTE = ['--ant-color-primary', '--ant-color-success', '--ant-color-warning', '--ant-color-error', '--ant-color-info'].map((v) => `var(${v})`);
const colorOf = (n: number) => PALETTE[n % PALETTE.length];
// Busca de uma página do gráfico: o gráfico é do HEAD e do upstream, sem escopo de arquivo.
const graphPage = (offset: number) => backend.gitGraph(PAGE, offset);

export type GraphRemote = 'fetch' | 'pull' | 'push';
export type GraphSectionProps = {
  open: boolean;
  onToggle: () => void;
  /** Altura dada pelo divisor (só vale com a seção aberta). O painel guarda o valor. */
  height: number;
  onHeight: (height: number) => void;
  /** HEAD atual (hash completo, '' sem commit): a linha dele é o anel vazado. */
  head: string;
  /** Muda quando o gráfico deve recarregar: HEAD, branch, ahead, behind ou pedido de atualizar. Quem monta o texto é o painel. */
  stamp: string;
  /** Tela Git aberta e lista de mudanças visível. Sem isso, não consulta o Git. */
  active: boolean;
  selected: TabTarget | null | undefined;
  onOpenTarget: OpenCommitTarget;
  onRefresh: () => void;
  /** Rótulo e trava de cada ação remota, vindos do painel (que tem as travas de busy e de locked). */
  remote: Record<GraphRemote, { label: string; disabled: boolean }>;
  /** Ação remota em curso (mostra o carregando no botão). */
  running: GraphRemote | null;
  onRemote: (kind: GraphRemote) => void;
};

// Posição do centro de uma faixa, na coluna dela.
const laneX = (lane: number) => lane * LANE + LANE / 2;
// Caminho de um segmento: reto quando a faixa não muda; curva com tangente vertical quando muda de faixa.
function segment(x1: number, y1: number, x2: number, y2: number) {
  if (x1 === x2) return `M${x1} ${y1}V${y2}`;
  const mid = (y1 + y2) / 2;
  return `M${x1} ${y1}C${x1} ${mid} ${x2} ${mid} ${x2} ${y2}`;
}

/** Faixas de uma linha de commit: a metade de cima liga a borda de cima ao meio, a de baixo liga o meio à borda de baixo. */
function CommitLanes({ row, head }: { row: GraphRow; head: boolean }) {
  const mid = ROW / 2;
  return (
    <svg width={row.width * LANE} height={ROW} className="shrink-0" aria-hidden>
      {row.lines.map((line: GraphLine, i) => {
        const top = line.half === 'top';
        return (
          <path
            key={i} d={segment(laneX(line.from), top ? 0 : mid, laneX(line.to), top ? mid : ROW)}
            fill="none" strokeWidth={1.5} style={{ stroke: colorOf(line.color) }}
          />
        );
      })}
      {/* O HEAD é um anel vazado; os outros commits, ponto cheio. */}
      <circle
        cx={laneX(row.col)} cy={mid} r={4}
        strokeWidth={head ? 2 : 0}
        style={head ? { fill: 'var(--ant-color-bg-container)', stroke: colorOf(row.color) } : { fill: colorOf(row.color) }}
      />
    </svg>
  );
}

/** Faixas que seguem para baixo, ao lado de uma linha de arquivo de um commit aberto: retas verticais de borda a borda. */
function GapLanes({ row }: { row: GraphRow }) {
  return (
    <svg width={row.width * LANE} height={FILE_ROW} className="shrink-0" aria-hidden>
      {row.lines.filter((l) => l.half === 'bottom').map((l, i) => (
        <path key={i} d={`M${laneX(l.to)} 0V${FILE_ROW}`} fill="none" strokeWidth={1.5} style={{ stroke: colorOf(l.color) }} />
      ))}
    </svg>
  );
}

/**
 * Etiqueta de uma ref: branch (a atual destacada), remota com nuvem, tag com a etiqueta. Na linha estreita, as que não são a atual
 * viram só o ícone (o nome fica no title e aparece quando a linha tem espaço). A atual sempre mostra o nome, com reticências.
 */
function RefBadge({ item }: { item: GitRef }) {
  const Icon = item.kind === 'remote' ? Cloud : item.kind === 'tag' ? Tag : GitBranch;
  const kind = item.kind === 'remote' ? 'remota' : item.kind === 'tag' ? 'tag' : 'branch';
  return (
    <span
      title={`${item.name} (${item.current ? `${kind} atual` : kind})`}
      className={`flex min-w-0 max-w-[9rem] shrink items-center gap-1 rounded-sm px-1${item.current ? ' bg-muted text-foreground' : ' bg-muted text-muted-foreground'}`}
    >
      <Icon className="size-3 shrink-0" aria-hidden />
      <Text as="span" ellipsis fontSize={11} className={`min-w-0${item.current ? ' font-semibold' : ' hidden @[22rem]:inline'}`}>{item.name}</Text>
    </span>
  );
}

/** Nota no lugar dos arquivos de um commit aberto (lendo, vazio ou erro), com as faixas que seguem. */
function DetailNote({ row, text, danger = false }: { row: GraphRow; text: string; danger?: boolean }) {
  return (
    <ul>
      <li className="flex min-w-0">
        <GapLanes row={row} />
        <Text as="p" role="status" fontSize={12} type={danger ? 'danger' : 'secondary'} className="flex h-6 min-w-0 items-center pl-6">{text}</Text>
      </li>
    </ul>
  );
}

/**
 * A seção "Gráfico" do painel: cabeçalho com o chevron, o título e as ações (Buscar, Receber, Enviar e atualizar).
 * Aberta, ocupa `height` e tem o divisor acima (arrastar ou setas); fechada, sobra só o cabeçalho. Cada parte rola por conta própria.
 */
export function GraphSection({
  open, onToggle, height, onHeight, head, stamp, active, selected, onOpenTarget, onRefresh, remote, running, onRemote,
}: GraphSectionProps) {
  // Lista paginada (a anterior fica na tela até a nova chegar) e detalhe do commit aberto: os mesmos hooks do histórico.
  const { items: commits, more, loading, error, reload: reloadList, loadMore } = usePagedList(graphPage, false);
  const { openHash, detail, detailError, open: openCommit, close: closeCommit } = useCommitDetail();
  // Chave da última carga: enquanto ela não muda, voltar à seção ou à tela não recarrega.
  const loaded = useRef<string | null>(null);
  // Faixas calculadas sobre todas as linhas carregadas, de cima para baixo (a página nova continua as faixas da anterior).
  const rows = useMemo(() => layoutGraph(commits ?? []), [commits]);

  function reload(key: string) {
    loaded.current = key;
    reloadList(null);
  }

  // Só carrega com a tela visível e a seção aberta, e só quando a chave mudou desde a última carga.
  useEffect(() => {
    if (!active || !open || loaded.current === stamp) return;
    reload(stamp);
    // `reload` só lê refs e setters: não precisa estar nas dependências.
  }, [active, open, stamp]);

  // Escolher o mesmo commit de novo fecha. Trocar de commit descarta a lista anterior e pede a nova.
  function toggleCommit(hash: string) {
    if (openHash === hash) closeCommit();
    else openCommit(hash);
  }

  // Divisor: arrastar ou setas. O máximo deixa a lista de arquivos no mínimo; o espaço é o da lista (irmã anterior) mais a seção.
  const drag = useRef<{ y: number; start: number; max: number } | null>(null);
  const maxFor = (divider: HTMLElement) => ((divider.previousElementSibling as HTMLElement | null)?.clientHeight ?? 0) + height - FILES_MIN;
  const clamp = (n: number, max: number) => Math.max(GRAPH_MIN, Math.min(n, max));

  const remoteButton = (kind: GraphRemote, icon: typeof CloudDownload) => (
    <ActionIcon
      size="small" variant="borderless" icon={icon} disabled={remote[kind].disabled} loading={running === kind}
      aria-label={remote[kind].label} title={remote[kind].label} onClick={() => onRemote(kind)}
    />
  );

  const header = (
    <Flexbox horizontal align="center" gap={2} paddingInline={4} className="shrink-0">
      <button
        type="button" onClick={onToggle} aria-expanded={open}
        className="flex min-w-0 flex-1 items-center gap-1 rounded-sm py-1 pl-1 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50"
      >
        {open
          ? <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
          : <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />}
        <Text as="span" ellipsis weight={600} fontSize={12} className="min-w-0 shrink">Gráfico</Text>
      </button>
      {remoteButton('fetch', CloudDownload)}
      {remoteButton('pull', ArrowDownToLine)}
      {remoteButton('push', ArrowUpFromLine)}
      <ActionIcon
        size="small" variant="borderless" icon={RefreshCw} loading={loading}
        aria-label="Atualizar o gráfico" title="Atualizar o gráfico" onClick={onRefresh}
      />
    </Flexbox>
  );

  let list;
  if (!commits) list = note(error || 'Lendo o gráfico…', !!error);
  else if (!commits.length) list = note(error || (head ? 'Nenhum commit para mostrar.' : 'Este repositório ainda não tem commit.'), !!error);
  else {
    list = (
      <>
        <ul className="grid grid-cols-[minmax(0,1fr)]">
          {commits.map((c, i) => {
            const row = rows[i];
            const isHead = c.hash === head;
            const isOpen = openHash === c.hash;
            const names = c.refs.map((r) => (r.current ? `${r.name} (atual)` : r.name)).join(', ');
            const menu = [
              { key: 'hash', label: 'Copiar hash', onClick: () => void copy(c.hash, 'Hash copiado') },
              { key: 'subject', label: 'Copiar assunto', onClick: () => void copy(c.subject, 'Assunto copiado') },
            ];
            return (
              <li key={c.hash} data-commit={c.hash} data-head={isHead ? 'true' : undefined}>
                <ContextMenuTrigger items={menu}>
                  <div>
                    <button
                      type="button" onClick={() => toggleCommit(c.hash)} aria-expanded={isOpen}
                      title={`${c.short} — ${c.subject || '(sem mensagem)'}. ${c.author}, ${when(c.date, { dateStyle: 'short', timeStyle: 'short' })}.${names ? ` Etiquetas: ${names}.` : ''}`}
                      className={`@container flex h-[22px] w-full min-w-0 items-center gap-2 rounded-sm pr-2 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50${isOpen ? ' bg-muted' : ''}`}
                    >
                      <CommitLanes row={row} head={isHead} />
                      {/* O assunto tem prioridade: base zero (só cresce) e piso de 6rem. Etiquetas e autor encolhem antes dele. */}
                      <Text as="span" ellipsis fontSize={13} weight={isHead ? 600 : 400} className="min-w-[6rem] flex-1">{c.subject || '(sem mensagem)'}</Text>
                      {c.refs.map((r) => <RefBadge key={`${r.kind}:${r.name}`} item={r} />)}
                      <Text as="span" ellipsis fontSize={11} type="secondary" className="hidden min-w-0 max-w-[30%] shrink @[24rem]:inline">{c.author}</Text>
                    </button>
                  </div>
                </ContextMenuTrigger>
                {isOpen && (detail && detail.commit.hash === c.hash
                  ? (detail.files.length
                    ? <CommitFileList hash={c.hash} files={detail.files} selected={selected} onOpenTarget={onOpenTarget} gutter={() => <GapLanes row={row} />} />
                    : <DetailNote row={row} text="Nenhum arquivo neste commit." />)
                  : <DetailNote row={row} text={detailError || 'Lendo os arquivos…'} danger={!!detailError} />)}
              </li>
            );
          })}
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
    <>
      {open && (
        <div
          role="separator" tabIndex={0} aria-orientation="horizontal" aria-label="Divisor do gráfico" aria-valuemin={GRAPH_MIN} aria-valuenow={height}
          title="Arrastar para redimensionar (setas do teclado também)"
          className="h-1 shrink-0 cursor-row-resize bg-transparent outline-none hover:bg-border focus-visible:bg-ring"
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            drag.current = { y: e.clientY, start: height, max: maxFor(e.currentTarget) };
          }}
          onPointerMove={(e) => {
            const d = drag.current;
            if (d) onHeight(clamp(d.start - (e.clientY - d.y), d.max));
          }}
          onPointerUp={() => { drag.current = null; }}
          onKeyDown={(e) => {
            const step = e.key === 'ArrowUp' ? KEY_STEP : e.key === 'ArrowDown' ? -KEY_STEP : 0;
            if (!step) return;
            e.preventDefault();
            onHeight(clamp(height + step, maxFor(e.currentTarget)));
          }}
        />
      )}
      <section className="flex shrink-0 flex-col" style={open ? { height } : undefined}>
        {header}
        {open && (
          <Flexbox className="min-h-0 overflow-y-auto overflow-x-hidden" flex={1} paddingBlock={4} style={{ paddingRight: 8 }}>
            {list}
          </Flexbox>
        )}
      </section>
    </>
  );
}
