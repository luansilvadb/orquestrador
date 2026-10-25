// Vistas de comparação do editor central: a comparação disco × edição antes de gravar, o diff staged/unstaged e o commit do histórico (só
// leitura), ambos com o arquivo inteiro num comparador contínuo. `Common` e `Ask` são o que o workspace entrega às abas de Git.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentProps, type KeyboardEvent, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { ActionIcon, Button, Text, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { getFiletypeFromFileName, parsePatchFiles } from '@pierre/diffs';
import { FileDiff, type FileDiffMetadata } from '@pierre/diffs/react';
import { ArrowRight, ChevronDown, ChevronUp, Columns2, FileText, Minus, Plus, Rows2, Undo2 } from 'lucide-react';
import { message } from '../backend/backend';
import { useBackend } from '../backend/backend-context';
import { usePolling } from '../shared/polling';
import { Note } from './media-view';
import { targetKey, type TabTarget } from './workspace-model';
import { normalizeNewlines, patchHunks, type PatchHunk } from './workspace-store';
import type { GitGuard } from '../git/git-guard';
import type { GitDiff, HunkRef, ReviewDiff } from '../backend/types';
import { blockRanges, changeKeys, compareOf, type CompareModel } from './compare-rows';

export type Ask = { title: string; body: ReactNode; actions: { key: string; label: string; danger?: boolean }[] };
export type Common = {
  active: boolean;
  locked: boolean;
  guard: GitGuard;
  refreshGit: () => void;
  openLine: (path: string, line: number) => void;
  hasDraft: (path: string) => boolean;
  // Só o buffer do arquivo (não o rascunho de conflito): é o que impede gravar a resolução por cima de uma edição ainda não salva.
  fileDirty: (path: string) => boolean;
  ask: (ask: Ask) => Promise<string | null>;
};

export const POLL_MS = 2500;

export type GitTabReadResult = { review: ReviewDiff } | { diff: GitDiff } | { error: string };
export type GitTabRead = { promise: Promise<GitTabReadResult>; result?: GitTabReadResult; patch?: string; fileDiff?: FileDiffMetadata; prefetchAt?: number; timer: number; consume(): void };

export function gitTabFileDiff(target: Extract<TabTarget, { kind: 'diff' | 'commit' }>, patch: string, revision?: string) {
  return parsePatchFiles(patch, `git-tab:${targetKey(target)}:${revision ?? ''}`)[0]?.files[0] ?? null;
}

export function canHighlightGitTabDiff(fileDiff: FileDiffMetadata) {
  return (fileDiff.lang ?? getFiletypeFromFileName(fileDiff.name)) !== 'text';
}

export function Reading({ text }: { text: string }) {
  const [late, setLate] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setLate(true), 200);
    return () => clearTimeout(timer);
  }, []);
  return <Flexbox flex={1} className="min-h-0 bg-[#1F1F1F]">{late && <Note text={text} />}</Flexbox>;
}

// Acima disso a comparação não é desenhada (duas áreas de texto gigantes travam a tela). A confirmação continua valendo.
const COMPARE_CHARS = 200_000;

// As duas versões lado a lado, como estavam quando o usuário pediu. A contagem compara linha a linha pela posição: é um indicador,
// não um diff. O texto do disco é o que a confirmação grava de volta como base, então ele precisa estar à vista.
export function CompareView({ path, disk, draft, mode }: { path: string; disk: string; draft: string; mode: 'keep' | 'use' }) {
  const big = disk.length > COMPARE_CHARS || draft.length > COMPARE_CHARS;
  const diskText = normalizeNewlines(disk);
  const diskLines = diskText.split('\n');
  const draftLines = draft.split('\n');
  let differ = 0;
  for (let i = 0; i < Math.max(diskLines.length, draftLines.length); i++) if (diskLines[i] !== draftLines[i]) differ++;
  return (
    <Flexbox gap={12}>
      <Text as="p" fontSize={14} type="secondary" style={{ margin: 0, overflowWrap: 'anywhere' }}>
        {mode === 'use'
          ? `${path}: sua edição será descartada e o texto do disco entra no lugar.`
          : `${path}: o disco passa a ter a sua edição. Se o arquivo mudar de novo antes de você confirmar, nada é gravado.`}
      </Text>
      {big ? (
        <Text as="p" role="status" fontSize={14} type="secondary" style={{ margin: 0 }}>
          Arquivo grande demais para comparar aqui. Confirme só se tem certeza do que está no disco.
        </Text>
      ) : (
        <>
          <Text as="p" role="status" fontSize={12} type="secondary" style={{ margin: 0 }}>
            {differ === 0 ? 'As duas versões têm o mesmo texto linha a linha.' : `${differ} linha(s) diferente(s) na mesma posição.`}
          </Text>
          <div className="grid min-w-0 gap-3 md:grid-cols-2">
            <section aria-label="Versão do disco" className="flex min-w-0 flex-col gap-1">
              <Text as="p" fontSize={12} weight={600} style={{ margin: 0 }}>Disco (atual)</Text>
              <pre className="max-h-[50vh] overflow-auto rounded border p-2 text-xs whitespace-pre select-text">{diskText}</pre>
            </section>
            <section aria-label="Sua edição" className="flex min-w-0 flex-col gap-1">
              <Text as="p" fontSize={12} weight={600} style={{ margin: 0 }}>Sua edição</Text>
              <pre className="max-h-[50vh] overflow-auto rounded border p-2 text-xs whitespace-pre select-text">{draft}</pre>
            </section>
          </div>
        </>
      )}
    </Flexbox>
  );
}

