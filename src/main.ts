import type { EditorState } from '@codemirror/state';
import type { ViewUpdate } from '@codemirror/view';
import { measureColdStart } from './perf';
import { createEditor } from './editor';
import { createPreview } from './preview';
import { syncPreviewToEditor } from './scroll-sync';
import { pickRecentFile } from './recent-picker';
import { createTabStrip } from './tab-strip';
import { createOutline } from './outline';
import { api, isTauri, type FileMeta, type OpenedFile, type Snapshot } from './ipc';
import { SAMPLE } from './sample';

const TYPING_PAUSE_MS = 150;
/** Bursts of watcher events (a save is several events) collapse into one check. */
const FILE_CHANGE_SETTLE_MS = 100;
const SNAPSHOT_INTERVAL_MS = 30_000;
/** Above this size the preview starts hidden (PRD: files > 5 MB open with preview off). */
const PREVIEW_AUTO_OFF_BYTES = 5 * 1024 * 1024;
/** PRD F14: up to 20 open files. */
const MAX_TABS = 20;
const UNTITLED = 'Untitled';

const $ = (id: string) => document.getElementById(id)!;

/** What a tab shows: a file from disk, or a new untitled document (path null). */
type EditorDocument = Pick<OpenedFile, 'text' | 'meta' | 'readOnly'> & {
  path: string | null;
  /** Disk mtime this buffer corresponds to; null for untitled or unknown. */
  mtimeMs: number | null;
};

interface Tab {
  id: number;
  path: string | null;
  meta: FileMeta;
  readOnly: boolean;
  mtimeMs: number | null;
  dirty: boolean;
  /** The tab's CodeMirror state (document, undo history, selection). Stale while the tab is active: the view owns it. */
  state: EditorState;
  scrollTop: number;
}

const NEW_DOCUMENT: EditorDocument = {
  path: null,
  text: '',
  meta: { bom: false, lineEnding: 'crlf' },
  readOnly: false,
  mtimeMs: null,
};

let typingPauseTimer: number | undefined;
let lastEditAt = 0;
let nextTabId = 1;
/** Mirrors Rust's close guard: true while any tab has unsaved changes. */
let windowDirty = false;
/** The recovery file's current content, to skip rewriting it unchanged. */
let lastSnapshotJson = '[]';
/** `#anchor` of a cross-file link, applied once the linked file's preview has rendered. */
let pendingAnchor: string | null = null;
/** User's Ctrl+Shift+V choice; large files additionally force the preview off. */
let previewWanted = true;
/** Opens run one at a time, so a multi-file drop creates tabs in order. */
let openQueue: Promise<void> = Promise.resolve();
const fileChangeTimers = new Map<number, number>();

performance.mark('js-start');
const editor = createEditor($('editor'), onEditorUpdate);
performance.mark('editor-ready');
const preview = createPreview($('preview'), onPreviewRendered, (href) => void followLink(href));
const syncPreviewScroll = syncPreviewToEditor(editor.view, $('preview'));
const outline = createOutline($('outline'), editor.view, (line) => editor.goToLine(line));
const tabStrip = createTabStrip($('tabs'), {
  onActivate: (id) => activate(tabById(id)),
  onClose: (id) => void closeTab(tabById(id)),
});

const tabs: Tab[] = [];
let active: Tab = createTab({ ...NEW_DOCUMENT }, editor.view.state);

// ---- tabs ----

function createTab(doc: EditorDocument, state?: EditorState): Tab {
  const tab: Tab = {
    id: nextTabId++,
    path: doc.path,
    meta: doc.meta,
    readOnly: doc.readOnly,
    mtimeMs: doc.mtimeMs,
    dirty: false,
    state: state ?? editor.createState(doc.text, { readOnly: doc.readOnly }),
    scrollTop: 0,
  };
  tabs.push(tab);
  return tab;
}

function tabById(id: number): Tab {
  return tabs.find((t) => t.id === id)!;
}

