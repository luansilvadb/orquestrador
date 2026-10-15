// Editor de texto (CodeMirror com o tema do app), usado pelo editor central (src/editor-workspace.tsx). Não lê nem grava o disco:
// o texto, o cursor e a rolagem vêm de fora, e o estado do CodeMirror fica guardado por `cacheKey` (o docId).
import { useEffect, useRef, useState } from 'react';
import { ContextMenuTrigger, type ContextMenuItem, toast } from '@lobehub/ui/base-ui';
import { basicSetup } from 'codemirror';
import { Compartment, EditorSelection, EditorState, Prec, StateEffect } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { copyLineDown, deleteLine, indentWithTab, moveLineDown, moveLineUp, redo, selectAll, toggleComment, undo } from '@codemirror/commands';
import { openSearchPanel, selectNextOccurrence } from '@codemirror/search';
import { LanguageDescription, syntaxHighlighting, HighlightStyle } from '@codemirror/language';
import { tags } from '@lezer/highlight';
import { indentationMarkers } from '@replit/codemirror-indentation-markers';
import { showMinimap } from '@replit/codemirror-minimap';
import { bracketColors } from './bracket-colors';
import { listMarkers } from './list-markers';
import { message } from './backend';
import { action, blocked, DIVIDER } from './file-tabs';
import type { Scroll } from './workspace-store';

