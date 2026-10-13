// Large-document benchmark, driven over the WebView2 DevTools protocol (see cdp.ts).
//   tsx bench-large.ts gen <kb> <out.md>   writes a docs-like Markdown file of about <kb> KB
//   tsx bench-large.ts measure <label>     measures the running app (--remote-debugging-port=9222)
import { writeFileSync } from 'node:fs';
import { connectToPage, sleep } from './cdp';

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

async function wake() {
  const { evaluate, close } = await connectToPage();
  await evaluate('document.body.offsetHeight');
  close();
}

function generateDocument(targetBytes: number) {
  const code: Record<string, string> = {
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

async function measure(label: string) {
  const { send, evaluate, close } = await connectToPage();

  if (process.env.MARKY_BENCH_CSS) {
    await evaluate(
      `(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(process.env.MARKY_BENCH_CSS)}; document.head.append(s); })()`,
    );
    await sleep(500);
  }

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

  const key = (type: string, k: string, code: string, vk: number, modifiers = 0) =>
    send('Input.dispatchKeyEvent', { type, modifiers, key: k, code, windowsVirtualKeyCode: vk });
  const percentile = (values: number[], p: number) => {
    const sorted = [...values].sort((a, b) => a - b);
    return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
  };
  const round = (n: number) => Math.round(n * 10) / 10;

  await evaluate(`document.querySelector('.cm-content').focus()`);
  await key('keyDown', 'Home', 'Home', 36, 2);
  await key('keyUp', 'Home', 'Home', 36, 2);
  await sleep(1500);

  await evaluate('window.__latency.length = 0; window.__stalls.length = 0');
  for (let i = 0; i < 30; i++) {
    await send('Input.insertText', { text: 'a' });
    await sleep(70);
  }
  await sleep(1500);
  const burst = (await evaluate('window.__latency.slice()')) as number[];
  const burstStalls = (await evaluate('window.__stalls.slice()')) as number[];

  await evaluate('window.__latency.length = 0; window.__stalls.length = 0');
  const previewMs: number[] = [];
  const parseMs: number[] = [];
  const readStatus = () => evaluate(`document.getElementById('st-render').textContent`) as Promise<string | undefined>;
  for (let i = 0; i < 12; i++) {
    const before = await readStatus();
    await send('Input.insertText', { text: 'b' });
    for (let waited = 0; waited < 10_000 && (await readStatus()) === before; waited += 100) await sleep(100);
    await sleep(300);
    const status = await readStatus();
    const match = /preview (\d+) ms \(parse ([\d.]+)\)/.exec(status ?? '');
    if (match) {
      previewMs.push(Number(match[1]));
      parseMs.push(Number(match[2]));
    }
  }
  const paced = (await evaluate('window.__latency.slice()')) as number[];
  const pacedStalls = (await evaluate('window.__stalls.slice()')) as number[];
  const docKb = Math.round(Number(await evaluate(`document.querySelector('.cm-content').innerText.length`)) / 1024);

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
  close();
}
