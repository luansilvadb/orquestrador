// A tela Git: à esquerda o painel de controle de versão (cabeçalho, branch, mensagem com o commit e as seções Preparadas
// e Alterações); à direita as alterações do arquivo selecionado. Tudo roda na pasta do projeto e só por clique do usuário.
// O status atualiza sozinho (usePolled, no App); cada ação atualiza na hora.
import { useEffect, useRef, useState } from 'react';
import { ActionIcon, Button, DraggablePanel, DropdownMenu, Input, Popover, Text, TextArea, toast } from '@lobehub/ui/base-ui';
import { Flexbox, PatchDiff } from '@lobehub/ui';
import { ArrowDown, ArrowUp, Check, ChevronDown, ChevronRight, Ellipsis, File, GitBranch, Minus, Plus, RefreshCw, SquareArrowOutUpRight } from 'lucide-react';
import { backend, message } from './backend';
import { usePolling } from './polling';
import type { GitBranches, GitDiff, GitFile, GitKind, GitStatus } from './types';

const LOCKED_TITLE = 'Há executores trabalhando. Espere o lote terminar.';
const KIND: Record<GitKind, { letter: string; title: string }> = {
  modified: { letter: 'M', title: 'modificado' },
  added: { letter: 'A', title: 'novo' },
  deleted: { letter: 'D', title: 'apagado' },
  renamed: { letter: 'R', title: 'renomeado' },
  untracked: { letter: 'U', title: 'não rastreado' },
  conflicted: { letter: 'C', title: 'em conflito' },
};

// Erro de ação: a mensagem do backend em toast, selecionável (o body tem user-select: none).
const fail = (e: unknown) => toast.error({ title: <span className="select-text">{message(e)}</span>, duration: 7000 });
const lastLine = (text: string) => text.trim().split('\n').pop() ?? '';
const commits = (n: number) => `${n} ${n === 1 ? 'commit' : 'commits'}`;
// Caminhos de um arquivo para preparar ou tirar: renomeado leva o nome antigo junto.
const pathsOf = (f: GitFile) => (f.origPath ? [f.path, f.origPath] : [f.path]);
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
 * `enabled` falso não consulta; `clearOnError` zera o dado quando a consulta falha, em vez de manter o anterior.
 */
export function usePolled<T>(read: () => Promise<T>, enabled: boolean, clearOnError: boolean) {
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
      if (clearOnError) {
        last.current = '';
        setData(null);
      }
      setError(message(e));
    }
  };
  usePolling(refresh);
  return { data, error, refresh };
}

type Remote = 'fetch' | 'pull' | 'push';
// Operação em curso. O commit com push passa por 'commit' e depois por 'push'.
// stage e branch entram na mesma trava: enquanto uma roda, nada mexe no índice nem na branch.
type Busy = Remote | 'commit' | 'stage' | 'branch';
type CommitMode = 'commit' | 'commit-push';
type WithBusy = <T>(kind: Busy, task: () => Promise<T>) => Promise<T | undefined>;
const REMOTE: Record<Remote, { run: () => Promise<string>; done: string }> = {
  fetch: { run: () => backend.gitFetch(), done: 'Busca concluída' },
  pull: { run: () => backend.gitPull(), done: 'Commits recebidos' },
  push: { run: () => backend.gitPush(), done: 'Enviado' },
};
const BUSY_TEXT: Record<Busy, string> = {
  fetch: 'Buscando…', pull: 'Recebendo…', push: 'Enviando…', commit: 'Commitando…', stage: 'Atualizando o preparo…', branch: 'Mudando de branch…',
};
const MODE_LABEL: Record<CommitMode, string> = { commit: 'Commit', 'commit-push': 'Commit e Push' };

/** Arquivo selecionado na tela Git: o caminho e o lado (preparado ou não) da seção clicada. */
type Selection = { path: string; staged: boolean };

