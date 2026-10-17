// Backend simulado, só para desenvolvimento no navegador (nunca entra no build de produção).
// No painel Terminal (Ctrl+`), cada aba é um shell simulado: ecoa o que se digita, com a própria linha em edição. `exit` encerra só aquela aba (código 0);
// o `+` abre outra, e o X fecha a aba sem evento. O programa simulado é sempre `pwsh.exe`.
// No console: `__mock.close()` simula fechar a janela.
// `__mock.remove('src/App.tsx')` apaga um arquivo da árvore do projeto.
// A árvore (`list_dir`) é em memória e muda com as ações do menu e com `__mock.remove`.
// O Git tem estado só no que a mock modela: blocos (preparar, tirar do preparo, descartar), arquivo descartado, commit, histórico, gráfico e
// conflitos (resolver e aceitar remoção). Branches, fetch, pull e push são respostas fixas; as refs são fixas, exceto a `main`, que anda com o commit. Regex da pesquisa e da substituição usa a
// sintaxe do JavaScript (o backend usa a do Rust); o texto de busca e de substituição é o mesmo nos dois.
// Na pesquisa (Ctrl+Shift+F), `search` filtra de verdade, mas só os arquivos de `TEXTS` têm linhas; os demais não casam nada.
// Ao abrir um arquivo, `read_file` devolve o texto de `TEXTS`; `write_file_checked` (a gravação do editor) atualiza esse texto. `.png` vem binário, `dados/exportacao.json` vem grande demais, e os demais começam vazios. `read_media` (aba de imagem e áudio) devolve um PNG 32×32 e um WAV de 2 s em silêncio para `.png` e `.wav`; `__mock.failRead = ['caminho']` faz a leitura falhar, e `__mock.corruptRead = ['caminho']` devolve bytes que o navegador não decodifica.
// Menu do arquivo na árvore: `rename_file` e `delete_files` mudam a árvore e `TEXTS` de verdade (com as mesmas recusas do backend); `reveal_file` e
// `open_with` só conferem que o arquivo existe, porque o Explorer, o diálogo e a Lixeira são do Windows. `start_shell` aceita `cwd`.
// Área vazia da árvore: `create_file` e `create_dir` criam de verdade na raiz (com as mesmas recusas); `reveal_dir` só confere a pasta.
// O clipboard do simulado é uma variável em memória, nunca o do Windows: `copy_files` (copiar ou recortar) e `paste_files_detailed` mexem nela.
// `paste_files_detailed` move ou copia a árvore e `TEXTS` de verdade, e nome repetido vira "nome (2)". `__mock.clipboard()` mostra o que há.
// `rename_file` renomeia pasta também (leva os textos de dentro). `__mock.addFolder('pasta')` cria pasta de teste; `__mock.failOn = ['src/a.ts']`
// faz colar ou mover falhar de I/O nesse item, no meio da operação: o que já saiu segue como feito (só para o check de arquivos e pesquisa).
// Modo do Git (`?git=`): `repo` (padrão, tudo como antes), `no-repo` (pasta sem repositório; `git_init` cria e passa a `repo`), `no-git` (Git ausente)
// e `repo-above` (pasta dentro de outro repositório, raiz `C:\projetos`). Fora de `repo`, `git_status` devolve só o `kind` (e a raiz, em `repo-above`).
// `?subfolder=1` faz `choose_project` devolver `subfolder: true`. Em execução, `__mock.gitMode = 'no-repo'` e `__mock.subfolder = true` trocam isso
// (por exemplo, um `git init` feito pelo terminal, que o painel vê na consulta seguinte).
import { EDITOR_HEAD_LINES } from './mock-fixtures';
import { parseDiffFromFile } from '@pierre/diffs';
import type { Transport } from './backend';
import { fileNameError } from './file-tab-actions';
import { splitPath } from './git-history';
import {
  type Batch, type Choice, type ClipboardSnapshot, type Commit, type CommitDetail, type CommitFile, type Conflict, type Events,
  type GitDiff, type GitFile, type GitKind, type GitRef, type GraphCommit, type HunkRef, type Moved, type Options, type QuickOpen,
  type ReplaceApplied, type ReplacePreview, type ReplaceQuery, type ReplaceRequest, type ReviewDiff, type SearchMatch,
  type SearchResults, type SkipReason,
} from './types';

type Handler = (payload: never) => void;

// Modo do Git e pasta escolhida: vêm da URL na carga do módulo e mudam em tempo de execução por `__mock`.
const GIT_MODES = ['repo', 'no-repo', 'no-git', 'repo-above'] as const;
type GitMode = (typeof GIT_MODES)[number];
const params = new URLSearchParams(window.location.search);
const REPO_ROOT = 'C:\\projetos';

const CLAUDE = ['low', 'medium', 'high', 'xhigh', 'max'], CODEX = ['low', 'medium', 'high', 'xhigh'];
const options: Options = {
  tools: [
    { name: 'claude', efforts: CLAUDE, models: ['fable', 'opus', 'sonnet'].map((name) => ({ name, efforts: CLAUDE }))
      .concat({ name: 'haiku', efforts: ['low', 'medium', 'high'] }) },
    { name: 'codex', efforts: CODEX, models: [{ name: 'gpt-5.5', efforts: CODEX }, { name: 'gpt-5.5-mini', efforts: ['low', 'medium'] }] },
  ],
  last: { orchestrator: { tool: 'codex', model: '', effort: '' }, executor: { tool: 'claude', model: 'opus', effort: 'high' } },
};

const handlers = new Map<string, Handler[]>();
const emit = <K extends keyof Events>(event: K, payload: Events[K]) =>
  handlers.get(event)?.forEach((h) => (h as (p: Events[K]) => void)(payload));

const base64 = (text: string) => new TextEncoder().encode(text).toBase64();
const terms = new Map<string, { seq: number; text: string }>();
function out(id: string, text: string) {
  const term = terms.get(id) ?? { seq: 0, text: '' };
  terms.set(id, term);
  term.text += text;
  emit('term-data', { id, seq: ++term.seq, data: base64(text) });
}

// Árvore simulada do projeto, em memória. Pastas são `Folder`; arquivos são `{ dir: false }`.
type Folder = { dir: true; children: Record<string, Entry> };
type Entry = Folder | { dir: false };
const file = (): Entry => ({ dir: false });
const folder = (children: Record<string, Entry> = {}): Folder => ({ dir: true, children });
const tree = folder({
  '.gitignore': file(),
  'README.md': file(),
  'debug.log': file(),
  'package.json': file(),
  assets: folder({ 'logo.png': file() }),
  dados: folder({ 'exportacao.json': file() }),
  docs: folder({ 'design-system.md': file(), 'rascunho-painel.md': file(), 'apagado.md': file(), 'visual-comparador.md': file() }),
  src: folder({
    'App.tsx': file(), 'editor-workspace.tsx': file(), 'index.css': file(), 'longo.ts': file(), 'main.tsx': file(), 'conflito.ts': file(), 'sem-quebra.ts': file(),
    components: folder({ ui: folder({ 'button.tsx': file(), 'card.tsx': file() }) }),
    lib: folder({ 'utils.ts': file() }),
  }),
  tests: folder(),
  dist: folder({ 'index.js': file() }),
});
// O `.gitignore` simulado: a pasta `dist` e qualquer `.log`. Só vale no modo `repo`, como o `check-ignore` do backend.
const ignoredPath = (path: string) => debug.gitMode === 'repo' && (path === 'dist' || path.startsWith('dist/') || path.endsWith('.log'));

// Lista do Git: só os arquivos alterados, com o lado preparado (`staged`) e o das alterações (`unstaged`).
const entry = (path: string, staged: GitKind | null, unstaged: GitKind | null): GitFile => ({ path, origPath: null, staged, unstaged });
const byPath = (a: GitFile, b: GitFile) => a.path.localeCompare(b.path, 'pt-BR');
const files: GitFile[] = [
  entry('README.md', null, 'modified'),
  entry('docs/visual-comparador.md', null, 'modified'),
  entry('docs/design-system.md', 'modified', null),
  entry('docs/rascunho-painel.md', null, 'untracked'),
  entry('src/App.tsx', 'modified', 'modified'),
  entry('src/editor-workspace.tsx', 'modified', 'modified'),
  entry('src/sem-quebra.ts', null, 'modified'),
  entry('dados/exportacao.json', null, 'modified'),
  entry('assets/logo.png', null, 'modified'),
  // Conflitos de merge: só o lado das alterações (como o Git e o backend), sem nada preparado. Texto em `TEXTS`, lados em `conflicts`.
  entry('src/conflito.ts', null, 'conflicted'),
  entry('docs/apagado.md', null, 'conflicted'),
];
// src/editor-workspace.tsx: o HEAD é o trecho de EDITOR_HEAD_LINES; o preparado muda a linha 62 (remove ganha um parâmetro);
// a pasta muda a linha 28 e a 32 (que ficam no mesmo bloco, a 3 linhas uma da outra) e a linha 96 (outro bloco).
const EDITOR_HEAD = EDITOR_HEAD_LINES.join('\n') + '\n';
const EDITOR_INDEX = EDITOR_HEAD.replace('  remove(paths: string[]): void;', '  remove(paths: string[], keep?: boolean): void;');
const EDITOR_WORKTREE = EDITOR_INDEX
  .replace('useLayoutEffect, useRef, useState,', 'useLayoutEffect, useMemo, useRef, useState,')
  .replace("import { Columns2, Ellipsis, Pin, X } from 'lucide-react';", "import { ChevronDown, Columns2, Ellipsis, Pin, X } from 'lucide-react';")
  .replace('type DiskVersion = { text: string; bom: boolean; readOnly: boolean };', 'type DiskVersion = { text: string; bom: boolean; readOnly: boolean; size: number };');