function tabByPath(path: string): Tab | undefined {
  return tabs.find((t) => t.path?.toLowerCase() === path.toLowerCase());
}

function tabText(tab: Tab): string {
  return tab === active ? editor.text() : tab.state.doc.toString();
}

/** An untouched untitled tab, which opening a file reuses instead of adding a tab. */
function isPristine(tab: Tab): boolean {
  return !tab.path && !tab.dirty && tabText(tab) === '';
}

function activate(tab: Tab) {
  if (tab === active) return editor.view.focus();
  active.state = editor.view.state;
  active.scrollTop = editor.view.scrollDOM.scrollTop;
  active = tab;
  lastEditAt = 0;
  editor.show(tab.state, tab.scrollTop);
  showActiveTab();
}

/** Brings every view in line with the active tab. */
function showActiveTab() {
  preview.setDocumentFolder(folderOf(active.path));
  applyPreviewVisibility();
  refreshAllViews();
  renderTabs();
}

function activateNeighbour(offset: number) {
  const index = tabs.indexOf(active);
  activate(tabs[(index + offset + tabs.length) % tabs.length]);
}

/** Replaces a tab's document (open into a pristine tab, reload, recovery) and shows it if active. */
function loadIntoTab(tab: Tab, doc: EditorDocument) {
  const previousPath = tab.path;
  tab.path = doc.path;
  tab.meta = doc.meta;
  tab.readOnly = doc.readOnly;
  tab.mtimeMs = doc.mtimeMs;
  tab.state = editor.createState(doc.text, { readOnly: doc.readOnly });
  setDirty(false, tab);
  if (tab === active) {
    lastEditAt = 0;
    editor.show(tab.state);
    performance.mark('editor-loaded');
    showActiveTab();
  } else {
    renderTabs();
  }
  if (previousPath !== tab.path) watchPath(tab.path, previousPath);
}

function addTab(doc: EditorDocument): Tab | null {
  if (tabs.length >= MAX_TABS) {
    void notifyError(`Marky keeps up to ${MAX_TABS} files open. Close a tab first.`);
    return null;
  }
  const tab = createTab(doc);
  activate(tab);
  watchPath(tab.path, null);
  return tab;
}

function renderTabs() {
  tabStrip.render(
    tabs.map((t) => ({ id: t.id, title: fileName(t.path), path: t.path, dirty: t.dirty })),
    active.id,
  );
}

// ---- editor events ----

function onEditorUpdate(update: ViewUpdate) {
  if (update.selectionSet || update.docChanged) {
    updateCursorPosition();
    outline.highlightCurrent();
  }
  if (!update.docChanged) return;

  lastEditAt = performance.now();
  setDirty(true);
  clearTimeout(typingPauseTimer);
  typingPauseTimer = window.setTimeout(renderAfterTypingPause, TYPING_PAUSE_MS);
}

/** Single place that changes dirty state; Rust hears only window-level transitions, never keystrokes. */
function setDirty(dirty: boolean, tab: Tab = active) {
  if (tab.dirty === dirty) return;
  tab.dirty = dirty;
  if (tab === active) updateTitle();
  renderTabs();
  syncWindowDirty();
  // A tab that became clean must drop out of the recovery file now, not in 30 s.
  if (!dirty) snapshotDirtyTabs();
}

function syncWindowDirty() {
  const anyDirty = tabs.some((t) => t.dirty);
  if (anyDirty === windowDirty) return;
  windowDirty = anyDirty;
  if (isTauri) void api.setDirty(anyDirty);
}

function renderAfterTypingPause() {
  const text = editor.text();
  preview.render(text);
  updateWordCount(text);
  outline.refresh();
}

