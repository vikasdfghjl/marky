# Decisions

Decisions about Marky that are not obvious from the code, with the reasons and the evidence behind them. Newest first. Each entry says what was decided, what else was considered, and what it costs; change one only with new evidence, and add a new entry rather than rewriting an old one.

## 2026-11-07: Math is ported with RaTeX; Mermaid diagrams move to an on-demand helper with merman

Context: step 3 of [docs/gpui-migration.md](docs/gpui-migration.md), whose "Step 3: math and diagrams without JavaScript" section has the full research. Math was built on 7 Nov 2026; the diagram helper is not built yet.

### Decision

- **Math: RaTeX in the main program.** Inline `$…$`, display `$$…$$` and ```` ```math ```` fences render in the preview as SVG drawn by GPUI, in the theme's text colour. Broken TeX shows its source and the error, as the app does.
- **Diagrams: merman in a separate helper program**, `marky-diagrams.exe`, with all 34 diagram types and the Cytoscape layout. The main program runs it only when a document has a ```` ```mermaid ```` fence, as the app loads Mermaid only then, and draws the SVG it returns. Mermaid's `htmlLabels` is set to false, so every label is SVG text that GPUI can draw.
- No JavaScript engine, as the migration doc requires.

### Options considered

| | Port in the main program | Port in a helper program | Drop |
| --- | --- | --- | --- |
| Math (RaTeX, +3.0 MB) | **Chosen**: formulas are common and inline, so a process round trip per document would delay them | Keeps the main program small, but every math-heavy page waits on the helper | Shows TeX source |
| Diagrams (merman, +6.85 MB for 7 types, +10.2 MB for all 34) | Breaks the 12 MB hard limit | **Chosen**: size goes to the installer; cost only when a document has a diagram | Shows Mermaid source |

### Evidence

