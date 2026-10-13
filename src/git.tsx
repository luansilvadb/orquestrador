// A tela Git: à esquerda o painel (branch, commit, arquivos preparados e alterados); à direita as alterações
// do arquivo selecionado. Tudo roda na pasta do projeto e só por clique do usuário. O status atualiza sozinho
// (usePolled, no App); cada ação atualiza na hora.
import { useEffect, useRef, useState } from 'react';
import { ActionIcon, Button, DraggablePanel, Input, Popover, Text, TextArea, toast } from '@lobehub/ui/base-ui';
import { Flexbox, PatchDiff } from '@lobehub/ui';
import { ArrowDown, ArrowUp, Check, GitBranch, Minus, Plus, RefreshCw } from 'lucide-react';
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

type Op = 'fetch' | 'pull' | 'push';
const REMOTE: Record<Op, { run: () => Promise<string>; done: string }> = {
  fetch: { run: () => backend.gitFetch(), done: 'Busca concluída' },
  pull: { run: () => backend.gitPull(), done: 'Commits recebidos' },
  push: { run: () => backend.gitPush(), done: 'Enviado' },
};

/** Arquivo selecionado na tela Git: o caminho e o lado (preparado ou não) da seção clicada. */
type Selection = { path: string; staged: boolean };

/** Painel do Git: de cima para baixo, a linha da branch, o commit e as seções Preparadas e Alterações. */
function GitPanel({ status, error, refresh, locked, selected, onSelect }: {
  status: GitStatus | null;
  error: string;
  refresh: () => Promise<void>;
  locked: boolean;
  selected: Selection | null;
  onSelect: (path: string, staged: boolean) => void;
}) {
  const [op, setOp] = useState<Op | null>(null);
  const [text, setText] = useState('');
  const [committing, setCommitting] = useState(false);

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
  const canCommit = !!text.trim() && staged.length > 0 && !committing;

  async function remote(kind: Op) {
    setOp(kind);
    await run(async () => {
      const out = await REMOTE[kind].run();
      toast.success({ title: REMOTE[kind].done, description: lastLine(out), duration: 3000 });
    }, refresh);
    setOp(null);
  }

  async function commit() {
    if (!canCommit) return;
    setCommitting(true);
    if (await run(() => backend.gitCommit(text.trim()), refresh)) setText('');
    setCommitting(false);
  }

  const busy = op !== null;
  const commitHint = !staged.length ? 'Prepare arquivos para commitar' : !text.trim() ? 'Escreva a mensagem do commit' : 'Commita o que está preparado';

  return (
    <Flexbox className="min-h-0" flex={1}>
      <Flexbox gap={4} paddingInline={12} paddingBlock={8}>
        <Flexbox horizontal align="center" gap={4}>
          <BranchPicker branch={status.branch} locked={locked} refresh={refresh} />
          <ActionIcon
            variant="outlined" size="small" icon={RefreshCw} spin={op === 'fetch'} aria-label="Buscar" disabled={busy}
            title="Buscar (fetch): traz as informações do remoto, sem mexer nos arquivos"
            onClick={() => remote('fetch')}
          />
          <Button
            size="small" className="px-2 font-mono" icon={ArrowDown} loading={op === 'pull'} disabled={busy || locked}
            aria-label={status.behind > 0 ? `Receber, ${commits(status.behind)}` : 'Receber'}
            title={locked ? LOCKED_TITLE : `Receber (pull): traz os commits do remoto${status.behind > 0 ? ` (${commits(status.behind)} a receber)` : ''}`}
            onClick={() => remote('pull')}
          >
            {status.behind > 0 && status.behind}
          </Button>
          <Button
            size="small" className="px-2 font-mono" icon={ArrowUp} loading={op === 'push'} disabled={busy}
            aria-label={status.ahead > 0 ? `Enviar, ${commits(status.ahead)}` : 'Enviar'}
            title={`Enviar (push): envia os commits locais${status.ahead > 0 ? ` (${commits(status.ahead)} a enviar)` : ''}`}
            onClick={() => remote('push')}
          >
            {status.ahead > 0 && status.ahead}
          </Button>
        </Flexbox>
        {locked && <Text as="p" fontSize={12} type="secondary">{LOCKED_TITLE}</Text>}
      </Flexbox>

      <Flexbox className="border-y" gap={8} paddingInline={12} paddingBlock={8}>
        <TextArea
          rows={3} resize value={text} aria-label="Mensagem do commit"
          placeholder="Mensagem do commit (Ctrl+Enter commita)"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.ctrlKey && e.key === 'Enter') {
              e.preventDefault();
              void commit();
            }
          }}
          classNames={{ input: 'text-sm select-text' }}
        />
        <Button
          type="primary" size="small" style={{ alignSelf: 'flex-start' }} disabled={!canCommit} loading={committing}
          title={commitHint} onClick={commit}
        >
          Commit
        </Button>
      </Flexbox>

      <Flexbox className="min-h-0 overflow-y-auto" flex={1} paddingBlock={4} style={{ paddingRight: 12 }}>
        <FileSection title="Preparadas" files={staged} staged onStage={(paths) => run(() => backend.gitUnstage(paths), refresh)} selected={selected} onSelect={onSelect} />
        <FileSection title="Alterações" files={unstaged} staged={false} onStage={(paths) => run(() => backend.gitStage(paths), refresh)} selected={selected} onSelect={onSelect} />
        {!staged.length && !unstaged.length && <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 12, paddingBlock: 8 }}>Nenhuma alteração.</Text>}
      </Flexbox>
    </Flexbox>
  );
}

