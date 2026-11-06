//! One buffer. Characters arrive through GPUI's platform input handler, because that
//! is how Windows text input reaches a view. Each source line is a `StyledText`, so clicks and
//! the caret use GPUI's own text layout (the pattern in gpui's `examples/input.rs`) and stay
//! right at any font size and with word wrap.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui::{
    App, Bounds, ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, FontWeight, GlobalElementId, HighlightStyle, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, Pixels, Point, Position, PromptLevel, ScrollHandle, Style, StyledText, Subscription,
    TextLayout, UTF16Selection, Window, actions, canvas, deferred, div, fill, point, prelude::*, px, relative, rgb,
    size, svg,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::files::{self, FileMeta, OpenedFile};
use crate::perf;
use crate::preview::{self, Block, Kind};
use crate::settings::{self, ViewSettings};

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
    ]
);

const CARET: u32 = 0x2563eb;

#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenMenu {
    File,
    Edit,
    View,
}

struct Document {
    path: String,
    content: String,
    meta: FileMeta,
    /// Lossy decode. Saving would corrupt the file, so Save refuses it.
    read_only: bool,
    dirty: bool,
    selected: Range<usize>,
    /// The caret is at `selected.start` when the selection was made backwards.
    selection_reversed: bool,
    marked: Option<Range<usize>>,
    blocks: Vec<Block>,
    followed_cursor_line: Option<usize>,
    synced_source_line: Option<usize>,
}

impl Document {
    fn open(opened: OpenedFile) -> Self {
        let blocks = preview::blocks(&opened.text);
        Self {
            path: opened.path,
            content: opened.text,
            meta: opened.meta,
            read_only: opened.read_only,
            dirty: false,
            selected: 0..0,
            selection_reversed: false,
            marked: None,
            blocks,
            followed_cursor_line: None,
            synced_source_line: None,
        }
    }
}

pub struct Prototype {
    focus: FocusHandle,
    documents: Vec<Document>,
    active: usize,
    /// Process start, taken at the top of `main`.
    started: Instant,
    logged_first_frame: bool,
    /// When the last edit arrived; logged as `type_ms` once that frame has painted.
    typed_at: Option<Instant>,
    settings: ViewSettings,
    dark: bool,
    config_dir: PathBuf,
    open_menu: Option<OpenMenu>,
    source_scroll: ScrollHandle,
    preview_scroll: ScrollHandle,
    /// Where the caret was painted in the last frame. The IME places its candidate window here.
    caret_bounds: Option<Bounds<Pixels>>,
    /// The last error, shown in the path bar: a release build has no console.
    status: Option<String>,
    window_title: String,
    /// Set once the user chose Discard, so the second close request goes through.
    close_confirmed: bool,
    _appearance: Subscription,
}

