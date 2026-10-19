// Marcadores de mudança do Git no editor, como os do VS Code/Antigravity e do Zed: no gutter, uma barra nas linhas acrescentadas e
// modificadas e um triângulo vermelho onde linhas foram apagadas; na faixa da barra de rolagem vertical, marcas nas mesmas posições; no
// minimap, a faixa de git (ver `minimapGutter`). Duas comparações, como o Zed: buffer × HEAD é o que aparece, e buffer × índice diz se
// o bloco já está em stage (`staged_hollow`: preparado = só o contorno, não preparado = barra cheia).
// O diff é o `Chunk` do @codemirror/merge: `Chunk.build` quando a base chega e `Chunk.updateB` (incremental) a cada digitação, então
// os marcadores acompanham o buffer, inclusive não salvo. A base chega pelo efeito `setGitBase`, que não mexe no documento.
// Cores do VS Code Dark Modern (editorGutter.*Background; overview ruler e minimap com as mesmas, ver docs/interface.md).
// Clicar no marcador abre o bloco (`gitOpenField`, a posição no buffer do bloco aberto); o trecho aberto é desenhado por git-hunk.ts.
import { Chunk } from '@codemirror/merge';
import { Facet, StateEffect, StateField, Text, type ChangeDesc, type EditorState } from '@codemirror/state';
import { EditorView, GutterMarker, gutter, ViewPlugin, type ViewUpdate } from '@codemirror/view';
import type { FileBase } from '../backend/types';
import { DIFF, textOf, touches } from './git-stage';

export type Kind = 'added' | 'modified' | 'deleted';
/**
 * Um bloco buffer × HEAD, em linhas do buffer (1-based). O apagado tem `from === to`: a linha onde fica a marca. `chunk` é o trecho do
 * @codemirror/merge (A = HEAD, B = buffer), em posições.
 */
export type Hunk = { kind: Kind; staged: boolean; from: number; to: number; end: boolean; chunk: Chunk };

const COLOR: Record<Kind, string> = { added: '#2EA043', modified: '#0078D4', deleted: '#F85149' };
// editorOverviewRuler.*Foreground: a cor do gutter com 60% de opacidade (0x99).
const RULER: Record<Kind, string> = { added: '#2EA04399', modified: '#0078D499', deleted: '#F8514999' };
// Altura mínima de uma marca na barra de rolagem, e a largura da barra vertical do tema (file-view.tsx).
const MIN_MARK = 3;
const SCROLLBAR = 14;

type Diff = {
  source: FileBase;
  head: Text;
  index: Text;
  // `null` com o documento acima do limite: sem marcadores até ele voltar a caber.
  vsHead: readonly Chunk[] | null;
  vsIndex: readonly Chunk[] | null;
  hunks: readonly Hunk[];
  minimap: Record<number, string>;
};

/** Troca a base (o texto do Git, com `\n`). `null` tira os marcadores. */
export const setGitBase = StateEffect.define<FileBase | null>();

// Tamanho máximo do documento com marcadores (o mesmo limite de cor do editor). O menor valor vence.
const gitLimit = Facet.define<number, number>({ combine: (values) => (values.length ? Math.min(...values) : Infinity) });

function hunksOf(vsHead: readonly Chunk[], vsIndex: readonly Chunk[], doc: Text): Hunk[] {
  const out: Hunk[] = [];
  let i = 0;
  for (const c of vsHead) {
    const kind: Kind = c.fromA === c.toA ? 'added' : c.fromB === c.toB ? 'deleted' : 'modified';
    // Os dois conjuntos vêm em ordem no buffer: o ponteiro só avança.
    while (i < vsIndex.length && vsIndex[i].toB < c.fromB) i++;
    let staged = true;
    for (let j = i; j < vsIndex.length && vsIndex[j].fromB <= c.toB; j++) if (touches(vsIndex[j], c)) { staged = false; break; }
    // Apagado no fim do arquivo sem quebra final: a posição passa do documento; a marca vai na última linha, embaixo.
    const end = c.fromB > doc.length;
    const from = doc.lineAt(Math.min(c.fromB, doc.length)).number;
    const to = kind === 'deleted' ? from : doc.lineAt(Math.min(c.endB, doc.length)).number;
    out.push({ kind, staged, from, to, end, chunk: c });
  }
  return out;
}

