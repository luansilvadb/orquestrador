// A pesquisa de texto nos arquivos do projeto, na coluna da árvore de pastas. A busca roda no backend (comando `search`) e a substituição
// em vários arquivos tem prévia (`replace_preview`, não grava) e aplicação (`replace_apply`, só nos arquivos escolhidos da prévia).
// Aqui ficam o cabeçalho com as ações, o campo com as três opções, os filtros de caminho, o resumo, a lista de resultados por arquivo
// e a revisão da substituição (antes e depois, com caixa por arquivo).
import { type KeyboardEvent as ReactKeyboardEvent, type ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import { CaseSensitive, CaseUpper, ChevronDown, ChevronRight, ChevronsDownUp, ChevronsUpDown, Eraser, File, ListFilter, type LucideIcon, Regex, RefreshCw, Replace, WholeWord, X } from 'lucide-react';
import { ActionIcon, Alert, Button, ContextMenuTrigger, Input, Modal, ScrollArea, Text, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from './backend';
import { action, DIVIDER } from './file-tabs';
import type { ReplacePreview, ReplaceQuery, SearchMatch, SearchOptions, SearchResults, SkipReason } from './types';

const DELAY_MS = 300;
// Mesmo limite do backend: com mais linhas casadas, só as primeiras aparecem e a lista avisa.
const LIMIT = 2000;
// Histórico de consultas no navegador do app: só as mais recentes, sem repetir.
const HISTORY_KEY = 'orquestrador.search.history';
const HISTORY_MAX = 20;
// Linhas trocadas mostradas por arquivo na revisão; o resto vira a contagem.
const REVIEW_LINES = 30;
// Quantos arquivos o diálogo de confirmação lista antes de resumir.
const CONFIRM_LIST = 12;
const HINT = 'Digite para pesquisar nos arquivos do projeto.';
const REGEX_HINT = 'Sintaxe de regex do Rust. Capturas: $1 ou ${nome}. Sem lookaround (?=, (?!, (?<=, (?<!) e sem referências como \\1.';
const REPLACE_HINT = 'Texto literal. Deixe vazio para apagar o que casar.';
const NO_OPTIONS: SearchOptions = { caseSensitive: false, wholeWord: false, regex: false };
const TOGGLES: { key: keyof SearchOptions; icon: LucideIcon; label: string }[] = [
  { key: 'caseSensitive', icon: CaseSensitive, label: 'Diferenciar maiúsculas de minúsculas' },
  { key: 'wholeWord', icon: WholeWord, label: 'Palavra inteira' },
  { key: 'regex', icon: Regex, label: 'Expressão regular' },
];
const REASON: Record<SkipReason, string> = {
  binary: 'binário',
  notUtf8: 'não é UTF-8',
  tooLarge: 'maior que 1 MiB',
  notFile: 'não é arquivo comum',
  unreadable: 'não pôde ser lido',
};

/** Tudo o que a busca e a substituição usam, do jeito que está nos campos. */
type Form = {
  query: string;
  options: SearchOptions;
  // Filtros de caminho como digitados: trechos separados por vírgula.
  include: string;
  exclude: string;
  // Pasta da pesquisa (relativa, com "/"); `null` é o projeto inteiro.
  scope: string | null;
  replacement: string;
  // Substituição que segue a caixa da ocorrência (ver `ReplaceQuery.preserveCase`).
  preserveCase: boolean;
};
const EMPTY_FORM: Form = { query: '', options: NO_OPTIONS, include: '', exclude: '', scope: null, replacement: '', preserveCase: false };

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
    parts.push(
      text.slice(last, at),
      <mark key={at} className="rounded-sm bg-[var(--ant-color-primary-bg)] text-foreground shadow-[inset_0_0_0_1px_var(--ant-color-primary-border)]">{m[0]}</mark>,
    );
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

// Trechos separados por vírgula, sem espaço nas pontas e sem os vazios.
const terms = (text: string) => text.split(',').map((t) => t.trim()).filter(Boolean);

/** Filtros da consulta. Na pasta escolhida sem filtro de inclusão, o backend recebe a pasta como trecho; o recorte exato vem depois, em `inScope`. */
function filtersOf(form: Form) {
  const include = terms(form.include);
  return {
    include: include.length ? include : form.scope ? [`${form.scope}/*`] : [],
    exclude: terms(form.exclude),
  };
}
const queryOf = (form: Form): ReplaceQuery => ({ text: form.query, ...form.options, ...filtersOf(form), preserveCase: form.preserveCase });

// Só entra o que está dentro da pasta da pesquisa. O backend casa trechos em qualquer ponto do caminho, então o recorte é conferido aqui.
const inScope = (path: string, scope: string | null) => !scope || path.startsWith(`${scope}/`);

/** Resumo da busca: contagem de linhas e de arquivos, com o aviso do limite quando passou dele. */
function summary(lines: number, files: number, truncated: boolean): string {
  if (!lines) return 'Nenhum resultado.';
  const text = `${lines} ${lines === 1 ? 'resultado' : 'resultados'} em ${files} ${files === 1 ? 'arquivo' : 'arquivos'}`;
  return truncated ? `${text}. Só os ${LIMIT} primeiros aparecem.` : text;
}

/** Histórico salvo. Leitura com falha (sem acesso ao armazenamento ou JSON inválido) vira histórico vazio. */
function readHistory(): string[] {
  try {
    const list: unknown = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? '[]');
    if (!Array.isArray(list)) return [];
    return [...new Set(list.filter((q): q is string => typeof q === 'string' && q !== ''))].slice(0, HISTORY_MAX);
  } catch {
    return [];
  }
}

/** Grava o histórico. Sem espaço ou sem acesso, a busca segue e o histórico vale só nesta sessão. */
function writeHistory(list: string[]) {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(list));
  } catch {
    // Sem armazenamento: nada a fazer além de seguir sem persistir.
  }
}

