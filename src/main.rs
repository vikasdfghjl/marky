//! Marky, a fast Markdown editor for Windows. One file at a time can be edited; more files open as
//! tabs. A release build has no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod allowlist;
mod diagnostics;
mod files;
mod store;
mod watcher;

mod bitmaps;
mod buffer;
mod contrast;
mod diagrams;
mod editor;
mod extras;
mod high_contrast;
mod icons;
mod instance;
mod math;
mod perf;
mod preview;
mod preview_view;
mod settings;
mod syntax;
mod text_field;

use std::path::PathBuf;
use std::rc::Rc;

use editor::Prototype;
use gpui::{
    App, AppContext, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};

fn main() {
    // The app's startup_ms also counts from the top of main, so the two numbers compare.
    perf::start();
    // A second launch forwards its file to the first window and exits, as the app's single instance does.
    let config_dir = settings::own_config_dir();
    let local_dir = settings::own_local_dir();
    let startup_arg = std::env::args().skip(1).find(|arg| !arg.is_empty() && !arg.starts_with('-')).map(PathBuf::from);
    let instance_lock = match instance::start(&config_dir) {
        Ok(instance::Start::Primary(file)) => Some(file),
        Ok(instance::Start::Secondary) => {
            let _ = instance::forward(&config_dir, startup_arg.as_deref());
            return;
        }
        Err(err) => {
            eprintln!("single instance unavailable: {err}");
            None
        }
    };
    diagnostics::init(&local_dir, env!("CARGO_PKG_VERSION"));
    high_contrast::refresh();
    let recovery = store::read_snapshots(&local_dir);
    let geometry = store::load_window_geometry(&config_dir);
    // Without a file (the Start menu shortcut), Marky opens an untitled tab once settings say which
    // line ending new files use.
    let opened = startup_arg.map(plain_path).map(|path| match files::read(&path) {
        Ok(opened) => opened,
        Err(err) => {
            // A release build has no console, so the reason has to be shown.
            let message = format!("Could not open {}: {err}", path.display());
            eprintln!("{message}");
            rfd::MessageDialog::new().set_title("Marky").set_description(message).show();
            std::process::exit(1);
        }
    });
    perf::mark("read");
    let legacy_dir = settings::legacy_config_dir();

    // The platform (Win32 windows, DirectWrite text, DirectX 11) is GPUI's Windows backend.
    let platform = Rc::new(
        gpui_windows::WindowsPlatform::new(false).expect("failed to initialize the Windows platform"),
    );
    let application = gpui::Application::with_platform(platform).with_assets(icons::IconAssets);
    perf::mark("platform");
    application.run(move |cx: &mut App| {
        perf::mark("gpui");
        bind_keys(cx);
        // GPUI keeps running after its last window closes unless the app quits.
        cx.on_window_closed(|cx, _closed| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let window_bounds = match geometry {
            Some(saved) => {
                let bounds = Bounds::new(
                    point(px(saved.x as f32), px(saved.y as f32)),
                    size(px(saved.width as f32), px(saved.height as f32)),
                );
                if saved.maximized { WindowBounds::Maximized(bounds) } else { WindowBounds::Windowed(bounds) }
            }
            None => WindowBounds::Windowed(Bounds::centered(None, size(px(1100.), px(720.)), cx)),
        };
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Marky".into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    perf::mark("os-window");
                    let settings = settings::load(&config_dir, &legacy_dir);
                    let opened = opened.unwrap_or_else(|| files::OpenedFile::untitled(settings.new_file_line_ending));
                    cx.new(|cx| {
                        Prototype::new(opened, settings, config_dir.clone(), local_dir.clone(), recovery, instance_lock, window, cx)
                    })
                },
            )
            .unwrap();
        perf::mark("window");
        window
            .update(cx, |view, window, cx| {
                let handle = view.focus_handle(cx);
                window.focus(&handle, cx);
                let entity = cx.entity();
                window.on_window_should_close(cx, move |window, cx| entity.update(cx, |view, cx| view.may_close(window, cx)));
            })
            .unwrap();
        cx.activate(true);
    });
}

/// `canonicalize` on Windows prefixes `\\?\`, which the path bar should not show.
fn plain_path(path: PathBuf) -> PathBuf {
    let canonical = path.canonicalize().unwrap_or(path);
    let text = canonical.to_string_lossy();
    let Some(rest) = text.strip_prefix(r"\\?\") else {
        return canonical;
    };
    PathBuf::from(match rest.strip_prefix(r"UNC\") {
        Some(unc) => format!(r"\\{unc}"),
        None => rest.to_string(),
    })
}

