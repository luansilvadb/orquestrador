// Faixa lateral Issues: cabeçalho com Nova issue e filtrar, o erro do arquivo das issues (se houver), o filtro (texto e labels), a seção
// "Em andamento" (um cartão com a issue que está com o orquestrador), a seção "Abertas" (a próxima no topo e as abertas), a seção "Fechadas
// recentes" e o rodapé com a contagem.
// O estado vem do store único do App (`useIssues`). Os cabeçalhos das seções, o cartão, as linhas da fila e as fechadas formam uma lista só
// para o teclado (roving tabindex, como a árvore do VS Code): ↑/↓ andam por tudo, Enter ou Espaço no cabeçalho recolhe e expande (← recolhe,
// → expande), e clicar ou Enter numa linha escolhe a issue (o detalhe da área central a mostra; o foco fica aqui). O duplo clique ou Enter na
// já escolhida leva o foco ao detalhe, e Ctrl+Enter faz o que o Colar faria. O Colar (e o Reabrir das fechadas) só aparece na linha escolhida
// e na próxima. Concluir e Voltar às abertas ficam só no detalhe; sem conseguir abrir o arquivo das issues, a faixa diz só isso e o detalhe
// tem o motivo.
import { useEffect, useId, useRef, useState, type CSSProperties, type FocusEvent, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react';
import { ChevronDown, ChevronRight, Clock, ListFilter, Plus, RotateCcw, Search } from 'lucide-react';
import { ActionIcon, Button, Input, Text, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { message } from './backend';
import { filterLabels, passesFilter, readSections, recentClosed, relativeTime, rovingStep, sortIssues, SECTIONS_DEFAULT, type IssueSections } from './issue-text';
import {
  ActiveMark, FIELD_FOCUS, FOCUS_RING, FOCUS_RING_INSET, GH, IssueIcon, LabelToggle, SECONDARY, StartButton, useNow,
  type StartIssue,
} from './issue-ui';
import { useDetailIssue, useIssues } from './issues-store';
import type { ActiveIssue, Issue } from './types';

const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });
const iso = (ms: number) => new Date(ms).toISOString();
// O estado de recolher das seções, o mesmo em todos os projetos.
const SECTIONS_KEY = 'orquestrador.issues.sections';
const RECENT_CLOSED = 5;
// O que o filtro ativo não achou: diz o que está ativo (busca, labels ou os dois).
const noMatchText = (query: string, wanted: string[]) =>
  `Nenhuma issue corresponde ${query.trim() !== '' && wanted.length > 0 ? 'à busca e às labels' : wanted.length > 0 ? 'às labels marcadas' : 'à busca'}.`;
// Chave de uma linha da lista, em `data-row`: `#N`. As linhas de issue também levam o número em `data-issue`.
const issueKey = (n: number) => `#${n}`;
// Chave do cabeçalho de uma seção na mesma lista (`data-row` e `data-section`).
const sectionKey = (s: keyof IssueSections) => `§${s}`;
// O que o texto de uma linha da fila diz fora da vista: tempo e o nome das labels (no `title` e na descrição acessível). O número fica fora:
// ele já está no nome acessível da linha.
const issueDetails = (i: Issue, now: number) => [
  relativeTime(i.createdAt, now),
  i.labels.length > 0 && `${i.labels.length === 1 ? 'label' : 'labels'}: ${i.labels.join(', ')}`,
].filter(Boolean).join(' · ');
// Largura da coluna do `#N` (mono de 12 px, alinhado à direita) que cabe o maior número das linhas de Abertas e de Fechadas recentes, e o
// relógio com o número da próxima: os títulos começam na mesma coluna em todas elas. Vai em `--issue-num` na lista; o `ch` é o do número.
const numColumn = (numbers: number[], next: number | null) => {
  const digits = Math.max(1, ...numbers.map((n) => String(n).length));
  return next === null ? `${digits + 1}ch` : `max(${digits + 1}ch, calc(${String(next).length + 1}ch + 18px))`;
};

