// Seção "Grafo" do controle de versão: os commits do HEAD e do upstream da branch atual, no estilo do Source Control do VS Code.
// Cada linha tem as faixas desenhadas à esquerda (layoutGraph), o assunto, as etiquetas das refs e o autor. Clique num commit abre
// os arquivos dele ali mesmo, e clique num arquivo abre a alteração daquele commit. Só consulta com a tela Git visível e a seção aberta.
import { useEffect, useMemo, useRef } from 'react';
import { ActionIcon, Button, ContextMenuTrigger, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { ArrowDownToLine, ArrowUpFromLine, ChevronRight, Cloud, CloudDownload, GitBranch, RefreshCw, Tag, Target } from 'lucide-react';
import { backend } from '../backend/backend';
import { CommitFileList, note, type OpenCommitTarget, PAGE, useCommitDetail, usePagedList, when } from './git-history';
import { copy } from '../shared/paths';
import { type GraphLine, type GraphRow, layoutGraph } from './git-graph';
import type { TabTarget } from '../editor/workspace-model';
import type { GitRef } from '../backend/types';

// Medidas em px, as do grafo do Antigravity/VS Code: faixas de 11 px (a primeira começa em 11, então uma faixa ocupa 22), linha de
// commit de 22 px e linha de arquivo (a mesma do painel).
const LANE = 11;
const ROW = 22;
const FILE_ROW = 22;
// Altura da seção aberta. O gráfico e a lista de arquivos não descem do mínimo ao arrastar o divisor; setas andam de 16 em 16.
export const GRAPH_DEFAULT = 260;
export const GRAPH_MIN = 96;
export const FILES_MIN = 96;
const KEY_STEP = 16;
// Cores das faixas, as do grafo do Antigravity (scmGraph.historyItemRefColor e scmGraph.foreground1 a 5): a primeira faixa, a da
// branch atual, em azul; as outras em ciclo. São cores de dado, iguais nos temas claro e escuro.
const PALETTE = ['#59a4f9', '#ffb000', '#dc267f', '#994f00', '#40b0a6', '#b66dff'];
const colorOf = (n: number) => PALETTE[n % PALETTE.length];
// Cores das etiquetas: branch local em azul, remota em lilás, tag em laranja (historyItemRefColor, RemoteRefColor e BaseRefColor).
const REF_COLOR = { local: '#59a4f9', remote: '#b180d7', tag: '#ea5c00' } as const;
// Fundo da barra lateral: o contorno dos pontos usa esta cor para cortar a linha em volta deles, como no VS Code.
const BG = 'var(--ant-color-bg-layout)';
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

// Posição do centro de uma faixa, na coluna dela; a largura do desenho tem uma faixa a mais de folga.
const laneX = (lane: number) => (lane + 1) * LANE;
const widthOf = (row: GraphRow) => (row.width + 1) * LANE;
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
    <svg width={widthOf(row)} height={ROW} className="shrink-0" aria-hidden>
      {row.lines.map((line: GraphLine, i) => {
        const top = line.half === 'top';
        return (
          <path
            key={i} d={segment(laneX(line.from), top ? 0 : mid, laneX(line.to), top ? mid : ROW)}
            fill="none" strokeWidth={1} strokeLinecap="round" style={{ stroke: colorOf(line.color) }}
          />
        );
      })}
      {/* Como no VS Code: ponto de raio 5 com contorno de 2 px no fundo (o ponto fica com 8 px e corta a linha); o HEAD é um anel
          de raio 7 com o miolo no fundo. */}
      <circle cx={laneX(row.col)} cy={mid} r={head ? 7 : 5} strokeWidth={2} style={{ fill: colorOf(row.color), stroke: BG }} />
      {head && <circle cx={laneX(row.col)} cy={mid} r={2} strokeWidth={4} style={{ fill: BG, stroke: BG }} />}
    </svg>
  );
}

