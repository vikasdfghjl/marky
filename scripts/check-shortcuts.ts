// Shortcuts are shown in four places: SHORTCUTS in src/core/constants.ts, the native menu's
// hints in menu.rs, toolbar tooltips in index.html and the README table. This fails when they
// disagree. Exits 1 on any mismatch.
import { readFileSync } from 'node:fs';

const read = (path: string) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');

// Keys CodeMirror's own keymap handles; the menu only mirrors them (see constants.ts).
const EDITOR_KEYS: Record<string, string> = { undo: 'Ctrl+Z', redo: 'Ctrl+Y', find: 'Ctrl+F' };

const shortcutsBlock = read('src/core/constants.ts').match(/export const SHORTCUTS[^{]*\{([^}]*)\}/)?.[1];
if (!shortcutsBlock) throw new Error('SHORTCUTS not found in src/core/constants.ts');
const keyOf: Record<string, string> = { ...EDITOR_KEYS };
for (const [, key, command] of shortcutsBlock.matchAll(/'([^']+)': '([a-z-]+)'/g)) keyOf[command] = key;

const problems: string[] = [];
const expect = (where: string, command: string, shown: string) => {
  const key = keyOf[command];
  if (!key) problems.push(`${where}: shows ${shown} for "${command}", which has no shortcut`);
  else if (shown !== key) problems.push(`${where}: shows ${shown} for "${command}", but the shortcut is ${key}`);
};

// menu.rs: .text("save", "&Save\tCtrl+S"); a toggle may have no hint.
const menuItems = [...read('src-tauri/src/menu.rs').matchAll(/\.(?:text|check)\("([a-z-]+)", "([^"]*)"\)/g)];
for (const [, command, label] of menuItems) {
  const hint = label.split('\\t')[1];
  if (hint) expect('menu.rs', command, hint);
  else if (keyOf[command]) problems.push(`menu.rs: "${command}" has no hint for its shortcut ${keyOf[command]}`);
}

// index.html: <button ... data-command="new" ... title="New tab (Ctrl+N)">
for (const [button] of read('index.html').matchAll(/<button[^>]*>/g)) {
  const command = button.match(/data-command="([^"]+)"/)?.[1];
  const shown = button.match(/title="[^"]*\(([^)]+)\)"/)?.[1];
  if (command && shown) expect('index.html', command, shown);
}

// README: every shortcut appears in the Keyboard shortcuts section.
const readme = read('README.md').match(/## Keyboard shortcuts\r?\n([\s\S]*?)\r?\n## /)?.[1] ?? '';
for (const key of Object.values(keyOf)) {
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  if (!new RegExp(`(?<![\\w+])${escaped}(?![\\w+])`).test(readme)) problems.push(`README.md: ${key} is missing from Keyboard shortcuts`);
}

for (const problem of problems) console.log(`FAIL  ${problem}`);
if (problems.length) process.exit(1);
console.log(`PASS  ${Object.keys(keyOf).length} shortcuts agree across main.ts, menu.rs, index.html and README.md`);
