// A pesquisa de texto nos arquivos do projeto, na coluna da árvore de pastas. A busca roda no backend (comando `search`);
// aqui ficam o campo com as três opções, as ações, o resumo e a lista de resultados agrupada por arquivo.
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { CaseSensitive, ChevronDown, ChevronRight, ChevronsDownUp, ChevronsUpDown, Eraser, type LucideIcon, Regex, RefreshCw, WholeWord } from 'lucide-react';
import { ActionIcon, Input, ScrollArea, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from './backend';
import type { SearchOptions, SearchResults } from './types';

const DELAY_MS = 300;
// Mesmo limite do backend: com mais linhas casadas, só as primeiras aparecem e a lista avisa.
const LIMIT = 2000;
const NO_OPTIONS: SearchOptions = { caseSensitive: false, wholeWord: false, regex: false };
const TOGGLES: { key: keyof SearchOptions; icon: LucideIcon; label: string }[] = [
  { key: 'caseSensitive', icon: CaseSensitive, label: 'Diferenciar maiúsculas de minúsculas' },
  { key: 'wholeWord', icon: WholeWord, label: 'Palavra inteira' },
  { key: 'regex', icon: Regex, label: 'Expressão regular' },
];

// Caracteres especiais escapados quando a consulta é texto, não regex.
const escapeRegExp = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** RegExp do destaque, com as mesmas opções da busca. Se não puder ser montada (regex inválida), devolve null. */
function highlighter(query: string, options: SearchOptions): RegExp | null {
  if (!query) return null;
  try {
    const source = options.regex ? query : escapeRegExp(query);
    return new RegExp(options.wholeWord ? `\\b(?:${source})\\b` : source, options.caseSensitive ? 'g' : 'gi');
  } catch {
    return null;
  }
}

/** Texto com os trechos que casam destacados. Sem RegExp ou sem casamento, o texto sai puro. */
function marked(text: string, pattern: RegExp | null): ReactNode {
  if (!pattern) return text;
  const parts: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(pattern)) {
    if (!m[0]) continue; // casamento vazio não tem o que destacar
    const at = m.index;
    parts.push(text.slice(last, at), <mark key={at} className="rounded-sm bg-foreground/20 text-foreground">{m[0]}</mark>);
    last = at + m[0].length;
  }
  parts.push(text.slice(last));
  return parts;
}

// Caminho relativo com "/": nome do arquivo e a pasta dele (vazia na raiz).
const split = (path: string) => {
  const slash = path.lastIndexOf('/');
  return { name: path.slice(slash + 1), dir: slash >= 0 ? path.slice(0, slash) : '' };
};

/** Resumo da busca: contagem de linhas e de arquivos, com o aviso do limite quando passou dele. */
function summary(results: SearchResults): string {
  const lines = results.files.reduce((n, f) => n + f.matches.length, 0);
  if (!lines) return 'Nenhum resultado.';
  const files = results.files.length;
  const text = `${lines} ${lines === 1 ? 'resultado' : 'resultados'} em ${files} ${files === 1 ? 'arquivo' : 'arquivos'}`;
  return results.truncated ? `${text}. Só os ${LIMIT} primeiros aparecem.` : text;
}