Spike, 7 Nov 2026: seven diagram types render in 0.1–2.6 ms each with merman 0.9.3 and draw correctly with resvg 0.46 (GPUI's SVG renderer) once `htmlLabels` is false; formulas render like KaTeX in 0.4–1.2 ms with RaTeX 0.1.15. Sizes are added to an empty program built with the prototype's release profile.

### Consequences

- **Program size: 10.87 MB measured** with math built in (RaTeX added 1.99 MB, less than the 3.0 MB it adds to an empty program, because the prototype already links much of what it needs). Over the README's 8 MB target and under its 12 MB hard limit; choosing RaTeX accepted this, and it replaces the 8.88 MB accepted with the GPUI pin.
- The helper is a second program to build, sign and ship, and a process to start (tens of milliseconds) when a document first shows a diagram. It should stay running while such documents are open and render all of a document's diagrams in one request.
- Diagram output differs slightly from Mermaid.js: edge-label backgrounds sit a little off and long class names can overflow their box.

## 2026-11-06: The GPUI prototype uses GPUI from Zed's repository, pinned at b77ec90

Context: [docs/gpui-migration.md](docs/gpui-migration.md), `gpui-prototype/Cargo.toml`.

### Decision

- `gpui` and `gpui_platform` come from `https://github.com/zed-industries/zed` at commit **`b77ec90`** (3 Jul 2026), instead of the crates.io release 0.2.2. Both crates use the same commit.
- Move early: every feature written against 0.2.2 was more to port later.
- The pin moves only on purpose, with a build, the tests and a start-up measurement, and an update to this entry.

### Why this commit and not the newest

The newest `main` (`78fde14`, 6 Nov 2026) does not build as a Git dependency. Zed #58496 (3 Jul 2026) added `tooling/lints/test_fixture`, a workspace with stub packages named `gpui` (0.0.0) and `gpui_shared_string`. Cargo loads every package in a Git dependency's repository, keeps both `gpui` packages because their versions differ, and resolves `gpui_platform`'s `gpui` to the stub. It fails with "gpui does not have feature windows-manifest"; the version (0.46.0) and feature list it prints belong to `usvg` and mislead. `b77ec90` is the commit just before #58496, and it already has what the move is for:

- one Direct3D device instead of two (Zed #49001, 12 Feb 2026)
- AccessKit (Zed #56065, 27 May 2026)
- the `gpui` / `gpui_platform` split and the current API up to July

### Options considered

| Option | For | Against |
| --- | --- | --- |
| Stay on crates.io 0.2.2 | Nothing to change | Two Direct3D devices (~185 ms), no AccessKit, an API that trails Zed by about a year |
| **Zed Git, pinned at `b77ec90` (chosen)** | Zed's own source; all three gains | Four months behind `main`; Cargo downloads the whole Zed repository (about 540 MB, 17 minutes here) |
| Zed Git at the newest commit | Current API | Does not resolve (the fixture above) |
| `gpui-pre` crates.io snapshots (what gpui-kit uses) | Current, small download | Published by a third party (`huacnlee`), not Zed |
| Zed as a Git submodule with path dependencies | Any commit, no resolution clash | A second 540 MB checkout in the repository, and machine-specific paths |

### Evidence

Same session, i5-9300H, `typical.md`, medians of 5 runs:

| Build | Start | Memory | Typing p50 / p95 | Program |
| --- | ---: | ---: | ---: | ---: |
| Prototype on GPUI 0.2.2 (earlier session) | 641 ms | 17.0 MB | 1.4 / 2.7 ms | 8.42 MB |
| **Prototype on Zed `b77ec90`** | **474 ms** | 16.3 MB | 1.4 / 2.4 ms | 8.88 MB |
| Marky 0.3.3 | 810 ms | 102.1 MB | | 6.74 MB |

Start-up phases moved as the profile predicted: GPUI's platform set-up dropped from about 600 to about 400 ms. Start is now 41% faster than Marky, against 25% on 0.2.2. The port took two changes: `gpui_platform::application()` replaces `Application::new()`, and `window.focus` and `on_window_closed` gained a `cx` and a window-id argument. Clippy is clean and all 32 tests pass.

### Consequences

- **Program size: 8.88 MB**, 0.46 MB more than on 0.2.2 (AccessKit and the newer platform code) and 0.88 MB over the README's 8 MB target. **Accepted on 6 Nov 2026**; this supersedes the 8.42 MB accepted for highlighting and is the written acceptance the migration gate asks for.
- The pin cannot move past `b77ec90` until Zed excludes the test fixture from its repository's packages (or renames the stubs). Reporting it upstream would unblock this.
- Cargo keeps the Zed checkout in `~/.cargo/git` (about 0.9 GB with its history).
- API reference for the pin is the source in Cargo's checkout; docs.rs only covers 0.2.2.

## 2026-11-06: Code highlighting in the GPUI prototype uses tree-sitter, with nine grammars

Context: [docs/gpui-migration.md](docs/gpui-migration.md). This applies to `gpui-prototype/`, not the shipping app, which keeps CodeMirror's Lezer parsers.

### Decision

- **tree-sitter** highlights code in fences, in both the editor and the preview, as Zed does. Each fence is highlighted once per change; both panes read the same cached result, so they always agree.
- The default build includes **nine grammars**: Bash, CSS, Go, HTML (with its embedded CSS and JavaScript), JavaScript (with JSX), JSON, Python, TOML and YAML.
- **TypeScript, C and Rust are left out** of the default build. ```` ```ts ```` and ```` ```tsx ```` fences use the JavaScript grammar, which colours everything except type annotations. C and Rust fences stay plain.
- Each grammar is a Cargo feature (`lang-bash`, `lang-rust`, and so on), so a language is added or removed with a flag, for example `cargo build --release --features lang-rust`, without code changes.
- Markdown syntax in the editor (headings, emphasis, code spans, links, quotes, list markers, fences) comes from the same pulldown-cmark parse the preview uses, with no extra dependency. Colours are the app's `--hl-*` light and dark pairs from `styles.css`.

### Options considered

| Option | For | Against |
| --- | --- | --- |
| syntect (Sublime grammars) | 50+ languages in about 1–1.5 MB (estimated, not built) | Regex-based and slower; not what Zed uses |
| **tree-sitter (chosen)** | Accurate, fast, incremental; Zed's approach and grammars | Every grammar is compiled C that adds to the program |
| Markdown syntax only | No size cost | Code in fences stays plain, unlike the app |

### Evidence

Measured on 6 Nov 2026 (i5-9300H, release profile matching `src-tauri`):

- The prototype was **4.49 MB** before highlighting. With all twelve grammars it was **12.78 MB**, over the README's 12 MB hard limit for the program.
- The compiled grammars add about half their static-library size. Library sizes: TypeScript and TSX 5.79 MB, Bash 2.76, Rust 2.29, C 1.37, Python 1.02, JavaScript 0.92, the tree-sitter runtime 0.92, Go 0.52, YAML 0.46, CSS 0.31, HTML 0.08, TOML 0.08, JSON 0.02.
- The chosen set was estimated at about 8.0 MB and **measured at 8.42 MB**: 0.42 MB over the README's 8 MB target for the program, and under its 12 MB hard limit. Highlighting adds 3.93 MB to the 4.49 MB prototype; the app's program is 6.74 MB.
- **Accepted on 6 Nov 2026: 8.42 MB**, rather than dropping Go and YAML (about 0.5 MB together) to get under 8 MB. This is the written acceptance the migration gate asks for when the program exceeds the target.

Speed with the chosen set, `gpui-prototype/bench.ps1`, medians of 3 runs, on files with about 1,500 fences per MB (Bash, JavaScript, JSON and Rust):

| File | Start | Memory | Typing p50 | Preview after typing stops (parse) | Before highlighting |
| --- | ---: | ---: | ---: | ---: | --- |
| 5 KB (`typical.md`) | 641 ms | 17.0 MB | 1.4 ms | 32 ms (0.2 ms) | unchanged |
| 1 MB | 644 ms | 34.4 MB | 1.6 ms | 205 ms (41 ms) | 29.4 MB, 100 ms (22 ms) |
| 4 MB | 679 ms | 83.4 MB | 1.6 ms | 324 ms (159 ms) | 68.4 MB, 251 ms (92 ms) |

Start-up and typing do not change: grammars are compiled on first use, on the background thread. Large files pay in the background parse and in memory for the colour spans. Marky 0.3.3 takes 475 ms (1 MB) and 1,426 ms (4 MB) to update its preview.

Sets that were weighed: dropping only TypeScript (~9.9 MB), TypeScript and C (~9.2 MB, keeps Rust), and TypeScript, C, Rust and Bash (~6.7 MB, leaves shell fences plain, which are common in READMEs).

### Consequences

- Rust and C code in fences is plain, and TypeScript types are not coloured, until the build budget allows them or grammars load at run time (Zed loads grammars as WebAssembly extensions; that would move their size out of the program and into separate files).
- `tree-sitter-highlight` 0.27 lets the **later** pattern win when several capture the same node. Queries are joined general first and specific last (JavaScript, then TypeScript, then JSX), and the JSON key pattern is repeated at the end, because tree-sitter-json 0.24's query was written for the older first-wins rule. Re-check key colours after upgrading any grammar.
- Adding a grammar back is a size decision: measure the program with the feature on and record the result here.
