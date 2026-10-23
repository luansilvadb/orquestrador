// Painel Git da barra lateral: o controle de versão (branch, commit, Preparadas, Alterações e Gráfico) ou o histórico do projeto e de um arquivo.
// Clique num arquivo abre a revisão em prévia; duplo clique abre de vez; conflito abre o conflito. O histórico abre o arquivo de um commit.
// Tudo roda na pasta do projeto e só por clique do usuário. O status atualiza sozinho (usePolled, no App); cada ação atualiza na hora.
import { useEffect, useRef, useState } from 'react';
import { ActionIcon, Button, ContextMenuTrigger, DropdownMenu, Input, Popover, Text, TextArea, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { ArrowDown, ArrowUp, Check, ChevronDown, ChevronRight, Ellipsis, GitBranch, History, LoaderCircle, Minus, Plus, RefreshCw, SquareArrowOutUpRight } from 'lucide-react';
import { backend, message } from '../backend/backend';
import { FileIcon } from '../shared/file-icon';
import { KIND_COLOR } from '../explorer/explorer-decorations';
import { GraphSection, type GraphRemote } from './git-graph-view';
import { HistoryPanel, type HistoryRequest, KIND, type OpenCommitTarget } from './git-history';
import { splitPath } from '../shared/paths';
import { usePolling } from '../shared/polling';
import type { DiffSide, TabTarget } from '../editor/workspace-model';
import type { GitGuard } from './git-guard';
import type { GitBranches, GitFile, GitKind, GitState, GitStatus } from '../backend/types';

const LOCKED_TITLE = 'Há executores trabalhando. Espere o lote terminar.';

// Erro de ação: a mensagem do backend em toast, selecionável (o body tem user-select: none).
const fail = (e: unknown) => toast.error({ title: <span className="select-text">{message(e)}</span>, duration: 7000 });
const lastLine = (text: string) => text.trim().split('\n').pop() ?? '';
const commits = (n: number) => `${n} ${n === 1 ? 'commit' : 'commits'}`;
// Caminhos de um arquivo para preparar ou tirar: renomeado leva o nome antigo junto.
const pathsOf = (f: GitFile) => (f.origPath ? [f.path, f.origPath] : [f.path]);
// Chave de uma linha marcada: o lado separa a preparada da alteração do mesmo arquivo.
const keyOf = (side: DiffSide, path: string) => `${side}:${path}`;
// Uma ação: erro em toast, o status relido de qualquer jeito, e devolve se deu certo.
async function run(task: () => Promise<unknown>, refresh: () => Promise<void>): Promise<boolean> {
  try {
    await task();
    return true;
  } catch (e) {
    fail(e);
    return false;
  } finally {
    await refresh();
  }
}

/**
 * Consulta repetida (usePolling) e a mesma consulta sob demanda (`refresh`). O estado só muda quando a resposta é outra, para não piscar.
 * `enabled` falso não consulta. Consulta que falha zera o dado e guarda a mensagem de erro.
 */
export function usePolled<T>(read: () => Promise<T>, enabled: boolean) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState('');
  const last = useRef('');
  const refresh = async () => {
    if (!enabled) return;
    try {
      const next = await read();
      const text = JSON.stringify(next);
      setError('');
      if (text !== last.current) {
        last.current = text;
        setData(next);
      }
    } catch (e) {
      last.current = '';
      setData(null);
      setError(message(e));
    }
  };
  usePolling(refresh, 1500, enabled);
  return { data, error, refresh };
}

type GitPoll = ReturnType<typeof usePolled<GitState>>;
export type GitSidebarProps = {
  git: GitPoll;
  locked: boolean;
  /** Trava compartilhada com o editor central: uma operação Git da UI por vez, nas duas telas. */
  guard: GitGuard;
  /** A tela Git está aberta: sem isso, o histórico não consulta o Git. */
  active: boolean;
  /** Alvo aberto no editor central: marca a linha de diff (do mesmo lado) ou de conflito correspondente. */
  selected?: TabTarget | null;
  onOpenTarget: OpenCommitTarget;
  /** `permanent` abre de vez (duplo clique) em vez de prévia. */
  onOpenFile: (path: string, options?: { permanent?: boolean }) => void;
  /** Pedido de histórico: um pedido novo abre a lista (do projeto, ou do arquivo em `path`). */
  historyRequest?: HistoryRequest;
};

