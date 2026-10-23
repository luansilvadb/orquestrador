// Bloco aberto de um marcador de mudança do Git (git-gutter.ts), como o do Zed/WU: clicar no marcador, ou Alt+F5, abre o trecho ali
// mesmo. Acima da primeira linha do bloco (no apagado, na fronteira), um widget de bloco do CodeMirror mostra as linhas antigas do HEAD
// em vermelho, com a fonte e a sintaxe do arquivo; as linhas novas ficam em verde. A barra do widget prepara ou tira do preparo
// (git-stage.ts, gravado pelo `gitIndexWriter` de quem monta o editor), restaura as linhas do HEAD no buffer (edição normal, entra no
// Desfazer) e vai para a alteração anterior/próxima, dando a volta no arquivo. O widget não tem número de linha: a numeração do buffer
// não muda. Com o bloco fechado não há decoração nenhuma, então a geometria do texto é a mesma.
// Cores: diffEditor.removedLineBackground e insertedLineBackground do VS Code (src/vs/platform/theme/common/colors/editorColors.ts:
// defaultRemoveColor rgba(255, 0, 0, .2) e defaultInsertColor rgba(155, 185, 85, .2); o Dark Modern não muda nenhuma das duas).
import { Facet, Prec, StateField, type EditorState } from '@codemirror/state';
import { Decoration, type DecorationSet, EditorView, keymap, ViewPlugin, WidgetType } from '@codemirror/view';
import { highlightingFor, language } from '@codemirror/language';
import { highlightTree } from '@lezer/highlight';
import { gitDiff, gitHunks, openGitHunk, openHunk, type Hunk } from './git-gutter';
import { stageHunk, unstageHunk } from './git-stage';

const REMOVED = '#FF000033';
const INSERTED = '#9BB95533';
// Linhas antigas mostradas; o resto vira "… mais N linhas". Acima de tantos caracteres, as antigas ficam sem cor de sintaxe.
const OLD_LIMIT = 200;
const HIGHLIGHT_LIMIT = 20_000;
const LINE = 19;
const BAR = 22;

/** Grava o texto novo do índice (`expected` é o índice que a tela viu). Sem ele, o bloco não prepara (rascunho, conflito). */
export const gitIndexWriter = Facet.define<(text: string, expected: string) => Promise<void>, ((text: string, expected: string) => Promise<void>) | null>({
  combine: (values) => values[0] ?? null,
});

/** Vai para a alteração seguinte (`1`) ou anterior (`-1`), com o bloco aberto, dando a volta no arquivo. `false` sem alterações. */
export function gotoGitHunk(view: EditorView, dir: 1 | -1) {
  const hunks = gitHunks(view.state);
  if (!hunks.length) return false;
  const { doc } = view.state;
  const open = openHunk(view.state);
  let i: number;
  if (open) i = (hunks.indexOf(open) + dir + hunks.length) % hunks.length;
  else {
    // Sem bloco aberto, conta a partir da linha do cursor, como o VS Code.
    const line = doc.lineAt(view.state.selection.main.head).number;
    i = dir > 0 ? hunks.findIndex((h) => h.from > line) : hunks.findLastIndex((h) => h.from < line);
    if (i < 0) i = dir > 0 ? 0 : hunks.length - 1;
  }
  const h = hunks[i];
  const at = doc.line(h.from).from;
  view.dispatch({ selection: { anchor: at }, effects: [openGitHunk.of(h.chunk.fromB), EditorView.scrollIntoView(at, { y: 'center' })] });
  view.focus();
  return true;
}

/** Fecha o bloco aberto. `false` se não havia nenhum. */
export function closeGitHunk(view: EditorView) {
  if (!openHunk(view.state)) return false;
  view.dispatch({ effects: openGitHunk.of(null) });
  return true;
}

/** Troca as linhas do bloco aberto pelas do HEAD, como edição normal (Desfazer, buffer sujo; não grava disco nem índice). */
export function restoreGitHunk(view: EditorView) {
  const h = openHunk(view.state);
  const diff = gitDiff(view.state);
  if (!h || !diff || view.state.readOnly) return false;
  const { chunk } = h;
  // Posições "tamanho + 1" (trecho até o fim) caem no tamanho, como em git-stage.ts.
  const insert = diff.head.sliceString(chunk.fromA, Math.min(chunk.toA, diff.head.length));
  view.dispatch({ changes: { from: chunk.fromB, to: Math.min(chunk.toB, view.state.doc.length), insert }, userEvent: 'input.restore' });
  view.focus();
  return true;
}

