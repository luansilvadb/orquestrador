// Colorização de pares de parênteses, colchetes e chaves, como a do VS Code/Antigravity (`editor.bracketPairColorization`): a cor segue
// o nível de aninhamento, em ciclo de três (dourado, orquídea, azul). Um fechamento sem abertura (`)` solto) fica em vermelho, como o
// `unexpectedBracket` do Antigravity. Nas gramáticas Lezer com nós de par (`closedBy`/`openedBy`), os pares vêm da árvore de sintaxe,
// então o que está dentro de string ou comentário não conta. O `${` de template é uma abertura de dois caracteres (InterpolationStart)
// que fecha com o `}` do InterpolationEnd, como a configuração da linguagem; a expressão dentro dele usa o parser.
// Nas demais (texto puro, Markdown, SQL, modos legados do StreamLanguage), os pares vêm do texto, e os trechos que a árvore marca como
// comentário ou string (`tags.comment`, `tags.string` ou subtags) ficam de fora. Acima do limite de cor do editor, o texto não é lido.
// No mesmo percurso da árvore, o marcador de lista do Markdown (`-`, `*`, `+` ou `1.`) fica em azul-claro, como
// punctuation.definition.list do Dark Modern. O parser põe `tags.list` no nó inteiro da lista (BulletList/OrderedList), e uma regra de
// cor para essa tag pintaria o texto e o código de cada item; por isso o marcador é decorado pelo próprio nó ListMark, e o tema de
// sintaxe não define `tags.list`.
import { RangeSetBuilder } from '@codemirror/state';
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';
import { language, syntaxTree, type Language } from '@codemirror/language';
import { NodeProp, type NodeSet, type SyntaxNode } from '@lezer/common';
import { getStyleTags, tags } from '@lezer/highlight';

const LEVELS = [
  Decoration.mark({ class: 'cm-bracket-1' }),
  Decoration.mark({ class: 'cm-bracket-2' }),
  Decoration.mark({ class: 'cm-bracket-3' }),
];
const UNEXPECTED = Decoration.mark({ class: 'cm-bracket-unexpected' });
const LIST_MARK = Decoration.mark({ class: 'cm-list-mark' });
const BRACKETS = new Set(['(', ')', '[', ']', '{', '}']);
const OPENING: Record<string, string> = { ')': '(', ']': '[', '}': '{' };

// Se a gramática tem nós de par. Só LRParser e MarkdownParser expõem `nodeSet`; o StreamLanguage não, e cai no texto.
const hasPairNodes = new WeakMap<Language, boolean>();
const pairNodes = (lang: Language | null) => {
  if (!lang) return false;
  let has = hasPairNodes.get(lang);
  if (has === undefined) {
    const set = (lang.parser as { nodeSet?: NodeSet }).nodeSet;
    has = !!set?.types.some((t) => t.prop(NodeProp.closedBy));
    hasPairNodes.set(lang, has);
  }
  return has;
};

// Nós são objetos novos a cada acesso na árvore; o mesmo nó pai é identificado pela posição e pelo tipo.
const sameNode = (a: SyntaxNode | null, b: SyntaxNode | null) => !!a && !!b && a.from === b.from && a.to === b.to && a.type === b.type;

/** Decorações dos pares e dos marcadores de lista visíveis. O nível conta desde o começo do documento até o fim da parte visível. */
function build(view: EditorView, limit: number): DecorationSet {
  if (!pairNodes(view.state.facet(language))) return buildFromText(view, limit);
  const builder = new RangeSetBuilder<Decoration>();
  const { from, to } = view.viewport;
  // Abertos ainda sem fechamento: o nó pai de cada um (o fechamento precisa do mesmo pai) e o nível dele.
  const open: { parent: SyntaxNode | null; level: number }[] = [];
  syntaxTree(view.state).iterate({
    to,
    enter: (node) => {
      // Os nós entram em ordem de posição, então marcadores e pares chegam ao builder já em ordem crescente.
      if (node.name === 'ListMark') {
        if (node.from >= from) builder.add(node.from, node.to, LIST_MARK);
        return;
      }
      const opens = !!node.type.prop(NodeProp.closedBy);
      const closes = !!node.type.prop(NodeProp.openedBy);
      if (!opens && !closes) return;
      // Só o caractere do par ou o `${` de template: nós como as tags do JSX também têm `closedBy`, mas não são parênteses.
      const text = view.state.sliceDoc(node.from, node.to);
      const pair = text.length === 1 ? BRACKETS.has(text) : node.name === 'InterpolationStart' && text === '${';
      if (!pair) return;
      const parent = node.node.parent;
      let deco: Decoration;
      if (opens) {
        deco = LEVELS[open.length % 3];
        open.push({ parent, level: open.length });
      } else {
        let at = open.length - 1;
        while (at >= 0 && !sameNode(open[at].parent, parent)) at--;
        if (at >= 0) {
          // Fecha o par e descarta os abertos de dentro dele que nunca fecharam.
          deco = LEVELS[open[at].level % 3];
          open.length = at;
        } else {
          // Fechamento sem abertura no mesmo nó pai: o `)` solto, em vermelho.
          deco = UNEXPECTED;
        }
      }
      if (node.from >= from) builder.add(node.from, node.to, deco);
    },
  });
  return builder.finish();
}

