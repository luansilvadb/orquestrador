// A tela Git: à esquerda o painel (branch, commit, arquivos preparados e alterados); à direita as alterações
// do arquivo selecionado. Tudo roda na pasta do projeto e só por clique do usuário. O status atualiza sozinho
// (useGitStatus); cada ação atualiza na hora.
import { useEffect, useRef, useState, Fragment, type PointerEvent as ReactPointerEvent, type KeyboardEvent } from 'react';
import { toast } from 'sonner';
import { ArrowDown, ArrowUp, Check, GitBranch, LoaderCircle, Minus, Plus, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Textarea } from '@/components/ui/textarea';
import { cn } from '@/lib/utils';
import { backend, message } from './backend';
import { usePolling } from './polling';
import type { GitBranches, GitDiff, GitFile, GitKind, GitStatus } from './types';

export const LOCKED_TITLE = 'Há executores trabalhando. Espere o lote terminar.';
const KIND: Record<GitKind, { letter: string; title: string }> = {
  modified: { letter: 'M', title: 'modificado' },
  added: { letter: 'A', title: 'novo' },
  deleted: { letter: 'D', title: 'apagado' },
  renamed: { letter: 'R', title: 'renomeado' },
  untracked: { letter: 'U', title: 'não rastreado' },
  conflicted: { letter: 'C', title: 'em conflito' },
};
const SIGN = { add: '+', del: '-', context: ' ' };

// Erro de ação: a mensagem do backend em toast, selecionável (o body tem user-select: none).
const fail = (e: unknown) => toast.error(<span className="select-text">{message(e)}</span>, { duration: 7000 });
const lastLine = (text: string) => text.trim().split('\n').pop() ?? '';
const commits = (n: number) => `${n} ${n === 1 ? 'commit' : 'commits'}`;
// Caminhos de um arquivo para preparar ou tirar: renomeado leva o nome antigo junto.
const pathsOf = (f: GitFile) => (f.origPath ? [f.path, f.origPath] : [f.path]);

/** Status do Git, atualizado a cada 1,5 s. O estado só muda quando a resposta é diferente, para a lista não piscar. Sem projeto (`enabled` falso), não consulta. */
export function useGitStatus(enabled: boolean) {
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [error, setError] = useState('');
  const last = useRef('');
  const refresh = async () => {
    if (!enabled) return;
    try {
      const next = await backend.gitStatus();
      const text = JSON.stringify(next);
      setError('');
      if (text !== last.current) {
        last.current = text;
        setStatus(next);
      }
    } catch (e) {
      last.current = '';
      setStatus(null);
      setError(message(e));
    }
  };
  usePolling(refresh);
  return { status, error, refresh };
}

type Op = 'fetch' | 'pull' | 'push';
const REMOTE: Record<Op, { run: () => Promise<string>; done: string }> = {
  fetch: { run: () => backend.gitFetch(), done: 'Busca concluída' },
  pull: { run: () => backend.gitPull(), done: 'Commits recebidos' },
  push: { run: () => backend.gitPush(), done: 'Enviado' },
};

/** Arquivo selecionado na tela Git: o caminho e o lado (preparado ou não) da seção clicada. */
export type Selection = { path: string; staged: boolean };
const WIDTH = { initial: 240, min: 160, max: 600 };
const clampWidth = (v: number) => Math.min(WIDTH.max, Math.max(WIDTH.min, v));

