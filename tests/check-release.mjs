// Check das regras de release (scripts/release.mjs), sem rede: `node tests/check-release.mjs`.
// Sai com 1 e mostra a falha se alguma regra quebrar. A conferência real do repositório é `node scripts/release.mjs check-versions`.
import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, randomBytes, sign } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { test as check } from 'node:test';
import { parse as parseYaml } from 'yaml';
import {
  cargoLockVersions, cargoPackageVersion, checkNotes, checkVersions, CI_WAIT_MS, installerName, installerUrl, noteProblems, prepareManifest,
  REQUIRED_SECTIONS, releaseBody, verifyBody, verifyInstaller, verifyManifest,
  waitForCi,
} from '../scripts/release.mjs';

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

const CI_SHA = 'a'.repeat(40);
const ciRun = (overrides = {}) => ({
  attempt: 1,
  conclusion: 'success',
  createdAt: '2026-10-30T10:00:00Z',
  databaseId: 100,
  event: 'push',
  headBranch: 'main',
  headSha: CI_SHA,
  startedAt: '2026-10-30T10:00:05Z',
  status: 'completed',
  updatedAt: '2026-10-30T10:01:00Z',
  workflowDatabaseId: 20,
  workflowName: 'ci',
  ...overrides,
});
const ciOutput = (...runs) => JSON.stringify(runs);

check('CI: só aceita push de ci.yml em main para o SHA exato', async () => {
  let calledWith;
  const message = await waitForCi(CI_SHA, {
    exec: async (args, options) => {
      calledWith = { args, options };
      return ciOutput(ciRun());
    },
  });
  assert.match(message, new RegExp(CI_SHA));
  const { args, options } = calledWith;
  assert.deepEqual(args.slice(0, 3), ['run', 'list', '--repo']);
  assert.ok(args.includes('--workflow') && args[args.indexOf('--workflow') + 1] === 'ci.yml');
  assert.ok(args.includes('--commit') && args[args.indexOf('--commit') + 1] === CI_SHA);
  assert.ok(args.includes('--branch') && args[args.indexOf('--branch') + 1] === 'main');
  assert.ok(args.includes('--event') && args[args.indexOf('--event') + 1] === 'push');
  assert.ok(args.includes('--limit') && args[args.indexOf('--limit') + 1] === '1000');
  assert.ok(args.includes('--json'));
  assert.ok(!args.includes('--status') && options.timeout > 0 && options.timeout <= CI_WAIT_MS);
});

check('Workflow: permissões e gates precedem alterações remotas e publicação', () => {
  const workflow = parseYaml(readFileSync(new URL('../.github/workflows/release.yml', import.meta.url), 'utf8'));
  const steps = workflow.jobs.release.steps;
  assert.equal(workflow.permissions.actions, 'read');

  const gateSteps = steps.map((step, index) => ({ step, index }))
    .filter(({ step }) => typeof step.run === 'string'
      && step.run.includes('git rev-parse HEAD')
      && step.run.includes('node scripts/release.mjs wait-ci $sha'));
  assert.equal(gateSteps.length, 2);

  const writes = steps.map((step, index) => ({ step, index }))
    .filter(({ step }) => step.uses?.startsWith('tauri-apps/tauri-action@')
      || /\bgh release (?:create|edit|upload|delete)\b/.test(step.run ?? ''));
  assert.ok(writes.length > 0);
  assert.ok(writes.every(({ index }) => gateSteps[0].index < index));

  const publish = gateSteps[1].step.run;
  const publishCommand = publish.indexOf('--draft=false --latest');
  assert.ok(publishCommand > publish.lastIndexOf('node scripts/release.mjs wait-ci $sha'));
});

check('CI: aguarda execução ausente ou pendente e só retorna após sucesso', async () => {
  const previous = ciRun();
  const queued = ciRun({ databaseId: 101, createdAt: '2026-10-30T11:00:00Z', updatedAt: '2026-10-30T11:00:01Z', status: 'queued', conclusion: '', startedAt: '0001-01-01T00:00:00Z' });
  const queuedSuccess = ciRun({ ...queued, startedAt: '2026-10-30T11:00:05Z', updatedAt: '2026-10-30T11:01:00Z', status: 'completed', conclusion: 'success' });
  const inProgress = ciRun({ databaseId: 102, createdAt: '2026-10-30T10:59:00Z', startedAt: '2026-10-30T11:00:05Z', updatedAt: '2026-10-30T11:00:10Z', status: 'in_progress', conclusion: '' });
  const inProgressSuccess = ciRun({ ...inProgress, updatedAt: '2026-10-30T11:01:00Z', status: 'completed', conclusion: 'success' });
  for (const [first, second] of [[[], [ciRun()]], [[queued], [queuedSuccess]], [[previous, queued], [previous, queuedSuccess]], [[inProgress], [inProgressSuccess]]]) {
    let calls = 0;
    const clock = { value: 0 };
    await waitForCi(CI_SHA, {
      timeoutMs: 100,
      pollMs: 20,
      now: () => clock.value,
      sleep: async (ms) => { clock.value += ms; },
      exec: async () => ciOutput(...(calls++ === 0 ? first : second)),
    });
    assert.equal(calls, 2);
  }
});

check('CI: conclusão terminal incoerente ou ausente bloqueia', async () => {
  await assert.rejects(waitForCi(CI_SHA, {
    exec: async () => ciOutput(ciRun({ status: 'in_progress', conclusion: 'success' })),
  }), /conclusão inesperada/);
  await assert.rejects(waitForCi(CI_SHA, {
    exec: async () => ciOutput(ciRun({ conclusion: '' })),
  }), /conclusão ausente/);
});

