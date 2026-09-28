# Marky

A fast, lightweight Markdown editor for Windows, built with [Tauri 2](https://v2.tauri.app/) and [CodeMirror 6](https://codemirror.net/).

Marky opens a `.md` file, lets you edit it with syntax highlighting and a live side-by-side preview, and saves it. It uses the WebView2 runtime that already ships with Windows, so no browser engine is bundled.

> **Status:** early development (M3 release readiness). See [PRD.md](PRD.md) for the full plan.

## Benchmarks

Latest run: 28 Sep 2026 on an Intel Core i5-9300H laptop with 16 GB RAM, using a release build with README.md open. Each figure is the median of 7 runs.

| Metric                            | Measured           | Target   | Hard limit | Status |
| --------------------------------- | ------------------ | -------- | ---------- | ------ |
| Installer (NSIS)                  | 4.46 MB            | ≤ 6 MB   | 10 MB      | ✅     |
| App exe                           | 6.70 MB            | ≤ 8 MB   | 12 MB      | ✅     |
| Cold start to editable            | 816 ms (785–962)   | ≤ 800 ms | 1,000 ms   | ⚠️     |
| Preview update after typing stops | ~160 ms            | ≤ 200 ms | 400 ms     | ✅     |
| Idle RAM (app + WebView2)         | 104.3 MB           | ≤ 105 MB | 120 MB     | ✅     |
| Idle CPU                          | 0%                 | 0%       | < 0.5%     | ✅     |

Cold start breaks down into about 520–550 ms for WebView2 to create its browser process (before any app code runs) and about 270 ms in the frontend: 90 ms to load and compile the startup JavaScript, then one ~80 ms task that lays out the opened file. Run-to-run spread on this machine is about ±150 ms. Idle RAM is almost all WebView2 itself: the renderer (44 MB), the browser process (33 MB) and the GPU process (18 MB); Marky's own process is 4 MB. Minimised, Marky asks WebView2 to trim memory and drops to about 70 MB. The 28 Sep performance pass cut cold start from 924 to 816 ms and idle RAM from 110 to 104 MB: the network service now runs inside the browser process (one process fewer to start, 5 MB less), and Markdown no longer pulls the JavaScript and CSS parsers into the startup bundle (initial JS 229 → 175 KB gzipped). Mermaid and KaTeX load only when a document uses them, and code-block parsers load in the preview worker on first use.
To reproduce:

```bash
npx tauri build --no-bundle
```

```bash
./scripts/bench.ps1
```

Add `-Details` to see the per-phase timeline, or `-BrowserArgs "..."` to try other WebView2 flags without rebuilding.

## Install (beta)

Download the latest installer (`Marky_<version>_x64-setup.exe`) or the portable ZIP from the repository's **Releases** page; `SHA256SUMS.txt` there lists the checksums. Every 0.x release is a beta and is marked as a pre-release.

Beta builds are **unsigned**, so Windows SmartScreen shows _Windows protected your PC_ on first run. Choose **More info**, then **Run anyway**. The installer is per-user and needs no admin rights. A portable ZIP (just `marky.exe`) is also built; it needs the WebView2 runtime, which Windows 11 and current Windows 10 already have.

## Features

Working now:

- Markdown editing with GFM syntax highlighting; fenced code blocks highlighted via lazily loaded language packs
- Live preview rendered in a Web Worker (tables, task lists, strikethrough, autolinks), scrolled in sync with the editor
- Code blocks highlighted in the preview with the same parsers and colours as the editor (100+ languages, loaded on first use)
- YAML frontmatter highlighted in both panes (SKILL.md, docs sites, AI config files)
- GitHub alerts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`) rendered as callouts
- Mermaid diagrams (` ```mermaid ` fences), drawn with `securityLevel: 'strict'`, following the light/dark theme; loaded only when a document has one, and syntax errors show inline
- Math with KaTeX, using GitHub's syntax: `$…$` inline, `$$…$$` blocks and ` ```math ` fences. Prices like $5 stay text, bad TeX shows inline, and MathML is included for screen readers; it loads only for documents with math
- Outline sidebar (Ctrl+Shift+O): headings from the Markdown syntax tree, highlighting the section under the cursor; click to jump
- Links: `#heading` links jump within the document (GitHub-style heading ids), relative `.md` links open in a new tab, and web links open in your browser
- Local images in the preview: relative paths resolve against the file's folder; remote images are blocked
- Raw HTML in Markdown is shown as text, never executed
- Light/dark theme that follows the Windows setting
- Find and replace
- Status bar: line/column, word and character count, line ending, encoding
- Open from the command line or Explorer "Open with"; UTF-8 with or without BOM; CRLF/LF preserved on save; non-UTF-8 files open read-only
- Crash-safe saves (temp file, flush, atomic rename; follows symlinks), verified by a test that kills the process mid-save
- Native Save / Don't save / Cancel prompt before closing, opening another file or starting a new one
- External change detection: a clean buffer reloads silently when git or an AI agent rewrites the file; an edited buffer asks Reload / Keep mine
- Tabs: up to 20 files, each with its own undo history, cursor and scroll position. The + after the last tab (or Ctrl+N) opens a new one. Opening a file that is already open focuses its tab; a second launch from Explorer opens in a new tab
- Drag and drop files onto the window to open them, one tab each
- Native menu bar (File, Edit, View, Help), with Alt and arrow-key navigation. Toggles (Editor, Preview, Outline, Word Wrap, Auto Save) show a tick when on
- Reading view: hide the editor (the panel button beside the gear, or Ctrl+Shift+E) to read the preview full width
- Settings (gear at the top right, or Ctrl+,): theme (System, Light or Dark, including the title bar and menu), editor font size, word wrap, auto-save, the line ending for new files, and editor, preview and outline visibility. Saved to `settings.json` in the app's config folder
- Recent files (last 10), Ctrl+R
- Crash recovery: unsaved changes are snapshotted every 30 s and offered back after a crash

