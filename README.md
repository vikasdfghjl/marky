# Marky

Marky is a Markdown editor for Windows. It pairs a fast text editor with a live preview, and it uses the Windows WebView2 runtime instead of bundling a browser.

**v0.3.3** · [Download a release](https://github.com/vikasdfghjl/marky/releases) · [Changelog](CHANGELOG.md) · [Contributor guide](AGENTS.md)

## Features

- **Editing:** syntax highlighting, find and replace, word wrap, up to 20 tabs with separate undo histories, drag-and-drop, opening from Explorer or the command line, up to 10 recent files, and a remembered window size and position. Saves keep the file's encoding and line endings.
- **Preview:** scrolling stays in step with the editor, and only the changed part is redrawn. Tables, task lists, strikethrough, autolinks, footnotes, definition lists, abbreviations, subscript, superscript, marked text, optional curly quotes and dashes, Mermaid diagrams, and KaTeX math.
- **File safety:** a save does not leave a half-written file. Prompts before discarding unsaved changes, reload or keep when the file changes on disk, automatic saving, and crash recovery every 30 seconds. Files over 128 MB cannot be opened.
- **Privacy:** Markdown HTML stays disabled. Remote images do not load. Web and email links open outside the app; relative links are limited to Markdown and text files you opened.
- **Interface:** light, dark, or system theme; native menus; outline; reading view; and a status bar with line, word, character, encoding, and line-ending details.

## Install

Download the Windows installer (`Marky_<version>_x64-setup.exe`) or portable ZIP from [Releases](https://github.com/vikasdfghjl/marky/releases). The installer runs per-user and does not need administrator access. Windows 10/11 requires the WebView2 Runtime; the installer can fetch it if needed. Releases are unsigned. If SmartScreen appears, choose **More info** → **Run anyway**.

Marky keeps its log at `%LOCALAPPDATA%\dev.marky.editor\marky.log` and settings under `%APPDATA%\dev.marky.editor\`. The log stays on your machine and can include file paths from errors, but not document text.

## Benchmarks

Release-build measurements on an i5-9300H with 16 GB RAM, using a 5 KB sample (`scripts/fixtures/typical.md`). Values are medians of 5–7 runs. Program size, startup, and memory are from **6 Nov 2026**; the installer and preview are from **9 Oct 2026**; the other rows are from **29 Sep 2026**. Budgets and measurement notes are in the [contributor guide](AGENTS.md).

| Metric | Measured | Target | Hard limit |
| --- | ---: | ---: | ---: |
| Installer | 4.46 MB | ≤ 6 MB | 10 MB |
| Program | 6.74 MB | ≤ 8 MB | 12 MB |
| Window visible in the theme colour | 187 ms | ≤ 300 ms | 500 ms |
| Dark-mode white flash | 0 ms | 0 ms | 0 ms |
| Startup, ready to edit | 863 ms | ≤ 900 ms | 1,000 ms |
| Typing | 11 ms | ≤ 20 ms | 33 ms |
| Preview after typing stops | 42 ms | ≤ 200 ms | 400 ms |
| Memory while in use | 103.7 MB | ≤ 105 MB | 120 MB |
| Memory after 30 seconds idle | 82.2 MB | ≤ 85 MB | 100 MB |
| First keystroke after idle | 35–38 ms | ≤ 60 ms | 100 ms |
| Idle CPU | 0.25% | ≈ 0% | < 0.5% |

On a release build, run `npx tauri build --no-bundle`, then `scripts/bench.ps1`. For large files, run `scripts/bench-large.ps1`. Preview turns off automatically above 5 MB.

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

The app works from the keyboard, including Tab, arrow keys, Home, End, and Delete. The preview can be focused for scrolling, and the editor exposes its text to assistive technology. Text contrast meets WCAG 2.2 AA. The status bar is not announced as it changes.

## Development

**Requirements:** Windows, Node.js 20+, Rust with the MSVC toolchain, C++ build tools, and WebView2.

Built with [Tauri 2](https://v2.tauri.app/) and [CodeMirror 6](https://codemirror.net/). TypeScript runs the interface; Rust handles files, dialogs, and Windows integration.

```bash
npm install
npm run tauri dev    # Run the native app
npm run dev          # Run in a browser at localhost:1420
npm run check        # Type checks, unit tests, shortcut checks, and Rust checks
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

Browser mode keeps files in memory. Dialogs, file watching, and local images in the preview need the native app.

## License

[MIT](LICENSE). [Third-party notices](THIRD_PARTY_NOTICES.txt) ship with the app.