// Preparar (ou tirar do preparo, se o bloco já está preparado): calcula o índice novo com o texto do buffer, inclusive não salvo.
async function stageOpen(view: EditorView) {
  const h = openHunk(view.state);
  const diff = gitDiff(view.state);
  const write = view.state.facet(gitIndexWriter);
  if (!h || !diff || !write || diff.source.index === null) return;
  const index = diff.source.index;
  const buffer = view.state.doc.toString();
  await write(h.staged ? unstageHunk(diff.source.head ?? '', index, buffer, h.chunk) : stageHunk(index, buffer, h.chunk), index);
}

// Ícones do lucide (os mesmos do painel Git: Plus, Minus, Undo2, ChevronUp, ChevronDown e X), em SVG puro: o widget é DOM do CodeMirror.
const ICON: Record<string, string[]> = {
  plus: ['M5 12h14', 'M12 5v14'],
  minus: ['M5 12h14'],
  undo: ['M9 14 4 9l5-5', 'M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11'],
  up: ['m18 15-6-6-6 6'],
  down: ['m6 9 6 6 6-6'],
  x: ['M18 6 6 18', 'm6 6 12 12'],
};
const SVG = 'http://www.w3.org/2000/svg';
function icon(name: string) {
  const svg = document.createElementNS(SVG, 'svg');
  for (const [k, v] of Object.entries({ viewBox: '0 0 24 24', width: '14', height: '14', fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'aria-hidden': 'true' })) svg.setAttribute(k, v);
  for (const d of ICON[name]) {
    const path = document.createElementNS(SVG, 'path');
    path.setAttribute('d', d);
    svg.appendChild(path);
  }
  return svg;
}

const el = (tag: string, cls: string, text?: string) => {
  const node = document.createElement(tag);
  node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};

// Linhas antigas com a sintaxe do arquivo: o parser da linguagem atual lê só o trecho antigo (fora de contexto, então um trecho no meio
// de um comentário ou de uma string longa pode sair com cor diferente; é o "se for simples").
function oldLines(view: EditorView, text: string) {
  const marks: { from: number; to: number; cls: string }[] = [];
  const lang = view.state.facet(language);
  if (lang && text.length <= HIGHLIGHT_LIMIT) {
    highlightTree(lang.parser.parse(text), { style: (tags) => highlightingFor(view.state, tags) }, (from, to, cls) => marks.push({ from, to, cls }));
  }
  const out: HTMLElement[] = [];
  let pos = 0;
  let m = 0;
  for (const line of text.split('\n')) {
    const div = el('div', 'cm-gitHunk-line');
    const end = pos + line.length;
    let at = pos;
    while (m < marks.length && marks[m].to <= pos) m++;
    for (let k = m; k < marks.length && marks[k].from < end; k++) {
      const from = Math.max(marks[k].from, pos);
      const to = Math.min(marks[k].to, end);
      if (from > at) div.append(text.slice(at, from));
      div.append(el('span', marks[k].cls, text.slice(from, to)));
      at = to;
    }
    if (at < end) div.append(text.slice(at, end));
    if (line === '') div.append(document.createElement('br'));
    out.push(div);
    pos = end + 1;
  }
  return out;
}

type Info = { old: string[]; staged: boolean; at: number; total: number; canStage: boolean; readOnly: boolean; lang: unknown };

