import type { MarkdownIt, StateBlock, StateInline, Token } from 'markdown-it';
import type katexModule from 'katex';
import { dataLineAttr, fenceLanguage } from './markdown-plugins';

// Math in the preview (PRD F16), with GitHub's syntax: `$…$` inline, `$$…$$` blocks
// and ```math fences. KaTeX renders to a string, so it runs here in the worker and
// loads only for documents that contain math; the main thread lazily adds its CSS.

type Katex = typeof katexModule;
let katex: Katex | null = null;

/** Loads KaTeX if the parsed document contains math. */
export async function prepareMath(tokens: Token[]) {
  if (katex || !containsMath(tokens)) return;
  katex = (await import('katex')).default;
}

function containsMath(tokens: Token[]): boolean {
  return tokens.some(
    (t) => t.type === 'math_block' || (t.type === 'fence' && isMathFence(t)) || (t.children ?? []).some((c) => c.type === 'math_inline'),
  );
}

function isMathFence(token: Token): boolean {
  return fenceLanguage(token) === 'math';
}

/** Display math for both `$$` blocks and "math" fences. */
function mathBlockHtml(token: Token, escapeHtml: (s: string) => string): string {
  return `<div class="math-block"${dataLineAttr(token)}>${renderTex(token.content, true, escapeHtml)}</div>\n`;
}

/** Rendered math, or the escaped source while KaTeX is still loading. */
function renderTex(tex: string, displayMode: boolean, escapeHtml: (s: string) => string): string {
  if (!katex) return escapeHtml(tex);
  return katex.renderToString(tex, {
    displayMode,
    output: 'htmlAndMathml', // MathML for screen readers
    throwOnError: false, // bad TeX shows inline instead of breaking the preview
    errorColor: 'var(--alert-caution)', // a theme token, so errors meet contrast in both themes
    trust: false, // no \href, \url, \includegraphics or \html* from untrusted files
    strict: 'ignore',
    maxSize: 50, // em; stops files from requesting enormous boxes
  });
}

export function mathPlugin(md: MarkdownIt) {
  md.block.ruler.before('fence', 'math_block', mathBlock, { alt: ['paragraph', 'reference', 'blockquote', 'list'] });
  md.inline.ruler.after('escape', 'math_inline', mathInline);

  md.renderer.rules.math_block = (tokens, idx) => mathBlockHtml(tokens[idx], md.utils.escapeHtml);
  md.renderer.rules.math_inline = (tokens, idx) => renderTex(tokens[idx].content, false, md.utils.escapeHtml);
}

/** Renders a ```math fence like a $$ block; returns null for other fences. */
export function renderMathFence(token: Token, escapeHtml: (s: string) => string): string | null {
  if (!isMathFence(token)) return null;
  return mathBlockHtml(token, escapeHtml);
}

/** `$$` on its own line (or `$$ … $$` on one line) up to a closing `$$`. */
function mathBlock(state: StateBlock, startLine: number, endLine: number, silent: boolean): boolean {
  const start = state.bMarks[startLine] + state.tShift[startLine];
  const firstLine = state.src.slice(start, state.eMarks[startLine]).trim();
  if (state.sCount[startLine] - state.blkIndent >= 4 || !firstLine.startsWith('$$')) return false;

  const opening = firstLine.slice(2);
  const oneLiner = opening.trimEnd().endsWith('$$') && opening.trim().length > 2;
  let content: string;
  let lastLine = startLine;

  if (oneLiner) {
    content = opening.trimEnd().slice(0, -2);
  } else {
    let closing = startLine + 1;
    while (closing < endLine && !lineText(state, closing).trimEnd().endsWith('$$')) closing++;
    if (closing >= endLine) return false;
    const closingText = lineText(state, closing).trimEnd();
    content = [opening, state.getLines(startLine + 1, closing, state.tShift[startLine], false), closingText.slice(0, -2)]
      .filter((part) => part.trim() !== '')
      .join('\n');
    lastLine = closing;
  }
  if (silent) return true;

  const token = state.push('math_block', 'math', 0);
  token.block = true;
  token.content = content.trim();
  token.markup = '$$';
  token.map = [startLine, lastLine + 1];
  state.line = lastLine + 1;
  return true;
}

function lineText(state: StateBlock, line: number): string {
  return state.src.slice(state.bMarks[line] + state.tShift[line], state.eMarks[line]);
}

/**
 * `$…$` with pandoc/GitHub rules, so prices stay text: the opening `$` is followed by a
 * non-space, the closing `$` follows a non-space and is not followed by a digit.
 */
function mathInline(state: StateInline, silent: boolean): boolean {
  const { src, pos } = state;
  if (src[pos] !== '$' || src[pos + 1] === '$') return false;
  if (!src[pos + 1] || /\s/.test(src[pos + 1])) return false;

  let end = pos + 1;
  while ((end = src.indexOf('$', end)) !== -1) {
    const escaped = src[end - 1] === '\\';
    const closesCleanly = !/\s/.test(src[end - 1]) && !/\d/.test(src[end + 1] ?? '');
    if (!escaped && closesCleanly) break;
    end++;
  }
  if (end === -1) return false;
  if (!silent) {
    const token = state.push('math_inline', 'math', 0);
    token.markup = '$';
    token.content = src.slice(pos + 1, end);
  }
  state.pos = end + 1;
  return true;
}
