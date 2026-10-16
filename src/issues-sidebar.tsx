// Faixa lateral Issues: cabeçalho com atualizar, filtrar e abrir no GitHub, o repositório e a sincronia, o filtro (texto e labels), a captura
// rápida de ideias, a seção "Em andamento" (a issue com o orquestrador), a seção "Na fila" (a próxima no topo, as criações a caminho ou que
// falharam, e as abertas), a seção "Fechadas recentes" e o rodapé com a contagem e o "Ver todas". O estado vem do store único do App
// (`useIssues`). As linhas da fila e das fechadas formam uma lista só para o teclado (roving tabindex) e mostram a prévia da issue no hover.
import {
  useEffect, useLayoutEffect, useRef, useState, type FocusEvent, type KeyboardEvent, type MouseEvent, type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { Check, ChevronDown, ChevronRight, CircleDot, ExternalLink, ListFilter, MessageSquare, RefreshCw, RotateCcw, Search, Undo2 } from 'lucide-react';
import { ActionIcon, Button, Input, Text, TextArea, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from './backend';
import {
  filterLabels, passesFilter, previewBody, readSections, recentClosed, relativeTime, rovingStep, sortIssues, SECTIONS_DEFAULT,
  type IssueSections,
} from './issue-text';
import {
  FailedActions, FOCUS_RING, GH, GREEN_BUTTON, IssueIcon, LabelChips, LabelDot, LabelPill, LabelToggle, MENU_OPEN_SHOWS, NextMark, pendingKind,
  pendingLabels, StartButton, useNow, type StartIssue,
} from './issue-ui';
import { useIssues, type ActiveIssue, type NewIssue, type PendingIssue } from './issues-store';
import type { Issue, Label } from './types';

const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });
const issuesUrl = (owner: string, repo: string) => `https://github.com/${owner}/${repo}/issues`;
const iso = (ms: number) => new Date(ms).toISOString();
// O estado de recolher das seções, o mesmo em todos os projetos.
const SECTIONS_KEY = 'orquestrador.issues.sections';
const RECENT_CLOSED = 5;
const PREVIEW_DELAY = 500;
const PREVIEW_ID = 'issues-preview';
const NO_MATCH = 'Nenhuma issue corresponde ao filtro.';
// Chave de uma linha da lista, em `data-row`: `#N` numa issue, o `localId` numa criação.
const issueKey = (n: number) => `#${n}`;

function loadSections(): IssueSections {
  try {
    return readSections(localStorage.getItem(SECTIONS_KEY));
  } catch {
    return { ...SECTIONS_DEFAULT };
  }
}