function onPreviewRendered(parseMs: number) {
  if (pendingAnchor) {
    jumpToAnchor(pendingAnchor);
    pendingAnchor = null;
  }
  syncPreviewScroll();
  // Budget: last keystroke to preview paint, including the typing pause.
  if (!lastEditAt) return;
  const latencyMs = performance.now() - lastEditAt;
  $('st-render').textContent = `preview ${Math.round(latencyMs)} ms (parse ${parseMs.toFixed(1)})`;
}

// ---- status bar & title ----

function updateCursorPosition() {
  const { state } = editor.view;
  const head = state.selection.main.head;
  const line = state.doc.lineAt(head);
  $('st-pos').textContent = `Ln ${line.number}, Col ${head - line.from + 1}`;
}

function updateWordCount(text: string) {
  const words = text.match(/\S+/g)?.length ?? 0;
  $('st-count').textContent = `${words.toLocaleString()} words, ${text.length.toLocaleString()} chars`;
}

function encodingLabel(): string {
  if (active.readOnly) return 'Not UTF-8 (read-only)';
  return active.meta.bom ? 'UTF-8 BOM' : 'UTF-8';
}

function updateFileFormatStatus() {
  $('st-eol').textContent = active.meta.lineEnding.toUpperCase();
  $('st-enc').textContent = encodingLabel();
}

function fileName(path: string | null): string {
  if (!path) return UNTITLED;
  return path.split(/[\\/]/).pop()!;
}

function folderOf(path: string | null): string | null {
  if (!path) return null;
  return path.replace(/[\\/][^\\/]*$/, '');
}

function updateTitle() {
  const title = `${active.dirty ? '● ' : ''}${fileName(active.path)} — Marky`;
  document.title = title;
  if (isTauri) void api.setTitle(title);
}

function refreshAllViews() {
  const text = editor.text();
  preview.render(text);
  updateWordCount(text);
  outline.refresh();
  updateCursorPosition();
  updateFileFormatStatus();
  updateTitle();
}

function applyPreviewVisibility() {
  const visible = previewWanted && editor.view.state.doc.length <= PREVIEW_AUTO_OFF_BYTES;
  if (visible === preview.visible) return;
  preview.setVisible(visible);
  if (visible) preview.render(editor.text());
}

const OUTLINE_KEY = 'marky.outline';

/** A per-install convenience, so browser storage is enough; it may be unavailable, which is fine. */
function toggleOutline() {
  outline.setVisible(!outline.visible);
  try {
    localStorage.setItem(OUTLINE_KEY, outline.visible ? '1' : '0');
  } catch {
    // Private mode or blocked storage: the choice just is not remembered.
  }
}

function restoreOutlineVisibility() {
  try {
    outline.setVisible(localStorage.getItem(OUTLINE_KEY) === '1');
  } catch {
    // Stays hidden.
  }
}

function togglePreview() {
  previewWanted = !preview.visible;
  applyPreviewVisibility();
}

// ---- documents ----

async function notifyError(message: string) {
  if (!isTauri) return window.alert(message);
  await api.showError(message);
}

/** Save / Don't save / Cancel for the active tab. True means its content may be discarded. */
async function resolveUnsavedChanges(): Promise<boolean> {
  if (!active.dirty) return true;
  if (!isTauri) return window.confirm(`Discard unsaved changes to ${fileName(active.path)}?`);

  const choice = await api.askUnsavedChanges(fileName(active.path));
  if (choice === 'cancel') return false;
  if (choice === 'discard') return true;
  return save();
}

function queueOpen(path: string) {
  openQueue = openQueue.then(() => openPath(path));
}

/** Focuses the file's tab if it is open; otherwise opens it in a new tab (or the pristine one). */
async function openPath(path: string) {
  const existing = tabByPath(path);
  if (existing) return activate(existing);

  let file: OpenedFile;
  try {
    file = await api.openFile(path);
  } catch (err) {
    return void (await notifyError(`Could not open ${path}\n\n${err}`));
  }
  if (isPristine(active)) return loadIntoTab(active, file);
  addTab(file);
}