function loadSections(): IssueSections {
  try {
    return readSections(localStorage.getItem(SECTIONS_KEY));
  } catch {
    return { ...SECTIONS_DEFAULT };
  }
}

export function IssuesSidebar({ start, onShowDetail, onNewIssue }: {
  start: StartIssue;
  /** Ativa a aba central Issues (o detalhe da issue escolhida); com `focus`, o foco vai para o detalhe. */
  onShowDetail: (focus: boolean) => void;
  /** Abre (ou ativa) a aba Nova issue da área central. */
  onNewIssue: () => void;
}) {
  const store = useIssues();
  const { loaded } = store;
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
  // "Limpar filtro" do aviso de nenhum resultado: limpa busca e labels e devolve o foco ao campo (antes de o botão sumir com o aviso).
  const clearFilter = () => {
    filterInput.current?.focus();
    setQuery('');
    setWanted([]);
  };
  const noMatch = noMatchText(query, wanted);
  const now = useNow();

  const active = store.issues.find((i) => i.number === store.active?.number && i.state === 'open') ?? null;
  // Abertas: as abertas menos a em andamento, com a próxima no topo.
  const open = sortIssues(store.issues.filter((i) => i.state === 'open' && i !== active), 'newest');
  // O filtro vale para a fila e as fechadas; a em andamento fica fora dele.
  const filtering = filterOn && (query.trim() !== '' || wanted.length > 0);
  const passes = (i: Issue) => passesFilter(i, query, wanted);
  const shownOpen = open.filter(passes);
  const next = shownOpen.find((i) => i.number === store.next) ?? null;
  const rest = shownOpen.filter((i) => i !== next);
  const closed = recentClosed(store.issues.filter(passes), RECENT_CLOSED);
  const numWidth = numColumn([...shownOpen, ...closed].map((i) => i.number), next?.number ?? null);
  const footer = [
    `${filtering ? `${shownOpen.length} de ${open.length}` : open.length} ${open.length === 1 ? 'aberta' : 'abertas'}`,
    active && '1 em andamento',
  ].filter(Boolean).join(' · ');

  // Teclado: uma parada de Tab na lista (a última linha focada, ou a primeira), e ↑/↓/Home/End entre os cabeçalhos das seções e as linhas
  // visíveis. O cartão Em andamento é a primeira linha da lista quando a seção está aberta.
  const cardOpen = sections.active && active !== null && store.active !== null;
  const keys = loaded ? [
    sectionKey('active'), ...(cardOpen ? [issueKey(active.number)] : []),
    sectionKey('queue'), ...(sections.queue ? [...(next ? [issueKey(next.number)] : []), ...rest.map((i) => issueKey(i.number))] : []),
    sectionKey('closed'), ...(sections.closed ? closed.map((i) => issueKey(i.number)) : []),
  ] : [];
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

  const rowOf = (target: EventTarget) => {
    const row = (target as HTMLElement).closest?.<HTMLElement>('[data-row]');
    return row && list.current?.contains(row) ? row : null;
  };
  // A issue escolhida (a que o detalhe mostra, mesmo sem escolha explícita) leva o `aria-current` e o Colar (ou o Reabrir) na linha.
  const shown = useDetailIssue()?.number ?? null;
  // Linha a colar depois da renderização: o Ctrl+Enter numa linha que ainda não era a escolhida (só a escolhida tem o botão).
  const startAfter = useRef<string | null>(null);
  useEffect(() => {
    const key = startAfter.current;
    if (key === null || !list.current) return;
    startAfter.current = null;
    [...list.current.querySelectorAll<HTMLElement>('[data-row]')].find((r) => r.dataset.row === key)?.querySelector<HTMLElement>('[data-start]')?.click();
  });
  // Escolhe a issue da linha e mostra o detalhe. Os cabeçalhos de seção (sem `data-issue`) não escolhem nada.
  const choose = (row: HTMLElement, focusDetail: boolean) => {
    if (!row.dataset.issue) return false;
    store.select(Number(row.dataset.issue));
    onShowDetail(focusDetail);
    return true;
  };
  const onListKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const row = rowOf(e.target);
    if (!row || e.target !== row || e.altKey || e.metaKey || e.shiftKey) return;
    // Cabeçalho de seção: Enter e Espaço ficam com o clique do próprio botão; ← recolhe e → expande.
    const section = row.dataset.section as keyof IssueSections | undefined;
    if (section && !e.ctrlKey) {
      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        e.preventDefault();
        if (sections[section] === (e.key === 'ArrowLeft')) toggle(section);
        return;
      }
      if (e.key === 'Enter' || e.key === ' ') return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      // Enter escolhe a linha; com ela já escolhida, leva o foco ao detalhe. Ctrl+Enter faz o que o Colar faria: cola, ou abre o menu com
      // o orquestrador ocupado. A fechada e a em andamento não têm Colar, só escolhem.
      if (e.ctrlKey) {
        if (!choose(row, false)) return;
        const button = row.querySelector<HTMLElement>('[data-start]');
        if (button) button.click();
        else startAfter.current = row.dataset.row!;
      } else {
        choose(row, row.getAttribute('aria-current') === 'true');
      }
      return;
    }
    if (e.ctrlKey) return;
    const rows = [...list.current!.querySelectorAll<HTMLElement>('[data-row]')];
    const to = rovingStep(rows.indexOf(row), rows.length, e.key);
    if (to === null) return;
    e.preventDefault();
    rows[to].focus();
  };
  const onListFocus = (e: FocusEvent<HTMLDivElement>) => {
    const row = rowOf(e.target);
    if (row) setCurrent(row.dataset.row!);
  };
  // Clique escolhe a linha (e mostra o detalhe, sem tirar o foco da faixa); duplo clique leva o foco ao detalhe. Os botões da linha têm as
  // ações deles.
  const onListClick = (e: MouseEvent<HTMLDivElement>, focusDetail: boolean) => {
    const row = rowOf(e.target);
    if (row && !(e.target as HTMLElement).closest('button, a, input')) choose(row, focusDetail);
  };
  const reopen = (issue: Issue, row: HTMLElement) => {
    if (row.contains(document.activeElement)) refocus.current = issueKey(issue.number);
    store.setState(issue.number, true).then(() => toast.success({ title: `#${issue.number} reaberta`, duration: 3000 }), report);
  };

  return (
    <Flexbox ref={root} height="100%" className="min-h-0">
      {/* Cabeçalho de 36 px no padrão do Git e da pesquisa. */}
      <Flexbox horizontal align="center" gap={2} paddingInline={12} height={36} flex="none">
        <Text style={SECONDARY} as="h2" ellipsis weight={500} fontSize={12} className="min-w-0 flex-1">Issues</Text>
        {loaded && (
          <>
            <ActionIcon
              size="small" variant="borderless" icon={Plus} className={FOCUS_RING} aria-label="Nova issue" title="Nova issue (Ctrl+Shift+I)"
              aria-keyshortcuts="Control+Shift+I" onClick={onNewIssue}
            />
            <ActionIcon
              size="small" variant="borderless" icon={ListFilter} active={filterOn} aria-pressed={filterOn} aria-controls="issues-filter"
              className={FOCUS_RING} aria-label="Filtrar issues" title={filterOn ? 'Limpar e esconder o filtro' : 'Filtrar'} onClick={toggleFilter}
            />
          </>
        )}
      </Flexbox>

      {/* O arquivo das issues não abriu (o motivo inteiro fica no detalhe) ou uma gravação falhou (o motivo vai aqui mesmo). */}
      {store.error && (
        <Flexbox gap={2} align="flex-start" paddingInline={12} className="pb-2" flex="none">
          <Text as="span" role="alert" type="danger" fontSize={12} lineHeight={1.35} ellipsis={{ rows: 3 }} title={store.error}>
            {loaded ? store.error : 'Não foi possível abrir as issues.'}
          </Text>
          <Button
            type="link" size="small" className={`h-auto! p-0! text-[12px]! ${FOCUS_RING}`}
            onClick={() => (loaded ? void store.refresh() : onShowDetail(true))}
          >
            {loaded ? 'Tentar de novo' : 'Ver detalhes'}
          </Button>
        </Flexbox>
      )}

      {loaded && filterOn && (
        <Flexbox id="issues-filter" gap={6} paddingInline={8} className="pb-2" flex="none">
          <Input
            ref={filterInput} size="small" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Filtrar"
            aria-label="Filtrar as abertas e as fechadas por título ou #número" autoComplete="off"
            onKeyDown={(e) => {
              // Esc limpa o texto; com o campo vazio não faz nada aqui (segue o que já fazia).
              if (e.key === 'Escape' && query !== '') {
                e.preventDefault();
                e.stopPropagation();
                setQuery('');
              }
            }} className={`h-[26px]! text-[12px]! ${FIELD_FOCUS}`}
            prefix={<Search aria-hidden className="size-3.5 text-[var(--ant-color-text-tertiary)]" />}
          />
          <FilterChips issues={store.issues} selected={wanted} onChange={setWanted} />
        </Flexbox>
      )}

      {loaded && (
        <>
          <div
            ref={list} className="min-h-0 flex-1 overflow-y-auto pb-1.5" style={{ '--issue-num': numWidth } as CSSProperties}
            onKeyDown={onListKey} onFocus={onListFocus} onClick={(e) => onListClick(e, false)} onDoubleClick={(e) => onListClick(e, true)}
          >
            <ActiveSection
              open={sections.active} onToggle={() => toggle('active')} issue={active} since={store.active} now={now} busy={start?.busy ?? false}
              tabKey={tabKey} selected={active !== null && active.number === shown}
            />
            <QueueSection
              open={sections.queue} onToggle={() => toggle('queue')} next={next} issues={rest} now={now}
              start={start} filtering={filtering} tabKey={tabKey} shown={shown} hasActive={active !== null}
            />
            <ClosedSection
              open={sections.closed} onToggle={() => toggle('closed')} issues={closed} filtering={filtering} tabKey={tabKey}
              shown={shown} onReopen={reopen}
            />
            {/* Uma vez só, no fim: as seções vazias com o filtro ativo ficam só com o contador 0. */}
            {filtering && !shownOpen.length && !closed.length && <NoMatch text={noMatch} onClear={clearFilter} />}
          </div>

          <Flexbox horizontal align="center" paddingInline={12} height={28} flex="none" className="border-t">
            <Text style={SECONDARY} as="span" fontSize={12} ellipsis className="min-w-0" title={footer}>{footer}</Text>
          </Flexbox>
        </>
      )}
    </Flexbox>
  );
}

