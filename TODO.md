# TODO

Remaining work after the GPUI migration (`docs/gpui-migration.md`), for the next phase. Details and measurements for each item are in the migration doc. Do not start these while the current phase is still settling.

## Packaging (done)

Done on 7 Nov 2026:

- [x] Installer that ships both programs (`marky.exe`, `marky-diagrams.exe`), signed through `scripts/sign.ps1`, plus a portable ZIP. `installer/marky.nsi` and `scripts/build-installer.ps1` build it; the installer is 6.53 MB and the ZIP 9.11 MB.
- [x] CI for the GPUI build: both release binaries, clippy, unit tests (the contrast test included), notices, e2e, installer, program-size budget and the portable ZIP.
- [x] Remove the previous shell: its Rust folder, the TypeScript frontend, Vite, Vitest, the browser mode and `npm run dev` are gone. The Rust file rules and their tests moved to `src/`; release scripts use the new installer.

## Performance

- [ ] Re-run `scripts/bench-large.ps1` at 8 MB to confirm the stalls are gone. The block comparison now runs on the parse thread, the old blocks are freed in the background, and images are freed from per-document lists. Unit tests pass; the benchmark has not been re-run. `bench-large.ps1` now has `-Via Messages`, so it can run without foregrounding the window; `type_ms` is not comparable that way, but `stall_ms` and `preview_ms` are.
- [ ] Work out why GPUI draws later after a posted key than after a real one (`bench.ps1 -Via Messages`: p95 about 17 ms against 2.6 ms), and which of the two matches a person typing. The p95 is one display refresh. GPUI paces frames (`FramePacer`, `DrawCoordinator`, the vsync thread re-invalidating each window), and a posted `WM_KEYDOWN` enters the posted queue while SendKeys goes through the OS input queue; 50 ms key spacing is also three vsyncs, so posted keys can phase-lock to the refresh. Finish the trace before treating window messages as a typing measure.

## Editor and preview

- [ ] IME reconversion: **blocked on GPUI**, whose Windows backend handles no `WM_IME_REQUEST` (still so at `9a8ab54`). The app already answers GPUI's point-to-character request; enabling reconversion needs an upstream GPUI change.
- [ ] Consider moving the preview block comparison to the parse thread: at 8 MB, applying a parse took 55–67 ms on the main thread and produced the only stalls over 50 ms. No frame took over 50 ms at any size.
- [ ] Re-check preview memory on large documents. Text, parsed blocks, the line index and images still grow with the document (about 13 MB per MB before syntax highlighting), and the preview builds all blocks even though both panes are virtualised.

## Known limitations

- Menus are drawn in the window rather than native Windows menus.
- Releases are unsigned until a signing command is configured.
- The migration document keeps historical `gpui-prototype/` paths; the app now lives at the repository root.
