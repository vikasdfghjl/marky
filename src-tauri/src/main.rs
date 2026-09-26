// Release builds use the GUI subsystem so no console window opens.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod files;
mod menu;
mod store;
mod watcher;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use files::{FileMeta, OpenedFile};
use notify::RecommendedWatcher;
use serde::Serialize;
use store::Snapshot;
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, State, WebviewWindow, Window, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult};

const MARKDOWN_EXTS: &[&str] = &["md", "markdown", "txt"];
const APP_NAME: &str = "Marky";
const SAVE_LABEL: &str = "Save";
const DONT_SAVE_LABEL: &str = "Don't save";
const CANCEL_LABEL: &str = "Cancel";

/// Background read of the startup file: the path and its result.
type StartupRead = std::thread::JoinHandle<(PathBuf, io::Result<OpenedFile>)>;

struct AppState {
    started: Instant,
    /// Paths the user chose via dialog, drag-drop, CLI arg, Explorer, recent files or
    /// crash recovery. The webview can only touch these, so a compromised preview
    /// cannot reach other files.
    user_chosen: Mutex<HashSet<PathBuf>>,
    /// The CLI/Explorer file, read on a background thread while WebView2 starts (~600 ms).
    startup_read: Mutex<Option<StartupRead>>,
    /// Mirrors the editor's unsaved state so a clean window closes without a JS round trip.
    dirty: AtomicBool,
    /// One watcher per open tab's file; dropping an entry stops that watch.
    watchers: Mutex<HashMap<PathBuf, RecommendedWatcher>>,
}

impl AppState {
    fn remember_user_choice(&self, path: PathBuf) -> PathBuf {
        let path = std::path::absolute(&path).unwrap_or(path);
        self.user_chosen.lock().unwrap().insert(path.clone());
        path
    }

    fn require_user_choice(&self, path: &str) -> Result<PathBuf, String> {
        let path = std::path::absolute(path).map_err(|e| e.to_string())?;
        if !self.user_chosen.lock().unwrap().contains(&path) {
            return Err("path was not selected by the user".into());
        }
        Ok(path)
    }
}

/// Everything the frontend needs for its first paint, in one IPC round trip.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupState {
    file: Option<OpenedFile>,
    /// Set when the startup file could not be read; the frontend reports it.
    file_error: Option<String>,
    /// Unsaved tabs left by a session that did not end cleanly (empty when none).
    recovery: Vec<Snapshot>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

/// The first non-flag argument, resolved against the launching process's cwd.
fn first_file_arg(args: impl IntoIterator<Item = String>, cwd: &Path) -> Option<PathBuf> {
    args.into_iter().find(|a| !a.starts_with('-')).map(|a| cwd.join(a))
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

fn local_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_local_data_dir().map_err(|e| e.to_string())
}

/// Bookkeeping after a successful open or save: recent list (F9) and letting the
/// preview load images from the document's folder (F12). Neither may fail the open/save.
fn record_file_access(app: &AppHandle, path: &Path) {
    if let Ok(dir) = config_dir(app) {
        let _ = store::push_recent(&dir, path);
    }
    if let Some(folder) = path.parent() {
        let _ = app.asset_protocol_scope().allow_directory(folder, true);
    }
}

// ---- startup & diagnostics ----

/// One-shot: the pre-read startup file and any crash-recovery snapshot (whose path is re-authorised).
#[tauri::command]
async fn startup_state(app: AppHandle, state: State<'_, AppState>) -> Result<StartupState, String> {
    let recovery = store::read_snapshots(&local_data_dir(&app)?);
    for path in recovery.iter().filter_map(|s| s.path.as_ref()) {
        state.remember_user_choice(PathBuf::from(path));
    }

    let pending_read = state.startup_read.lock().unwrap().take();
    let Some(pending_read) = pending_read else {
        return Ok(StartupState { file: None, file_error: None, recovery });
    };
    let (path, read) = pending_read.join().map_err(|_| "startup file reader panicked".to_string())?;
    match read {
        Ok(file) => {
            record_file_access(&app, &path);
            Ok(StartupState { file: Some(file), file_error: None, recovery })
        }
        Err(e) => Ok(StartupState { file: None, file_error: Some(format!("Could not open {}\n\n{e}", display(&path))), recovery }),
    }
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
    let Some(log) = std::env::var_os("MARKY_PERF_LOG") else { return };
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(f, "{line}");
    }
}

