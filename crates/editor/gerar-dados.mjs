// Gera `assets/linguagens.json` e `assets/dark-plus.json` a partir dos mesmos pacotes que o React usa (Linguist, Shiki e
// @codemirror/language-data), para a detecção de linguagem e as cores baterem. Rodar da raiz: `node crates/editor/gerar-dados.mjs`.
import { writeFileSync } from 'node:fs';
import { bundledLanguagesInfo } from 'shiki/langs';
import * as linguist from 'linguist-languages';
import { languages } from '@codemirror/language-data';
import darkPlus from 'shiki/themes/dark-plus.mjs';

const names = {};
for (const info of bundledLanguagesInfo) for (const n of [info.id, info.name, ...(info.aliases ?? [])]) names[n.toLowerCase()] = info.id;
const filenames = new Map();
const extensions = new Map();
for (const language of Object.values(linguist)) {
  const candidates = [language.name, ...(language.aliases ?? []), language.tmScope?.replace(/^source\./, '')];
  const lang = candidates.map((c) => c && names[c.toLowerCase()]).find(Boolean);
  if (!lang) continue;
  for (const f of language.filenames ?? []) { const k = f.toLowerCase(); filenames.set(k, filenames.has(k) && filenames.get(k) !== lang ? null : lang); }
  for (const e of language.extensions ?? []) { const k = e.toLowerCase(); extensions.set(k, extensions.has(k) && extensions.get(k) !== lang ? null : lang); }
}
const codemirror = languages.map((d) => ({ name: d.name, alias: d.alias, extensions: d.extensions, filename: d.filename ? d.filename.source : null, flags: d.filename ? d.filename.flags : "" }));
const shiki = bundledLanguagesInfo.map((i) => ({ id: i.id, name: i.name, aliases: i.aliases ?? [] }));
writeFileSync(new URL('./assets/linguagens.json', import.meta.url), JSON.stringify({
  names, filenames: Object.fromEntries(filenames), extensions: Object.fromEntries(extensions), codemirror, shiki,
}));
const theme = darkPlus.default ?? darkPlus;
writeFileSync(new URL('./assets/dark-plus.json', import.meta.url), JSON.stringify({ colors: theme.colors, tokenColors: theme.tokenColors }, null, 1));
