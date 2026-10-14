// Regras puras da faixa de abas dos arquivos: alvos dos fechamentos em lote, a aba vizinha, a ordem com as fixadas e os textos copiados.
// Sem React, para o check de comportamento rodar direto no Node.

export type BatchKind = 'others' | 'right' | 'saved' | 'all';
export type CopyKind = 'relative' | 'absolute' | 'navigation';
// Arquivos abertos na área do meio, o ativo (ou nenhum) e os fixados. Fixados vêm primeiro em `paths`.
export type OpenFiles = { paths: string[]; active: string | null; pinned: string[] };

/** Abas que um fechamento em lote alcança. Fixadas nunca entram: só o fechamento explícito (X, Fechar, Ctrl+F4) fecha uma fixada. */
export function batchTargets(
  paths: readonly string[], pinned: readonly string[], dirty: ReadonlySet<string>, kind: BatchKind, from?: string,
): string[] {
  const at = from === undefined ? -1 : paths.indexOf(from);
  return paths.filter((path, index) => {
    if (pinned.includes(path)) return false;
    switch (kind) {
      case 'others': return at >= 0 && path !== from;
      case 'right': return at >= 0 && index > at;
      case 'saved': return !dirty.has(path);
      case 'all': return true;
    }
  });
}

/** Aba ativa que sai: a vizinha da direita entre as que ficam; sem ela, a da esquerda; sem nenhuma, nada. */
export function neighborAfterClose(paths: readonly string[], active: string, closing: ReadonlySet<string>): string | null {
  const at = paths.indexOf(active);
  if (at < 0) return null;
  const right = paths.slice(at + 1).find((p) => !closing.has(p));
  if (right) return right;
  return paths.slice(0, at).reverse().find((p) => !closing.has(p)) ?? null;
}

/** Fixadas primeiro; dentro de cada grupo, a ordem que já estava. */
export function pinnedFirst(paths: readonly string[], pinned: readonly string[]): string[] {
  return [...paths.filter((p) => pinned.includes(p)), ...paths.filter((p) => !pinned.includes(p))];
}

/** Estado sem as abas `closing`, com a ativa passando para a vizinha se ela sai. Não cuida de rascunho: quem chama já decidiu. */
export function dropFrom(state: OpenFiles, closing: ReadonlySet<string>): OpenFiles {
  return {
    paths: state.paths.filter((p) => !closing.has(p)),
    pinned: state.pinned.filter((p) => !closing.has(p)),
    active: state.active && closing.has(state.active) ? neighborAfterClose(state.paths, state.active, closing) : state.active,
  };
}

/** Fixa ou desafixa a aba. As fixadas vão para o começo da faixa, cada grupo na ordem que já tinha. */
export function pinFrom(state: OpenFiles, path: string): OpenFiles {
  if (!state.paths.includes(path)) return state;
  const pinned = state.pinned.includes(path) ? state.pinned.filter((p) => p !== path) : [...state.pinned, path];
  return { ...state, pinned, paths: pinnedFirst(state.paths, pinned) };
}

/**
 * Fechamento em lote, como uma só operação. Rodada por rodada: os alvos saem do estado atual (`read`), e os rascunhos deles
 * que ainda não foram confirmados pedem `confirm` (uma só chamada por rodada, com todos juntos). Cancelar devolve null e nada fecha.
 * Uma aba limpa que fica suja durante a confirmação não some sem pergunta; uma aba que sumiu nesse meio tempo não entra.
 * Devolve os caminhos a fechar (vazio se não há alvo).
 */
export async function settleBatch(
  kind: BatchKind,
  from: string | undefined,
  read: () => { paths: readonly string[]; pinned: readonly string[]; dirty: ReadonlySet<string> },
  confirm: (unsaved: string[]) => Promise<boolean>,
): Promise<string[] | null> {
  const confirmed = new Set<string>();
  for (;;) {
    const { paths, pinned, dirty } = read();
    const targets = batchTargets(paths, pinned, dirty, kind, from);
    if (targets.length === 0) return [];
    const unsaved = targets.filter((p) => dirty.has(p) && !confirmed.has(p));
    if (unsaved.length === 0) return targets;
    if (!(await confirm(unsaved))) return null;
    unsaved.forEach((p) => confirmed.add(p));
  }
}

