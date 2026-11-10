//! The window: tabs, the source editor, the preview, the outline, menus and the find bar.
//!
//! Text lives in a rope (`buffer.rs`) with undo and redo. Characters arrive through GPUI's platform
//! input handler, because that is how Windows text input and IME reach a view. Both panes are
//! virtualised GPUI lists, so only the lines and blocks near the viewport are laid out, and the
//! preview is re-parsed on a background thread once typing pauses, as in the app.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, FocusHandle, Focusable, HighlightStyle,
    ExternalPaths, IntoElement, ListAlignment, ListOffset, ListScrollEvent, ListState, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollHandle,
    PromptLevel, SharedString, StyledText, Subscription, Task, TextAlign, TextLayout, TextRun, UTF16Selection, Window, WindowBounds, actions, canvas,
    deferred, div, fill, list, point, prelude::*, px, rgb, size, svg,
};

use crate::allowlist;
use crate::buffer::{self, Buffer, EditKind, LineSplice, SavePoint};
use crate::{diagrams, diagnostics, instance, math, store, watcher};
use crate::files::{self, FileMeta, OpenedFile};
use crate::perf;
use crate::preview::{self, Block, Kind};
use crate::preview_view::{self, Look, tone};
use crate::settings::{self, ViewSettings};
use crate::syntax::{self, Span};
use crate::text_field::{FieldEvent, InputLayer, TextField};

actions!(
    marky,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectAll,
        LineStart,
        LineEnd,
        Enter,
        Paste,
        Cut,
        Copy,
        Quit,
        Save,
        Undo,
        Redo,
        Find,
        FindReplace,
        FindNext,
        FindPrevious,
        FindClose,
        SelectUp,
        SelectDown,
        SelectLineStart,
        SelectLineEnd,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        DocumentStart,
        DocumentEnd,
        SelectDocumentStart,
        SelectDocumentEnd,
        DeleteWordLeft,
        DeleteWordRight,
        Indent,
        Outdent,
    ]
);

mod a11y;
mod chrome;

pub use chrome::*;

/// CodeMirror's default indent unit, which Tab adds and Shift+Tab removes (the app binds `indentWithTab`).
const INDENT: &str = "  ";

const CARET: u32 = 0x2563eb;

/// Lines and blocks laid out beyond the viewport, so scrolling a little does not wait for layout.
const OVERDRAW: f32 = 400.;

/// The app's `--font-mono` list, in its order. Windows 10 has only Consolas; Windows 11 adds Cascadia.
const MONO_FONTS: [&str; 3] = ["Cascadia Code", "Cascadia Mono", "Consolas"];

/// GPUI replaces a missing family with its UI font instead of trying the next one, so pick the first installed.
fn mono_family(cx: &App) -> SharedString {
    let installed = cx.text_system().all_font_names();
    let family = MONO_FONTS.into_iter().find(|name| installed.iter().any(|font| font == name)).unwrap_or("Consolas");
    family.into()
}