// Operação em curso. O commit com push passa por 'commit' e depois por 'push'.
// stage e branch entram na mesma trava: enquanto uma roda, nada mexe no índice nem na branch.
type Busy = GraphRemote | 'commit' | 'stage' | 'branch';
type CommitMode = 'commit' | 'commit-push';
type WithBusy = <T>(kind: Busy, task: () => Promise<T>) => Promise<T | undefined>;
const REMOTE: Record<GraphRemote, { run: () => Promise<string>; done: string }> = {
  fetch: { run: () => backend.gitFetch(), done: 'Busca concluída' },
  pull: { run: () => backend.gitPull(), done: 'Commits recebidos' },
  push: { run: () => backend.gitPush(), done: 'Enviado' },
};
const BUSY_TEXT: Record<Busy, string> = {
  fetch: 'Buscando…', pull: 'Recebendo…', push: 'Enviando…', commit: 'Commitando…', stage: 'Atualizando o preparo…', branch: 'Mudando de branch…',
};
const MODE_LABEL: Record<CommitMode, string> = { commit: 'Commit', 'commit-push': 'Commit e Push' };

/** Uma linha de diff ou de conflito está aberta no editor? O lado separa as seções; conflito vale no lado em conflito. */
function isOpenTarget(target: TabTarget | null | undefined, side: DiffSide, f: GitFile) {
  if (!target) return false;
  if (target.kind === 'conflict') return target.path === f.path && f[side] === 'conflicted';
  return target.kind === 'diff' && target.side === side && target.path === f.path;
}

/** O que cada linha pode fazer, vindo do painel (que tem o estado e as travas). */
type RowActions = {
  pick: (f: GitFile, side: DiffSide, ctrl: boolean) => void;
  review: (f: GitFile, side: DiffSide) => void;
  openFile: (f: GitFile) => void;
  stage: (f: GitFile, side: DiffSide, list: GitFile[]) => void;
  stageAll: (side: DiffSide) => void;
};

/**
 * Controle de versão: cabeçalho com atualizar, histórico e o menu de Buscar, Receber e Enviar; a branch; o commit; e as duas seções.
 * Fica montado mesmo com o histórico na frente: a mensagem digitada e os recolhidos não se perdem.
 */
