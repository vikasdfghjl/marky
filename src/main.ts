import type { EditorState, Text } from '@codemirror/state';
import type { ViewUpdate } from '@codemirror/view';
import { trimMemoryWhenIdle } from './idle';
import { measureColdStart } from './perf';
import { createEditor } from './editor';
import { createPreview } from './preview';
import { syncPreviewToEditor } from './scroll-sync';
import { pickRecentFile } from './recent-picker';
import { createTabStrip } from './tab-strip';
import { createOutline } from './outline';
import { statusBar } from './status-bar';
import { listenForShortcuts } from './shortcuts';
import { createSettingsStore, readSettings, type Settings } from './settings';
import { createSettingsPanel } from './settings-panel';
import { applyTheme } from './theme';
import { URL_SCHEME, UNTITLED, fileName, folderOf } from './paths';
import { api, type FileMeta, type OpenedFile, type Snapshot } from './ipc';
import { byId } from './dom';
import { createSerialQueue } from './serial-queue';
import { logUncaughtErrors } from './error-log';

logUncaughtErrors();

const TYPING_PAUSE_MS = 150;
/** Bursts of watcher events (a save is several events) collapse into one check. */
const FILE_CHANGE_SETTLE_MS = 100;
const SNAPSHOT_INTERVAL_MS = 30_000;
/** Above this size the preview starts hidden (PRD: files > 5 MB open with preview off). */
const PREVIEW_AUTO_OFF_BYTES = 5 * 1024 * 1024;
/** Auto-save waits for a real pause, not every typing pause, to keep disk writes and watcher events rare. */
const AUTO_SAVE_DELAY_MS = 1000;
/** PRD F14: up to 20 open files. */
const MAX_TABS = 20;

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
let autoSaveTimer: number | undefined;
let lastEditAt = 0;
let nextTabId = 1;
/** Mirrors Rust's close guard: true while any tab has unsaved changes. */
let windowDirty = false;
/**
 * What the recovery file holds: each dirty tab's id, document, path and format. Compared by
 * identity (CodeMirror documents are immutable), so the 30 s timer never re-serialises a large
 * unchanged document just to find nothing changed. Null when unknown (a write failed).
 */
let snapshotKey: unknown[] | null = [];
/** `#anchor` of a cross-file link, applied once the linked file's preview has rendered. */
let pendingAnchor: string | null = null;
/** One at a time, so a multi-file drop creates tabs in order. */
const opens = createSerialQueue('open');
/** One at a time, so an older snapshot never lands after a newer one (or after the clear on save). */
const snapshotWrites = createSerialQueue('recovery snapshot');
/** One at a time: saves share settings.json.tmp on the Rust side. */
const settingsWrites = createSerialQueue('settings save');
const fileChangeTimers = new Map<number, number>();

performance.mark('js-start');
const editor = createEditor(byId('editor'), onEditorUpdate);
performance.mark('editor-ready');
const preview = createPreview(byId('preview'), onPreviewRendered, (href) => void followLink(href));
const syncPreviewScroll = syncPreviewToEditor(editor.view, byId('preview'));
const outline = createOutline(byId('outline'), editor.view, (line) => editor.goToLine(line));
const tabStrip = createTabStrip(byId('tabs'), {
  onActivate: (id) => activate(tabById(id)),
  onClose: (id) => void closeTab(tabById(id)),
});

const settings = createSettingsStore((current) => settingsWrites.push(() => api.saveSettings(current)));
settings.onChange(applySettings);
const settingsPanel = createSettingsPanel(byId('settings-button') as HTMLButtonElement, settings);
/** Input trims nothing; going quiet for a while does (idle.ts). Non-input use calls this directly. */
const noteActivity = trimMemoryWhenIdle();
byId('new-tab-button').addEventListener('click', () => runCommand('new'));
const editorToggle = byId('editor-toggle');
editorToggle.addEventListener('click', () => runCommand('toggle-editor'));

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
  const tab = tabs.find((t) => t.id === id);
  if (!tab) throw new Error(`No tab with id ${id}`);
  return tab;
}

