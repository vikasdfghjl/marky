import { parser } from '@lezer/html';
import { LanguageSupport, LRLanguage } from '@codemirror/language';

// Stands in for @codemirror/lang-html inside @codemirror/lang-markdown only (see vite.config.ts).
// Markdown uses it to highlight inline HTML tags; the real package also parses <script> and
// <style> contents, which pulls the JavaScript and CSS parsers (~130 KB) into the startup bundle.
// ```html fences still load the full package lazily through language-data.

const htmlTagLanguage = LRLanguage.define({
  name: 'html',
  parser,
  languageData: { commentTokens: { block: { open: '<!--', close: '-->' } } },
});

export function html(): LanguageSupport {
  return new LanguageSupport(htmlTagLanguage);
}

/** Tag completion is off (`completeHTMLTags: false` in editor.ts), so this is never called. */
export function htmlCompletionSource(): null {
  return null;
}
