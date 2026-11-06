//! The window: tabs, the source editor, the preview, the outline, menus and the find bar.
//!
//! Text lives in a rope (`buffer.rs`) with undo and redo. Characters arrive through GPUI's platform
//! input handler, because that is how Windows text input and IME reach a view. Both panes are
//! virtualised GPUI lists, so only the lines and blocks near the viewport are laid out, and the
//! preview is re-parsed on a background thread once typing pauses, as in the app.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, FocusHandle, Focusable, HighlightStyle,
    IntoElement, ListAlignment, ListOffset, ListScrollEvent, ListState, MouseButton, MouseDownEvent, Pixels, Point,
    PromptLevel, SharedString, StyledText, Subscription, Task, TextLayout, UTF16Selection, Window, actions, canvas,
    deferred, div, fill, list, prelude::*, px, rgb, size, svg,
};

use crate::buffer::{self, Buffer, EditKind, LineSplice};
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
    ]
);

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
            parse_task: None,
            last_parse_ms: 0.,
            edited_at: None,
            lines,
            block_list: ListState::new(0, ListAlignment::Top, px(OVERDRAW)),
            followed_cursor_line: None,
            synced_source_line: None,
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
            focus: cx.focus_handle(),
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
        doc.parse_task = Some(cx.spawn(async move |this, cx| {
            if !pause.is_zero() {
                cx.background_executor().timer(pause).await;
            }
            let started = Instant::now();
            // Both panes from one snapshot: the preview's blocks and the editor's colours. Code in
            // fences is highlighted once; the second call finds it in the tree-sitter cache.
            let (blocks, spans) = cx
                .background_executor()
                .spawn(async move {
                    let text = rope.to_string();
                    (preview::blocks(&text), syntax::source_spans(&text))
                })
                .await;
            let parse_ms = started.elapsed().as_secs_f64() * 1000.0;
            let _ = this.update(cx, |this, cx| this.apply_parse(id, version, blocks, spans, parse_ms, cx));
        }));
    }

    /// Replaces only the blocks that changed, so the preview keeps its measured heights and scroll position.
    fn apply_parse(&mut self, id: u64, version: u64, blocks: Vec<Block>, spans: Vec<Vec<Span>>, parse_ms: f64, cx: &mut Context<Self>) {
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == id) else { return };
        if doc.buffer.version() != version {
            return;
        }
        doc.last_parse_ms = parse_ms;
        // From the last keystroke to the preview showing it: the pause, the parse and this update.
        if let Some(edited_at) = doc.edited_at.take() {
            perf::log(&format!("preview_ms={:.0} parse_ms={parse_ms:.1}", edited_at.elapsed().as_secs_f64() * 1000.0));
        }
        let (changed, count) = preview_view::changed_blocks(&doc.blocks, &blocks);
        doc.block_list.splice(changed, count);
        doc.blocks = blocks;
        doc.spans = spans;
        doc.synced_source_line = None;
        cx.notify();
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

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc_mut();
        doc.selected = 0..doc.buffer.len();
        doc.selection_reversed = false;
        doc.buffer.break_group();
        cx.notify();
    }

    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc();
        let start = doc.buffer.line_start(doc.buffer.line_of(doc.cursor()));
        self.move_to(start, cx);
    }

    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc();
        let end = doc.buffer.line_range(doc.buffer.line_of(doc.cursor())).end;
        self.move_to(end, cx);
    }

    /// A caret move also ends the current typing group, so the next keystroke is a new undo step.
    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let doc = self.doc_mut();
        let offset = doc.buffer.boundary(offset);
        doc.selected = offset..offset;
        doc.selection_reversed = false;
        doc.buffer.break_group();
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
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

    /// A click on a source line: GPUI's layout of that line maps the point to a character, wrapped or not.
    fn click_line(&mut self, line: usize, layout: &TextLayout, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.open_menu = None;
        let range = self.doc().buffer.line_range(line);
        let (Ok(index) | Err(index)) = layout.index_for_position(position);
        self.move_to(range.start + index.min(range.len()), cx);
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
    fn find_close(&mut self, _: &FindClose, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        if self.find.take().is_some() {
            window.focus(&self.focus, cx);
        }
        cx.notify();
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

    fn save_clicked(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.write_active(cx);
    }

    fn open_file(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
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

    fn toggle_menu(&mut self, menu: OpenMenu, cx: &mut Context<Self>) {
        self.open_menu = if self.open_menu == Some(menu) { None } else { Some(menu) };
        cx.notify();
    }

    fn close_menu(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.open_menu.is_none() {
            return;
        }
        self.open_menu = None;
        cx.notify();
    }

    /// Hiding the editor never changes `showPreview`; the preview shows anyway, as in the app.
    fn toggle_editor(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings.show_editor = !self.settings.show_editor;
        self.keep_settings(cx);
    }

    fn toggle_preview(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings.show_preview = !self.settings.show_preview;
        self.keep_settings(cx);
    }

    fn toggle_word_wrap(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings.word_wrap = !self.settings.word_wrap;
        self.keep_settings(cx);
    }

    fn toggle_theme(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings.theme = self.settings.theme.next();
        self.keep_settings(cx);
    }

    fn toggle_outline(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings.show_outline = !self.settings.show_outline;
        self.keep_settings(cx);
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
    fn menu_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .bg(tone(self.dark, 0x27272a, 0xf4f4f5))
            .text_size(px(13.))
            .child(self.menu_button("File", OpenMenu::File, cx))
            .child(self.menu_button("Edit", OpenMenu::Edit, cx))
            .child(self.menu_button("View", OpenMenu::View, cx))
    }

    fn menu_button(&mut self, label: &'static str, menu: OpenMenu, cx: &mut Context<Self>) -> gpui::Div {
        let opened = self.open_menu == Some(menu);
        let dark = self.dark;
        let button = div()
            .px_3()
            .py_1()
            .when(opened, |button| button.bg(tone(dark, 0x3f3f46, 0xe4e4e7)))
            .child(label.to_string())
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| this.toggle_menu(menu, cx)));
        if !opened {
            return button;
        }
        // The list is taken out of the bar's height and painted after the panes, so it covers the tab instead of pushing it down.
        button.child(deferred(self.menu_items(menu, cx).absolute().top_full().left_0().occlude()))
    }

    fn menu_items(&mut self, menu: OpenMenu, cx: &mut Context<Self>) -> gpui::Div {
        let dark = self.dark;
        let column = div()
            .flex()
            .flex_col()
            .py_1()
            .bg(tone(dark, 0x3f3f46, 0xffffff))
            .border_1()
            .border_color(tone(dark, 0x52525b, 0xd4d4d8));
        // Each item closes the menu and runs the same handler as its shortcut.
        let item = |label: &str, shortcut: &str| command(label, shortcut, dark);
        match menu {
            OpenMenu::File => column
                .child(item("Open", "").on_mouse_down(MouseButton::Left, cx.listener(Self::open_file)))
                .child(item("Save", "Ctrl+S").on_mouse_down(MouseButton::Left, cx.listener(Self::save_clicked)))
                .child(item("Quit", "").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.quit(&Quit, window, cx)))),
            OpenMenu::Edit => column
                .child(item("Undo", "Ctrl+Z").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    this.open_menu = None;
                    this.undo(&Undo, window, cx);
                })))
                .child(item("Redo", "Ctrl+Y").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    this.open_menu = None;
                    this.redo(&Redo, window, cx);
                })))
                .child(item("Cut", "Ctrl+X").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    this.open_menu = None;
                    this.cut(&Cut, window, cx);
                })))
                .child(item("Copy", "Ctrl+C").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    this.open_menu = None;
                    this.copy(&Copy, window, cx);
                })))
                .child(item("Paste", "Ctrl+V").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    this.open_menu = None;
                    this.paste(&Paste, window, cx);
                })))
                .child(item("Select all", "Ctrl+A").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    this.open_menu = None;
                    this.select_all(&SelectAll, window, cx);
                })))
                .child(item("Find", "Ctrl+F").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.open_find(false, window, cx))))
                .child(item("Replace", "Ctrl+H").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.open_find(true, window, cx)))),
            OpenMenu::View => {
                let editor = if self.settings.show_editor { "Hide editor" } else { "Show editor" };
                let preview = if self.settings.show_preview { "Hide preview" } else { "Show preview" };
                let wrap = if self.settings.word_wrap { "Word wrap on" } else { "Word wrap off" };
                let theme = self.settings.theme.label();
                let outline = if self.settings.show_outline { "Hide outline" } else { "Show outline" };
                column
                    .child(item(editor, "").on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_editor)))
                    .child(item(preview, "").on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_preview)))
                    .child(item(outline, "").on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_outline)))
                    .child(item(wrap, "").on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_word_wrap)))
                    .child(item(theme, "").on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_theme)))
                    .child(item("Larger text", "").on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.step_font(1, cx))))
                    .child(item("Smaller text", "").on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.step_font(-1, cx))))
            }
        }
    }

    fn toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let dirty = self.doc().buffer.is_dirty();
        let read_only = self.doc().read_only;
        let editor = self.settings.show_editor;
        let preview = self.settings.preview_visible();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .bg(tone(dark, 0x27272a, 0xe4e4e7))
            .text_size(px(13.))
            .child(tool("Open", "icons/open.svg", false, true, dark).on_mouse_down(MouseButton::Left, cx.listener(Self::open_file)))
            .child(
                tool(if read_only { "Read-only" } else { "Save" }, "icons/save.svg", dirty, dirty, dark)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::save_clicked)),
            )
            .child(
                tool("Editor", if editor { "icons/editor.svg" } else { "icons/editor-off.svg" }, editor, editor, dark)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_editor)),
            )
            .child(
                tool("Preview", if preview { "icons/preview.svg" } else { "icons/preview-off.svg" }, preview, preview, dark)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_preview)),
            )
            .child(
                tool("Outline", "icons/outline.svg", self.settings.show_outline, self.settings.show_outline, dark)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_outline)),
            )
    }

    fn tab_strip(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active;
        let dark = self.dark;
        let tabs: Vec<(String, bool)> = self.documents.iter().map(|doc| (file_name(&doc.path), doc.buffer.is_dirty())).collect();
        div().flex().flex_row().bg(tone(dark, 0x27272a, 0xe4e4e7)).children(tabs.into_iter().enumerate().map(|(index, (name, dirty))| {
            let name = if dirty { format!("{name} •") } else { name };
            let current = index == active;
            div()
                .flex()
                .flex_row()
                .items_center()
                .px_2()
                .py_1()
                .bg(if current { tone(dark, 0x3f3f46, 0xffffff) } else { tone(dark, 0x27272a, 0xe4e4e7) })
                .child(
                    div()
                        .child(name)
                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| this.activate_tab(index, cx))),
                )
                .child(
                    div()
                        .pl_2()
                        .child("×")
                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| this.close_tab(index, window, cx))),
                )
        }))
    }

    fn path_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let bar = div()
            .flex()
            .flex_row()
            .gap_3()
            .px_3()
            .py_2()
            .text_size(px(12.))
            .text_color(tone(dark, 0xa1a1aa, 0x52525b))
            .bg(tone(dark, 0x27272a, 0xe4e4e7))
            .child(self.doc().path.clone())
            .on_mouse_down(MouseButton::Left, cx.listener(Self::close_menu));
        match &self.status {
            Some(status) => bar.child(div().text_color(tone(dark, 0xfca5a5, 0xb91c1c)).child(status.clone())),
            None => bar,
        }
    }

    fn find_bar(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let dark = self.dark;
        let find = self.find.as_ref()?;
        for field in [&find.query, &find.replacement] {
            field.update(cx, |field, _| field.dark = dark);
        }
        let selected = self.doc().selected.clone();
        let count = match (find.matches.len(), find.matches.iter().position(|found| *found == selected)) {
            (0, _) if find.query.read(cx).text().is_empty() => String::new(),
            (0, _) => "No results".to_string(),
            (total, Some(current)) => format!("{} of {total}", current + 1),
            (total, None) => format!("{total} matches"),
        };
        let button = |label: &str| {
            div().px_2().py(px(3.)).bg(tone(dark, 0x3f3f46, 0xe4e4e7)).text_color(tone(dark, 0xfafafa, 0x18181b)).child(label.to_string())
        };
        Some(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .text_size(px(13.))
                .bg(tone(dark, 0x27272a, 0xe4e4e7))
                .text_color(tone(dark, 0xa1a1aa, 0x52525b))
                .child(find.query.clone())
                .child(div().min_w(px(80.)).child(count))
                .child(button("Previous").on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.find_step(-1, cx))))
                .child(button("Next").on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.find_step(1, cx))))
                .when(find.show_replace, |bar| {
                    bar.child(find.replacement.clone())
                        .child(button("Replace").on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.replace_current(cx))))
                        .child(button("Replace all").on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.replace_all(cx))))
                })
                .child(button("×").on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.find_close(&FindClose, window, cx)))),
        )
    }

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
        let row = div()
            .relative()
            .px_3()
            .when(ix == 0, |row| row.pt_2())
            .map(|row| if self.settings.word_wrap { row.whitespace_normal() } else { row.whitespace_nowrap() })
            .child(styled)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.click_line(ix, &click_layout, event.position, window, cx);
                    cx.stop_propagation();
                }),
            );
        if !selected.is_empty() || !(line.start..=line.end).contains(&cursor) {
            return row.into_any_element();
        }
        let column = cursor - line.start;
        let view = cx.entity();
        row.child(
            canvas(
                |_, _, _| {},
                move |_, _, window, cx| {
                    let Some(origin) = layout.position_for_index(column) else { return };
                    let caret = Bounds::new(origin, size(px(1.5), layout.line_height()));
                    window.paint_quad(fill(caret, rgb(CARET)));
                    view.update(cx, |view, _| view.caret_bounds = Some(caret));
                },
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
    }

    fn editor_column(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let lines = self.doc().lines.clone();
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(tone(dark, 0x18181b, 0xfafafa))
            .text_color(tone(dark, 0xfafafa, 0x18181b))
            .child(InputLayer { view: cx.entity(), focus: self.focus.clone() })
            .child(
                div()
                    .id("source")
                    .size_full()
                    // The I-beam over the whole pane, including the space past the end of short lines.
                    .cursor_text()
                    // Monospace, as in the app's editor, so padded table columns line up.
                    .font_family(self.mono.clone())
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::click_source))
                    .child(list(lines, cx.processor(|this, ix, _window, cx| this.render_line(ix, cx))).size_full()),
            )
    }

    fn preview_column(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let blocks = self.doc().block_list.clone();
        div()
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
                        let look = Look { font_size: this.settings.font_size, dark: this.dark, mono: this.mono.clone(), folder };
                        match this.doc().blocks.get(ix) {
                            Some(block) => preview_view::preview_item(block, ix, &look, cx),
                            None => div().into_any_element(),
                        }
                    }),
                )
                .size_full(),
            )
    }

    fn outline_column(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let headings: Vec<(usize, u8, String)> = self
            .doc()
            .blocks
            .iter()
            .filter_map(|block| match block.kind {
                Kind::Heading(level) => Some((block.line, level, block.text.clone())),
                _ => None,
            })
            .collect();
        let column = div()
            .id("outline")
            .w(px(220.))
            .h_full()
            .overflow_y_scroll()
            .bg(tone(self.dark, 0x18181b, 0xf4f4f5))
            .px_2()
            .py_2()
            .text_size(px(13.));
        if headings.is_empty() {
            return column.child("No headings");
        }
        column.children(headings.into_iter().map(|(line, level, text)| {
            div()
                .py_1()
                .pl(px(4. + (level.saturating_sub(1) as f32) * 12.))
                .child(text)
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| this.jump_to_line(line, cx)))
        }))
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

    /// Only used for IME reconversion by mouse. Not supported in the prototype; GPUI accepts `None`.
    fn character_index_for_point(&mut self, _point: Point<Pixels>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<usize> {
        None
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
        self.dark = self.settings.is_dark(window.appearance());
        self.caret_bounds = None;
        self.update_title(window);
        self.refresh_matches(cx);
        self.sync_scroll();
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
            .when(show_editor, |row| row.child(self.editor_column(cx)))
            .when(show_preview, |row| row.child(self.preview_column(cx)));
        let find_bar = self.find_bar(cx);

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(tone(dark, 0x18181b, 0xf4f4f5))
            .text_size(px(font_size))
            .text_color(tone(dark, 0xfafafa, 0x18181b))
            .key_context("Prototype")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
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
            // Last child, so it paints after the panes.
            .child(self.frame_timer())
    }
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn tool(label: &str, icon: &'static str, pressed: bool, bright: bool, dark: bool) -> gpui::Div {
    let color = if bright { tone(dark, 0xfafafa, 0x18181b) } else { tone(dark, 0xa1a1aa, 0x71717a) };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_2()
        .py_1()
        .bg(if pressed { tone(dark, 0x3f3f46, 0xffffff) } else { tone(dark, 0x27272a, 0xe4e4e7) })
        .text_color(color)
        .child(svg().path(icon).w(px(16.)).h(px(16.)).text_color(color))
        .child(label.to_string())
}

fn command(label: &str, shortcut: &str, dark: bool) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .gap_4()
        .px_3()
        .py_1()
        .w(px(200.))
        .text_color(tone(dark, 0xfafafa, 0x18181b))
        .child(label.to_string())
        .child(div().text_color(tone(dark, 0xa1a1aa, 0x71717a)).child(shortcut.to_string()))
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
