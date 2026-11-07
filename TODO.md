# TODO

Open work on the GPUI migration (`docs/gpui-migration.md`), set aside on 7 Nov 2026 to move ahead rather than polish. Details and measurements for each item are in the migration doc.

## Step 5: test harness and packaging (next)

- [ ] Installer that ships both programs (`gpui-prototype.exe`, `marky-diagrams.exe`), signed through `scripts/sign.ps1`, plus a portable ZIP.
- [ ] CI for the GPUI build: build, clippy, unit tests (the contrast test included), `e2e.ps1`.

## Performance

- [ ] Re-run `gpui-prototype/bench-large.ps1` at 8 MB to confirm the stalls are gone. The block comparison now runs on the parse thread, the old blocks are freed in the background, and images are freed from per-document lists. Unit tests pass; the benchmark has not been re-run.
- [ ] Work out why GPUI draws later after a posted key than after a real one (`bench.ps1 -Via Messages`: p95 about 17 ms against 2.6 ms), and which of the two matches a person typing.

## Features the prototype lacks (the app has them)

- [ ] File watcher: reload on external change, and the Reload / Keep mine prompt.
- [ ] Crash recovery snapshots and the restore prompt.
- [ ] Recent files, single instance (a second launch opens a tab), drag and drop, New Tab, Save As.
- [ ] Auto-save and the line ending for new files (and their settings).

## Accessibility

- [ ] Bounds on the editor's text runs, so a screen magnifier can follow the caret.
- [ ] A TextPattern for the preview, so it can be reviewed character by character (blocks have names only).
- [ ] Every preview link is a tab stop; consider one stop for the preview with links reached by arrows.
- [ ] The settings panel does not hold the focus while open.
- [ ] Windows high-contrast themes.
- [ ] Underlined access keys in the menus.

## Editor and preview

- [ ] Sub- and superscript in the editor at 80% size.
- [ ] IME reconversion: blocked on GPUI, whose Windows backend handles no `WM_IME_REQUEST` (still so at `9a8ab54`).

## Step 6: remove Tauri

- [ ] Drop the webview, `src/`, the frontend bundle budget and `npm run dev`; keep the Rust file rules and their tests; port the release scripts to the new installer.
