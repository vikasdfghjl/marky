import type { InlineContext, MarkdownConfig } from '@lezer/markdown';
import { Tag, tags as t } from '@lezer/highlight';

/**
 * Editor syntax for the inline extensions the preview renders with markdown-it plugins
 * (preview.worker.ts): ~sub~, ^sup^ and ==marked==. Both panes must agree on what counts as
 * each, so the parsing rules follow those plugins.
 */
const BACKSLASH = 92;
const EQUALS = 61;

export const markdownTags = {
  subscript: Tag.define(),
  superscript: Tag.define(),
  marked: Tag.define(),
};

/**
 * `~sub~` / `^sup^`: no unescaped whitespace inside, and `~~` is left to strikethrough.
 * lang-markdown already parses these as Subscript and Superscript, but with one shared tag
 * that a later style rule cannot override, so this runs first and emits nodes of its own.
 */
function singleDelimiter(node: string, char: string, before: string, tag: Tag): MarkdownConfig {
  const delimiter = char.charCodeAt(0);
  const mark = `${node}Mark`;
  return {
    defineNodes: [
      { name: node, style: tag },
      { name: mark, style: t.processingInstruction },
    ],
    parseInline: [
      {
        name: node,
        before,
        parse: (cx, next, pos) => parseSingleDelimited(cx, next, pos, { node, mark, delimiter }),
      },
    ],
  };
}

/** Matches `~text~` or `^text^` at `pos`: non-empty, no unescaped whitespace. Returns the end, or -1. */
function parseSingleDelimited(
  cx: InlineContext,
  next: number,
  pos: number,
  { node, mark, delimiter }: { node: string; mark: string; delimiter: number },
): number {
  if (next !== delimiter || cx.char(pos + 1) === delimiter) return -1;
  const children = [cx.elt(mark, pos, pos + 1)];
  let i = pos + 1;
  while (i < cx.end) {
    const ch = cx.char(i);
    if (ch === delimiter) {
      if (i === pos + 1) return -1; // empty: `~~` is strikethrough's
      return cx.addElement(cx.elt(node, pos, i + 1, [...children, cx.elt(mark, i, i + 1)]));
    }
    if (ch === BACKSLASH) {
      children.push(cx.elt('Escape', i, i + 2));
      i += 2;
      continue;
    }
    if (/\s/.test(String.fromCharCode(ch))) return -1;
    i++;
  }
  return -1;
}

const PUNCTUATION = /[\p{P}\p{S}]/u;
const MARKED_DELIMITER = { resolve: 'Marked', mark: 'MarkedMark' };

/**
 * `==marked==`, matched like ~~strikethrough~~ (and like markdown-it's own delimiter scan):
 * a run of exactly two `=` opens when not followed by whitespace and closes when not preceded
 * by it, so `a == b` stays plain text.
 */
const marked: MarkdownConfig = {
  defineNodes: [
    { name: 'Marked', style: { 'Marked/...': markdownTags.marked } },
    { name: 'MarkedMark', style: t.processingInstruction },
  ],
  parseInline: [
    {
      name: 'Marked',
      after: 'Emphasis',
      parse(cx, next, pos) {
        if (next !== EQUALS || cx.char(pos + 1) !== EQUALS || cx.char(pos + 2) === EQUALS) return -1;
        const before = cx.slice(pos - 1, pos);
        const after = cx.slice(pos + 2, pos + 3);
        const spaceBefore = /\s|^$/.test(before);
        const spaceAfter = /\s|^$/.test(after);
        const punctBefore = PUNCTUATION.test(before);
        const punctAfter = PUNCTUATION.test(after);
        const opens = !spaceAfter && (!punctAfter || spaceBefore || punctBefore);
        const closes = !spaceBefore && (!punctBefore || spaceAfter || punctAfter);
        return cx.addDelimiter(MARKED_DELIMITER, pos, pos + 2, opens, closes);
      },
    },
  ],
};

export const markdownInlineSyntax: MarkdownConfig[] = [
  singleDelimiter('Sub', '~', 'Subscript', markdownTags.subscript),
  singleDelimiter('Sup', '^', 'Superscript', markdownTags.superscript),
  marked,
];