// Chave de uma linha dispensada: caminho e posição juntos, para não confundir linhas de arquivos diferentes.
const lineKey = (path: string, m: SearchMatch) => JSON.stringify([path, m.line, m.col]);

// Texto de um arquivo para a área de transferência: caminho numa linha e, depois, "  linha: texto" por resultado.
const blockText = (path: string, matches: SearchMatch[]) =>
  [path, ...matches.map((m) => `  ${m.line}: ${m.text.trimStart()}`)].join('\n');

/** Copia para a área de transferência. Falha vira toast de erro; sucesso, toast curto. */
async function copy(text: string, done: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast.success({ title: done, duration: 2000 });
  } catch (e) {
    toast.error({ title: message(e), duration: 5000 });
  }
}

/** Linhas que mudaram entre o original e o resultado. A substituição não cria nem junta linhas, então o índice serve de pareamento. */
function changedLines(original: string, result: string) {
  const before = original.split('\n');
  const after = result.split('\n');
  const out: { line: number; before: string; after: string }[] = [];
  for (let i = 0; i < before.length; i++) {
    if (before[i] !== after[i]) out.push({ line: i + 1, before: before[i].replace(/\r$/, ''), after: (after[i] ?? '').replace(/\r$/, '') });
  }
  return out;
}

type Shown = { query: string; options: SearchOptions; scope: string | null };
type Preview = { query: ReplaceQuery; replacement: string; scope: string | null; data: ReplacePreview };
type Report = { applied: { path: string; count: number }[]; errors: { path: string; error: string }[]; draft: string[]; failure: string };

