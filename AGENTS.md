# AGENTS.md — Marky

Marky is a Windows Markdown editor: Tauri 2 (Rust) + WebView2 + CodeMirror 6 + markdown-it. Its reason to exist is being small and fast. [PRD.md](PRD.md) is the source of truth for scope, milestones and performance budgets; read it before adding a feature or changing a budget-sensitive path.

## Architecture invariants

- **Rust owns the disk; the webview owns the text.** The document lives in CodeMirror state and crosses IPC only on open, save and external change. Typing stays inside the webview.
- **Paths come only from Rust.** Dialogs, drag-drop, CLI args and single-instance forwards call `AppState::remember_user_choice`; `open_file`/`save_file` call `require_user_choice`. Any new way to obtain a path registers it through `remember_user_choice` on the Rust side.
- **Least-privilege webview.** `src-tauri/capabilities/default.json` grants only events and set-title. Prefer a custom command over a plugin permission; when a permission is unavoidable, scope it narrowly and say why in the capability's description.
- **Untrusted Markdown.** The preview renders with `html: false`, never navigates, and runs under the CSP in `tauri.conf.json` (`csp` and `devCsp` change together). Enabling raw HTML means routing output through DOMPurify first.
- **Text model.** The editor always holds LF text. `files.rs` detects BOM and line ending on read and restores them on write, so saving an unedited file reproduces it byte-for-byte. All writes go through `files::write` (temp file, `sync_all`, atomic `fs::rename`). `ReplaceFileW` looks nicer because it keeps ACLs, but a kill mid-call leaves no file; `killed_mid_save_leaves_old_or_new_content` guards this.
- **Dirty state has one writer.** `setDirty` in `main.ts` updates the title and mirrors transitions to Rust (`set_dirty`), which holds the window close only when dirty. Route every dirty change through it.
- **Lazy by default.** Language packs, and later KaTeX and Mermaid, load through dynamic `import()`. The initial JS (entry chunk plus its modulepreloads) stays ≤ 250 KB gzipped. The preview worker stays a separate emitted file because the CSP allows `worker-src 'self'` only.

## Performance budgets

Budgets are release-blocking. Measure on a release build (`npx tauri build --no-bundle`), then run `./scripts/bench.ps1`; dev-build numbers are meaningless. `-Details` prints the per-phase timeline written through `MARKY_PERF_LOG`.

The README's **Benchmarks** section is the single record of current numbers. After any benchmark run, update that table, its date and the breakdown sentence in the same change.

Known cost structure: about 610 ms of cold start is WebView2 creating its browser process before `setup` runs, and flags don't move it. Optimise the frontend part: bundle size, first layout, IPC round trips. The WebView2 flags in `tauri.conf.json` (`additionalBrowserArgs`) replace Tauri's defaults, so keep the `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection` prefix. Keep the GPU enabled; it protects typing and scroll latency.

## Verification

A change is done when all three pass:

```bash
npm run typecheck
```

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

For changes to file handling, IPC, shortcuts or window lifecycle, also run the end-to-end smoke test against a release build (`npx tauri build --no-bundle`, then `./scripts/e2e.ps1`); every line must read PASS. Extend `scripts/e2e.mjs` when adding a flow it can drive. Native dialogs block it, so it covers only dialog-free paths. For purely visual changes, check `npm run dev` in a browser.

## Environment gotchas

- Shells are PowerShell on Windows. Shells opened before Rust was installed lack cargo on PATH; prepend `$env:USERPROFILE\.cargo\bin`.
- Vite uses port 1420 with `strictPort`. Stop any other dev server before `npm run tauri dev`.
- `npm run dev` runs in a plain browser: `isTauri` is false, a sample document loads, and open/save are no-ops.
- Tauri `async` commands that take `State<'_, _>` must return `Result`.
- The `STATIC_VCRUNTIME is deprecated` build warning comes from the Tauri CLI, not this repo.

## Code style

Pragmatic, readable code that balances SOLID and DRY without overengineering:

- Guard clauses and early returns (`let … else { return }` in Rust) keep functions flat and reject invalid state first.
- Names reveal domain intent (`remember_user_choice`, `renderAfterTypingPause`, `currentFile`).
- Add an abstraction when a second caller needs it.
- Comments explain *why*: a budget, a security reason, a platform quirk.
- The frontend is vanilla TypeScript with no UI framework (PRD decision; SolidJS is the fallback if state grows).
