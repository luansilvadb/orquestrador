// Os terminais, ao lado da árvore de pastas na tela principal: o orquestrador e um executor por aba. Todos ficam montados; só o da aba ativa aparece.
import { useState, type ComponentType, type CSSProperties, type ReactNode } from 'react';
import { Bot, CircleCheck, CircleHelp, CircleX, Clock, Hourglass, LoaderCircle, SlidersHorizontal, Square } from 'lucide-react';
import { Button, Tag, TabsIndicator, TabsList, TabsRoot, TabsTab } from '@lobehub/ui/base-ui';
import ClaudeIcon from '@lobehub/icons/es/Claude';
import CodexIcon from '@lobehub/icons/es/Codex';
import { cn } from 'cn';
import { attachExecutor, TermHost, terminals } from './terminals';
import { ORCHESTRATOR, type Choice, type ExecutorNode, type NodeState, type Tool } from './types';

// Cada estado: rótulo e ícone com texto (regra do design system); a cor vem dos tokens --status-* de index.css.
export const STATES: Record<NodeState, { label: string; icon: typeof Clock; color: string; tone: string }> = {
  'awaiting-choice': { label: 'aguardando escolha', icon: CircleHelp, color: 'var(--status-awaiting)', tone: 'text-[var(--status-awaiting)]' },
  queued: { label: 'na fila', icon: Clock, color: 'var(--status-queued)', tone: 'text-[var(--status-queued)]' },
  running: { label: 'executando', icon: LoaderCircle, color: 'var(--status-running)', tone: 'text-[var(--status-running)]' },
  'awaiting-orchestrator': { label: 'aguardando orquestrador', icon: Hourglass, color: 'var(--status-prompt)', tone: 'text-[var(--status-prompt)]' },
  done: { label: 'concluído', icon: CircleCheck, color: 'var(--status-done)', tone: 'text-[var(--status-done)]' },
  stopped: { label: 'interrompido', icon: Square, color: 'var(--status-stopped)', tone: 'text-[var(--status-stopped)]' },
  failed: { label: 'falhou', icon: CircleX, color: 'var(--status-failed)', tone: 'text-[var(--status-failed)]' },
};
// O Tag do Lobe UI só pinta cor hex ou predefinida; o token entra pelo style (fundo e borda são a mesma cor, mais fraca).
const tagStyle = (color: string): CSSProperties => ({
  color,
  background: `color-mix(in oklab, ${color} 14%, transparent)`,
  borderColor: `color-mix(in oklab, ${color} 45%, transparent)`,
});
export const FINISHED: NodeState[] = ['done', 'stopped', 'failed'];
const CANCELLABLE: NodeState[] = ['queued', 'running', 'awaiting-orchestrator'];
// Texto da camada que cobre o terminal do executor, por estado; os outros estados mostram o terminal direto.
const COVER_TEXT: Partial<Record<NodeState, string>> = {
  'awaiting-choice': 'Aguardando a escolha dos executores.',
  queued: 'Na fila: começa quando um executor terminar ou o teto de simultâneos subir.',
};

// Ícones monocromáticos (herdam currentColor); o import direto por arquivo não puxa o pacote inteiro.
const TOOL_ICON: Record<Tool, ComponentType<{ size?: number; className?: string }>> = {
  claude: ClaudeIcon,
  codex: CodexIcon,
};

/** Escolha em partes separadas por "·": ferramenta em destaque, modelo e esforço apagados; parte vazia some. */
const ChoiceText = ({ choice, className }: { choice: Choice; className?: string }) => {
  const parts = [choice.model, choice.effort].filter(Boolean);
  if (parts.length === 0) parts.push('padrão do CLI');
  const ToolIcon = TOOL_ICON[choice.tool];
  return (
    <span className={cn('flex min-w-0 items-center gap-1.5 font-mono', className)} title={[choice.tool, ...parts].join(' · ')}>
      <ToolIcon size={14} className="shrink-0 text-foreground" />
      <span className="min-w-0 truncate">
        <span className="text-foreground">{choice.tool}</span>
        {parts.map((part, i) => (
          <span key={i}>
            <span className="px-1.5 text-muted-foreground">·</span>
            <span className="text-muted-foreground">{part}</span>
          </span>
        ))}
      </span>
    </span>
  );
};