function ChangesPanel({ status, error, refresh, locked, guard, active, selected, onOpenTarget, onOpenFile, onShowHistory }: {
  status: GitStatus | null;
  error: string;
  refresh: () => Promise<void>;
  locked: boolean;
  guard: GitGuard;
  /** Tela Git aberta e esta lista visível: só então o gráfico consulta o Git. */
  active: boolean;
  selected: TabTarget | null | undefined;
  onOpenTarget: OpenCommitTarget;
  /** `permanent` abre de vez (duplo clique) em vez de prévia. */
  onOpenFile: (path: string, options?: { permanent?: boolean }) => void;
  onShowHistory: () => void;
}) {
  const [busy, setBusy] = useState<Busy | null>(null);
  // O segundo clique chega antes de o estado (busy) mudar: a trava que vale é a do ref.
  const busyRef = useRef(false);
  const [text, setText] = useState('');
  const [mode, setMode] = useState<CommitMode>('commit');
  // Recolhido fica aqui, no painel, e não na seção: a seção some quando a lista esvazia, e o estado não pode sumir junto.
  const [collapsed, setCollapsed] = useState({ staged: false, unstaged: false, graph: false });
  // Altura que o divisor deu à seção Gráfico, e o contador de pedidos de recarregar o gráfico (cada ação soma 1).
  const [graphHeight, setGraphHeight] = useState<number | null>(null);
  const [graphTick, setGraphTick] = useState(0);
  // Linhas marcadas com Ctrl+clique. Preparar ou tirar age sobre elas quando a linha escolhida está marcada.
  const [marked, setMarked] = useState<Set<string>>(new Set());
  const refreshGraph = () => setGraphTick((n) => n + 1);

  if (!status) {
    return (
      <Flexbox className="min-h-0" flex={1} padding={12}>
        <Text as="p" role="status" fontSize={12} type={error ? 'danger' : 'secondary'}>
          {error || 'Lendo o Git…'}
        </Text>
      </Flexbox>
    );
  }

  const staged = status.files.filter((f) => f.staged);
  const unstaged = status.files.filter((f) => f.unstaged);
  const canCommit = !!text.trim() && staged.length > 0 && !busy;
  const hint = !staged.length ? 'Prepare arquivos para commitar' : !text.trim() ? 'Escreva a mensagem do commit' : '';
  // Buscar, Receber e Enviar: o menu do cabeçalho e os botões do gráfico usam o mesmo rótulo e a mesma trava.
  const remoteActions: Record<GraphRemote, { label: string; disabled: boolean }> = {
    fetch: { label: 'Buscar', disabled: busy !== null },
    pull: { label: status.behind > 0 ? `Receber (${commits(status.behind)})` : 'Receber', disabled: busy !== null || locked },
    push: { label: status.ahead > 0 ? `Enviar (${commits(status.ahead)})` : 'Enviar', disabled: busy !== null },
  };
  const remoteMenu = (['fetch', 'pull', 'push'] as const).map((key) => ({
    key, label: remoteActions[key].label, disabled: remoteActions[key].disabled, onClick: () => void remote(key),
  }));
  const running = busy === 'fetch' || busy === 'pull' || busy === 'push' ? busy : null;
  // Depois de Buscar, Receber, Enviar ou commit, o gráfico recarrega junto com o status.
  const refreshWithGraph = async () => {
    refreshGraph();
    await refresh();
  };
  // A escolha só muda o modo do botão e do Ctrl+Enter: nada roda aqui.
  const modeMenu = (['commit', 'commit-push'] as const).map((key) => ({
    key, label: MODE_LABEL[key], icon: mode === key ? <Check /> : undefined, onClick: () => setMode(key),
  }));

  // Uma operação por vez, com a mesma trava do ref. Libera no finally: sucesso ou erro, a tela volta a aceitar ações.
  const withBusy: WithBusy = async (kind, task) => {
    if (busyRef.current) return undefined;
    // Trava compartilhada: se o editor central está salvando ou gravando um diff, esta ação espera (e avisa).
    if (!guard.enter()) return undefined;
    busyRef.current = true;
    setBusy(kind);
    try {
      return await task();
    } finally {
      busyRef.current = false;
      setBusy(null);
      guard.leave();
    }
  };

  const remote = (kind: GraphRemote) => withBusy(kind, () => run(async () => {
    const out = await REMOTE[kind].run();
    toast.success({ title: REMOTE[kind].done, description: lastLine(out), duration: 3000 });
  }, refreshWithGraph));

  // Commit, e depois o push. O push só sai se o commit deu certo: commit com erro mantém a mensagem e não envia.
  // Commit feito limpa a mensagem mesmo se o envio falhar, para ninguém commitar o mesmo texto de novo; Enviar no menu tenta só o envio.
  // A limpeza só vale se o texto ainda é o enviado: o que foi digitado durante o commit fica.
  async function commit(then: CommitMode) {
    if (!canCommit) return;
    const sent = text;
    const msg = sent.trim();
    await withBusy('commit', async () => {
      if (await run(() => backend.gitCommit(msg), refreshWithGraph)) {
        setText((now) => (now === sent ? '' : now));
        if (then === 'commit-push') {
          setBusy('push');
          const pushed = await run(async () => {
            const out = await backend.gitPush();
            toast.success({ title: REMOTE.push.done, description: lastLine(out), duration: 3000 });
          }, refreshWithGraph);
          if (!pushed) toast.warning({ title: 'Commit criado, mas o envio falhou', description: 'Use Enviar no menu para tentar de novo. Não precisa commitar outra vez.', duration: 7000 });
        } else {
          toast.success({ title: 'Commit criado', duration: 3000 });
        }
      }
    });
  }

  // Preparar ou tirar do preparo: lista vazia vale para tudo. Deu certo, a marcação some junto com a linha.
  const stagePaths = (paths: string[], unstage: boolean) => withBusy('stage', async () => {
    if (await run(() => (unstage ? backend.gitUnstage(paths) : backend.gitStage(paths)), refresh)) setMarked(new Set());
  });
  const toggle = (key: keyof typeof collapsed) => setCollapsed((prev) => ({ ...prev, [key]: !prev[key] }));

  // Abre a revisão do lado escolhido. Conflito abre o conflito, e não o diff.
  const openRow = (f: GitFile, side: DiffSide, permanent: boolean) => {
    const target: TabTarget = f[side] === 'conflicted' ? { kind: 'conflict', path: f.path } : { kind: 'diff', side, path: f.path };
    onOpenTarget(target, { permanent });
  };
  const actions: RowActions = {
    pick: (f, side, ctrl) => {
      const key = keyOf(side, f.path);
      if (ctrl) {
        setMarked((prev) => {
          const next = new Set(prev);
          if (next.has(key)) next.delete(key);
          else next.add(key);
          return next;
        });
        return;
      }
      setMarked(new Set([key]));
      openRow(f, side, false);
    },
    review: (f, side) => openRow(f, side, true),
    openFile: (f) => onOpenFile(f.path, { permanent: true }),
    stage: (f, side, list) => {
      const batch = marked.has(keyOf(side, f.path)) ? list.filter((x) => marked.has(keyOf(side, x.path))) : [];
      const picked = batch.length > 1 ? batch : [f];
      void stagePaths(picked.flatMap(pathsOf), side === 'staged');
    },
    stageAll: (side) => void stagePaths([], side === 'staged'),
  };

  return (
    <Flexbox className="min-h-0" flex={1}>
      <Flexbox horizontal align="center" gap={2} paddingInline={12} height={36} flex="none">
        <Text as="h2" ellipsis weight={500} fontSize={12} type="secondary" className="min-w-0 flex-1">Controle do Código-Fonte</Text>
        <ActionIcon size="small" variant="borderless" icon={History} aria-label="Ver o histórico do projeto" title="Histórico do projeto" onClick={onShowHistory} />
        <ActionIcon size="small" variant="borderless" icon={RefreshCw} aria-label="Atualizar status" title="Atualizar status do Git" onClick={() => void refresh()} />
        <DropdownMenu items={remoteMenu} placement="bottomRight">
          <ActionIcon
            size="small" variant="borderless" icon={Ellipsis} loading={busy === 'fetch' || busy === 'pull' || busy === 'push'}
            aria-label="Mais opções: buscar, receber e enviar" title="Buscar, receber e enviar"
          />
        </DropdownMenu>
      </Flexbox>

      <Flexbox paddingInline={12} gap={2}>
        <Flexbox horizontal align="center" gap={6}>
          <BranchPicker branch={status.branch} locked={locked} busy={busy} withBusy={withBusy} refresh={refresh} />
          <Text
            as="span" fontSize={12} type="secondary" noWrap className="shrink-0 tabular-nums"
            title={`${commits(status.ahead)} a enviar, ${commits(status.behind)} a receber`}
          >
            <ArrowUp className="inline size-3 align-[-1px]" aria-hidden />{status.ahead}{' '}
            <ArrowDown className="inline size-3 align-[-1px]" aria-hidden />{status.behind}
          </Text>
        </Flexbox>
        {(busy || locked) && (
          <Text as="p" role="status" fontSize={12} type="secondary">{busy ? BUSY_TEXT[busy] : LOCKED_TITLE}</Text>
        )}
      </Flexbox>

      {/* Mensagem e botão nas medidas do Antigravity: campo de 26 px (cresce até 4 linhas), 4 px de folga, e o botão dividido de 28 px
          (ação com o ✓, divisor de 1 px e a seta que escolhe a ação), raio de 2 px; desabilitado fica a 40%. O texto usa o fundo do
          app, que contrasta com a primária do tema (clara no escuro, como no botão primário do lobehub). */}
      <Flexbox gap={4} paddingInline={12} paddingBlock={4} flex="none">
        <TextArea
          autoSize={{ minRows: 1, maxRows: 4 }} value={text} aria-label={`Mensagem do commit (Ctrl+Enter: ${MODE_LABEL[mode]})`}
          title={`Ctrl+Enter: ${MODE_LABEL[mode]}`} placeholder="Mensagem (Ctrl+Enter)"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.ctrlKey && e.key === 'Enter') {
              e.preventDefault();
              void commit(mode);
            }
          }}
          style={{ minHeight: 26, padding: '3px 6px', borderRadius: 4, fontSize: 13, lineHeight: '18px' }}
          classNames={{ input: 'select-text' }}
        />
        {/* Habilitado, na cor primária com o texto escuro do fundo; sem poder commitar, cinza com o texto secundário, legível (a
            opacidade de 40% do Antigravity some aqui, porque a primária do tema é clara e o texto escuro desaparece no fundo preto). */}
        <div
          className={`flex h-7 overflow-hidden rounded-[2px] ${canCommit
            ? 'bg-[var(--ant-color-primary)] text-[var(--ant-color-bg-layout)] [--hover:var(--ant-color-primary-hover)]'
            : 'bg-[var(--ant-color-fill-secondary)] text-[var(--ant-color-text-secondary)] [--hover:var(--ant-color-fill)]'}`}
        >
          <button
            type="button" disabled={!canCommit || busy === 'commit'} title={hint || MODE_LABEL[mode]} onClick={() => void commit(mode)}
            className="flex min-w-0 flex-1 items-center justify-center gap-1 px-1 text-[13px] outline-none hover:bg-[var(--hover)] focus-visible:ring-2 focus-visible:ring-current focus-visible:ring-inset disabled:cursor-default disabled:hover:bg-transparent"
          >
            {busy === 'commit' ? <LoaderCircle className="size-4 shrink-0 animate-spin" aria-hidden /> : <Check className="size-4 shrink-0" aria-hidden />}
            <span className="truncate">{MODE_LABEL[mode]}</span>
          </button>
          <span aria-hidden className="my-[5px] w-px bg-current opacity-40" />
          <DropdownMenu items={modeMenu} placement="bottomRight">
            <button
              type="button" aria-label={`Escolher a ação do commit (atual: ${MODE_LABEL[mode]})`} title="Escolher a ação do commit"
              className="flex w-[26px] shrink-0 items-center justify-center outline-none hover:bg-[var(--hover)] focus-visible:ring-2 focus-visible:ring-current focus-visible:ring-inset"
            >
              <ChevronDown className="size-4" aria-hidden />
            </button>
          </DropdownMenu>
        </div>
        {/* Sem nada preparado, a dica fica embaixo; com arquivos, a falta da mensagem já está no placeholder e na ação. */}
        {!staged.length && <Text as="p" fontSize={12} type="secondary">{hint}</Text>}
      </Flexbox>

      <Flexbox className="min-h-0 overflow-y-auto overflow-x-hidden" flex={1}>
        {/* Sem arquivos preparados, a seção some (como no VS Code); o estado de recolhido continua no ChangesPanel. */}
        {staged.length > 0 && (
          <FileSection
            title="Preparadas" side="staged" files={staged} collapsed={collapsed.staged} onToggle={() => toggle('staged')}
            busy={busy !== null} marked={marked} open={selected} actions={actions}
          />
        )}
        <FileSection
          title="Alterações" side="unstaged" files={unstaged} collapsed={collapsed.unstaged} onToggle={() => toggle('unstaged')}
          busy={busy !== null} marked={marked} open={selected} actions={actions}
        />
      </Flexbox>

      {/* Gráfico: aberto, divide a altura com a lista de arquivos pelo divisor; fechado, é só o cabeçalho no pé do painel. */}
      <GraphSection
        open={!collapsed.graph} onToggle={() => toggle('graph')} height={graphHeight} onHeight={setGraphHeight}
        head={status.head} stamp={[graphTick, status.head, status.branch, status.ahead, status.behind].join('|')}
        active={active} selected={selected} onOpenTarget={onOpenTarget} onRefresh={refreshGraph}
        remote={remoteActions} running={running} onRemote={(kind) => void remote(kind)}
      />
    </Flexbox>
  );
}