/** Chips do filtro: as labels em uso nas abertas; com mais de uma marcada, a issue precisa ter todas. */
function FilterChips({ issues, selected, onChange }: {
  issues: readonly Issue[];
  selected: string[];
  onChange: (next: string[]) => void;
}) {
  const shown = filterLabels(issues, selected);
  if (!shown.length) return null;
  const toggle = (name: string) => onChange(selected.includes(name) ? selected.filter((n) => n !== name) : [...selected, name]);
  return (
    <div role="group" aria-label="Filtrar por label" className="flex flex-wrap items-center gap-1">
      {shown.map((name) => <LabelToggle key={name} name={name} on={selected.includes(name)} onToggle={() => toggle(name)} />)}
    </div>
  );
}

/**
 * Cabeçalho recolhível de uma seção da faixa, no padrão das seções do Git: chevron, título e a contagem à direita. `note` completa o nome
 * acessível. É uma linha da lista do teclado (`data-row`, parada de Tab só quando é a atual): Enter e Espaço recolhem, ← e → também.
 */
function SectionHeader({ id, section, title, count, open, onToggle, note, tabKey }: {
  id: string; section: keyof IssueSections; title: string; count: number; open: boolean; onToggle: () => void; note?: string; tabKey: string | null;
}) {
  return (
    <Flexbox horizontal align="center" height={22} className="pr-2">
      <button
        type="button" onClick={onToggle} aria-expanded={open} aria-label={`${title}, ${count} ${count === 1 ? 'issue' : 'issues'}${note ? `, ${note}` : ''}`}
        data-row={sectionKey(section)} data-section={section} tabIndex={sectionKey(section) === tabKey ? 0 : -1}
        className={`flex h-full min-w-0 flex-1 items-center text-left outline-none ${FOCUS_RING_INSET}`}
      >
        {open ? <ChevronDown className="mx-0.5 size-4 shrink-0" aria-hidden /> : <ChevronRight className="mx-0.5 size-4 shrink-0" aria-hidden />}
        <span id={id} aria-hidden className="min-w-0 shrink truncate text-[12px] font-medium">{title}</span>
        <span className="ml-auto shrink-0 rounded-full bg-[var(--ant-color-fill-secondary)] px-[5px] text-[12px] leading-[18px] tabular-nums">{count}</span>
      </button>
    </Flexbox>
  );
}

