// Check do contrato dos comandos: as chaves de `Commands` (src/backend/types.ts) são as mesmas da lista `generate_handler![...]` (src-tauri/src/lib.rs).
// Lê os dois arquivos por texto, sem compilar nada. Roda sem dependências: `node tests/check-commands.mjs`. Sai com 1 e mostra quais nomes faltam de cada lado.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test as check } from 'node:test';

const lib = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const types = readFileSync(new URL('../src/backend/types.ts', import.meta.url), 'utf8');

// Nomes registrados: a lista vai do `generate_handler![` ao primeiro `]`, sem os comentários de linha.
const LISTA = 'generate_handler![';
const inicioLista = lib.indexOf(LISTA);
assert.notEqual(inicioLista, -1, 'generate_handler![ não encontrado em lib.rs');
const fimLista = lib.indexOf(']', inicioLista);
const registrados = new Set(
  lib.slice(inicioLista + LISTA.length, fimLista)
    .split(/\r?\n/).map((linha) => linha.replace(/\/\/.*$/, '')).join(' ')
    .split(/[,\s]+/).filter(Boolean),
);

// Chaves declaradas: uma por linha de `Commands`, no formato `  nome_do_comando: {`.
const inicioMapa = types.indexOf('export type Commands = {');
assert.notEqual(inicioMapa, -1, 'export type Commands não encontrado em types.ts');
const fimMapa = types.indexOf('\n};', inicioMapa);
const declarados = new Set([...types.slice(inicioMapa, fimMapa).matchAll(/^  ([a-z_0-9]+): \{/gm)].map((m) => m[1]));

check('Commands (types.ts) tem as mesmas chaves que generate_handler! (lib.rs)', () => {
  assert.ok(registrados.size > 0 && declarados.size > 0, 'nenhum comando lido: o formato de lib.rs ou de types.ts mudou');
  const problemas = [];
  const faltamEmCommands = [...registrados].filter((nome) => !declarados.has(nome)).sort();
  const faltamNoRust = [...declarados].filter((nome) => !registrados.has(nome)).sort();
  if (faltamEmCommands.length) problemas.push(`no generate_handler! (lib.rs) e faltando em Commands (types.ts): ${faltamEmCommands.join(', ')}`);
  if (faltamNoRust.length) problemas.push(`em Commands (types.ts) e faltando no generate_handler! (lib.rs): ${faltamNoRust.join(', ')}`);
  assert.deepEqual(problemas, [], problemas.join('\n'));
});
