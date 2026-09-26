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
- **One command table.** Menu items (`menu.rs`) and keyboard shortcuts (`SHORTCUTS`) both dispatch through `commands` in `main.ts`. Menu labels show shortcut hints (`\tCtrl+S`) but register no native accelerators, so each key is handled once, in the frontend.
- **Lazy by default.** Language packs, and later KaTeX and Mermaid, load through dynamic `import()`. The initial JS (entry chunk plus its modulepreloads) stays ≤ 250 KB gzipped. The preview worker stays a separate emitted file because the CSP allows `worker-src 'self'` only.

## Performance budgets

Budgets are release-blocking. Measure on a release build (`npx tauri build --no-bundle`), then run `./scripts/bench.ps1`; dev-build numbers are meaningless. `-Details` prints the per-phase timeline written through `MARKY_PERF_LOG`.

The README's **Benchmarks** section is the single record of current numbers. After any benchmark run, update that table, its date and the breakdown sentence in the same change.

Known cost structure: about 610 ms of cold start is WebView2 creating its browser process before `setup` runs, and flags don't move it. Optimise the frontend part: bundle size, first layout, IPC round trips. The WebView2 flags in `tauri.conf.json` (`additionalBrowserArgs`) replace Tauri's defaults, so keep the `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection` prefix. Keep the GPU enabled; it protects typing and scroll latency.

Tried in M3 with no effect beyond noise, so don't repeat them without new evidence: V8 explicit compile hints on the startup chunks; pre-reading the startup file (kept for simpler startup, not speed); WebView2 flags `--in-process-gpu`, `--js-flags=--lite-mode`, `--disable-crash-reporter`, `--renderer-process-limit=1`. Idle RAM is about 90% WebView2 (renderer 39 MB, browser 33 MB, GPU 18 MB); the app's JS heap is about 12 MB. The remaining startup cost is one 60–120 ms first-layout task after `showDocument`. Warm, the same load costs about 15 ms of JS.

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

Versions are plain `X.Y.Z`. `package.json` is the only source: `tauri.conf.json` reads it and `scripts/release.ps1` syncs `Cargo.toml`. Never hand-edit versions or create tags directly; run the script, and leave pushing to the user (`git push origin HEAD --follow-tags`). A pushed `vX.Y.Z` tag publishes a GitHub Release through `release.yml`, and 0.x releases are pre-releases. Pre-release suffixes like `-beta.1` are rejected; the NSIS, MSI and winget versioning paths all prefer plain numbers.

The beta ships **unsigned** (PRD decision). Release builds use plain `npx tauri build`, which is what CI does while no `MARKY_SIGN_COMMAND` secret exists. Signing is route-agnostic. `scripts/sign.ps1` runs `MARKY_SIGN_COMMAND` (with a `{file}` placeholder), and `scripts/build-signed.ps1` merges the hook into the config for that build only and then verifies every Authenticode signature. Keep certificates, credentials and the command itself in CI secrets only. winget manifests come from `scripts/winget-manifest.ps1` (schema 1.12.0, validated with `winget validate`).

## Dependencies and licensing

Marky is MIT. After adding, removing or upgrading any npm or Cargo dependency, run `node scripts/third-party-notices.mjs` and commit the regenerated `THIRD_PARTY_NOTICES.txt`; CI fails if it is stale. The file ships in the installer (`bundle.resources`) and the portable ZIP. Prefer MIT, Apache-2.0, BSD, ISC or Zlib dependencies. Flag anything copyleft (GPL, LGPL, AGPL) before adding it, and keep MPL-2.0 to unmodified crates.

## Environment gotchas

- Shells are PowerShell on Windows. Shells opened before Rust was installed lack cargo on PATH; prepend `$env:USERPROFILE\.cargo\bin`.
- Vite uses port 1420 with `strictPort`. Stop any other dev server before `npm run tauri dev`.
- `npm run dev` runs in a plain browser: `isTauri` is false, a sample document loads, and open/save are no-ops.
- Tauri `async` commands that take `State<'_, _>` must return `Result`.
- The `STATIC_VCRUNTIME is deprecated` build warning comes from the Tauri CLI, not this repo.
- markdown-it 15 ships its own types (`import MarkdownIt, { type Token } from 'markdown-it'`); `@types/markdown-it` conflicts with them.
- CodeMirror's DOM exposes `cmTile`, not a view. In browser-side tests, get the view with `EditorView.findFromDOM` (import the module from the URL the app loaded).

## Code style

Pragmatic, readable code that balances SOLID and DRY without overengineering:

- Guard clauses and early returns (`let … else { return }` in Rust) keep functions flat and reject invalid state first.
- Names reveal domain intent (`remember_user_choice`, `renderAfterTypingPause`, `currentFile`).
- Add an abstraction when a second caller needs it.
- Comments explain *why*: a budget, a security reason, a platform quirk.
- The frontend is vanilla TypeScript with no UI framework (PRD decision; SolidJS is the fallback if state grows).