/// The preview waits for typing to pause: twice the last parse, 30–150 ms, like the app's `typingPauseMs`.
/// A typical document updates about 40 ms after the last keystroke; a large one never parses mid-burst.
fn parse_pause(last_parse_ms: f64) -> Duration {
    Duration::from_secs_f64((last_parse_ms * 2.0).clamp(30.0, 150.0) / 1000.0)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenMenu {
    File,
    Edit,
    View,
}

struct Document {
    /// Finds the document again when a background parse finishes, even if tabs moved meanwhile.
    id: u64,
    /// Empty for a New Tab document until it is saved.
    path: String,
    meta: FileMeta,
    /// From the last read or save; a watcher event is ours when the file still has this time.
    mtime_ms: u64,
    /// Lossy decode. Saving would corrupt the file, so Save refuses it.
    read_only: bool,
    /// A recovered document's text is unsaved even though its undo history is empty.
    restored: bool,
    buffer: Buffer,
    selected: Range<usize>,
    /// The caret is at `selected.start` when the selection was made backwards.
    selection_reversed: bool,
    marked: Option<Range<usize>>,
    /// Shared with the next background parse, which compares its result against them.
    blocks: Arc<Vec<Block>>,
    /// The formulas and diagrams in `blocks`, gathered by the parse: freeing unused images reads these.
    images: Arc<ImagesInUse>,
    /// The preview's links and the outline's headings, gathered from `blocks` by the parse, so
    /// drawing a frame does not walk every block.
    links: Vec<(Arc<str>, usize)>,
    headings: Vec<Heading>,
    /// Syntax colours per source line, from the last background parse.
    spans: Vec<Vec<Span>>,
    /// The longest line in characters, from the last parse: the width to scroll to when word wrap is off.
    longest_line: usize,
    parse_task: Option<Task<()>>,
    last_parse_ms: f64,
    /// The last edit the preview has not caught up with; logged as `preview_ms` when it does.
    edited_at: Option<Instant>,
    /// One item per source line.
    lines: ListState,
    /// One item per preview block.
    block_list: ListState,
    followed_cursor_line: Option<usize>,
    synced_source_line: Option<usize>,
    /// Words and UTF-16 characters, counted with each background parse, for the status bar.
    counts: Counts,
    /// The last update's time from keystroke to preview, and its parse time, for the status bar.
    last_preview: Option<(f64, f64)>,
    /// Dropping this stops watching the file.
    watcher: Option<notify::RecommendedWatcher>,
    /// Replaced by every watcher event, so a burst checks the file once after it settles.
    external_task: Option<Task<()>>,
    /// Replaced by every edit: auto-saves once typing stops.
    auto_save_task: Option<Task<()>>,
    /// The save being written on a background thread, and the target of one asked for meanwhile.
    save_task: Option<Task<()>>,
    queued_save: Option<PathBuf>,
}

/// What a background parse hands back to the window. Everything that costs time in proportion to
/// the document is done on the parse thread, so applying it stays cheap at any size (at 8 MB the
/// comparison alone took 55–67 ms on the main thread).
struct Parsed {
    blocks: Vec<Block>,
    spans: Vec<Vec<Span>>,
    longest: usize,
    counts: Counts,
    /// The blocks this parse was compared against, and what changed: the range of old blocks to
    /// replace and how many new ones replace them.
    base: Arc<Vec<Block>>,
    changed: (Range<usize>, usize),
    images: ImagesInUse,
    links: Vec<(Arc<str>, usize)>,
    headings: Vec<Heading>,
}

/// The formula and diagram sources a document shows, which keep their bitmaps alive.
#[derive(Default)]
struct ImagesInUse {
    diagrams: HashSet<String>,
    /// TeX and whether it is display math.
    formulas: HashSet<(String, bool)>,
}

impl ImagesInUse {
    fn of(blocks: &[Block]) -> Self {
        let mut images = Self::default();
        for block in blocks {
            match block.kind {
                Kind::Diagram => {
                    images.diagrams.insert(block.text.clone());
                }
                Kind::Math => {
                    images.formulas.insert((block.text.clone(), true));
                }
                _ => {}
            }
            let cells = block.table.iter().flat_map(|table| table.rows.iter().flatten());
            for run in block.runs.iter().chain(cells.flatten()).filter(|run| run.style.math) {
                images.formulas.insert((run.text.clone(), run.style.display));
            }
        }
        images
    }
}

/// One recovery write: the held snapshots and each dirty document as a rope, which is cheap to
/// copy on the main thread and is turned into text on the thread that writes it.
struct RecoveryWrite {
    held: Vec<store::Snapshot>,
    documents: Vec<(Option<String>, ropey::Rope, FileMeta)>,
}

impl RecoveryWrite {
    /// Saves the snapshots, or clears the file when nothing is left to recover.
    fn write(self, dir: &Path) -> std::io::Result<()> {
        let mut snapshots = self.held;
        snapshots.extend(self.documents.into_iter().map(|(path, rope, meta)| store::Snapshot { path, text: rope.to_string(), meta }));
        if snapshots.is_empty() { store::clear_snapshots(dir) } else { store::save_snapshots(dir, &snapshots) }
    }
}

/// The status bar's counts: words as the app counts them (`/\S+/g`), characters as JavaScript's `length`.
#[derive(Clone, Copy, Default)]
struct Counts {
    words: usize,
    chars: usize,
}

impl Counts {
    fn of(text: &str) -> Self {
        Self { words: text.split_whitespace().count(), chars: text.encode_utf16().count() }
    }
}

impl Document {
    fn open(opened: OpenedFile, id: u64, cx: &mut Context<Prototype>) -> Self {
        let buffer = Buffer::new(&opened.text);
        let lines = ListState::new(buffer.line_count(), ListAlignment::Top, px(OVERDRAW));
        // The preview follows the source pane, so scrolling the source re-renders the view.
        lines.set_scroll_handler(cx.listener(|_, _: &ListScrollEvent, _, cx| cx.notify()));
        Self {
            id,
            path: opened.path,
            meta: opened.meta,
            mtime_ms: opened.mtime_ms,
            read_only: opened.read_only,
            restored: false,
            buffer,
            selected: 0..0,
            selection_reversed: false,
            marked: None,
            blocks: Arc::default(),
            images: Arc::default(),
            links: Vec::new(),
            headings: Vec::new(),
            spans: Vec::new(),
            longest_line: 0,
            parse_task: None,
            last_parse_ms: 0.,
            edited_at: None,
            lines,
            block_list: ListState::new(0, ListAlignment::Top, px(OVERDRAW)),
            followed_cursor_line: None,
            synced_source_line: None,
            counts: Counts::default(),
            last_preview: None,
            watcher: None,
            external_task: None,
            auto_save_task: None,
            save_task: None,
            queued_save: None,
        }
    }

    fn is_dirty(&self) -> bool {
        self.restored || self.buffer.is_dirty()
    }

    fn display_name(&self) -> String {
        if self.path.is_empty() { "Untitled".to_string() } else { file_name(&self.path) }
    }

    fn cursor(&self) -> usize {
        if self.selection_reversed { self.selected.start } else { self.selected.end }
    }

    /// Keeps the line list and the syntax colours in step with an edit. The edited lines keep their
    /// old colours until the next parse (at most 150 ms after typing stops), so a heading being typed
    /// does not flicker plain; lines the edit added start plain.
    fn splice_lines(&mut self, splice: LineSplice) {
        self.lines.splice(splice.first..splice.first + splice.old, splice.new);
        if splice.first >= self.spans.len() {
            return;
        }
        let kept = splice.old.min(splice.new);
        let old_end = (splice.first + splice.old).min(self.spans.len());
        let replace = (splice.first + kept).min(old_end)..old_end;
        self.spans.splice(replace, std::iter::repeat_with(Vec::new).take(splice.new - kept));
    }
}

struct FindBar {
    query: Entity<TextField>,
    replacement: Entity<TextField>,
    show_replace: bool,
    /// Sorted, non-overlapping byte ranges.
    matches: Vec<Range<usize>>,
    /// Document id, buffer version and query the matches belong to.
    searched: Option<(u64, u64, String)>,
    _subscriptions: Vec<Subscription>,
}

pub struct Prototype {
    focus: FocusHandle,
    documents: Vec<Document>,
    active: usize,
    next_document_id: u64,
    logged_first_frame: bool,
    /// When the last edit arrived; logged as `type_ms` once that frame has painted.
    typed_at: Option<Instant>,
    settings: ViewSettings,
    dark: bool,
    /// The editor's and the preview's code font.
    mono: SharedString,
    config_dir: PathBuf,
    open_menu: Option<OpenMenu>,
    find: Option<FindBar>,
    /// Where the caret was painted in the last frame. The IME places its candidate window here.
    caret_bounds: Option<Bounds<Pixels>>,
    /// The last error or result, shown in the path bar: a release build has no console.
    status: Option<String>,
    window_title: String,
    /// Set once the user chose Discard, so the second close request goes through.
    close_confirmed: bool,
    /// The left button went down in the source and has not come up: moves extend the selection.
    dragging: bool,
    /// Horizontal scroll of the source pane when word wrap is off.
    h_scroll: ScrollHandle,
    /// Where the caret was last painted, and for which offset: the horizontal follow needs a current one.
    caret_painted: Option<(usize, Bounds<Pixels>)>,
    /// The caret moved; scroll sideways to it once it has been painted.
    follow_x: bool,
    last_cursor: Option<(u64, usize)>,
    /// The monospace font's character width at the current size.
    char_width: Pixels,
    /// One per menu item, so the arrows can move the focus through an open menu.
    menu_focus: Vec<FocusHandle>,
    /// The open menu itself, focused when it was opened with the mouse, so the arrows work without an item highlighted.
    menu_root: FocusHandle,
    /// The outline is one tab stop; the arrows move this highlight through its headings.
    outline_focus: FocusHandle,
    outline_cursor: Option<usize>,
    preview_focus: FocusHandle,
    /// The preview is one tab stop; the left and right arrows select one of its links.
    preview_link: Option<usize>,
    /// Lines laid out this frame, shared with the accessibility callback, which needs their bounds
    /// after prepaint to give each text run a rectangle a screen magnifier can follow.
    prepaint_lines: Rc<RefCell<Vec<(usize, TextLayout)>>>,
    settings_focus: FocusHandle,
    /// One handle per control in the settings panel, so Tab and Shift+Tab stay inside it while it is
    /// open instead of moving out into the editor.
    settings_controls: Vec<FocusHandle>,
    settings_open: bool,
    /// The most recent files, newest first, from the app's `recent.json`.
    recent: Vec<PathBuf>,
    recent_open: bool,
    recent_focus: FocusHandle,
    /// Crash-recovery snapshots, and the snapshots held until the user restores or discards them.
    recovery_dir: PathBuf,
    held_recovery: Vec<store::Snapshot>,
    recovery_task: Option<Task<()>>,
    /// Document ids and buffer versions in the last recovery write, so an unchanged dirty document
    /// is not serialised again every 30 seconds. `recovery_dirty` covers held snapshots changing.
    recovery_versions: Vec<(u64, u64)>,
    recovery_dirty: bool,
    /// Watches `open-request.txt`, written by a second launch.
    request_watcher: Option<notify::RecommendedWatcher>,
    /// The single-instance lock; held for the window's lifetime.
    _instance_lock: Option<std::fs::File>,
    /// Esc was pressed in the editor: the next Tab moves the focus instead of indenting, as with
    /// CodeMirror's `indentWithTab`, so a keyboard user can always leave the editor.
    tab_leaves: bool,
    /// Where the pointer is while a drag selection is outside the source pane, and the task that
    /// keeps scrolling towards it.
    drag_outside: Option<Point<Pixels>>,
    drag_scroll: Option<Task<()>>,
    /// Source lines painted in the last frame, with their layouts.
    painted_lines: Vec<(usize, TextLayout)>,
    /// When this frame's render began: a frame over 50 ms is logged as `stall_ms`, the
    /// counterpart of the long tasks the large-file benchmark counts.
    frame_started: Option<Instant>,
    /// The last window bounds, written after a resize settles.
    geometry_task: Option<Task<()>>,
    /// A close asked for while a save was still being written; it asks again once the save is done.
    close_task: Option<Task<()>>,
    _appearance: Subscription,
    _bounds: Subscription,
}

impl Prototype {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        opened: OpenedFile,
        settings: ViewSettings,
        config_dir: PathBuf,
        recovery_dir: PathBuf,
        recovery: Vec<store::Snapshot>,
        instance_lock: Option<std::fs::File>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // "System" follows Windows' light or dark mode while the window is open.
        let appearance = cx.observe_window_appearance(window, |_, _, cx| cx.notify());
        let bounds = cx.observe_window_bounds(window, |this, window, cx| this.schedule_geometry_save(window, cx));
        let document = Document::open(opened, 0, cx);
        perf::mark("open");
        let mono = mono_family(cx);
        perf::mark("fonts");
        let mut this = Self {
            focus: cx.focus_handle().tab_stop(true),
            documents: vec![document],
            active: 0,
            next_document_id: 1,
            logged_first_frame: false,
            typed_at: None,
            dark: settings.is_dark(window.appearance()),
            mono,
            settings,
            config_dir: config_dir.clone(),
            open_menu: None,
            find: None,
            caret_bounds: None,
            status: None,
            window_title: String::new(),
            close_confirmed: false,
            dragging: false,
            h_scroll: ScrollHandle::new(),
            caret_painted: None,
            follow_x: false,
            last_cursor: None,
            char_width: px(9.),
            menu_focus: (0..MENU_ITEMS).map(|_| cx.focus_handle()).collect(),
            menu_root: cx.focus_handle(),
            outline_focus: cx.focus_handle().tab_stop(true),
            outline_cursor: None,
            preview_focus: cx.focus_handle().tab_stop(true),
            preview_link: None,
            prepaint_lines: Rc::new(RefCell::new(Vec::new())),
            settings_focus: cx.focus_handle(),
            settings_controls: (0..14).map(|_| cx.focus_handle()).collect(),
            settings_open: false,
            recent: store::load_recent(&config_dir).into_iter().filter(|path| path.is_file()).collect(),
            recent_open: false,
            recent_focus: cx.focus_handle(),
            recovery_dir,
            held_recovery: Vec::new(),
            recovery_task: None,
            recovery_versions: Vec::new(),
            recovery_dirty: true,
            request_watcher: None,
            _instance_lock: instance_lock,
            tab_leaves: false,
            drag_outside: None,
            drag_scroll: None,
            painted_lines: Vec::new(),
            frame_started: None,
            geometry_task: None,
            close_task: None,
            _appearance: appearance,
            _bounds: bounds,
        };
        // The source paints first; the preview follows when the background parse is done, like the app.
        this.schedule_parse(0, Duration::ZERO, cx);
        this.watch_document(0, cx);
        this.restore_or_discard(recovery, cx);
        this.start_recovery_timer(cx);
        this.watch_requests(cx);
        this.check_pending_request(cx);
        this
    }

    fn doc(&self) -> &Document {
        &self.documents[self.active]
    }

    fn doc_mut(&mut self) -> &mut Document {
        &mut self.documents[self.active]
    }

    fn report(&mut self, message: String) {
        eprintln!("{message}");
        self.status = Some(message);
    }

    // ----- Editing -----

    /// Every change to the text goes through here: the buffer, its undo history, the source list and the preview.
    fn edit(&mut self, range: Range<usize>, text: &str, kind: EditKind, cx: &mut Context<Self>) {
        self.tab_leaves = false;
        if range.is_empty() && text.is_empty() {
            return;
        }
        let doc = self.doc_mut();
        let before = doc.selected.clone();
        let (len_before, version_before) = (doc.buffer.len(), doc.buffer.version());
        let (caret, splice) = doc.buffer.edit(range, text, kind, before);
        doc.splice_lines(splice);
        doc.selected = caret..caret;
        doc.selection_reversed = false;
        doc.marked = None;
        self.update_matches_after_edit(splice, len_before, version_before);
        self.after_change(cx);
    }

    /// With the find bar open, an edit re-searches only the lines it replaced; searching the whole
    /// document on every keystroke cost the main thread time in proportion to its size.
    fn update_matches_after_edit(&mut self, splice: LineSplice, len_before: usize, version_before: u64) {
        let Some(find) = self.find.as_mut() else { return };
        let Some((id, version, query)) = find.searched.as_mut() else { return };
        let buffer = &self.documents[self.active].buffer;
        // A query with a line break could match across the edited lines; search it all again.
        if *id != self.documents[self.active].id || *version != version_before || query.contains('\n') {
            return;
        }
        let new_lines = buffer.line_start(splice.first)..buffer.line_start(splice.first + splice.new);
        let old_end = new_lines.end + len_before - buffer.len();
        let fresh = buffer::find_all(&buffer.slice(new_lines.clone()), query)
            .into_iter()
            .map(|found| found.start + new_lines.start..found.end + new_lines.start)
            .collect();
        buffer::splice_matches(&mut find.matches, new_lines.start..old_end, new_lines.end, fresh);
        *version = buffer.version();
    }

    fn after_change(&mut self, cx: &mut Context<Self>) {
        self.preview_link = None;
        self.typed_at = Some(Instant::now());
        self.doc_mut().edited_at = self.typed_at;
        let (id, pause) = (self.doc().id, parse_pause(self.doc().last_parse_ms));
        self.schedule_parse(id, pause, cx);
        self.schedule_auto_save(id, cx);
        cx.notify();
    }

    /// Parses a snapshot of the text on a background thread after `pause`. Each edit replaces the
    /// task, which cancels the previous one, so only the text after the last keystroke is parsed.
    fn schedule_parse(&mut self, id: u64, pause: Duration, cx: &mut Context<Self>) {
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == id) else { return };
        let rope = doc.buffer.snapshot();
        let version = doc.buffer.version();
        let flavor = preview::Flavor { typographer: self.settings.typographer };
        let base = doc.blocks.clone();
        doc.parse_task = Some(cx.spawn(async move |this, cx| {
            if !pause.is_zero() {
                cx.background_executor().timer(pause).await;
            }
            let started = Instant::now();
            // Both panes from one snapshot: the preview's blocks and the editor's colours. Code in
            // fences is highlighted once; the second call finds it in the tree-sitter cache.
            let parsed = cx
                .background_executor()
                .spawn(async move {
                    let text = rope.to_string();
                    let longest = text.lines().map(|line| line.chars().count()).max().unwrap_or(0);
                    let blocks = preview::blocks_with(&text, flavor);
                    let changed = preview_view::changed_blocks(&base, &blocks);
                    let images = ImagesInUse::of(&blocks);
                    let (links, headings) = (links_of(&blocks), headings_of(&blocks));
                    Parsed { blocks, spans: syntax::source_spans(&text), longest, counts: Counts::of(&text), base, changed, images, links, headings }
                })
                .await;
            let parse_ms = started.elapsed().as_secs_f64() * 1000.0;
            let _ = this.update(cx, |this, cx| this.apply_parse(id, version, parsed, parse_ms, cx));
        }));
    }

    /// Replaces only the blocks that changed, so the preview keeps its measured heights and scroll position.
    fn apply_parse(&mut self, id: u64, version: u64, parsed: Parsed, parse_ms: f64, cx: &mut Context<Self>) {
        let applying = Instant::now();
        let Parsed { blocks, spans, longest, counts, base, changed, images, links, headings } = parsed;
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == id) else { return };
        if doc.buffer.version() != version {
            return;
        }
        doc.last_parse_ms = parse_ms;
        doc.counts = counts;
        // From the last keystroke to the preview showing it: the pause, the parse and this update.
        if let Some(edited_at) = doc.edited_at.take() {
            let latency = edited_at.elapsed().as_secs_f64() * 1000.0;
            perf::log(&format!("preview_ms={latency:.0} parse_ms={parse_ms:.1}"));
            doc.last_preview = Some((latency, parse_ms));
        }
        // Only one parse per document is in flight (a new one replaces the task), so the blocks it
        // compared against are still the shown ones; if not, compare here.
        let (changed, count) = if Arc::ptr_eq(&base, &doc.blocks) { changed } else { preview_view::changed_blocks(&doc.blocks, &blocks) };
        doc.block_list.splice(changed, count);
        let old_blocks = std::mem::replace(&mut doc.blocks, Arc::new(blocks));
        let old_spans = std::mem::replace(&mut doc.spans, spans);
        doc.images = Arc::new(images);
        doc.links = links;
        doc.headings = headings;
        doc.longest_line = longest;
        doc.synced_source_line = None;
        // Freeing a large document's old blocks and colours is itself tens of milliseconds.
        cx.background_spawn(async move { drop((old_blocks, old_spans, base)) }).detach();
        self.release_unused_images(cx);
        // The main thread's part of a parse: splicing the list and freeing images.
        let ms = applying.elapsed().as_secs_f64() * 1000.0;
        if ms > 50.0 {
            perf::log(&format!("stall_ms={ms:.0} apply"));
        }
        cx.notify();
    }

    /// Frees the bitmaps of formulas and diagrams that no open document shows any more at the
    /// current font size and theme: every edit of one makes a new bitmap, and GPUI keeps each until
    /// it is dropped.
    fn release_unused_images(&self, cx: &mut App) {
        let images = self.documents.iter().map(|doc| doc.images.as_ref());
        let mut diagrams_in_use = HashSet::new();
        let mut formulas_in_use = HashSet::new();
        for found in images {
            diagrams_in_use.extend(found.diagrams.iter().map(String::as_str));
            formulas_in_use.extend(found.formulas.iter().map(|(tex, display)| (tex.as_str(), *display)));
        }
        diagrams::release_unused(&diagrams_in_use, self.dark, cx);
        math::release_unused(&formulas_in_use, self.settings.font_size, self.dark, cx);
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc();
        if !doc.selected.is_empty() {
            let range = doc.selected.clone();
            self.edit(range, "", EditKind::Other, cx);
            return;
        }
        let cursor = doc.cursor();
        let range = doc.buffer.previous_grapheme(cursor)..cursor;
        self.edit(range, "", EditKind::Deleting, cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc();
        if !doc.selected.is_empty() {
            let range = doc.selected.clone();
            self.edit(range, "", EditKind::Other, cx);
            return;
        }
        let cursor = doc.cursor();
        let range = cursor..doc.buffer.next_grapheme(cursor);
        self.edit(range, "", EditKind::Deleting, cx);
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.doc().selected.clone();
        self.edit(range, "\n", EditKind::Typing, cx);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        let range = self.doc().selected.clone();
        // The buffer is LF only, like the app's editor; files::write restores the file's line ending.
        self.edit(range, &text.replace("\r\n", "\n"), EditKind::Other, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc();
        if doc.selected.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(doc.buffer.slice(doc.selected.clone())));
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc().selected.is_empty() {
            return;
        }
        self.copy(&Copy, window, cx);
        let range = self.doc().selected.clone();
        self.edit(range, "", EditKind::Other, cx);
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc_mut();
        let Some((selection, splices)) = doc.buffer.undo() else { return };
        self.restore(selection, splices, cx);
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc_mut();
        let Some((selection, splices)) = doc.buffer.redo() else { return };
        self.restore(selection, splices, cx);
    }

    fn restore(&mut self, selection: Range<usize>, splices: Vec<LineSplice>, cx: &mut Context<Self>) {
        let doc = self.doc_mut();
        for splice in splices {
            doc.splice_lines(splice);
        }
        doc.selected = doc.buffer.boundary(selection.start)..doc.buffer.boundary(selection.end);
        doc.selection_reversed = false;
        doc.marked = None;
        self.after_change(cx);
    }

    // ----- Caret and selection -----

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc();
        let target = if doc.selected.is_empty() { doc.buffer.previous_grapheme(doc.cursor()) } else { doc.selected.start };
        self.move_to(target, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc();
        let target = if doc.selected.is_empty() { doc.buffer.next_grapheme(doc.cursor()) } else { doc.selected.end };
        self.move_to(target, cx);
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.vertical_target(self.doc().cursor(), -1);
        self.move_to(target, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.vertical_target(self.doc().cursor(), 1);
        self.move_to(target, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.previous_grapheme(self.doc().cursor());
        self.select_to(target, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.next_grapheme(self.doc().cursor());
        self.select_to(target, cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.vertical_target(self.doc().cursor(), -1);
        self.select_to(target, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.vertical_target(self.doc().cursor(), 1);
        self.select_to(target, cx);
    }

    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let start = self.home_target();
        self.select_to(start, cx);
    }

    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.end_target();
        self.select_to(end, cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.previous_word(self.doc().cursor());
        self.move_to(target, cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.next_word(self.doc().cursor());
        self.move_to(target, cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.previous_word(self.doc().cursor());
        self.select_to(target, cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.doc().buffer.next_word(self.doc().cursor());
        self.select_to(target, cx);
    }

    fn document_start(&mut self, _: &DocumentStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn document_end(&mut self, _: &DocumentEnd, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.doc().buffer.len();
        self.move_to(end, cx);
    }

    fn select_document_start(&mut self, _: &SelectDocumentStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_document_end(&mut self, _: &SelectDocumentEnd, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.doc().buffer.len();
        self.select_to(end, cx);
    }

    /// Ctrl+Backspace and Ctrl+Delete remove a word, or the selection if there is one.
    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc();
        let range = if doc.selected.is_empty() { doc.buffer.previous_word(doc.cursor())..doc.cursor() } else { doc.selected.clone() };
        self.edit(range, "", EditKind::Other, cx);
    }

    fn delete_word_right(&mut self, _: &DeleteWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc();
        let range = if doc.selected.is_empty() { doc.cursor()..doc.buffer.next_word(doc.cursor()) } else { doc.selected.clone() };
        self.edit(range, "", EditKind::Other, cx);
    }

    /// Tab indents the lines the selection touches, as CodeMirror's `indentMore` does; after Esc it
    /// moves the focus on instead.
    fn indent(&mut self, _: &Indent, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.tab_leaves) {
            window.focus_next(cx);
            return;
        }
        self.reindent(|line| Some(format!("{INDENT}{line}")), cx);
    }

    /// Shift+Tab removes one indent unit (or a tab) from each line, as `indentLess` does.
    fn outdent(&mut self, _: &Outdent, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.tab_leaves) {
            window.focus_prev(cx);
            return;
        }
        self.reindent(
            |line| {
                let removed = line.strip_prefix(INDENT).or_else(|| line.strip_prefix('\t')).or_else(|| line.strip_prefix(' '))?;
                Some(removed.to_string())
            },
            cx,
        );
    }

    /// Rewrites each line the selection touches with `change` (None leaves it), as one undo step,
    /// and keeps the selection on the same text.
    fn reindent(&mut self, change: impl Fn(&str) -> Option<String>, cx: &mut Context<Self>) {
        let doc = self.doc();
        let selected = doc.selected.clone();
        let first = doc.buffer.line_of(selected.start);
        let mut last = doc.buffer.line_of(selected.end);
        // A selection that ends at the start of a line does not touch that line.
        if last > first && doc.buffer.line_start(last) == selected.end {
            last -= 1;
        }
        let span = doc.buffer.line_start(first)..doc.buffer.line_range(last).end;
        let old = doc.buffer.slice(span.clone());
        let mut new = String::with_capacity(old.len() + (last - first + 1) * INDENT.len());
        let (mut shift_start, mut shift_end) = (0isize, 0isize);
        let mut line_start = span.start;
        for (index, line) in old.split('\n').enumerate() {
            if index > 0 {
                new.push('\n');
            }
            let changed = change(line).unwrap_or_else(|| line.to_string());
            let delta = changed.len() as isize - line.len() as isize;
            if line_start <= selected.start {
                shift_start += delta.max(line_start as isize - selected.start as isize);
            }
            if line_start <= selected.end {
                shift_end += delta.max(line_start as isize - selected.end as isize);
            }
            new.push_str(&changed);
            line_start += line.len() + 1;
        }
        if new == old {
            return;
        }
        let reversed = doc.selection_reversed;
        self.edit(span, &new, EditKind::Other, cx);
        let doc = self.doc_mut();
        let start = (selected.start as isize + shift_start).max(0) as usize;
        let end = (selected.end as isize + shift_end).max(start as isize) as usize;
        doc.selected = doc.buffer.boundary(start)..doc.buffer.boundary(end);
        doc.selection_reversed = reversed;
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc_mut();
        doc.selected = 0..doc.buffer.len();
        doc.selection_reversed = false;
        doc.buffer.break_group();
        cx.notify();
    }

    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        let start = self.home_target();
        self.move_to(start, cx);
    }

    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.end_target();
        self.move_to(end, cx);
    }

    /// A caret move also ends the current typing group, so the next keystroke is a new undo step.
    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.tab_leaves = false;
        let doc = self.doc_mut();
        let offset = doc.buffer.boundary(offset);
        doc.selected = offset..offset;
        doc.selection_reversed = false;
        doc.buffer.break_group();
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.tab_leaves = false;
        let doc = self.doc_mut();
        let offset = doc.buffer.boundary(offset);
        if doc.selection_reversed {
            doc.selected.start = offset;
        } else {
            doc.selected.end = offset;
        }
        if doc.selected.end < doc.selected.start {
            doc.selection_reversed = !doc.selection_reversed;
            doc.selected = doc.selected.end..doc.selected.start;
        }
        doc.buffer.break_group();
        cx.notify();
    }

    fn select(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let doc = self.doc_mut();
        doc.selected = doc.buffer.boundary(range.start)..doc.buffer.boundary(range.end);
        doc.selection_reversed = false;
        doc.marked = None;
        doc.buffer.break_group();
        cx.notify();
    }

    /// The character under `position` on source line `line`. GPUI's layout of that line maps the
    /// point, wrapped or not; the line is on screen, so it has been laid out.
    fn offset_at(&self, line: usize, layout: &TextLayout, position: Point<Pixels>) -> usize {
        let range = self.doc().buffer.line_range(line);
        let (Ok(index) | Err(index)) = layout.index_for_position(position);
        range.start + index.min(range.len())
    }

    /// A click on a source line: places the caret, or extends the selection with Shift. A double
    /// click selects the word and a triple click the line. Dragging then extends from here.
    fn click_line(&mut self, line: usize, layout: &TextLayout, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.open_menu = None;
        let offset = self.offset_at(line, layout, event.position);
        self.dragging = true;
        match event.click_count {
            2 => {
                let word = self.doc().buffer.word_at(offset);
                self.select(word, cx);
            }
            count if count >= 3 => {
                let doc = self.doc();
                let line = doc.buffer.line_of(offset);
                let range = doc.buffer.line_start(line)..doc.buffer.line_start(line + 1);
                self.select(range, cx);
            }
            _ if event.modifiers.shift => self.select_to(offset, cx),
            _ => self.move_to(offset, cx),
        }
    }

    /// While the left button is held after a click in the source, the selection follows the pointer.
    fn drag_line(&mut self, line: usize, layout: &TextLayout, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            return;
        }
        let offset = self.offset_at(line, layout, event.position);
        if offset != self.doc().cursor() {
            self.select_to(offset, cx);
        }
    }

    /// A drag selection that leaves the source pane keeps going, as in CodeMirror: beside the pane it
    /// extends to the start or end of the line level with the pointer; above or below it the pane
    /// scrolls towards the pointer, faster the further away it is, until the button comes up.
    fn drag_anywhere(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging || !self.settings.show_editor {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            self.drag_outside = None;
            return;
        }
        let pane = self.doc().lines.viewport_bounds();
        let position = event.position;
        if position.y >= pane.top() && position.y < pane.bottom() {
            self.drag_outside = None;
            let beside = position.x < pane.left() || position.x >= pane.right();
            if let Some(line) = beside.then(|| self.visible_line_at(position.y)).flatten() {
                let range = self.doc().buffer.line_range(line);
                self.select_to(if position.x < pane.left() { range.start } else { range.end }, cx);
            }
            return;
        }
        let started = self.drag_outside.is_none();
        self.drag_outside = Some(position);
        if started {
            self.drag_scroll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_millis(16)).await;
                    if !this.update(cx, |this, cx| this.drag_scroll_step(cx)).unwrap_or(false) {
                        break;
                    }
                }
            }));
        }
    }

    /// One frame of scrolling while a drag is above or below the pane. Returns whether to go on.
    fn drag_scroll_step(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(pointer) = self.drag_outside.filter(|_| self.dragging) else { return false };
        let doc = self.doc();
        let pane = doc.lines.viewport_bounds();
        let up = pointer.y < pane.top();
        let distance = if up { pane.top() - pointer.y } else { pointer.y - pane.bottom() };
        let step = (distance * 0.5).clamp(px(4.), px(80.));
        doc.lines.scroll_by(if up { -step } else { step });
        let target = if up {
            doc.buffer.line_start(doc.lines.logical_scroll_top().item_ix)
        } else {
            let last = self.visible_line_at(pane.bottom() - px(1.)).unwrap_or(doc.buffer.line_count().saturating_sub(1));
            doc.buffer.line_range(last).end
        };
        self.select_to(target, cx);
        // The caret is meant to be at the edge, so the follow-the-caret scroll must not undo the drag.
        let line = self.doc().buffer.line_of(self.doc().cursor());
        self.doc_mut().followed_cursor_line = Some(line);
        true
    }

    /// The source line drawn at height `y`, among the lines laid out in the last frame.
    fn visible_line_at(&self, y: Pixels) -> Option<usize> {
        let lines = &self.doc().lines;
        let top = lines.logical_scroll_top().item_ix;
        (top..lines.item_count()).take(400).map_while(|line| lines.bounds_for_item(line).map(|bounds| (line, bounds))).find(|(_, bounds)| bounds.bottom() > y).map(|(line, _)| line)
    }

    /// A click below the last line puts the caret at the end.
    fn click_source(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.open_menu = None;
        let end = self.doc().buffer.len();
        self.move_to(end, cx);
    }

    fn jump_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let offset = self.doc().buffer.line_start(line.saturating_sub(1));
        self.move_to(offset, cx);
    }

    // ----- Find and replace -----

    fn open_find(&mut self, show_replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        if self.find.is_none() {
            let query = cx.new(|cx| TextField::new("Find", cx));
            let replacement = cx.new(|cx| TextField::new("Replace with", cx));
            let subscriptions = vec![
                cx.subscribe(&query, |this, _, event: &FieldEvent, cx| match event {
                    FieldEvent::Changed => this.search_from_caret(cx),
                    FieldEvent::Submitted => this.find_step(1, cx),
                }),
                cx.subscribe(&replacement, |this, _, event: &FieldEvent, cx| {
                    if let FieldEvent::Submitted = event {
                        this.replace_current(cx);
                    }
                }),
            ];
            self.find = Some(FindBar { query, replacement, show_replace, matches: Vec::new(), searched: None, _subscriptions: subscriptions });
        }
        let Some(find) = self.find.as_mut() else { return };
        find.show_replace |= show_replace;
        let query = find.query.clone();
        // A selection on one line becomes the query, as in most editors.
        let doc = self.doc();
        let selected = doc.buffer.slice(doc.selected.clone());
        if !selected.is_empty() && !selected.contains('\n') {
            query.update(cx, |field, cx| field.set_text(&selected, cx));
        }
        let handle = query.focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    fn find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(false, window, cx);
    }

    fn find_replace(&mut self, _: &FindReplace, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(true, window, cx);
    }

    fn find_next(&mut self, _: &FindNext, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.open_find(false, window, cx);
            return;
        }
        self.find_step(1, cx);
    }

    fn find_previous(&mut self, _: &FindPrevious, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.open_find(false, window, cx);
            return;
        }
        self.find_step(-1, cx);
    }

    /// Esc closes what is open, innermost first: a menu, the settings panel, the find bar. With
    /// nothing open, in the editor, it lets the next Tab leave the editor instead of indenting.
    fn find_close(&mut self, _: &FindClose, window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
        if self.open_menu.is_some() {
            self.close_menus(window, cx);
            return;
        }
        if self.close_recent(window, cx) {
            return;
        }
        if self.close_settings(window, cx) {
            return;
        }
        if self.find.take().is_some() {
            self.focus_editor(window, cx);
            return;
        }
        if self.focus.is_focused(window) {
            self.tab_leaves = true;
        }
    }

    /// Searches again only when the query, the document or its text changed.
    fn refresh_matches(&mut self, cx: &App) {
        let Some(find) = self.find.as_mut() else { return };
        let doc = &self.documents[self.active];
        let query = find.query.read(cx).text();
        // This runs every frame; compare before copying the query.
        let current = find.searched.as_ref().is_some_and(|(id, version, searched)| *id == doc.id && *version == doc.buffer.version() && searched == query);
        if current {
            return;
        }
        find.matches = buffer::find_all(&doc.buffer.text(), query);
        find.searched = Some((doc.id, doc.buffer.version(), query.to_string()));
    }

    fn matches(&self) -> &[Range<usize>] {
        self.find.as_ref().map(|find| find.matches.as_slice()).unwrap_or(&[])
    }

    /// While typing a query, the first match at or after the caret is selected.
    fn search_from_caret(&mut self, cx: &mut Context<Self>) {
        self.refresh_matches(cx);
        let from = self.doc().selected.start;
        let matches = self.matches();
        let Some(found) = matches.iter().find(|found| found.start >= from).or(matches.first()).cloned() else {
            cx.notify();
            return;
        };
        self.select(found, cx);
    }

    fn find_step(&mut self, direction: isize, cx: &mut Context<Self>) {
        self.refresh_matches(cx);
        let selected = self.doc().selected.clone();
        let matches = self.matches();
        let found = if direction > 0 {
            matches.iter().find(|found| found.start >= selected.end && **found != selected).or(matches.first())
        } else {
            matches.iter().rev().find(|found| found.end <= selected.start).or(matches.last())
        };
        let Some(found) = found.cloned() else { return };
        self.select(found, cx);
    }

    fn replacement_text(&self, cx: &App) -> String {
        self.find.as_ref().map(|find| find.replacement.read(cx).text().to_string()).unwrap_or_default()
    }

    /// Replaces the selected match, then selects the next one.
    fn replace_current(&mut self, cx: &mut Context<Self>) {
        self.refresh_matches(cx);
        let selected = self.doc().selected.clone();
        if self.matches().contains(&selected) {
            let replacement = self.replacement_text(cx);
            self.edit(selected, &replacement, EditKind::Other, cx);
            self.refresh_matches(cx);
        }
        self.find_step(1, cx);
    }

    /// One edit from the first match to the last, so Replace all is a single undo step.
    fn replace_all(&mut self, cx: &mut Context<Self>) {
        self.refresh_matches(cx);
        let matches = self.matches().to_vec();
        let (Some(first), Some(last)) = (matches.first(), matches.last()) else { return };
        let span = first.start..last.end;
        let replacement = self.replacement_text(cx);
        let text = self.doc().buffer.slice(span.clone());
        let mut replaced = String::with_capacity(text.len());
        let mut copied = span.start;
        for found in &matches {
            replaced.push_str(&text[copied - span.start..found.start - span.start]);
            replaced.push_str(&replacement);
            copied = found.end;
        }
        self.edit(span, &replaced, EditKind::Other, cx);
        self.status = Some(format!("Replaced {} matches", matches.len()));
    }

    // ----- Files, tabs and closing -----

    /// Restores the file's BOM and line ending. A clean buffer and a lossy decode are left on disk as they are.
    /// A New Tab document has no file yet, so Save asks for a name instead.
    fn write_active(&mut self, cx: &mut Context<Self>) {
        self.write_document(self.doc().id, cx);
    }

    fn write_document(&mut self, id: u64, cx: &mut Context<Self>) {
        self.open_menu = None;
        cx.notify();
        let Some(index) = self.document_index(id) else { return };
        let doc = &self.documents[index];
        if doc.path.is_empty() {
            self.save_as_document(id, cx);
            return;
        }
        if doc.read_only {
            let message = format!("Not saved: {} is not valid UTF-8", doc.path);
            self.report(message);
            return;
        }
        if !doc.is_dirty() {
            return;
        }
        let target = PathBuf::from(&doc.path);
        self.start_save(id, target, cx);
    }

    fn document_index(&self, id: u64) -> Option<usize> {
        self.documents.iter().position(|doc| doc.id == id)
    }

    /// Writes the document on a background thread: syncing a file to disk can take tens of
    /// milliseconds, which typing must not wait for. One save per document runs at a time, so an
    /// older snapshot never lands after a newer one; a save asked for meanwhile runs next.
    fn start_save(&mut self, id: u64, target: PathBuf, cx: &mut Context<Self>) {
        let Some(index) = self.document_index(id) else { return };
        let doc = &mut self.documents[index];
        if doc.save_task.is_some() {
            doc.queued_save = Some(target);
            return;
        }
        let point = doc.buffer.save_point();
        let rope = doc.buffer.snapshot();
        let meta = doc.meta;
        doc.save_task = Some(cx.spawn(async move |this, cx| {
            let path = target.clone();
            let written = cx.background_executor().spawn(async move { files::write(&path, &rope.to_string(), meta) }).await;
            let _ = this.update(cx, |this, cx| this.finish_save(id, target, point, written, cx));
        }));
    }

    /// The file now holds the text as it was when the save started; later edits stay unsaved.
    /// A save to a new path (Save As) also moves the tab, its watcher and the recent list there.
    fn finish_save(&mut self, id: u64, target: PathBuf, point: SavePoint, written: std::io::Result<u64>, cx: &mut Context<Self>) {
        let Some(index) = self.document_index(id) else { return };
        let doc = &mut self.documents[index];
        doc.save_task = None;
        let queued = doc.queued_save.take();
        cx.notify();
        match written {
            Ok(mtime) => {
                let moved = doc.path.is_empty() || !files::same_path(Path::new(&doc.path), &target);
                doc.path = target.to_string_lossy().into_owned();
                doc.mtime_ms = mtime;
                doc.read_only = false;
                doc.restored = false;
                doc.buffer.mark_saved_at(point);
                self.status = None;
                self.record_recent(&target, cx);
                if moved {
                    self.watch_document(id, cx);
                }
            }
            Err(err) => self.report(format!("Could not save {}: {err}", target.display())),
        }
        let Some(next) = queued else { return };
        let doc = &self.documents[index];
        if files::same_path(Path::new(&doc.path), &next) && !doc.is_dirty() {
            return;
        }
        self.start_save(id, next, cx);
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        self.write_active(cx);
    }

    fn open_file(&mut self, cx: &mut Context<Self>) {
        self.open_menu = None;
        cx.notify();
        let Some(path) = rfd::FileDialog::new().add_filter("Markdown", files::MARKDOWN_EXTS).pick_file() else {
            return;
        };
        self.open_path(&path, cx);
    }

    /// Opens `path` as a tab, or switches to the tab that already has it.
    fn open_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(index) = self.documents.iter().position(|doc| files::same_path(Path::new(&doc.path), path)) {
            self.record_recent(path, cx);
            self.activate_tab(index, cx);
            return;
        }
        match files::read(path) {
            Ok(opened) => {
                self.status = None;
                let id = self.next_document_id;
                self.next_document_id += 1;
                let document = Document::open(opened, id, cx);
                self.documents.push(document);
                self.schedule_parse(id, Duration::ZERO, cx);
                self.watch_document(id, cx);
                self.record_recent(path, cx);
                self.activate_tab(self.documents.len() - 1, cx);
            }
            Err(err) => self.report(format!("Could not open {}: {err}", path.display())),
        }
    }

    fn new_tab(&mut self, cx: &mut Context<Self>) {
        let id = self.next_document_id;
        self.next_document_id += 1;
        let opened = OpenedFile::untitled(self.settings.new_file_line_ending);
        self.documents.push(Document::open(opened, id, cx));
        self.schedule_parse(id, Duration::ZERO, cx);
        self.activate_tab(self.documents.len() - 1, cx);
        self.status = None;
    }

    /// Saves under a new name. A New Tab document has no path until this runs.
    fn save_as_document(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.document_index(id) else { return };
        let suggested = if self.documents[index].path.is_empty() { "Untitled.md".to_string() } else { self.documents[index].display_name() };
        let Some(target) = rfd::FileDialog::new().add_filter("Markdown", files::MARKDOWN_EXTS).set_file_name(suggested).save_file() else {
            return;
        };
        self.start_save(id, target, cx);
    }

    /// Writing the list syncs it to disk, so it happens in the background, and not at all when the
    /// path is already at the top (every auto-save of the same file).
    fn record_recent(&mut self, path: &Path, cx: &mut Context<Self>) {
        if path.as_os_str().is_empty() || self.recent.first().is_some_and(|first| files::same_path(first, path)) {
            return;
        }
        let config_dir = self.config_dir.clone();
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let recent = cx
                .background_executor()
                .spawn(async move {
                    store::push_recent(&config_dir, &path)?;
                    Ok::<_, std::io::Error>(store::load_recent(&config_dir).into_iter().filter(|path| path.is_file()).collect::<Vec<_>>())
                })
                .await;
            let _ = this.update(cx, |this, cx| match recent {
                Ok(recent) => {
                    this.recent = recent;
                    cx.notify();
                }
                Err(err) => diagnostics::write(&format!("could not update the recent files list: {err}")),
            });
        })
        .detach();
    }

    /// Watches a document's file for changes made by another program. The app debounces and compares
    /// mtimes; `check_external_change` does the same, after the burst settles.
    fn watch_document(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.document_index(id) else { return };
        if self.documents[index].path.is_empty() {
            self.documents[index].watcher = None;
            self.documents[index].external_task = None;
            return;
        }
        let path = PathBuf::from(&self.documents[index].path);
        let (sender, receiver) = async_channel::unbounded::<()>();
        match watcher::watch(&path, move || {
            let _ = sender.try_send(());
        }) {
            Ok(watcher) => self.documents[index].watcher = Some(watcher),
            Err(err) => {
                self.documents[index].watcher = None;
                diagnostics::write(&format!("could not watch {}: {err}", path.display()));
            }
        }
        self.documents[index].external_task = Some(cx.spawn(async move |this, cx| {
            while receiver.recv().await.is_ok() {
                // A save can arrive as several events; read once after they stop.
                while receiver.try_recv().is_ok() {}
                cx.background_executor().timer(Duration::from_millis(200)).await;
                while receiver.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.check_external_change(id, cx)).is_err() {
                    break;
                }
            }
        }));
    }

    fn check_external_change(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.document_index(id) else { return };
        // Our own save is still being written; its mtime is recorded when it finishes.
        if self.documents[index].save_task.is_some() {
            return;
        }
        let path = PathBuf::from(&self.documents[index].path);
        if path.as_os_str().is_empty() {
            return;
        }
        let Some(now) = files::mtime_ms(&path).ok() else {
            self.report(format!("{} was removed from disk", self.documents[index].display_name()));
            return;
        };
        if now == self.documents[index].mtime_ms {
            return;
        }
        let Ok(opened) = files::read(&path) else { return };
        // Our own save, or another program writing the same bytes: only the time changed.
        if opened.text == self.documents[index].buffer.text() {
            self.documents[index].mtime_ms = opened.mtime_ms;
            self.documents[index].meta = opened.meta;
            return;
        }
        if self.documents[index].is_dirty() {
            let name = self.documents[index].display_name();
            let reload = rfd::MessageDialog::new()
                .set_title("Marky")
                .set_level(rfd::MessageLevel::Warning)
                .set_description(format!("{name} changed on disk. Reload it and lose your unsaved changes?"))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if reload != rfd::MessageDialogResult::Yes {
                // Keep mine: later events for the same file are ignored until it changes again.
                self.documents[index].mtime_ms = opened.mtime_ms;
                return;
            }
        }
        let name = self.documents[index].display_name();
        self.reload_document(id, opened, cx);
        self.report(format!("Reloaded {name} from disk"));
    }

    fn reload_document(&mut self, id: u64, opened: OpenedFile, cx: &mut Context<Self>) {
        let Some(index) = self.document_index(id) else { return };
        self.documents[index] = Document::open(opened, id, cx);
        self.preview_link = None;
        self.schedule_parse(id, Duration::ZERO, cx);
        self.watch_document(id, cx);
        self.release_unused_images(cx);
        cx.notify();
    }

    fn schedule_auto_save(&mut self, id: u64, cx: &mut Context<Self>) {
        if !self.settings.auto_save {
            return;
        }
        let Some(index) = self.document_index(id) else { return };
        if self.documents[index].path.is_empty() || self.documents[index].read_only {
            return;
        }
        self.documents[index].auto_save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1000)).await;
            let _ = this.update(cx, |this, cx| {
                if !this.settings.auto_save {
                    return;
                }
                if this.documents.iter().any(|doc| doc.id == id && doc.is_dirty()) {
                    this.write_document(id, cx);
                }
            });
        }));
    }

    // ----- Crash recovery, recent files and the single-instance request -----

    /// The app's recovery prompt. Yes restores the tabs, No deletes the snapshots (an explicit
    /// Discard), and Cancel holds them: they are written back with the next snapshot and on close.
    fn restore_or_discard(&mut self, recovery: Vec<store::Snapshot>, cx: &mut Context<Self>) {
        if recovery.is_empty() {
            return;
        }
        let names: Vec<String> = recovery
            .iter()
            .map(|snapshot| snapshot.path.as_deref().map(file_name).unwrap_or_else(|| "Untitled".to_string()))
            .collect();
        let answer = rfd::MessageDialog::new()
            .set_title("Marky")
            .set_level(rfd::MessageLevel::Warning)
            .set_description(format!("Restore unsaved changes to {}?", names.join(", ")))
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        match answer {
            rfd::MessageDialogResult::Yes => {
                for snapshot in recovery {
                    let path = snapshot.path.unwrap_or_default();
                    let mtime_ms = if path.is_empty() { 0 } else { files::mtime_ms(Path::new(&path)).unwrap_or(0) };
                    let opened = OpenedFile { path: path.clone(), text: snapshot.text, meta: snapshot.meta, mtime_ms, read_only: false };
                    // The startup file may already be open; its recovered text replaces what was read.
                    if !path.is_empty() {
                        if let Some(index) = self.documents.iter().position(|doc| files::same_path(Path::new(&doc.path), Path::new(&path))) {
                            let id = self.documents[index].id;
                            let mut document = Document::open(opened, id, cx);
                            document.restored = true;
                            self.documents[index] = document;
                            self.schedule_parse(id, Duration::ZERO, cx);
                            self.watch_document(id, cx);
                            continue;
                        }
                    }
                    let id = self.next_document_id;
                    self.next_document_id += 1;
                    let mut document = Document::open(opened, id, cx);
                    document.restored = true;
                    self.documents.push(document);
                    self.schedule_parse(id, Duration::ZERO, cx);
                    self.watch_document(id, cx);
                }
                self.activate_tab(self.documents.len() - 1, cx);
            }
            rfd::MessageDialogResult::No => {
                if let Err(err) = store::clear_snapshots(&self.recovery_dir) {
                    diagnostics::write(&format!("could not clear recovery snapshots: {err}"));
                }
            }
            _ => {
                self.held_recovery = recovery;
                self.recovery_dirty = true;
            }
        }
    }

    fn start_recovery_timer(&mut self, cx: &mut Context<Self>) {
        self.recovery_task = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(30)).await;
            if this.update(cx, |this, cx| this.write_recovery(cx)).is_err() {
                break;
            }
        }));
    }

    fn recovery_versions(&self) -> Vec<(u64, u64)> {
        self.documents.iter().filter(|doc| doc.is_dirty()).map(|doc| (doc.id, doc.buffer.version())).collect()
    }

    /// What the next recovery write holds, or `None` when no buffer version and no held snapshot
    /// changed since the last one: serialising every dirty document is itself a cost at 8 MB.
    fn take_recovery_changes(&mut self) -> Option<RecoveryWrite> {
        let versions = self.recovery_versions();
        if !self.recovery_dirty && versions == self.recovery_versions {
            return None;
        }
        self.recovery_versions = versions;
        self.recovery_dirty = false;
        let documents = self
            .documents
            .iter()
            .filter(|doc| doc.is_dirty())
            .map(|doc| ((!doc.path.is_empty()).then(|| doc.path.clone()), doc.buffer.snapshot(), doc.meta))
            .collect();
        Some(RecoveryWrite { held: self.held_recovery.clone(), documents })
    }

    fn write_recovery(&mut self, cx: &mut Context<Self>) {
        let Some(changes) = self.take_recovery_changes() else { return };
        let dir = self.recovery_dir.clone();
        cx.background_spawn(async move {
            if let Err(err) = changes.write(&dir) {
                diagnostics::write(&format!("could not save recovery snapshots: {err}"));
            }
        })
        .detach();
    }

    /// Synchronous, for closing: a background write could be lost when the process ends.
    fn flush_recovery(&mut self) {
        let Some(changes) = self.take_recovery_changes() else { return };
        if let Err(err) = changes.write(&self.recovery_dir) {
            diagnostics::write(&format!("could not save recovery snapshots on close: {err}"));
        }
    }

    fn watch_requests(&mut self, cx: &mut Context<Self>) {
        let dir = self.config_dir.clone();
        let (sender, receiver) = async_channel::unbounded::<()>();
        match watcher::watch(&instance::request_path(&dir), move || {
            let _ = sender.try_send(());
        }) {
            Ok(watcher) => self.request_watcher = Some(watcher),
            Err(err) => diagnostics::write(&format!("could not watch for a second launch: {err}")),
        }
        cx.spawn(async move |this, cx| {
            while receiver.recv().await.is_ok() {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                while receiver.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.check_pending_request(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn check_pending_request(&mut self, cx: &mut Context<Self>) {
        match instance::take_request(&self.config_dir) {
            Some(Some(path)) => self.open_path(&path, cx),
            Some(None) => cx.activate(true),
            None => {}
        }
    }

    /// Every link the preview draws, in document order, with the block it is in. The preview is one
    /// tab stop; the left and right arrows move through this list and Enter follows the chosen link.
    fn preview_links(&self) -> &[(Arc<str>, usize)] {
        &self.doc().links
    }

    /// The preview's text as one read-only AccessKit text pattern, capped like the editor's so a
    /// large document does not build a tree the size of its text on every frame.
    fn preview_described(&self, window: &Window) -> Option<Vec<a11y::Line>> {
        if !window.is_a11y_active() {
            return None;
        }
        let mut lines = Vec::new();
        let mut spent = 0;
        for (index, block) in self.doc().blocks.iter().enumerate() {
            if lines.len() >= a11y::LINES_DESCRIBED || spent >= a11y::CHARS_DESCRIBED {
                break;
            }
            let text = preview_view::accessible_text(block);
            if text.is_empty() {
                continue;
            }
            spent += text.len();
            lines.push(a11y::Line { index, text, bounds: None, positions: None, widths: None });
        }
        // AccessKit marks a line end with the line break character, as in the editor's runs.
        let last = lines.len().saturating_sub(1);
        for line in lines.iter_mut().take(last) {
            line.text.push('\n');
        }
        Some(lines)
    }

    /// A click on a link in the preview, with the app's rules (`followLink`): web and mail links open
    /// in the default program; a footnote reference scrolls the preview to its footnote; a relative
    /// link opens a .md, .markdown or .txt file next to this one as a tab. The preview never navigates.
    pub fn follow_link(&mut self, link: &str, _window: &mut Window, cx: &mut Context<Self>) {
        if link.starts_with("http://") || link.starts_with("https://") || link.starts_with("mailto:") {
            cx.open_url(link);
            return;
        }
        if let Some(number) = link.strip_prefix("#fn:").and_then(|number| number.parse::<u32>().ok()) {
            let doc = self.doc();
            if let Some(index) = doc.blocks.iter().position(|block| block.kind == Kind::Footnote(number)) {
                doc.block_list.scroll_to(ListOffset { item_ix: index, offset_in_item: px(0.) });
                cx.notify();
            }
            return;
        }
        // A footnote's ↩ scrolls back to the block that refers to it.
        if let Some(number) = link.strip_prefix("#fnref:") {
            let target = format!("#fn:{number}");
            let doc = self.doc();
            let refers = |runs: &[preview::Run]| runs.iter().any(|run| run.style.link.as_deref() == Some(target.as_str()));
            let found = doc.blocks.iter().position(|block| refers(&block.runs) || block.table.as_ref().is_some_and(|table| table.rows.iter().flatten().any(|cell| refers(cell))));
            if let Some(index) = found {
                doc.block_list.scroll_to(ListOffset { item_ix: index, offset_in_item: px(0.) });
                cx.notify();
            }
            return;
        }
        // The app gives headings no ids, so other #anchors go nowhere, as there.
        if link.starts_with('#') {
            return;
        }
        let target = link.split('#').next().unwrap_or_default().replace("%20", " ");
        match allowlist::link_target(Path::new(&self.doc().path), &target) {
            Ok(path) => self.open_path(&path, cx),
            Err(err) => {
                self.report(format!("Not opened: {link} ({err})"));
                cx.notify();
            }
        }
    }

    /// Each tab keeps its own lists, so switching back restores its scroll positions.
    fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.documents.len() {
            return;
        }
        self.active = index;
        self.preview_link = None;
        self.doc_mut().synced_source_line = None;
        cx.notify();
    }

    fn dirty_count(&self) -> usize {
        self.documents.iter().filter(|doc| doc.is_dirty()).count()
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.documents.len() == 1 || index >= self.documents.len() {
            return;
        }
        if !self.documents[index].is_dirty() {
            self.remove_tab(index, cx);
            return;
        }
        // Tabs can move while the prompt is open, so the answer finds the tab again by id.
        let id = self.documents[index].id;
        let message = format!("Discard unsaved changes in {}?", self.documents[index].display_name());
        self.confirm_discard(&message, window, cx, move |this, _, cx| {
            if let Some(index) = this.document_index(id) {
                this.remove_tab(index, cx);
            }
        });
    }

    fn remove_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.documents.len() == 1 || index >= self.documents.len() {
            return;
        }
        self.documents.remove(index);
        if self.active > index || self.active == self.documents.len() {
            self.active -= 1;
        }
        self.doc_mut().synced_source_line = None;
        self.release_unused_images(cx);
        cx.notify();
    }

    /// Asks before unsaved edits are thrown away. The prompt is asynchronous, so `then` runs later, and only on Discard.
    fn confirm_discard(
        &mut self,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let answer = window.prompt(PromptLevel::Warning, message, Some("Unsaved changes will be lost."), &["Discard", "Cancel"], cx);
        cx.spawn_in(window, async move |this, cx| {
            if !matches!(answer.await, Ok(0)) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| then(this, window, cx));
        })
        .detach();
    }

    /// The title-bar X and Quit both come here. Returns whether the window may close now.
    pub fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.documents.iter().any(|doc| doc.save_task.is_some()) {
            self.close_after_saves(window, cx);
            return false;
        }
        let dirty = self.dirty_count();
        if dirty == 0 || self.close_confirmed {
            // A postponed recovery survives a normal close; with nothing left to recover, this clears it.
            self.flush_recovery();
            return true;
        }
        let message = if dirty == 1 { "Discard unsaved changes in 1 file?".to_string() } else { format!("Discard unsaved changes in {dirty} files?") };
        self.confirm_discard(&message, window, cx, |this, window, _| {
            // Discarding everything: nothing is left to recover, and held snapshots stay held.
            for document in &mut this.documents {
                document.restored = false;
                document.buffer.mark_saved();
            }
            this.close_confirmed = true;
            this.flush_recovery();
            window.remove_window();
        });
        false
    }

    /// A close while a save is being written waits for it and then asks again, so the file is
    /// complete and the unsaved-changes prompt sees what was saved. Saves take milliseconds.
    fn close_after_saves(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_task.is_some() {
            return;
        }
        self.close_task = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(15)).await;
                let Ok(saving) = this.update(cx, |this, _| this.documents.iter().any(|doc| doc.save_task.is_some())) else { return };
                if !saving {
                    break;
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.close_task = None;
                if this.may_close(window, cx) {
                    window.remove_window();
                }
            });
        }));
    }

    fn quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        if self.may_close(window, cx) {
            cx.quit();
        }
    }

    // ----- Menus and view settings -----

    fn close_menu(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.open_menu.is_none() {
            return;
        }
        self.open_menu = None;
        cx.notify();
    }

    fn step_font(&mut self, delta: isize, cx: &mut Context<Self>) {
        let current = settings::FONT_SIZES.iter().position(|size| *size == self.settings.font_size).unwrap_or(2);
        let next = current as isize + delta;
        if next < 0 || next >= settings::FONT_SIZES.len() as isize {
            self.open_menu = None;
            cx.notify();
            return;
        }
        self.settings.font_size = settings::FONT_SIZES[next as usize];
        // Formulas are laid out per font size, so the old size's bitmaps are no longer drawn.
        self.release_unused_images(cx);
        self.keep_settings(cx);
    }

    /// Writes the restored window bounds a moment after a resize or move stops, so dragging does
    /// not write the file on every frame.
    fn schedule_geometry_save(&mut self, window: &Window, cx: &mut Context<Self>) {
        let bounds = match window.window_bounds() {
            WindowBounds::Windowed(bounds) => (bounds, false),
            WindowBounds::Maximized(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, true),
        };
        let geometry = store::WindowGeometry {
            width: f64::from(f32::from(bounds.0.size.width)),
            height: f64::from(f32::from(bounds.0.size.height)),
            x: f64::from(f32::from(bounds.0.origin.x)),
            y: f64::from(f32::from(bounds.0.origin.y)),
            maximized: bounds.1,
        };
        let config_dir = self.config_dir.clone();
        self.geometry_task = Some(cx.spawn(async move |_this, cx| {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            if let Err(err) = store::save_window_geometry(&config_dir, &geometry) {
                diagnostics::write(&format!("could not save the window geometry: {err}"));
            }
        }));
    }

    fn keep_settings(&mut self, cx: &mut Context<Self>) {
        if let Err(message) = settings::save(&self.config_dir, &self.settings) {
            self.report(message);
        }
        self.open_menu = None;
        cx.notify();
    }

    // ----- Scrolling and timing -----

    /// The caret stays visible, and the preview shows the block for the top visible source line.
    fn sync_scroll(&mut self) {
        let show_editor = self.settings.show_editor;
        let show_preview = self.settings.preview_visible();
        let doc = self.doc_mut();
        let cursor_line = doc.buffer.line_of(doc.cursor());
        if doc.followed_cursor_line != Some(cursor_line) {
            doc.followed_cursor_line = Some(cursor_line);
            if show_editor {
                doc.lines.scroll_to_reveal_item(cursor_line);
            }
        }
        if !show_editor || !show_preview || doc.blocks.is_empty() {
            return;
        }
        let top = doc.lines.logical_scroll_top().item_ix;
        if doc.synced_source_line == Some(top) {
            return;
        }
        doc.synced_source_line = Some(top);
        let block = preview::block_at_line(&doc.blocks, top + 1);
        doc.block_list.scroll_to(ListOffset { item_ix: block, offset_in_item: px(0.) });
    }

    /// With word wrap off, keeps the caret in view sideways after it moves. The caret's bounds are
    /// only known once it is painted, so the scroll happens on the frame after the move; scrolling by
    /// hand is left alone until the caret moves again. Returns whether the offset changed.
    fn follow_caret_sideways(&mut self) -> bool {
        let doc = self.doc();
        let cursor = (doc.id, doc.cursor());
        if self.last_cursor != Some(cursor) {
            self.last_cursor = Some(cursor);
            self.follow_x = true;
        }
        if !self.follow_x {
            return false;
        }
        let Some((_, caret)) = self.caret_painted.filter(|(painted, _)| *painted == cursor.1) else {
            return false;
        };
        self.follow_x = false;
        let view = self.h_scroll.bounds();
        let margin = px(32.);
        let mut offset = self.h_scroll.offset();
        if caret.right() > view.right() - margin {
            offset.x -= caret.right() - (view.right() - margin);
        } else if caret.left() < view.left() + margin {
            offset.x = (offset.x + (view.left() + margin - caret.left())).min(px(0.));
        } else {
            return false;
        }
        self.h_scroll.set_offset(offset);
        true
    }

    /// Startup and typing are timed to the end of the frame's paint, like the app's
    /// measurements (a rAF callback that reports after paint).
    fn frame_timer(&mut self) -> impl IntoElement {
        let first_frame = !self.logged_first_frame;
        self.logged_first_frame = true;
        let typed_at = self.typed_at.take();
        let started = self.frame_started.take();
        canvas(
            // Last child: its prepaint runs once layout and every other prepaint are done.
            move |_, _, _| {
                if first_frame {
                    perf::mark("layout");
                }
            },
            move |_, _, _, _| {
                if first_frame {
                    perf::log_startup();
                }
                if let Some(typed_at) = typed_at {
                    perf::log(&format!("type_ms={:.1}", typed_at.elapsed().as_secs_f64() * 1000.0));
                }
                // Render, layout, prepaint and paint of one frame, all on the main thread. The first
                // frame is start-up, timed as such.
                if let Some(started) = started.filter(|_| !first_frame) {
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    if ms > 50.0 {
                        perf::log(&format!("stall_ms={ms:.0} frame"));
                    }
                }
            },
        )
        .absolute()
        .size_0()
    }

    fn update_title(&mut self, window: &mut Window) {
        let doc = self.doc();
        let title = format!("{}{} - Marky", doc.display_name(), if doc.is_dirty() { " •" } else { "" });
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
    }
}

