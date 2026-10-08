import type { FileMeta, MarkyApi, OpenedFile, UnsavedChoice } from './ipc';
import { folderOf, joinWindowsPath } from './paths';
import { SAMPLE } from './sample';

// Stand-in for the Rust core when the frontend runs in a plain browser (`npm run dev`).
// Files live in memory, so open, save, recent files and relative links all work for
// UI development; native prompts become alert/confirm, and "closing" reloads the page.

const FOLDER = 'C:\\Marky';
const SAMPLE_PATH = `${FOLDER}\\sample.md`;
const DEFAULT_META: FileMeta = { bom: false, lineEnding: 'lf' };

interface MemoryFile {
  text: string;
  meta: FileMeta;
  mtimeMs: number;
}

const files = new Map<string, MemoryFile>([[SAMPLE_PATH, { text: SAMPLE, meta: DEFAULT_META, mtimeMs: Date.now() }]]);
/** Most recently opened or saved first. */
const recent: string[] = [SAMPLE_PATH];

function touchRecent(path: string) {
  const index = recent.indexOf(path);
  if (index >= 0) recent.splice(index, 1);
  recent.unshift(path);
}

function readFile(path: string): OpenedFile {
  const file = files.get(path);
  if (!file) throw new Error(`No such file in the browser stand-in: ${path}`);
  touchRecent(path);
  return { path, text: file.text, meta: file.meta, mtimeMs: file.mtimeMs, readOnly: false };
}

const noListener = () => Promise.resolve(() => {});

// Settings survive reloads through browser storage; it may be unavailable, then defaults apply.
const SETTINGS_KEY = 'marky.settings';

function loadSettings(): unknown {
  try {
    return JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? 'null');
  } catch {
    return null;
  }
}

function storeSettings(settings: object) {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // Not remembered across reloads.
  }
}

export const browserApi: MarkyApi = {
  startupState: async () => ({ file: readFile(SAMPLE_PATH), fileError: null, recovery: [], settings: loadSettings() }),
  // No process start to measure against: report time since navigation.
  startupMs: async (jsMs) => jsMs,
  pickOpen: async () => null,
  pickSave: async (suggested) => `${FOLDER}\\${suggested ?? 'Untitled.md'}`,
  openFile: async (path) => readFile(path),
  saveFile: async (path, text, meta) => {
    const mtimeMs = Date.now();
    files.set(path, { text, meta, mtimeMs });
    touchRecent(path);
    return mtimeMs;
  },
  fileMtime: async (path) => files.get(path)?.mtimeMs ?? null,
  watchFile: async () => {},
  unwatchFile: async () => {},
  recentFiles: async () => [...recent],
  resolveLink: async (from, target) => {
    const path = joinWindowsPath(folderOf(from) ?? FOLDER, target);
    if (!files.has(path)) throw new Error(`Linked file not found: ${path}`);
    return path;
  },
  openExternal: async (url) => void window.open(url, '_blank', 'noopener'),
  snapshotBuffers: async () => {},
  setDirty: async () => {},
  askUnsavedChanges: async (name): Promise<UnsavedChoice> =>
    window.confirm(`Discard unsaved changes to ${name}?`) ? 'discard' : 'cancel',
  askChoice: async (message, confirm, cancel) => window.confirm(`${message}\n\nOK: ${confirm}\nCancel: ${cancel}`),
  showError: async (message) => window.alert(message),
  closeWindow: async () => window.location.reload(),
  onOpenRequest: noListener,
  onMenuCommand: noListener,
  onFileChanged: noListener,
  onDragHover: noListener,
  onCloseRequested: noListener,
  setTitle: async () => {},
  saveSettings: async (settings) => storeSettings(settings),
  setMenuChecks: async () => {},
  setIdle: async () => {},
  logError: async (message) => console.error(message),
  // No asset protocol: relative images stay unresolved in the browser.
  assetBase: () => '',
};