// ---- Diff (preparadas e alterações) e commit: o arquivo inteiro num comparador contínuo ----

type ViewMode = 'split' | 'unified';

// Comparador: lado a lado por padrão, com a fonte e as cores do editor (docs/interface.md). Linhas longas quebram dentro das colunas;
// o motor alinha os lados. A geometria é medida nos elementos de linha do shadow DOM depois de cada render.
type DiffOptions = NonNullable<ComponentProps<typeof FileDiff>['options']>;
// A grade tem cinco trilhas: número, texto, vão de 40 px, número e texto. Com o worker, tema e `lineDiffType` valem do pool
// (`diff-worker.tsx`); os daqui ficam para quando o worker falha e o realce volta à thread principal.
const COMPARE_DIFF_OPTIONS: DiffOptions = {
  theme: { dark: 'dark-plus', light: 'dark-plus' },
  disableFileHeader: true,
  overflow: 'wrap',
  diffIndicators: 'none',
  lineDiffType: 'word-alt',
  unsafeCSS: `
    :host {
      --diffs-dark-bg: #1F1F1F;
      --diffs-light-bg: #1F1F1F;
      --diffs-bg-context-override: #1F1F1F;
      --diffs-bg-context-gutter-override: #1F1F1F;
      --diffs-bg-buffer-override: #1F1F1F;
      --diffs-bg-separator-override: #1F1F1F;
      --diffs-bg-addition-override: #9bb95533;
      --diffs-bg-deletion-override: #ff000033;
      --diffs-bg-addition-emphasis-override: #9ccc2c33;
      --diffs-bg-deletion-emphasis-override: #ff000033;
      --diffs-fg-number: #6E7681;
      --diffs-font-family: Consolas, 'Courier New', monospace;
      --diffs-font-size: 14px;
      --diffs-line-height: 19px;
    }
    [data-separator-wrapper], [data-separator] { display: none; }
    pre[data-diff-type='split'][data-overflow='wrap'] { grid-template-columns: var(--diffs-grid-number-column-width) minmax(0, var(--compare-l, 50fr)) 40px var(--diffs-grid-number-column-width) minmax(0, var(--compare-r, 50fr)); background-color: #1f1f1f; }
    pre[data-diff-type='split'][data-overflow='wrap'] > [data-deletions] > [data-gutter] { grid-column: 1; }
    pre[data-diff-type='split'][data-overflow='wrap'] > [data-deletions] > [data-content] { grid-column: 2; border-right: 1px solid #3c3c3c; }
    pre[data-diff-type='split'][data-overflow='wrap'] > [data-additions] > [data-gutter] { grid-column: 4; border-left: 0; }
    pre[data-diff-type='split'][data-overflow='wrap'] > [data-additions] > [data-content] { grid-column: 5; }
    pre[data-diff-type='split'] [data-column-number] { padding-inline-end: 4px; }
    pre[data-diff-type='split'] [data-column-number][data-line-type='change-deletion']::after,
    pre[data-diff-type='single'] [data-column-number][data-line-type='change-deletion']::after { content: '−'; margin-inline-start: 4px; color: #6e7681; }
    pre[data-diff-type='split'] [data-column-number][data-line-type='change-addition']::after,
    pre[data-diff-type='single'] [data-column-number][data-line-type='change-addition']::after { content: '+'; margin-inline-start: 4px; color: #6e7681; }
    pre[data-background] [data-line][data-line-type='change-deletion'] { --diffs-line-bg: #ff000033; }
    pre[data-background] [data-line][data-line-type='change-addition'] { --diffs-line-bg: #9bb95533; }
    pre[data-background] [data-line][data-line-type='change-deletion'] span[data-decoration-bg] { --diffs-decoration-bg: #ff000033; }
    pre[data-background] [data-line][data-line-type='change-addition'] span[data-decoration-bg] { --diffs-decoration-bg: #9ccc2c33; }
    pre[data-diff-type='split'] [data-content-buffer] { background-image: linear-gradient(-45deg, #cccccc33 12.5%, #0000 12.5%, #0000 50%, #cccccc33 50%, #cccccc33 62.5%, #0000 62.5%, #0000 100%); background-size: 8px 8px; background-color: #1f1f1f; }
    [data-code] { padding-top: 0 !important; padding-bottom: 0 !important; }
  `,
};