// Linha da faixa: hover em `bg-muted` e o anel de foco por dentro, só no foco por teclado.
const ROW_FOCUS = `outline-none hover:bg-muted ${FOCUS_RING_INSET}`;
// Linha escolhida (a que o detalhe mostra): `bg-muted` e o filete de 2 px na cor primária à esquerda, o da seleção de lista do app.
const ROW_SELECTED = 'bg-muted before:pointer-events-none before:absolute before:inset-y-0 before:left-0 before:w-0.5 before:bg-[var(--ant-color-primary)]';

/**
 * Seção "Em andamento": a issue com o orquestrador num cartão (tom de `bg-card` e filete neutro de 1 px, sem acento na borda), com a marca
 * "Em andamento" e o `#N`, o título em até duas linhas, há quanto tempo está lá e o estado (colada e aguardando envio, com as outras do
 * rascunho, se houver; fora do rascunho sem rodada; orquestrador trabalhando, com três pontos pulsando, ou livre). Sem ações: Concluir e Voltar às abertas ficam só no detalhe. O cartão é uma linha da
 * lista do teclado, logo abaixo do cabeçalho: escolhe a issue e mostra o detalhe.
 */
function ActiveSection({ open, onToggle, issue, since, now, busy, tabKey, selected }: {
  open: boolean;
  onToggle: () => void;
  issue: Issue | null;
  since: ActiveIssue | null;
  now: number;
  busy: boolean;
  tabKey: string | null;
  selected: boolean;
}) {
  const { draft } = useIssues();
  const key = issue ? issueKey(issue.number) : null;
  // O estado vem do rascunho do orquestrador: no rascunho, aguarda envio; fora dele e sem rodada, o texto não está mais lá (o orquestrador
  // reabriu, ou outro texto foi enviado); com rodada, o orquestrador.
  const drafted = !!since && draft.includes(since.number);
  const others = drafted ? draft.filter((n) => n !== since.number) : [];
  const state = drafted ? 'colada, aguardando envio' : since && !since.turned ? 'não está no rascunho' : busy ? 'orquestrador trabalhando' : 'orquestrador livre';
  const also = others.length ? `no rascunho também: ${others.map((n) => `#${n}`).join(', ')}` : '';
  const id = useId(), titleId = `${id}-t`, sinceId = `${id}-s`, statusId = `${id}-st`, headId = `${id}-h`;
  return (
    <Flexbox as="section" gap={0} aria-labelledby={headId}>
      <SectionHeader id={headId} section="active" title="Em andamento" count={issue ? 1 : 0} open={open} onToggle={onToggle} tabKey={tabKey} />
      {open && (issue && since ? (
        <div
          role="group" tabIndex={key === tabKey ? 0 : -1} data-row={key} data-issue={issue.number}
          aria-labelledby={titleId} aria-describedby={`${sinceId} ${statusId}`}
          aria-current={selected ? 'true' : undefined}
          className={`mx-2 mt-0.5 mb-1.5 rounded-md border px-2.5 py-2 ${ROW_FOCUS} ${selected ? 'bg-muted' : 'bg-card'}`}
        >
          <Flexbox horizontal align="center" gap={8}>
            <ActiveMark />
            <Text style={SECONDARY} as="span" code fontSize={12}>{`#${issue.number}`}</Text>
          </Flexbox>
          <Text id={titleId} as="div" fontSize={13} weight={500} lineHeight="18px" ellipsis={{ rows: 2 }} title={issue.title} className="mt-1.5">
            <span className="sr-only">{`#${issue.number} `}</span>{issue.title}<span className="sr-only"> (em andamento)</span>
          </Text>
          <Text id={sinceId} style={SECONDARY} as="div" fontSize={12} className="mt-1 tabular-nums">{`no orquestrador ${relativeTime(iso(since.since), now)}`}</Text>
          <Flexbox id={statusId} horizontal align="center" gap={6} role="status" className="mt-1 text-[12px] text-[var(--ant-color-text-secondary)]">
            {!drafted && since.turned && busy && (
              <span aria-hidden className="inline-flex shrink-0 items-center gap-[3px]">
                {[0, 200, 400].map((delay) => (
                  <span
                    key={delay} className="size-1 rounded-full motion-safe:animate-pulse"
                    style={{ background: GH.amber, animationDelay: `${delay}ms` }}
                  />
                ))}
              </span>
            )}
            <span className="min-w-0 truncate" title={state}>{state}</span>
          </Flexbox>
          {also && <Text style={SECONDARY} as="div" fontSize={12} ellipsis title={also} className="mt-0.5">{also}</Text>}
        </div>
      ) : (
        <Text as="p" fontSize={12} style={{ ...SECONDARY, paddingInline: 24, paddingBlock: 4 }}>Nada em andamento.</Text>
      ))}
    </Flexbox>
  );
}