Built, not yet tested end to end: the open dialog, the native drop itself (the overlay and the open path are tested), and the Reload / Keep mine and crash-recovery prompts.

Not yet built: taskbar jump-list entries for recent files (F9, needs Win32 code).

## Keyboard shortcuts

| Shortcut        | Action                                              |
| --------------- | --------------------------------------------------- |
| Ctrl+N          | New tab                                             |
| Ctrl+O          | Open file                                           |
| Ctrl+R          | Open recent file                                    |
| Ctrl+S          | Save                                                |
| Ctrl+Shift+S    | Save As                                             |
| Ctrl+W          | Close tab (the last tab closes the window)          |
| Ctrl+Tab        | Next tab (Ctrl+Shift+Tab: previous)                 |
| Ctrl+Shift+E    | Toggle editor (hidden: preview-only reading view)   |
| Ctrl+Shift+V    | Toggle preview                                      |
| Ctrl+Shift+O    | Toggle outline sidebar                              |
| Alt+Z           | Toggle word wrap                                    |
| Ctrl+,          | Settings                                            |
| Ctrl+F          | Find                                                |
| Ctrl+H          | Find and replace                                    |
| Ctrl+Z / Ctrl+Y | Undo / redo                                         |
| Esc, then Tab   | Move focus out of the editor (Tab normally indents) |
| Ctrl+M          | Toggle Tab between indenting and moving focus       |

## Accessibility