// Linha → cor da faixa de git do minimap (minimapGutter.*Background).
function minimapOf(hunks: readonly Hunk[]) {
  const out: Record<number, string> = {};
  for (const h of hunks) for (let n = h.from; n <= h.to; n++) out[n] = COLOR[h.kind];
  return out;
}
function sameMap(a: Record<number, string>, b: Record<number, string>) {
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((k) => a[Number(k)] === b[Number(k)]);
}

function derive(prev: Diff | null, source: FileBase, head: Text, index: Text, doc: Text, limit: number, changes?: ChangeDesc): Diff {
  let vsHead: readonly Chunk[] | null = null;
  let vsIndex: readonly Chunk[] | null = null;
  if (doc.length <= limit) {
    const incremental = !!changes && !!prev?.vsHead && !!prev.vsIndex;
    vsHead = incremental ? Chunk.updateB(prev!.vsHead!, head, doc, changes!, DIFF) : Chunk.build(head, doc, DIFF);
    // Índice igual ao HEAD (nada em stage): a mesma comparação serve para as duas.
    vsIndex = index === head ? vsHead : incremental ? Chunk.updateB(prev!.vsIndex!, index, doc, changes!, DIFF) : Chunk.build(index, doc, DIFF);
  }
  const hunks = vsHead && vsIndex ? hunksOf(vsHead, vsIndex, doc) : [];
  const map = minimapOf(hunks);
  // O minimap só redesenha a faixa quando a configuração dele muda: o mesmo objeto enquanto as linhas e as cores forem as mesmas.
  const minimap = prev && sameMap(prev.minimap, map) ? prev.minimap : map;
  return { source, head, index, vsHead, vsIndex, hunks, minimap };
}

function fromBase(base: FileBase | null, state: EditorState): Diff | null {
  if (!base) return null;
  // Base "commitada" = HEAD (arquivo novo no índice aparece todo como acrescentado); índice ausente (conflito) cai no HEAD.
  const head = textOf(base.head ?? '');
  const indexText = base.index ?? base.head ?? '';
  const index = indexText === (base.head ?? '') ? head : textOf(indexText);
  return derive(null, base, head, index, state.doc, state.facet(gitLimit));
}

const gitField = StateField.define<Diff | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setGitBase)) value = fromBase(e.value, tr.state);
    if (!value || !tr.docChanged || tr.effects.some((e) => e.is(setGitBase))) return value;
    return derive(value, value.source, value.head, value.index, tr.newDoc, tr.state.facet(gitLimit), tr.changes);
  },
});

/** A base atual já é esta (mesmos textos)? Quem busca a base só despacha quando mudou. */
export function sameGitBase(state: EditorState, base: FileBase | null) {
  const current = state.field(gitField, false)?.source ?? null;
  return current === base || (!!current && !!base && current.head === base.head && current.index === base.index);
}

/** Blocos atuais (para a tela e o QA). */
export const gitHunks = (state: EditorState): readonly Hunk[] => state.field(gitField, false)?.hunks ?? [];

/** Base atual com os textos do HEAD e do índice (o bloco aberto lê daqui), ou `null` sem marcadores. */
export const gitDiff = (state: EditorState) => state.field(gitField, false) ?? null;

/** Abre o bloco que contém esta posição do buffer (`null` fecha). Um bloco aberto por vez. */
export const openGitHunk = StateEffect.define<number | null>();

/** Bloco que contém a posição, inclusive nas pontas (o apagado tem `fromB === toB`). */
export const hunkAtPos = (hunks: readonly Hunk[], pos: number) => hunks.find((h) => h.chunk.fromB <= pos && pos <= h.chunk.toB) ?? null;

// Bloco aberto, guardado como o começo dele no buffer. A posição segue as edições (digitar dentro do bloco mantém aberto) e volta para o
// começo do bloco que a contém; se nenhum bloco a contém mais (restaurado, base nova), fecha.
const gitOpenField = StateField.define<number | null>({
  create: () => null,
  update(pos, tr) {
    let next = pos === null ? null : tr.changes.mapPos(pos, -1);
    for (const e of tr.effects) if (e.is(openGitHunk)) next = e.value;
    if (next === null) return null;
    return hunkAtPos(gitHunks(tr.state), next)?.chunk.fromB ?? null;
  },
});

/** O bloco aberto agora, ou `null`. */
export const openHunk = (state: EditorState) => {
  const pos = state.field(gitOpenField, false);
  return pos === null || pos === undefined ? null : hunkAtPos(gitHunks(state), pos);
};

