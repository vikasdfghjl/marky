import type { ViewUpdate } from '@codemirror/view';
import { createEditor } from './editor';
import { createPreview } from './preview';
import { api, isTauri, type OpenedFile } from './ipc';
import { SAMPLE } from './sample';

const TYPING_PAUSE_MS = 150;
/** Above this size the preview starts hidden (PRD: files > 5 MB open with preview off). */
const PREVIEW_AUTO_OFF_BYTES = 5 * 1024 * 1024;
const UNTITLED = 'Untitled';

const $ = (id: string) => document.getElementById(id)!;

/** What the editor shows: a file from disk, or a new untitled document (path null). */
type EditorDocument = Pick<OpenedFile, 'text' | 'meta' | 'readOnly'> & { path: string | null };

const NEW_DOCUMENT: EditorDocument = {
  path: null,
  text: '',
  meta: { bom: false, lineEnding: 'crlf' },
  readOnly: false,
};

const currentFile = {
  path: NEW_DOCUMENT.path,
  meta: NEW_DOCUMENT.meta,
  readOnly: NEW_DOCUMENT.readOnly,
  dirty: false,
};

let typingPauseTimer: number | undefined;
let lastEditAt = 0;

performance.mark('js-start');
const startupLongTasks: string[] = [];
new PerformanceObserver((list) => {
  for (const t of list.getEntries()) startupLongTasks.push(`${Math.round(t.startTime)}+${Math.round(t.duration)}`);
}).observe({ type: 'longtask', buffered: true });
const editor = createEditor($('editor'), onEditorUpdate);
performance.mark('editor-ready');
const preview = createPreview($('preview'), showPreviewLatency);

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
  if (isTauri) void api.setDirty(dirty);
}

function renderAfterTypingPause() {
  const text = editor.text();
  preview.render(text);
  updateWordCount(text);
}

/** Budget: last keystroke to preview paint, including the typing pause. */
function showPreviewLatency(parseMs: number) {
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

// ---- file actions ----

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
  setDirty(false);
  preview.setVisible(editorDocument.text.length <= PREVIEW_AUTO_OFF_BYTES);
  editor.load(editorDocument.text, { readOnly: editorDocument.readOnly });
  performance.mark('editor-loaded');
  refreshAllViews();
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
  try {
    await api.saveFile(path, text, currentFile.meta);
  } catch (err) {
    await notifyError(`Could not save ${path}\n\n${err}`);
    return false;
  }
  currentFile.path = path;
  // Stay dirty if the user typed while the save was in flight.
  setDirty(editor.text() !== text);
  updateTitle();
  return true;
}

function togglePreview() {
  preview.setVisible(!preview.visible);
  if (preview.visible) preview.render(editor.text());
}

// ---- shortcuts ----

function shortcutAction(e: KeyboardEvent): (() => unknown) | null {
  if (!e.ctrlKey || e.altKey) return null;
  const key = e.key.toLowerCase();
  if (key === 'n' && !e.shiftKey) return newDocument;
  if (key === 'w' && !e.shiftKey) return closeWindow;
  if (key === 'o' && !e.shiftKey) return openWithDialog;
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

/** Cold start to editable: process start (Rust) to the first frame after the file is shown. */
async function reportStartupTiming() {
  const nav = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined;
  const marks = performance.getEntriesByType('mark').map((m) => `${m.name}=${Math.round(m.startTime)}`);
  if (nav) marks.unshift(`html=${Math.round(nav.responseEnd)}`, `dcl=${Math.round(nav.domContentLoadedEventEnd)}`);
  for (const p of performance.getEntriesByType('paint')) marks.push(`${p.name}=${Math.round(p.startTime)}`);
  if (startupLongTasks.length) marks.push(`longtasks=${startupLongTasks.join(',')}`);
  const startupMs = await api.startupMs(performance.now(), marks.join(' '));
  $('st-perf').textContent = `start ${Math.round(startupMs)} ms`;
}

function startInBrowser() {
  editor.load(SAMPLE);
  refreshAllViews();
  requestAnimationFrame(() => {
    $('st-perf').textContent = `start ${Math.round(performance.now())} ms (browser)`;
  });
}

async function start() {
  if (!isTauri) return startInBrowser();

  const [, , startupPath] = await Promise.all([
    api.onOpenRequest((path) => void openPath(path)),
    api.onCloseRequested(() => void closeWindow()),
    api.startupFile(),
  ]);
  performance.mark('ipc-ready');
  if (startupPath) await openPath(startupPath);
  else refreshAllViews();
  performance.mark('file-loaded');

  requestAnimationFrame(() => {
    performance.mark('first-frame');
    // A message posted from rAF runs after that frame's paint.
    const afterPaint = new MessageChannel();
    afterPaint.port1.onmessage = () => {
      performance.mark('painted');
      void reportStartupTiming();
    };
    afterPaint.port2.postMessage(null);
  });
}

void start();
