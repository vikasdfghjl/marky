# TODO

Open work on the GPUI migration (`docs/gpui-migration.md`), for the next phase. Details and measurements for each item are in the migration doc. Do not start these while the current phase is still settling.

## Step 5: installer and CI

- [ ] Installer that ships both programs (`gpui-prototype.exe`, `marky-diagrams.exe`), signed through `scripts/sign.ps1`, plus a portable ZIP. A local NSIS is at `%LOCALAPPDATA%\tauri\NSIS\makensis.exe`; CI can install NSIS or use Tauri's copy rather than adding another bundler.
- [ ] CI for the GPUI build: build both release binaries, clippy, unit tests (the contrast test included), `e2e.ps1`, and the program-size budget.

## Performance

- [ ] Re-run `gpui-prototype/bench-large.ps1` at 8 MB to confirm the stalls are gone. The block comparison now runs on the parse thread, the old blocks are freed in the background, and images are freed from per-document lists. Unit tests pass; the benchmark has not been re-run. `bench-large.ps1` now has `-Via Messages`, so it can run without foregrounding the window; `type_ms` is not comparable that way, but `stall_ms` and `preview_ms` are.
- [ ] Work out why GPUI draws later after a posted key than after a real one (`bench.ps1 -Via Messages`: p95 about 17 ms against 2.6 ms), and which of the two matches a person typing. The p95 is one display refresh. GPUI paces frames (`FramePacer`, `DrawCoordinator`, the vsync thread re-invalidating each window), and a posted `WM_KEYDOWN` enters the posted queue while SendKeys goes through the OS input queue; 50 ms key spacing is also three vsyncs, so posted keys can phase-lock to the refresh. Finish the trace before treating window messages as a typing measure.

## Document features (done)

Done in the prototype on 7 Nov 2026:

- [x] File watcher: reload on external change, and the Reload / Keep mine prompt.
- [x] Crash recovery snapshots and the restore prompt.
- [x] Recent files, single instance (a second launch opens a tab), drag and drop, New Tab, Save As.
- [x] Auto-save and the line ending for new files (and their settings).

The end-to-end test drives the watcher, New Tab, single instance, Open Recent and auto-save. The
recovery and Save As prompts and the OS drag session need native input, so those stay manual; the
snapshot format is covered by `store.rs` tests. Still open: the installer and CI (Step 5), the
performance items, accessibility and the editor items below.

## Accessibility (done)

- [x] Bounds on the editor's text runs, so a screen magnifier can follow the caret (bounds plus per-character positions and advances; the e2e test checks a caret rectangle).
- [x] A TextPattern for the preview, so it can be reviewed character by character (synthetic text runs, capped like the editor's).
- [x] One tab stop for the preview; the left and right arrows select its links and Enter follows one.
- [x] The settings panel holds the focus: Tab and Shift+Tab cycle its controls and wrap.
- [x] Windows high-contrast themes (`high_contrast.rs`; the mapping is unit-tested, the live theme is not driven by the harness).
- [x] Underlined access keys in the menus, which run the item while its menu is open.

Program 11.34 MB after this work. The next items are the installer and CI, the performance work, and the editor gaps.

## Editor and preview

- [x] Sub- and superscript in the editor at 80% size. The delimiters are hidden, the full-size text stays for layout, caret and clicks, and an 80% copy is painted above or below it. The e2e fixture includes `H~2~O` and `19^th^` so the painting path runs.
- [ ] IME reconversion: **blocked on GPUI**, whose Windows backend handles no `WM_IME_REQUEST` (still so at `9a8ab54`). The prototype already answers GPUI's point-to-character request; enabling reconversion needs an upstream GPUI change.

## Step 6: remove Tauri

Gated on the prototype covering the same tasks; do not remove anything before that.

- [ ] Drop the webview, `src/`, the frontend bundle budget and `npm run dev`; keep the Rust file rules and their tests; port the release scripts to the new installer.
