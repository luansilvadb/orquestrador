// Página Issues da área central, no layout da página de Issues do GitHub (modo escuro) com as cores do app: cabeçalho com "New issue",
// a busca, e a caixa com Open/Closed, o filtro de Labels, a ordenação e as linhas. As criações a caminho ou que falharam ficam no topo
// da lista Open. O estado vem do store único do App (`useIssues`); busca, filtro e ordenação são só desta tela.
import { useState } from 'react';
import { Check, ChevronDown, MessageSquare, Search } from 'lucide-react';
import { Button, DropdownMenu, Input, Text, toast } from '@lobehub/ui/base-ui';
import { Center, Flexbox } from '@lobehub/ui';
import { backend, message } from './backend';
import { hasLabels, matchesQuery, relativeTime, sortIssues, type IssueOrder } from './issue-text';
import {
  ActiveMark, FailedActions, GREEN_BUTTON, IssueIcon, LabelMenu, LabelPill, NextMark, pendingKind, pendingLabels, StartButton, useNow, type StartIssue,
} from './issue-ui';
import { useIssues, type PendingIssue } from './issues-store';
import type { Issue } from './types';

const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });
const ORDERS: { key: IssueOrder; label: string }[] = [
  { key: 'newest', label: 'Mais novas' },
  { key: 'oldest', label: 'Mais antigas' },
  { key: 'updated', label: 'Atualizadas recentemente' },
];
// Ações que aparecem só no hover e com o foco na linha; com `opacity`, o Tab ainda as alcança (a linha de uma criação não tem outro foco).
const HOVER_ONLY = 'opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100';
// O Iniciar, como o da faixa: escondido até o hover ou o foco na linha (o título é focável, então o Tab chega nele), com ou sem
// orquestrador, e visível enquanto o menu dele está aberto. `invisible` guarda o lugar: a linha não muda de altura no hover.
const START_SLOT = 'invisible group-focus-within:visible group-hover:visible has-[[aria-expanded=true]]:visible';