export function IssuesSidebar({ start, onOpenPage, onOpenEditor, captureRequest, onCaptureDismiss }: {
  start: StartIssue;
  /** Abre ou ativa a aba central Issues. */
  onOpenPage: () => void;
  /** Abre a aba Nova issue com o que estava na captura. */
  onOpenEditor: (seed: NewIssue) => void;
  /** Sobe a cada Ctrl+Shift+I: a captura ganha o foco e abre. */
  captureRequest: number;
  /** Esc com a captura vazia depois do Ctrl+Shift+I: volta para onde estava. */
  onCaptureDismiss: () => void;
}) {
  const store = useIssues();
  const { status, refreshing } = store;
  const ready = status?.kind === 'ready' ? status : null;
  // O estado de recolhida fica aqui, não na seção: sobrevive à lista vazia e às atualizações. E ao recarregar, pelo localStorage.
  const [sections, setSections] = useState(loadSections);
  const toggle = (key: keyof IssueSections) => {
    const next = { ...sections, [key]: !sections[key] };
    setSections(next);
    try {
      localStorage.setItem(SECTIONS_KEY, JSON.stringify(next));
    } catch {
      // Sem localStorage: só não sobrevive a um recarregamento.
    }
  };
  // Filtro da fila e das fechadas: desligar limpa o texto e as labels.
  const [filterOn, setFilterOn] = useState(false);
  const [query, setQuery] = useState('');
  const [wanted, setWanted] = useState<string[]>([]);
  const filterInput = useRef<HTMLInputElement>(null);
  const toggleFilter = () => {
    if (filterOn) {
      setQuery('');
      setWanted([]);
    }
    setFilterOn(!filterOn);
  };
  useEffect(() => {
    if (filterOn) filterInput.current?.focus();
  }, [filterOn]);
  const now = useNow(1000);

  const active = store.issues.find((i) => i.number === store.active?.number && i.state === 'open') ?? null;
  // Na fila: as abertas menos a em andamento, com a próxima no topo (acima até das criações).
  const open = sortIssues(store.issues.filter((i) => i.state === 'open' && i !== active), 'newest');
  const pending = [...store.pending].sort((a, b) => b.createdAt - a.createdAt);
  const queued = pending.length + open.length;
  // O filtro vale para a fila e as fechadas; a em andamento e as criações ficam fora dele.
  const filtering = filterOn && (query.trim() !== '' || wanted.length > 0);
  const passes = (i: Issue) => passesFilter(i, query, wanted);
  const shownOpen = open.filter(passes);
  const next = shownOpen.find((i) => i.number === store.next) ?? null;
  const rest = shownOpen.filter((i) => i !== next);
  const closed = recentClosed(store.issues.filter(passes), RECENT_CLOSED);

  // Teclado: uma parada de Tab na lista (a última linha focada, ou a primeira), e ↑/↓/Home/End entre as linhas visíveis.
  const keys = [
    ...(sections.queue ? [...(next ? [issueKey(next.number)] : []), ...pending.map((p) => p.localId), ...rest.map((i) => issueKey(i.number))] : []),
    ...(sections.closed ? closed.map((i) => issueKey(i.number)) : []),
  ];
  const [current, setCurrent] = useState<string | null>(null);
  const tabKey = current !== null && keys.includes(current) ? current : keys[0] ?? null;
  const list = useRef<HTMLDivElement>(null);
  const root = useRef<HTMLDivElement>(null);
  // Linha a focar depois da renderização: a reaberta, que passa das fechadas para a fila (ou a primeira, com a fila recolhida).
  const refocus = useRef<string | null>(null);
  useEffect(() => {
    const key = refocus.current;
    if (key === null || !list.current) return;
    refocus.current = null;
    const rows = [...list.current.querySelectorAll<HTMLElement>('[data-row]')];
    (rows.find((r) => r.dataset.row === key) ?? rows[0])?.focus();
  });

  // Prévia: 500 ms parado sobre uma linha (ou com o foco nela). Some ao sair, ao rolar, ao clicar, com Enter e com Esc.
  const [preview, setPreview] = useState<{ key: string; top: number; left: number; container: Element } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const anchor = useRef<HTMLElement | null>(null);
  const hidePreview = () => {
    window.clearTimeout(timer.current);
    anchor.current = null;
    setPreview(null);
  };
  const schedulePreview = (row: HTMLElement) => {
    window.clearTimeout(timer.current);
    anchor.current = row;
    timer.current = window.setTimeout(() => {
      // Com um menu aberto (o do Iniciar, por exemplo), a prévia não aparece por cima dele.
      if (anchor.current !== row || !row.isConnected || !root.current || document.querySelector('[role="menu"]')) return;
      const side = root.current.getBoundingClientRect(), r = row.getBoundingClientRect();
      // Fora do `div.ant-app` as variáveis do tema não valem: a prévia vai para dentro dele.
      setPreview({ key: row.dataset.row!, top: r.top - 6, left: side.right + 8, container: root.current.closest('.ant-app') ?? document.body });
    }, PREVIEW_DELAY);
  };
  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => {
    if (!preview) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') hidePreview();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [preview]);
  // A linha saiu da tela (seção recolhida, filtro, issue que mudou de seção): a prévia some junto.
  useEffect(() => {
    if (preview && !anchor.current?.isConnected) hidePreview();
  });
  const previewIssue = preview ? store.issues.find((i) => issueKey(i.number) === preview.key) ?? null : null;
  const previewKey = previewIssue ? preview!.key : null;

  const rowOf = (target: EventTarget) => {
    const row = (target as HTMLElement).closest?.<HTMLElement>('[data-row]');
    return row && list.current?.contains(row) ? row : null;
  };
  const onListKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const row = rowOf(e.target);
    if (!row || e.target !== row || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (e.key === 'Enter') {
      // O mesmo que o Iniciar: cola, ou abre o menu com o orquestrador ocupado. A fechada e a criação não têm Iniciar.
      e.preventDefault();
      hidePreview();
      row.querySelector<HTMLElement>('[data-start]')?.click();
      return;
    }
    const rows = [...list.current!.querySelectorAll<HTMLElement>('[data-row]')];
    const to = rovingStep(rows.indexOf(row), rows.length, e.key);
    if (to === null) return;
    e.preventDefault();
    rows[to].focus();
  };
  const onListFocus = (e: FocusEvent<HTMLDivElement>) => {
    const row = rowOf(e.target);
    if (!row) return;
    setCurrent(row.dataset.row!);
    if (e.target === row && row.hasAttribute('data-preview')) schedulePreview(row);
    else hidePreview();
  };
  const onListBlur = (e: FocusEvent<HTMLDivElement>) => {
    if (e.target === anchor.current) hidePreview();
  };
  const onListOver = (e: MouseEvent<HTMLDivElement>) => {
    const row = rowOf(e.target);
    const target = row?.hasAttribute('data-preview') ? row : null;
    if (target === anchor.current) return;
    hidePreview();
    if (target) schedulePreview(target);
  };
  const reopen = (issue: Issue, row: HTMLElement) => {
    if (row.contains(document.activeElement)) refocus.current = issueKey(issue.number);
    hidePreview();
    store.setState(issue.number, true).then(() => toast.success({ title: `#${issue.number} reaberta no GitHub`, duration: 3000 }), report);
  };

  return (
    <Flexbox ref={root} height="100%" className="min-h-0">
      {/* Cabeçalho de 36 px no padrão do Git e da pesquisa. */}
      <Flexbox horizontal align="center" gap={2} paddingInline={12} height={36} flex="none">
        <Text as="h2" ellipsis weight={500} fontSize={12} type="secondary" className="min-w-0 flex-1">Issues</Text>
        <ActionIcon
          size="small" variant="borderless" icon={RefreshCw} spin={refreshing} className={FOCUS_RING}
          aria-label="Atualizar issues" title="Atualizar" onClick={() => void store.refresh()}
        />
        {ready && (
          <>
            <ActionIcon
              size="small" variant="borderless" icon={ListFilter} active={filterOn} aria-pressed={filterOn} aria-controls="issues-filter"
              className={FOCUS_RING} aria-label="Filtrar issues" title={filterOn ? 'Limpar e esconder o filtro' : 'Filtrar'} onClick={toggleFilter}
            />
            <ActionIcon
              size="small" variant="borderless" icon={ExternalLink} className={FOCUS_RING}
              aria-label="Abrir as issues no GitHub" title="Abrir no GitHub"
              onClick={() => void backend.githubOpen(issuesUrl(ready.owner, ready.repo)).catch(report)}
            />
          </>
        )}
      </Flexbox>

      {!status && (
        <Text as="p" role="status" fontSize={12} type="secondary" style={{ paddingInline: 12, paddingBlock: 4 }}>Conectando ao GitHub…</Text>
      )}

      {status?.kind === 'unavailable' && (
        <Flexbox gap={12} paddingInline={12} paddingBlock={4}>
          <Text as="p" role="status" fontSize={12} type="secondary">{status.message}</Text>
          <Button block loading={refreshing} disabled={refreshing} onClick={() => void store.refresh()}>Tentar de novo</Button>
        </Flexbox>
      )}

      {ready && (
        <>
          <Flexbox horizontal align="center" gap={6} paddingInline={12} className="-mt-1.5 pb-2" flex="none">
            <Text as="span" code fontSize={11} type="secondary" ellipsis title={`${ready.owner}/${ready.repo}`} className="min-w-0">
              {`${ready.owner}/${ready.repo}`}
            </Text>
            <SyncState syncedAt={store.syncedAt} error={store.error} now={now} />
          </Flexbox>

          {filterOn && (
            <Flexbox id="issues-filter" gap={6} paddingInline={8} className="pb-2" flex="none">
              <Input
                ref={filterInput} size="small" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Filtrar"
                aria-label="Filtrar a fila e as fechadas por título ou #número" autoComplete="off" className="h-[26px]! text-[12px]!"
                prefix={<Search aria-hidden className="size-3.5 text-[var(--ant-color-text-tertiary)]" />}
              />
              <FilterChips issues={store.issues} labels={store.labels} selected={wanted} onChange={setWanted} />
            </Flexbox>
          )}

          <Capture onOpenEditor={onOpenEditor} focusRequest={captureRequest} onDismiss={onCaptureDismiss} />

          <div
            ref={list} className="min-h-0 flex-1 overflow-y-auto pb-1.5"
            onKeyDown={onListKey} onFocus={onListFocus} onBlur={onListBlur} onMouseOver={onListOver} onMouseLeave={hidePreview}
            onPointerDown={hidePreview} onScroll={hidePreview}
          >
            <ActiveSection
              open={sections.active} onToggle={() => toggle('active')} issue={active} since={store.active} now={now} busy={start?.busy ?? false}
            />
            <QueueSection
              open={sections.queue} onToggle={() => toggle('queue')} pending={pending} next={next} issues={rest} now={now} start={start}
              filtering={filtering} tabKey={tabKey} previewKey={previewKey}
            />
            <ClosedSection
              open={sections.closed} onToggle={() => toggle('closed')} issues={closed} filtering={filtering} tabKey={tabKey}
              previewKey={previewKey} onReopen={reopen}
            />
          </div>

          <Flexbox horizontal align="center" justify="space-between" gap={8} paddingInline={12} height={28} flex="none" className="border-t">
            <Text as="span" fontSize={11} type="secondary" noWrap>{`${queued} na fila${active ? ' · 1 em andamento' : ''}`}</Text>
            <Button type="link" size="small" className="h-auto! p-0! text-[11px]!" onClick={onOpenPage}>Ver todas</Button>
          </Flexbox>

          {preview && previewIssue && createPortal(
            <PreviewCard issue={previewIssue} top={preview.top} left={preview.left} now={now} />, preview.container,
          )}
        </>
      )}
    </Flexbox>
  );
}

/** Chips do filtro: as labels do repositório em uso nas abertas; com mais de uma marcada, a issue precisa ter todas. */
function FilterChips({ issues, labels, selected, onChange }: {
  issues: readonly Issue[];
  labels: readonly Label[];
  selected: string[];
  onChange: (next: string[]) => void;
}) {
  const shown = filterLabels(issues, labels, selected);
  if (!shown.length) return null;
  const toggle = (name: string) => onChange(selected.includes(name) ? selected.filter((n) => n !== name) : [...selected, name]);
  return (
    <div role="group" aria-label="Filtrar por label" className="flex flex-wrap items-center gap-1">
      {shown.map((label) => (
        <LabelToggle key={label.name} label={label} on={selected.includes(label.name)} onToggle={() => toggle(label.name)} />
      ))}
    </div>
  );
}

/**
 * Cartão flutuante da prévia, à direita da faixa e por cima da área central: título, labels, "#N · autor abriu há X", comentários e a
 * descrição em texto simples, cortada em 8 linhas. Não recebe foco nem o ponteiro (`pointer-events-none`); a linha aponta para ele com
 * `aria-describedby`.
 */
function PreviewCard({ issue, top, left, now }: { issue: Issue; top: number; left: number; now: number }) {
  const card = useRef<HTMLDivElement>(null);
  // Alinhado com a linha e inteiro na janela: a altura só se sabe depois de renderizar.
  useLayoutEffect(() => {
    const el = card.current;
    if (el) el.style.top = `${Math.max(8, Math.min(top, window.innerHeight - el.offsetHeight - 8))}px`;
  });
  const body = previewBody(issue.body, 8);
  const closed = issue.state === 'closed';
  const comments = issue.comments === 1 ? '1 comentário' : `${issue.comments} comentários`;
  return (
    <div
      ref={card} id={PREVIEW_ID} role="tooltip"
      className="pointer-events-none fixed z-50 w-80 rounded-md border bg-[var(--ant-color-bg-elevated)] p-3 shadow-[var(--ant-box-shadow-secondary)]"
      style={{ top, left }}
    >
      <Flexbox horizontal gap={8} align="flex-start">
        <span className="mt-px"><IssueIcon kind={closed ? 'closed' : 'open'} /></span>
        <Text as="div" fontSize={13} weight={600} lineHeight="18px" className="min-w-0 break-words">{issue.title}</Text>
      </Flexbox>
      <div className="ml-6">
        {issue.labels.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1">{issue.labels.map((l) => <LabelPill key={l.name} label={l} />)}</div>
        )}
        <Text as="div" type="secondary" fontSize={12} className="mt-1.5">
          <span className="font-mono">{`#${issue.number}`}</span>
          {` · ${issue.author} abriu ${relativeTime(issue.createdAt, now)}`}
          {closed && ` · fechada ${relativeTime(issue.closedAt ?? issue.updatedAt, now)}`}
          {` · ${comments}`}
        </Text>
        <div className="mt-2 border-t pt-2">
          {body
            ? <Text as="div" fontSize={12} lineHeight={1.5} className="line-clamp-8 break-words whitespace-pre-line">{body}</Text>
            : <Text as="div" fontSize={12} type="secondary">Sem descrição.</Text>}
        </div>
      </div>
    </div>
  );
}

