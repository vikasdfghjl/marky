import type { ViewUpdate } from '@codemirror/view';
import { measureColdStart } from './perf';
import { createEditor } from './editor';
import { createPreview } from './preview';
import { syncPreviewToEditor } from './scroll-sync';
import { pickRecentFile } from './recent-picker';
import { api, isTauri, type OpenedFile } from './ipc';
import { SAMPLE } from './sample';

const TYPING_PAUSE_MS = 150;
/** Bursts of watcher events (a save is several events) collapse into one check. */
const FILE_CHANGE_SETTLE_MS = 100;
const SNAPSHOT_INTERVAL_MS = 30_000;
/** Above this size the preview starts hidden (PRD: files > 5 MB open with preview off). */
const PREVIEW_AUTO_OFF_BYTES = 5 * 1024 * 1024;
const UNTITLED = 'Untitled';

const $ = (id: string) => document.getElementById(id)!;

/** What the editor shows: a file from disk, or a new untitled document (path null). */
type EditorDocument = Pick<OpenedFile, 'text' | 'meta' | 'readOnly'> & {
  path: string | null;
  /** Disk mtime this buffer corresponds to; null for untitled or unknown. */
  mtimeMs: number | null;
};

const NEW_DOCUMENT: EditorDocument = {
  path: null,
  text: '',
  meta: { bom: false, lineEnding: 'crlf' },
  readOnly: false,
  mtimeMs: null,
};

const currentFile = {
  path: NEW_DOCUMENT.path,
  meta: NEW_DOCUMENT.meta,
  readOnly: NEW_DOCUMENT.readOnly,
  mtimeMs: NEW_DOCUMENT.mtimeMs,
  dirty: false,
};

let typingPauseTimer: number | undefined;
let fileChangeTimer: number | undefined;
let lastEditAt = 0;
let lastSnapshotText: string | null = null;

performance.mark('js-start');
const editor = createEditor($('editor'), onEditorUpdate);
performance.mark('editor-ready');
const preview = createPreview($('preview'), onPreviewRendered);
const syncPreviewScroll = syncPreviewToEditor(editor.view, $('preview'));

// ---- editor events ----

function onEditorUpdate(update: ViewUpdate) {
  if (update.selectionSet || update.docChanged) updateCursorPosition();
  if (!update.docChanged) return;

  lastEditAt = performance.now();
  setDirty(true);
  clearTimeout(typingPauseTimer);
  typingPauseTimer = window.setTimeout(renderAfterTypingPause, TYPING_PAUSE_MS);
}

/** Single place that changes dirty state; Rust is told only on transitions, never per keystroke. */
function setDirty(dirty: boolean) {
  if (currentFile.dirty === dirty) return;
  currentFile.dirty = dirty;
  updateTitle();
  if (!isTauri) return;
  void api.setDirty(dirty);
  if (!dirty) clearRecoverySnapshot();
}

function renderAfterTypingPause() {
  const text = editor.text();
  preview.render(text);
  updateWordCount(text);
}

function onPreviewRendered(parseMs: number) {
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
  if (currentFile.readOnly) return 'Not UTF-8 (read-only)';
  return currentFile.meta.bom ? 'UTF-8 BOM' : 'UTF-8';
}

function updateFileFormatStatus() {
  $('st-eol').textContent = currentFile.meta.lineEnding.toUpperCase();
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
  const title = `${currentFile.dirty ? '● ' : ''}${fileName(currentFile.path)} — Marky`;
  document.title = title;
  if (isTauri) void api.setTitle(title);
}

function refreshAllViews() {
  const text = editor.text();
  preview.render(text);
  updateWordCount(text);
  updateCursorPosition();
  updateFileFormatStatus();
  updateTitle();
}

// ---- documents ----

async function notifyError(message: string) {
  if (!isTauri) return window.alert(message);
  await api.showError(message);
}