// ---- file dialogs & I/O ----

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
async fn open_file(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<OpenedFile, String> {
    let path = state.require_user_choice(&path)?;
    let opened = files::read(&path).map_err(|e| e.to_string())?;
    record_file_access(&app, &path);
    Ok(opened)
}

/// Returns the new mtime so the frontend can recognise its own write in `file-changed`.
#[tauri::command]
async fn save_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    text: String,
    meta: FileMeta,
) -> Result<u64, String> {
    let path = state.require_user_choice(&path)?;
    let mtime = files::write(&path, &text, meta).map_err(|e| e.to_string())?;
    record_file_access(&app, &path);
    Ok(mtime)
}

/// None when the file no longer exists.
#[tauri::command]
async fn file_mtime(state: State<'_, AppState>, path: String) -> Result<Option<u64>, String> {
    let path = state.require_user_choice(&path)?;
    Ok(files::mtime_ms(&path).ok())
}

// ---- links in the preview ----

/// Resolves a relative link clicked in `from`'s preview. Only links from a user-chosen
/// file to an existing .md/.markdown/.txt file are followed; clicking one is the user's
/// choice, so the target joins the allowlist.
#[tauri::command]
async fn resolve_link(state: State<'_, AppState>, from: String, target: String) -> Result<String, String> {
    let from = state.require_user_choice(&from)?;
    let folder = from.parent().ok_or("the document has no folder")?;
    // absolute() normalises `.` and `..` on Windows.
    let path = std::path::absolute(folder.join(&target)).map_err(|e| e.to_string())?;
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !MARKDOWN_EXTS.contains(&extension.as_str()) {
        return Err(format!("Marky follows links to .md, .markdown and .txt files only: {target}"));
    }
    if !path.is_file() {
        return Err(format!("Linked file not found: {}", display(&path)));
    }
    Ok(display(&state.remember_user_choice(path)))
}

/// Opens a web or mail link in the default app. Anything else (file:, javascript:, …) is refused.
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase()).unwrap_or_default();
    if !["http", "https", "mailto"].contains(&scheme.as_str()) {
        return Err(format!("Refusing to open a {scheme}: link"));
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

// ---- external change detection (F8) ----

/// Emits `file-changed` (payload: the path) whenever another program touches `path`.
#[tauri::command]
async fn watch_file(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let path = state.require_user_choice(&path)?;
    if state.watchers.lock().unwrap().contains_key(&path) {
        return Ok(());
    }
    let emitter = app.clone();
    let payload = display(&path);
    let watcher = watcher::watch(&path, move || {
        let _ = emitter.emit_to("main", "file-changed", &payload);
    })
    .map_err(|e| e.to_string())?;
    state.watchers.lock().unwrap().insert(path, watcher);
    Ok(())
}

#[tauri::command]
fn unwatch_file(state: State<'_, AppState>, path: String) {
    let Ok(path) = std::path::absolute(&path) else { return };
    state.watchers.lock().unwrap().remove(&path);
}

// ---- recent files (F9) ----

/// Existing recent files, most recent first. Listing them re-authorises them for opening.
#[tauri::command]
async fn recent_files(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let recent = store::load_recent(&config_dir(&app)?);
    Ok(recent
        .into_iter()
        .filter(|p| p.is_file())
        .map(|p| display(&state.remember_user_choice(p)))
        .collect())
}

// ---- crash recovery (F13); the snapshot itself is read by `startup_state` ----

/// Replaces the recovery file with the given unsaved tabs; an empty list clears it.
#[tauri::command]
async fn snapshot_buffers(app: AppHandle, snapshots: Vec<Snapshot>) -> Result<(), String> {
    let dir = local_data_dir(&app)?;
    let result = if snapshots.is_empty() { store::clear_snapshot(&dir) } else { store::save_snapshots(&dir, &snapshots) };
    result.map_err(|e| e.to_string())
}


// ---- prompts & window lifecycle ----

#[tauri::command]
fn set_dirty(state: State<'_, AppState>, dirty: bool) {
    state.dirty.store(dirty, Ordering::Relaxed);
}

#[tauri::command]
async fn ask_unsaved_changes(window: WebviewWindow, file_name: String) -> UnsavedChoice {
    let result = window
        .dialog()
        .message(format!("Do you want to save changes to {file_name}?"))
        .title(APP_NAME)
        .kind(MessageDialogKind::Warning)
        .parent(&window)
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            SAVE_LABEL.into(),
            DONT_SAVE_LABEL.into(),
            CANCEL_LABEL.into(),
        ))
        .blocking_show_with_result();
    match result {
        MessageDialogResult::Custom(label) if label == SAVE_LABEL => UnsavedChoice::Save,
        MessageDialogResult::Custom(label) if label == DONT_SAVE_LABEL => UnsavedChoice::Discard,
        MessageDialogResult::Yes => UnsavedChoice::Save,
        MessageDialogResult::No => UnsavedChoice::Discard,
        // Closing the dialog with Esc or the title-bar X keeps the document open.
        _ => UnsavedChoice::Cancel,
    }
}

