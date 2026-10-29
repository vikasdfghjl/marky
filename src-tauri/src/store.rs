//! Small JSON files in the app's own folders: recent files (F9), settings, and the
//! crash-recovery snapshots of unsaved tabs (F13).

use crate::diagnostics;
use crate::files::{self, FileMeta};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const RECENT_FILE: &str = "recent.json";
const RECENT_LIMIT: usize = 10;
const SNAPSHOT_FILE: &str = "recovery.json";
const SETTINGS_FILE: &str = "settings.json";
const WINDOW_FILE: &str = "window.json";
const RECENT_MAX_BYTES: u64 = 64 * 1024;
const SETTINGS_MAX_BYTES: u64 = 16 * 1024;
const WINDOW_MAX_BYTES: u64 = 1024;
/// Refuse to load larger recovery files: parsing and IPC would dominate startup (see AGENTS.md).
const RECOVERY_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Last main-window bounds in logical pixels (`tauri.conf.json` defaults apply when missing).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowGeometry {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
    pub maximized: bool,
}

impl WindowGeometry {
    /// Rejects corrupt or absurd values from hand-edited JSON.
    pub fn is_usable(&self) -> bool {
        const MIN_W: f64 = 480.0;
        const MIN_H: f64 = 320.0;
        const MAX: f64 = 10_000.0;
        self.width >= MIN_W
            && self.height >= MIN_H
            && self.width <= MAX
            && self.height <= MAX
            && self.x.abs() <= MAX
            && self.y.abs() <= MAX
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Snapshot {
    /// None for an untitled document.
    pub path: Option<String>,
    pub text: String,
    pub meta: FileMeta,
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    max_bytes: usize,
}

impl LimitedBuffer {
    fn new(max_bytes: usize) -> Self {
        Self { bytes: Vec::new(), max_bytes }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl io::Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(size) = self.bytes.len().checked_add(bytes.len()) else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "recovery snapshots exceed the 8 MB limit"));
        };
        if size > self.max_bytes {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "recovery snapshots exceed the 8 MB limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Most recent first. A missing or corrupt list reads as empty rather than failing startup.
pub fn load_recent(config_dir: &Path) -> Vec<PathBuf> {
    read_json(&config_dir.join(RECENT_FILE), RECENT_MAX_BYTES).unwrap_or_default()
}

pub fn push_recent(config_dir: &Path, path: &Path) -> io::Result<()> {
    let mut recent = load_recent(config_dir);
    recent.retain(|p| !files::same_path(p, path));
    recent.insert(0, path.to_path_buf());
    recent.truncate(RECENT_LIMIT);
    write_json(&config_dir.join(RECENT_FILE), &recent)
}

/// The user's settings as the frontend wrote them (it owns the schema and fills defaults).
/// Missing or corrupt reads as `Null`, which means "all defaults".
pub fn load_settings(config_dir: &Path) -> serde_json::Value {
    read_json(&config_dir.join(SETTINGS_FILE), SETTINGS_MAX_BYTES).unwrap_or(serde_json::Value::Null)
}

pub fn save_settings(config_dir: &Path, settings: &serde_json::Value) -> io::Result<()> {
    write_json(&config_dir.join(SETTINGS_FILE), settings)
}

pub fn load_window_geometry(config_dir: &Path) -> Option<WindowGeometry> {
    read_json(&config_dir.join(WINDOW_FILE), WINDOW_MAX_BYTES).filter(WindowGeometry::is_usable)
}

pub fn save_window_geometry(config_dir: &Path, geometry: &WindowGeometry) -> io::Result<()> {
    if !geometry.is_usable() {
        return Ok(());
    }
    write_json(&config_dir.join(WINDOW_FILE), geometry)
}

pub fn save_snapshots(data_dir: &Path, snapshots: &[Snapshot]) -> io::Result<()> {
    let mut buffer = LimitedBuffer::new(RECOVERY_MAX_BYTES as usize);
    serde_json::to_writer(&mut buffer, snapshots).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("could not serialize recovery snapshots within the 8 MB limit: {e}; the existing recovery file was kept"),
        )
    })?;
    fs::create_dir_all(data_dir)?;
    files::write_atomically(&data_dir.join(SNAPSHOT_FILE), &buffer.into_inner())
}

/// Empty when there is nothing to recover. Also reads the single-snapshot format written before tabs.
pub fn read_snapshots(data_dir: &Path) -> Vec<Snapshot> {
    let path = data_dir.join(SNAPSHOT_FILE);
    if let Ok(meta) = fs::metadata(&path) {
        let size = meta.len();
        if size > RECOVERY_MAX_BYTES {
            let mb = size.div_ceil(1024 * 1024);
            let limit = RECOVERY_MAX_BYTES / (1024 * 1024);
            diagnostics::write(&format!("recovery file is {mb} MB; ignoring it (limit {limit} MB)"));
            return vec![];
        }
    }
    if let Some(snapshots) = read_json::<Vec<Snapshot>>(&path, RECOVERY_MAX_BYTES) {
        return snapshots;
    }
    read_json::<Snapshot>(&path, RECOVERY_MAX_BYTES).into_iter().collect()
}

