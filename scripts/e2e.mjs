// Drives the running app over the WebView2 DevTools protocol (Node 24 has a global WebSocket).
// Usage: node e2e.mjs <fixture.md>. Prints one PASS/FAIL line per check.
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const [fixture, exe, secondFixture] = process.argv.slice(2);
const port = 9222;
const BOM = Buffer.from([0xef, 0xbb, 0xbf]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
ws.onmessage = (e) => {
  const msg = JSON.parse(e.data);
  if (!msg.id || !pending.has(msg.id)) return;
  pending.get(msg.id)(msg);
  pending.delete(msg.id);
};
await new Promise((r) => (ws.onopen = r));

const send = (method, params = {}) =>
  new Promise((r) => {
    const id = ++nextId;
    pending.set(id, r);
    ws.send(JSON.stringify({ id, method, params }));
  });
const evaluate = async (expression) =>
  (await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })).result.result.value;
const invoke = (command, args) =>
  evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args ?? {})})
    .then((v) => ({ ok: v }), (e) => ({ err: String(e) }))`);
const title = () => evaluate('document.title');
const editorText = () => evaluate(`document.querySelector('.cm-content').innerText`);
const pressKey = (key, code, vk, modifiers) =>
  Promise.all(
    ['keyDown', 'keyUp'].map((type) => send('Input.dispatchKeyEvent', { type, modifiers, key, code, windowsVirtualKeyCode: vk })),
  );
const pressCtrl = (key, code, vk) =>
  Promise.all(
    ['keyDown', 'keyUp'].map((type) =>
      send('Input.dispatchKeyEvent', { type, modifiers: 2, key, code, windowsVirtualKeyCode: vk }),
    ),
  );

let failures = 0;
const check = (name, ok, detail = '') => {
  if (!ok) failures++;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
};

// ---- open, format detection, preview ----
check('startup file opened', (await title()) === 'fixture.md — Marky', await title());
check(
  'BOM + CRLF detected',
  (await evaluate(`document.getElementById('st-enc').textContent + ' ' + document.getElementById('st-eol').textContent`)) ===
    'UTF-8 BOM CRLF',
);
check('preview rendered', await evaluate(`!!document.querySelector('#preview h1')`));
await sleep(500);
check(
  'relative image loads from the file folder (F12)',
  await evaluate(`(() => { const img = document.querySelector('#preview img'); return !!img && img.complete && img.naturalWidth > 0; })()`),
  await evaluate(`document.querySelector('#preview img')?.src ?? 'no img'`),
);

// ---- edit and save ----
await evaluate(`document.querySelector('.cm-content').focus()`);
await send('Input.insertText', { text: 'Added line\n' });
await sleep(300);
check('dirty marker after typing', (await title()).startsWith('● '));

await pressCtrl('s', 'KeyS', 83);
await sleep(800);
check('clean after Ctrl+S', (await title()) === 'fixture.md — Marky', await title());
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
await sleep(1000);
check('clean buffer reloads external change', (await editorText()).includes('Changed on disk'), await editorText());
check('still clean after reload', (await title()) === 'fixture.md — Marky', await title());

// ---- our own save does not trigger a reload prompt ----
await send('Input.insertText', { text: 'x' });
await pressCtrl('s', 'KeyS', 83);
await sleep(1000);
check('own save is not treated as external', (await title()) === 'fixture.md — Marky', await title());

// ---- IME composition (PRD input requirement) ----
await evaluate(`document.querySelector('.cm-content').focus()`);
await send('Input.imeSetComposition', { text: 'にほんご', selectionStart: 4, selectionEnd: 4 });
await sleep(150);
await send('Input.insertText', { text: '日本語' }); // commits, replacing the composition
await sleep(300);
const composed = await editorText();
check(
  'IME composition commits without leftovers',
  composed.includes('日本語') && !composed.includes('にほんご'),
  JSON.stringify(composed),
);
await pressCtrl('s', 'KeyS', 83);
await sleep(800);
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
await sleep(200);
const hiddenAfterToggle = await evaluate(`document.getElementById('preview').hidden`);
await emitMenu('toggle-preview');
await sleep(200);
check('menu command toggles preview', hiddenAfterToggle && !(await evaluate(`document.getElementById('preview').hidden`)));
// Hiding the editor leaves a preview-only reading view; Replace then brings the editor back.
await emitMenu('toggle-editor');
await sleep(200);
const readingView = await evaluate(`[document.getElementById('editor').hidden, document.getElementById('preview').hidden,
  document.getElementById('editor-toggle').getAttribute('aria-pressed')].join()`);
check('hiding the editor leaves the preview', readingView === 'true,false,false', readingView);
await emitMenu('replace');
await sleep(200);
check('menu Replace shows the editor and focuses the replace field',
  !(await evaluate(`document.getElementById('editor').hidden`)) &&
  (await evaluate(`document.activeElement?.getAttribute('name') === 'replace'`)));
await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });

// ---- tabs (F14) and single instance: a second launch opens its file in a new tab ----
const tabTitles = () => evaluate(`[...document.querySelectorAll('[role=tab]')].map((t) => t.querySelector('.tab-title').textContent)`);
const activeTab = () => evaluate(`document.querySelector('[role=tab][aria-selected=true] .tab-title').textContent`);
spawn(exe, [secondFixture], { detached: true, stdio: 'ignore' }).unref();
await sleep(2500);
check('second launch opens a new tab', JSON.stringify(await tabTitles()) === JSON.stringify(['fixture.md', 'second.md']), JSON.stringify(await tabTitles()));
check('new tab is active and shows its file', (await activeTab()) === 'second.md' && (await editorText()).includes('Second file'));
await pressKey('Tab', 'Tab', 9, 2);
await sleep(200);
check('Ctrl+Tab switches back with the text intact', (await activeTab()) === 'fixture.md' && (await editorText()).includes('日本語'));
spawn(exe, [secondFixture], { detached: true, stdio: 'ignore' }).unref();
await sleep(2500);
check('reopening an open file focuses its tab', (await tabTitles()).length === 2 && (await activeTab()) === 'second.md');
await pressCtrl('w', 'KeyW', 87);
await sleep(300);
check('Ctrl+W closes a clean tab', JSON.stringify(await tabTitles()) === JSON.stringify(['fixture.md']), JSON.stringify(await tabTitles()));

// ---- drag and drop overlay (the native drop itself is not scriptable) ----
await invoke('plugin:event|emit', { event: 'drag-hover', payload: true });
await sleep(100);
const overlayShown = await evaluate(`document.body.classList.contains('drag-over')`);
await invoke('plugin:event|emit', { event: 'drag-hover', payload: false });
await sleep(100);
check('drag over the window shows the drop overlay', overlayShown && !(await evaluate(`document.body.classList.contains('drag-over')`)));

// ---- Markdown for docs and code: highlighting, frontmatter, alerts, anchors, links ----
await evaluate(`document.querySelector('.cm-content').focus()`);
await pressKey('End', 'End', 35, 2); // Ctrl+End
await send('Input.insertText', {
  text: '\n\n## Docs Section\n\n> [!TIP]\n> Use tabs.\n\n```rust\nfn main() { let x = 1; }\n```\n\n[jump](#docs-section) [other](second.md) [web](https://example.com)\n',
});
await sleep(800);
check('code blocks highlighted in preview', await evaluate(`!!document.querySelector('#preview pre code .tok-keyword')`));
check('GitHub alert rendered as callout', await evaluate(`document.querySelector('#preview .markdown-alert-tip .markdown-alert-title')?.textContent === 'Tip'`));
check('heading gets a GitHub-style anchor id', await evaluate(`!!document.querySelector('#preview h2#docs-section')`));
await evaluate(`document.querySelector('#preview a[href=\"second.md\"]').click()`);
await sleep(1500);
check('relative .md link opens in a new tab', (await activeTab()) === 'second.md', JSON.stringify(await tabTitles()));
await pressCtrl('w', 'KeyW', 87);
await sleep(300);
// Mermaid loads lazily, draws under the app's CSP, and reports bad syntax inline.
await evaluate(`document.querySelector('.cm-content').focus()`);
await pressKey('End', 'End', 35, 2); // Ctrl+End
await send('Input.insertText', {
  text: '\n\n```mermaid\nflowchart LR\n  A[Editor] --> B[Preview]\n```\n\n```mermaid\nflowchart LR\n  A[ --> \n```\n',
});
await sleep(4000);
check('mermaid diagram drawn as SVG', await evaluate(`!!document.querySelector('#preview .mermaid[data-drawn="svg"] svg')`));
check('mermaid syntax error shown inline', await evaluate(`!!document.querySelector('#preview .mermaid-error')`));

// KaTeX math renders in the worker; its CSS and fonts load lazily under the app's CSP.
await send('Input.insertText', { text: '\nInline $a^2 + b^2 = c^2$ costs $5 and $10.\n\n$$\n\\int_0^1 x\\,dx\n$$\n' });
await sleep(3000);
check('inline and display math rendered', await evaluate(`!!document.querySelector('#preview p .katex') && !!document.querySelector('#preview .katex-display')`));
check('prices with $ stay plain text', await evaluate(`document.getElementById('preview').textContent.includes('costs $5 and $10')`));
check('KaTeX fonts load under the CSP', await evaluate(`document.fonts.ready.then(() => document.fonts.check('16px KaTeX_Main'))`));

// Outline sidebar (F17): headings from the syntax tree, click to jump.
await emitMenu('toggle-outline');
await sleep(300);
const outlineItems = await evaluate(`[...document.querySelectorAll('#outline button')].map((b) => b.textContent)`);
check('outline lists the document headings', outlineItems.includes('Docs Section'), JSON.stringify(outlineItems));
await evaluate(`[...document.querySelectorAll('#outline button')].find((b) => b.textContent === 'Docs Section').click()`);
await sleep(300);
check('outline click moves the editor to the heading', (await evaluate(`document.getElementById('st-pos').textContent`)).startsWith('Ln ') &&
  (await evaluate(`document.querySelector('#outline [aria-current]')?.textContent`)) === 'Docs Section');
await emitMenu('toggle-outline'); // leave it hidden: visibility persists across launches

// Settings: the gear panel (opened here through its menu command) changes the app live.
await emitMenu('settings');
await sleep(300);
check('Settings opens the settings panel', await evaluate(`!document.getElementById('settings-panel').hidden`));
const setSetting = (selector, value) =>
  evaluate(`(() => {
    const control = document.querySelector('#settings-panel ${selector}');
    if (control.type === 'checkbox' || control.type === 'radio') control.checked = ${JSON.stringify(value)};
    else control.value = ${JSON.stringify(value)};
    control.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
await setSetting('[value=dark]', true);
await setSetting('select', '18');
await sleep(300);
check('theme and font size apply live', (await evaluate(`document.documentElement.dataset.theme`)) === 'dark' &&
  (await evaluate(`getComputedStyle(document.querySelector('.cm-scroller')).fontSize`)) === '18px');
await setSetting('[data-setting=autoSave]', true);
await evaluate(`document.querySelector('.cm-content').focus()`);
await pressKey('End', 'End', 35, 2); // Ctrl+End
await send('Input.insertText', { text: '\nauto-saved line\n' });
await sleep(2000);
check('auto-save writes the file after typing stops', readFileSync(fixture, 'utf8').includes('auto-saved line') &&
  !(await title()).startsWith('● '), await title());
const settingsFile = `${process.env.APPDATA}\\dev.marky.editor\\settings.json`;
const savedSettings = JSON.parse(readFileSync(settingsFile, 'utf8'));
check('settings persist to settings.json', savedSettings.theme === 'dark' && savedSettings.autoSave === true, JSON.stringify(savedSettings));
// Back to defaults, so later checks and the next run start clean.
await setSetting('[value=system]', true);
await setSetting('select', '14');
await setSetting('[data-setting=autoSave]', false);
await evaluate(`document.getElementById('settings-button').click()`);
// Left off on purpose: e2e.ps1 checks that the native menu's Word Wrap tick followed the key.
await evaluate(`document.querySelector('.cm-content').focus()`);
await pressKey('z', 'KeyZ', 90, 1); // Alt+Z
await sleep(300);
check('Alt+Z turns word wrap off', !(await evaluate(`document.querySelector('.cm-content').classList.contains('cm-lineWrapping')`)));

const outside = await invoke('resolve_link', { from: fixture, target: '../../../Windows/win.ini' });
check('links to non-Markdown files are refused', !!outside.err, outside.err ?? 'FOLLOWED');
const script = await invoke('open_external', { url: 'javascript:alert(1)' });
const fileUrl = await invoke('open_external', { url: 'file:///C:/Windows/win.ini' });
check('only http(s)/mailto links open externally', !!script.err && !!fileUrl.err);

// ---- leave the buffer dirty for the close-guard check in e2e.ps1 ----
await send('Input.insertText', { text: 'unsaved ' });
await sleep(300);
check('dirty again before close test', (await title()).startsWith('● '));

ws.close();
process.exit(failures ? 1 : 0);
