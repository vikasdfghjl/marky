# AGENTS.md — Marky

Marky is a Windows Markdown editor: Tauri 2 (Rust) + WebView2 + CodeMirror 6 + markdown-it. Its reason to exist is being small and fast. [PRD.md](PRD.md) is the source of truth for scope, milestones and performance budgets; read it before adding a feature or changing a budget-sensitive path.

## Architecture invariants

- **Rust owns the disk; the webview owns the text.** The document lives in CodeMirror state and crosses IPC only on open, save, external change and the 30-second recovery snapshot. Typing stays inside the webview.
- **Paths come only from Rust.** Dialogs, drag-drop, CLI args, single-instance forwards, the recent list and crash recovery call `AppState::remember_user_choice`; `open_file`/`save_file` call `require_user_choice`. Any new way to obtain a path registers it through `remember_user_choice` on the Rust side.
- **Least-privilege webview.** `src-tauri/capabilities/default.json` grants only events and set-title. Prefer a custom command over a plugin permission; when a permission is unavoidable, scope it narrowly and say why in the capability's description.
- **Untrusted Markdown.** The preview renders with `html: false`, never navigates, and runs under the CSP in `tauri.conf.json` (`csp` and `devCsp` change together). Enabling raw HTML means routing output through DOMPurify first.
- **Text model.** The editor always holds LF text. `files.rs` detects BOM and line ending on read and restores them on write, so saving an unedited file reproduces it byte-for-byte. All writes go through `files::write` (temp file, `sync_all`, atomic `fs::rename`). `ReplaceFileW` looks nicer because it keeps ACLs, but a kill mid-call leaves no file; `killed_mid_save_leaves_old_or_new_content` guards this.
- **Preview images are scoped to opened folders.** The asset-protocol scope starts empty; `record_file_access` widens it to a file's folder after a successful open or save. That is the only place it widens.
- **External changes are recognised by mtime.** `watcher.rs` watches the file's folder (saves replace files by rename). The frontend debounces events and compares disk mtime with `currentFile.mtimeMs`, which `save_file` updates, so our own saves never prompt.
- **Startup is one IPC call.** `startup_state` returns the startup file (pre-read on a Rust thread while WebView2 starts) and any recovery snapshot. The file paints first; recovery is offered after. `showDocument` clears only a snapshot this session wrote, so a crash snapshot survives until the user answers.
- **Dirty state has one writer.** `setDirty(dirty, tab)` in `main.ts` updates the tab, the title and the tab strip, and tells Rust (`set_dirty`) only when the window-level "any tab dirty" state flips. Rust then holds the window close only when something is dirty. Route every dirty change through it.
- **Tabs share one editor view.** Each `Tab` owns a CodeMirror `EditorState` (document, undo history, selection). The active tab's `state` field is stale while the view owns it: read the live text with `tabText(tab)`, and switch only through `activate` or `closeTab`. Watchers, recovery snapshots and the Reload / Keep mine prompt are per tab.
- **Callers never branch on the runtime.** `ipc.ts` picks `tauriApi` or `browserApi` once; both implement `MarkyApi`. A new command gets a real implementation and a stand-in, and no code outside `ipc.ts` checks for Tauri.
- **Settings have one store.** `settings.ts` owns the schema and defaults and reads saved JSON field by field; Rust stores it opaquely (`settings.json`, 16 KB cap) and applies the theme to the title bar itself, so the webview needs no set-theme permission. Features react through `applySettings` in `main.ts`. Saves are queued one at a time: async commands can finish out of order and would share `settings.json.tmp`. A new setting needs a field, a default, a reader line and a `data-setting` control in `settings-panel.ts`.
- **Colours are light-dark() pairs.** Each token in `styles.css` is `light-dark(light, dark)`; `theme.ts` sets `data-theme` to force one scheme. Code that draws its own colours (Mermaid) asks `isDark()` and listens with `onThemeChange`.
- **One command table.** Menu items (`menu.rs`) and keyboard shortcuts (`SHORTCUTS`) both dispatch through `commands` in `main.ts`. Menu labels show shortcut hints (`\tCtrl+S`) but register no native accelerators, so each key is handled once, in the frontend. Toggle items are check items bound to a boolean setting in `MENU_TOGGLES`; `applySettings` sends their ticks to Rust (`set_menu_checks`), so a toggle from any source updates the menu.
- **Preview Markdown extensions live in `markdown-plugins.ts`** (frontmatter, GitHub alerts, heading anchors) and must keep `data-line` source maps, because synced scroll and `#anchor` jumps depend on them. Code highlighting (`code-highlight.ts`) reuses the editor's Lezer parsers and `tok-*` classes mapped to the `--hl-*` tokens, so both panes always agree on colours.
- **Links never navigate the webview.** Preview clicks go to `followLink`: http(s) and mailto open through the Rust `open_external` (no opener plugin or JS permission); relative links go through `resolve_link`, which only follows .md/.markdown/.txt targets from user-chosen files.
- **Mermaid draws on the main thread** (it needs the DOM) from placeholders the worker emits, with `securityLevel: 'strict'` and an SVG cache keyed by source and theme. The outline reads headings from the editor's syntax tree and does no work while hidden.
- **Math renders in the worker** (`math.ts`): KaTeX's `renderToString` needs no DOM. It runs with `trust: false`, `throwOnError: false` and a finite `maxSize`, and inline `$` follows pandoc/GitHub rules so prices stay text. KaTeX's CSS and fonts load on the main thread only once `.katex` appears. Mermaid bundles its own KaTeX for diagram labels; the two copies are in separate bundles (worker and page), so an npm override cannot dedupe them. Don't retry that.
- **The preview updates by block.** `preview.worker.ts` renders each top-level block (a level-0 token, or a level-0 open token through its matching close) separately and returns `{html, signature, line}` per block. `preview-blocks.ts` keeps the DOM nodes of every block and, per render, replaces only the middle range between the common prefix and suffix of signatures. The signature hashes the block's HTML with `data-line` written relative to the block's own line, so a line added above only renumbers `data-line` on kept blocks (`followLine`) instead of rebuilding them. Never set `#preview.innerHTML` directly: it discards drawn Mermaid diagrams and reintroduces the full-DOM stall. Anything that must be redrawn for kept blocks (Mermaid SVGs are theme-specific) calls `replaceEverythingNextTime()`. Parsing uses a detached `<div>`, not `<template>`: a template's content lives in a second inert Document (about 2 MB of RAM).
- **The panes are CSS-contained.** `contain: strict` on `#editor`, `#preview` and `#outline` stops an editor keystroke from re-walking the preview DOM (typing at 4 MB: 67 → 19 ms). It relies on the panes getting their size from the flex layout, never from their content; keep that when changing the layout. `content-visibility: auto` on preview blocks was tried and did not help.
- **Lazy by default.** Language packs, Mermaid and KaTeX load through dynamic `import()`. The initial JS (entry chunk plus its modulepreloads) stays ≤ 250 KB gzipped. The preview worker stays a separate emitted file because the CSP allows `worker-src 'self'` only.
- **Markdown gets a slim HTML language.** `@codemirror/lang-markdown` imports `@codemirror/lang-html` only to highlight inline tags, and that package pulls the JavaScript and CSS parsers (~45 KB gzipped) into startup. A resolver in `vite.config.ts` swaps in `markdown-html.ts` for that one importer; ```` ```html ```` fences still lazy-load the real package. Never import `@codemirror/lang-html` statically from app code.
- **The main window is created in code, not from config.** `tauri.conf.json` sets `"create": false`, and `window.rs` builds it in the setup hook with the saved theme (or Windows' app mode, read from the registry) and a matching background colour, so WebView2 never paints its default white before the page loads (it was 630–730 ms of white in dark mode). Change `--bg` in `styles.css` and you change `DARK_BACKGROUND`/`LIGHT_BACKGROUND` in `window.rs` with it. Check with `./scripts/bench-flash.ps1` (expect "white for 0 ms").
- **Minimised windows trim memory.** `memory.rs` sets WebView2's `MemoryUsageTargetLevel` to low on minimise and back to normal on restore (about 104 → 70 MB). Not on focus loss: paging memory back in would make Alt+Tab stutter.

## Performance budgets

Budgets are release-blocking. Measure on a release build (`npx tauri build --no-bundle`), then run `./scripts/bench.ps1`; dev-build numbers are meaningless. `-Details` prints the per-phase timeline written through `MARKY_PERF_LOG`.

The README's **Benchmarks** section is the single record of current numbers. After any benchmark run, update that table, its date and the breakdown sentence in the same change.

Known cost structure: about 520–550 ms of cold start is WebView2 creating its browser process before `setup` runs. Optimise the frontend part: bundle size, first layout, IPC round trips. The WebView2 flags in `tauri.conf.json` (`additionalBrowserArgs`) replace Tauri's defaults, so keep the `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection` prefix. `--enable-features=NetworkServiceInProcess2` runs the network service inside the browser process: one process fewer to start (roughly 50–90 ms) and 5 MB less RAM; Marky makes no network requests, so keep it. Keep the GPU enabled; it protects typing and scroll latency.

Known bundle quirk: Rolldown places its module helpers in a shared d3 chunk (from Mermaid), so the entry preloads about 16 KB gzipped of d3. The initial JS is 175 KB, within budget. Chunk groups can't move the helpers, because they aren't modules; don't retry that.

Large documents (`./scripts/bench-large.ps1`, results in the README): the editor scales, and the preview now updates by block (see the invariant above), which took the longest main-thread stall at 1 MB from 0.9 s to 0.09 s. What is left: the first render still builds the whole preview DOM, so idle RAM grows about 0.4 MB per KB of document (155 MB at 100 KB against the PRD's 105 MB target), and parsing stays O(document) in the worker (1.3 s at 4 MB). Moving parsing to Rust would not fix the DOM cost; rendering only the blocks near the viewport would, but synced scroll, `#anchor` jumps and the outline read the full DOM. Windows trims an untouched window's working set, so RAM for big documents differs before and after any use; the harness reports both. Measure with the harness before and after any preview change.
Profile before optimising: a Chromium startup trace (`--trace-startup=devtools.timeline,disabled-by-default-devtools.timeline.stack --trace-startup-format=json --trace-startup-file=<path>` in `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, then close the window normally so it flushes) shows each forced layout with its JS stack. The file-load task is mostly layout and text shaping, not JavaScript.

Tried with no effect beyond noise, or worse, so don't repeat them without new evidence: V8 explicit compile hints on the startup chunks; pre-reading the startup file (kept for simpler startup, not speed); WebView2 flags `--in-process-gpu`, `--js-flags=--lite-mode`, `--disable-crash-reporter`, `--renderer-process-limit=1`, `--disable-features=StorageServiceOutOfProcess` (the storage service stays out of process). Deferring the editor's `focus()` in `editor.show` to the next frame made startup ~100 ms slower: its synchronous layout is what gets the editor painted before the preview's HTML arrives. Idle RAM is about 90% WebView2 (renderer 44 MB, browser 33 MB, GPU 18 MB). Warm, the same load costs about 15 ms of JS.

## Check the docs first

Tauri 2 and CodeMirror 6 move quickly, and memory of their APIs goes stale. Before using or changing an API from either, read the current reference: [v2.tauri.app/reference](https://v2.tauri.app/reference/) and [codemirror.net/docs/ref](https://codemirror.net/docs/ref/). Also check the installed crate or package source (`~/.cargo/registry/src`, `node_modules`) for exact signatures. Example: CodeMirror's `lineBlockAtHeight` is relative to `documentTop`, not `scrollTop`. Resolve GitHub Action versions from each action's latest release.

## Verification

A change is done when these pass:

```bash
npm run typecheck
```

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

For colour or theme changes, also run `node scripts/contrast.mjs` (WCAG AA, 4.5:1 for all text in both themes).

For changes to file handling, IPC, shortcuts or window lifecycle, also run the end-to-end smoke test against a release build (`npx tauri build --no-bundle`, then `./scripts/e2e.ps1`); every line must read PASS. Extend `scripts/e2e.mjs` when adding a flow it can drive. Native dialogs block it, so it covers only dialog-free paths. For purely visual changes, check `npm run dev` in a browser. CI (`.github/workflows/ci.yml`) runs all of this plus audits, `scripts/check-size.ps1` and the end-to-end test on every push.

## Releasing

Every user-visible change adds a line under `## [Unreleased]` in `CHANGELOG.md` (Keep a Changelog headings: Added, Changed, Fixed, Removed, Security), written for users, not as a commit log. `release.ps1` refuses an empty Unreleased section and stamps it with the version and date; `release.yml` publishes that section as the release notes.

Versions are plain `X.Y.Z`. `package.json` is the only source: `tauri.conf.json` reads it and `scripts/release.ps1` syncs `Cargo.toml`. Never hand-edit versions or create tags directly; run the script, and leave pushing to the user (`git push origin HEAD --follow-tags`). A pushed `vX.Y.Z` tag publishes a GitHub Release through `release.yml`, and 0.x releases are pre-releases. Pre-release suffixes like `-beta.1` are rejected; the NSIS, MSI and winget versioning paths all prefer plain numbers.

The beta ships **unsigned** (PRD decision). Release builds use plain `npx tauri build`, which is what CI does while no `MARKY_SIGN_COMMAND` secret exists. Signing is route-agnostic. `scripts/sign.ps1` runs `MARKY_SIGN_COMMAND` (with a `{file}` placeholder), and `scripts/build-signed.ps1` merges the hook into the config for that build only and then verifies every Authenticode signature. Keep certificates, credentials and the command itself in CI secrets only. winget manifests come from `scripts/winget-manifest.ps1` (schema 1.12.0, validated with `winget validate`).

## Dependencies and licensing

Marky is MIT. After adding, removing or upgrading any npm or Cargo dependency, run `node scripts/third-party-notices.mjs` and commit the regenerated `THIRD_PARTY_NOTICES.txt`; CI fails if it is stale. The file ships in the installer (`bundle.resources`) and the portable ZIP. Prefer MIT, Apache-2.0, BSD, ISC or Zlib dependencies. `package.json` overrides `lodash-es` to ^4.18.1 to clear advisories in Mermaid's `chevrotain` pin; drop the override once Mermaid no longer needs it. Flag anything copyleft (GPL, LGPL, AGPL) before adding it, and keep MPL-2.0 to unmodified crates.

## Environment gotchas

- Shells are PowerShell on Windows. Shells opened before Rust was installed lack cargo on PATH; prepend `$env:USERPROFILE\.cargo\bin`.
- Vite uses port 1420 with `strictPort`. Stop any other dev server before `npm run tauri dev`.
- `npm run dev` runs in a plain browser against `browser-api.ts`, an in-memory stand-in for the Rust core. The sample opens as `C:\Marky\sample.md`; save, Save As, recent files and relative links work in memory; native prompts become `alert`/`confirm`; closing reloads the page. There is no open dialog, file watching or asset protocol.
- Tauri `async` commands that take `State<'_, _>` must return `Result`.
- The `STATIC_VCRUNTIME is deprecated` build warning comes from the Tauri CLI, not this repo.
- markdown-it 15 ships its own types (`import MarkdownIt, { type Token } from 'markdown-it'`); `@types/markdown-it` conflicts with them.
- CodeMirror's DOM exposes `cmTile`, not a view. In browser-side tests, get the view with `EditorView.findFromDOM` (import the module from the URL the app loaded).

## Code style

Pragmatic, readable code that balances SOLID and DRY without overengineering:

- Guard clauses and early returns (`let … else { return }` in Rust) keep functions flat and reject invalid state first.
- Names reveal domain intent (`remember_user_choice`, `renderAfterTypingPause`, `currentFile`).
- Add an abstraction when a second caller needs it, and reuse the ones that exist: `paths.ts` (`fileName`, `folderOf`, `URL_SCHEME`, `joinWindowsPath`), `fenceLanguage` and `dataLineAttr` in `markdown-plugins.ts`, `statusBar`, `showInEditor` for putting a tab into the view, `message_dialog` in `main.rs` for every native message box, and `scripts/lib.ps1` for process trees.
- Comments explain *why*: a budget, a security reason, a platform quirk.
- The frontend is vanilla TypeScript with no UI framework (PRD decision; SolidJS is the fallback if state grows).
