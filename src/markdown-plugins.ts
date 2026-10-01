import type { MarkdownIt, StateBlock, StateCore, Token } from 'markdown-it';
import footnote from 'markdown-it-footnote';

/** First word of a fence's info string, lower-cased (```ts title="x" → ts). */
export function fenceLanguage(token: Token): string {
  return token.info.trim().split(/\s+/)[0].toLowerCase();
}

/** ` data-line="N"` for a block token's source line (synced scroll, #anchor jumps), or ''. */
export function dataLineAttr(token: Token): string {
  return token.map ? ` data-line="${token.map[0]}"` : '';
}

/**
 * YAML frontmatter (`---` … `---` on the first lines), as used by SKILL.md, docs
 * sites and AI config files. Emitted as a `yaml` fence so it is highlighted like
 * any code block and keeps its source-line map for synced scroll.
 */
export function frontmatter(md: MarkdownIt) {
  md.block.ruler.before('table', 'frontmatter', (state: StateBlock, startLine: number, endLine: number, silent: boolean) => {
    if (startLine !== 0 || lineText(state, 0) !== '---') return false;
    let closing = 1;
    while (closing < endLine && !['---', '...'].includes(lineText(state, closing))) closing++;
    if (closing >= endLine) return false;
    if (silent) return true;

    const token = state.push('fence', 'code', 0);
    token.info = 'yaml';
    token.markup = '---';
    token.content = state.getLines(1, closing, 0, true);
    token.map = [0, closing + 1];
    token.attrSet('class', 'frontmatter');
    state.line = closing + 1;
    return true;
  });
}

function lineText(state: StateBlock, line: number): string {
  return state.src.slice(state.bMarks[line] + state.tShift[line], state.eMarks[line]).trimEnd();
}

/**
 * Footnotes (`[^1]` with a `[^1]: …` definition, or inline `^[…]`), listed in a section at
 * the end. Each definition's `<li>` carries the `data-line` of its source, so `[1]` jumps
 * there. Inline footnotes get none: their text sits mid-document, and synced scroll reads
 * `data-line` in document order.
 */
export function footnotes(md: MarkdownIt) {
  md.use(footnote);
  md.core.ruler.after('footnote_tail', 'footnote_lines', (state: StateCore) => {
    const tokens = state.tokens;
    const start = tokens.findIndex((token) => token.type === 'footnote_block_open');
    if (start === -1) return;
    for (let i = start + 1; i < tokens.length - 1; i++) {
      if (tokens[i].type === 'footnote_open') tokens[i].map = tokens[i + 1].map;
    }
    // The section's own line, so the preview can keep it when only lines above it moved.
    tokens[start].map = tokens.slice(start).find((token) => token.map)?.map ?? null;
  });
  const renderItem = md.renderer.rules.footnote_open!;
  md.renderer.rules.footnote_open = (tokens, idx, options, env, self) =>
    renderItem(tokens, idx, options, env, self).replace(/>$/, `${dataLineAttr(tokens[idx])}>`);
}
