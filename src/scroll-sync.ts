import type { EditorView } from '@codemirror/view';

/**
 * Keeps the preview aligned with the editor's top visible line (F10). The preview's
 * block elements carry `data-line` (0-based source line) from the worker; the scroll
 * position is interpolated between the blocks around the editor's top line.
 *
 * One direction only: editor drives preview. Returns `sync` so callers can re-align
 * after the preview re-renders.
 */
export function syncPreviewToEditor(view: EditorView, preview: HTMLElement): () => void {
  let pendingFrame = 0;

  function sync() {
    if (preview.hidden) return;
    const scroller = view.scrollDOM;
    if (scroller.scrollTop <= 0) return void (preview.scrollTop = 0);
    if (isScrolledToBottom(scroller)) return void (preview.scrollTop = preview.scrollHeight);

    const topLine = sourceLineAtTop(view);
    const blocks = Array.from(preview.querySelectorAll<HTMLElement>('[data-line]'));
    const after = blocks.findIndex((b) => Number(b.dataset.line) > topLine);
    const before = blocks[after === -1 ? blocks.length - 1 : after - 1];
    if (!before) return;

    const beforeLine = Number(before.dataset.line);
    const beforeTop = offsetWithin(preview, before);
    const next = after === -1 ? null : blocks[after];
    const nextLine = next ? Number(next.dataset.line) : beforeLine + 1;
    const nextTop = next ? offsetWithin(preview, next) : beforeTop + before.offsetHeight;
    const progress = (topLine - beforeLine) / Math.max(1, nextLine - beforeLine);
    preview.scrollTop = beforeTop + (nextTop - beforeTop) * progress;
  }

  view.scrollDOM.addEventListener('scroll', () => {
    if (pendingFrame) return;
    pendingFrame = requestAnimationFrame(() => {
      pendingFrame = 0;
      sync();
    });
  });

  return sync;
}

/** Fractional 0-based source line at the top of the editor viewport. */
function sourceLineAtTop(view: EditorView): number {
  // lineBlockAtHeight is relative to documentTop (screen coordinates of the first line).
  const height = view.scrollDOM.getBoundingClientRect().top - view.documentTop;
  const block = view.lineBlockAtHeight(Math.max(0, height));
  const line = view.state.doc.lineAt(block.from).number - 1;
  const withinBlock = block.height ? (height - block.top) / block.height : 0;
  return line + Math.min(1, Math.max(0, withinBlock));
}

function isScrolledToBottom(el: HTMLElement): boolean {
  return el.scrollTop + el.clientHeight >= el.scrollHeight - 2;
}

function offsetWithin(container: HTMLElement, el: HTMLElement): number {
  return el.getBoundingClientRect().top - container.getBoundingClientRect().top + container.scrollTop;
}
