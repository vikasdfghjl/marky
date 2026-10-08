# Changelog

Notable changes to Marky, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow [Semantic Versioning](https://semver.org/). Add entries under **Unreleased** as you go; `scripts/release.ps1` turns that section into the release, and the GitHub Release uses it as its notes.

## [Unreleased]

### Added

- A local log at `%LOCALAPPDATA%\dev.marky.editor\marky.log` records crashes and internal errors, so a bug report can include what went wrong. It stays on your machine.

### Changed

- Files larger than 128 MB are refused with a message instead of risking running out of memory. Files up to that size open as before.

### Security

- Mermaid diagrams now use the same up-to-date KaTeX as the rest of the preview for math in labels, replacing an older copy with a known advisory.

### Fixed

- The untitled document Marky opens with now uses the line ending chosen under "Line ending for new files". It was always CRLF.
- Marky no longer offers to restore a file you had already saved. An older crash-recovery copy could land after the newer one.
- Opening and saving at the same moment (for example, an auto-save while a file opens) no longer drops a file from Open Recent.

## [0.2.9] - 2026-10-01

### Removed

- Inserted text: `++inserted++` now stays as typed. Only one Markdown library understood it, and everywhere else the plus signs show literally. `==marked==` stays.

## [0.2.8] - 2026-10-01

### Added

- Subscript and superscript: `H~2~O` and `19^th^` show as H₂O and 19ᵗʰ in the preview, and small and shifted in the editor. `~~strikethrough~~`, code and math are unaffected.
- Inserted and marked text: `++inserted++` is underlined and `==marked==` highlighted, in the editor and the preview.
- Footnotes: `[^1]` references with `[^1]: …` definitions, and inline `^[…]` footnotes, listed at the end of the preview. Clicking a footnote number jumps to its definition.
- Definition lists in the preview: a term on one line, then its definition on a line starting with `: `.
- Abbreviations in the preview: define one with `*[HTML]: Hyper Text Markup Language` and every HTML in the document shows the full form on hover.

### Removed

Marky targets Markdown documents, not one platform's flavour of them, so syntax that only one site understands is gone from the preview:

- GitHub alerts: `> [!NOTE]`, `[!TIP]` and the rest now show as ordinary blockquotes.
- Emoji shortcodes: `:rocket:` stays as typed.
- GitHub-style heading ids: headings no longer get ids, so `#heading` links no longer jump. Footnote links still do.

## [0.2.7] - 2026-10-01

### Added

- Emoji shortcodes in the preview: `:rocket:`, `:+1:`, `:tada:` and the rest of GitHub's names show as emoji. Text smileys like `:)` stay as you typed them, and shortcodes inside code are left alone.
- Smart typography in the preview: straight quotes become curly, `--` and `---` become en and em dashes, `...` becomes an ellipsis, and (c), (r), (tm) and +- become ©, ®, ™ and ±. Code and math are left alone. It is on by default; turn it off in Settings if you write things like `--force` without backticks.

## [0.2.6] - 2026-09-29

### Changed

- Marky gives memory back when you leave it alone: after 30 seconds without a key press, click or scroll (and while minimised) it drops from about 99 MB to about 77 MB for a typical document. The first key press afterwards takes a few milliseconds longer.
- Editing large documents is much smoother: the preview now updates only the part you changed instead of redrawing everything. On a 1 MB file the longest freeze drops from 0.9 s to 0.1 s, typing latency from 30 ms to 18 ms, and memory while typing from 1.5 GB to 0.57 GB. On a 4 MB file typing goes from 67 ms to 19 ms. Diagrams stay drawn while you edit elsewhere.

### Fixed

- No more white flash when opening Marky in dark mode: the window used to show white for about 0.65 s before the editor appeared. It now starts in the theme's own colour, for System, Light and Dark.

## [0.2.5] - 2026-09-28

### Changed

- Faster startup: a cold start now takes about 816 ms instead of 924 ms on the test laptop. The startup code is 23% smaller, and WebView2 starts one process fewer.
- Lower memory use: about 104 MB idle instead of 110 MB, and about 70 MB while Marky is minimised.

### Fixed

- Changing two settings in quick succession could save the older value.

## [0.2.4] - 2026-09-28

### Added

- Hide the editor for a preview-only reading view: the panel button beside the settings gear, View → Editor, Ctrl+Shift+E, or Settings → Show editor. The choice is remembered.

### Changed

- Hiding the editor or the preview turns the other one on, so the window never goes empty.
- Find and Replace bring the editor back when it is hidden.

## [0.2.3] - 2026-09-28

### Added

- A + button after the last tab opens a new tab, like Ctrl+N.

## [0.2.2] - 2026-09-28

### Added

- Tick marks on the native menu's toggle items: View → Preview, Outline and Word Wrap, and File → Auto Save. They stay in step with the shortcuts and the settings panel.
- View → Word Wrap (Alt+Z) and File → Auto Save menu items.

### Changed

- View menu items are named after what they show ("Preview", "Outline") instead of "Toggle Preview" and "Toggle Outline"; the tick shows the state.

### Fixed

- Ctrl+Shift+V could not turn the preview setting off while a very large file had the preview hidden automatically.

## [0.2.1] - 2026-09-28

The first release after 0.1.0 (0.2.0 was never tagged).

### Added

- Tabs: several documents in one window, each with its own undo history, selection, file watcher and recovery snapshot. Ctrl+N opens a new tab, Ctrl+W closes one, Ctrl+Tab and Ctrl+Shift+Tab switch between them.
- Native menu bar (File, Edit, View, Help) with shortcut hints, and Help → About.
- Drag and drop: drop Markdown files on the window to open them.
- Settings panel (gear at the right of the tab row, File → Settings…, Ctrl+,): theme (System, Light or Dark, including the title bar and menu), editor font size, word wrap, auto-save, line ending for new files, and preview and outline visibility. Saved to `settings.json`.
- Auto-save, off by default: saves a file about a second after typing stops. Untitled and read-only documents are never auto-saved.
- Preview features for technical writing:
  - Code blocks highlighted with the same colours as the editor.
  - YAML frontmatter shown as a metadata block.
  - GitHub alerts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`).
  - Heading anchors, and `#anchor` links that jump within the document.
  - Links to other `.md` files open them; web and mailto links open in the default app.
  - Mermaid diagrams, drawn with strict security and following the theme.
  - KaTeX math (`$…$` and `$$…$$`), where prices like $5 stay plain text.
- Outline sidebar (Ctrl+Shift+O) listing the document's headings and highlighting the current one.

### Changed

- The frontend runs in a plain browser (`npm run dev`) against an in-memory stand-in for the Rust core.
- The end-to-end test covers technical Markdown instead of Japanese and Hindi IME input.

## [0.1.0] - 2026-09-27

The first public beta. It is unsigned, so Windows SmartScreen asks for confirmation.

### Added

- Markdown editing with GFM syntax highlighting, and fenced code blocks highlighted through lazily loaded language packs.
- Live preview rendered in a Web Worker, with tables, task lists, strikethrough and autolinks, scrolled in sync with the editor.
- Local images in the preview, resolved against the file's folder. Remote images are blocked and raw HTML is shown as text.
- Light and dark themes that follow Windows.
- Find and replace.
- Status bar showing line and column, word and character count, line ending and encoding.
- Opening files from the command line and Explorer's "Open with". UTF-8 with or without a BOM, and CRLF or LF line endings, are preserved on save; files in other encodings open read-only.
- Crash-safe saves: a temporary file, a flush, then an atomic rename.
- A Save, Don't save or Cancel prompt before unsaved changes are lost.
- External change detection: a file with no unsaved edits reloads by itself when another program changes it; with unsaved edits, Marky asks whether to reload or keep your version.
- Recent files (Ctrl+R).
- Crash recovery: unsaved changes are saved every 30 seconds and offered back after a crash.
- WCAG 2.2 AA contrast in both themes, and full keyboard access.
- Per-user NSIS installer (no admin prompt), portable ZIP, and a SHA-256 checksum file with every GitHub Release.
