// Check das regras puras da faixa de abas (src/file-tab-actions.ts): fechamentos em lote, vizinha, fixadas e textos copiados.
// Roda sem dependências e sem navegador: `node scripts/check-file-tabs.mjs`. Sai com 1 e mostra a falha se alguma regra quebrar.
import assert from 'node:assert/strict';
import {
  batchTargets, dropFrom, fileNameError, isWithin, neighborAfterClose, pathText, pinFrom, pinnedFirst, relativeTo, renamedFiles, settleBatch,
} from '../src/file-tab-actions.ts';

const checks = [];
const check = (name, fn) => checks.push({ name, fn });
const none = new Set();

const paths = ['a.ts', 'b.ts', 'c.ts', 'd.ts'];

check('Fechar Outros: não inclui a aba de referência nem fixadas', () => {
  assert.deepEqual(batchTargets(paths, ['a.ts'], none, 'others', 'c.ts'), ['b.ts', 'd.ts']);
});
check('Fechar Outros sem referência não alcança nada', () => {
  assert.deepEqual(batchTargets(paths, [], none, 'others'), []);
});
check('Fechar à Direita: só as não fixadas depois da referência', () => {
  assert.deepEqual(batchTargets(paths, ['c.ts'], none, 'right', 'b.ts'), ['d.ts']);
});
check('Fechar à Direita da última aba: vazio', () => {
  assert.deepEqual(batchTargets(paths, [], none, 'right', 'd.ts'), []);
});
check('Fechar Salvos: pula sujas e fixadas', () => {
  assert.deepEqual(batchTargets(paths, ['b.ts'], new Set(['c.ts']), 'saved'), ['a.ts', 'd.ts']);
});
check('Fechar Tudo: só as não fixadas (fixadas ficam)', () => {
  assert.deepEqual(batchTargets(paths, ['a.ts', 'd.ts'], none, 'all'), ['b.ts', 'c.ts']);
});

check('Vizinha ao fechar: a da direita entre as que ficam', () => {
  assert.equal(neighborAfterClose(paths, 'b.ts', new Set(['b.ts'])), 'c.ts');
});
check('Vizinha ao fechar: sem direita, a da esquerda', () => {
  assert.equal(neighborAfterClose(paths, 'd.ts', new Set(['d.ts'])), 'c.ts');
});
check('Vizinha ao fechar: pula as que também saem', () => {
  assert.equal(neighborAfterClose(paths, 'b.ts', new Set(['b.ts', 'c.ts'])), 'd.ts');
});
check('Vizinha ao fechar: nenhuma sobra, null', () => {
  assert.equal(neighborAfterClose(['a.ts'], 'a.ts', new Set(['a.ts'])), null);
});

check('Fixadas vão para o começo, na ordem que já tinham', () => {
  assert.deepEqual(pinnedFirst(paths, ['c.ts', 'a.ts']), ['a.ts', 'c.ts', 'b.ts', 'd.ts']);
});
check('Fixar move a aba para o começo; desafixar a devolve à ordem', () => {
  const pinned = pinFrom({ paths, active: 'b.ts', pinned: [] }, 'c.ts');
  assert.deepEqual(pinned.paths, ['c.ts', 'a.ts', 'b.ts', 'd.ts']);
  assert.deepEqual(pinned.pinned, ['c.ts']);
  assert.deepEqual(pinFrom(pinned, 'c.ts').pinned, []);
});
check('Fixar caminho que não está aberto não muda o estado', () => {
  const state = { paths, active: 'a.ts', pinned: [] };
  assert.equal(pinFrom(state, 'zzz.ts'), state);
});
check('Fechar a ativa passa a ativa para a vizinha; fechar outra a mantém', () => {
  const state = { paths, active: 'b.ts', pinned: ['a.ts'] };
  assert.equal(dropFrom(state, new Set(['b.ts'])).active, 'c.ts');
  assert.equal(dropFrom(state, new Set(['d.ts'])).active, 'b.ts');
  assert.deepEqual(dropFrom(state, new Set(['a.ts'])).pinned, []);
});

check('Fechamento em lote sem rascunho: fecha sem perguntar', async () => {
  let asked = false;
  const result = await settleBatch('all', undefined,
    () => ({ paths, pinned: [], dirty: none }), async () => { asked = true; return true; });
  assert.deepEqual(result, paths);
  assert.equal(asked, false);
});
check('Cancelar a confirmação não fecha nenhuma aba', async () => {
  const result = await settleBatch('all', undefined,
    () => ({ paths, pinned: [], dirty: new Set(['a.ts']) }), async () => false);
  assert.equal(result, null);
});
check('Confirmar fecha as não fixadas, sujas inclusive', async () => {
  const result = await settleBatch('all', undefined,
    () => ({ paths, pinned: ['d.ts'], dirty: new Set(['a.ts']) }), async () => true);
  assert.deepEqual(result, ['a.ts', 'b.ts', 'c.ts']);
});
check('Cancelar com uma suja e uma limpa conserva as duas (lote Fechar Tudo)', async () => {
  const dirty = new Set(['a.ts']);
  const result = await settleBatch('all', undefined,
    () => ({ paths: ['a.ts', 'b.ts'], pinned: [], dirty }), async () => false);
  assert.equal(result, null);
});

check('Texto relativo usa o separador do Windows quando o projeto tem \\', () => {
  assert.equal(pathText('relative', 'src/App.tsx', 'D:\\orquestrador'), 'src\\App.tsx');
});
check('Texto absoluto junta projeto e caminho, sem barra final duplicada', () => {
  assert.equal(pathText('absolute', 'src/App.tsx', 'D:\\orquestrador\\'), 'D:\\orquestrador\\src\\App.tsx');
  assert.equal(pathText('absolute', 'src/App.tsx', '/home/u/proj'), '/home/u/proj/src/App.tsx');
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
check('Renomear troca o caminho na aba, na ativa e nos fixados, sem mudar a ordem', () => {
  const state = { paths: ['a.ts', 'b.ts', 'c.ts'], active: 'b.ts', pinned: ['a.ts'] };
  assert.deepEqual(renamedFiles(state, 'b.ts', 'x.ts'), { paths: ['a.ts', 'x.ts', 'c.ts'], active: 'x.ts', pinned: ['a.ts'] });
  assert.deepEqual(renamedFiles(state, 'a.ts', 'y.ts'), { paths: ['y.ts', 'b.ts', 'c.ts'], active: 'b.ts', pinned: ['y.ts'] });
});
check('Renomear arquivo que não está aberto não muda o estado', () => {
  const state = { paths: ['a.ts'], active: 'a.ts', pinned: [] };
  assert.equal(renamedFiles(state, 'zzz.ts', 'y.ts'), state);
});

let failed = 0;
for (const { name, fn } of checks) {
  try {
    await fn();
    console.log(`ok   ${name}`);
  } catch (e) {
    failed++;
    console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`);
  }
}
console.log(`\n${checks.length - failed}/${checks.length} verificações passaram`);
process.exitCode = failed ? 1 : 0;
