// Release builds use the GUI subsystem so no console window opens.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod allowlist;
mod diagnostics;
mod files;
mod memory;
mod menu;
mod store;
mod watcher;
mod window;

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use allowlist::UserChosenPaths;
use files::{FileMeta, OpenedFile, MARKDOWN_EXTS};
use notify::RecommendedWatcher;
use serde::Serialize;
use store::Snapshot;
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, State, WebviewWindow, Window, WindowEvent, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogBuilder, MessageDialogButtons, MessageDialogKind, MessageDialogResult};

const APP_NAME: &str = "Marky";
const SAVE_LABEL: &str = "Save";
const DONT_SAVE_LABEL: &str = "Don't save";
const CANCEL_LABEL: &str = "Cancel";
/// Settings are small; anything larger is not ours.
const SETTINGS_MAX_BYTES: usize = 16 * 1024;

type StartupRead = std::thread::JoinHandle<(PathBuf, io::Result<OpenedFile>)>;

struct AppState {
    started: Instant,
    user_chosen: UserChosenPaths,
    /// The CLI/Explorer file, read on a background thread while WebView2 starts (~600 ms).
    startup_read: Mutex<Option<StartupRead>>,
    /// Mirrors the editor's unsaved state so a clean window closes without a JS round trip.
    dirty: AtomicBool,
    /// One watcher per open tab's file; dropping an entry stops that watch.
    watchers: Mutex<HashMap<PathBuf, RecommendedWatcher>>,
    /// Held while updating recent.json. Opens and saves run concurrently (an auto-save during an
    /// open), and its read-modify-write would otherwise lose an entry.
    recent_list_writes: Mutex<()>,
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
    /// Saved settings, or null for defaults; the frontend owns the schema.
    settings: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Choice {
    Confirm,
    Deny,
    Dismiss,
}

/// The first non-flag argument, resolved against the launching process's cwd.
fn first_file_arg(args: impl IntoIterator<Item = String>, cwd: &Path) -> Option<PathBuf> {
    args.into_iter().find(|a| !a.starts_with('-')).map(|a| cwd.join(a))
}

fn path_string(path: &Path) -> String {
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
        let state = app.state::<AppState>();
        let _one_writer = state.recent_list_writes.lock().unwrap();
        if let Err(e) = store::push_recent(&dir, path) {
            diagnostics::write(&format!("could not update the recent files list: {e}"));
        }
    }
    if let Some(folder) = path.parent() {
        // Recursive: images usually sit in a subfolder such as img/.
        let _ = app.asset_protocol_scope().allow_directory(folder, true);
    }
}

// ---- startup & diagnostics ----

/// Called once. Recovered paths are re-authorised so the restored tabs can be saved back.
#[tauri::command]
async fn startup_state(app: AppHandle, state: State<'_, AppState>) -> Result<StartupState, String> {
    let recovery = store::read_snapshots(&local_data_dir(&app)?);
    for path in recovery.iter().filter_map(|s| s.path.as_ref()) {
        state.user_chosen.remember(PathBuf::from(path));
    }

    let settings = store::load_settings(&config_dir(&app)?);
    let pending_read = state.startup_read.lock().unwrap().take();
    let Some(pending_read) = pending_read else {
        return Ok(StartupState { file: None, file_error: None, recovery, settings });
    };
    let (path, read) = pending_read.join().map_err(|_| "startup file reader panicked".to_string())?;
    let (file, file_error) = match read {
        Ok(file) => {
            record_file_access(&app, &path);
            (Some(file), None)
        }
        Err(e) => (None, Some(format!("Could not open {}\n\n{e}", path_string(&path)))),
    };
    Ok(StartupState { file, file_error, recovery, settings })
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
    Ok(Some(path_string(&state.user_chosen.remember(path))))
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
    Ok(Some(path_string(&state.user_chosen.remember(path))))
}

