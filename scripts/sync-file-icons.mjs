// Copia o tema de ícones Symbols (o padrão do Antigravity/VS Code, MIT) para src/assets/symbols e gera o theme.json que o app lê.
// Vão só os ícones que o app alcança, porque o bundle leva todo arquivo da pasta icons.
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
const maps = { fileNames: lower(theme.fileNames), fileExtensions: lower(theme.fileExtensions), langNames, langExtensions, folderNames: lower(theme.folderNames) };
// Associação para ícone sem definição sai, como no VS Code: vale a regra seguinte.
for (const map of Object.values(maps)) for (const [k, v] of Object.entries(map)) if (!theme.iconDefinitions[v]) delete map[k];
// As chaves que src/shared/file-icons.ts pode devolver: os valores dos mapas e os padrões.
const reachable = new Set([theme.file, theme.folder, ...Object.values(maps).flatMap(Object.values)]);
const icons = Object.fromEntries(Object.entries(theme.iconDefinitions)
  .filter(([key]) => reachable.has(key))
  .map(([key, def]) => [key, def.iconPath.replace(/^\.\/icons\//, '').trimEnd()]));

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
// Alguns caminhos do tema terminam com espaço: o arquivo não tem, e o espaço sai do valor gravado.
for (const path of new Set(Object.values(icons))) cpSync(join(source, 'src', 'icons', path), join(out, 'icons', path));
cpSync(join(source, 'LICENSE'), join(out, 'LICENSE'));
writeFileSync(join(out, 'theme.json'), `${JSON.stringify({ file: theme.file, folder: theme.folder, icons, ...maps })}\n`);
console.log(`${Object.keys(icons).length} de ${Object.keys(theme.iconDefinitions).length} ícones alcançáveis; ${Object.keys(langExtensions).length} extensões e ${Object.keys(langNames).length} nomes vindos das linguagens.`);
