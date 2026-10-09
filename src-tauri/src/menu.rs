//! Native menu bar (File, Edit, View, Help).
//!
//! Items register no accelerators: every shortcut is handled once, in the frontend.
//! The "\tCtrl+S" suffix only displays the hint; muda encodes a label with an
//! accelerator the same way, so Windows right-aligns it identically.
//!
//! Toggle items are check items. Their ticks mirror settings the frontend owns, so the
//! frontend sets them (`set_checks`) whenever one of those settings changes.

use std::collections::HashMap;

use tauri::menu::{Menu, MenuBuilder, MenuEvent, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::MessageDialogKind;

const ABOUT: &str = "about";

/// Item ids are the frontend's command names (see `commands` in main.ts).
pub fn build(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let file = SubmenuBuilder::new(app, "&File")
        .text("new", "&New Tab\tCtrl+N")
        .text("open", "&Open…\tCtrl+O")
        .text("open-recent", "Open &Recent…\tCtrl+R")
        .separator()
        .text("save", "&Save\tCtrl+S")
        .text("save-as", "Save &As…\tCtrl+Shift+S")
        .check("toggle-auto-save", "Auto Sa&ve")
        .separator()
        .text("settings", "Se&ttings…\tCtrl+,")
        .separator()
        .text("close", "&Close Tab\tCtrl+W")
        .build()?;
    let edit = SubmenuBuilder::new(app, "&Edit")
        .text("undo", "&Undo\tCtrl+Z")
        .text("redo", "&Redo\tCtrl+Y")
        .separator()
        .text("find", "&Find\tCtrl+F")
        .text("replace", "&Replace\tCtrl+H")
        .build()?;
    let view = SubmenuBuilder::new(app, "&View")
        .check("toggle-editor", "&Editor\tCtrl+Shift+E")
        .check("toggle-preview", "&Preview\tCtrl+Shift+V")
        .check("toggle-outline", "&Outline\tCtrl+Shift+O")
        .check("toggle-word-wrap", "&Word Wrap\tAlt+Z")
        .separator()
        .text("next-tab", "Ne&xt Tab\tCtrl+Tab")
        .text("previous-tab", "Pre&vious Tab\tCtrl+Shift+Tab")
        .build()?;
    let help = SubmenuBuilder::new(app, "&Help").text(ABOUT, "&About Marky").build()?;
    MenuBuilder::new(app).items(&[&file, &edit, &view, &help]).build()
}

/// About is answered natively; everything else is a frontend command.
pub fn on_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    if id == ABOUT {
        return show_about(app);
    }
    let _ = app.emit_to("main", "menu", id);
}

/// Ticks or clears the named check items (ids are command names); unknown ids are ignored.
pub fn set_checks(app: &AppHandle, checks: &HashMap<String, bool>) {
    let Some(menu) = app.menu() else { return };
    let submenus = menu.items().unwrap_or_default();
    for (id, &checked) in checks {
        let item = submenus.iter().filter_map(|s| s.as_submenu()).find_map(|s| s.get(id.as_str()));
        let Some(item) = item.as_ref().and_then(|i| i.as_check_menuitem()) else { continue };
        let _ = item.set_checked(checked);
    }
}

fn show_about(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    let version = &app.package_info().version;
    let text = format!(
        "Marky {version}\n\nA fast, lightweight Markdown editor.\nMIT License. Third-party licenses are in THIRD_PARTY_NOTICES.txt next to marky.exe."
    );
    crate::message_dialog(&window, text, MessageDialogKind::Info).title("About Marky").show(|_| {});
}