/**
 * Seção "Abertas": a próxima no topo (com o relógio) e depois as abertas, da mais nova para a mais velha. A em andamento não entra (o nome
 * acessível diz isso). `issues` e `next` já vêm filtradas, e o aviso de nenhum resultado fica no fim da lista, uma vez só. O Colar aparece
 * na linha escolhida e na próxima.
 */
function QueueSection({ open, onToggle, next, issues, now, start, filtering, tabKey, shown, hasActive }: {
  open: boolean;
  onToggle: () => void;
  next: Issue | null;
  issues: Issue[];
  now: number;
  start: StartIssue;
  filtering: boolean;
  tabKey: string | null;
  shown: number | null;
  hasActive: boolean;
}) {
  const headId = useId();
  const count = issues.length + (next ? 1 : 0);
  const issueRow = (issue: Issue) => {
    const selected = issue.number === shown;
    return (
      <QueueRow
        key={issue.number} rowKey={issueKey(issue.number)} issue={issue.number} selected={selected} tabKey={tabKey} kind="open" title={issue.title}
        number={issue.number} after={issue === next ? ' (próxima)' : ''} next={issue === next} details={issueDetails(issue, now)}
        actions={selected || issue === next ? <StartButton issue={issue} start={start} /> : null}
      />
    );
  };
  return (
    <Flexbox as="section" gap={0} aria-labelledby={headId}>
      <SectionHeader
        id={headId} section="queue" title="Abertas" count={count} open={open} onToggle={onToggle} tabKey={tabKey}
        note={hasActive ? 'sem a em andamento' : undefined}
      />
      {open && count > 0 && (
        <ul className="grid grid-cols-[minmax(0,1fr)]">
          {next && issueRow(next)}
          {issues.map(issueRow)}
        </ul>
      )}
      {open && !count && !filtering && (
        <Text as="p" fontSize={12} style={{ ...SECONDARY, paddingInline: 24, paddingBlock: 4 }}>Nenhuma issue aberta.</Text>
      )}
    </Flexbox>
  );
}

