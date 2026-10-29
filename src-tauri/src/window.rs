//! The main window, created in code (`"create": false` in tauri.conf.json) so it can start
//! with the app's background colour. A config-created window shows WebView2's default white
//! for the ~650 ms until the page paints, which is a flash on every launch in dark mode.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::window::Color;
use tauri::{AppHandle, Manager, Theme, WebviewWindow, WebviewWindowBuilder};

use crate::store::{self, WindowGeometry};

// Must match --bg in styles.css, the colour the page paints first.
const DARK_BACKGROUND: Color = Color(0x1e, 0x1f, 0x22, 0xff);
const LIGHT_BACKGROUND: Color = Color(0xff, 0xff, 0xff, 0xff);

static GEOMETRY_SAVE_TICK: AtomicU64 = AtomicU64::new(0);

fn browser_args(configured: Option<&str>, additional: Option<&str>) -> Option<String> {
    let args = [configured, additional]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|args| !args.is_empty())
        .collect::<Vec<_>>();
    (!args.is_empty()).then(|| args.join(" "))
}

/// The theme forced by the saved settings; `None` (`system` or anything else) follows Windows.
pub(crate) fn saved_theme(settings: &serde_json::Value) -> Option<Theme> {
    match settings.get("theme").and_then(|t| t.as_str()) {
        Some("light") => Some(Theme::Light),
        Some("dark") => Some(Theme::Dark),
        _ => None,
    }
}

pub fn create_main(app: &AppHandle, settings: &serde_json::Value, config_dir: &Path) -> tauri::Result<WebviewWindow> {
    let forced = saved_theme(settings);
    let dark = match forced {
        Some(theme) => theme == Theme::Dark,
        None => system_prefers_dark(),
    };
    let config = app.config().app.windows.first().cloned().ok_or(tauri::Error::WindowNotFound)?;
    let additional_browser_args = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").ok();
    let browser_args = browser_args(
        config.additional_browser_args.as_deref(),
        additional_browser_args.as_deref(),
    );
    let mut builder = WebviewWindowBuilder::from_config(app, &config)?
        .theme(forced)
        .background_color(if dark { DARK_BACKGROUND } else { LIGHT_BACKGROUND });
    if let Some(args) = browser_args.as_deref() {
        builder = builder.additional_browser_args(args);
    }
    if let Some(geo) = store::load_window_geometry(config_dir) {
        if geo.maximized {
            builder = builder.maximized(true);
        } else {
            builder = builder.inner_size(geo.width, geo.height).position(geo.x, geo.y);
        }
    }
    builder.build()
}

/// Writes the current bounds to `window.json` (debounced after resize/move).
pub fn schedule_geometry_save(app: &AppHandle, config_dir: PathBuf) {
    let tick = GEOMETRY_SAVE_TICK.fetch_add(1, Ordering::Relaxed) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        std::thread::sleep(Duration::from_millis(500));
        if GEOMETRY_SAVE_TICK.load(Ordering::Relaxed) != tick {
            return;
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = persist_geometry(&window, &config_dir);
        }
    });
}

pub fn persist_geometry(window: &WebviewWindow, config_dir: &Path) -> io::Result<()> {
    let geometry = capture_geometry(window)?;
    store::save_window_geometry(config_dir, &geometry)
}

fn capture_geometry(window: &WebviewWindow) -> io::Result<WindowGeometry> {
    let scale = window.scale_factor().map_err(io::Error::other)?;
    let size = window.inner_size().map_err(io::Error::other)?.to_logical(scale);
    let position = window.outer_position().map_err(io::Error::other)?.to_logical(scale);
    Ok(WindowGeometry {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized: window.is_maximized().unwrap_or(false),
    })
}

/// The Windows "app mode" setting. Read from the registry because no window exists yet to ask.
fn system_prefers_dark() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|uses_light| uses_light == 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::browser_args;

    #[test]
    fn browser_args_keep_defaults_and_append_environment_overrides() {
        assert_eq!(
            browser_args(
                Some("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --enable-features=NetworkServiceInProcess2"),
                Some("--remote-debugging-port=9222"),
            ),
            Some("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --enable-features=NetworkServiceInProcess2 --remote-debugging-port=9222".into())
        );
    }

    #[test]
    fn browser_args_ignore_empty_values() {
        assert_eq!(browser_args(Some("  "), Some("")), None);
    }
}
