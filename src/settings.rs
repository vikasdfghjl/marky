//! View settings in Marky's `settings.json` schema. A first run of the GPUI app can start from the
//! old prototype's file; changes are always written to the app's own folder. Every key this app does
//! not use is kept as it was read.

use std::fs;
use std::path::{Path, PathBuf};

use gpui::WindowAppearance;
use serde_json::{Map, Value};

use crate::files::{self, LineEnding};

const FILE_NAME: &str = "settings.json";
const MAX_BYTES: u64 = 16 * 1024;
pub const FONT_SIZES: [f32; 6] = [13.0, 14.0, 15.0, 16.0, 18.0, 20.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    Light,
    Dark,
    System,
}

impl ThemeChoice {
    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::System,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewSettings {
    pub theme: ThemeChoice,
    pub font_size: f32,
    pub word_wrap: bool,
    pub show_editor: bool,
    pub show_preview: bool,
    pub show_outline: bool,
    /// Smart quotes, dashes and ellipses in the preview (markdown-it's `typographer`). On by default, as in the app.
    pub typographer: bool,
    /// Save a document about a second after typing stops. Off by default (PRD F13).
    pub auto_save: bool,
    /// The line ending of New Tab documents. Existing files keep their own.
    pub new_file_line_ending: LineEnding,
    /// Keys this window does not use, written back unchanged.
    rest: Map<String, Value>,
    /// An oversized or malformed own file is shown with defaults and not overwritten.
    save: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::System,
            font_size: 15.0,
            word_wrap: true,
            show_editor: true,
            show_preview: true,
            show_outline: false,
            typographer: true,
            auto_save: false,
            new_file_line_ending: LineEnding::Crlf,
            rest: Map::new(),
            save: true,
        }
    }
}

impl ViewSettings {
    /// `System` follows the window's appearance, which GPUI reports and updates on its own.
    pub fn is_dark(&self, appearance: WindowAppearance) -> bool {
        match self.theme {
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
            ThemeChoice::System => matches!(appearance, WindowAppearance::Dark | WindowAppearance::VibrantDark),
        }
    }

    /// The app's rule (settings.ts): with the editor hidden, the preview shows even if `showPreview` is off.
    pub fn preview_visible(&self) -> bool {
        self.show_preview || !self.show_editor
    }
}

fn roaming() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// Where Marky keeps `settings.json`, `recent.json` and the single-instance lock.
pub fn own_config_dir() -> PathBuf {
    roaming().join("dev.marky.editor")
}

/// The GPUI prototype's folder, read once so its settings survive the migration. Never written.
pub fn legacy_config_dir() -> PathBuf {
    roaming().join("dev.marky.gpui-prototype")
}

/// Crash-recovery snapshots and the log.
pub fn own_local_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("dev.marky.editor")
}

pub fn load(config_dir: &Path, legacy_dir: &Path) -> ViewSettings {
    let root = match read_object(&config_dir.join(FILE_NAME)) {
        Read::Object(root) => root,
        Read::LeaveAlone => return ViewSettings { save: false, ..ViewSettings::default() },
        Read::Missing => match read_object(&legacy_dir.join(FILE_NAME)) {
            Read::Object(root) => root,
            Read::Missing => return ViewSettings::default(),
            Read::LeaveAlone => return ViewSettings { save: false, ..ViewSettings::default() },
        },
    };
    let defaults = ViewSettings::default();
    ViewSettings {
        theme: ThemeChoice::parse(root.get("theme").and_then(Value::as_str)),
        font_size: font_size_of(root.get("fontSize").and_then(Value::as_f64)),
        word_wrap: root.get("wordWrap").and_then(Value::as_bool).unwrap_or(defaults.word_wrap),
        show_editor: root.get("showEditor").and_then(Value::as_bool).unwrap_or(defaults.show_editor),
        show_preview: root.get("showPreview").and_then(Value::as_bool).unwrap_or(defaults.show_preview),
        show_outline: root.get("showOutline").and_then(Value::as_bool).unwrap_or(defaults.show_outline),
        typographer: root.get("typographer").and_then(Value::as_bool).unwrap_or(defaults.typographer),
        auto_save: root.get("autoSave").and_then(Value::as_bool).unwrap_or(defaults.auto_save),
        new_file_line_ending: match root.get("newFileLineEnding").and_then(Value::as_str) {
            Some("lf") => LineEnding::Lf,
            Some("crlf") => LineEnding::Crlf,
            _ => defaults.new_file_line_ending,
        },
        rest: root,
        save: true,
    }
}