/**
 * Linha de 24 px, como as da lista do Git: ícone de estado (14 px), o `#N` na coluna comum (`NumberCell`; na próxima, com o relógio antes
 * dele) e o título, que corta com reticências. O tempo e o nome das labels saem da vista e ficam no `title` da linha e na descrição
 * acessível (`details`). A linha é focável (uma parada de Tab na lista inteira, a de `tabKey`). O Colar da escolhida e da próxima fica no
 * fim da linha, no fluxo, só com o ícone (24 px), sem mudar a altura. A escolhida leva `aria-current`.
 */
function QueueRow({ rowKey, issue, selected, tabKey, kind, title, number, after = '', next = false, details, actions }: {
  rowKey: string;
  issue: number;
  selected: boolean;
  tabKey: string | null;
  kind: Parameters<typeof IssueIcon>[0]['kind'];
  title: string;
  /** O `#N` visível, que abre o nome acessível. */
  number: number;
  /** Texto só para leitor de tela depois do título ("(próxima)"). */
  after?: string;
  /** É a próxima: o relógio antes do `#N`. */
  next?: boolean;
  /** O que a linha não mostra (tempo, labels): vai no `title` e na descrição acessível. */
  details: string;
  actions: ReactNode;
}) {
  // Com as ações no fluxo de uma linha que não é a atual, cada uma viraria uma parada de Tab: só as da linha atual entram na ordem do teclado.
  const acts = useRef<HTMLDivElement>(null), tabbable = rowKey === tabKey;
  const id = useId(), numId = `${id}-n`, titleId = `${id}-t`, metaId = `${id}-m`;
  useEffect(() => {
    acts.current?.querySelectorAll<HTMLElement>('button').forEach((b) => (tabbable ? b.removeAttribute('tabindex') : b.setAttribute('tabindex', '-1')));
  });
  const heading = (
    <Text id={titleId} as="div" fontSize={13} ellipsis lineHeight="18px">
      {title}{after && <span className="sr-only">{after}</span>}
    </Text>
  );
  return (
    <li
      tabIndex={tabbable ? 0 : -1} aria-labelledby={`${numId} ${titleId}`} aria-describedby={metaId} data-row={rowKey} data-issue={issue}
      aria-current={selected ? 'true' : undefined} title={`${title} · ${details}`}
      className={`relative flex h-6 items-center gap-1.5 pr-2 pl-3.5 ${ROW_FOCUS} ${selected ? ROW_SELECTED : ''}`}
    >
      <IssueIcon kind={kind} size={14} />
      <NumberCell id={numId} number={number} next={next} />
      <div className="relative min-w-0 flex-1 overflow-hidden">{heading}</div>
      <span id={metaId} className="sr-only">{details}</span>
      {actions && <div ref={acts} className="relative flex flex-none items-center gap-0.5">{actions}</div>}
    </li>
  );
}

