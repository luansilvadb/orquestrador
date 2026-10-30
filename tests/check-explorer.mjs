// Resolução do Material Icon Theme e decorações do Explorer.
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { test as check } from 'node:test';
import theme from '../src/assets/material-icons/theme.json' with { type: 'json' };
import { decorations } from '../src/explorer/explorer-decorations.ts';
import { iconFile, iconKey } from '../src/shared/file-icons.ts';

check('pastas fechadas e abertas', () => {
  assert.equal(iconKey('a/b/SRC', true), theme.folderNames.src);
  assert.equal(iconKey('C:\\a\\SRC', true, true), theme.folderNamesExpanded.src);
  assert.equal(iconKey('desconhecida', true), theme.folder);
  assert.equal(iconKey('desconhecida', true, true), theme.folderExpanded);
});

check('nomes, extensões, caminhos e desconhecidos', () => {
  for (const name of ['package.json', 'vite.config.ts', '.gitignore', 'Dockerfile']) {
    assert.equal(iconKey(`C:\\repo\\${name.toUpperCase()}`, false), theme.fileNames[name.toLowerCase()]);
  }
  for (const [name, extension] of Object.entries({ 'app.tsx': 'tsx', 'main.ts': 'ts', 'index.js': 'js', 'data.json': 'json', 'main.rs': 'rs', 'Program.cs': 'cs', 'script.gd': 'gd', 'scene.tscn': 'tscn', 'resource.tres': 'tres', 'project.godot': 'godot' })) {
    assert.equal(iconKey(`a/b/${name}`, false), theme.fileExtensions[extension], name);
  }
  assert.equal(iconKey('.env', false), theme.fileNames['.env'] ?? theme.file);
  assert.equal(iconKey('sem-extensao', false), theme.file);
  assert.equal(iconKey('.inventado', false), theme.file);
});

check('ordem de resolução', () => {
  assert.equal(iconKey('src/types.d.ts', false), theme.fileExtensions['d.ts']);
  assert.equal(iconKey('a/b/VITE.CONFIG.TS', false), theme.fileNames['vite.config.ts']);
});

check('todo ícone mapeado tem SVG', () => {
  // Toda chave que iconKey pode devolver: os valores dos mapas e os padrões. O caminho tem de bater exato com o arquivo.
  const maps = [theme.fileNames, theme.fileExtensions, theme.langNames, theme.langExtensions, theme.folderNames, theme.folderNamesExpanded];
  const keys = new Set([theme.file, theme.folder, theme.folderExpanded, ...maps.flatMap(Object.values)]);
  const broken = [...keys].filter((key) => {
    const file = iconFile(key);
    return !file || !existsSync(join(import.meta.dirname, '..', 'src', 'assets', 'material-icons', 'icons', file));
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
