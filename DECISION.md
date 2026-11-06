# Decisions

Decisions about Marky that are not obvious from the code, with the reasons and the evidence behind them. Newest first. Each entry says what was decided, what else was considered, and what it costs; change one only with new evidence, and add a new entry rather than rewriting an old one.

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