/** Ponto verde e "sincronizado há N s" (a cada segundo; depois de um minuto, o tempo relativo), ou ponto âmbar com a falha no `title`. */
function SyncState({ syncedAt, error, now }: { syncedAt: number | null; error: string | null; now: number }) {
  const seconds = syncedAt === null ? 0 : Math.max(0, Math.floor((now - syncedAt) / 1000));
  const text = error ? 'falha ao atualizar'
    : syncedAt === null ? 'sincronizando…'
      : seconds < 60 ? `sincronizado há ${seconds} s` : `sincronizado ${relativeTime(iso(syncedAt), now)}`;
  return (
    <span className="ml-auto flex shrink-0 items-center gap-[5px] text-[11px] text-muted-foreground tabular-nums" title={error ?? undefined} role="status">
      <span
        aria-hidden className="size-1.5 rounded-full"
        style={{ background: error ? 'var(--ant-color-warning)' : 'var(--ant-color-success)' }}
      />
      {text}
    </span>
  );
}

/**
 * Captura rápida: o título sempre visível; com foco ou com algo escrito, o cartão expande com a descrição, as labels e o rodapé.
 * Enter no título cria; Shift+Enter vai para a descrição; na descrição, Enter quebra a linha e Ctrl+Enter cria; Esc limpa e recolhe.
 * Aberta pelo Ctrl+Shift+I (`focusRequest`), o Esc com o cartão vazio chama `onDismiss` (o App volta para a tela de antes); criar não sai
 * daqui, para anotar várias em seguida. Vale até o foco sair do cartão.
 */
