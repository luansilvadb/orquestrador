// Os terminais, ao lado da árvore de pastas na tela principal: o orquestrador e um executor por aba. Todos ficam montados; só o da aba ativa aparece.
import { useEffect, useRef, useState, type ComponentType, type ReactNode } from 'react';
import { Bot, CircleCheck, CircleHelp, CircleX, Clock, Hourglass, LoaderCircle, SlidersHorizontal, Square } from 'lucide-react';
import { Button, colorsPreset, Tag, TabsIndicator, TabsList, TabsRoot, TabsTab, Text } from '@lobehub/ui/base-ui';
import { Center, Flexbox, type PresetColorKey } from '@lobehub/ui';
import ClaudeIcon from '@lobehub/icons/es/Claude';
import CodexIcon from '@lobehub/icons/es/Codex';
import type { TermView } from './term';
import { attachExecutor, TermHost, terminals } from './terminals';
import { ShellPanel } from './shell-panel';
import { ORCHESTRATOR, type Choice, type ExecutorNode, type NodeState, type Tool } from './types';

// Cada estado: rótulo e ícone com texto (regra do design system); a cor é um nome predefinido do Lobe UI, o mesmo para o Tag e para o ícone da aba.
export const STATES: Record<NodeState, { label: string; icon: typeof Clock; color: PresetColorKey }> = {
  'awaiting-choice': { label: 'aguardando escolha', icon: CircleHelp, color: 'gold' },
  queued: { label: 'na fila', icon: Clock, color: 'gray' },
  running: { label: 'executando', icon: LoaderCircle, color: 'blue' },
  'awaiting-orchestrator': { label: 'aguardando orquestrador', icon: Hourglass, color: 'purple' },
  done: { label: 'concluído', icon: CircleCheck, color: 'green' },
  stopped: { label: 'interrompido', icon: Square, color: 'orange' },
  failed: { label: 'falhou', icon: CircleX, color: 'red' },
};
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
const ChoiceText = ({ choice, flex }: { choice: Choice; flex?: 'none' }) => {
  const parts = [choice.model, choice.effort].filter(Boolean);
  if (parts.length === 0) parts.push('padrão do CLI');
  const ToolIcon = TOOL_ICON[choice.tool];
  return (
    <Flexbox horizontal align="center" gap={6} allowShrink flex={flex} title={[choice.tool, ...parts].join(' · ')}>
      <ToolIcon size={14} className="shrink-0 text-foreground" />
      <Text ellipsis code className="min-w-0">
        <span>{choice.tool}</span>
        {parts.map((part, i) => (
          <span key={i}>
            <Text as="span" type="secondary" className="px-1.5">·</Text>
            <Text as="span" type="secondary">{part}</Text>
          </span>
        ))}
      </Text>
    </Flexbox>
  );
};

/** Camada que cobre o terminal do executor com um aviso, sem sair da aba. */
const Cover = ({ children }: { children: ReactNode }) => (
  <Flexbox className="absolute inset-0 overflow-auto bg-background/95" padding={16}>
    <Center className="min-h-full">{children}</Center>
  </Flexbox>
);

// Quanto da coluna fica fora do painel Terminal: padding de 16 px em cima e embaixo, a faixa de abas (36 px), os dois
// gaps de 8 px e a margem de 4 px do painel, além de um mínimo de 160 px para o cartão de cima nunca sumir.
const RESERVED_HEIGHT = 16 * 2 + 36 + 8 * 2 + 4 + 160;

type TerminalsProps = {
  orchestrator: { choice: Choice | null; started: boolean } | null;
  executors: ExecutorNode[];
  executorChoice: Choice | null;
  notice: string;
  shellOpen: boolean;
  shellMounted: boolean;
  onOpen: () => void;
  onExecutorChoice: () => void;
  onStop: (id: string) => void;
  onShellClose: () => void;
  onShellReady: (view: TermView) => void;
};

