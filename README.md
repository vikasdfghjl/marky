# Marky

Fast Markdown editor for Windows: [Tauri 2](https://v2.tauri.app/) + [CodeMirror 6](https://codemirror.net/), live preview in a Web Worker, no bundled browser (WebView2). TypeScript UI; Rust for files, dialogs and security.

**Status:** beta 0.3.x — M3 (signing, winget, a11y). Scope and budgets: [PRD.md](PRD.md). Contributor invariants: [AGENTS.md](AGENTS.md).

## Benchmarks

Release build, median of 5–7 runs on i5-9300H / 16 GB, fixture `scripts/fixtures/typical.md` (5 KB). **9 Oct 2026** (sizes, cold start, idle RAM, preview); other rows **29 Sep 2026**. Targets and methodology: [PRD.md](PRD.md).

| Metric | Measured | Target | Hard limit |
| --- | --- | --- | --- |
| Installer (NSIS) | 4.46 MB | ≤ 6 MB | 10 MB |
| App exe | 6.71 MB | ≤ 8 MB | 12 MB |
| Window visible (theme colour) | 187 ms | ≤ 300 ms | 500 ms |
| White flash (dark mode) | 0 ms | 0 ms | 0 ms |
| Cold start to editable | 815 ms | ≤ 900 ms | 1,000 ms |
| Typing latency p50 | 11 ms | ≤ 20 ms | 33 ms |
| Preview after typing stops | 42 ms | ≤ 200 ms | 400 ms |
| Idle RAM in use | 99.6 MB | ≤ 105 MB | 120 MB |
| Idle RAM after 30 s idle | 75.3 MB | ≤ 85 MB | 100 MB |
| First keystroke after idle trim | 35–38 ms | ≤ 60 ms | 100 ms |
| Idle CPU | 0.25% | ≈ 0% | < 0.5% |

Large files: `./scripts/bench-large.ps1` (preview off above 5 MB). Reproduce: `npx tauri build --no-bundle` then `./scripts/bench.ps1` (`-Details`, `-File`, `-SkipTrimmedSample`). Update this table after a benchmark run.

## Install

Download **Releases** → `Marky_<version>_x64-setup.exe` or portable ZIP (`SHA256SUMS.txt`). 0.x = pre-release beta. **Unsigned:** SmartScreen → More info → Run anyway. Per-user install, no admin. WebView2 is on Windows 10/11 already.

**Support:** log at `%LOCALAPPDATA%\dev.marky.editor\marky.log` (local only; paths in errors, not document text). Settings: `%APPDATA%\dev.marky.editor\`. Open limit 128 MB.

## Features

- **Edit:** GFM highlighting, find/replace, word wrap, tabs (20), undo per tab, UTF-8/BOM, CRLF/LF on save, CLI and Explorer, drag-drop, recent (10), window geometry remembered
- **Preview:** synced scroll, block updates, GFM + footnotes, def lists, abbr, `~sub~` / `^sup^` / `==mark==`, smart typography (optional), Mermaid (lazy), KaTeX math, outline (Ctrl+Shift+O), reading view (hide editor)
- **Files:** atomic save, unsaved prompt, external change (reload or keep), 30 s crash recovery, auto-save (settings)
- **Safe:** preview `html: false`, no remote images, http(s)/mailto external, relative links only to user-opened `.md`/`.markdown`/`.txt`
- **UI:** light/dark/system, native menu, status bar (line, counts, encoding, EOL)

Not built: taskbar jump-list recent files.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl+N | New tab |
| Ctrl+O | Open file |
| Ctrl+R | Open recent file |
| Ctrl+S | Save |
| Ctrl+Shift+S | Save As |
| Ctrl+W | Close tab (last tab closes window) |
| Ctrl+Tab | Next tab (Ctrl+Shift+Tab: previous) |
| Ctrl+Shift+E | Toggle editor (preview-only reading view) |
| Ctrl+Shift+V | Toggle preview |
| Ctrl+Shift+O | Toggle outline sidebar |
| Alt+Z | Toggle word wrap |
| Ctrl+, | Settings |
| Ctrl+F | Find |
| Ctrl+H | Find and replace |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Esc, then Tab | Move focus out of the editor (Tab normally indents) |
| Ctrl+M | Toggle Tab between indenting and moving focus |

## Accessibility

WCAG 2.2 AA contrast (`tsx scripts/contrast.ts`, CI). Full keyboard use; preview focusable for scroll. Tabs support arrow keys, Home/End, Delete, and Ctrl+Tab; closing controls remain keyboard-accessible. CodeMirror exposes the editor to AT; status bar is not a live region. IME covered in e2e.

## Development

**Needs:** Node 20+, Rust (MSVC), C++ build tools, WebView2.

```bash
npm install
npm run tauri dev    # native app
npm run dev          # browser + in-memory stand-in (localhost:1420)
npm run check        # typecheck, vitest, shortcuts, clippy, cargo test — before every PR
```

| Command | Purpose |
| --- | --- |
| `npm run build` / `npm run tauri build` | Frontend / release installer |
| `npm run test` | Vitest (`src/**/*.test.ts`) |
| `npx tsx scripts/third-party-notices.ts` | Regenerate notices after dep changes |
| `./scripts/e2e.ps1` | Smoke test (release build first) |
| `./scripts/check-size.ps1` | Exe/installer size gates |
| `./scripts/bench.ps1` | Benchmarks |

**Release:** `npm run release -- -Version X.Y.Z`, then `git push origin HEAD --follow-tags`. [CHANGELOG.md](CHANGELOG.md) Unreleased required. Tag runs CI then publishes artifacts. Signed: `MARKY_SIGN_COMMAND` + `./scripts/build-signed.ps1`. winget: `./scripts/winget-manifest.ps1`.

**Dependabot:** merge Cargo/npm PRs → regen `THIRD_PARTY_NOTICES.txt`. Avoid `windows-core` majors outside Tauri’s 0.62 line.

**Layout:** `src/` (editor, preview worker, UI, platform IPC), `src-tauri/` (files, store, watcher, window), `scripts/` (TS via `tsx`, PS harnesses). `MARKY_PERF_LOG=<file>` logs startup phases; status bar shows `start` and `preview` timings.

## License

[MIT](LICENSE). Third-party notices ship with the app: [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
