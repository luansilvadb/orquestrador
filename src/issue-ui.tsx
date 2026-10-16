// Peças de interface comuns à faixa Issues (`issues-sidebar.tsx`), ao detalhe da issue (`issues-page.tsx`) e à aba do formulário
// (`new-issue.tsx`): as cores de estado (só nos ícones), o ícone de estado, a bolinha e o chip neutro de label, o seletor de labels (com a
// criação de uma nova), as marcas de estado (`Tag` com ícone: aberta, fechada, em andamento, próxima), o botão Colar no orquestrador (com
// o menu quando ele está ocupado) e as ações de estado da issue (Concluir, Voltar às abertas, Reabrir: o cartão Em andamento e o detalhe
// usam as mesmas). E o relógio único das telas das Issues (`useNow`, `Ago`).
import { lazy, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react';
import { Activity, Check, CircleCheck, CircleDot, ClipboardPaste, Clock, Plus, RotateCcw, Undo2 } from 'lucide-react';
import { Flexbox } from '@lobehub/ui';
import { ActionIcon, Button, DropdownMenu, Input, Modal, Tag, Text, toast, type DropdownItem } from '@lobehub/ui/base-ui';
import { message } from './backend';
import { allLabels, LABEL_MAX, labelColor, pickLabel, relativeTime, topLabels } from './issue-text';
import { useIssues } from './issues-store';
import type { ActiveIssue, Issue } from './types';

// O Markdown do Lobe UI (com os plugins dele) dobra o pacote principal: só carrega na primeira vez que alguém o mostra (a aba Visualizar do
// formulário e a descrição do detalhe), e é um pedaço só. É o mesmo `Markdown` de `@lobehub/ui`, pelo caminho do módulo dele
// (`es/Markdown/index`), para o Vite separar o pedaço. Quem usa envolve em `Suspense`.
export const IssueMarkdown = lazy(() => import('@lobehub/ui/es/Markdown/index'));

/** Cores de estado das issues, as do GitHub no modo escuro, só para ícones: aberta, fechada e o âmbar da issue em andamento. Texto e botões usam os tokens do tema. */
export const GH = { open: '#3fb950', closed: '#a371f7', amber: '#d29922' } as const;

// O relógio das telas das Issues: um intervalo só, de 1 s, ligado enquanto alguém o lê. A faixa e o detalhe leem o mesmo `Date.now()`,
// e os tempos relativos iguais dizem o mesmo nos dois lugares.
let clockNow = Date.now();
let clockTimer: number | undefined;
const clockListeners = new Set<() => void>();
function subscribeClock(listener: () => void) {
  if (!clockListeners.size) {
    // Parado até aqui: a hora guardada é velha (o `useSyncExternalStore` relê depois de assinar).
    clockNow = Date.now();
    clockTimer = window.setInterval(() => {
      clockNow = Date.now();
      clockListeners.forEach((l) => l());
    }, 1000);
  }
  clockListeners.add(listener);
  return () => {
    clockListeners.delete(listener);
    if (!clockListeners.size) window.clearInterval(clockTimer);
  };
}
const readClock = () => clockNow;

/** O `Date.now()` do relógio das Issues, refeito a cada segundo: quem usa renderiza a cada segundo (a faixa). */
export const useNow = () => useSyncExternalStore(subscribeClock, readClock);

/** Um texto do relógio das Issues (`format(now)`): quem usa só renderiza de novo quando o texto muda. */
export const useNowText = (format: (now: number) => string) => useSyncExternalStore(subscribeClock, () => format(clockNow));

/** "agora", "há 5 min"… desde `iso`, no relógio das Issues: só este texto renderiza de novo, não a tela em volta (o detalhe usa no lugar de um `now`). */
export function Ago({ iso }: { iso: string }) {
  return <>{useNowText((now) => relativeTime(iso, now))}</>;
}

/** Colar indisponível: sem o orquestrador aberto não há onde colar. */
export const START_OFF = 'Abra o orquestrador para colar uma issue.';
/** O que o botão faz: só cola. Vai no `title` e na descrição acessível do botão, e como dica visível no detalhe. */
export const START_HINT = 'Cola no rascunho do orquestrador; você envia.';
const BUSY_HINT = 'Orquestrador ocupado: cola agora ou quando ele terminar; você envia.';
/** O Colar no orquestrador, um ponto só no App para a faixa e o detalhe; null quando o orquestrador não está aberto. */
export type StartIssue = {
  /** Rodada do orquestrador em andamento: o botão abre o menu em vez de colar. */
  busy: boolean;
  /** Cola a issue no rascunho do orquestrador (sem enviar) e faz dela a em andamento. */
  now(issue: Issue): void;
  /** Marca a issue como a próxima, colada quando o orquestrador terminar; null tira a marca. */
  later(issue: Issue | null): void;
} | null;

export type IssueIconKind = 'open' | 'closed';
const ICONS = {
  open: { icon: CircleDot, color: GH.open, label: 'Aberta' },
  closed: { icon: CircleCheck, color: GH.closed, label: 'Fechada' },
} as const;

/** Ícone de estado da issue, com o nome do estado para leitores de tela: aberta (verde) e fechada (roxo). */
export function IssueIcon({ kind, size = 16 }: { kind: IssueIconKind; size?: number }) {
  const { icon: Icon, color, label } = ICONS[kind];
  return <Icon role="img" aria-label={label} size={size} className="shrink-0" style={{ color }} />;
}

export type ConfirmOptions = { title: string; body: string; confirmLabel: string; cancelLabel?: string };
/** As escolhas de uma pergunta (`useChoice`), depois do Cancelar e nesta ordem; `danger` é a destrutiva. */
export type ChoiceOptions<K extends string> = { title: string; body: string; cancelLabel?: string; choices: { key: K; label: string; danger?: boolean }[] };
/**
 * Pergunta numa janela (Modal), no padrão da do descarte da Nova issue: `choose(...)` devolve a chave do botão escolhido; Esc, Cancelar e
 * clique fora devolvem null. O Cancelar vem primeiro e recebe o foco. Quem usa renderiza o `dialog` em qualquer ponto da própria árvore (a
 * janela vai para um portal).
 * Fechada, a janela devolve o foco a quem a abriu; se esse elemento saiu da tela com a resposta, `afterClose(fn)` põe o foco em outro lugar
 * depois disso (sem janela, `fn` roda na hora).
 */
export function useChoice<K extends string>() {
  const [ask, setAsk] = useState<(ChoiceOptions<K> & { resolve: (key: K | null) => void }) | null>(null);
  // A janela está aberta ou fechando: o `fn` do `afterClose` espera o fim da animação e a devolução de foco da própria janela.
  const shown = useRef(false);
  const waiting = useRef<(() => void)[]>([]);
  const [closed, setClosed] = useState(0);
  useEffect(() => {
    const fns = waiting.current.splice(0);
    if (!fns.length) return;
    // A devolução de foco da janela roda numa microtarefa depois de ela sair da árvore; o quadro seguinte vem depois dela.
    const frame = requestAnimationFrame(() => fns.forEach((fn) => fn()));
    return () => cancelAnimationFrame(frame);
  }, [closed]);
  // A resposta pode tirar da tela quem usa a pergunta (Concluir some com o painel "Com o orquestrador"): a janela sai junto, sem `afterClose`.
  useEffect(() => () => {
    const fns = waiting.current.splice(0);
    if (fns.length) requestAnimationFrame(() => fns.forEach((fn) => fn()));
  }, []);
  const choose = (options: ChoiceOptions<K>) => {
    shown.current = true;
    return new Promise<K | null>((resolve) => setAsk({ ...options, resolve }));
  };
  const answer = (key: K | null) => {
    ask?.resolve(key);
    setAsk(null);
  };
  const afterClose = (fn: () => void) => {
    if (shown.current) waiting.current.push(fn);
    else fn();
  };
  const dialog = (
    <Modal
      open={ask !== null} title={ask?.title} width={480} closable={false} keyboard onCancel={() => answer(null)}
      afterClose={() => {
        shown.current = false;
        setClosed((n) => n + 1);
      }}
      footer={(
        <Flexbox horizontal wrap="wrap" justify="flex-end" gap={8} width="100%">
          <Button onClick={() => answer(null)}>{ask?.cancelLabel ?? 'Cancelar'}</Button>
          {ask?.choices.map((c) => <Button key={c.key} danger={c.danger} onClick={() => answer(c.key)}>{c.label}</Button>)}
        </Flexbox>
      )}
    >
      <Text as="div" fontSize={14} lineHeight={1.6} style={{ ...SECONDARY, margin: 0, overflowWrap: 'anywhere' }}>{ask?.body}</Text>
    </Modal>
  );
  return { choose, dialog, afterClose };
}

/** Confirmação de dois botões (`useChoice` com uma escolha só, a destrutiva): `confirm(...)` devolve true só no botão de confirmar. */
export function useConfirm() {
  const { choose, dialog, afterClose } = useChoice<'ok'>();
  const confirm = ({ confirmLabel, ...options }: ConfirmOptions) =>
    choose({ ...options, choices: [{ key: 'ok', label: confirmLabel, danger: true }] }).then((key) => key === 'ok');
  return { confirm, dialog, afterClose };
}

/** Bolinha com a cor da label (`labelColor`) e o nome, para a faixa e as listas de escolha. */
export function LabelDot({ name }: { name: string }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1">
      <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ background: `#${labelColor(name)}` }} />
      <span className="truncate">{name}</span>
    </span>
  );
}

