import { type EditorState, RangeSetBuilder } from '@codemirror/state';
import { Decoration, type DecorationSet, type EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';

// Linha quebrada continua no recuo da original, como o `wrappingIndent: "same"` do VS Code. O padding empurra a linha inteira e o
// text-indent negativo devolve a primeira parte ao lugar; o recuo em `ch` vale porque a fonte do editor é monoespaçada. Os guias de
// recuo não se mexem: o `::before` deles é absoluto, medido da borda da linha, não do padding.
function indentOf(text: string, tabSize: number) {
  let columns = 0;
  for (const ch of text) {
    if (ch === ' ') columns += 1;
    else if (ch === '\t') columns += tabSize - (columns % tabSize);
    else return columns;
  }
  return 0; // linha só de espaços: nada a continuar
}

function build(view: EditorView) {
  const builder = new RangeSetBuilder<Decoration>();
  const state: EditorState = view.state;
  for (const { from, to } of view.visibleRanges) {
    for (let pos = from; pos <= to;) {
      const line = state.doc.lineAt(pos);
      const columns = indentOf(line.text, state.tabSize);
      if (columns) builder.add(line.from, line.from, Decoration.line({ attributes: { style: `padding-left: ${columns}ch; text-indent: -${columns}ch` } }));
      pos = line.to + 1;
    }
  }
  return builder.finish();
}

export const wrapIndent = ViewPlugin.fromClass(class {
  decorations: DecorationSet;
  constructor(view: EditorView) {
    this.decorations = build(view);
  }
  update(update: ViewUpdate) {
    if (update.docChanged || update.viewportChanged) this.decorations = build(update.view);
  }
}, { decorations: (plugin) => plugin.decorations });