export function IssuesPage({ start, onNew }: { start: StartIssue; onNew: () => void }) {
  const store = useIssues();
  const { status } = store;
  const [tab, setTab] = useState<'open' | 'closed'>('open');
  const [query, setQuery] = useState('');
  const [wanted, setWanted] = useState<string[]>([]);
  const [order, setOrder] = useState<IssueOrder>('newest');
  const now = useNow(30_000);

  const passes = (title: string, number: number | null, labels: string[]) => matchesQuery(title, number, query) && hasLabels(labels, wanted);
  const shown = store.issues.filter((i) => passes(i.title, i.number, i.labels.map((l) => l.name)));
  const open = sortIssues(shown.filter((i) => i.state === 'open'), order);
  const closed = sortIssues(shown.filter((i) => i.state === 'closed'), order);
  const pending = store.pending.filter((p) => passes(p.title, null, p.labels)).sort((a, b) => b.createdAt - a.createdAt);
  const filtering = query.trim() !== '' || wanted.length > 0;
  const rows = tab === 'open' ? open : closed;
  const empty = tab === 'open' ? rows.length + pending.length === 0 : rows.length === 0;
  const ready = status?.kind === 'ready';

  const stateButton = (key: 'open' | 'closed', label: string, n: number) => (
    <Button
      type="text" size="small" aria-pressed={tab === key} onClick={() => setTab(key)}
      className={`h-auto! gap-1.5! px-1! text-[14px]! ${tab === key ? 'font-semibold! text-[var(--ant-color-text)]!' : 'text-[var(--ant-color-text-secondary)]!'}`}
    >
      {label}
      <span className="rounded-full bg-[var(--ant-color-fill-secondary)] px-[7px] text-[12px] leading-[18px] font-medium text-[var(--ant-color-text)] tabular-nums">{n}</span>
    </Button>
  );

  return (
    <div className="h-full min-h-0 overflow-y-auto">
      <Flexbox gap={16} padding={24} className="mx-auto w-full max-w-[1280px]">
        <Flexbox horizontal align="center" justify="space-between" gap={12}>
          <Text as="h1" fontSize={20} weight={600} style={{ margin: 0 }}>Issues</Text>
          <Button
            className={GREEN_BUTTON} disabled={!ready} title={ready ? undefined : 'O GitHub não está disponível agora.'} onClick={onNew}
          >
            New issue
          </Button>
        </Flexbox>

        <Input
          value={query} onChange={(e) => setQuery(e.target.value)} allowClear onClear={() => setQuery('')}
          prefix={<Search aria-hidden className="size-4 text-[var(--ant-color-text-tertiary)]" />}
          placeholder="Buscar por título ou #número" aria-label="Buscar issues" autoComplete="off"
        />

        <div className="overflow-hidden rounded-md border">
          {!status || status.kind === 'unavailable' ? (
            <Center gap={12} padding={32}>
              <Text as="p" role="status" type="secondary" fontSize={14} style={{ margin: 0, textAlign: 'center' }}>
                {status ? status.message : 'Conectando ao GitHub…'}
              </Text>
              {status && (
                <Button loading={store.refreshing} disabled={store.refreshing} onClick={() => void store.refresh()}>Tentar de novo</Button>
              )}
            </Center>
          ) : (
            <>
              <Flexbox
                horizontal align="center" justify="space-between" gap={16} height={48} paddingInline={16}
                className="border-b bg-[var(--ant-color-fill-quaternary)]"
              >
                <Flexbox horizontal align="center" gap={16} role="group" aria-label="Estado">
                  {stateButton('open', 'Open', open.length + pending.length)}
                  {stateButton('closed', 'Closed', closed.length)}
                </Flexbox>
                <Flexbox horizontal align="center" gap={16}>
                  <LabelMenu labels={store.labels} selected={wanted} onChange={setWanted}>
                    <Button type="text" size="small" className="h-auto! gap-1! px-1! text-[13px]! text-[var(--ant-color-text-secondary)]!" aria-label={`Filtrar por labels${wanted.length ? ` (${wanted.length} marcadas)` : ''}`}>
                      Labels
                      {wanted.length > 0 && (
                        <span className="rounded-full bg-[var(--ant-color-fill-secondary)] px-1.5 text-[11px] leading-4 text-[var(--ant-color-text)] tabular-nums">{wanted.length}</span>
                      )}
                      <ChevronDown aria-hidden className="size-3.5" />
                    </Button>
                  </LabelMenu>
                  <DropdownMenu
                    placement="bottomRight"
                    items={ORDERS.map((o) => ({
                      key: o.key, label: o.label, icon: o.key === order ? Check : undefined, onClick: () => setOrder(o.key),
                    }))}
                  >
                    <Button type="text" size="small" className="h-auto! gap-1! px-1! text-[13px]! text-[var(--ant-color-text-secondary)]!" aria-label={`Ordenar: ${ORDERS.find((o) => o.key === order)!.label}`}>
                      {ORDERS.find((o) => o.key === order)!.label}
                      <ChevronDown aria-hidden className="size-3.5" />
                    </Button>
                  </DropdownMenu>
                </Flexbox>
              </Flexbox>

              {empty ? (
                <Center gap={8} padding={32}>
                  {filtering && <Search aria-hidden className="size-6 text-[var(--ant-color-text-tertiary)]" />}
                  <Text as="p" type="secondary" fontSize={14} style={{ margin: 0 }}>
                    {filtering ? 'Nenhuma issue corresponde à busca.' : tab === 'open' ? 'Nenhuma issue aberta.' : 'Nenhuma issue fechada.'}
                  </Text>
                </Center>
              ) : (
                <ul aria-label={tab === 'open' ? 'Issues abertas' : 'Issues fechadas'}>
                  {tab === 'open' && pending.map((p) => <PendingRow key={p.localId} pending={p} />)}
                  {rows.map((issue) => <IssueRow key={issue.number} issue={issue} now={now} start={start} />)}
                </ul>
              )}
            </>
          )}
        </div>
      </Flexbox>
    </div>
  );
}

