import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

/** False when the frontend runs in a plain browser via `npm run dev` (no Rust core). */
export const isTauri = '__TAURI_INTERNALS__' in window;

export type LineEnding = 'lf' | 'crlf';

export interface FileMeta {
  bom: boolean;
  lineEnding: LineEnding;
}

export interface OpenedFile {
  path: string;
  /** Always LF-normalised; the original line ending is in `meta`. */
  text: string;
  meta: FileMeta;
  mtimeMs: number;
  /** True when the file was not valid UTF-8 and was decoded lossily. */
  readOnly: boolean;
}

// Paths are only ever produced by the Rust side (dialog, drag-drop, CLI arg),
// which records them in an allowlist; open/save refuse anything else.
export const api = {
  startupFile: () => invoke<string | null>('startup_file'),
  /** `jsMs` is time since navigation start, so Rust can split webview vs. frontend cost. */
  startupMs: (jsMs: number, marks: string) => invoke<number>('startup_ms', { jsMs, marks }),
  pickOpen: () => invoke<string | null>('pick_open'),
  pickSave: (suggested?: string) => invoke<string | null>('pick_save', { suggested }),
  openFile: (path: string) => invoke<OpenedFile>('open_file', { path }),
  saveFile: (path: string, text: string, meta: FileMeta) =>
    invoke<number>('save_file', { path, text, meta }),
  setDirty: (dirty: boolean) => invoke<void>('set_dirty', { dirty }),
  askUnsavedChanges: (fileName: string) => invoke<UnsavedChoice>('ask_unsaved_changes', { fileName }),
  showError: (message: string) => invoke<void>('show_error', { message }),
  closeWindow: () => invoke<void>('close_window'),
  onOpenRequest: (cb: (path: string) => void): Promise<UnlistenFn> =>
    listen<string>('open-request', (e) => cb(e.payload)),
  /** Fired only when the window is closed with unsaved changes; clean windows close natively. */
  onCloseRequested: (cb: () => void): Promise<UnlistenFn> => listen('close-requested', cb),
  setTitle: (title: string) => getCurrentWindow().setTitle(title),
};

export type UnsavedChoice = 'save' | 'discard' | 'cancel';