pub fn clear_snapshots(data_dir: &Path) -> io::Result<()> {
    match fs::remove_file(data_dir.join(SNAPSHOT_FILE)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn read_json<T: DeserializeOwned>(path: &Path, max_bytes: u64) -> Option<T> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::with_capacity((max_bytes + 1).min(RECOVERY_MAX_BYTES) as usize);
    file.take(max_bytes + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > max_bytes {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    files::write_atomically(path, &serde_json::to_vec(value).map_err(io::Error::other)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::LineEnding;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("marky-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn recent_is_deduplicated_newest_first_and_capped() {
        let dir = temp_dir("recent");
        for i in 0..12 {
            push_recent(&dir, Path::new(&format!("C:\\docs\\{i}.md"))).unwrap();
        }
        push_recent(&dir, Path::new("c:\\DOCS\\5.md")).unwrap();

        let recent = load_recent(&dir);
        assert_eq!(recent.len(), RECENT_LIMIT);
        assert_eq!(recent[0], Path::new("c:\\DOCS\\5.md"));
        assert_eq!(recent[1], Path::new("C:\\docs\\11.md"));
        assert!(!recent[1..].iter().any(|p| p.as_os_str().eq_ignore_ascii_case("C:\\docs\\5.md")));
    }

    #[test]
    fn corrupt_recent_list_reads_as_empty() {
        let dir = temp_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(RECENT_FILE), b"{not json").unwrap();
        assert!(load_recent(&dir).is_empty());
    }

    #[test]
    fn snapshots_round_trip_and_clear() {
        let dir = temp_dir("snapshot");
        let snapshot = |path: Option<&str>, text: &str| Snapshot {
            path: path.map(Into::into),
            text: text.into(),
            meta: FileMeta { bom: true, line_ending: LineEnding::Crlf },
        };
        let snapshots = vec![snapshot(Some("C:\\docs\\a.md"), "unsaved\n"), snapshot(None, "untitled\n")];
        save_snapshots(&dir, &snapshots).unwrap();
        assert_eq!(read_snapshots(&dir), snapshots);

        clear_snapshots(&dir).unwrap();
        assert!(read_snapshots(&dir).is_empty());
        clear_snapshots(&dir).unwrap();
    }

    #[test]
    fn oversized_snapshots_do_not_replace_the_existing_recovery_file() {
        let dir = temp_dir("oversized-snapshots");
        let existing = Snapshot {
            path: Some("C:\\docs\\a.md".into()),
            text: "recoverable\n".into(),
            meta: FileMeta { bom: false, line_ending: LineEnding::Lf },
        };
        save_snapshots(&dir, &[existing]).unwrap();

        let oversized = Snapshot {
            path: None,
            text: "x".repeat(RECOVERY_MAX_BYTES as usize),
            meta: FileMeta { bom: false, line_ending: LineEnding::Lf },
        };
        let error = save_snapshots(&dir, &[oversized]).unwrap_err();

        assert!(error.to_string().contains("exceed the 8 MB limit"));
        assert_eq!(read_snapshots(&dir)[0].text, "recoverable\n");
    }

    #[test]
    fn settings_round_trip_and_default_to_null() {
        let dir = temp_dir("settings");
        assert_eq!(load_settings(&dir), serde_json::Value::Null);
        let settings = serde_json::json!({ "theme": "dark", "fontSize": 15 });
        save_settings(&dir, &settings).unwrap();
        assert_eq!(load_settings(&dir), settings);
    }

    #[test]
    fn oversized_app_json_files_fall_back_to_defaults() {
        let dir = temp_dir("oversized-app-json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(RECENT_FILE), vec![b' '; RECENT_MAX_BYTES as usize + 1]).unwrap();
        fs::write(dir.join(SETTINGS_FILE), vec![b' '; SETTINGS_MAX_BYTES as usize + 1]).unwrap();
        fs::write(dir.join(WINDOW_FILE), vec![b' '; WINDOW_MAX_BYTES as usize + 1]).unwrap();

        assert!(load_recent(&dir).is_empty());
        assert_eq!(load_settings(&dir), serde_json::Value::Null);
        assert!(load_window_geometry(&dir).is_none());
    }

    #[test]
    fn reads_single_snapshot_written_before_tabs() {
        let dir = temp_dir("legacy");
        let single = Snapshot { path: None, text: "old\n".into(), meta: FileMeta { bom: false, line_ending: LineEnding::Lf } };
        write_json(&dir.join(SNAPSHOT_FILE), &single).unwrap();
        assert_eq!(read_snapshots(&dir), vec![single]);
    }

    #[test]
    fn window_geometry_round_trips_and_rejects_invalid() {
        let dir = temp_dir("window");
        let ok = WindowGeometry { width: 1000.0, height: 700.0, x: 40.0, y: 80.0, maximized: false };
        save_window_geometry(&dir, &ok).unwrap();
        assert_eq!(load_window_geometry(&dir), Some(ok.clone()));

        let bad = WindowGeometry { width: 100.0, height: 700.0, x: 0.0, y: 0.0, maximized: false };
        save_window_geometry(&dir, &bad).unwrap();
        assert_eq!(load_window_geometry(&dir), Some(ok));

        save_window_geometry(&dir, &WindowGeometry { width: 1200.0, height: 800.0, x: 0.0, y: 0.0, maximized: true }).unwrap();
        assert!(load_window_geometry(&dir).unwrap().maximized);
    }

    #[test]
    fn oversized_recovery_file_reads_as_empty() {
        use std::io::Write;

        let dir = temp_dir("huge-recovery");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SNAPSHOT_FILE);
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(b" ").unwrap();
        file.set_len(RECOVERY_MAX_BYTES + 1).unwrap();
        assert!(read_snapshots(&dir).is_empty());
    }
}
