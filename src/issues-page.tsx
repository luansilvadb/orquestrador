// Detalhe da issue na aba Issues da área central: a issue escolhida na faixa (ou, sem escolha, a em andamento, a próxima ou a primeira aberta;
// ver `detailIssue`). Cabeçalho com o título em toda a largura, o estado em `Tag`, "autor abriu há X", os comentários e as labels, e, numa
// linha abaixo, a barra de ações (Colar no orquestrador como ação principal, Concluir e Voltar às abertas só quando não há o painel, Reabrir,
// Abrir no GitHub); na issue em andamento, o painel "Com o orquestrador" (o que ele está fazendo, os executores agora e as ações Ir ao
// orquestrador, Concluir e Voltar às abertas); na próxima, uma linha com o Não colar depois; depois, a descrição em Markdown e os
// comentários carregados do GitHub. É também o único lugar da indisponibilidade do GitHub: título, mensagem, Tentar de novo e, quando a
// mensagem traz um comando, o Rodar no Terminal. A faixa é a fila e o único lugar de lista, busca e filtro: esta tela não tem nada disso.
// O estado vem do store único do App (`useIssues`); os executores e as ações de terminal vêm por props.
import { Suspense, useEffect, useId, useRef, useState } from 'react';
import { Clock, ExternalLink, MessageSquare, SquareTerminal } from 'lucide-react';
import { ActionIcon, Button, Text, toast } from '@lobehub/ui/base-ui';
import { Center } from '@lobehub/ui';
import { backend, message } from './backend';
import { commandIn, issueList } from './issue-text';
import {
  Ago, FOCUS_RING, GH, GithubMessage, IssueActions, IssueMarkdown, LabelPill, MARKDOWN_FOCUS, NextMark, SECONDARY, StartButton, StateMark, ActiveMark, type StartIssue,
} from './issue-ui';
import { useDetailIssue, useIssues } from './issues-store';
import { FINISHED, STATES } from './terminal-screen';
import type { ExecutorNode, Issue, IssueComment, NodeState } from './types';

const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });
const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
// Medida de leitura da descrição e dos comentários.
const MEASURE = 'max-w-[72ch]';

export function IssuesPage({ start, onNew, focusKey, executors, onFocusOrchestrator, onRunInTerminal }: {
  start: StartIssue;
  /** Abre a aba Nova issue (o botão do estado vazio). */
  onNew: () => void;
  /** Sobe a cada pedido de foco no detalhe (duplo clique ou Enter com a linha já selecionada na faixa): a rolagem da tela recebe o foco. */
  focusKey: number;
  /** Executores vivos do App, resumidos no painel "Com o orquestrador". */
  executors: readonly ExecutorNode[];
  /** Leva o foco ao terminal do orquestrador. */
  onFocusOrchestrator: () => void;
  /** Abre o Terminal e cola o comando no shell ativo, sem Enter. */
  onRunInTerminal: (command: string) => void;
}) {
  const store = useIssues();
  const { status } = store;
  const issue = useDetailIssue();
  const command = status?.kind === 'unavailable' ? commandIn(status.message) : null;
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (focusKey === 0) return;
    const frame = requestAnimationFrame(() => root.current?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(frame);
  }, [focusKey]);
  // Concluir, Voltar às abertas, Reabrir e Não colar depois tiram o próprio botão da tela: o foco vem para o detalhe, que segue com a issue.
  const focusDetail = () => root.current?.focus({ preventScroll: true });

  return (
    <div ref={root} tabIndex={-1} className="@container h-full min-h-0 overflow-x-hidden overflow-y-auto outline-none">
      <div className="mx-auto w-full max-w-[928px] min-w-0 p-3 @[30rem]:p-6">
        {status?.kind === 'ready' ? (
          issue ? (
            <IssueDetail issue={issue} start={start} executors={executors} onFocusOrchestrator={onFocusOrchestrator} onDone={focusDetail} />
          ) : (
            <Center gap={12} padding={32}>
              <Text as="p" fontSize={14} style={{ ...SECONDARY, margin: 0 }}>Nenhuma issue aberta.</Text>
              <Button className={FOCUS_RING} onClick={onNew}>Nova issue</Button>
            </Center>
          )
        ) : status ? (
          // A indisponibilidade só é dita aqui (a faixa mostra uma linha curta com "Ver detalhes").
          <Center gap={12} padding={32}>
            <Text as="h1" fontSize={20} weight={600} lineHeight="28px" style={{ margin: 0 }}>GitHub indisponível</Text>
            {/* As ações ficam juntas sob a mensagem: com comando, o Rodar no Terminal é a principal; sem, o Tentar de novo. */}
            <GithubMessage message={status.message} size={14}>
              {command && (
                <Button type="primary" size="small" className={FOCUS_RING} icon={<SquareTerminal className="size-3.5" />} onClick={() => onRunInTerminal(command)}>
                  Rodar no Terminal
                </Button>
              )}
              <Button
                type={command ? undefined : 'primary'} size="small" className={FOCUS_RING} loading={store.refreshing} disabled={store.refreshing}
                onClick={() => void store.refresh()}
              >
                Tentar de novo
              </Button>
            </GithubMessage>
          </Center>
        ) : (
          <Center gap={12} padding={32}>
            <Text as="p" role="status" fontSize={14} style={{ ...SECONDARY, margin: 0, textAlign: 'center' }}>Conectando ao GitHub…</Text>
          </Center>
        )}
      </div>
    </div>
  );
}