/**
 * Texto copiado por cada ação. Relativo e absoluto usam o separador do sistema (o do projeto: `\` no Windows, `/` no resto),
 * já que os caminhos guardados no estado usam `/`. A navegação é o caminho absoluto com linha e coluna (1-based) do cursor;
 * sem cursor, devolve null e quem chama não copia nada.
 */
export function pathText(kind: CopyKind, path: string, project: string, cursor?: { line: number; col: number }): string | null {
  const sep = project.includes('\\') ? '\\' : '/';
  // A raiz ('') é o próprio projeto: relativo é "." e absoluto é a pasta sem barra final (nunca texto vazio).
  if (path === '') {
    if (kind === 'relative') return '.';
    // Raiz de unidade (`D:\`) mantém a barra: sem ela, `D:` vira o diretório atual da unidade, não a raiz.
    if (kind === 'absolute') return /^[A-Za-z]:[\\/]+$/.test(project) ? project : project.replace(/[\\/]+$/, '') || project;
    return null;
  }
  const relative = path.split('/').join(sep);
  if (kind === 'relative') return relative;
  const absolute = `${project.replace(/[\\/]+$/, '')}${sep}${relative}`;
  if (kind === 'absolute') return absolute;
  return cursor ? `${absolute}:${cursor.line}:${cursor.col}` : null;
}

// Nomes que o Windows reserva, com ou sem extensão. A mesma lista está em `src-tauri/src/files.rs`.
const RESERVED = new Set(['CON', 'PRN', 'AUX', 'NUL', ...Array.from({ length: 9 }, (_, i) => `COM${i + 1}`), ...Array.from({ length: 9 }, (_, i) => `LPT${i + 1}`)]);

/**
 * Motivo para um nome de arquivo não servir no Windows (ou null se serve). Mostrado antes do envio; o backend confere de novo.
 * Mesmas mensagens de `check_name` em `src-tauri/src/files.rs`.
 */
export function fileNameError(name: string): string | null {
  if (name.trim() === '') return 'Digite um nome para o arquivo.';
  if (name === '.' || name === '..' || new TextEncoder().encode(name).length > 255) return 'Esse nome não é permitido.';
  if (/[\u0000-\u001f<>:"\/\\|?*]/.test(name)) return 'O nome tem caractere que o Windows não aceita: < > : " / \\ | ? *';
  if (name.startsWith(' ') || name.endsWith(' ') || name.endsWith('.')) {
    return 'O nome não pode começar nem terminar com espaço, nem terminar com ponto.';
  }
  if (name.toLowerCase() === '.git') return 'A pasta .git não pode ser alterada aqui.';
  if (RESERVED.has(name.split('.')[0].toUpperCase())) return `“${name}” é um nome reservado pelo Windows.`;
  return null;
}

/** Caminho relativo ("/") de `abs` dentro de `project`, ou null se está fora (ou é o próprio projeto). Sem diferença de caixa nem de barra, como no Windows. */
export function relativeTo(project: string, abs: string): string | null {
  const root = project.replace(/[\\/]+$/, '').replace(/\\/g, '/');
  const full = abs.replace(/\\/g, '/');
  if (full.slice(0, root.length).toLowerCase() !== root.toLowerCase() || full[root.length] !== '/') return null;
  return full.slice(root.length + 1);
}

/** `path` é `base` ou está dentro dele (os dois relativos, com "/"). Sem diferença de caixa, como no Windows; a barra final separa pasta de irmã com prefixo igual (`src` não pega `src2`). */
export function isWithin(path: string, base: string): boolean {
  const p = path.toLowerCase();
  const b = base.toLowerCase();
  return p === b || p.startsWith(`${b}/`);
}

/** Abas abertas depois de um arquivo ser renomeado: o caminho antigo vira o novo na lista, nos fixados e na ativa; a ordem não muda. */
export function renamedFiles(state: OpenFiles, from: string, to: string): OpenFiles {
  if (!state.paths.includes(from)) return state;
  const swap = (path: string) => (path === from ? to : path);
  return { paths: state.paths.map(swap), pinned: state.pinned.map(swap), active: state.active === null ? null : swap(state.active) };
}