// Fixture do comparador (src/sem-quebra.ts): 150 linhas, nada preparado. A pasta põe 4 linhas no começo, troca uma linha por duas perto
// do fim e acaba sem quebra de linha: três blocos, com quantidades desiguais de linhas e o aviso do motor no último.
const SEM_QUEBRA_HEAD = Array.from({ length: 150 }, (_, i) => `const v${i} = ${i};`);
const SEM_QUEBRA_WORK = [
  ...SEM_QUEBRA_HEAD.slice(0, 2), 'const novo1 = 1;', 'const novo2 = 2;', 'const novo3 = 3;', 'const novo4 = 4;',
  ...SEM_QUEBRA_HEAD.slice(2, 139), 'const troca1 = 1;', 'const troca2 = 2;', ...SEM_QUEBRA_HEAD.slice(140, 149), 'const v149 = 1490;',
].join('\n');
// Fixture visual do comparador: linhas longas alteradas e dois intervalos com alturas diferentes entre os lados.
const COMPARADOR_HEAD = [
  '# Guia do comparador', '',
  'Este parágrafo descreve o comportamento anterior do comparador lado a lado e tem uma linha comprida para testar a quebra automática dentro da coluna sem criar rolagem horizontal.',
  'A calha mantém os números de linha e os sinais de cada lado.', '',
  '## Trecho com mais linhas no novo',
  'A versão antiga tinha somente esta explicação curta.',
  '', '## Trecho com menos linhas no novo',
  'Esta primeira linha explica em detalhes a configuração anterior do painel Git e como as alterações eram apresentadas ao usuário.',
  'Esta segunda linha continua a explicação anterior com outro texto longo para que a quebra apareça nas duas colunas.',
  'Esta terceira linha fecha o conteúdo antigo que será reduzido na nova versão.', '',
  'O restante do documento permanece igual depois das mudanças.',
];
const COMPARADOR_WORK = [
  '# Guia do comparador', '',
  'Este parágrafo descreve o comportamento atual do comparador lado a lado e tem uma linha comprida que deve quebrar dentro da coluna mantendo a rolagem vertical e os números de linha visíveis.',
  'A calha mantém os números de linha e os sinais de cada lado.', '',
  '## Trecho com mais linhas no novo',
  'A versão nova mostra o primeiro detalhe.',
  'A versão nova mostra o segundo detalhe.',
  'A versão nova mostra o terceiro detalhe.',
  '', '## Trecho com menos linhas no novo',
  'O texto atualizado resume a configuração do painel Git numa única linha mais comprida, que deve quebrar automaticamente dentro da coluna sem afetar a calha.', '',
  'O restante do documento permanece igual depois das mudanças.',
];
// Conteúdo simulado de alguns arquivos da árvore, para a pesquisa. Linhas separadas por "\n".
const TEXTS: Record<string, string> = {
  'README.md': '# Orquestrador\n\nUm app para Windows que põe CLIs de IA para trabalhar em papéis separados.\nO orquestrador lê o projeto e delega; os executores trabalham em paralelo.\n',
  'docs/design-system.md': '# Design system\n\nA interface usa o Lobe UI, com Tailwind CSS v4. Tema escuro fixo.\nMonoespaçado só em terminal e valores técnicos.\n',
  'src/App.tsx': 'import { useState } from \'react\';\n\nexport function App() {\n  const [route, setRoute] = useState(\'arquivos\');\n  return <main>Orquestrador {route}</main>;\n}\n',
  'src/main.tsx': 'import { createRoot } from \'react-dom/client\';\nimport { App } from \'./App\';\n\ncreateRoot(document.getElementById(\'root\')!).render(<App />);\n',
  // Arquivo longo do comparador: o HEAD e o preparado vêm de EDITOR_HEAD_LINES (em GIT_TEXT).
  'src/editor-workspace.tsx': EDITOR_WORKTREE,
  // Arquivo longo para conferir a rolagem: 300 linhas, linha em branco a cada 7 e uma linha bem comprida.
  'src/longo.ts': Array.from({ length: 300 }, (_, i) => (i % 7 === 6 ? '' : i === 150 ? `export const longa = '${'x'.repeat(400)}';` : `export const linha${i} = ${i * 3};`)).join('\n'),
  'src/lib/utils.ts': '// Caminho relativo com "/": a raiz é vazia.\n// 🚀 join também usa "/" no Windows.\nexport const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);\n',
  // Conflitos: o texto da pasta traz os marcadores (ou, no apagado, o lado que sobrou).
  'src/conflito.ts': '<<<<<<< ours\nexport const v = 2;\n=======\nexport const v = 3;\n>>>>>>> theirs\n',
  'docs/apagado.md': '# Rascunho\n\nAjuste.\n',
  'src/sem-quebra.ts': SEM_QUEBRA_WORK,
  'docs/visual-comparador.md': COMPARADOR_WORK.join('\n') + '\n',
};

/** Pasta pelo caminho relativo com "/" ('' é a raiz); arquivo ou caminho inexistente dá undefined. */
function folderAt(path: string): Folder | undefined {
  let node: Entry | undefined = tree;
  for (const name of path.split('/').filter(Boolean)) node = node?.dir ? node.children[name] : undefined;
  return node?.dir ? node : undefined;
}
// Pastas primeiro, depois arquivos, alfabética: a mesma ordem do backend.
const listing = (dir: Folder, path: string) => Object.entries(dir.children)
  .map(([name, entry]) => ({ name, dir: entry.dir, ignored: ignoredPath(path ? `${path}/${name}` : name) }))
  .sort((a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name, 'pt-BR'));
// Arquivo novo no disco: a árvore o mostra.
function addFile(dirPath: string, name: string) {
  const dir = folderAt(dirPath);
  if (dir) dir.children[name] = file();
}
// Apagar pela árvore: o arquivo some do disco.
function removePath(path: string) {
  const parts = path.split('/'), name = parts.pop()!;
  const dir = folderAt(parts.join('/'));
  if (dir) delete dir.children[name];
}
// Pasta do projeto no simulado (a mesma que `choose_project` devolve) e a conversão de caminho relativo para o do Windows e de volta.
const MOCK_ROOT = 'C:\\projetos\\exemplo';
const absolute = (rel: string) => (rel ? `${MOCK_ROOT}\\${rel.split('/').join('\\')}` : MOCK_ROOT);
const relOf = (abs: string) => {
  if (!abs.startsWith(`${MOCK_ROOT}\\`)) throw `Fora do projeto simulado: ${abs}`;
  return abs.slice(MOCK_ROOT.length + 1).split('\\').join('/');
};
// Clipboard simulado: caminhos absolutos, como o do Windows. Começa vazio e só o simulado o altera.
let clip: ClipboardSnapshot = { paths: [], cut: false };
const sameClip = (snap: ClipboardSnapshot) =>
  clip.cut === snap.cut && clip.paths.length === snap.paths.length && clip.paths.every((p, i) => p === snap.paths[i]);

// Chave real de um nome na pasta, sem diferença de caixa (o Windows não diferencia). Undefined se não existe.
const existingKey = (dir: Folder, name: string) => Object.keys(dir.children).find((k) => k.toLowerCase() === name.toLowerCase());
// Criação na raiz ou em outra pasta: as mesmas recusas do backend. Nunca sobrescreve.
function create(dirPath: string, name: string, asFolder: boolean): string {
  const problem = fileNameError(name);
  if (problem) throw problem;
  const dir = folderAt(dirPath);
  if (!dir) throw `Pasta não encontrada: ${dirPath}`;
  if (existingKey(dir, name) !== undefined) throw `Já existe um item chamado “${name}” nesta pasta. Nada foi criado.`;
  dir.children[name] = asFolder ? folder() : file();
  return dirPath ? `${dirPath}/${name}` : name;
}
// Colar como o backend: confere todas as origens antes; nome repetido vira "nome (2)", "nome (3)"...; recorte move a entrada e os textos, cópia duplica.
// `done` recebe cada item assim que sai (como o backend): se um item falha no meio, o que já saiu continua registrado.
function paste(dest: string, snap: ClipboardSnapshot, done: Moved[]) {
  if (snap.paths.length === 0) throw 'Não há arquivo ou pasta copiado para colar.';
  const target = folderAt(dest);
  if (!target) throw `Pasta não encontrada: ${dest}`;
  // Item repetido ou dentro de outro da mesma colagem: recusa antes de mexer, como o backend.
  const rels = snap.paths.map(relOf);
  rels.forEach((r, i) => rels.forEach((o, j) => {
    if (i === j) return;
    const name = splitPath(r).name;
    if (i < j && o.toLowerCase() === r.toLowerCase()) throw `“${name}” está repetido na colagem. Nada foi colado.`;
    if (r.toLowerCase().startsWith(`${o.toLowerCase()}/`)) throw `“${name}” está dentro de “${splitPath(o).name}”, que também está na colagem. Escolha só um dos dois. Nada foi colado.`;
  }));
  const plan = snap.paths.map((abs) => {
    const rel = relOf(abs);
    const { dir, name } = splitPath(rel);
    const entry = folderAt(dir)?.children[name];
    if (!entry) throw `“${name}” não existe mais. Nada foi colado.`;
    if (rel.split('/').some((part) => part.toLowerCase() === '.git')) throw `“${name}” está dentro de uma pasta .git. Nada foi colado.`;
    if (entry.dir && (dest === rel || dest.startsWith(`${rel}/`))) throw `Não é possível colar “${name}” dentro dela mesma.`;
    if (snap.cut && dir === dest) throw `“${name}” já está nesta pasta. Recortar e colar na mesma pasta não faz nada.`;
    return { rel, name, dir, entry };
  });
  const taken = new Set<string>();
  const pick = (name: string, asFolder: boolean) => {
    const cut = name.lastIndexOf('.');
    const stem = asFolder || cut <= 0 ? name : name.slice(0, cut), ext = asFolder || cut <= 0 ? '' : name.slice(cut);
    for (let n = 1; ; n++) {
      const candidate = n === 1 ? name : `${stem} (${n})${ext}`;
      if (!taken.has(candidate.toLowerCase()) && existingKey(target, candidate) === undefined) {
        taken.add(candidate.toLowerCase());
        return candidate;
      }
    }
  };
  for (const { rel, name, dir, entry } of plan) {
    if (debug.failOn.includes(rel)) throw `Falha de I/O simulada ao colar “${name}”.`;
    const newName = pick(name, entry.dir);
    const newRel = dest ? `${dest}/${newName}` : newName;
    if (snap.cut) delete folderAt(dir)!.children[name];
    target.children[newName] = snap.cut ? entry : structuredClone(entry);
    // Cópia deixa os textos originais.
    moveTexts(rel, newRel, !snap.cut);
    done.push({ from: absolute(rel), to: newRel });
  }
  // Recorte colado esvazia o clipboard, mas só se ele ainda for o mesmo que foi colado (como o backend).
  if (snap.cut && sameClip(snap)) clip = { paths: [], cut: false };
}
// Textos da árvore sob `from` (o próprio arquivo ou os de dentro da pasta) seguem para `to`; com `keep`, os originais ficam.
const moveTexts = (from: string, to: string, keep = false) => {
  for (const key of Object.keys(TEXTS)) {
    if (key !== from && !key.startsWith(`${from}/`)) continue;
    TEXTS[to + key.slice(from.length)] = TEXTS[key];
    if (!keep) delete TEXTS[key];
  }
};
// Arquivo que precisa existir para a ação; pasta ou ausente dá a mesma mensagem do backend.
function requireFile(path: string) {
  const { dir, name } = splitPath(path);
  const entry = folderAt(dir)?.children[name];
  if (!entry || entry.dir) throw `Arquivo não encontrado: ${path}`;
  return entry;
}