/// Shortcuts by where the focus is. Editing keys act in the editor and the find fields only, so
/// Enter and Space can press a focused button; the arrows move through a menu, the outline or the
/// preview when one of them has the focus. The app's `SHORTCUTS` table and menu hints are the model.
fn bind_keys(cx: &mut App) {
    cx.bind_keys(key_bindings());
}

fn key_bindings() -> Vec<KeyBinding> {
    // Ctrl, not `secondary-`: since July GPUI resolves `secondary` only when told the OS, and
    // `KeyBinding::new` is not, so it panics. Marky is Windows-only, where `secondary` was Ctrl.
    const TEXT: Option<&str> = Some("Editor || TextField");
    vec![
        KeyBinding::new("backspace", editor::Backspace, TEXT),
        KeyBinding::new("delete", editor::Delete, TEXT),
        KeyBinding::new("left", editor::Left, TEXT),
        KeyBinding::new("right", editor::Right, TEXT),
        KeyBinding::new("up", editor::Up, TEXT),
        KeyBinding::new("down", editor::Down, TEXT),
        KeyBinding::new("shift-left", editor::SelectLeft, TEXT),
        KeyBinding::new("shift-right", editor::SelectRight, TEXT),
        KeyBinding::new("ctrl-a", editor::SelectAll, TEXT),
        KeyBinding::new("home", editor::LineStart, TEXT),
        KeyBinding::new("end", editor::LineEnd, TEXT),
        KeyBinding::new("enter", editor::Enter, TEXT),
        KeyBinding::new("ctrl-v", editor::Paste, TEXT),
        KeyBinding::new("ctrl-c", editor::Copy, TEXT),
        KeyBinding::new("ctrl-x", editor::Cut, TEXT),
        KeyBinding::new("shift-up", editor::SelectUp, TEXT),
        KeyBinding::new("shift-down", editor::SelectDown, TEXT),
        KeyBinding::new("shift-home", editor::SelectLineStart, TEXT),
        KeyBinding::new("shift-end", editor::SelectLineEnd, TEXT),
        KeyBinding::new("ctrl-left", editor::WordLeft, TEXT),
        KeyBinding::new("ctrl-right", editor::WordRight, TEXT),
        KeyBinding::new("ctrl-shift-left", editor::SelectWordLeft, TEXT),
        KeyBinding::new("ctrl-shift-right", editor::SelectWordRight, TEXT),
        KeyBinding::new("ctrl-home", editor::DocumentStart, TEXT),
        KeyBinding::new("ctrl-end", editor::DocumentEnd, TEXT),
        KeyBinding::new("ctrl-shift-home", editor::SelectDocumentStart, TEXT),
        KeyBinding::new("ctrl-shift-end", editor::SelectDocumentEnd, TEXT),
        KeyBinding::new("ctrl-backspace", editor::DeleteWordLeft, TEXT),
        KeyBinding::new("ctrl-delete", editor::DeleteWordRight, TEXT),
        KeyBinding::new("ctrl-z", editor::Undo, TEXT),
        KeyBinding::new("ctrl-y", editor::Redo, TEXT),
        KeyBinding::new("ctrl-shift-z", editor::Redo, TEXT),
        // Shift+Enter is a new line in the editor and "previous match" in the find field.
        KeyBinding::new("shift-enter", editor::Enter, Some("Editor")),
        KeyBinding::new("shift-enter", editor::FindPrevious, Some("TextField")),
        // Tab indents in the editor, as the app's `indentWithTab`; elsewhere it moves the focus. GPUI
        // lets the later of two matching bindings win, so the window-wide pair comes first.
        KeyBinding::new("tab", editor::FocusNext, None),
        KeyBinding::new("shift-tab", editor::FocusPrevious, None),
        KeyBinding::new("tab", editor::Indent, Some("Editor")),
        KeyBinding::new("shift-tab", editor::Outdent, Some("Editor")),
        // While the settings panel is open, Tab stays among its controls.
        KeyBinding::new("tab", editor::SettingsNext, Some("Settings")),
        KeyBinding::new("shift-tab", editor::SettingsPrevious, Some("Settings")),
        // Anywhere in the window.
        KeyBinding::new("ctrl-n", editor::NewTab, None),
        KeyBinding::new("ctrl-o", editor::OpenFile, None),
        KeyBinding::new("ctrl-r", editor::OpenRecent, None),
        KeyBinding::new("ctrl-s", editor::Save, None),
        KeyBinding::new("ctrl-shift-s", editor::SaveAs, None),
        KeyBinding::new("ctrl-w", editor::CloseTab, None),
        KeyBinding::new("ctrl-,", editor::OpenSettings, None),
        KeyBinding::new("ctrl-f", editor::Find, None),
        KeyBinding::new("ctrl-h", editor::FindReplace, None),
        KeyBinding::new("f3", editor::FindNext, None),
        KeyBinding::new("shift-f3", editor::FindPrevious, None),
        KeyBinding::new("escape", editor::FindClose, None),
        KeyBinding::new("ctrl-shift-e", editor::ToggleEditor, None),
        KeyBinding::new("ctrl-shift-v", editor::TogglePreview, None),
        KeyBinding::new("ctrl-shift-o", editor::ToggleOutline, None),
        KeyBinding::new("alt-z", editor::ToggleWordWrap, None),
        KeyBinding::new("ctrl-tab", editor::NextTab, None),
        KeyBinding::new("ctrl-shift-tab", editor::PreviousTab, None),
        KeyBinding::new("alt-f", editor::OpenFileMenu, None),
        KeyBinding::new("f10", editor::OpenFileMenu, None),
        KeyBinding::new("alt-e", editor::OpenEditMenu, None),
        KeyBinding::new("alt-v", editor::OpenViewMenu, None),
        // An open menu.
        KeyBinding::new("down", editor::MenuNext, Some("Menu")),
        KeyBinding::new("up", editor::MenuPrevious, Some("Menu")),
        KeyBinding::new("left", editor::MenuLeft, Some("Menu")),
        KeyBinding::new("right", editor::MenuRight, Some("Menu")),
        // The underlined access keys. Registering them as actions means GPUI consumes the key-down,
        // so Windows does not turn it into a character the focused editor would type.
        KeyBinding::new("a", editor::MenuAccessKey('a'), Some("Menu")),
        KeyBinding::new("c", editor::MenuAccessKey('c'), Some("Menu")),
        KeyBinding::new("e", editor::MenuAccessKey('e'), Some("Menu")),
        KeyBinding::new("f", editor::MenuAccessKey('f'), Some("Menu")),
        KeyBinding::new("l", editor::MenuAccessKey('l'), Some("Menu")),
        KeyBinding::new("m", editor::MenuAccessKey('m'), Some("Menu")),
        KeyBinding::new("n", editor::MenuAccessKey('n'), Some("Menu")),
        KeyBinding::new("o", editor::MenuAccessKey('o'), Some("Menu")),
        KeyBinding::new("p", editor::MenuAccessKey('p'), Some("Menu")),
        KeyBinding::new("q", editor::MenuAccessKey('q'), Some("Menu")),
        KeyBinding::new("r", editor::MenuAccessKey('r'), Some("Menu")),
        KeyBinding::new("s", editor::MenuAccessKey('s'), Some("Menu")),
        KeyBinding::new("t", editor::MenuAccessKey('t'), Some("Menu")),
        KeyBinding::new("u", editor::MenuAccessKey('u'), Some("Menu")),
        KeyBinding::new("v", editor::MenuAccessKey('v'), Some("Menu")),
        KeyBinding::new("w", editor::MenuAccessKey('w'), Some("Menu")),
        // The outline.
        KeyBinding::new("down", editor::OutlineNext, Some("Outline")),
        KeyBinding::new("up", editor::OutlinePrevious, Some("Outline")),
        KeyBinding::new("enter", editor::OutlineOpen, Some("Outline")),
        // The preview scrolls from the keyboard when it has the focus.
        KeyBinding::new("up", editor::PreviewLineUp, Some("Preview")),
        KeyBinding::new("down", editor::PreviewLineDown, Some("Preview")),
        KeyBinding::new("right", editor::PreviewLinkNext, Some("Preview")),
        KeyBinding::new("left", editor::PreviewLinkPrevious, Some("Preview")),
        KeyBinding::new("enter", editor::PreviewLinkOpen, Some("Preview")),
        KeyBinding::new("pageup", editor::PreviewPageUp, Some("Preview")),
        KeyBinding::new("pagedown", editor::PreviewPageDown, Some("Preview")),
        KeyBinding::new("space", editor::PreviewPageDown, Some("Preview")),
        KeyBinding::new("home", editor::PreviewTop, Some("Preview")),
        KeyBinding::new("end", editor::PreviewBottom, Some("Preview")),
    ]
}

#[cfg(test)]
mod tests {
    /// `KeyBinding::new` panics on a keystroke GPUI cannot parse, which in a release build closes the
    /// window before it opens. Moving the GPUI pin to 9a8ab54 did exactly that (`secondary-` changed).
    #[test]
    fn every_key_binding_parses() {
        assert!(super::key_bindings().len() > 60);
    }
}