function Capture({ onOpenEditor, focusRequest, onDismiss }: { onOpenEditor: (seed: NewIssue) => void; focusRequest: number; onDismiss: () => void }) {
  const store = useIssues();
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [labels, setLabels] = useState<string[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [focused, setFocused] = useState(false);
  const [invalid, setInvalid] = useState(false);
  const card = useRef<HTMLDivElement>(null);
  const titleRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const open = expanded || title !== '' || body !== '' || labels.length > 0;
  const viaShortcut = useRef(false);
  useEffect(() => {
    if (!focusRequest) return;
    viaShortcut.current = true;
    titleRef.current?.focus();
    setExpanded(true);
  }, [focusRequest]);

  const clear = () => {
    setTitle('');
    setBody('');
    setLabels([]);
    setExpanded(false);
  };
  // Título vazio não cria: o cartão sacode (só sem `prefers-reduced-motion`) e a borda fica vermelha por um instante.
  const refuse = () => {
    setInvalid(true);
    window.setTimeout(() => setInvalid(false), 600);
    titleRef.current?.focus();
    if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      card.current?.animate(
        [{ transform: 'none' }, { transform: 'translateX(-4px)' }, { transform: 'translateX(4px)' }, { transform: 'translateX(-3px)' }, { transform: 'translateX(2px)' }, { transform: 'none' }],
        { duration: 320, easing: 'ease-out' },
      );
    }
  };
  const create = () => {
    if (!title.trim()) return refuse();
    store.create({ title: title.trim(), body: body.trim(), labels });
    // O foco vem antes de limpar: focar o título expande o cartão, e a limpeza o recolhe de novo.
    titleRef.current?.focus();
    clear();
  };
  const toEditor = () => {
    onOpenEditor({ title, body, labels });
    clear();
  };
  const cancel = () => {
    if (viaShortcut.current && title === '' && body === '' && labels.length === 0) {
      viaShortcut.current = false;
      clear();
      onDismiss();
      return;
    }
    titleRef.current?.focus();
    clear();
  };

  const onTitleKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === 'Enter' && e.shiftKey) {
      e.preventDefault();
      setExpanded(true);
      requestAnimationFrame(() => bodyRef.current?.focus());
    } else if (e.key === 'Enter') {
      e.preventDefault();
      create();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    }
  };
  const onBodyKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      create();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    }
  };
  // Recolhe só quando o foco sai do cartão (a lista de labels abre fora dele, num portal, e não conta como saída).
  const onFocusIn = () => {
    setFocused(true);
    setExpanded(true);
  };
  const onFocusOut = (e: FocusEvent<HTMLDivElement>) => {
    const next = e.relatedTarget as Element | null;
    if (next && (e.currentTarget.contains(next) || next.closest('[role="menu"]'))) return;
    viaShortcut.current = false;
    setFocused(false);
    setExpanded(false);
  };

  const border = invalid ? 'var(--ant-color-error)' : focused ? 'var(--ant-color-primary)' : 'var(--ant-color-border)';
  return (
    <div
      ref={card} onFocus={onFocusIn} onBlur={onFocusOut}
      className="mx-2 mb-2 flex-none rounded-md border bg-card transition-colors"
      style={{ borderColor: border }}
    >
      <Flexbox horizontal align="center" gap={8} paddingInline={8} height={32}>
        <CircleDot aria-hidden className="size-4 shrink-0 text-[var(--ant-color-text-quaternary)]" />
        <Input
          ref={titleRef} variant="borderless" size="small" value={title} onChange={(e) => setTitle(e.target.value)} onKeyDown={onTitleKey}
          placeholder="Anotar uma ideia…" aria-label="Título da nova issue" autoComplete="off"
          className="min-w-0 flex-1 px-0!"
        />
      </Flexbox>
      {open && (
        <Flexbox gap={6} paddingInline={8} className="pb-2">
          <TextArea
            ref={bodyRef} autoSize={{ minRows: 3, maxRows: 8 }} value={body} onChange={(e) => setBody(e.target.value)} onKeyDown={onBodyKey}
            placeholder="Descrição (opcional)" aria-label="Descrição da nova issue" className="text-[12.5px]!"
          />
          <LabelChips issues={store.issues} labels={store.labels} selected={labels} onChange={setLabels} />
          <Flexbox horizontal align="center" gap={8} className="mt-0.5">
            <Flexbox flex={1} className="min-w-0">
              <Text as="span" fontSize={10.5} type="secondary" lineHeight={1.35}>Enter cria · Esc cancela</Text>
              <Button type="link" size="small" className="h-auto! self-start! p-0! text-[11px]!" onClick={toEditor}>Abrir no editor completo</Button>
            </Flexbox>
            <Button size="small" className={GREEN_BUTTON} onClick={create}>Criar</Button>
          </Flexbox>
        </Flexbox>
      )}
    </div>
  );
}