let closeHandler: (() => Promise<boolean>) | null = null;
// Shells do painel Terminal: só `exit` ou `close_shell` os encerram.
// Cada um guarda a própria linha em edição.
const shells = new Map<string, { line: string; prompt: string }>();
const debug = { close: async () => {
  const closed = closeHandler ? await closeHandler() : true;
  if (closed) document.body.innerHTML = '<p style="padding:24px">Janela fechada (simulado).</p>';
  return closed;
}, remove: (path: string) => removePath(path),
  // Clipboard simulado: o que as cópias e recortes do simulado puseram nele. Nunca toca no clipboard do Windows.
  clipboard: () => structuredClone(clip),
  // Edição externa: outro programa gravou o arquivo. Só troca o texto que o disco simulado devolve (o que `read_file` e `write_file_checked` comparam).
  external: (rel: string, text: string) => { TEXTS[rel] = text; },
  // Texto do disco simulado (o que está gravado, não o rascunho do editor).
  disk: (rel: string) => TEXTS[rel] ?? '',
  // Arquivo novo no disco simulado (fixture de teste): aparece na árvore com o texto dado.
  addFile: (rel: string, text = '') => { const { dir, name } = splitPath(rel); addFile(dir, name); TEXTS[rel] = text; },
  // Pasta nova no disco simulado (fixture de teste): aparece na árvore. Pasta-mãe que não existe é ignorada.
  addFolder: (rel: string) => { const { dir, name } = splitPath(rel); const parent = folderAt(dir); if (parent && !parent.children[name]) parent.children[name] = folder(); },
  // Falha de I/O simulada: colar ou mover para quando chega num destes caminhos de origem (relativos), como um disco que recusa um item.
  failOn: [] as string[],
  // Falha de leitura de mídia (`read_media`) nestes caminhos, como um disco que não entrega o arquivo.
  failRead: [] as string[],
  // Bytes que não são imagem nem áudio nestes caminhos: a leitura chega, mas o navegador recusa a decodificação.
  corruptRead: [] as string[],
  // Latência de I/O por comando (ms, somada à base): a gravação demora e o usuário pode digitar enquanto ela não chega.
  delay: {} as Record<string, number>,
  // Conflito de fixture (só o check de Git): lados `ours/theirs` e o texto da pasta (`null` = arquivo ausente).
  addConflict: (rel: string, sides: Sides, text: string | null) => {
    const { dir, name } = splitPath(rel);
    if (text !== null) { addFile(dir, name); TEXTS[rel] = text; }
    conflicts.set(rel, sides);
    files.push({ path: rel, origPath: null, staged: null, unstaged: 'conflicted' });
  },
  // Modo do Git do simulado (ver o topo do arquivo). Trocar em execução simula um `git init` ou `git` removido por fora.
  gitMode: (GIT_MODES.find((m) => m === params.get('git')) ?? 'repo') as GitMode,
  // `choose_project` devolve a pasta como dentro de um repositório (o projeto aberto é a raiz dele).
  subfolder: params.get('subfolder') === '1',
  // Chamadas de Git que chegaram ao simulado, com argumento, resultado e erro (só para o check conferir o payload).
  calls: [] as { cmd: string; args: unknown; result: unknown; error: string | null }[] };
(window as unknown as { __mock: typeof debug }).__mock = debug;

const SHELL_PROMPT = 'PS C:\\projetos\\exemplo> ';
function shellTyped(id: string, data: string) {
  const shell = shells.get(id)!;
  for (const char of data) {
    if (char === '\r') {
      const command = shell.line.trim();
      shell.line = '';
      out(id, '\r\n');
      if (command === 'exit') {
        shells.delete(id);
        terms.delete(id);
        emit('shell-exit', { id, code: 0 });
        return;
      }
      if (command) out(id, `eco: ${command}\r\n`);
      out(id, shell.prompt);
    } else if (char === '\x7f') {
      if (shell.line) { shell.line = shell.line.slice(0, -1); out(id, '\b \b'); }
    } else if (char >= ' ') {
      shell.line += char;
      out(id, char);
    }
  }
}

// ---------- Git com estado (simulado): blocos, descarte, histórico, commit e conflitos ----------

// Um bloco do diff: a linha `@@` e as linhas do corpo, sem a quebra de linha.
// Texto de cada versão de um arquivo alterado: `head` (o commit), `index` (o preparado) e a pasta (`TEXTS`).
// O diff sai dessas três, como o Git: o arquivo inteiro é o comparador (display) e os blocos são os de 3 linhas de contexto.
const APP_WORKTREE = TEXTS['src/App.tsx'];
const README_HEAD = '# Orquestrador\n\nUm app para Windows que põe CLIs de IA para trabalhar em papéis separados.\n';
const GIT_TEXT: Record<string, { head: string; index: string }> = {
  // Só a pasta mudou: o README ganhou o último parágrafo.
  'README.md': { head: README_HEAD, index: README_HEAD },
  // Preparado: a frase da linha 4 foi encurtada no HEAD; a pasta tem a versão atual.
  'docs/design-system.md': { head: TEXTS['docs/design-system.md'].replace('terminal e valores técnicos.', 'terminal.'), index: TEXTS['docs/design-system.md'] },
  // Preparado: `arquivos` (linha 4). Na pasta, a linha 5 perdeu o `·`.
  'src/App.tsx': {
    head: APP_WORKTREE.replace("useState('arquivos')", "useState('painel')").replace('Orquestrador {route}', 'Orquestrador · {route}'),
    index: APP_WORKTREE.replace('Orquestrador {route}', 'Orquestrador · {route}'),
  },
  'src/editor-workspace.tsx': { head: EDITOR_HEAD, index: EDITOR_INDEX },
  'src/sem-quebra.ts': { head: SEM_QUEBRA_HEAD.join('\n') + '\n', index: SEM_QUEBRA_HEAD.join('\n') + '\n' },
  'docs/visual-comparador.md': { head: COMPARADOR_HEAD.join('\n') + '\n', index: COMPARADOR_HEAD.join('\n') + '\n' },
};
type Hunk = { header: string; body: string[]; oldAt: number; oldLen: number; newAt: number; newLen: number; oldLines: string[]; newLines: string[] };
const textLines = (text: string) => (text === '' ? [] : text.replace(/\n$/, '').split('\n'));
const joinLines = (ls: string[]) => ls.map((l) => l + '\n').join('');
// A marca que o Git põe depois da última linha de um lado sem quebra de linha. O motor a mostra como um aviso no próprio diff.
const NO_EOL = '\\ No newline at end of file';
// Início e tamanho de um trecho no cabeçalho do bloco, como o Git escreve: sem contagem quando é 1; a linha anterior quando é 0.
const span = (at: number, len: number) => (len === 1 ? String(at + 1) : `${len === 0 ? at : at + 1},${len}`);
// Os blocos do motor público (`parseDiffFromFile`, com `context` linhas) em linhas com sinal e nos intervalos de cada lado.
// Os textos vêm sem a quebra final: o motor marca isso nos flags `noEOFCR*` e aqui vira a linha `NO_EOL`.
function hunksOf(path: string, a: string, b: string, context: number): Hunk[] {
  // O motor guarda cada linha com a quebra dela; o patch e os dados do simulado usam a linha sem ela.
  const line = (text: string) => text.replace(/\n$/, '');
  const meta = parseDiffFromFile({ name: path, contents: a }, { name: path, contents: b }, { context });
  return meta.hunks.map((h) => {
    const items: { tag: ' ' | '-' | '+'; text: string }[] = [];
    for (const c of h.hunkContent) {
      if (c.type === 'context') for (let i = 0; i < c.lines; i++) items.push({ tag: ' ', text: line(meta.additionLines[c.additionLineIndex + i]) });
      else {
        for (let i = 0; i < c.deletions; i++) items.push({ tag: '-', text: line(meta.deletionLines[c.deletionLineIndex + i]) });
        for (let i = 0; i < c.additions; i++) items.push({ tag: '+', text: line(meta.additionLines[c.additionLineIndex + i]) });
      }
    }
    // A marca vai depois da última linha de cada lado que não tem quebra. Contexto no fim marca os dois lados na mesma linha, uma vez só.
    const lastOf = (side: '-' | '+') => items.reduce((at, it, k) => (it.tag === ' ' || it.tag === side ? k : at), -1);
    const marks = new Set([h.noEOFCRDeletions ? lastOf('-') : -1, h.noEOFCRAdditions ? lastOf('+') : -1]);
    const body = items.flatMap((it, k) => (marks.has(k) ? [it.tag + it.text, NO_EOL] : [it.tag + it.text]));
    const oldLines = items.filter((it) => it.tag !== '+').map((it) => it.text);
    const newLines = items.filter((it) => it.tag !== '-').map((it) => it.text);
    // Com 0 linhas, o início do motor é a linha anterior (é o ponto de inserção), como no cabeçalho do Git.
    const oldAt = oldLines.length === 0 ? h.deletionStart : h.deletionStart - 1;
    const newAt = newLines.length === 0 ? h.additionStart : h.additionStart - 1;
    return {
      header: `@@ -${span(oldAt, oldLines.length)} +${span(newAt, newLines.length)} @@`, body,
      oldAt, oldLen: oldLines.length, newAt, newLen: newLines.length, oldLines, newLines,
    };
  });
}
// O diff de um lado: o bloco com 3 linhas de contexto (o que o Git mostra e as ações usam) e o arquivo inteiro (`full`). Vazio sem diferença.
function diffOf(path: string, a: string, b: string) {
  if (a === b) return { patch: '', full: '', hunks: [] as Hunk[] };
  const hunks = hunksOf(path, a, b, 3);
  if (hunks.length === 0) return { patch: '', full: '', hunks };
  const file = `diff --git a/${path} b/${path}\n--- a/${path}\n+++ b/${path}\n`;
  const text = (hs: Hunk[]) => hs.map((h) => h.header + '\n' + h.body.map((l) => l + '\n').join('')).join('');
  return { patch: file + text(hunks), full: file + text(hunksOf(path, a, b, 1000000)), hunks };
}
// Lado do diff: o preparado contra o HEAD, ou a pasta contra o preparado. Arquivo sem texto do Git não tem diferença.
const diffSide = (path: string, staged: boolean) => {
  const t = GIT_TEXT[path];
  if (!t) return { patch: '', full: '', hunks: [] as Hunk[] };
  return staged ? diffOf(path, t.head, t.index) : diffOf(path, t.index, TEXTS[path] ?? '');
};
// Troca o trecho [at, at+len) de um texto pelas linhas dadas.
const replaceLines = (text: string, at: number, len: number, put: string[]) => {
  const ls = textLines(text);
  return joinLines([...ls.slice(0, at), ...put, ...ls.slice(at + len)]);
};
// Impressão digital (djb2) de um texto: só serve para dizer se mudou. Mesma função para diff, arquivo e conflito.
const hash = (text: string) => {
  let h = 5381;
  for (let i = 0; i < text.length; i++) h = ((h << 5) + h + text.charCodeAt(i)) | 0;
  return (h >>> 0).toString(16).padStart(8, '0');
};
const isFile = (path: string) => { const { dir, name } = splitPath(path); const e = folderAt(dir)?.children[name]; return !!e && !e.dir; };
const textOf = (path: string) => (isFile(path) ? TEXTS[path] ?? '' : '');
// Revisão de um arquivo na pasta: a do texto, ou `ausente` quando não existe (como no backend).
const revOf = (path: string) => (isFile(path) ? hash(textOf(path)) : 'ausente');