/** Chip neutro da label: fundo de preenchimento do tema, a bolinha na cor dela e o nome em texto primário. */
export function LabelPill({ name }: { name: string }) {
  return (
    <span className="inline-flex h-5 shrink-0 items-center gap-1 rounded-[4px] bg-[var(--ant-color-fill-secondary)] px-1.5 text-[12px] leading-[18px] whitespace-nowrap text-[var(--ant-color-text)]">
      <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ background: `#${labelColor(name)}` }} />
      {name}
    </span>
  );
}

// Os itens de checkbox ficam fora do CSS global dos menus (`[role='menu'] > [role='menuitem']`, index.css): sem isto têm ~33 px; o padrão do app é 26 px.
const LABEL_ITEM = '[&_[role=menuitemcheckbox]]:h-[26px]! [&_[role=menuitemcheckbox]]:min-h-[26px]! [&_[role=menuitemcheckbox]]:py-0! [&_[role=menuitemcheckbox]]:px-[26px]! [&_[role=menuitemcheckbox]]:text-[13px]! [&_[role=menuitemcheckbox]]:leading-[26px]!';

/**
 * Lista das labels em uso no projeto para marcar e desmarcar (o "+" do formulário), com a bolinha de cor de cada uma, e,
 * no fim, "Nova label…" (`onNew`). A lista fica aberta enquanto se marca; Esc ou clique fora fecha.
 */
