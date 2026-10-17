// Colorização de pares de parênteses, colchetes e chaves, como a do VS Code/Antigravity (`editor.bracketPairColorization`): a cor segue
// o nível de aninhamento, em ciclo de três (dourado, orquídea, azul). Um fechamento sem abertura (`)` solto) fica em vermelho, como o
// `unexpectedBracket` do Antigravity. Os pares vêm da árvore de sintaxe (nós com `closedBy`/`openedBy`), então o que está dentro de
// string ou comentário não conta. O `${` de template é uma abertura de dois caracteres (InterpolationStart) que fecha com o `}` do
// InterpolationEnd, como a configuração da linguagem; a expressão dentro dele usa o parser. Sem parser (arquivo grande ou linguagem
// desconhecida), nada é pintado.
// No mesmo percurso da árvore, o marcador de lista do Markdown (`-`, `*`, `+` ou `1.`) fica em azul-claro, como
// punctuation.definition.list do Dark Modern. O parser põe `tags.list` no nó inteiro da lista (BulletList/OrderedList), e uma regra de
// cor para essa tag pintaria o texto e o código de cada item; por isso o marcador é decorado pelo próprio nó ListMark, e o tema de
// sintaxe não define `tags.list`.
import { RangeSetBuilder } from '@codemirror/state';
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';
import { syntaxTree } from '@codemirror/language';
import { NodeProp, type SyntaxNode } from '@lezer/common';

const LEVELS = [
  Decoration.mark({ class: 'cm-bracket-1' }),
  Decoration.mark({ class: 'cm-bracket-2' }),
  Decoration.mark({ class: 'cm-bracket-3' }),
];
const UNEXPECTED = Decoration.mark({ class: 'cm-bracket-unexpected' });
const LIST_MARK = Decoration.mark({ class: 'cm-list-mark' });
const BRACKETS = new Set(['(', ')', '[', ']', '{', '}']);

// Nós são objetos novos a cada acesso na árvore; o mesmo nó pai é identificado pela posição e pelo tipo.
const sameNode = (a: SyntaxNode | null, b: SyntaxNode | null) => !!a && !!b && a.from === b.from && a.to === b.to && a.type === b.type;

/** Decorações dos pares e dos marcadores de lista visíveis. O nível conta desde o começo do documento até o fim da parte visível. */
function build(view: EditorView): DecorationSet {
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
