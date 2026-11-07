# AGENTS.md — Marky

Marky is a Windows Markdown editor built in Rust with [GPUI](https://gpui.rs/) (Zed's GPU UI framework), with no webview. Its reason to exist is being small and fast. `docs/gpui-migration.md` records how the port was measured and what was decided; this file is the working guide. The app code lives in `src/`, and test, benchmark and release scripts in `scripts/`.

## Architecture invariants

- **Rust owns the disk and the UI.** Documents live in a rope in `buffer.rs`; the editor state, undo history, tabs and selection are Rust. There is no IPC boundary and no webview. Typing stays in the GPUI view.
- **Paths come from the user.** Dialogs (`rfd`), drag and drop, CLI arguments, the recent list, recovery and the single-instance request file are the only ways a path enters. Relative links go through `allowlist::link_target`, which follows only existing `.md`, `.markdown` and `.txt` files next to the document. The preview never navigates.
- **Text model.** The buffer always holds LF text. `files.rs` detects and preserves BOM and line endings, so saving an unedited file reproduces it byte for byte. Every write, documents and JSON files alike, goes through `files::write_atomically` (a temp file unique per write, `sync_all`, atomic `fs::rename`). `killed_mid_save_leaves_old_or_new_content` guards this.
- **Opening is size-limited.** `files::read` refuses files over `MAX_OPEN_BYTES` (128 MB) before reading them. Every way of opening goes through it.
- **Recovery and recent files reuse the app's formats.** `store.rs` writes `recovery.json` and `recent.json`; recovery snapshots are written every 30 s only when a buffer version changed, and are held until the user explicitly restores or discards them. At startup the file path is read before the window opens, so the first frame contains the document.
- **Diagnostics stay on the machine.** `diagnostics.rs` writes `marky.log` in `%LOCALAPPDATA%\dev.marky.editor`: panics (release builds abort and have no console), failed queued writes, and background write failures. Nothing is sent anywhere, and the log is capped. Route a swallowed error that could cost data through it rather than `let _ =`.
- **External changes are recognised by mtime.** `watcher.rs` watches each open document's folder (saves replace files by rename). Clean documents reload; dirty ones prompt Reload / Keep mine. A save updates the document's mtime, so our own saves never prompt.
- **Single instance.** `instance.rs` holds an exclusive handle on `instance.lock` (`share_mode(0)`); a second launch writes its path to `open-request.txt` and exits. Opening the lock briefly retries, because a just-exited process can hold it for a few milliseconds.
- **Tabs have independent state.** Each document has its own buffer, undo history, scroll positions and watcher. The active tab is the one the view reads; switch only through the tab helpers.
- **One command table.** Menu items, toolbar buttons and keyboard shortcuts all run a `Command` in `editor/chrome.rs`. Shortcuts are registered in `src/main.rs` (`key_bindings`); menu mnemonics are actions, not key listeners, so GPUI consumes the key-down instead of typing it. A new action gets a shortcut, a menu entry if it is user-facing, and an end-to-end check where practical.
- **Settings have one store.** `settings.rs` owns the schema and defaults, reads saved JSON field by field, and writes `settings.json` atomically to `%APPDATA%\dev.marky.editor`. Unknown keys are preserved. A first run may read the old prototype's `dev.marky.gpui-prototype` file once, but never writes it. A new setting needs a field, a default, a reader line, a control in the settings panel and a save.
- **Colours are light/dark pairs.** `contrast.rs` checks every text pair at 4.5:1 in both themes and the focus ring at 3:1; `high_contrast.rs` maps the same pairs to Windows' high-contrast system colours. Any new colour must pass the contrast test. Mermaid and math bitmaps are theme-specific and are dropped when the theme changes.
- **Markdown, not a platform's flavour.** The preview renders CommonMark, the GFM basics every tool supports (tables, task lists, strikethrough, autolinks), and syntax several unrelated tools share: footnotes, definition lists, abbreviations, `~sub~`/`^sup^`, `==mark==`, math and Mermaid. GitHub alerts, emoji shortcodes, GitHub heading ids, VuePress containers and markdown-it's `++inserted++` stay out. The test: several unrelated tools render it, and it still reads well in tools that do not.
- **`src/` is grouped by subsystem.** `editor/` holds the editor view and chrome/accessibility helpers; `preview*`, `extras.rs`, `math.rs`, `diagrams.rs` hold the preview; `bitmaps.rs` owns images, formulas and diagrams; `settings`, `contrast`, `high_contrast`, `diagnostics`, `instance`, `perf`, `store`, `watcher`, `files` and `allowlist` have no UI. `main.rs` wires them together.
- **Preview updates by block.** The preview is parsed on a background thread after a pause; each top-level block carries its source line, and only changed blocks are replaced. Parsing uses pulldown-cmark plus `extras.rs` for what it lacks. Keep the line map: synced scrolling, outline jumps and `#fn:` links depend on it.
- **Math renders in Rust.** `math.rs` uses RaTeX with `trust: false`, `throwOnError: false` and a finite `maxSize`; an inline `$` must hug its content, so prices stay text. Formulas are cached by size, text colour and theme; unused bitmaps are dropped after each parse and on font-size/theme changes.
- **Mermaid renders in process.** `diagrams.rs` uses merman with `securityLevel: 'strict'` and `htmlLabels: false`, on the background thread, only for diagrams the virtualised preview shows. Renderers are cached per theme per worker thread; merman's 5 s deadline reports a diagram it cannot finish. The SVG is rasterised here so unused bitmaps are dropped with the rest of the preview's images.
- **The editor and preview are virtualised GPUI lists.** Cost follows the screen, not the file. Source lines are `StyledText`; clicks, the caret and the IME candidate window use GPUI's text layout. The preview pause scales with parse time, 30–150 ms. Measure with `scripts/bench-large.ps1` before changing either bound.
- **Window geometry is persisted.** `main.rs` restores `window.json`; `schedule_geometry_save` writes after a move or resize settles, not every frame. The window is created in code with the saved theme.
- **Accessibility is part of the UI.** Editor text runs carry painted bounds and per-character x offsets/advances so UI Automation has a real caret rectangle. The preview exposes a document with synthetic text runs for character/word/line review, and is one tab stop (Left/Right select links, Enter follows). The settings panel traps focus; menu access keys are underlined and registered actions. A new control needs a role, a name and its state.

## Performance budgets

Budgets are release-blocking. The README's **Benchmarks** section is the single record of current numbers; update that table, its date and the measurement sentence after any benchmark run.

- Program `marky.exe`: ≤ 24 MB hard, ≤ 22 MB target. It includes all Mermaid diagram types; the accepted overrun is recorded in `DECISION.md`.
- Installer: ≤ 6 MB target, ≤ 10 MB hard.
- Startup ≤ 900 ms target / 1,000 ms hard; memory ≤ 35 MB target / 60 MB hard; typing ≤ 20 ms target / 33 ms hard; preview ≤ 200 ms target / 400 ms hard.

Measure a release build (`cargo build --release --workspace`), then `./scripts/bench.ps1`; large files use `./scripts/bench-large.ps1`; the launch-flash check is `./scripts/bench-flash.ps1`. Dev-build numbers are meaningless.

Benchmark pitfalls, each of which has produced a wrong conclusion here: (1) a crash-recovery snapshot left in `%LOCALAPPDATA%\dev.marky.editor\recovery.json` is loaded at every launch, adds memory and can pop a recovery prompt. The e2e harness uses temporary folders; a manual run should delete it and any `instance.lock` first. (2) Windows trims an untouched window's working set, so RAM differs before and after use; `bench-large.ps1` reports both. (3) Background load moves typing latency by up to 2× between sessions; compare only within one session. (4) PowerShell `Process.MainWindowHandle` can return a hidden window; find the visible window by class or enumerate visible windows. (5) Don't wait a fixed time for the preview or the window; wait for the status bar or a title change (`Wait-Until` in `scripts/lib.ps1`). (6) `SendKeys` needs the window in front; `-Via Messages` is useful for stalls but its typing numbers are not comparable.

Large documents: virtualisation keeps typing and startup independent of file size, but text, parsed blocks, line index and images still grow with the document (about 13 MB per MB before syntax highlighting). The preview builds all blocks, so idle memory grows with the document; measure before and after any preview change.

## Verification

A change is done when these pass (the same commands are in the `Makefile`):

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

```bash
cargo build --release --workspace
```

For colour or theme changes, the contrast test is part of `cargo test`. For file handling, window lifecycle, shortcuts or packaging, also run the end-to-end smoke test against a release build:

```bash
./scripts/e2e.ps1
```

Every line must read PASS or SKIP with a documented reason. Native dialogs block the harness, so it covers only dialog-free paths; extend `scripts/e2e.ps1` when adding a flow it can drive. After packaging changes, run `./scripts/build-installer.ps1`, `./scripts/check-size.ps1` and `./scripts/package-portable.ps1`. CI (`.github/workflows/ci.yml`) runs clippy, tests, the notice check, the release build, the installer, size budgets, e2e and the portable ZIP on pushes to `main` and `dev` and on pull requests. Harness scripts wait for state, never a fixed time: a cold CI runner starts GPUI far slower than this laptop.

## Releasing

Every user-visible change adds a line under `## [Unreleased]` in `CHANGELOG.md` (Keep a Changelog headings: Added, Changed, Fixed, Removed, Security), written for users, not as a commit log. `release.ps1` refuses an empty Unreleased section and stamps it with the version and date; `release.yml` publishes that section as the release notes.

Versions are plain `X.Y.Z`. `Cargo.toml` is the only source: `scripts/release.ps1` syncs `Cargo.lock`, `package.json` and `package-lock.json`. Never hand-edit versions or create tags directly. Create a release with `npm run release -- -Version X.Y.Z`, then push with `git push origin HEAD --follow-tags`. A pushed `vX.Y.Z` tag publishes a GitHub Release through `release.yml`, which first runs the whole of `ci.yml` on the tag and publishes the artifacts that run built and tested; never add a publish path that skips it. Suffixes such as `-beta.1` are rejected; NSIS and winget both prefer plain numbers.

The installer is per-user NSIS (`installer/marky.nsi`), built by `scripts/build-installer.ps1`; it carries Marky, the licence and the notices, and registers the app with **Open with**. The portable ZIP carries the same files. Releases ship **unsigned** until a signing command is configured. Signing is route-agnostic: `scripts/sign.ps1` runs `MARKY_SIGN_COMMAND` (with a `{file}` placeholder), and `scripts/build-signed.ps1` builds, signs the program before the installer, signs the installer, then verifies every Authenticode signature. Keep certificates, credentials and the command in CI secrets only. winget manifests come from `scripts/winget-manifest.ps1` (schema 1.12.0, validated with `winget validate`).

## Dependencies and licensing

Marky is MIT. After adding, removing or upgrading any Rust dependency, run `npm run notices` and commit the regenerated `THIRD_PARTY_NOTICES.txt`; CI fails if it is stale. The file ships in the installer and portable ZIP. Prefer MIT, Apache-2.0, BSD, ISC or Zlib dependencies. Flag anything copyleft (GPL, LGPL, AGPL) before adding it. GPUI is Apache-2.0 and pinned to one Zed commit (`Cargo.toml`); moving the pin needs a build, the tests and a startup measurement, and an entry in `DECISION.md`. Zed's own editor/markdown crates are GPL: read them, do not copy them.

## Environment gotchas

- Shells are PowerShell on Windows. Shells opened before Rust was installed can lack cargo on PATH; prepend `$env:USERPROFILE\.cargo\bin`.
- `cargo run --release` without a path opens an Untitled tab, as the Start menu shortcut does; pass `scripts/fixtures/typical.md` (or use `make run`) for the fixture. Never default to a path from the build machine: `env!` paths are baked into the shipped program.
- NSIS is needed for `build-installer.ps1`; CI installs it with Chocolatey. `makensis` from PATH or the standard NSIS install folders is used.
- Node is only for regenerating notices; there is no frontend build, dev server or webview runtime to install.

## Code style

Pragmatic, readable code that balances SOLID and DRY without overengineering:

- Guard clauses and early returns (`let … else { return }` in Rust) keep functions flat and reject invalid state first.
- Names reveal domain intent (`files::write_atomically`, `schedule_geometry_save`, `apply_settings`).
- Add an abstraction when a second caller needs it, and reuse the ones that exist: `files::same_path`, `allowlist::link_target`, `store`'s JSON readers, `scripts/lib.ps1` for process trees and waits.
- Comments explain *why*: a budget, a security reason, a platform quirk.
- The preview and editor must agree on what is markup; share the parser's rules rather than duplicating them.