// Recalcula a linha de `files` a partir do texto: sem diferença de um lado, o lado some; sem nada dos dois, a linha sai.
// Só mexe em arquivo com texto do Git; conflito e arquivo novo têm estado próprio.
function syncEntry(path: string) {
  const i = files.findIndex((f) => f.path === path);
  const t = GIT_TEXT[path];
  if (i < 0 || !t) return;
  const staged = t.head !== t.index ? 'modified' : null;
  const unstaged = t.index !== (TEXTS[path] ?? '') ? 'modified' : null;
  if (!staged && !unstaged) files.splice(i, 1);
  else files[i] = { ...files[i], staged, unstaged };
}

// Diff de um lado com a revisão e se o arquivo é não rastreado (`untracked`), como o backend. `display` é o arquivo inteiro.
function reviewDiff(path: string, staged: boolean): ReviewDiff {
  const none = { binary: false, tooLarge: false, patch: '' };
  if (path.endsWith('.png')) return { diff: { ...none, binary: true }, display: { ...none, binary: true }, revision: hash(''), untracked: false };
  if (path === 'dados/exportacao.json') return { diff: { ...none, tooLarge: true }, display: { ...none, tooLarge: true }, revision: hash(''), untracked: false };
  // Arquivo novo: o patch é o arquivo inteiro como arquivo novo. Não rastreado, o Git não separa em blocos; preparado, é um bloco só.
  const entry = files.find((f) => f.path === path);
  if (staged ? entry?.staged === 'added' : entry?.unstaged === 'untracked') {
    const lines = (TEXTS[path] ?? '').split('\n');
    if (lines[lines.length - 1] === '') lines.pop();
    const patch = `diff --git a/${path} b/${path}\nnew file mode 100644\n--- /dev/null\n+++ b/${path}\n@@ -0,0 +1,${lines.length} @@\n` + lines.map((l) => '+' + l + '\n').join('');
    const diff = { binary: false, tooLarge: false, patch };
    return { diff, display: diff, revision: hash(patch), untracked: !staged };
  }
  const d = diffSide(path, staged);
  return {
    diff: { binary: false, tooLarge: false, patch: d.patch }, display: { binary: false, tooLarge: false, patch: d.full },
    revision: hash(d.patch), untracked: false,
  };
}

// Confere a revisão do lado e devolve o bloco. Qualquer diferença recusa e não muda nada.
function takeHunk(ref: HunkRef, staged: boolean): Hunk {
  const d = diffSide(ref.path, staged);
  if (ref.revision !== hash(d.patch)) throw 'O arquivo mudou desde que o diff foi lido. Nada foi feito; atualize a tela.';
  const hunk = d.hunks[ref.index];
  if (!hunk || hunk.header !== ref.header) throw 'O bloco não existe mais. Nada foi feito; atualize a tela.';
  return hunk;
}
// Preparar: o bloco da pasta entra no preparado. Tirar: o bloco do preparado volta ao HEAD. Descartar: a pasta volta ao preparado nesse trecho.
function stageHunk(ref: HunkRef) {
  const h = takeHunk(ref, false);
  GIT_TEXT[ref.path].index = replaceLines(GIT_TEXT[ref.path].index, h.oldAt, h.oldLen, h.newLines);
  syncEntry(ref.path);
}
function unstageHunk(ref: HunkRef) {
  const h = takeHunk(ref, true);
  GIT_TEXT[ref.path].index = replaceLines(GIT_TEXT[ref.path].index, h.newAt, h.newLen, h.oldLines);
  syncEntry(ref.path);
}
function discardHunk(ref: HunkRef) {
  const h = takeHunk(ref, false);
  TEXTS[ref.path] = replaceLines(TEXTS[ref.path], h.newAt, h.newLen, h.oldLines);
  syncEntry(ref.path);
}
// Descarta a alteração da pasta de um arquivo rastreado, se ele ainda está na revisão lida. Não rastreado é recusado.
function discardFile(path: string, revision: string) {
  const entry = files.find((f) => f.path === path);
  if (!entry || entry.unstaged === 'untracked') throw `${path} não está rastreado: o Git não o descarta. Mande-o para a Lixeira pela tela.`;
  if (revision !== hash(diffSide(path, false).patch)) throw 'O arquivo mudou desde que o diff foi lido. Nada foi descartado; atualize a tela.';
  if (GIT_TEXT[path]) TEXTS[path] = GIT_TEXT[path].index;
  syncEntry(path);
}
// `git add`: o que está na pasta vai para o preparado; arquivo novo vira `added`.
function stagePaths(paths: string[]) {
  for (const path of paths.length ? paths : files.map((f) => f.path)) {
    const entry = files.find((f) => f.path === path);
    if (!entry || entry.unstaged === 'conflicted') continue;
    if (entry.unstaged === 'untracked') Object.assign(entry, { staged: 'added', unstaged: null });
    else if (GIT_TEXT[path]) {
      GIT_TEXT[path].index = TEXTS[path];
      syncEntry(path);
    }
  }
}
// `git restore --staged`: o preparado volta para o HEAD; arquivo novo volta a não rastreado.
function unstagePaths(paths: string[]) {
  for (const path of paths.length ? paths : files.map((f) => f.path)) {
    const entry = files.find((f) => f.path === path);
    if (!entry) continue;
    if (entry.staged === 'added') Object.assign(entry, { staged: null, unstaged: 'untracked' });
    else if (GIT_TEXT[path]) {
      GIT_TEXT[path].index = GIT_TEXT[path].head;
      syncEntry(path);
    }
  }
}