/** O `#N` da linha na coluna comum (`--issue-num`): mono de 12 px em texto secundário, alinhado à direita, sem encolher; na próxima, o relógio antes. */
function NumberCell({ id, number, next = false }: { id: string; number: number; next?: boolean }) {
  return (
    <span className="flex w-[var(--issue-num)] shrink-0 items-center justify-end gap-1 font-mono text-[12px] tabular-nums text-[var(--ant-color-text-secondary)]">
      {next && (
        <span title="Próxima: colada quando o orquestrador terminar" className="inline-flex shrink-0">
          <Clock aria-hidden size={14} />
        </span>
      )}
      <span id={id}>{`#${number}`}</span>
    </span>
  );
}

/**
 * Seção "Fechadas recentes" (começa recolhida): as 5 fechadas mais recentes que passam pelo filtro, em linhas de 24 px com a anatomia das
 * abertas: o ícone, o `#N` na mesma coluna e o título em 13 px no texto secundário. Clicar escolhe a issue (o detalhe a mostra); o Reabrir aparece na escolhida. Com o filtro
 * ativo e nada para mostrar, fica só o contador 0: o aviso de nenhum resultado é um só, no fim da lista.
 */
function ClosedSection({ open, onToggle, issues, filtering, tabKey, shown, onReopen }: {
  open: boolean;
  onToggle: () => void;
  issues: Issue[];
  filtering: boolean;
  tabKey: string | null;
  shown: number | null;
  onReopen: (issue: Issue, row: HTMLElement) => void;
}) {
  const headId = useId();
  return (
    <Flexbox as="section" gap={0} aria-labelledby={headId}>
      <SectionHeader id={headId} section="closed" title="Fechadas recentes" count={issues.length} open={open} onToggle={onToggle} tabKey={tabKey} />
      {open && (issues.length ? (
        <ul className="grid grid-cols-[minmax(0,1fr)]">
          {issues.map((issue) => (
            <ClosedRow key={issue.number} issue={issue} tabbable={issueKey(issue.number) === tabKey} selected={issue.number === shown} onReopen={onReopen} />
          ))}
        </ul>
      ) : (
        !filtering && <Text as="p" fontSize={12} style={{ ...SECONDARY, paddingInline: 24, paddingBlock: 4 }}>Nenhuma issue fechada recentemente.</Text>
      ))}
    </Flexbox>
  );
}

