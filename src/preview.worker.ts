import MarkdownIt, { type Token } from 'markdown-it';
import taskLists from 'markdown-it-task-lists';
import { highlightFence, prepareLanguages } from './code-highlight';
import { frontmatter, githubAlerts, headingAnchors } from './markdown-plugins';

export interface RenderRequest {
  id: number;
  text: string;
  /** Folder of the open file, for resolving relative image paths; null for untitled documents. */
  documentFolder: string | null;
  /** URL prefix of Tauri's asset protocol (`http://asset.localhost/` on Windows); empty outside Tauri. */
  assetBase: string;
}

export interface RenderResult {
  id: number;
  html: string;
  ms: number;
}

// A type alias (not an interface) so it satisfies markdown-it's index-signature Env.
type RenderEnv = {
  documentFolder: string | null;
  assetBase: string;
};

// html: false renders raw HTML as text, so untrusted files cannot inject markup.
const md = new MarkdownIt({
  html: false,
  linkify: true,
  highlight: (code, language) => (language ? highlightFence(code, language) : ''),
})
  .use(taskLists, { enabled: false })
  .use(frontmatter)
  .use(githubAlerts)
  .use(headingAnchors);

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
  const html = renderDefaultFence(tokens, idx, options, env, self);
  const line = token.map ? ` data-line="${token.map[0]}"` : '';
  const frontmatterClass = token.attrGet('class') === 'frontmatter' ? ' class="frontmatter"' : '';
  return html.replace(/^<pre/, `<pre${line}${frontmatterClass}`);
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

const URL_SCHEME = /^[a-z][a-z0-9+.-]*:/i;

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

/** Joins and normalises `.`/`..` segments; never climbs above the drive or share root. */
function joinWindowsPath(folder: string, relative: string): string {
  const parts = folder.split(/[\\/]+/).filter(Boolean);
  const rootLength = folder.startsWith('\\\\') ? 2 : 1; // \\server\share or C:
  for (const segment of relative.split(/[\\/]+/)) {
    if (segment === '' || segment === '.') continue;
    if (segment === '..') {
      if (parts.length > rootLength) parts.pop();
      continue;
    }
    parts.push(segment);
  }
  const prefix = folder.startsWith('\\\\') ? '\\\\' : '';
  return prefix + parts.join('\\');
}

/** First word of each fence's info string (```ts title="x" → ts). */
function fenceLanguages(tokens: Token[]): Set<string> {
  const names = new Set<string>();
  for (const token of tokens) {
    const name = token.type === 'fence' ? token.info.trim().split(/\s+/)[0] : '';
    if (name) names.add(name);
  }
  return names;
}

self.onmessage = async (e: MessageEvent<RenderRequest>) => {
  const start = performance.now();
  const { id, text, documentFolder, assetBase } = e.data;
  const env: RenderEnv = { documentFolder, assetBase };
  // Parse once; load any fence languages (async, first use only), then render synchronously.
  const tokens = md.parse(text, env);
  await prepareLanguages(fenceLanguages(tokens));
  const html = md.renderer.render(tokens, md.options, env);
  const result: RenderResult = { id, html, ms: performance.now() - start };
  self.postMessage(result);
};
