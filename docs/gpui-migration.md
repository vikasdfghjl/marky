# GPUI migration

Status: proposal. Nothing in the app moves until the prototype below beats today's release build on the same machine.

Marky is a Windows Markdown editor: Tauri 2, WebView2, CodeMirror 6, and markdown-it. [GPUI](https://gpui.rs/) is Zed's GPU UI framework for Rust. It can replace the window. It does not replace the editor or the preview.

## Why this is a gate, not a port

Most of a cold start is WebView2 creating its browser process, about 520–550 ms before `setup` runs. Idle memory is about 90% WebView2 (renderer, browser, and GPU processes). Those figures are in [AGENTS.md](../AGENTS.md). A GPUI window draws with DirectX 11 on Windows and has no browser process ([GPUI examples](https://gpui.rs/examples/)).

The preview is the other half of the product, and it is HTML. markdown-it emits blocks with `data-line` for synced scrolling. KaTeX and Mermaid run from that HTML. Code colours are shared with the editor's Lezer parsers. GPUI does not provide a browser DOM, and it does not turn its controls into native widgets. Its examples cover layout, text, a one-line input, menus, SVG, and lists. They do not include a Markdown editor. The element API can host a custom editor layout; that is a capability, not a component ([GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)).

A full port would throw away `src/` (CodeMirror, the preview worker, panels) and the Tauri window, then rebuild editing, preview, menus, and accessibility. The file layer in `src-tauri/src/` can stay.

## What GPUI is, as of this note

Two different things are called GPUI 0.2.2. The **published crate** on crates.io is what the prototype builds. Zed's **`main` branch** still says 0.2.2 in its `Cargo.toml` but has moved on. Claims below say which one they are about.

- License Apache-2.0. Pre-1.0: breaking changes are expected between versions ([GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)).
- Published crate: one crate. Windows uses Win32 for the window, DirectWrite for text and DirectX 11 for drawing, with no extra features. `main` splits the platform code into a separate `gpui_platform` crate.
- UI is a Rust view tree styled with a Tailwind-like API. CSS support is whatever Zed needs, not the full CSS platform.
- Keyboard input is actions and key bindings, dispatched in-process. There is no webview IPC.
- **Accessibility: the published crate has no AccessKit.** `main` depends on `accesskit` and has an `a11y` example ([Cargo.toml on main](https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml)). Getting an accessibility tree means a git dependency on a reviewed Zed revision, not the crates.io release.
- Menus: on Windows the published crate stores the menu set by `set_menus` but creates no native menu bar. The app draws its own.
- The crate can draw SVG (`resvg` / `usvg`). Something still has to produce the SVG. Mermaid is that producer today, and it is JavaScript.
- Closing the last window does not end the process; the app must quit itself (`on_window_closed`, see the `on_window_close_quit` example).

Apache-2.0 is acceptable next to Marky's MIT license. Pin exactly: `version = "=0.2.2"` or a git `rev`. A bare `"0.2.2"` accepts any later 0.2.x.

**Zed's own crates are not GPUI.** `editor`, `rope`, `markdown` and `mermaid_render` are GPL-3.0-or-later. Read them to learn how Zed solves a problem; do not copy their code into MIT Marky.

## What stays

These already live in Rust and do not depend on the webview:

- `files.rs` — read and atomic write, encoding and line endings, the 128 MB open limit
- `allowlist.rs` — paths come from Rust, and links only follow user-chosen files
- `watcher.rs`, `store.rs`, `diagnostics.rs` — external changes, recent files, recovery, settings storage, the local log

The rules in [AGENTS.md](../AGENTS.md) still hold: one writer for dirty state, recovery deleted only by an explicit Discard, diagnostics stay on the machine.

## What would be rewritten

| Today | After a port |
| --- | --- |
| Tauri window and WebView2 (`window.rs`, `memory.rs`) | A GPUI window. Idle-memory trimming via WebView2 goes away with the browser process. The launch-flash fix (`bench-flash.ps1`) has to be re-checked. |
| CodeMirror (`src/editor/`) | A GPUI text view with its own buffer, undo, selection, find, and Windows text input. |
| markdown-it preview (`src/preview/`) | A Rust layout of the same blocks, still mapped back to source lines for synced scrolling. |
| KaTeX (`math.ts`) | A Rust math renderer, or the feature is dropped. There is no DOM for KaTeX. |
| Mermaid (`mermaid-blocks.ts`) | A Rust renderer that outputs SVG, drawn with GPUI's SVG support, or drop diagrams. |
| Native menus (`menu.rs`) and the shortcut table | GPUI actions and drawn menus. `scripts/check-shortcuts.ts` has to learn the new binding site. |
| Theme, settings panel, tabs, outline, status bar | GPUI views. `scripts/contrast.ts` reads `styles.css` and needs a new source of colours. |
| `ipc.ts` and the browser stand-in | Deleted once no webview remains. `npm run dev` in a browser goes away. |
| Tauri plugins: dialog, single instance, drag-drop, CLI args, title-bar theme | A file dialog crate (`rfd`), a named-pipe or mutex single instance, Win32 drag-drop, and the title-bar colour set by hand. |
| `npx tauri build`: NSIS and MSI installers, file associations, `bundle.resources` | A separate installer pipeline. `release.yml`, `check-size.ps1`, `build-signed.ps1` and the winget manifests all assume Tauri's output. |
| End-to-end and benchmark harness: `e2e.ts` and `bench-large.ts` drive the WebView2 DevTools protocol (`cdp.ts`); the bench scripts find the `Tauri Window` class | A new driver, through UI Automation or a test hook in the app. Without it the AGENTS.md verification rules have nothing to run. |
| The invariants in AGENTS.md | About half describe the webview, CSS or IPC and have to be rewritten with the port. |

## Prototype

`gpui-prototype/` is a throwaway binary beside the app, not inside the Tauri shell. One window. It opens files through `files::read` (File → Open, or the path on the command line) and keeps each one as a tab. It edits the active tab and paints headings, paragraphs, emphasis, quotes, lists, code, tables, display math, and Mermaid fences from a Rust CommonMark parse (pulldown-cmark). Frontmatter is skipped, as in the app. Tables follow the app's look: borders, a bold header row, shaded even rows and column alignment; columns are sized in proportion to their longest cell. The editor and all code use the app's monospace font (the first installed of Cascadia Code, Cascadia Mono, Consolas). Math is the TeX source and a diagram is the Mermaid source: this window does not run KaTeX or Mermaid.

Syntax is coloured in the app's `--hl-*` palette. The editor colours Markdown (headings, emphasis, code spans, links, quotes, list markers, fences) from the same pulldown-cmark parse as the preview, and code in fences is highlighted with tree-sitter in both panes from one cached result, so the panes agree. The default build has nine grammars (Bash, CSS, Go, HTML, JavaScript, JSON, Python, TOML, YAML); TypeScript fences use the JavaScript grammar, and C and Rust stay plain. Each grammar is a Cargo feature (`--features lang-rust`); the reasons and sizes are in [DECISION.md](../DECISION.md).

The text lives in a rope (`ropey`, MIT) with undo and redo: typing and deleting group into one step until a pause, a new line or a caret move; an IME composition is one step; undoing back to the saved state makes the tab clean again. Ctrl+F opens a find bar and Ctrl+H adds replace: matches are found without regard to ASCII case and highlighted, Enter or F3 steps forward, Shift+Enter or Shift+F3 back, and Replace all is a single undo step.

Both panes are virtualised GPUI lists: only the source lines and preview blocks near the viewport are laid out, so cost follows the screen, not the file. The preview is parsed on a background thread once typing pauses (twice the last parse, 30–150 ms, like the app's `typingPauseMs`), and only the blocks that changed are replaced, like the app's `preview-blocks.ts`. Each source line is a GPUI `StyledText`, so clicks, the caret and the IME candidate window use GPUI's own text layout, at any text size and with word wrap. The source pane leads synced scrolling. An outline lists headings and jumps to the matching line. File, Edit, and View menus, plus a bar for Open, Save, and the editor, preview, and outline, are drawn in the window; the bar uses the app's folder, editor, and preview icons. Save goes through `files::write`, so the file's BOM and line ending come back. A dirty tab is marked in the tab and the title. Closing a dirty tab, quitting, or closing the window asks before discarding. Errors show in the path bar, because a release build has no console.

View covers the editor, the preview, the outline, word wrap, text size, and light, dark or system theme (system follows Windows while the window is open). The first run reads those choices from the app's `settings.json`; changes are written to the prototype's own `%APPDATA%\dev.marky.gpui-prototype\settings.json`, so the prototype never edits the app's settings. As in the app, hiding the editor shows the preview without changing `showPreview`.

```bash
cargo run --manifest-path gpui-prototype/Cargo.toml --release
cargo run --manifest-path gpui-prototype/Cargo.toml --release -- path\to\file.md
```

With no path it opens `scripts/fixtures/typical.md`. The release profile matches `src-tauri`'s (`opt-level = "s"`, LTO, one codegen unit, `panic = "abort"`, stripped), so the executable sizes compare. With `MARKY_PERF_LOG` set it writes `startup_ms` (with the start-up phases), `type_ms` and `preview_ms` lines there, in the format `scripts/bench.ps1` reads. `-File` measures another document, such as the files `bench-large.ts gen` makes.

### Measuring

Build both release binaries, then run both benchmarks in one session on the machine that produced the [README benchmarks](../README.md). Background load moves typing latency by up to 2× between sessions ([AGENTS.md](../AGENTS.md)).

```bash
npx tauri build --no-bundle
```

```bash
./scripts/bench.ps1
```

```bash
cargo build --manifest-path gpui-prototype/Cargo.toml --release
```

```bash
./gpui-prototype/bench.ps1
```

`gpui-prototype/bench.ps1` measures what `bench.ps1` does, the same way: the same fixture, `startup_ms` from the top of `main` to the end of the first painted frame, and the private working set 10 s after launch. It also types into the window and reports `type_ms`, from the edit arriving to the end of the frame that shows it. The app's typing figure comes from `bench-large.ts` (a `requestAnimationFrame` callback that reports after paint), so the two are close but not identical; compare them as orders of magnitude, not to the millisecond.

### The gate

The prototype continues only when all four are true. The comparison is with Marky measured in the same session, because this laptop's speed moves between sessions; "clearly better" means at least 20% better:

1. Cold start, ready to edit, is at least 20% faster than Marky's.
2. Memory while in use is at least 20% lower than Marky's.
3. Typing p50 is no slower than Marky's and within the README budget of 20 ms.
4. The program is within the README budget of 8 MB, or the increase is written down and accepted here before any further work.

If any one fails, stop. The rest of the port will not recover a slow or heavy window.

Record the results, the GPUI version or revision, and the date below. Do not put prototype numbers in the README table; it records Marky.

| Date | Build | Cold start | Memory | Typing p50 / p95 | Program |
| --- | --- | --- | --- | --- | --- |
| 6 Nov 2026 | Marky 0.3.3 (`bench.ps1`; typing from `bench-large.ps1 -SizesKb 5`) | 863 ms | 103.7 MB | 15.9 / 18.8 ms | 6.74 MB |
| 6 Nov 2026 | Prototype, crates.io GPUI =0.2.2 (`gpui-prototype/bench.ps1`) | 651 ms (−25%) | 18.9 MB (−82%) | 4.1 / 8.2 ms | 4.49 MB |

Result, 6 Nov 2026: **the gate passes**, on an i5-9300H, five runs each, medians. Read it with these limits:

- Cold start passes with little margin: 25% against the 20% bar, and one of the five runs took 892 ms. The start-up profile below shows where the time goes and how to recover about 185 ms.
- The prototype does much less than Marky: no code highlighting, KaTeX, Mermaid, recovery, file watching or recent files. Memory and start time will grow as those arrive; the margin on memory is large enough to absorb that, the margin on start time is not.
- Typing is measured differently. Marky's figure is a DevTools key event to after paint, on a generated 6 KB document; the prototype's is an OS keystroke (SendKeys) from arrival to the end of its paint, on `typical.md`. Treat it as "both well within budget", not as a 4× speed-up. The prototype logged 201 of 300 keystrokes, because the window lost focus during some runs.
- Large documents are measured separately, below.

### Start-up profile

Measured 6 Nov 2026 with phase marks on the `startup_ms` line and a temporary timing build of GPUI 0.2.2 (typical runs, `typical.md`):

| Phase | Time |
| --- | ---: |
| `Application::new()`: OLE | 2 ms |
| DXGI factory | 13 ms |
| Choosing the adapter, which creates a Direct3D 11 device to test it and drops it | ~265 ms |
| Creating the Direct3D 11 device again, to keep | ~185 ms |
| DirectWrite text system | ~85 ms |
| Message window, drag-drop helper, icon, Windows version | ~45 ms |
| Window, first render and layout, first paint | ~75 ms |

About 85–90% of the start is GPUI's platform set-up, before any window or document: the two Direct3D device creations alone are about 70%. The second one is waste, and Zed's `main` already removes it: `get_adapter` in `crates/gpui_windows/src/directx_devices.rs` keeps the device it tests. Moving to a reviewed Zed revision would bring start-up to roughly 465 ms and is also what step 4 needs for AccessKit. The document itself costs almost nothing at start: reading `typical.md` takes under 1 ms, parsing moved to the background, and the first frame lays out only visible lines.

### Large documents

Files from `bench-large.ts gen`, 6 Nov 2026, same session for both builds. Prototype: `gpui-prototype/bench.ps1 -File`, medians of 3 runs. Marky 0.3.3: `bench-large.ps1`, one start-up run per size. Typing is p50 per keystroke, with the measurement caveat above.

| File | Prototype before the editor rework | Prototype now | Marky 0.3.3 |
| --- | --- | --- | --- |
| 5 KB (`typical.md`) | 651 ms · 18.9 MB · 4.1 ms | 643 ms · 16.4 MB · 1.3 ms · 31 ms | 863 ms · 103.7 MB · 15.9 ms |
| 100 KB | 954 ms · 144 MB · 201 ms | 677 ms · 18.0 MB · 1.5 ms · 34 ms | 1,373 ms · 157 MB · 15.5 ms · 146 ms |
| 1 MB | 7.1 s · 1.23 GB · 5.2 s | 682 ms · 29.4 MB · 1.7 ms · 100 ms | 997 ms · 482 MB · 15.8 ms · 475 ms |
| 4 MB | did not open within 60 s | 723 ms · 68.4 MB · 1.5 ms · 251 ms | 974 ms · 1,352 MB · 15.6 ms · 1,426 ms |

Start · memory 10 s after launch · typing p50 · preview after typing stops. Typing p95 stayed at or under 2.6 ms at every size. Parsing 4 MB takes 92 ms on the background thread, against 1,152 ms in Marky's worker.

Before the rework the prototype laid out every line and re-parsed on every keystroke; virtualised lists and the background parse made start-up and typing independent of file size. The first measurement of the rework found the preview 3.4 s behind at 1 MB: block line numbers were counted from the start of the document for every block, so parsing was quadratic, and a parse cancelled by the next keystroke kept running on its thread, so they piled up. Counting from the previous block fixed both. Memory still grows with the file (the text, the parsed blocks and the line index), at about 13 MB per MB against Marky's 300.

Syntax highlighting, added afterwards, leaves start-up and typing unchanged and costs the large files in the background parse and in memory: at 4 MB, 83.4 MB and 324 ms to update the preview (parse 159 ms), against 68.4 MB and 251 ms without it. The program grew from 4.49 MB to 8.42 MB, 0.42 MB over the README's 8 MB target. That increase was accepted on 6 Nov 2026, as gate criterion 4 requires; the reasons are in [DECISION.md](../DECISION.md).

## After the gate

Do these in order. Each one keeps the app shippable; do not delete WebView2 until the GPUI window covers the same tasks.

1. **Editor.** One document on a rope, undo, find and replace, word wrap, Windows text input, drawn with a virtualised list so cost follows the screen, not the file. Done in the prototype on 6 Nov 2026 (tabs with separate undo histories included). Still missing: selecting with the mouse or Shift+Up/Down, word and document-wide caret moves (Ctrl+arrows, Ctrl+Home/End), Home/End on wrapped rows, horizontal scrolling when word wrap is off (long lines are cut off), IME reconversion, and case folding beyond ASCII in find.
2. **Plain preview.** The syntax Marky renders today that is text layout: tables, task lists, strikethrough, footnotes, definition lists, abbreviations, subscript, superscript, marked text. Keep a line map so scrolling can stay in step. Share one parser's idea of a token with the editor so the two panes do not disagree.
3. **Math and diagrams.** Decide explicitly to port them or to drop them. Do not embed a JavaScript engine to keep KaTeX and Mermaid; that spends the browser cost again.
4. **Chrome.** Menus, shortcuts, settings, outline, status bar, light and dark theme, keyboard access and an AccessKit tree that exposes the editor text. Contrast still has to meet WCAG 2.2 AA. This step needs a GPUI revision with AccessKit (see above).
5. **Test harness and packaging.** A driver for the end-to-end and benchmark scripts, and an installer pipeline, before anything is removed.
6. **Remove Tauri.** Drop the webview, `src/`, the frontend bundle budget, and `npm run dev`. Keep the Rust file rules and the tests that protect them; port the release scripts to the new installer.

## References

Official GPUI material (Apache-2.0):

- [GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md), [gpui.rs](https://gpui.rs/), and [docs.rs for 0.2.2](https://docs.rs/gpui/0.2.2).
- `examples/input.rs`, also shipped in the 0.2.2 crate: the reference text input. It implements `EntityInputHandler` for Windows text input and IME, and maps clicks and the caret through text layout (`closest_index_for_x`, `x_for_index`, `bounds_for_range`).
- `examples/uniform_list.rs` and the `list` element: virtualised rows, for an editor or preview that should not lay out the whole document. The prototype uses `list` for both panes, because wrapped lines and preview blocks have different heights; `ListState::splice` keeps measured heights when lines change.
- `examples/set_menus.rs` and `examples/on_window_close_quit.rs`: menus, and quitting when the window closes.
- `examples/a11y.rs` on `main` only, with [AccessKit](https://github.com/AccessKit/accesskit).

Reference implementations to read but not copy (GPL-3.0-or-later): Zed's `crates/editor`, `crates/rope`, `crates/markdown` and `crates/mermaid_render`.

Permissive libraries:

- [tree-sitter](https://tree-sitter.github.io/tree-sitter/) and `tree-sitter-highlight` (MIT), with the grammar crates `tree-sitter-<language>` (MIT). In `tree-sitter-highlight` 0.27 the later of several patterns on one node wins; older grammar queries assume the opposite (see DECISION.md).

- [pulldown-cmark](https://docs.rs/pulldown-cmark/0.13/pulldown_cmark/struct.Options.html) (MIT). Its options cover frontmatter, footnotes, task lists, definition lists, `^sup^`/`~sub~` and math, but not abbreviations or `==mark==`. Leave `ENABLE_GFM` off: it adds GitHub alerts, which Marky does not render.
- [gpui-kit / gpui-component](https://github.com/longbridge/gpui-kit) (Apache-2.0, not from Zed): a multi-line editor, virtual lists and a Markdown view built on GPUI.
- [merman](https://github.com/Latias94/merman) (MIT or Apache-2.0): a Rust Mermaid renderer that outputs SVG; Zed's `mermaid_render` is built on it.
- No official non-JavaScript replacement for KaTeX was found. Math is an open decision for step 3.

## Risks

- **API churn.** GPUI is pre-1.0 and changes with Zed, and the crates.io release already trails `main`. A port owns that breakage.
- **Text input.** A correct Windows editor includes IME, selection, and undo. The prototype has the input handler and IME placement, but no undo, drag selection, or vertical selection.
- **Accessibility.** WebView2 exposes the editor text today. GPUI needs a revision with AccessKit, and the app must build the tree for its custom editor.
- **Menus.** Marky uses native Windows menus. GPUI draws its own. Recheck keyboard access rather than assuming the native menu behavior carries over.
- **Size and licences.** The 4.46 MB installer is small because Windows already has the browser. The prototype pulls in about 450 crates; licences include Unicode-3.0, Zlib and BSL-1.0, which need notices in `THIRD_PARTY_NOTICES.txt`, and nothing copyleft.
- **Installer and SmartScreen.** The app stays unsigned until a signing command exists. Changing UI toolkits does not change that.

## Out of scope until the gate passes

Rewriting `src/`, adding GPUI to `src-tauri`, keeping a webview and a GPUI window in one release, or changing the README performance table.