check('CI: SHA, branch, evento ou workflow retornado diferentes bloqueiam', async () => {
  for (const run of [
    ciRun({ headSha: 'b'.repeat(40) }),
    ciRun({ headBranch: 'release' }),
    ciRun({ event: 'pull_request' }),
    ciRun({ workflowName: 'outro' }),
  ]) {
    await assert.rejects(waitForCi(CI_SHA, { exec: async () => ciOutput(run) }), /diferente do esperado/);
  }
});

check('CI: falha e cancelamento da tentativa mais recente não são mascarados por sucesso anterior', async () => {
  const previous = ciRun({ databaseId: 101, createdAt: '2026-10-30T09:00:00Z', startedAt: '2026-10-30T09:00:05Z', updatedAt: '2026-10-30T09:01:00Z' });
  for (const conclusion of ['failure', 'cancelled', 'skipped']) {
    const latest = ciRun({ attempt: 2, databaseId: 102, createdAt: '2026-10-30T11:00:00Z', startedAt: '2026-10-30T11:00:05Z', updatedAt: '2026-10-30T11:01:00Z', conclusion });
    await assert.rejects(waitForCi(CI_SHA, { exec: async () => ciOutput(previous, latest) }), /tentativa 2/);
  }
});

check('CI: tentativa reexecutada mais recentemente vence run com criação posterior', async () => {
  const newerRun = ciRun({ databaseId: 103, createdAt: '2026-10-30T10:00:00Z', startedAt: '2026-10-30T10:00:05Z', updatedAt: '2026-10-30T10:01:00Z' });
  const rerun = ciRun({ attempt: 2, databaseId: 104, createdAt: '2026-10-30T09:00:00Z', startedAt: '2026-10-30T11:00:05Z', updatedAt: '2026-10-30T11:01:00Z', conclusion: 'failure' });
  await assert.rejects(waitForCi(CI_SHA, { exec: async () => ciOutput(newerRun, rerun) }), /tentativa 2/);
});

check('CI: API, JSON e campos inválidos bloqueiam sem aguardar', async () => {
  await assert.rejects(waitForCi(CI_SHA, { exec: async () => { throw new Error('gh indisponível'); } }), /gh indisponível/);
  await assert.rejects(waitForCi(CI_SHA, { exec: async () => '{' }), /JSON inválido/);
  await assert.rejects(waitForCi(CI_SHA, { exec: async () => '{}' }), /não retornou uma lista/);
  await assert.rejects(waitForCi(CI_SHA, { exec: async () => ciOutput({ ...ciRun(), updatedAt: 'inválido' }) }), /updatedAt inválido/);
  await assert.rejects(waitForCi(CI_SHA, { exec: async () => ciOutput(ciRun({ conclusion: 0 })) }), /conclusion inválida/);
  await assert.rejects(waitForCi(CI_SHA, { exec: async () => ciOutput(ciRun({ headSha: 42 })) }), /diferente do esperado/);
});

check('CI: ausência ou pendência persistente e sucesso depois do prazo expiram', async () => {
  for (const output of ['[]', ciOutput(ciRun({ status: 'in_progress', conclusion: '' }))]) {
    let clock = 0;
    await assert.rejects(waitForCi(CI_SHA, {
      timeoutMs: 50,
      pollMs: 25,
      now: () => clock,
      sleep: async (ms) => { clock += ms; },
      exec: async () => output,
    }), /30 minutos/);
  }

  let clock = 0;
  let calls = 0;
  await assert.rejects(waitForCi(CI_SHA, {
    timeoutMs: 50,
    now: () => clock,
    exec: async (_args, { timeout }) => { calls += 1; clock += timeout + 1; return ciOutput(ciRun()); },
  }), /30 minutos/);
  assert.equal(calls, 1);
});

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

// O corpo publicado começa em `## Resumo`, sem o H1 da fonte, e tem as sete seções na ordem.
check('Corpo da release sem o título: começa em ## Resumo, sem H1, com todas as seções na ordem', () => {
  const body = releaseBody(validNotes());
  assert.match(body, /^## Resumo\n/);
  assert.doesNotMatch(body, /^# /m);
  assert.deepEqual(body.split(/^## /m).slice(1).map((part) => part.split('\n')[0].trim()), REQUIRED_SECTIONS);
  assert.equal(releaseBody(`\r\n\r\n${validNotes().replace(/\n/g, '\r\n')}\r\n`), body);
});

check('Corpo da release: igual às notas sem o título, só com espaço final ou CRLF/LF diferente; título repetido, seção perdida, texto alterado ou vazio é recusado', () => {
  const body = releaseBody(validNotes());
  assert.equal(verifyBody(`${body}\n\n`, validNotes()), 'corpo da release igual às notas, sem o título.');
  assert.equal(verifyBody(body.replace(/\n/g, '\r\n'), validNotes()), 'corpo da release igual às notas, sem o título.');
  assert.throws(() => verifyBody(validNotes(), validNotes()), /sem o título/);
  assert.throws(() => verifyBody(`# Orquestrador v0.1.7\r\n\r\n${body}`, validNotes()), /sem o título/);
  assert.throws(() => verifyBody(body.replace(/## Instalação[\s\S]*?(?=## Comparação)/, ''), validNotes()), /sem o título/);
  assert.throws(() => verifyBody(`${body}x`, validNotes()), /sem o título/);
  assert.throws(() => verifyBody(body.replace('Resumo', 'Resumo alterado'), validNotes()), /sem o título/);
  assert.throws(() => verifyBody('', validNotes()), /sem o título/);
});
