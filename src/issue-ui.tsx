// Peças de interface comuns à faixa Issues (`issues-sidebar.tsx`), à página Issues (`issues-page.tsx`) e à aba Nova issue (`new-issue.tsx`):
// as cores fixas que imitam o GitHub (só nestas telas, e só aqui), o ícone de estado, a bolinha e a pílula de label, o seletor de labels,
// as marcas de em andamento e de próxima, o botão Iniciar (com o menu quando o orquestrador está ocupado) e as ações de uma criação que falhou.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { CircleAlert, CircleCheck, CircleDot, Clock, LoaderCircle, Play, Plus, RotateCw, Trash2 } from 'lucide-react';
import { ActionIcon, Button, DropdownMenu, type DropdownItem } from '@lobehub/ui/base-ui';
import { labelTextColor, topLabels } from './issue-text';
import { useIssues, type PendingIssue } from './issues-store';
import type { Issue, IssueLabel, Label } from './types';

/** Cores do GitHub (modo escuro): o botão verde e o hover dele, os ícones de aberta, fechada e falha, e o âmbar da issue em andamento.
 * O resto usa os tokens do tema. */
export const GH = { green: '#238636', greenHover: '#2ea043', open: '#3fb950', closed: '#a371f7', failed: '#f85149', amber: '#d29922' } as const;
/** Classes do botão verde do GitHub ("New issue", "Criar issue", "Criar"), com as mesmas cores de `GH` (o Tailwind precisa do texto literal). */
export const GREEN_BUTTON = 'border-0! bg-[#238636]! text-white! hover:bg-[#2ea043]! disabled:bg-[#238636]! disabled:text-white! disabled:opacity-50!';

/** Relógio da tela: o `Date.now()` de agora, refeito a cada `ms`. */
export function useNow(ms: number) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(timer);
  }, [ms]);
  return now;
}

/** Iniciar indisponível: sem o orquestrador aberto não há onde colar. */
export const START_OFF = 'Abra o orquestrador para iniciar uma issue.';
/** O Iniciar, um ponto só no App para a faixa e a página; null quando o orquestrador não está aberto. */
export type StartIssue = {
  /** Rodada do orquestrador em andamento: o Iniciar abre o menu em vez de colar. */
  busy: boolean;
  /** Cola a issue no rascunho do orquestrador (sem enviar) e faz dela a em andamento. */
  now(issue: Issue): void;
  /** Marca a issue como a próxima, colada quando o orquestrador terminar; null tira a marca. */
  later(issue: Issue | null): void;
} | null;

export type IssueIconKind = 'open' | 'closed' | 'sending' | 'failed';
const ICONS = {
  open: { icon: CircleDot, color: GH.open, label: 'Aberta' },
  closed: { icon: CircleCheck, color: GH.closed, label: 'Fechada' },
  sending: { icon: LoaderCircle, color: undefined, label: 'Enviando' },
  failed: { icon: CircleAlert, color: GH.failed, label: 'Falhou' },
} as const;

/** Ícone de estado da issue, com o nome do estado para leitores de tela: aberta (verde), fechada (roxo), enviando (anel girando) e falhou (vermelho). */
export function IssueIcon({ kind, size = 16 }: { kind: IssueIconKind; size?: number }) {
  const { icon: Icon, color, label } = ICONS[kind];
  return (
    <Icon
      role="img" aria-label={label} size={size}
      className={`shrink-0${kind === 'sending' ? ' animate-spin text-[var(--ant-color-text-tertiary)]' : ''}`}
      style={color ? { color } : undefined}
    />
  );
}

export const pendingKind = (p: PendingIssue): IssueIconKind => (p.status === 'sending' ? 'sending' : 'failed');

/** Bolinha com a cor da label e o nome, para a faixa e as listas de escolha. */
export function LabelDot({ label }: { label: IssueLabel }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1">
      <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ background: `#${label.color}` }} />
      <span className="truncate">{label.name}</span>
    </span>
  );
}

/** Pílula da label como no GitHub: fundo na cor dela e o texto preto ou branco que contrasta. */
export function LabelPill({ label }: { label: IssueLabel }) {
  return (
    <span
      className="inline-flex h-5 shrink-0 items-center rounded-full px-[7px] text-[12px] leading-[18px] font-medium whitespace-nowrap"
      style={{ background: `#${label.color}`, color: labelTextColor(label.color) }}
    >
      {label.name}
    </span>
  );
}