#[tauri::command]
async fn open_file(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<OpenedFile, String> {
    let path = state.user_chosen.require(&path)?;
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
    let path = state.user_chosen.require(&path)?;
    let mtime = files::write(&path, &text, meta).map_err(|e| e.to_string())?;
    record_file_access(&app, &path);
    Ok(mtime)
}

/// None when the file no longer exists.
#[tauri::command]
async fn file_mtime(state: State<'_, AppState>, path: String) -> Result<Option<u64>, String> {
    let path = state.user_chosen.require(&path)?;
    Ok(files::mtime_ms(&path).ok())
}

// ---- links in the preview ----

/// Resolves a relative link clicked in a user-chosen file's preview (rules in `allowlist::link_target`).
#[tauri::command]
async fn resolve_link(state: State<'_, AppState>, from: String, target: String) -> Result<String, String> {
    let from = state.user_chosen.require(&from)?;
    let path = allowlist::link_target(&from, &target)?;
    Ok(path_string(&state.user_chosen.remember(path)))
}

/// Opens a web or mail link in the default app. Anything else (file:, javascript:, …) is refused.
/// Async like every command that can block: Tauri runs non-async commands on the UI thread.
#[tauri::command]
async fn open_external(url: String) -> Result<(), String> {
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
    let path = state.user_chosen.require(&path)?;
    if state.watchers.lock().unwrap().contains_key(&path) {
        return Ok(());
    }
    let emitter = app.clone();
    let payload = path_string(&path);
    let watcher = watcher::watch(&path, move || {
        let _ = emitter.emit_to("main", "file-changed", &payload);
    })
    .map_err(|e| e.to_string())?;
    state.watchers.lock().unwrap().insert(path, watcher);
    Ok(())
}

/// No allowlist check: dropping a watch exposes nothing. Async because dropping a watcher waits
/// for its thread to stop.
#[tauri::command]
async fn unwatch_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let Ok(path) = std::path::absolute(&path) else { return Ok(()) };
    state.watchers.lock().unwrap().remove(&path);
    Ok(())
}

// ---- recent files (F9) ----

/// Existing recent files, most recent first. Listing them re-authorises them for opening.
#[tauri::command]
async fn recent_files(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let recent = store::load_recent(&config_dir(&app)?);
    Ok(recent
        .into_iter()
        .filter(|p| p.is_file())
        .map(|p| path_string(&state.user_chosen.remember(p)))
        .collect())
}

// ---- crash recovery (F13); the snapshot itself is read by `startup_state` ----

/// Replaces the recovery file with the given unsaved tabs; an empty list clears it.
#[tauri::command]
async fn snapshot_buffers(app: AppHandle, snapshots: Vec<Snapshot>) -> Result<(), String> {
    let dir = local_data_dir(&app)?;
    let result = if snapshots.is_empty() { store::clear_snapshots(&dir) } else { store::save_snapshots(&dir, &snapshots) };
    result.map_err(|e| e.to_string())
}

// ---- settings ----

#[tauri::command]
async fn save_settings(app: AppHandle, window: WebviewWindow, settings: serde_json::Value) -> Result<(), String> {
    if !settings.is_object() {
        return Err("settings must be a JSON object".into());
    }
    if settings.to_string().len() > SETTINGS_MAX_BYTES {
        return Err("settings are too large".into());
    }
    store::save_settings(&config_dir(&app)?, &settings).map_err(|e| e.to_string())?;
    apply_window_theme(&window, &settings);
    Ok(())
}

/// Title bar and native menu follow the theme setting; `system` (or anything else) follows Windows.
/// Done here rather than from JS so the webview needs no set-theme permission.
fn apply_window_theme(window: &WebviewWindow, settings: &serde_json::Value) {
    let _ = window.set_theme(window::saved_theme(settings));
}

/// An uncaught error or a failed background write in the webview, for marky.log.
#[tauri::command]
async fn log_error(message: String) {
    diagnostics::write(&format!("frontend: {message}"));
}

/// The user has gone idle or come back; Rust trims WebView2's memory while idle (memory.rs).
#[tauri::command]
fn set_idle(app: AppHandle, idle: bool) {
    memory::set_idle(&app, idle);
}

/// Menu ticks for toggle settings; the frontend sends them whenever one changes.
#[tauri::command]
fn set_menu_checks(app: AppHandle, checks: HashMap<String, bool>) {
    menu::set_checks(&app, &checks);
}

// ---- prompts & window lifecycle ----

/// Every message box: app title, modal to the window that asked.
fn message_dialog(window: &WebviewWindow, text: impl Into<String>, kind: MessageDialogKind) -> MessageDialogBuilder<Wry> {
    window.dialog().message(text).title(APP_NAME).kind(kind).parent(window)
}

