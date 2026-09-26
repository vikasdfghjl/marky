// Release builds use the GUI subsystem so no console window opens.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod files;
mod store;
mod watcher;

use std::collections::HashSet;
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

struct AppState {
    started: Instant,
    /// Paths the user chose via dialog, drag-drop, CLI arg, Explorer, recent files or
    /// crash recovery. The webview can only touch these, so a compromised preview
    /// cannot reach other files.
    user_chosen: Mutex<HashSet<PathBuf>>,
    startup_file: Mutex<Option<PathBuf>>,
    /// Mirrors the editor's unsaved state so a clean window closes without a JS round trip.
    dirty: AtomicBool,
    /// One watcher for the one open file; replacing it stops the previous watch.
    watcher: Mutex<Option<RecommendedWatcher>>,
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

// ---- external change detection (F8) ----

/// Emits `file-changed` whenever another program touches `path`. Replaces any previous watch.
#[tauri::command]
async fn watch_file(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let path = state.require_user_choice(&path)?;
    let emitter = app.clone();
    let payload = display(&path);
    let watcher = watcher::watch(&path, move || {
        let _ = emitter.emit_to("main", "file-changed", &payload);
    })
    .map_err(|e| e.to_string())?;
    *state.watcher.lock().unwrap() = Some(watcher);
    Ok(())
}

#[tauri::command]
fn unwatch_file(state: State<'_, AppState>) {
    *state.watcher.lock().unwrap() = None;
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

// ---- crash recovery (F13) ----

#[tauri::command]
async fn snapshot_buffer(app: AppHandle, snapshot: Snapshot) -> Result<(), String> {
    store::save_snapshot(&local_data_dir(&app)?, &snapshot).map_err(|e| e.to_string())
}

#[tauri::command]
async fn clear_snapshot(app: AppHandle) -> Result<(), String> {
    store::clear_snapshot(&local_data_dir(&app)?).map_err(|e| e.to_string())
}

/// The snapshot left by a session that did not end cleanly, if any. Its path is re-authorised.
#[tauri::command]
async fn read_recovery(app: AppHandle, state: State<'_, AppState>) -> Result<Option<Snapshot>, String> {
    let Some(snapshot) = store::read_snapshot(&local_data_dir(&app)?) else { return Ok(None) };
    if let Some(path) = &snapshot.path {
        state.remember_user_choice(PathBuf::from(path));
    }
    Ok(Some(snapshot))
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
        WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
            let Some(path) = paths.first() else { return };
            request_open(window.app_handle(), path.clone());
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
        startup_file: Mutex::new(None),
        dirty: AtomicBool::new(false),
        watcher: Mutex::new(None),
    };
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
        .on_window_event(on_window_event)
        .invoke_handler(tauri::generate_handler![
            startup_file,
            startup_ms,
            pick_open,
            pick_save,
            open_file,
            save_file,
            file_mtime,
            watch_file,
            unwatch_file,
            recent_files,
            snapshot_buffer,
            clear_snapshot,
            read_recovery,
            set_dirty,
            ask_unsaved_changes,
            ask_choice,
            show_error,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running Marky");
}
