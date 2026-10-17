// A pesquisa de texto nos arquivos do projeto, na coluna da árvore de pastas, no layout do painel Pesquisar do Antigravity/VS Code:
// cabeçalho com Atualizar, Limpar, Árvore/Lista e Recolher; a seta à esquerda abre o Substituir embaixo da busca; o `...` abre os
// campos de incluir e excluir. Cada ocorrência é uma linha da lista, agrupada por arquivo (ou por pasta, na árvore).
// A busca roda no backend (comando `search`). Com texto no Substituir, a prévia (`replace_preview`, não grava) mostra na própria
// lista o que sai riscado e o que entra; a troca por arquivo e o Substituir Tudo gravam com `replace_apply`, só nos arquivos da prévia.
import { type KeyboardEvent as ReactKeyboardEvent, useEffect, useMemo, useRef, useState } from 'react';
import {
  BookOpen, CaseSensitive, CaseUpper, ChevronRight, ChevronsDownUp, ChevronsUpDown, Ellipsis, List, ListTree, ListX,
  type LucideIcon, Regex, ReplaceAll, RotateCw, WholeWord, X,
} from 'lucide-react';
import { ActionIcon, Button, ContextMenuTrigger, Input, Modal, ScrollArea, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { backend, message } from './backend';
import { FileIcon } from './file-icon';
import { copy, join, splitPath } from './git-history';
import { action, DIVIDER } from './file-tabs';
import type { ReplacePreview, ReplaceQuery, SearchFile, SearchMatch, SearchOptions, SearchResults, SkipReason } from './types';

const DELAY_MS = 300;
// Mesmo limite do backend: com mais linhas casadas, só as primeiras aparecem e a lista avisa.
const LIMIT = 2000;
// Histórico de consultas no navegador do app: só as mais recentes, sem repetir.
const HISTORY_KEY = 'orquestrador.search.history';
const HISTORY_MAX = 20;
// Lista ou árvore, guardado no navegador do app. O padrão é a lista, como no VS Code.
const VIEW_KEY = 'orquestrador.search.view';
// Como no VS Code: o trecho antes da ocorrência fica com no máximo 26 caracteres (cortado em limite de palavra, com "…").
const BEFORE_MAX = 26;
const AFTER_MAX = 250;
// Quantos arquivos o diálogo de confirmação lista antes de resumir.
const CONFIRM_LIST = 12;
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
// Medidas do Antigravity: botão dentro do campo com 20 px e ícone de 16 px; linhas da lista com 22 px e recuo de 8 px por nível.
const INPUT_TOGGLE = { size: 16, blockSize: 20, borderRadius: 3 } as const;
const INPUT_STYLE = { height: 26 };
const INDENT = 8;
const FOCUS = 'focus-visible:ring-2 focus-visible:ring-ring/50';

/** Tudo o que a busca e a substituição usam, do jeito que está nos campos. */
type Form = {
  query: string;
  options: SearchOptions;
  // Filtros de caminho como digitados: trechos separados por vírgula. `./` no começo vale a partir da raiz do projeto.
  include: string;
  exclude: string;
  // Só nos arquivos abertos no editor.
  onlyOpen: boolean;
  replacement: string;
  // Substituição que segue a caixa da ocorrência (ver `ReplaceQuery.preserveCase`).
  preserveCase: boolean;
};
const EMPTY_FORM: Form = { query: '', options: NO_OPTIONS, include: '', exclude: '', onlyOpen: false, replacement: '', preserveCase: false };

// Caracteres especiais escapados quando a consulta é texto, não regex.
const escapeRegExp = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** RegExp das ocorrências, com as mesmas opções da busca. Se não puder ser montada (regex que só o backend entende), devolve null. */
function highlighter(query: string, options: SearchOptions): RegExp | null {
  if (!query) return null;
  try {
    const source = options.regex ? query : escapeRegExp(query);
    return new RegExp(options.wholeWord ? `\\b(?:${source})\\b` : source, options.caseSensitive ? 'g' : 'gi');
  } catch {
    return null;
  }
}

/** O `lcut` do VS Code: tira os espaços do começo e, se passar de `max`, corta no começo em limite de palavra e põe `prefix`. */
export function lcut(text: string, max: number, prefix = '…'): string {
  const trimmed = text.trimStart();
  if (trimmed.length < max) return trimmed;
  const re = /\b/g;
  let at = 0;
  while (re.test(trimmed) && !(trimmed.length - re.lastIndex < max)) {
    at = re.lastIndex;
    re.lastIndex += 1;
  }
  return at === 0 ? trimmed : prefix + trimmed.substring(at).trimStart();
}

/** Uma ocorrência na lista. `start`/`end` são posições em `text` (a janela que o backend mandou); -1 quando a RegExp daqui não achou. */
type Hit = { path: string; line: number; col: number; text: string; start: number; end: number; offset: number };

/** As ocorrências de uma linha do backend. `col` de cada uma sai da coluna da primeira (UTF-16, como o índice das strings do JS).
 *  `offset` é onde a janela começa na linha inteira, para achar a ocorrência na prévia. Sem casamento aqui, a linha vira uma só. */
export function hitsOf(path: string, m: SearchMatch, pattern: RegExp | null): Hit[] {
  const found = pattern ? [...m.text.matchAll(pattern)].filter((x) => x[0]) : [];
  if (!found.length) return [{ path, line: m.line, col: m.col, text: m.text, start: -1, end: -1, offset: 0 }];
  const first = found[0].index;
  return found.map((x) => ({
    path, line: m.line, col: m.col + x.index - first, text: m.text, start: x.index, end: x.index + x[0].length, offset: m.col - 1 - first,
  }));
}

/** O que entra no lugar de cada ocorrência de `before`, lido de `after` (a linha que o backend calculou). O texto entre as ocorrências
 *  não muda, então ele serve de âncora. Devolve posição em `before` → substituto, ou null quando o alinhamento é ambíguo. */
export function replacementsOf(before: string, after: string, pattern: RegExp): Map<number, string> | null {
  const found = [...before.matchAll(pattern)].filter((x) => x[0]);
  if (!found.length) return null;
  const gaps: string[] = [];
  let last = 0;
  for (const f of found) {
    gaps.push(before.slice(last, f.index));
    last = f.index + f[0].length;
  }
  const tail = before.slice(last);
  if (!after.startsWith(gaps[0]) || !after.endsWith(tail) || after.length < gaps[0].length + tail.length) return null;
  const end = after.length - tail.length;
  const out = new Map<number, string>();
  let pos = gaps[0].length;
  for (let i = 0; i < found.length; i++) {
    const gap = gaps[i + 1];
    if (gap === '') return null; // ocorrências coladas: não dá para saber onde uma termina
    const next = gap === undefined ? end : after.indexOf(gap, pos);
    if (next < pos || next > end) return null;
    out.set(found[i].index, after.slice(pos, next));
    pos = next + (gap?.length ?? 0);
  }
  return out;
}

// Trechos separados por vírgula, sem espaço nas pontas e sem os vazios.
const terms = (text: string) => text.split(',').map((t) => t.trim()).filter(Boolean);
const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** Resumo da busca: contagem de resultados e de arquivos, com o aviso do limite quando passou dele. */
function summary(hits: number, files: number, truncated: boolean): string {
  if (!hits) return 'Nenhum resultado.';
  const text = `${plural(hits, 'resultado', 'resultados')} em ${plural(files, 'arquivo', 'arquivos')}`;
  return truncated ? `${text}. Só as ${LIMIT} primeiras linhas aparecem.` : text;
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

/** Grava no armazenamento do navegador. Sem espaço ou sem acesso, segue sem persistir. */
function store(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Sem armazenamento: vale só nesta sessão.
  }
}

function readView(): 'list' | 'tree' {
  try {
    return localStorage.getItem(VIEW_KEY) === 'tree' ? 'tree' : 'list';
  } catch {
    return 'list';
  }
}

// Chave de uma ocorrência dispensada: caminho e posição juntos, para não confundir arquivos diferentes.
const hitKey = (h: { path: string; line: number; col: number }) => JSON.stringify([h.path, h.line, h.col]);

// Texto de um arquivo para a área de transferência: caminho numa linha e, depois, "  linha: texto" por resultado.
const blockText = (path: string, hits: Hit[]) =>
  [path, ...[...new Map(hits.map((h) => [h.line, h])).values()].map((h) => `  ${h.line}: ${h.text.trimStart()}`)].join('\n');

/** Linhas de um texto da prévia, sem o CR do fim. A substituição não cria nem junta linhas: o índice pareia original e resultado. */
const linesOf = (text: string) => text.split('\n').map((l) => l.replace(/\r$/, ''));

/** Uma linha da lista na tela: pasta (só na árvore), arquivo ou ocorrência. */
type Row =
  | { kind: 'dir'; key: string; path: string; label: string; depth: number; count: number }
  | { kind: 'file'; key: string; path: string; depth: number; hits: Hit[]; tree: boolean }
  | { kind: 'hit'; key: string; hit: Hit; depth: number };

type Dir = { name: string; path: string; dirs: Map<string, Dir>; files: { path: string; hits: Hit[] }[] };

/** As linhas da tela, na ordem. Na árvore, pastas com uma só subpasta e sem arquivo ficam numa linha só ("src/components"). */
function rowsOf(files: { path: string; hits: Hit[] }[], view: 'list' | 'tree', collapsed: Set<string>): Row[] {
  const rows: Row[] = [];
  const pushFile = (f: { path: string; hits: Hit[] }, depth: number, tree: boolean) => {
    const key = `f:${f.path}`;
    rows.push({ kind: 'file', key, path: f.path, depth, hits: f.hits, tree });
    if (!collapsed.has(key)) for (const h of f.hits) rows.push({ kind: 'hit', key: hitKey(h), hit: h, depth: depth + 1 });
  };
  if (view === 'list') {
    for (const f of files) pushFile(f, 0, false);
    return rows;
  }
  const root: Dir = { name: '', path: '', dirs: new Map(), files: [] };
  for (const f of files) {
    let node = root;
    for (const part of splitPath(f.path).dir.split('/').filter(Boolean)) {
      const path = join(node.path, part);
      if (!node.dirs.has(part)) node.dirs.set(part, { name: part, path, dirs: new Map(), files: [] });
      node = node.dirs.get(part)!;
    }
    node.files.push(f);
  }
  const count = (d: Dir): number => d.files.reduce((n, f) => n + f.hits.length, 0) + [...d.dirs.values()].reduce((n, c) => n + count(c), 0);
  const walk = (dir: Dir, depth: number) => {
    for (const child of [...dir.dirs.values()].sort((a, b) => a.name.localeCompare(b.name))) {
      let node = child;
      let label = child.name;
      while (node.files.length === 0 && node.dirs.size === 1) {
        node = [...node.dirs.values()][0];
        label += `/${node.name}`;
      }
      const key = `d:${node.path}`;
      rows.push({ kind: 'dir', key, path: node.path, label, depth, count: count(node) });
      if (!collapsed.has(key)) walk(node, depth + 1);
    }
    for (const f of dir.files) pushFile(f, depth, true);
  };
  walk(root, 0);
  return rows;
}

// As chaves das pastas da árvore (comprimidas ou não) acima de um caminho: abrir todas mostra o arquivo.
const ancestors = (path: string) => {
  const parts = splitPath(path).dir.split('/').filter(Boolean);
  return parts.map((_, i) => `d:${parts.slice(0, i + 1).join('/')}`);
};

/** A prévia que está valendo: a resposta do backend e, por arquivo, a revisão e as linhas antes e depois. */
type Preview = { key: string; query: ReplaceQuery; replacement: string; data: ReplacePreview };
type Report = { replaced: number; files: number; replacement: string; errors: { path: string; error: string }[]; left: string[] };

/** Pesquisa de texto e substituição no layout do Antigravity: cabeçalho, campos, detalhes, resumo e a lista de resultados. */
export function SearchScreen({ active, focusKey, onOpen, folder, seed, refreshKey, step, hasDraft, openPaths, onChanged }: {
  active: boolean;
  focusKey: number;
  /** Abre o arquivo na linha e na coluna (UTF-16, a partir de 1). Clique simples pede prévia; duplo clique ou Enter pedem permanente. */
  onOpen: (path: string, line: number, col?: number, options?: { permanent?: boolean }) => void;
  /** Pedido de pesquisa só nesta pasta (`path` relativo, '' é o projeto): vira `./pasta` no campo de incluir. Um objeto novo a cada pedido. */
  folder: { path: string } | null;
  /** Texto selecionado no editor: vira a consulta e a busca roda na hora. Um objeto novo a cada pedido. */
  seed: { text: string } | null;
  /** Sobe a cada gravação no editor: refaz a busca atual. */
  refreshKey: number;
  /** F4 (`dir` 1) ou Shift+F4 (`dir` -1): anda pelos resultados. Um objeto novo a cada tecla. */
  step: { dir: 1 | -1 } | null;
  /** Diz se o arquivo tem alteração não salva no editor. Consultado ao exibir e de novo ao substituir. */
  hasDraft: (path: string) => boolean;
  /** Caminhos abertos no editor, para "só nos editores abertos". */
  openPaths: () => string[];
  /** Chamado depois que a substituição gravou arquivos, para o App recarregar o que estiver aberto. */
  onChanged: () => void;
}) {
  const [form, setForm] = useState<Form>(EMPTY_FORM);
  const [results, setResults] = useState<SearchResults | null>(null);
  // Consulta e opções da resposta que está na tela: o destaque usa estas, não as do campo.
  const [shown, setShown] = useState<{ query: string; options: SearchOptions }>({ query: '', options: NO_OPTIONS });
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  const [view, setView] = useState<'list' | 'tree'>(readView);
  // Arquivos (`f:`) e pastas (`d:`) recolhidos. Começam todos abertos a cada busca nova.
  const [collapsed, setCollapsed] = useState<string[]>([]);
  // Resultados dispensados na tela (não mexem no disco). Zerados a cada busca nova.
  const [dismissedFiles, setDismissedFiles] = useState<string[]>([]);
  const [dismissedHits, setDismissedHits] = useState<string[]>([]);
  // Histórico de consultas, a mais recente primeiro. `at` é a posição de navegação com ArrowUp/ArrowDown (-1 é o texto digitado).
  const [history, setHistory] = useState<string[]>(readHistory);
  const [at, setAt] = useState(-1);
  const typed = useRef('');
  // Trocar pelo histórico não deve reordená-lo: a busca feita por ele não entra de novo no histórico.
  const skipHistory = useRef(false);
  const resultsBox = useRef<HTMLDivElement>(null);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [replaceOpen, setReplaceOpen] = useState(false);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [previewError, setPreviewError] = useState('');
  const [report, setReport] = useState<Report | null>(null);
  const [applying, setApplying] = useState(false);
  // Substituir Tudo espera a confirmação com a prévia que será aplicada.
  const [confirming, setConfirming] = useState<Preview | null>(null);
  // Ref no contêiner do campo, não no `Input`: o lobehub lê `element.ref` e o React 19 avisa. O campo é achado dentro do contêiner.
  const fieldBox = useRef<HTMLDivElement>(null);
  const field = () => fieldBox.current?.querySelector('input') ?? null;
  const timer = useRef(0);
  const previewTimer = useRef(0);
  // Só a resposta mais nova vale: uma antiga que chega depois é descartada (busca e prévia, cada uma com o seu contador).
  const latest = useRef(0);
  const latestPreview = useRef(0);
  // Resultado atual (clique ou F4): a linha marcada na lista e de onde o F4 sai.
  const [current, setCurrent] = useState<{ path: string; line: number; col: number } | null>(null);
  // Texto do pedido de seleção que espera aparecer no campo para ser selecionado.
  const selectAfter = useRef<string | null>(null);

  const filtersOf = (f: Form) => ({ include: terms(f.include), exclude: terms(f.exclude), ...(f.onlyOpen ? { only: openPaths() } : {}) });
  const queryOf = (f: Form): ReplaceQuery => ({ text: f.query, ...f.options, ...filtersOf(f), preserveCase: f.preserveCase });
  // A prévia vale para esta combinação de consulta, filtros e substituição; mudou qualquer um, ela é refeita.
  const previewKey = (f: Form) => JSON.stringify([queryOf(f), f.replacement]);

  // Ao entrar na tela, e a cada Ctrl+Shift+F (`focusKey`), o campo recebe o foco com o texto selecionado.
  useEffect(() => {
    if (!active) return;
    field()?.focus();
    field()?.select();
  }, [active, focusKey]);

  // Localizar na pasta: a pasta vai para o campo de incluir (como o VS Code faz), os detalhes abrem e a busca refaz, se há consulta.
  useEffect(() => {
    if (!folder) return;
    const next = { ...form, include: folder.path ? `./${folder.path}` : '' };
    setForm(next);
    setDetailsOpen(true);
    if (next.query) now(next);
    field()?.focus();
  }, [folder]);

  // Texto selecionado no editor: vira a consulta e a busca roda na hora, como Enter. O campo é selecionado depois que o texto novo aparece nele.
  useEffect(() => {
    if (!seed) return;
    setAt(-1);
    const next = { ...form, query: seed.text };
    setForm(next);
    now(next);
    if (seed.text === form.query) field()?.select();
    else selectAfter.current = seed.text;
  }, [seed]);

  // Depois do render, seleciona o campo quando o texto do pedido já está nele. Sem pedido pendente, não faz nada.
  useEffect(() => {
    if (selectAfter.current === null || field()?.value !== selectAfter.current) return;
    selectAfter.current = null;
    field()?.select();
  });

  // Gravação no editor: refaz a busca atual sem entrar no histórico, sem roubar o foco e sem tirar o recolhido e o dispensado.
  useEffect(() => {
    if (!refreshKey || !form.query) return;
    void run(form, true);
    // Só o contador de gravações dispara; o resto vem do render atual.
  }, [refreshKey]);

  useEffect(() => store(HISTORY_KEY, JSON.stringify(history)), [history]);
  useEffect(() => store(VIEW_KEY, view), [view]);

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
      const found = await backend.search(f.query, f.options, filtersOf(f));
      if (id !== latest.current) return;
      setResults(found);
      setShown({ query: f.query, options: f.options });
      if (quiet) {
        // Recolhido só sai da lista se o arquivo sumiu da busca.
        setCollapsed((prev) => prev.filter((k) => !k.startsWith('f:') || found.files.some((x) => `f:${x.path}` === k)));
      } else {
        setCollapsed([]);
        setDismissedFiles([]);
        setDismissedHits([]);
      }
    } catch (e) {
      if (id !== latest.current) return;
      setResults(null);
      setError(message(e));
    } finally {
      if (id === latest.current) setPending(false);
    }
  }

  // Digitar espera um pouco; Enter, o botão Atualizar e as opções buscam na hora.
  function now(next: Form) {
    window.clearTimeout(timer.current);
    void run(next);
  }
  const later = (next: Form) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void run(next), DELAY_MS);
  };

  // Muda campos do formulário e busca (agora ou com espera, conforme `mode`). O relatório da última substituição sai.
  const change = (patch: Partial<Form>, mode: 'later' | 'now' | 'none') => {
    const next = { ...form, ...patch };
    setForm(next);
    setReport(null);
    if (mode === 'later') later(next);
    if (mode === 'now') now(next);
  };

  const toggle = (key: keyof SearchOptions) => change({ options: { ...form.options, [key]: !form.options[key] } }, 'now');

  // Limpar: como o VS Code, tira a consulta, a substituição, os filtros e os resultados.
  const clear = () => {
    setAt(-1);
    change({ query: '', replacement: '', include: '', exclude: '', onlyOpen: false }, 'now');
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

  // A prévia aparece na lista com o Substituir aberto e algum texto nele (vazio apaga, mas só se vê ao confirmar, como no VS Code).
  const showReplace = replaceOpen && form.replacement !== '' && !!form.query;
  const wantedKey = showReplace ? previewKey(form) : '';

  // Pede a prévia para a forma atual. Não grava nada; uma resposta antiga é descartada.
  async function loadPreview(f: Form): Promise<Preview | null> {
    const id = ++latestPreview.current;
    const query = queryOf(f);
    try {
      const data = await backend.replacePreview(query, f.replacement);
      if (id !== latestPreview.current) return null;
      const next: Preview = { key: previewKey(f), query, replacement: f.replacement, data };
      setPreview(next);
      setPreviewError('');
      return next;
    } catch (e) {
      if (id !== latestPreview.current) return null;
      setPreview(null);
      setPreviewError(message(e));
      return null;
    }
  }

  // A prévia acompanha os campos (com a mesma espera da busca) e cada resposta nova da busca (uma gravação muda as revisões).
  useEffect(() => {
    window.clearTimeout(previewTimer.current);
    if (!wantedKey) {
      latestPreview.current++;
      setPreviewError('');
      return;
    }
    const f = form;
    previewTimer.current = window.setTimeout(() => void loadPreview(f), DELAY_MS);
  }, [wantedKey, results]);

  const livePreview = preview && preview.key === wantedKey ? preview : null;
  const pattern = highlighter(shown.query, shown.options);

  // Substitutos por arquivo e linha, lidos da prévia: posição na linha inteira → texto que entra.
  const replacements = useMemo(() => {
    const out = new Map<string, Map<number, Map<number, string> | null>>();
    if (!livePreview || !pattern) return out;
    for (const f of livePreview.data.files) {
      const before = linesOf(f.original);
      const after = linesOf(f.result);
      const lines = new Map<number, Map<number, string> | null>();
      before.forEach((b, i) => {
        if (b !== after[i]) lines.set(i + 1, replacementsOf(b, after[i] ?? '', new RegExp(pattern.source, pattern.flags)));
      });
      out.set(f.path, lines);
    }
    return out;
  }, [livePreview, shown]);
  // O que entra no lugar da ocorrência, ou null quando a prévia não diz (arquivo fora dela ou linha ambígua).
  const replacementOf = (h: Hit) => (h.start < 0 ? null : replacements.get(h.path)?.get(h.line)?.get(h.offset + h.start) ?? null);

  // Grava a substituição nos arquivos dados (`null` é tudo o que está na tela). Arquivo com rascunho, com ocorrência dispensada ou fora da
  // prévia fica de fora e é dito no relatório. `confirmed` é a prévia já confirmada no diálogo do Substituir Tudo.
  async function replaceIn(paths: string[] | null, confirmed?: Preview) {
    if (!form.query || applying) return;
    const prev = confirmed ?? (livePreview ?? (await loadPreview(form)));
    if (!prev) return;
    const visible = new Set(files.map((f) => f.path));
    const wanted = prev.data.files.filter((f) => visible.has(f.path) && (!paths || paths.includes(f.path)));
    if (!paths && !confirmed) {
      setConfirming(prev);
      return;
    }
    const left: string[] = [];
    const picks = wanted.filter((f) => {
      const why = hasDraft(f.path) ? 'alteração não salva no editor' : partial.has(f.path) ? 'ocorrência dispensada (a troca é do arquivo inteiro)' : '';
      if (why) left.push(`${f.path}: ${why}`);
      return !why;
    });
    for (const p of paths ?? []) if (!prev.data.files.some((f) => f.path === p)) left.push(`${p}: ficou fora da prévia (limite de arquivos)`);
    if (!picks.length) {
      setReport({ replaced: 0, files: 0, replacement: prev.replacement, errors: [], left });
      return;
    }
    setApplying(true);
    try {
      const done = await backend.replaceApply({
        query: prev.query,
        replacement: prev.replacement,
        files: picks.map((f) => ({ path: f.path, revision: f.revision })),
      });
      setReport({ replaced: done.applied.reduce((n, a) => n + a.count, 0), files: done.applied.length, replacement: prev.replacement, errors: done.errors, left });
      if (done.applied.length) onChanged();
      await run(form, true);
    } catch (e) {
      setReport({ replaced: 0, files: 0, replacement: prev.replacement, errors: [{ path: '', error: message(e) }], left });
    } finally {
      setApplying(false);
    }
  }

  // ---------- resultados ----------

  // `files` é o que está na tela: sem os dispensados. Resumo, recolher e navegação contam só isto.
  const gone = new Set(dismissedHits);
  const scoped = (results?.files ?? []).filter((f) => !dismissedFiles.includes(f.path));
  const allHits = (f: SearchFile) => f.matches.flatMap((m) => hitsOf(f.path, m, pattern));
  const withHits = scoped.map((f) => ({ path: f.path, all: allHits(f) }));
  const files = withHits.map(({ path, all }) => ({ path, hits: all.filter((h) => !gone.has(hitKey(h))) })).filter((f) => f.hits.length > 0);
  // Arquivos com alguma ocorrência dispensada: a troca grava o arquivo inteiro, então eles ficam fora dela.
  const partial = new Set(withHits.filter(({ all }) => all.some((h) => gone.has(hitKey(h)))).map(({ path }) => path));
  const skipped = results?.skipped ?? [];
  const hitCount = files.reduce((n, f) => n + f.hits.length, 0);
  const collapsedSet = new Set(collapsed);
  const rows = rowsOf(files, view, collapsedSet);
  const collapsible = view === 'tree' ? rowsOf(files, view, new Set()).filter((r) => r.kind !== 'hit').map((r) => r.key) : files.map((f) => `f:${f.path}`);
  const someOpen = collapsible.some((k) => !collapsedSet.has(k));
  const toggleAll = () => setCollapsed(someOpen ? collapsible : []);
  const toggleRow = (key: string) => setCollapsed((prev) => (prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key]));
  const mark = (h: Hit) => setCurrent({ path: h.path, line: h.line, col: h.col });
  const isCurrent = (h: Hit) => current?.path === h.path && current.line === h.line && current.col === h.col;

  // F4 e Shift+F4: anda pela ordem da tela (arquivo recolhido entra também), com volta. Abre como prévia e não tira o foco do teclado.
  useEffect(() => {
    if (!step) return;
    const all = files.flatMap((f) => f.hits);
    if (!all.length) return;
    const idx = current ? all.findIndex((h) => isCurrent(h)) : -1;
    const hit = all[idx < 0 ? (step.dir > 0 ? 0 : all.length - 1) : (idx + step.dir + all.length) % all.length];
    mark(hit);
    const open = new Set([`f:${hit.path}`, ...ancestors(hit.path)]);
    setCollapsed((prev) => prev.filter((k) => !open.has(k)));
    onOpen(hit.path, hit.line, hit.col);
  }, [step]);

  // Mantém o resultado atual à vista. `nearest` não mexe na lista quando ele já está visível.
  useEffect(() => {
    resultsBox.current?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [current]);

  // Foco vai para a linha vizinha antes de a atual sair da tela; sem vizinha, volta ao campo.
  const focusNeighbor = (leaving: Element) => {
    const items = [...(resultsBox.current?.querySelectorAll<HTMLElement>('[data-result]') ?? [])];
    const own = leaving.querySelector('[data-result]');
    const idx = items.indexOf(own as HTMLElement);
    const next = items.slice(idx + 1).find((el) => !leaving.contains(el)) ?? items.slice(0, Math.max(idx, 0)).findLast((el) => !leaving.contains(el));
    if (next) next.focus();
    else field()?.focus();
  };
  // Dispensar só tira da tela. `row` vem do clique ou da tecla; pelo menu de contexto não há foco a mover.
  const dismissHit = (h: Hit, row?: Element | null) => {
    if (row) focusNeighbor(row);
    setDismissedHits((prev) => [...prev, hitKey(h)]);
  };
  const dismissFile = (path: string, row?: Element | null) => {
    if (row) focusNeighbor(row);
    setDismissedFiles((prev) => [...prev, path]);
  };

  // Menus de contexto: copiar linha, caminho ou o que está na tela, e dispensar.
  const allText = () => files.map((f) => blockText(f.path, f.hits)).join('\n\n');
  const hitMenu = (h: Hit) => [
    action('copy', 'Copiar', () => void copy(h.text.trimStart(), 'Linha copiada')),
    action('copy-path', 'Copiar caminho', () => void copy(h.path, 'Caminho copiado')),
    action('copy-all', 'Copiar tudo', () => void copy(allText(), `${plural(hitCount, 'resultado copiado', 'resultados copiados')}`)),
    DIVIDER,
    action('dismiss', 'Dispensar', () => dismissHit(h)),
  ];
  const fileMenu = (path: string, hits: Hit[]) => [
    action('copy-path', 'Copiar caminho', () => void copy(path, 'Caminho copiado')),
    action('copy-all', 'Copiar tudo', () => void copy(blockText(path, hits), 'Resultados do arquivo copiados')),
    DIVIDER,
    action('dismiss', 'Dispensar', () => dismissFile(path)),
  ];
  // Campo preenchido sem resposta ainda (o debounce espera) mostra "Pesquisando…".
  const status = error || (pending ? 'Pesquisando…' : results ? summary(hitCount, files.length, results.truncated) : form.query ? 'Pesquisando…' : '');

  // Setas sobem e descem entre as linhas; Início e Fim vão à primeira e à última.
  const onResultsKey = (e: ReactKeyboardEvent<HTMLElement>) => {
    const keys = ['ArrowDown', 'ArrowUp', 'Home', 'End'];
    if (!keys.includes(e.key)) return;
    const items = [...e.currentTarget.querySelectorAll<HTMLButtonElement>('[data-result]')];
    const idx = items.indexOf(e.target as HTMLButtonElement);
    if (idx < 0 && e.key !== 'Home' && e.key !== 'End') return;
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 : e.key === 'ArrowDown' ? idx + 1 : idx - 1;
    const target = items[Math.min(Math.max(next, 0), items.length - 1)];
    if (!target) return;
    e.preventDefault();
    target.focus();
  };

  // Posição que o próximo ArrowUp alcança. A consulta atual já está no topo do histórico (entrou ao buscar), então, sem navegação,
  // a primeira subida pula essa e traz a anterior.
  const upFrom = at < 0 && history[0] === form.query ? 1 : at + 1;
  const canClear = !!(form.query || form.replacement || form.include || form.exclude || results);

  // Linha da lista. O recuo é de 8 px por nível e a seta ocupa 30 px (8 + 16 + 6), como o `monaco-tl-twistie`.
  const renderRow = (row: Row) => {
    const indent = { paddingLeft: row.depth * INDENT };
    if (row.kind === 'hit') {
      const h = row.hit;
      const rep = showReplace ? replacementOf(h) : null;
      const before = h.start < 0 ? lcut(h.text, AFTER_MAX) : lcut(h.text.slice(0, h.start), BEFORE_MAX);
      const inside = h.start < 0 ? '' : h.text.slice(h.start, h.end);
      const after = h.start < 0 ? '' : h.text.slice(h.end, h.end + AFTER_MAX);
      const struck = showReplace && h.start >= 0;
      return (
        // Cada ocorrência tem o botão que abre (Enter abre de forma permanente, clique e Espaço abrem como prévia; duplo clique abre permanente) e o de dispensar.
        <ContextMenuTrigger key={row.key} items={() => hitMenu(h)}>
          <div className="group/row relative flex h-[22px] items-center hover:bg-[var(--ant-color-fill-tertiary)] has-[[aria-current=true]]:bg-[var(--ant-color-fill-secondary)]" style={indent} data-row>
            <button
              type="button" data-result title={`${h.path}:${h.line}:${h.col}`} aria-current={isCurrent(h) ? 'true' : undefined}
              aria-label={`Linha ${h.line}: ${h.text.trim()}`}
              onClick={() => { mark(h); onOpen(h.path, h.line, h.col); }}
              onDoubleClick={() => onOpen(h.path, h.line, h.col, { permanent: true })}
              onKeyDown={(e) => {
                if (e.key === 'Delete') {
                  e.preventDefault();
                  dismissHit(h, e.currentTarget.closest('[data-row]'));
                  return;
                }
                if (e.key !== 'Enter') return;
                e.preventDefault();
                mark(h);
                onOpen(h.path, h.line, h.col, { permanent: true });
              }}
              className={`flex h-full min-w-0 flex-1 items-center pl-[30px] pr-2 text-left text-[13px] leading-[22px] outline-none ${FOCUS}`}
            >
              <span className="min-w-0 overflow-hidden text-ellipsis whitespace-pre">
                {before}
                {inside && (
                  <span className={struck ? 'search-hit search-hit-removed' : 'search-hit'}>{inside}</span>
                )}
                {struck && rep && <span className="search-hit-added">{rep}</span>}
                {after}
              </span>
            </button>
            <span className="hidden h-full shrink-0 items-center pr-2 group-hover/row:flex group-focus-within/row:flex">
              <ActionIcon
                size="small" variant="borderless" icon={X} aria-label="Dispensar" title="Ignorar (Delete)"
                onClick={(e) => dismissHit(h, e.currentTarget.closest('[data-row]'))} className={FOCUS}
              />
            </span>
          </div>
        </ContextMenuTrigger>
      );
    }
    const open = !collapsedSet.has(row.key);
    const twistie = <ChevronRight className={`size-4 shrink-0 text-[var(--ant-color-text-secondary)] transition-transform${open ? ' rotate-90' : ''}`} aria-hidden />;
    const count = row.kind === 'dir' ? row.count : row.hits.length;
    const badge = (
      <span className="mr-3 ml-1 shrink-0 rounded-full bg-[var(--ant-color-fill-secondary)] px-[5px] text-[11px] leading-[18px] tabular-nums group-hover/file:hidden group-focus-within/file:hidden">
        {count}
      </span>
    );
    if (row.kind === 'dir') {
      return (
        <div key={row.key} className="group/file relative flex h-[22px] items-center hover:bg-[var(--ant-color-fill-tertiary)]" style={indent}>
          <button
            type="button" data-result aria-expanded={open} title={row.path} onClick={() => toggleRow(row.key)}
            className={`flex h-full min-w-0 flex-1 items-center pl-2 text-left text-[13px] outline-none ${FOCUS}`}
          >
            {twistie}
            <FileIcon path={row.path} dir className="ml-1.5 size-4" />
            <span className="ml-1.5 min-w-0 flex-1 truncate">{row.label}</span>
            {badge}
          </button>
        </div>
      );
    }
    const { name, dir } = splitPath(row.path);
    const draft = hasDraft(row.path);
    return (
      // O botão de dispensar fica ao lado do de abrir, não dentro dele: botão dentro de botão é HTML inválido. Mostra com o mouse ou o foco na linha.
      <ContextMenuTrigger key={row.key} items={() => fileMenu(row.path, row.hits)}>
        <div className="group/file relative flex h-[22px] items-center hover:bg-[var(--ant-color-fill-tertiary)]" style={indent} data-file data-row>
          <button
            type="button" data-result aria-expanded={open} title={row.path} onClick={() => toggleRow(row.key)}
            aria-label={`${plural(row.hits.length, 'resultado', 'resultados')} em ${row.path}`}
            className={`flex h-full min-w-0 flex-1 items-center pl-2 text-left text-[13px] outline-none ${FOCUS}`}
            onKeyDown={(e) => {
              if (e.key !== 'Delete') return;
              e.preventDefault();
              dismissFile(row.path, e.currentTarget.closest('[data-row]'));
            }}
          >
            {twistie}
            <FileIcon path={row.path} className="ml-1.5 size-4" />
            <span className="ml-1.5 flex min-w-0 flex-1 items-baseline">
              <span className={dir && !row.tree ? 'max-w-[70%] shrink-0 truncate' : 'min-w-0 truncate'}>{name}</span>
              {dir && !row.tree && <span className="ml-[0.45em] min-w-0 flex-1 truncate text-[0.9em] text-[var(--ant-color-text-secondary)]">{dir}</span>}
            </span>
            {draft && <Text as="span" type="warning" fontSize={11} className="ml-1 shrink-0">alterado</Text>}
            {badge}
          </button>
          <span className="hidden h-full shrink-0 items-center gap-0.5 pr-2 group-hover/file:flex group-focus-within/file:flex">
            {showReplace && (
              <ActionIcon
                size="small" variant="borderless" icon={ReplaceAll} aria-label={`Substituir tudo em ${row.path}`} title="Substituir Tudo"
                disabled={applying} onClick={() => void replaceIn([row.path])} className={FOCUS}
              />
            )}
            <ActionIcon
              size="small" variant="borderless" icon={X} aria-label="Dispensar" title="Ignorar (Delete)"
              onClick={(e) => dismissFile(row.path, e.currentTarget.closest('[data-row]'))} className={FOCUS}
            />
          </span>
        </div>
      </ContextMenuTrigger>
    );
  };

  const confirmFiles = confirming ? confirming.data.files.filter((f) => files.some((x) => x.path === f.path)) : [];
  const confirmCount = confirmFiles.reduce((n, f) => n + f.count, 0);

  return (
    <Flexbox height="100%" className="min-h-0">
      <Flexbox horizontal align="center" gap={2} height={36} paddingInline={12} flex="none">
        <Text as="h2" ellipsis weight={500} fontSize={12} type="secondary" className="min-w-0 flex-1">Pesquisar</Text>
        <HeaderAction icon={RotateCw} label="Atualizar" disabled={!form.query} spin={pending} onClick={() => now(form)} />
        <HeaderAction icon={ListX} label="Limpar os Resultados da Pesquisa" disabled={!canClear} onClick={clear} />
        <HeaderAction
          icon={view === 'list' ? ListTree : List} label={view === 'list' ? 'Exibir como Árvore' : 'Exibir como Lista'}
          onClick={() => setView((v) => (v === 'list' ? 'tree' : 'list'))}
        />
        <HeaderAction
          icon={someOpen ? ChevronsDownUp : ChevronsUpDown} label={someOpen ? 'Recolher Tudo' : 'Expandir Tudo'}
          disabled={!files.length} onClick={toggleAll}
        />
      </Flexbox>

      {/* Campos: margem 0 12px 0 2px e padding 6px 0, como o `.search-widgets-container`. A seta de 16 px ocupa a altura dos dois campos. */}
      <div ref={fieldBox} className="flex-none pt-1.5 pr-3 pb-1.5 pl-0.5">
        <div className="relative pl-[18px]">
          <button
            type="button" aria-label="Ativar/Desativar Substituição" aria-expanded={replaceOpen} title="Ativar/Desativar Substituição"
            onClick={() => { setReplaceOpen((v) => !v); setReport(null); }}
            className={`absolute top-0 left-0 flex h-full w-4 items-center justify-center rounded-sm text-[var(--ant-color-text-secondary)] outline-none hover:bg-[var(--ant-color-fill-tertiary)] ${FOCUS}`}
          >
            <ChevronRight className={`size-4 transition-transform${replaceOpen ? ' rotate-90' : ''}`} aria-hidden />
          </button>
          <Input
            size="small" value={form.query} placeholder="Pesquisar" aria-label="Pesquisar nos arquivos do projeto" style={INPUT_STYLE}
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
                    key={key} icon={icon} size={INPUT_TOGGLE} variant="borderless" active={form.options[key]}
                    aria-pressed={form.options[key]} aria-label={label} title={label} onClick={() => toggle(key)} className={FOCUS}
                  />
                ))}
              </Flexbox>
            )}
          />
          {replaceOpen && (
            <div className="mt-1.5 flex items-center gap-1">
              <Input
                size="small" value={form.replacement} placeholder="Substituir" aria-label="Substituir por" style={INPUT_STYLE} className="min-w-0 flex-1"
                onChange={(e) => change({ replacement: e.target.value }, 'none')}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && e.ctrlKey && e.altKey) {
                    e.preventDefault();
                    void replaceIn(null);
                  }
                }}
                suffix={(
                  <ActionIcon
                    icon={CaseUpper} size={INPUT_TOGGLE} variant="borderless" active={form.preserveCase}
                    aria-pressed={form.preserveCase} aria-label="Preservar maiúsculas" title="Preservar Maiúsculas e Minúsculas"
                    onClick={() => change({ preserveCase: !form.preserveCase }, 'none')} className={FOCUS}
                  />
                )}
              />
              <ActionIcon
                size="small" variant="borderless" icon={ReplaceAll} aria-label="Substituir Tudo" title="Substituir Tudo (Ctrl+Alt+Enter)"
                disabled={!form.query || !files.length || applying} loading={applying} onClick={() => void replaceIn(null)} className={FOCUS}
              />
            </div>
          )}
        </div>
        {/* O `...` fica à direita, embaixo dos campos, e abre os filtros de caminho (`.query-details`). */}
        <div className="relative ml-[18px] min-h-4">
          <button
            type="button" aria-label="Ativar/Desativar os Detalhes da Pesquisa" aria-expanded={detailsOpen} title="Ativar/Desativar os Detalhes da Pesquisa"
            onClick={() => setDetailsOpen((v) => !v)}
            className={`absolute -right-0.5 top-0 z-10 flex h-4 w-[25px] items-center justify-center rounded-sm text-[var(--ant-color-text-secondary)] outline-none hover:bg-[var(--ant-color-fill-tertiary)] ${FOCUS}`}
          >
            <Ellipsis className="size-4" aria-hidden />
          </button>
          {detailsOpen && (
            <div className="pb-1">
              <h4 className="m-0 truncate pt-1 text-[11px] leading-[18px] font-normal">arquivos a serem incluídos</h4>
              <Input
                size="small" value={form.include} aria-label="Incluir caminhos" placeholder="por exemplo, *.ts, ./src" style={INPUT_STYLE}
                onChange={(e) => change({ include: e.target.value }, 'later')}
                onKeyDown={(e) => { if (e.key === 'Enter') now(form); }}
                suffix={(
                  <ActionIcon
                    icon={BookOpen} size={INPUT_TOGGLE} variant="borderless" active={form.onlyOpen}
                    aria-pressed={form.onlyOpen} aria-label="Pesquisar só nos editores abertos" title="Fazer a pesquisa somente nos Editores Abertos"
                    onClick={() => change({ onlyOpen: !form.onlyOpen }, 'now')} className={FOCUS}
                  />
                )}
              />
              <h4 className="m-0 truncate pt-1 text-[11px] leading-[18px] font-normal">arquivos a serem excluídos</h4>
              <Input
                size="small" value={form.exclude} aria-label="Excluir caminhos" placeholder="por exemplo, *.test.ts, ./dist" style={INPUT_STYLE}
                onChange={(e) => change({ exclude: e.target.value }, 'later')}
                onKeyDown={(e) => { if (e.key === 'Enter') now(form); }}
              />
            </div>
          )}
        </div>
      </div>

      {/* Mensagens (`.messages`): resumo, erros, o que ficou de fora e o resultado da última substituição. */}
      <div className="-mt-[5px] flex-none pr-3 pb-1 pl-[22px] text-[13px] text-[var(--ant-color-text-secondary)] select-text">
        {status && <p role="status" className={error ? 'text-[var(--ant-color-error)]' : ''}>{status}</p>}
        {showReplace && previewError && <p role="alert" className="text-[var(--ant-color-error)]">{previewError}</p>}
        {showReplace && livePreview && livePreview.data.omittedFiles > 0 && (
          <p className="text-[var(--ant-color-warning)]">
            {`${plural(livePreview.data.omittedFiles, 'arquivo com ocorrência ficou', 'arquivos com ocorrência ficaram')} fora da prévia (limite). Não serão alterados.`}
          </p>
        )}
        {report && (
          <>
            {report.files > 0 && (
              <p role="status">{`${plural(report.replaced, 'ocorrência substituída', 'ocorrências substituídas')} em ${plural(report.files, 'arquivo', 'arquivos')} por ${report.replacement ? `“${report.replacement}”` : 'nada (apagadas)'}.`}</p>
            )}
            {report.errors.map((err) => (
              <p key={err.path || err.error} role="alert" className="text-[var(--ant-color-error)]">{err.path ? `${err.path}: ${err.error}` : err.error}</p>
            ))}
            {report.left.length > 0 && (
              <p role="status" className="text-[var(--ant-color-warning)]">{`Ficaram de fora: ${report.left.join('; ')}.`}</p>
            )}
          </>
        )}
        {skipped.length > 0 && (
          <details className="text-[12px]">
            <summary className="cursor-pointer select-none">
              {`${plural(skipped.length + (results?.skippedOmitted ?? 0), 'arquivo ficou', 'arquivos ficaram')} de fora`}
            </summary>
            {skipped.map((s) => (
              <p key={s.path} className="truncate" title={`${s.path}: ${REASON[s.reason]}`}>{`${s.path}: ${REASON[s.reason]}`}</p>
            ))}
            {!!results?.skippedOmitted && <p>{`e mais ${results.skippedOmitted}`}</p>}
          </details>
        )}
      </div>

      <Flexbox flex={1} className="min-h-0">
        <ScrollArea className="h-full" disableContentFit>
          <div ref={resultsBox} role="tree" aria-label="Resultados da pesquisa" onKeyDown={onResultsKey}>
            {rows.map(renderRow)}
          </div>
        </ScrollArea>
      </Flexbox>

      {/* Substituir Tudo: confirmação concreta, com quantas trocas, em quais arquivos e por qual texto. */}
      <Modal
        open={!!confirming}
        title={`Substituir ${plural(confirmCount, 'ocorrência', 'ocorrências')} em ${plural(confirmFiles.length, 'arquivo', 'arquivos')}?`}
        closable={false}
        keyboard
        maskClosable
        onCancel={() => !applying && setConfirming(null)}
        footer={(
          <Flexbox horizontal justify="flex-end" gap={8}>
            <Button onClick={() => setConfirming(null)} disabled={applying}>Cancelar</Button>
            <Button
              type="primary" danger disabled={applying} loading={applying}
              onClick={() => {
                const prev = confirming;
                setConfirming(null);
                if (prev) void replaceIn(confirmFiles.map((f) => f.path), prev);
              }}
            >
              Substituir
            </Button>
          </Flexbox>
        )}
      >
        <Flexbox gap={8}>
          <Text as="p" fontSize={14}>
            {`Por ${confirming?.replacement ? `“${confirming.replacement}”` : 'nada (apagar)'}. As alterações vão para o disco; para voltar atrás, use o Git do projeto. Arquivos com alteração não salva ou com ocorrência dispensada ficam de fora.`}
          </Text>
          <Flexbox gap={2} style={{ maxHeight: 180, overflow: 'auto' }}>
            {confirmFiles.slice(0, CONFIRM_LIST).map((f) => (
              <Text key={f.path} as="span" code fontSize={12} ellipsis title={f.path}>{`${f.path} (${f.count})`}</Text>
            ))}
            {confirmFiles.length > CONFIRM_LIST && <Text as="span" type="secondary" fontSize={12}>{`e mais ${confirmFiles.length - CONFIRM_LIST}`}</Text>}
          </Flexbox>
        </Flexbox>
      </Modal>
    </Flexbox>
  );
}

/** Ação do cabeçalho: 22×22 px, ícone de 16 px, como as do `.composite.title` do Antigravity. */
function HeaderAction({ icon, label, onClick, disabled, spin }: { icon: LucideIcon; label: string; onClick: () => void; disabled?: boolean; spin?: boolean }) {
  return (
    <ActionIcon
      icon={icon} size="small" variant="borderless" aria-label={label} title={label} disabled={disabled} spin={spin}
      onClick={onClick} className={FOCUS}
    />
  );
}
