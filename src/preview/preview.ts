import { api } from '../platform/ipc';
import { drawMermaidBlocks, onMermaidThemeChange } from './mermaid-blocks';
import { createBlockDom } from './preview-blocks';
import { logError } from '../core/error-log';
import type { RenderReply, RenderRequest } from './worker/preview.worker';

export interface Preview {
  render(text: string): void;
  setVisible(visible: boolean): void;
  /** Folder that relative image paths resolve against; null for untitled documents. */
  setDocumentFolder(folder: string | null): void;
  setTypographer(on: boolean): void;
  readonly visible: boolean;
}

let mathStyles: Promise<unknown> | null = null;

/** KaTeX's stylesheet and fonts (~300 KB) load only once a document actually shows math. */
function loadMathStylesIfNeeded(el: HTMLElement) {
  if (mathStyles || !el.querySelector('.katex')) return;
  mathStyles = import('katex/dist/katex.min.css');
}

/**
 * Parsing runs in a worker; the main thread only replaces the blocks that changed (see
 * preview-blocks.ts). Link clicks never navigate the webview: they go to `onLinkClick`
 * with the raw href.
 */
export function createPreview(
  el: HTMLElement,
  onRendered: (parseMs: number) => void,
  onLinkClick: (href: string) => void,
): Preview {
  const worker = new Worker(new URL('./worker/preview.worker.ts', import.meta.url), { type: 'module' });
  const assetBase = api.assetBase();
  let seq = 0;
  let visible = true;
  let documentFolder: string | null = null;
  let typographer = false;
  let lastText = '';
  const blockDom = createBlockDom(el);

  worker.onmessage = async (e: MessageEvent<RenderReply>) => {
    const reply = e.data;
    const { id } = reply;
    if (id !== seq) return; // stale result, a newer render is in flight
    if ('error' in reply) return logError('preview render', reply.error);
    blockDom.apply(reply.blocks);
    loadMathStylesIfNeeded(el);
    await drawMermaidBlocks(el, () => id === seq);
    if (id !== seq) return;
    requestAnimationFrame(() => onRendered(reply.ms));
  };
  // The worker failed to load or threw outside a render (render failures arrive as replies).
  worker.onerror = (e) => logError('preview worker', e.message);

  onMermaidThemeChange(() => {
    blockDom.replaceEverythingNextTime(); // drawn diagrams are theme-specific
    render(lastText);
  });

  el.addEventListener('click', (e) => {
    const link = (e.target as Element).closest('a');
    if (!link) return;
    e.preventDefault();
    const href = link.getAttribute('href');
    if (href) onLinkClick(href);
  });

  function render(text: string) {
    lastText = text;
    if (!visible) return;
    worker.postMessage({ id: ++seq, text, documentFolder, assetBase, typographer } satisfies RenderRequest);
  }

  return {
    render,
    setVisible(v) {
      visible = v;
      el.hidden = !v;
    },
    setDocumentFolder(folder) {
      documentFolder = folder;
    },
    setTypographer(on) {
      if (on === typographer) return;
      typographer = on;
      // Nothing to redo before the first document arrives (settings load at startup).
      if (lastText) render(lastText);
    },
    get visible() {
      return visible;
    },
  };
}
