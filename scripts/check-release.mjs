// Check das regras de release (scripts/release.mjs), sem dependências e sem rede: `node scripts/check-release.mjs`.
// Sai com 1 e mostra a falha se alguma regra quebrar. A conferência real do repositório é `node scripts/release.mjs check-versions`.
import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, randomBytes, sign } from 'node:crypto';
import {
  cargoLockVersions, cargoPackageVersion, checkNotes, checkVersions, installerName, installerUrl, isNewer, noteProblems, prepareManifest,
  verifyBody, verifyInstaller, verifyManifest,
} from './release.mjs';

const checks = [];
const check = (name, fn) => checks.push({ name, fn });

// Notas mínimas que passam: todas as seções, com texto suficiente em cada uma.
const WORDS = 'Texto concreto sobre a mudança, com o comportamento real do app e o que muda para quem usa. '
  + 'A descrição diz o que o usuário vê, o que mudou por baixo e qualquer limite que continua valendo depois da atualização.';
const section = (title, words = WORDS) => `## ${title}\n\n${words}\n`;
const validNotes = (version = '0.1.7', previous = '0.1.6') => [
  `# Orquestrador v${version}`,
  section('Resumo'),
  section('Novidades'),
  section('Melhorias e correções'),
  section('Impacto da atualização'),
  section('Requisitos e limitações'),
  section('Instalação'),
  section('Comparação com a versão anterior', `Comparado com v${previous}, o app agora faz isso e aquilo, com a mesma base de dados e a mesma configuração do usuário.`),
].join('\n');

// Assinatura no formato do Tauri: base64 do .sig do minisign, com o comentário confiável em tabs.
const sigFor = (version, extra = '') => Buffer.from(
  `untrusted comment: signature from tauri secret key\nRUSIG\ntrusted comment: timestamp:1700000000\tfile:Orquestrador_${version}_x64-setup.exe${extra}\tversion:${version}\nGLOBALSIG\n`,
).toString('base64');

const manifest = (version, sig, notes = 'antigas') => JSON.stringify({
  version,
  notes,
  pub_date: '2026-10-14T00:00:00Z',
  platforms: { 'windows-x86_64': { signature: sig, url: `https://api.github.com/repos/x/assets/1` } },
}, null, 2);

check('Notas completas passam, com a comparação citando a versão anterior', () => {
  assert.deepEqual(noteProblems(validNotes(), '0.1.7', '0.1.6'), []);
  assert.equal(checkNotes(validNotes(), '0.1.7', '0.1.6').startsWith('# Orquestrador v0.1.7'), true);
});

check('Notas ausentes, vazias ou só com o título são recusadas', () => {
  for (const text of ['', '   \n', '# Orquestrador v0.1.7\n']) {
    assert.throws(() => checkNotes(text, '0.1.7'), /Notas da 0\.1\.7 recusadas/);
  }
});

check('Título com outra versão é recusado', () => {
  assert.match(noteProblems(validNotes('0.1.6'), '0.1.7').join('\n'), /primeira linha/);
});

check('Texto genérico e de modelo é recusado', () => {
  const generic = validNotes().replace('## Resumo', '## Resumo\n\nTODO escrever depois');
  assert.match(noteProblems(generic, '0.1.7', '0.1.6').join('\n'), /genérico: "TODO"/);
  const auto = validNotes().replace('Texto concreto', 'Instalador para Windows, por usuário. O app se atualiza sozinho ao abrir.');
  assert.match(noteProblems(auto, '0.1.7', '0.1.6').join('\n'), /genérico/);
});