/**
 * Cabeçalho (título, estado, quem abriu, comentários, labels e as ações), descrição e comentários de uma issue. Os tempos ("há X") vêm do
 * relógio das Issues pelo `Ago`: só eles se atualizam, sem renderizar de novo a descrição e os comentários. `onDone` leva o foco ao detalhe
 * depois de uma ação que some com o próprio botão.
 */
function IssueDetail({ issue, start, executors, onFocusOrchestrator, onDone }: {
  issue: Issue; start: StartIssue; executors: readonly ExecutorNode[]; onFocusOrchestrator: () => void; onDone: () => void;
}) {
  const store = useIssues();
  const isOpen = issue.state === 'open';
  const isActive = isOpen && store.active?.number === issue.number;
  const isNext = isOpen && store.next === issue.number;
  const body = issue.body.trim();
  const hasBar = isOpen ? !(isActive && store.draft.includes(issue.number)) : true;
  return (
    <article aria-label={`Issue #${issue.number}`} className="@container flex min-w-0 flex-col">
      <header className="flex flex-col gap-3 border-b pb-4">
        <div className="min-w-0">
          <Text as="h1" fontSize={20} weight={600} lineHeight="28px" style={{ margin: 0, overflowWrap: 'anywhere', textWrap: 'balance' }}>
            {issue.title}
            <span className="ml-2 font-normal" style={SECONDARY}>{`#${issue.number}`}</span>
          </Text>
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1.5 text-[12px]" style={SECONDARY}>
            {isActive ? <ActiveMark size="middle" /> : <StateMark closed={!isOpen} size="middle" />}
            {isNext && <NextMark size="middle" />}
            <span>
              {`${issue.author} abriu `}<Ago iso={issue.createdAt} />
              {!isOpen && <>{' · fechada '}<Ago iso={issue.closedAt ?? issue.updatedAt} /></>}
            </span>
            {issue.comments > 0 && (
              <span className="inline-flex items-center gap-1">
                <MessageSquare aria-hidden className="size-3.5" />
                {count(issue.comments, 'comentário', 'comentários')}
              </span>
            )}
            {issue.labels.map((l) => <LabelPill key={l.name} label={l} />)}
            <ActionIcon
              size="small" icon={ExternalLink} className={FOCUS_RING} aria-label={`Abrir a issue #${issue.number} no GitHub`} title="Abrir no GitHub"
              onClick={() => void backend.githubOpen(issue.url).catch(report)}
            />
          </div>
        </div>
        {/* Os botões fluem numa linha abaixo do título e quebram só quando preciso; a dica do Colar (`order-last basis-full`) fica numa linha
            abaixo deles, à esquerda como eles. Concluir e Voltar às abertas moram no painel da em andamento: aqui só o Reabrir da fechada.
            A barra só existe com botão: o Colar (aberta, menos a em andamento ainda não enviada, que o `StartButton` omite) ou o Reabrir. */}
        {hasBar && (
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 [&_span.order-last]:text-left!">
            {isOpen && <StartButton issue={issue} start={start} variant="primary" />}
            {!isActive && <IssueActions issue={issue} onDone={onDone} />}
          </div>
        )}
      </header>

      {isActive && <OrchestratorPanel issue={issue} start={start} executors={executors} onFocusOrchestrator={onFocusOrchestrator} onDone={onDone} />}
      {isNext && !isActive && (
        <div className="mt-4 flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-[8px] border bg-card px-3 py-2">
          <Clock aria-hidden className="size-4 shrink-0" style={SECONDARY} />
          <Text as="span" role="status" fontSize={14} style={SECONDARY} className="min-w-0 flex-1">
            Será colada sozinha quando o orquestrador terminar a rodada
          </Text>
          <Button
            size="small" className={FOCUS_RING}
            onClick={() => {
              onDone();
              if (start) start.later(null);
              else store.setNext(null);
            }}
          >
            Não colar depois
          </Button>
        </div>
      )}

      <div className={`${MEASURE} pt-4`}>
        {body ? (
          <Suspense fallback={<Text as="p" role="status" fontSize={14} style={{ ...SECONDARY, margin: 0 }}>Carregando a descrição…</Text>}>
            <div className={MARKDOWN_FOCUS}><IssueMarkdown fontSize={14}>{body}</IssueMarkdown></div>
          </Suspense>
        ) : <Text as="p" fontSize={14} style={{ ...SECONDARY, margin: 0 }}>Sem descrição.</Text>}
      </div>

      {issue.comments > 0 && <Comments issue={issue} />}
    </article>
  );
}

