// Marcador de lista do Markdown (`-`, `*`, `+` ou `1.`) em azul-claro, como punctuation.definition.list do Dark Modern. O parser põe
// `tags.list` no nó inteiro da lista (BulletList/OrderedList), e uma regra de cor para essa tag pintaria o texto e o código de cada
// item; por isso o marcador é decorado pelo próprio nó ListMark, e o tema não define `tags.list`. Sem Markdown, nada é pintado.
import { RangeSetBuilder } from '@codemirror/state';
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';
import { syntaxTree } from '@codemirror/language';

const MARK = Decoration.mark({ class: 'cm-list-mark' });

/** Decorações dos marcadores visíveis. */
function build(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const { from, to } = view.viewport;
  syntaxTree(view.state).iterate({
    to,
    enter: (node) => {
      if (node.name === 'ListMark' && node.from >= from) builder.add(node.from, node.to, MARK);
    },
  });
  return builder.finish();
}

export const listMarkers = [
  ViewPlugin.fromClass(class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = build(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged || syntaxTree(u.state) !== syntaxTree(u.startState)) this.decorations = build(u.view);
    }
  }, { decorations: (v) => v.decorations }),
  // Cor de punctuation.definition.list do Dark Modern. O seletor do tema tem duas classes (`.ͼ1 .cm-list-mark`), então vence a cor
  // da sintaxe, que é de uma classe só.
  EditorView.theme({ '.cm-list-mark': { color: '#6796E6' } }),
];
