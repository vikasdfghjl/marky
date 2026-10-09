//! The main window, created in code (`"create": false` in tauri.conf.json) so it can start
//! with the app's background colour. A config-created window shows WebView2's default white
//! for the ~650 ms until the page paints, which is a flash on every launch in dark mode.

use tauri::window::Color;
use tauri::{AppHandle, Theme, WebviewWindow, WebviewWindowBuilder};

// Must match --bg in styles.css, the colour the page paints first.
const DARK_BACKGROUND: Color = Color(0x1e, 0x1f, 0x22, 0xff);
const LIGHT_BACKGROUND: Color = Color(0xff, 0xff, 0xff, 0xff);

/// The theme forced by the saved settings; `None` (`system` or anything else) follows Windows.
pub fn saved_theme(settings: &serde_json::Value) -> Option<Theme> {
    match settings.get("theme").and_then(|t| t.as_str()) {
        Some("light") => Some(Theme::Light),
        Some("dark") => Some(Theme::Dark),
        _ => None,
    }
}

pub fn create_main(app: &AppHandle, settings: &serde_json::Value) -> tauri::Result<WebviewWindow> {
    let forced = saved_theme(settings);
    let dark = match forced {
        Some(theme) => theme == Theme::Dark,
        None => system_prefers_dark(),
    };
    let config = app.config().app.windows.first().cloned().ok_or(tauri::Error::WindowNotFound)?;
    WebviewWindowBuilder::from_config(app, &config)?
        .theme(forced)
        .background_color(if dark { DARK_BACKGROUND } else { LIGHT_BACKGROUND })
        .build()
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