/** Painel do Git: de cima para baixo, a linha da branch, o commit e as seções Preparadas e Alterações. */
export function GitPanel({ status, error, refresh, locked, selected, onSelect }: {
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
      <div className="grid min-h-0 flex-1 content-start p-3">
        <p role="status" className={cn('text-xs', error ? 'text-destructive' : 'text-muted-foreground')}>
          {error || 'Lendo o Git…'}
        </p>
      </div>
    );
  }

  const staged = status.files.filter((f) => f.staged);
  const unstaged = status.files.filter((f) => f.unstaged);
  const canCommit = !!text.trim() && staged.length > 0 && !committing;

  // Ação simples: erro em toast e o status relido logo em seguida.
  async function act(task: () => Promise<unknown>) {
    try {
      await task();
    } catch (e) {
      fail(e);
    } finally {
      await refresh();
    }
  }

  async function remote(kind: Op) {
    setOp(kind);
    try {
      const out = await REMOTE[kind].run();
      toast.success(REMOTE[kind].done, { description: lastLine(out), duration: 3000 });
    } catch (e) {
      fail(e);
    } finally {
      setOp(null);
      await refresh();
    }
  }

  async function commit() {
    if (!canCommit) return;
    setCommitting(true);
    try {
      await backend.gitCommit(text.trim());
      setText('');
    } catch (e) {
      fail(e);
    } finally {
      setCommitting(false);
      await refresh();
    }
  }

  const busy = op !== null;
  const commitHint = !staged.length ? 'Prepare arquivos para commitar' : !text.trim() ? 'Escreva a mensagem do commit' : 'Commita o que está preparado';

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="grid gap-1 px-3 py-2">
        <div className="flex items-center gap-1">
          <BranchPicker branch={status.branch} locked={locked} refresh={refresh} />
          <Button
            variant="outline" size="icon-sm" aria-label="Buscar" disabled={busy}
            title="Buscar (fetch): traz as informações do remoto, sem mexer nos arquivos"
            onClick={() => remote('fetch')}
          >
            {op === 'fetch' ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
          </Button>
          <Button
            variant="outline" size="sm" className="px-2 font-mono" disabled={busy || locked}
            aria-label={status.behind > 0 ? `Receber, ${commits(status.behind)}` : 'Receber'}
            title={locked ? LOCKED_TITLE : `Receber (pull): traz os commits do remoto${status.behind > 0 ? ` (${commits(status.behind)} a receber)` : ''}`}
            onClick={() => remote('pull')}
          >
            {op === 'pull' ? <LoaderCircle className="animate-spin" /> : <ArrowDown />}
            {status.behind > 0 && status.behind}
          </Button>
          <Button
            variant="outline" size="sm" className="px-2 font-mono" disabled={busy}
            aria-label={status.ahead > 0 ? `Enviar, ${commits(status.ahead)}` : 'Enviar'}
            title={`Enviar (push): envia os commits locais${status.ahead > 0 ? ` (${commits(status.ahead)} a enviar)` : ''}`}
            onClick={() => remote('push')}
          >
            {op === 'push' ? <LoaderCircle className="animate-spin" /> : <ArrowUp />}
            {status.ahead > 0 && status.ahead}
          </Button>
        </div>
        {locked && <p className="text-xs text-muted-foreground">{LOCKED_TITLE}</p>}
      </div>

      <div className="grid gap-2 border-y px-3 py-2">
        <Textarea
          rows={3} value={text} aria-label="Mensagem do commit"
          placeholder="Mensagem do commit (Ctrl+Enter commita)"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.ctrlKey && e.key === 'Enter') {
              e.preventDefault();
              void commit();
            }
          }}
          className="min-h-16 resize-y text-sm select-text"
        />
        <Button size="sm" className="justify-self-start" disabled={!canCommit} title={canCommit ? 'Commita o que está preparado' : commitHint} onClick={commit}>
          {committing && <LoaderCircle className="animate-spin" />}
          Commit
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto py-1 pr-3">
        <FileSection title="Preparadas" files={staged} staged onStage={(paths) => act(() => backend.gitUnstage(paths))} selected={selected} onSelect={onSelect} />
        <FileSection title="Alterações" files={unstaged} staged={false} onStage={(paths) => act(() => backend.gitStage(paths))} selected={selected} onSelect={onSelect} />
        {!staged.length && !unstaged.length && <p className="px-3 py-2 text-xs text-muted-foreground">Nenhuma alteração.</p>}
      </div>
    </div>
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

  async function choose(target: string) {
    if (target === branches?.current) return;
    try {
      await backend.gitSwitch(target);
      setOpen(false);
    } catch (e) {
      fail(e);
    } finally {
      await refresh();
    }
  }

  async function create() {
    const target = name.trim();
    if (!target || locked) return;
    try {
      await backend.gitCreateBranch(target);
      setOpen(false);
    } catch (e) {
      fail(e);
    } finally {
      await refresh();
    }
  }

  return (
    <Popover open={open} onOpenChange={changeOpen}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="min-w-0 flex-1 justify-start px-2 font-mono" title={`Branch atual: ${branch}. Trocar ou criar uma branch`}>
          <GitBranch />
          <span className="min-w-0 truncate">{branch}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="grid w-64 gap-2 p-2">
        {locked && <p className="px-1 text-xs text-muted-foreground">{LOCKED_TITLE}</p>}
        {branchError && <p role="status" className="px-1 text-xs text-destructive">{branchError}</p>}
        {!branches && !branchError && <p className="px-1 text-xs text-muted-foreground">Lendo as branches…</p>}
        {branches && (
          <div className="grid max-h-60 gap-0.5 overflow-y-auto">
            {branches.local.map((item) => {
              const current = item === branches.current;
              return (
                <Button
                  key={item} variant="ghost" size="sm" className="w-full justify-start gap-2 font-mono"
                  disabled={locked} aria-current={current ? 'true' : undefined}
                  title={locked ? LOCKED_TITLE : current ? 'Branch atual' : `Trocar para ${item}`}
                  onClick={() => choose(item)}
                >
                  <Check className={cn('shrink-0', !current && 'invisible')} />
                  <span className="min-w-0 truncate">{item}</span>
                </Button>
              );
            })}
          </div>
        )}
        <form className="flex gap-1" onSubmit={(e) => { e.preventDefault(); void create(); }}>
          <Input
            value={name} onChange={(e) => setName(e.target.value)} disabled={locked}
            placeholder="Nova branch" aria-label="Nome da nova branch"
            className="h-8 min-w-0 flex-1 font-mono"
          />
          <Button type="submit" size="sm" disabled={locked || !name.trim()} title={locked ? LOCKED_TITLE : 'Cria a branch e troca para ela'}>
            Criar
          </Button>
        </form>
      </PopoverContent>
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
    <section className="grid gap-0.5 px-1 pb-1">
      <div className="flex items-center gap-2 px-2 py-1">
        <h3 className="min-w-0 flex-1 truncate text-xs font-medium text-muted-foreground">{title} ({files.length})</h3>
        <Button variant="ghost" size="xs" onClick={() => onStage([])}>
          {staged ? 'tirar tudo' : 'preparar tudo'}
        </Button>
      </div>
      <ul className="grid">
        {files.map((f) => {
          const kind = (staged ? f.staged : f.unstaged) as GitKind;
          const { letter, title: word } = KIND[kind];
          const slash = f.path.lastIndexOf('/');
          const name = f.path.slice(slash + 1);
          const dir = slash >= 0 ? f.path.slice(0, slash) : '';
          const full = f.origPath ? `${f.path} (renomeado de ${f.origPath})` : f.path;
          const isSelected = selected?.path === f.path && selected.staged === staged;
          return (
            <li key={f.path} className={cn('flex items-center gap-1 rounded-sm hover:bg-muted', isSelected && 'bg-muted')}>
              <button
                type="button" onClick={() => onSelect(f.path, staged)} aria-current={isSelected ? 'true' : undefined}
                title={`${full}: ${word}. Ver alterações`}
                className="flex min-w-0 flex-1 items-center gap-2 rounded-sm py-1 pl-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                <span title={word} className="w-3 shrink-0 text-center font-mono text-xs font-semibold">{letter}</span>
                <span className="min-w-0 shrink truncate">{name}</span>
                {dir && <span className="min-w-0 shrink-2 truncate text-xs text-muted-foreground">{dir}</span>}
              </button>
              <Button
                variant="ghost" size="icon-xs"
                aria-label={`${staged ? 'Tirar do preparo' : 'Preparar'} ${f.path}`}
                title={staged ? 'Tirar do preparo' : 'Preparar'}
                onClick={() => onStage(pathsOf(f))}
              >
                {staged ? <Minus /> : <Plus />}
              </Button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** Tela Git: o painel à esquerda, com a borda direita arrastável, e à direita as alterações do arquivo selecionado. */
export function GitScreen({ git, locked, active }: { git: ReturnType<typeof useGitStatus>; locked: boolean; active: boolean }) {
  const [selected, setSelected] = useState<Selection | null>(null);
  const [width, setWidth] = useState(WIDTH.initial);
  const resize = useRef<{ x: number; width: number } | null>(null);
  const { status, error, refresh } = git;

  // Arquivo que sai da seção (commit, preparo ou desfazer) deixa de estar selecionado.
  useEffect(() => {
    if (!selected || !status) return;
    const listed = status.files.some((f) => f.path === selected.path && (selected.staged ? f.staged : f.unstaged));
    if (!listed) setSelected(null);
  }, [status, selected]);

  // A borda captura o ponteiro: o arrasto segue mesmo quando ele sai da faixa.
  const startResize = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    resize.current = { x: e.clientX, width };
  };
  const moveResize = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (resize.current) setWidth(clampWidth(resize.current.width + e.clientX - resize.current.x));
  };
  const endResize = () => {
    resize.current = null;
  };
  const keyResize = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    setWidth((w) => clampWidth(w + (e.key === 'ArrowLeft' ? -16 : 16)));
  };

  return (
    <div className="flex h-full min-h-0">
      <aside style={{ width }} className="relative flex min-h-0 shrink-0 flex-col border-r bg-card">
        <GitPanel
          status={status} error={error} refresh={refresh} locked={locked}
          selected={selected} onSelect={(path, staged) => setSelected({ path, staged })}
        />
        <div
          role="separator" aria-orientation="vertical" aria-label="Largura do painel"
          aria-valuemin={WIDTH.min} aria-valuemax={WIDTH.max} aria-valuenow={width}
          tabIndex={0} title="Arraste para mudar a largura"
          onPointerDown={startResize} onPointerMove={moveResize} onPointerUp={endResize} onPointerCancel={endResize}
          onKeyDown={keyResize}
          className="absolute inset-y-0 -right-1 z-10 w-2 cursor-col-resize outline-none hover:bg-ring/30 focus-visible:bg-ring/50"
        />
      </aside>
      <div className="h-full min-h-0 min-w-0 flex-1">
        {selected
          ? <DiffView key={`${selected.staged}:${selected.path}`} path={selected.path} staged={selected.staged} active={active} />
          : <p className="p-4 text-sm text-muted-foreground">Clique num arquivo para ver as alterações.</p>}
      </div>
    </div>
  );
}

/**
 * Alterações de um arquivo: uma coluna monoespaçada, com números antes e depois, sinal e rolagem.
 * Atualiza só enquanto a tela Git está visível (`active`); o que não mudou não é trocado, então a rolagem fica como está.
 */
export function DiffView({ path, staged, active }: { path: string; staged: boolean; active: boolean }) {
  const [diff, setDiff] = useState<GitDiff | null>(null);
  const [error, setError] = useState('');
  const last = useRef('');
  usePolling(async () => {
    if (!active) return;
    try {
      const next = await backend.gitDiff(path, staged);
      const text = JSON.stringify(next);
      setError('');
      if (text !== last.current) {
        last.current = text;
        setDiff(next);
      }
    } catch (e) {
      setError(message(e));
    }
  });

  const note = (text: string, tone = 'text-muted-foreground') => (
    <p role="status" className={cn('p-4 text-sm', tone)}>{text}</p>
  );
  let body;
  if (!diff) body = note(error || 'Lendo as alterações…', error ? 'text-destructive' : undefined);
  else if (diff.binary) body = note('Arquivo binário: não há linhas para mostrar.');
  else if (diff.tooLarge) body = note('Alteração grande demais para mostrar aqui.');
  else if (!diff.hunks.length) body = note('Sem alterações neste arquivo.');
  else {
    body = (
      <div className="h-full overflow-auto font-mono text-xs leading-5 select-text">
        <div className="w-max min-w-full py-1">
          {diff.hunks.map((hunk, i) => (
            <Fragment key={i}>
              <div className="min-w-full bg-muted/50 px-3 whitespace-pre text-muted-foreground">{hunk.header}</div>
              {hunk.lines.map((line, j) => (
                <div
                  key={j}
                  className={cn(
                    'grid min-w-full grid-cols-[4rem_4rem_1.5rem_1fr]',
                    line.kind === 'add' && 'bg-diff-add/15',
                    line.kind === 'del' && 'bg-diff-del/15',
                  )}
                >
                  <span className="px-2 text-right text-muted-foreground select-none">{line.old ?? ''}</span>
                  <span className="px-2 text-right text-muted-foreground select-none">{line.new ?? ''}</span>
                  <span
                    className={cn(
                      'text-center select-none',
                      line.kind === 'add' && 'text-diff-add',
                      line.kind === 'del' && 'text-diff-del',
                    )}
                  >
                    {SIGN[line.kind]}
                  </span>
                  <span className="pr-4 whitespace-pre">{line.text}</span>
                </div>
              ))}
            </Fragment>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-2 border-b bg-card px-3 py-1 text-xs">
        <code title={path} className="min-w-0 truncate text-muted-foreground">
          {path}{staged ? ' (preparado)' : ''}
        </code>
        {error && diff && <span role="status" className="shrink-0 text-destructive">{error}</span>}
      </div>
      <div className="min-h-0 flex-1">{body}</div>
    </div>
  );
}