// Commits do histórico, do mais novo para o mais antigo. `files` diz o que cada um mudou; `origPath` só na renomeação.
// `parents` são os pais, o primeiro é a linha principal. O commit de merge (4b2e8d0) lista os arquivos contra o primeiro pai.
type Rec = Commit & { parents: string[]; files: CommitFile[] };
// Grafo: a `main` segue a linha de a2c4e6f; `feat/painel-git` (d7e1a9c) sai dela e volta pelo merge 4b2e8d0. HEAD é 9f3a1c2, uma à frente de origin/main.
const commits: Rec[] = [
  { hash: '9f3a1c2b7d4e5f60718293a4b5c6d7e8f9a0b1c2', short: '9f3a1c2', author: 'Luan', date: '2026-10-10T18:42:00-03:00',
    subject: 'Prepara v0.1.7 com notas completas e publicação validada', parents: ['4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11'],
    files: [{ path: 'README.md', origPath: null, kind: 'modified' }, { path: 'docs/releases/v0.1.7.md', origPath: null, kind: 'added' }] },
  { hash: '4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11', short: '4b2e8d0', author: 'Luan', date: '2026-10-08T11:05:00-03:00',
    subject: 'Abre e edita arquivos no meio da tela', parents: ['a2c4e6f8b0d2e4f6a8c0b2d4e6f8a0c2e4b6d8f0', 'd7e1a9c3b5f7081a2c4e6f8a0b2d4f6e8a0c2e4b'],
    files: [
      { path: 'src/App.tsx', origPath: null, kind: 'modified' }, { path: 'src/lib/utils.ts', origPath: 'src/utils.ts', kind: 'renamed' },
      { path: 'src/components/ui/card.tsx', origPath: null, kind: 'modified' }, { path: 'docs/design-system.md', origPath: null, kind: 'modified' },
    ] },
  { hash: 'd7e1a9c3b5f7081a2c4e6f8a0b2d4f6e8a0c2e4b', short: 'd7e1a9c', author: 'Luan', date: '2026-10-05T09:30:00-03:00',
    subject: 'Corrige a ordem das pastas na árvore', parents: ['a2c4e6f8b0d2e4f6a8c0b2d4e6f8a0c2e4b6d8f0'],
    files: [{ path: 'src/components/ui/card.tsx', origPath: null, kind: 'modified' }, { path: 'docs/design-system.md', origPath: null, kind: 'modified' }] },
  { hash: 'a2c4e6f8b0d2e4f6a8c0b2d4e6f8a0c2e4b6d8f0', short: 'a2c4e6f', author: 'Luan', date: '2026-10-01T16:00:00-03:00',
    subject: 'Versão inicial do explorador', parents: [],
    files: [
      { path: 'README.md', origPath: null, kind: 'added' }, { path: 'src/App.tsx', origPath: null, kind: 'added' }, { path: 'src/main.tsx', origPath: null, kind: 'added' },
      { path: 'src/utils.ts', origPath: null, kind: 'added' }, { path: 'src/components/ui/card.tsx', origPath: null, kind: 'added' },
      { path: 'docs/design-system.md', origPath: null, kind: 'added' },
    ] },
];
const plain = (c: Rec): Commit => ({ hash: c.hash, short: c.short, author: c.author, date: c.date, subject: c.subject });
// Refs simuladas: `main` é a branch atual e anda com commitStaged; `origin/main` fica um commit atrás (ahead: 1); a branch lateral e a tag são fixas.
const CURRENT = 'main';
const refs: { name: string; kind: GitRef['kind']; hash: string }[] = [
  { name: 'main', kind: 'branch', hash: '9f3a1c2b7d4e5f60718293a4b5c6d7e8f9a0b1c2' },
  { name: 'feat/painel-git', kind: 'branch', hash: 'd7e1a9c3b5f7081a2c4e6f8a0b2d4f6e8a0c2e4b' },
  { name: 'origin/main', kind: 'remote', hash: '4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11' },
  { name: 'v0.1.7', kind: 'tag', hash: '9f3a1c2b7d4e5f60718293a4b5c6d7e8f9a0b1c2' },
];
const tipOf = (name: string) => refs.find((r) => r.name === name)!.hash;
const headHash = () => tipOf(CURRENT);
// Todos os commits alcançáveis a partir de `from` (inclusive), pelos pais.
function ancestors(from: string[]): Set<string> {
  const seen = new Set<string>(), stack = [...from];
  while (stack.length) {
    const hash = stack.pop()!;
    if (seen.has(hash)) continue;
    seen.add(hash);
    stack.push(...commits.find((c) => c.hash === hash)!.parents);
  }
  return seen;
}
// Commits à frente e atrás do upstream (origin/main) na branch atual.
const divergence = () => {
  const mine = ancestors([headHash()]), theirs = ancestors([tipOf('origin/main')]);
  return { ahead: [...mine].filter((h) => !theirs.has(h)).length, behind: [...theirs].filter((h) => !mine.has(h)).length };
};
// Refs que apontam para `hash`, na ordem do contrato: current, branches, remotes, tags.
const RANK: Record<GitRef['kind'], number> = { branch: 0, remote: 1, tag: 2, head: 3 };
const refsAt = (hash: string): GitRef[] => refs
  .filter((r) => r.hash === hash)
  .map((r) => ({ name: r.name, kind: r.kind, current: r.kind === 'branch' && r.name === CURRENT }))
  .sort((a, b) => Number(b.current) - Number(a.current) || RANK[a.kind] - RANK[b.kind]);
// Paginação como o backend: `limit` entre 1 e 200, `skip` pula os já mostrados.
const page = <T>(list: T[], limit: number, skip: number) => list.slice(skip, skip + Math.min(200, Math.max(1, limit)));
// Gráfico: HEAD e o upstream, mais novo primeiro. `commits` já está nessa ordem (cada pai é mais antigo que o filho).
function graph(limit: number, skip: number): GraphCommit[] {
  const reach = ancestors([headHash(), tipOf('origin/main')]);
  return page(commits.filter((c) => reach.has(c.hash)), limit, skip)
    .map((c) => ({ ...plain(c), parents: c.parents, refs: refsAt(c.hash) }));
}
// Confere o formato do id (hexadecimal, 4 a 64) e acha o commit por prefixo, como o backend.
function commitById(id: string): Rec {
  if (!/^[0-9a-f]{4,64}$/i.test(id)) throw `Commit inválido: ${id}`;
  const found = commits.find((c) => c.hash.startsWith(id.toLowerCase()));
  if (!found) throw `Commit não encontrado: ${id}`;
  return found;
}
// Patch de exemplo de um arquivo de commit: o arquivo criado vem como arquivo novo, como no `diff-tree`; os outros, como alteração.
const commitPatch = (path: string, kind: GitKind) => kind === 'added'
  ? `diff --git a/${path} b/${path}\nnew file mode 100644\n--- /dev/null\n+++ b/${path}\n@@ -0,0 +1 @@\n+depois\n`
  : `diff --git a/${path} b/${path}\n--- a/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-antes\n+depois\n`;
// Log do HEAD (todos os pais, como `git log`), com filtro de caminho opcional.
function history(limit: number, skip: number, path?: string | null): Commit[] {
  const reach = ancestors([headHash()]);
  return page(commits.filter((c) => reach.has(c.hash) && (!path || c.files.some((f) => f.path === path || f.origPath === path))), limit, skip)
    .map(plain);
}
function commitDiff(id: string, path?: string | null): GitDiff {
  const c = commitById(id);
  const patch = c.files.filter((f) => !path || f.path === path || f.origPath === path).map((f) => commitPatch(f.path, f.kind)).join('');
  return { binary: false, tooLarge: false, patch };
}
// Commit com o que está preparado. Recusa mensagem vazia e nada preparado; depois, o preparado sai da lista.
function commitStaged(message: string) {
  if (!message.trim()) throw 'Escreva a mensagem do commit.';
  const staged = files.filter((f) => f.staged !== null);
  if (staged.length === 0) throw 'Nada preparado para o commit.';
  const id = hash(`${message}${Date.now()}`).repeat(5);
  // O pai é o HEAD de antes; a branch atual passa a apontar para o commit novo.
  const parent = headHash();
  commits.unshift({
    hash: id, short: id.slice(0, 7), author: 'Luan', date: new Date().toISOString(), subject: message.split('\n')[0], parents: [parent],
    files: staged.map((f) => ({ path: f.path, origPath: f.origPath, kind: f.staged as GitKind })),
  });
  refs.find((r) => r.name === CURRENT)!.hash = id;
  for (const f of staged) {
    if (GIT_TEXT[f.path]) {
      GIT_TEXT[f.path].head = GIT_TEXT[f.path].index;
      syncEntry(f.path);
    } else if (f.unstaged === null) files.splice(files.indexOf(f), 1);
    else f.staged = null;
  }
}

// Conflitos de merge: os dois lados. `theirs` `null` quer dizer que o outro lado apagou o arquivo.
type Sides = { ours: string | null; theirs: string | null };
const conflicts = new Map<string, Sides>([
  ['src/conflito.ts', { ours: 'export const v = 2;\n', theirs: 'export const v = 3;\n' }],
  ['docs/apagado.md', { ours: '# Rascunho\n\nAjuste.\n', theirs: null }],
]);
const conflictOf = (path: string): Sides => {
  const found = conflicts.get(path);
  if (!found) throw `${path} não está em conflito.`;
  return found;
};
function conflictState(path: string): Conflict {
  const c = conflictOf(path);
  return { ours: c.ours, theirs: c.theirs, worktree: isFile(path) ? textOf(path) : null, binary: false, revision: revOf(path) };
}
// Grava a resolução só se a pasta ainda está na revisão lida. Com `markResolved`, o arquivo sai do conflito e fica preparado.
function writeResolution(path: string, content: string, revision: string, markResolved: boolean) {
  conflictOf(path);
  if (revision !== revOf(path)) throw 'O arquivo mudou desde que foi lido. Nada foi gravado; atualize a tela.';
  if (markResolved && /^(<<<<<<<|=======|>>>>>>>)/m.test(content)) {
    throw 'Ainda há marcadores de conflito no texto (<<<<<<<, =======, >>>>>>>). Remova-os antes de marcar como resolvido.';
  }
  if (!isFile(path)) { const { dir, name } = splitPath(path); addFile(dir, name); }
  TEXTS[path] = content;
  if (markResolved) {
    conflicts.delete(path);
    files[files.findIndex((f) => f.path === path)] = { path, origPath: null, staged: 'modified', unstaged: null };
  }
}
// Aceita a remoção quando um lado apagou o arquivo; o outro lado tem de estar vazio. Vira remoção preparada.
function acceptDeletion(path: string, revision: string) {
  const c = conflictOf(path);
  if (c.ours !== null && c.theirs !== null) throw 'Os dois lados têm conteúdo: resolva o texto em vez de aceitar a remoção.';
  if (revision !== revOf(path)) throw 'O arquivo mudou desde que foi lido. Nada foi removido; atualize a tela.';
  const { dir, name } = splitPath(path);
  delete folderAt(dir)!.children[name];
  delete TEXTS[path];
  conflicts.delete(path);
  files[files.findIndex((f) => f.path === path)] = { path, origPath: null, staged: 'deleted', unstaged: null };
}

