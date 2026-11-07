//! The window's chrome around the panes: menus, toolbar, tabs, path bar, find bar, outline, settings
//! panel and status bar.
//!
//! Menu items, toolbar buttons and shortcuts all run a [`Command`], as the app's menus, toolbar and
//! `SHORTCUTS` all go through its `commands` table. Every control can be reached from the keyboard
//! (Tab, the menu keys, or arrows inside a menu or the outline) and has a role and a name for
//! assistive technology; GPUI reports them through AccessKit.

use gpui::{ClickEvent, Role, Stateful, Toggled, transparent_black};

use super::*;

actions!(
    prototype,
    [
        OpenFile,
        OpenSettings,
        CloseTab,
        NextTab,
        PreviousTab,
        ToggleEditor,
        TogglePreview,
        ToggleOutline,
        ToggleWordWrap,
        OpenFileMenu,
        OpenEditMenu,
        OpenViewMenu,
        MenuNext,
        MenuPrevious,
        MenuLeft,
        MenuRight,
        FocusNext,
        FocusPrevious,
        OutlineNext,
        OutlinePrevious,
        OutlineOpen,
        PreviewLineUp,
        PreviewLineDown,
        PreviewPageUp,
        PreviewPageDown,
        PreviewTop,
        PreviewBottom,
    ]
);

/// What a menu item, toolbar button or shortcut does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Open,
    Save,
    Settings,
    CloseTab,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Find,
    Replace,
    ToggleEditor,
    TogglePreview,
    ToggleOutline,
    ToggleWordWrap,
    NextTab,
    PreviousTab,
    LargerText,
    SmallerText,
}

impl Command {
    /// Commands that put the focus somewhere of their own; the others hand it back to the editor
    /// when they come from a menu.
    fn moves_focus(self) -> bool {
        matches!(self, Self::Find | Self::Replace | Self::Settings | Self::Open)
    }
}

struct Entry {
    label: &'static str,
    shortcut: &'static str,
    command: Command,
}

const fn entry(label: &'static str, shortcut: &'static str, command: Command) -> Entry {
    Entry { label, shortcut, command }
}

/// The app's menus (`menu.rs`) with their shortcut hints, less what the prototype does not have
/// (New Tab, Open Recent, Save As, Auto Save). Larger and Smaller text are the prototype's own.
fn entries(menu: OpenMenu) -> &'static [Entry] {
    const FILE: [Entry; 5] = [
        entry("Open…", "Ctrl+O", Command::Open),
        entry("Save", "Ctrl+S", Command::Save),
        entry("Settings…", "Ctrl+,", Command::Settings),
        entry("Close Tab", "Ctrl+W", Command::CloseTab),
        entry("Quit", "", Command::Quit),
    ];
    const EDIT: [Entry; 8] = [
        entry("Undo", "Ctrl+Z", Command::Undo),
        entry("Redo", "Ctrl+Y", Command::Redo),
        entry("Cut", "Ctrl+X", Command::Cut),
        entry("Copy", "Ctrl+C", Command::Copy),
        entry("Paste", "Ctrl+V", Command::Paste),
        entry("Select All", "Ctrl+A", Command::SelectAll),
        entry("Find", "Ctrl+F", Command::Find),
        entry("Replace", "Ctrl+H", Command::Replace),
    ];
    const VIEW: [Entry; 8] = [
        entry("Editor", "Ctrl+Shift+E", Command::ToggleEditor),
        entry("Preview", "Ctrl+Shift+V", Command::TogglePreview),
        entry("Outline", "Ctrl+Shift+O", Command::ToggleOutline),
        entry("Word Wrap", "Alt+Z", Command::ToggleWordWrap),
        entry("Next Tab", "Ctrl+Tab", Command::NextTab),
        entry("Previous Tab", "Ctrl+Shift+Tab", Command::PreviousTab),
        entry("Larger Text", "", Command::LargerText),
        entry("Smaller Text", "", Command::SmallerText),
    ];
    match menu {
        OpenMenu::File => &FILE,
        OpenMenu::Edit => &EDIT,
        OpenMenu::View => &VIEW,
    }
}

/// Room for the longest menu: one focus handle per item, so the arrows can move between them.
pub const MENU_ITEMS: usize = 8;