/** Painel do Git: cabeçalho com atualizar e o menu de Buscar, Receber e Enviar; a branch; o commit; e as duas seções. */
function GitPanel({ status, error, refresh, locked, selected, onSelect, onOpenFile }: {
  status: GitStatus | null;
  error: string;
  refresh: () => Promise<void>;
  locked: boolean;
  selected: Selection | null;
  onSelect: (path: string, staged: boolean) => void;
  onOpenFile: (path: string) => void;
}) {
  const [busy, setBusy] = useState<Busy | null>(null);
  // O segundo clique chega antes de o estado (busy) mudar: a trava que vale é a do ref.
  const busyRef = useRef(false);
  const [text, setText] = useState('');
  const [mode, setMode] = useState<CommitMode>('commit');
  // Recolhido fica aqui, no painel, e não na seção: a seção some quando a lista esvazia, e o estado não pode sumir junto.
  const [collapsed, setCollapsed] = useState({ staged: false, unstaged: false });

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
  const remoteMenu = [
    { key: 'fetch', label: 'Buscar', disabled: busy !== null, onClick: () => void remote('fetch') },
    { key: 'pull', label: status.behind > 0 ? `Receber (${commits(status.behind)})` : 'Receber', disabled: busy !== null || locked, onClick: () => void remote('pull') },
    { key: 'push', label: status.ahead > 0 ? `Enviar (${commits(status.ahead)})` : 'Enviar', disabled: busy !== null, onClick: () => void remote('push') },
  ];
  // A escolha só muda o modo do botão e do Ctrl+Enter: nada roda aqui.
  const modeMenu = (['commit', 'commit-push'] as const).map((key) => ({
    key, label: MODE_LABEL[key], icon: mode === key ? <Check /> : undefined, onClick: () => setMode(key),
  }));

  // Uma operação por vez, com a mesma trava do ref. Libera no finally: sucesso ou erro, a tela volta a aceitar ações.
  const withBusy: WithBusy = async (kind, task) => {
    if (busyRef.current) return undefined;
    busyRef.current = true;
    setBusy(kind);
    try {
      return await task();
    } finally {
      busyRef.current = false;
      setBusy(null);
    }
  };

  const remote = (kind: Remote) => withBusy(kind, () => run(async () => {
    const out = await REMOTE[kind].run();
    toast.success({ title: REMOTE[kind].done, description: lastLine(out), duration: 3000 });
  }, refresh));

  // Commit, e depois o push. O push só sai se o commit deu certo: commit com erro mantém a mensagem e não envia.
  // Commit feito limpa a mensagem mesmo se o envio falhar, para ninguém commitar o mesmo texto de novo; Enviar no menu tenta só o envio.
  // A limpeza só vale se o texto ainda é o enviado: o que foi digitado durante o commit fica.
  async function commit(then: CommitMode) {
    if (!canCommit) return;
    const sent = text;
    const msg = sent.trim();
    await withBusy('commit', async () => {
      if (await run(() => backend.gitCommit(msg), refresh)) {
        setText((now) => (now === sent ? '' : now));
        if (then === 'commit-push') {
          setBusy('push');
          const pushed = await run(async () => {
            const out = await backend.gitPush();
            toast.success({ title: REMOTE.push.done, description: lastLine(out), duration: 3000 });
          }, refresh);
          if (!pushed) toast.warning({ title: 'Commit criado, mas o envio falhou', description: 'Use Enviar no menu para tentar de novo. Não precisa commitar outra vez.', duration: 7000 });
        } else {
          toast.success({ title: 'Commit criado', duration: 3000 });
        }
      }
    });
  }

  // Preparar ou tirar do preparo: lista vazia vale para tudo.
  const stage = (paths: string[], unstage: boolean) => withBusy('stage', () => run(() => (unstage ? backend.gitUnstage(paths) : backend.gitStage(paths)), refresh));
  const toggle = (key: keyof typeof collapsed) => setCollapsed((prev) => ({ ...prev, [key]: !prev[key] }));

  return (
    <Flexbox className="min-h-0" flex={1}>
      <Flexbox horizontal align="center" gap={2} paddingInline={12} paddingBlock={6}>
        <Text as="h2" ellipsis weight={500} fontSize={12} type="secondary" className="min-w-0 flex-1">Controle de versão</Text>
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

      <Flexbox className="border-y" gap={6} paddingInline={12} paddingBlock={8}>
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
          classNames={{ input: 'text-sm select-text' }}
        />
        <Flexbox horizontal align="center" gap={2}>
          <Button
            type="primary" size="small" className="min-w-0 flex-1!" disabled={!canCommit} loading={busy === 'commit'}
            title={hint || MODE_LABEL[mode]} onClick={() => void commit(mode)}
          >
            {MODE_LABEL[mode]}
          </Button>
          <DropdownMenu items={modeMenu} placement="bottomRight">
            <ActionIcon size="small" variant="outlined" icon={ChevronDown} aria-label={`Escolher a ação do commit (atual: ${MODE_LABEL[mode]})`} title="Escolher a ação do commit" />
          </DropdownMenu>
        </Flexbox>
        {/* Sem nada preparado, a dica fica embaixo; com arquivos, a falta da mensagem já está no placeholder e na ação. */}
        {!staged.length && <Text as="p" fontSize={12} type="secondary">{hint}</Text>}
      </Flexbox>

      <Flexbox className="min-h-0 overflow-y-auto overflow-x-hidden" flex={1} paddingBlock={4} style={{ paddingRight: 12 }}>
        {/* Sem arquivos preparados, a seção some (como no VS Code); o estado de recolhido continua no GitPanel. */}
        {staged.length > 0 && (
          <FileSection
            title="Preparadas" files={staged} staged collapsed={collapsed.staged} onToggle={() => toggle('staged')}
            busy={busy !== null} onStage={(paths) => stage(paths, true)} selected={selected} onSelect={onSelect} onOpenFile={onOpenFile}
          />
        )}
        <FileSection
          title="Alterações" files={unstaged} staged={false} collapsed={collapsed.unstaged} onToggle={() => toggle('unstaged')}
          busy={busy !== null} onStage={(paths) => stage(paths, false)} selected={selected} onSelect={onSelect} onOpenFile={onOpenFile}
        />
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
 * Recolhida, mostra só o cabeçalho. Vazia, mostra o aviso no lugar da lista.
 */
function FileSection({ title, files, staged, collapsed, onToggle, busy, onStage, selected, onSelect, onOpenFile }: {
  title: string;
  files: GitFile[];
  staged: boolean;
  collapsed: boolean;
  onToggle: () => void;
  busy: boolean;
  onStage: (paths: string[]) => void;
  selected: Selection | null;
  onSelect: (path: string, staged: boolean) => void;
  onOpenFile: (path: string) => void;
}) {
  const allLabel = staged ? 'Tirar todas do preparo' : 'Preparar todas';
  return (
    <Flexbox as="section" gap={0}>
      <Flexbox horizontal align="center" gap={2} paddingInline={4}>
        <button
          type="button" onClick={onToggle} aria-expanded={!collapsed}
          className="flex min-w-0 flex-1 items-center gap-1 rounded-sm py-1 pl-1 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          {collapsed
            ? <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
            : <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />}
          <Text as="span" ellipsis weight={600} fontSize={12} className="min-w-0 shrink">{title}</Text>
          <Text as="span" fontSize={11} type="secondary" className="ml-auto shrink-0 rounded-full bg-muted px-1.5 tabular-nums">{files.length}</Text>
        </button>
        <ActionIcon
          size="small" variant="borderless" icon={staged ? Minus : Plus} disabled={busy || !files.length}
          aria-label={allLabel} title={allLabel} onClick={() => onStage([])}
        />
      </Flexbox>
      {!collapsed && (files.length ? (
        // A coluna não pode crescer com o nome: sem o minmax(0,1fr), o texto nowrap empurra a lista para fora da seção.
        <ul className="grid grid-cols-[minmax(0,1fr)]">
          {files.map((f) => {
            const kind = (staged ? f.staged : f.unstaged) as GitKind;
            const { letter, title: word } = KIND[kind];
            const slash = f.path.lastIndexOf('/');
            const name = f.path.slice(slash + 1);
            const dir = slash >= 0 ? f.path.slice(0, slash) : '';
            const full = f.origPath ? `${f.path} (renomeado de ${f.origPath})` : f.path;
            const isSelected = selected?.path === f.path && selected.staged === staged;
            return (
              // Linha em três partes, sem nada por cima: a área de texto, o slot das ações (sempre reservado, só aparece no hover ou foco) e a letra do estado, à direita.
              <li
                key={f.path}
                className={`group flex h-6 items-center rounded-sm hover:bg-muted${isSelected ? ' bg-muted shadow-[inset_2px_0_0_var(--ant-color-primary)]' : ''}`}
              >
                <button
                  type="button" onClick={() => onSelect(f.path, staged)} aria-current={isSelected ? 'true' : undefined}
                  title={`${full}: ${word}. Ver alterações`}
                  className="flex h-full min-w-0 flex-1 items-center gap-1.5 rounded-sm pl-6 pr-1 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                >
                  <File className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
                  <Text as="span" fontSize={13} ellipsis className="max-w-[70%] shrink-0">{name}</Text>
                  {dir && <Text as="span" type="secondary" fontSize={12} ellipsis className="min-w-0 flex-1">{dir}</Text>}
                </button>
                {/* Largura fixa: o nome não salta no hover, e as ações nunca cobrem a letra. Sem pointer-events fora do hover, o clique passa; com opacity, o Tab alcança as ações. */}
                <Flexbox
                  horizontal align="center" justify="flex-end" className="w-12 shrink-0 pointer-events-none opacity-0 transition-opacity group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100"
                >
                  {kind !== 'deleted' && (
                    <ActionIcon
                      size="small" variant="borderless" icon={SquareArrowOutUpRight}
                      aria-label={`Abrir ${f.path} no editor`} title="Abrir no editor"
                      onClick={() => onOpenFile(f.path)}
                    />
                  )}
                  <ActionIcon
                    size="small" variant="borderless" icon={staged ? Minus : Plus} disabled={busy}
                    aria-label={`${staged ? 'Tirar do preparo' : 'Preparar'} ${f.path}`}
                    title={staged ? 'Tirar do preparo' : 'Preparar'}
                    onClick={() => onStage(pathsOf(f))}
                  />
                </Flexbox>
                <Text as="span" title={word} fontSize={12} weight={600} type="secondary" className="w-5 shrink-0 pr-1 text-center font-mono">{letter}</Text>
              </li>
            );
          })}
        </ul>
      ) : (
        <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 24, paddingBlock: 4 }}>Nenhuma alteração.</Text>
      ))}
    </Flexbox>
  );
}