// Alterna lado a lado e unificado. O rótulo é o modo para onde o clique leva (lado a lado é o padrão).
function ModeToggle({ mode, onChange }: { mode: ViewMode; onChange: (mode: ViewMode) => void }) {
  const split = mode === 'split';
  const label = split ? 'Unificado' : 'Lado a lado';
  return <ActionIcon size="small" variant="borderless" icon={split ? Rows2 : Columns2} aria-label={label} title={label} onClick={() => onChange(split ? 'unified' : 'split')} />;
}

// Cabeçalho do arquivo no patch (tudo antes do primeiro `@@`). O descarte de bloco mostra o cabeçalho com o bloco.
const fileHead = (patch: string) => {
  const at = patch.indexOf('\n@@');
  return at < 0 ? patch : patch.slice(0, at);
};
// Arquivo criado (não rastreado, preparado ou num commit): o cabeçalho traz `new file mode`, a mesma regra com que o motor desenha uma
// coluna só. Sem original para comparar, não há divisor nem troca entre lado a lado e unificado.
const isNewFile = (patch: string) => /^new file mode /m.test(fileHead(patch));

// Geometria medida no motor, em pixels do palco: cada unidade vai do topo da primeira linha de mudança dela ao fim da última. A medida
// vem do DOM depois de cada render (`onPostRender`), então as linhas auxiliares (o aviso de fim sem quebra) entram sem estimativa.
type Geo = { patch: string; mode: ViewMode; height: number; units: ([number, number] | null)[] };
const sameGeo = (a: Geo, b: Geo) => a.patch === b.patch && a.mode === b.mode && a.height === b.height && JSON.stringify(a.units) === JSON.stringify(b.units);

// Divisor do lado a lado: cada lado tem pelo menos 15%, no arraste e nas setas.
const SPLIT_MIN = 15;
const SPLIT_MAX = 85;
const clampSplit = (pct: number) => Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, pct));

