// O painel do Orquestrador, à direita da tela principal, em todas as telas: o orquestrador e um executor por aba. Todos os terminais ficam montados; só o da aba ativa aparece.
// Redimensionável pela borda esquerda. Abre em 572 px; a largura que o usuário dá fica guardada por projeto, no layout que o App leva na recuperação do editor.
import { useLayoutEffect, useRef, useState, type ComponentType, type ReactNode } from 'react';
import { Check, CircleCheck, CircleHelp, CircleX, Clock, List, LoaderCircle, SlidersHorizontal, Square, SquareTerminal } from 'lucide-react';
import { Button, colorsPreset, DraggablePanel, DropdownMenu, Tag, TabsIndicator, TabsList, TabsRoot, TabsTab, Text } from '@lobehub/ui/base-ui';
import { Center, Flexbox, type PresetColorKey } from '@lobehub/ui';
import ClaudeIcon from '@lobehub/icons/es/Claude';
import CodexIcon from '@lobehub/icons/es/Codex';
import { SECONDARY } from '../window/text-style';
import { OpenProjectPrompt } from '../shared/open-project';
import { TermHost, terminals } from './terminals';
import { revealTab, wheelToScroll } from '../shared/tab-strip';
import { ORCHESTRATOR, TOOL_NAME, type Choice, type ExecutorNode, type NodeState, type Tool } from '../backend/types';

// Cada estado: rótulo e ícone com texto (regra do design system); a cor é um nome predefinido do Lobe UI, o mesmo para o Tag e para o ícone da aba.
export const STATES: Record<NodeState, { label: string; icon: typeof Clock; color: PresetColorKey }> = {
  'awaiting-choice': { label: 'aguardando escolha', icon: CircleHelp, color: 'gold' },
  queued: { label: 'na fila', icon: Clock, color: 'gray' },
  running: { label: 'executando', icon: LoaderCircle, color: 'blue' },
  done: { label: 'concluído', icon: CircleCheck, color: 'green' },
  stopped: { label: 'interrompido', icon: Square, color: 'orange' },
  failed: { label: 'falhou', icon: CircleX, color: 'red' },
};
export const FINISHED: NodeState[] = ['done', 'stopped', 'failed'];
const CANCELLABLE: NodeState[] = ['queued', 'running'];
// Largura inicial, sem layout guardado do projeto; a mínima deixa a barra da aba ativa legível em 320 px.
export const PANEL_WIDTH = 572;
export const PANEL_MIN = 320;
// Objeto constante: o duplo clique na alça volta para a largura inicial (o DraggablePanel usa `defaultSize` nesse reset).
const PANEL_DEFAULT_SIZE = { width: PANEL_WIDTH };
// Texto da camada que cobre o terminal do executor, por estado; os outros estados mostram o terminal direto.
const COVER_TEXT: Partial<Record<NodeState, string>> = {
  'awaiting-choice': 'Aguardando a escolha dos executores.',
  queued: 'Na fila: começa automaticamente quando houver capacidade, sem ajuste manual.',
};

// Ícones monocromáticos (herdam currentColor); o import direto por arquivo não puxa o pacote inteiro.
const TOOL_ICON: Record<Tool, ComponentType<{ size?: number; className?: string }>> = {
  claude: ClaudeIcon,
  codex: CodexIcon,
};

// Acha a aba pelo `data-tab` e a revela na faixa (a regra de rolagem está em tab-strip).
const reveal = (box: HTMLElement, id: string) => revealTab(box, [...box.querySelectorAll<HTMLElement>('[data-tab]')].find((el) => el.dataset.tab === id));

/** Texto completo da escolha, para o tooltip e o aria-label. */
const choiceLabel = (choice: Choice) => [TOOL_NAME[choice.tool], choice.model || 'padrão do CLI', choice.effort].filter(Boolean).join(' · ');