export function LabelMenu({ labels, selected, onChange, onNew, children }: {
  labels: readonly string[];
  selected: readonly string[];
  onChange: (next: string[]) => void;
  onNew: () => void;
  children: ReactNode;
}) {
  const items: DropdownItem[] = [
    ...labels.map((name) => ({
      type: 'checkbox' as const, key: name, label: <LabelDot name={name} />, checked: selected.includes(name), closeOnClick: false,
      onCheckedChange: (checked: boolean) => onChange(checked ? [...selected, name] : selected.filter((n) => n !== name)),
    })),
    ...(labels.length ? [{ type: 'divider' as const, key: 'divider' }] : []),
    { key: 'new', icon: Plus, label: 'Nova label…', onClick: onNew },
  ];
  return <DropdownMenu items={items} placement="bottomLeft" popupProps={{ className: LABEL_ITEM }}>{children}</DropdownMenu>;
}

/**
 * O seletor de labels do formulário: as 3 mais usadas no projeto como chips que marcam e desmarcam (marcados, com fundo
 * preenchido e borda), as outras marcadas também como chips, e o "+", que abre a lista de todas as labels em uso e o "Nova label…" (sem
 * nenhuma label no projeto, o "+" vai direto para o campo). O campo da label nova fica ao lado do "+": Enter marca a label (a grafia de
 * uma que já existe vale, sem diferenciar maiúsculas) e devolve o foco ao "+"; Esc desiste; sair do campo também.
 */
