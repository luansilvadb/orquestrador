// Os terminais, ao lado da árvore de pastas na tela principal: o orquestrador e um executor por aba. Todos ficam montados; só o da aba ativa aparece.
import { useState, type ReactNode } from 'react';
import { CircleCheck, CircleHelp, CircleX, Clock, Hourglass, LoaderCircle, Square } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { cn } from 'cn';
import { attachExecutor, TermHost, terminals } from './terminals';
import { describeChoice, ORCHESTRATOR, type Choice, type ExecutorNode, type NodeState } from './types';

// Cada estado: rótulo e ícone com texto (regra do design system); a cor vem dos tokens --status-* de index.css.
export const STATES: Record<NodeState, { label: string; icon: typeof Clock; variant: 'outline' | 'secondary' | 'destructive' | 'default'; className?: string; tone: string }> = {
  'awaiting-choice': { label: 'aguardando escolha', icon: CircleHelp, variant: 'outline', tone: 'text-[var(--status-awaiting)]' },
  queued: { label: 'na fila', icon: Clock, variant: 'outline', tone: 'text-[var(--status-queued)]' },
  running: { label: 'executando', icon: LoaderCircle, variant: 'default', tone: 'text-[var(--status-running)]' },
  'awaiting-orchestrator': {
    label: 'aguardando orquestrador', icon: Hourglass, variant: 'outline',
    className: 'border-[var(--status-prompt)] text-[var(--status-prompt)]', tone: 'text-[var(--status-prompt)]',
  },
  done: { label: 'concluído', icon: CircleCheck, variant: 'secondary', tone: 'text-[var(--status-done)]' },
  stopped: { label: 'interrompido', icon: Square, variant: 'outline', tone: 'text-[var(--status-stopped)]' },
  failed: { label: 'falhou', icon: CircleX, variant: 'destructive', tone: 'text-[var(--status-failed)]' },
};
export const FINISHED: NodeState[] = ['done', 'stopped', 'failed'];
const CANCELLABLE: NodeState[] = ['queued', 'running', 'awaiting-orchestrator'];
// Texto da camada que cobre o terminal do executor, por estado; os outros estados mostram o terminal direto.
const COVER_TEXT: Partial<Record<NodeState, string>> = {
  'awaiting-choice': 'Aguardando a escolha dos executores.',
  queued: 'Na fila: começa quando um executor terminar ou o teto de simultâneos subir.',
};

/** Camada que cobre o terminal do executor com um aviso, sem sair da aba. */
const Cover = ({ children }: { children: ReactNode }) => (
  <div className="absolute inset-0 overflow-auto bg-background/95 p-4">
    <div className="flex min-h-full items-center justify-center">{children}</div>
  </div>
);

type TerminalsProps = {
  orchestrator: { title: string; started: boolean } | null;
  executors: ExecutorNode[];
  executorChoice: Choice | null;
  notice: string;
  onOpen: () => void;
  onExecutorChoice: () => void;
  onStop: (id: string) => void;
};

