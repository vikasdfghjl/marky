# Marky

A fast, lightweight Markdown editor for Windows, built with [Tauri 2](https://v2.tauri.app/) and [CodeMirror 6](https://codemirror.net/).

Marky opens a `.md` file, lets you edit it with syntax highlighting and a live side-by-side preview, and saves it. It uses the WebView2 runtime that already ships with Windows, so no browser engine is bundled.

> **Status:** early development (M3 release readiness). See [PRD.md](PRD.md) for the full plan.

## Benchmarks

Latest runs: 9 Oct 2026 for sizes, cold start, idle RAM (`./scripts/bench.ps1`) and the preview update (`./scripts/bench-large.ps1`), 29 Sep 2026 for the other rows, on an Intel Core i5-9300H laptop with 16 GB RAM, using a release build with `scripts/fixtures/typical.md` open: a 5 KB document with YAML frontmatter, `bash` and `json` code fences and a table, about the median Markdown file on a developer machine. Each figure is the median of 5–7 runs. The budgets are in [PRD.md](PRD.md), where the evidence for them is too.

| Metric                                 | Measured             | Target   | Hard limit | Status |
| -------------------------------------- | -------------------- | -------- | ---------- | ------ |
| Installer (NSIS)                       | 4.46 MB              | ≤ 6 MB   | 10 MB      | ✅     |
| App exe                                | 6.71 MB              | ≤ 8 MB   | 12 MB      | ✅     |
| Window visible in theme colour         | 187 ms (119–207)     | ≤ 300 ms | 500 ms     | ✅     |
| White flash at launch (dark mode)      | 0 ms                 | 0 ms     | 0 ms       | ✅     |
| Cold start to editable                 | 815 ms (782–849)     | ≤ 900 ms | 1,000 ms   | ✅     |
| Typing latency, p50 (input to painted) | 11 ms                | ≤ 20 ms  | 33 ms      | ✅     |
| Preview update after typing stops      | 42 ms                | ≤ 200 ms | 400 ms     | ✅     |
| Idle RAM in use                        | 99.6 MB              | ≤ 105 MB | 120 MB     | ✅     |
| Idle RAM after 30 s with no input      | 75.3 MB              | ≤ 85 MB  | 100 MB     | ✅     |
| First keystroke after an idle trim     | 35–38 ms             | ≤ 60 ms  | 100 ms     | ✅     |
| Idle CPU                               | 0.25% of the machine | ≈ 0%     | < 0.5%     | ✅     |

Real documents cost a little more than the fixture. On a developer workstation the median real file (5.7 KB, frontmatter plus two fences) idles at 102 MB and the 95th-percentile files (30 KB) at 110–114 MB, inside the 120 MB limit. Each ordinary feature adds memory: a code-fence language 3–6 MB, YAML frontmatter about 6 MB, a table about 2 MB.

Cold start breaks down into about 520–570 ms for WebView2 to create its browser process (before any app code runs) and about 260–290 ms in the frontend: about 80 ms to load and compile the startup JavaScript, then one 60–80 ms task that lays out the opened file. Run-to-run spread on this machine is about ±50 ms, and larger on busy days; it drifts by 1–2 MB and 30 ms from day to day, so compare figures only within one session. Idle RAM is almost all WebView2 itself: the renderer (44 MB), the browser process (33 MB) and the GPU process (18 MB); Marky's own process is 4 MB. After 30 seconds without input (and while minimised) Marky asks WebView2 to trim its memory, which takes a typical document from about 100 MB to about 75 MB; a minimised window drops to about 60–70 MB. Hovering does not count as activity, so a resting mouse does not stop the trim.

**Launch flash.** With Windows in dark mode the window used to show white for 630–730 ms before the UI painted. `window.rs` now creates the window with the theme's background colour, and `./scripts/bench-flash.ps1` measures 0 ms of white. Cold start did not change.

**Larger documents.** Measured with `./scripts/bench-large.ps1` (rows up to 1 MB on 9 Oct 2026, 4 MB and 8 MB on 29 Sep) on generated docs-style Markdown (headings, tables, fenced code in four languages; denser in code than real files, so it overstates cost). The 8 MB file is above the 5 MB threshold, so its preview turns itself off. Files over 100 KB are about 1 in 700 on a real workstation (see the PRD), so these are held to "stays usable", not to a memory budget.

| Document           | Cold start with file | Idle RAM   | Typing p50 / p95 | Preview update after typing stops | Longest main-thread stall |
| ------------------ | -------------------- | ---------- | ---------------- | --------------------------------- | ------------------------- |
| 6 KB (typical)     | 825 ms               | 105 MB     | 19 / 22 ms       | 42 ms (parse 3 ms)                | none                      |
| 100 KB             | 784 ms               | 155 MB     | 18 / 22 ms       | 124 ms (parse 39 ms)              | none                      |
| 1 MB               | 864 ms               | 468 MB     | 19 / 21 ms       | 468 ms (parse 278 ms)             | 112 ms                    |
| 4 MB               | 1,022 ms             | 0.3–1.1 GB | 12 / 27 ms       | 1,439 ms (parse 1,181 ms)         | 544 ms                    |
| 8 MB (preview off) | 1,098 ms             | 197 MB     | 10 / 15 ms       | –                                 | 90 ms                     |

RAM for the 4 MB file varies between runs because Windows trims an untouched window's working set at times, so it is shown as a range. The preview used to replace its whole DOM on every render. It now updates by block: the worker renders each top-level block separately, and the page replaces only the blocks that changed (untouched blocks keep their DOM, drawn diagrams included, and lines added above only renumber `data-line`). CSS containment on the panes also stops editor keystrokes from re-walking the preview. What that changed on the same machine:

| Before → after               | 100 KB       | 1 MB           | 4 MB             |
| ---------------------------- | ------------ | -------------- | ---------------- |
| Longest main-thread stall    | 83 ms → none | 901 → 100 ms   | 3,268 → 544 ms   |
| Typing p50 (containment A/B) | –            | 30 → 18 ms     | 67 → 19 ms       |
| Preview update after typing  | 291 → 198 ms | 1,228 → 488 ms | 4,676 → 1,439 ms |
| RAM after typing 40 keys     | 291 → 176 MB | 1,498 → 567 MB | 2,464 → 1,388 MB |

What is still open: the first render builds the whole preview DOM, so memory grows about 0.4 MB per KB of document (151 MB at 100 KB), and parsing is done over the whole document in the worker (1.2 s at 4 MB). The PRD deliberately does not budget for this; windowed rendering is the fix if large files turn out to matter. Typing latency is input to the next painted frame, so it includes one frame; it was 19–20 ms in an earlier, busier session on the same machine, which is why it is compared only within a session. Opening a 1 MB file costs about 115–125 ms between IPC ready and loaded (budget 150 ms).

**Preview pause.** The preview used to wait a fixed 150 ms after the last keystroke. It now waits twice the last parse time, between 30 and 150 ms, so a typical document keeps up almost at once while a large one keeps the full pause. Same session, before → after: 6 KB 176 → 42 ms, 100 KB 198 → 124 ms, 1 MB 477 → 468 ms, with typing p50 unchanged (6 KB 20.8 → 19.2 ms, 1 MB 20.3 → 18.5 ms). That session was busier than the 29 Sep one, which is why typing reads 18–19 ms here and 11 ms in the older rows.

Two things skew benchmarks, and the scripts now guard against both: a crash-recovery snapshot left by a killed run (an 8.5 MB one once added about 150 ms to every start and 20+ MB of RAM), which the harnesses delete; and Windows trimming an untouched window's memory, which is why RAM is quoted in use and after the idle trim.

To reproduce:

```bash
npx tauri build --no-bundle
```

```bash
./scripts/bench.ps1
```

Add `-Details` to see the per-phase timeline, `-SkipTrimmedSample` to skip the 32 s idle wait, `-File` to benchmark another document, or `-BrowserArgs "..."` to try other WebView2 flags without rebuilding. `./scripts/bench-large.ps1` runs the large-document table, and `./scripts/bench-flash.ps1` checks the launch flash (it needs a visible desktop).

## Install (beta)

Download the latest installer (`Marky_<version>_x64-setup.exe`) or the portable ZIP from the repository's **Releases** page; `SHA256SUMS.txt` there lists the checksums. Every 0.x release is a beta and is marked as a pre-release.

Beta builds are **unsigned**, so Windows SmartScreen shows _Windows protected your PC_ on first run. Choose **More info**, then **Run anyway**. The installer is per-user and needs no admin rights. A portable ZIP (just `marky.exe`) is also built; it needs the WebView2 runtime, which Windows 11 and current Windows 10 already have.

**Reporting a problem.** Marky keeps a small log of crashes and internal errors at `%LOCALAPPDATA%\dev.marky.editor\marky.log` (it never leaves your machine). Attach it to a bug report; it holds error messages (which can include file paths) and timestamps, not the text of your documents. Files over 128 MB are refused rather than risking running out of memory.

## Features

Working now:

- Markdown editing with GFM syntax highlighting; fenced code blocks highlighted via lazily loaded language packs
- Live preview rendered in a Web Worker (tables, task lists, strikethrough, autolinks), scrolled in sync with the editor
- Code blocks highlighted in the preview with the same parsers and colours as the editor (100+ languages, loaded on first use)
- YAML frontmatter highlighted in both panes (SKILL.md, docs sites, AI config files)
- Subscript and superscript: `H~2~O` and `19^th^`. No spaces inside (escape them as `\ `); `~~strikethrough~~` is unaffected. The editor draws them small, below or above the line
- Marked text: `==marked==` is highlighted in both panes; `a == b` stays plain text
- Footnotes: `[^1]` with a `[^1]: …` definition (several paragraphs allowed), or inline `^[…]`, listed at the end of the preview. Click a number to jump to its definition, and ↩ to jump back
- Definition lists: a term line, then `: definition` (or `~ definition`) lines; indent following paragraphs by four spaces to keep them in the definition
- Abbreviations: `*[HTML]: Hyper Text Markup Language` anywhere in the file turns every whole-word HTML outside code into an `<abbr>` with that title on hover
- Smart typography in the preview (on by default, Settings to turn off): "quotes" become “curly”, `--` and `---` become – and —, `...` becomes …, and (c), (r), (tm), +- become ©, ®, ™, ±. Code and math are unaffected
- Mermaid diagrams (` ```mermaid ` fences), drawn with `securityLevel: 'strict'`, following the light/dark theme; loaded only when a document has one, and syntax errors show inline
- Math with KaTeX: `$…$` inline, `$$…$$` blocks and ` ```math ` fences. Prices like $5 stay text, bad TeX shows inline, and MathML is included for screen readers; it loads only for documents with math
- Outline sidebar (Ctrl+Shift+O): headings from the Markdown syntax tree, highlighting the section under the cursor; click to jump
- Links: footnote numbers and ↩ jump within the document, relative `.md` links open in a new tab, and web links open in your browser
- Local images in the preview: relative paths resolve against the file's folder; remote images are blocked
- Raw HTML in Markdown is shown as text, never executed
- Remembers the main window's size, position and maximized state between launches
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
- Settings (gear at the top right, or Ctrl+,): theme (System, Light or Dark, including the title bar and menu), editor font size, word wrap, smart typography, auto-save, the line ending for new files, and editor, preview and outline visibility. Saved to `settings.json` in the app's config folder
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
npm run release -- -Version 0.3.0
```

```bash
git push origin HEAD --follow-tags
```

The script (`scripts/release.ps1`) refuses to run if the Unreleased section is empty. It bumps `package.json` and `Cargo.toml`, renames Unreleased to `[0.3.0] - <date>`, commits, and creates the annotated tag `v0.3.0`. Pushing the tag runs `.github/workflows/release.yml`, which checks that the tag matches the version and that CHANGELOG has a section for it, runs the whole CI workflow on the tag (audits, tests, build, size budgets, end-to-end), and only then publishes the installer and portable ZIP that CI built and tested, with `SHA256SUMS.txt`, as a GitHub Release. The release notes are that version's CHANGELOG section plus install instructions.

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
| `npm run check`                                   | Typecheck, clippy and Rust tests: the checks every change must pass                                                                                       |
| `npm run release -- -Version X.Y.Z`               | Version bump, CHANGELOG stamp, commit and tag (does not push)                                                                                             |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Rust tests: encoding, atomic save, kill-mid-save, recent/recovery                                                                                         |
| `./scripts/e2e.ps1`                               | End-to-end smoke test of the release build (needs `npx tauri build --no-bundle` first)                                                                    |
| `./scripts/check-size.ps1`                        | Fail if the exe or installer exceeds the PRD hard limits (needs a full `npx tauri build`)                                                                 |

The status bar shows two development metrics: `start` (process start to first editable frame) and `preview` (last keystroke to preview paint). Set `MARKY_PERF_LOG=<file>` to have the app append its startup timeline to that file.

## Project structure

```
index.html              App shell: editor pane, preview pane, status bar
src/
  main.ts               App wiring: tabs, save/open, external changes, recovery, the command table
  styles.css            Theme variables (light/dark), layout, picker and preview typography
  platform/             The bridge to the Rust core
    ipc.ts              Typed wrappers around Tauri commands and events; picks the real core or the stand-in
    browser-api.ts      In-memory stand-in for the Rust core, used by `npm run dev`
    sample.ts           The document the stand-in opens
  editor/               CodeMirror
    editor.ts           CodeMirror 6 setup and highlight theme
    markdown-inline-syntax.ts  Editor highlighting for ~sub~, ^sup^ and ==mark==, matching the preview
    markdown-html.ts    Slim HTML language for inline tags in Markdown (keeps JS/CSS parsers out of startup)
  preview/              The preview pane (main thread)
    preview.ts          Sends text to the worker, applies the blocks it returns
    preview-blocks.ts   Block-level DOM diff: replaces only the preview blocks an edit changed
    mermaid-blocks.ts   Lazy Mermaid rendering with an SVG cache
    scroll-sync.ts      Editor-to-preview synced scroll via source-line markers
    worker/             Runs in the preview worker: no DOM, no IPC
      preview.worker.ts markdown-it rendering per block, local-image resolution
      markdown-plugins.ts  Frontmatter, footnotes, shared fence and source-line helpers
      math.ts           `$` / `$$` / `math` parsing and lazy KaTeX rendering
      code-highlight.ts Lezer-based highlighting for code fences
  ui/                   Panels and window chrome
    tab-strip.ts        Tab strip (tablist semantics)
    status-bar.ts       Footer: cursor, counts, file format, timings
    outline.ts          Outline sidebar from the syntax tree
    settings-panel.ts   The gear's settings panel
    recent-picker.ts    Ctrl+R recent-files list
    shortcuts.ts        Keyboard dispatch into the command table (blocks reload keys)
    dom.ts              Element lookup that fails fast
  core/                 Services with no UI of their own
    settings.ts         Settings model, defaults, defensive reading, store
    theme.ts            Effective light/dark theme (System, Light or Dark)
    paths.ts            File-name, folder and path helpers shared by page and worker
    serial-queue.ts     Runs async writes one at a time, in order
    error-log.ts        Sends uncaught errors to marky.log
    idle.ts             Reports going idle / active to Rust after 30 s without input (memory trimming)
    perf.ts             Cold-start instrumentation
src-tauri/
  src/main.rs           Commands, dialogs, window lifecycle, single instance
  src/allowlist.rs      Which files the webview may touch, and which links it may follow
  src/files.rs          Read/write with BOM and line-ending handling, atomic save
  src/watcher.rs        External change detection (watches the file's folder)
  src/store.rs          Recent-files list and crash-recovery snapshots (JSON in app folders)
  src/menu.rs           Native menu bar
  src/window.rs         Creates the main window with the theme's background colour (no launch flash)
  src/memory.rs         Asks WebView2 to trim memory while the window is minimised or the user is idle
  src/diagnostics.rs    marky.log: panics and reported errors, kept on the machine
  tauri.conf.json       Window, CSP, asset protocol, bundle and file-association config
  capabilities/         Webview permissions (least privilege)
scripts/
  fixtures/typical.md   The benchmark document: 5 KB, frontmatter, code fences, a table (about the median real file)
  bench.ps1             Release-build benchmark on scripts/fixtures/typical.md: cold start, WebView2 ready, idle RAM in use and after the idle trim, sizes
  bench-large.ps1, .mjs Large-document benchmark: cold start, RAM, typing latency, preview update, stalls
  bench-flash.ps1       Times the white flash at launch (pixel probe; needs a desktop)
  lib.ps1               Shared script helpers (process-tree walk and stop, Wait-Until)
  cdp.mjs               Minimal DevTools-protocol client shared by e2e.mjs and bench-large.mjs
  e2e.ps1, e2e.mjs      Release-build smoke test driven over the WebView2 DevTools protocol
  check-size.ps1        Size budgets (release-blocking)
  contrast.mjs          WCAG AA contrast audit of the theme tokens
  check-shortcuts.mjs   Fails when menu hints, tooltips or this README disagree with the app's shortcuts
  knip.json             Optional unused-code scan: `npx knip` (scripts/*.mjs are harness entry points)
  sign.ps1              Signing hook (route-agnostic, driven by MARKY_SIGN_COMMAND)
  build-signed.ps1      Signed release build plus signature verification
  winget-manifest.ps1   winget manifests for a published release
  third-party-notices.mjs  THIRD_PARTY_NOTICES.txt from the npm and Cargo dependency graphs
  release.ps1           (npm run release) Version bump + CHANGELOG stamp + commit + annotated tag (does not push)
  release-notes.ps1     A version's CHANGELOG section (release notes); fails if it is missing or empty
  package-portable.ps1  Portable ZIP with LICENSE and notices
.github/workflows/
  ci.yml                Windows CI on main, dev and PRs: checks, tests, audits, build, budgets, e2e, installer + portable ZIP
  dependabot.yml        Weekly npm and Cargo dependency update PRs
  bench.yml             Manual cold-start and size benchmark (Actions → Benchmark)
  release.yml           On tag vX.Y.Z: run ci.yml, then publish the artifacts it tested as a GitHub Release
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
