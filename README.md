# Marky

A fast, lightweight Markdown editor for Windows, built with [Tauri 2](https://v2.tauri.app/) and [CodeMirror 6](https://codemirror.net/).

Marky opens a `.md` file, lets you edit it with syntax highlighting and a live side-by-side preview, and saves it. It uses the WebView2 runtime that already ships with Windows, so no browser engine is bundled.

> **Status:** early development (M1 MVP). See [PRD.md](PRD.md) for the full plan.

## Benchmarks

Latest run: 27 Sep 2026 on an Intel Core i5-9300H laptop with 16 GB RAM, using a release build with README.md open. Each figure is the median of 5 runs.

| Metric                            | Measured | Target   | Hard limit | Status |
| --------------------------------- | -------- | -------- | ---------- | ------ |
| Installer (NSIS)                  | 1.68 MB  | ≤ 6 MB   | 10 MB      | ✅     |
| App exe                           | 3.75 MB  | ≤ 8 MB   | 12 MB      | ✅     |
| Cold start to editable            | 891 ms   | ≤ 400 ms | 700 ms     | ❌     |
| Preview update after typing stops | ~160 ms  | ≤ 200 ms | 400 ms     | ✅     |
| Idle RAM (app + WebView2)         | 97.5 MB  | ≤ 90 MB  | 120 MB     | ⚠️     |
| Idle CPU                          | 0%       | 0%       | < 0.5%     | ✅     |

Cold start breaks down into about 610 ms for WebView2 to create its browser process (before any app code runs) and about 280 ms in the frontend: loading the bundle, first layout and first paint. The WebView2 part is mostly a fixed cost, so the budget was revised from 400 / 700 ms to 800 / 1,000 ms; remaining gains come from the frontend.

To reproduce:

```bash
npx tauri build --no-bundle
```

```bash
./scripts/bench.ps1
```

Add `-Details` to see the per-phase timeline, or `-BrowserArgs "..."` to try other WebView2 flags without rebuilding.

## Features

Working now:

- Markdown editing with GFM syntax highlighting; fenced code blocks highlighted via lazily loaded language packs
- Live preview rendered in a Web Worker (tables, task lists, strikethrough, autolinks)
- Raw HTML in Markdown is shown as text, never executed
- Light/dark theme that follows the Windows setting
- Find and replace
- Status bar: line/column, word and character count, line ending, encoding
- Open from the command line or Explorer "Open with"; UTF-8 with or without BOM; CRLF/LF preserved on save; non-UTF-8 files open read-only
- Crash-safe saves (temp file, flush, atomic rename; follows symlinks), verified by a test that kills the process mid-save
- Native Save / Don't save / Cancel prompt before closing, opening another file or starting a new one

Built, not yet tested end to end: open dialog, drag-drop, and single instance (a second launch forwards its file to the running window).

## Keyboard shortcuts

| Shortcut        | Action           |
| --------------- | ---------------- |
| Ctrl+N          | New document     |
| Ctrl+O          | Open file        |
| Ctrl+S          | Save             |
| Ctrl+Shift+S    | Save As          |
| Ctrl+W          | Close window     |
| Ctrl+Shift+V    | Toggle preview   |
| Ctrl+F          | Find             |
| Ctrl+H          | Find and replace |
| Ctrl+Z / Ctrl+Y | Undo / redo      |

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

### Commands

```bash
npm install
```

```bash
npm run tauri dev
```

| Command                                           | What it does                                                                                            |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `npm run dev`                                     | Frontend only, in a browser at http://localhost:1420 (loads a sample document; file open/save disabled) |
| `npm run tauri dev`                               | Full native app with hot reload                                                                         |
| `npm run tauri build`                             | Release build and NSIS installer in `src-tauri/target/release/bundle/`                                  |
| `npm run typecheck`                               | TypeScript type-check                                                                                   |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Rust tests: encoding, line endings, atomic save, kill-mid-save                                          |
| `./scripts/e2e.ps1`                               | End-to-end smoke test of the release build (needs `npx tauri build --no-bundle` first)                  |

The status bar shows two development metrics: `start` (process start to first editable frame) and `preview` (last keystroke to preview paint). Set `MARKY_PERF_LOG=<file>` to have the app append its startup timeline to that file.

## Project structure

```
index.html              App shell: editor pane, preview pane, status bar
src/
  main.ts               App wiring: file actions, shortcuts, status bar, startup timing
  editor.ts             CodeMirror 6 setup and highlight theme
  preview.ts            Preview pane; sends text to the worker, swaps in HTML
  preview.worker.ts     markdown-it rendering off the main thread
  ipc.ts                Typed wrappers around Tauri commands and events
  styles.css            Theme variables (light/dark) and preview typography
src-tauri/
  src/main.rs           Commands, path allowlist, dialogs, drag-drop, single instance
  src/files.rs          Read/write with BOM and line-ending handling, atomic replace
  tauri.conf.json       Window, CSP, bundle and file-association config
  capabilities/         Webview permissions (least privilege)
scripts/
  bench.ps1             Release-build benchmark: cold start, WebView2 ready, idle RAM, sizes
  e2e.ps1, e2e.mjs      Release-build smoke test driven over the WebView2 DevTools protocol
```

## Architecture

Rust owns the disk; the webview owns the text. The document lives in CodeMirror state and crosses IPC only on open and save, never per keystroke. Preview parsing runs in a Web Worker at most once per 150 ms of idle typing.

## Security

Every Markdown file is treated as untrusted input:

- The preview renders with `html: false` and never navigates the webview.
- A strict Content Security Policy blocks inline scripts, frames and network access.
- File dialogs run in Rust, and the open/save commands only accept paths the user chose (dialog, drag-drop, command line, Explorer). The webview has no file system or dialog permissions of its own.

## Roadmap

| Milestone | Scope                                                                                                        |
| --------- | ------------------------------------------------------------------------------------------------------------ |
| M0 Spike  | Scaffold, editor + preview worker, measure RAM and cold start                                                |
| M1 MVP    | Open, edit, save, preview, unsaved-changes guard, theme, find                                                |
| M2 Beta   | External change detection, recent files, synced scroll, status bar, local images, crash recovery, installers |
| M3 v1.0   | Code signing, winget, accessibility and IME pass                                                             |
| M4 v1.x   | Tabs, HTML/PDF export, Mermaid and KaTeX, outline, auto-update                                               |

Details are in [PRD.md](PRD.md).
