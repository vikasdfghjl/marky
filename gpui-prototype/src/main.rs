//! Throwaway window for docs/gpui-migration.md. One file at a time can be edited;
//! more files open as tabs. View settings start from the app's settings.json and are kept in the prototype's own file.
// Like the app: a release build has no console window (its console host would also count towards RAM).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[allow(dead_code)]
#[path = "../../src-tauri/src/files.rs"]
mod files;

mod bitmaps;
mod buffer;
mod diagrams;
mod editor;
mod icons;
mod math;
mod perf;
mod preview;
mod preview_view;
mod settings;
mod syntax;
mod text_field;

use std::path::PathBuf;

use editor::Prototype;
use gpui::{
    App, AppContext, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};

fn main() {
    // The app's startup_ms also counts from the top of main, so the two numbers compare.
    perf::start();
    let path = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/fixtures/typical.md")
    });
    let path = plain_path(path);
    let opened = match files::read(&path) {
        Ok(opened) => opened,
        Err(err) => {
            // A release build has no console, so the reason has to be shown.
            let message = format!("Could not open {}: {err}", path.display());
            eprintln!("{message}");
            rfd::MessageDialog::new().set_title("Marky GPUI").set_description(message).show();
            std::process::exit(1);
        }
    };
    perf::mark("read");
    let app_dir = settings::app_config_dir();
    let own_dir = settings::own_config_dir();

    // The platform (Win32 windows, DirectWrite text, DirectX 11) comes from gpui_platform.
    let application = gpui_platform::application().with_assets(icons::IconAssets);
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
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Marky GPUI".into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    perf::mark("os-window");
                    let settings = settings::load(&app_dir, &own_dir);
                    cx.new(|cx| Prototype::new(opened, settings, own_dir.clone(), window, cx))
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

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", editor::Backspace, None),
        KeyBinding::new("delete", editor::Delete, None),
        KeyBinding::new("left", editor::Left, None),
        KeyBinding::new("right", editor::Right, None),
        KeyBinding::new("up", editor::Up, None),
        KeyBinding::new("down", editor::Down, None),
        KeyBinding::new("shift-left", editor::SelectLeft, None),
        KeyBinding::new("shift-right", editor::SelectRight, None),
        KeyBinding::new("secondary-a", editor::SelectAll, None),
        KeyBinding::new("home", editor::LineStart, None),
        KeyBinding::new("end", editor::LineEnd, None),
        KeyBinding::new("enter", editor::Enter, None),
        KeyBinding::new("secondary-v", editor::Paste, None),
        KeyBinding::new("secondary-c", editor::Copy, None),
        KeyBinding::new("secondary-x", editor::Cut, None),
        KeyBinding::new("secondary-s", editor::Save, None),
        KeyBinding::new("shift-up", editor::SelectUp, None),
        KeyBinding::new("shift-down", editor::SelectDown, None),
        KeyBinding::new("shift-home", editor::SelectLineStart, None),
        KeyBinding::new("shift-end", editor::SelectLineEnd, None),
        KeyBinding::new("ctrl-left", editor::WordLeft, None),
        KeyBinding::new("ctrl-right", editor::WordRight, None),
        KeyBinding::new("ctrl-shift-left", editor::SelectWordLeft, None),
        KeyBinding::new("ctrl-shift-right", editor::SelectWordRight, None),
        KeyBinding::new("ctrl-home", editor::DocumentStart, None),
        KeyBinding::new("ctrl-end", editor::DocumentEnd, None),
        KeyBinding::new("ctrl-shift-home", editor::SelectDocumentStart, None),
        KeyBinding::new("ctrl-shift-end", editor::SelectDocumentEnd, None),
        KeyBinding::new("ctrl-backspace", editor::DeleteWordLeft, None),
        KeyBinding::new("ctrl-delete", editor::DeleteWordRight, None),
        KeyBinding::new("secondary-z", editor::Undo, None),
        KeyBinding::new("secondary-y", editor::Redo, None),
        KeyBinding::new("secondary-shift-z", editor::Redo, None),
        KeyBinding::new("secondary-f", editor::Find, None),
        KeyBinding::new("secondary-h", editor::FindReplace, None),
        KeyBinding::new("f3", editor::FindNext, None),
        KeyBinding::new("shift-f3", editor::FindPrevious, None),
        KeyBinding::new("escape", editor::FindClose, None),
        // Shift+Enter is a new line in the editor and "previous match" in the find field.
        KeyBinding::new("shift-enter", editor::Enter, None),
        KeyBinding::new("shift-enter", editor::FindPrevious, Some("TextField")),
    ]);
}
