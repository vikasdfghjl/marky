# Changelog

Notable changes to Marky, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow [Semantic Versioning](https://semver.org/). Add entries under **Unreleased** as you go; `scripts/release.ps1` turns that section into the release, and the GitHub Release uses it as its notes.

## [Unreleased]

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