// ----- Drawing -----

impl Prototype {
    /// One source line, laid out only when the list needs it. Syntax colours, find matches and the
    /// selection are layered highlights on the line's own `StyledText`, whose layout also maps clicks
    /// and places the caret.
    fn render_line(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let dark = self.dark;
        let doc = &self.documents[self.active];
        let start = doc.buffer.line_start(ix);
        let text = doc.buffer.line(ix);
        let line = start..start + text.len();
        let selected = doc.selected.clone();
        let cursor = doc.cursor();
        // `~sub~` and `^sup^` are drawn at 80% and shifted; their delimiters are hidden. Ranges
        // inside inline code are left as typed, as in the preview.
        let scripts: Vec<(Range<usize>, bool)> = preview::script_ranges(&text)
            .into_iter()
            .filter(|(range, _)| {
                !doc.spans
                    .get(ix)
                    .is_some_and(|spans| spans.iter().any(|(span, token)| *token == syntax::Token::Code && span.start < range.end && range.start < span.end))
            })
            .collect();

        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        if !text.is_empty() {
            // Colours from the last parse. Right after an edit they can be a few characters off, or
            // past the end of the line, until the next parse; clamp them to character boundaries.
            for (range, token) in doc.spans.get(ix).map(Vec::as_slice).unwrap_or(&[]) {
                let span = text.floor_char_boundary(range.start)..text.floor_char_boundary(range.end);
                if !span.is_empty() {
                    highlights.push((span, token.style(dark)));
                }
            }
            let found_style = HighlightStyle { background_color: Some(tone(dark, 0x713f12, 0xfef08a).into()), ..HighlightStyle::default() };
            let matches = self.matches();
            let first = matches.partition_point(|found| found.end <= line.start);
            for found in matches[first..].iter().take_while(|found| found.start < line.end) {
                if found.start < selected.end && selected.start < found.end {
                    continue;
                }
                highlights.push((found.start.max(line.start) - start..found.end.min(line.end) - start, found_style));
            }
            let overlap = selected.start.max(line.start)..selected.end.min(line.end);
            if !overlap.is_empty() {
                let selection_style = HighlightStyle { background_color: Some(tone(dark, 0x1e3a8a, 0xbfdbfe).into()), ..HighlightStyle::default() };
                highlights.push((overlap.start - start..overlap.end - start, selection_style));
            }
            // The full-size text under a script stays for layout, caret and clicks, but is not drawn;
            // the 80% copy is painted over it in the canvas below, with the delimiters hidden.
            let hidden = HighlightStyle { color: Some(gpui::transparent_black()), ..HighlightStyle::default() };
            for (range, _) in &scripts {
                highlights.push((range.start.saturating_sub(1)..(range.end + 1).min(text.len()), hidden));
            }
        }
        // One copy of the line, shared by the text, the script overlay and the caret lookup.
        let text = SharedString::from(text);
        let styled_text = if text.is_empty() { SharedString::from(" ") } else { text.clone() };
        let styled = StyledText::new(styled_text).with_highlights(syntax::merge(&highlights));
        let layout = styled.layout().clone();
        self.prepaint_lines.borrow_mut().push((ix, layout.clone()));
        let click_layout = layout.clone();
        let drag_layout = layout.clone();
        let script_text = text.clone();
        let base_size = self.settings.font_size;
        let row = div()
            .relative()
            .px_3()
            .when(ix == 0, |row| row.pt_2())
            .map(|row| if self.settings.word_wrap { row.whitespace_normal() } else { row.whitespace_nowrap() })
            .child(styled)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.click_line(ix, &click_layout, event, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| this.drag_line(ix, &drag_layout, event, cx)));
        let caret_column = (selected.is_empty() && (line.start..=line.end).contains(&cursor)).then(|| cursor - line.start);
        let caret_next = caret_column.and_then(|column| text.get(column..)?.chars().next().map(|ch| column + ch.len_utf8()));
        let view = cx.entity();
        // Paints the caret, and records the line's layout once it has been painted: Home and End on
        // wrapped rows and the IME's point-to-character lookup may only use painted layouts.
        row.child(
            canvas(
                |_, _, _| {},
                move |_, _, window, cx| {
                    let caret = caret_column.and_then(|column| caret_origin(&layout, column, caret_next)).map(|origin| Bounds::new(origin, size(px(1.5), layout.line_height())));
                    if let Some(caret) = caret {
                        let color = crate::high_contrast::caret().unwrap_or_else(|| rgb(CARET));
                        window.paint_quad(fill(caret, color));
                    }
                    // The small script text: 80% of the line, above or below the baseline. The
                    // full-size copy underneath is transparent, so only this is visible.
                    let base = window.text_style();
                    for (range, sup) in &scripts {
                        let Some(origin) = layout.position_for_index(range.start) else { continue };
                        let content = &script_text[range.clone()];
                        if content.is_empty() {
                            continue;
                        }
                        let run = TextRun {
                            len: content.len(),
                            font: base.font(),
                            color: base.color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line_height = layout.line_height() * 0.8;
                        let small = window.text_system().shape_line(
                            SharedString::from(content.to_string()),
                            px(base_size * 0.8),
                            std::slice::from_ref(&run),
                            None,
                        );
                        let top = layout.bounds().top() + if *sup { px(0.) } else { layout.line_height() * 0.45 };
                        small.paint(point(origin.x, top), line_height, TextAlign::Left, None, window, cx).ok();
                    }
                    view.update(cx, |view, cx| {
                        view.painted_lines.push((ix, layout.clone()));
                        let Some(caret) = caret else { return };
                        view.caret_bounds = Some(caret);
                        view.caret_painted = Some((cursor, caret));
                        // Painted where it moved to: one more frame scrolls sideways to it.
                        if view.follow_x {
                            cx.notify();
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
    }

    /// The layout of source line `line` if it was painted in the last frame.
    fn painted_layout(&self, line: usize) -> Option<&TextLayout> {
        self.painted_lines.iter().find(|(painted, _)| *painted == line).map(|(_, layout)| layout)
    }

    /// The visual row the caret is on, as byte offsets into its line, when word wrap has split the
    /// line into several rows on screen.
    fn caret_row(&self) -> Option<Range<usize>> {
        if !self.settings.word_wrap {
            return None;
        }
        let doc = self.doc();
        let cursor = doc.cursor();
        let line = doc.buffer.line_of(cursor);
        let layout = self.painted_layout(line)?;
        let column = cursor - doc.buffer.line_start(line);
        let text = doc.buffer.line(line);
        let next = text.get(column..).and_then(|rest| rest.chars().next()).map(|ch| column + ch.len_utf8());
        let caret = caret_origin(layout, column, next)?;
        let bounds = layout.bounds();
        let y = caret.y + layout.line_height() / 2.;
        let (Ok(start) | Err(start)) = layout.index_for_position(point(bounds.left(), y));
        let (Ok(end) | Err(end)) = layout.index_for_position(point(bounds.right() - px(1.), y));
        let length = doc.buffer.line_range(line).len();
        (start > 0 || end < length).then(|| start.min(length)..end.min(length))
    }

    /// Home, as CodeMirror's `cursorLineBoundaryBackward`: the start of the visual row; from there
    /// (or on the first row), the end of the indentation, then the start of the line.
    fn home_target(&self) -> usize {
        let doc = self.doc();
        let cursor = doc.cursor();
        let line = doc.buffer.line_of(cursor);
        let start = doc.buffer.line_start(line);
        if let Some(row) = self.caret_row().filter(|row| row.start > 0 && start + row.start != cursor) {
            return start + row.start;
        }
        let text = doc.buffer.line(line);
        let indent = text.len() - text.trim_start().len();
        if indent > 0 && cursor != start + indent { start + indent } else { start }
    }

    /// End: the end of the visual row, then the end of the line. Without caret affinity the end of
    /// a wrapped row is drawn at the start of the next one, so the caret stops before the space the
    /// row was broken at (or its last character).
    fn end_target(&self) -> usize {
        let doc = self.doc();
        let cursor = doc.cursor();
        let line = doc.buffer.line_of(cursor);
        let range = doc.buffer.line_range(line);
        let Some(row) = self.caret_row().filter(|row| row.end < range.len()) else { return range.end };
        let text = doc.buffer.line(line);
        let target = range.start + text.floor_char_boundary(row.end.saturating_sub(1)).max(row.start);
        if target == cursor { range.end } else { target }
    }

    /// The editor's lines and selection for assistive technology (`a11y.rs`). Nothing is gathered
    /// while no screen reader or other client is listening.
    fn describe_text(&self, window: &Window) -> Option<(Vec<a11y::Line>, a11y::Point, a11y::Point)> {
        if !window.is_a11y_active() {
            return None;
        }
        let doc = self.doc();
        let buffer = &doc.buffer;
        let point_of = |offset: usize| {
            let line = buffer.line_of(offset);
            let start = buffer.line_start(line);
            a11y::Point { line, char_index: buffer.slice(start..offset).chars().count() }
        };
        let anchor_offset = if doc.selection_reversed { doc.selected.end } else { doc.selected.start };
        let (focus, anchor) = (point_of(doc.cursor()), point_of(anchor_offset));
        let last = buffer.line_count().saturating_sub(1);
        let lines = a11y::lines_to_describe(doc.lines.logical_scroll_top().item_ix, buffer.line_count(), focus.line, anchor.line)
            .into_iter()
            .map(|index| {
                let mut text = buffer.line(index);
                if index < last {
                    text.push('\n');
                }
                a11y::Line { index, text, bounds: None, positions: None, widths: None }
            })
            .collect();
        let lines = a11y::within_budget(lines, [focus.line, anchor.line]);
        Some((lines, anchor, focus))
    }

    /// The editing keys, which act only while the editor has the focus, so Enter and Space can press
    /// a focused button and the arrows can move through a menu.
    fn editor_actions<E: InteractiveElement>(&self, element: E, cx: &mut Context<Self>) -> E {
        element
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::document_start))
            .on_action(cx.listener(Self::document_end))
            .on_action(cx.listener(Self::select_document_start))
            .on_action(cx.listener(Self::select_document_end))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::indent))
            .on_action(cx.listener(Self::outdent))
    }

    fn editor_column(&mut self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let mut described = self.describe_text(window);
        let scale = window.scale_factor();
        let prepaint = self.prepaint_lines.clone();
        let lines = self.doc().lines.clone();
        let source = list(lines, cx.processor(|this, ix, _window, cx| this.render_line(ix, cx))).size_full();
        // Without word wrap, long lines scroll sideways: the list is as wide as the longest line.
        let source = if self.settings.word_wrap {
            source.into_any_element()
        } else {
            let width = self.char_width * self.doc().longest_line as f32 + px(48.);
            div()
                .id("source-x")
                .size_full()
                .overflow_x_scroll()
                .track_scroll(&self.h_scroll)
                .child(div().h_full().min_w_full().w(width).child(source))
                .into_any_element()
        };
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(tone(dark, 0x18181b, 0xfafafa))
            .text_color(tone(dark, 0xfafafa, 0x18181b))
            .child(InputLayer { view: cx.entity(), focus: self.focus.clone() })
            .child(self.editor_actions(
                div()
                    .id("source")
                    .key_context("Editor")
                    .track_focus(&self.focus)
                    .role(gpui::Role::MultilineTextInput)
                    .aria_label("Editor")
                    .a11y_synthetic_children(move |builder| {
                        if let Some((lines, anchor, focus)) = &mut described {
                            // The lines were laid out during this prepaint; their boxes and per-character
                            // positions are in window coordinates, which is what a magnifier uses to
                            // follow the caret. AccessKit needs both to turn the caret into a rectangle.
                            let layouts: std::collections::HashMap<usize, TextLayout> =
                                prepaint.borrow().iter().map(|(line, layout)| (*line, layout.clone())).collect();
                            for line in lines.iter_mut() {
                                let Some(layout) = layouts.get(&line.index) else { continue };
                                let bounds = layout.bounds();
                                let left = f32::from(bounds.left());
                                line.bounds = Some(gpui::accesskit::Rect {
                                    x0: (left * scale) as f64,
                                    y0: (f32::from(bounds.top()) * scale) as f64,
                                    x1: (f32::from(bounds.right()) * scale) as f64,
                                    y1: (f32::from(bounds.bottom()) * scale) as f64,
                                });
                                let mut positions = Vec::with_capacity(line.text.len());
                                let mut widths = Vec::with_capacity(line.text.len());
                                let mut byte = 0;
                                for ch in line.text.chars() {
                                    let at = layout
                                        .position_for_index(byte)
                                        .map(|point| (f32::from(point.x) - left).max(0.0))
                                        .unwrap_or(0.0);
                                    let next = layout
                                        .position_for_index(byte + ch.len_utf8())
                                        .map(|point| (f32::from(point.x) - left).max(0.0))
                                        .unwrap_or(at);
                                    let width = if ch == '\n' { 0.0 } else { (next - at).max(0.0) };
                                    positions.push(at * scale);
                                    widths.push(width * scale);
                                    byte += ch.len_utf8();
                                }
                                line.positions = Some(positions);
                                line.widths = Some(widths);
                            }
                            a11y::describe(builder, lines, *anchor, *focus);
                        }
                    })
                    .size_full()
                    // The I-beam over the whole pane, including the space past the end of short lines.
                    .cursor_text()
                    // Monospace, as in the app's editor, so padded table columns line up.
                    .font_family(self.mono.clone())
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::click_source))
                    .child(source),
                cx,
            ))
    }

