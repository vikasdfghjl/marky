import type { EditorState } from '@codemirror/state';
import type { FileMeta } from './ipc';

const $ = (id: string) => document.getElementById(id)!;

function encodingLabel(meta: FileMeta, readOnly: boolean): string {
  if (readOnly) return 'Not UTF-8 (read-only)';
  return meta.bom ? 'UTF-8 BOM' : 'UTF-8';
}

/** The footer: cursor, counts, file format, and the two development timings. */
export const statusBar = {
  cursor(state: EditorState) {
    const head = state.selection.main.head;
    const line = state.doc.lineAt(head);
    $('st-pos').textContent = `Ln ${line.number}, Col ${head - line.from + 1}`;
  },

  counts(text: string) {
    const words = text.match(/\S+/g)?.length ?? 0;
    $('st-count').textContent = `${words.toLocaleString()} words, ${text.length.toLocaleString()} chars`;
  },

  format(meta: FileMeta, readOnly: boolean) {
    $('st-eol').textContent = meta.lineEnding.toUpperCase();
    $('st-enc').textContent = encodingLabel(meta, readOnly);
  },

  /** Last keystroke to preview paint, including the typing pause (PRD budget). */
  previewLatency(latencyMs: number, parseMs: number) {
    $('st-render').textContent = `preview ${Math.round(latencyMs)} ms (parse ${parseMs.toFixed(1)})`;
  },

  startup(ms: number, browser = false) {
    $('st-perf').textContent = `start ${Math.round(ms)} ms${browser ? ' (browser)' : ''}`;
  },
};