/** Faixas que seguem para baixo, ao lado de uma linha de arquivo de um commit aberto: retas verticais de borda a borda. */
function GapLanes({ row }: { row: GraphRow }) {
  return (
    <svg width={widthOf(row)} height={FILE_ROW} className="shrink-0" aria-hidden>
      {row.lines.filter((l) => l.half === 'bottom').map((l, i) => (
        <path key={i} d={`M${laneX(l.to)} 0V${FILE_ROW}`} fill="none" strokeWidth={1} style={{ stroke: colorOf(l.color) }} />
      ))}
    </svg>
  );
}

/**
 * Etiqueta de uma ref, como a do grafo do Antigravity: pílula de 18 px e raio de 10 px, na cor do tipo (branch azul, remota lilás,
 * tag laranja), com o texto escuro de 12 px. A branch atual tem o alvo e sempre mostra o nome; as outras, na linha estreita, viram
 * só o ícone (o nome fica no title e aparece quando a linha tem espaço).
 */
function RefBadge({ item }: { item: GitRef }) {
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

/** Nota no lugar dos arquivos de um commit aberto (lendo, vazio ou erro), com as faixas que seguem. */
function DetailNote({ row, text, danger = false }: { row: GraphRow; text: string; danger?: boolean }) {
  return (
    <ul>
      <li className="flex min-w-0">
        <GapLanes row={row} />
        <Text as="p" role="status" fontSize={12} type={danger ? 'danger' : 'secondary'} className="flex h-[22px] min-w-0 items-center pl-5">{text}</Text>
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

  // Só carrega com a tela visível e a seção aberta, e só quando a chave mudou desde a última carga.
  useEffect(() => {
    if (!active || !open || loaded.current === stamp) return;
    loaded.current = stamp;
    reloadList(null);
    // `reloadList` só lê refs e setters: não precisa estar nas dependências.
  }, [active, open, stamp]);

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
    <Flexbox horizontal align="center" gap={2} height={22} className="shrink-0 pr-2">
      <button
        type="button" onClick={onToggle} aria-expanded={open}
        className="flex h-full min-w-0 flex-1 items-center text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
      >
        <ChevronRight className={`mx-0.5 size-4 shrink-0 transition-transform${open ? ' rotate-90' : ''}`} aria-hidden />
        <span className="min-w-0 shrink truncate text-[12px] font-medium">Grafo</span>
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
                    {/* Escolher o mesmo commit de novo fecha. Trocar de commit descarta a lista anterior e pede a nova. */}
                    <button
                      type="button" onClick={() => (openHash === c.hash ? closeCommit() : openCommit(c.hash))} aria-expanded={isOpen}
                      title={`${c.short} — ${c.subject || '(sem mensagem)'}. ${c.author}, ${when(c.date, { dateStyle: 'short', timeStyle: 'short' })}.${names ? ` Etiquetas: ${names}.` : ''}`}
                      className={`@container flex h-[22px] w-full min-w-0 items-center gap-1 pr-3 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset${isOpen ? ' bg-muted' : ''}`}
                    >
                      <CommitLanes row={row} head={isHead} />
                      {/* O assunto tem prioridade: base zero (só cresce) e piso de 6rem. Etiquetas e autor encolhem antes dele. */}
                      <Text as="span" ellipsis fontSize={13} weight={isHead ? 600 : 400} className="min-w-[6rem] flex-1">{c.subject || '(sem mensagem)'}</Text>
                      {c.refs.map((r) => <RefBadge key={`${r.kind}:${r.name}`} item={r} />)}
                      <span className="hidden min-w-0 max-w-[30%] shrink truncate text-[0.9em] opacity-70 @[24rem]:inline">{c.author}</span>
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
          <Flexbox className="min-h-0 overflow-y-auto overflow-x-hidden" flex={1}>
            {list}
          </Flexbox>
        )}
      </section>
    </>
  );
}
