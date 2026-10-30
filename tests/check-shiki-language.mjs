import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createServer } from 'vite';

test('identifica arquivos por metadados Linguist e gramáticas Shiki', async () => {
  const server = await createServer({ server: { middlewareMode: true }, appType: 'custom' });
  try {
    const { shikiLanguage } = await server.ssrLoadModule('/src/editor/shiki-colors.ts');
    for (const [filename, language] of Object.entries({
      'player.gd': 'gdscript', 'level.tscn': 'gdresource', 'item.tres': 'gdresource',
      'project.godot': 'gdresource', 'code.exs': 'elixir', 'index.cjs': 'javascript',
      'index.mjs': 'javascript', 'script.cmd': 'bat', '.env.example': 'dotenv',
      'settings.env': 'dotenv', Dockerfile: 'docker', 'Cargo.toml': 'toml',
      'code.ts': 'typescript', 'code.tsx': 'tsx', 'code.js': 'javascript',
      'code.jsx': 'javascript', 'code.json': 'json', 'code.html': 'html',
      'code.css': 'css', 'code.scss': 'scss', 'code.yaml': 'yaml',
      'code.md': 'markdown', 'code.c': 'c', 'code.cpp': 'cpp',
      'code.h': 'c', 'code.cs': 'csharp', 'code.fs': 'fsharp',
      'code.rs': 'rust', 'code.py': 'python', 'code.astro': 'astro',
      'code.ts.unknown': null, 'typescript': null,
      'unknown.xyzdesconhecida': null,
    })) assert.equal(shikiLanguage(filename), language, filename);
  } finally { await server.close(); }
});
