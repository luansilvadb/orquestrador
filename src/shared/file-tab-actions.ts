// Regras puras de caminho e nome do app: texto copiado, caminho relativo ao projeto, pasta que contém e nomes que o Windows recusa.
// Sem React, para o check de comportamento rodar direto no Node.

export type CopyKind = 'relative' | 'absolute' | 'navigation';

/** Caminho absoluto: unidade (`C:\` ou `C:/`), UNC (`\\`) ou raiz (`/`). */
export const isAbsolutePath = (path: string) => /^(?:[a-z]:[\\/]|\\\\|\/)/i.test(path);

/**
 * Texto copiado por cada ação. Relativo e absoluto usam o separador do sistema (o do projeto: `\` no Windows, `/` no resto),
 * já que os caminhos guardados no estado usam `/`. A navegação é o caminho absoluto com linha e coluna (1-based) do cursor;
 * sem cursor, devolve null e quem chama não copia nada.
 */
export function pathText(kind: CopyKind, path: string, project: string, cursor?: { line: number; col: number }): string | null {
  const absolutePath = isAbsolutePath(path);
  const sep = project.includes('\\') || (absolutePath && path.includes('\\')) ? '\\' : '/';
  if (absolutePath) {
    const absolute = path.replace(/[\\/]/g, sep);
    const root = project.replace(/[\\/]+$/, '');
    const comparable = absolute.replace(/[\\/]+/g, '/').toLowerCase();
    const rootComparable = root.replace(/[\\/]+/g, '/').toLowerCase();
    const relative = root && comparable.startsWith(`${rootComparable}/`) ? absolute.slice(root.length + 1) : null;
    if (kind === 'relative') return relative?.replace(/[\\/]/g, sep) ?? null;
    return kind === 'absolute' ? absolute : cursor ? `${absolute}:${cursor.line}:${cursor.col}` : null;
  }
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

// Nomes que o Windows reserva, com ou sem extensão. A mesma lista está em `src-tauri/src/ide/files.rs`.
const RESERVED = new Set(['CON', 'PRN', 'AUX', 'NUL', ...Array.from({ length: 9 }, (_, i) => `COM${i + 1}`), ...Array.from({ length: 9 }, (_, i) => `LPT${i + 1}`)]);

/**
 * Motivo para um nome de arquivo não servir no Windows (ou null se serve). Mostrado antes do envio; o backend confere de novo.
 * Mesmas mensagens de `check_name` em `src-tauri/src/ide/files.rs`.
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

/** Caminho relativo ("/") de `abs` dentro de `project`, ou null se está fora (ou é o próprio projeto, ou não há projeto). Sem diferença de caixa nem de barra, como no Windows. */
export function relativeTo(project: string, abs: string): string | null {
  if (!project) return null;
  const root = project.replace(/[\\/]+$/, '').replace(/\\/g, '/');
  const full = abs.replace(/\\/g, '/');
  if (full.slice(0, root.length).toLowerCase() !== root.toLowerCase() || full[root.length] !== '/') return null;
  return full.slice(root.length + 1);
}
