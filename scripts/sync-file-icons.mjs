import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { createRequire } from 'node:module';
import { generateManifest } from 'material-icon-theme';
import * as linguist from 'linguist-languages';

const require = createRequire(import.meta.url);
const source = dirname(require.resolve('material-icon-theme/package.json'));
const out = join(import.meta.dirname, '..', 'src', 'assets', 'material-icons');
const manifest = generateManifest();
const lower = (map = {}) => Object.fromEntries(Object.entries(map).map(([name, icon]) => [name.toLowerCase(), icon]));
const languageIds = lower(manifest.languageIds);
const langNames = {};
const langExtensions = {};
for (const language of Object.values(linguist)) {
  const candidates = [language.name, ...(language.aliases ?? []), language.tmScope?.replace(/^source\./, '')];
  const icons = new Set(candidates.map((name) => name && languageIds[name.toLowerCase()]).filter(Boolean));
  if (icons.size !== 1) continue;
  const icon = [...icons][0];
  for (const name of language.filenames ?? []) langNames[name.toLowerCase()] ??= icon;
  for (const extension of language.extensions ?? []) langExtensions[extension.replace(/^\./, '').toLowerCase()] ??= icon;
}
const maps = {
  fileNames: lower(manifest.fileNames), fileExtensions: lower(manifest.fileExtensions),
  folderNames: lower(manifest.folderNames), folderNamesExpanded: lower(manifest.folderNamesExpanded),
  langNames, langExtensions,
};
for (const map of Object.values(maps)) for (const [name, icon] of Object.entries(map)) {
  if (!manifest.iconDefinitions[icon]) delete map[name];
}
const keys = new Set([manifest.file, manifest.folder, manifest.folderExpanded, ...Object.values(maps).flatMap(Object.values)]);
const icons = Object.fromEntries([...keys].map((key) => {
  const path = manifest.iconDefinitions[key]?.iconPath;
  if (!path) throw new Error(`Ícone sem definição: ${key}`);
  return [key, path.split('/').pop()];
}));
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'icons'), { recursive: true });
for (const filename of new Set(Object.values(icons))) cpSync(join(source, 'icons', filename), join(out, 'icons', filename));
cpSync(join(source, 'LICENSE'), join(out, 'LICENSE'));
cpSync(join(source, 'LICENSE'), join(import.meta.dirname, '..', 'public', 'material-icons-LICENSE.txt'));
writeFileSync(join(out, 'theme.json'), `${JSON.stringify({ file: manifest.file, folder: manifest.folder, folderExpanded: manifest.folderExpanded, icons, ...maps })}\n`);
console.log(`Material Icon Theme ${JSON.parse(readFileSync(join(source, 'package.json'))).version}: ${Object.keys(icons).length} SVGs`);