// O arquivo inteiro numa só visão: nada separa as mudanças. A unidade atual (bloco, ou trecho no commit) tem contorno, cada unidade
// tem uma marca na faixa da direita, e um clique numa mudança a escolhe. A rolagem vai à primeira linha da unidade atual, e só quando
// ela, o modo ou o arquivo mudam: a atualização do diff não rola a tela. No lado a lado, a proporção das colunas de texto e a posição do vão
// são atualizadas no palco sem refazer o diff nem rolar a tela.
function Continuous({ patch, fileDiff, compare, mode: asked, current, unitName, onPick, center }: {
  patch: string; fileDiff: FileDiffMetadata; compare: CompareModel; mode: ViewMode; current: number; unitName: string; onPick: (unit: number) => void; center?: ReactNode;
}) {
  const mode: ViewMode = isNewFile(patch) ? 'unified' : asked;
  const body = useRef<HTMLDivElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const [geo, setGeo] = useState<Geo | null>(null);
  const [left, setLeft] = useState(50);
  const sash = useRef<HTMLDivElement>(null);
  // Chave de cada linha de mudança (ver `changeKeys`) para a unidade dela. As duas visões têm as mesmas linhas de mudança.
  const unitOfKey = useMemo(() => {
    const m = new Map<string, number>();
    for (const r of compare.unified) for (const k of changeKeys(r)) m.set(k, r.unit);
    return m;
  }, [compare]);
  // O motor chama `onPostRender` e `onLineClick` fora do render: eles leem o estado atual por esta referência. As opções do motor
  // ficam estáveis, então um diff igual não é refeito.
  const latest = useRef({ patch, compare, mode, onPick, unitOfKey });
  useLayoutEffect(() => {
    latest.current = { patch, compare, mode, onPick, unitOfKey };
  });

  // Mede as linhas de mudança e as unidades no DOM do motor (o shadow DOM do diff). O modo é o que o motor desenhou (a `pre`), que pode
  // ainda ser o anterior logo depois da troca. Só troca o estado quando a geometria muda.
  const measure = () => {
    const el = stage.current;
    const root = el?.querySelector('diffs-container')?.shadowRoot;
    const pre = root?.querySelector('pre[data-diff-type]');
    // Aba escondida (outra aba ativa) não tem geometria: não mede. Ao aparecer, o `useLayoutEffect` abaixo mede.
    if (!el || !root || !pre || el.getClientRects().length === 0) return;
    const drawn: ViewMode = pre.getAttribute('data-diff-type') === 'split' ? 'split' : 'unified';
    const { patch: text, compare: model } = latest.current;
    const box = el.getBoundingClientRect();
    const at = new Map<string, [number, number]>();
    root.querySelectorAll('[data-line]').forEach((line) => {
      const t = line.getAttribute('data-line-type');
      if (t !== 'change-deletion' && t !== 'change-addition') return;
      const r = line.getBoundingClientRect();
      at.set((t === 'change-deletion' ? 'd' : 'a') + line.getAttribute('data-line'), [r.top - box.top, r.bottom - box.top]);
    });
    const rows = drawn === 'split' ? model.split : model.unified;
    const units = model.units.map((u): [number, number] | null => {
      const s = u[drawn];
      if (s[0] < 0) return null;
      const first = at.get(changeKeys(rows[s[0]])[0] ?? '');
      const last = at.get(changeKeys(rows[s[1]])[0] ?? '');
      return first && last ? [first[0], last[1]] : null;
    });
    if (drawn === 'split') {
      const content = root.querySelector('pre[data-diff-type="split"] > [data-deletions] > [data-content]');
      if (content) stage.current?.style.setProperty('--compare-center-x', `${content.getBoundingClientRect().right - box.left + 20}px`);
    }
    const next: Geo = { patch: text, mode: drawn, height: box.height, units };
    setGeo((prev) => (prev && sameGeo(prev, next) ? prev : next));
  };
  // A rolagem pede a primeira linha da unidade atual. Só é pedida quando a unidade, o modo ou o arquivo (pela `key`) mudam; a geometria
  // chega depois do render do motor, então o pedido espera por ela.
  const jump = useRef(true);
  useEffect(() => { jump.current = true; }, [current, mode]);
  useEffect(() => {
    const ready = geo && geo.patch === patch && geo.mode === mode ? geo : null;
    const s = ready?.units[current];
    if (!jump.current || !s || !body.current) return;
    jump.current = false;
    body.current.scrollTop = Math.max(0, s[0] - body.current.clientHeight / 3);
    // Um Próximo desabilitado no último bloco tira o foco da aba (depois do commit); o F8 seguinte tem de achar o corpo, não o documento.
    setTimeout(() => { if (document.activeElement === document.body) body.current?.focus({ preventScroll: true }); }, 0);
  }, [geo, current, mode, patch]);

  // A geometria vale só para o patch e o modo que estão na tela; enquanto o motor não mediu, não há marca nem faixa.
  const ready = geo && geo.patch === patch && geo.mode === mode ? geo : null;
  // Sem medida válida (o patch mudou com a aba escondida, ou ainda não houve render), mede assim que a aba estiver na tela.
  useLayoutEffect(() => { if (!ready) measure(); });
  const cur = ready?.units[current] ?? null;
  const frac = (y: number) => `${(y / (ready?.height || 1)) * 100}%`;
  // Um clique numa linha de mudança escolhe a unidade dela, pelo número e pelo lado que o motor deu.
  const engine = useMemo<DiffOptions>(() => ({
    ...COMPARE_DIFF_OPTIONS,
    // O realce do worker chega depois e troca as linhas do motor; a âncora de rolagem do navegador some com elas e o arraste do divisor
    // perderia a linha do topo. Um passo de rolagem faz o navegador escolher a âncora de novo.
    onPostRender: () => {
      const el = body.current;
      if (el && el.scrollTop > 0) { el.scrollTop -= 1; el.scrollTop += 1; }
      measure();
    },
    onLineClick: (props) => {
      const t = props.lineType;
      const unit = latest.current.unitOfKey.get((t === 'change-deletion' ? 'd' : t === 'change-addition' ? 'a' : '') + props.lineNumber);
      if (unit !== undefined) latest.current.onPick(unit);
    },
  }), []);
  // O motor não renderiza de novo no arraste: o pointer só troca a variável do palco e o `aria-valuenow` do divisor, pelo DOM. O estado
  // recebe a proporção no fim. `diff` é o mesmo elemento enquanto o patch e o modo não mudam, então o React não mexe no motor.
  const options = useMemo<DiffOptions>(() => ({ ...engine, diffStyle: mode }), [engine, mode]);
  const diff = useMemo(() => <FileDiff fileDiff={fileDiff} options={options} className="block w-full" />, [fileDiff, options]);
  const drag = useRef<{ x: number; start: number; width: number; pct: number } | null>(null);
  const paint = (pct: number) => {
    const el = stage.current;
    el?.style.setProperty('--compare-l', `${pct}fr`);
    el?.style.setProperty('--compare-r', `${100 - pct}fr`);
    const root = el?.querySelector('diffs-container')?.shadowRoot;
    const content = root?.querySelector('pre[data-diff-type="split"] > [data-deletions] > [data-content]');
    if (el && content) el.style.setProperty('--compare-center-x', `${content.getBoundingClientRect().right - el.getBoundingClientRect().left + 20}px`);
    sash.current?.setAttribute('aria-valuenow', String(Math.round(pct)));
  };
  useLayoutEffect(() => {
    paint(left);
    const el = stage.current;
    if (!el) return;
    const observer = new ResizeObserver(() => { paint(left); measure(); });
    observer.observe(el);
    return () => observer.disconnect();
  }, [left]);
  const onSashDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    // Sem seleção de texto no arraste; o foco vai ao divisor, que é focável.
    e.currentTarget.focus({ preventScroll: true });
    e.currentTarget.setPointerCapture(e.pointerId);
    const root = stage.current?.querySelector('diffs-container')?.shadowRoot;
    const delText = root?.querySelector('pre[data-diff-type="split"] > [data-deletions] > [data-content]')?.getBoundingClientRect();
    const addText = root?.querySelector('pre[data-diff-type="split"] > [data-additions] > [data-content]')?.getBoundingClientRect();
    drag.current = { x: e.clientX, start: left, width: Math.max(1, (delText?.width ?? 0) + (addText?.width ?? 0)), pct: left };
    e.currentTarget.setAttribute('data-dragging', '');
    body.current?.setAttribute('data-dragging', '');
  };
  const onSashMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    d.pct = clampSplit(d.start + ((e.clientX - d.x) / d.width) * 100);
    paint(d.pct);
  };
  const onSashUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    e.preventDefault();
    drag.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    e.currentTarget.removeAttribute('data-dragging');
    body.current?.removeAttribute('data-dragging');
    setLeft(d.pct);
    measure();
  };
  // Setas esquerda e direita movem 2 pontos percentuais; a rolagem da tela fica parada.
  const onSashKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    setLeft(clampSplit(left + (e.key === 'ArrowRight' ? 2 : -2)));
    requestAnimationFrame(measure);
  };
  return (
    <Flexbox flex={1} className="min-h-0 compare-view">
      <div
        ref={body} tabIndex={0} aria-label="Arquivo inteiro: F8 vai à próxima mudança, Shift+F8 à anterior" className="compare-body"
      >
        <div ref={stage} className="compare-stage" style={{ '--compare-l': '50fr', '--compare-r': '50fr' } as CSSProperties}>
          {diff}
          {cur && <div className="compare-current" data-unit={current} style={{ top: cur[0], height: cur[1] - cur[0] }} />}
          {mode === 'split' && center && <div className="compare-center" style={cur ? { top: cur[0] + (cur[1] - cur[0]) / 2 } : undefined}>{center}</div>}
          {mode === 'split' && (
            <div
              ref={sash} role="separator" aria-orientation="vertical" aria-label="Divisor entre o original e o modificado"
              aria-valuemin={SPLIT_MIN} aria-valuemax={SPLIT_MAX} aria-valuenow={Math.round(left)} tabIndex={0} className="compare-sash"
              onPointerDown={onSashDown} onPointerMove={onSashMove} onPointerUp={onSashUp} onPointerCancel={onSashUp}
              onDoubleClick={() => setLeft(50)} onKeyDown={onSashKey}
            />
          )}
        </div>
      </div>
      <div className="compare-ruler">
        {ready && compare.units.map((unit, i) => {
          const s = ready.units[i];
          if (!s) return null;
          return (
            <button
              key={i} type="button" className={`compare-mark compare-mark-${unit.kind}`} data-current={i === current || undefined}
              style={{ top: frac(s[0]), height: `max(3px, ${frac(s[1] - s[0])})` }}
              aria-label={`Ir para ${unitName.toLowerCase()} ${i + 1}, linha ${unit.line}`} title={`${unitName} ${i + 1}, linha ${unit.line}`}
              onClick={() => onPick(i)}
            />
          );
        })}
      </div>
    </Flexbox>
  );
}