// ponytail: acima de 200 mil caracteres, carregar parser e colorir custa mais que a leitura simples.
const COLOR_LIMIT = 200_000;
// Quantos ecos de digitação guardar: a renderização atrasada nunca passa de algumas teclas.
const ECHO_LIMIT = 64;
// Medidas e cores do editor do Antigravity (Monaco), tema Dark Modern: texto de 14 px em linhas de 19 px, Consolas no Windows, sem
// ligaduras (editor.fontLigatures é false por padrão); à esquerda, 19 px da margem de ícones, 31 px dos números (alinhados à direita)
// e 26 px da dobra, que só aparece com o ponteiro na medianiz; a linha do cursor tem só uma borda, sem fundo. As cores são fixas (hex),
// de dark_modern.json, dark_vs.json e do bundle do workbench (ver docs/interface.md, "Cores e fontes do editor"): não herdam os tokens
// do antd, que mudam com o tema da interface. As barras de rolagem têm 14 px (vertical) e 12 px (horizontal), quadradas.
const darkTheme = EditorView.theme({
  '&': {
    height: '100%',
    color: '#CCCCCC',
    backgroundColor: '#1F1F1F',
    fontSize: '14px',
  },
  '.cm-scroller': {
    overflow: 'auto',
    fontFamily: "Consolas, 'Courier New', monospace",
    fontWeight: '400',
    fontVariantLigatures: 'none',
    lineHeight: '19px',
  },
  '.cm-scroller::-webkit-scrollbar': { width: '14px', height: '12px' },
  '.cm-scroller::-webkit-scrollbar-track, .cm-scroller::-webkit-scrollbar-corner': { background: 'transparent' },
  '.cm-scroller::-webkit-scrollbar-thumb': { background: 'rgb(121 121 121 / 0.4)' },
  '.cm-scroller::-webkit-scrollbar-thumb:hover': { background: 'rgb(100 100 100 / 0.7)' },
  '.cm-scroller::-webkit-scrollbar-thumb:active': { background: 'rgb(191 191 191 / 0.4)' },
  '.cm-content': {
    padding: '0',
    caretColor: '#AEAFAD',
  },
  '.cm-line': { padding: '0' },
  '.cm-gutters': {
    color: '#6E7681',
    backgroundColor: '#1F1F1F',
    border: 'none',
  },
  '.cm-lineNumbers .cm-gutterElement': { padding: '0 0 0 19px', minWidth: '50px', textAlign: 'right' },
  '.cm-foldGutter .cm-gutterElement': { width: '26px', paddingLeft: '6px', opacity: '0', transition: 'opacity 0.1s' },
  '.cm-gutters:hover .cm-foldGutter .cm-gutterElement': { opacity: '1' },
  '.cm-activeLineGutter': { backgroundColor: 'transparent', color: '#CCCCCC' },
  '.cm-activeLine': { backgroundColor: 'transparent', boxShadow: 'inset 0 0 0 2px #282828' },
  '&.cm-focused': { outline: 'none' },
  // Seleção com foco é #264F78 e sem foco é #3A3D41 (inactiveSelectionBackground). O seletor com `&.cm-focused` precisa ser o mesmo
  // do CodeMirror, que tem mais especificidade e venceria um seletor simples.
  '.cm-selectionBackground': { backgroundColor: '#3A3D41' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground': { backgroundColor: '#264F78' },
  // Par sob o cursor: fundo verde translúcido e caixa #888 (editorBracketMatch). A caixa é sombra interna, não borda: uma borda real
  // aumentaria a largura inline do caractere em 2 px e empurraria o texto. Sem par, o `nonmatching` só muda a cor, como o vermelho.
  '.cm-matchingBracket, &.cm-focused .cm-matchingBracket': { backgroundColor: '#0064001A', boxShadow: 'inset 0 0 0 1px #888' },
  '.cm-nonmatchingBracket, &.cm-focused .cm-nonmatchingBracket': { backgroundColor: 'transparent' },
  '.cm-nonmatchingBracket, .cm-nonmatchingBracket *': { color: 'rgba(255, 18, 18, 0.8)' },
}, { dark: true });

// Minimap à direita, como o do Antigravity: 120 px, com os caracteres desenhados e a janela visível só com o ponteiro. Arquivo
// acima do limite de cor fica sem ele (desenhar tudo custa como colorir). O objeto é fixo para o facet não recriar o minimap a cada tecla.
// O minimap só redesenha quando o texto ou a configuração dele mudam; por isso, quando a linguagem chega (depois, assíncrona), ele
// recebe uma configuração nova (`minimap()` outra vez), e assim passa a desenhar com as cores da sintaxe.
const MINIMAP = { create: () => ({ dom: document.createElement('div') }), displayText: 'characters' as const, showOverlay: 'mouse-over' as const };
const minimap = () => {
  const config = { ...MINIMAP };
  return showMinimap.compute(['doc'], (state) => (state.doc.length > COLOR_LIMIT ? null : config));
};

// Cores de sintaxe do tema padrão do Antigravity (Dark Modern, que herda as do Dark+), lidas do editor dele: controle de fluxo e
// import/export em rosa, declarações e literais em azul, variáveis e propriedades em azul-claro, funções em amarelo, tipos em verde-água.
// O minimap desenha com estas mesmas cores.
const darkHighlight = HighlightStyle.define([
  { tag: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment], color: '#6a9955' },
  { tag: [tags.keyword, tags.definitionKeyword, tags.modifier, tags.operatorKeyword, tags.self, tags.bool, tags.null, tags.atom], color: '#569cd6' },
  { tag: [tags.controlKeyword, tags.moduleKeyword], color: '#c586c0' },
  { tag: [tags.string, tags.special(tags.string), tags.docString, tags.attributeValue], color: '#ce9178' },
  { tag: tags.regexp, color: '#d16969' },
  { tag: [tags.escape, tags.character], color: '#d7ba7d' },
  { tag: [tags.number, tags.integer, tags.float, tags.unit], color: '#b5cea8' },
  { tag: [tags.variableName, tags.propertyName, tags.attributeName, tags.definition(tags.variableName), tags.special(tags.variableName)], color: '#9cdcfe' },
  { tag: tags.constant(tags.variableName), color: '#4fc1ff' },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName), tags.function(tags.definition(tags.variableName)), tags.annotation], color: '#dcdcaa' },
  { tag: [tags.typeName, tags.className, tags.namespace, tags.standard(tags.typeName)], color: '#4ec9b0' },
  { tag: tags.tagName, color: '#569cd6' },
  { tag: tags.angleBracket, color: '#808080' },
  { tag: tags.labelName, color: '#c8c8c8' },
  { tag: [tags.operator, tags.derefOperator], color: '#d4d4d4' },
  { tag: [tags.punctuation, tags.separator], color: '#cccccc' },
  { tag: tags.invalid, color: '#f44747' },
  // Markdown (markup.heading, markup.bold e markup.inline.raw do Dark Modern): título e negrito azuis, código inline salmão
  // (tags.monospace é o que o Markdown usa para `código`). O marcador de lista tem cor própria (list-markers.ts): `tags.list` do
  // parser cobre a lista inteira, então não entra aqui.
  { tag: tags.heading, color: '#569cd6', fontWeight: 'bold' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strong, color: '#569cd6', fontWeight: 'bold' },
  { tag: tags.monospace, color: '#ce9178' },
  { tag: tags.strikethrough, textDecoration: 'line-through' },
  { tag: [tags.link, tags.url], color: '#ce9178', textDecoration: 'underline' },
  { tag: tags.quote, color: '#6a9955' },
]);
const basicExtensions = [
  basicSetup,
  keymap.of([indentWithTab]),
  darkTheme,
  syntaxHighlighting(darkHighlight),
  bracketColors,
  listMarkers,
  // Guias de recuo de 1 px; a do bloco do cursor fica mais forte.
  indentationMarkers({
    thickness: 1,
    // editorIndentGuide.background1 e activeBackground1 do dark_vs.json (o app é sempre escuro).
    colors: {
      dark: '#404040', activeDark: '#707070',
      light: '#404040', activeLight: '#707070',
    },
  }),
];


