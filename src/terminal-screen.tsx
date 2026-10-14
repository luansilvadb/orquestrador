// O painel do Orquestrador, à direita da tela principal, em todas as telas: o orquestrador e um executor por aba. Todos os terminais ficam montados; só o da aba ativa aparece.
// Redimensionável pela borda esquerda; a largura não é guardada entre aberturas do app.
import { useState, type ComponentType, type ReactNode } from 'react';
import { CircleCheck, CircleHelp, CircleX, Clock, Hourglass, LoaderCircle, SlidersHorizontal, Square } from 'lucide-react';
import { Button, colorsPreset, DraggablePanel, Tag, TabsIndicator, TabsList, TabsRoot, TabsTab, Text } from '@lobehub/ui/base-ui';
import { Center, Flexbox, type PresetColorKey } from '@lobehub/ui';
import ClaudeIcon from '@lobehub/icons/es/Claude';
import CodexIcon from '@lobehub/icons/es/Codex';
import { attachExecutor, TermHost, terminals } from './terminals';
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
// Largura ao abrir o app (não se guarda entre aberturas); a mínima deixa a barra da aba ativa legível em 320 px.
export const PANEL_WIDTH = 520;
export const PANEL_MIN = 320;
// Objeto constante: o duplo clique na alça volta para a largura inicial (o DraggablePanel usa `defaultSize` nesse reset).
const PANEL_DEFAULT_SIZE = { width: PANEL_WIDTH };
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

/** Escolha em partes separadas por "·": ferramenta em destaque, modelo e esforço apagados; parte vazia some. Encolhe com reticências. */
const ChoiceText = ({ choice }: { choice: Choice }) => {
  const parts = [choice.model, choice.effort].filter(Boolean);
  if (parts.length === 0) parts.push('padrão do CLI');
  const ToolIcon = TOOL_ICON[choice.tool];
  return (
    <Flexbox horizontal align="center" gap={6} allowShrink className="min-w-0" title={[choice.tool, ...parts].join(' · ')}>
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

type TerminalsProps = {
  orchestrator: { choice: Choice | null; started: boolean } | null;
  executors: ExecutorNode[];
  executorChoice: Choice | null;
  notice: string;
  // Largura exibida e largura máxima, calculadas pelo App (dependem da janela e da faixa da árvore). A máxima fica indefinida enquanto a janela não foi medida.
  width: number;
  maxWidth?: number;
  // Largura que o usuário deu, no fim do arrasto ou no duplo clique na alça.
  onWidthChange: (width: number) => void;
  onOpen: () => void;
  onExecutorChoice: () => void;
  onStop: (id: string) => void;
};

/** Painel à direita: abas em cima (orquestrador fixo, um executor por aba) e, embaixo, o terminal da aba ativa. */
export function TerminalsScreen({ orchestrator, executors, executorChoice, notice, width, maxWidth, onWidthChange, onOpen, onExecutorChoice, onStop }: TerminalsProps) {
  const [picked, setPicked] = useState<string>(ORCHESTRATOR);
  // Aba de executor que sumiu (executor que terminou, reset do orquestrador) volta para o orquestrador.
  const active = executors.some((e) => e.id === picked) ? picked : ORCHESTRATOR;
  const node = executors.find((e) => e.id === active);
  const meta = node && STATES[node.state];

  // Clicar na aba do orquestrador dá foco ao terminal dele, depois que a aba fica visível.
  const choose = (id: string) => {
    setPicked(id);
    if (id === ORCHESTRATOR) requestAnimationFrame(() => terminals.get(ORCHESTRATOR)?.term.focus());
  };

  // Sem padding nem cartão arredondado: o painel é a superfície inteira, separada do meio pela linha do DraggablePanel.
  return (
    <DraggablePanel
      placement="right" expandable={false} showHandleWideArea={false}
      size={{ width }} defaultSize={PANEL_DEFAULT_SIZE} onSizeChange={(_, size) => typeof size?.width === 'number' && onWidthChange(size.width)}
      minWidth={PANEL_MIN} maxWidth={maxWidth}
      backgroundColor="var(--ant-color-bg-container)"
    >
      <Flexbox height="100%" className="min-h-0">
        {/* Faixa de abas: uma linha de 36 px com a borda de baixo, no mesmo idioma do cabeçalho do painel Terminal. */}
        <Flexbox horizontal align="center" height={36} flex="none" paddingInline={4} className="border-b">
          <TabsRoot value={active} onValueChange={(key: string) => choose(key)} className="min-w-0 flex-1 gap-0">
            {/* A roda vertical rola a faixa na horizontal; o Shift e o trackpad já rolam na horizontal sozinhos. */}
            <TabsList
              className="h-8 w-full flex-nowrap justify-start overflow-x-auto overflow-y-hidden rounded-full [scrollbar-width:none]!"
              onWheel={(e) => { if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY; }}
            >
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
        </Flexbox>

        <Flexbox flex={1} className="relative min-h-0">
          {orchestrator && (
            <Flexbox horizontal align="center" gap={8} height={36} flex="none" paddingInline={12} className="whitespace-nowrap border-b">
              {node && meta ? (
                <>
                  {node.state !== 'running' && (
                    <Tag className="min-w-0 shrink" color={meta.color} icon={<meta.icon />} title={meta.label}>
                      <Text as="span" ellipsis>{meta.label}</Text>
                    </Tag>
                  )}
                  <Text as="span" code type="secondary" className="shrink-0">#{node.index + 1}</Text>
                  <Text ellipsis className="min-w-24 flex-1" title={node.title}>{node.title}</Text>
                  {node.choice && <ChoiceText choice={node.choice} />}
                  {CANCELLABLE.includes(node.state) && (
                    <Button type="default" size="small" className="shrink-0" title="Interrompe só este executor" icon={<Square className="size-3" />} onClick={() => onStop(node.id)}>
                      Parar
                    </Button>
                  )}
                </>
              ) : (
                <>
                  {orchestrator.choice && <ChoiceText choice={orchestrator.choice} />}
                  {orchestrator.started && (
                    <Button
                      type="default" ghost size="small" className="ml-auto min-w-0 shrink"
                      title="Escolha dos executores; a troca vale para os próximos"
                      aria-haspopup="dialog"
                      onClick={onExecutorChoice}
                    >
                      {/* Classes no Button perdem para o CSS sem camada do Lobe UI; por isso o peso e o leading ficam neste span */}
                      <Flexbox horizontal as="span" align="center" gap={6} allowShrink className="min-w-0 font-normal leading-normal">
                        <Text as="span" type="secondary" className="shrink-0">Executores</Text>
                        {executorChoice
                          ? <ChoiceText choice={executorChoice} />
                          : <Text as="span" type="secondary" ellipsis>ainda não escolhido</Text>}
                        <SlidersHorizontal className="size-3 shrink-0 text-muted-foreground" />
                      </Flexbox>
                    </Button>
                  )}
                </>
              )}
            </Flexbox>
          )}

          <div className="relative min-h-0 flex-1 overflow-hidden bg-terminal">
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
      </Flexbox>
    </DraggablePanel>
  );
}
