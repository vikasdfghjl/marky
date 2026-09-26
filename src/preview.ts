import type { RenderRequest, RenderResult } from './preview.worker';

export interface Preview {
  render(text: string): void;
  setVisible(visible: boolean): void;
  readonly visible: boolean;
}

/** Parsing runs in a worker; the main thread only swaps in the HTML. */
export function createPreview(el: HTMLElement, onRendered: (parseMs: number) => void): Preview {
  const worker = new Worker(new URL('./preview.worker.ts', import.meta.url), { type: 'module' });
  let seq = 0;
  let visible = true;

  worker.onmessage = (e: MessageEvent<RenderResult>) => {
    if (e.data.id !== seq) return; // stale result, a newer render is in flight
    el.innerHTML = e.data.html;
    requestAnimationFrame(() => onRendered(e.data.ms));
  };

  // The preview must never navigate the webview. External links open via tauri-plugin-opener in M2.
  el.addEventListener('click', (e) => {
    if ((e.target as Element).closest('a')) e.preventDefault();
  });

  return {
    render(text) {
      if (visible) worker.postMessage({ id: ++seq, text } satisfies RenderRequest);
    },
    setVisible(v) {
      visible = v;
      el.hidden = !v;
    },
    get visible() {
      return visible;
    },
  };
}