const EMPTY: Record<number, string> = {};
/** Faixa de git do minimap (`gutters` do @replit/codemirror-minimap): objeto estável enquanto os marcadores não mudam. */
export const minimapGutter = (state: EditorState) => state.field(gitField, false)?.minimap ?? EMPTY;
export { gitField };

class GitMarker extends GutterMarker {
  constructor(readonly cls: string) {
    super();
    this.elementClass = cls;
  }
  eq(other: GutterMarker) {
    return other instanceof GitMarker && other.cls === this.cls;
  }
}
const markers = new Map<string, GitMarker>();
function markerOf(h: Hunk, line: number) {
  const cls = [
    'cm-git', `cm-git-${h.kind}`, h.staged ? 'cm-git-staged' : '',
    line === h.from ? 'cm-git-first' : '', line === h.to ? 'cm-git-last' : '', h.end ? 'cm-git-end' : '',
  ].filter(Boolean).join(' ');
  let marker = markers.get(cls);
  if (!marker) markers.set(cls, (marker = new GitMarker(cls)));
  return marker;
}

// Bloco que contém a linha (busca binária: os blocos vêm em ordem e não se sobrepõem).
function hunkAt(hunks: readonly Hunk[], line: number) {
  let lo = 0, hi = hunks.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (hunks[mid].to < line) lo = mid + 1;
    else if (hunks[mid].from > line) hi = mid - 1;
    else return hunks[mid];
  }
  return null;
}

const gitGutter = gutter({
  class: 'cm-gitGutter',
  lineMarker: (view, block) => {
    const hunks = gitHunks(view.state);
    if (!hunks.length) return null;
    const line = view.state.doc.lineAt(block.from).number;
    const h = hunkAt(hunks, line);
    return h ? markerOf(h, line) : null;
  },
  lineMarkerChange: (u) => u.startState.field(gitField, false) !== u.state.field(gitField, false),
  // Clicar no marcador abre o bloco; clicar de novo fecha. O foco vai para o texto, para o Esc fechar.
  domEventHandlers: {
    mousedown: (view, block, event) => {
      if ((event as MouseEvent).button !== 0) return false;
      const h = hunkAt(gitHunks(view.state), view.state.doc.lineAt(block.from).number);
      if (!h) return false;
      const open = view.state.field(gitOpenField, false);
      view.dispatch({ effects: openGitHunk.of(open === h.chunk.fromB ? null : h.chunk.fromB) });
      view.focus();
      return true;
    },
  },
});

// Marcas na faixa da barra de rolagem vertical do `.cm-scroller` (por cima dela, sem receber o ponteiro). A posição é a do bloco da
// linha na altura do documento (o mapa de alturas do CodeMirror, que conta as linhas quebradas). Sem rolagem vertical (documento curto),
// não há faixa e as marcas somem, para não cair em cima do minimap.
const overview = ViewPlugin.fromClass(class {
  dom: HTMLElement;
  constructor(readonly view: EditorView) {
    this.dom = document.createElement('div');
    this.dom.className = 'cm-gitOverview';
    this.dom.setAttribute('aria-hidden', 'true');
    view.dom.appendChild(this.dom);
    this.schedule();
  }
  update(u: ViewUpdate) {
    if (u.docChanged || u.geometryChanged || u.heightChanged || u.startState.field(gitField, false) !== u.state.field(gitField, false)) this.schedule();
  }
  schedule() {
    this.view.requestMeasure({
      key: this,
      read: (view) => {
        const scroller = view.scrollDOM;
        // Barra que ocupa espaço (WebView2) tem a largura medida; barra sobreposta ou escondida (Chrome headless) mede 0 e fica com os
        // 14 px do tema. Sem rolagem vertical, não há faixa.
        const bar = scroller.scrollHeight > scroller.clientHeight ? scroller.offsetWidth - scroller.clientWidth || SCROLLBAR : 0;
        const track = scroller.clientHeight;
        const total = Math.max(view.contentHeight, 1);
        const { doc } = view.state;
        const marks = bar <= 0 ? [] : gitHunks(view.state).map((h) => {
          const top = view.lineBlockAt(doc.line(h.from).from);
          const bottom = h.kind === 'deleted' ? top : view.lineBlockAt(doc.line(h.to).from);
          const y = h.kind === 'deleted' ? (h.end ? top.bottom : top.top) : top.top;
          const height = h.kind === 'deleted' ? 0 : bottom.bottom - top.top;
          return { kind: h.kind, top: (y / total) * track, height: Math.max(MIN_MARK, (height / total) * track) };
        });
        return { top: scroller.offsetTop, track, bar, marks };
      },
      write: ({ top, track, bar, marks }) => {
        Object.assign(this.dom.style, { top: `${top}px`, height: `${track}px`, width: `${Math.max(bar, 0)}px` });
        const frag = document.createDocumentFragment();
        for (const m of marks) {
          const el = document.createElement('div');
          el.className = `cm-gitOverview-mark cm-gitOverview-${m.kind}`;
          // A marca não passa da faixa (a última linha fica encostada embaixo).
          const at = Math.min(Math.max(m.top - (m.kind === 'deleted' ? MIN_MARK / 2 : 0), 0), track - m.height);
          el.style.top = `${at}px`;
          el.style.height = `${m.height}px`;
          frag.appendChild(el);
        }
        this.dom.replaceChildren(frag);
      },
    });
  }
  destroy() {
    this.dom.remove();
  }
});

