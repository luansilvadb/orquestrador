// Regras do fluxo de release: versões iguais nos manifests, notas completas e o latest.json que o app lê.
// Só usa módulos nativos do Node. Uso: node scripts/release.mjs <comando> ... (detalhes em docs/releases/README.md).
import { createHash, createPublicKey, verify } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { basename } from 'node:path';
import { fileURLToPath } from 'node:url';

export const REPO = 'luansilvadb/orquestrador';
export const REQUIRED_SECTIONS = [
  'Resumo',
  'Novidades',
  'Melhorias e correções',
  'Impacto da atualização',
  'Requisitos e limitações',
  'Instalação',
  'Comparação com a versão anterior',
];
// Texto de modelo ou de release automática: nunca pode chegar à release.
const PLACEHOLDER = /\b(TODO|TBD|XXX)\b|\{\{|<!--|Instalador para Windows, por usuário\. O app se atualiza sozinho ao abrir\./;
const MIN_NOTES_CHARS = 1500;
const MIN_SECTION_CHARS = 60;
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

export function checkSemver(version) {
  if (!SEMVER.test(version)) throw new Error(`"${version}" não é uma versão X.Y.Z.`);
  return version;
}

// Estado de uma versão: `version` do Cargo.toml é a do pacote `[package]`, não a de dependências.
export function cargoPackageVersion(text) {
  const lines = text.split(/\r?\n/);
  const start = lines.indexOf('[package]');
  if (start < 0) return undefined;
  for (const line of lines.slice(start + 1)) {
    if (line.startsWith('[')) break;
    const m = /^version\s*=\s*"([^"]+)"/.exec(line);
    if (m) return m[1];
  }
  return undefined;
}

// Todas as entradas `name = ...` com o nome pedido no Cargo.lock (deve haver uma só).
export function cargoLockVersions(text, name) {
  const found = [];
  for (const block of text.split('[[package]]')) {
    const m = /^\s*name = "([^"]+)"\r?\nversion = "([^"]+)"/.exec(block);
    if (m && m[1] === name) found.push(m[2]);
  }
  return found;
}

export function manifestVersions(root) {
  const read = (path) => readFileSync(`${root}/${path}`, 'utf8');
  const pkg = JSON.parse(read('package.json'));
  const lock = JSON.parse(read('package-lock.json'));
  const conf = JSON.parse(read('src-tauri/tauri.conf.json'));
  const lockEntries = cargoLockVersions(read('src-tauri/Cargo.lock'), 'orquestrador');
  return {
    'package.json': pkg.version,
    'package-lock.json': lock.version,
    'package-lock.json (packages[""])': lock.packages?.['']?.version,
    'src-tauri/tauri.conf.json': conf.version,
    'src-tauri/Cargo.toml': cargoPackageVersion(read('src-tauri/Cargo.toml')),
    'src-tauri/Cargo.lock': lockEntries.length === 1 ? lockEntries[0] : `${lockEntries.length} entradas`,
  };
}

export function checkVersions(versions, expected) {
  checkSemver(expected);
  const wrong = Object.entries(versions).filter(([, found]) => found !== expected);
  if (wrong.length > 0) {
    const lines = wrong.map(([file, found]) => `  ${file}: ${found ?? 'ausente'}`).join('\n');
    throw new Error(`A versão ${expected} não bate com:\n${lines}`);
  }
  return expected;
}

// Compara X.Y.Z numericamente: a nova versão precisa ser maior que a publicada.
export function isNewer(version, previous) {
  const a = version.split('.').map(Number);
  const b = previous.split('.').map(Number);
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] > b[i];
  }
  return false;
}

