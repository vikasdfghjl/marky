// Large-document benchmark, driven over the WebView2 DevTools protocol (Node 24 has a global WebSocket).
//   node bench-large.mjs gen <kb> <out.md>      writes a docs-like Markdown file of about <kb> KB
//   node bench-large.mjs measure <label>        measures the running app (start it with --remote-debugging-port=9222)
// Measures typing latency (input event to the next painted frame), the preview update after typing
// stops, and main-thread stalls. See scripts/bench-large.ps1, which launches the app per file size.
import { writeFileSync } from 'node:fs';

const [mode, arg1, arg2] = process.argv.slice(2);

if (mode === 'gen') {
  writeFileSync(arg2, generateDocument(Number(arg1) * 1024));
} else if (mode === 'wake') {
  await wake();
} else if (mode === 'measure') {
  await measure(arg1);
} else {
  throw new Error('usage: gen <kb> <out.md> | wake | measure <label>');
}

/**
 * Touches the page once (forcing layout). An untouched app's working set is partly trimmed by
 * Windows, and any use pages it back in, so RAM is reported both before and after this.
 */
async function wake() {
  const page = (await (await fetch('http://127.0.0.1:9222/json')).json()).find((t) => t.type === 'page');
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r) => (ws.onopen = r));
  const done = new Promise((r) => (ws.onmessage = () => r()));
  ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression: 'document.body.offsetHeight' } }));
  await done;
  ws.close();
}

/** API-reference style content: headings, prose, lists, tables, and fenced code in four languages. */
function generateDocument(targetBytes) {
  const code = {
    js: 'export async function load(id) {\n  const res = await fetch(`/api/items/${id}`);\n  if (!res.ok) throw new Error(res.statusText);\n  return res.json();\n}',
    rust: 'pub fn parse(input: &str) -> Result<Config, Error> {\n    let value: toml::Value = input.parse()?;\n    Config::try_from(value)\n}',
    json: '{\n  "name": "service",\n  "retries": 3,\n  "hosts": ["a.example.com", "b.example.com"]\n}',
    bash: 'export SERVICE_URL=https://example.com\ncurl -sf "$SERVICE_URL/health" || exit 1\nsystemctl restart service',
  };
  const languages = Object.keys(code);
  const parts = ['# API reference\n'];
  let bytes = parts[0].length;
  for (let i = 1; bytes < targetBytes; i++) {
    const lang = languages[i % languages.length];
    const section = `
## Section ${i}: configuring the ${lang} client

The \`timeout${i}\` option controls how requests are routed. See [the guide](https://example.com/docs/${i}) and **remember** that _defaults_ differ per environment; the values below are examples, not recommendations.

- First item with \`inline code\` and more words to fill the line
- Second item with a [link to the section](#section-${i}-configuring-the-${lang}-client)
- Third item

| Option | Type | Default |
| --- | --- | --- |
| \`timeout\` | number | ${i} |
| \`retries\` | number | 3 |

\`\`\`${lang}
${code[lang]}
\`\`\`

> **Note**
> Changes take effect after the next restart.
`;
    parts.push(section);
    bytes += section.length;
  }
  return parts.join('');
}

async function measure(label) {
  const port = 9222;
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const page = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find((t) => t.type === 'page');
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
    (await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })).result.result?.value;

  // MARKY_BENCH_CSS="#preview{...}" tries a style change on the running app without a rebuild.
  if (process.env.MARKY_BENCH_CSS) {
    await evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(process.env.MARKY_BENCH_CSS)}; document.head.append(s); })()`);
    await sleep(500);
  }

  // In the page: input -> next painted frame latency, and main-thread stalls over 50 ms.
  await evaluate(`(() => {
    window.__latency = [];
    window.__stalls = [];
    new PerformanceObserver((list) => list.getEntries().forEach((e) => window.__stalls.push(e.duration))).observe({ type: 'longtask' });
    const afterPaint = new MessageChannel();
    afterPaint.port1.onmessage = (e) => window.__latency.push(performance.now() - e.data);
    document.addEventListener('beforeinput', () => {
      const start = performance.now();
      requestAnimationFrame(() => afterPaint.port2.postMessage(start));
    }, true);
  })()`);

  const key = (type, k, code, vk, modifiers = 0) =>
    send('Input.dispatchKeyEvent', { type, modifiers, key: k, code, windowsVirtualKeyCode: vk });
  const percentile = (values, p) => {
    const sorted = [...values].sort((a, b) => a - b);
    return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
  };
  const round = (n) => Math.round(n * 10) / 10;

  await evaluate(`document.querySelector('.cm-content').focus()`);
  await key('keyDown', 'Home', 'Home', 36, 2); // Ctrl+Home: type at the top of the document
  await key('keyUp', 'Home', 'Home', 36, 2);
  await sleep(1500); // let the initial preview render and highlighting settle

  // Burst: keys 70 ms apart stay inside the 150 ms typing pause, so this is editing without preview work.
  await evaluate('window.__latency.length = 0; window.__stalls.length = 0');
  for (let i = 0; i < 30; i++) {
    await send('Input.insertText', { text: 'a' });
    await sleep(70);
  }
  await sleep(1500);
  const burst = await evaluate('window.__latency.slice()');
  const burstStalls = await evaluate('window.__stalls.slice()');

  // Paced: a pause after each key lets the preview re-render every time.
  await evaluate('window.__latency.length = 0; window.__stalls.length = 0');
  const previewMs = [];
  const parseMs = [];
  for (let i = 0; i < 12; i++) {
    await send('Input.insertText', { text: 'b' });
    await sleep(900);
    const status = await evaluate(`document.getElementById('st-render').textContent`);
    const match = /preview (\d+) ms \(parse ([\d.]+)\)/.exec(status ?? '');
    if (match) {
      previewMs.push(Number(match[1]));
      parseMs.push(Number(match[2]));
    }
  }
  const paced = await evaluate('window.__latency.slice()');
  const pacedStalls = await evaluate('window.__stalls.slice()');
  const docKb = Math.round((await evaluate(`document.querySelector('.cm-content').innerText.length`)) / 1024);

  console.log(
    JSON.stringify({
      label,
      typingBurstMs: { p50: round(percentile(burst, 0.5)), p95: round(percentile(burst, 0.95)), max: round(Math.max(...burst)) },
      typingWithPreviewMs: { p50: round(percentile(paced, 0.5)), p95: round(percentile(paced, 0.95)), max: round(Math.max(...paced)) },
      previewAfterTypingStopsMs: previewMs.length ? round(percentile(previewMs, 0.5)) : null,
      parseMs: parseMs.length ? round(percentile(parseMs, 0.5)) : null,
      stallsOver50Ms: { burst: burstStalls.length, withPreview: pacedStalls.length, worst: round(Math.max(0, ...burstStalls, ...pacedStalls)) },
      renderedKbInEditorDom: docKb,
    }),
  );
  ws.close();
}
