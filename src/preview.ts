import { convertFileSrc } from '@tauri-apps/api/core';
import { isTauri } from './ipc';
import type { RenderRequest, RenderResult } from './preview.worker';

export interface Preview {
  render(text: string): void;
  setVisible(visible: boolean): void;
  /** Folder that relative image paths resolve against; null for untitled documents. */
  setDocumentFolder(folder: string | null): void;
  readonly visible: boolean;
}

/**
 * Parsing runs in a worker; the main thread only swaps in the HTML. Link clicks never
 * navigate the webview: they go to `onLinkClick` with the raw href.
 */
export function createPreview(
  el: HTMLElement,
  onRendered: (parseMs: number) => void,
  onLinkClick: (href: string) => void,
): Preview {
  const worker = new Worker(new URL('./preview.worker.ts', import.meta.url), { type: 'module' });
  const assetBase = isTauri ? convertFileSrc('') : '';
  let seq = 0;
  let visible = true;
  let documentFolder: string | null = null;

  worker.onmessage = (e: MessageEvent<RenderResult>) => {
    if (e.data.id !== seq) return; // stale result, a newer render is in flight
    el.innerHTML = e.data.html;
    requestAnimationFrame(() => onRendered(e.data.ms));
  };

  el.addEventListener('click', (e) => {
    const link = (e.target as Element).closest('a');
    if (!link) return;
    e.preventDefault();
    const href = link.getAttribute('href');
    if (href) onLinkClick(href);
  });

  return {
    render(text) {
      if (!visible) return;
      worker.postMessage({ id: ++seq, text, documentFolder, assetBase } satisfies RenderRequest);
    },
    setVisible(v) {
      visible = v;
      el.hidden = !v;
    },
    setDocumentFolder(folder) {
      documentFolder = folder;
    },
    get visible() {
      return visible;
    },
  };
}
