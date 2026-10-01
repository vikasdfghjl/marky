import type { InlineContext, MarkdownConfig } from '@lezer/markdown';
import { Tag, tags as t } from '@lezer/highlight';

/**
 * Editor syntax for the inline extensions the preview renders with markdown-it plugins
 * (preview.worker.ts): ~sub~, ^sup^ and ==marked==. Both panes must agree on what counts as
 * each, so the parsing rules follow those plugins.
 */
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
  const code = char.charCodeAt(0);
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
        parse(cx: InlineContext, next: number, pos: number) {
          if (next !== code || cx.char(pos + 1) === code) return -1;
          const children = [cx.elt(mark, pos, pos + 1)];
          for (let i = pos + 1; i < cx.end; i++) {
            const ch = cx.char(i);
            if (ch === code) {
              if (i === pos + 1) return -1;
              return cx.addElement(cx.elt(node, pos, i + 1, [...children, cx.elt(mark, i, i + 1)]));
            }
            if (ch === 92 /* \ */) children.push(cx.elt('Escape', i, i++ + 2));
            else if (/\s/.test(String.fromCharCode(ch))) return -1;
          }
          return -1;
        },
      },
    ],
  };
}

const PUNCTUATION = /[\p{P}\p{S}]/u;
const EQUALS = 61; // '='
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