    fn preview_column(&mut self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let blocks = self.doc().block_list.clone();
        let focused_ring = chrome::ring(dark);
        if self.preview_link.is_some_and(|index| index >= self.preview_links().len()) {
            self.preview_link = None;
        }
        let selected = self.preview_link.and_then(|index| self.preview_links().get(index)).map(|(link, _)| link.clone());
        let preview_label = match &selected {
            Some(link) => SharedString::from(format!("Preview, link {link}")),
            None => SharedString::from("Preview"),
        };
        let described = self.preview_described(window);
        self.preview_keys(
            div()
                .id("preview")
                .key_context("Preview")
                .track_focus(&self.preview_focus)
                .role(gpui::Role::Document)
                .aria_label(preview_label)
                .a11y_synthetic_children(move |builder| {
                    if let Some(lines) = &described {
                        a11y::describe_document(builder, lines);
                    }
                })
                .border_1()
                .border_color(gpui::transparent_black())
                .focus_visible(move |style| style.border_color(focused_ring)),
            cx,
        )
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(tone(dark, 0x09090b, 0xffffff))
            .text_color(tone(dark, 0xfafafa, 0x18181b))
            .child(
                list(
                    blocks,
                    cx.processor(move |this, ix, window, cx| {
                        let folder = Path::new(&this.doc().path).parent().map(Path::to_path_buf);
                        let look = Look {
                            font_size: this.settings.font_size,
                            dark: this.dark,
                            mono: this.mono.clone(),
                            folder,
                            svg: cx.svg_renderer(),
                            selected_link: selected.clone(),
                        };
                        let item = match this.doc().blocks.get(ix) {
                            Some(block) => preview_view::preview_item(block, ix, &look, cx),
                            None => div().into_any_element(),
                        };
                        // A formula over this frame's layout budget is drawn in the next one.
                        if math::take_deferred() {
                            window.request_animation_frame();
                        }
                        item
                    }),
                )
                .size_full(),
            )
    }
}

