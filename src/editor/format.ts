import type { CursorOptions, Options, Plugin } from 'prettier';
import type { FormatterInfo } from '../backend/types';

type Parser = { name: string; plugins: string[] };
type Plugins = Record<string, Plugin>;
type PrettierModules = { api: typeof import('prettier/standalone'); plugins: Plugins };

let prettierModules: Promise<PrettierModules> | null = null;

function loadPrettier(): Promise<PrettierModules> {
  return prettierModules ??= Promise.all([
    import('prettier/standalone'),
    import('prettier/plugins/babel'),
    import('prettier/plugins/estree'),
    import('prettier/plugins/typescript'),
    import('prettier/plugins/postcss'),
    import('prettier/plugins/html'),
    import('prettier/plugins/markdown'),
    import('prettier/plugins/yaml'),
  ]).then(([api, babel, estree, typescript, postcss, html, markdown, yaml]) => ({
    api,
    plugins: { babel, estree, typescript, postcss, html, markdown, yaml } as unknown as Plugins,
  }));
}

function parserFor(path: string): Parser {
  const name = path.replace(/\\/g, '/').split('/').pop()?.toLowerCase() ?? '';
  const ext = name.slice(name.lastIndexOf('.') + 1);
  if (['js', 'jsx', 'mjs', 'cjs', 'json', 'jsonc', 'json5'].includes(ext)) return { name: ext.startsWith('json') ? ext : 'babel', plugins: ['babel', 'estree'] };
  if (['ts', 'tsx', 'mts', 'cts'].includes(ext)) return { name: 'typescript', plugins: ['typescript', 'estree'] };
  if (['css', 'scss', 'less'].includes(ext)) return { name: ext, plugins: ['postcss'] };
  if (['html', 'htm', 'vue'].includes(ext)) return { name: ext === 'vue' ? 'vue' : 'html', plugins: ['html'] };
  if (['md', 'markdown'].includes(ext)) return { name: 'markdown', plugins: ['markdown'] };
  return { name: 'yaml', plugins: ['yaml'] };
}

async function configOf(info: Extract<FormatterInfo, { kind: 'prettier' }>): Promise<Record<string, unknown>> {
  if (!info.config) return {};
  let value: unknown;
  if (info.configKind === 'json') value = JSON.parse(info.config);
  else if (info.configKind === 'yaml') value = (await import('yaml')).parse(info.config);
  else {
    try {
      value = JSON.parse(info.config);
    } catch {
      value = (await import('yaml')).parse(info.config);
    }
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('A configuração do Prettier precisa ser um objeto.');
  return value as Record<string, unknown>;
}

export async function formatPrettier(
  source: string,
  path: string,
  info: Extract<FormatterInfo, { kind: 'prettier' }>,
  cursorOffset: number,
): Promise<{ text: string; cursorOffset: number; endOfLine?: string }> {
  const [{ api, plugins }, config] = await Promise.all([loadPrettier(), configOf(info)]);
  const parser = parserFor(path);
  const options = {
    ...info.options,
    ...config,
    parser: parser.name,
    filepath: path,
    plugins: parser.plugins.map((name) => plugins[name]),
    cursorOffset,
  } as CursorOptions & Options;
  const result = await api.formatWithCursor(source, options);
  return { text: result.formatted, cursorOffset: result.cursorOffset, endOfLine: typeof options.endOfLine === 'string' ? options.endOfLine : undefined };
}