function newDocument() {
  if (isPristine(active)) return editor.view.focus();
  addTab({ ...NEW_DOCUMENT });
}

/** Closing the last tab closes the window, as in Windows Notepad. */
async function closeTab(tab: Tab = active) {
  activate(tab);
  if (!(await resolveUnsavedChanges())) return;
  if (tabs.length === 1) return closeLastTab();

  const index = tabs.indexOf(tab);
  tabs.splice(index, 1);
  watchPath(null, tab.path);
  // Activate the neighbour directly: the closed tab's state must not be stashed back.
  active = tabs[Math.min(index, tabs.length - 1)];
  lastEditAt = 0;
  editor.show(active.state, active.scrollTop);
  showActiveTab();
  syncWindowDirty();
  snapshotDirtyTabs();
}

async function closeLastTab() {
  if (isTauri) return api.closeWindow();
  loadIntoTab(active, { ...NEW_DOCUMENT });
}

/** Asks about every unsaved tab in turn; Cancel on any of them keeps the window open. */
async function closeWindow() {
  if (!isTauri) return;
  for (const tab of tabs.filter((t) => t.dirty)) {
    activate(tab);
    if (!(await resolveUnsavedChanges())) return;
  }
  await api.closeWindow();
}

async function openWithDialog() {
  if (!isTauri) return;
  const path = await api.pickOpen();
  if (!path) return;
  queueOpen(path);
}

async function openRecent() {
  if (!isTauri) return;
  const path = await pickRecentFile(await api.recentFiles());
  if (!path) return;
  queueOpen(path);
}

/** The existing path, or a new one from the Save As dialog (null if cancelled). */
async function resolveSavePath(forceSaveAs: boolean): Promise<string | null> {
  if (active.path && !forceSaveAs) return active.path;
  const suggested = active.path ? fileName(active.path) : `${UNTITLED}.md`;
  return api.pickSave(suggested);
}

/** Saves the active tab. True when it is on disk afterwards; false if cancelled or failed. */
async function save(forceSaveAs = false): Promise<boolean> {
  if (!isTauri || active.readOnly) return false;
  const tab = active;
  const path = await resolveSavePath(forceSaveAs);
  if (!path) return false;

  const text = editor.text();
  let mtimeMs: number;
  try {
    mtimeMs = await api.saveFile(path, text, tab.meta);
  } catch (err) {
    await notifyError(`Could not save ${path}\n\n${err}`);
    return false;
  }
  const previousPath = tab.path;
  tab.path = path;
  tab.mtimeMs = mtimeMs;
  // Stay dirty if the user typed while the save was in flight.
  setDirty(tabText(tab) !== text, tab);
  updateTitle();
  renderTabs();
  if (previousPath !== path) {
    preview.setDocumentFolder(folderOf(path));
    watchPath(path, previousPath);
  }
  return true;
}

// ---- links in the preview ----

const URL_SCHEME = /^[a-z][a-z0-9+.-]*:/i;

/** `#heading` scrolls, http(s)/mailto opens the browser, relative .md links open in a tab. Other schemes are ignored. */
async function followLink(href: string) {
  if (href.startsWith('#')) return jumpToAnchor(decodeURIComponent(href.slice(1)));
  if (/^(https?|mailto):/i.test(href)) return void api.openExternal(href).catch((err) => notifyError(String(err)));
  if (URL_SCHEME.test(href) || !isTauri) return;
  if (!active.path) return void notifyError('Save this document first so its relative links have a folder to resolve from.');

  const [target, anchor] = href.split('#', 2);
  let path: string;
  try {
    path = await api.resolveLink(active.path, decodeURIComponent(target));
  } catch (err) {
    return void notifyError(String(err));
  }
  pendingAnchor = anchor ? decodeURIComponent(anchor) : null;
  queueOpen(path);
}

