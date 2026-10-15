// Copia o tema de ícones Symbols (o padrão do Antigravity/VS Code, MIT) para src/assets/symbols e gera o theme.json que o app lê.
// O languageId do tema vira extensão e nome de arquivo pelas linguagens que as extensões da IDE declaram, porque o app não tem o
// registro de linguagens do VS Code. Uso, com a pasta `extensions` da IDE instalada:
//   node scripts/sync-file-icons.mjs "C:\Users\<você>\AppData\Local\Programs\Antigravity IDE\resources\app\extensions"
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const extensions = process.argv[2];
if (!extensions || !existsSync(join(extensions, 'theme-symbols'))) {
  console.error('Passe a pasta `extensions` da IDE (a que tem theme-symbols).');
  process.exit(1);
}
const source = join(extensions, 'theme-symbols');
const theme = JSON.parse(readFileSync(join(source, 'src', 'symbol-icon-theme.json'), 'utf8'));
const out = join(import.meta.dirname, '..', 'src', 'assets', 'symbols');

// Linguagens declaradas pelas extensões: id -> extensões e nomes de arquivo. Os do tema ganham dessas.
const langExtensions = {};
const langNames = {};
for (const dir of readdirSync(extensions)) {
  const manifest = join(extensions, dir, 'package.json');
  if (!existsSync(manifest)) continue;
  for (const lang of JSON.parse(readFileSync(manifest, 'utf8')).contributes?.languages ?? []) {
    const icon = theme.languageIds[lang.id];
    if (!icon) continue;
    for (const ext of lang.extensions ?? []) langExtensions[ext.replace(/^\./, '').toLowerCase()] ??= icon;
    for (const name of lang.filenames ?? []) langNames[name.toLowerCase()] ??= icon;
  }
}

const lower = (map) => Object.fromEntries(Object.entries(map ?? {}).map(([k, v]) => [k.toLowerCase(), v]));
const icons = Object.fromEntries(Object.entries(theme.iconDefinitions).map(([key, def]) => [key, def.iconPath.replace(/^\.\/icons\//, '')]));

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
cpSync(join(source, 'src', 'icons'), join(out, 'icons'), { recursive: true });
cpSync(join(source, 'LICENSE'), join(out, 'LICENSE'));
writeFileSync(join(out, 'theme.json'), `${JSON.stringify({
  file: theme.file,
  folder: theme.folder,
  icons,
  fileNames: lower(theme.fileNames),
  fileExtensions: lower(theme.fileExtensions),
  langNames,
  langExtensions,
  folderNames: lower(theme.folderNames),
})}\n`);
console.log(`${Object.keys(icons).length} ícones; ${Object.keys(langExtensions).length} extensões e ${Object.keys(langNames).length} nomes vindos das linguagens.`);