/// Returns why the settings could not be saved, for the status line.
pub fn save(config_dir: &Path, settings: &ViewSettings) -> Result<(), String> {
    if !settings.save {
        return Ok(());
    }
    let mut root = settings.rest.clone();
    root.insert("theme".into(), Value::String(settings.theme.as_str().into()));
    root.insert("fontSize".into(), serde_json::json!(settings.font_size));
    root.insert("wordWrap".into(), Value::Bool(settings.word_wrap));
    root.insert("showEditor".into(), Value::Bool(settings.show_editor));
    root.insert("showPreview".into(), Value::Bool(settings.show_preview));
    root.insert("showOutline".into(), Value::Bool(settings.show_outline));
    root.insert("typographer".into(), Value::Bool(settings.typographer));
    root.insert("autoSave".into(), Value::Bool(settings.auto_save));
    root.insert(
        "newFileLineEnding".into(),
        Value::String(match settings.new_file_line_ending {
            LineEnding::Lf => "lf",
            LineEnding::Crlf => "crlf",
        }
        .into()),
    );
    let bytes = serde_json::to_vec_pretty(&Value::Object(root)).map_err(|err| format!("could not encode settings: {err}"))?;
    fs::create_dir_all(config_dir).map_err(|err| format!("could not create {}: {err}", config_dir.display()))?;
    files::write_atomically(&config_dir.join(FILE_NAME), &bytes).map_err(|err| format!("could not save settings: {err}"))
}

fn font_size_of(value: Option<f64>) -> f32 {
    let Some(value) = value else { return 15.0 };
    FONT_SIZES.into_iter().find(|size| (*size - value as f32).abs() < 0.1).unwrap_or(15.0)
}

enum Read {
    Missing,
    /// Present, but too large or not an object. Do not overwrite it.
    LeaveAlone,
    Object(Map<String, Value>),
}

fn read_object(path: &Path) -> Read {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Read::Missing,
    };
    if bytes.len() as u64 > MAX_BYTES {
        return Read::LeaveAlone;
    }
    match serde_json::from_slice(&bytes) {
        Ok(Value::Object(map)) => Read::Object(map),
        _ => Read::LeaveAlone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("marky-gpui-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn starts_from_the_prototype_file_and_saves_only_to_the_apps_own() {
        let legacy = scratch("legacy");
        let own = scratch("own");
        let legacy_json = br#"{"theme":"light","autoSave":true,"newFileLineEnding":"lf","fontSize":15}"#;
        fs::write(legacy.join(FILE_NAME), legacy_json).unwrap();

        let mut settings = load(&own, &legacy);
        assert_eq!(settings.theme, ThemeChoice::Light);
        assert!(settings.auto_save);
        assert_eq!(settings.new_file_line_ending, LineEnding::Lf);
        assert!(!settings.is_dark(WindowAppearance::Dark));
        settings.theme = ThemeChoice::Dark;
        settings.show_outline = true;
        save(&own, &settings).unwrap();

        assert_eq!(fs::read(legacy.join(FILE_NAME)).unwrap(), legacy_json);
        let saved: Value = serde_json::from_slice(&fs::read(own.join(FILE_NAME)).unwrap()).unwrap();
        assert_eq!(saved["theme"], "dark");
        assert_eq!(saved["autoSave"], true);
        assert_eq!(saved["newFileLineEnding"], "lf");
        assert_eq!(saved["showOutline"], true);
        assert_eq!(load(&own, &legacy).theme, ThemeChoice::Dark);
        let _ = fs::remove_dir_all(&legacy);
        let _ = fs::remove_dir_all(&own);
    }

    #[test]
    fn hiding_the_editor_keeps_the_preview_without_changing_the_setting() {
        let settings = ViewSettings { show_editor: false, show_preview: false, ..ViewSettings::default() };
        assert!(settings.preview_visible());
        assert!(!settings.show_preview);
    }
}