// Os estados vivos dos executores, na ordem em que o resumo os conta.
const LIVE: NodeState[] = ['running', 'queued', 'awaiting-choice', 'awaiting-orchestrator'];

/** "2 executando · 1 na fila" com os rótulos de `STATES`, ou "Nenhum executor rodando". Os executores não são ligados a uma issue. */
function executorsSummary(executors: readonly ExecutorNode[]): string {
  const parts = LIVE
    .map((state) => [executors.filter((e) => e.state === state).length, STATES[state].label] as const)
    .filter(([n]) => n)
    .map(([n, label]) => `${n} ${label}`);
  return parts.length ? parts.join(' · ') : 'Nenhum executor rodando';
}

/**
 * Painel "Com o orquestrador" da issue em andamento: o que o orquestrador faz agora (no mesmo vocabulário do cartão da faixa), os executores
 * vivos e as ações da issue (Ir ao orquestrador, Concluir, Voltar às abertas). Bloco `bg-card` com filete de 1 px, sem sombra nem acento na
 * borda (Flat-Bench Rule). Os executores são do orquestrador como um todo, não desta issue: o texto diz "executores agora".
 */
function OrchestratorPanel({ issue, start, executors, onFocusOrchestrator, onDone }: {
  issue: Issue; start: StartIssue; executors: readonly ExecutorNode[]; onFocusOrchestrator: () => void; onDone: () => void;
}) {
  const { active, draft } = useIssues();
  const headingId = useId();
  if (!active) return null;
  // O estado vem do rascunho do orquestrador (ver o cartão Em andamento da faixa).
  const drafted = draft.includes(active.number);
  const others = drafted ? draft.filter((n) => n !== active.number) : [];
  const busy = start?.busy ?? false;
  const alive = executors.filter((e) => !FINISHED.includes(e.state));
  return (
    <section aria-labelledby={headingId} className="mt-4 flex flex-col gap-3 rounded-[8px] border bg-card p-3 @[30rem]:p-4">
      <Text as="h2" id={headingId} fontSize={14} weight={600} style={{ margin: 0 }}>Com o orquestrador</Text>
      <Text as="div" role="status" fontSize={14} style={SECONDARY} className="flex flex-wrap items-center gap-x-2 gap-y-1">
        {!drafted && active.turned && busy && (
          <span aria-hidden className="inline-flex gap-[3px]">
            {[0, 200, 400].map((delay) => (
              <span key={delay} className="size-1 rounded-full motion-safe:animate-pulse" style={{ background: GH.amber, animationDelay: `${delay}ms` }} />
            ))}
          </span>
        )}
        <span>
          {drafted
            ? 'Colada no rascunho do orquestrador, aguardando você enviar'
            : !active.turned ? 'Não está no rascunho do orquestrador. Cole de novo para enviar.' : busy ? 'Orquestrador trabalhando' : 'Orquestrador livre'}
          {' · '}
          <span className="tabular-nums">{'no orquestrador '}<Ago iso={new Date(active.since).toISOString()} /></span>
        </span>
      </Text>
      {others.length > 0 && <Text as="div" fontSize={14} style={SECONDARY}>{`O rascunho também tem ${issueList(others)}.`}</Text>}
      <Text as="div" fontSize={14} style={SECONDARY}>
        {`Executores agora: ${executorsSummary(alive)}`}
      </Text>
      <div className="flex flex-wrap items-center gap-2">
        <Button className={FOCUS_RING} disabled={!start} title={start ? undefined : 'O orquestrador não está aberto.'} icon={<SquareTerminal className="size-4" />} onClick={onFocusOrchestrator}>
          Ir ao orquestrador
        </Button>
        <IssueActions issue={issue} onDone={onDone} />
      </div>
    </section>
  );
}