export function LabelChips({ issues, selected, onChange }: {
  issues: readonly Issue[];
  selected: readonly string[];
  onChange: (next: string[]) => void;
}) {
  const all = allLabels(issues);
  const top = topLabels(issues, 3);
  const shown = [...top, ...selected.filter((name) => !top.includes(name))];
  const toggle = (name: string) => onChange(selected.includes(name) ? selected.filter((n) => n !== name) : [...selected, name]);
  const [adding, setAdding] = useState(false);
  const [text, setText] = useState('');
  // O "+" é achado pelo grupo: um `ref` no gatilho do menu passaria pelo `element.ref`, que o React 19 não tem mais.
  const group = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  // O menu devolve o foco ao "+" quando fecha pelo "Nova label…": o campo o pega no quadro seguinte.
  useEffect(() => {
    if (!adding) return;
    const frame = requestAnimationFrame(() => field.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [adding]);
  const close = () => {
    // O foco vai ao "+" antes de o campo sumir: não se perde no documento.
    group.current?.querySelector<HTMLElement>('[data-label-plus]')?.focus();
    setText('');
    setAdding(false);
  };
  const add = () => {
    const name = pickLabel([...all, ...selected], text);
    if (name && !selected.includes(name)) onChange([...selected, name]);
    close();
  };
  const plusButton = (
    <ActionIcon
      data-label-plus icon={Plus} size={{ size: 14, blockSize: 20, borderRadius: 10 }} variant="outlined" className={FOCUS_RING}
      aria-label={all.length ? 'Marcar labels ou criar uma nova' : 'Criar uma label'} title={all.length ? 'Labels' : 'Nova label'}
      onClick={all.length ? undefined : () => setAdding(true)}
    />
  );
  return (
    <div ref={group} role="group" aria-label="Labels" className="flex flex-wrap items-center gap-1">
      {shown.map((name) => <LabelToggle key={name} name={name} on={selected.includes(name)} onToggle={() => toggle(name)} />)}
      {all.length ? <LabelMenu labels={all} selected={selected} onChange={onChange} onNew={() => setAdding(true)}>{plusButton}</LabelMenu> : plusButton}
      {adding && (
        <Input
          ref={field} size="small" value={text} maxLength={LABEL_MAX} placeholder="Nova label" aria-label="Nome da nova label" autoComplete="off"
          onChange={(e) => setText(e.target.value)} onBlur={() => setAdding(false)}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing || (e.key !== 'Enter' && e.key !== 'Escape')) return;
            // Enter e Esc são só do campo: não chegam ao formulário (nem criam a issue).
            e.preventDefault();
            e.stopPropagation();
            if (e.key === 'Enter') add();
            else close();
          }}
          className={`h-5! w-36! text-[12px]! ${FIELD_FOCUS}`}
        />
      )}
    </div>
  );
}