// Triângulo do apagado (aponta para o texto, como o do VS Code): cheio no não preparado, só o contorno no preparado.
const triangle = (fill: boolean) => `url("data:image/svg+xml,${encodeURIComponent(fill
  ? `<svg xmlns='http://www.w3.org/2000/svg' width='5' height='8'><path d='M0 0L5 4L0 8Z' fill='${COLOR.deleted}'/></svg>`
  : `<svg xmlns='http://www.w3.org/2000/svg' width='5' height='8'><path d='M.5 1.2V6.8L4 4Z' fill='none' stroke='${COLOR.deleted}'/></svg>`)}")`;

// O gutter tem 6 px e fica entre os números e a dobra pela ordem do flex (os gutters do CodeMirror seguem a ordem das extensões, e os
// do basicSetup vêm antes); os 6 px saem da dobra (file-view.tsx), então o texto não muda de lugar. A barra tem 3 px, a 1 px dos números,
// e cobre a altura inteira de cada linha: linhas seguidas formam uma barra contínua. No preparado, a borda de 1 px fecha em cima na
// primeira linha e embaixo na última.
const theme = EditorView.theme({
  '.cm-lineNumbers': { order: '0' },
  '.cm-gitGutter': { order: '1', width: '6px' },
  '.cm-foldGutter': { order: '2' },
  '.cm-gitGutter .cm-gutterElement': { position: 'relative' },
  '.cm-gitGutter .cm-git': { cursor: 'pointer' },
  '.cm-git-added': { '--git': COLOR.added },
  '.cm-git-modified': { '--git': COLOR.modified },
  '.cm-git-added::before, .cm-git-modified::before': {
    content: '""', position: 'absolute', left: '1px', top: '0', bottom: '0', width: '3px', boxSizing: 'border-box', background: 'var(--git)',
  },
  '.cm-git-staged::before': { background: 'transparent', border: '1px solid var(--git)', borderTopWidth: '0', borderBottomWidth: '0' },
  '.cm-git-staged.cm-git-first::before': { borderTopWidth: '1px' },
  '.cm-git-staged.cm-git-last::before': { borderBottomWidth: '1px' },
  '.cm-git-deleted::before': {
    content: '""', position: 'absolute', left: '1px', top: '-4px', width: '5px', height: '8px', backgroundImage: triangle(true),
  },
  '.cm-git-deleted.cm-git-end::before': { top: 'auto', bottom: '-4px' },
  '.cm-git-deleted.cm-git-staged::before': { backgroundImage: triangle(false) },
  '.cm-gitOverview': { position: 'absolute', right: '0', pointerEvents: 'none', zIndex: '201' },
  // Faixa da esquerda da régua, como o SCM do VS Code (OverviewRulerLane.Left).
  '.cm-gitOverview-mark': { position: 'absolute', left: '0', width: '6px' },
  '.cm-gitOverview-added': { background: RULER.added },
  '.cm-gitOverview-modified': { background: RULER.modified },
  '.cm-gitOverview-deleted': { background: RULER.deleted },
});

/** Extensão completa: o estado, o gutter, as marcas na barra de rolagem e o tema. Documento acima de `limit` caracteres fica sem marcadores. */
export const gitChanges = (limit: number) => [gitLimit.of(limit), gitField, gitOpenField, gitGutter, overview, theme];