// ---------- Pesquisa, abertura rápida e substituição simuladas ----------

// Mesmo limite do backend: até 2000 linhas casadas; mais que isso marca `truncated`. A prévia mostra até 100 arquivos com texto.
const SEARCH_LIMIT = 2000;
const PREVIEW_FILES = 100;
// O padrão da consulta: literal escapa o texto; palavra inteira prende em `\b`. Sem diferença de caixa por padrão.
function compile(q: ReplaceQuery): RegExp {
  if (/[\n\r\0]/.test(q.text)) throw 'A consulta não pode ter quebra de linha.';
  const source = q.regex ? q.text : q.text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  try {
    return new RegExp(q.wholeWord ? `\\b(?:${source})\\b` : source, q.caseSensitive ? '' : 'i');
  } catch {
    throw 'Expressão regular inválida. Confira a sintaxe.';
  }
}
// Posição do primeiro casamento não vazio na linha, ou -1.
function firstHit(re: RegExp, body: string): number {
  for (const m of body.matchAll(new RegExp(re.source, `${re.flags}g`))) if (m[0].length > 0) return m.index ?? -1;
  return -1;
}
// Ajusta a caixa do substituto à ocorrência, como o backend: toda maiúscula, toda minúscula ou só a primeira maiúscula.
function keepCase(matched: string, rep: string): string {
  const letters = [...matched].filter((c) => c.toLowerCase() !== c.toUpperCase());
  if (letters.length === 0) return rep;
  const [first, ...rest] = letters;
  if (letters.every((c) => c === c.toUpperCase())) return rep.toUpperCase();
  if (letters.every((c) => c === c.toLowerCase())) return rep.toLowerCase();
  if (first === first.toUpperCase() && rest.every((c) => c === c.toLowerCase())) {
    const [head = '', ...tail] = [...rep];
    return head.toUpperCase() + tail.join('');
  }
  return rep;
}
// Substitui linha a linha, como o backend, e conta as ocorrências não vazias. O CR do fim fica de fora e volta no lugar.
function replaceIn(text: string, re: RegExp, literal: boolean, rep: string, preserve: boolean): { result: string; count: number } {
  const global = new RegExp(re.source, `${re.flags}g`);
  const single = new RegExp(re.source, re.flags);
  // Substituto de uma ocorrência: literal vai como está; regex expande os grupos, e a caixa pode ser ajustada.
  const sub = (matched: string) => {
    const out = literal ? rep : matched.replace(single, rep);
    return preserve ? keepCase(matched, out) : out;
  };
  let count = 0;
  const result = text.split('\n').map((line) => {
    const cr = line.endsWith('\r') ? '\r' : '';
    const body = cr ? line.slice(0, -1) : line;
    count += [...body.matchAll(global)].filter((m) => m[0].length > 0).length;
    return body.replace(global, sub) + cr;
  }).join('\n');
  return { result, count };
}
// Arquivos que a mock não lê como texto, com o motivo que o backend daria.
const unreadable = (path: string): SkipReason | null => (path.endsWith('.png') ? 'binary' : path === 'dados/exportacao.json' ? 'tooLarge' : null);
// Filtro de caminho: `*` qualquer sequência, `?` um caractere, sem diferença de caixa, em qualquer parte do caminho. Com `./` na frente,
// vale a partir da raiz e até o fim de um nome (`./src` é a pasta src, não src2), como no backend.
const globBody = (term: string) => term.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.');
const globRe = (term: string) => {
  const rooted = term === '.' ? '' : term.startsWith('./') ? term.slice(2).replace(/\/+$/, '') : null;
  if (rooted === null) return new RegExp(globBody(term), 'i');
  return rooted ? new RegExp(`^${globBody(rooted)}(?:/|$)`, 'i') : /^/;
};
function allFiles(dir: Folder = tree, prefix = ''): string[] {
  return Object.entries(dir.children).flatMap(([name, e]) => (e.dir ? allFiles(e, `${prefix}${name}/`) : [`${prefix}${name}`]));
}
// Arquivos da pesquisa: os da árvore que passam nos filtros, em ordem alfabética.
function candidates(include: string[], exclude: string[], only?: string[]): string[] {
  const inc = include.map((t) => t.trim().replace(/\\/g, '/')).filter(Boolean).map(globRe);
  const exc = exclude.map((t) => t.trim().replace(/\\/g, '/')).filter(Boolean).map(globRe);
  return allFiles().filter((p) => (!only || only.includes(p)) && (inc.length === 0 || inc.some((r) => r.test(p))) && !exc.some((r) => r.test(p))).sort();
}

function searchText(q: ReplaceQuery): SearchResults {
  const out: SearchResults = { files: [], truncated: false, skipped: [], skippedOmitted: 0 };
  if (!q.text.trim()) return out;
  const re = compile(q);
  let hits = 0;
  for (const path of candidates(q.include, q.exclude, q.only)) {
    const why = unreadable(path);
    if (why) { out.skipped.push({ path, reason: why }); continue; }
    const matches: SearchMatch[] = [];
    const lines = textOf(path).split('\n');
    for (let i = 0; i < lines.length && !out.truncated; i++) {
      const body = lines[i].replace(/\r$/, '');
      const at = firstHit(re, body);
      if (at < 0) continue;
      if (hits === SEARCH_LIMIT) { out.truncated = true; break; }
      hits++;
      matches.push({ line: i + 1, col: at + 1, text: body.slice(0, 400) });
    }
    if (matches.length) out.files.push({ path, matches });
    if (out.truncated) break;
  }
  return out;
}

// Prévia: não grava. Arquivo com ocorrência entra com texto original e resultado; o que passa do limite só conta.
function replacePreview(q: ReplaceQuery, replacement: string): ReplacePreview {
  const out: ReplacePreview = { files: [], omittedFiles: 0 };
  if (!q.text.trim()) return out;
  if (/[\n\r\0]/.test(replacement)) throw 'A substituição precisa ser de uma linha só.';
  const re = compile(q);
  for (const path of candidates(q.include, q.exclude, q.only)) {
    if (unreadable(path)) continue;
    const { result, count } = replaceIn(textOf(path), re, !q.regex, replacement, !!q.preserveCase);
    if (count === 0) continue;
    if (out.files.length < PREVIEW_FILES) out.files.push({ path, revision: revOf(path), count, original: textOf(path), result });
    else out.omittedFiles++;
  }
  return out;
}

// Aplicação: só os arquivos escolhidos, com a revisão da prévia. Cada um passa por conferência; o erro é por arquivo.
function replaceApply({ query, replacement, files: picks }: ReplaceRequest): ReplaceApplied {
  const out: ReplaceApplied = { applied: [], errors: [] };
  if (!query.text.trim()) throw 'Digite o texto a substituir.';
  if (/[\n\r\0]/.test(replacement)) throw 'A substituição precisa ser de uma linha só.';
  const re = compile(query);
  const listed = new Set(candidates(query.include, query.exclude, query.only));
  for (const pick of picks) {
    try {
      if (!listed.has(pick.path) || unreadable(pick.path)) throw 'Este arquivo não está na pesquisa atual (filtro, ignorado ou apagado). Nada foi gravado.';
      if (pick.revision !== revOf(pick.path)) throw 'O arquivo mudou desde a prévia. Nada foi gravado; faça a prévia de novo.';
      const { result, count } = replaceIn(textOf(pick.path), re, !query.regex, replacement, !!query.preserveCase);
      if (count === 0) throw 'Não há mais o que substituir neste arquivo. Nada foi gravado.';
      TEXTS[pick.path] = result;
      out.applied.push({ path: pick.path, count });
    } catch (e) {
      out.errors.push({ path: pick.path, error: String(e) });
    }
  }
  return out;
}

// Abertura rápida: os caminhos que têm todos os termos. Nome primeiro, depois o mais curto.
function quickOpen(term: string): QuickOpen {
  const words = term.toLowerCase().split(/\s+/).filter(Boolean);
  const hits = allFiles().filter((p) => words.every((w) => p.toLowerCase().includes(w)));
  const nameHit = (p: string) => (words.every((w) => splitPath(p).name.toLowerCase().includes(w)) ? 0 : 1);
  hits.sort((a, b) => nameHit(a) - nameHit(b) || a.length - b.length || a.localeCompare(b));
  return { paths: hits.slice(0, 100), total: hits.length };
}

// ---------- Lotes de arquivos (mover, lixeira, colar) ----------

