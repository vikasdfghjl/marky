import MarkdownIt, { type Token } from 'markdown-it';
import taskLists from 'markdown-it-task-lists';

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
const md = new MarkdownIt({ html: false, linkify: true }).use(taskLists, { enabled: false });

// Tag block elements with their source line for synced scroll (F10).
md.core.ruler.push('source_lines', (state) => {
  for (const tok of state.tokens) {
    if (tok.map && tok.nesting === 1) tok.attrSet('data-line', String(tok.map[0]));
  }
});

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

self.onmessage = (e: MessageEvent<RenderRequest>) => {
  const start = performance.now();
  const { id, text, documentFolder, assetBase } = e.data;
  const html = md.render(text, { documentFolder, assetBase } satisfies RenderEnv);
  const result: RenderResult = { id, html, ms: performance.now() - start };
  self.postMessage(result);
};