/** Resolves the Save / Don't save / Cancel prompt. True means the current document may be replaced. */
async function resolveUnsavedChanges(): Promise<boolean> {
  if (!currentFile.dirty) return true;
  if (!isTauri) return window.confirm(`Discard unsaved changes to ${fileName(currentFile.path)}?`);

  const choice = await api.askUnsavedChanges(fileName(currentFile.path));
  if (choice === 'cancel') return false;
  if (choice === 'discard') return true;
  return save();
}

function showDocument(editorDocument: EditorDocument) {
  lastEditAt = 0;
  currentFile.path = editorDocument.path;
  currentFile.meta = editorDocument.meta;
  currentFile.readOnly = editorDocument.readOnly;
  currentFile.mtimeMs = editorDocument.mtimeMs;
  setDirty(false);
  preview.setDocumentFolder(folderOf(editorDocument.path));
  preview.setVisible(editorDocument.text.length <= PREVIEW_AUTO_OFF_BYTES);
  editor.load(editorDocument.text, { readOnly: editorDocument.readOnly });
  performance.mark('editor-loaded');
  refreshAllViews();
  watchCurrentFile();
}

async function openPath(path: string) {
  if (!(await resolveUnsavedChanges())) return;
  try {
    showDocument(await api.openFile(path));
  } catch (err) {
    await notifyError(`Could not open ${path}\n\n${err}`);
  }
}

async function newDocument() {
  if (!(await resolveUnsavedChanges())) return;
  showDocument(NEW_DOCUMENT);
}

async function closeWindow() {
  if (!isTauri) return;
  if (!(await resolveUnsavedChanges())) return;
  await api.closeWindow();
}

async function openWithDialog() {
  if (!isTauri) return;
  const path = await api.pickOpen();
  if (!path) return;
  await openPath(path);
}

async function openRecent() {
  if (!isTauri) return;
  const path = await pickRecentFile(await api.recentFiles());
  if (!path) return;
  await openPath(path);
}

/** The existing path, or a new one from the Save As dialog (null if cancelled). */
async function resolveSavePath(forceSaveAs: boolean): Promise<string | null> {
  if (currentFile.path && !forceSaveAs) return currentFile.path;
  const suggested = currentFile.path ? fileName(currentFile.path) : `${UNTITLED}.md`;
  return api.pickSave(suggested);
}

/** True when the document is on disk afterwards; false if cancelled or failed. */
async function save(forceSaveAs = false): Promise<boolean> {
  if (!isTauri || currentFile.readOnly) return false;
  const path = await resolveSavePath(forceSaveAs);
  if (!path) return false;

  const text = editor.text();
  let mtimeMs: number;
  try {
    mtimeMs = await api.saveFile(path, text, currentFile.meta);
  } catch (err) {
    await notifyError(`Could not save ${path}\n\n${err}`);
    return false;
  }
  const pathChanged = path !== currentFile.path;
  currentFile.path = path;
  currentFile.mtimeMs = mtimeMs;
  // Stay dirty if the user typed while the save was in flight.
  setDirty(editor.text() !== text);
  updateTitle();
  if (pathChanged) {
    preview.setDocumentFolder(folderOf(path));
    watchCurrentFile();
  }
  return true;
}

function togglePreview() {
  preview.setVisible(!preview.visible);
  if (preview.visible) preview.render(editor.text());
}

// ---- external changes (F8) ----

function watchCurrentFile() {
  if (!isTauri) return;
  if (!currentFile.path) return void api.unwatchFile();
  api.watchFile(currentFile.path).catch(() => {
    // Watching is best-effort (e.g. network shares); editing still works.
  });
}

function onFileChangedOnDisk() {
  clearTimeout(fileChangeTimer);
  fileChangeTimer = window.setTimeout(() => void reconcileWithDisk(), FILE_CHANGE_SETTLE_MS);
}

/** Clean buffer: reload silently. Dirty buffer: ask Reload / Keep mine. Our own saves are ignored by mtime. */
async function reconcileWithDisk() {
  const path = currentFile.path;
  if (!path) return;
  const diskMtime = await api.fileMtime(path);
  if (diskMtime === currentFile.mtimeMs) return;
  if (diskMtime === null) return onDeletedOnDisk();
  if (!currentFile.dirty) return reloadKeepingPosition(path);

  // Ask once per disk version, even if the user keeps their changes.
  currentFile.mtimeMs = diskMtime;
  const reload = await api.askChoice(
    `${fileName(path)} was changed by another program. Reload it and lose your unsaved changes?`,
    'Reload',
    'Keep mine',
  );
  if (reload) await reloadKeepingPosition(path);
}