/**
 * Tela Git: o painel à esquerda, com a borda direita arrastável, na mesma largura da faixa de arquivos (o App guarda a largura);
 * à direita, as alterações do arquivo selecionado.
 */
export function GitScreen({ git, locked, active, width, minWidth, maxWidth, onWidthChange, onOpenFile }: {
  git: ReturnType<typeof usePolled<GitStatus>>;
  locked: boolean;
  active: boolean;
  width: number;
  minWidth: number;
  maxWidth?: number;
  onWidthChange: (width: number) => void;
  onOpenFile: (path: string) => void;
}) {
  const [selected, setSelected] = useState<Selection | null>(null);
  const { data, error, refresh } = git;

  // Arquivo que sai da seção (commit, preparo ou desfazer) deixa de estar selecionado.
  useEffect(() => {
    if (!selected || !data) return;
    const listed = data.files.some((f) => f.path === selected.path && (selected.staged ? f.staged : f.unstaged));
    if (!listed) setSelected(null);
  }, [data, selected]);

  return (
    <Flexbox horizontal height="100%" className="min-h-0">
      <DraggablePanel
        placement="left" minWidth={minWidth} maxWidth={maxWidth} size={{ width }} expandable={false} showHandleWideArea={false}
        onSizeChange={(_, size) => typeof size?.width === 'number' && onWidthChange(size.width)}
        backgroundColor="var(--ant-color-bg-layout)"
      >
        <Flexbox height="100%">
          <GitPanel
            status={data} error={error} refresh={refresh} locked={locked}
            selected={selected} onSelect={(path, staged) => setSelected({ path, staged })} onOpenFile={onOpenFile}
          />
        </Flexbox>
      </DraggablePanel>
      <Flexbox flex={1} height="100%" className="min-h-0 min-w-0">
        {selected
          ? <DiffView key={`${selected.staged}:${selected.path}`} path={selected.path} staged={selected.staged} active={active} />
          : <Text as="p" fontSize={14} type="secondary" style={{ padding: 16 }}>Clique num arquivo para ver as alterações.</Text>}
      </Flexbox>
    </Flexbox>
  );
}

