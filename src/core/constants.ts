import type { Settings } from './settings';

/** The preview pause tracks parse time while staying within the measured 30–150 ms range. */
export const MAX_TYPING_PAUSE_MS = 150;
export const MIN_TYPING_PAUSE_MS = 30;

/** Watcher bursts from a single save collapse into one disk check. */
export const FILE_CHANGE_SETTLE_MS = 100;
export const SNAPSHOT_INTERVAL_MS = 30_000;

/** PRD: files over 5 MB open with the preview off. */
export const PREVIEW_AUTO_OFF_BYTES = 5 * 1024 * 1024;

/** Longer than the typing pause, so disk writes and watcher events stay rare. */
export const AUTO_SAVE_DELAY_MS = 1000;
export const MAX_TABS = 20;

export const NEW_DOCUMENT = {
  path: null,
  text: '',
  meta: { bom: false, lineEnding: 'crlf' },
  readOnly: false,
  mtimeMs: null,
} as const;

/** Native menu check items and their corresponding settings. */
export const MENU_TOGGLES = {
  'toggle-editor': 'showEditor',
  'toggle-preview': 'showPreview',
  'toggle-outline': 'showOutline',
  'toggle-word-wrap': 'wordWrap',
  'toggle-auto-save': 'autoSave',
} as const satisfies Record<string, keyof Settings>;

/** Undo, redo and find remain in CodeMirror's keymap; the menu only mirrors those shortcuts. */
export const SHORTCUTS = {
  'Ctrl+N': 'new',
  'Ctrl+O': 'open',
  'Ctrl+R': 'open-recent',
  'Ctrl+S': 'save',
  'Ctrl+Shift+S': 'save-as',
  'Ctrl+W': 'close',
  'Ctrl+H': 'replace',
  'Ctrl+Shift+E': 'toggle-editor',
  'Ctrl+Shift+V': 'toggle-preview',
  'Ctrl+Shift+O': 'toggle-outline',
  'Alt+Z': 'toggle-word-wrap',
  'Ctrl+,': 'settings',
  'Ctrl+Tab': 'next-tab',
  'Ctrl+Shift+Tab': 'previous-tab',
} as const satisfies Record<string, string>;