function tabByPath(path: string): Tab | undefined {
  return tabs.find((t) => t.path?.toLowerCase() === path.toLowerCase());
}

function tabDoc(tab: Tab): Text {
  return tab === active ? editor.view.state.doc : tab.state.doc;
}

function tabText(tab: Tab): string {
  return tabDoc(tab).toString();
}

/** An untouched untitled tab, which opening a file reuses instead of adding a tab. */
function isPristine(tab: Tab): boolean {
  return !tab.path && !tab.dirty && tabText(tab) === '';
}

function activate(tab: Tab) {
  if (tab === active) {
    editor.view.focus();
    return;
  }
  active.state = editor.view.state;
  active.scrollTop = editor.view.scrollDOM.scrollTop;
  active = tab;
  showInEditor(tab, tab.scrollTop);
}

/** Puts the (already active) tab's state in the editor and refreshes everything around it. */
function showInEditor(tab: Tab, scrollTop: number) {
  lastEditAt = 0;
  editor.show(tab.state, scrollTop);
  showActiveTab();
}

/** Brings every view in line with the active tab. */
function showActiveTab() {
  preview.setDocumentFolder(folderOf(active.path));
  applyPaneVisibility();
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
    showInEditor(tab, 0);
    performance.mark('editor-loaded');
  } else {
    renderTabs();
  }
  if (previousPath !== tab.path) watchPath(tab.path, previousPath);
}

function addTab(doc: EditorDocument): Tab | null {
  if (tabs.length >= MAX_TABS) {
    void api.showError(`Marky keeps up to ${MAX_TABS} files open. Close a tab first.`);
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
    statusBar.cursor(update.state);
    outline.highlightCurrent();
  }
  if (!update.docChanged) return;

  lastEditAt = performance.now();
  setDirty(true);
  clearTimeout(typingPauseTimer);
  typingPauseTimer = window.setTimeout(renderAfterTypingPause, TYPING_PAUSE_MS);
  scheduleAutoSave();
}

/** Saves the active tab once typing has paused, if auto-save is on. Untitled and read-only tabs are never auto-saved. */
function scheduleAutoSave() {
  clearTimeout(autoSaveTimer);
  const tab = active;
  if (!settings.get().autoSave || !tab.path || tab.readOnly) return;
  autoSaveTimer = window.setTimeout(() => {
    // save() writes the editor's text, so only while this tab is still the one shown.
    if (tab === active && tab.dirty) void save();
  }, AUTO_SAVE_DELAY_MS);
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
  void api.setDirty(anyDirty);
}

function renderAfterTypingPause() {
  const text = editor.text();
  preview.render(text);
  statusBar.counts(text);
  outline.refresh();
}

function onPreviewRendered(parseMs: number) {
  if (pendingAnchor) {
    jumpToAnchor(pendingAnchor);
    pendingAnchor = null;
  }
  syncPreviewScroll();
  if (lastEditAt) statusBar.previewLatency(performance.now() - lastEditAt, parseMs);
}

// ---- title & views ----

function updateTitle() {
  const title = `${active.dirty ? '● ' : ''}${fileName(active.path)} — Marky`;
  document.title = title;
  void api.setTitle(title);
}

function refreshAllViews() {
  const text = editor.text();
  preview.render(text);
  statusBar.counts(text);
  statusBar.cursor(editor.view.state);
  statusBar.format(active.meta, active.readOnly);
  outline.refresh();
  updateTitle();
}

/**
 * Shows the panes the settings ask for, but never neither: hiding the editor brings the
 * preview back, and the editor stays when a very large file has no preview.
 */
