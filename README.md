# Marky

Marky is a Markdown editor for Windows, built with [Tauri 2](https://v2.tauri.app/) and [CodeMirror 6](https://codemirror.net/). It pairs a fast text editor with a live preview and uses the Windows WebView2 runtime instead of bundling a browser. TypeScript runs the interface; Rust handles files, dialogs, and OS integration.

**Beta · v0.3.3** · [Download a release](https://github.com/vikasdfghjl/marky/releases) · [Changelog](CHANGELOG.md) · [Contributor guide](AGENTS.md)

Marky keeps up to 10 recent files in its Open Recent list. It does not add files to the Windows taskbar Jump List.

## Features

- **Editing:** syntax highlighting, find and replace, word wrap, up to 20 tabs with separate undo histories, UTF-8 and BOM support, LF/CRLF preservation, drag-and-drop, command-line and Explorer file opening, recent files, and remembered window size and position.
- **Preview:** synced scrolling and block updates; tables, task lists, strikethrough, autolinks, footnotes, definition lists, abbreviations, subscript, superscript, marked text, optional smart typography, Mermaid diagrams, and KaTeX math.
- **File safety:** atomic saves, unsaved-change prompts, external-change reload or keep, automatic saving, and crash recovery every 30 seconds. Files over 128 MB cannot be opened.
- **Privacy:** Markdown HTML stays disabled. Remote images do not load. Web and email links open outside the app; relative links are limited to user-opened Markdown and text files.
- **Interface:** light, dark, or system theme; native menus; outline; reading view; and status bar with line, word, character, encoding, and line-ending details.

## Install

Download the Windows installer (`Marky_<version>_x64-setup.exe`) or portable ZIP from [Releases](https://github.com/vikasdfghjl/marky/releases). The installer runs per-user and does not need administrator access. Windows 10/11 requires the WebView2 Runtime; the installer can download its bootstrapper if needed. Releases are unsigned beta builds. If SmartScreen appears, choose **More info** → **Run anyway**.

Marky keeps its log at `%LOCALAPPDATA%\dev.marky.editor\marky.log` and settings under `%APPDATA%\dev.marky.editor\`. The log stays on your machine and can include file paths from errors, but not document text.

## Benchmarks

Release-build measurements on an i5-9300H with 16 GB RAM, using `scripts/fixtures/typical.md` (5 KB). Values are medians of 5–7 runs. Size, cold-start, idle-memory, and preview measurements are from **9 Oct 2026**; other measurements are from **29 Sep 2026**. See the contributor guide for budgets and benchmark caveats.

| Metric | Measured | Target | Hard limit |
| --- | ---: | ---: | ---: |
| NSIS installer | 4.46 MB | ≤ 6 MB | 10 MB |
| App executable | 6.71 MB | ≤ 8 MB | 12 MB |
| Window visible in theme colour | 187 ms | ≤ 300 ms | 500 ms |
| Dark-mode white flash | 0 ms | 0 ms | 0 ms |
| Cold start to editable | 815 ms | ≤ 900 ms | 1,000 ms |
| Typing latency (p50) | 11 ms | ≤ 20 ms | 33 ms |
| Preview after typing stops | 42 ms | ≤ 200 ms | 400 ms |
| Idle RAM, in use | 99.6 MB | ≤ 105 MB | 120 MB |
| Idle RAM, after 30 seconds | 75.3 MB | ≤ 85 MB | 100 MB |
| First keystroke after idle trim | 35–38 ms | ≤ 60 ms | 100 ms |
| Idle CPU | 0.25% | ≈ 0% | < 0.5% |

Reproduce on a release build: `npx tauri build --no-bundle`, then run `scripts/bench.ps1` in PowerShell. Add `-Details`, `-File`, or `-SkipTrimmedSample` for additional output and options. For large-file measurements, run `scripts/bench-large.ps1`; preview turns off automatically for files over 5 MB. Update this table, its date, and the measurement summary after every benchmark run.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl+N | New tab |
| Ctrl+O | Open file |
| Ctrl+R | Open recent file |
| Ctrl+S | Save |
| Ctrl+Shift+S | Save as |
| Ctrl+W | Close tab; close the window when it is the last tab |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+Shift+E | Toggle editor (reading view) |
| Ctrl+Shift+V | Toggle preview |
| Ctrl+Shift+O | Toggle outline |
| Alt+Z | Toggle word wrap |
| Ctrl+, | Settings |
| Ctrl+F / Ctrl+H | Find / find and replace |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Esc, then Tab | Move focus out of the editor; Tab normally indents |
| Ctrl+M | Toggle whether Tab indents or moves focus |

## Accessibility

The interface supports keyboard use, including tab navigation, arrow keys, Home/End, and Delete. The preview can receive focus for scrolling, and the editor exposes its text to assistive technology. Contrast meets WCAG 2.2 AA; CI runs `npx tsx scripts/contrast.ts`. The status bar is not a live region.

## Development

**Requirements:** Windows, Node.js 20+, Rust with the MSVC toolchain, C++ build tools, and WebView2.

```bash
npm install
npm run tauri dev    # Run the native app
npm run dev          # Run in a browser with an in-memory stand-in at localhost:1420
npm run check        # Run type checks, unit tests, shortcut checks, Clippy, and Rust tests
```

| Command | Purpose |
| --- | --- |
| `npm run build` | Type-check and build the frontend |
| `npm run tauri build` | Build the Windows installer |
| `npm run test` | Run Vitest (`src/**/*.test.ts`) |
| `npx tsx scripts/third-party-notices.ts` | Regenerate notices after dependency changes |
| `scripts/e2e.ps1` | Run the smoke test (build the release app first) |
| `scripts/check-size.ps1` | Check executable and installer size budgets |
| `scripts/bench.ps1` | Measure startup, typing, preview, and memory |

The browser build uses an in-memory file-system stand-in; it does not support native dialogs, file watching, or the asset protocol.

## Releases and contributions

Create a release with `npm run release -- -Version X.Y.Z`, then push with `git push origin HEAD --follow-tags`. Add user-facing notes under **Unreleased** in [CHANGELOG.md](CHANGELOG.md) before releasing. The release tag runs CI before publishing artifacts. To build a signed installer, configure `MARKY_SIGN_COMMAND` and run `scripts/build-signed.ps1`. Generate winget manifests with `scripts/winget-manifest.ps1`.

After changing npm or Cargo dependencies, regenerate `THIRD_PARTY_NOTICES.txt`. Avoid `windows-core` major versions outside Tauri's 0.62 line. The app code lives in `src/`, Rust commands and file handling in `src-tauri/`, and test, benchmark, and release scripts in `scripts/`. Set `MARKY_PERF_LOG=<file>` to write startup phase timings to a local file.

## License

[MIT](LICENSE). [Third-party notices](THIRD_PARTY_NOTICES.txt) ship with the app.