impl EntityInputHandler for Prototype {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let buffer = &self.doc().buffer;
        let range = buffer.offset_of_utf16(range_utf16.start)..buffer.offset_of_utf16(range_utf16.end);
        *actual_range = Some(buffer.utf16_of(range.start)..buffer.utf16_of(range.end));
        Some(buffer.slice(range))
    }

    fn selected_text_range(&mut self, _ignore_disabled_input: bool, _window: &mut Window, _cx: &mut Context<Self>) -> Option<UTF16Selection> {
        let doc = self.doc();
        Some(UTF16Selection {
            range: doc.buffer.utf16_of(doc.selected.start)..doc.buffer.utf16_of(doc.selected.end),
            reversed: doc.selection_reversed,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        let doc = self.doc();
        doc.marked.as_ref().map(|range| doc.buffer.utf16_of(range.start)..doc.buffer.utf16_of(range.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.doc_mut().marked = None;
    }

    /// Typed text and the end of an IME composition. Typing at the caret groups into one undo step.
    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let range = self.edit_range(range_utf16);
        let kind = if range.is_empty() || self.doc().marked.as_ref() == Some(&range) { EditKind::Typing } else { EditKind::Other };
        self.edit(range, new_text, kind, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.edit_range(range_utf16);
        let replaced_at = range.start;
        self.edit(range, new_text, EditKind::Typing, cx);
        let doc = self.doc_mut();
        let end = replaced_at + new_text.len();
        doc.marked = (!new_text.is_empty()).then_some(replaced_at..end);
        if let Some(selected) = new_selected_utf16 {
            let start = replaced_at + utf16_to_byte(new_text, selected.start);
            doc.selected = start..replaced_at + utf16_to_byte(new_text, selected.end);
        }
    }

    /// The IME asks where to put its candidate window. The caret painted last frame is the composition point;
    /// its bounds already include scrolling, padding and the column.
    fn bounds_for_range(&mut self, _range_utf16: Range<usize>, element_bounds: Bounds<Pixels>, window: &mut Window, _cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        Some(self.caret_bounds.unwrap_or_else(|| Bounds::new(element_bounds.origin, size(px(1.5), window.line_height()))))
    }

    /// The character under a point, in UTF-16 units, for IME reconversion by mouse. Only lines painted
    /// in the last frame can answer. (GPUI's Windows backend does not yet ask: it handles no
    /// `WM_IME_REQUEST`, so Windows' reconversion key cannot reach any GPUI view, at 9a8ab54 either.)
    fn character_index_for_point(&mut self, position: Point<Pixels>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<usize> {
        let line = self.visible_line_at(position.y)?;
        let layout = self.painted_layout(line)?;
        let offset = self.offset_at(line, layout, position);
        Some(self.doc().buffer.utf16_of(offset))
    }
}

impl Prototype {
    fn edit_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        let doc = self.doc();
        if let Some(range) = range_utf16 {
            return doc.buffer.offset_of_utf16(range.start)..doc.buffer.offset_of_utf16(range.end);
        }
        doc.marked.clone().unwrap_or_else(|| doc.selected.clone())
    }
}

impl Focusable for Prototype {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Prototype {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frame_started = Some(Instant::now());
        math::begin_frame();
        crate::high_contrast::refresh_periodically();
        if !self.logged_first_frame {
            perf::mark("render");
        }
        let dark = self.settings.is_dark(window.appearance());
        if dark != self.dark {
            self.dark = dark;
            self.release_unused_images(cx);
        }
        self.caret_bounds = None;
        self.painted_lines.clear();
        self.prepaint_lines.borrow_mut().clear();
        self.update_title(window);
        self.refresh_matches(cx);
        self.sync_scroll();
        if !self.settings.word_wrap {
            let font = gpui::font(self.mono.clone());
            let font_id = window.text_system().resolve_font(&font);
            if let Ok(width) = window.text_system().em_advance(font_id, px(self.settings.font_size)) {
                self.char_width = width;
            }
            if self.follow_caret_sideways() {
                cx.notify();
            }
        }
        let dark = self.dark;
        let show_editor = self.settings.show_editor;
        let show_preview = self.settings.preview_visible();
        let show_outline = self.settings.show_outline;
        let font_size = self.settings.font_size;

        let panes = div()
            .flex()
            .flex_1()
            .min_h_0()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::close_menu))
            .when(show_outline, |row| row.child(self.outline_column(cx)))
            .when(show_editor, |row| row.child(self.editor_column(window, cx)))
            .when(show_preview, |row| row.child(self.preview_column(window, cx)))
            .when(self.recent_open, |row| row.child(self.recent_panel(cx)))
            .when(self.settings_open, |row| row.child(self.settings_panel(cx)));
        let find_bar = self.find_bar(cx);

        let root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(tone(dark, 0x18181b, 0xf4f4f5))
            .text_size(px(font_size))
            .text_color(tone(dark, 0xfafafa, 0x18181b))
            .key_context("Prototype")
            // Wherever the button comes up, a drag selection ends.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.dragging = false;
                    this.drag_outside = None;
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| this.drag_anywhere(event, cx)))
            // Dropping files on the window opens each one as a tab, as the app does.
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                for path in paths.paths() {
                    if path.is_file() {
                        this.open_path(path, cx);
                    }
                }
            }))
            .on_action(cx.listener(Self::find))
            .on_action(cx.listener(Self::find_replace))
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_previous))
            .on_action(cx.listener(Self::find_close))
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::save))
            .child(self.menu_bar(cx))
            .child(self.toolbar(cx))
            .child(self.tab_strip(cx))
            .children(find_bar)
            .child(panes)
            .child(self.status_bar(cx))
            // Last child, so it paints after the panes.
            .child(self.frame_timer());
        self.command_actions(root, cx)
    }
}