/** Chip de label que marca e desmarca (`aria-pressed`): bolinha e nome; marcado, com fundo preenchido, borda clara e o ✓ (não só cor). O formulário e o filtro da faixa usam. */
export function LabelToggle({ name, on, onToggle }: { name: string; on: boolean; onToggle: () => void }) {
  return (
    <Button
      size="small" shape="round" aria-pressed={on} onClick={onToggle}
      className={`h-5! gap-1! px-2! text-[12px]! ${FOCUS_RING}${on ? ' bg-[var(--ant-color-fill)]! border-[var(--ant-color-text-secondary)]!' : ''}`}
    >
      {on && <Check aria-hidden className="size-3 shrink-0" />}
      <LabelDot name={name} />
    </Button>
  );
}

/** Azul de foco do app (`focus-blue` do DESIGN.md, o mesmo do comparador e do Git): indicador não textual precisa de 3:1, e o `--color-ring` não chega. */
export const FOCUS_BLUE = '#007fd4';

/** Foco por teclado visível nos `ActionIcon` (sozinho, ele não mostra o foco), no azul de foco, opaco. */
export const FOCUS_RING = 'focus-visible:ring-2 focus-visible:ring-[#007fd4]';

/** O anel de foco nos botões que o Markdown do Lobe UI desenha (copiar do bloco de código): vai no invólucro do `IssueMarkdown`. */
export const MARKDOWN_FOCUS = '[&_button:focus-visible]:ring-2 [&_button:focus-visible]:ring-[#007fd4]';

/** O mesmo anel por dentro, para controles que ocupam a largura toda (cabeçalho de seção, linha da lista). */
export const FOCUS_RING_INSET = `${FOCUS_RING} focus-visible:ring-inset`;

/** Campo (`Input`/`TextArea` do Lobe UI) focado: borda e filete de 1 px no azul de foco. Vai no `className` do campo. */
export const FIELD_FOCUS = 'focus-within:border-[#007fd4]! focus-within:shadow-[0_0_0_1px_#007fd4]!';

/** Informação em texto pequeno (erro, metadado): o secundário do tema (#aaaaaa). O `type="secondary"` do Lobe UI resolve para o terciário (#6f6f6f, 4,2:1). */
export const SECONDARY = { color: 'var(--ant-color-text-secondary)' } as const;

/**
 * Botão Colar no orquestrador (o "Iniciar" de antes): só cola a issue no rascunho dele, quem envia é o usuário. Orquestrador livre: cola
 * na hora. Ocupado: abre o menu com "Colar agora" (que diz, havendo outra em andamento, que ela volta às abertas) e "Colar quando o
 * orquestrador terminar" (ou "Não colar depois", se ela já é a próxima; na em andamento, que não pode ser a próxima, só o "Colar agora"); Esc fecha e devolve o foco ao botão. Sem orquestrador fica
 * desabilitado, com o motivo no `title`. A issue já em andamento: com o texto no rascunho do orquestrador (`draft`) o botão some, sem
 * dica; fora dele (enviada, ou o orquestrador reabriu) vira o secundário "Colar de novo". O menu fica sempre montado (só não abre com o orquestrador livre): o botão
 * não perde o foco quando o orquestrador muda de estado. O `data-start` é por onde o Ctrl+Enter na linha da faixa aciona o botão.
 * `row` é o compacto da linha da faixa, só o ícone de colar num `ActionIcon` como as ações de linha do Git (o nome no `aria-label`, a dica
 * só para leitores de tela); `primary` é o do detalhe: `primary` branco de tinta, a ação principal
 * da tela (o `fill` do Lobe UI renderiza escuro), com a dica (ou o motivo de estar desabilitado) em texto secundário, numa linha própria
 * (`order-last basis-full`) depois de todos os botões do contêiner flex-wrap de quem o usa.
 */