const MENUS: [(OpenMenu, &str); 3] = [(OpenMenu::File, "File"), (OpenMenu::Edit, "Edit"), (OpenMenu::View, "View")];

/// The focus ring: blue, at least 3:1 against every background it is drawn on (WCAG 2.2 non-text contrast).
pub fn ring(dark: bool) -> gpui::Rgba {
    tone(dark, 0x60a5fa, 0x2563eb)
}

/// Text on the chrome's backgrounds that is quieter than the main text, still at least 4.5:1.
pub fn quiet(dark: bool) -> gpui::Rgba {
    tone(dark, 0xa1a1aa, 0x52525b)
}

fn toggled(on: bool) -> Toggled {
    if on { Toggled::True } else { Toggled::False }
}

impl Prototype {
    /// Every command, whichever control or key started it.
    pub fn run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        match command {
            Command::Open => self.open_file(cx),
            Command::Save => self.write_active(cx),
            Command::Settings => self.toggle_settings(window, cx),
            Command::CloseTab => self.close_tab(self.active, window, cx),
            Command::Quit => self.quit(&Quit, window, cx),
            Command::Undo => self.undo(&Undo, window, cx),
            Command::Redo => self.redo(&Redo, window, cx),
            Command::Cut => self.cut(&Cut, window, cx),
            Command::Copy => self.copy(&Copy, window, cx),
            Command::Paste => self.paste(&Paste, window, cx),
            Command::SelectAll => self.select_all(&SelectAll, window, cx),
            Command::Find => self.open_find(false, window, cx),
            Command::Replace => self.open_find(true, window, cx),
            Command::ToggleEditor => {
                // Hiding the editor never changes `showPreview`; the preview shows anyway, as in the app.
                self.settings.show_editor = !self.settings.show_editor;
                self.keep_settings(cx);
            }
            Command::TogglePreview => {
                self.settings.show_preview = !self.settings.show_preview;
                self.keep_settings(cx);
            }
            Command::ToggleOutline => {
                self.settings.show_outline = !self.settings.show_outline;
                self.keep_settings(cx);
            }
            Command::ToggleWordWrap => {
                self.settings.word_wrap = !self.settings.word_wrap;
                self.keep_settings(cx);
            }
            Command::NextTab => self.activate_tab((self.active + 1) % self.documents.len(), cx),
            Command::PreviousTab => self.activate_tab((self.active + self.documents.len() - 1) % self.documents.len(), cx),
            Command::LargerText => self.step_font(1, cx),
            Command::SmallerText => self.step_font(-1, cx),
        }
        cx.notify();
    }

    /// The shortcuts for commands that have no action of their own in `editor.rs`.
    pub fn command_actions<E: InteractiveElement>(&self, element: E, cx: &mut Context<Self>) -> E {
        element
            .on_action(cx.listener(|this, _: &OpenFile, window, cx| this.run(Command::Open, window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| this.run(Command::Settings, window, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.run(Command::CloseTab, window, cx)))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.run(Command::NextTab, window, cx)))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| this.run(Command::PreviousTab, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleEditor, window, cx| this.run(Command::ToggleEditor, window, cx)))
            .on_action(cx.listener(|this, _: &TogglePreview, window, cx| this.run(Command::TogglePreview, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleOutline, window, cx| this.run(Command::ToggleOutline, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleWordWrap, window, cx| this.run(Command::ToggleWordWrap, window, cx)))
            .on_action(cx.listener(|this, _: &OpenFileMenu, window, cx| this.open_menu_from_keys(OpenMenu::File, window, cx)))
            .on_action(cx.listener(|this, _: &OpenEditMenu, window, cx| this.open_menu_from_keys(OpenMenu::Edit, window, cx)))
            .on_action(cx.listener(|this, _: &OpenViewMenu, window, cx| this.open_menu_from_keys(OpenMenu::View, window, cx)))
            .on_action(cx.listener(|_, _: &FocusNext, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &FocusPrevious, window, cx| window.focus_prev(cx)))
    }

    pub fn focus_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // ----- Menus -----

    pub fn menu_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut bar = div()
            .id("menu-bar")
            .role(Role::MenuBar)
            .aria_label("Menu")
            .flex()
            .flex_row()
            .items_center()
            .bg(tone(self.dark, 0x27272a, 0xf4f4f5))
            .text_size(px(13.));
        for (menu, label) in MENUS {
            bar = bar.child(self.menu_button(label, menu, cx));
        }
        bar
    }

    fn menu_button(&mut self, label: &'static str, menu: OpenMenu, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let opened = self.open_menu == Some(menu);
        let dark = self.dark;
        let button = div()
            .id(label)
            .role(Role::MenuItem)
            .aria_label(label)
            .aria_expanded(opened)
            .px_3()
            .py_1()
            .when(opened, |button| button.bg(tone(dark, 0x3f3f46, 0xe4e4e7)))
            .child(label)
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if this.open_menu == Some(menu) {
                    this.close_menus(window, cx);
                } else {
                    this.open_menu_at(menu, event.is_keyboard(), window, cx);
                }
            }));
        if !opened {
            return button;
        }
        // The list is taken out of the bar's height and painted after the panes, so it covers the tab instead of pushing it down.
        button.child(deferred(self.menu_items(menu, cx).absolute().top_full().left_0().occlude()))
    }

    /// Alt+F, Alt+E, Alt+V and F10 open a menu with its first item focused; the arrows move from there.
    fn open_menu_from_keys(&mut self, menu: OpenMenu, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu_at(menu, true, window, cx);
    }

    fn open_menu_at(&mut self, menu: OpenMenu, focus_first: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = Some(menu);
        window.focus(if focus_first { &self.menu_focus[0] } else { &self.menu_root }, cx);
        cx.notify();
    }

    /// Closes any open menu. If the focus was in it, it goes back to the editor.
    pub fn close_menus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.open_menu.take() else { return };
        if self.menu_root.is_focused(window) || self.menu_focus[..entries(menu).len()].iter().any(|handle| handle.is_focused(window)) {
            self.focus_editor(window, cx);
        }
        cx.notify();
    }

    fn menu_step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.open_menu else { return };
        let count = entries(menu).len() as isize;
        let current = self.menu_focus[..count as usize].iter().position(|handle| handle.is_focused(window)).map_or(-1, |index| index as isize);
        let next = if current < 0 && delta < 0 { count - 1 } else { (current + delta).rem_euclid(count) };
        window.focus(&self.menu_focus[next as usize], cx);
    }

    /// Left and Right move to the neighbouring menu, as in Windows menus.
    fn menu_switch(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.open_menu else { return };
        let index = MENUS.iter().position(|(candidate, _)| *candidate == menu).unwrap_or(0) as isize;
        let (next, _) = MENUS[(index + delta).rem_euclid(MENUS.len() as isize) as usize];
        self.open_menu_at(next, true, window, cx);
    }

    fn checked(&self, command: Command) -> Option<bool> {
        match command {
            Command::ToggleEditor => Some(self.settings.show_editor),
            Command::TogglePreview => Some(self.settings.preview_visible()),
            Command::ToggleOutline => Some(self.settings.show_outline),
            Command::ToggleWordWrap => Some(self.settings.word_wrap),
            _ => None,
        }
    }

    fn menu_items(&mut self, menu: OpenMenu, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let dark = self.dark;
        let highlight = tone(dark, 0x52525b, 0xdbeafe);
        let column = div()
            .id("menu-items")
            .key_context("Menu")
            .track_focus(&self.menu_root)
            .role(Role::Menu)
            .flex()
            .flex_col()
            .py_1()
            .bg(tone(dark, 0x3f3f46, 0xffffff))
            .border_1()
            .border_color(tone(dark, 0x52525b, 0xd4d4d8))
            .on_action(cx.listener(|this, _: &MenuNext, window, cx| this.menu_step(1, window, cx)))
            .on_action(cx.listener(|this, _: &MenuPrevious, window, cx| this.menu_step(-1, window, cx)))
            .on_action(cx.listener(|this, _: &MenuLeft, window, cx| this.menu_switch(-1, window, cx)))
            .on_action(cx.listener(|this, _: &MenuRight, window, cx| this.menu_switch(1, window, cx)));
        let items = entries(menu).iter().enumerate().map(|(index, entry)| {
            let command = entry.command;
            let checked = self.checked(command);
            div()
                .id(("menu-item", index))
                .track_focus(&self.menu_focus[index])
                .role(if checked.is_some() { Role::MenuItemCheckBox } else { Role::MenuItem })
                .aria_label(entry.label)
                .when_some(checked, |item, on| item.aria_toggled(toggled(on)))
                .hover(|style| style.bg(highlight))
                .focus(|style| style.bg(highlight))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.run(command, window, cx);
                    if !command.moves_focus() {
                        this.focus_editor(window, cx);
                    }
                }))
                .child(menu_row(entry.label, entry.shortcut, checked, dark))
        });
        column.children(items.collect::<Vec<_>>())
    }

    // ----- Toolbar, tabs, path bar -----

    pub fn toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let dirty = self.doc().buffer.is_dirty();
        let read_only = self.doc().read_only;
        let editor = self.settings.show_editor;
        let preview = self.settings.preview_visible();
        let outline = self.settings.show_outline;
        div()
            .id("toolbar")
            .role(Role::Toolbar)
            .aria_label("Toolbar")
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .bg(tone(dark, 0x27272a, 0xe4e4e7))
            .text_size(px(13.))
            .child(self.tool_button("tool-open", "Open", "icons/open.svg", Command::Open, None, true, cx))
            .child(self.tool_button("tool-save", if read_only { "Read-only" } else { "Save" }, "icons/save.svg", Command::Save, None, dirty, cx))
            .child(self.tool_button("tool-editor", "Editor", if editor { "icons/editor.svg" } else { "icons/editor-off.svg" }, Command::ToggleEditor, Some(editor), true, cx))
            .child(self.tool_button("tool-preview", "Preview", if preview { "icons/preview.svg" } else { "icons/preview-off.svg" }, Command::TogglePreview, Some(preview), true, cx))
            .child(self.tool_button("tool-outline", "Outline", "icons/outline.svg", Command::ToggleOutline, Some(outline), true, cx))
    }

    /// A toolbar button. `pressed` buttons are toggles that are on; `enabled` is false only for
    /// Save with nothing to save, the one control drawn dimmed (WCAG exempts inactive controls).
    #[allow(clippy::too_many_arguments)]
    fn tool_button(
        &self,
        id: &'static str,
        label: &'static str,
        icon: &'static str,
        command: Command,
        pressed: Option<bool>,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let dark = self.dark;
        let color = if enabled { tone(dark, 0xfafafa, 0x18181b) } else { tone(dark, 0x71717a, 0xa1a1aa) };
        let on = pressed.unwrap_or(false);
        focus_ring(
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .when_some(pressed, |button, on| button.aria_toggled(toggled(on)))
                .focusable()
                .tab_stop(true)
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .bg(if on { tone(dark, 0x3f3f46, 0xffffff) } else { tone(dark, 0x27272a, 0xe4e4e7) })
                .text_color(color)
                .child(svg().path(icon).w(px(16.)).h(px(16.)).text_color(color))
                .child(label)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.run(command, window, cx))),
            dark,
        )
    }

    pub fn tab_strip(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active;
        let dark = self.dark;
        let tabs: Vec<(String, bool)> = self.documents.iter().map(|doc| (file_name(&doc.path), doc.buffer.is_dirty())).collect();
        let count = tabs.len();
        div().id("tabs").role(Role::TabList).aria_label("Open files").flex().flex_row().bg(tone(dark, 0x27272a, 0xe4e4e7)).children(
            tabs.into_iter().enumerate().map(|(index, (name, dirty))| {
                let label = if dirty { format!("{name}, unsaved changes") } else { name.clone() };
                let shown = if dirty { format!("{name} •") } else { name.clone() };
                let current = index == active;
                div()
                    .id(("tab", index))
                    .role(Role::Tab)
                    .aria_label(SharedString::from(label))
                    .aria_selected(current)
                    .aria_position_in_set(index + 1)
                    .aria_size_of_set(count)
                    .flex()
                    .flex_row()
                    .items_center()
                    .px_2()
                    .py_1()
                    .bg(if current { tone(dark, 0x3f3f46, 0xffffff) } else { tone(dark, 0x27272a, 0xe4e4e7) })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.activate_tab(index, cx)))
                    .child(shown)
                    .when(count > 1, |tab| {
                        tab.child(
                            div()
                                .id(("close-tab", index))
                                .role(Role::Button)
                                .aria_label(SharedString::from(format!("Close {name}")))
                                .pl_2()
                                .child("×")
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.close_tab(index, window, cx);
                                })),
                        )
                    })
            }),
        )
    }

    pub fn path_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let bar = div()
            .id("path-bar")
            .flex()
            .flex_row()
            .gap_3()
            .px_3()
            .py_2()
            .text_size(px(12.))
            .text_color(quiet(dark))
            .bg(tone(dark, 0x27272a, 0xe4e4e7))
            // AccessKit names a label by its value.
            .child(div().id("path").role(Role::Label).aria_value(SharedString::from(self.doc().path.clone())).child(self.doc().path.clone()))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::close_menu));
        match &self.status {
            // An alert, so a screen reader reads a failed save or open out when it happens.
            Some(status) => bar.child(
                div().id("status-message").role(Role::Alert).aria_label(SharedString::from(status.clone())).text_color(tone(dark, 0xfca5a5, 0xb91c1c)).child(status.clone()),
            ),
            None => bar,
        }
    }

    pub fn find_bar(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
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
        let button = |id: &'static str, label: &'static str, name: &'static str| {
            focus_ring(
                div()
                    .id(id)
                    .role(Role::Button)
                    .aria_label(name)
                    .focusable()
                    .tab_stop(true)
                    .px_2()
                    .py(px(3.))
                    .bg(tone(dark, 0x3f3f46, 0xffffff))
                    .text_color(tone(dark, 0xfafafa, 0x18181b))
                    .child(label),
                dark,
            )
        };
        Some(
            div()
                .id("find-bar")
                .role(Role::Search)
                .aria_label("Find and replace")
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .text_size(px(13.))
                .bg(tone(dark, 0x27272a, 0xe4e4e7))
                .text_color(quiet(dark))
                .child(find.query.clone())
                .child(div().id("find-count").role(Role::Status).aria_label(SharedString::from(count.clone())).min_w(px(80.)).child(count))
                .child(button("find-previous", "Previous", "Previous match").on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.find_step(-1, cx))))
                .child(button("find-next", "Next", "Next match").on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.find_step(1, cx))))
                .when(find.show_replace, |bar| {
                    bar.child(find.replacement.clone())
                        .child(button("replace-one", "Replace", "Replace").on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.replace_current(cx))))
                        .child(button("replace-all", "Replace all", "Replace all").on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.replace_all(cx))))
                })
                .child(button("find-close", "×", "Close find").on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.find_close(&FindClose, window, cx)))),
        )
    }

    // ----- Outline -----

    /// The headings of the document, with the one the caret is under highlighted, as in the app
    /// (`outline.ts` follows the caret, not the scroll position). The list is one tab stop; the
    /// arrows move through it and Enter jumps, so a long outline does not fill the Tab order.
    pub fn outline_column(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let headings = self.headings();
        let caret_line = self.doc().buffer.line_of(self.doc().cursor()) + 1;
        let current = headings.iter().rposition(|(line, _, _)| *line <= caret_line);
        let selected = self.outline_cursor.filter(|index| *index < headings.len()).or(current);
        let column = focus_ring(
            div()
                .id("outline")
                .key_context("Outline")
                .track_focus(&self.outline_focus)
                .role(Role::List)
                .aria_label("Outline")
                .w(px(220.))
                .h_full()
                .overflow_y_scroll()
                .bg(tone(dark, 0x18181b, 0xf4f4f5))
                .px_2()
                .py_2()
                .text_size(px(13.))
                .on_action(cx.listener(|this, _: &OutlineNext, _, cx| this.outline_step(1, cx)))
                .on_action(cx.listener(|this, _: &OutlinePrevious, _, cx| this.outline_step(-1, cx)))
                .on_action(cx.listener(|this, _: &OutlineOpen, window, cx| this.outline_open(window, cx))),
            dark,
        );
        if headings.is_empty() {
            return column.child(div().text_color(quiet(dark)).child("No headings"));
        }
        let count = headings.len();
        column.children(headings.into_iter().enumerate().map(|(index, (line, level, text))| {
            let is_current = Some(index) == current;
            let is_selected = Some(index) == selected;
            div()
                .id(("heading", index))
                .role(Role::ListItem)
                .aria_label(SharedString::from(text.clone()))
                .aria_level(level as usize)
                .aria_position_in_set(index + 1)
                .aria_size_of_set(count)
                .aria_selected(is_selected)
                .when(is_selected, |item| item.aria_active_descendant())
                .py_1()
                .pl(px(4. + (level.saturating_sub(1) as f32) * 12.))
                .when(is_current, |item| item.text_color(ring(dark)).font_weight(gpui::FontWeight::BOLD))
                .when(is_selected && self.outline_cursor.is_some(), |item| item.bg(tone(dark, 0x27272a, 0xe4e4e7)))
                .cursor_pointer()
                .child(text)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.jump_to_line(line, cx);
                    this.outline_cursor = None;
                    this.focus_editor(window, cx);
                }))
        }))
    }

    fn headings(&self) -> Vec<(usize, u8, String)> {
        self.doc()
            .blocks
            .iter()
            .filter_map(|block| match block.kind {
                Kind::Heading(level) => Some((block.line, level, block.text.clone())),
                _ => None,
            })
            .collect()
    }

    fn outline_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.headings().len();
        if count == 0 {
            return;
        }
        let caret_line = self.doc().buffer.line_of(self.doc().cursor()) + 1;
        let from = self.outline_cursor.unwrap_or_else(|| self.headings().iter().rposition(|(line, _, _)| *line <= caret_line).unwrap_or(0));
        self.outline_cursor = Some((from as isize + delta).clamp(0, count as isize - 1) as usize);
        cx.notify();
    }

    fn outline_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let headings = self.headings();
        let caret_line = self.doc().buffer.line_of(self.doc().cursor()) + 1;
        let index = self.outline_cursor.or_else(|| headings.iter().rposition(|(line, _, _)| *line <= caret_line));
        let Some((line, _, _)) = index.and_then(|index| headings.get(index)) else { return };
        self.jump_to_line(*line, cx);
        self.outline_cursor = None;
        self.focus_editor(window, cx);
    }

    // ----- Status bar -----

    /// The app's footer (`status-bar.ts`): caret position, counts, line ending, encoding and the two
    /// timings. Positions and counts are in UTF-16 units, like the app's JavaScript strings.
    pub fn status_bar(&mut self) -> impl IntoElement {
        let dark = self.dark;
        let doc = self.doc();
        let cursor = doc.cursor();
        let line = doc.buffer.line_of(cursor);
        let column = doc.buffer.utf16_of(cursor) - doc.buffer.utf16_of(doc.buffer.line_start(line)) + 1;
        let encoding = if doc.read_only {
            "Not UTF-8 (read-only)"
        } else if doc.meta.bom {
            "UTF-8 BOM"
        } else {
            "UTF-8"
        };
        let ending = match doc.meta.line_ending {
            files::LineEnding::Crlf => "CRLF",
            files::LineEnding::Lf => "LF",
        };
        let mut items = vec![
            format!("Ln {}, Col {column}", line + 1),
            format!("{} words, {} chars", grouped(doc.counts.words), grouped(doc.counts.chars)),
            ending.to_string(),
            encoding.to_string(),
        ];
        if let Some((latency, parse)) = doc.last_preview {
            items.push(format!("preview {latency:.0} ms (parse {parse:.1})"));
        }
        if let Some(startup) = perf::startup_ms() {
            items.push(format!("start {startup:.0} ms"));
        }
        div()
            .id("status-bar")
            .role(Role::Group)
            .aria_label("Status bar")
            .flex()
            .flex_row()
            .gap_4()
            .px_3()
            .py(px(3.))
            .text_size(px(12.))
            .bg(tone(dark, 0x27272a, 0xe4e4e7))
            .text_color(quiet(dark))
            .children(items.into_iter().enumerate().map(|(index, item)| {
                div().id(("status", index)).role(Role::Label).aria_value(SharedString::from(item.clone())).child(item)
            }))
    }

    // ----- Settings panel -----

    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = !self.settings_open;
        if self.settings_open {
            window.focus(&self.settings_focus, cx);
        } else {
            self.focus_editor(window, cx);
        }
        cx.notify();
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.settings_open {
            return false;
        }
        self.settings_open = false;
        self.focus_editor(window, cx);
        true
    }

    /// The app's settings panel (`settings-panel.ts`) less what the prototype does not have (auto-save
    /// and the line ending of new files). A side panel, not a modal: Esc or Close puts it away.
    pub fn settings_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let settings = self.settings.clone();
        let heading = |text: &'static str| div().pt_3().pb_1().font_weight(gpui::FontWeight::BOLD).child(text);
        let theme_choice = |id: &'static str, label: &'static str, choice: settings::ThemeChoice, cx: &mut Context<Self>| {
            choice_row(id, label, Role::RadioButton, settings.theme == choice, dark)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.settings.theme = choice;
                    this.keep_settings(cx);
                }))
        };
        let flag = |id: &'static str, label: &'static str, on: bool, change: fn(&mut ViewSettings), cx: &mut Context<Self>| {
            choice_row(id, label, Role::CheckBox, on, dark).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                change(&mut this.settings);
                this.keep_settings(cx);
            }))
        };
        let size_button = |id: &'static str, label: &'static str, name: &'static str, delta: isize, cx: &mut Context<Self>| {
            focus_ring(
                div()
                    .id(id)
                    .role(Role::Button)
                    .aria_label(name)
                    .focusable()
                    .tab_stop(true)
                    .px_2()
                    .bg(tone(dark, 0x3f3f46, 0xffffff))
                    .child(label)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.step_font(delta, cx))),
                dark,
            )
        };
        let size = settings.font_size;
        let typographer = settings.typographer;
        div()
            .id("settings")
            .key_context("Settings")
            .track_focus(&self.settings_focus)
            .role(Role::Group)
            .aria_label("Settings")
            .w(px(280.))
            .h_full()
            .overflow_y_scroll()
            .px_3()
            .py_2()
            .text_size(px(13.))
            .bg(tone(dark, 0x18181b, 0xf4f4f5))
            .border_l_1()
            .border_color(tone(dark, 0x3f3f46, 0xd4d4d8))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .child(div().id("settings-title").role(Role::Heading).aria_level(2).aria_label("Settings").font_weight(gpui::FontWeight::BOLD).child("Settings"))
                    .child(
                        focus_ring(
                            div().id("settings-close").role(Role::Button).aria_label("Close settings").focusable().tab_stop(true).px_2().child("×"),
                            dark,
                        )
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.close_settings(window, cx);
                        })),
                    ),
            )
            .child(heading("Theme"))
            .child(
                div()
                    .id("theme-group")
                    .role(Role::RadioGroup)
                    .aria_label("Theme")
                    .child(theme_choice("theme-system", "System", settings::ThemeChoice::System, cx))
                    .child(theme_choice("theme-light", "Light", settings::ThemeChoice::Light, cx))
                    .child(theme_choice("theme-dark", "Dark", settings::ThemeChoice::Dark, cx)),
            )
            .child(heading("Editor font size"))
            .child(
                div()
                    .id("font-size")
                    .role(Role::SpinButton)
                    .aria_label(SharedString::from(format!("Editor font size {size} px")))
                    .aria_numeric_value(size as f64)
                    .aria_min_numeric_value(settings::FONT_SIZES[0] as f64)
                    .aria_max_numeric_value(settings::FONT_SIZES[settings::FONT_SIZES.len() - 1] as f64)
                    .on_a11y_action(gpui::AccessibleAction::Increment, {
                        let view = cx.entity().downgrade();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.step_font(1, cx));
                        }
                    })
                    .on_a11y_action(gpui::AccessibleAction::Decrement, {
                        let view = cx.entity().downgrade();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.step_font(-1, cx));
                        }
                    })
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(size_button("font-smaller", "−", "Smaller text", -1, cx))
                    .child(format!("{size} px"))
                    .child(size_button("font-larger", "+", "Larger text", 1, cx)),
            )
            .child(heading("Editor and preview"))
            .child(flag("set-wrap", "Word wrap", settings.word_wrap, |s| s.word_wrap = !s.word_wrap, cx))
            .child(
                choice_row("set-typographer", "Smart quotes and dashes in the preview", Role::CheckBox, typographer, dark)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_typographer(!typographer, cx))),
            )
            .child(heading("Panes"))
            .child(flag("set-editor", "Show editor", settings.show_editor, |s| s.show_editor = !s.show_editor, cx))
            .child(flag("set-preview", "Show preview", settings.show_preview, |s| s.show_preview = !s.show_preview, cx))
            .child(flag("set-outline", "Show outline", settings.show_outline, |s| s.show_outline = !s.show_outline, cx))
    }

    /// Smart punctuation changes what the preview shows, so every open document is parsed again.
    fn set_typographer(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.typographer = on;
        self.keep_settings(cx);
        let ids: Vec<u64> = self.documents.iter().map(|doc| doc.id).collect();
        for id in ids {
            self.schedule_parse(id, Duration::ZERO, cx);
        }
    }

    // ----- Preview scrolling from the keyboard -----

    pub fn preview_keys<E: InteractiveElement>(&self, element: E, cx: &mut Context<Self>) -> E {
        let line = px(40.);
        element
            .on_action(cx.listener(move |this, _: &PreviewLineUp, _, cx| this.scroll_preview(line, cx)))
            .on_action(cx.listener(move |this, _: &PreviewLineDown, _, cx| this.scroll_preview(-line, cx)))
            .on_action(cx.listener(|this, _: &PreviewPageUp, _, cx| {
                let page = this.doc().block_list.viewport_bounds().size.height * 0.9;
                this.scroll_preview(page, cx);
            }))
            .on_action(cx.listener(|this, _: &PreviewPageDown, _, cx| {
                let page = this.doc().block_list.viewport_bounds().size.height * 0.9;
                this.scroll_preview(-page, cx);
            }))
            .on_action(cx.listener(|this, _: &PreviewTop, _, cx| {
                this.doc().block_list.scroll_to(ListOffset { item_ix: 0, offset_in_item: px(0.) });
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PreviewBottom, _, cx| {
                this.doc().block_list.scroll_to_end();
                cx.notify();
            }))
    }

    /// Positive `by` scrolls up, like a wheel turned away from you.
    fn scroll_preview(&mut self, by: Pixels, cx: &mut Context<Self>) {
        self.doc().block_list.scroll_by(-by);
        cx.notify();
    }
}

