//! Native menu bar (File, Edit, View, Help).
//!
//! Items register no accelerators: every shortcut is handled once, in the frontend.
//! The "\tCtrl+S" suffix only displays the hint; muda encodes a label with an
//! accelerator the same way, so Windows right-aligns it identically.

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
        .text("toggle-preview", "Toggle &Preview\tCtrl+Shift+V")
        .text("toggle-outline", "Toggle &Outline\tCtrl+Shift+O")
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

fn show_about(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    let version = &app.package_info().version;
    let text = format!(
        "Marky {version}\n\nA fast, lightweight Markdown editor.\nMIT License. Third-party licenses are in THIRD_PARTY_NOTICES.txt next to marky.exe."
    );
    crate::message_dialog(&window, text, MessageDialogKind::Info).title("About Marky").show(|_| {});
}