export function StartButton({ issue, start, variant = 'row' }: { issue: Issue; start: StartIssue; variant?: 'row' | 'primary' }) {
  const { active, next, draft } = useIssues();
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  // O orquestrador ficou livre com o menu aberto: o menu some (o `open` abaixo exige `busy`) e não volta sozinho na próxima rodada.
  const busy = start?.busy ?? false;
  useEffect(() => {
    if (!busy) setOpen(false);
  }, [busy]);
  // Os itens só mudam com o que eles mostram: recriados a cada renderização (o relógio da faixa bate a cada segundo), o menu aberto
  // remontaria os itens e o foco do teclado sairia deles. As ações vêm do ref, sempre as da renderização mais recente.
  const latest = useRef({ issue, start });
  useLayoutEffect(() => {
    latest.current = { issue, start };
  });
  const busyWith = active?.number ?? null, isNext = next === issue.number, isActive = busyWith === issue.number;
  // A em andamento não pode ser a próxima: o menu dela fica só com o Colar agora.
  const items = useMemo<DropdownItem[]>(() => [{
    type: 'group', key: 'busy', label: busyWith === null ? 'Orquestrador ocupado' : `Orquestrador ocupado com #${busyWith}`,
    children: [
      {
        key: 'now', icon: ClipboardPaste, onClick: () => latest.current.start?.now(latest.current.issue),
        label: busyWith !== null && !isActive ? `Colar agora (#${busyWith} volta às abertas)` : 'Colar agora',
      },
      ...(isActive ? [] : [isNext
        ? { key: 'cancel', icon: Clock, label: 'Não colar depois', onClick: () => latest.current.start?.later(null) }
        : { key: 'later', icon: Clock, label: 'Colar quando o orquestrador terminar', onClick: () => latest.current.start?.later(latest.current.issue) }]),
    ],
  }], [busyWith, isActive, isNext]);
  const again = active?.number === issue.number;
  const label = again ? `Colar a issue #${issue.number} de novo no orquestrador` : `Colar a issue #${issue.number} no orquestrador`, hintId = useId();
  const primary = variant === 'primary';
  const hint = !start ? START_OFF : start.busy ? BUSY_HINT : START_HINT;
  // A dica é a descrição do botão (o `title` sozinho não é lido em botão desabilitado); no detalhe ela também é visível.
  const hintNode = primary
    ? <Text as="span" id={hintId} fontSize={12} lineHeight={1.4} style={SECONDARY} className="order-last min-w-0 basis-full @[44rem]:text-right">{hint}</Text>
    : <span id={hintId} className="sr-only">{hint}</span>;
  const text = again ? 'Colar de novo' : 'Colar no orquestrador';
  // O mesmo botão nos dois lugares: com texto no detalhe, só o ícone (em `currentColor`) na linha da faixa.
  const trigger = (props: { ref?: typeof button; disabled?: boolean; title?: string }) => primary
    ? (
      <Button
        {...props} data-start type={again ? undefined : 'primary'} icon={<ClipboardPaste className="size-3.5" />} className={FOCUS_RING}
        aria-label={label} aria-describedby={hintId}
      >
        {text}
      </Button>
    )
    : <ActionIcon {...props} data-start size="small" variant="borderless" icon={ClipboardPaste} className={FOCUS_RING} aria-label={label} aria-describedby={hintId} />;
  const group = (children: ReactNode) => <>{children}</>;
  // No rascunho do orquestrador, ainda não enviada: nada a oferecer (colar de novo duplicaria o texto no rascunho).
  if (again && draft.includes(issue.number)) return null;
  if (!start) {
    return group(
      <>
        {/* O `ActionIcon` desabilitado não recebe o ponteiro e a dica dele não abriria: no compacto, o motivo vai no `title` de fora. */}
        {primary ? trigger({ disabled: true, title: START_OFF }) : <span title={START_OFF} className="inline-flex">{trigger({ disabled: true })}</span>}
        {hintNode}
      </>,
    );
  }
  return group(
    <>
      <DropdownMenu
        items={items} placement={primary ? 'bottomLeft' : 'bottomRight'} open={open && start.busy}
        onOpenChange={(want, details) => {
          // Fechou pelo Esc ou por um item: o foco volta ao botão já aqui, antes de a tela atualizar. Depois, sem o menu aberto, a faixa
          // pode ter tirado o botão da linha (só a selecionada o mostra), e a devolução do próprio menu cairia no body.
          if (!want && details.reason !== 'outside-press' && details.reason !== 'focus-out') button.current?.focus();
          if (!want || start.busy) return setOpen(want);
          // Livre: o clique (ou Enter e Espaço) no botão cola na hora, sem menu.
          if (details.reason === 'trigger-press') start.now(issue);
        }}
      >
        {trigger({ ref: button, title: `${start.busy ? BUSY_HINT : START_HINT}${primary ? '' : ' (Ctrl+Enter)'}` })}
      </DropdownMenu>
      {hintNode}
    </>,
  );
}