/** Cabeçalho recolhível de uma seção da faixa, no padrão das seções do Git: chevron, título e a contagem à direita. */
function SectionHeader({ title, count, open, onToggle }: { title: string; count: number; open: boolean; onToggle: () => void }) {
  return (
    <Flexbox horizontal align="center" height={22} className="pr-2">
      <button
        type="button" onClick={onToggle} aria-expanded={open}
        className="flex h-full min-w-0 flex-1 items-center text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
      >
        {open ? <ChevronDown className="mx-0.5 size-4 shrink-0" aria-hidden /> : <ChevronRight className="mx-0.5 size-4 shrink-0" aria-hidden />}
        <span className="min-w-0 shrink truncate text-[12px] font-medium">{title}</span>
        <span className="ml-auto shrink-0 rounded-full bg-[var(--ant-color-fill-secondary)] px-[5px] text-[11px] leading-[18px] tabular-nums">{count}</span>
      </button>
    </Flexbox>
  );
}

/**
 * Seção "Em andamento": a issue com o orquestrador num cartão com o filete âmbar, o título em até duas linhas, há quanto tempo está lá,
 * o estado do orquestrador (trabalhando, com três pontos pulsando, ou livre), e Concluir (fecha no GitHub) e Voltar à fila.
 */
function ActiveSection({ open, onToggle, issue, since, now, busy }: {
  open: boolean;
  onToggle: () => void;
  issue: Issue | null;
  since: ActiveIssue | null;
  now: number;
  busy: boolean;
}) {
  const store = useIssues();
  const finish = (n: number) => store.finish(n).then(() => toast.success({ title: `#${n} fechada no GitHub`, duration: 3000 }), report);
  return (
    <Flexbox as="section" gap={0}>
      <SectionHeader title="Em andamento" count={issue ? 1 : 0} open={open} onToggle={onToggle} />
      {open && (issue && since ? (
        <div className="mx-2 mt-0.5 mb-1.5 rounded-md border border-l-2 bg-card px-2.5 py-2" style={{ borderLeftColor: GH.amber }}>
          <Text as="div" fontSize={13} weight={500} lineHeight="18px" ellipsis={{ rows: 2 }} title={issue.title}>{issue.title}</Text>
          <Text as="div" fontSize={11} type="secondary" className="mt-1 tabular-nums">
            <span className="font-mono">{`#${issue.number}`}</span>
            {` · no orquestrador ${relativeTime(iso(since.since), now)}`}
          </Text>
          <Flexbox horizontal align="center" gap={6} role="status" className="mt-1 text-[11px] text-muted-foreground">
            {busy && (
              <span aria-hidden className="inline-flex gap-[3px]">
                {[0, 200, 400].map((delay) => (
                  <span
                    key={delay} className="size-1 rounded-full motion-safe:animate-pulse"
                    style={{ background: GH.amber, animationDelay: `${delay}ms` }}
                  />
                ))}
              </span>
            )}
            {busy ? 'orquestrador trabalhando' : 'orquestrador livre'}
          </Flexbox>
          <Flexbox horizontal gap={6} className="mt-2">
            <Button
              size="small" icon={<Check className="size-3.5" />} title="Fecha a issue no GitHub"
              aria-label={`Concluir a issue #${issue.number} (fecha no GitHub)`} onClick={() => void finish(issue.number)}
            >
              Concluir
            </Button>
            <Button
              size="small" icon={<Undo2 className="size-3.5" />} aria-label={`Voltar a issue #${issue.number} à fila`} onClick={() => store.setActive(null)}
            >
              Voltar à fila
            </Button>
          </Flexbox>
        </div>
      ) : (
        <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 24, paddingBlock: 4 }}>Nada em andamento. Inicie uma issue da fila.</Text>
      ))}
    </Flexbox>
  );
}

