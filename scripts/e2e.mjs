// Drives the running app over the WebView2 DevTools protocol (see cdp.mjs).
// Run by e2e.ps1: node e2e.mjs <fixture.md> <marky.exe> <second.md>. Prints one PASS/FAIL line per check.
import { spawn } from 'node:child_process';
import { readFileSync, truncateSync, unlinkSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { connectToPage, sleep } from './cdp.mjs';

const [fixture, exe, secondFixture] = process.argv.slice(2);
const BOM = Buffer.from([0xef, 0xbb, 0xbf]);

const { send, evaluate, close } = await connectToPage();
const invoke = (command, args) =>
  evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args ?? {})})
    .then((v) => ({ ok: v }), (e) => ({ err: String(e) }))`);
const title = () => evaluate('document.title');
const editorText = () => evaluate(`document.querySelector('.cm-content').innerText`);
// CDP modifier bits.
const ALT = 1;
const CTRL = 2;
const pressKey = (key, code, vk, modifiers) =>
  Promise.all(
    ['keyDown', 'keyUp'].map((type) => send('Input.dispatchKeyEvent', { type, modifiers, key, code, windowsVirtualKeyCode: vk })),
  );
const pressCtrl = (key, code, vk) => pressKey(key, code, vk, CTRL);

/**
 * Polls `condition` until it returns something truthy or `timeoutMs` passes, and returns its
 * last result. Waiting for the state a check needs, not a fixed time, keeps the run fast on a
 * quiet machine and stable on a busy one.
 */
const waitFor = async (condition, timeoutMs = 5000) => {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const result = await condition();
    if (result || Date.now() >= deadline) return result;
    await sleep(50);
  }
};
const isClean = async () => (await title()) === 'fixture.md — Marky';
const isDirty = async () => (await title()).startsWith('● ');
const exists = (selector) => evaluate(`!!document.querySelector(${JSON.stringify(selector)})`);
const isHidden = (id) => evaluate(`document.getElementById(${JSON.stringify(id)}).hidden`);

let failures = 0;
const check = (name, ok, detail = '') => {
  if (!ok) failures++;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
};

// ---- open, format detection, preview ----
check('startup file opened', await waitFor(isClean, 30_000), await title());
check(
  'BOM + CRLF detected',
  (await evaluate(`document.getElementById('st-enc').textContent + ' ' + document.getElementById('st-eol').textContent`)) ===
    'UTF-8 BOM CRLF',
);
check('preview rendered', await waitFor(() => exists('#preview h1')));
check(
  'relative image loads from the file folder (F12)',
  await waitFor(() => evaluate(`(() => { const img = document.querySelector('#preview img'); return !!img && img.complete && img.naturalWidth > 0; })()`)),
  await evaluate(`document.querySelector('#preview img')?.src ?? 'no img'`),
);

// ---- edit and save ----
await evaluate(`document.querySelector('.cm-content').focus()`);
await send('Input.insertText', { text: 'Added line\n' });
check('dirty marker after typing', await waitFor(isDirty));

await pressCtrl('s', 'KeyS', 83);
check('clean after Ctrl+S', await waitFor(isClean), await title());
const expected = Buffer.concat([BOM, Buffer.from('Added line\r\n# Fixture\r\n\r\n![dot](img/dot.png)\r\n\r\nBody text\r\n')]);
check('saved bytes keep BOM and CRLF', readFileSync(fixture).equals(expected));

// ---- security ----
const leak = await invoke('open_file', { path: 'C:\\Windows\\win.ini' });
check('unselected path is refused', !!leak.err, leak.err ?? 'LEAKED');

// ---- recent files (F9) ----
const recent = await invoke('recent_files');
check('opened file is in recent files', recent.ok?.[0]?.toLowerCase() === fixture.toLowerCase(), JSON.stringify(recent));

// ---- external change while clean reloads silently (F8) ----
writeFileSync(fixture, Buffer.concat([BOM, Buffer.from('# Changed on disk\r\n')]));
check('clean buffer reloads external change', await waitFor(async () => (await editorText()).includes('Changed on disk')), await editorText());
check('still clean after reload', await isClean(), await title());

// ---- our own save does not trigger a reload prompt ----
await send('Input.insertText', { text: 'x' });
await waitFor(isDirty);
await pressCtrl('s', 'KeyS', 83);
await waitFor(isClean);
// A negative check, so a fixed wait: give the watcher's events for our own write time to arrive and be judged.
await sleep(500);
check('own save is not treated as external', await isClean(), await title());

// ---- IME composition (PRD input requirement) ----
await evaluate(`document.querySelector('.cm-content').focus()`);
await send('Input.imeSetComposition', { text: 'にほんご', selectionStart: 4, selectionEnd: 4 });
await waitFor(async () => (await editorText()).includes('にほんご'), 1000);
await send('Input.insertText', { text: '日本語' }); // commits, replacing the composition
const composed = await waitFor(async () => {
  const text = await editorText();
  return text.includes('日本語') && !text.includes('にほんご') && text;
});
check('IME composition commits without leftovers', !!composed, JSON.stringify(await editorText()));
await waitFor(isDirty);
await pressCtrl('s', 'KeyS', 83);
await waitFor(isClean);
const savedText = readFileSync(fixture, 'utf8');
check('IME text saved as UTF-8', savedText.includes('日本語'), JSON.stringify(savedText));

// ---- crash-recovery snapshot round trip (F13) ----
const snapshots = [{ path: fixture, text: 'recovered text\n', meta: { bom: false, lineEnding: 'lf' } }];
await invoke('snapshot_buffers', { snapshots });
const read = await invoke('startup_state');
check('recovery snapshot round-trips', read.ok?.recovery?.[0]?.text === snapshots[0].text, JSON.stringify(read));
await invoke('snapshot_buffers', { snapshots: [] });
check('recovery snapshot clears', (await invoke('startup_state')).ok?.recovery?.length === 0);

// ---- native menu commands reach the frontend (clicks arrive as 'menu' events) ----
const emitMenu = (id) => invoke('plugin:event|emit', { event: 'menu', payload: id });
await emitMenu('toggle-preview');
const hiddenAfterToggle = await waitFor(() => isHidden('preview'));
await emitMenu('toggle-preview');
check('menu command toggles preview', hiddenAfterToggle && (await waitFor(async () => !(await isHidden('preview')))));
// Hiding the editor leaves a preview-only reading view; Replace then brings the editor back.
const paneState = () => evaluate(`[document.getElementById('editor').hidden, document.getElementById('preview').hidden,
  document.getElementById('editor-toggle').getAttribute('aria-pressed')].join()`);
await emitMenu('toggle-editor');
await waitFor(async () => (await paneState()) === 'true,false,false');
const readingView = await paneState();
check('hiding the editor leaves the preview', readingView === 'true,false,false', readingView);
await emitMenu('replace');
check('menu Replace shows the editor and focuses the replace field',
  await waitFor(async () => !(await isHidden('editor')) &&
    (await evaluate(`document.activeElement?.getAttribute('name') === 'replace'`))));
await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });

// ---- tabs (F14) and single instance: a second launch opens its file in a new tab ----
const tabTitles = () => evaluate(`[...document.querySelectorAll('[role=tab]')].map((t) => t.querySelector('.tab-title').textContent)`);
const tabTitlesAre = async (...expected) => JSON.stringify(await tabTitles()) === JSON.stringify(expected);
const activeTab = () => evaluate(`document.querySelector('[role=tab][aria-selected=true] .tab-title').textContent`);
const activeTabIs = async (name) => (await activeTab()) === name;
// A second launch starts a whole process before it forwards its file, so it gets longer.
const LAUNCH_TIMEOUT_MS = 15_000;
spawn(exe, [secondFixture], { detached: true, stdio: 'ignore' }).unref();
check('second launch opens a new tab', await waitFor(() => tabTitlesAre('fixture.md', 'second.md'), LAUNCH_TIMEOUT_MS), JSON.stringify(await tabTitles()));
check('new tab is active and shows its file',
  await waitFor(async () => (await activeTabIs('second.md')) && (await editorText()).includes('Second file')));
await pressCtrl('Tab', 'Tab', 9);
check('Ctrl+Tab switches back with the text intact',
  await waitFor(async () => (await activeTabIs('fixture.md')) && (await editorText()).includes('日本語')));
spawn(exe, [secondFixture], { detached: true, stdio: 'ignore' }).unref();
check('reopening an open file focuses its tab',
  await waitFor(async () => (await activeTabIs('second.md')) && (await tabTitles()).length === 2, LAUNCH_TIMEOUT_MS));
await pressCtrl('w', 'KeyW', 87);
check('Ctrl+W closes a clean tab', await waitFor(() => tabTitlesAre('fixture.md')), JSON.stringify(await tabTitles()));

// ---- drag and drop overlay (the native drop itself is not scriptable) ----
const dragOverlayShown = () => evaluate(`document.body.classList.contains('drag-over')`);
await invoke('plugin:event|emit', { event: 'drag-hover', payload: true });
const overlayShown = await waitFor(dragOverlayShown);
await invoke('plugin:event|emit', { event: 'drag-hover', payload: false });
check('drag over the window shows the drop overlay', overlayShown && (await waitFor(async () => !(await dragOverlayShown()))));

// ---- Markdown for docs and code: highlighting, frontmatter, footnotes, links ----
await evaluate(`document.querySelector('.cm-content').focus()`);
await pressCtrl('End', 'End', 35);
await send('Input.insertText', {
  text: '\n\n## Docs Section\n\nSee the note.[^e2e]\n\n[^e2e]: A footnote.\n\n```rust\nfn main() { let x = 1; }\n```\n\n[other](second.md) [web](https://example.com)\n',
});
check('code blocks highlighted in preview', await waitFor(() => exists('#preview pre code .tok-keyword')));
check('footnote listed with its source line', await waitFor(() => exists('#preview .footnotes .footnote-item[data-line]')));
await waitFor(() => exists('#preview a[href="second.md"]'));
await evaluate(`document.querySelector('#preview a[href=\"second.md\"]').click()`);
check('relative .md link opens in a new tab', await waitFor(() => activeTabIs('second.md')), JSON.stringify(await tabTitles()));
await pressCtrl('w', 'KeyW', 87);
await waitFor(() => tabTitlesAre('fixture.md'));
// Mermaid loads lazily, draws under the app's CSP, and reports bad syntax inline.
await evaluate(`document.querySelector('.cm-content').focus()`);
await pressCtrl('End', 'End', 35);
await send('Input.insertText', {
  text: '\n\n```mermaid\nflowchart LR\n  A[Editor] --> B[Preview]\n```\n\n```mermaid\nflowchart LR\n  A[ --> \n```\n',
});
// Mermaid is about 1 MB to load and parse.
const MERMAID_TIMEOUT_MS = 15_000;
check('mermaid diagram drawn as SVG', await waitFor(() => exists('#preview .mermaid[data-drawn="svg"] svg'), MERMAID_TIMEOUT_MS));
check('mermaid syntax error shown inline', await waitFor(() => exists('#preview .mermaid-error'), MERMAID_TIMEOUT_MS));

// KaTeX math renders in the worker; its CSS and fonts load lazily under the app's CSP.
await send('Input.insertText', { text: '\nInline $a^2 + b^2 = c^2$ costs $5 and $10.\n\n$$\n\\int_0^1 x\\,dx\n$$\n' });
check('inline and display math rendered',
  await waitFor(async () => (await exists('#preview p .katex')) && (await exists('#preview .katex-display')), 10_000));
check('prices with $ stay plain text', await evaluate(`document.getElementById('preview').textContent.includes('costs $5 and $10')`));
check('KaTeX fonts load under the CSP', await evaluate(`document.fonts.ready.then(() => document.fonts.check('16px KaTeX_Main'))`));

// Incremental preview: an edit replaces only the block it changed. Untouched blocks keep their DOM
// nodes (a drawn Mermaid diagram included), and lines added above only renumber data-line.
await evaluate(`document.querySelectorAll('#preview > *').forEach((node) => (node.__kept = true))`);
await evaluate(`document.querySelector('.cm-content').focus()`);
await pressCtrl('End', 'End', 35);
await send('Input.insertText', { text: '\nOne more paragraph.\n' });
await waitFor(() => evaluate(`[...document.querySelectorAll('#preview > *')].some((n) => !n.__kept && n.textContent.includes('One more paragraph.'))`));
const afterTyping = await evaluate(`(() => {
  const blocks = [...document.querySelectorAll('#preview > *')];
  return { total: blocks.length, rebuilt: blocks.filter((n) => !n.__kept).length,
    diagramKept: !!document.querySelector('#preview .mermaid[data-drawn="svg"]')?.__kept };
})()`);
check('typing rebuilds only the changed preview block', afterTyping.rebuilt <= 2 && afterTyping.diagramKept, JSON.stringify(afterTyping));

const linesBefore = await evaluate(`(() => {
  const nodes = [...document.querySelectorAll('#preview [data-line]')];
  nodes.forEach((node) => (node.__line = Number(node.dataset.line)));
  return nodes.length;
})()`);
const lineShift = () => evaluate(`(() => {
  const nodes = [...document.querySelectorAll('#preview [data-line]')].filter((n) => n.__line !== undefined);
  return { kept: nodes.length, shiftedByOne: nodes.every((n) => Number(n.dataset.line) === n.__line + 1) };
})()`);
await pressCtrl('Home', 'Home', 36);
await send('Input.insertText', { text: '\n' });
await waitFor(async () => (await lineShift()).shiftedByOne);
const afterInsert = await lineShift();
check('a line added above renumbers the preview instead of rebuilding it',
  afterInsert.kept === linesBefore && linesBefore > 5 && afterInsert.shiftedByOne, `${linesBefore} before, ${JSON.stringify(afterInsert)}`);

// Outline sidebar (F17): headings from the syntax tree, click to jump.
const outlineItems = () => evaluate(`[...document.querySelectorAll('#outline button')].map((b) => b.textContent)`);
await emitMenu('toggle-outline');
await waitFor(async () => (await outlineItems()).includes('Docs Section'));
check('outline lists the document headings', (await outlineItems()).includes('Docs Section'), JSON.stringify(await outlineItems()));
await evaluate(`[...document.querySelectorAll('#outline button')].find((b) => b.textContent === 'Docs Section').click()`);
check('outline click moves the editor to the heading',
  await waitFor(async () => (await evaluate(`document.getElementById('st-pos').textContent`)).startsWith('Ln ') &&
    (await evaluate(`document.querySelector('#outline [aria-current]')?.textContent`)) === 'Docs Section'));
await emitMenu('toggle-outline'); // leave it hidden: visibility persists across launches

// Settings: the gear panel (opened here through its menu command) changes the app live.
await emitMenu('settings');
check('Settings opens the settings panel', await waitFor(async () => !(await isHidden('settings-panel'))));
const setSetting = (selector, value) =>
  evaluate(`(() => {
    const control = document.querySelector('#settings-panel ${selector}');
    if (control.type === 'checkbox' || control.type === 'radio') control.checked = ${JSON.stringify(value)};
    else control.value = ${JSON.stringify(value)};
    control.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
await setSetting('[value=dark]', true);
await setSetting('select', '18');
check('theme and font size apply live', await waitFor(async () => (await evaluate(`document.documentElement.dataset.theme`)) === 'dark' &&
  (await evaluate(`getComputedStyle(document.querySelector('.cm-scroller')).fontSize`)) === '18px'));
await setSetting('[data-setting=autoSave]', true);
await evaluate(`document.querySelector('.cm-content').focus()`);
await pressCtrl('End', 'End', 35);
await send('Input.insertText', { text: '\nauto-saved line\n' });
check('auto-save writes the file after typing stops',
  await waitFor(async () => readFileSync(fixture, 'utf8').includes('auto-saved line') && !(await isDirty())), await title());
const settingsFile = `${process.env.APPDATA}\\dev.marky.editor\\settings.json`;
const readSavedSettings = () => {
  try {
    return JSON.parse(readFileSync(settingsFile, 'utf8'));
  } catch {
    return null; // not written yet, or mid-rename
  }
};
const savedSettings = await waitFor(() => {
  const saved = readSavedSettings();
  return saved?.theme === 'dark' && saved?.autoSave === true && saved;
});
check('settings persist to settings.json', !!savedSettings, JSON.stringify(readSavedSettings()));
// Back to defaults, so later checks and the next run start clean.
await setSetting('[value=system]', true);
await setSetting('select', '14');
await setSetting('[data-setting=autoSave]', false);
await evaluate(`document.getElementById('settings-button').click()`);
// Left off on purpose: e2e.ps1 checks that the native menu's Word Wrap tick followed the key.
await evaluate(`document.querySelector('.cm-content').focus()`);
await pressKey('z', 'KeyZ', 90, ALT);
check('Alt+Z turns word wrap off', await waitFor(async () => !(await exists('.cm-content.cm-lineWrapping'))));

const outside = await invoke('resolve_link', { from: fixture, target: '../../../Windows/win.ini' });
check('links to non-Markdown files are refused', !!outside.err, outside.err ?? 'FOLLOWED');
const script = await invoke('open_external', { url: 'javascript:alert(1)' });
const fileUrl = await invoke('open_external', { url: 'file:///C:/Windows/win.ini' });
check('only http(s)/mailto links open externally', !!script.err && !!fileUrl.err);

// Files over the size limit are refused with a message instead of exhausting memory. truncateSync
// extends the file without writing its bytes, so this costs no time or real disk.
const hugeFile = join(dirname(fixture), 'huge.md');
writeFileSync(hugeFile, '');
truncateSync(hugeFile, 129 * 1024 * 1024);
const hugePath = await invoke('resolve_link', { from: fixture, target: 'huge.md' });
const huge = await invoke('open_file', { path: hugePath.ok });
unlinkSync(hugeFile);
check('files over the size limit are refused', /Marky opens files up to 128 MB/.test(huge.err ?? ''), huge.err ?? 'OPENED');

// Diagnostics: errors reported by the frontend land in marky.log in the app's data folder.
const logFile = `${process.env.LOCALAPPDATA}\\dev.marky.editor\\marky.log`;
const probe = `e2e probe ${Date.now()}`;
await invoke('log_error', { message: probe });
const logged = () => {
  try {
    return readFileSync(logFile, 'utf8').includes(probe);
  } catch {
    return false;
  }
};
check('frontend errors reach marky.log', await waitFor(logged));

// Idle memory trimming: the frontend reports going idle and coming back (waiting 30 s here would
// only prove the timer; the RAM effect is measured by bench.ps1).
const wentIdle = await invoke('set_idle', { idle: true });
const cameBack = await invoke('set_idle', { idle: false });
check('idle and active reports reach Rust', !wentIdle.err && !cameBack.err, JSON.stringify([wentIdle, cameBack]));

// ---- leave the buffer dirty for the close-guard check in e2e.ps1 ----
await send('Input.insertText', { text: 'unsaved ' });
check('dirty again before close test', await waitFor(isDirty));

// Recovery snapshots are written on a 30 s timer, and a tick that lands while the buffer is clean
// clears the file. Wait for one that holds this edit so the relaunch check in e2e.ps1 is deterministic.
const recoveryFile = `${process.env.LOCALAPPDATA}\\dev.marky.editor\\recovery.json`;
const recoveryHoldsEdit = () => {
  try {
    return readFileSync(recoveryFile, 'utf8').includes('unsaved ');
  } catch {
    return false; // not written yet
  }
};
check('unsaved edits are snapshotted for crash recovery', await waitFor(recoveryHoldsEdit, 35_000));

close();
process.exit(failures ? 1 : 0);