function applyPaneVisibility() {
  const { showEditor, showPreview } = settings.get();
  const previewVisible = (showPreview || !showEditor) && editor.view.state.doc.length <= PREVIEW_AUTO_OFF_BYTES;
  const editorVisible = showEditor || !previewVisible;
  setEditorVisible(editorVisible);
  if (previewVisible === preview.visible) return;
  preview.setVisible(previewVisible);
  if (previewVisible) preview.render(editor.text());
}

function setEditorVisible(visible: boolean) {
  const pane = byId('editor');
  editorToggle.setAttribute('aria-pressed', String(visible));
  if (pane.hidden === !visible) return;
  const hadFocus = pane.contains(document.activeElement);
  pane.hidden = !visible;
  // Keep keyboard focus on something visible.
  if (visible) editor.view.focus();
  else if (hadFocus) byId('preview').focus();
}

/** Find and replace work in the editor, so they bring it back when it is hidden. */
function showingEditor(action: () => void) {
  return () => {
    if (!settings.get().showEditor) settings.update({ showEditor: true });
    action();
  };
}

// ---- settings ----

/** Applies changed settings; at startup `changed` is every setting. */
function applySettings(current: Settings, changed: Partial<Settings>) {
  if ('theme' in changed) applyTheme(current.theme);
  // Before pane visibility, which can render: the first render then already has the right flag.
  if ('typographer' in changed) preview.setTypographer(current.typographer);
  if ('fontSize' in changed) {
    document.documentElement.style.setProperty('--editor-font-size', `${current.fontSize}px`);
    editor.view.requestMeasure();
  }
  if ('wordWrap' in changed) editor.setWordWrap(current.wordWrap);
  if ('showEditor' in changed || 'showPreview' in changed) applyPaneVisibility();
  if ('showOutline' in changed) outline.setVisible(current.showOutline);
  // autoSave and newFileLineEnding are read when they are used.
  syncMenuChecks(current, changed);
}

/** Native menu check items (ids in menu.rs) and the boolean setting each one toggles. */
const MENU_TOGGLES = {
  'toggle-editor': 'showEditor',
  'toggle-preview': 'showPreview',
  'toggle-outline': 'showOutline',
  'toggle-word-wrap': 'wordWrap',
  'toggle-auto-save': 'autoSave',
} as const satisfies Record<string, keyof Settings>;

function syncMenuChecks(current: Settings, changed: Partial<Settings>) {
  const checks = Object.entries(MENU_TOGGLES).filter(([, setting]) => setting in changed);
  if (!checks.length) return;
  void api.setMenuChecks(Object.fromEntries(checks.map(([id, setting]) => [id, current[setting]])));
}

function toggleSetting(setting: (typeof MENU_TOGGLES)[keyof typeof MENU_TOGGLES]) {
  const on = !settings.get()[setting];
  const changes: Partial<Settings> = { [setting]: on };
  // Hiding one pane turns the other on, so the window never goes empty and the ticks stay true.
  if (!on && setting === 'showEditor') changes.showPreview = true;
  if (!on && setting === 'showPreview') changes.showEditor = true;
  settings.update(changes);
}

/** An empty untitled document with the configured line ending. */
function untitledDocument(): EditorDocument {
  return { ...NEW_DOCUMENT, meta: { bom: false, lineEnding: settings.get().newFileLineEnding } };
}

// ---- documents ----

/** Save / Don't save / Cancel for the active tab. True means its content may be discarded. */
async function resolveUnsavedChanges(): Promise<boolean> {
  if (!active.dirty) return true;

  const choice = await api.askUnsavedChanges(fileName(active.path));
  if (choice === 'cancel') return false;
  if (choice === 'discard') return true;
  return save();
}

function queueOpen(path: string) {
  noteActivity();
  opens.push(() => openPath(path));
}

