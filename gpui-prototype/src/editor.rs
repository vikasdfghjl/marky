//! The window: tabs, the source editor, the preview, the outline, menus and the find bar.
//!
//! Text lives in a rope (`buffer.rs`) with undo and redo. Characters arrive through GPUI's platform
//! input handler, because that is how Windows text input and IME reach a view. Both panes are
//! virtualised GPUI lists, so only the lines and blocks near the viewport are laid out, and the
//! preview is re-parsed on a background thread once typing pauses, as in the app.

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, FocusHandle, Focusable, HighlightStyle,
    IntoElement, ListAlignment, ListOffset, ListScrollEvent, ListState, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollHandle,
    PromptLevel, SharedString, StyledText, Subscription, Task, TextLayout, UTF16Selection, Window, actions, canvas,
    deferred, div, fill, list, point, prelude::*, px, rgb, size, svg,
};

use crate::buffer::{self, Buffer, EditKind, LineSplice};
use crate::{diagrams, math};
use crate::files::{self, FileMeta, OpenedFile};
use crate::perf;
use crate::preview::{self, Block, Kind};
use crate::preview_view::{self, Look, tone};
use crate::settings::{self, ViewSettings};
use crate::syntax::{self, Span};
use crate::text_field::{FieldEvent, InputLayer, TextField};

actions!(
    prototype,
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
    path: String,
    meta: FileMeta,
    /// Lossy decode. Saving would corrupt the file, so Save refuses it.
    read_only: bool,
    buffer: Buffer,
    selected: Range<usize>,
    /// The caret is at `selected.start` when the selection was made backwards.
    selection_reversed: bool,
    marked: Option<Range<usize>>,
    blocks: Vec<Block>,
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
}

/// The status bar's counts: words as the app counts them (`/\S+/g`), characters as JavaScript's `length`.
/// What a background parse hands back to the window.
struct Parsed {
    blocks: Vec<Block>,
    spans: Vec<Vec<Span>>,
    longest: usize,
    counts: Counts,
}

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
            read_only: opened.read_only,
            buffer,
            selected: 0..0,
            selection_reversed: false,
            marked: None,
            blocks: Vec::new(),
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
        }
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
    settings_focus: FocusHandle,
    settings_open: bool,
    /// Esc was pressed in the editor: the next Tab moves the focus instead of indenting, as with
    /// CodeMirror's `indentWithTab`, so a keyboard user can always leave the editor.
    tab_leaves: bool,
    /// Where the pointer is while a drag selection is outside the source pane, and the task that
    /// keeps scrolling towards it.
    drag_outside: Option<Point<Pixels>>,
    drag_scroll: Option<Task<()>>,
    /// Source lines painted in the last frame, with their layouts.
    painted_lines: Vec<(usize, TextLayout)>,
    _appearance: Subscription,
}