/// A focusable control with the ring drawn in its border, which is transparent until the control
/// is focused from the keyboard, so the layout does not move.
pub fn focus_ring(element: Stateful<gpui::Div>, dark: bool) -> Stateful<gpui::Div> {
    let color = ring(dark);
    element.border_1().border_color(transparent_black()).focus_visible(move |style| style.border_color(color))
}

/// A check box or radio button: a box drawn in text, the label, and its state for assistive technology.
fn choice_row(id: &'static str, label: &'static str, role: Role, on: bool, dark: bool) -> Stateful<gpui::Div> {
    let mark = match (role, on) {
        (Role::RadioButton, true) => "◉",
        (Role::RadioButton, false) => "○",
        (_, true) => "☑",
        (_, false) => "☐",
    };
    focus_ring(
        div()
            .id(id)
            .role(role)
            .aria_label(label)
            .aria_toggled(toggled(on))
            .focusable()
            .tab_stop(true)
            .flex()
            .flex_row()
            .gap_2()
            .py(px(2.))
            .cursor_pointer()
            .child(mark)
            .child(label),
        dark,
    )
}

fn menu_row(label: &str, shortcut: &str, checked: Option<bool>, dark: bool) -> gpui::Div {
    let check = match checked {
        Some(true) => "✓",
        _ => "",
    };
    div()
        .flex()
        .flex_row()
        .gap_2()
        .px_3()
        .py_1()
        .w(px(240.))
        .text_color(tone(dark, 0xfafafa, 0x18181b))
        .child(div().w(px(12.)).child(check))
        .child(div().flex_1().child(label.to_string()))
        // Lighter than `quiet` in the dark theme: the menu's background is lighter than the bars'.
        .child(div().text_color(tone(dark, 0xd4d4d8, 0x52525b)).child(shortcut.to_string()))
}

/// Thousands separated with commas, like `toLocaleString()` in an English locale.
fn grouped(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1234567), "1,234,567");
    }

    #[test]
    fn every_menu_fits_the_focus_handles() {
        for (menu, _) in MENUS {
            assert!(entries(menu).len() <= MENU_ITEMS);
        }
    }
}