const report = (e: unknown) => toast.error({ title: message(e), duration: 7000 });

/**
 * Ações de estado de uma issue, as mesmas no detalhe e no cartão Em andamento da faixa: na em andamento, Concluir (fecha a issue) e
 * Voltar às abertas; na fechada, Reabrir. A aberta comum não tem nenhuma (o Colar é à parte). Concluir sem rodada do orquestrador desde o
 * Colar pergunta antes; fechada, o aviso oferece Desfazer. Com o texto dela no rascunho do orquestrador, a pergunta e os avisos dizem que ele
 * continua lá. Cada ação seleciona a issue antes: depois de Concluir ela continua no detalhe,
 * já como Fechada e com o Reabrir à mão. `size` `small` é o da faixa. Cada ação tira o próprio botão da tela: `onDone` leva o foco a outro
 * lugar (o detalhe), depois de a pergunta fechar, se houve uma.
 */
export function IssueActions({ issue, size, onDone }: { issue: Issue; size?: 'small'; onDone?: () => void }) {
  const store = useIssues();
  const { confirm, dialog, afterClose } = useConfirm();
  const done = () => onDone && afterClose(onDone);
  const { number } = issue;
  const since = store.active?.number === number && issue.state === 'open' ? store.active : null;
  // O texto dela está no rascunho do orquestrador (colado e não enviado): sair do andamento não o apaga.
  const drafted = store.draft.includes(number);
  const icon = size === 'small' ? 'size-3.5' : 'size-4';
  const undo = (prev: ActiveIssue) => store.undoFinish(prev).then(
    () => toast.success({ title: `#${prev.number} reaberta e de volta a Em andamento`, duration: 3000 }), report,
  );
  // Sem rodada do orquestrador desde o Colar, fechar é dar por feita uma issue em que ele ainda não trabalhou: pergunta antes.
  const finish = async (prev: ActiveIssue) => {
    store.select(number);
    if (!prev.turned && !await confirm({
      title: `Concluir a #${number} sem o orquestrador ter trabalhado nela?`,
      body: `A #${number} será fechada, mas o orquestrador ainda não trabalhou nela: o texto foi colado e nenhuma rodada começou depois.${drafted ? ' O texto colado continua no rascunho do orquestrador: apague-o lá, para não enviar uma issue já fechada.' : ''}`,
      confirmLabel: 'Fechar', cancelLabel: 'Não fechar',
    })) return;
    done();
    await store.finish(number).then(() => toast.success({
      title: drafted ? `#${number} fechada. Apague o texto colado no rascunho do orquestrador.` : `#${number} fechada`, duration: 8000,
      actions: [{ label: 'Desfazer', variant: 'text', onClick: () => void undo(prev) }],
    }), report);
  };
  const reopen = () => {
    store.select(number);
    done();
    store.setState(number, true).then(() => toast.success({ title: `#${number} reaberta`, duration: 3000 }), report);
  };
  return (
    <>
      {since && (
        <>
          <Button
            size={size} className={FOCUS_RING} icon={<Check className={icon} />} title="Fecha a issue"
            aria-label={`Concluir a issue #${number} (fecha a issue)`} onClick={() => void finish(since)}
          >
            Concluir
          </Button>
          <Button
            size={size} className={FOCUS_RING} icon={<Undo2 className={icon} />} aria-label={`Voltar a issue #${number} às abertas`}
            onClick={() => {
              store.select(number);
              done();
              store.setActive(null);
              if (drafted) {
                toast.info({
                  title: `#${number} voltou para as abertas. O texto dela continua no rascunho do orquestrador: apague-o lá se não for enviar.`, duration: 6000,
                });
              }
            }}
          >
            Voltar às abertas
          </Button>
        </>
      )}
      {issue.state === 'closed' && (
        <Button size={size} className={FOCUS_RING} icon={<RotateCcw className={icon} />} aria-label={`Reabrir a issue #${number}`} title="Reabre a issue" onClick={reopen}>
          Reabrir
        </Button>
      )}
      {dialog}
    </>
  );
}