#[tauri::command]
fn set_dirty(state: State<'_, AppState>, dirty: bool) {
    state.dirty.store(dirty, Ordering::Relaxed);
}

#[tauri::command]
async fn ask_unsaved_changes(window: WebviewWindow, file_name: String) -> UnsavedChoice {
    let result = message_dialog(&window, format!("Do you want to save changes to {file_name}?"), MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            SAVE_LABEL.into(),
            DONT_SAVE_LABEL.into(),
            CANCEL_LABEL.into(),
        ))
        .blocking_show_with_result();
    // The plugin reports every button, Esc and the X by label (Esc and the X as Cancel). The role
    // arms only guard against a plugin update that stops doing so.
    match result {
        MessageDialogResult::Custom(label) if label == SAVE_LABEL => UnsavedChoice::Save,
        MessageDialogResult::Custom(label) if label == DONT_SAVE_LABEL => UnsavedChoice::Discard,
        MessageDialogResult::Yes => UnsavedChoice::Save,
        MessageDialogResult::No => UnsavedChoice::Discard,
        // Closing the dialog with Esc or the title-bar X keeps the document open.
        _ => UnsavedChoice::Cancel,
    }
}

/// Esc and the title-bar X are reported as the last button: `dismiss` when given, otherwise
/// `deny`. A question whose `deny` destroys something must offer a harmless `dismiss`.
#[tauri::command]
async fn ask_choice(window: WebviewWindow, message: String, confirm: String, deny: String, dismiss: Option<String>) -> Choice {
    let buttons = match dismiss {
        Some(dismiss) => MessageDialogButtons::YesNoCancelCustom(confirm.clone(), deny.clone(), dismiss),
        None => MessageDialogButtons::OkCancelCustom(confirm.clone(), deny.clone()),
    };
    let result = message_dialog(&window, message, MessageDialogKind::Warning).buttons(buttons).blocking_show_with_result();
    // The role arm guards against a plugin update that stops reporting buttons by label;
    // anything unrecognised is the harmless Dismiss.
    match result {
        MessageDialogResult::Custom(label) if label == confirm => Choice::Confirm,
        MessageDialogResult::Custom(label) if label == deny => Choice::Deny,
        MessageDialogResult::Ok | MessageDialogResult::Yes => Choice::Confirm,
        _ => Choice::Dismiss,
    }
}

#[tauri::command]
async fn show_error(window: WebviewWindow, message: String) {
    message_dialog(&window, message, MessageDialogKind::Error).blocking_show();
}

/// Closes without raising CloseRequested again. The frontend has already resolved unsaved changes
/// and written the recovery file's final content, which keeps any recovery the user postponed.
#[tauri::command]
fn close_window(window: WebviewWindow) -> Result<(), String> {
    window.destroy().map_err(|e| e.to_string())
}

fn request_open(app: &AppHandle, path: PathBuf) {
    let path = app.state::<AppState>().user_chosen.remember(path);
    let _ = app.emit_to("main", "open-request", path_string(&path));
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
        WindowEvent::Resized(_) => memory::follow_minimized(window),
        _ => {}
    }
}

fn main() {
    let started = Instant::now();
    let cwd = std::env::current_dir().unwrap_or_default();
    let startup = first_file_arg(std::env::args().skip(1), &cwd);

    let state = AppState {
        started,
        user_chosen: UserChosenPaths::default(),
        startup_read: Mutex::new(None),
        dirty: AtomicBool::new(false),
        watchers: Mutex::default(),
        recent_list_writes: Mutex::default(),
    };
    if let Some(path) = startup {
        let path = state.user_chosen.remember(path);
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
        .setup(move |app| {
            if let Ok(dir) = local_data_dir(app.handle()) {
                diagnostics::init(&dir, &app.package_info().version.to_string());
            }
            // The window is created here, not from config, so its title bar and background
            // already match the saved theme when it first appears (no white flash).
            let settings = config_dir(app.handle()).map(|dir| store::load_settings(&dir)).unwrap_or_default();
            window::create_main(app.handle(), &settings)?;
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
            save_settings,
            set_menu_checks,
            set_idle,
            log_error,
            set_dirty,
            ask_unsaved_changes,
            ask_choice,
            show_error,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running Marky");
}