impl Prototype {
    pub fn new(
        opened: OpenedFile,
        settings: ViewSettings,
        config_dir: PathBuf,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // "System" follows Windows' light or dark mode while the window is open.
        let appearance = cx.observe_window_appearance(window, |_, _, cx| cx.notify());
        Self {
            focus: cx.focus_handle(),
            documents: vec![Document::open(opened)],
            active: 0,
            started,
            logged_first_frame: false,
            typed_at: None,
            dark: settings.is_dark(window.appearance()),
            settings,
            config_dir,
            open_menu: None,
            source_scroll: ScrollHandle::new(),
            preview_scroll: ScrollHandle::new(),
            caret_bounds: None,
            status: None,
            window_title: String::new(),
            close_confirmed: false,
            _appearance: appearance,
        }
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

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc().selected.is_empty() {
            self.select_to(previous_grapheme(&self.doc().content, self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc().selected.is_empty() {
            self.select_to(next_grapheme(&self.doc().content, self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if !self.doc().selected.is_empty() {
            self.move_to(self.doc().selected.start, cx);
            return;
        }
        self.move_to(previous_grapheme(&self.doc().content, self.cursor_offset()), cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if !self.doc().selected.is_empty() {
            self.move_to(self.doc().selected.end, cx);
            return;
        }
        self.move_to(next_grapheme(&self.doc().content, self.cursor_offset()), cx);
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(vertical_target(&self.doc().content, self.cursor_offset(), -1), cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(vertical_target(&self.doc().content, self.cursor_offset(), 1), cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(previous_grapheme(&self.doc().content, self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_grapheme(&self.doc().content, self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.doc().content.len();
        let doc = self.doc_mut();
        doc.selected = 0..end;
        doc.selection_reversed = false;
        cx.notify();
    }

    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        let (start, _) = line_range(&self.doc().content, self.cursor_offset());
        self.move_to(start, cx);
    }

    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let (_, end) = line_range(&self.doc().content, self.cursor_offset());
        self.move_to(end, cx);
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        // The buffer is LF only, like the app's editor; files::write restores the file's line ending.
        self.replace_text_in_range(None, &text.replace("\r\n", "\n"), window, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc().selected.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.doc().content[self.doc().selected.clone()].to_string()));
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc().selected.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.doc().content[self.doc().selected.clone()].to_string()));
        self.replace_text_in_range(None, "", window, cx);
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = char_boundary(&self.doc().content, offset);
        let doc = self.doc_mut();
        doc.selected = offset..offset;
        doc.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = char_boundary(&self.doc().content, offset);
        let doc = self.doc_mut();
        if doc.selection_reversed {
            doc.selected.start = offset;
        } else {
            doc.selected.end = offset;
        }
        if doc.selected.end < doc.selected.start {
            doc.selection_reversed = !doc.selection_reversed;
            doc.selected = doc.selected.end..doc.selected.start;
        }
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.doc().selection_reversed { self.doc().selected.start } else { self.doc().selected.end }
    }

    /// Returns the caret after the edit. Deleting nothing (Backspace at the start) leaves the tab clean.
    fn replace(&mut self, range: Range<usize>, new_text: &str) -> usize {
        if range.is_empty() && new_text.is_empty() {
            return range.start;
        }
        self.typed_at = Some(Instant::now());
        let doc = self.doc_mut();
        doc.content.replace_range(range.clone(), new_text);
        doc.blocks = preview::blocks(&doc.content);
        doc.dirty = true;
        range.start + new_text.len()
    }

    /// Restores the file's BOM and line ending. A clean buffer and a lossy decode are left on disk as they are.
    fn write_active(&mut self, cx: &mut Context<Self>) {
        self.open_menu = None;
        cx.notify();
        if self.doc().read_only {
            let message = format!("Not saved: {} is not valid UTF-8", self.doc().path);
            self.report(message);
            return;
        }
        if !self.doc().dirty {
            return;
        }
        let path = self.doc().path.clone();
        match files::write(Path::new(&path), &self.doc().content, self.doc().meta) {
            Ok(_) => {
                self.doc_mut().dirty = false;
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

    /// A click on a source line: GPUI's layout of that line maps the point to a character, wrapped or not.
    fn click_line(&mut self, line: Range<usize>, layout: &TextLayout, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.open_menu = None;
        let (Ok(index) | Err(index)) = layout.index_for_position(position);
        self.move_to(line.start + index.min(line.len()), cx);
    }

    /// A click below the last line puts the caret at the end.
    fn click_source(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.open_menu = None;
        self.move_to(self.doc().content.len(), cx);
    }

    fn dirty_count(&self) -> usize {
        self.documents.iter().filter(|doc| doc.dirty).count()
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

    fn menu_quit(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.quit(&Quit, window, cx);
    }

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

    fn menu_select_all(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.select_all(&SelectAll, window, cx);
    }

    fn menu_copy(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.copy(&Copy, window, cx);
    }

    fn menu_cut(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.cut(&Cut, window, cx);
    }

    fn menu_paste(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        self.paste(&Paste, window, cx);
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

    fn larger_text(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.step_font(1, cx);
    }

    fn smaller_text(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.step_font(-1, cx);
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

    fn toggle_outline(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.settings.show_outline = !self.settings.show_outline;
        self.keep_settings(cx);
    }

    fn keep_settings(&mut self, cx: &mut Context<Self>) {
        if let Err(message) = settings::save(&self.config_dir, &self.settings) {
            self.report(message);
        }
        self.open_menu = None;
        cx.notify();
    }

    /// The source pane leads. The preview shows the block for the top visible source line.
    fn sync_scroll(&mut self) {
        if !self.settings.show_editor || !self.settings.preview_visible() {
            return;
        }
        let cursor_line = line_index(&self.doc().content, self.cursor_offset());
        if self.doc().followed_cursor_line != Some(cursor_line) {
            self.doc_mut().followed_cursor_line = Some(cursor_line);
            self.source_scroll.scroll_to_item(cursor_line);
        }
        let top = self.source_scroll.top_item();
        if self.doc().synced_source_line == Some(top) || self.doc().blocks.is_empty() {
            return;
        }
        self.doc_mut().synced_source_line = Some(top);
        self.preview_scroll.scroll_to_top_of_item(preview::block_at_line(&self.doc().blocks, top + 1));
    }

    fn open_file(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        cx.notify();
        let Some(path) = rfd::FileDialog::new().add_filter("Markdown", files::MARKDOWN_EXTS).pick_file() else {
            return;
        };
        if let Some(index) = self.documents.iter().position(|doc| files::same_path(Path::new(&doc.path), &path)) {
            self.activate_tab(index, cx);
            return;
        }
        match files::read(&path) {
            Ok(opened) => {
                self.documents.push(Document::open(opened));
                self.activate_tab(self.documents.len() - 1, cx);
            }
            Err(err) => self.report(format!("Could not open {}: {err}", path.display())),
        }
    }

    fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.documents.len() {
            return;
        }
        self.active = index;
        self.reset_view();
        cx.notify();
    }

    fn reset_view(&mut self) {
        self.doc_mut().followed_cursor_line = None;
        self.doc_mut().synced_source_line = None;
        self.source_scroll.set_offset(point(px(0.), px(0.)));
        self.preview_scroll.set_offset(point(px(0.), px(0.)));
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.documents.len() == 1 || index >= self.documents.len() {
            return;
        }
        if !self.documents[index].dirty {
            self.remove_tab(index, cx);
            return;
        }
        // Tabs can move while the prompt is open, so the answer finds the tab again by path.
        let path = self.documents[index].path.clone();
        let message = format!("Discard unsaved changes in {}?", file_name(&path));
        self.confirm_discard(&message, window, cx, move |this, _, cx| {
            if let Some(index) = this.documents.iter().position(|doc| doc.path == path) {
                this.remove_tab(index, cx);
            }
        });
    }

    fn remove_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.documents.len() == 1 || index >= self.documents.len() {
            return;
        }
        let shown = self.active == index;
        self.documents.remove(index);
        if self.active > index || self.active == self.documents.len() {
            self.active -= 1;
        }
        if shown {
            self.reset_view();
        }
        cx.notify();
    }

    fn jump_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let offset = offset_of_line(&self.doc().content, line.saturating_sub(1));
        self.move_to(offset, cx);
    }

    /// Startup and typing are timed to the end of the frame's paint, like the app's
    /// measurements (a rAF callback that reports after paint).
    fn frame_timer(&mut self) -> impl IntoElement {
        let first_frame = (!self.logged_first_frame).then_some(self.started);
        self.logged_first_frame = true;
        let typed_at = self.typed_at.take();
        canvas(
            |_, _, _| {},
            move |_, _, _, _| {
                if let Some(started) = first_frame {
                    perf::log(&format!("startup_ms={:.0}", started.elapsed().as_secs_f64() * 1000.0));
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
        let title = format!("{}{} - Marky GPUI", file_name(&doc.path), if doc.dirty { " •" } else { "" });
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
    }
}

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
        match menu {
            OpenMenu::File => column
                .child(command("Open", dark).on_mouse_down(MouseButton::Left, cx.listener(Self::open_file)))
                .child(command("Save", dark).on_mouse_down(MouseButton::Left, cx.listener(Self::save_clicked)))
                .child(command("Quit", dark).on_mouse_down(MouseButton::Left, cx.listener(Self::menu_quit))),
            OpenMenu::Edit => column
                .child(command("Select all", dark).on_mouse_down(MouseButton::Left, cx.listener(Self::menu_select_all)))
                .child(command("Copy", dark).on_mouse_down(MouseButton::Left, cx.listener(Self::menu_copy)))
                .child(command("Cut", dark).on_mouse_down(MouseButton::Left, cx.listener(Self::menu_cut)))
                .child(command("Paste", dark).on_mouse_down(MouseButton::Left, cx.listener(Self::menu_paste))),
            OpenMenu::View => {
                let editor = if self.settings.show_editor { "Hide editor" } else { "Show editor" };
                let preview = if self.settings.show_preview { "Hide preview" } else { "Show preview" };
                let wrap = if self.settings.word_wrap { "Word wrap on" } else { "Word wrap off" };
                let theme = self.settings.theme.label();
                let outline = if self.settings.show_outline { "Hide outline" } else { "Show outline" };
                column
                    .child(command(editor, dark).on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_editor)))
                    .child(command(preview, dark).on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_preview)))
                    .child(command(outline, dark).on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_outline)))
                    .child(command(wrap, dark).on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_word_wrap)))
                    .child(command(theme, dark).on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_theme)))
                    .child(command("Larger text", dark).on_mouse_down(MouseButton::Left, cx.listener(Self::larger_text)))
                    .child(command("Smaller text", dark).on_mouse_down(MouseButton::Left, cx.listener(Self::smaller_text)))
            }
        }
    }

    fn toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let dirty = self.doc().dirty;
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
        let tabs: Vec<(String, bool)> = self.documents.iter().map(|doc| (file_name(&doc.path), doc.dirty)).collect();
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

    fn editor_column(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let view = cx.entity();
        let doc = self.doc();
        let cursor = if doc.selection_reversed { doc.selected.start } else { doc.selected.end };
        let selected = doc.selected.clone();
        let selection_color = tone(dark, 0x1e3a8a, 0xbfdbfe);
        let rows: Vec<gpui::Div> = line_spans(&doc.content)
            .into_iter()
            .map(|line| {
                let text = &doc.content[line.clone()];
                let mut styled = StyledText::new(if text.is_empty() { " ".to_string() } else { text.to_string() });
                let overlap = selected.start.max(line.start)..selected.end.min(line.end);
                if !overlap.is_empty() {
                    let highlight = HighlightStyle { background_color: Some(selection_color.into()), ..HighlightStyle::default() };
                    styled = styled.with_highlights([(overlap.start - line.start..overlap.end - line.start, highlight)]);
                }
                let layout = styled.layout().clone();
                let click_layout = layout.clone();
                let click_line = line.clone();
                let row = div()
                    .relative()
                    .map(|row| if self.settings.word_wrap { row.whitespace_normal() } else { row.whitespace_nowrap() })
                    .child(styled)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.click_line(click_line.clone(), &click_layout, event.position, window, cx);
                            cx.stop_propagation();
                        }),
                    );
                if !selected.is_empty() || !(line.start..=line.end).contains(&cursor) {
                    return row;
                }
                let column = cursor - line.start;
                let view = view.clone();
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
            })
            .collect();

        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(tone(dark, 0x18181b, 0xfafafa))
            .text_color(tone(dark, 0xfafafa, 0x18181b))
            .child(TextInputLayer { view })
            .child(
                div()
                    .id("source")
                    .flex_1()
                    .overflow_scroll()
                    .track_scroll(&self.source_scroll)
                    .px_3()
                    .py_2()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::click_source))
                    .children(rows),
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
        let content = &self.doc().content;
        let range = utf16_to_utf8(content, range_utf16.start)..utf16_to_utf8(content, range_utf16.end);
        *actual_range = Some(utf8_to_utf16(content, range.start)..utf8_to_utf16(content, range.end));
        Some(content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let doc = self.doc();
        Some(UTF16Selection {
            range: utf8_to_utf16(&doc.content, doc.selected.start)..utf8_to_utf16(&doc.content, doc.selected.end),
            reversed: doc.selection_reversed,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        let doc = self.doc();
        doc.marked.as_ref().map(|range| utf8_to_utf16(&doc.content, range.start)..utf8_to_utf16(&doc.content, range.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.doc_mut().marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.edit_range(range_utf16);
        let cursor = self.replace(range, new_text);
        let doc = self.doc_mut();
        doc.selected = cursor..cursor;
        doc.selection_reversed = false;
        doc.marked = None;
        cx.notify();
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
        let cursor = self.replace(range, new_text);
        let doc = self.doc_mut();
        doc.marked = (!new_text.is_empty()).then_some(replaced_at..cursor);
        doc.selected = match new_selected_utf16 {
            Some(selected) => replaced_at + utf16_to_utf8(new_text, selected.start)..replaced_at + utf16_to_utf8(new_text, selected.end),
            None => cursor..cursor,
        };
        doc.selection_reversed = false;
        cx.notify();
    }

    /// The IME asks where to put its candidate window. The caret painted last frame is the composition point;
    /// its bounds already include scrolling, padding and the column.
    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(self.caret_bounds.unwrap_or_else(|| Bounds::new(element_bounds.origin, size(px(1.5), window.line_height()))))
    }

    /// Only used for IME reconversion by mouse. Not supported in the prototype; GPUI accepts `None`.
    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Prototype {
    fn edit_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        let doc = self.doc();
        if let Some(range) = range_utf16 {
            return utf16_to_utf8(&doc.content, range.start)..utf16_to_utf8(&doc.content, range.end);
        }
        if let Some(range) = &doc.marked {
            return range.clone();
        }
        doc.selected.clone()
    }
}

impl Focusable for Prototype {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Prototype {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.dark = self.settings.is_dark(window.appearance());
        self.caret_bounds = None;
        self.update_title(window);
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
            .when(show_preview, |row| row.child(preview_column(&self.doc().blocks, &self.preview_scroll, font_size, dark)));

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
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::save))
            .child(self.menu_bar(cx))
            .child(self.toolbar(cx))
            .child(self.tab_strip(cx))
            .child(self.path_bar(cx))
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

fn command(label: &str, dark: bool) -> gpui::Div {
    div().px_3().py_1().w(px(180.)).text_color(tone(dark, 0xfafafa, 0x18181b)).child(label.to_string())
}

fn preview_column(blocks: &[Block], scroll: &ScrollHandle, font_size: f32, dark: bool) -> impl IntoElement {
    div()
        .id("preview")
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .overflow_y_scroll()
        .track_scroll(scroll)
        .bg(tone(dark, 0x09090b, 0xffffff))
        .text_color(tone(dark, 0xfafafa, 0x18181b))
        .px_4()
        .py_3()
        .children(blocks.iter().map(|block| preview_block(block, font_size, dark)))
}

fn preview_block(block: &Block, font_size: f32, dark: bool) -> gpui::Div {
    let size = match block.kind {
        Kind::Heading(1) => px(font_size + 8.),
        Kind::Heading(2) => px(font_size + 4.),
        Kind::Heading(_) => px(font_size + 2.),
        _ => px(font_size),
    };
    let body = styled_runs(block, size, dark);
    match block.kind {
        Kind::Quote => div().mb_3().pl_3().border_l_2().border_color(tone(dark, 0x71717a, 0xa1a1aa)).child(body),
        Kind::Item { ordered } => {
            let marker = match ordered {
                Some(number) => format!("{number}."),
                None => "•".to_string(),
            };
            div().mb_1().flex().flex_row().gap_2().child(marker).child(body)
        }
        Kind::Code => div().mb_3().px_3().py_2().bg(tone(dark, 0x27272a, 0xf4f4f5)).child(body),
        Kind::Math => div().mb_3().px_3().py_2().bg(tone(dark, 0x1e1b4b, 0xeef2ff)).text_color(tone(dark, 0xe0e7ff, 0x312e81)).child(body),
        Kind::Diagram => div()
            .mb_3()
            .px_3()
            .py_2()
            .border_1()
            .border_color(tone(dark, 0x52525b, 0xa1a1aa))
            .child(div().text_size(px(12.)).text_color(tone(dark, 0xa1a1aa, 0x52525b)).child("Mermaid source"))
            .child(body),
        Kind::Heading(_) | Kind::Paragraph => div().mb_3().child(body),
    }
}

fn styled_runs(block: &Block, size: Pixels, dark: bool) -> gpui::Div {
    let runs = block.runs.iter().map(|run| {
        let mut text = div().whitespace_normal().text_size(size).child(run.text.clone());
        if run.style.bold {
            text = text.font_weight(FontWeight::BOLD);
        }
        if run.style.italic {
            text = text.italic();
        }
        if run.style.strike {
            text = text.line_through();
        }
        if run.style.code {
            text = text.bg(tone(dark, 0x3f3f46, 0xe4e4e7)).px_1();
        }
        text
    });
    if block.runs.len() <= 1 {
        return div().w_full().children(runs);
    }
    div().w_full().flex().flex_row().flex_wrap().children(runs)
}

fn tone(dark: bool, dark_value: u32, light_value: u32) -> gpui::Rgba {
    if dark { rgb(dark_value) } else { rgb(light_value) }
}

/// GPUI delivers typed text only while an element is painting. This layer fills the
/// source column without taking a row, so the caret keeps receiving keystrokes.
struct TextInputLayer {
    view: Entity<Prototype>,
}

impl IntoElement for TextInputLayer {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextInputLayer {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            position: Position::Absolute,
            size: size(relative(1.).into(), relative(1.).into()),
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.view.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
    }
}

fn line_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            spans.push(start..index);
            start = index + 1;
        }
    }
    spans.push(start..text.len());
    spans
}