impl Prototype {
    pub fn new(opened: OpenedFile, settings: ViewSettings, config_dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // "System" follows Windows' light or dark mode while the window is open.
        let appearance = cx.observe_window_appearance(window, |_, _, cx| cx.notify());
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
            config_dir,
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
            settings_focus: cx.focus_handle(),
            settings_open: false,
            tab_leaves: false,
            drag_outside: None,
            drag_scroll: None,
            painted_lines: Vec::new(),
            _appearance: appearance,
        };
        // The source paints first; the preview follows when the background parse is done, like the app.
        this.schedule_parse(0, Duration::ZERO, cx);
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
        let (caret, splice) = doc.buffer.edit(range, text, kind, before);
        doc.splice_lines(splice);
        doc.selected = caret..caret;
        doc.selection_reversed = false;
        doc.marked = None;
        self.after_change(cx);
    }

    fn after_change(&mut self, cx: &mut Context<Self>) {
        self.typed_at = Some(Instant::now());
        self.doc_mut().edited_at = self.typed_at;
        let (id, pause) = (self.doc().id, parse_pause(self.doc().last_parse_ms));
        self.schedule_parse(id, pause, cx);
        cx.notify();
    }

    /// Parses a snapshot of the text on a background thread after `pause`. Each edit replaces the
    /// task, which cancels the previous one, so only the text after the last keystroke is parsed.
    fn schedule_parse(&mut self, id: u64, pause: Duration, cx: &mut Context<Self>) {
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == id) else { return };
        let rope = doc.buffer.snapshot();
        let version = doc.buffer.version();
        let flavor = preview::Flavor { typographer: self.settings.typographer };
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
                    Parsed { blocks: preview::blocks_with(&text, flavor), spans: syntax::source_spans(&text), longest, counts: Counts::of(&text) }
                })
                .await;
            let parse_ms = started.elapsed().as_secs_f64() * 1000.0;
            let _ = this.update(cx, |this, cx| this.apply_parse(id, version, parsed, parse_ms, cx));
        }));
    }

    /// Replaces only the blocks that changed, so the preview keeps its measured heights and scroll position.
    fn apply_parse(&mut self, id: u64, version: u64, parsed: Parsed, parse_ms: f64, cx: &mut Context<Self>) {
        let Parsed { blocks, spans, longest, counts } = parsed;
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
        let (changed, count) = preview_view::changed_blocks(&doc.blocks, &blocks);
        doc.block_list.splice(changed, count);
        doc.blocks = blocks;
        doc.spans = spans;
        doc.longest_line = longest;
        doc.synced_source_line = None;
        self.release_unused_images(cx);
        cx.notify();
    }

    /// Frees the bitmaps of formulas and diagrams that no open document shows any more at the
    /// current font size and theme: every edit of one makes a new bitmap, and GPUI keeps each until
    /// it is dropped.
    fn release_unused_images(&self, cx: &mut App) {
        let mut diagrams_in_use = HashSet::new();
        let mut formulas_in_use = HashSet::new();
        for block in self.documents.iter().flat_map(|doc| &doc.blocks) {
            match block.kind {
                Kind::Diagram => {
                    diagrams_in_use.insert(block.text.as_str());
                }
                Kind::Math => {
                    formulas_in_use.insert((block.text.as_str(), true));
                }
                _ => {}
            }
            let cells = block.table.iter().flat_map(|table| table.rows.iter().flatten());
            for run in block.runs.iter().chain(cells.flatten()).filter(|run| run.style.math) {
                formulas_in_use.insert((run.text.as_str(), run.style.display));
            }
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

    /// Esc closes the find bar, or an open menu.
    /// Esc closes what is open, innermost first: a menu, the settings panel, the find bar. With
    /// nothing open, in the editor, it lets the next Tab leave the editor instead of indenting.
    fn find_close(&mut self, _: &FindClose, window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
        if self.open_menu.is_some() {
            self.close_menus(window, cx);
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
        let query = find.query.read(cx).text().to_string();
        let key = (doc.id, doc.buffer.version(), query);
        if find.searched.as_ref() == Some(&key) {
            return;
        }
        find.matches = buffer::find_all(&doc.buffer.text(), &key.2);
        find.searched = Some(key);
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
    fn write_active(&mut self, cx: &mut Context<Self>) {
        self.open_menu = None;
        cx.notify();
        if self.doc().read_only {
            let message = format!("Not saved: {} is not valid UTF-8", self.doc().path);
            self.report(message);
            return;
        }
        if !self.doc().buffer.is_dirty() {
            return;
        }
        let path = self.doc().path.clone();
        let text = self.doc().buffer.text();
        match files::write(Path::new(&path), &text, self.doc().meta) {
            Ok(_) => {
                self.doc_mut().buffer.mark_saved();
                self.status = None;
            }
            Err(err) => self.report(format!("Could not save {path}: {err}")),
        }
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
                self.activate_tab(self.documents.len() - 1, cx);
            }
            Err(err) => self.report(format!("Could not open {}: {err}", path.display())),
        }
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
        let path = Path::new(&self.doc().path).parent().map(|folder| folder.join(&target));
        let opens = path.as_ref().filter(|path| {
            let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
            path.is_file() && files::MARKDOWN_EXTS.iter().any(|allowed| allowed.eq_ignore_ascii_case(extension))
        });
        match opens {
            Some(path) => {
                let path = path.clone();
                self.open_path(&path, cx);
            }
            None => {
                self.report(format!("Not opened: {link} (links open only .md, .markdown and .txt files)"));
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
        self.doc_mut().synced_source_line = None;
        cx.notify();
    }

    fn dirty_count(&self) -> usize {
        self.documents.iter().filter(|doc| doc.buffer.is_dirty()).count()
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.documents.len() == 1 || index >= self.documents.len() {
            return;
        }
        if !self.documents[index].buffer.is_dirty() {
            self.remove_tab(index, cx);
            return;
        }
        // Tabs can move while the prompt is open, so the answer finds the tab again by id.
        let id = self.documents[index].id;
        let message = format!("Discard unsaved changes in {}?", file_name(&self.documents[index].path));
        self.confirm_discard(&message, window, cx, move |this, _, cx| {
            if let Some(index) = this.documents.iter().position(|doc| doc.id == id) {
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
        let dirty = self.dirty_count();
        if dirty == 0 || self.close_confirmed {
            return true;
        }
        let message = if dirty == 1 { "Discard unsaved changes in 1 file?".to_string() } else { format!("Discard unsaved changes in {dirty} files?") };
        self.confirm_discard(&message, window, cx, |this, window, _| {
            this.close_confirmed = true;
            window.remove_window();
        });
        false
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
            },
        )
        .absolute()
        .size_0()
    }

    fn update_title(&mut self, window: &mut Window) {
        let doc = self.doc();
        let title = format!("{}{} - Marky GPUI", file_name(&doc.path), if doc.buffer.is_dirty() { " •" } else { "" });
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

        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        if !text.is_empty() {
            // Colours from the last parse. Right after an edit they can be a few characters off, or
            // past the end of the line, until the next parse; clamp them to character boundaries.
            for (range, token) in doc.spans.get(ix).map(Vec::as_slice).unwrap_or(&[]) {
                let span = floor_boundary(&text, range.start)..floor_boundary(&text, range.end);
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
        }
        let styled = StyledText::new(if text.is_empty() { " ".to_string() } else { text }).with_highlights(syntax::merge(&highlights));
        let layout = styled.layout().clone();
        let click_layout = layout.clone();
        let drag_layout = layout.clone();
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
        let line_text = doc.buffer.line(ix);
        let caret_next = caret_column.and_then(|column| line_text.get(column..)?.chars().next().map(|ch| column + ch.len_utf8()));
        let view = cx.entity();
        // Paints the caret, and records the line's layout once it has been painted: Home and End on
        // wrapped rows and the IME's point-to-character lookup may only use painted layouts.
        row.child(
            canvas(
                |_, _, _| {},
                move |_, _, window, cx| {
                    let caret = caret_column.and_then(|column| caret_origin(&layout, column, caret_next)).map(|origin| Bounds::new(origin, size(px(1.5), layout.line_height())));
                    if let Some(caret) = caret {
                        window.paint_quad(fill(caret, rgb(CARET)));
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
        let mut end = row.end;
        while end > row.start && !text.is_char_boundary(end - 1) {
            end -= 1;
        }
        let target = range.start + floor_boundary(&text, end.saturating_sub(1)).max(row.start);
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
                a11y::Line { index, text }
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
        let described = self.describe_text(window);
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
                        if let Some((lines, anchor, focus)) = described {
                            a11y::describe(builder, lines, anchor, focus);
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

    fn preview_column(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let blocks = self.doc().block_list.clone();
        let focused_ring = chrome::ring(dark);
        self.preview_keys(
            div()
                .id("preview")
                .key_context("Preview")
                .track_focus(&self.preview_focus)
                .role(gpui::Role::Document)
                .aria_label("Preview")
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
                    cx.processor(|this, ix, _window, cx| {
                        let folder = Path::new(&this.doc().path).parent().map(Path::to_path_buf);
                        let look = Look { font_size: this.settings.font_size, dark: this.dark, mono: this.mono.clone(), folder, svg: cx.svg_renderer() };
                        match this.doc().blocks.get(ix) {
                            Some(block) => preview_view::preview_item(block, ix, &look, cx),
                            None => div().into_any_element(),
                        }
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
            .when(show_preview, |row| row.child(self.preview_column(cx)))
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
            .child(self.path_bar(cx))
            .children(find_bar)
            .child(panes)
            .child(self.status_bar())
            // Last child, so it paints after the panes.
            .child(self.frame_timer());
        self.command_actions(root, cx)
    }
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

/// The nearest char boundary at or before `offset`, within `text`.
fn floor_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
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