/** Camada que cobre o terminal do executor com um aviso, sem sair da aba. */
const Cover = ({ children }: { children: ReactNode }) => (
  <div className="absolute inset-0 overflow-auto bg-background/95 p-4">
    <div className="flex min-h-full items-center justify-center">{children}</div>
  </div>
);

type TerminalsProps = {
  orchestrator: { choice: Choice | null; started: boolean } | null;
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
      {/* Só a faixa de abas: os terminais são renderizados logo abaixo, por este arquivo. */}
      <TabsRoot value={active} onValueChange={(key: string) => choose(key)} className="min-w-0 gap-0">
        <TabsList className="h-9 w-full flex-nowrap justify-start overflow-x-auto overflow-y-hidden rounded-full [scrollbar-width:none]">
          <TabsIndicator />
          <TabsTab value={ORCHESTRATOR} className="flex-none px-3">Orquestrador</TabsTab>
          {orchestrator && executors.map((e) => {
            const info = STATES[e.state];
            const Icon = info.icon;
            const label = `Executor #${e.index + 1}, ${info.label}: ${e.title}`;
            return (
              <TabsTab key={e.id} value={e.id} title={label} aria-label={label} className="flex-none gap-1.5 px-3">
                <Icon className={cn(info.tone, e.state === 'running' && 'animate-spin')} />
                <span className="font-mono">#{e.index + 1}</span>
              </TabsTab>
            );
          })}
        </TabsList>
      </TabsRoot>

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border bg-card">
        {orchestrator && (
          <div className="flex h-9 shrink-0 items-center gap-2 whitespace-nowrap border-b px-3 text-xs">
            {node && meta ? (
              <>
                <Tag className="shrink-0" icon={<meta.icon className={node.state === 'running' ? 'animate-spin' : undefined} />} style={tagStyle(meta.color)}>
                  {meta.label}
                </Tag>
                <span className="shrink-0 font-mono text-muted-foreground">#{node.index + 1}</span>
                <span className="min-w-0 flex-1 truncate" title={node.title}>{node.title}</span>
                {node.choice && <ChoiceText choice={node.choice} className="shrink-0" />}
                {CANCELLABLE.includes(node.state) && (
                  <Button type="default" size="small" className="shrink-0" title="Interrompe só este executor" icon={<Square className="size-3" />} onClick={() => onStop(node.id)}>
                    Parar
                  </Button>
                )}
              </>
            ) : (
              <>
                <div className="flex min-w-0 items-center gap-2">
                  <span className="flex shrink-0 items-center gap-1.5 font-medium text-foreground">
                    <Bot className="size-3.5 text-muted-foreground" />
                    Orquestrador
                  </span>
                  {orchestrator.choice && <ChoiceText choice={orchestrator.choice} />}
                </div>
                <span className="flex-1" />
                {orchestrator.started && (
                  <Button
                    type="default" ghost size="small" className="shrink-0"
                    title="Escolha dos executores; a troca vale para os próximos"
                    aria-haspopup="dialog"
                    onClick={onExecutorChoice}
                  >
                    {/* Classes no Button perdem para o CSS sem camada do Lobe UI; por isso o peso e o leading ficam neste span */}
                    <span className="flex items-center gap-1.5 font-normal leading-normal">
                      <span className="text-muted-foreground">Executores</span>
                      {executorChoice
                        ? <ChoiceText choice={executorChoice} />
                        : <span className="text-muted-foreground">ainda não escolhido</span>}
                      <SlidersHorizontal className="size-3 text-muted-foreground" />
                    </span>
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
              <Button type="primary" onClick={onOpen}>Abrir terminal</Button>
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
