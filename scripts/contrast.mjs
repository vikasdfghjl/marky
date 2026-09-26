// WCAG 2.2 AA contrast audit of the theme tokens in src/styles.css (both themes).
// Text needs 4.5:1 against every background it is drawn on. Exits 1 on any failure.
import { readFileSync } from 'node:fs';

const css = readFileSync(new URL('../src/styles.css', import.meta.url), 'utf8');

function tokens(block) {
  return Object.fromEntries([...block.matchAll(/--([\w-]+):\s*(#[0-9a-f]{6})/gi)].map((m) => [m[1], m[2]]));
}
const light = tokens(css.slice(0, css.indexOf('@media (prefers-color-scheme: dark)')));
const dark = { ...light, ...tokens(css.slice(css.indexOf('@media (prefers-color-scheme: dark)'))) };

function luminance(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const lin = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}
function ratio(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// [text token, background token] pairs as they are actually drawn.
const syntax = ['hl-heading', 'hl-link', 'hl-code', 'hl-quote', 'hl-muted', 'hl-keyword', 'hl-string', 'hl-number', 'hl-comment', 'hl-fn', 'hl-type', 'hl-prop'];
const pairs = [
  ['fg', 'bg'], ['fg', 'active-line'], ['fg', 'selection'], ['fg', 'panel'], ['fg', 'code-bg'],
  ['muted', 'bg'], ['muted', 'panel'], ['link', 'bg'],
  ...syntax.flatMap((t) => [[t, 'bg'], [t, 'active-line']]),
];

let failures = 0;
for (const [name, theme] of [['light', light], ['dark', dark]]) {
  for (const [text, bg] of pairs) {
    const r = ratio(theme[text], theme[bg]);
    if (r >= 4.5) continue;
    failures++;
    console.log(`FAIL  ${name}: --${text} ${theme[text]} on --${bg} ${theme[bg]} = ${r.toFixed(2)}:1`);
  }
}
console.log(failures ? `${failures} pair(s) below 4.5:1` : `PASS  all ${pairs.length * 2} text/background pairs meet 4.5:1`);
process.exit(failures ? 1 : 0);