async function reloadKeepingPosition(path: string) {
  const position = editor.viewPosition();
  try {
    showDocument(await api.openFile(path));
  } catch {
    return; // Mid-write by the other program; the next watcher event retries.
  }
  editor.restoreViewPosition(position);
}

/** Keep the buffer and mark it unsaved, so closing prompts to save it back. */
function onDeletedOnDisk() {
  currentFile.mtimeMs = null;
  setDirty(true);
}

// ---- crash recovery (F13) ----

function snapshotIfDirty() {
  if (!currentFile.dirty) return;
  const text = editor.text();
  if (text === lastSnapshotText) return;
  lastSnapshotText = text;
  void api.snapshotBuffer({ path: currentFile.path, text, meta: currentFile.meta });
}

function clearRecoverySnapshot() {
  if (lastSnapshotText === null) return;
  lastSnapshotText = null;
  void api.clearSnapshot();
}

/** True when the user restored a snapshot left by a session that did not end cleanly. */
async function offerRecovery(): Promise<boolean> {
  const snapshot = await api.readRecovery();
  if (!snapshot) return false;

  const restore = await api.askChoice(
    `Marky did not close cleanly. Restore your unsaved changes to ${fileName(snapshot.path)}?`,
    'Restore',
    'Discard',
  );
  if (!restore) {
    await api.clearSnapshot();
    return false;
  }
  const mtimeMs = snapshot.path ? await api.fileMtime(snapshot.path) : null;
  showDocument({ ...snapshot, readOnly: false, mtimeMs });
  setDirty(true);
  lastSnapshotText = snapshot.text;
  return true;
}

// ---- shortcuts ----

function shortcutAction(e: KeyboardEvent): (() => unknown) | null {
  // A reload would silently drop the buffer; there is nothing to refresh.
  if (e.key === 'F5') return () => {};
  if (!e.ctrlKey || e.altKey) return null;
  const key = e.key.toLowerCase();
  if (key === 'n' && !e.shiftKey) return newDocument;
  if (key === 'w' && !e.shiftKey) return closeWindow;
  if (key === 'o' && !e.shiftKey) return openWithDialog;
  if (key === 'r') return e.shiftKey ? () => {} : openRecent;
  if (key === 's') return () => save(e.shiftKey);
  if (key === 'v' && e.shiftKey) return togglePreview;
  if (key === 'h' && !e.shiftKey) return editor.openReplace;
  return null;
}

// Capture phase so app shortcuts win over the editor and the webview defaults.
window.addEventListener(
  'keydown',
  (e) => {
    const action = shortcutAction(e);
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
    void action();
  },
  { capture: true },
);

// ---- startup ----

function startInBrowser() {
  editor.load(SAMPLE);
  refreshAllViews();
  requestAnimationFrame(() => {
    $('st-perf').textContent = `start ${Math.round(performance.now())} ms (browser)`;
  });
}

async function start() {
  if (!isTauri) return startInBrowser();

  const [, , , startupPath] = await Promise.all([
    api.onOpenRequest((path) => void openPath(path)),
    api.onCloseRequested(() => void closeWindow()),
    api.onFileChanged(onFileChangedOnDisk),
    api.startupFile(),
  ]);
  performance.mark('ipc-ready');

  // Recovery first: opening a file would otherwise clear the snapshot.
  const recovered = await offerRecovery();
  if (startupPath) await openPath(startupPath);
  else if (!recovered) refreshAllViews();
  performance.mark('file-loaded');

  window.setInterval(snapshotIfDirty, SNAPSHOT_INTERVAL_MS);
  measureColdStart((ms) => ($('st-perf').textContent = `start ${Math.round(ms)} ms`));
}

void start();