/**
 * Marcas de estado da issue: `Tag` do Lobe UI com ícone e rótulo (estado nunca só por cor, nem na forma do chip de label). `small`
 * (20 px) é a das linhas da faixa; `middle` a do cabeçalho do detalhe. A variante é `outlined` (o filete do tema, sem preenchimento): o
 * chip de label (`LabelPill`) é preenchido, e lado a lado no detalhe as duas formas não se confundem.
 */
type MarkSize = 'small' | 'middle';
/** O ícone vai com tamanho explícito (sem ele o lucide sai com 24 px e passa do filete): 12 px na `small`, 14 px na `middle`. */
const MARK_ICON = { small: 'size-3', middle: 'size-3.5' } as const;

/*
 * Sem os presets de cor do Lobe UI (`green`, `purple`, `gold`, `gray`: texto de 12 px abaixo de 4,5:1, 4,2:1 na "Aberta"): o texto é neutro e
 * a cor fica só no ícone. A `outlined` do Lobe UI pinta o fundo com `colorBgContainer`; `background: transparent` o tira, e o texto fica
 * sobre a superfície de baixo. Contraste medido: primário (#ffffff) 21:1 no detalhe (#000000), 19,4:1 no cartão Em andamento (#0d0d0d)
 * e 19,1:1 nele escolhido (#0f0f0f); secundário (#aaaaaa) 9,0:1 no detalhe.
 */
const MARK_TEXT = { color: 'var(--ant-color-text)', background: 'transparent' } as const;

/** Aberta (ícone verde de aberta) ou fechada (ícone roxo de fechada); o rótulo é texto primário. */
export function StateMark({ closed, size = 'small' }: { closed: boolean; size?: MarkSize }) {
  const Icon = closed ? CircleCheck : CircleDot;
  return (
    <Tag size={size} variant="outlined" icon={<Icon className={MARK_ICON[size]} style={{ color: closed ? GH.closed : GH.open }} />} className="shrink-0" style={MARK_TEXT}>
      {closed ? 'Fechada' : 'Aberta'}
    </Tag>
  );
}

/** "Próxima", com o relógio e o rótulo no texto secundário: a que o orquestrador cola sozinho quando terminar a rodada. */
export function NextMark({ size = 'small' }: { size?: MarkSize }) {
  return <Tag size={size} variant="outlined" icon={<Clock className={MARK_ICON[size]} />} className="shrink-0" style={{ ...MARK_TEXT, ...SECONDARY }}>Próxima</Tag>;
}

/** "Em andamento", com o ícone de atividade em âmbar: a issue que está com o orquestrador. */
export function ActiveMark({ size = 'small' }: { size?: MarkSize }) {
  return <Tag size={size} variant="outlined" icon={<Activity className={MARK_ICON[size]} style={{ color: GH.amber }} />} className="shrink-0" style={MARK_TEXT}>Em andamento</Tag>;
}
