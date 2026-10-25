// Check das regras puras de caminho e nome usadas pelo app (src/shared/file-tab-actions.ts): texto copiado, caminho relativo, pasta que contém,
// e nomes recusados no Windows. Roda sem dependências e sem navegador: `node tests/check-file-tabs.mjs`. Sai com 1 se alguma regra quebrar.
// As regras de abas (fechamento em lote, vizinha, fixadas, renomear) estão em tests/check-workspace.mjs, no modelo que o editor usa.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import { fileNameError, isAbsolutePath, pathText, relativeTo } from '../src/shared/file-tab-actions.ts';
import { isWithin } from '../src/editor/workspace-model.ts';

check('Texto relativo usa o separador do Windows quando o projeto tem \\', () => {
  assert.equal(pathText('relative', 'src/App.tsx', 'D:\\orquestrador'), 'src\\App.tsx');
});
check('Texto absoluto junta projeto e caminho, sem barra final duplicada', () => {
  assert.equal(pathText('absolute', 'src/App.tsx', 'D:\\orquestrador\\'), 'D:\\orquestrador\\src\\App.tsx');
  assert.equal(pathText('absolute', 'src/App.tsx', '/home/u/proj'), '/home/u/proj/src/App.tsx');
});
check('Arquivo solto mantém o caminho absoluto e não inventa caminho relativo', () => {
  assert.equal(pathText('absolute', 'C:\\arquivos\\solto.ts', ''), 'C:\\arquivos\\solto.ts');
  assert.equal(pathText('navigation', 'C:\\arquivos\\solto.ts', '', { line: 3, col: 2 }), 'C:\\arquivos\\solto.ts:3:2');
  assert.equal(pathText('relative', 'C:\\arquivos\\solto.ts', ''), null);
  assert.equal(pathText('relative', 'D:/proj/src/a.ts', 'D:\\proj'), 'src\\a.ts');
});
check('Caminho de navegação traz linha e coluna; sem cursor, null', () => {
  assert.equal(pathText('navigation', 'src/App.tsx', 'D:\\orquestrador', { line: 12, col: 3 }), 'D:\\orquestrador\\src\\App.tsx:12:3');
  assert.equal(pathText('navigation', 'src/App.tsx', 'D:\\orquestrador'), null);
});

check('Raiz do projeto: relativo é "." e absoluto é a pasta, sem barra final; nunca texto vazio', () => {
  assert.equal(pathText('relative', '', 'D:\\orquestrador'), '.');
  assert.equal(pathText('absolute', '', 'D:\\orquestrador\\'), 'D:\\orquestrador');
  assert.equal(pathText('absolute', '', '/home/u/proj'), '/home/u/proj');
  assert.equal(pathText('navigation', '', 'D:\\orquestrador'), null);
});
check('Raiz de unidade do Windows: absoluto da raiz é D:\\ (com a barra), nunca D:', () => {
  assert.equal(pathText('absolute', '', 'D:\\'), 'D:\\');
  assert.equal(pathText('absolute', '', 'D:/'), 'D:/');
  assert.equal(pathText('relative', '', 'D:\\'), '.');
});
check('Raiz do Unix: absoluto da raiz é "/"', () => {
  assert.equal(pathText('absolute', '', '/'), '/');
});
check('Caminho relativo a partir do absoluto: dentro do projeto, sem diferença de caixa nem de barra', () => {
  assert.equal(relativeTo('D:\\orquestrador', 'D:\\orquestrador\\src\\App.tsx'), 'src/App.tsx');
  assert.equal(relativeTo('D:\\Orquestrador\\', 'd:/orquestrador/src/App.tsx'), 'src/App.tsx');
  assert.equal(relativeTo('D:\\orquestrador', 'D:\\orquestrador-antigo\\x.ts'), null);
  assert.equal(relativeTo('D:\\orquestrador', 'C:\\outra\\x.ts'), null);
  assert.equal(relativeTo('D:\\orquestrador', 'D:\\orquestrador'), null);
  assert.equal(relativeTo('', '/x/y.ts'), null, 'sem projeto nada é relativo');
});
check('Caminho absoluto: unidade, UNC e raiz; relativo não', () => {
  for (const path of ['C:\\x', 'c:/x', '\\\\srv\\x', '/x']) assert.equal(isAbsolutePath(path), true, path);
  for (const path of ['', 'x/y.ts', 'C:x', 'C:']) assert.equal(isAbsolutePath(path), false, path);
});
check('Dentro de: a própria pasta e o que está nela, nunca um irmão com prefixo igual', () => {
  assert.equal(isWithin('src', 'src'), true);
  assert.equal(isWithin('src/App.tsx', 'src'), true);
  assert.equal(isWithin('srcx/App.tsx', 'src'), false);
  assert.equal(isWithin('src2/a.ts', 'src'), false);
  assert.equal(isWithin('src2', 'src'), false);
});
check('Dentro de: sem diferença de caixa (readme.md está em README.md)', () => {
  assert.equal(isWithin('readme.md', 'README.md'), true);
  assert.equal(isWithin('Pasta/Sub/a.ts', 'pasta'), true);
  assert.equal(isWithin('Pasta2/a.ts', 'pasta'), false);
});
check('Dentro de: pasta irmã com caixa diferente e prefixo igual não entra', () => {
  assert.equal(isWithin('SRC2/a.ts', 'src'), false);
  assert.equal(isWithin('Src/App.tsx', 'SRC'), true);
});
check('.git é recusado em qualquer caixa, como nome novo', () => {
  assert.equal(fileNameError('.git'), 'A pasta .git não pode ser alterada aqui.');
  assert.equal(fileNameError('.GIT'), 'A pasta .git não pode ser alterada aqui.');
  assert.equal(fileNameError('.gitignore'), null);
});
check('Nome de arquivo: aceita nome comum, com espaço e com acento', () => {
  for (const name of ['novo.txt', 'Arquivo com espaço.md', 'ação.ts', 'conta.con', '.env']) assert.equal(fileNameError(name), null, name);
});
check('Nome de arquivo: recusa vazio, ponto, separador, caractere proibido e reservado', () => {
  for (const name of ['', '   ', '.', '..', 'a/b', 'a\\b', 'a:b', 'a*b', 'a?b', 'a"b', 'a<b', 'a|b', 'a\u0001b', ' x', 'x ', 'x.', 'CON', 'nul.txt', 'com1.md', 'LPT9.log']) {
    assert.notEqual(fileNameError(name), null, `${JSON.stringify(name)} deveria ser recusado`);
  }
});
check('Nome recusado traz o motivo em português, com a barra invertida inteira', () => {
  assert.equal(fileNameError('a\\b'), 'O nome tem caractere que o Windows não aceita: < > : " / \\ | ? *');
  assert.equal(fileNameError('CON.txt'), '“CON.txt” é um nome reservado pelo Windows.');
});