fn line_index(text: &str, offset: usize) -> usize {
    text[..char_boundary(text, offset)].bytes().filter(|byte| *byte == b'\n').count()
}

fn line_range(text: &str, offset: usize) -> (usize, usize) {
    let offset = char_boundary(text, offset);
    let start = text[..offset].rfind('\n').map(|index| index + 1).unwrap_or(0);
    let end = text[offset..].find('\n').map(|index| offset + index).unwrap_or(text.len());
    (start, end)
}

fn offset_of_line(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut seen = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte != b'\n' {
            continue;
        }
        seen += 1;
        if seen == line {
            return index + 1;
        }
    }
    text.len()
}

/// The nearest char boundary at or before `offset`. Every offset that slices the text passes through here.
fn char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Up or Down: the same column, counted in graphemes, on the line above or below.
/// Counting bytes could land inside a character on a line with non-ASCII text.
fn vertical_target(text: &str, offset: usize, delta: isize) -> usize {
    let (start, end) = line_range(text, offset);
    if delta < 0 && start == 0 {
        return 0;
    }
    if delta > 0 && end == text.len() {
        return text.len();
    }
    let column = text[start..char_boundary(text, offset)].graphemes(true).count();
    let probe = if delta < 0 { start - 1 } else { end + 1 };
    let (target_start, target_end) = line_range(text, probe);
    text[target_start..target_end]
        .grapheme_indices(true)
        .nth(column)
        .map(|(index, _)| target_start + index)
        .unwrap_or(target_end)
}