/** Pesquisa de texto: o campo com as três opções, as ações, o resumo e a lista de resultados por arquivo. */
export function SearchScreen({ active, focusKey }: { active: boolean; focusKey: number }) {
  const [query, setQuery] = useState('');
  const [options, setOptions] = useState<SearchOptions>(NO_OPTIONS);
  const [results, setResults] = useState<SearchResults | null>(null);
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  // Arquivos recolhidos. Começam todos abertos a cada busca nova.
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const field = useRef<HTMLInputElement>(null);
  const timer = useRef(0);
  // Só a resposta da busca mais nova vale: uma antiga que chega depois é descartada.
  const latest = useRef(0);

  // Ao entrar na tela, e a cada Ctrl+Shift+F (`focusKey`), o campo recebe o foco com o texto selecionado.
  useEffect(() => {
    if (!active) return;
    field.current?.focus();
    field.current?.select();
  }, [active, focusKey]);

  // Uma busca. Campo vazio limpa sem chamar o backend.
  async function search(text: string, next: SearchOptions) {
    const id = ++latest.current;
    setError('');
    if (!text) {
      setResults(null);
      setPending(false);
      return;
    }
    setPending(true);
    try {
      const found = await backend.search(text, next);
      if (id !== latest.current) return;
      setResults(found);
      setCollapsed([]);
    } catch (e) {
      if (id !== latest.current) return;
      setResults(null);
      setError(message(e));
    } finally {
      if (id === latest.current) setPending(false);
    }
  }

  // Digitar espera um pouco; Enter, o botão Atualizar e as opções buscam na hora.
  const later = (text: string) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void search(text, options), DELAY_MS);
  };
  const now = (text = query, next = options) => {
    window.clearTimeout(timer.current);
    void search(text, next);
  };

  const toggle = (key: keyof SearchOptions) => {
    const next = { ...options, [key]: !options[key] };
    setOptions(next);
    now(query, next);
  };

  const clear = () => {
    window.clearTimeout(timer.current);
    setQuery('');
    void search('', options);
    field.current?.focus();
  };

  const files = results?.files ?? [];
  const allCollapsed = files.length > 0 && collapsed.length === files.length;
  const toggleAll = () => setCollapsed(allCollapsed ? [] : files.map((f) => f.path));
  const toggleFile = (path: string) =>
    setCollapsed((prev) => (prev.includes(path) ? prev.filter((p) => p !== path) : [...prev, path]));
  const pattern = highlighter(query, options);

  return (
    <Flexbox height="100%" className="min-h-0">
      <Flexbox gap={6} paddingInline={8} paddingBlock={8}>
        <Input
          ref={field} size="small" value={query} placeholder="Pesquisar" aria-label="Pesquisar nos arquivos do projeto"
          onChange={(e) => {
            setQuery(e.target.value);
            later(e.target.value);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') now();
          }}
          suffix={(
            <Flexbox horizontal gap={2}>
              {TOGGLES.map(({ key, icon, label }) => (
                <ActionIcon
                  key={key} icon={icon} size="small" variant="borderless" active={options[key]}
                  aria-pressed={options[key]} aria-label={label} title={label}
                  onClick={() => toggle(key)}
                />
              ))}
            </Flexbox>
          )}
        />
        <Flexbox horizontal justify="flex-end" gap={2}>
          <ActionIcon
            icon={RefreshCw} size="small" variant="borderless" spin={pending} disabled={!query}
            aria-label="Atualizar" title="Atualizar: refaz a mesma busca" onClick={() => now()}
          />
          <ActionIcon
            icon={Eraser} size="small" variant="borderless" disabled={!query}
            aria-label="Limpar" title="Limpar a pesquisa" onClick={clear}
          />
          <ActionIcon
            icon={allCollapsed ? ChevronsUpDown : ChevronsDownUp} size="small" variant="borderless" disabled={!files.length}
            aria-label={allCollapsed ? 'Expandir tudo' : 'Recolher tudo'}
            title={allCollapsed ? 'Expandir todos os arquivos' : 'Recolher todos os arquivos'} onClick={toggleAll}
          />
        </Flexbox>
      </Flexbox>

      <Text as="p" role="status" fontSize={12} type={error ? 'danger' : 'secondary'} className="px-3 pb-2 select-text">
        {error || (pending ? 'Pesquisando…' : results ? summary(results) : '')}
      </Text>

      <Flexbox flex={1} className="min-h-0">
        <ScrollArea className="h-full" disableContentFit>
          <Flexbox paddingBlock={4}>
            {files.map((file) => {
              const open = !collapsed.includes(file.path);
              const { name, dir } = split(file.path);
              return (
                <Flexbox as="section" key={file.path}>
                  <button
                    type="button" aria-expanded={open} onClick={() => toggleFile(file.path)} title={file.path}
                    className="flex w-full min-w-0 items-center gap-1 rounded-sm px-2 py-1 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50"
                  >
                    {open
                      ? <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
                      : <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />}
                    <Text as="span" fontSize={14} ellipsis className="min-w-0 shrink">{name}</Text>
                    {dir && <Text as="span" type="secondary" fontSize={12} ellipsis className="min-w-0 shrink-2">{dir}</Text>}
                    <Text as="span" type="secondary" fontSize={12} className="ml-auto shrink-0 pl-2">{file.matches.length}</Text>
                  </button>
                  {open && file.matches.map((m) => (
                    // Linha de resultado não é botão: clicar ainda não faz nada (o app não abre arquivos).
                    <Flexbox
                      key={m.line} horizontal align="center" gap={8} paddingInline="24px 8px" paddingBlock={1}
                      title={`${file.path}:${m.line}`} className="min-w-0 select-text"
                    >
                      <Text as="span" code type="secondary" fontSize={12} className="w-10 shrink-0 text-right">{m.line}</Text>
                      <Text as="span" code fontSize={12} ellipsis className="min-w-0 flex-1">
                        {marked(m.text.trimStart(), pattern)}
                      </Text>
                    </Flexbox>
                  ))}
                </Flexbox>
              );
            })}
          </Flexbox>
        </ScrollArea>
      </Flexbox>
    </Flexbox>
  );
}