/** Botão da branch atual e a escolha: lista das locais (a atual marcada) e um campo para criar e trocar. */
function BranchPicker({ branch, locked, refresh }: { branch: string; locked: boolean; refresh: () => Promise<void> }) {
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
    if (target === branches?.current) return;
    if (await run(() => backend.gitSwitch(target), refresh)) setOpen(false);
  }

  async function create() {
    const target = name.trim();
    if (!target || locked) return;
    if (await run(() => backend.gitCreateBranch(target), refresh)) setOpen(false);
  }

  // trigger="click": o padrão do Popover do Lobe UI é abrir com o mouse em cima.
  return (
    <Popover
      open={open} onOpenChange={changeOpen} trigger="click" placement="bottomLeft"
      className="w-64"
      content={
        <Flexbox gap={8} padding={8}>
          {locked && <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 4 }}>{LOCKED_TITLE}</Text>}
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
                    disabled={locked} aria-current={current ? 'true' : undefined}
                    title={locked ? LOCKED_TITLE : current ? 'Branch atual' : `Trocar para ${item}`}
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
              size="small" value={name} onChange={(e) => setName(e.target.value)} disabled={locked}
              placeholder="Nova branch" aria-label="Nome da nova branch"
              className="min-w-0 flex-1 font-mono"
            />
            <Button type="primary" htmlType="submit" size="small" disabled={locked || !name.trim()} title={locked ? LOCKED_TITLE : 'Cria a branch e troca para ela'}>
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

/** Uma seção do painel: cabeçalho com a contagem e o botão de tudo, e uma linha por arquivo. Vazia some. */
function FileSection({ title, files, staged, onStage, selected, onSelect }: {
  title: string;
  files: GitFile[];
  staged: boolean;
  onStage: (paths: string[]) => void;
  selected: Selection | null;
  onSelect: (path: string, staged: boolean) => void;
}) {
  if (!files.length) return null;
  return (
    <Flexbox as="section" gap={2} paddingInline={4} paddingBlock="0 4px">
      <Flexbox horizontal align="center" gap={8} paddingInline={8} paddingBlock={4}>
        <Text as="h3" ellipsis weight={500} fontSize={12} type="secondary" className="min-w-0 flex-1">{title} ({files.length})</Text>
        <Button type="text" size="small" onClick={() => onStage([])}>
          {staged ? 'tirar tudo' : 'preparar tudo'}
        </Button>
      </Flexbox>
      {/* A coluna não pode crescer com o nome: sem o minmax(0,1fr), o texto nowrap empurra a lista para fora da seção. */}
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
            <li key={f.path} className={`flex items-center gap-1 rounded-sm hover:bg-muted${isSelected ? ' bg-muted' : ''}`}>
              <button
                type="button" onClick={() => onSelect(f.path, staged)} aria-current={isSelected ? 'true' : undefined}
                title={`${full}: ${word}. Ver alterações`}
                className="flex min-w-0 flex-1 items-center gap-2 rounded-sm py-1 pl-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                <Text as="span" code strong fontSize={12} title={word} className="w-3 shrink-0 text-center">{letter}</Text>
                <Text as="span" fontSize={14} ellipsis className="min-w-0 shrink">{name}</Text>
                {dir && <Text as="span" type="secondary" fontSize={12} ellipsis className="min-w-0 shrink-2">{dir}</Text>}
              </button>
              <ActionIcon
                variant="borderless" size="small" icon={staged ? Minus : Plus}
                aria-label={`${staged ? 'Tirar do preparo' : 'Preparar'} ${f.path}`}
                title={staged ? 'Tirar do preparo' : 'Preparar'}
                onClick={() => onStage(pathsOf(f))}
              />
            </li>
          );
        })}
      </ul>
    </Flexbox>
  );
}

/** Tela Git: o painel à esquerda, com a borda direita arrastável, e à direita as alterações do arquivo selecionado. */
export function GitScreen({ git, locked, active }: { git: ReturnType<typeof usePolled<GitStatus>>; locked: boolean; active: boolean }) {
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
        placement="left" minWidth={160} maxWidth={600} defaultSize={{ width: 240 }} expandable={false}
        backgroundColor="var(--ant-color-bg-container)"
      >
        <Flexbox height="100%">
          <GitPanel
            status={data} error={error} refresh={refresh} locked={locked}
            selected={selected} onSelect={(path, staged) => setSelected({ path, staged })}
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