/** Abas em cima (orquestrador fixo, um executor por aba) e, embaixo, o terminal da aba ativa. O painel Terminal fica por último. */
export function TerminalsScreen({
  orchestrator, executors, executorChoice, notice, shellOpen, shellMounted, onOpen, onExecutorChoice, onStop, onShellClose, onShellReady,
}: TerminalsProps) {
  const [picked, setPicked] = useState<string>(ORCHESTRATOR);
  // Aba de executor que sumiu (executor que terminou, reset do orquestrador) volta para a do orquestrador.
  const active = executors.some((e) => e.id === picked) ? picked : ORCHESTRATOR;
  const node = executors.find((e) => e.id === active);
  const meta = node && STATES[node.state];
  // Altura da coluna: o painel Terminal pode crescer até deixar o cartão de cima no mínimo.
  const column = useRef<HTMLDivElement>(null);
  const [columnHeight, setColumnHeight] = useState(0);
  useEffect(() => {
    const observer = new ResizeObserver(() => setColumnHeight(column.current?.clientHeight ?? 0));
    observer.observe(column.current!);
    return () => observer.disconnect();
  }, []);

  // Clicar na aba do orquestrador dá foco ao terminal dele, depois que a aba fica visível.
  const choose = (id: string) => {
    setPicked(id);
    if (id === ORCHESTRATOR) requestAnimationFrame(() => terminals.get(ORCHESTRATOR)?.term.focus());
  };

  return (
    <Flexbox ref={column} className="min-h-0" width="100%" height="100%" gap={8} padding={16}>
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
                <Icon className={e.state === 'running' ? 'animate-spin' : undefined} style={{ color: colorsPreset(info.color) }} />
                <Text as="span" code>#{e.index + 1}</Text>
              </TabsTab>
            );
          })}
        </TabsList>
      </TabsRoot>

      <Flexbox className="min-h-0 overflow-hidden rounded-xl border bg-card" flex={1}>
        {orchestrator && (
          <Flexbox horizontal align="center" gap={8} height={36} flex="none" paddingInline={12} className="whitespace-nowrap border-b">
            {node && meta ? (
              <>
                {node.state !== 'running' && (
                  <Tag className="shrink-0" color={meta.color} icon={<meta.icon />}>
                    {meta.label}
                  </Tag>
                )}
                <Text as="span" code type="secondary" className="shrink-0">#{node.index + 1}</Text>
                <Text ellipsis className="min-w-0 flex-1" title={node.title}>{node.title}</Text>
                {node.choice && <ChoiceText choice={node.choice} flex="none" />}
                {CANCELLABLE.includes(node.state) && (
                  <Button type="default" size="small" className="shrink-0" title="Interrompe só este executor" icon={<Square className="size-3" />} onClick={() => onStop(node.id)}>
                    Parar
                  </Button>
                )}
              </>
            ) : (
              <>
                <Flexbox horizontal align="center" gap={8} allowShrink>
                  <Flexbox horizontal align="center" gap={6} flex="none">
                    <Bot className="size-3.5 text-muted-foreground" />
                    <Text as="span" weight={500}>Orquestrador</Text>
                  </Flexbox>
                  {orchestrator.choice && <ChoiceText choice={orchestrator.choice} />}
                </Flexbox>
                <Flexbox flex={1} />
                {orchestrator.started && (
                  <Button
                    type="default" ghost size="small" className="shrink-0"
                    title="Escolha dos executores; a troca vale para os próximos"
                    aria-haspopup="dialog"
                    onClick={onExecutorChoice}
                  >
                    {/* Classes no Button perdem para o CSS sem camada do Lobe UI; por isso o peso e o leading ficam neste span */}
                    <Flexbox horizontal as="span" align="center" gap={6} className="font-normal leading-normal">
                      <Text as="span" type="secondary">Executores</Text>
                      {executorChoice
                        ? <ChoiceText choice={executorChoice} />
                        : <Text as="span" type="secondary">ainda não escolhido</Text>}
                      <SlidersHorizontal className="size-3 text-muted-foreground" />
                    </Flexbox>
                  </Button>
                )}
              </>
            )}
          </Flexbox>
        )}

        <div className={`relative mx-1.5 mb-1.5 min-h-0 flex-1 overflow-hidden rounded-lg bg-terminal${orchestrator ? '' : ' mt-1.5'}`}>
          {orchestrator && (
            <div className={`absolute inset-0${active !== ORCHESTRATOR ? ' invisible' : ''}`}>
              <TermHost id={ORCHESTRATOR} writable />
            </div>
          )}
          {orchestrator && executors.map((e) => (
            <div
              key={e.id}
              className={`absolute inset-0${active !== e.id ? ' invisible' : ''}${FINISHED.includes(e.state) ? ' [&_.term]:opacity-60' : ''}`}
            >
              <TermHost id={e.id} writable={false} onReady={attachExecutor} />
              {COVER_TEXT[e.state] && (
                <Cover>
                  <Text as="p" type="secondary" fontSize={14} align="center" className="max-w-[300px]">{COVER_TEXT[e.state]}</Text>
                </Cover>
              )}
            </div>
          ))}

          {!orchestrator && (
            <Flexbox align="center" gap={12} padding={16} width="min(380px, calc(100% - 32px))" height="fit-content" className="absolute inset-0 m-auto">
              {notice && <Text as="p" role="status" fontSize={14} className="rounded-md border bg-card px-3 py-1.5">{notice}</Text>}
              <Button type="primary" onClick={onOpen}>Abrir terminal</Button>
              <Text as="p" type="secondary" fontSize={12} align="center">
                Abre o orquestrador, com quem você conversa. Os executores aparecem em abas quando ele delega.
              </Text>
            </Flexbox>
          )}
        </div>
      </Flexbox>

      <ShellPanel
        open={shellOpen} mounted={shellMounted} maxHeight={columnHeight - RESERVED_HEIGHT}
        onClose={onShellClose} onReady={onShellReady}
      />
    </Flexbox>
  );
}