type Loaded = { number: number; items: IssueComment[] | null; error: string | null; loading: boolean };

/**
 * Comentários da issue, do mais antigo ao mais novo, carregados do GitHub. Recarrega quando a issue muda e quando a contagem dela muda
 * (alguém comentou); a resposta de um pedido que já não vale (outra issue, ou outra contagem no meio) é descartada. Com a lista já na tela,
 * o recarregamento não a troca por "Carregando".
 */
function Comments({ issue }: { issue: Issue }) {
  const [view, setView] = useState<Loaded>({ number: issue.number, items: null, error: null, loading: true });
  const [attempt, setAttempt] = useState(0);
  const headingId = useId();
  useEffect(() => {
    let current = true;
    // Os comentários da issue anterior não valem para esta.
    const keep = (v: Loaded) => (v.number === issue.number ? v.items : null);
    setView((v) => ({ number: issue.number, items: keep(v), error: null, loading: true }));
    backend.githubIssueComments(issue.number).then(
      (items) => current && setView({ number: issue.number, items, error: null, loading: false }),
      (e) => current && setView((v) => ({ number: issue.number, items: keep(v), error: message(e), loading: false })),
    );
    return () => {
      current = false;
    };
  }, [issue.number, issue.comments, attempt]);
  const mine = view.number === issue.number ? view : null;
  const items = mine?.items ?? null;
  return (
    <section aria-labelledby={headingId} className="mt-8 border-t pt-4">
      <Text as="h2" id={headingId} fontSize={14} weight={600} style={{ margin: 0 }}>
        Comentários
        <span className="ml-2 font-normal tabular-nums" style={SECONDARY}>{issue.comments}</span>
      </Text>
      {(!mine || (mine.loading && !items)) && (
        <Text as="p" role="status" fontSize={14} style={{ ...SECONDARY, margin: '12px 0 0' }}>Carregando comentários…</Text>
      )}
      {mine?.error && (
        <div className="mt-3 flex flex-col items-start gap-2">
          <GithubMessage message={mine.error} size={14} block />
          <Button size="small" className={FOCUS_RING} loading={mine.loading} onClick={() => setAttempt((n) => n + 1)}>Tentar de novo</Button>
        </div>
      )}
      {items && !items.length && <Text as="p" fontSize={14} style={{ ...SECONDARY, margin: '12px 0 0' }}>Nenhum comentário.</Text>}
      {items && items.length > 0 && (
        <ul className={`${MEASURE} mt-1 divide-y`}>
          {items.map((c) => (
            <li key={c.id} className="py-4">
              <div className="mb-1.5 flex flex-wrap items-baseline gap-x-2 text-[12px]">
                <span className="text-[14px] font-semibold">{c.author}</span>
                <span style={SECONDARY}><Ago iso={c.createdAt} /></span>
              </div>
              <Suspense fallback={null}>
                <div className={MARKDOWN_FOCUS}><IssueMarkdown fontSize={14}>{c.body}</IssueMarkdown></div>
              </Suspense>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