/**
 * Seção "Na fila": a próxima no topo (com o relógio), as criações (mais novas em cima) e depois as abertas, da mais nova para a mais velha.
 * A em andamento não entra. `issues` e `next` já vêm filtradas; as criações não passam pelo filtro.
 */
function QueueSection({ open, onToggle, pending, next, issues, now, start, filtering, tabKey, previewKey }: {
  open: boolean;
  onToggle: () => void;
  pending: PendingIssue[];
  next: Issue | null;
  issues: Issue[];
  now: number;
  start: StartIssue;
  filtering: boolean;
  tabKey: string | null;
  previewKey: string | null;
}) {
  const store = useIssues();
  const found = issues.length + (next ? 1 : 0);
  const count = pending.length + found;
  const issueRow = (issue: Issue) => (
    <QueueRow
      key={issue.number} rowKey={issueKey(issue.number)} tabKey={tabKey} previewKey={previewKey} preview kind="open" title={issue.title}
      label={`#${issue.number} ${issue.title}${issue === next ? ' (próxima)' : ''}`}
      meta={(
        <>
          {issue === next && <NextMark />}
          <span className="shrink-0 font-mono text-[11px]">{`#${issue.number}`}</span>
          {issue.labels.map((l) => <LabelDot key={l.name} label={l} />)}
          <span className="shrink-0">{relativeTime(issue.createdAt, now)}</span>
          {issue.comments > 0 && (
            <span className="inline-flex shrink-0 items-center gap-1" aria-label={`${issue.comments} ${issue.comments === 1 ? 'comentário' : 'comentários'}`}>
              <MessageSquare aria-hidden className="size-3" />
              <span className="font-mono">{issue.comments}</span>
            </span>
          )}
        </>
      )}
      actions={<StartButton issue={issue} start={start} />}
    />
  );
  return (
    <Flexbox as="section" gap={0}>
      <SectionHeader title="Na fila" count={count} open={open} onToggle={onToggle} />
      {open && count > 0 && (
        <ul className="grid grid-cols-[minmax(0,1fr)]">
          {next && issueRow(next)}
          {pending.map((p) => (
            <QueueRow
              key={p.localId} rowKey={p.localId} tabKey={tabKey} previewKey={previewKey} preview={false} kind={pendingKind(p)} title={p.title}
              label={`${p.status === 'sending' ? 'Enviando' : 'Falhou'}: ${p.title}`}
              meta={(
                <>
                  {p.status === 'sending'
                    ? <span>enviando…</span>
                    : <span className="min-w-0 truncate" title={p.error}>{`falhou: ${p.error ?? 'erro desconhecido'}`}</span>}
                  {pendingLabels(p.labels, store.labels).map((l) => <LabelDot key={l.name} label={l} />)}
                  <span className="shrink-0">{relativeTime(iso(p.createdAt), now)}</span>
                </>
              )}
              actions={p.status === 'failed' ? <FailedActions pending={p} retry={store.retry} discard={store.discard} compact /> : null}
            />
          ))}
          {issues.map(issueRow)}
        </ul>
      )}
      {/* Com filtro e nenhuma issue passando, o aviso fica abaixo das criações (que o filtro não esconde). */}
      {open && !found && (filtering || !count) && (
        <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 24, paddingBlock: 4 }}>
          {filtering ? NO_MATCH : 'Fila vazia. Anote uma ideia acima.'}
        </Text>
      )}
    </Flexbox>
  );
}

