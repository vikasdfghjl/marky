# TODO

Remaining work after the GPUI migration (`docs/gpui-migration.md`), for the next phase. Details and measurements for each item are in the migration doc. Do not start these while the current phase is still settling.

## Packaging (done)

Done on 7 Nov 2026:

- [x] Installer for `marky.exe`, signed through `scripts/sign.ps1`, plus a portable ZIP. `installer/marky.nsi` and `scripts/build-installer.ps1` build it; the installer is 6.27 MB and the ZIP 8.78 MB.
- [x] CI for the GPUI build: the release program, clippy, unit tests (the contrast test included), notices, e2e, installer, program-size budget and the portable ZIP.
- [x] Remove the previous shell: its Rust folder, the TypeScript frontend, Vite, Vitest, the browser mode and `npm run dev` are gone. The Rust file rules and their tests moved to `src/`; release scripts use the new installer.

## Performance

- [ ] Re-run `scripts/bench-large.ps1` at 8 MB to confirm the stalls are gone. The block comparison now runs on the parse thread, the old blocks are freed in the background, and images are freed from per-document lists. Unit tests pass; the benchmark has not been re-run. `bench-large.ps1` now has `-Via Messages`, so it can run without foregrounding the window; `type_ms` is not comparable that way, but `stall_ms` and `preview_ms` are.
- [ ] Work out why GPUI draws later after a posted key than after a real one (`bench.ps1 -Via Messages`: p95 about 17 ms against 2.6 ms), and which of the two matches a person typing. The p95 is one display refresh. GPUI paces frames (`FramePacer`, `DrawCoordinator`, the vsync thread re-invalidating each window), and a posted `WM_KEYDOWN` enters the posted queue while SendKeys goes through the OS input queue; 50 ms key spacing is also three vsyncs, so posted keys can phase-lock to the refresh. Finish the trace before treating window messages as a typing measure.

## Performance audit (10 Nov 2026): what is left

From a read-through of the code for work that grows with the document or touches the disk on the
main thread. The quick wins are done: saves, the recent list and recovery text off the main thread;
find re-searching only edited lines; links and headings gathered by the parse; image paths cached;
formula layout capped per frame; one copy per source line. Measure each of the rest with
`bench.ps1` and `bench-large.ps1` before and after.

### Main thread

- [ ] Split the window into views (source, preview, outline, status bar) and cache the preview and outline with `AnyView::cached`, so a keystroke redraws only the editor. Today `Prototype` is one view, and every keystroke rebuilds the menus, tabs, outline, visible preview blocks and status bar. Largest structural win; also the single-responsibility fix below.
- [ ] Virtualise the outline (`uniform_list`): it builds one element per heading every frame.
- [ ] Draw a paragraph's text runs as one `StyledText`/`InteractiveText` instead of one `div` per word (`preview_view::styled_runs`). Keep boxes only for paragraphs with inline formulas or images, and keep links as accessibility nodes.
- [ ] Cache the editor's and preview's accessibility text by buffer version, scroll top and selection; it is rebuilt every frame while any UI Automation client listens, with two `position_for_index` calls per character.
- [ ] Find: a changed query still copies the whole document (`buffer.text()`) to search it; search the rope's chunks or a background snapshot instead.
- [ ] `render_line`: `script_ranges` is recomputed every frame, and `syntax::merge` is O(points × spans), quadratic on long minified lines in fences; use a sweep.
- [ ] `preview::block_at_line` is a linear scan on every scroll; binary-search the body blocks (footnotes are appended out of line order).
- [ ] Resolve the font and `em_advance` once per font or size change, not every frame with word wrap off.

### Background parse (8 MB: preview 746 ms, parse 594 ms)

- [ ] One pulldown-cmark pass for both the preview's blocks and the editor's colours, and one byte pass for the counts and the longest line. At 4 MB the second pass is about 67 of 159 ms.
- [ ] Incremental re-parse: from the last top-level boundary before an edit until the new blocks match the old ones again; full parse when the document has footnotes, abbreviations or link reference definitions.
- [ ] tree-sitter: keep only the cache entries the last parse used (today it grows per keystroke in a fence until 20,000 entries), reuse one `Highlighter` per thread, and hash with `foldhash` or `rustc-hash` instead of SipHash.
- [ ] Mermaid: renders of superseded sources are never cancelled, and a stale result can re-enter the cache after `release_unused`. Check a generation number before drawing and before storing.
- [ ] Formula and diagram cache lookups copy the whole source into a new `String` key every frame; key by an `Arc<str>` shared with the run.

### Memory (about 13 MB per MB of document)

- [ ] Preview text is held twice (`Block.text` and the runs) and copied again when drawn; store runs as `SharedString`.
- [ ] Editor colours: `Vec<Vec<(Range<usize>, Token)>>` is 24 bytes per line plus 24 per span; `u32` ranges in a `Box<[_]>` roughly halve it.
- [ ] Undo keeps one `Edit` with two `String`s per keystroke, unbounded; append to the group's last edit and cap the history by bytes.
- [ ] Opening a file copies it about four times (`to_owned`, `replace`, then the rope); use `String::from_utf8`, count line endings with `memchr`, and build the rope directly.

### Toolchain and build

- [ ] Pin the toolchain in `rust-toolchain.toml` (1.98.1 here); budgets depend on the compiler. `rust-version` is now 1.91.
- [ ] Move to edition 2024 with `cargo fix --edition`; its shorter `if let` and tail-expression temporaries make the cache `Mutex` guards less error-prone.
- [ ] Try mimalloc as the global allocator; measure program size (about 0.9 MB of room) and idle memory.
- [ ] Try `opt-level = 3` for `pulldown-cmark`, `ropey`, `tree-sitter*` and `unicode-segmentation` only, keeping `"s"` elsewhere; compare `parse_ms` with the size cost. PGO (`cargo-pgo`) later.
- [ ] Add a `[profile.profiling]` (release, `debug = "line-tables-only"`, `strip = false`) for samply or WPA.
- [ ] Add a `[lints.clippy]` table so CI enforces `redundant_clone`, `assigning_clones`, `needless_pass_by_value` and `or_fun_call`.

### Code structure

- [ ] Rename `Prototype` (it is the app's window now) and split `editor.rs` (about 2,600 lines) by subsystem: documents and saving, find, recovery, panes.
- [ ] One set of UTF-16 offset helpers for the editor and `text_field.rs`.
- [ ] Shorten the longest functions: `render_line`, the accessibility closure in `editor_column`, `restore_or_discard`, `preview::blocks_with`, `settings_panel`.
- [ ] Name the settings panel's focus handles with an enum instead of `settings_controls[10]`.

## Editor and preview

- [ ] IME reconversion: **blocked on GPUI**, whose Windows backend handles no `WM_IME_REQUEST` (still so at `9a8ab54`). The app already answers GPUI's point-to-character request; enabling reconversion needs an upstream GPUI change.
- [x] Move the preview block comparison to the parse thread (done; the 8 MB re-run above is still open).
- [ ] Re-check preview memory on large documents. Text, parsed blocks, the line index and images still grow with the document (about 13 MB per MB before syntax highlighting), and the preview builds all blocks even though both panes are virtualised.

## Known limitations

- Menus are drawn in the window rather than native Windows menus.
- Releases are unsigned until a signing command is configured.
- The migration document keeps historical `gpui-prototype/` paths; the app now lives at the repository root.
