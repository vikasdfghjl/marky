// Generates THIRD_PARTY_NOTICES.txt: license texts of everything that ships inside Marky.
//   JS:   production npm packages (bundled into the frontend); dev tooling is excluded.
//   Rust: crates linked into the Windows exe (normal edges, no proc-macros or build deps).
// Identical license texts are printed once with every package that uses them.
// Usage: node scripts/third-party-notices.mjs
import { execFileSync, execSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const LICENSE_FILE = /^(licen[cs]e|copying|notice)/i;
const EXEC_OPTIONS = { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 };
// npm is a .cmd shim on Windows, so it runs through the shell as one fixed string.
const npm = (command) => execSync(`npm ${command}`, EXEC_OPTIONS);
// cargo runs without a shell: its --format string contains '|'.
const cargo = (args) => execFileSync('cargo', args, EXEC_OPTIONS);

// Standard texts for crates that declare a license but ship no file; the copyright line comes from their authors.
const TEMPLATES = {
  MIT: (holder) => `MIT License

Copyright (c) ${holder}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`,
  'BSD-3-Clause': (holder) => `BSD 3-Clause License

Copyright (c) ${holder}

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.`,
};

/** Copyright holder from a crate's Cargo.toml `authors`, else "the <name> authors". */
function copyrightHolder(pkg) {
  const manifest = pkg.folder && existsSync(join(pkg.folder, 'Cargo.toml')) ? readFileSync(join(pkg.folder, 'Cargo.toml'), 'utf8') : '';
  const authors = manifest.match(/^authors\s*=\s*\[([^\]]*)\]/m)?.[1];
  const names = authors ? [...authors.matchAll(/"([^"<]+?)\s*(?:<[^>]*>)?"/g)].map((m) => m[1].trim()) : [];
  return names.length ? names.join(', ') : `the ${pkg.id.split(' ')[0]} authors`;
}

function templateText(pkg) {
  const spdx = Object.keys(TEMPLATES).find((id) => new RegExp(`\\b${id}\\b`).test(pkg.license));
  return spdx ? TEMPLATES[spdx](copyrightHolder(pkg)) : null;
}

/** License files in a package folder. Prefers the MIT text when the package offers MIT among alternatives. */
function licenseTexts(folder, spdx) {
  if (!existsSync(folder)) return [];
  const files = readdirSync(folder).filter((f) => LICENSE_FILE.test(f)).sort();
  const mitFile = files.find((f) => /mit/i.test(f));
  const chosen = /\bMIT\b/.test(spdx ?? '') && mitFile ? [mitFile] : files;
  return chosen.map((f) => readFileSync(join(folder, f), 'utf8').replace(/\r\n/g, '\n').trim());
}

function npmPackages() {
  const nodes = JSON.parse(npm('query .prod'));
  return nodes
    .filter((n) => n.path && n.path !== root && n.name)
    .map((n) => ({ id: `${n.name} ${n.version} (npm)`, license: n.license ?? 'UNKNOWN', folder: n.path }));
}

function cratePackages() {
  const registry = join(homedir(), '.cargo', 'registry', 'src');
  const indexes = existsSync(registry) ? readdirSync(registry).map((d) => join(registry, d)) : [];
  const lines = cargo([
    'tree', '--manifest-path', 'src-tauri/Cargo.toml', '--target', 'x86_64-pc-windows-msvc',
    '-e', 'normal,no-proc-macro', '--prefix', 'none', '--no-dedupe', '--format', '{p}|{l}',
  ]).split('\n');

  const seen = new Map();
  for (const line of lines) {
    const [pkg, license] = line.trim().split('|');
    const match = pkg?.match(/^(\S+) v(\S+)/);
    if (!match || match[1] === 'marky') continue;
    const [, name, version] = match;
    const folder = indexes.map((i) => join(i, `${name}-${version}`)).find(existsSync);
    seen.set(`${name} ${version}`, { id: `${name} ${version} (crate)`, license: license || 'UNKNOWN', folder });
  }
  return [...seen.values()];
}

const packages = [...npmPackages(), ...cratePackages()].sort((a, b) => a.id.localeCompare(b.id));

// Group packages by identical license text.
const groups = new Map();
const withoutFile = [];
for (const pkg of packages) {
  const shipped = licenseTexts(pkg.folder ?? '', pkg.license);
  const fallback = shipped.length ? null : templateText(pkg);
  const texts = fallback ? [fallback] : shipped;
  if (texts.length === 0) {
    withoutFile.push(pkg);
    continue;
  }
  const text = texts.join('\n\n---\n\n');
  if (!groups.has(text)) groups.set(text, []);
  groups.get(text).push(pkg);
}

const rule = '='.repeat(80);
let out = `THIRD-PARTY NOTICES

Marky (MIT, see LICENSE) includes the third-party software listed below.
Source code for every package is available from its registry page:
https://crates.io/crates/<name>/<version> for crates and https://www.npmjs.com/package/<name> for npm packages.
${packages.length} packages. Generated by scripts/third-party-notices.mjs; do not edit by hand.
`;
for (const [text, pkgs] of groups) {
  out += `\n${rule}\n${pkgs.map((p) => `${p.id}: ${p.license}`).join('\n')}\n${rule}\n\n${text}\n`;
}
if (withoutFile.length) {
  out += `\n${rule}\nPackages that ship no license file (license as declared by the package):\n${rule}\n\n`;
  out += withoutFile.map((p) => `${p.id}: ${p.license}`).join('\n') + '\n';
}

writeFileSync(join(root, 'THIRD_PARTY_NOTICES.txt'), out);
console.log(`THIRD_PARTY_NOTICES.txt: ${packages.length} packages, ${groups.size} distinct license texts, ${withoutFile.length} without a license file, ${(out.length / 1024).toFixed(0)} KB`);