- All text meets WCAG 2.2 AA contrast (4.5:1) in both themes; `node scripts/contrast.mjs` checks every text/background pair and runs in CI.
- Everything works from the keyboard. The preview pane is focusable, so it can be scrolled with the arrow and Page keys.
- The editor is exposed to screen readers by CodeMirror. The status bar is deliberately not a live region, so it does not announce every keystroke.
- IME composition (verified with Japanese input in the end-to-end test) commits cleanly and saves as UTF-8.

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) 20 or later
- [Rust](https://rustup.rs/) (stable, MSVC toolchain)
- Microsoft C++ Build Tools with the "Desktop development with C++" workload
- Microsoft Edge WebView2 runtime (preinstalled on Windows 11 and current Windows 10)

On Windows you can install Rust and the build tools with winget:

```bash
winget install --id Rustlang.Rustup -e
```

```bash
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

### Releasing

Versions are plain `X.Y.Z`, and `package.json` is the single source (`tauri.conf.json` points at it). User-visible changes go under **Unreleased** in [CHANGELOG.md](CHANGELOG.md) as they land. To cut a release from a clean `main`:

```bash
./scripts/release.ps1 -Version 0.3.0
```

```bash
git push origin HEAD --follow-tags
```

The script refuses to run if the Unreleased section is empty. It bumps `package.json` and `Cargo.toml`, renames Unreleased to `[0.3.0] - <date>`, commits, and creates the annotated tag `v0.3.0`. Pushing the tag runs `.github/workflows/release.yml`, which checks that the tag matches the version, runs the checks, builds, enforces the size budgets and publishes a GitHub Release with the installer, the portable ZIP and `SHA256SUMS.txt`. The release notes are that version's CHANGELOG section plus install instructions.

- **Signed build** (not used during the unsigned beta): set `MARKY_SIGN_COMMAND` to your signing command with `{file}` where the file path goes, then run `./scripts/build-signed.ps1`. Tauri calls the hook for the app, the installer, the uninstaller and the NSIS plugins, and the script verifies every Authenticode signature afterwards. In CI, set the same value as a repository secret. Plain `npx tauri build` stays unsigned.
- **winget:** after uploading the installer to a release, run `./scripts/winget-manifest.ps1 -InstallerUrl <url>`. It writes validated 1.12.0 manifests to `dist-winget/` for a pull request to microsoft/winget-pkgs.

### Commands

```bash
npm install
```

```bash
npm run tauri dev
```

| Command                                           | What it does                                                                                                                                              |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `npm run dev`                                     | Frontend only, in a browser at http://localhost:1420, against an in-memory stand-in for the Rust core (sample file; save and recent files work in memory) |
| `npm run tauri dev`                               | Full native app with hot reload                                                                                                                           |
| `npm run tauri build`                             | Release build and NSIS installer in `src-tauri/target/release/bundle/`                                                                                    |
| `npm run typecheck`                               | TypeScript type-check                                                                                                                                     |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Rust tests: encoding, atomic save, kill-mid-save, recent/recovery                                                                                         |
| `./scripts/e2e.ps1`                               | End-to-end smoke test of the release build (needs `npx tauri build --no-bundle` first)                                                                    |
| `./scripts/check-size.ps1`                        | Fail if the exe or installer exceeds the PRD hard limits (needs a full `npx tauri build`)                                                                 |

The status bar shows two development metrics: `start` (process start to first editable frame) and `preview` (last keystroke to preview paint). Set `MARKY_PERF_LOG=<file>` to have the app append its startup timeline to that file.

## Project structure

```
index.html              App shell: editor pane, preview pane, status bar
src/
  main.ts               App wiring: documents, save/open, external changes, recovery, shortcuts
  editor.ts             CodeMirror 6 setup and highlight theme
  markdown-html.ts      Slim HTML language for inline tags in Markdown (keeps JS/CSS parsers out of startup)
  preview.ts            Preview pane; sends text to the worker, swaps in HTML
  preview.worker.ts     markdown-it rendering and local-image resolution off the main thread
  markdown-plugins.ts   Frontmatter, GitHub alerts, heading anchors
  code-highlight.ts     Lezer-based code highlighting for preview fences
  mermaid-blocks.ts     Lazy Mermaid rendering with an SVG cache
  math.ts               `$` / `$$` / `math` parsing and lazy KaTeX rendering (in the worker)
  outline.ts            Outline sidebar from the syntax tree
  scroll-sync.ts        Editor-to-preview synced scroll via source-line markers
  recent-picker.ts      Ctrl+R recent-files list
  tab-strip.ts          Tab strip rendering (tablist semantics)
  status-bar.ts         Footer: cursor, counts, file format, timings
  settings.ts           Settings model, defaults, defensive reading, store
  settings-panel.ts     The gear's settings panel
  theme.ts              Effective light/dark theme (System, Light or Dark)
  shortcuts.ts          Keyboard dispatch into the command table (blocks reload keys)
  paths.ts              File-name, folder and path helpers shared by page and worker
  perf.ts               Cold-start instrumentation
  ipc.ts                Typed wrappers around Tauri commands and events; picks the real core or the stand-in
  browser-api.ts        In-memory stand-in for the Rust core, used by `npm run dev`
  styles.css            Theme variables (light/dark), picker and preview typography
src-tauri/
  src/main.rs           Commands, path allowlist, dialogs, window lifecycle, single instance
  src/files.rs          Read/write with BOM and line-ending handling, atomic save
  src/watcher.rs        External change detection (watches the file's folder)
  src/store.rs          Recent-files list and crash-recovery snapshots (JSON in app folders)
  src/menu.rs           Native menu bar
  src/memory.rs         Asks WebView2 to trim memory while the window is minimised
  tauri.conf.json       Window, CSP, asset protocol, bundle and file-association config
  capabilities/         Webview permissions (least privilege)
scripts/
  bench.ps1             Release-build benchmark: cold start, WebView2 ready, idle RAM, sizes
  lib.ps1               Shared script helpers (process-tree walk and stop)
  e2e.ps1, e2e.mjs      Release-build smoke test driven over the WebView2 DevTools protocol
  check-size.ps1        Size budgets (release-blocking)
  contrast.mjs          WCAG AA contrast audit of the theme tokens
  sign.ps1              Signing hook (route-agnostic, driven by MARKY_SIGN_COMMAND)
  build-signed.ps1      Signed release build plus signature verification
  winget-manifest.ps1   winget manifests for a published release
  third-party-notices.mjs  THIRD_PARTY_NOTICES.txt from the npm and Cargo dependency graphs
  release.ps1           Version bump + CHANGELOG stamp + commit + annotated tag (does not push)
  package-portable.ps1  Portable ZIP with LICENSE and notices
.github/workflows/
  ci.yml                Windows CI: checks, tests, audits, build, budgets, e2e, installer + portable ZIP
  release.yml           On tag vX.Y.Z: build and publish the GitHub Release
```

## Architecture

Rust owns the disk; the webview owns the text. The document lives in CodeMirror state and crosses IPC only on open, save, external change and the 30-second recovery snapshot, never per keystroke. Preview parsing runs in a Web Worker at most once per 150 ms of idle typing.

## Security

Every Markdown file is treated as untrusted input:

- The preview renders with `html: false` and never navigates the webview.
- A strict Content Security Policy blocks inline scripts, frames and network access.
- File dialogs run in Rust, and the open/save commands only accept paths the user chose (dialog, drag-drop, command line, Explorer, recent files). The webview has no file system or dialog permissions of its own.
- The preview can load images only from folders of files the user opened; the asset-protocol scope starts empty and widens at runtime.
- Clicked links: only http(s) and mailto open externally; relative links are followed only from an opened file to an existing .md, .markdown or .txt file.

## Roadmap

| Milestone | Scope                                                                                                        |
| --------- | ------------------------------------------------------------------------------------------------------------ |
| M0 Spike  | Scaffold, editor + preview worker, measure RAM and cold start                                                |
| M1 MVP    | Open, edit, save, preview, unsaved-changes guard, theme, find                                                |
| M2 Beta   | External change detection, recent files, synced scroll, status bar, local images, crash recovery, installers |
| M3 v1.0   | Code signing, winget, accessibility and IME pass                                                             |
| M4 v1.x   | Tabs, HTML/PDF export, Mermaid and KaTeX, outline, auto-update                                               |

Details are in [PRD.md](PRD.md).

## License

[MIT](LICENSE). Marky bundles third-party open-source packages; their licenses are in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt), which ships next to `marky.exe` in the installer and the portable ZIP.
