// Check do parser de links dos terminais (src/terminal/term-link-parse.ts): URLs, caminhos relativos e absolutos, sufixos de posição e
// pontuação final. Uso: node tests/check-term-links.mjs
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import { findTermLinks, resolveTermPath } from '../src/terminal/term-link-parse.ts';

const one = (text) => {
  const links = findTermLinks(text);
  assert.equal(links.length, 1, `${text}: ${JSON.stringify(links)}`);
  const { start, end, ...rest } = links[0];
  return { ...rest, text: text.slice(start, end) };
};

check('relativos', () => {
  assert.deepEqual(one('veja src/app.tsx agora'), { kind: 'path', path: 'src/app.tsx', text: 'src/app.tsx' });
  assert.deepEqual(one('./src/app.tsx'), { kind: 'path', path: './src/app.tsx', text: './src/app.tsx' });
  assert.deepEqual(one('em ..\\x\\y.ts'), { kind: 'path', path: '..\\x\\y.ts', text: '..\\x\\y.ts' });
  assert.deepEqual(one('README.md'), { kind: 'path', path: 'README.md', text: 'README.md' });
});

check('absolutos do Windows', () => {
  assert.equal(one('D:\\orquestrador\\src\\app.tsx').path, 'D:\\orquestrador\\src\\app.tsx');
  assert.equal(one('at (D:/orquestrador/src/app.tsx:3:4)').path, 'D:/orquestrador/src/app.tsx');
});

check('sufixos de posição', () => {
  assert.deepEqual(one('src/a.ts:12'), { kind: 'path', path: 'src/a.ts', line: 12, text: 'src/a.ts:12' });
  assert.deepEqual(one('src/a.ts:12:5'), { kind: 'path', path: 'src/a.ts', line: 12, col: 5, text: 'src/a.ts:12:5' });
  assert.deepEqual(one('src/a.ts(12,5)'), { kind: 'path', path: 'src/a.ts', line: 12, col: 5, text: 'src/a.ts(12,5)' });
  assert.deepEqual(one('src/a.ts:12-20'), { kind: 'path', path: 'src/a.ts', line: 12, text: 'src/a.ts:12-20' });
});

check('pontuação final', () => {
  for (const text of ['src/a.ts.', 'src/a.ts,', '(src/a.ts)', "'src/a.ts'", '"src/a.ts"', '`src/a.ts`']) assert.equal(one(text).text, 'src/a.ts', text);
  assert.equal(one('src/a.ts:3.').text, 'src/a.ts:3');
});

check('URLs', () => {
  assert.deepEqual(one('abra https://example.com/a?b=1.'), { kind: 'url', url: 'https://example.com/a?b=1', text: 'https://example.com/a?b=1' });
  assert.equal(one('(veja https://example.com/x)').url, 'https://example.com/x');
  assert.equal(one('https://en.wikipedia.org/wiki/A_(b)').url, 'https://en.wikipedia.org/wiki/A_(b)');
  assert.equal(one('http://localhost:5173/src/a.ts,').url, 'http://localhost:5173/src/a.ts');
});

check('texto sem link', () => {
  for (const text of ['', 'olá mundo', 'versão 1.5 e 2.0', 'a: b', '...']) assert.deepEqual(findTermLinks(text), [], text);
});

check('resolução', () => {
  assert.equal(resolveTermPath('./src/../a.ts', 'D:\\proj'), 'D:/proj/a.ts');
  assert.equal(resolveTermPath('D:\\x\\y.ts', 'C:\\p'), 'D:/x/y.ts');
  assert.equal(resolveTermPath('a.ts', null), null);
  assert.equal(resolveTermPath('../../a', 'C:/p'), null);
});