// Linha da faixa: hover em `bg-muted` e o anel de foco por dentro, só no foco por teclado.
const ROW_FOCUS = 'outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset';

/**
 * Linha de 44 px: ícone de estado, título cortado e, embaixo, os detalhes em texto pequeno. A linha é focável (uma parada de Tab na
 * lista inteira, a de `tabKey`); as ações aparecem à direita no hover e com o foco dentro dela (`group-focus-within`), como as da linha
 * do Git. Com `preview`, a linha mostra a prévia da issue (e fica sem o `title` nativo, que cobriria a prévia).
 */
function QueueRow({ rowKey, tabKey, previewKey, preview, kind, title, label, meta, actions }: {
  rowKey: string;
  tabKey: string | null;
  previewKey: string | null;
  preview: boolean;
  kind: Parameters<typeof IssueIcon>[0]['kind'];
  title: string;
  label: string;
  meta: ReactNode;
  actions: ReactNode;
}) {
  return (
    <li
      tabIndex={rowKey === tabKey ? 0 : -1} aria-label={label} data-row={rowKey} data-preview={preview ? '' : undefined}
      aria-describedby={previewKey === rowKey ? PREVIEW_ID : undefined}
      className={`group flex h-11 items-center gap-2 pr-2 pl-3.5 ${ROW_FOCUS}`}
    >
      <span className="mt-1.5 self-start"><IssueIcon kind={kind} /></span>
      <div className="min-w-0 flex-1">
        <Text as="div" fontSize={13} ellipsis title={preview ? undefined : title} lineHeight="18px">{title}</Text>
        <div className="flex min-w-0 items-center gap-2 overflow-hidden text-[11px] leading-4 whitespace-nowrap text-muted-foreground">{meta}</div>
      </div>
      {actions && <div className={`hidden shrink-0 items-center gap-0.5 group-focus-within:flex group-hover:flex ${MENU_OPEN_SHOWS}`}>{actions}</div>}
    </li>
  );
}