/** Ferramenta em texto normal; modelo em monoespaçado, que corta com reticências; esforço como detalhe, em texto secundário com espaçamento. Encolhe sem empurrar os vizinhos. Com `compact`, modelo e esforço somem abaixo de 28 rem do contêiner da barra (o tooltip mantém o texto completo). */
const ChoiceText = ({ choice, compact = false }: { choice: Choice; compact?: boolean }) => {
  const detail = compact ? ' hidden @[28rem]:inline' : '';
  return (
    <Flexbox horizontal align="center" gap={6} allowShrink className={compact ? 'min-w-0 shrink-0 @[28rem]:shrink' : 'min-w-0'} title={choiceLabel(choice)}>
      <Text as="span" weight={500} className="shrink-0">{TOOL_NAME[choice.tool]}</Text>
      {choice.model && <Text as="span" code type="secondary" ellipsis className={`min-w-0${detail}`}>{choice.model}</Text>}
      {!choice.model && !choice.effort && <Text as="span" type="secondary" ellipsis className={`min-w-0${detail}`}>padrão do CLI</Text>}
      {choice.effort && <Text as="span" code type="secondary" className={`shrink-0${detail}`}>{choice.effort}</Text>}
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
  projectId: string | null;
  projectOpen: boolean;
  onOpenProject: () => void;
  orchestrator: { choice: Choice | null } | null;
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
export function TerminalsScreen({ projectId, projectOpen, onOpenProject, orchestrator, executors, executorChoice, notice, width, maxWidth, onWidthChange, onOpen, onExecutorChoice, onStop }: TerminalsProps) {
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

  // A aba ativa é revelada ao escolher, ao chegar executor (lista de ids muda) e ao redimensionar o painel (a faixa muda de largura).
  const strip = useRef<HTMLDivElement>(null);
  const activeRef = useRef(active);
  const tabIds = executors.map((e) => e.id).join('\n');
  useLayoutEffect(() => {
    activeRef.current = active;
    if (strip.current) reveal(strip.current, active);
  }, [active, tabIds]);
  useLayoutEffect(() => {
    const box = strip.current;
    if (!box) return;
    const observer = new ResizeObserver(() => reveal(box, activeRef.current));
    observer.observe(box);
    return () => observer.disconnect();
  }, []);

  // Lista de todas as abas, com o Orquestrador; a marca de ativa é o check no rótulo. Só existe com executores.
  const OrchestratorIcon = orchestrator?.choice ? TOOL_ICON[orchestrator.choice.tool] : SquareTerminal;
  const ExecutorToolIcon = executorChoice ? TOOL_ICON[executorChoice.tool] : SquareTerminal;
  // Rótulo e ícone de cada executor, calculados uma vez: o menu e a faixa usam os mesmos.
  const rows = executors.map((e) => {
    const info = STATES[e.state];
    return {
      e, Icon: info.icon, color: colorsPreset(info.color), spin: e.state === 'running' ? ' animate-spin' : '',
      label: `Executor #${e.index + 1}, ${info.label}: ${e.title}`, desc: info.label,
    };
  });
  const tabMenu = [
    {
      key: ORCHESTRATOR, title: 'Orquestrador',
      icon: <OrchestratorIcon className="size-3.5" />,
      label: (
        <Flexbox horizontal align="center" gap={6} className="min-w-0">
          <Check className={`size-3.5 shrink-0${active === ORCHESTRATOR ? '' : ' invisible'}`} />
          <Text as="span" ellipsis className="min-w-0">Orquestrador</Text>
        </Flexbox>
      ),
      onClick: () => choose(ORCHESTRATOR),
    },
    ...rows.map(({ e, Icon, color, spin, label: title, desc }) => {
      return {
        key: e.id, title,
        icon: <Icon className={`size-3.5${spin}`} style={{ color }} />,
        desc,
        label: (
          <Flexbox horizontal align="center" gap={6} className="min-w-0">
            <Check className={`size-3.5 shrink-0${active === e.id ? '' : ' invisible'}`} />
            <Text as="span" className="shrink-0 tabular-nums">#{e.index + 1}</Text>
            <Text as="span" ellipsis className="min-w-0">{e.title}</Text>
          </Flexbox>
        ),
        onClick: () => choose(e.id),
      };
    }),
  ];

  // Sem padding nem cartão arredondado: o painel é a superfície inteira, separada do meio pela linha do DraggablePanel.
  return (
    <DraggablePanel
      placement="right" expandable={false} showHandleWideArea={false}
      size={{ width }} defaultSize={PANEL_DEFAULT_SIZE} onSizeChange={(_, size) => typeof size?.width === 'number' && onWidthChange(size.width)}
      minWidth={PANEL_MIN} maxWidth={maxWidth}
      backgroundColor="var(--ant-color-bg-container)"
    >
      <Flexbox height="100%" className="min-h-0">
        {/* Faixa de abas: 36 px, com as abas em variant square (linha de 2 px na ativa). Só a faixa rola na horizontal, com a barra oculta; a lista fica fora da rolagem. */}
        <Flexbox horizontal align="stretch" gap={4} height={36} flex="none" paddingInline={4} className="relative">
          {/* Divisor de largura total, por baixo da faixa; vem antes da faixa no DOM para o indicador da aba ativa pintar por cima. A sombra nativa da lista sai, para não duplicar a linha. */}
          <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-border" />
          <Flexbox
            ref={strip} horizontal align="stretch" flex={1} className="min-w-0 overflow-x-auto [scrollbar-width:none]!"
            onWheel={wheelToScroll}
          >
            {/* `overflow-clip` recorta o indicador sem criar outra área de rolagem. O gap 0 entre abas e a sombra zerada substituem o gap de 16 px e a borda nativa do variant square. */}
            <TabsRoot variant="square" value={active} onValueChange={(key: string) => choose(key)} className="gap-0">
              <TabsList className="h-full flex-nowrap justify-start overflow-clip" style={{ gap: 0, boxShadow: 'none' }}>
                <TabsIndicator />
                <TabsTab value={ORCHESTRATOR} data-tab={ORCHESTRATOR} className="flex-none gap-1.5 px-3">
                  <OrchestratorIcon className="size-3.5 shrink-0" />
                  Orquestrador
                </TabsTab>
                {orchestrator && rows.map(({ e, Icon, color, spin, label }) => {
                  return (
                    <TabsTab key={e.id} value={e.id} data-tab={e.id} title={label} aria-label={label} className="flex-none gap-1.5 px-3">
                      <Icon className={`size-3.5 shrink-0${spin}`} style={{ color }} />
                      <Text as="span" className="tabular-nums">#{e.index + 1}</Text>
                    </TabsTab>
                  );
                })}
              </TabsList>
            </TabsRoot>
          </Flexbox>
          {/* Fora da rolagem: só com executores, e com a contagem de todas as abas, como o botão da lista nos painéis Arquivos e Terminal. */}
          {orchestrator && executors.length > 0 && (
            <DropdownMenu items={tabMenu} placement="bottomRight">
              <Button
                type="text" size="small" icon={<List className="size-3.5" />} className="shrink-0 self-center"
                aria-label={`Lista de abas: ${tabMenu.length}`} title={`Todas as abas (${tabMenu.length})`}
              >
                {tabMenu.length}
              </Button>
            </DropdownMenu>
          )}
        </Flexbox>

        <Flexbox flex={1} className="relative min-h-0">
          {orchestrator && (
            <Flexbox horizontal align="center" gap={8} height={36} flex="none" paddingInline={12} className="@container whitespace-nowrap border-b">
              {node && meta ? (
                <>
                  {node.state !== 'running' && (
                    <Tag className="min-w-0 shrink" color={meta.color} icon={<meta.icon />} title={meta.label}>
                      <Text as="span" ellipsis>{meta.label}</Text>
                    </Tag>
                  )}
                  <Text as="span" code type="secondary" className="shrink-0">#{node.index + 1}</Text>
                  <Text ellipsis className="w-0 min-w-16 flex-1" title={node.title}>{node.title}</Text>
                  {node.choice && <ChoiceText choice={node.choice} compact />}
                  {CANCELLABLE.includes(node.state) && (
                    <Button type="default" size="small" className="shrink-0" title="Interrompe só este executor" icon={<Square className="size-3" />} onClick={() => onStop(node.id)}>
                      Parar
                    </Button>
                  )}
                </>
              ) : (
                <>
                  {orchestrator.choice && <ChoiceText choice={orchestrator.choice} />}
                  {orchestrator.choice && (
                    <Button
                      type="default" size="small" className="ml-auto shrink-0"
                      icon={<SlidersHorizontal className="size-3.5" />}
                      title={executorChoice ? `Executores: ${choiceLabel(executorChoice)}. Escolha dos executores; a troca vale para os próximos` : 'Configurar executores; a escolha vale para os próximos'}
                      aria-label={executorChoice ? `Executores: ${choiceLabel(executorChoice)}` : 'Configurar executores'}
                      aria-haspopup="dialog"
                      onClick={onExecutorChoice}
                    >
                      {/* Classes no Button perdem para o CSS sem camada do Lobe UI; por isso o peso e o leading ficam neste span */}
                      <Flexbox horizontal as="span" align="center" gap={6} className="font-normal leading-normal">
                        {executorChoice ? (
                          <>
                            <Text as="span" className="hidden shrink-0 font-normal @[28rem]:inline">Executores</Text>
                            <ExecutorToolIcon size={14} className="shrink-0" />
                            <Text as="span" weight={500} className="shrink-0 font-normal">{TOOL_NAME[executorChoice.tool]}</Text>
                          </>
                        ) : (
                          <>
                            <span className="@[28rem]:hidden">Configurar</span>
                            <span className="hidden @[28rem]:inline">Configurar executores</span>
                          </>
                        )}
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
                <TermHost id={ORCHESTRATOR} project={projectId} writable />
              </div>
            )}
            {orchestrator && executors.map((e) => (
              <div
                key={e.id}
                className={`absolute inset-0${active !== e.id ? ' invisible' : ''}`}
              >
                <TermHost id={e.id} project={projectId} writable={false} onReady={(view) => { view.setLive(); void view.attach(); }} />
                {COVER_TEXT[e.state] && (
                  <Cover>
                    <Text as="p" type="secondary" fontSize={14} align="center" className="max-w-[300px]">{COVER_TEXT[e.state]}</Text>
                  </Cover>
                )}
              </div>
            ))}

            {!projectOpen ? (
              <OpenProjectPrompt onOpen={onOpenProject} description="Abra uma pasta para iniciar o Orquestrador." />
            ) : !orchestrator && (
              <Flexbox align="center" gap={12} padding={16} width="min(380px, calc(100% - 32px))" height="fit-content" className="absolute inset-0 m-auto">
                {notice && <Text as="p" role="status" fontSize={14} className="rounded-md border bg-card px-3 py-1.5">{notice}</Text>}
                <Button type="primary" onClick={onOpen}>Abrir terminal</Button>
                <Text as="p" fontSize={12} align="center" style={SECONDARY}>
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
