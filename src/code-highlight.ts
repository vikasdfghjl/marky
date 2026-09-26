import type { LanguageDescription, LanguageSupport } from '@codemirror/language';
import { classHighlighter, highlightCode } from '@lezer/highlight';

// Highlights fenced code in the preview with the same Lezer parsers the editor uses,
// so both panes agree on colours (tok-* classes map to the editor's --hl-* tokens in
// styles.css). Everything here loads on first use: a document without fenced code
// never pays for it.

let descriptions: readonly LanguageDescription[] | null = null;
const loaded = new Map<string, LanguageSupport | null>();

/** Loads parsers for every fence language in the document; unknown languages stay plain. */
export async function prepareLanguages(names: Iterable<string>) {
  const missing = [...names].map((n) => n.toLowerCase()).filter((n) => !loaded.has(n));
  if (missing.length === 0) return;
  descriptions ??= (await import('@codemirror/language-data')).languages;
  const { LanguageDescription } = await import('@codemirror/language');
  await Promise.all(
    missing.map(async (name) => {
      const description = LanguageDescription.matchLanguageName(descriptions!, name, true);
      loaded.set(name, description ? await description.load() : null);
    }),
  );
}

/** Highlighted HTML for a fence, or '' when its language is unknown or not loaded (markdown-it then escapes it). */
export function highlightFence(code: string, languageName: string): string {
  const support = loaded.get(languageName.toLowerCase());
  if (!support) return '';
  const tree = support.language.parser.parse(code);
  let html = '';
  highlightCode(
    code,
    tree,
    classHighlighter,
    (text, classes) => {
      const escaped = escapeHtml(text);
      html += classes ? `<span class="${classes}">${escaped}</span>` : escaped;
    },
    () => (html += '\n'),
  );
  return html;
}

function escapeHtml(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