// Devolve a lista de problemas; vazia quer dizer que as notas servem.
export function noteProblems(text, version, previous) {
  const body = text.trim();
  const problems = [];
  if (body.split('\n')[0].trim() !== `# Orquestrador v${version}`) {
    problems.push(`a primeira linha deve ser "# Orquestrador v${version}"`);
  }
  if (body.length < MIN_NOTES_CHARS) {
    problems.push(`notas curtas demais (${body.length} caracteres; mínimo ${MIN_NOTES_CHARS})`);
  }
  const placeholder = PLACEHOLDER.exec(body);
  if (placeholder) problems.push(`texto de modelo ou genérico: "${placeholder[0]}"`);

  const sections = body.split(/^## /m).slice(1).map((part) => {
    const end = part.indexOf('\n');
    return { title: part.slice(0, end).trim(), text: part.slice(end + 1).trim() };
  });
  for (const required of REQUIRED_SECTIONS) {
    const section = sections.find((s) => s.title.startsWith(required));
    if (!section) {
      problems.push(`falta a seção "## ${required}"`);
    } else if (section.text.length < MIN_SECTION_CHARS) {
      problems.push(`a seção "## ${required}" está vazia ou curta demais`);
    } else if (required.startsWith('Comparação') && previous && !section.text.includes(`v${previous}`)) {
      problems.push(`a comparação não cita v${previous}`);
    }
  }
  return problems;
}

export function checkNotes(text, version, previous) {
  const problems = noteProblems(text, version, previous);
  if (problems.length > 0) throw new Error(`Notas da ${version} recusadas:\n  - ${problems.join('\n  - ')}`);
  return text.trim();
}

export function installerName(version) {
  return `Orquestrador_${version}_x64-setup.exe`;
}

export function installerUrl(version, repo = REPO) {
  return `https://github.com/${repo}/releases/download/v${version}/${installerName(version)}`;
}

