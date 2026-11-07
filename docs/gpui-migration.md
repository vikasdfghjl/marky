# GPUI migration

Status: completed 7 Nov 2026. The prototype became the app at the repository root and the previous shell was removed; the paths below name `gpui-prototype/` as it was during the migration, and the measurements predate the final packaging.

Marky was a Windows Markdown editor built with the previous desktop shell, WebView2, CodeMirror 6 and markdown-it. [GPUI](https://gpui.rs/) is Zed's GPU UI framework for Rust; it replaces the window, the editor and the preview. This document is the record of that migration.

## Why this is a gate, not a port

Most of a cold start is WebView2 creating its browser process, about 520–550 ms before `setup` runs. Idle memory is about 90% WebView2 (renderer, browser, and GPU processes). Those figures are in [AGENTS.md](../AGENTS.md). A GPUI window draws with DirectX 11 on Windows and has no browser process ([GPUI examples](https://gpui.rs/examples/)).

The preview is the other half of the product, and it is HTML. markdown-it emits blocks with `data-line` for synced scrolling. KaTeX and Mermaid run from that HTML. Code colours are shared with the editor's Lezer parsers. GPUI does not provide a browser DOM, and it does not turn its controls into native widgets. Its examples cover layout, text, a one-line input, menus, SVG, and lists. They do not include a Markdown editor. The element API can host a custom editor layout; that is a capability, not a component ([GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)).

A full port would throw away `src/` (CodeMirror, the preview worker, panels) and the previous shell's window, then rebuild editing, preview, menus, and accessibility. The file layer in the previous shell's Rust layer can stay.

## What GPUI is, as of this note

Two different things are called GPUI 0.2.2: the **published crate** on crates.io, and Zed's **`main` branch**, which still says 0.2.2 in its `Cargo.toml` but has moved on. The prototype started on the published crate and, since 6 Nov 2026, uses `main` pinned at one commit: `b77ec90` (3 Jul 2026) at first, `9a8ab54` (6 Nov 2026) since 7 Nov; the reasons are in [DECISION.md](../DECISION.md).

- License Apache-2.0. Pre-1.0: breaking changes are expected between commits ([GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)).
- On `main` the platform code is a separate crate: an app starts with `gpui_platform::application()`. Windows uses Win32 for the window, DirectWrite for text and DirectX 11 for drawing, with no extra features.
- UI is a Rust view tree styled with a Tailwind-like API. CSS support is whatever Zed needs, not the full CSS platform.
- Keyboard input is actions and key bindings, dispatched in-process. There is no webview IPC.
- **Accessibility.** The published crate has no AccessKit. `main` has it (Zed #56065, May 2026): elements with an `id` take ARIA-style attributes (`.role(Role::Button)`, `.aria_label(...)`, `.aria_level(...)`, `.aria_toggled(...)`), `gpui_windows` exposes them through UI Automation (`accesskit_windows`), and `examples/a11y.rs` and the `_accessibility` module document it.
- Menus: on Windows GPUI stores the menu set by `set_menus` but creates no native menu bar. The app draws its own.
- GPUI can draw SVG (`resvg` / `usvg`). Something still has to produce the SVG. Mermaid is that producer today, and it is JavaScript.
- Closing the last window does not end the process; the app must quit itself (`on_window_closed`, see the `on_window_close_quit` example).

Apache-2.0 is acceptable next to Marky's MIT license. Pin exactly: a git `rev`, or `version = "=x.y.z"`. A bare version accepts later releases.

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
| The previous shell's window and WebView2 (`window.rs`, `memory.rs`) | A GPUI window. Idle-memory trimming via WebView2 goes away with the browser process. The launch-flash fix (`bench-flash.ps1`) has to be re-checked. |
| CodeMirror (`src/editor/`) | A GPUI text view with its own buffer, undo, selection, find, and Windows text input. |
| markdown-it preview (`src/preview/`) | A Rust layout of the same blocks, still mapped back to source lines for synced scrolling. |
| KaTeX (`math.ts`) | A Rust math renderer, or the feature is dropped. There is no DOM for KaTeX. |
| Mermaid (`mermaid-blocks.ts`) | A Rust renderer that outputs SVG, drawn with GPUI's SVG support, or drop diagrams. |
| Native menus (`menu.rs`) and the shortcut table | GPUI actions and drawn menus. `scripts/check-shortcuts.ts` has to learn the new binding site. |
| Theme, settings panel, tabs, outline, status bar | GPUI views. `scripts/contrast.ts` reads `styles.css` and needs a new source of colours. |
| `ipc.ts` and the browser stand-in | Deleted once no webview remains. `npm run dev` in a browser goes away. |
| The previous shell's plugins: dialog, single instance, drag-drop, CLI args, title-bar theme | A file dialog crate (`rfd`), a named-pipe or mutex single instance, Win32 drag-drop, and the title-bar colour set by hand. |
| The previous release build: NSIS and MSI installers, file associations, `bundle.resources` | A separate installer pipeline. `release.yml`, `check-size.ps1`, `build-signed.ps1` and the winget manifests all assume the previous shell's output. |
| End-to-end and benchmark harness: `e2e.ts` and `bench-large.ts` drive the WebView2 DevTools protocol (`cdp.ts`); the bench scripts find the previous window class | A new driver, through UI Automation or a test hook in the app. Without it the AGENTS.md verification rules have nothing to run. |
| The invariants in AGENTS.md | About half describe the webview, CSS or IPC and have to be rewritten with the port. |

## Prototype

`gpui-prototype/` is a throwaway binary beside the app, not inside the previous shell. One window. It opens files through `files::read` (File → Open, or the path on the command line) and keeps each one as a tab. It edits the active tab and paints headings, paragraphs, emphasis, quotes, lists and task lists, code, tables, footnotes, definition lists, images, display math, and Mermaid fences, with strikethrough, `~sub~`, `^sup^` and `==marked==` text, from a Rust CommonMark parse (pulldown-cmark). pulldown-cmark never opens a single `~` or `^` inside a word, so `H~2~O`, `19^th^` and `==mark==` are found in the text afterwards with markdown-it's rules, as the app's plugins do. Frontmatter is skipped, as in the app. Links follow the app's rules: web and mail links open in the default program, a footnote reference scrolls to its footnote, and a relative link opens only a .md, .markdown or .txt file, as a tab; the preview never navigates. Images load only from the document's folder; remote images show their alt text, as the app's CSP blocks them. Tables follow the app's look: borders, a bold header row, shaded even rows and column alignment; columns are sized in proportion to their longest cell. The editor and all code use the app's monospace font (the first installed of Cascadia Code, Cascadia Mono, Consolas). Math is drawn with RaTeX, and Mermaid diagrams with merman in a helper program (see step 3 below).

Syntax is coloured in the app's `--hl-*` palette. The editor colours Markdown (headings, emphasis, code spans, links, quotes, list markers, fences) from the same pulldown-cmark parse as the preview, and code in fences is highlighted with tree-sitter in both panes from one cached result, so the panes agree. The default build has nine grammars (Bash, CSS, Go, HTML, JavaScript, JSON, Python, TOML, YAML); TypeScript fences use the JavaScript grammar, and C and Rust stay plain. Each grammar is a Cargo feature (`--features lang-rust`); the reasons and sizes are in [DECISION.md](../DECISION.md).

The text lives in a rope (`ropey`, MIT) with undo and redo: typing and deleting group into one step until a pause, a new line or a caret move; an IME composition is one step; undoing back to the saved state makes the tab clean again. Ctrl+F opens a find bar and Ctrl+H adds replace: matches are found without regard to ASCII case and highlighted, Enter or F3 steps forward, Shift+Enter or Shift+F3 back, and Replace all is a single undo step.

Both panes are virtualised GPUI lists: only the source lines and preview blocks near the viewport are laid out, so cost follows the screen, not the file. The preview is parsed on a background thread once typing pauses (twice the last parse, 30–150 ms, like the app's `typingPauseMs`), and only the blocks that changed are replaced, like the app's `preview-blocks.ts`. Each source line is a GPUI `StyledText`, so clicks, the caret and the IME candidate window use GPUI's own text layout, at any text size and with word wrap. The source pane leads synced scrolling. An outline lists headings and jumps to the matching line. File, Edit, and View menus, plus a bar for Open, Save, and the editor, preview, and outline, are drawn in the window; the bar uses the app's folder, editor, and preview icons. Save goes through `files::write`, so the file's BOM and line ending come back. A dirty tab is marked in the tab and the title. Closing a dirty tab, quitting, or closing the window asks before discarding. Errors show in the path bar, because a release build has no console.

The app's other document features followed on 7 Nov 2026. New Tab (Ctrl+N) creates an untitled document with the configured line ending; Save on one, or Save As (Ctrl+Shift+S), asks for a path. Open Recent (Ctrl+R) is a side panel, built from the app's `recent.json`. A watcher on each file reloads a clean document when another program saves it, and asks Reload / Keep mine when it is dirty; our own save is recognised by its mtime. A second launch forwards its path through `open-request.txt` to the running window, which opens it as a tab, and exits. Dropping files on the window opens each as a tab. Auto-save, off by default, writes a second after typing stops; untitled and read-only documents are left alone. Crash-recovery snapshots are written every 30 seconds, only when a buffer version changed, and offered at the next launch; the held/restore/discard rules follow the app's.

View covers the editor, the preview, the outline, word wrap, text size, and light, dark or system theme (system follows Windows while the window is open). The first run reads those choices from the app's `settings.json`; changes are written to the prototype's own `%APPDATA%\dev.marky.gpui-prototype\settings.json`, so the prototype never edits the app's settings. As in the app, hiding the editor shows the preview without changing `showPreview`.

```bash
cargo run --manifest-path gpui-prototype/Cargo.toml --release
cargo run --manifest-path gpui-prototype/Cargo.toml --release -- path\to\file.md
```

With no path it opens `scripts/fixtures/typical.md`. The release profile matches `the previous shell`'s (`opt-level = "s"`, LTO, one codegen unit, `panic = "abort"`, stripped), so the executable sizes compare. With `MARKY_PERF_LOG` set it writes `startup_ms` (with the start-up phases), `type_ms` and `preview_ms` lines there, in the format `scripts/bench.ps1` reads. `-File` measures another document, such as the files `bench-large.ts gen` makes.

### Measuring

Build both release binaries, then run both benchmarks in one session on the machine that produced the [README benchmarks](../README.md). Background load moves typing latency by up to 2× between sessions ([AGENTS.md](../AGENTS.md)).

```bash
cargo build --release --workspace
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
| 6 Nov 2026, later session | Marky 0.3.3 (`bench.ps1`) | 810 ms | 102.1 MB | | 6.74 MB |
| 6 Nov 2026, same session | Prototype with editor core and highlighting, GPUI from Zed at `b77ec90` | 474 ms (−41%) | 16.3 MB (−84%) | 1.4 / 2.4 ms | 8.88 MB |

Result, 6 Nov 2026: **the gate passes**, on an i5-9300H, five runs each, medians. Read it with these limits:

- On GPUI 0.2.2, cold start passed with little margin: 25% against the 20% bar. The start-up profile below found why; on the pinned Zed commit the margin is 41%. The program is 8.88 MB, over the 8 MB target; see DECISION.md.
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

About 85–90% of the start is GPUI's platform set-up, before any window or document: the two Direct3D device creations alone are about 70%. The second one is waste, and Zed's `main` already removes it (Zed #49001, Feb 2026): `get_adapter` in `crates/gpui_windows/src/directx_devices.rs` keeps the device it tests. The document itself costs almost nothing at start: reading `typical.md` takes under 1 ms, parsing moved to the background, and the first frame lays out only visible lines.

After moving to Zed at `b77ec90` (same marks, five runs): platform set-up about 400 ms instead of about 600, window about 55 ms, render, layout and paint about 25 ms; **474 ms in all against 641 ms** on 0.2.2 earlier the same day, close to the 185 ms the profile predicted.

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

Syntax highlighting, added afterwards, leaves start-up and typing unchanged and costs the large files in the background parse and in memory: at 4 MB, 83.4 MB and 324 ms to update the preview (parse 159 ms), against 68.4 MB and 251 ms without it. The program grew from 4.49 MB to 8.42 MB with highlighting, and to 8.88 MB with GPUI from Zed's repository: 0.88 MB over the README's 8 MB target. That size was accepted on 6 Nov 2026, as gate criterion 4 requires; the reasons are in [DECISION.md](../DECISION.md).

## After the gate

Do these in order. Each one keeps the app shippable; do not delete WebView2 until the GPUI window covers the same tasks.

1. **Editor.** One document on a rope, undo, find and replace, word wrap, Windows text input, drawn with a virtualised list so cost follows the screen, not the file. Done in the prototype on 6 Nov 2026 (tabs with separate undo histories included). Selection and movement followed on 6 Nov 2026: drag, Shift+click, double-click (word) and triple-click (line) with the mouse; Shift with the arrows, Home and End; Ctrl+arrows by word, Ctrl+Home/End, Ctrl+Backspace/Delete by word; and, with word wrap off, horizontal scrolling that follows the caret. The gaps followed on 7 Nov 2026: a drag past the top or bottom of the pane scrolls towards the pointer (faster the further away) and one beside the pane extends to that line's start or end; Home and End work on wrapped rows as CodeMirror's do (row, then indentation or line), with a caret at a wrap point drawn at the start of the next row; find ignores case for every letter, as CodeMirror's `toLowerCase` comparison does; Tab and Shift+Tab indent and outdent the lines the selection touches (the app's `indentWithTab`), and Esc then Tab leaves the editor. IME reconversion stays open: the prototype answers GPUI's point-to-character question, but GPUI's Windows backend handles no `WM_IME_REQUEST` at b77ec90, so Windows never asks a GPUI view to reconvert (still so at `9a8ab54`).
2. **Plain preview.** The syntax Marky renders today that is text layout: tables, task lists, strikethrough, footnotes, definition lists, abbreviations, subscript, superscript, marked text. Keep a line map so scrolling can stay in step. Share one parser's idea of a token with the editor so the two panes do not disagree. Done in the prototype on 6 Nov 2026; the rest followed on 7 Nov 2026 in `extras.rs`, for what pulldown-cmark lacks: abbreviations (definitions hidden, every whole-word use underlined with its title on hover), bare links with linkify-it's defaults (so `README.md` is a link, as in the app), the typographer setting (pulldown-cmark's smart punctuation plus (c), (r), (tm) and +-), footnotes gathered at the end under a rule in reference order with a ↩ back link (unreferenced ones left out), images inside a paragraph, and thematic breaks, which were not drawn at all. A paragraph with several styles now flows word by word, so a run no longer drops to the next line whole. Sub- and superscript now render at 80% in the editor too (7 Nov 2026): the delimiters are hidden, the full-size text remains for layout, caret and clicks, and an 80% copy is painted above or below the line. Task boxes are read-only, as in the app.
3. **Math and diagrams.** Decide explicitly to port them or to drop them. Do not embed a JavaScript engine to keep KaTeX and Mermaid; that spends the browser cost again. Research on 7 Nov 2026 is below, under "Step 3: math and diagrams without JavaScript". Done in the prototype on 7 Nov 2026: math with RaTeX in the program, diagrams with merman in a helper program.
4. **Chrome.** Menus, shortcuts, settings, outline, status bar, light and dark theme, keyboard access and an AccessKit tree that exposes the editor text. Contrast still has to meet WCAG 2.2 AA. The pinned Zed commit has AccessKit (see above). Done in the prototype on 7 Nov 2026; see "Step 4: chrome and accessibility" below.
5. **Test harness and packaging.** A driver for the end-to-end and benchmark scripts, and an installer pipeline, before anything is removed. The end-to-end driver is built (7 Nov 2026); see "Step 5: test harness" below. Typing in the benchmark without the window in front was tried and is not a faithful measure (see below). The large-file benchmark followed the same day. The installer and CI followed in step 6.
6. **Remove the previous shell.** Done 7 Nov 2026. The webview, `src/`, the frontend bundle budget and `npm run dev` are gone; the Rust file rules and their tests are kept in `src/`, and the release scripts build the new NSIS installer. The app now lives at the repository root; the `gpui-prototype/` paths in this document are historical.

## Step 3: math and diagrams without JavaScript

Research, 7 Nov 2026. A scratch program rendered the app sample's flowchart and formulas, six more common diagram types and four harder formulas to SVG, drew the SVGs with resvg 0.46 (what GPUI uses, with the system fonts loaded, as GPUI does), and measured what each library adds to a program built with the prototype's release profile.

**Diagrams: [merman](https://github.com/Latias94/merman) 0.9.3** (MIT or Apache-2.0; what Zed uses). Flowchart, sequence, class, state, ER, Gantt and pie diagrams all render, in 0.1–2.6 ms each, in Mermaid's light and dark themes. Mermaid's default HTML labels (`<foreignObject>`) only draw in a browser, so the flowchart, class and state labels were blank until the configuration set `htmlLabels: false`; with it every label is SVG text. Remaining differences from Mermaid.js are small: edge-label backgrounds sit slightly off, and a bold class name can overflow its box.

**Math: [RaTeX](https://github.com/erweixin/RaTeX) 0.1.15** (MIT crates; KaTeX-compatible, pure Rust, with KaTeX's fonts embedded). Fractions, sums, roots, stretchy brackets, matrices and aligned equations render like KaTeX, in 0.4–1.2 ms each; broken TeX returns a parse error the preview can show, as the app does. The text colour is a layout option, which dark mode needs. Ruled out: `katex` and `mathjax_svg` (they embed a JavaScript engine), `pulldown-latex` and `latex2mathml` (MathML, which GPUI cannot draw), `katex-rs` (HTML output), Typst with mitex (a whole typesetting system for formulas), and the crates.io `rex`, which is a programming language, not the ReX renderer.

**Size**, added to an otherwise empty program:

| Library | Adds |
| --- | ---: |
| merman, seven common diagram types, Cytoscape layout | 6.85 MB |
| merman, all 34 diagram types, Cytoscape layout (Zed's set) | 10.2 MB |
| merman default (adds the ELK layout and math labels) | 14.4 MB |
| RaTeX with embedded KaTeX fonts | 3.0 MB |

Some of it overlaps with what the prototype already links, so the real increase is a little smaller. The prototype is 8.88 MB; the README's program target is 8 MB and its hard limit 12 MB. Linking merman in breaks the hard limit even with seven diagram types. The app loads Mermaid only when a document has a diagram; the native equivalent is a separate helper program, run only then, which keeps the size out of the main program and puts it in the installer.

Decided 7 Nov 2026 (DECISION.md): math is ported with RaTeX in the main program; diagrams are ported with merman in an on-demand helper program.

Math is built in the prototype (7 Nov 2026). pulldown-cmark's math extension finds `$…$` and `$$…$$` with the same rule as the app (prices like `$5 and $10` stay text), and ```` ```math ```` fences are formulas too. RaTeX lays each formula out to SVG in the theme's text colour at KaTeX's 1.21× size, and GPUI draws it as an image. Formulas are laid out only when the preview shows them and cached by size and theme. Display formulas are centred; inline ones sit in the text, which then flows word by word around them. TeX that RaTeX cannot parse shows its source and the error. The program is 10.87 MB with math.

Diagrams are built in the prototype (7 Nov 2026). ```` ```mermaid ```` fences go to `marky-diagrams.exe` (`gpui-prototype/diagrams/`), a separate program with merman (all 34 diagram types, Cytoscape layout), which the prototype starts the first time the preview shows a diagram. It takes one JSON request per line (source and theme) and answers with the SVG or Mermaid's error, using the app's settings: its light or dark theme, `securityLevel: 'strict'`, and `htmlLabels: false` so every label is SVG text. It exits when the prototype does (its input closes) and after a minute without a request; the next diagram starts it again. merman gives up on a diagram after 5 s, the prototype kills a helper that does not answer within 10 s, and a helper that crashes is restarted once. As with formulas, only diagrams the preview shows are drawn, and each is cached by source and theme. A diagram is centred and never wider than drawn, shrinking with the pane, as in the app; while it is being drawn its source shows in its place, and a broken one shows `Mermaid: <error>`, as the app does.

Measured on 7 Nov 2026 (i5-9300H, release builds). Memory is the private working set of the prototype and the helper together, 10 s after launch, median of 3 runs with `bench.ps1 -Keys 0`:

| | Value |
| --- | ---: |
| `marky-diagrams.exe` | 10.35 MB |
| `gpui-prototype.exe` | 10.91 MB (10.87 MB before the helper client) |
| Starting the helper and getting one answer | about 50 ms |
| A file with three diagrams and two broken ones | window painted at 465 ms; first diagram 130 ms later, last 200 ms later |
| Memory, `typical.md` (no diagram, helper not started) | 16.4 MB, unchanged |
| Memory, one small sequence diagram | 21.7 MB, of which the helper 2.9 MB |
| Memory, three larger diagrams (flowchart, sequence, pie) | 44.6 MB |

Most of a diagram's memory is its bitmap. GPUI rasterises an SVG image at twice its size and keeps the bitmap until it is told to drop it, and every edit of a diagram makes a new one, so the prototype makes the bitmap itself (`Image::to_image_data`, on the background thread) and drops the bitmaps of diagrams that no open document contains: after each parse, when a tab closes and when the theme changes. Thirty edits of a pie chart's title left memory where it was (21.4 MB before, 21.8 MB after).

Formulas had the same leak and get the same fix (7 Nov 2026): RaTeX's SVG becomes a bitmap the preview owns, and the bitmaps of formulas that no open document contains at the current font size and theme are dropped, also when the font size changes. Typing 120 characters into a display formula and deleting them again, with the previous build against this one: 14.8 → 175.7 → 162.4 MB before, 14.7 → 39.0 → 22.9 MB now. The peak is the wide formula itself: GPUI gives an image wider than 1,024 px a texture of its own, at least 1,024 px tall. The bitmap is now made while the preview is laid out instead of on GPUI's loader thread; start-up on a file with 48 formulas did not change (479 ms before, 481 ms now, medians of 5), its memory went from 25.6 to 23.5 MB, and formulas appear in the first frame instead of the one after. `$…$` with unbalanced braces, which pulldown-cmark leaves as text, is now found with the app's own rule (`math.ts`), so `$\frac{1}{$` shows its error as in the app; so does a `$$` paragraph that pulldown-cmark did not take as math.

## Step 4: chrome and accessibility

Built in the prototype on 7 Nov 2026 (`src/editor/chrome.rs`, `src/editor/a11y.rs`, `src/contrast.rs`).

- **One command table.** Menu items, toolbar buttons and shortcuts all run a `Command`, as the app's menus, toolbar and `SHORTCUTS` go through its `commands`. The menus are the app's (`menu.rs`) with its shortcut hints; toggles show a tick. The shortcuts are the app's: Ctrl+N, Ctrl+O, Ctrl+R, Ctrl+S, Ctrl+Shift+S, Ctrl+W, Ctrl+, (settings), Ctrl+F, Ctrl+H, Ctrl+Shift+E/V/O, Alt+Z, Ctrl+Tab and Ctrl+Shift+Tab. New Tab, Open Recent, Save As and Auto Save joined them with the document features.
- **Status bar**, as `status-bar.ts`: line and column, words and characters (UTF-16, like JavaScript's `length`, counted on the background parse), line ending, encoding, the last preview update and its parse time, and the start-up time.
- **Outline** follows the caret, as the app's does. It is one tab stop: the arrows move through the headings and Enter jumps, so a long outline does not fill the Tab order.
- **Settings panel** (Ctrl+, or File → Settings…), as `settings-panel.ts`: theme (System, Light, Dark), editor font size, word wrap, smart quotes and dashes (which re-parses every open document), auto-save, the line ending of new files, and the three panes. A side panel, not a modal; Esc closes it. Updated on 7 Nov 2026 with auto-save and the new-file line ending.
- **Keyboard.** Tab and Shift+Tab move through the toolbar, find bar, outline, editor, preview and the settings; inside the preview the left and right arrows select links and Enter follows one. Enter and Space press the focused control (GPUI's keyboard click). In the editor Tab indents, as the app's `indentWithTab`; Esc then Tab leaves. F10 or Alt+F, Alt+E and Alt+V open a menu with its first item focused; the arrows move through it and to the next menu, Esc closes. The preview scrolls with the arrows, Page Up/Down, Space, Home and End when it has the focus. Editing keys act only in the editor and the find fields, so Enter cannot type into the document while a button has the focus. The focused control has a visible ring.
- **AccessKit.** Every control has a role, a name and its state: the menu bar and its items (with expanded and checked states), toolbar toggle buttons, the tab list, the outline list, the find bar, the settings' radio buttons, check boxes and font-size spin button (with increment and decrement actions), and labelled status items. The editor is a multi-line text input whose lines are AccessKit text runs with the caret and selection, which on Windows is UI Automation's TextPattern: a screen reader can read by character, word and line and follow the caret. Only the lines on screen, plus the caret's and the selection anchor's, are described, as CodeMirror keeps only visible lines in its DOM, and nothing is gathered while no assistive technology is connected. The preview is a document whose blocks have their roles (heading with level, paragraph, list item, quote, code, table, formula, image, footnote, rule) and text; links and abbreviations are nodes of their own. The preview is one tab stop: the arrows select its links and Enter follows one.
- **Contrast.** `contrast.rs` checks the prototype's colour pairs in both themes: 4.5:1 for all text (chrome, menus, status bar, preview, links, errors, selection and find highlights, and every syntax colour on the editor), 3:1 for the focus ring on each background it is drawn on. It found the menus' shortcut hints at 4.0:1 in the dark theme, now 7:1.

Checked in the window through Windows UI Automation, the interface screen readers use: the tree has the roles and names above; the editor's TextPattern returns the document and its caret line, which follows the arrow keys; Esc then Tab moves the focus from the editor to the preview, where the left and right arrows select a link and Enter follows it; F10, Down, Down, Enter opens the settings with the focus in them; Tab in the editor indents.

Measured on 7 Nov 2026, same machine: start 503 ms and 16.6 MB on `typical.md` (median of 5; 474–492 ms and 16.3–16.4 MB in earlier sessions), program 11.04 MB (10.91 MB before), still under the 12 MB hard limit. With the accessibility work: program 11.34 MB.

GPUI then moved from `b77ec90` to `9a8ab54` (Zed `main`, 6 Nov 2026; DECISION.md). Same session, `typical.md`, two rounds of five: start 499 and 512 ms against 506 and 500 ms on `b77ec90`, memory 16.2 and 16.3 MB against 16.6 MB, program 11.21 MB (11.04 MB). Everything above was checked again on it: the UI Automation tree and text pattern, Tab, Esc then Tab, F10 and the settings, diagrams and math. Cascadia Code's ligatures now show in the editor (`-->` as an arrow), as they do in the app, which does not turn them off.

The accessibility gaps closed on 7 Nov 2026:

- **Caret tracking.** Editor text runs carry their painted bounds and each character's x offset and advance, so AccessKit gives UI Automation a rectangle for the caret and for a selected range. The e2e test reads the character after the caret through `GetBoundingRectangles` and checks its position and height.
- **Preview text.** The preview's Document node has synthetic text runs for its blocks, capped like the editor's, so a screen reader can review it by character, word and line instead of hearing block names.
- **One preview tab stop.** Links are no longer tab stops. The preview stays one stop; the left and right arrows select a link, Enter follows it, and the selected link draws the focus ring.
- **Settings focus.** The panel's controls have their own focus handles, and Tab / Shift+Tab cycle them and wrap, so the focus cannot leave the open panel.
- **High contrast.** `high_contrast.rs` reads Windows' `SPI_GETHIGHCONTRAST` colours and maps the prototype's light/dark pairs onto `COLOR_WINDOW`, `COLOR_WINDOWTEXT`, `COLOR_BTNFACE`, `COLOR_HIGHLIGHT`, `COLOR_HOTLIGHT` and `COLOR_GRAYTEXT`, refreshing at most once a second. The rules are unit-tested with a synthetic palette; exercising them live needs the OS theme switched.
- **Access keys.** Menu bar names and menu items underline their mnemonic, and the letter runs the item while its menu is open. The letters are actions, not key listeners, so GPUI consumes the key-down and Windows does not also type it into the editor.

Sub- and superscript at 80% in the editor followed on 7 Nov 2026. IME reconversion remains blocked: the prototype answers GPUI's point-to-character question, but GPUI's Windows backend handles no `WM_IME_REQUEST` at `9a8ab54`, so Windows never asks a GPUI view to reconvert.

## Step 5: test harness

`gpui-prototype/e2e.ps1`, built on 7 Nov 2026, is the prototype's counterpart of `scripts/e2e.ps1`. GPUI has no DevTools protocol, so it drives the window through Windows UI Automation, as a screen reader does: it reads the AccessKit tree from step 4, finds controls by role and name, and presses them through their Invoke, Toggle or Expand patterns (menus open with Expand, which the menu buttons now handle). Text is typed with window messages and read back through the editor's TextPattern; files are compared byte for byte. It needs no mouse, keyboard or window in front, so it can run while someone works, and it runs with temporary `APPDATA` and `LOCALAPPDATA`, so neither the app's settings nor the prototype's settings, recent list, recovery snapshots or log are read or changed.

It checks, against a release build:

- opening: the title, the editor's text (no BOM, LF), the rendered preview, CRLF and UTF-8 BOM in the status bar, prices left as text, footnotes at the end, a Mermaid fence sent to the diagram helper;
- editing and saving: the unsaved marker, Save, the saved bytes (BOM and CRLF kept), Undo and Redo from the Edit menu;
- find (case-insensitive count, Esc back to the editor), the toolbar's preview, editor and outline toggles, the outline's headings;
- the settings panel from the File menu, a setting saved to `settings.json`, closing the panel;
- links: a relative `.md` link opening a tab, closing that tab, a link to a PDF refused;
- accessibility: focusing the editor from UI Automation, the caret followed through the TextPattern, a screen rectangle for the caret, the preview's TextPattern, Tab staying among the settings controls, Esc then Tab leaving the editor, the right arrow and Enter following a preview link, and the underlined menu key N opening a New Tab;
- closing: held with unsaved changes, immediate when clean, the diagram helper exiting with the window, and a file over 128 MB refused before it is read;
- the document features added on 7 Nov 2026: New Tab and closing its clean tab, a clean document reloading an external write, a second launch opening a tab, Open Recent listing the file just opened, and auto-save writing about a second after typing stops.

On 7 Nov 2026, after the accessibility work: 43 pass, 4 skipped, in about 15 s. The skipped ones need native input this harness does not drive: restoring recovery snapshots and Save As ask in a native dialog, an OS drag session cannot be started, and IME composition cannot be sent as window messages (`buffer.rs` tests the composition path). Ctrl shortcuts are reached through the menus, because GPUI reads modifier keys from the keyboard's state, which window messages do not set. High-contrast colours are unit-tested, because switching the OS theme is not something the harness does.

The first runs found a real difference from mouse use: pressing a link through UI Automation leaves the focus on the link, as a browser does, so the test moves it back to the editor with UI Automation's SetFocus.

### Typing without the window in front

`bench.ps1` types with SendKeys, which needs the prototype in front and types into whatever window is there. On 7 Nov 2026 it gained `-Via Messages`, which posts the key presses to the window instead (key-down and key-up, which GPUI's message loop turns into a character), so a benchmark could run while someone uses the machine. Every keystroke arrives, and `type_ms` is measured inside the prototype either way, but the numbers differ. Same build, same session, `typical.md`, three runs of 60 keystrokes each:

| How the keys arrive | p50 | p95 |
| --- | ---: | ---: |
| SendKeys, window in front | 1.6 ms | 2.6 ms |
| Window messages, window behind | 2.5–3.8 ms | 16.4–17.3 ms |
| Window messages, window in front | 3.3 ms | 16.1 ms |
| Window messages, key held 2, 15, 60 or 100 ms | 3.0–5.8 ms | 17.1–17.9 ms |

A p95 of one display refresh means that after a posted key, the frame waits for GPUI's display-paced frame, which a real key press does not. Neither the window being in front nor the key-up's timing explains it. GPUI throttles frames for inactive windows and paces the rest by the display, and something in the real input path draws sooner, but this was not traced further. So SendKeys stays the typing measure of record, and `-Via Messages` prints its result as not comparable. The question it leaves is which of the two a person's typing matches: a typist holds a key for tens of milliseconds, unlike SendKeys.

### Large files

`gpui-prototype/bench-large.ps1` (7 Nov 2026) is the counterpart of `scripts/bench-large.ps1`. It uses the same documents: the app's generator, ported to PowerShell and checked byte for byte against it. It measures:

- cold start with the file open and idle RAM, three runs;
- a burst of 30 keys, 70 ms apart, and 12 keys that each wait for the preview to catch up;
- the preview update and its parse time;
- main-thread stalls over 50 ms, logged by the prototype as frames or parse results that take longer than that.

It types with SendKeys, and checks before every key that the prototype is still in front.

Both scripts ran one after the other in the same session, on the same machine:

| File | Build | Start | Idle RAM | Typing p50 / p95 | Preview after typing (parse) | Stalls with preview (worst) | RAM after typing |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 100 KB | Marky | 1,188 ms | 156.8 MB | 17.9 / 21.0 ms | 108 ms (30 ms) | 0 | 212.4 MB |
| | Prototype | 528 ms | 18.9 MB | 3.2 / 5.3 ms | 48 ms (9 ms) | 0 | 19.6 MB |
| 1 MB | Marky | 983 ms | 487.7 MB | 18.0 / 21.5 ms | 496 ms (284 ms) | 1 (110 ms) | 590.5 MB |
| | Prototype | 483 ms | 36.1 MB | 3.2 / 5.2 ms | 237 ms (85 ms) | 0 | 37.4 MB |
| 4 MB | Marky | 1,128 ms | 1,372 MB | 18.3 / 30.5 ms | 1,412 ms (1,127 ms) | 14 (410 ms) | 1,658 MB |
| | Prototype | 515 ms | 84.0 MB | 3.1 / 6.5 ms | 453 ms (297 ms) | 0 | 85.2 MB |
| 8 MB | Marky (preview off above 5 MB) | 1,198 ms | 200 MB | 10.6 / 18.4 ms | off | 17 (147 ms) | 73 MB |
| | Prototype (preview on) | 581 ms | 144.8 MB | 3.5 / 6.4 ms | 746 ms (594 ms) | 12 (67 ms) | 148 MB |

Typing compares loosely. The app's figure runs from `beforeinput` to the frame after the next paint, so it includes waiting for that frame. The prototype's ends when the frame showing the edit has painted, and with SendKeys GPUI draws that frame at once (see "Typing without the window in front"). The app's typing also arrives through the DevTools protocol, not the keyboard.

The prototype keeps its preview at 8 MB, where the app turns its preview off. All 13 of its stalls at that size were parse results being applied: comparing about 90,000 old and new blocks, and walking them to free unused images, took 55–67 ms on the main thread. Doing the comparison on the parse thread would remove them; that is not done yet. No frame took over 50 ms at any size.

## Step 6: removing the previous shell

Done 7 Nov 2026, with the packaging port in the same change.

- **Layout.** `gpui-prototype/` became the repository root: `src/`, `diagrams/`, `Cargo.toml`, `Cargo.lock` and the app's icon. The previous shell's Rust layer, the TypeScript frontend, Vite, Vitest, `index.html`, the browser stand-in and its dependencies were deleted. The Rust modules the prototype borrowed by path (`files`, `allowlist`, `diagnostics`, `store`, `watcher`) moved into `src/`; `allowlist::link_target` is now used by the preview's link handler rather than by a webview command gate.
- **Data folders.** Settings, recent files and the single-instance lock are Marky's own (`%APPDATA%\dev.marky.editor`), and recovery and the log use `%LOCALAPPDATA%\dev.marky.editor`. The prototype's old settings folder is read once for a first run and never written.
- **Window.** GPUI now restores and saves `window.json` itself, replacing the previous shell's geometry handling.
- **Packaging.** `installer/marky.nsi` plus `scripts/build-installer.ps1` build a per-user NSIS installer for both programs. `package-portable.ps1`, `check-size.ps1`, `build-signed.ps1`, `release.ps1`, `winget-manifest.ps1`, `ci.yml`, `bench.yml` and `release.yml` all point at the root crate. `Cargo.toml` is the version source; Node remains only for the notice generator.
- **Verification.** Clippy, 85 tests, the release build and the e2e smoke test (43 pass, 4 skipped, 0 fail) passed. Release measurements: start 465 ms, memory 16.9 MB, typing p50 1.8 / p95 2.8 ms, preview 30 ms; `marky.exe` 11.35 MB, helper 10.39 MB, installer 6.53 MB, portable ZIP 9.11 MB. The installer is 0.53 MB over its 6 MB target and under its 10 MB hard limit; `DECISION.md` records it.

## References

Official GPUI material (Apache-2.0):

- [GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md) and [gpui.rs](https://gpui.rs/). For the pinned commit, read the source in Cargo's checkout (`~/.cargo/git/checkouts/zed-*/9a8ab54/crates/gpui`); docs.rs only has the published 0.2.2.
- `examples/input.rs`: the reference text input. It implements `EntityInputHandler` for Windows text input and IME, and maps clicks and the caret through text layout (`closest_index_for_x`, `x_for_index`, `bounds_for_range`).
- `examples/uniform_list.rs` and the `list` element: virtualised rows, for an editor or preview that should not lay out the whole document. The prototype uses `list` for both panes, because wrapped lines and preview blocks have different heights; `ListState::splice` keeps measured heights when lines change.
- `examples/set_menus.rs` and `examples/on_window_close_quit.rs`: menus, and quitting when the window closes.
- `examples/a11y.rs` and `src/_accessibility.rs`, with [AccessKit](https://github.com/AccessKit/accesskit).

Reference implementations to read but not copy (GPL-3.0-or-later): Zed's `crates/editor`, `crates/rope`, `crates/markdown` and `crates/mermaid_render`.

Permissive libraries:

- [tree-sitter](https://tree-sitter.github.io/tree-sitter/) and `tree-sitter-highlight` (MIT), with the grammar crates `tree-sitter-<language>` (MIT). In `tree-sitter-highlight` 0.27 the later of several patterns on one node wins; older grammar queries assume the opposite (see DECISION.md).

- [pulldown-cmark](https://docs.rs/pulldown-cmark/0.13/pulldown_cmark/struct.Options.html) (MIT). Its options cover frontmatter, footnotes, task lists, definition lists, `^sup^`/`~sub~` and math, but not abbreviations or `==mark==`. Leave `ENABLE_GFM` off: it adds GitHub alerts, which Marky does not render.
- [gpui-kit / gpui-component](https://github.com/longbridge/gpui-kit) (Apache-2.0, not from Zed): a multi-line editor, virtual lists and a Markdown view built on GPUI.
- [merman](https://github.com/Latias94/merman) (MIT or Apache-2.0): a Rust Mermaid renderer that outputs SVG; Zed's `mermaid_render` is built on it.
- No official non-JavaScript replacement for KaTeX was found; RaTeX (step 3) is a third-party port of its layout.

## Risks

- **API churn.** GPUI is pre-1.0 and changes with Zed, and the crates.io release already trails `main`. A port owns that breakage. Moving the prototype from 0.2.2 to `b77ec90` took two changes (`gpui_platform::application()`, and `window.focus` taking `cx`); from `b77ec90` to `9a8ab54` (four months of Zed) took one, `secondary-` in key bindings, which now panics at start-up because `KeyBinding::new` is not told the OS.
- **Consuming Zed by Git.** Cargo downloads the whole Zed repository (about 540 MB) and keeps its history (about 0.9 GB). The lint test fixture with stub `gpui` packages that seemed to block newer commits on 6 Nov did not stop them on 7 Nov (DECISION.md).
- **Text input.** A correct Windows editor includes IME, selection, and undo. The prototype has the input handler and IME placement, but no undo, drag selection, or vertical selection.
- **Accessibility.** WebView2 exposes the editor text today. The pinned GPUI has AccessKit, but the app must build the tree for its custom editor.
- **Menus.** Marky uses native Windows menus. GPUI draws its own. Recheck keyboard access rather than assuming the native menu behavior carries over.
- **Size and licences.** The final installer is 6.53 MB because it now carries both Rust programs rather than reusing Windows' browser. The app pulls in about 450 crates; licences include Unicode-3.0, Zlib and BSL-1.0, which are in `THIRD_PARTY_NOTICES.txt`, and nothing copyleft.
- **Installer and SmartScreen.** The app stays unsigned until a signing command exists. Changing UI toolkits does not change that.

## Out of scope

The gate has passed and the previous shell is gone. Keeping a webview alongside GPUI in one release, or reintroducing a browser runtime, stays out of scope; the remaining deferred work is in [TODO.md](../TODO.md).