/// A heading for the outline: its 1-based source line, level and text.
struct Heading {
    line: usize,
    level: u8,
    text: SharedString,
}

fn headings_of(blocks: &[Block]) -> Vec<Heading> {
    blocks
        .iter()
        .filter_map(|block| match block.kind {
            Kind::Heading(level) => Some(Heading { line: block.line, level, text: SharedString::from(block.text.clone()) }),
            _ => None,
        })
        .collect()
}

/// Every link in `blocks`, images aside, in document order, with the index of its block.
fn links_of(blocks: &[Block]) -> Vec<(Arc<str>, usize)> {
    let mut links = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let cells = block.table.iter().flat_map(|table| table.rows.iter().flatten().flatten());
        for run in block.runs.iter().chain(cells) {
            let Some(link) = run.style.link.as_ref() else { continue };
            if !link.starts_with("image:") {
                links.push((link.clone(), index));
            }
        }
    }
    links
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn utf16_to_byte(text: &str, utf16_offset: usize) -> usize {
    let mut utf16 = 0;
    for (index, ch) in text.char_indices() {
        if utf16 >= utf16_offset {
            return index;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

/// Where the caret is drawn for `column` in a painted line. A wrap point is both the end of one
/// visual row and the start of the next; GPUI places it at the end, but a caret there belongs at the
/// start of the next row (CodeMirror's forward association), where typing will put the text.
/// `next` is the offset after the character at `column`, if there is one.
fn caret_origin(layout: &TextLayout, column: usize, next: Option<usize>) -> Option<Point<Pixels>> {
    let here = layout.position_for_index(column)?;
    let Some(after) = next.and_then(|next| layout.position_for_index(next)) else { return Some(here) };
    if after.y > here.y { Some(point(layout.bounds().left(), after.y)) } else { Some(here) }
}