check('Seção obrigatória faltando ou vazia é recusada', () => {
  const missing = validNotes().replace(/## Requisitos e limitações[\s\S]*?(?=## Instalação)/, '');
  assert.match(noteProblems(missing, '0.1.7', '0.1.6').join('\n'), /falta a seção "## Requisitos e limitações"/);
  const empty = validNotes().replace(section('Resumo'), '## Resumo\n\n');
  assert.match(noteProblems(empty, '0.1.7', '0.1.6').join('\n'), /"## Resumo" está vazia ou curta demais/);
});

check('Notas curtas demais são recusadas, mesmo com todas as seções', () => {
  const short = ['# Orquestrador v0.1.7', '## Resumo\n\nCurto.', '## Novidades\n\nCurto.'].join('\n');
  assert.match(noteProblems(short, '0.1.7').join('\n'), /curtas demais/);
});

check('Comparação que não cita a versão anterior é recusada', () => {
  assert.match(noteProblems(validNotes('0.1.7', '0.1.5'), '0.1.7', '0.1.6').join('\n'), /não cita v0\.1\.6/);
});

check('Versões: todos iguais passam; um divergente é apontado', () => {
  const same = { a: '0.1.7', b: '0.1.7' };
  assert.equal(checkVersions(same, '0.1.7'), '0.1.7');
  assert.throws(() => checkVersions({ a: '0.1.7', b: '0.1.6', c: undefined }, '0.1.7'), /b: 0\.1\.6[\s\S]*c: ausente/);
  assert.throws(() => checkVersions(same, 'v0.1.7'), /não é uma versão X\.Y\.Z/);
});

check('Cargo: lê a versão do [package] e as entradas do orquestrador no Cargo.lock', () => {
  const toml = '[package]\nname = "orquestrador"\nversion = "0.1.7"\n\n[dependencies]\nserde = { version = "1" }\n';
  assert.equal(cargoPackageVersion(toml), '0.1.7');
  assert.equal(cargoPackageVersion('[dependencies]\nversion = "9.9.9"\n'), undefined);
  const lock = '[[package]]\nname = "serde"\nversion = "1.0.0"\n\n[[package]]\nname = "orquestrador"\nversion = "0.1.7"\ndependencies = []\n';
  assert.deepEqual(cargoLockVersions(lock, 'orquestrador'), ['0.1.7']);
  assert.deepEqual(cargoLockVersions('name = "x"\n', 'orquestrador'), []);
});

check('Versão mais nova compara como número, não como texto', () => {
  assert.equal(isNewer('0.1.7', '0.1.6'), true);
  assert.equal(isNewer('0.1.10', '0.1.9'), true);
  assert.equal(isNewer('0.1.6', '0.1.6'), false);
  assert.equal(isNewer('0.1.5', '0.1.6'), false);
});

check('Assinatura: a versão assinada é o campo version: exato, não um prefixo', () => {
  const ok = sigFor('0.1.7');
  const fixed = prepareManifest(manifest('0.1.7', ok), '0.1.7', validNotes());
  assert.doesNotThrow(() => verifyManifest(fixed, '0.1.7', ok, validNotes()));
  // Um sig de 0.1.70 não serve para 0.1.7: o campo tem de ser igual.
  const other = Buffer.from('trusted comment: timestamp:1\tversion:0.1.70\n').toString('base64');
  assert.throws(() => verifyManifest(manifest('0.1.7', other), '0.1.7', other), /versão assinada é 0\.1\.70/);
});

check('Manifesto: URL de rascunho, assinatura trocada ou notes antigas são recusados', () => {
  const sig = sigFor('0.1.7');
  const notes = validNotes();
  const fixed = prepareManifest(manifest('0.1.7', sig), '0.1.7', notes);
  assert.doesNotThrow(() => verifyManifest(fixed, '0.1.7', sig, notes));
  assert.throws(() => verifyManifest(manifest('0.1.7', sig), '0.1.7', sig, notes), /URL https:\/\/api\.github/);
  assert.throws(() => verifyManifest(fixed, '0.1.7', sigFor('0.1.7', '\tother'), notes), /não é a do \.sig/);
  assert.throws(() => verifyManifest(fixed, '0.1.7', sig, 'outra coisa'), /notes do latest\.json/);
  assert.throws(() => verifyManifest(fixed, '0.1.8', sig), /anuncia 0\.1\.7, não 0\.1\.8/);
});

check('Notes do latest.json: CRLF x LF e espaço final são o mesmo texto; conteúdo alterado é recusado', () => {
  const sig = sigFor('0.1.7');
  const lf = validNotes();
  const crlf = lf.replace(/\n/g, '\r\n');
  // O manifesto publicado da v0.1.7 traz CRLF; as notas locais são LF. Nos dois sentidos, e com espaço final.
  const crlfManifest = JSON.stringify({ ...JSON.parse(prepareManifest(manifest('0.1.7', sig), '0.1.7', lf)), notes: crlf }, null, 2);
  assert.doesNotThrow(() => verifyManifest(crlfManifest, '0.1.7', sig, lf));
  assert.doesNotThrow(() => verifyManifest(prepareManifest(manifest('0.1.7', sig), '0.1.7', crlf), '0.1.7', sig, crlf));
  assert.doesNotThrow(() => verifyManifest(crlfManifest, '0.1.7', sig, `${crlf}\r\n`));
  assert.throws(() => verifyManifest(crlfManifest, '0.1.7', sig, lf.replace('Resumo', 'Resumo alterado')), /notes do latest\.json/);
  assert.throws(() => verifyManifest(crlfManifest, '0.1.7', sig, `${lf}x`), /notes do latest\.json/);
  // prepareManifest grava a forma canônica (LF) para as próximas versões.
  assert.equal(JSON.parse(prepareManifest(manifest('0.1.7', sig), '0.1.7', crlf)).notes, lf.trim());
});

check('prepareManifest põe a URL pública e o texto completo das notas', () => {
  const sig = sigFor('0.1.7');
  const out = JSON.parse(prepareManifest(manifest('0.1.7', sig), '0.1.7', `${validNotes()}\n`));
  assert.equal(out.platforms['windows-x86_64'].url, installerUrl('0.1.7'));
  assert.equal(out.platforms['windows-x86_64'].url, 'https://github.com/luansilvadb/orquestrador/releases/download/v0.1.7/Orquestrador_0.1.7_x64-setup.exe');
  assert.equal(out.notes, validNotes().trim());
  assert.equal(out.platforms['windows-x86_64'].signature, sig);
  assert.throws(() => prepareManifest(manifest('0.1.6', sig), '0.1.7', 'x'), /anuncia 0\.1\.6/);
});

// Assinatura Minisign de teste, com chaves Ed25519 geradas aqui (nenhum segredo do projeto). Formato igual ao do Tauri.
const keyPair = (keyId = randomBytes(8)) => {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const raw = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const keyFile = `untrusted comment: minisign public key: ${keyId.toString('hex').toUpperCase()}\n${Buffer.concat([Buffer.from('Ed'), keyId, raw]).toString('base64')}\n`;
  return { keyB64: Buffer.from(keyFile).toString('base64'), keyId, privateKey };
};
const signInstaller = (key, file, { name = installerName('0.1.7'), version = '0.1.7', alg = 'ED' } = {}) => {
  const message = alg === 'ED' ? createHash('blake2b512').update(file).digest() : file;
  const sig = sign(null, message, key.privateKey);
  const trusted = `timestamp:1700000000\tfile:${name}\tversion:${version}`;
  const global = sign(null, Buffer.concat([sig, Buffer.from(trusted)]), key.privateKey);
  const text = `untrusted comment: signature from test key\n${Buffer.concat([Buffer.from(alg), key.keyId, sig]).toString('base64')}\ntrusted comment: ${trusted}\n${global.toString('base64')}\n`;
  return Buffer.from(text).toString('base64');
};
// Edita uma linha do .sig decodificado (0 comentário, 1 assinatura, 2 comentário confiável, 3 global) e recodifica.
const editSig = (sigB64, edit) => Buffer.from(edit(Buffer.from(sigB64, 'base64').toString('utf8').split('\n')).join('\n')).toString('base64');
const flipLine = (b64, at) => {
  const bytes = Buffer.from(b64, 'base64');
  bytes[at] ^= 1;
  return bytes.toString('base64');
};
const INSTALLER = Buffer.from('conteúdo de teste do instalador, nunca distribuído');
const NAME = installerName('0.1.7');

check('Instalador assinado confere (alg ED e Ed); arquivo, assinatura, comentário e chave adulterados falham', () => {
  const key = keyPair();
  const sig = signInstaller(key, INSTALLER);
  assert.match(verifyInstaller('0.1.7', NAME, INSTALLER, sig, key.keyB64), /conferidos \(versão 0\.1\.7\)/);
  assert.doesNotThrow(() => verifyInstaller('0.1.7', NAME, INSTALLER, signInstaller(key, INSTALLER, { alg: 'Ed' }), key.keyB64));
  // Arquivo: um bit a mais no instalador.
  const fileBad = Buffer.from(INSTALLER);
  fileBad[3] ^= 1;
  assert.throws(() => verifyInstaller('0.1.7', NAME, fileBad, sig, key.keyB64), /não confere com a chave/);
  // Assinatura do payload: um bit trocado na linha de assinatura.
  const sigBad = editSig(sig, (lines) => { lines[1] = flipLine(lines[1], 20); return lines; });
  assert.throws(() => verifyInstaller('0.1.7', NAME, INSTALLER, sigBad, key.keyB64), /não confere com a chave/);
  // Comentário confiável: a versão trocada no comentário quebra só a assinatura global.
  const commentBad = editSig(sig, (lines) => { lines[2] = lines[2].replace('version:0.1.7', 'version:0.1.8'); return lines; });
  assert.throws(() => verifyInstaller('0.1.7', NAME, INSTALLER, commentBad, key.keyB64), /comentário confiável alterado/);
  // Chave: outra chave com o mesmo key id, outra chave com outro key id, e um bit da chave pública trocado.
  assert.throws(() => verifyInstaller('0.1.7', NAME, INSTALLER, sig, keyPair(key.keyId).keyB64), /não confere com a chave/);
  assert.throws(() => verifyInstaller('0.1.7', NAME, INSTALLER, sig, keyPair().keyB64), /assinatura de outra chave/);
  const keyText = Buffer.from(key.keyB64, 'base64').toString('utf8').split('\n');
  const keyBad = Buffer.from(`${keyText[0]}\n${flipLine(keyText[1], 30)}\n`).toString('base64');
  assert.throws(() => verifyInstaller('0.1.7', NAME, INSTALLER, sig, keyBad), /não confere com a chave/);
});

check('Versão ou nome de arquivo assinados diferentes do instalador são recusados', () => {
  const key = keyPair();
  assert.throws(() => verifyInstaller('0.1.7', NAME, INSTALLER, signInstaller(key, INSTALLER, { version: '0.1.8' }), key.keyB64), /versão assinada é 0\.1\.8/);
  assert.throws(() => verifyInstaller('0.1.7', NAME, INSTALLER, signInstaller(key, INSTALLER, { name: 'outro.exe' }), key.keyB64), /diz o arquivo outro\.exe/);
  assert.throws(() => verifyInstaller('0.1.7', 'outro.exe', INSTALLER, signInstaller(key, INSTALLER), key.keyB64), /se chama outro\.exe/);
});

check('Corpo da release: igual às notas, só com espaço final ou CRLF/LF diferente; mudança interna, texto vazio ou faltando é recusado', () => {
  assert.equal(verifyBody(`${validNotes()}\n\n`, validNotes()), 'corpo da release igual às notas.');
  assert.equal(verifyBody(validNotes().replace(/\n/g, '\r\n'), validNotes()), 'corpo da release igual às notas.');
  assert.throws(() => verifyBody(`${validNotes()}x`, validNotes()), /não é o texto completo/);
  assert.throws(() => verifyBody(validNotes().replace('Resumo', 'Resumo alterado'), validNotes()), /não é o texto completo/);
  assert.throws(() => verifyBody('', validNotes()), /não é o texto completo/);
});

let failed = 0;
for (const { name, fn } of checks) {
  try {
    fn();
    console.log(`ok   ${name}`);
  } catch (error) {
    failed += 1;
    console.error(`FALHA ${name}\n${error.message}`);
  }
}
console.log(`${checks.length - failed} de ${checks.length} checks passaram.`);
if (failed > 0) process.exit(1);