// Pedido de navegação a uma linha (1-based) e coluna opcional (1-based, em unidades UTF-16), vindo de um resultado da pesquisa. `id` é novo a cada clique.
export type Reveal = { path: string; line: number; col?: number; id: number };

// Estado do CodeMirror por buffer (`cacheKey`). Ao mover a aba para outro grupo o React desmonta a vista; o estado guardado
// mantém o histórico de Desfazer e a seleção. Quem fecha a aba ou o buffer chama `forgetEditorState`.
const savedStates = new Map<string, EditorState>();
export function forgetEditorState(cacheKey: string) {
  savedStates.delete(cacheKey);
}
// Vistas montadas, pela mesma chave (docId). Mesmo arquivo pode estar em dois grupos: a última focada fica no fim da lista.
// Só o editor central lê daqui (texto selecionado); cada vista sai da lista ao desmontar.
const liveViews = new Map<string, EditorView[]>();
export function liveEditorView(cacheKey: string): EditorView | null {
  const list = liveViews.get(cacheKey);
  return list?.[list.length - 1] ?? null;
}
function markLive(cacheKey: string, view: EditorView, focused: boolean) {
  const list = (liveViews.get(cacheKey) ?? []).filter((v) => v !== view);
  if (focused) list.push(view);
  else list.unshift(view);
  liveViews.set(cacheKey, list);
}
function unmarkLive(cacheKey: string, view: EditorView) {
  const list = (liveViews.get(cacheKey) ?? []).filter((v) => v !== view);
  if (list.length) liveViews.set(cacheKey, list);
  else liveViews.delete(cacheKey);
}

// Pedido de cursor inicial (1-based), usado quando o buffer ainda não tem estado guardado (ex.: restaurado da recuperação).
export type StartCursor = { line: number; col: number };

const readOnlyExt = (readOnly: boolean) => [EditorView.editable.of(!readOnly), EditorState.readOnly.of(readOnly)];

// Põe o cursor em `at`, rola até ele no centro e foca a vista (reveal da busca e ir para linha:coluna).
const goTo = (view: EditorView, at: number) => {
  view.dispatch({ selection: { anchor: at }, effects: EditorView.scrollIntoView(at, { y: 'center' }) });
  view.focus();
};

// Posição (offset UTF-16) de linha:coluna 1-based num texto com `\n`. Limita ao documento.
function offsetOf(text: string, start: StartCursor): number {
  const lines = text.split('\n');
  const line = Math.min(Math.max(start.line, 1), lines.length);
  let at = 0;
  for (let i = 1; i < line; i++) at += lines[i - 1].length + 1;
  return at + Math.min(Math.max(start.col - 1, 0), lines[line - 1].length);
}

// Ir para linha:coluna, com validação (linha dentro do documento e coluna não negativa).
export function parseGoto(value: string, lines: number, lineLength: (line: number) => number): { line: number; col: number } | string {
  const match = /^\s*(\d+)(?:\s*:\s*(\d+))?\s*$/.exec(value);
  if (!match) return 'Digite linha ou linha:coluna, só números.';
  const line = Number(match[1]);
  if (line < 1 || line > lines) return `A linha vai de 1 a ${lines}.`;
  const col = match[2] ? Number(match[2]) : 1;
  if (col < 1 || col > lineLength(line) + 1) return `A coluna vai de 1 a ${lineLength(line) + 1} nesta linha.`;
  return { line, col };
}

