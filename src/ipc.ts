import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { browserApi } from './browser-api';


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
const tauriApi = {
  /** One-shot at startup: the pre-read CLI/Explorer file and any crash-recovery snapshot. */
  startupState: () => invoke<StartupState>('startup_state'),
  /** `jsMs` is time since navigation start, so Rust can split webview vs. frontend cost. */
  startupMs: (jsMs: number, marks: string) => invoke<number>('startup_ms', { jsMs, marks }),
  pickOpen: () => invoke<string | null>('pick_open'),
  pickSave: (suggested?: string) => invoke<string | null>('pick_save', { suggested }),
  openFile: (path: string) => invoke<OpenedFile>('open_file', { path }),
  saveFile: (path: string, text: string, meta: FileMeta) =>
    invoke<number>('save_file', { path, text, meta }),
  /** Null when the file no longer exists. */
  fileMtime: (path: string) => invoke<number | null>('file_mtime', { path }),
  watchFile: (path: string) => invoke<void>('watch_file', { path }),
  unwatchFile: (path: string) => invoke<void>('unwatch_file', { path }),
  recentFiles: () => invoke<string[]>('recent_files'),
  /** Absolute path of a relative .md link clicked in `from`'s preview (rejects other targets). */
  resolveLink: (from: string, target: string) => invoke<string>('resolve_link', { from, target }),
  /** Opens an http(s) or mailto link in the default app. */
  openExternal: (url: string) => invoke<void>('open_external', { url }),
  /** Replaces the recovery file with these unsaved tabs; an empty list clears it. */
  snapshotBuffers: (snapshots: Snapshot[]) => invoke<void>('snapshot_buffers', { snapshots }),
  setDirty: (dirty: boolean) => invoke<void>('set_dirty', { dirty }),
  askUnsavedChanges: (fileName: string) => invoke<UnsavedChoice>('ask_unsaved_changes', { fileName }),
  /** True only when the user picks `confirm`. */
  askChoice: (message: string, confirm: string, cancel: string) =>
    invoke<boolean>('ask_choice', { message, confirm, cancel }),
  showError: (message: string) => invoke<void>('show_error', { message }),
  closeWindow: () => invoke<void>('close_window'),
  onOpenRequest: (cb: (path: string) => void): Promise<UnlistenFn> =>
    listen<string>('open-request', (e) => cb(e.payload)),
  /** Native menu clicks; the payload is a command id (see `commands` in main.ts). */
  onMenuCommand: (cb: (command: string) => void): Promise<UnlistenFn> =>
    listen<string>('menu', (e) => cb(e.payload)),
  /** Fires in bursts for any change to the watched file, including our own saves. */
  onFileChanged: (cb: (path: string) => void): Promise<UnlistenFn> =>
    listen<string>('file-changed', (e) => cb(e.payload)),
  /** True while files are dragged over the window, false when they leave or drop. */
  onDragHover: (cb: (hovering: boolean) => void): Promise<UnlistenFn> =>
    listen<boolean>('drag-hover', (e) => cb(e.payload)),
  /** Fired only when the window is closed with unsaved changes; clean windows close natively. */
  onCloseRequested: (cb: () => void): Promise<UnlistenFn> => listen('close-requested', cb),
  setTitle: (title: string) => getCurrentWindow().setTitle(title),
  /** Persists settings; Rust also applies the theme to the title bar and native menu. */
  saveSettings: (settings: object) => invoke<void>('save_settings', { settings }),
  /** URL prefix of the asset protocol that serves preview images from opened folders. */
  assetBase: () => convertFileSrc(''),
};

export type MarkyApi = typeof tauriApi;

/**
 * The Rust core, or an in-memory stand-in when the frontend runs in a plain browser
 * (`npm run dev`). Callers never branch on which one they got.
 */
export const api: MarkyApi = '__TAURI_INTERNALS__' in window ? tauriApi : browserApi;

export type UnsavedChoice = 'save' | 'discard' | 'cancel';

export interface StartupState {
  file: OpenedFile | null;
  fileError: string | null;
  /** Unsaved tabs left by a session that did not end cleanly; empty when none. */
  recovery: Snapshot[];
  /** Saved settings as written, or null; read them through `readSettings`. */
  settings: unknown;
}

/** Crash-recovery copy of an unsaved buffer. */
export interface Snapshot {
  path: string | null;
  text: string;
  meta: FileMeta;
}