/** Moves the editor to the heading's source line; synced scroll then brings the preview along. */
function jumpToAnchor(id: string) {
  const target = $('preview').querySelector<HTMLElement>(`[id="${CSS.escape(id)}"]`);
  const line = target?.closest<HTMLElement>('[data-line]')?.dataset.line;
  if (line !== undefined) editor.goToLine(Number(line));
}

// ---- external changes (F8) ----

/** Moves a tab's file watch from `previous` to `next` (either may be null). Best-effort, e.g. on network shares. */
function watchPath(next: string | null, previous: string | null) {
  if (!isTauri) return;
  if (previous && !tabByPath(previous)) void api.unwatchFile(previous);
  if (next) api.watchFile(next).catch(() => {});
}

function onFileChangedOnDisk(path: string) {
  const tab = tabByPath(path);
  if (!tab) return;
  clearTimeout(fileChangeTimers.get(tab.id));
  fileChangeTimers.set(tab.id, window.setTimeout(() => void reconcileWithDisk(tab), FILE_CHANGE_SETTLE_MS));
}

/** Clean tab: reload silently. Dirty tab: ask Reload / Keep mine. Our own saves are ignored by mtime. */
async function reconcileWithDisk(tab: Tab) {
  const path = tab.path;
  if (!path || !tabs.includes(tab)) return;
  const diskMtime = await api.fileMtime(path);
  if (diskMtime === tab.mtimeMs) return;
  if (diskMtime === null) return onDeletedOnDisk(tab);
  if (!tab.dirty) return reloadKeepingPosition(tab);

  // Ask once per disk version, even if the user keeps their changes.
  tab.mtimeMs = diskMtime;
  const reload = await api.askChoice(
    `${fileName(path)} was changed by another program. Reload it and lose your unsaved changes?`,
    'Reload',
    'Keep mine',
  );
  if (reload) await reloadKeepingPosition(tab);
}

async function reloadKeepingPosition(tab: Tab) {
  const wasActive = tab === active;
  const position = wasActive ? editor.viewPosition() : null;
  const scrollTop = tab.scrollTop;
  let file: OpenedFile;
  try {
    file = await api.openFile(tab.path!);
  } catch {
    return; // Mid-write by the other program; the next watcher event retries.
  }
  loadIntoTab(tab, file);
  if (position && tab === active) editor.restoreViewPosition(position);
  else tab.scrollTop = scrollTop;
}

/** Keep the buffer and mark it unsaved, so closing prompts to save it back. */
function onDeletedOnDisk(tab: Tab) {
  tab.mtimeMs = null;
  setDirty(true, tab);
}

// ---- crash recovery (F13) ----

/** Writes every unsaved tab to the recovery file, or clears it when none are left. */
function snapshotDirtyTabs() {
  if (!isTauri) return;
  const snapshots: Snapshot[] = tabs
    .filter((t) => t.dirty)
    .map((t) => ({ path: t.path, text: tabText(t), meta: t.meta }));
  const json = JSON.stringify(snapshots);
  if (json === lastSnapshotJson) return;
  lastSnapshotJson = json;
  void api.snapshotBuffers(snapshots);
}

/** Offers back the unsaved tabs of a session that did not end cleanly. */
async function offerRecovery(snapshots: Snapshot[]) {
  const names = snapshots.map((s) => fileName(s.path)).join(', ');
  const restore = await api.askChoice(
    `Marky did not close cleanly. Restore your unsaved changes to ${names}?`,
    'Restore',
    'Discard',
  );
  if (!restore) {
    lastSnapshotJson = '[]';
    return void api.snapshotBuffers([]);
  }
  for (const snapshot of snapshots) await restoreSnapshot(snapshot);
  snapshotDirtyTabs();
}