// Editor do arquivo (CodeMirror com o tema do app). Exportado para o editor central (src/editor-workspace.tsx) reutilizar.
export function CodeEditor({ text, readOnly, path, reveal, onChange, onSave, onCursor, onScroll, cacheKey, start, scroll }: {
  text: string; readOnly: boolean; path: string; reveal: Reveal | null; onChange: (text: string) => void; onSave: () => void;
  onCursor: (line: number, col: number) => void; onScroll?: (scroll: Scroll) => void; cacheKey?: string; start?: StartCursor | null;
  /** Rolagem a restaurar ao montar (ex.: a vista foi movida de grupo ou o app foi reaberto). */
  scroll?: Scroll | null;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const language = useRef(new Compartment());
  const editable = useRef(new Compartment());
  const mini = useRef(new Compartment());
  // Último pedido já atendido: o editor fica montado ao trocar de aba, e um pedido antigo não pode mover o cursor de novo.
  const revealed = useRef(0);
  // Textos que este editor já mandou para cima (um por digitação, em ordem). O pai devolve o texto com atraso: uma renderização
  // com um texto antigo não pode reverter o documento, senão cada volta gera outra edição e o React estoura o limite de atualizações.
  const echoes = useRef<string[]>([]);
  const latest = useRef({ onChange, onSave, onCursor, onScroll });
  latest.current = { onChange, onSave, onCursor, onScroll };
  const [gotoOpen, setGotoOpen] = useState(false);
  const [gotoText, setGotoText] = useState('');
  const [gotoError, setGotoError] = useState('');
  // Linha e coluna (1-based) do cursor, para o caminho de navegação da aba. Só informa; não mexe no editor.
  const reportCursor = (state: EditorState) => {
    const head = state.selection.main.head;
    const line = state.doc.lineAt(head);
    latest.current.onCursor(line.number, head - line.from + 1);
  };

  // Comandos da vista atual. Lidam com a vista ausente (desmontada) sem erro.
  const run = (command: (v: EditorView) => boolean) => {
    const current = view.current;
    if (current) command(current);
  };
  const clipboardCopy = async (cut: boolean) => {
    const current = view.current;
    if (!current) return;
    const { from, to } = current.state.selection.main;
    if (from === to) return;
    try {
      if (!navigator.clipboard) throw new Error('A área de transferência não está disponível aqui.');
      await navigator.clipboard.writeText(current.state.sliceDoc(from, to));
      // Só corta depois que o texto foi para a área de transferência: falha no clipboard não apaga nada.
      if (cut && !readOnly) current.dispatch({ changes: { from, to }, selection: { anchor: from } });
    } catch (e) {
      toast.error({ title: message(e), duration: 5000 });
    }
  };
  const clipboardPaste = async () => {
    const current = view.current;
    if (!current || readOnly) return;
    try {
      if (!navigator.clipboard) throw new Error('A área de transferência não está disponível aqui.');
      const text = await navigator.clipboard.readText();
      current.dispatch(current.state.replaceSelection(text));
      current.focus();
    } catch (e) {
      toast.error({ title: message(e), duration: 5000 });
    }
  };
  const editItems = (): ContextMenuItem[] => {
    const current = view.current;
    const hasSelection = !!current && !current.state.selection.main.empty;
    const edit = (label: string, key: string, fn: () => void, shortcut?: string) => (readOnly
      ? blocked(key, label, 'O arquivo é somente leitura.')
      : action(key, label, fn, shortcut));
    return [
      edit('Desfazer', 'undo', () => run(undo), 'Ctrl+Z'),
      edit('Refazer', 'redo', () => run(redo), 'Ctrl+Y'),
      DIVIDER,
      readOnly || !hasSelection ? blocked('cut', 'Cortar', readOnly ? 'O arquivo é somente leitura.' : 'Selecione um trecho.')
        : action('cut', 'Cortar', () => void clipboardCopy(true), 'Ctrl+X'),
      !hasSelection ? blocked('copy', 'Copiar', 'Selecione um trecho.') : action('copy', 'Copiar', () => void clipboardCopy(false), 'Ctrl+C'),
      edit('Colar', 'paste', () => void clipboardPaste(), 'Ctrl+V'),
      action('select-all', 'Selecionar Tudo', () => run(selectAll), 'Ctrl+A'),
      action('select-next', 'Selecionar Próxima Ocorrência', () => run(selectNextOccurrence), 'Ctrl+D'),
      DIVIDER,
      edit('Comentar', 'comment', () => run(toggleComment), 'Ctrl+/'),
      edit('Mover Linha para Cima', 'move-up', () => run(moveLineUp), 'Alt+↑'),
      edit('Mover Linha para Baixo', 'move-down', () => run(moveLineDown), 'Alt+↓'),
      edit('Duplicar Linha', 'duplicate', () => run(copyLineDown), 'Shift+Alt+↓'),
      edit('Excluir Linha', 'delete-line', () => run(deleteLine), 'Ctrl+Shift+K'),
      DIVIDER,
      action('find', 'Localizar', () => run(openSearchPanel), 'Ctrl+F'),
      edit('Substituir', 'replace', () => run(openSearchPanel), 'Ctrl+H'),
      action('goto', 'Ir para a Linha', () => setGotoOpen(true), 'Ctrl+G'),
    ];
  };

  useEffect(() => {
    if (!host.current) return;
    const cached = cacheKey ? savedStates.get(cacheKey) : undefined;
    // Extensões são sempre as atuais (listeners apontam para este componente). Estado guardado mantém o histórico de Desfazer.
    const extensions = [
      ...basicExtensions,
      Prec.highest(keymap.of([{
        key: 'Mod-s',
        run: () => {
          latest.current.onSave();
          return true;
        },
      }, {
        key: 'Mod-g',
        run: () => {
          setGotoOpen(true);
          return true;
        },
      }, {
        key: 'Mod-h',
        run: (v) => openSearchPanel(v),
      }])),
      editable.current.of(readOnlyExt(readOnly)),
      // Foco nesta vista: ela passa a ser a entrada do arquivo para o texto selecionado.
      EditorView.domEventHandlers({
        focus: (_event, v) => {
          if (cacheKey) markLive(cacheKey, v, true);
        },
      }),
      language.current.of([]),
      mini.current.of(minimap()),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          const value = update.state.doc.toString();
          echoes.current.push(value);
          if (echoes.current.length > ECHO_LIMIT) echoes.current.shift();
          // Fora da transação do CodeMirror: o estado do pai muda depois dela, e rajadas de teclas não aninham atualizações do React.
          queueMicrotask(() => latest.current.onChange(value));
        }
        if (update.docChanged || update.selectionSet) reportCursor(update.state);
      }),
    ];
    let state: EditorState;
    if (cached && cached.doc.toString() === text) {
      state = cached.update({ effects: StateEffect.reconfigure.of(extensions) }).state;
    } else {
      const at = start ? offsetOf(text, start) : 0;
      state = EditorState.create({ doc: text, selection: EditorSelection.cursor(at), extensions });
    }
    view.current = new EditorView({ state, parent: host.current });
    if (cacheKey) markLive(cacheKey, view.current, false);
    reportCursor(view.current.state);
    const dom = view.current.scrollDOM;
    // A rolagem só é aplicada depois da medição: antes, o CodeMirror ainda não sabe a altura do documento e prende o valor.
    const restore = scroll;
    let frame = 0;
    if (restore) {
      frame = requestAnimationFrame(() => {
        dom.scrollTop = restore.top;
        dom.scrollLeft = restore.left;
      });
    }
    const onDomScroll = () => latest.current.onScroll?.({ top: dom.scrollTop, left: dom.scrollLeft });
    dom.addEventListener('scroll', onDomScroll, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      dom.removeEventListener('scroll', onDomScroll);
      if (cacheKey && view.current) savedStates.set(cacheKey, view.current.state);
      if (cacheKey && view.current) unmarkLive(cacheKey, view.current);
      view.current?.destroy();
      view.current = null;
    };
  }, []);

  useEffect(() => {
    const current = view.current;
    if (!current) return;
    const source = text.length <= COLOR_LIMIT
      ? import('@codemirror/language-data').then(({ languages }) => {
        const description = LanguageDescription.matchFilename(languages, path);
        return description?.load() ?? null;
      })
      : Promise.resolve(null);
    let live = true;
    void source.then((support) => {
      if (live && view.current === current) {
        current.dispatch({ effects: [language.current.reconfigure(support ?? []), mini.current.reconfigure(minimap())] });
      }
    });
    return () => {
      live = false;
    };
  }, [path, text.length > COLOR_LIMIT]);

  useEffect(() => {
    const current = view.current;
    if (!current) return;
    const old = current.state.doc.toString();
    if (old === text) return;
    const echo = echoes.current.lastIndexOf(text);
    if (echo >= 0) {
      // Eco de uma digitação já feita aqui: o documento está à frente, então só descarta os ecos já vistos.
      echoes.current = echoes.current.slice(echo + 1);
      return;
    }
    // Texto de fora (disco, recuperação): a fila de ecos não vale mais.
    echoes.current = [];
    let from = 0;
    while (from < old.length && from < text.length && old[from] === text[from]) from++;
    let oldEnd = old.length;
    let newEnd = text.length;
    while (oldEnd > from && newEnd > from && old[oldEnd - 1] === text[newEnd - 1]) {
      oldEnd--;
      newEnd--;
    }
    current.dispatch({ changes: { from, to: oldEnd, insert: text.slice(from, newEnd) } });
  }, [text]);

  // Depois da sincronização do texto: um pedido novo usa o documento atualizado. Só roda quando o pedido muda, não a cada digitação.
  // Põe o cursor na linha, rola até ela e foca o editor. A linha é limitada ao documento atual, que pode ser diferente do que a busca leu.
  useEffect(() => {
    const current = view.current;
    if (!current || !reveal || reveal.id === revealed.current) return;
    revealed.current = reveal.id;
    const { doc } = current.state;
    const line = doc.line(Math.min(Math.max(reveal.line, 1), doc.lines));
    const at = line.from + Math.min(Math.max((reveal.col ?? 1) - 1, 0), line.length);
    goTo(current, at);
  }, [reveal]);

  useEffect(() => {
    const current = view.current;
    if (current) {
      current.dispatch({ effects: editable.current.reconfigure(readOnlyExt(readOnly)) });
    }
  }, [readOnly]);

  const submitGoto = () => {
    const current = view.current;
    if (!current) return;
    const { doc } = current.state;
    const result = parseGoto(gotoText, doc.lines, (n) => doc.line(n).length);
    if (typeof result === 'string') {
      setGotoError(result);
      return;
    }
    const line = doc.line(result.line);
    const at = line.from + Math.min(result.col - 1, line.length);
    goTo(current, at);
    setGotoOpen(false);
    setGotoError('');
  };

  return (
    <div className="relative h-full min-h-0">
      <ContextMenuTrigger items={editItems}>
        <div ref={host} className="h-full min-h-0 overflow-hidden" />
      </ContextMenuTrigger>
      {gotoOpen && (
        <form
          role="search"
          aria-label="Ir para a linha"
          className="absolute top-2 right-4 z-10 flex flex-wrap items-center gap-2 rounded-md border bg-card p-2 shadow"
          onSubmit={(e) => { e.preventDefault(); submitGoto(); }}
          onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); setGotoOpen(false); setGotoError(''); view.current?.focus(); } }}
        >
          <label className="text-xs text-muted-foreground" htmlFor="goto-line">Linha:coluna</label>
          <input
            id="goto-line"
            autoFocus
            inputMode="numeric"
            value={gotoText}
            onChange={(e) => { setGotoText(e.target.value); setGotoError(''); }}
            className="w-28 rounded border bg-transparent px-2 py-1 text-xs"
            placeholder="ex.: 42:3"
            aria-invalid={gotoError !== ''}
          />
          <button type="submit" className="rounded border px-2 py-1 text-xs">Ir</button>
          {gotoError && <span role="alert" className="basis-full text-xs text-red-500">{gotoError}</span>}
        </form>
      )}
    </div>
  );
}