// F8 vai à próxima mudança, Shift+F8 à anterior. Vale em toda a aba, então o foco pode estar numa ação da toolbar.
const stepKey = (e: KeyboardEvent<HTMLElement>, step: (by: number) => void) => {
  if (e.key !== 'F8') return;
  e.preventDefault();
  step(e.shiftKey ? -1 : 1);
};

// `shown`: a aba é a atual do grupo. As abas escondidas ficam montadas e não consultam o Git; ao voltar, leem o diff de novo,
// porque a aba irmã pode ter mudado o índice (a revisão velha seria recusada pelo Git).
export function DiffTab({
  target, common, shown, read }: { target: Extract<TabTarget, { kind: 'diff' }>; common: Common; shown: boolean; read?: GitTabRead }) {
  const backend = useBackend();
  const staged = target.side === 'staged';
  const [initialRead] = useState(() => read);
  const initial = initialRead?.result;
  const [review, setReview] = useState<ReviewDiff | null>(() => initial && 'review' in initial ? initial.review : null);
  const [error, setError] = useState(() => initial && 'error' in initial ? initial.error : '');
  const firstRead = useRef(initialRead?.promise ?? null);
  const [mode, setMode] = useState<ViewMode>('split');
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  // Bloco atual pelo índice (os blocos são os do Git, com 3 linhas de contexto). O primeiro é o atual ao abrir; trocar de arquivo volta a ele.
  const [at, setAt] = useState(0);
  useEffect(() => setAt(0), [target.path, target.side]);
  useEffect(() => { initialRead?.consume(); }, [initialRead]);
  const load = async () => {
    const pending = firstRead.current;
    firstRead.current = null;
    const result = pending
      ? await pending
      : await backend.gitReviewDiff(target.path, staged).then((review) => ({ review } as const), (e: unknown) => ({ error: message(e) } as const));
    if ('error' in result) {
      setError(result.error);
    } else if ('review' in result) {
      setReview(result.review);
      setError('');
    }
  };
  usePolling(load, POLL_MS, common.active && shown && `${target.path}|${target.side}`);

  // Uma operação por vez: o clique duplo não dispara duas (o ref não espera o re-render).
  const run = async (work: () => Promise<void>) => {
    if (busyRef.current || common.locked) return;
    if (!common.guard.enter()) return;
    busyRef.current = true;
    setBusy(true);
    const attempt = async () => {
      await work();
      await load();
      common.refreshGit();
    };
    await attempt().catch(async (e: unknown) => {
      toast.error({ title: message(e), duration: 7000 });
      await load();
    }).finally(() => {
      busyRef.current = false;
      setBusy(false);
      common.guard.leave();
    });
  };

  // Não rastreado: o Git não separa em blocos. O comparador mostra o arquivo novo inteiro e a ação é preparar o arquivo.
  const untracked = review !== null && review.untracked;
  const hunks: PatchHunk[] = review && !untracked ? patchHunks(review.diff.patch) : [];
  // Só os campos que o comparador usa entram nas dependências: uma resposta igual à anterior (o poll) não refaz o arquivo nem a rolagem.
  const shownPatch = review ? review.display.patch : '';
  const shownBinary = review ? review.display.binary : false;
  const shownLarge = review ? review.display.tooLarge : false;
  const diffPatch = review ? review.diff.patch : '';
  const fileDiff = useMemo(() => {
    if (!review) return null;
    if (initialRead?.patch === review.display.patch && initialRead.fileDiff) return initialRead.fileDiff;
    return gitTabFileDiff(target, review.display.patch, review.revision);
  }, [target.path, target.side, review?.display.patch, review?.revision]);
  const compare = useMemo(() => (
    shownPatch !== '' && !shownBinary && !shownLarge
      ? compareOf(shownPatch, !untracked && diffPatch !== '' ? blockRanges(diffPatch) : null)
      : null
  ), [shownPatch, shownBinary, shownLarge, untracked, diffPatch]);
  const blocked_ = common.locked || busy;
  const draftOpen = common.hasDraft(target.path);
  const n = hunks.length;
  // O bloco pode sumir (preparado ou descartado): o atual nunca passa do último.
  const cur = n === 0 ? -1 : Math.min(at, n - 1);
  const step = (by: number) => {
    if (n === 0) return;
    setAt(Math.min(Math.max(cur + by, 0), n - 1));
  };
  const block = cur >= 0 ? hunks[cur] : null;
  const head = review ? fileHead(review.diff.patch) : '';
  // Ações por bloco pelo índice: a barra age no bloco atual. O `HunkRef` sai do bloco visto agora.
  const refAt = (i: number): HunkRef | null => (hunks[i] && review ? { path: target.path, index: i, header: hunks[i].header, revision: review.revision } : null);
  const ref = cur >= 0 ? refAt(cur) : null;
  const stageAt = (i: number) => { const r = refAt(i); if (r) run(() => backend.gitStageHunk(r)); };
  const unstageAt = (i: number) => { const r = refAt(i); if (r) run(() => backend.gitUnstageHunk(r)); };
  const discardAt = async (i: number) => {
    const r = refAt(i);
    if (!r) return;
    // Confirma mostrando o patch capturado agora; o clique em confirmar usa este mesmo `r`, sem reler outro patch.
    const ok = await common.ask({
      title: 'Descartar este bloco?',
      body: <Text as="pre" fontSize={12} className="max-h-64 overflow-auto whitespace-pre">{`${head}\n${hunks[i].text}`}</Text>,
      actions: [{ key: 'ok', label: 'Descartar', danger: true }],
    });
    if (ok !== 'ok') return;
    if (common.hasDraft(target.path)) {
      toast.error({ title: 'Há um rascunho aberto deste arquivo. Salve ou descarte antes de descartar o bloco.', duration: 6000 });
      return;
    }
    await run(() => backend.gitDiscardHunk(r));
  };

  if (error && !review) return <Note text={error} danger />;
  if (!review) return <Reading text="Lendo as alterações…" />;
  if (review.diff.binary) return <Note text="Arquivo binário: o diff não é mostrado." />;
  if (review.diff.tooLarge || review.display.tooLarge) return <Note text="Diff grande demais para mostrar aqui." />;
  if (review.diff.patch === '' || !compare) return <Note text="Não há alterações neste lado." />;
  if (!fileDiff) return <Note text="Não foi possível interpretar o diff." danger />;

  const counter = untracked ? 'arquivo novo, não rastreado' : n === 0 ? (staged ? 'preparadas' : 'alterações') : `bloco ${cur + 1} de ${n}`;
  return (
    <Flexbox flex={1} className="min-h-0" onKeyDown={(e) => stepKey(e, step)}>
      <Flexbox horizontal align="center" gap={2} height={28} paddingInline={8} flex="none" className="border-b compare-toolbar">
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={target.path} className="min-w-0 flex-1">{target.path}</Text>
        <Text as="span" type="secondary" fontSize={11} className="shrink-0 whitespace-nowrap">{counter}</Text>
        {n > 0 && (
          <>
            <ActionIcon
              size="small" variant="borderless" icon={ChevronUp} aria-label="Bloco anterior" title="Bloco anterior (Shift+F8)"
              disabled={cur <= 0} onClick={() => step(-1)}
            />
            <ActionIcon
              size="small" variant="borderless" icon={ChevronDown} aria-label="Próximo bloco" title="Próximo bloco (F8)"
              disabled={cur >= n - 1} onClick={() => step(1)}
            />
          </>
        )}
        {ref && block && (
          <>
            <ActionIcon
              size="small" variant="borderless" icon={FileText} aria-label={`Abrir bloco ${cur + 1} de ${n} no arquivo`}
              title={`Abrir o bloco ${cur + 1} de ${n} no arquivo`} onClick={() => common.openLine(target.path, compare.units[cur].line)}
            />
            {staged
              ? <ActionIcon size="small" variant="borderless" icon={Minus} aria-label={`Tirar bloco ${cur + 1} de ${n} do preparo`} title={`Tirar o bloco ${cur + 1} de ${n} do preparo`} disabled={blocked_} onClick={() => unstageAt(cur)} />
              : <ActionIcon size="small" variant="borderless" icon={Plus} aria-label={`Preparar bloco ${cur + 1} de ${n}`} title={`Preparar o bloco ${cur + 1} de ${n}`} disabled={blocked_} onClick={() => stageAt(cur)} />}
            {!staged && (
              <ActionIcon
                size="small" variant="borderless" danger icon={ArrowRight} aria-label={`Descartar bloco ${cur + 1} de ${n}`}
                disabled={blocked_ || draftOpen}
                title={draftOpen ? 'Resolva o rascunho aberto deste arquivo antes de descartar.' : `Descartar o bloco ${cur + 1} de ${n}`}
                onClick={() => discardAt(cur)}
              />
            )}
          </>
        )}
        {untracked && !staged && <Button size="small" disabled={blocked_} onClick={() => run(() => backend.gitStage([target.path]))}>Preparar arquivo</Button>}
        {!isNewFile(review.display.patch) && <ModeToggle mode={mode} onChange={setMode} />}
        {!staged && (
          <ActionIcon
            size="small" variant="borderless" danger icon={Undo2} aria-label="Descartar arquivo"
            disabled={blocked_ || draftOpen}
            title={draftOpen ? 'Resolva o rascunho aberto deste arquivo antes de descartar.' : 'Descartar todas as alterações deste arquivo'}
            onClick={async () => {
              const ok = await common.ask({
                title: 'Descartar as alterações do arquivo?',
                body: <Text as="pre" fontSize={12} className="max-h-64 overflow-auto whitespace-pre">{review.diff.patch}</Text>,
                actions: [{ key: 'ok', label: 'Descartar', danger: true }],
              });
              if (ok !== 'ok') return;
              // Rascunho aberto pode ter surgido durante a confirmação: a checagem vale agora, não só no render anterior.
              if (common.hasDraft(target.path)) {
                toast.error({ title: 'Há um rascunho aberto deste arquivo. Salve ou descarte antes de descartar as alterações.', duration: 6000 });
                return;
              }
              const captured = review.revision;
              await run(() => backend.gitDiscardFile(target.path, captured));
            }}
          />
        )}
      </Flexbox>
      <Continuous key={`${target.path}|${target.side}`} patch={review.display.patch} fileDiff={fileDiff} compare={compare} mode={mode} current={cur} unitName="Bloco" onPick={setAt}
        center={ref && block && !untracked ? <div className="compare-center-actions">
          {!staged && <ActionIcon size="small" variant="borderless" danger icon={ArrowRight} aria-label={`Descartar bloco ${cur + 1} de ${n}`} title={`Descartar o bloco ${cur + 1} de ${n}`} disabled={blocked_ || draftOpen} onClick={() => discardAt(cur)} />}
          {staged
            ? <ActionIcon size="small" variant="borderless" icon={Minus} aria-label={`Tirar bloco ${cur + 1} de ${n} do preparo`} title={`Tirar o bloco ${cur + 1} de ${n} do preparo`} disabled={blocked_} onClick={() => unstageAt(cur)} />
            : <ActionIcon size="small" variant="borderless" icon={Plus} aria-label={`Preparar bloco ${cur + 1} de ${n}`} title={`Preparar o bloco ${cur + 1} de ${n}`} disabled={blocked_} onClick={() => stageAt(cur)} />}
        </div> : undefined}
      />
    </Flexbox>
  );
}