/** Linha de uma fechada: o nome acessível é o número e o título; a descrição diz que está fechada. */
function ClosedRow({ issue, tabbable, selected, onReopen }: {
  issue: Issue; tabbable: boolean; selected: boolean; onReopen: (issue: Issue, row: HTMLElement) => void;
}) {
  const id = useId(), numId = `${id}-n`, titleId = `${id}-t`, descId = `${id}-d`;
  return (
    <li
      tabIndex={tabbable ? 0 : -1} aria-labelledby={`${numId} ${titleId}`} aria-describedby={descId} data-row={issueKey(issue.number)} data-issue={issue.number}
      aria-current={selected ? 'true' : undefined}
      className={`relative flex h-6 items-center gap-1.5 pr-2 pl-3.5 ${ROW_FOCUS} ${selected ? ROW_SELECTED : ''}`}
    >
      <IssueIcon kind="closed" size={14} />
      <NumberCell id={numId} number={issue.number} />
      <Text id={titleId} style={SECONDARY} as="span" fontSize={13} lineHeight="18px" ellipsis title={issue.title} className="min-w-0 flex-1">{issue.title}</Text>
      <span id={descId} className="sr-only">fechada</span>
      {/* Só a linha atual deixa o botão no Tab: a lista é uma parada só. */}
      {selected && (
        <Button
          tabIndex={tabbable ? 0 : -1} size="small" icon={<RotateCcw className="size-3.5" />} aria-label={`Reabrir a issue #${issue.number}`} title="Reabre a issue"
          className={`h-5! flex-none px-1.5! text-[12px]! ${FOCUS_RING}`}
          onClick={(e) => onReopen(issue, e.currentTarget.closest('li')!)}
        >
          Reabrir
        </Button>
      )}
    </li>
  );
}

/** Aviso de nenhum resultado do filtro, com o botão que limpa a busca e as labels. */
function NoMatch({ text, onClear }: { text: string; onClear: () => void }) {
  return (
    <Flexbox gap={4} align="flex-start" style={{ paddingInline: 24, paddingBlock: 4 }}>
      <Text as="p" fontSize={12} style={SECONDARY}>{text}</Text>
      <Button size="small" className={`h-[22px]! text-[12px]! ${FOCUS_RING}`} onClick={onClear}>Limpar filtro</Button>
    </Flexbox>
  );
}