/**
 * Alterações de um arquivo: o patch unificado, mostrado pelo PatchDiff do Lobe UI, com rolagem.
 * Atualiza só enquanto a tela Git está visível (`active`); o que não mudou não é trocado, então a rolagem fica como está.
 */
function DiffView({ path, staged, active }: { path: string; staged: boolean; active: boolean }) {
  const { data: diff, error } = usePolled<GitDiff>(() => backend.gitDiff(path, staged), active, false);

  const note = (text: string, type: 'danger' | 'secondary' | undefined = 'secondary') => (
    <Flexbox padding={16}>
      <Text as="p" role="status" fontSize={14} type={type}>{text}</Text>
    </Flexbox>
  );
  let body;
  if (!diff) body = note(error || 'Lendo as alterações…', error ? 'danger' : undefined);
  else if (diff.binary) body = note('Arquivo binário: não há linhas para mostrar.');
  else if (diff.tooLarge) body = note('Alteração grande demais para mostrar aqui.');
  else if (!diff.patch) body = note('Sem alterações neste arquivo.');
  else {
    body = (
      <Flexbox height="100%" className="overflow-auto select-text">
        <PatchDiff patch={diff.patch} viewMode="unified" showHeader={false} variant="borderless" />
      </Flexbox>
    );
  }

  return (
    <Flexbox height="100%" className="min-h-0">
      <Flexbox horizontal align="center" gap={8} paddingInline={12} paddingBlock={4} className="border-b bg-card">
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={path} className="min-w-0">
          {path}{staged ? ' (preparado)' : ''}
        </Text>
        {error && diff && <Text as="span" role="status" type="danger" fontSize={12} className="shrink-0">{error}</Text>}
      </Flexbox>
      <Flexbox flex={1} className="min-h-0">{body}</Flexbox>
    </Flexbox>
  );
}
