# Marky

Marky is a fast Markdown editor for Windows, written in Rust with [GPUI](https://gpui.rs/). It has no WebView2 or bundled browser: the window, editor, preview and menus are drawn with DirectX 11, and a typical document idles around 18 MB.

**v0.4.0** · [Download a release](https://github.com/vikasdfghjl/marky/releases) · [Changelog](CHANGELOG.md) · [Contributor guide](AGENTS.md)

## Features

- **Editing:** syntax highlighting, lists that continue on Enter, find and replace, word wrap, tabs with separate undo histories, drag and drop, opening from Explorer or the command line, ten recent files, and a remembered window size and position. Saves keep the file's BOM and line endings.
- **Preview:** scrolling stays in step with the editor, and only changed blocks are replaced. Tables, task lists, strikethrough, autolinks, footnotes, definition lists, abbreviations, subscript, superscript, marked text, curly quotes and dashes, Mermaid diagrams, and math.
- **File safety:** atomic saves, prompts before discarding unsaved changes, reload or keep when the file changes on disk, auto-save, and crash recovery every 30 seconds. Files over 128 MB are refused before they are read.
- **Privacy:** Markdown HTML is not rendered. Remote images do not load. Web and mail links open outside the app; relative links open only Markdown and text files next to the document.
- **Interface:** light, dark or system theme, high-contrast support, drawn menus, outline, reading view, and a status bar with line, word, character, encoding and line-ending details.
- **Accessibility:** the editor exposes its text and caret to Windows UI Automation; the preview can be read by character, word and line; menus have underlined access keys; the settings panel keeps keyboard focus; links are one preview tab stop rather than many.

Mermaid diagrams are rendered inside `marky.exe` with merman, in the background and only when the preview shows a diagram. There is no separate helper process to install or start.

## Install

Download `Marky_<version>_x64-setup.exe` or `marky-<version>-portable-x64.zip` from [Releases](https://github.com/vikasdfghjl/marky/releases). The installer is per-user, needs no administrator access, and installs under `%LOCALAPPDATA%\Programs\Marky`. It registers Marky with **Open with** for `.md`, `.markdown` and `.txt` without taking over the default. Releases are unsigned; if SmartScreen appears, choose **More info** → **Run anyway**.

Settings and recent files live in `%APPDATA%\dev.marky.editor\`; recovery snapshots and `marky.log` live in `%LOCALAPPDATA%\dev.marky.editor\`. The log stays on your machine and can include file paths from errors, but not document text.

## Benchmarks

Release-build measurements on an i5-9300H with 16 GB RAM, using `scripts/fixtures/typical.md` (5 KB). Values are from **7 Nov 2026**, medians of five runs unless noted. Budgets are release-blocking; the measurement notes are in the [contributor guide](AGENTS.md).

| Metric | Measured | Target | Hard limit |
| --- | ---: | ---: | ---: |
| Installer | 6.27 MB | ≤ 6 MB | 10 MB |
| Program (`marky.exe`) | 21.07 MB | ≤ 22 MB | 24 MB |
| Portable ZIP | 8.78 MB | — | — |
| Startup, ready to edit | 450 ms | ≤ 900 ms | 1,000 ms |
| Typing | 1.8 ms p50 / 2.9 ms p95 | ≤ 20 ms | 33 ms |
| Preview after typing stops | 31 ms | ≤ 200 ms | 400 ms |
| Memory, 10 s after launch | 17.5 MB | ≤ 35 MB | 60 MB |

The program grew to accommodate all Mermaid diagram types in one executable; the accepted reasons are in [DECISION.md](DECISION.md). The installer is 6.27 MB, 0.27 MB over its 6 MB target and under the 10 MB hard limit.

Large-file numbers from the migration prototype (`scripts/bench-large.ps1`, 7 Nov 2026) are in [docs/gpui-migration.md](docs/gpui-migration.md). Re-measure them before changing preview or editor layout.

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
| F3 / Shift+F3 | Find next / previous |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Alt+F, Alt+E, Alt+V, F10 | Open File, Edit or View menu |
| Esc, then Tab | Move focus out of the editor; Tab normally indents |

The preview is one tab stop. With it focused, Left/Right select a link, Enter follows it, Up/Down and Page Up/Page Down scroll, and Home/End go to the top or bottom.

## Development

**Requirements:** Windows 10/11, Rust with the MSVC toolchain, and the C++ build tools. NSIS is needed only to build the installer; CI installs it with Chocolatey. Node.js is needed only to regenerate third-party notices.

Built with [GPUI](https://gpui.rs/) from Zed's repository, [pulldown-cmark](https://docs.rs/pulldown-cmark), [ropey](https://docs.rs/ropey), [RaTeX](https://github.com/erweixin/RaTeX), [merman](https://github.com/Latias94/merman) and tree-sitter.

```bash
cargo run --release -- scripts/fixtures/typical.md
cargo run --release -- path\to\file.md
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --workspace
./scripts/e2e.ps1
```

The same workflow is in the `Makefile`:

```bash
make build
make check
make installer
make size
make e2e
make release VERSION=0.4.0
```

| Command | Purpose |
| --- | --- |
| `./scripts/build-installer.ps1` | Build the per-user NSIS installer after a release build |
| `./scripts/package-portable.ps1` | Build the portable ZIP |
| `./scripts/check-size.ps1` | Enforce program and installer size budgets |
| `./scripts/bench.ps1` | Measure startup, typing, preview and memory |
| `./scripts/bench-large.ps1` | Measure 100 KB to 8 MB documents |
| `npm run notices` | Regenerate notices after dependency changes |

`scripts/e2e.ps1` drives the release build through Windows UI Automation and uses temporary `APPDATA`/`LOCALAPPDATA`, so it does not read or change your settings.

## Known limitations

- IME reconversion is blocked upstream: GPUI's Windows backend at the pinned commit does not handle `WM_IME_REQUEST`. The app answers GPUI's point-to-character request, so the remaining fix belongs in GPUI.
- Menus are drawn in the window, not native Windows menus.
- Releases are unsigned until a signing command is configured.

## License

[MIT](LICENSE). [Third-party notices](THIRD_PARTY_NOTICES.txt) ship with the installer and portable ZIP.
