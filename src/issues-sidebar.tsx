// Faixa lateral Issues: cabeçalho com atualizar, filtrar e abrir no GitHub, o repositório (só o nome; `dono/repo` no title) e a sincronia, o
// filtro (texto e labels), a captura rápida de ideias, a seção "Em andamento" (um cartão com a issue que está com o orquestrador), a seção
// "Abertas" (a próxima no topo, as criações a caminho ou que falharam, e as abertas), a seção "Fechadas recentes" e o rodapé com a contagem.
// O estado vem do store único do App (`useIssues`). Os cabeçalhos das seções, o cartão, as linhas da fila e as fechadas formam uma lista só
// para o teclado (roving tabindex, como a árvore do VS Code): ↑/↓ andam por tudo, Enter ou Espaço no cabeçalho recolhe e expande (← recolhe,
// → expande), e clicar ou Enter numa linha escolhe a issue (o detalhe da área central a mostra; o foco fica aqui). O duplo clique ou Enter na
// já escolhida leva o foco ao detalhe, e Ctrl+Enter faz o que o Colar faria. O Colar (e o Reabrir das fechadas) só aparece na linha escolhida
// e na próxima. Concluir e Voltar às abertas ficam só no detalhe; sem o GitHub, a faixa diz só "GitHub indisponível" e o detalhe tem o resto.
import { useEffect, useId, useRef, useState, type CSSProperties, type FocusEvent, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react';
import { ChevronDown, ChevronRight, CircleDot, Clock, ExternalLink, ListFilter, Maximize2, RefreshCw, RotateCcw, Search } from 'lucide-react';
import { ActionIcon, Button, Input, Text, TextArea, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from './backend';
import {
  failedReason, filterLabels, passesFilter, pendingRetrying, readSections, recentClosed, relativeTime, rovingStep, sortIssues, SECTIONS_DEFAULT,
  type IssueSections,
} from './issue-text';
import {
  ActiveMark, FailedActions, FIELD_FOCUS, FOCUS_BLUE, issuesUrl, FOCUS_RING, FOCUS_RING_INSET, GH, IssueIcon, LabelChips,
  LabelToggle, pendingKind, pendingLabels, SECONDARY, StartButton, useNow, WAITING_TEXT, type StartIssue,
} from './issue-ui';
import { useDetailIssue, useIssues, type ActiveIssue, type NewIssue, type PendingIssue } from './issues-store';
import type { Issue, Label } from './types';

const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });
const iso = (ms: number) => new Date(ms).toISOString();
// O estado de recolher das seções, o mesmo em todos os projetos.
const SECTIONS_KEY = 'orquestrador.issues.sections';
const RECENT_CLOSED = 5;
// O que o filtro ativo não achou: diz o que está ativo (busca, labels ou os dois).
const noMatchText = (query: string, wanted: string[]) =>
  `Nenhuma issue corresponde ${query.trim() !== '' && wanted.length > 0 ? 'à busca e às labels' : wanted.length > 0 ? 'às labels marcadas' : 'à busca'}.`;