/// Two-button question; true only when the user picks `confirm`.
#[tauri::command]
async fn ask_choice(window: WebviewWindow, message: String, confirm: String, cancel: String) -> bool {
    let result = window
        .dialog()
        .message(message)
        .title(APP_NAME)
        .kind(MessageDialogKind::Warning)
        .parent(&window)
        .buttons(MessageDialogButtons::OkCancelCustom(confirm.clone(), cancel))
        .blocking_show_with_result();
    match result {
        MessageDialogResult::Custom(label) => label == confirm,
        MessageDialogResult::Ok => true,
        _ => false,
    }
}

#[tauri::command]
async fn show_error(window: WebviewWindow, message: String) {
    window
        .dialog()
        .message(message)
        .title(APP_NAME)
        .kind(MessageDialogKind::Error)
        .parent(&window)
        .blocking_show();
}

/// Closes without raising CloseRequested again; the frontend has already resolved
/// unsaved changes, so any recovery snapshot is obsolete.
#[tauri::command]
fn close_window(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if let Ok(dir) = local_data_dir(&app) {
        let _ = store::clear_snapshot(&dir);
    }
    window.destroy().map_err(|e| e.to_string())
}

fn request_open(app: &AppHandle, path: PathBuf) {
    let path = app.state::<AppState>().remember_user_choice(path);
    let _ = app.emit_to("main", "open-request", display(&path));
}

fn has_unsaved_changes(window: &Window) -> bool {
    window.state::<AppState>().dirty.load(Ordering::Relaxed)
}

fn on_window_event(window: &Window, event: &WindowEvent) {
    match event {
        WindowEvent::DragDrop(DragDropEvent::Enter { .. }) => {
            let _ = window.emit_to("main", "drag-hover", true);
        }
        WindowEvent::DragDrop(DragDropEvent::Leave) => {
            let _ = window.emit_to("main", "drag-hover", false);
        }
        WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
            let _ = window.emit_to("main", "drag-hover", false);
            for path in paths {
                request_open(window.app_handle(), path.clone());
            }
        }
        WindowEvent::CloseRequested { api, .. } if has_unsaved_changes(window) => {
            api.prevent_close();
            let _ = window.emit_to("main", "close-requested", ());
        }
        _ => {}
    }
}

fn main() {
    let started = Instant::now();
    let cwd = std::env::current_dir().unwrap_or_default();
    let startup = first_file_arg(std::env::args().skip(1), &cwd);

    let state = AppState {
        started,
        user_chosen: Mutex::default(),
        startup_read: Mutex::new(None),
        dirty: AtomicBool::new(false),
        watchers: Mutex::default(),
    };
    if let Some(path) = startup {
        let path = state.remember_user_choice(path);
        let reader = std::thread::spawn(move || {
            let read = files::read(&path);
            (path, read)
        });
        *state.startup_read.lock().unwrap() = Some(reader);
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
        .menu(menu::build)
        .on_menu_event(menu::on_event)
        .manage(state)
        .setup(move |_| {
            perf_log(&format!("setup_ms={:.0}", started.elapsed().as_secs_f64() * 1000.0));
            Ok(())
        })
        .on_page_load(move |_, payload| {
            let phase = format!("{:?}", payload.event()).to_lowercase();
            perf_log(&format!("page_{phase}_ms={:.0}", started.elapsed().as_secs_f64() * 1000.0));
        })
        .on_window_event(on_window_event)
        .invoke_handler(tauri::generate_handler![
            startup_state,
            startup_ms,
            pick_open,
            pick_save,
            open_file,
            save_file,
            file_mtime,
            resolve_link,
            open_external,
            watch_file,
            unwatch_file,
            recent_files,
            snapshot_buffers,
            set_dirty,
            ask_unsaved_changes,
            ask_choice,
            show_error,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running Marky");
}
