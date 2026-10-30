import { Facet, StateEffect, StateField, type EditorState } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, type DecorationSet } from '@codemirror/view';
import { LanguageDescription } from '@codemirror/language';
import { languages } from '@codemirror/language-data';
import { bundledLanguages, bundledLanguagesInfo } from 'shiki/langs';
import * as linguist from 'linguist-languages';
import type { Language } from 'linguist-languages';
import type { createHighlighter } from 'shiki';

const LIMIT = 200_000;
const names = new Map<string, string>();
for (const info of bundledLanguagesInfo) {
  for (const name of [info.id, info.name, ...(info.aliases ?? [])]) names.set(name.toLowerCase(), info.id);
}
const filenames = new Map<string, string | null>();
const extensions = new Map<string, string | null>();
for (const language of Object.values(linguist) as Language[]) {
  const candidates = [language.name, ...(language.aliases ?? []), language.tmScope?.replace(/^source\./, '')];
  const lang = candidates.map((candidate) => candidate && names.get(candidate.toLowerCase())).find(Boolean);
  if (!lang) continue;
  for (const filename of language.filenames ?? []) {
    const key = filename.toLowerCase();
    filenames.set(key, filenames.has(key) && filenames.get(key) !== lang ? null : lang);
  }
  for (const extension of language.extensions ?? []) {
    const key = extension.toLowerCase();
    extensions.set(key, extensions.has(key) && extensions.get(key) !== lang ? null : lang);
  }
}
const fileName = (path: string) => path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);

export function shikiLanguage(path: string): string | null {
  const name = fileName(path);
  if (/^\.env(?:\..+)?$|\.env$/i.test(name)) return 'dotenv';
  const lower = name.toLowerCase();
  const filenameLanguage = filenames.get(lower);
  if (filenameLanguage) return filenameLanguage;
  for (let dot = lower.indexOf('.'); dot >= 0; dot = lower.indexOf('.', dot + 1)) {
    const extension = lower.slice(dot);
    const language = extensions.get(extension);
    if (language) return language;
  }
  const description = LanguageDescription.matchFilename(languages, name);
  if (description) {
    for (const candidate of [description.name, ...description.alias]) {
      const found = names.get(candidate.toLowerCase());
      if (found) return found;
    }
  }
  return null;
}

let highlighter: ReturnType<typeof createHighlighter> | undefined;
let readyHighlighter: Awaited<ReturnType<typeof createHighlighter>> | undefined;
const ready = new Set<string>();
const loaded = new Map<string, Promise<void>>();
function load(lang: string) {
  let pending = loaded.get(lang);
  if (!pending) {
    highlighter ??= import('shiki').then(({ createHighlighter }) => createHighlighter({ themes: ['dark-plus'], langs: [] }));
    pending = highlighter.then(async (h) => { await h.loadLanguage(lang as keyof typeof bundledLanguages); readyHighlighter = h; ready.add(lang); });
    loaded.set(lang, pending);
    pending.catch(() => { if (loaded.get(lang) === pending) loaded.delete(lang); });
  }
  return pending;
}

export function preloadShiki(path: string) {
  const lang = shikiLanguage(path);
  return lang ? load(lang).catch(() => {}) : Promise.resolve();
}

const setColors = StateEffect.define<DecorationSet>();
export const shikiLanguageFacet = Facet.define<string | null, string | null>({ combine: (values) => values[0] ?? null });
export function shikiTokens(state: EditorState, text: string) {
  const lang = state.facet(shikiLanguageFacet);
  try {
    return lang && ready.has(lang) ? readyHighlighter!.codeToTokensBase(text, { lang: lang as keyof typeof bundledLanguages, theme: 'dark-plus' }) : [];
  } catch { return []; }
}
function tokenize(text: string, lang: string): DecorationSet {
  const lines = readyHighlighter!.codeToTokensBase(text, { lang: lang as keyof typeof bundledLanguages, theme: 'dark-plus' });
  const marks: { from: number; to: number; value: Decoration }[] = [];
  for (const line of lines) for (const token of line) {
    if (!token.color || !token.content.length) continue;
    marks.push({ from: token.offset, to: token.offset + token.content.length,
      value: Decoration.mark({ attributes: { style: `color: ${token.color}${(token.fontStyle ?? 0) & 1 ? '; font-style: italic' : ''}${(token.fontStyle ?? 0) & 2 ? '; font-weight: bold' : ''}${(token.fontStyle ?? 0) & 4 ? '; text-decoration: underline' : ''}` } }) });
  }
  return Decoration.set(marks, true);
}

const colors = (lang: string | null) => StateField.define<DecorationSet>({
  create: (state: EditorState) => {
    try { return lang && ready.has(lang) && state.doc.length <= LIMIT ? tokenize(state.doc.toString(), lang) : Decoration.none; }
    catch { return Decoration.none; }
  },
  update(value, transaction) {
    if (transaction.docChanged) value = Decoration.none;
    for (const effect of transaction.effects) if (effect.is(setColors)) value = effect.value;
    return value;
  },
  provide: (field) => EditorView.decorations.from(field),
});

export function shikiColors(path: string) {
  const lang = shikiLanguage(path);
  return [shikiLanguageFacet.of(lang), colors(lang), ViewPlugin.fromClass(class {
    private timer: ReturnType<typeof setTimeout> | undefined;
    private generation = 0;
    private alive = true;
    constructor(private view: EditorView) { this.schedule(); }
    update(update: { docChanged: boolean }) { if (update.docChanged) this.schedule(); }
    destroy() { this.alive = false; this.generation++; clearTimeout(this.timer); }
    private schedule() {
      const generation = ++this.generation;
      clearTimeout(this.timer);
      if (!lang || this.view.state.doc.length > LIMIT) return;
      this.timer = setTimeout(async () => {
        const text = this.view.state.doc.toString();
        try {
          await load(lang);
          if (!this.alive || generation !== this.generation) return;
          this.view.dispatch({ effects: setColors.of(tokenize(text, lang)) });
        } catch { /* Arquivo permanece legível quando a gramática falha. */ }
      }, 30);
    }
  })];
}