/**
 * Seção "Fechadas recentes" (começa recolhida): as 5 fechadas mais recentes que passam pelo filtro, em linhas de 28 px com o ícone, `#N` e o
 * título em secundário (o clique abre no GitHub). O Reabrir aparece no hover e com o foco na linha.
 */
function ClosedSection({ open, onToggle, issues, filtering, tabKey, previewKey, onReopen }: {
  open: boolean;
  onToggle: () => void;
  issues: Issue[];
  filtering: boolean;
  tabKey: string | null;
  previewKey: string | null;
  onReopen: (issue: Issue, row: HTMLElement) => void;
}) {
  return (
    <Flexbox as="section" gap={0}>
      <SectionHeader title="Fechadas recentes" count={issues.length} open={open} onToggle={onToggle} />
      {open && (issues.length ? (
        <ul className="grid grid-cols-[minmax(0,1fr)]">
          {issues.map((issue) => {
            const key = issueKey(issue.number), tabbable = key === tabKey;
            return (
              <li
                key={issue.number} tabIndex={tabbable ? 0 : -1} aria-label={`#${issue.number} ${issue.title} (fechada)`} data-row={key} data-preview=""
                aria-describedby={previewKey === key ? PREVIEW_ID : undefined}
                className={`group flex h-7 items-center gap-2 pr-2 pl-3.5 ${ROW_FOCUS}`}
              >
                <IssueIcon kind="closed" size={14} />
                <span className="shrink-0 font-mono text-[11px] text-muted-foreground">{`#${issue.number}`}</span>
                {/* Só a linha atual deixa o título no Tab: a lista é uma parada só. */}
                <button
                  type="button" tabIndex={tabbable ? 0 : -1} onClick={() => void backend.githubOpen(issue.url).catch(report)}
                  aria-label={`Abrir a issue #${issue.number} no GitHub`}
                  className="flex min-w-0 flex-1 rounded-sm text-left outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
                >
                  <Text as="span" type="secondary" fontSize={12} ellipsis className="min-w-0">{issue.title}</Text>
                </button>
                <div className="hidden shrink-0 group-focus-within:flex group-hover:flex">
                  <Button
                    size="small" icon={<RotateCcw className="size-3.5" />} aria-label={`Reabrir a issue #${issue.number}`} title="Reabre no GitHub"
                    className="h-[22px]! px-1.5! text-[11px]!"
                    onClick={(e) => onReopen(issue, e.currentTarget.closest('li')!)}
                  >
                    Reabrir
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      ) : (
        <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 24, paddingBlock: 4 }}>
          {filtering ? NO_MATCH : 'Nenhuma issue fechada recentemente.'}
        </Text>
      ))}
    </Flexbox>
  );
}
