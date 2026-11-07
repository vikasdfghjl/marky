//! A single-line text field for the find bar, and the element that connects any view to Windows
//! text input. Text reaches a view only through GPUI's platform input handler (see gpui's
//! `examples/input.rs`), so the field gets typing and IME composition the same way the editor does.

use std::ops::Range;

use gpui::{
    App, Bounds, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Point, Position,
    SharedString, Style, UTF16Selection, Window, div, prelude::*, px, relative, size,
};

use crate::editor::{
    Backspace, Copy, Cut, Delete, DeleteWordLeft, DeleteWordRight, DocumentEnd, DocumentStart, Down, Enter, Left, LineEnd, LineStart, Paste,
    Redo, Right, SelectAll, SelectDocumentEnd, SelectDocumentStart, SelectDown, SelectLeft, SelectLineEnd, SelectLineStart, SelectRight,
    SelectUp, SelectWordLeft, SelectWordRight, Undo, Up, WordLeft, WordRight,
};
use crate::preview_view::tone;

pub enum FieldEvent {
    Changed,
    Submitted,
}

pub struct TextField {
    text: String,
    /// IME composition in progress.
    marked: Option<Range<usize>>,
    focus: FocusHandle,
    placeholder: SharedString,
    pub dark: bool,
}

impl EventEmitter<FieldEvent> for TextField {}

impl TextField {
    pub fn new(placeholder: &str, cx: &mut Context<Self>) -> Self {
        Self { text: String::new(), marked: None, focus: cx.focus_handle(), placeholder: placeholder.to_string().into(), dark: true }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.text = text.to_string();
        self.marked = None;
        cx.emit(FieldEvent::Changed);
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.text.pop().is_some() {
            cx.emit(FieldEvent::Changed);
            cx.notify();
        }
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::Submitted);
    }

    /// The field holds one line, so a pasted block contributes its first line.
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        self.text.push_str(text.lines().next().unwrap_or(""));
        cx.emit(FieldEvent::Changed);
        cx.notify();
    }

    fn utf16_range(&self, range: &Range<usize>) -> Range<usize> {
        to_utf16(&self.text, range.start)..to_utf16(&self.text, range.end)
    }

    fn byte_range(&self, range_utf16: &Range<usize>) -> Range<usize> {
        from_utf16(&self.text, range_utf16.start)..from_utf16(&self.text, range_utf16.end)
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let focused = self.focus.is_focused(window);
        let empty = self.text.is_empty();
        let shown: SharedString = if empty { self.placeholder.clone() } else { self.text.clone().into() };
        let caret = || div().w(px(1.5)).h(px(14.)).bg(tone(dark, 0x2563eb, 0x2563eb));
        div()
            .key_context("TextField")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::paste))
            // The caret stays at the end of the field. These keys would otherwise reach the editor
            // behind it and move its caret or undo its text while the user types a query.
            .on_action(|_: &Left, _, _| {})
            .on_action(|_: &Right, _, _| {})
            .on_action(|_: &Up, _, _| {})
            .on_action(|_: &Down, _, _| {})
            .on_action(|_: &LineStart, _, _| {})
            .on_action(|_: &LineEnd, _, _| {})
            .on_action(|_: &SelectLeft, _, _| {})
            .on_action(|_: &SelectRight, _, _| {})
            .on_action(|_: &SelectAll, _, _| {})
            .on_action(|_: &Delete, _, _| {})
            .on_action(|_: &Copy, _, _| {})
            .on_action(|_: &Cut, _, _| {})
            .on_action(|_: &Undo, _, _| {})
            .on_action(|_: &Redo, _, _| {})
            .on_action(|_: &SelectUp, _, _| {})
            .on_action(|_: &SelectDown, _, _| {})
            .on_action(|_: &SelectLineStart, _, _| {})
            .on_action(|_: &SelectLineEnd, _, _| {})
            .on_action(|_: &WordLeft, _, _| {})
            .on_action(|_: &WordRight, _, _| {})
            .on_action(|_: &SelectWordLeft, _, _| {})
            .on_action(|_: &SelectWordRight, _, _| {})
            .on_action(|_: &DocumentStart, _, _| {})
            .on_action(|_: &DocumentEnd, _, _| {})
            .on_action(|_: &SelectDocumentStart, _, _| {})
            .on_action(|_: &SelectDocumentEnd, _, _| {})
            .on_action(|_: &DeleteWordLeft, _, _| {})
            .on_action(|_: &DeleteWordRight, _, _| {})
            .relative()
            .w(px(220.))
            .px_2()
            .py(px(3.))
            .cursor_text()
            .bg(tone(dark, 0x18181b, 0xffffff))
            .border_1()
            .border_color(if focused { tone(dark, 0x2563eb, 0x2563eb) } else { tone(dark, 0x52525b, 0xd4d4d8) })
            .text_color(if empty { tone(dark, 0x71717a, 0x71717a) } else { tone(dark, 0xfafafa, 0x18181b) })
            .whitespace_nowrap()
            .overflow_hidden()
            .flex()
            .flex_row()
            .items_center()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)))
            .child(InputLayer { view: cx.entity(), focus: self.focus.clone() })
            // The caret is at the end of typed text, and before the placeholder in an empty field.
            .when(focused && empty, |field| field.child(caret()))
            .child(shown)
            .when(focused && !empty, |field| field.child(caret()))
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.byte_range(&range_utf16);
        *actual_range = Some(self.utf16_range(&range));
        Some(self.text[range].to_string())
    }

    /// The caret always sits at the end of the field.
    fn selected_text_range(&mut self, _ignore_disabled_input: bool, _window: &mut Window, _cx: &mut Context<Self>) -> Option<UTF16Selection> {
        let end = to_utf16(&self.text, self.text.len());
        Some(UTF16Selection { range: end..end, reversed: false })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.utf16_range(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let range = self.edit_range(range_utf16);
        self.text.replace_range(range, &new_text.replace(['\r', '\n'], " "));
        self.marked = None;
        cx.emit(FieldEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.edit_range(range_utf16);
        self.text.replace_range(range.clone(), new_text);
        self.marked = (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
        cx.notify();
    }

    fn bounds_for_range(&mut self, _range_utf16: Range<usize>, element_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
    }

    fn character_index_for_point(&mut self, _point: Point<Pixels>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl TextField {
    fn edit_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        if let Some(range) = range_utf16 {
            return self.byte_range(&range);
        }
        self.marked.clone().unwrap_or(self.text.len()..self.text.len())
    }
}

fn to_utf16(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].encode_utf16().count()
}

fn from_utf16(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (index, ch) in text.char_indices() {
        if utf16 >= offset {
            return index;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

/// Registers `view` as the window's text input while `focus` is focused. GPUI delivers typed text
/// only to an input handler registered during paint, so the layer fills its parent without taking space.
pub struct InputLayer<V: EntityInputHandler> {
    pub view: Entity<V>,
    pub focus: FocusHandle,
}

impl<V: EntityInputHandler> IntoElement for InputLayer<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<V: EntityInputHandler> Element for InputLayer<V> {
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
        window.handle_input(&self.focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
    }
}
