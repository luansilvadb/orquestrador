// Um arquivo aberto na área do meio: barra com caminho, avisos e salvar, e editor com releitura enquanto estiver visível.
// Cada arquivo aberto fica montado; só o ativo recebe `active` e lê o disco. Fechar é pela aba (src/file-tabs.tsx).
import { useCallback, useEffect, useRef, useState } from 'react';
import { Save, RotateCcw } from 'lucide-react';
import { ActionIcon, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { basicSetup } from 'codemirror';
import { EditorState, Compartment } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { indentWithTab } from '@codemirror/commands';
import { LanguageDescription, syntaxHighlighting, HighlightStyle } from '@codemirror/language';
import { tags } from '@lezer/highlight';
import { backend, message } from './backend';
import { usePolled } from './git';
import type { FileContent } from './types';

// ponytail: acima de 200 mil caracteres, carregar parser e colorir custa mais que a leitura simples.
const COLOR_LIMIT = 200_000;
const darkTheme = EditorView.theme({
  '&': {
    height: '100%',
    color: 'var(--ant-color-text)',
    backgroundColor: 'var(--ant-color-bg-container)',
    fontSize: '13px',
  },
  '.cm-scroller': {
    overflow: 'auto',
    fontFamily: 'var(--default-mono-font-family)',
    lineHeight: '1.5',
  },
  '.cm-content': {
    padding: '12px 12px 12px 0',
    caretColor: 'var(--ant-color-text)',
  },
  '.cm-gutters': {
    color: 'var(--ant-color-text-tertiary)',
    backgroundColor: 'var(--ant-color-bg-container)',
    border: 'none',
  },
  '.cm-activeLine, .cm-activeLineGutter': {
    backgroundColor: 'var(--ant-color-fill-quaternary)',
  },
  '&.cm-focused': { outline: 'none' },
  '.cm-selectionBackground': { backgroundColor: 'var(--ant-color-primary-bg)' },
}, { dark: true });

const darkHighlight = HighlightStyle.define([
  { tag: tags.keyword, color: '#c792ea' },
  { tag: tags.string, color: '#c3e88d' },
  { tag: tags.comment, color: '#7f8c98', fontStyle: 'italic' },
  { tag: tags.number, color: '#f78c6c' },
  { tag: tags.function(tags.variableName), color: '#82aaff' },
  { tag: [tags.typeName, tags.className, tags.namespace], color: '#ffcb6b' },
  { tag: tags.propertyName, color: '#80cbc4' },
  { tag: tags.operator, color: '#89ddff' },
  { tag: tags.heading, color: '#82aaff', fontWeight: 'bold' },
  { tag: tags.link, color: '#80cbc4', textDecoration: 'underline' },
]);
const basicExtensions = [
  basicSetup,
  keymap.of([indentWithTab]),
  darkTheme,
  syntaxHighlighting(darkHighlight),
];

// A descrição do CodeMirror também reconhece nomes de linguagem sem extensão.
const normalizeNewlines = (text: string) => text.replace(/\r\n|\r/g, '\n');
const newlineOf = (text: string) => text.includes('\r\n') ? '\r\n' : '\n';
const toDiskNewlines = (text: string, newline: string) => text.replace(/\n/g, newline);

// Pedido de navegação a uma linha (1-based), vindo de um resultado da pesquisa. `id` é novo a cada clique.
export type Reveal = { path: string; line: number; id: number };

function CodeEditor({ text, readOnly, path, reveal, onChange, onSave, onCursor }: {
  text: string; readOnly: boolean; path: string; reveal: Reveal | null; onChange: (text: string) => void; onSave: () => void;
  onCursor: (line: number, col: number) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const language = useRef(new Compartment());
  const editable = useRef(new Compartment());
  // Último pedido já atendido: o editor fica montado ao trocar de aba, e um pedido antigo não pode mover o cursor de novo.
  const revealed = useRef(0);
  const latest = useRef({ onChange, onSave, onCursor });
  latest.current = { onChange, onSave, onCursor };
  // Linha e coluna (1-based) do cursor, para o caminho de navegação da aba. Só informa; não mexe no editor.
  const reportCursor = (state: EditorState) => {
    const head = state.selection.main.head;
    const line = state.doc.lineAt(head);
    latest.current.onCursor(line.number, head - line.from + 1);
  };

  useEffect(() => {
    if (!host.current) return;
    const state = EditorState.create({
      doc: text,
      extensions: [
        ...basicExtensions,
        editable.current.of([
          EditorView.editable.of(!readOnly),
          EditorState.readOnly.of(readOnly),
        ]),
        language.current.of([]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) latest.current.onChange(update.state.doc.toString());
          if (update.docChanged || update.selectionSet) reportCursor(update.state);
        }),
        keymap.of([{
          key: 'Mod-s',
          run: () => {
            latest.current.onSave();
            return true;
          },
        }]),
      ],
    });
    view.current = new EditorView({ state, parent: host.current });
    reportCursor(state);
    return () => {
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
        current.dispatch({ effects: language.current.reconfigure(support ?? []) });
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
    const { from } = doc.line(Math.min(Math.max(reveal.line, 1), doc.lines));
    current.dispatch({ selection: { anchor: from }, effects: EditorView.scrollIntoView(from, { y: 'center' }) });
    current.focus();
  }, [reveal]);

  useEffect(() => {
    const current = view.current;
    if (current) {
      current.dispatch({
        effects: editable.current.reconfigure([
          EditorView.editable.of(!readOnly),
          EditorState.readOnly.of(readOnly),
        ]),
      });
    }
  }, [readOnly]);

  return <div ref={host} className="h-full min-h-0 overflow-hidden" />;
}

export function FileView({ path, active, reveal, onDirtyChange, onCursor }: {
  path: string; active: boolean; reveal: Reveal | null; onDirtyChange: (path: string, dirty: boolean) => void;
  onCursor: (path: string, line: number, col: number) => void;
}) {
  const { data: file, error, refresh } = usePolled<FileContent>(() => backend.readFile(path), active, false);
  // base e draft ficam em LF; newline guarda o formato do disco para a próxima gravação.
  const [draft, setDraft] = useState<string | null>(null);
  const [base, setBase] = useState('');
  const [newline, setNewline] = useState('\n');
  const [diskChanged, setDiskChanged] = useState(false);
  const [saveError, setSaveError] = useState('');
  const [saving, setSaving] = useState(false);
  const dirty = draft !== null && draft !== base;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  // `onDirtyChange` é estável no App: o efeito só roda quando a edição muda de estado.
  useEffect(() => onDirtyChange(path, dirty), [dirty, path, onDirtyChange]);

  useEffect(() => {
    if (!file || file.binary || file.tooLarge) return;
    const diskText = normalizeNewlines(file.text);
    if (draft === null) {
      setBase(diskText);
      setDraft(diskText);
      setNewline(newlineOf(file.text));
      setDiskChanged(false);
    } else if (diskText === base) {
      setDiskChanged(false);
    } else if (diskText === draft && !dirtyRef.current) {
      // A gravação acabou de chegar pelo polling; a releitura só confirma a base.
      setBase(diskText);
      setNewline(newlineOf(file.text));
      setDiskChanged(false);
    } else if (dirtyRef.current) {
      // Com edição pendente, não substitui o rascunho: oferece recarregar explicitamente.
      setDiskChanged(true);
    } else {
      setBase(diskText);
      setDraft(diskText);
      setNewline(newlineOf(file.text));
      setDiskChanged(false);
    }
  }, [file]);

  const change = useCallback((value: string) => {
    setDraft(value);
    setSaveError('');
  }, []);
  const save = useCallback(async () => {
    if (draft === null || !dirty || file?.readOnly || saving) return;
    setSaving(true);
    setSaveError('');
    try {
      await backend.writeFile(path, toDiskNewlines(draft, newline));
      setBase(draft);
      setDiskChanged(false);
      await refresh();
    } catch (e) {
      setSaveError(message(e));
    } finally {
      setSaving(false);
    }
  }, [draft, dirty, file?.readOnly, newline, path, refresh, saving]);

  const reload = () => {
    if (!file || file.binary || file.tooLarge) return;
    const diskText = normalizeNewlines(file.text);
    setBase(diskText);
    setDraft(diskText);
    setNewline(newlineOf(file.text));
    setDiskChanged(false);
    setSaveError('');
  };

  const note = (text: string, type: 'danger' | 'secondary' | undefined = 'secondary') => (
    <Flexbox padding={16}>
      <Text as="p" role="status" fontSize={14} type={type}>
        {text}
      </Text>
    </Flexbox>
  );
  let body;
  if (!file) body = note(error || 'Lendo o arquivo…', error ? 'danger' : undefined);
  else if (file.binary) body = note('Arquivo binário: não há texto para mostrar.');
  else if (file.tooLarge) body = note('Arquivo grande demais para mostrar aqui.');
  else if (draft === null) body = note(error || 'Lendo o arquivo…', error ? 'danger' : undefined);
  else {
    // Quatro pixels de respiro evitam que a rolagem fique sob as alças dos painéis.
    body = (
      <Flexbox flex={1} className="min-h-0 pr-1 pb-1">
        <Flexbox flex={1} className="min-h-0 overflow-hidden">
          <CodeEditor
            text={draft}
            readOnly={file.readOnly}
            path={path}
            // Só o arquivo ativo e o pedido dele: um pedido de outro arquivo, ou feito enquanto este está oculto, espera a aba ficar ativa.
            reveal={active && reveal?.path === path ? reveal : null}
            onChange={change}
            onSave={() => void save()}
            onCursor={(line, col) => onCursor(path, line, col)}
          />
        </Flexbox>
      </Flexbox>
    );
  }
  const name = path.slice(path.lastIndexOf('/') + 1);
  const notice = saveError || (file?.readOnly
    ? 'Arquivo não está em UTF-8; somente leitura.'
    : diskChanged
      ? 'O arquivo mudou no disco.'
      : error && file
        ? error
        : '');

  return (
    <Flexbox height="100%" className="min-h-0">
      {/* Barra de 36 px, abaixo da faixa de abas dos arquivos. */}
      <Flexbox
        horizontal
        align="center"
        gap={8}
        height={36}
        flex="none"
        paddingInline={12}
        className="border-b bg-card"
      >
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={path} className="min-w-0">
          {path}
        </Text>
        {dirty && (
          <Text as="span" aria-label="Alteração não salva" title="Alteração não salva" className="shrink-0">
            ●
          </Text>
        )}
        {diskChanged && (
          <ActionIcon
            variant="borderless"
            size="small"
            icon={RotateCcw}
            aria-label="Recarregar arquivo do disco"
            title="Recarregar arquivo do disco"
            onClick={reload}
          />
        )}
        {notice && (
          <Text
            as="span"
            role="status"
            type={saveError || (error && file) ? 'danger' : 'secondary'}
            fontSize={12}
            ellipsis
            title={notice}
            className="min-w-0"
          >
            {notice}
          </Text>
        )}
        <ActionIcon
          variant="borderless"
          size="small"
          icon={Save}
          className="ml-auto shrink-0"
          aria-label={`Salvar o arquivo ${name}`}
          title={`Salvar o arquivo ${name}`}
          disabled={!dirty || !!file?.readOnly || saving}
          onClick={() => void save()}
        />
      </Flexbox>
      <Flexbox flex={1} className="min-h-0">
        {body}
      </Flexbox>
    </Flexbox>
  );
}