class HunkWidget extends WidgetType {
  constructor(readonly info: Info) {
    super();
  }
  eq(other: HunkWidget) {
    const a = this.info, b = other.info;
    return a.staged === b.staged && a.at === b.at && a.total === b.total && a.canStage === b.canStage && a.readOnly === b.readOnly
      && a.lang === b.lang && a.old.length === b.old.length && a.old.every((l, i) => l === b.old[i]);
  }
  get estimatedHeight() {
    // A barra, as linhas e as duas bordas de 1 px.
    return BAR + Math.min(this.info.old.length, OLD_LIMIT + 1) * LINE + 2;
  }
  toDOM(view: EditorView) {
    const { info } = this;
    const dom = el('div', 'cm-gitHunk');
    dom.setAttribute('role', 'group');
    dom.setAttribute('aria-label', `Alteração ${info.at} de ${info.total}`);
    const bar = el('div', 'cm-gitHunk-bar');
    bar.append(el('span', 'cm-gitHunk-title', `Alteração ${info.at} de ${info.total}${info.staged ? ' · preparada' : ''}`));
    const buttons: HTMLButtonElement[] = [];
    // `aria-disabled` em vez de `disabled`: o botão bloqueado continua mostrando o motivo no `title`.
    const button = (cls: string, label: string, title: string, iconName: string, run: () => void | Promise<void>, blocked?: string, text?: string) => {
      const b = el('button', `cm-gitHunk-button ${cls}`) as HTMLButtonElement;
      b.type = 'button';
      b.title = blocked ?? title;
      b.setAttribute('aria-label', label);
      if (blocked) b.setAttribute('aria-disabled', 'true');
      b.append(icon(iconName));
      if (text) b.append(el('span', '', text));
      // O foco fica no texto (o Esc continua fechando) e o clique não mexe na seleção.
      b.addEventListener('mousedown', (e) => e.preventDefault());
      b.addEventListener('click', async () => {
        if (b.getAttribute('aria-disabled') === 'true' || dom.classList.contains('cm-gitHunk-busy')) return;
        dom.classList.add('cm-gitHunk-busy');
        try {
          await run();
        } finally {
          dom.classList.remove('cm-gitHunk-busy');
        }
      });
      buttons.push(b);
      bar.append(b);
    };
    if (view.state.facet(gitIndexWriter)) {
      const blocked = info.readOnly ? 'O arquivo é somente leitura.' : info.canStage ? undefined : 'Sem índice para este arquivo (conflito): não há como preparar.';
      if (info.staged) button('cm-gitHunk-stage', 'Tirar do preparo', 'Tirar este bloco do preparo', 'minus', () => stageOpen(view), blocked, 'Tirar do preparo');
      else button('cm-gitHunk-stage', 'Preparar', 'Preparar este bloco (o texto do editor, mesmo não salvo)', 'plus', () => stageOpen(view), blocked, 'Preparar');
    }
    button('cm-gitHunk-restore', 'Restaurar', 'Restaurar as linhas do HEAD neste bloco (Ctrl+Z desfaz)', 'undo', () => void restoreGitHunk(view),
      info.readOnly ? 'O arquivo é somente leitura.' : undefined, 'Restaurar');
    button('cm-gitHunk-prev', 'Alteração anterior', 'Alteração anterior (Shift+Alt+F5)', 'up', () => void gotoGitHunk(view, -1));
    button('cm-gitHunk-next', 'Próxima alteração', 'Próxima alteração (Alt+F5)', 'down', () => void gotoGitHunk(view, 1));
    button('cm-gitHunk-close', 'Fechar', 'Fechar (Esc)', 'x', () => { closeGitHunk(view); view.focus(); });
    dom.append(bar);
    if (info.old.length) {
      const old = el('div', 'cm-gitHunk-old');
      old.append(...oldLines(view, info.old.slice(0, OLD_LIMIT).join('\n')));
      if (info.old.length > OLD_LIMIT) {
        const rest = info.old.length - OLD_LIMIT;
        old.append(el('div', 'cm-gitHunk-line cm-gitHunk-more', `… mais ${rest} ${rest === 1 ? 'linha' : 'linhas'}`));
      }
      dom.append(old);
    }
    return dom;
  }
}

// Linhas antigas do bloco (o trecho A do chunk, sem a quebra que o fecha; o trecho até o fim do HEAD não tem essa quebra).
function oldOf(h: Hunk, head: EditorState['doc']) {
  const { chunk } = h;
  if (chunk.fromA === chunk.toA) return [];
  const text = head.sliceString(chunk.fromA, Math.min(chunk.toA, head.length));
  return (chunk.toA <= head.length && text.endsWith('\n') ? text.slice(0, -1) : text).split('\n');
}

function build(state: EditorState): DecorationSet {
  const h = openHunk(state);
  const diff = gitDiff(state);
  if (!h || !diff) return Decoration.none;
  const hunks = gitHunks(state);
  const { doc } = state;
  const widget = new HunkWidget({
    old: oldOf(h, diff.head), staged: h.staged, at: hunks.indexOf(h) + 1, total: hunks.length,
    canStage: diff.source.index !== null, readOnly: state.readOnly, lang: state.facet(language),
  });
  const ranges = [h.kind === 'deleted' && h.end
    ? Decoration.widget({ widget, block: true, side: 1 }).range(doc.length)
    : Decoration.widget({ widget, block: true, side: -1 }).range(doc.line(h.from).from)];
  if (h.kind !== 'deleted') for (let n = h.from; n <= h.to; n++) ranges.push(Decoration.line({ class: 'cm-gitHunk-new' }).range(doc.line(n).from));
  return Decoration.set(ranges, true);
}