async function restoreSnapshot(snapshot: Snapshot) {
  const mtimeMs = snapshot.path ? await api.fileMtime(snapshot.path) : null;
  const doc: EditorDocument = { ...snapshot, readOnly: false, mtimeMs };
  const existing = snapshot.path ? tabByPath(snapshot.path) : undefined;
  // The user may already be typing in the startup file's tab; never overwrite their work.
  if (existing?.dirty) return;

  const reusable = existing ?? (isPristine(active) ? active : null);
  if (reusable) loadIntoTab(reusable, doc);
  const target = reusable ?? addTab(doc);
  if (target) setDirty(true, target);
}

// ---- commands: one table for the menu bar and the keyboard ----

/** Ids match the native menu items in src-tauri/src/menu.rs. */
const commands = {
  new: newDocument,
  open: openWithDialog,
  'open-recent': openRecent,
  save: () => save(),
  'save-as': () => save(true),
  close: () => closeTab(),
  undo: editor.undo,
  redo: editor.redo,
  find: editor.openFind,
  replace: editor.openReplace,
  'toggle-preview': togglePreview,
  'toggle-outline': toggleOutline,
  'next-tab': () => activateNeighbour(1),
  'previous-tab': () => activateNeighbour(-1),
} satisfies Record<string, () => unknown>;

type CommandId = keyof typeof commands;

function runCommand(id: string) {
  if (!(id in commands)) return;
  void commands[id as CommandId]();
}

/** Undo, redo and find are left to CodeMirror's own keymap; the menu only mirrors them. */
const SHORTCUTS: Record<string, CommandId> = {
  'Ctrl+N': 'new',
  'Ctrl+O': 'open',
  'Ctrl+R': 'open-recent',
  'Ctrl+S': 'save',
  'Ctrl+Shift+S': 'save-as',
  'Ctrl+W': 'close',
  'Ctrl+H': 'replace',
  'Ctrl+Shift+V': 'toggle-preview',
  'Ctrl+Shift+O': 'toggle-outline',
  'Ctrl+Tab': 'next-tab',
  'Ctrl+Shift+Tab': 'previous-tab',
};

/** A reload would silently drop every buffer; there is nothing to refresh. */
const BLOCKED_KEYS = new Set(['F5', 'Ctrl+F5', 'Ctrl+Shift+R']);

function keyChord(e: KeyboardEvent): string {
  const key = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  return [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', key].filter(Boolean).join('+');
}

// Capture phase so app shortcuts win over the editor and the webview defaults.
window.addEventListener(
  'keydown',
  (e) => {
    const chord = keyChord(e);
    const command = SHORTCUTS[chord];
    if (!command && !BLOCKED_KEYS.has(chord)) return;
    e.preventDefault();
    e.stopPropagation();
    if (command) runCommand(command);
  },
  { capture: true },
);

// ---- startup ----

function startInBrowser() {
  restoreOutlineVisibility();
  loadIntoTab(active, { ...NEW_DOCUMENT, text: SAMPLE });
  requestAnimationFrame(() => {
    $('st-perf').textContent = `start ${Math.round(performance.now())} ms (browser)`;
  });
}

async function start() {
  if (!isTauri) return startInBrowser();

  const [, , , , , startup] = await Promise.all([
    api.onOpenRequest(queueOpen),
    api.onMenuCommand(runCommand),
    api.onCloseRequested(() => void closeWindow()),
    api.onFileChanged(onFileChangedOnDisk),
    api.onDragHover((hovering) => document.body.classList.toggle('drag-over', hovering)),
    api.startupState(),
  ]);
  performance.mark('ipc-ready');
  restoreOutlineVisibility();

  // Paint first: the file was pre-read in Rust while WebView2 started.
  if (startup.file) loadIntoTab(active, startup.file);
  else showActiveTab();
  performance.mark('file-loaded');
  measureColdStart((ms) => ($('st-perf').textContent = `start ${Math.round(ms)} ms`));

  window.setInterval(snapshotDirtyTabs, SNAPSHOT_INTERVAL_MS);
  if (startup.fileError) await notifyError(startup.fileError);
  if (startup.recovery.length) await offerRecovery(startup.recovery);
}

void start();