/** Pesquisa de texto e substituição: o cabeçalho com as ações, o campo com as opções, os filtros, o resumo, os resultados e a revisão. */
export function SearchScreen({ active, focusKey, onOpen, folder, seed, refreshKey, step, hasDraft, onChanged }: {
  active: boolean;
  focusKey: number;
  /** Abre o arquivo na linha e na coluna (UTF-16, a partir de 1). Clique simples pede prévia; duplo clique ou Enter pedem permanente. */
  onOpen: (path: string, line: number, col?: number, options?: { permanent?: boolean }) => void;
  /** Pedido de pesquisa só nesta pasta (`path` relativo, '' é o projeto). Um novo `id` a cada pedido. */
  folder?: { path: string; id: number } | null;
  /** Texto selecionado no editor: vira a consulta e a busca roda na hora. Um novo `id` a cada pedido. */
  seed?: { text: string; id: number } | null;
  /** Sobe a cada gravação no editor: refaz a busca atual, se não há revisão de substituição aberta. */
  refreshKey?: number;
  /** F4 (`dir` 1) ou Shift+F4 (`dir` -1): anda pelos resultados. Um novo `id` a cada tecla. */
  step?: { dir: 1 | -1; id: number } | null;
  /** Diz se o arquivo tem alteração não salva no editor. Consultado ao exibir e de novo ao aplicar. */
  hasDraft?: (path: string) => boolean;
  /** Chamado com os caminhos que a substituição gravou, para o App recarregar o que estiver aberto. */
  onChanged?: (paths: string[]) => void;
}) {
  const [form, setForm] = useState<Form>(EMPTY_FORM);
  const [results, setResults] = useState<SearchResults | null>(null);
  // Consulta, opções e pasta da resposta que está na tela: o destaque e o recorte usam estas, não as do campo.
  const [shown, setShown] = useState<Shown>({ query: '', options: NO_OPTIONS, scope: null });
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  // Arquivos recolhidos. Começam todos abertos a cada busca nova.
  const [collapsed, setCollapsed] = useState<string[]>([]);
  // Resultados dispensados na tela (não mexem no disco). Zerados a cada resposta nova da busca.
  const [dismissedFiles, setDismissedFiles] = useState<string[]>([]);
  const [dismissedLines, setDismissedLines] = useState<string[]>([]);
  // Histórico de consultas, a mais recente primeiro. `at` é a posição de navegação com ArrowUp/ArrowDown (-1 é o texto digitado).
  const [history, setHistory] = useState<string[]>(readHistory);
  const [at, setAt] = useState(-1);
  const typed = useRef('');
  // Trocar pelo histórico não deve reordená-lo: a busca feita por ele não entra de novo no histórico.
  const skipHistory = useRef(false);
  const resultsBox = useRef<HTMLDivElement>(null);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [replaceOpen, setReplaceOpen] = useState(false);
  // Substituição: a prévia que está na tela, os arquivos marcados, o resultado da última aplicação e o que está rodando.
  const [preview, setPreview] = useState<Preview | null>(null);
  const [previewError, setPreviewError] = useState('');
  const [picked, setPicked] = useState<string[]>([]);
  const [report, setReport] = useState<Report | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [applying, setApplying] = useState(false);
  const [confirming, setConfirming] = useState(false);
  // Ref no contêiner do campo, não no `Input`: o lobehub lê `element.ref` e o React 19 avisa. O campo é achado dentro do contêiner.
  const fieldBox = useRef<HTMLDivElement>(null);
  const field = () => fieldBox.current?.querySelector('input') ?? null;
  const timer = useRef(0);
  // Só a resposta da busca mais nova vale: uma antiga que chega depois é descartada.
  const latest = useRef(0);
  // Mesma regra para a prévia: qualquer mudança nos campos invalida a prévia em voo.
  const latestPreview = useRef(0);
  // Resultado atual (clique ou F4): a linha marcada na lista e de onde o F4 sai.
  const [current, setCurrent] = useState<{ path: string; line: number; col: number } | null>(null);
  // Texto do pedido de seleção que espera aparecer no campo para ser selecionado.
  const selectAfter = useRef<string | null>(null);

  // Ao entrar na tela, e a cada Ctrl+Shift+F (`focusKey`), o campo recebe o foco com o texto selecionado.
  useEffect(() => {
    if (!active) return;
    field()?.focus();
    field()?.select();
  }, [active, focusKey]);

  // Pedido de pesquisa na pasta: aplica o recorte e refaz a busca, se já há consulta.
  useEffect(() => {
    if (!folder) return;
    const scope = folder.path || null;
    setForm((prev) => ({ ...prev, scope }));
    setFiltersOpen(true);
    invalidate();
    if (form.query) void run({ ...form, scope });
    field()?.focus();
    // Só o `id` do pedido importa: cada pedido de pasta é novo, mesmo para a mesma pasta.
  }, [folder?.id]);

  // Texto selecionado no editor: vira a consulta e a busca roda na hora, como Enter. O campo é selecionado depois que o texto novo aparece nele.
  useEffect(() => {
    if (!seed) return;
    window.clearTimeout(timer.current);
    setAt(-1);
    const next = { ...form, query: seed.text };
    setForm(next);
    invalidate();
    void run(next);
    if (seed.text === form.query) field()?.select();
    else selectAfter.current = seed.text;
    // Só o `id` do pedido importa: cada pedido é novo, mesmo com o mesmo texto.
  }, [seed?.id]);

  // Depois do render, seleciona o campo quando o texto do pedido já está nele. Sem pedido pendente, não faz nada.
  useEffect(() => {
    if (selectAfter.current === null || field()?.value !== selectAfter.current) return;
    selectAfter.current = null;
    field()?.select();
  });

  // Gravação no editor: refaz a busca atual sem entrar no histórico, sem roubar o foco e sem tirar o recolhido e o dispensado. Com revisão de substituição aberta, espera.
  useEffect(() => {
    if (!refreshKey || !form.query || preview) return;
    void run(form, true);
    // Só o contador de gravações dispara; o resto vem do render atual.
  }, [refreshKey]);

  // Grava o histórico a cada mudança (leitura e gravação protegidas em `readHistory` e `writeHistory`).
  useEffect(() => writeHistory(history), [history]);

  // Uma busca. Consulta vazia limpa sem chamar o backend. `quiet` é a atualização depois de gravar: não entra no histórico e mantém recolhidos e dispensados.
  async function run(f: Form, quiet = false) {
    const id = ++latest.current;
    setError('');
    if (!f.query) {
      setResults(null);
      setPending(false);
      return;
    }
    // A consulta entra no histórico no topo, sem repetição.
    if (!quiet && !skipHistory.current) setHistory((prev) => [f.query, ...prev.filter((q) => q !== f.query)].slice(0, HISTORY_MAX));
    setPending(true);
    try {
      const filters = filtersOf(f);
      const found = await backend.search(f.query, f.options, filters);
      if (id !== latest.current) return;
      setResults(found);
      setShown({ query: f.query, options: f.options, scope: f.scope });
      if (quiet) {
        // Recolhido só sai da lista se o arquivo sumiu da busca.
        setCollapsed((prev) => prev.filter((p) => found.files.some((x) => x.path === p)));
      } else {
        setCollapsed([]);
        setDismissedFiles([]);
        setDismissedLines([]);
      }
    } catch (e) {
      if (id !== latest.current) return;
      setResults(null);
      setError(message(e));
    } finally {
      if (id === latest.current) setPending(false);
    }
  }

  // Qualquer mudança nos campos tira a prévia, a seleção dela e o resultado da aplicação anterior.
  function invalidate() {
    latestPreview.current++;
    setPreview(null);
    setPreviewError('');
    setPicked([]);
    setConfirming(false);
    setReport(null);
    setPreviewing(false);
  }

  // Digitar espera um pouco; Enter, o botão Atualizar e as opções buscam na hora.
  const later = (next: Form) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void run(next), DELAY_MS);
  };
  const now = (next: Form) => {
    window.clearTimeout(timer.current);
    void run(next);
  };

  // Muda campos do formulário: tira a prévia e busca (agora ou com espera, conforme `mode`).
  const change = (patch: Partial<Form>, mode: 'later' | 'now' | 'none') => {
    const next = { ...form, ...patch };
    setForm(next);
    invalidate();
    if (mode === 'later') later(next);
    if (mode === 'now') now(next);
  };

  const toggle = (key: keyof SearchOptions) => change({ options: { ...form.options, [key]: !form.options[key] } }, 'now');

  const clear = () => {
    window.clearTimeout(timer.current);
    setAt(-1);
    setForm((prev) => ({ ...prev, query: '' }));
    invalidate();
    void run({ ...form, query: '' });
    field()?.focus();
  };

  // Troca o campo pela consulta do histórico (`index`) ou pelo texto digitado antes da navegação (-1), e busca na hora.
  const recall = (index: number) => {
    setAt(index);
    skipHistory.current = true;
    change({ query: index < 0 ? typed.current : history[index] }, 'now');
    skipHistory.current = false;
  };

  // ---------- substituição ----------

  // Prévia: não grava nada. Marca por padrão os arquivos que não têm alteração não salva.
  async function previewReplace() {
    if (!form.query) return;
    const id = ++latestPreview.current;
    const query = queryOf(form);
    setPreviewing(true);
    setPreviewError('');
    setReport(null);
    setConfirming(false);
    try {
      const data = await backend.replacePreview(query, form.replacement);
      if (id !== latestPreview.current) return;
      const next: Preview = { query, replacement: form.replacement, scope: form.scope, data };
      setPreview(next);
      setPicked(visibleFiles(next).filter((f) => !hasDraft?.(f.path)).map((f) => f.path));
    } catch (e) {
      if (id !== latestPreview.current) return;
      setPreview(null);
      setPreviewError(message(e));
    } finally {
      if (id === latestPreview.current) setPreviewing(false);
    }
  }

  // Arquivos da prévia que entram nesta pasta da pesquisa. Os demais nem aparecem na revisão.
  const visibleFiles = (p: Preview) => p.data.files.filter((f) => inScope(f.path, p.scope));
  const reviewed = useMemo(
    () => (preview ? visibleFiles(preview).map((f) => ({ file: f, pairs: changedLines(f.original, f.result) })) : []),
    [preview],
  );
  const chosen = reviewed.filter(({ file }) => picked.includes(file.path));
  const chosenCount = chosen.reduce((n, { file }) => n + file.count, 0);
  const draftOf = (path: string) => !!hasDraft?.(path);

  // Aplicação: só os arquivos marcados na prévia. Alteração não salva que apareceu desde a prévia também fica de fora, e é dita no relatório.
  async function applyChosen() {
    if (!preview || applying) return;
    setConfirming(false);
    const files = chosen.map(({ file }) => file).filter((f) => !draftOf(f.path));
    const draft = chosen.map(({ file }) => file.path).filter(draftOf);
    if (!files.length) {
      setReport({ applied: [], errors: [], draft, failure: '' });
      return;
    }
    setApplying(true);
    try {
      const done = await backend.replaceApply({
        query: preview.query,
        replacement: preview.replacement,
        files: files.map((f) => ({ path: f.path, revision: f.revision })),
      });
      latestPreview.current++;
      setPreview(null);
      setPicked([]);
      setReport({ applied: done.applied, errors: done.errors, draft, failure: '' });
      if (done.applied.length) onChanged?.(done.applied.map((a) => a.path));
      if (form.query) await run(form);
    } catch (e) {
      setReport({ applied: [], errors: [], draft, failure: message(e) });
    } finally {
      setApplying(false);
    }
  }

  const togglePick = (path: string) =>
    setPicked((prev) => (prev.includes(path) ? prev.filter((p) => p !== path) : [...prev, path]));

  // ---------- resultados ----------

  // `files` é o que está na tela: sem os dispensados. Resumo, recolher e navegação contam só isto.
  const scoped = (results?.files ?? []).filter((f) => inScope(f.path, shown.scope));
  const gone = new Set(dismissedLines);
  const files = scoped
    .filter((f) => !dismissedFiles.includes(f.path))
    .map((f) => ({ ...f, matches: f.matches.filter((m) => !gone.has(lineKey(f.path, m))) }))
    .filter((f) => f.matches.length > 0);
  const skipped = (results?.skipped ?? []).filter((s) => inScope(s.path, shown.scope));
  const lines = files.reduce((n, f) => n + f.matches.length, 0);
  const allCollapsed = files.length > 0 && collapsed.length === files.length;
  const toggleAll = () => setCollapsed(allCollapsed ? [] : files.map((f) => f.path));
  const toggleFile = (path: string) =>
    setCollapsed((prev) => (prev.includes(path) ? prev.filter((p) => p !== path) : [...prev, path]));
  const mark = (path: string, m: SearchMatch) => setCurrent({ path, line: m.line, col: m.col });
  const isCurrent = (path: string, m: SearchMatch) => current?.path === path && current.line === m.line && current.col === m.col;

  // F4 e Shift+F4: anda pela ordem da tela (arquivo recolhido entra também), com volta. Abre como prévia e não tira o foco do teclado.
  useEffect(() => {
    if (!step) return;
    const all = files.flatMap((f) => f.matches.map((m) => ({ path: f.path, line: m.line, col: m.col })));
    if (!all.length) return;
    const idx = current ? all.findIndex((r) => r.path === current.path && r.line === current.line && r.col === current.col) : -1;
    const hit = all[idx < 0 ? (step.dir > 0 ? 0 : all.length - 1) : (idx + step.dir + all.length) % all.length];
    setCurrent(hit);
    setCollapsed((prev) => prev.filter((p) => p !== hit.path));
    onOpen(hit.path, hit.line, hit.col);
    // Só o `id` da tecla importa: cada toque é novo, mesmo na mesma direção.
  }, [step?.id]);

  // Mantém o resultado atual à vista. `nearest` não mexe na lista quando ele já está visível.
  useEffect(() => {
    resultsBox.current?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [current]);

  // Foco vai para a linha (ou o cabeçalho) vizinha antes de a atual sair da tela; sem vizinha, volta ao campo.
  const focusNeighbor = (leaving: Element, attr: 'data-row' | 'data-file') => {
    const rows = [...(resultsBox.current?.querySelectorAll<HTMLElement>(`[${attr}]`) ?? [])];
    const idx = rows.indexOf(leaving as HTMLElement);
    const next = rows[idx + 1] ?? rows[idx - 1];
    if (next) next.querySelector<HTMLElement>('button')?.focus();
    else field()?.focus();
  };
  // Dispensar uma linha só tira da tela. `row` vem do clique ou da tecla; pelo menu de contexto não há foco a mover.
  const dismissLine = (path: string, m: SearchMatch, row?: Element | null) => {
    if (row) focusNeighbor(row, 'data-row');
    setDismissedLines((prev) => [...prev, lineKey(path, m)]);
  };
  const dismissFile = (path: string, row?: Element | null) => {
    if (row) focusNeighbor(row, 'data-file');
    setDismissedFiles((prev) => [...prev, path]);
  };

  // Menus de contexto: copiar linha, caminho ou o que está na tela, e dispensar.
  const allText = () => files.map((f) => blockText(f.path, f.matches)).join('\n\n');
  const lineMenu = (path: string, m: SearchMatch, row?: Element | null) => [
    action('copy', 'Copiar', () => void copy(m.text.trimStart(), 'Linha copiada')),
    action('copy-path', 'Copiar caminho', () => void copy(path, 'Caminho copiado')),
    action('copy-all', 'Copiar tudo', () => void copy(allText(), `${lines} ${lines === 1 ? 'resultado copiado' : 'resultados copiados'}`)),
    DIVIDER,
    action('dismiss', 'Dispensar', () => dismissLine(path, m, row)),
  ];
  const fileMenu = (path: string, matches: SearchMatch[], row?: Element | null) => [
    action('copy-path', 'Copiar caminho', () => void copy(path, 'Caminho copiado')),
    action('copy-all', 'Copiar tudo', () => void copy(blockText(path, matches), 'Resultados do arquivo copiados')),
    DIVIDER,
    action('dismiss', 'Dispensar', () => dismissFile(path, row)),
  ];
  const pattern = highlighter(shown.query, shown.options);
  // Campo preenchido sem resposta ainda (o debounce espera) mostra "Pesquisando…", não a dica.
  const status = error || (pending ? 'Pesquisando…' : results ? summary(lines, files.length, results.truncated) : form.query ? 'Pesquisando…' : HINT);

  // Setas sobem e descem entre os resultados; Início e Fim vão ao primeiro e ao último.
  const onResultsKey = (e: ReactKeyboardEvent<HTMLElement>) => {
    const keys = ['ArrowDown', 'ArrowUp', 'Home', 'End'];
    if (!keys.includes(e.key)) return;
    const items = [...e.currentTarget.querySelectorAll<HTMLButtonElement>('[data-result]')];
    const at = items.indexOf(e.target as HTMLButtonElement);
    if (at < 0 && e.key !== 'Home' && e.key !== 'End') return;
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 : e.key === 'ArrowDown' ? at + 1 : at - 1;
    const target = items[Math.min(Math.max(next, 0), items.length - 1)];
    if (!target) return;
    e.preventDefault();
    target.focus();
  };

  // Posição que o próximo ArrowUp alcança. A consulta atual já está no topo do histórico (entrou ao buscar), então, sem navegação,
  // a primeira subida pula essa e traz a anterior.
  const upFrom = at < 0 && history[0] === form.query ? 1 : at + 1;

  const filterCount = (terms(form.include).length + terms(form.exclude).length) + (form.scope ? 1 : 0);
  const hasPreviewReview = !!preview;

  return (
    <Flexbox height="100%" className="min-h-0">
      <Flexbox horizontal align="center" gap={2} height={36} paddingInline={12}>
        <Text as="h2" ellipsis weight={500} fontSize={12} type="secondary" className="min-w-0 flex-1">Pesquisar</Text>
        <ActionIcon
          size="small" variant="borderless" icon={ListFilter} active={filtersOpen || filterCount > 0}
          aria-label="Filtros de caminho" aria-expanded={filtersOpen} title="Filtros de caminho: incluir e excluir"
          onClick={() => setFiltersOpen((v) => !v)} className="focus-visible:ring-2 focus-visible:ring-ring/50"
        />
        <ActionIcon
          size="small" variant="borderless" icon={Replace} active={replaceOpen}
          aria-label="Substituir" aria-expanded={replaceOpen} title="Substituir em vários arquivos"
          onClick={() => setReplaceOpen((v) => !v)} className="focus-visible:ring-2 focus-visible:ring-ring/50"
        />
        <ActionIcon
          size="small" variant="borderless" icon={RefreshCw} spin={pending} disabled={!form.query}
          aria-label="Atualizar" title="Atualizar: refaz a mesma busca" onClick={() => now(form)}
          className="focus-visible:ring-2 focus-visible:ring-ring/50"
        />
        <ActionIcon
          size="small" variant="borderless" icon={Eraser} disabled={!form.query}
          aria-label="Limpar" title="Limpar a pesquisa" onClick={clear}
          className="focus-visible:ring-2 focus-visible:ring-ring/50"
        />
        <ActionIcon
          size="small" variant="borderless" icon={allCollapsed ? ChevronsUpDown : ChevronsDownUp} disabled={!files.length}
          aria-label={allCollapsed ? 'Expandir tudo' : 'Recolher tudo'}
          title={allCollapsed ? 'Expandir todos os arquivos' : 'Recolher todos os arquivos'} onClick={toggleAll}
          className="focus-visible:ring-2 focus-visible:ring-ring/50"
        />
      </Flexbox>

      <Flexbox ref={fieldBox} paddingInline={8} paddingBlock={6} gap={6}>
        <Input
          size="small" value={form.query} placeholder="Pesquisar" aria-label="Pesquisar nos arquivos do projeto" style={{ minHeight: 32 }}
          onChange={(e) => {
            // Digitar sai da navegação do histórico: o texto novo vira o rascunho.
            setAt(-1);
            change({ query: e.target.value }, 'later');
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') now(form);
            // ArrowUp sobe no histórico (mais antiga); ArrowDown desce e, da mais recente, volta ao rascunho.
            else if (e.key === 'ArrowUp' && upFrom < history.length) {
              e.preventDefault();
              if (at < 0) typed.current = form.query;
              recall(upFrom);
            } else if (e.key === 'ArrowDown' && at >= 0) {
              e.preventDefault();
              recall(at - 1);
            }
          }}
          suffix={(
            <Flexbox horizontal align="center" flex="none" gap={2}>
              {TOGGLES.map(({ key, icon, label }) => (
                <ActionIcon
                  key={key} icon={icon} size={{ size: 18, blockSize: 24, borderRadius: 4 }} variant="borderless" active={form.options[key]}
                  aria-pressed={form.options[key]} aria-label={label} title={label}
                  onClick={() => toggle(key)} className="focus-visible:ring-2 focus-visible:ring-ring/50"
                />
              ))}
            </Flexbox>
          )}
        />

        {filtersOpen && (
          <Flexbox gap={6} aria-label="Filtros de caminho">
            {form.scope !== null && (
              <Flexbox horizontal align="center" gap={4}>
                <Text as="span" fontSize={12} type="secondary" className="min-w-0 flex-1" ellipsis title={form.scope || 'Projeto inteiro'}>
                  Só na pasta <Text as="span" code fontSize={12}>{form.scope || '/'}</Text>
                </Text>
                <ActionIcon
                  size="small" variant="borderless" icon={X} aria-label="Pesquisar no projeto inteiro"
                  title="Tirar o recorte e pesquisar no projeto inteiro" onClick={() => change({ scope: null }, 'now')}
                  className="focus-visible:ring-2 focus-visible:ring-ring/50"
                />
              </Flexbox>
            )}
            <Input
              size="small" value={form.include} aria-label="Incluir caminhos" placeholder="Incluir: src/*, *.ts"
              onChange={(e) => change({ include: e.target.value }, 'later')}
              onKeyDown={(e) => { if (e.key === 'Enter') now(form); }}
            />
            <Input
              size="small" value={form.exclude} aria-label="Excluir caminhos" placeholder="Excluir: *.test.ts, dist"
              onChange={(e) => change({ exclude: e.target.value }, 'later')}
              onKeyDown={(e) => { if (e.key === 'Enter') now(form); }}
            />
            <Text as="p" type="secondary" fontSize={11}>
              Trechos separados por vírgula. * vale qualquer sequência e ? um caractere. Não diferencia maiúsculas.
            </Text>
          </Flexbox>
        )}

        {replaceOpen && (
          <Flexbox gap={6}>
            <Input
              size="small" value={form.replacement} aria-label="Substituir por" placeholder="Substituir por (vazio apaga)"
              onChange={(e) => change({ replacement: e.target.value }, 'none')}
              onKeyDown={(e) => { if (e.key === 'Enter') void previewReplace(); }}
              suffix={(
                // Muda a substituição, então tira a prévia: a revisão antiga não vale com outra regra.
                <ActionIcon
                  icon={CaseUpper} size={{ size: 18, blockSize: 24, borderRadius: 4 }} variant="borderless" active={form.preserveCase}
                  aria-pressed={form.preserveCase} aria-label="Preservar maiúsculas" title="Preservar maiúsculas"
                  onClick={() => change({ preserveCase: !form.preserveCase }, 'none')} className="focus-visible:ring-2 focus-visible:ring-ring/50"
                />
              )}
            />
            <Flexbox horizontal align="center" gap={6}>
              <Button size="small" onClick={() => void previewReplace()} disabled={!form.query || previewing || applying} loading={previewing}>
                Ver substituições
              </Button>
              {preview && (
                <Button size="small" type="text" onClick={invalidate}>Fechar revisão</Button>
              )}
            </Flexbox>
            <Text as="p" type="secondary" fontSize={11}>{form.options.regex ? REGEX_HINT : REPLACE_HINT}</Text>
          </Flexbox>
        )}
      </Flexbox>

      {!hasPreviewReview && (
        <Text
          as="p" role="status" fontSize={12} type={error ? 'danger' : 'secondary'} className="select-text"
          style={{ paddingInline: 12, paddingBottom: 6 }}
        >
          {status}
        </Text>
      )}

      {report && (
        <Flexbox paddingInline={8} paddingBlock={4} gap={4}>
          {report.failure && <Alert type="error" role="alert" description={report.failure} className="select-text" />}
          {report.applied.length > 0 && (
            <Alert
              type={report.errors.length || report.draft.length ? 'warning' : 'success'}
              role="status"
              description={`${report.applied.length} ${report.applied.length === 1 ? 'arquivo alterado' : 'arquivos alterados'}, ${report.applied.reduce((n, a) => n + a.count, 0)} substituições.`}
            />
          )}
          {report.errors.length > 0 && (
            <Alert
              type="error" role="alert"
              description={(
                <Flexbox gap={2} className="select-text">
                  <span>{`${report.errors.length} ${report.errors.length === 1 ? 'arquivo não foi alterado' : 'arquivos não foram alterados'}:`}</span>
                  {report.errors.map((err) => <span key={err.path}>{`${err.path}: ${err.error}`}</span>)}
                </Flexbox>
              )}
            />
          )}
          {report.draft.length > 0 && (
            <Alert
              type="warning" role="status"
              description={`${report.draft.length} ${report.draft.length === 1 ? 'arquivo ficou' : 'arquivos ficaram'} de fora por ter alteração não salva no editor: ${report.draft.join(', ')}.`}
            />
          )}
        </Flexbox>
      )}

      {hasPreviewReview && preview ? (
        <Flexbox flex={1} className="min-h-0" gap={0}>
          <Flexbox horizontal align="center" gap={6} paddingInline={12} paddingBlock={4}>
            <Text as="p" fontSize={12} type="secondary" className="min-w-0 flex-1" ellipsis role="status">
              {`${preview.data.totalMatches} ${preview.data.totalMatches === 1 ? 'troca' : 'trocas'} em ${visibleFiles(preview).length} ${visibleFiles(preview).length === 1 ? 'arquivo' : 'arquivos'} · ${picked.length} marcado${picked.length === 1 ? '' : 's'}`}
            </Text>
            <Button size="small" type="text" onClick={() => setPicked(reviewed.filter(({ file }) => !draftOf(file.path)).map(({ file }) => file.path))}>Marcar todos</Button>
            <Button size="small" type="text" onClick={() => setPicked([])}>Nenhum</Button>
          </Flexbox>
          {preview.data.omittedFiles > 0 && (
            <Text as="p" fontSize={12} type="warning" style={{ paddingInline: 12 }}>
              {`${preview.data.omittedFiles} ${preview.data.omittedFiles === 1 ? 'arquivo com ocorrência ficou' : 'arquivos com ocorrência ficaram'} de fora desta revisão (limite). Não serão alterados.`}
            </Text>
          )}
          {preview.data.skippedOmitted + preview.data.skipped.length > 0 && (
            <Text as="p" fontSize={12} type="secondary" style={{ paddingInline: 12 }}>
              {`${preview.data.skipped.length + preview.data.skippedOmitted} ${preview.data.skipped.length + preview.data.skippedOmitted === 1 ? 'arquivo ficou' : 'arquivos ficaram'} de fora (binário, não UTF-8, grande ou especial). Não serão alterados.`}
            </Text>
          )}
          <Flexbox flex={1} className="min-h-0">
            <ScrollArea className="h-full" disableContentFit>
              <Flexbox paddingBlock={4}>
                {reviewed.map(({ file, pairs }) => {
                  const draft = draftOf(file.path);
                  const { name, dir } = split(file.path);
                  return (
                    <Flexbox as="section" key={file.path} paddingInline={8} paddingBlock={4} gap={2} className="border-b">
                      <label className={`flex min-w-0 items-center gap-2 ${draft ? 'opacity-60' : ''}`}>
                        <input
                          type="checkbox" className="size-4 shrink-0 accent-[var(--ant-color-primary)]"
                          checked={picked.includes(file.path) && !draft} disabled={draft}
                          aria-label={`Substituir em ${file.path}`}
                          onChange={() => togglePick(file.path)}
                        />
                        <Text as="span" fontSize={13} ellipsis className="min-w-0 shrink-0">{name}</Text>
                        {dir && <Text as="span" type="secondary" fontSize={12} ellipsis className="min-w-0 flex-1">{dir}</Text>}
                        <Text as="span" type="secondary" fontSize={11} className="ml-auto shrink-0 rounded-full bg-muted px-1.5 tabular-nums">{file.count}</Text>
                      </label>
                      {draft && (
                        <Text as="p" fontSize={12} type="warning">Alteração não salva no editor: salve ou descarte antes para substituir.</Text>
                      )}
                      {pairs.slice(0, REVIEW_LINES).map((pair) => (
                        <Flexbox key={pair.line} gap={0} className="font-mono text-[12px]">
                          <Text as="span" type="secondary" fontSize={11} className="tabular-nums">{pair.line}</Text>
                          <Text as="span" type="secondary" fontSize={12} ellipsis title={pair.before} className="line-through">
                            {marked(pair.before.trimStart(), highlighter(preview.query.text, preview.query))}
                          </Text>
                          <Text as="span" fontSize={12} ellipsis title={pair.after} className="bg-[var(--ant-color-success-bg)]">{pair.after.trimStart()}</Text>
                        </Flexbox>
                      ))}
                      {pairs.length > REVIEW_LINES && (
                        <Text as="span" type="secondary" fontSize={12}>{`e mais ${pairs.length - REVIEW_LINES} ${pairs.length - REVIEW_LINES === 1 ? 'linha' : 'linhas'}`}</Text>
                      )}
                    </Flexbox>
                  );
                })}
              </Flexbox>
            </ScrollArea>
          </Flexbox>
          <Flexbox horizontal align="center" justify="flex-end" gap={6} paddingInline={8} paddingBlock={6} className="border-t">
            <Button type="primary" size="small" onClick={() => setConfirming(true)} disabled={!chosen.length || applying} loading={applying}>
              {`Substituir em ${chosen.length} ${chosen.length === 1 ? 'arquivo' : 'arquivos'}`}
            </Button>
          </Flexbox>
        </Flexbox>
      ) : (
        <>
          {previewError && (
            <Text as="p" role="alert" fontSize={12} type="danger" className="select-text" style={{ paddingInline: 12, paddingBottom: 6 }}>{previewError}</Text>
          )}
          <Flexbox flex={1} className="min-h-0">
            <ScrollArea className="h-full" disableContentFit>
              <Flexbox ref={resultsBox} paddingBlock={4} onKeyDown={onResultsKey}>
                {skipped.length > 0 && (
                  <details className="px-2 py-1 text-[12px] text-muted-foreground">
                    <summary className="cursor-pointer select-none">
                      {`${skipped.length + (results?.skippedOmitted ?? 0)} ${skipped.length + (results?.skippedOmitted ?? 0) === 1 ? 'arquivo ficou' : 'arquivos ficaram'} de fora`}
                    </summary>
                    <Flexbox gap={2} paddingBlock={2}>
                      {skipped.map((s) => (
                        <Text key={s.path} as="span" fontSize={12} ellipsis title={`${s.path}: ${REASON[s.reason]}`}>{`${s.path}: ${REASON[s.reason]}`}</Text>
                      ))}
                      {!!results?.skippedOmitted && (
                        <Text as="span" fontSize={12}>{`e mais ${results.skippedOmitted}`}</Text>
                      )}
                    </Flexbox>
                  </details>
                )}
                {files.map((file) => {
                  const open = !collapsed.includes(file.path);
                  const { name, dir } = split(file.path);
                  const draft = draftOf(file.path);
                  return (
                    <Flexbox as="section" key={file.path}>
                      {/* O botão de dispensar fica ao lado do de abrir, não dentro dele: botão dentro de botão é HTML inválido. Mostra com o mouse ou o foco na linha. */}
                      <ContextMenuTrigger items={() => fileMenu(file.path, file.matches)}>
                        <Flexbox horizontal align="center" gap={2} className="group/file" data-file>
                          <button
                            type="button" aria-expanded={open} onClick={() => toggleFile(file.path)} title={file.path}
                            className="flex h-6 min-w-0 flex-1 items-center gap-1.5 rounded-sm pl-1 pr-2 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50"
                          >
                            {open
                              ? <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
                              : <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />}
                            <File className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
                            {/* Nome e pasta dividem o espaço do grupo: o nome tem prioridade, até 70% dele; a pasta é que encolhe primeiro. Sem pasta, o nome usa tudo. */}
                            <Flexbox horizontal align="center" flex={1} gap={6} className="min-w-0">
                              <Text as="span" fontSize={13} ellipsis className={dir ? 'max-w-[70%] shrink-0' : 'min-w-0 flex-1'}>{name}</Text>
                              {dir && <Text as="span" type="secondary" fontSize={12} ellipsis className="min-w-0 flex-1">{dir}</Text>}
                            </Flexbox>
                            {draft && <Text as="span" type="warning" fontSize={11} className="shrink-0">alterado</Text>}
                            <Text as="span" type="secondary" fontSize={11} className="ml-auto shrink-0 rounded-full bg-muted px-1.5 tabular-nums">{file.matches.length}</Text>
                          </button>
                          <ActionIcon
                            size="small" variant="borderless" icon={X} aria-label="Dispensar" title="Dispensar"
                            onClick={(e) => dismissFile(file.path, e.currentTarget.closest('[data-file]'))}
                            className="shrink-0 opacity-0 group-hover/file:opacity-100 group-focus-within/file:opacity-100 focus-visible:ring-2 focus-visible:ring-ring/50"
                          />
                        </Flexbox>
                      </ContextMenuTrigger>
                      {open && file.matches.map((m) => {
                        const where = `${file.path}:${m.line}:${m.col}`;
                        return (
                          // Cada linha tem o botão que abre (Enter abre de forma permanente, clique e Espaço abrem como prévia; duplo clique abre permanente) e o de dispensar.
                          <ContextMenuTrigger key={`${m.line}:${m.col}`} items={() => lineMenu(file.path, m)}>
                            <Flexbox horizontal align="center" gap={2} className="group/row" data-row>
                              <button
                                type="button" data-result title={where} aria-current={isCurrent(file.path, m) ? 'true' : undefined}
                                onClick={() => { mark(file.path, m); onOpen(file.path, m.line, m.col); }}
                                onDoubleClick={() => onOpen(file.path, m.line, m.col, { permanent: true })}
                                onKeyDown={(e) => {
                                  if (e.key === 'Delete') {
                                    e.preventDefault();
                                    dismissLine(file.path, m, e.currentTarget.closest('[data-row]'));
                                    return;
                                  }
                                  if (e.key !== 'Enter') return;
                                  e.preventDefault();
                                  mark(file.path, m);
                                  onOpen(file.path, m.line, m.col, { permanent: true });
                                }}
                                className={`flex h-6 min-w-0 flex-1 items-center gap-2 rounded-sm pl-6 pr-2 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 ${isCurrent(file.path, m) ? 'bg-muted' : ''}`}
                              >
                                <Text as="span" type="secondary" fontSize={12} className="min-w-8 shrink-0 text-right font-mono tabular-nums">{m.line}</Text>
                                <Text as="span" fontSize={12} ellipsis className="min-w-0 flex-1 font-mono">
                                  {marked(m.text.trimStart(), pattern)}
                                </Text>
                              </button>
                              <ActionIcon
                                size="small" variant="borderless" icon={X} aria-label="Dispensar" title="Dispensar"
                                onClick={(e) => dismissLine(file.path, m, e.currentTarget.closest('[data-row]'))}
                                className="mr-1 shrink-0 opacity-0 group-hover/row:opacity-100 group-focus-within/row:opacity-100 focus-visible:ring-2 focus-visible:ring-ring/50"
                              />
                            </Flexbox>
                          </ContextMenuTrigger>
                        );
                      })}
                    </Flexbox>
                  );
                })}
              </Flexbox>
            </ScrollArea>
          </Flexbox>
        </>
      )}

      {/* Confirmação concreta: quantos arquivos e quantas trocas, e quais. Os com alteração não salva já não entram aqui. */}
      <Modal
        open={confirming}
        title={`Substituir em ${chosen.length} ${chosen.length === 1 ? 'arquivo' : 'arquivos'}?`}
        closable={false}
        keyboard
        maskClosable
        onCancel={() => !applying && setConfirming(false)}
        footer={(
          <Flexbox horizontal justify="flex-end" gap={8}>
            <Button onClick={() => setConfirming(false)} disabled={applying}>Cancelar</Button>
            <Button type="primary" danger onClick={() => void applyChosen()} disabled={applying} loading={applying}>
              Substituir
            </Button>
          </Flexbox>
        )}
      >
        <Flexbox gap={8}>
          <Text as="p" fontSize={14}>
            {`${chosenCount} ${chosenCount === 1 ? 'troca' : 'trocas'} por ${preview?.replacement ? `“${preview.replacement}”` : 'nada (apagar)'}. As alterações vão para o disco; para voltar atrás, use o Git do projeto.`}
          </Text>
          <Flexbox gap={2} style={{ maxHeight: 180, overflow: 'auto' }}>
            {chosen.slice(0, CONFIRM_LIST).map(({ file }) => (
              <Text key={file.path} as="span" code fontSize={12} ellipsis title={file.path}>{`${file.path} (${file.count})`}</Text>
            ))}
            {chosen.length > CONFIRM_LIST && <Text as="span" type="secondary" fontSize={12}>{`e mais ${chosen.length - CONFIRM_LIST}`}</Text>}
          </Flexbox>
        </Flexbox>
      </Modal>
    </Flexbox>
  );
}