// Roda `run` e devolve o que foi feito. Como o backend, o primeiro erro para e vai para `error`; `done` fica com o que já foi.
function batchOf<T>(run: (done: T[]) => void): Batch<T> {
  const done: T[] = [];
  try {
    run(done);
    return { done, error: null };
  } catch (e) {
    return { done, error: String(e) };
  }
}
// Seleção: lista não vazia, existente, sem repetido e sem item dentro de outro. Qualquer recusa aborta antes de mexer.
function checkSelection(paths: string[]) {
  if (paths.length === 0) throw 'Nenhum item selecionado.';
  for (const p of paths) if (!existsAt(p)) throw `Item não encontrado: ${p}`;
  const names = paths.map((p) => splitPath(p).name);
  const keys = paths.map((p) => p.toLowerCase());
  keys.forEach((k, i) => keys.forEach((o, j) => {
    if (i === j) return;
    if (i < j && k === o) throw `“${names[i]}” está repetido na seleção.`;
    if (k.startsWith(`${o}/`)) throw `“${names[i]}” está dentro de “${names[j]}”, que também está na seleção. Escolha só um dos dois.`;
  }));
}
const existsAt = (path: string) => { const { dir, name } = splitPath(path); return folderAt(dir)?.children[name] !== undefined; };
// Tira `path` (e o que está dentro, se for pasta) da árvore e dos textos.
function deleteEntry(path: string) {
  const { dir, name } = splitPath(path);
  delete folderAt(dir)!.children[name];
  for (const key of Object.keys(TEXTS)) if (key === path || key.startsWith(`${path}/`)) delete TEXTS[key];
}
function moveItems(paths: string[], dest: string): Batch<Moved> {
  return batchOf<Moved>((done) => {
    checkSelection(paths);
    const target = folderAt(dest);
    if (!target) throw `Pasta não encontrada: ${dest}`;
    const taken = new Set<string>();
    for (const p of paths) {
      const { dir, name } = splitPath(p);
      if (dest === p || dest.startsWith(`${p}/`)) throw `Não é possível mover “${name}” para dentro dela mesma ou de uma pasta dentro dela. Nada foi movido.`;
      if (dir === dest) throw `“${name}” já está nesta pasta. Nada foi movido.`;
      if (taken.has(name.toLowerCase()) || existingKey(target, name) !== undefined) throw `Já existe um item chamado “${name}” na pasta de destino. Nada foi movido.`;
      taken.add(name.toLowerCase());
    }
    for (const p of paths) {
      const { dir, name } = splitPath(p);
      if (debug.failOn.includes(p)) throw `Falha de I/O simulada ao mover “${name}”.`;
      const entry = folderAt(dir)!.children[name];
      delete folderAt(dir)!.children[name];
      target.children[name] = entry;
      const to = dest ? `${dest}/${name}` : name;
      moveTexts(p, to);
      done.push({ from: p, to });
    }
  });
}

// Mídia simulada: `read_media` devolve bytes válidos de um PNG 32×32 (`.png`) e de um WAV de 2 s em silêncio (`.wav`), para arquivos
// que estão na árvore. Outras extensões e caminhos fora da árvore recusam como o backend.
const MOCK_PNG = 'iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAGxElEQVR4nBXWkdvGIBSH4XAYDsNhGIZhOAyH4TAchsMwDIfhg8PhcDgcft/bH9B1rl+ncx8hBINACkaBEkwCLTACK3ACL5gFQbAIomAVJMEmyIJdUARV0ASHoAsQnIJLcAsewSv4BEIMDANyYBxQA9OAHjADdsAN+IG5IAwsA3FgHUgD20Ae2AfKQB1oA8dAH2DgHLgG7oFn4B34BoSQDBIpGSVKMkm0xEisxEm8ZJYEySKJklWSJJskS3ZJkVRJkxySLkFySi7JLXkkr+STCDEyjMiRcUSNTCN6xIzYETfiR+aRMLKMxJF1JI1sI3lkHykjdaSNHCN9hJFz5Bq5R56Rd+QbEUIxKKRiVCjFpNAKo7AKp/CKWREUiyIqVkVSbIqs2BVFURVNcSi6AsWpuBS34lG8ik8hxMQwISfGCTUxTegJM2En3ISfmCfCxDIRJ9aJNLFN5Il9okzUiTZxTPQJJs6Ja+KeeCbeiW9CCM2gkZpRozSTRmuMxmqcxmtmTdAsmqhZNUmzabJm1xRN1TTNoekaNKfm0tyaR/NqPo0QhsEgDaNBGSaDNhiDNTiDN8yGYFgM0bAakmEzZMNuKIZqaIbD0A0YTsNluA2P4TV8BiEsg0VaRouyTBZtMRZrcRZvmS3BsliiZbUky2bJlt1SLNXSLIelW7CclstyWx7La/ksQjgGh3SMDuWYHNphHNbhHN4xO4JjcUTH6kiOzZEdu6M4qqM5Dkd34Dgdl+N2PI7X8TmE8Awe6Rk9yjN5tMd4rMd5vGf2BM/iiZ7VkzybJ3t2T/FUT/Mcnu7Bc3ouz+15PK/n8wgxM8zImXFGzUwzesbM2Bk342fmmTCzzMSZdSbNbDN5Zp8pM3WmzRwzfYaZc+aauWeemXfmmxEiMARkYAyowBTQAROwARfwgTkQAksgBtZACmyBHNgDJVADLXAEeoDAGbgCd+AJvIEvIMTCsCAXxgW1MC3oBbNgF9yCX5gXwsKyEBfWhbSwLeSFfaEs1IW2cCz0BRbOhWvhXngW3oVvQYjIEJGRMaIiU0RHTMRGXMRH5kiILJEYWSMpskVyZI+USI20yBHpESJn5IrckSfyRr6IECvDilwZV9TKtKJXzIpdcSt+ZV4JK8tKXFlX0sq2klf2lbJSV9rKsdJXWDlXrpV75Vl5V74VIRJDQibGhEpMCZ0wCZtwCZ+YEyGxJGJiTaTElsiJPVESNdESR6InSJyJK3EnnsSb+NK/BxvDhtwYN9TGtKE3zIbdcBt+Y94IG8tG3Fg30sa2kTf2jbJRN9rGsdE32Dg3ro1749l4N77t34PMkJGZMaMyU0ZnTMZmXMZn5kzILJmYWTMps2VyZs+UTM20zJHpGTJn5srcmSfzZr7878HOsCN3xh21M+3oHbNjd9yO35l3ws6yE3fWnbSz7eSdfafs1J22c+z0HXbOnWvn3nl23p1v//egMBRkYSyowlTQBVOwBVfwhbkQCkshFtZCKmyFXNgLpVALrXAUeoHCWbgKd+EpvIWv/HtQGSqyMlZUZaroiqnYiqv4ylwJlaUSK2slVbZKruyVUqmVVjkqvULlrFyVu/JU3spX/z1oDA3ZGBuqMTV0wzRswzV8Y26ExtKIjbWRGlsjN/ZGadRGaxyN3qBxNq7G3Xgab+Nr/x4cDAfyYDxRB9OBPjAH9sAd+IP5IBwsB/FgPUgH20E+2A/KQT1oB8dBP+DgPLgO7oPn4D34jn8POkNHdsaO6kwd3TEd23Ed35k7obN0YmftpM7WyZ29Uzq10zpHp3fonJ2rc3eeztv5+r8Hv3UG+VsJUD9W0T+asL/xjv+NSMJvzBB/X5X0a3fyr2Uov9hpv9Lp/M4JF9zwwAsf/x6cDCfyZDxRJ9OJPjEn9sSd+JP5JJwsJ/FkPUkn20k+2U/KST1pJ8dJP3/XnyfXyX3ynLwn3/nvwcVwIS/GC3UxXegLc2Ev3IW/mC/CxXIRL9aLdLFd5Iv9olzUi3ZxXPTrV/x5cV3cF8/Fe/Fd/x7cDDfyZrxRN9ONvjE39sbd+Jv5JtwsN/FmvUk3202+2W/KTb1pN8dNv3/RnDfXzX3z3Lw33/3vwcPwIB/GB/UwPegH82Af3IN/mB/Cw/IQH9aH9LA95If9oTzUh/ZwPPTnF/z5cD3cD8/D+/A9/x68DC/yZXxRL9OLfjEv9sW9+Jf5JbwsL/FlfUkv20t+2V/KS31pL8dLf3/Per5cL/fL8/K+fO+/Bx/Dh/wYP9TH9KE/zIf9cB/+Y/4IH8tH/Fg/0sf2kT/2j/JRP9rH8dG/X9OcH9fH/fF8vB/fxx96SQBqX6h8bQAAAABJRU5ErkJggg==';
const mockBytes = (path: string): ArrayBuffer => {
  if (debug.failRead.includes(path)) throw `Falha de leitura simulada em “${path}”.`;
  if (debug.corruptRead.includes(path)) return Uint8Array.of(0, 1, 2, 3).buffer;
  const { name } = splitPath(path);
  const dot = name.lastIndexOf('.');
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
  requireFile(path);
  if (ext === 'png') return Uint8Array.fromBase64(MOCK_PNG).buffer;
  if (ext === 'wav') return wavSilence(2);
  throw `Tipo de arquivo não aberto como mídia: ${path}`;
};
// WAV PCM 8 bits mono a 8 kHz, em silêncio (128 é o zero do formato): cabeçalho de 44 bytes.
function wavSilence(seconds: number): ArrayBuffer {
  const rate = 8000, n = seconds * rate;
  const buf = new ArrayBuffer(44 + n), view = new DataView(buf);
  const ascii = (at: number, text: string) => [...text].forEach((c, i) => view.setUint8(at + i, c.charCodeAt(0)));
  ascii(0, 'RIFF'); view.setUint32(4, 36 + n, true); ascii(8, 'WAVE'); ascii(12, 'fmt ');
  view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
  view.setUint32(24, rate, true); view.setUint32(28, rate, true); view.setUint16(32, 1, true); view.setUint16(34, 8, true);
  ascii(36, 'data'); view.setUint32(40, n, true);
  new Uint8Array(buf, 44).fill(128);
  return buf;
}

