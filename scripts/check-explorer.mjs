// Check puro do Explorer: a escolha de ícones (src/shared/file-icons.ts), com os casos do Explorer do Antigravity aberto neste projeto e a
// ordem nome > extensão longa > extensão curta > linguagem > padrão, e as decorações do Git (src/explorer/explorer-decorations.ts).
// Roda sem dependências: `node scripts/check-explorer.mjs`.
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { test as check } from 'node:test';
import theme from '../src/assets/symbols/theme.json' with { type: 'json' };
import { decorations } from '../src/explorer/explorer-decorations.ts';
import { iconFile, iconKey } from '../src/shared/file-icons.ts';

check('pastas do print', () => {
  assert.equal(iconKey('.github', true), 'folder-github');
  assert.equal(iconKey('src', true), 'folder-orange-code');
  assert.equal(iconKey('docs', true), 'folder-blue');
  assert.equal(iconKey('scripts', true), 'folder-red-code');
  assert.equal(iconKey('dist', true), 'folder-purple-outline');
  assert.equal(iconKey('node_modules', true), 'folder-green');
  assert.equal(iconKey('src-tauri', true), 'folder');
  assert.equal(iconKey('a/b/SRC', true), 'folder-orange-code');
});

check('arquivos do print', () => {
  assert.equal(iconKey('.gitignore', false), 'git');
  assert.equal(iconKey('index.html', false), 'code-orange');
  assert.equal(iconKey('package.json', false), 'node');
  assert.equal(iconKey('package-lock.json', false), 'node');
  assert.equal(iconKey('README.md', false), 'markdown');
  assert.equal(iconKey('tsconfig.json', false), 'tsconfig');
  assert.equal(iconKey('vite.config.ts', false), 'vite');
});

check('ordem de resolução', () => {
  assert.equal(iconKey('src/types.d.ts', false), 'ts-types');
  assert.equal(iconKey('src/App.tsx', false), 'react-ts');
  assert.equal(iconKey('main.rs', false), 'rust');
  assert.equal(iconKey('.json', false), 'document');
  assert.equal(iconKey('sem-extensao', false), 'document');
  assert.equal(iconKey('LICENSE', false), 'license');
});

check('todo ícone mapeado tem SVG', () => {
  // Toda chave que iconKey pode devolver: os valores dos mapas e os padrões. O caminho tem de bater exato com o arquivo.
  const maps = [theme.fileNames, theme.fileExtensions, theme.langNames, theme.langExtensions, theme.folderNames];
  const keys = new Set([theme.file, theme.folder, ...maps.flatMap(Object.values)]);
  const broken = [...keys].filter((key) => {
    const file = iconFile(key);
    return !file || !existsSync(join(import.meta.dirname, '..', 'src', 'assets', 'symbols', 'icons', file));
  });
  assert.deepEqual(broken, []);
});

const f = (path, staged, unstaged) => ({ path, origPath: null, staged, unstaged });

check('decoração de arquivo: cor e letra da alteração mais forte', () => {
  const d = decorations([f('a.ts', null, 'modified'), f('b.ts', null, 'untracked'), f('c.ts', 'added', 'modified'), f('d.ts', null, 'conflicted')]);
  assert.deepEqual([d.get('a.ts').letter, d.get('a.ts').color], ['M', '#E2C08D']);
  assert.deepEqual([d.get('b.ts').letter, d.get('b.ts').color], ['U', '#73C991']);
  assert.equal(d.get('c.ts').letter, 'M', 'modificado ganha de novo no índice');
  assert.equal(d.get('d.ts').letter, '!');
  assert.equal(d.get('e.ts'), undefined);
});

check('pasta herda a alteração mais forte de dentro, com •', () => {
  const d = decorations([f('src/x/a.ts', null, 'untracked'), f('src/b.ts', null, 'modified'), f('docs/c.md', null, 'deleted')]);
  assert.deepEqual([d.get('src').letter, d.get('src').color], ['•', '#E2C08D']);
  assert.equal(d.get('src/x').color, '#73C991');
  assert.equal(d.get('docs').color, '#c74e39', 'apagado também colore a pasta');
  assert.equal(d.get(''), undefined, 'a raiz não é decorada');
});