// Chave de uma linha da lista, em `data-row`: `#N` numa issue, o `localId` numa criação. As linhas de issue também levam o número em `data-issue`.
const issueKey = (n: number) => `#${n}`;
// Chave do cabeçalho de uma seção na mesma lista (`data-row` e `data-section`).
const sectionKey = (s: keyof IssueSections) => `§${s}`;
// O que o texto de uma linha da fila diz fora da vista: tempo, comentários e o nome das labels (no `title` e na descrição acessível). O número
// fica fora: ele já está no nome acessível da linha.
const issueDetails = (i: Issue, now: number) => [
  relativeTime(i.createdAt, now),
  i.comments > 0 && `${i.comments} ${i.comments === 1 ? 'comentário' : 'comentários'}`,
  i.labels.length > 0 && `${i.labels.length === 1 ? 'label' : 'labels'}: ${i.labels.map((l) => l.name).join(', ')}`,
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

export function IssuesSidebar({ start, onShowDetail, onOpenEditor, captureRequest, onCaptureDismiss }: {
  start: StartIssue;
  /** Ativa a aba central Issues (o detalhe da issue escolhida); com `focus`, o foco vai para o detalhe. */
  onShowDetail: (focus: boolean) => void;
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
  // "Limpar filtro" do aviso de nenhum resultado: limpa busca e labels e devolve o foco ao campo (antes de o botão sumir com o aviso).
  const clearFilter = () => {
    filterInput.current?.focus();
    setQuery('');
    setWanted([]);
  };
  const noMatch = noMatchText(query, wanted);
  const now = useNow();

  const active = store.issues.find((i) => i.number === store.active?.number && i.state === 'open') ?? null;
  // Abertas: as abertas menos a em andamento, com a próxima no topo (acima até das criações).
  const open = sortIssues(store.issues.filter((i) => i.state === 'open' && i !== active), 'newest');
  const pending = [...store.pending].sort((a, b) => b.createdAt - a.createdAt);
  // O filtro vale para a fila e as fechadas; a em andamento e as criações ficam fora dele.
  const filtering = filterOn && (query.trim() !== '' || wanted.length > 0);
  const passes = (i: Issue) => passesFilter(i, query, wanted);
  const shownOpen = open.filter(passes);
  const next = shownOpen.find((i) => i.number === store.next) ?? null;
  const rest = shownOpen.filter((i) => i !== next);
  const closed = recentClosed(store.issues.filter(passes), RECENT_CLOSED);
  const numWidth = numColumn([...shownOpen, ...closed].map((i) => i.number), next?.number ?? null);
  // As criações que ainda não chegaram ao GitHub não contam como abertas: o rodapé as diz à parte (a que falhou continua contando durante a nova tentativa).
  const waiting = pending.filter((p) => p.status === 'waiting').length;
  const failed = pending.filter((p) => p.status === 'failed' || pendingRetrying(p)).length;
  const footer = [
    `${filtering ? `${shownOpen.length} de ${open.length}` : open.length} ${open.length === 1 ? 'aberta' : 'abertas'}`,
    active && '1 em andamento',
    waiting > 0 && `${waiting} ${waiting === 1 ? 'guardada' : 'guardadas'}`,
    failed > 0 && `${failed} ${failed === 1 ? 'falhou' : 'falharam'}`,
  ].filter(Boolean).join(' · ');

  // Teclado: uma parada de Tab na lista (a última linha focada, ou a primeira), e ↑/↓/Home/End entre os cabeçalhos das seções e as linhas
  // visíveis. O cartão Em andamento é a primeira linha da lista quando a seção está aberta.
  const cardOpen = sections.active && active !== null && store.active !== null;
  const showQueue = ready !== null || pending.length > 0;
  const keys = [
    ...(ready ? [sectionKey('active'), ...(cardOpen ? [issueKey(active.number)] : [])] : []),
    ...(showQueue ? [
      sectionKey('queue'),
      ...(sections.queue ? [...(ready && next ? [issueKey(next.number)] : []), ...pending.map((p) => p.localId), ...(ready ? rest : []).map((i) => issueKey(i.number))] : []),
    ] : []),
    ...(ready ? [sectionKey('closed'), ...(sections.closed ? closed.map((i) => issueKey(i.number)) : [])] : []),
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
  // Escolhe a issue da linha e mostra o detalhe. Criações (sem `data-issue`) não são escolhíveis.
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
  // Descartar uma criação tira a linha (e a pergunta "Descartar esta criação?", que é dela): o foco vai para a linha seguinte da seção, ou
  // para a anterior se era a última (o cabeçalho, se era a única). No quadro seguinte, depois da devolução de foco da janela que saiu.
  const discard = (localId: string) => {
    const rows = [...list.current!.querySelectorAll<HTMLElement>('[data-row]')];
    const at = rows.findIndex((r) => r.dataset.row === localId);
    const after = rows[at + 1];
    const key = (after && !after.dataset.section ? after : rows[at - 1])?.dataset.row;
    store.discard(localId);
    if (key === undefined) return;
    requestAnimationFrame(() => [...list.current?.querySelectorAll<HTMLElement>('[data-row]') ?? []].find((r) => r.dataset.row === key)?.focus());
  };
  const reopen = (issue: Issue, row: HTMLElement) => {
    if (row.contains(document.activeElement)) refocus.current = issueKey(issue.number);
    store.setState(issue.number, true).then(() => toast.success({ title: `#${issue.number} reaberta no GitHub`, duration: 3000 }), report);
  };

  return (
    <Flexbox ref={root} height="100%" className="min-h-0">
      {/* Cabeçalho de 36 px no padrão do Git e da pesquisa. */}
      <Flexbox horizontal align="center" gap={2} paddingInline={12} height={36} flex="none">
        <Text style={SECONDARY} as="h2" ellipsis weight={500} fontSize={12} className="min-w-0 flex-1">Issues</Text>
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
        <Text as="p" role="status" fontSize={12} style={{ ...SECONDARY, paddingInline: 12, paddingBlock: 4 }}>Conectando ao GitHub…</Text>
      )}

      {/* A mensagem inteira e o Tentar de novo ficam no detalhe; aqui só o aviso curto (o motivo no `title`) e o caminho até lá. */}
      {status?.kind === 'unavailable' && (
        <Flexbox gap={2} align="flex-start" paddingInline={12} className="pb-2" flex="none">
          <Text style={SECONDARY} as="span" fontSize={12} title={status.message}>GitHub indisponível</Text>
          <Button type="link" size="small" className={`h-auto! p-0! text-[12px]! ${FOCUS_RING}`} onClick={() => onShowDetail(true)}>Ver detalhes</Button>
        </Flexbox>
      )}

      {ready && (
        <>
          <Flexbox horizontal align="center" gap={6} paddingInline={12} className="-mt-1.5 pb-2" flex="none">
            <Text style={SECONDARY} as="span" code fontSize={12} ellipsis title={`${ready.owner}/${ready.repo}`} className="min-w-0">
              {ready.repo}
            </Text>
            <SyncState syncedAt={store.syncedAt} error={store.error} now={now} />
          </Flexbox>

          {filterOn && (
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
              <FilterChips issues={store.issues} labels={store.labels} selected={wanted} onChange={setWanted} />
            </Flexbox>
          )}
        </>
      )}

      {/* A captura não some sem o GitHub: a ideia fica guardada (`waiting`) entre as abertas e é criada quando ele voltar. */}
      {status !== null && (
        <>
          <Capture onOpenEditor={onOpenEditor} focusRequest={captureRequest} onDismiss={onCaptureDismiss} />

          <div
            ref={list} className="min-h-0 flex-1 overflow-y-auto pb-1.5" style={{ '--issue-num': numWidth } as CSSProperties}
            onKeyDown={onListKey} onFocus={onListFocus} onClick={(e) => onListClick(e, false)} onDoubleClick={(e) => onListClick(e, true)}
          >
            {ready && (
              <ActiveSection
                open={sections.active} onToggle={() => toggle('active')} issue={active} since={store.active} now={now} busy={start?.busy ?? false}
                tabKey={tabKey} selected={active !== null && active.number === shown}
              />
            )}
            {showQueue && (
              <QueueSection
                open={sections.queue} onToggle={() => toggle('queue')} pending={pending} next={ready ? next : null} issues={ready ? rest : []} now={now}
                start={start} filtering={filtering} tabKey={tabKey} shown={shown} hasActive={active !== null} onDiscard={discard}
              />
            )}
            {ready && (
              <ClosedSection
                open={sections.closed} onToggle={() => toggle('closed')} issues={closed} filtering={filtering} tabKey={tabKey}
                shown={shown} onReopen={reopen}
              />
            )}
            {/* Uma vez só, no fim: as seções vazias com o filtro ativo ficam só com o contador 0. */}
            {ready && filtering && !shownOpen.length && !closed.length && <NoMatch text={noMatch} onClear={clearFilter} />}
          </div>

          {ready && (
            <Flexbox horizontal align="center" paddingInline={12} height={28} flex="none" className="border-t">
              <Text style={SECONDARY} as="span" fontSize={12} ellipsis className="min-w-0" title={footer}>{footer}</Text>
            </Flexbox>
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

/** Ponto verde e "sincronizado há N s" (a cada segundo; depois de um minuto, o tempo relativo), ou ponto âmbar com a falha no `title`. */
function SyncState({ syncedAt, error, now }: { syncedAt: number | null; error: string | null; now: number }) {
  const seconds = syncedAt === null ? 0 : Math.max(0, Math.floor((now - syncedAt) / 1000));
  const text = error ? 'falha ao atualizar'
    : syncedAt === null ? 'sincronizando…'
      : seconds < 60 ? `sincronizado há ${seconds} s` : `sincronizado ${relativeTime(iso(syncedAt), now)}`;
  // O texto visível muda a cada segundo e não é região live; o leitor de tela só ouve as mudanças de estado, nesta região oculta.
  const announce = error ? 'Falha ao sincronizar com o GitHub.' : syncedAt === null ? 'Sincronizando com o GitHub…' : 'Sincronizado com o GitHub.';
  return (
    <span className="ml-auto flex shrink-0 items-center gap-[5px] text-[12px] text-[var(--ant-color-text-secondary)] tabular-nums" title={error ?? undefined}>
      <span className="sr-only" role="status" aria-live="polite">{announce}</span>
      <span
        aria-hidden className="size-1.5 rounded-full"
        style={{ background: error ? 'var(--ant-color-warning)' : 'var(--ant-color-success)' }}
      />
      <span>{text}</span>
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
  // Mensagem do título vazio: sobe a cada Enter recusado (a mensagem remonta e o leitor de tela a ouve de novo); 0 é sem mensagem.
  const [refused, setRefused] = useState(0);
  const refusedId = useId();
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
    setRefused(0);
    setTitle('');
    setBody('');
    setLabels([]);
    setExpanded(false);
  };
  // Título vazio não cria: o cartão sacode (só sem `prefers-reduced-motion`), a borda fica vermelha por um instante e, embaixo do campo, a
  // mensagem diz o motivo (até digitar algo no título ou a captura recolher).
  const refuse = () => {
    setRefused((n) => n + 1);
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
    if (store.status?.kind !== 'ready') toast.info({ title: 'Ideia guardada; será criada quando o GitHub estiver disponível.', duration: 4000 });
    // O foco vem antes de limpar: focar o título expande o cartão, e a limpeza o recolhe de novo.
    titleRef.current?.focus();
    clear();
  };
  const toEditor = () => {
    onOpenEditor({ title, body, labels });
    clear();
    // O botão que tinha o foco sai junto com o cartão aberto e não há blur: sem isto a borda azul ficaria acesa.
    viaShortcut.current = false;
    setFocused(false);
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
    setRefused(0);
    setFocused(false);
    setExpanded(false);
  };

  const border = invalid ? 'var(--ant-color-error)' : focused ? FOCUS_BLUE : 'var(--ant-color-border)';
  return (
    <div
      ref={card} onFocus={onFocusIn} onBlur={onFocusOut}
      className="@container mx-2 mb-2 flex-none rounded-md border bg-card transition-colors"
      style={{ borderColor: border }}
    >
      <Flexbox horizontal align="center" gap={8} paddingInline={8} height={32}>
        <CircleDot aria-hidden className="size-4 shrink-0 text-[var(--ant-color-text-quaternary)]" />
        {/* Sem o realce de erro do próprio Input (sairia colado no texto, sem recuo): o sinal é a borda do cartão e a mensagem abaixo. */}
        <Input
          ref={titleRef} variant="borderless" size="small" value={title} onKeyDown={onTitleKey}
          onChange={(e) => {
            setTitle(e.target.value);
            setRefused(0);
          }}
          aria-invalid={refused > 0 || undefined} aria-describedby={refused > 0 ? refusedId : undefined}
          placeholder="Anotar uma ideia…" aria-label="Título da nova issue" title="Anotar uma ideia (Ctrl+Shift+I)" aria-keyshortcuts="Control+Shift+I"
          autoComplete="off" className="min-w-0 flex-1 px-0! has-[[aria-invalid=true]]:border-transparent! has-[[aria-invalid=true]]:shadow-none!"
        />
        {/* A dica do atalho só cabe em cartão largo (>= 240 px) e some ao escrever ou focar. */}
        {title === '' && !focused && (
          <span aria-hidden className="hidden shrink-0 text-[12px] text-[var(--ant-color-text-secondary)] @[15rem]:inline">Ctrl+Shift+I</span>
        )}
      </Flexbox>
      {refused > 0 && (
        <Text key={refused} as="div" id={refusedId} role="alert" type="danger" fontSize={12} lineHeight={1.35} style={{ paddingInline: 32, paddingBottom: 4 }}>
          Escreva um título para criar.
        </Text>
      )}
      {open && (
        <Flexbox gap={6} paddingInline={8} className="pb-2">
          <TextArea
            ref={bodyRef} autoSize={{ minRows: 3, maxRows: 8 }} value={body} onChange={(e) => setBody(e.target.value)} onKeyDown={onBodyKey}
            placeholder="Descrição (opcional)" aria-label="Descrição da nova issue" className={`text-[13px]! ${FIELD_FOCUS}`}
          />
          <LabelChips issues={store.issues} labels={store.labels} selected={labels} onChange={setLabels} />
          <Flexbox horizontal align="center" gap={8} className="mt-0.5">
            <Flexbox flex={1} className="min-w-0">
              <Text style={SECONDARY} as="span" fontSize={12} lineHeight={1.35}>Enter cria · Esc cancela</Text>
              <Button
                type="text" size="small" icon={<Maximize2 className="size-3.5" />} aria-label="Abrir no editor completo" title="Abrir no editor completo"
                className={`self-start! px-1! text-[12px]! ${FOCUS_RING}`} onClick={toEditor}
              >
                Editor completo
              </Button>
            </Flexbox>
            <Button size="small" type="primary" className={FOCUS_RING} onClick={create}>Criar</Button>
          </Flexbox>
        </Flexbox>
      )}
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
 * Seção "Abertas": a próxima no topo (com o relógio), as criações (mais novas em cima) e depois as abertas, da mais nova para a mais velha.
 * A em andamento não entra (o nome acessível diz isso). `issues` e `next` já vêm filtradas; as criações não passam pelo filtro, e o aviso
 * de nenhum resultado fica no fim da lista, uma vez só. O Colar aparece na linha escolhida e na próxima.
 */
function QueueSection({ open, onToggle, pending, next, issues, now, start, filtering, tabKey, shown, hasActive, onDiscard }: {
  open: boolean;
  onToggle: () => void;
  pending: PendingIssue[];
  next: Issue | null;
  issues: Issue[];
  now: number;
  start: StartIssue;
  filtering: boolean;
  tabKey: string | null;
  shown: number | null;
  hasActive: boolean;
  /** Descarta uma criação e leva o foco à linha vizinha. */
  onDiscard: (localId: string) => void;
}) {
  const store = useIssues();
  const headId = useId();
  const found = issues.length + (next ? 1 : 0);
  const count = pending.length + found;
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
          {pending.map((p) => {
            const labels = pendingLabels(p.labels, store.labels);
            // O motivo inteiro, que quebra linha: a criação é rara e precisa ser lida. A que já falhou mantém o motivo e as ações (desabilitadas)
            // durante a nova tentativa, com o espaço do texto da falha reservado: a linha não muda de altura.
            const retrying = pendingRetrying(p);
            const reason = p.status === 'waiting' ? WAITING_TEXT : p.status === 'sending' && !retrying ? 'Enviando…' : failedReason(p);
            return (
              <QueueRow
                key={p.localId} rowKey={p.localId} issue={null} selected={false} tabKey={tabKey} kind={pendingKind(p)} title={p.title}
                details={[reason, relativeTime(iso(p.createdAt), now), labels.length > 0 && `labels: ${labels.map((l) => l.name).join(', ')}`].filter(Boolean).join(' · ')}
                reason={reason} reasonSpace={retrying ? failedReason(p, true) : undefined}
                actions={p.status !== 'sending' || retrying ? <FailedActions pending={p} retry={store.retry} discard={onDiscard} /> : null}
              />
            );
          })}
          {issues.map(issueRow)}
        </ul>
      )}
      {open && !count && !filtering && (
        <Text as="p" fontSize={12} style={{ ...SECONDARY, paddingInline: 24, paddingBlock: 4 }}>Nenhuma issue aberta. Anote uma ideia acima.</Text>
      )}
    </Flexbox>
  );
}

/**
 * Linha de 24 px, como as da lista do Git: ícone de estado (14 px), o `#N` na coluna comum (`NumberCell`; na próxima, com o relógio antes
 * dele) e o título, que corta com reticências. O tempo, os comentários e o nome das labels saem da vista e ficam no `title` da linha e na
 * descrição acessível (`details`). A linha é focável (uma parada de Tab na lista inteira, a de `tabKey`). O Colar da escolhida e da próxima
 * fica no fim da linha, no fluxo, só com o ícone (24 px), sem mudar a altura. `issue` é o número da issue (null numa criação, que não é
 * escolhível); a escolhida leva `aria-current`.
 *
 * A criação pendente (`reason`) é a exceção, de altura própria: o título numa linha e, embaixo, o motivo inteiro, que quebra linha, e as
 * ações com rótulo. São raras e precisam ser lidas. `reasonSpace` é um texto invisível no mesmo lugar do motivo, que guarda a altura dele
 * (o da falha, durante a nova tentativa).
 */
function QueueRow({ rowKey, issue, selected, tabKey, kind, title, number, after = '', next = false, details, reason, reasonSpace, actions }: {
  rowKey: string;
  issue: number | null;
  selected: boolean;
  tabKey: string | null;
  kind: Parameters<typeof IssueIcon>[0]['kind'];
  title: string;
  /** O `#N` visível, que abre o nome acessível (a criação não tem). */
  number?: number;
  /** Texto só para leitor de tela depois do título ("(próxima)"). */
  after?: string;
  /** É a próxima: o relógio antes do `#N`. */
  next?: boolean;
  /** O que a linha não mostra (tempo, comentários, labels): vai no `title` e na descrição acessível. */
  details: string;
  /** Só na criação pendente: o texto de baixo, visível e inteiro. */
  reason?: string;
  /** Só na criação pendente: o texto que reserva a altura do motivo, sem aparecer nem ser lido. */
  reasonSpace?: string;
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
  const rowProps = {
    tabIndex: tabbable ? 0 : -1, 'aria-labelledby': number === undefined ? titleId : `${numId} ${titleId}`, 'aria-describedby': metaId, 'data-row': rowKey, 'data-issue': issue ?? undefined,
    'aria-current': selected ? ('true' as const) : undefined, title: `${title} · ${details}`,
  };
  if (reason !== undefined) {
    return (
      <li {...rowProps} className={`relative flex min-h-6 items-start gap-1.5 py-1 pr-2 pl-3.5 ${ROW_FOCUS}`}>
        <span className="mt-0.5"><IssueIcon kind={kind} size={14} /></span>
        <div className="min-w-0 flex-1">
          {heading}
          <div id={metaId} className="grid text-[12px] leading-4 [overflow-wrap:anywhere] text-[var(--ant-color-text-secondary)]">
            <span className="[grid-area:1/1]">{reason}</span>
            {reasonSpace && <span aria-hidden className="invisible [grid-area:1/1]">{reasonSpace}</span>}
          </div>
          {actions && <div ref={acts} className="mt-1 flex flex-wrap items-center gap-1">{actions}</div>}
        </div>
      </li>
    );
  }
  return (
    <li {...rowProps} className={`relative flex h-6 items-center gap-1.5 pr-2 pl-3.5 ${ROW_FOCUS} ${selected ? ROW_SELECTED : ''}`}>
      <IssueIcon kind={kind} size={14} />
      {number !== undefined && <NumberCell id={numId} number={number} next={next} />}
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
 * abertas: o ícone, o `#N` na mesma coluna e o título em 13 px no texto secundário. Clicar escolhe a issue (o detalhe a mostra, e abre no GitHub de lá); o Reabrir aparece na escolhida. Com o filtro
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
          tabIndex={tabbable ? 0 : -1} size="small" icon={<RotateCcw className="size-3.5" />} aria-label={`Reabrir a issue #${issue.number}`} title="Reabre no GitHub"
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