/** Focuses the file's tab if it is open; otherwise opens it in a new tab (or the pristine one). */
async function openPath(path: string) {
  const existing = tabByPath(path);
  if (existing) return activate(existing);

  let file: OpenedFile;
  try {
    file = await api.openFile(path);
  } catch (err) {
    await api.showError(`Could not open ${path}\n\n${err}`);
    return;
  }
  if (isPristine(active)) return loadIntoTab(active, file);
  addTab(file);
}

function newDocument() {
  if (isPristine(active)) return editor.view.focus();
  addTab(untitledDocument());
}

/** Closing the last tab closes the window, as in Windows Notepad. */
async function closeTab(tab: Tab = active) {
  activate(tab);
  if (!(await resolveUnsavedChanges())) return;
  if (tabs.length === 1) return closeWindowNow();

  const index = tabs.indexOf(tab);
  tabs.splice(index, 1);
  watchPath(null, tab.path);
  // Activate the neighbour directly: the closed tab's state must not be stashed back.
  active = tabs[Math.min(index, tabs.length - 1)];
  showInEditor(active, active.scrollTop);
  syncWindowDirty();
  snapshotDirtyTabs();
}

/** Asks about every unsaved tab in turn; Cancel on any of them keeps the window open. */
async function confirmAndCloseWindow() {
  for (const tab of tabs.filter((t) => t.dirty)) {
    activate(tab);
    if (!(await resolveUnsavedChanges())) return;
  }
  await closeWindowNow();
}

/** Rust clears the recovery file as it closes the window, so a snapshot still in flight must land first. */
async function closeWindowNow() {
  await snapshotWrites.drained();
  await api.closeWindow();
}

async function openWithDialog() {
  const path = await api.pickOpen();
  if (!path) return;
  queueOpen(path);
}