fn previous_grapheme(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true).rev().find_map(|(index, _)| (index < offset).then_some(index)).unwrap_or(0)
}

fn next_grapheme(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true).find_map(|(index, _)| (index > offset).then_some(index)).unwrap_or(text.len())
}

fn utf16_to_utf8(text: &str, utf16_offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for ch in text.chars() {
        if utf16 >= utf16_offset {
            break;
        }
        utf16 += ch.len_utf16();
        utf8 += ch.len_utf8();
    }
    utf8
}

fn utf8_to_utf16(text: &str, utf8_offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for ch in text.chars() {
        if utf8 >= utf8_offset {
            break;
        }
        utf8 += ch.len_utf8();
        utf16 += ch.len_utf16();
    }
    utf16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_and_down_keep_the_column_in_graphemes_and_never_split_a_character() {
        let text = "a\néx\nabc";
        // After "a" on line 1, Down lands after "é" (2 bytes), not inside it.
        assert_eq!(vertical_target(text, 1, 1), 4);
        // At the end of "abc", Up lands at the end of the shorter line above.
        assert_eq!(vertical_target(text, text.len(), -1), 5);
        assert_eq!(vertical_target(text, 0, -1), 0);
        assert_eq!(vertical_target(text, 6, 1), text.len());
    }

    #[test]
    fn offsets_inside_a_character_snap_back() {
        assert_eq!(char_boundary("é", 1), 0);
        assert_eq!(char_boundary("é", 9), 2);
        assert_eq!(line_index("a\né", 3), 1);
    }
}
