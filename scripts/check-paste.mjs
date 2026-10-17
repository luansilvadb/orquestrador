// Check da colagem de arquivo e imagem no terminal (src/terminal/paste.ts), a mesma no orquestrador e nos shells: imagem solta vs arquivo, vários
// arquivos misturados e na ordem, texto sem repetição, o CLI que abre e sai no mesmo terminal, terminais com CLIs diferentes e o que não dá
// para colar virando erro. O efeito real nos CLIs fica no teste ignorado `src-tauri/tests/colagem.rs`. Roda sem dependências:
// `node scripts/check-paste.mjs`. Sai com 1 e mostra a falha se alguma regra quebrar.
import assert from 'node:assert/strict';
import { test as check } from 'node:test';
import { clipboardFiles, clipboardPaste } from '../src/terminal/paste.ts';

const BS = '\x1b[200~', BE = '\x1b[201~', F12 = '\x1b[24~';
const PNG = 'C:\\proj\\a.png', SPACED = 'C:\\com espaço\\b.png', TXT = 'C:\\proj\\nota.txt', DIR = 'C:\\proj\\pasta';
const data = (items, text = '') => ({ items, getData: () => text });

check('clipboardFiles: só texto fica com o paste do xterm (null), e nada é colado de novo por aqui', () => {
  assert.equal(clipboardFiles(data([{ kind: 'string', type: 'text/plain' }], 'oi')), null);
  assert.equal(clipboardFiles(null), null);
});

check('clipboardFiles: arquivo e imagem saem com o texto que veio junto', () => {
  assert.deepEqual(clipboardFiles(data([{ kind: 'file', type: 'image/png' }], 'alt')), { image: true, text: 'alt' });
  assert.deepEqual(clipboardFiles(data([{ kind: 'string', type: 'text/plain' }, { kind: 'file', type: '' }])), { image: false, text: '' });
});

check('imagem solta: o gesto de colar imagem de cada CLI, sem Enter e sem o texto junto', () => {
  assert.deepEqual(clipboardPaste('claude', [], true, 'alt', true), { write: '\x1bv' });
  assert.deepEqual(clipboardPaste('codex', [], true, 'alt', true), { write: '\x16' });
});

check('arquivo de imagem copiado: o caminho vai ao CLI (que anexa), não o gesto de imagem solta', () => {
  assert.deepEqual(clipboardPaste('claude', [PNG], true, '', true), { write: `${BS}${PNG} ${BE}` });
  assert.deepEqual(clipboardPaste('codex', [PNG], true, '', true), { write: `${BS}${PNG} ${BE}` });
});

check('Claude: vários arquivos misturados num paste só, um por linha (com espaço no fim, que separa o caminho do texto seguinte), na ordem, com aspas no caminho com espaço', () => {
  const out = clipboardPaste('claude', [TXT, PNG, DIR, SPACED], true, '', true);
  assert.deepEqual(out, { write: `${BS}${TXT} \n${PNG} \n${DIR} \n"${SPACED}" ${BE}` });
  assert.ok(!out.write.includes('\r'), 'sem Enter');
});

check('Codex: um paste por arquivo, na ordem, separados por F12, sem Enter', () => {
  const out = clipboardPaste('codex', [TXT, PNG, DIR, SPACED], true, '', true);
  assert.deepEqual(out.write.split(F12), [`${BS}${TXT} ${BE}`, `${BS}${PNG} ${BE}`, `${BS}${DIR} ${BE}`, `${BS}"${SPACED}" ${BE}`]);
  assert.ok(!/[\r\n]/.test(out.write), 'sem Enter nem quebra de linha');
  assert.ok(!out.write.replace(/\x1b\[(20[01]|24)~/g, '').includes('\x1b'), 'nenhuma outra tecla: o cursor no meio do rascunho não anda');
});

check('mesmo terminal: com o CLI aberto anexa, depois que ele sai os caminhos voltam a ir como texto do shell', () => {
  assert.ok('write' in clipboardPaste('codex', [PNG, TXT], false, '', true));
  assert.deepEqual(clipboardPaste(null, [PNG, TXT, SPACED], false, '', true), { paste: `${PNG} ${TXT} "${SPACED}"` });
  assert.deepEqual(clipboardPaste(null, [PNG], false, '', false), { paste: PNG });
});

check('cada terminal cola pelo próprio CLI: o mesmo clipboard dá saídas diferentes no Claude, no Codex e no shell', () => {
  const outs = ['claude', 'codex', null].map((cli) => JSON.stringify(clipboardPaste(cli, [PNG, TXT], true, '', true)));
  assert.equal(new Set(outs).size, 3);
});

check('CLI sem paste entre colchetes: os caminhos vão como texto, sem gesto', () => {
  assert.deepEqual(clipboardPaste('claude', [PNG, TXT], false, '', false), { paste: `${PNG} ${TXT}` });
});

check('o que não dá para colar vira erro, não colagem perdida calada', () => {
  assert.match(clipboardPaste(null, [], true, '', true).error, /imagem não foi colada/);
  assert.match(clipboardPaste('codex', [], false, '', true).error, /nada para colar/);
  assert.deepEqual(clipboardPaste(null, [], true, 'texto junto', true), { paste: 'texto junto' });
});
