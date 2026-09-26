// Drives the running app over the WebView2 DevTools protocol (Node 24 has a global WebSocket).
const port = 9222;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
ws.onmessage = (e) => {
  const msg = JSON.parse(e.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
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

const ctrlKey = (key, code, vk) =>
  Promise.all(
    ['keyDown', 'keyUp'].map((type) =>
      send('Input.dispatchKeyEvent', { type, modifiers: 2, key, code, windowsVirtualKeyCode: vk }),
    ),
  );

const results = [];
const check = (name, ok, detail = '') => results.push(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);

const mode = process.argv[2];

if (mode === 'edit-save') {
  check('startup file opened', (await evaluate('document.title')) === 'fixture.md — Marky', await evaluate('document.title'));
  check('BOM + CRLF detected', (await evaluate(`document.getElementById('st-enc').textContent + ' ' + document.getElementById('st-eol').textContent`)) === 'UTF-8 BOM CRLF');
  check('preview rendered', await evaluate(`!!document.querySelector('#preview h1')`));

  await evaluate(`document.querySelector('.cm-content').focus()`);
  await send('Input.insertText', { text: 'Added line\n' });
  await sleep(300);
  check('dirty marker after typing', (await evaluate('document.title')).startsWith('● '));

  await ctrlKey('s', 'KeyS', 83);
  await sleep(800);
  check('clean after Ctrl+S', (await evaluate('document.title')) === 'fixture.md — Marky', await evaluate('document.title'));

  const leak = await evaluate(
    `window.__TAURI_INTERNALS__.invoke('open_file', { path: 'C:\\\\Windows\\\\win.ini' }).then(() => 'LEAKED', (e) => 'blocked: ' + e)`,
  );
  check('unselected path is refused', leak.startsWith('blocked'), leak);

  await send('Input.insertText', { text: 'unsaved ' });
  await sleep(300);
  check('dirty again before close test', (await evaluate('document.title')).startsWith('● '));
}

console.log(results.join('\n'));
ws.close();