/**
 * Lista das labels do repositório para marcar e desmarcar (o "+" da captura, o seletor da aba Nova issue e o filtro da página),
 * com a bolinha de cor de cada uma. A lista fica aberta enquanto se marca; Esc ou clique fora fecha.
 */
export function LabelMenu({ labels, selected, onChange, children }: {
  labels: readonly IssueLabel[];
  selected: readonly string[];
  onChange: (next: string[]) => void;
  children: ReactNode;
}) {
  const items: DropdownItem[] = labels.length
    ? labels.map((label) => ({
      type: 'checkbox' as const, key: label.name, label: <LabelDot label={label} />, checked: selected.includes(label.name), closeOnClick: false,
      onCheckedChange: (checked: boolean) => onChange(checked ? [...selected, label.name] : selected.filter((n) => n !== label.name)),
    }))
    : [{ key: 'none', label: 'O repositório não tem labels.', disabled: true }];
  return <DropdownMenu items={items} placement="bottomLeft">{children}</DropdownMenu>;
}

/**
 * O seletor de labels da captura e da aba Nova issue: as 3 mais usadas como chips que marcam e desmarcam (na cor da label quando marcadas),
 * as outras marcadas pelo "+" também como chips, e o "+" que abre a lista de todas as labels do repositório.
 */
export function LabelChips({ issues, labels, selected, onChange }: {
  issues: readonly Issue[];
  labels: readonly Label[];
  selected: readonly string[];
  onChange: (next: string[]) => void;
}) {
  const top = topLabels([...issues], [...labels], 3);
  const shown = [...top, ...labels.filter((l) => selected.includes(l.name) && !top.some((t) => t.name === l.name))];
  const toggle = (name: string) => onChange(selected.includes(name) ? selected.filter((n) => n !== name) : [...selected, name]);
  return (
    <div role="group" aria-label="Labels da nova issue" className="flex flex-wrap items-center gap-1">
      {shown.map((label) => (
        <LabelToggle key={label.name} label={label} on={selected.includes(label.name)} onToggle={() => toggle(label.name)} />
      ))}
      <LabelMenu labels={labels} selected={selected} onChange={onChange}>
        <ActionIcon
          icon={Plus} size={{ size: 14, blockSize: 20, borderRadius: 10 }} variant="outlined"
          aria-label="Todas as labels do repositório" title="Todas as labels" className={FOCUS_RING}
        />
      </LabelMenu>
    </div>
  );
}

/** Chip de label que marca e desmarca (`aria-pressed`): a bolinha e o nome; marcado, na cor da label. A captura e o filtro da faixa usam. */
export function LabelToggle({ label, on, onToggle }: { label: IssueLabel; on: boolean; onToggle: () => void }) {
  return (
    <Button
      size="small" shape="round" aria-pressed={on} onClick={onToggle}
      className="h-5! gap-1! px-2! text-[11px]!"
      style={on ? { background: `#${label.color}`, borderColor: `#${label.color}`, color: labelTextColor(label.color) } : undefined}
    >
      {on ? label.name : <LabelDot label={label} />}
    </Button>
  );
}

/** Foco por teclado visível nos `ActionIcon` (sozinho, ele não mostra o foco), como no painel de pesquisa. */
export const FOCUS_RING = 'focus-visible:ring-2 focus-visible:ring-ring/50';

/** Mantém visíveis as ações escondidas de uma linha enquanto o menu do Iniciar está aberto (o mouse e o foco vão para o menu, fora da linha). */
export const MENU_OPEN_SHOWS = 'has-[[aria-expanded=true]]:flex';

/**
 * Botão Iniciar da linha. Orquestrador livre: cola a issue na hora. Ocupado: abre o menu com "Colar agora no chat" e "Iniciar quando o
 * orquestrador terminar" (ou "Não iniciar depois", se ela já é a próxima); Esc fecha e devolve o foco ao botão. Sem orquestrador fica
 * desabilitado, com o motivo no `title`. O menu fica sempre montado (só não abre com o orquestrador livre): o botão não perde o foco quando
 * o orquestrador muda de estado. O `data-start` é por onde o Enter na linha da faixa aciona o botão.
 */