// Troca a URL da API do asset (rascunho) pela pública e põe as notas completas no campo `notes`.
export function prepareManifest(text, version, notes, repo = REPO) {
  const manifest = JSON.parse(text);
  if (manifest.version !== version) throw new Error(`latest.json anuncia ${manifest.version}, não ${version}.`);
  const keys = Object.keys(manifest.platforms ?? {}).filter((key) => key.startsWith('windows-x86_64'));
  if (keys.length === 0) throw new Error('latest.json sem plataforma windows-x86_64.');
  for (const key of keys) manifest.platforms[key].url = installerUrl(version, repo);
  manifest.notes = canonicalText(notes.trim());
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

// O sig é o arquivo de assinatura do Tauri (base64 do .sig do minisign). O plugin lê `version:` como campo separado por tab.
export function signedVersion(sigText) {
  const decoded = Buffer.from(sigText.trim(), 'base64').toString('utf8');
  const line = decoded.split(/\r?\n/).find((l) => l.startsWith('trusted comment: '));
  if (!line) throw new Error('assinatura sem o comentário confiável.');
  const field = line.slice('trusted comment: '.length).split('\t').find((f) => f.startsWith('version:'));
  return field?.slice('version:'.length);
}

export function verifyManifest(text, version, sigText, notes, repo = REPO) {
  const manifest = JSON.parse(text);
  const problems = [];
  if (manifest.version !== version) problems.push(`anuncia ${manifest.version}, não ${version}`);
  const platform = manifest.platforms?.['windows-x86_64'];
  if (!platform) problems.push('sem windows-x86_64');
  else {
    if (platform.url !== installerUrl(version, repo)) problems.push(`URL ${platform.url}, esperada ${installerUrl(version, repo)}`);
    if (platform.signature !== sigText.trim()) problems.push('a assinatura não é a do .sig enviado');
  }
  const signed = signedVersion(sigText);
  if (signed !== version) problems.push(`a versão assinada é ${signed ?? 'ausente'}, não ${version}`);
  if (notes !== undefined && canonicalText(manifest.notes ?? '') !== canonicalText(notes)) problems.push('notes do latest.json não são o texto completo de docs/releases');
  if (problems.length > 0) throw new Error(`latest.json recusado:\n  - ${problems.join('\n  - ')}`);
  return `latest.json de ${version}: URL, assinatura, versão assinada e notes conferidos.`;
}

// Assinatura Minisign, como o Tauri grava (formato: https://jedisct1.github.io/minisign/). Só verificação, com Ed25519 e BLAKE2b do Node.
// Chave pública (pubkey do tauri.conf.json) = base64 de "untrusted comment: …\n" + base64(alg "Ed" + key id 8 bytes + chave 32 bytes).
// .sig = base64 de: "untrusted comment: …", base64(alg + key id + assinatura 64), "trusted comment: …" e base64(assinatura global 64).
// Alg "ED" assina o BLAKE2b-512 do arquivo (o modo que o Tauri usa); "Ed" assina o arquivo direto. A global cobre a assinatura
// do arquivo seguida do texto do comentário confiável, com a mesma chave.
const ED25519_SPKI_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');

export function parseMinisignKey(keyB64) {
  const text = Buffer.from(keyB64.trim(), 'base64').toString('utf8');
  const raw = Buffer.from(text.split(/\r?\n/)[1] ?? '', 'base64');
  if (raw.length !== 42 || raw.subarray(0, 2).toString() !== 'Ed') throw new Error('chave pública do updater não é Minisign Ed25519.');
  return { keyId: raw.subarray(2, 10), publicKey: raw.subarray(10, 42) };
}

export function parseMinisignSig(sigText) {
  const text = Buffer.from(sigText.trim(), 'base64').toString('utf8');
  const [comment, sigLine, trusted, globalLine] = text.split(/\r?\n/);
  if (!comment?.startsWith('untrusted comment: ') || !trusted?.startsWith('trusted comment: ')) {
    throw new Error('assinatura sem os comentários do formato Minisign.');
  }
  const sig = Buffer.from(sigLine ?? '', 'base64');
  const global = Buffer.from(globalLine ?? '', 'base64');
  if (sig.length !== 74 || global.length !== 64) throw new Error('assinatura Minisign com tamanho inválido.');
  return {
    algorithm: sig.subarray(0, 2).toString(),
    keyId: sig.subarray(2, 10),
    signature: sig.subarray(10, 74),
    global,
    trusted: trusted.slice('trusted comment: '.length),
  };
}

/** Confere o arquivo contra a chave: key id, assinatura do payload e assinatura global do comentário confiável. Devolve os campos do comentário. */
export function verifyMinisign(keyB64, sigText, file) {
  const key = parseMinisignKey(keyB64);
  const sig = parseMinisignSig(sigText);
  if (!sig.keyId.equals(key.keyId)) throw new Error('assinatura de outra chave: o key id não é o da pubkey do tauri.conf.json.');
  const message = sig.algorithm === 'ED' ? createHash('blake2b512').update(file).digest()
    : sig.algorithm === 'Ed' ? file
      : null;
  if (!message) throw new Error(`algoritmo de assinatura desconhecido: ${sig.algorithm}.`);
  const publicKey = createPublicKey({ key: Buffer.concat([ED25519_SPKI_PREFIX, key.publicKey]), format: 'der', type: 'spki' });
  if (!verify(null, message, publicKey, sig.signature)) throw new Error('assinatura do instalador não confere com a chave pública.');
  if (!verify(null, Buffer.concat([sig.signature, Buffer.from(sig.trusted, 'utf8')]), publicKey, sig.global)) {
    throw new Error('comentário confiável alterado: a assinatura global não confere.');
  }
  return Object.fromEntries(sig.trusted.split('\t').map((field) => field.split(/:(.*)/s).slice(0, 2)));
}

/** Instalador de `version` (nome `fileName`, conteúdo `file`) assinado pela chave pública, com o comentário confiável dizendo o mesmo arquivo e a mesma versão. */
export function verifyInstaller(version, fileName, file, sigText, keyB64) {
  checkSemver(version);
  if (fileName !== installerName(version)) throw new Error(`o instalador se chama ${fileName}, não ${installerName(version)}.`);
  const fields = verifyMinisign(keyB64, sigText, file);
  if (fields.file !== fileName) throw new Error(`o comentário confiável diz o arquivo ${fields.file}, não ${fileName}.`);
  if (fields.version !== version) throw new Error(`a versão assinada é ${fields.version ?? 'ausente'}, não ${version}.`);
  return `${fileName}: assinatura Minisign, key id e comentário confiável conferidos (versão ${version}).`;
}

// Só diferença de representação é ignorada: CRLF x LF (checkout no Windows x armazenamento do GitHub) e espaço final.
// Vale para o corpo da release e para o `notes` do latest.json; o conteúdo continua exato.
export function canonicalText(s) {
  return s.replace(/\r\n/g, '\n').replace(/\s+$/, '');
}

// Corpo da release: o GitHub já mostra o nome "Orquestrador vX.Y.Z", então o H1 da fonte não entra. Começa em `## Resumo`.
// Fonte (docs/releases) continua com H1 para identificar a versão; o `notes` do latest.json também leva o H1 como antes.
export function releaseBody(notes) {
  return canonicalText(notes).replace(/^\s*# [^\n]*\n/, '').replace(/^\s+/, '');
}

// O corpo da release tem de ser o texto de docs/releases sem o título.
export function verifyBody(body, notes) {
  if (canonicalText(body ?? '') !== releaseBody(notes)) throw new Error('o corpo da release não é o texto completo de docs/releases, sem o título.');
  return 'corpo da release igual às notas, sem o título.';
}

function run(argv) {
  const [command, ...args] = argv;
  const root = fileURLToPath(new URL('..', import.meta.url)).replace(/[\\/]$/, '');
  const read = (path) => readFileSync(path, 'utf8');
  switch (command) {
    case 'check-versions': {
      const expected = args[0] ?? manifestVersions(root)['package.json'];
      checkVersions(manifestVersions(root), expected);
      return `Versões ${expected} sincronizadas em todos os manifests.`;
    }
    case 'check-notes': {
      const [version, previous] = args;
      const path = `${root}/docs/releases/v${version}.md`;
      if (!existsSync(path)) throw new Error(`Notas ausentes: ${path} não existe. A release não sai sem elas.`);
      checkNotes(read(path), version, previous);
      return `Notas da v${version} completas.`;
    }
    case 'prepare-manifest': {
      const [version, file, notesFile] = args;
      writeFileSync(file, prepareManifest(read(file), version, read(notesFile)));
      return `latest.json de ${version} apontando para a URL pública, com as notas.`;
    }
    case 'verify-manifest': {
      const [version, file, sigFile, notesFile] = args;
      return verifyManifest(read(file), version, read(sigFile), notesFile ? read(notesFile) : undefined);
    }
    case 'verify-installer': {
      const [version, exe, sigFile] = args;
      const keyB64 = JSON.parse(read(`${root}/src-tauri/tauri.conf.json`)).plugins.updater.pubkey;
      return verifyInstaller(version, basename(exe), readFileSync(exe), read(sigFile), keyB64);
    }
    case 'release-body': {
      const [version, out] = args;
      checkSemver(version);
      writeFileSync(out, releaseBody(read(`${root}/docs/releases/v${version}.md`)), 'utf8');
      return `Corpo da release ${version} gravado em ${out}, sem o título.`;
    }
    case 'verify-body': {
      const [bodyFile, notesFile] = args;
      return verifyBody(read(bodyFile), read(notesFile));
    }
    default:
      throw new Error('comandos: check-versions [X.Y.Z], check-notes X.Y.Z [anterior], prepare-manifest X.Y.Z latest.json notas.md, verify-manifest X.Y.Z latest.json arquivo.sig [notas.md], verify-installer X.Y.Z instalador.exe arquivo.sig, release-body X.Y.Z corpo.md, verify-body corpo.md notas.md');
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    console.log(run(process.argv.slice(2)));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
