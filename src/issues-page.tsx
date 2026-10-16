// Detalhe da issue na aba Issues da área central: a issue escolhida na faixa (ou, sem escolha, a em andamento, a próxima ou a primeira aberta;
// ver `detailIssue`). Cabeçalho com o título em toda a largura, o estado em `Tag`, "criada há X", as labels, Editar e Excluir, e, numa
// linha abaixo, a barra de ações (Colar no orquestrador como ação principal, Reabrir na fechada); na issue em andamento, o painel "Com o
// orquestrador" (o que ele está fazendo, os executores agora e as ações Ir ao orquestrador, Concluir e Voltar às abertas); na próxima, uma
// linha com o Não colar depois; depois, a descrição em Markdown. Sem conseguir ler o arquivo das issues, o motivo e o Tentar de novo.
// A faixa é a fila e o único lugar de lista, busca e filtro: esta tela não tem nada disso.
// O estado vem do store único do App (`useIssues`); os executores e as ações de terminal e do formulário vêm por props.
import { Suspense, useEffect, useId, useRef, useState } from 'react';
import { Clock, Pencil, SquareTerminal, Trash2 } from 'lucide-react';
import { ActionIcon, Button, Text, toast } from '@lobehub/ui/base-ui';
import { Center } from '@lobehub/ui';
import { message } from './backend';
import { issueList } from './issue-text';
import {
  Ago, FOCUS_RING, GH, IssueActions, IssueMarkdown, LabelPill, MARKDOWN_FOCUS, NextMark, SECONDARY, StartButton, StateMark, ActiveMark, useConfirm,
  type StartIssue,
} from './issue-ui';
import { useDetailIssue, useIssues } from './issues-store';
import { FINISHED, STATES } from './terminal-screen';
import type { ExecutorNode, Issue, NodeState } from './types';

const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });
// Medida de leitura da descrição.
const MEASURE = 'max-w-[72ch]';

export function IssuesPage({ start, onNew, onEdit, focusKey, executors, onFocusOrchestrator }: {
  start: StartIssue;
  /** Abre a aba Nova issue (o botão do estado vazio). */
  onNew: () => void;
  /** Abre a issue no formulário, para editar. */
  onEdit: (issue: Issue) => void;
  /** Sobe a cada pedido de foco no detalhe (duplo clique ou Enter com a linha já selecionada na faixa): a rolagem da tela recebe o foco. */
  focusKey: number;
  /** Executores vivos do App, resumidos no painel "Com o orquestrador". */
  executors: readonly ExecutorNode[];
  /** Leva o foco ao terminal do orquestrador. */
  onFocusOrchestrator: () => void;
}) {
  const store = useIssues();
  const issue = useDetailIssue();
  const root = useRef<HTMLDivElement>(null);
  const [retrying, setRetrying] = useState(false);
  useEffect(() => {
    if (focusKey === 0) return;
    const frame = requestAnimationFrame(() => root.current?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(frame);
  }, [focusKey]);
  // Concluir, Voltar às abertas, Reabrir, Excluir e Não colar depois tiram o próprio botão da tela: o foco vem para o detalhe.
  const focusDetail = () => root.current?.focus({ preventScroll: true });
  const retry = () => {
    setRetrying(true);
    void store.refresh().finally(() => setRetrying(false));
  };

  return (
    <div ref={root} tabIndex={-1} className="@container h-full min-h-0 overflow-x-hidden overflow-y-auto outline-none">
      <div className="mx-auto w-full max-w-[928px] min-w-0 p-3 @[30rem]:p-6">
        {store.loaded ? (
          issue ? (
            <IssueDetail issue={issue} start={start} executors={executors} onEdit={onEdit} onFocusOrchestrator={onFocusOrchestrator} onDone={focusDetail} />
          ) : (
            <Center gap={12} padding={32}>
              <Text as="p" fontSize={14} style={{ ...SECONDARY, margin: 0 }}>Nenhuma issue aberta.</Text>
              <Button className={FOCUS_RING} onClick={onNew}>Nova issue</Button>
            </Center>
          )
        ) : store.error && (
          // O arquivo das issues não abriu (ilegível, de uma versão mais nova, disco recusando): o motivo inteiro só aqui.
          <Center gap={12} padding={32}>
            <Text as="h1" fontSize={20} weight={600} lineHeight="28px" style={{ margin: 0 }}>Não foi possível abrir as issues</Text>
            <Text as="p" role="status" fontSize={14} lineHeight={1.5} style={{ ...SECONDARY, margin: 0, overflowWrap: 'anywhere', textAlign: 'center' }}>
              {store.error}
            </Text>
            <Button type="primary" size="small" className={FOCUS_RING} loading={retrying} disabled={retrying} onClick={retry}>Tentar de novo</Button>
          </Center>
        )}
      </div>
    </div>
  );
}

/**
 * Cabeçalho (título, estado, quando foi criada, as labels, Editar, Excluir e as ações) e a descrição de uma issue. Os tempos ("há X")
 * vêm do relógio das Issues pelo `Ago`: só eles se atualizam, sem renderizar de novo a descrição. `onDone` leva o foco ao detalhe depois de
 * uma ação que some com o próprio botão. Excluir pergunta antes; com o texto da issue no rascunho do orquestrador, a pergunta lembra dele.
 */
function IssueDetail({ issue, start, executors, onEdit, onFocusOrchestrator, onDone }: {
  issue: Issue; start: StartIssue; executors: readonly ExecutorNode[]; onEdit: (issue: Issue) => void; onFocusOrchestrator: () => void; onDone: () => void;
}) {
  const store = useIssues();
  const { confirm, dialog, afterClose } = useConfirm();
  const isOpen = issue.state === 'open';
  const isActive = isOpen && store.active?.number === issue.number;
  const isNext = isOpen && store.next === issue.number;
  const body = issue.body.trim();
  const hasBar = isOpen ? !(isActive && store.draft.includes(issue.number)) : true;
  const remove = async () => {
    const { number } = issue;
    const drafted = store.draft.includes(number);
    if (!await confirm({
      title: `Excluir a #${number}?`,
      body: `A #${number} será apagada deste computador, com o título e a descrição. Não dá para desfazer.${drafted ? ' O texto dela continua no rascunho do orquestrador: apague-o lá.' : ''}`,
      confirmLabel: 'Excluir',
    })) return;
    afterClose(onDone);
    await store.remove(number).then(() => toast.success({ title: `#${number} excluída`, duration: 3000 }), report);
  };
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
              {'criada '}<Ago iso={issue.createdAt} />
              {!isOpen && <>{' · fechada '}<Ago iso={issue.closedAt ?? issue.updatedAt} /></>}
            </span>
            {issue.labels.map((name) => <LabelPill key={name} name={name} />)}
            <span className="inline-flex items-center gap-0.5">
              <ActionIcon size="small" icon={Pencil} className={FOCUS_RING} aria-label={`Editar a issue #${issue.number}`} title="Editar" onClick={() => onEdit(issue)} />
              <ActionIcon size="small" icon={Trash2} className={FOCUS_RING} aria-label={`Excluir a issue #${issue.number}`} title="Excluir" onClick={() => void remove()} />
            </span>
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
      {dialog}
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