/**
 * Uma issue: ícone, título (abre no GitHub) com a marca de em andamento ou de próxima e as labels em pílula, "#N · autor abriu há X"
 * (ou "fechada há X"), comentários e o Iniciar (a em andamento não tem).
 */
function IssueRow({ issue, now, start }: { issue: Issue; now: number; start: StartIssue }) {
  const store = useIssues();
  const isOpen = issue.state === 'open';
  const isActive = isOpen && store.active?.number === issue.number;
  const isNext = isOpen && store.next === issue.number;
  const when = isOpen ? `${issue.author} abriu ${relativeTime(issue.createdAt, now)}` : `fechada ${relativeTime(issue.closedAt ?? issue.updatedAt, now)}`;
  return (
    <li className="group flex items-start gap-3 border-t px-4 py-3 first:border-t-0 hover:bg-muted">
      <span className="pt-0.5"><IssueIcon kind={isOpen ? 'open' : 'closed'} /></span>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-x-1.5 gap-y-1">
          <button
            type="button" onClick={() => void backend.githubOpen(issue.url).catch(report)} title="Abrir no GitHub"
            className="min-w-0 text-left text-[14px] font-semibold text-[var(--ant-color-text)] outline-none hover:text-[var(--ant-color-primary)] hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            {issue.title}
          </button>
          {isActive && <ActiveMark />}
          {isNext && <NextMark pill />}
          {issue.labels.map((l) => <LabelPill key={l.name} label={l} />)}
        </div>
        <Text as="div" type="secondary" fontSize={12} style={{ marginTop: 4 }}>
          <span className="font-mono">{`#${issue.number}`}</span>{` · ${when}`}
        </Text>
      </div>
      <Flexbox horizontal align="center" gap={12} flex="none">
        {issue.comments > 0 && (
          <span className="inline-flex items-center gap-1 text-[12px] text-muted-foreground" aria-label={`${issue.comments} ${issue.comments === 1 ? 'comentário' : 'comentários'}`}>
            <MessageSquare aria-hidden className="size-3.5" />
            {issue.comments}
          </span>
        )}
        {/* O esconder fica no invólucro: no botão, o estilo de desabilitado do Lobe UI venceria. */}
        {isOpen && !isActive && <span className={START_SLOT}><StartButton issue={issue} start={start} /></span>}
      </Flexbox>
    </li>
  );
}

/** Uma criação que ainda não chegou ao GitHub: enviando, ou falhou com Tentar de novo e Descartar. */
function PendingRow({ pending }: { pending: PendingIssue }) {
  const store = useIssues();
  const failed = pending.status === 'failed';
  return (
    <li className="group flex items-start gap-3 border-t px-4 py-3 first:border-t-0 hover:bg-muted">
      <span className="pt-0.5"><IssueIcon kind={pendingKind(pending)} /></span>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-x-1.5 gap-y-1">
          <span className="min-w-0 text-[14px] font-semibold">{pending.title}</span>
          {pendingLabels(pending.labels, store.labels).map((l) => <LabelPill key={l.name} label={l} />)}
        </div>
        <Text as="div" type={failed ? 'danger' : 'secondary'} fontSize={12} style={{ marginTop: 4 }}>
          {failed ? `falhou: ${pending.error ?? 'erro desconhecido'}` : 'enviando…'}
        </Text>
      </div>
      {failed && (
        <Flexbox horizontal align="center" gap={4} flex="none" className={HOVER_ONLY}>
          <FailedActions pending={pending} retry={store.retry} discard={store.discard} />
        </Flexbox>
      )}
    </li>
  );
}
