// Colorização de pares de parênteses, colchetes e chaves, como a do VS Code (`editor.bracketPairColorization`): a cor segue o nível
// de aninhamento, em ciclo de três (dourado, orquídea, azul). Os pares vêm da árvore de sintaxe (nós com `closedBy`/`openedBy`),
// então o que está dentro de string ou comentário não conta. Sem parser (arquivo grande ou linguagem desconhecida), nada é pintado.
import { RangeSetBuilder } from '@codemirror/state';
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';
import { syntaxTree } from '@codemirror/language';
import { NodeProp } from '@lezer/common';

const LEVELS = [
  Decoration.mark({ class: 'cm-bracket-1' }),
  Decoration.mark({ class: 'cm-bracket-2' }),
  Decoration.mark({ class: 'cm-bracket-3' }),
];
const BRACKETS = new Set(['(', ')', '[', ']', '{', '}']);

/** Decorações dos pares visíveis. O nível conta desde o começo do documento até o fim da parte visível. */
function build(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const { from, to } = view.viewport;
  let depth = 0;
  syntaxTree(view.state).iterate({
    to,
    enter: (node) => {
      const opens = !!node.type.prop(NodeProp.closedBy);
      const closes = !!node.type.prop(NodeProp.openedBy);
      if (!opens && !closes) return;
      // Só o caractere do par: nós como as tags do JSX também têm `closedBy`, mas não são parênteses.
      if (node.to - node.from !== 1 || !BRACKETS.has(view.state.sliceDoc(node.from, node.to))) return;
      if (closes) depth = Math.max(depth - 1, 0);
      if (node.from >= from) builder.add(node.from, node.to, LEVELS[depth % 3]);
      if (opens) depth++;
    },
  });
  return builder.finish();
}

export const bracketColors = [
  ViewPlugin.fromClass(class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = build(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged || syntaxTree(u.state) !== syntaxTree(u.startState)) this.decorations = build(u.view);
    }
  }, { decorations: (v) => v.decorations }),
  // Cores do Dark Modern (editorBracketHighlight.foreground1 a 3).
  EditorView.theme({
    '.cm-bracket-1, .cm-bracket-1 > span': { color: '#ffd700' },
    '.cm-bracket-2, .cm-bracket-2 > span': { color: '#da70d6' },
    '.cm-bracket-3, .cm-bracket-3 > span': { color: '#179fff' },
  }),
];