/**
 * Estado vazio do painel fora do modo `repo`, no padrão do VS Code: o título e, abaixo, o que fazer. Só `no-repo` tem botão (inicializa o repositório);
 * os outros dizem o motivo. Quando o Git muda por fora, a consulta seguinte troca o estado e o painel segue sem recarregar.
 */
function NoRepoPanel({ state, refresh }: { state: Exclude<GitState, { kind: 'repo' }>; refresh: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  // Com sucesso, o status relido vem como `repo` e este painel some sozinho. Erro vai em toast (`run`).
  const init = async () => {
    setBusy(true);
    await run(() => backend.gitInit(), refresh);
    setBusy(false);
  };
  const text = state.kind === 'no-repo'
    ? 'A pasta aberta não tem um repositório Git. Inicialize um para usar os recursos do Git aqui: alterações, commits, branches e histórico.'
    : state.kind === 'no-git'
      ? 'O Git não foi encontrado neste computador. Instale o Git e abra o app de novo.'
      : `Esta pasta está dentro do repositório ${state.root}, que não é a raiz do projeto. O painel Git só funciona com a raiz do repositório aberta como projeto.`;
  return (
    <Flexbox className="min-h-0" flex={1}>
      <Flexbox paddingInline={12} paddingBlock={6}>
        <Text as="h2" ellipsis weight={500} fontSize={12} type="secondary">Controle do Código-Fonte</Text>
      </Flexbox>
      <Flexbox gap={12} paddingInline={12} paddingBlock={8}>
        <Text as="p" fontSize={12} type="secondary">{text}</Text>
        {state.kind === 'no-repo' && (
          <Button type="primary" block loading={busy} disabled={busy} onClick={() => void init()}>Inicializar repositório</Button>
        )}
      </Flexbox>
    </Flexbox>
  );
}

/** Botão da branch atual e a escolha: lista das locais (a atual marcada) e um campo para criar e trocar. */
function BranchPicker({ branch, locked, busy, withBusy, refresh }: {
  branch: string;
  locked: boolean;
  busy: Busy | null;
  withBusy: WithBusy;
  refresh: () => Promise<void>;
}) {
  // Trocar e criar também param com qualquer operação em curso (commit, envio, preparo); o executor segue o `locked` de sempre.
  const off = locked || busy !== null;
  const [open, setOpen] = useState(false);
  const [branches, setBranches] = useState<GitBranches | null>(null);
  const [branchError, setBranchError] = useState('');
  const [name, setName] = useState('');

  // Lista lida ao abrir, para mostrar as branches atuais.
  useEffect(() => {
    if (!open) return;
    let alive = true;
    backend.gitBranches().then(
      (list) => alive && setBranches(list),
      (e: unknown) => alive && setBranchError(message(e)),
    );
    return () => {
      alive = false;
    };
  }, [open]);

  function changeOpen(next: boolean) {
    setOpen(next);
    if (next) {
      setBranches(null);
      setBranchError('');
      setName('');
    }
  }

  // Trocar ou criar: o popover fecha só se deu certo.
  async function choose(target: string) {
    if (target === branches?.current || off) return;
    if (await withBusy('branch', () => run(() => backend.gitSwitch(target), refresh))) setOpen(false);
  }

  async function create() {
    const target = name.trim();
    if (!target || off) return;
    if (await withBusy('branch', () => run(() => backend.gitCreateBranch(target), refresh))) setOpen(false);
  }

  // trigger="click": o padrão do Popover do Lobe UI é abrir com o mouse em cima.
  return (
    <Popover
      open={open} onOpenChange={changeOpen} trigger="click" placement="bottomLeft"
      className="w-64"
      content={
        <Flexbox gap={8} padding={8}>
          {locked && <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 4 }}>{LOCKED_TITLE}</Text>}
          {!locked && busy && <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 4 }}>{BUSY_TEXT[busy]}</Text>}
          {branchError && <Text as="p" role="status" fontSize={12} type="danger" style={{ paddingInline: 4 }}>{branchError}</Text>}
          {!branches && !branchError && <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 4 }}>Lendo as branches…</Text>}
          {branches && (
            <Flexbox gap={2} className="max-h-60 overflow-y-auto">
              {branches.local.map((item) => {
                const current = item === branches.current;
                return (
                  <Button
                    key={item} type="text" size="small" className="w-full justify-start! gap-2 font-mono"
                    icon={<Check className={`shrink-0${current ? '' : ' invisible'}`} />}
                    disabled={off} aria-current={current ? 'true' : undefined}
                    title={locked ? LOCKED_TITLE : busy ? BUSY_TEXT[busy] : current ? 'Branch atual' : `Trocar para ${item}`}
                    onClick={() => choose(item)}
                  >
                    <Text as="span" ellipsis className="min-w-0">{item}</Text>
                  </Button>
                );
              })}
            </Flexbox>
          )}
          <Flexbox as="form" horizontal gap={4} onSubmit={(e) => { e.preventDefault(); void create(); }}>
            <Input
              size="small" value={name} onChange={(e) => setName(e.target.value)} disabled={off}
              placeholder="Nova branch" aria-label="Nome da nova branch"
              className="min-w-0 flex-1 font-mono"
            />
            <Button type="primary" htmlType="submit" size="small" disabled={off || !name.trim()} title={locked ? LOCKED_TITLE : 'Cria a branch e troca para ela'}>
              Criar
            </Button>
          </Flexbox>
        </Flexbox>
      }
    >
      <Button type="text" size="small" icon={GitBranch} className="min-w-0! flex-1! justify-start! px-2! font-mono!" title={`Branch atual: ${branch}. Trocar ou criar uma branch`}>
        <Text as="span" ellipsis className="min-w-0">{branch}</Text>
      </Button>
    </Popover>
  );
}

