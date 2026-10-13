import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { browserApi } from './browser-api';

export const LINE_ENDINGS = ['lf', 'crlf'] as const;
export type LineEnding = (typeof LINE_ENDINGS)[number];

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
  /** Called once: everything the first paint needs, in one round trip. */
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
  /** Esc and the title-bar X answer `dismiss` when it is given, otherwise `deny`. */
  askChoice: (message: string, confirm: string, deny: string, dismiss?: string) =>
    invoke<Choice>('ask_choice', { message, confirm, deny, dismiss }),
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
  /** Ticks native menu check items, keyed by command id. */
  setMenuChecks: (checks: Record<string, boolean>) => invoke<void>('set_menu_checks', { checks }),
  /** Reports the user going idle or active again, so Rust can trim WebView2's memory while idle. */
  setIdle: (idle: boolean) => invoke<void>('set_idle', { idle }),
  /** Appends a line to marky.log in the app's data folder; it never leaves the machine. */
  logError: (message: string) => invoke<void>('log_error', { message }),
  /** URL prefix of the asset protocol that serves preview images from opened folders. */
  assetBase: () => convertFileSrc(''),
};

export type MarkyApi = typeof tauriApi;

/**
 * The Rust core, or an in-memory stand-in when `npm run dev` runs in a plain browser. Callers
 * never branch on which one they got. Production builds always use the core, so the stand-in
 * (and its sample document) is left out of them.
 */
export const api: MarkyApi = import.meta.env.DEV && !('__TAURI_INTERNALS__' in window) ? browserApi : tauriApi;

export type UnsavedChoice = 'save' | 'discard' | 'cancel';
export type Choice = 'confirm' | 'deny' | 'dismiss';

interface StartupState {
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