// Widget de bloco tem de vir de um StateField (não de um ViewPlugin). Só recalcula quando algo que muda o bloco pode ter mudado: texto,
// efeitos (base nova, abrir/fechar) ou reconfiguração (linguagem, somente leitura).
const hunkField = StateField.define<DecorationSet>({
  create: build,
  update: (deco, tr) => (tr.docChanged || tr.effects.length || tr.reconfigured ? build(tr.state) : deco),
  provide: (f) => EditorView.decorations.from(f),
});

// A barra fica à direita da área visível do texto, mesmo com linhas longas (o widget tem a largura do conteúdo): é `sticky` a partir
// dos gutters da esquerda, com a largura visível entre eles e os outros gutters (o minimap, que fica por cima do conteúdo, à direita).
const barPlace = ViewPlugin.fromClass(class {
  constructor(readonly view: EditorView) {
    this.measure();
  }
  update(u: { geometryChanged: boolean; view: EditorView }) {
    if (u.geometryChanged) this.measure();
  }
  measure() {
    this.view.requestMeasure({
      key: this,
      read: (view) => {
        let before = 0, after = 0;
        for (const g of view.scrollDOM.querySelectorAll<HTMLElement>(':scope > .cm-gutters')) {
          if (g.classList.contains('cm-gutters-before')) before += g.offsetWidth;
          else after += g.offsetWidth;
        }
        return { left: before, width: Math.max(view.scrollDOM.clientWidth - before - after, 0) };
      },
      write: ({ left, width }, view) => {
        view.dom.style.setProperty('--git-hunk-left', `${left}px`);
        view.dom.style.setProperty('--git-hunk-width', `${width}px`);
      },
    });
  }
});

const hunkKeymap = Prec.high(keymap.of([
  { key: 'Escape', run: closeGitHunk },
  { key: 'Alt-F5', run: (view) => gotoGitHunk(view, 1) },
  { key: 'Shift-Alt-F5', run: (view) => gotoGitHunk(view, -1) },
]));

// Linhas antigas com a mesma fonte, tamanho e altura do editor (herdados do `.cm-scroller`); a barra usa a fonte da interface, pequena,
// com os botões no tamanho dos ActionIcon pequenos do app. Tons neutros fixos (hex), como o resto do tema do editor.
const theme = EditorView.theme({
  '.cm-gitHunk': { borderTop: '1px solid #454545', borderBottom: '1px solid #454545' },
  '.cm-gitHunk-bar': {
    position: 'sticky', left: 'var(--git-hunk-left, 0px)', width: 'var(--git-hunk-width, 100%)', boxSizing: 'border-box',
    display: 'flex', alignItems: 'center', justifyContent: 'flex-end', gap: '2px', height: `${BAR}px`, padding: '0 8px',
    fontFamily: "'Inter Variable', Inter, sans-serif", fontSize: '12px', lineHeight: `${BAR}px`, color: '#CCCCCC',
  },
  '.cm-gitHunk-title': { marginRight: 'auto', color: '#9D9D9D', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' },
  '.cm-gitHunk-button': {
    display: 'inline-flex', alignItems: 'center', gap: '4px', height: '20px', minWidth: '20px', padding: '0 3px', border: 'none',
    borderRadius: '4px', background: 'transparent', color: 'inherit', font: 'inherit', cursor: 'pointer', whiteSpace: 'nowrap',
  },
  '.cm-gitHunk-button:has(span)': { padding: '0 6px 0 4px' },
  '.cm-gitHunk-button:hover': { background: 'rgba(255, 255, 255, 0.1)' },
  '.cm-gitHunk-button[aria-disabled="true"]': { opacity: '0.4', cursor: 'default', background: 'transparent' },
  '.cm-gitHunk-busy .cm-gitHunk-button': { opacity: '0.4', cursor: 'progress' },
  '.cm-gitHunk-old': { background: REMOVED },
  '.cm-gitHunk-line': { whiteSpace: 'pre', minHeight: `${LINE}px` },
  '.cm-gitHunk-more': { color: '#9D9D9D', fontStyle: 'italic' },
  '.cm-line.cm-gitHunk-new': { backgroundColor: INSERTED },
});

/** Extensão do bloco aberto: decorações, posição da barra, teclado (Esc, Alt+F5, Shift+Alt+F5) e tema. */
export const gitHunkView = [hunkField, barPlace, hunkKeymap, theme];