/**
 * Uma seção do painel: cabeçalho com o chevron, o título e a contagem à direita, e ao lado as ações de tudo (irmãs, não aninhadas).
 * Recolhida, mostra só o cabeçalho. Vazia, mostra o aviso no lugar da lista. Cada linha tem o menu de contexto (clique direito ou Shift+F10).
 */
function FileSection({ title, side, files, collapsed, onToggle, busy, marked, open, actions }: {
  title: string;
  side: DiffSide;
  files: GitFile[];
  collapsed: boolean;
  onToggle: () => void;
  busy: boolean;
  marked: Set<string>;
  open: TabTarget | null | undefined;
  actions: RowActions;
}) {
  const staged = side === 'staged';
  const allLabel = staged ? 'Tirar todas do preparo' : 'Preparar todas';
  const markedHere = files.filter((f) => marked.has(keyOf(side, f.path))).length;
  return (
    <Flexbox as="section" gap={0}>
      {/* Cabeçalho de 22 px como o das seções do Antigravity: seta de 16 px com 2 px de margem, título de 12 px e o contador em pílula. */}
      <Flexbox horizontal align="center" gap={2} height={22} className="pr-2">
        <button
          type="button" onClick={onToggle} aria-expanded={!collapsed}
          className="flex h-full min-w-0 flex-1 items-center text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
        >
          <ChevronRight className={`mx-0.5 size-4 shrink-0 transition-transform${collapsed ? '' : ' rotate-90'}`} aria-hidden />
          <span className="min-w-0 shrink truncate text-[12px] font-medium">{title}</span>
          <span className="ml-auto shrink-0 rounded-full bg-[var(--ant-color-fill-secondary)] px-[5px] text-[11px] leading-[18px] tabular-nums">{files.length}</span>
        </button>
        <ActionIcon
          size="small" variant="borderless" icon={staged ? Minus : Plus} disabled={busy || !files.length}
          aria-label={allLabel} title={allLabel} onClick={() => actions.stageAll(side)}
        />
      </Flexbox>
      {!collapsed && (files.length ? (
        // A coluna não pode crescer com o nome: sem o minmax(0,1fr), o texto nowrap empurra a lista para fora da seção.
        <ul className="grid grid-cols-[minmax(0,1fr)]">
          {files.map((f) => {
            const kind = f[side] as GitKind;
            const { letter, title: word } = KIND[kind];
            const { name, dir } = splitPath(f.path);
            const full = f.origPath ? `${f.path} (renomeado de ${f.origPath})` : f.path;
            const key = keyOf(side, f.path);
            const isOpen = isOpenTarget(open, side, f);
            const isMarked = marked.has(key);
            const batch = markedHere > 1 && isMarked;
            const stageLabel = `${staged ? 'Tirar do preparo' : 'Preparar'}${batch ? ` (${markedHere})` : ''}`;
            const menu = [
              { key: 'review', label: kind === 'conflicted' ? 'Resolver conflito' : 'Revisar alterações', onClick: () => actions.review(f, side) },
              // Apagado não existe como arquivo atual: nada de abrir no editor.
              ...(kind === 'deleted' ? [] : [{ key: 'open', label: 'Abrir arquivo', onClick: () => actions.openFile(f) }]),
              { key: 'stage', label: stageLabel, disabled: busy, onClick: () => actions.stage(f, side, files) },
            ];
            return (
              <li key={f.path}>
                <ContextMenuTrigger items={menu}>
                  {/* Linha em três partes, sem nada por cima: a área de texto, o slot das ações (sempre reservado, só aparece no hover ou foco) e a letra do estado, à direita. */}
                  <div
                    className={`group flex h-[22px] items-center pr-2 hover:bg-muted${isOpen ? ' bg-muted shadow-[inset_2px_0_0_var(--ant-color-primary)]' : ''}${isMarked ? ' ring-1 ring-inset ring-ring/40' : ''}`}
                  >
                    <button
                      type="button"
                      onClick={(e) => actions.pick(f, side, e.ctrlKey || e.metaKey)}
                      onDoubleClick={() => actions.review(f, side)}
                      aria-current={isOpen ? 'true' : undefined}
                      title={`${full}: ${word}. Ver alterações (duplo clique abre de vez; Ctrl+clique marca)`}
                      className="flex h-full min-w-0 flex-1 items-center gap-1.5 pl-5 pr-1 text-left text-[13px] outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
                    >
                      {/* Nome e letra na cor do tipo de alteração (gitDecoration), como no Antigravity; a pasta em tamanho menor. */}
                      <FileIcon path={f.path} className="size-4" />
                      <span className="max-w-[70%] shrink-0 truncate" style={{ color: KIND_COLOR[kind] }}>{name}</span>
                      {dir && <span className="min-w-0 flex-1 truncate text-[0.9em] opacity-70">{dir}</span>}
                    </button>
                    {/* Largura fixa: o nome não salta no hover, e as ações nunca cobrem a letra. Sem pointer-events fora do hover, o clique passa; com opacity, o Tab alcança as ações. */}
                    <Flexbox
                      horizontal align="center" justify="flex-end" className="w-12 shrink-0 pointer-events-none opacity-0 transition-opacity group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100"
                    >
                      {kind !== 'deleted' && (
                        <ActionIcon
                          size="small" variant="borderless" icon={SquareArrowOutUpRight}
                          aria-label={`Abrir ${f.path} no editor`} title="Abrir no editor"
                          onClick={() => actions.openFile(f)}
                        />
                      )}
                      <ActionIcon
                        size="small" variant="borderless" icon={staged ? Minus : Plus} disabled={busy}
                        aria-label={`${stageLabel} ${f.path}`}
                        title={staged ? 'Tirar do preparo' : 'Preparar'}
                        onClick={() => actions.stage(f, side, files)}
                      />
                    </Flexbox>
                    <span title={word} className="w-4 shrink-0 text-center text-[12px]" style={{ color: KIND_COLOR[kind] }}>{letter}</span>
                  </div>
                </ContextMenuTrigger>
              </li>
            );
          })}
        </ul>
      ) : (
        <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 20, paddingBlock: 4 }}>Nenhuma alteração.</Text>
      ))}
    </Flexbox>
  );
}