const commands: Record<string, (args: any) => unknown> = {
  read_media: ({ path }: { path: string }) => mockBytes(path),
  update_status: () => ({ phase: 'ready' }),
  retry_update: () => ({ phase: 'ready' }),
  choose_project: () => ({ project: 'C:\\projetos\\exemplo', subfolder: debug.subfolder }),
  get_options: () => structuredClone(options),
  start_orchestrator: ({ choice }: { choice: Choice }) => {
    options.last.orchestrator = choice;
  },
  set_executor_choice: ({ choice }: { choice: Choice }) => { options.last.executor = choice; },
  start_shell: ({ id, cwd }: { id: string; cwd?: string }) => {
    // Pasta do projeto (relativa); sem ela, ou com '', é a raiz, como no backend. Pasta que não existe é recusada antes de abrir.
    if (cwd && !folderAt(cwd)) throw `Pasta não encontrada: ${cwd}`;
    const prompt = cwd ? `PS C:\\projetos\\exemplo\\${cwd.split('/').join('\\')}> ` : SHELL_PROMPT;
    // Começa do zero: o terminal antigo (se houver) some, como no backend.
    terms.delete(id);
    shells.set(id, { line: '', prompt });
    out(id, prompt);
    return 'pwsh.exe';
  },
  // Arquivo ou pasta: a pasta leva os textos de dentro (o backend renomeia a pasta inteira).
  rename_file: ({ path, name }: { path: string; name: string }) => {
    const { dir, name: from } = splitPath(path);
    if (!existsAt(path)) throw `Item não encontrado: ${path}`;
    const problem = fileNameError(name);
    if (problem) throw problem;
    if (name === from) return path;
    const parent = folderAt(dir)!;
    if (parent.children[name] !== undefined) throw `Já existe um item chamado “${name}” nesta pasta.`;
    parent.children[name] = parent.children[from];
    delete parent.children[from];
    const next = dir ? `${dir}/${name}` : name;
    moveTexts(path, next);
    return next;
  },
  reveal_file: ({ path }: { path: string }) => void requireFile(path),
  open_with: ({ path }: { path: string }) => void requireFile(path),
  // Área vazia da raiz: cria de verdade na árvore (e a lista relê). Pasta que não existe é recusada antes.
  create_file: ({ dir, name }: { dir: string; name: string }) => create(dir, name, false),
  create_dir: ({ dir, name }: { dir: string; name: string }) => create(dir, name, true),
  reveal_dir: ({ path }: { path: string }) => {
    if (!folderAt(path)) throw `Pasta não encontrada: ${path}`;
  },
  clipboard_snapshot: () => structuredClone(clip),
  // Colagem com o detalhe: o que foi colado (`from` absoluto, `to` relativo) e o erro do que parou. Recusas antes de mexer não deixam nada feito;
  // uma falha de I/O no meio deixa feito o que já saiu, como o backend.
  paste_files_detailed: ({ dest, snap }: { dest: string; snap: ClipboardSnapshot }): Batch<Moved> =>
    batchOf<Moved>((done) => paste(dest, snap, done)),
  write_file_checked: ({ path, text, base, bom }: { path: string; text: string; base: string; bom: boolean }) => {
    requireFile(path);
    // Como o backend: o BOM do disco tem de bater com `bom`, e o texto cru com `base`. Nada é normalizado.
    if (bom !== false || textOf(path) !== base) throw 'O arquivo foi alterado por outro programa desde que foi aberto. Nada foi gravado.';
    TEXTS[path] = text;
  },
  delete_files: ({ paths }: { paths: string[] }): Batch<string> => batchOf<string>((done) => {
    checkSelection(paths);
    for (const p of paths) {
      deleteEntry(p);
      done.push(p);
    }
  }),
  move_files: ({ paths, dest }: { paths: string[]; dest: string }): Batch<Moved> => moveItems(paths, dest),
  copy_files: ({ paths, cut }: { paths: string[]; cut: boolean }) => {
    checkSelection(paths);
    clip = { paths: paths.map(absolute), cut };
  },
  quick_open: ({ term }: { term: string }): QuickOpen => quickOpen(term),
  replace_preview: ({ query, replacement }: { query: ReplaceQuery; replacement: string }): ReplacePreview => replacePreview(query, replacement),
  replace_apply: ({ request }: { request: ReplaceRequest }): ReplaceApplied => replaceApply(request),
  // Mata o shell sem evento de saída: o terminal some e nenhum `shell-exit` chega.
  close_shell: ({ id }: { id: string }) => {
    shells.delete(id);
    terms.delete(id);
  },
  // O simulado não roda CLI: todo terminal é shell comum.
  term_cli: () => null,
  term_write: ({ id, data }: { id: string; data: string }) => {
    // Marcas de colagem entre aspas (bracketed paste) não são texto: o simulado só põe o conteúdo no rascunho.
    const text = data.replace(/\x1b\[20[01]~/g, '');
    if (shells.has(id)) shellTyped(id, text);
    // O orquestrador simulado só mostra o que chega (a colagem, por exemplo), com as quebras de linha do texto (LF no bracketed paste, CR no paste do xterm).
    else if (id === 'orchestrator') out(id, text.replace(/\r\n?|\n/g, '\r\n'));
  },
  list_dir: ({ path }: { path: string }) => {
    const dir = folderAt(path);
    if (!dir) throw `Pasta não encontrada: ${path}`;
    return listing(dir, path);
  },
  read_file: ({ path }: { path: string }) => {
    requireFile(path);
    if (path.endsWith('.png')) return { binary: true, tooLarge: false, readOnly: false, bom: false, text: '' };
    if (path === 'dados/exportacao.json') return { binary: false, tooLarge: true, readOnly: false, bom: false, text: '' };
    return { binary: false, tooLarge: false, readOnly: false, bom: false, text: TEXTS[path] ?? '' };
  },
  git_status: () => {
    Object.keys(GIT_TEXT).forEach(syncEntry);
    if (debug.gitMode === 'repo') return { kind: 'repo', status: { branch: CURRENT, ...divergence(), head: headHash(), files: [...files].sort(byPath) } };
    if (debug.gitMode === 'repo-above') return { kind: 'repo-above', root: REPO_ROOT };
    return { kind: debug.gitMode };
  },
  // Mesmas recusas do backend (`git::init`): só `no-repo` cria o repositório; nos outros, a mensagem de cada modo.
  git_init: () => {
    if (debug.gitMode === 'no-repo') { debug.gitMode = 'repo'; return null; }
    if (debug.gitMode === 'repo') throw 'A pasta já é um repositório Git.';
    if (debug.gitMode === 'no-git') throw 'O Git não foi encontrado neste computador.';
    throw `A pasta está dentro do repositório ${REPO_ROOT}; não dá para criar outro aqui.`;
  },
  git_stage: ({ paths }: { paths: string[] }) => { stagePaths(paths); return null; },
  git_unstage: ({ paths }: { paths: string[] }) => { unstagePaths(paths); return null; },
  git_commit: ({ message }: { message: string }) => { commitStaged(message); return null; },
  git_fetch: () => 'From github.com:luansilvadb/orquestrador\n   1a2b3c4..5d6e7f8  main -> origin/main\n',
  git_pull: () => 'Already up to date.\n',
  git_push: () => 'Enviando 1 commit(s) para origin/main\n   1a2b3c4..5d6e7f8  main -> main\n',
  git_branches: () => ({ current: 'main', local: ['main', 'feat/painel-git'] }),
  git_switch: () => null,
  git_create_branch: () => null,
  git_review_diff: ({ path, staged }: { path: string; staged: boolean }): ReviewDiff => reviewDiff(path, staged),
  git_stage_hunk: ({ hunk }: { hunk: HunkRef }) => { stageHunk(hunk); return null; },
  git_unstage_hunk: ({ hunk }: { hunk: HunkRef }) => { unstageHunk(hunk); return null; },
  git_discard_hunk: ({ hunk }: { hunk: HunkRef }) => { discardHunk(hunk); return null; },
  git_discard_file: ({ path, revision }: { path: string; revision: string }) => { discardFile(path, revision); return null; },
  git_history: ({ limit, skip, path }: { limit: number; skip: number; path: string | null }): Commit[] => history(limit, skip, path),
  git_graph: ({ limit, skip }: { limit: number; skip: number }): GraphCommit[] => graph(limit, skip),
  git_commit_detail: ({ id }: { id: string }): CommitDetail => {
    const c = commitById(id);
    return { commit: plain(c), files: c.files };
  },
  git_commit_diff: ({ id, path }: { id: string; path: string | null }): GitDiff => commitDiff(id, path),
  git_conflict: ({ path }: { path: string }): Conflict => conflictState(path),
  git_write_resolution: ({ path, content, revision, markResolved }: { path: string; content: string; revision: string; markResolved: boolean }) => {
    writeResolution(path, content, revision, markResolved);
    return null;
  },
  git_accept_deletion: ({ path, revision }: { path: string; revision: string }) => { acceptDeletion(path, revision); return null; },
  search: ({ query }: { query: ReplaceQuery }): SearchResults => searchText(query),
  term_resize: () => undefined,
  term_snapshot: ({ id }: { id: string }) => {
    const term = terms.get(id);
    return { seq: term?.seq ?? 0, data: base64(term?.text ?? '') };
  },
  stop_executor: () => {},
  has_running: () => false,
};

export const mock: Transport = {
  // O retrato é tirado aos 100 ms e entregue aos 200 ms: chegam eventos já contidos nele (a descartar)
  // e eventos posteriores a ele (a guardar) enquanto a resposta está a caminho.
  invoke: (cmd, args) => new Promise((resolve, reject) => setTimeout(() => {
    const log = cmd.startsWith('git_') ? { cmd, args: structuredClone(args), result: null as unknown, error: null as string | null } : null;
    if (log) debug.calls.push(log);
    try {
      const result = commands[cmd](args);
      if (log) log.result = structuredClone(result ?? null);
      if (cmd === 'term_snapshot') setTimeout(() => resolve(result), 100);
      else resolve(result);
    } catch (e) {
      if (log) log.error = String(e);
      reject(e);
    }
  }, (cmd === 'term_snapshot' ? 100 : 15) + (debug.delay[cmd] ?? 0))),
  listen: async (event, handler) => void handlers.set(event, [...(handlers.get(event) ?? []), handler as Handler]),
  onCloseRequested: async (mayClose) => { closeHandler = mayClose; },
  // Janela simulada: minimizar e maximizar não fazem nada; fechar segue o mesmo caminho de `__mock.close()`.
  minimize: async () => {},
  toggleMaximize: async () => {},
  close: async () => void (await debug.close()),
  isMaximized: async () => false,
  setTitle: async (title) => { document.title = title; },
  onResized: async (h) => {
    addEventListener('resize', h);
    return () => removeEventListener('resize', h);
  },
};
