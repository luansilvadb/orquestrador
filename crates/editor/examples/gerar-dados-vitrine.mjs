// Gera os arquivos do projeto simulado da vitrine com os mesmos textos de `src/backend/mock.ts` e `mock-fixtures.ts`.
// Rodar da raiz: `node crates/editor/examples/gerar-dados-vitrine.mjs`.
import { writeFileSync, readFileSync } from 'node:fs';
import { EDITOR_HEAD_LINES } from '../../../src/backend/mock-fixtures.ts';

const dir = new URL('./dados/', import.meta.url);
const mock = readFileSync(new URL('../../../src/backend/mock.ts', import.meta.url), 'utf8');
const literal = String.raw`('(?:[^'\\]|\\.)*')`;
const texto = (chave) => eval(new RegExp(`'${chave.replace(/[.]/g, '\\.')}': ${literal},`).exec(mock)[1]);
const head = EDITOR_HEAD_LINES.join('\n') + '\n';
const editor = head
  .replace('useLayoutEffect, useRef, useState,', 'useLayoutEffect, useMemo, useRef, useState,')
  .replace("import { Columns2, Ellipsis, Pin, X } from 'lucide-react';", "import { ChevronDown, Columns2, Ellipsis, Pin, X } from 'lucide-react';")
  .replace('type DiskVersion = { text: string; bom: boolean; readOnly: boolean };', 'type DiskVersion = { text: string; bom: boolean; readOnly: boolean; size: number };');
const longo = Array.from({ length: 300 }, (_, i) => (i % 7 === 6 ? '' : i === 150 ? `export const longa = '${'x'.repeat(400)}';` : `export const linha${i} = ${i * 3};`)).join('\n');
const capt = readFileSync(new URL('../../../scripts/capturar-referencia.mjs', import.meta.url), 'utf8');
const guia = eval(new RegExp(`const GUIA = ${literal};`).exec(capt)[1]);
const arquivos = {
  'README.md': texto('README.md'), 'App.tsx': texto('src/App.tsx'), 'main.tsx': texto('src/main.tsx'),
  'editor-workspace.tsx': editor, 'longo.ts': longo, 'guia.md': guia,
};
for (const [nome, t] of Object.entries(arquivos)) writeFileSync(new URL(nome, dir), t);