/**
 * Barra lateral do Git: o controle de versão, ou o histórico (do projeto, ou do arquivo de `historyRequest`).
 * Os dois ficam montados: alternar não perde a mensagem do commit, os recolhidos nem a lista de commits.
 * Fora do modo `repo` não há nada disso: o painel é só o estado vazio, e nada consulta o Git.
 */
export function GitSidebar({ git, locked, guard, active, selected, onOpenTarget, onOpenFile, historyRequest }: GitSidebarProps) {
  const [view, setView] = useState<'changes' | 'history'>('changes');
  const [request, setRequest] = useState<HistoryRequest>({ path: null });

  // Pedido novo abre o histórico no escopo pedido. Sem pedido (a montagem), a barra abre nas alterações.
  useEffect(() => {
    if (!historyRequest) return;
    setRequest(historyRequest);
    setView('history');
  }, [historyRequest]);

  // O cabeçalho alterna para o histórico do projeto inteiro; de volta, a lista de mudanças.
  const showHistory = () => {
    setRequest({ path: null });
    setView('history');
  };

  const { data, error, refresh } = git;
  if (data && data.kind !== 'repo') {
    return (
      <Flexbox height="100%" className="min-h-0">
        <NoRepoPanel state={data} refresh={refresh} />
      </Flexbox>
    );
  }
  return (
    <Flexbox height="100%" className="min-h-0">
      <div className={view === 'changes' ? 'flex min-h-0 flex-1 flex-col' : 'hidden'}>
        <ChangesPanel
          status={data?.status ?? null} error={error} refresh={refresh} locked={locked} guard={guard} selected={selected}
          active={active && view === 'changes'} onOpenTarget={onOpenTarget} onOpenFile={onOpenFile} onShowHistory={showHistory}
        />
      </div>
      <div className={view === 'history' ? 'flex min-h-0 flex-1 flex-col' : 'hidden'}>
        <HistoryPanel
          request={request} active={active && view === 'history'} selected={selected}
          onOpenTarget={onOpenTarget} onShowChanges={() => setView('changes')}
        />
      </div>
    </Flexbox>
  );
}
