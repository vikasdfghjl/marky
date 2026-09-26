//! Small JSON files in the app's own folders: the recent-files list (F9) and the
//! crash-recovery snapshot of an unsaved buffer (F13).

use crate::files::FileMeta;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const RECENT_FILE: &str = "recent.json";
const RECENT_LIMIT: usize = 10;
const SNAPSHOT_FILE: &str = "recovery.json";

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// None for an untitled document.
    pub path: Option<String>,
    pub text: String,
    pub meta: FileMeta,
}

/// Most recent first. A missing or corrupt list reads as empty rather than failing startup.
pub fn load_recent(config_dir: &Path) -> Vec<PathBuf> {
    read_json(&config_dir.join(RECENT_FILE)).unwrap_or_default()
}

pub fn push_recent(config_dir: &Path, path: &Path) -> io::Result<()> {
    let mut recent = load_recent(config_dir);
    recent.retain(|p| !p.as_os_str().eq_ignore_ascii_case(path.as_os_str()));
    recent.insert(0, path.to_path_buf());
    recent.truncate(RECENT_LIMIT);
    write_json(&config_dir.join(RECENT_FILE), &recent)
}

pub fn save_snapshot(data_dir: &Path, snapshot: &Snapshot) -> io::Result<()> {
    write_json(&data_dir.join(SNAPSHOT_FILE), snapshot)
}

pub fn read_snapshot(data_dir: &Path) -> Option<Snapshot> {
    read_json(&data_dir.join(SNAPSHOT_FILE))
}

pub fn clear_snapshot(data_dir: &Path) -> io::Result<()> {
    match fs::remove_file(data_dir.join(SNAPSHOT_FILE)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Temp file + rename so a crash never leaves half-written JSON.
fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent folder"))?;
    fs::create_dir_all(folder)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(value).map_err(io::Error::other)?)?;
    fs::rename(&tmp, path)
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
    fn snapshot_round_trips_and_clears() {
        let dir = temp_dir("snapshot");
        let snapshot = Snapshot {
            path: Some("C:\\docs\\a.md".into()),
            text: "unsaved\n".into(),
            meta: FileMeta { bom: true, line_ending: LineEnding::Crlf },
        };
        save_snapshot(&dir, &snapshot).unwrap();
        assert_eq!(read_snapshot(&dir), Some(snapshot));

        clear_snapshot(&dir).unwrap();
        assert_eq!(read_snapshot(&dir), None);
        clear_snapshot(&dir).unwrap();
    }
}