/** Pares lidos do texto, fora dos comentários e strings da árvore, e marcadores de lista; para gramáticas sem nós de par. */
function buildFromText(view: EditorView, limit: number): DecorationSet {
  const { from, to } = view.viewport;
  const marks: { from: number; to: number; deco: Decoration }[] = [];
  // Trechos de comentário e string, em ordem e sem sobreposição (o percurso não entra neles).
  const skip: { from: number; to: number }[] = [];
  syntaxTree(view.state).iterate({
    to,
    enter: (node) => {
      if (node.name === 'ListMark') {
        if (node.from >= from) marks.push({ from: node.from, to: node.to, deco: LIST_MARK });
        return;
      }
      const style = getStyleTags(node);
      if (style?.tags.some((t) => t.set.includes(tags.comment) || t.set.includes(tags.string))) {
        skip.push({ from: node.from, to: node.to });
        return false;
      }
    },
  });
  if (view.state.doc.length <= limit) {
    const open: { ch: string; level: number }[] = [];
    let pos = 0;
    let s = 0;
    for (const iter = view.state.doc.iterRange(0, to); !iter.next().done; pos += iter.value.length) {
      const chunk = iter.value;
      for (let i = 0; i < chunk.length; i++) {
        const ch = chunk[i];
        if (!BRACKETS.has(ch)) continue;
        const at = pos + i;
        while (s < skip.length && skip[s].to <= at) s++;
        if (s < skip.length && skip[s].from <= at) continue;
        let deco: Decoration;
        if (!OPENING[ch]) {
          deco = LEVELS[open.length % 3];
          open.push({ ch, level: open.length });
        } else {
          // Como no caminho da árvore: fecha a abertura do mesmo tipo mais próxima e descarta as de dentro; sem ela, vermelho.
          let top = open.length - 1;
          while (top >= 0 && open[top].ch !== OPENING[ch]) top--;
          if (top >= 0) {
            deco = LEVELS[open[top].level % 3];
            open.length = top;
          } else {
            deco = UNEXPECTED;
          }
        }
        if (at >= from) marks.push({ from: at, to: at + 1, deco });
      }
    }
  }
  const builder = new RangeSetBuilder<Decoration>();
  for (const m of marks.sort((a, b) => a.from - b.from)) builder.add(m.from, m.to, m.deco);
  return builder.finish();
}

/** `limit`: tamanho do documento acima do qual os pares não são lidos do texto (o limite de cor do editor). */
export const bracketColors = (limit: number) => [
  ViewPlugin.fromClass(class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = build(view, limit);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged || syntaxTree(u.state) !== syntaxTree(u.startState)) this.decorations = build(u.view, limit);
    }
  }, { decorations: (v) => v.decorations }),
  // Cores do Dark Modern (editorBracketHighlight.foreground1 a 3 e unexpectedBracket). O `*` vale para o texto que o tema de sintaxe
  // pinta dentro do mesmo marcador (ele e o `cm-matchingBracket` do CodeMirror ficam entre o marcador e o caractere). O marcador de
  // lista tem a cor de punctuation.definition.list; o seletor do tema tem duas classes (`.ͼ1 .cm-list-mark`), então vence a cor da
  // sintaxe, que é de uma classe só.
  EditorView.theme({
    '.cm-bracket-1, .cm-bracket-1 *': { color: '#FFD700' },
    '.cm-bracket-2, .cm-bracket-2 *': { color: '#DA70D6' },
    '.cm-bracket-3, .cm-bracket-3 *': { color: '#179FFF' },
    '.cm-bracket-unexpected, .cm-bracket-unexpected *': { color: 'rgba(255, 18, 18, 0.8)' },
    '.cm-list-mark': { color: '#6796E6' },
  }),
];
