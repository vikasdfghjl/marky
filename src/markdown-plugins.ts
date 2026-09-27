import type { MarkdownIt, StateBlock, StateCore, Token } from 'markdown-it';

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

const ALERT_TITLES = { NOTE: 'Note', TIP: 'Tip', IMPORTANT: 'Important', WARNING: 'Warning', CAUTION: 'Caution' } as const;
const ALERT_MARKER = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*(?:\n|$)/i;

/** GitHub alerts: a blockquote whose first line is `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` or `[!CAUTION]`. */
export function githubAlerts(md: MarkdownIt) {
  md.core.ruler.after('inline', 'github_alerts', (state: StateCore) => {
    const tokens = state.tokens;
    for (let i = 0; i < tokens.length - 2; i++) {
      if (tokens[i].type !== 'blockquote_open' || tokens[i + 1].type !== 'paragraph_open') continue;
      const inline = tokens[i + 2];
      const match = inline.content.match(ALERT_MARKER);
      if (!match) continue;

      const kind = match[1].toUpperCase() as keyof typeof ALERT_TITLES;
      tokens[i].attrSet('class', `markdown-alert markdown-alert-${kind.toLowerCase()}`);
      stripAlertMarker(inline, match[0]);

      // Our own markup, not user HTML: html_block tokens render verbatim even with html: false.
      const title = new state.Token('html_block', '', 0);
      title.content = `<p class="markdown-alert-title">${ALERT_TITLES[kind]}</p>\n`;
      tokens.splice(i + 1, 0, title);
    }
  });
}

/** Removes `[!NOTE]` and the line break after it from a paragraph's inline tokens. */
function stripAlertMarker(inline: Token, marker: string) {
  inline.content = inline.content.slice(marker.length);
  const children = inline.children ?? [];
  const first = children[0];
  if (first?.type === 'text') first.content = first.content.replace(/^\[![A-Za-z]+\][ \t]*/, '');
  if (first?.type === 'text' && first.content === '') children.shift();
  if (children[0]?.type === 'softbreak' || children[0]?.type === 'hardbreak') children.shift();
}

/**
 * GitHub-style heading ids (`## Getting Started` → `getting-started`, duplicates get
 * `-1`, `-2`), so `#links` inside the document resolve like they do on GitHub.
 */
export function headingAnchors(md: MarkdownIt) {
  md.core.ruler.after('inline', 'heading_anchors', (state: StateCore) => {
    const used = new Map<string, number>();
    const tokens = state.tokens;
    for (let i = 0; i < tokens.length - 1; i++) {
      if (tokens[i].type !== 'heading_open') continue;
      const base = githubSlug(inlineText(tokens[i + 1]));
      const count = used.get(base) ?? 0;
      used.set(base, count + 1);
      tokens[i].attrSet('id', count ? `${base}-${count}` : base);
    }
  });
}

/** Plain text of a heading, the way GitHub computes it: link text and code included, markup dropped. */
function inlineText(inline: Token): string {
  return (inline.children ?? [])
    .filter((c) => c.type === 'text' || c.type === 'code_inline')
    .map((c) => c.content)
    .join('');
}

export function githubSlug(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M}\s_-]/gu, '')
    .replace(/\s/g, '-');
}