// ---- Histórico (commit): leitura imutável, sem botões de mutação ----

// O commit mostra o arquivo inteiro. Sem blocos do Git, a unidade é cada trecho de mudança; a navegação é só de leitura.
export function CommitTab({
  target, read }: { target: Extract<TabTarget, { kind: 'commit' }>; read?: GitTabRead }) {
  const backend = useBackend();
  const [initialRead] = useState(() => read);
  const initial = initialRead?.result;
  const [diff, setDiff] = useState<GitDiff | null>(() => initial && 'diff' in initial ? initial.diff : null);
  const [error, setError] = useState(() => initial && 'error' in initial ? initial.error : '');
  const firstRead = useRef(initialRead?.promise ?? null);
  const [mode, setMode] = useState<ViewMode>('split');
  const [at, setAt] = useState(0);
  useEffect(() => {
    initialRead?.consume();
    if (diff || error) return; // histórico é imutável: lê uma vez
    const pending = firstRead.current;
    firstRead.current = null;
    void (pending
      ? pending
      : backend.gitCommitDiff(target.hash, target.path).then((diff) => ({ diff } as const), (e: unknown) => ({ error: message(e) } as const)))
      .then((result) => {
        if ('error' in result) setError(result.error);
        else if ('diff' in result) setDiff(result.diff);
      });
  }, [target.hash, target.path]);
  const fileDiff = useMemo(() => {
    if (!diff) return null;
    if (initialRead?.patch === diff.patch && initialRead.fileDiff) return initialRead.fileDiff;
    return gitTabFileDiff(target, diff.patch);
  }, [target.hash, target.path, diff?.patch]);
  const compare = useMemo(() => (diff && diff.patch !== '' && !diff.binary && !diff.tooLarge ? compareOf(diff.patch, null) : null), [diff]);
  if (error) return <Note text={error} danger />;
  if (!diff) return <Reading text="Lendo o commit…" />;
  if (diff.binary) return <Note text="Arquivo binário: o diff não é mostrado." />;
  if (diff.tooLarge) return <Note text="Diff grande demais para mostrar aqui." />;
  if (!compare) return <Note text="Este commit não altera o texto deste arquivo." />;
  if (!fileDiff) return <Note text="Não foi possível interpretar o diff." danger />;
  const n = compare.units.length;
  const cur = n === 0 ? -1 : Math.min(at, n - 1);
  const step = (by: number) => {
    if (n === 0) return;
    setAt(Math.min(Math.max(cur + by, 0), n - 1));
  };
  return (
    <Flexbox flex={1} className="min-h-0" onKeyDown={(e) => stepKey(e, step)}>
      <Flexbox horizontal align="center" gap={6} height={28} paddingInline={8} flex="none" className="border-b compare-toolbar">
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={target.path} className="min-w-0 flex-1">{target.path}</Text>
        <Text as="span" type="secondary" fontSize={11} className="shrink-0 whitespace-nowrap">commit {target.hash.slice(0, 7)} · somente leitura</Text>
        {n > 0 && (
          <>
            <Text as="span" type="secondary" fontSize={11} className="shrink-0 whitespace-nowrap">{`mudança ${cur + 1} de ${n}`}</Text>
            <ActionIcon size="small" variant="borderless" icon={ChevronUp} aria-label="Mudança anterior" title="Mudança anterior (Shift+F8)" disabled={cur <= 0} onClick={() => step(-1)} />
            <ActionIcon size="small" variant="borderless" icon={ChevronDown} aria-label="Próxima mudança" title="Próxima mudança (F8)" disabled={cur >= n - 1} onClick={() => step(1)} />
          </>
        )}
        {!isNewFile(diff.patch) && <ModeToggle mode={mode} onChange={setMode} />}
      </Flexbox>
      <Continuous key={`${target.hash}|${target.path}`} patch={diff.patch} fileDiff={fileDiff} compare={compare} mode={mode} current={cur} unitName="Mudança" onPick={setAt} />
    </Flexbox>
  );
}
