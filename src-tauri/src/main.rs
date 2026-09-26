// Release builds use the GUI subsystem so no console window opens.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod files;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use files::{FileMeta, OpenedFile};
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, State, WebviewWindow, WindowEvent};
use tauri_plugin_dialog::DialogExt;

const MARKDOWN_EXTS: &[&str] = &["md", "markdown", "txt"];

/// Paths the user chose via dialog, drag-drop, CLI arg or Explorer. The webview
/// can only open/save these, so a compromised preview cannot reach other files.
struct AppState {
    started: Instant,
    user_chosen: Mutex<HashSet<PathBuf>>,
    startup_file: Mutex<Option<PathBuf>>,
}

impl AppState {
    fn remember_user_choice(&self, path: PathBuf) -> PathBuf {
        let path = std::path::absolute(&path).unwrap_or(path);
        self.user_chosen.lock().unwrap().insert(path.clone());
        path
    }

    fn require_user_choice(&self, path: &str) -> Result<PathBuf, String> {
        let path = std::path::absolute(path).map_err(|e| e.to_string())?;
        if self.user_chosen.lock().unwrap().contains(&path) {
            Ok(path)
        } else {
            Err("path was not selected by the user".into())
        }
    }
}

/// The first non-flag argument, resolved against the launching process's cwd.
fn first_file_arg(args: impl IntoIterator<Item = String>, cwd: &Path) -> Option<PathBuf> {
    args.into_iter().find(|a| !a.starts_with('-')).map(|a| cwd.join(a))
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[tauri::command]
fn startup_file(state: State<'_, AppState>) -> Option<String> {
    state.startup_file.lock().unwrap().take().map(|p| display(&p))
}

/// Milliseconds since process start; the frontend calls this on its first editable frame.
#[tauri::command]
fn startup_ms(state: State<'_, AppState>, js_ms: f64, marks: String) -> f64 {
    let ms = state.started.elapsed().as_secs_f64() * 1000.0;
    perf_log(&format!("startup_ms={ms:.0} js_ms={js_ms:.0} (webview up at ~{:.0}) js: {marks}", ms - js_ms));
    ms
}

/// Opt-in timing log for benchmarking release builds, which have no console.
fn perf_log(line: &str) {
    if let Some(log) = std::env::var_os("MARKY_PERF_LOG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
            let _ = writeln!(f, "{line}");
        }
    }
}

#[tauri::command]
async fn pick_open(window: WebviewWindow, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let picked = window
        .dialog()
        .file()
        .set_parent(&window)
        .add_filter("Markdown", MARKDOWN_EXTS)
        .blocking_pick_file();
    let Some(picked) = picked else { return Ok(None) };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    Ok(Some(display(&state.remember_user_choice(path))))
}

#[tauri::command]
async fn pick_save(
    window: WebviewWindow,
    state: State<'_, AppState>,
    suggested: Option<String>,
) -> Result<Option<String>, String> {
    let mut dialog = window
        .dialog()
        .file()
        .set_parent(&window)
        .add_filter("Markdown", &["md", "markdown"])
        .add_filter("Text", &["txt"]);
    if let Some(name) = suggested {
        dialog = dialog.set_file_name(name);
    }
    let Some(picked) = dialog.blocking_save_file() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    Ok(Some(display(&state.remember_user_choice(path))))
}

#[tauri::command]
async fn open_file(state: State<'_, AppState>, path: String) -> Result<OpenedFile, String> {
    let path = state.require_user_choice(&path)?;
    files::read(&path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_file(
    state: State<'_, AppState>,
    path: String,
    text: String,
    meta: FileMeta,
) -> Result<u64, String> {
    let path = state.require_user_choice(&path)?;
    files::write(&path, &text, meta).map_err(|e| e.to_string())
}

fn request_open(app: &AppHandle, path: PathBuf) {
    let path = app.state::<AppState>().remember_user_choice(path);
    let _ = app.emit_to("main", "open-request", display(&path));
}

fn main() {
    let started = Instant::now();
    let cwd = std::env::current_dir().unwrap_or_default();
    let startup = first_file_arg(std::env::args().skip(1), &cwd);

    let state = AppState { started, user_chosen: Mutex::default(), startup_file: Mutex::new(None) };
    if let Some(path) = startup {
        let path = state.remember_user_choice(path);
        *state.startup_file.lock().unwrap() = Some(path);
    }

    tauri::Builder::default()
        // Must be first: a second launch (e.g. double-click in Explorer) forwards its file here and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            if let Some(path) = first_file_arg(argv.into_iter().skip(1), Path::new(&cwd)) {
                request_open(app, path);
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(move |_| {
            perf_log(&format!("setup_ms={:.0}", started.elapsed().as_secs_f64() * 1000.0));
            Ok(())
        })
        .on_page_load(move |_, payload| {
            let phase = format!("{:?}", payload.event()).to_lowercase();
            perf_log(&format!("page_{phase}_ms={:.0}", started.elapsed().as_secs_f64() * 1000.0));
        })
        .on_window_event(|window, event| {
            let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event else { return };
            let Some(path) = paths.first() else { return };
            request_open(window.app_handle(), path.clone());
        })
        .invoke_handler(tauri::generate_handler![
            startup_file,
            startup_ms,
            pick_open,
            pick_save,
            open_file,
            save_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running Marky");
}