async function openRecent() {
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
  if (active.readOnly) return false;
  const tab = active;
  const path = await resolveSavePath(forceSaveAs);
  if (!path) return false;

  const text = editor.text();
  let mtimeMs: number;
  try {
    mtimeMs = await api.saveFile(path, text, tab.meta);
  } catch (err) {
    await api.showError(`Could not save ${path}\n\n${err}`);
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

/** `#heading` scrolls, http(s)/mailto opens the browser, relative .md links open in a tab. Other schemes are ignored. */
async function followLink(href: string) {
  if (href.startsWith('#')) return jumpToAnchor(decodeURIComponent(href.slice(1)));
  if (/^(https?|mailto):/i.test(href)) {
    await api.openExternal(href).catch((err) => api.showError(String(err)));
    return;
  }
  if (URL_SCHEME.test(href)) return;
  if (!active.path) {
    await api.showError('Save this document first so its relative links have a folder to resolve from.');
    return;
  }

  const [target, anchor] = href.split('#', 2);
  let path: string;
  try {
    path = await api.resolveLink(active.path, decodeURIComponent(target));
  } catch (err) {
    await api.showError(String(err));
    return;
  }
  pendingAnchor = anchor ? decodeURIComponent(anchor) : null;
  queueOpen(path);
}

/** Moves the editor to the anchor target's source line (a footnote); synced scroll then brings the preview along. */
function jumpToAnchor(id: string) {
  const target = byId('preview').querySelector<HTMLElement>(`[id="${CSS.escape(id)}"]`);
  const line = target?.closest<HTMLElement>('[data-line]')?.dataset.line;
  if (line !== undefined) editor.goToLine(Number(line));
}

// ---- external changes (F8) ----

/** Moves a tab's file watch from `previous` to `next` (either may be null). Best-effort, e.g. on network shares. */
function watchPath(next: string | null, previous: string | null) {
  if (previous && !tabByPath(previous)) void api.unwatchFile(previous);
  if (next) api.watchFile(next).catch(() => {});
}

function onFileChangedOnDisk(path: string) {
  const tab = tabByPath(path);
  if (!tab) return;
  noteActivity();
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
  const path = tab.path;
  if (!path) return;
  const position = tab === active ? editor.viewPosition() : null;
  const scrollTop = tab.scrollTop;
  let file: OpenedFile;
  try {
    file = await api.openFile(path);
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
  const dirty = tabs.filter((t) => t.dirty);
  const key = dirty.flatMap((t) => [t.id, tabDoc(t), t.path, t.meta]);
  if (snapshotKey && sameItems(key, snapshotKey)) return;
  writeSnapshots(
    dirty.map((t) => ({ path: t.path, text: tabText(t), meta: t.meta })),
    key,
  );
}

/** Queues a recovery-file write. A failed write forgets the file's content, so the next snapshot retries. */
function writeSnapshots(snapshots: Snapshot[], key: unknown[]) {
  snapshotKey = key;
  snapshotWrites.push(() =>
    api.snapshotBuffers(snapshots).catch((err) => {
      if (snapshotKey === key) snapshotKey = null;
      throw err; // the queue logs it
    }),
  );
}

function sameItems(a: unknown[], b: unknown[]): boolean {
  return a.length === b.length && a.every((item, i) => item === b[i]);
}

/** Offers back the unsaved tabs of a session that did not end cleanly. */
async function offerRecovery(snapshots: Snapshot[]) {
  const names = snapshots.map((s) => fileName(s.path)).join(', ');
  const restore = await api.askChoice(
    `Marky did not close cleanly. Restore your unsaved changes to ${names}?`,
    'Restore',
    'Discard',
  );
  if (!restore) return writeSnapshots([], []);
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
  find: showingEditor(editor.openFind),
  replace: showingEditor(editor.openReplace),
  'toggle-editor': () => toggleSetting('showEditor'),
  'toggle-preview': () => toggleSetting('showPreview'),
  'toggle-outline': () => toggleSetting('showOutline'),
  'toggle-word-wrap': () => toggleSetting('wordWrap'),
  'toggle-auto-save': () => toggleSetting('autoSave'),
  settings: () => settingsPanel.toggle(),
  'next-tab': () => activateNeighbour(1),
  'previous-tab': () => activateNeighbour(-1),
} satisfies Record<string, () => unknown>;

type CommandId = keyof typeof commands;

function runCommand(id: string) {
  if (!(id in commands)) return;
  noteActivity(); // native menu clicks are not DOM input
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
  'Ctrl+Shift+E': 'toggle-editor',
  'Ctrl+Shift+V': 'toggle-preview',
  'Ctrl+Shift+O': 'toggle-outline',
  'Alt+Z': 'toggle-word-wrap',
  'Ctrl+,': 'settings',
  'Ctrl+Tab': 'next-tab',
  'Ctrl+Shift+Tab': 'previous-tab',
};

listenForShortcuts(SHORTCUTS, runCommand);

// ---- startup ----

async function start() {
  const listening = Promise.all([
    api.onOpenRequest(queueOpen),
    api.onMenuCommand(runCommand),
    api.onCloseRequested(() => void confirmAndCloseWindow()),
    api.onFileChanged(onFileChangedOnDisk),
    api.onDragHover((hovering) => {
      noteActivity();
      document.body.classList.toggle('drag-over', hovering);
    }),
  ]);
  const [startup] = await Promise.all([api.startupState(), listening]);
  performance.mark('ipc-ready');
  settings.load(readSettings(startup.settings));

  // Paint first: the file was pre-read in Rust while WebView2 started.
  if (startup.file) {
    loadIntoTab(active, startup.file);
  } else {
    // The first tab was created before settings loaded; give it the configured line ending.
    active.meta = untitledDocument().meta;
    showActiveTab();
  }
  performance.mark('file-loaded');
  measureColdStart((ms) => statusBar.startup(ms));

  window.setInterval(snapshotDirtyTabs, SNAPSHOT_INTERVAL_MS);
  if (startup.fileError) await api.showError(startup.fileError);
  if (startup.recovery.length) await offerRecovery(startup.recovery);
}

void start();