/** Abas em cima (orquestrador fixo, um executor por aba) e, embaixo, o terminal da aba ativa. */
export function TerminalsScreen({ orchestrator, executors, executorChoice, notice, onOpen, onExecutorChoice, onStop }: TerminalsProps) {
  const [picked, setPicked] = useState<string>(ORCHESTRATOR);
  // Aba de executor que sumiu (executor que terminou, reset do orquestrador) volta para a do orquestrador.
  const active = executors.some((e) => e.id === picked) ? picked : ORCHESTRATOR;
  const node = executors.find((e) => e.id === active);
  const meta = node && STATES[node.state];

  // Clicar na aba do orquestrador dá foco ao terminal dele, depois que a aba fica visível.
  const choose = (id: string) => {
    setPicked(id);
    if (id === ORCHESTRATOR) requestAnimationFrame(() => terminals.get(ORCHESTRATOR)?.term.focus());
  };

  return (
    <div className="flex h-full min-h-0 w-full flex-col gap-2 p-4">
      <Tabs value={active} onValueChange={choose} className="min-w-0 gap-0">
        <TabsList className="h-9 w-full justify-start overflow-x-auto overflow-y-hidden rounded-full [scrollbar-width:none]">
          <TabsTrigger value={ORCHESTRATOR} className="flex-none rounded-full px-3">Orquestrador</TabsTrigger>
          {orchestrator && executors.map((e) => {
            const info = STATES[e.state];
            const Icon = info.icon;
            const label = `Executor #${e.index + 1}, ${info.label}: ${e.title}`;
            return (
              <TabsTrigger key={e.id} value={e.id} title={label} aria-label={label} className="flex-none rounded-full px-3">
                <Icon className={cn(info.tone, e.state === 'running' && 'animate-spin')} />
                <span className="font-mono">#{e.index + 1}</span>
              </TabsTrigger>
            );
          })}
        </TabsList>
      </Tabs>

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border bg-card">
        {orchestrator && (
          <div className="flex h-9 shrink-0 items-center gap-2 border-b px-3 text-xs">
            {node && meta ? (
              <>
                <Badge variant={meta.variant} className={cn('gap-1', meta.className)}>
                  <meta.icon className={node.state === 'running' ? 'animate-spin' : undefined} />
                  {meta.label}
                </Badge>
                <span className="font-mono text-muted-foreground">#{node.index + 1}</span>
                <span className="min-w-0 flex-1 truncate" title={node.title}>{node.title}</span>
                {node.choice && <span className="truncate font-mono text-muted-foreground">{describeChoice(node.choice)}</span>}
                {CANCELLABLE.includes(node.state) && (
                  <Button variant="outline" size="xs" title="Interrompe só este executor" onClick={() => onStop(node.id)}>
                    parar
                  </Button>
                )}
              </>
            ) : (
              <>
                <Badge variant="secondary">orquestrador</Badge>
                <span className="min-w-0 truncate font-mono text-muted-foreground">{orchestrator.title}</span>
                <span className="flex-1" />
                {orchestrator.started && (
                  <Button
                    variant="outline" size="xs" className="font-mono"
                    title="Escolha dos executores; a troca vale para os próximos"
                    onClick={onExecutorChoice}
                  >
                    executores: {executorChoice ? describeChoice(executorChoice) : 'ainda não escolhido'}
                  </Button>
                )}
              </>
            )}
          </div>
        )}

        <div className={cn('relative mx-1.5 mb-1.5 min-h-0 flex-1 overflow-hidden rounded-lg bg-terminal', !orchestrator && 'mt-1.5')}>
          {orchestrator && (
            <div className={cn('absolute inset-0', active !== ORCHESTRATOR && 'invisible')}>
              <TermHost id={ORCHESTRATOR} writable />
            </div>
          )}
          {orchestrator && executors.map((e) => (
            <div
              key={e.id}
              className={cn(
                'absolute inset-0',
                active !== e.id && 'invisible',
                FINISHED.includes(e.state) && '[&_.term]:opacity-60',
              )}
            >
              <TermHost id={e.id} writable={false} onReady={attachExecutor} />
              {COVER_TEXT[e.state] && (
                <Cover>
                  <p className="max-w-[300px] text-center text-sm text-muted-foreground">{COVER_TEXT[e.state]}</p>
                </Cover>
              )}
            </div>
          ))}

          {!orchestrator && (
            <div className="absolute inset-0 m-auto grid h-fit w-[min(380px,calc(100%-32px))] justify-items-center gap-3 p-4 text-center">
              {notice && <p role="status" className="rounded-md border bg-card px-3 py-1.5 text-sm">{notice}</p>}
              <Button onClick={onOpen}>Abrir terminal</Button>
              <p className="text-xs text-muted-foreground">
                Abre o orquestrador: o único terminal onde você digita. Os executores aparecem em abas quando ele delega.
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