export function StartButton({ issue, start }: { issue: Issue; start: StartIssue }) {
  const { active, next } = useIssues();
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
  const busyWith = active?.number ?? null, isNext = next === issue.number;
  const items = useMemo<DropdownItem[]>(() => [{
    type: 'group', key: 'busy', label: busyWith === null ? 'Orquestrador ocupado' : `Orquestrador ocupado com #${busyWith}`,
    children: [
      { key: 'now', icon: Play, label: 'Colar agora no chat', onClick: () => latest.current.start?.now(latest.current.issue) },
      isNext
        ? { key: 'cancel', icon: Clock, label: 'Não iniciar depois', onClick: () => latest.current.start?.later(null) }
        : { key: 'later', icon: Clock, label: 'Iniciar quando o orquestrador terminar', onClick: () => latest.current.start?.later(latest.current.issue) },
    ],
  }], [busyWith, isNext]);
  const label = `Iniciar a issue #${issue.number}`;
  if (!start) {
    return <Button data-start size="small" icon={<Play className="size-3.5" />} disabled aria-label={label} title={START_OFF}>Iniciar</Button>;
  }
  return (
    <DropdownMenu
      items={items} placement="bottomRight" open={open && start.busy}
      onOpenChange={(want, details) => {
        // Fechou pelo Esc ou por um item: o foco volta ao botão já aqui, antes de a tela atualizar. Depois, sem o menu aberto, a faixa
        // esconde o botão (só fica no hover e no foco), e a devolução do próprio menu chegaria a um botão escondido e cairia no body.
        if (!want && details.reason !== 'outside-press' && details.reason !== 'focus-out') button.current?.focus();
        if (!want || start.busy) return setOpen(want);
        // Livre: o clique (ou Enter e Espaço) no botão cola na hora, sem menu.
        if (details.reason === 'trigger-press') start.now(issue);
      }}
    >
      <Button
        ref={button} data-start size="small" icon={<Play className="size-3.5" style={{ color: GH.open }} />} aria-label={label}
        title={start.busy ? 'O orquestrador está trabalhando: colar agora ou quando ele terminar' : 'Colar no orquestrador (sem enviar)'}
      >
        Iniciar
      </Button>
    </DropdownMenu>
  );
}

/** Etiqueta "próxima" com o relógio: na linha da faixa (`pill` falso, só o texto) e como pílula ao lado do título na página. */
export function NextMark({ pill = false }: { pill?: boolean }) {
  return (
    <span
      className={`inline-flex shrink-0 items-center gap-1 font-medium text-[var(--ant-color-primary-text)] ${pill ? 'h-5 rounded-full border border-[var(--ant-color-primary-border)] px-[7px] text-[12px] leading-[18px]' : ''}`}
    >
      <Clock aria-hidden className="size-3" />
      próxima
    </span>
  );
}

/** Pílula âmbar "em andamento", ao lado do título na página. */
export function ActiveMark() {
  return (
    <span
      className="inline-flex h-5 shrink-0 items-center rounded-full border px-[7px] text-[12px] leading-[18px] font-medium whitespace-nowrap"
      style={{ color: GH.amber, background: `${GH.amber}26`, borderColor: `${GH.amber}73` }}
    >
      em andamento
    </span>
  );
}

/** Ações de uma criação que falhou: reenviar ou descartar. `compact` (a faixa estreita) mostra só os ícones, com o nome no `title`. */
export function FailedActions({ pending, retry, discard, compact = false }: {
  pending: PendingIssue; retry: (id: string) => void; discard: (id: string) => void; compact?: boolean;
}) {
  if (compact) {
    return (
      <>
        <ActionIcon
          size="small" variant="borderless" icon={RotateCw} className={FOCUS_RING} title="Tentar de novo"
          aria-label={`Tentar de novo: ${pending.title}`} onClick={() => retry(pending.localId)}
        />
        <ActionIcon
          size="small" variant="borderless" icon={Trash2} className={FOCUS_RING} title="Descartar"
          aria-label={`Descartar: ${pending.title}`} onClick={() => discard(pending.localId)}
        />
      </>
    );
  }
  return (
    <>
      <Button size="small" icon={<RotateCw className="size-3.5" />} aria-label={`Tentar de novo: ${pending.title}`} onClick={() => retry(pending.localId)}>
        Tentar de novo
      </Button>
      <Button size="small" icon={<Trash2 className="size-3.5" />} aria-label={`Descartar: ${pending.title}`} onClick={() => discard(pending.localId)}>
        Descartar
      </Button>
    </>
  );
}

/** As labels de uma criação, com a cor do repositório (cinza quando a label não está mais lá). */
export const pendingLabels = (names: readonly string[], repo: readonly IssueLabel[]): IssueLabel[] =>
  names.map((name) => repo.find((l) => l.name === name) ?? { name, color: '8b949e' });
