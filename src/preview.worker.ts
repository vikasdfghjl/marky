import MarkdownIt, { type Token } from 'markdown-it';
import { full as emoji } from 'markdown-it-emoji';
import taskLists from 'markdown-it-task-lists';
import { highlightFence, prepareLanguages } from './code-highlight';
import { dataLineAttr, fenceLanguage, frontmatter, githubAlerts, headingAnchors } from './markdown-plugins';
import { mathPlugin, prepareMath, renderMathFence } from './math';
import { URL_SCHEME, joinWindowsPath } from './paths';
import type { RenderedBlock } from './preview-blocks';

export interface RenderRequest {
  id: number;
  text: string;
  /** Folder of the open file, for resolving relative image paths; null for untitled documents. */
  documentFolder: string | null;
  /** URL prefix of Tauri's asset protocol (`http://asset.localhost/` on Windows); empty outside Tauri. */
  assetBase: string;
  /** Smart quotes, dashes, ellipses and (c)/(tm)/(r) symbols (the `typographer` setting). */
  typographer: boolean;
}

export interface RenderResult {
  id: number;
  blocks: RenderedBlock[];
  ms: number;
}

// A type alias (not an interface) so it satisfies markdown-it's index-signature Env.
type RenderEnv = {
  documentFolder: string | null;
  assetBase: string;
};

// html: false renders raw HTML as text, so untrusted files cannot inject markup.
const md = new MarkdownIt({ html: false, linkify: true })
  .use(taskLists, { enabled: false })
  // GitHub's :shortcode: names only; no text smileys, so ":)" or "8)" stay as typed, as on GitHub.
  .use(emoji, { shortcuts: {} })
  .use(frontmatter)
  .use(githubAlerts)
  .use(headingAnchors)
  .use(mathPlugin);
md.set({ highlight: (code, language) => (language ? highlightFence(code, language, md.utils.escapeHtml) : '') });

// Tag block elements with their source line for synced scroll (F10) and #anchor jumps.
md.core.ruler.push('source_lines', (state) => {
  for (const tok of state.tokens) {
    if (tok.map && tok.nesting === 1) tok.attrSet('data-line', String(tok.map[0]));
  }
});

// Fences are rendered as <pre><code>, which has no data-line of its own; tag the <pre>.
const renderDefaultFence = md.renderer.rules.fence!;
md.renderer.rules.fence = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  const math = renderMathFence(token, md.utils.escapeHtml);
  if (math) return math;
  // Mermaid needs the DOM, so the main thread draws it (mermaid-blocks.ts) from this escaped source.
  if (fenceLanguage(token) === 'mermaid') {
    return `<div class="mermaid"${dataLineAttr(token)}>${md.utils.escapeHtml(token.content)}</div>\n`;
  }
  const html = renderDefaultFence(tokens, idx, options, env, self);
  const frontmatterClass = token.attrGet('class') === 'frontmatter' ? ' class="frontmatter"' : '';
  return html.replace(/^<pre/, `<pre${dataLineAttr(token)}${frontmatterClass}`);
};

// Relative image paths load from the document's folder via the asset protocol (F12).
// Remote images stay blocked by the CSP: the preview makes no network requests.
const renderDefaultImage = md.renderer.rules.image!;
md.renderer.rules.image = (tokens, idx, options, env, self) => {
  pointAtLocalAsset(tokens[idx], env as RenderEnv | undefined);
  return renderDefaultImage(tokens, idx, options, env, self);
};

function pointAtLocalAsset(token: Token, env: RenderEnv | undefined) {
  const src = token.attrGet('src');
  if (!src || !env) return;
  const localPath = resolveLocalImage(String(src), env);
  if (!localPath) return;
  token.attrSet('src', env.assetBase + encodeURIComponent(localPath));
}

function resolveLocalImage(src: string, env: RenderEnv): string | null {
  if (!env.documentFolder || !env.assetBase) return null;
  if (URL_SCHEME.test(src) || src.startsWith('/') || src.startsWith('\\') || src.startsWith('#')) return null;
  const withoutQuery = src.split(/[?#]/)[0];
  let relative: string;
  try {
    relative = decodeURIComponent(withoutQuery); // markdown-it percent-encodes spaces etc.
  } catch {
    return null;
  }
  return joinWindowsPath(env.documentFolder, relative);
}

/** Languages whose parsers the fences need; Mermaid is drawn, not highlighted. */
function fenceLanguages(tokens: Token[]): Set<string> {
  const names = new Set<string>();
  for (const token of tokens) {
    const name = token.type === 'fence' ? fenceLanguage(token) : '';
    if (name && name !== 'mermaid' && name !== 'math') names.add(name);
  }
  return names;
}

/**
 * Renders each top-level block on its own, so the page can replace only the blocks an edit
 * changed (preview-blocks.ts). A block is a level-0 token, or a level-0 open token through
 * its matching close.
 */
function renderBlocks(tokens: Token[], env: RenderEnv): RenderedBlock[] {
  const blocks: RenderedBlock[] = [];
  for (let start = 0; start < tokens.length; ) {
    const end = blockEnd(tokens, start);
    const block = tokens.slice(start, end + 1);
    const html = md.renderer.render(block, md.options, env);
    const line = block[0].map?.[0] ?? 0;
    blocks.push({ html, line, signature: signatureOf(html, line) });
    start = end + 1;
  }
  return blocks;
}

function blockEnd(tokens: Token[], start: number): number {
  if (tokens[start].nesting !== 1) return start;
  for (let i = start + 1; i < tokens.length; i++) {
    if (tokens[i].level === 0 && tokens[i].nesting === -1) return i;
  }
  return tokens.length - 1;
}

/** Content identity ignoring position: `data-line` becomes an offset from the block's own line. */
function signatureOf(html: string, line: number): string {
  const relative = html.replace(/ data-line="(\d+)"/g, (_, value) => ` data-line="+${Number(value) - line}"`);
  return `${relative.length}:${hash(relative)}`;
}

/** cyrb53: a fast 53-bit string hash; with the length it makes an accidental clash negligible. */
function hash(text: string): number {
  let h1 = 0xdeadbeef;
  let h2 = 0x41c6ce57;
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    h1 = Math.imul(h1 ^ code, 2654435761);
    h2 = Math.imul(h2 ^ code, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  return 4294967296 * (2097151 & h2) + (h1 >>> 0);
}

self.onmessage = async (e: MessageEvent<RenderRequest>) => {
  const start = performance.now();
  const { id, text, documentFolder, assetBase, typographer } = e.data;
  const env: RenderEnv = { documentFolder, assetBase };
  // Only parsing reads it (the replacements and smartquotes rules), and parsing is synchronous.
  md.set({ typographer });
  // Parse once; load any fence languages (async, first use only), then render synchronously.
  const tokens = md.parse(text, env);
  await Promise.all([prepareLanguages(fenceLanguages(tokens)), prepareMath(tokens)]);
  const result: RenderResult = { id, blocks: renderBlocks(tokens, env), ms: performance.now() - start };
  self.postMessage(result);
};
