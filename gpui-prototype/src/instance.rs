//! One prototype per user, as the app's single-instance plugin: a second launch writes the file it
//! was given to `open-request.txt` and exits; the running window watches that file and opens a tab.
//!
//! The lock is an exclusive handle on a file (`share_mode(0)`), not a marker file that could be left
//! behind by a crash: Windows drops the handle when the process ends, however it ends.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::files;

const LOCK_FILE: &str = "instance.lock";
const REQUEST_FILE: &str = "open-request.txt";

pub enum Start {
    /// The first launch; keep the handle open for as long as the window runs.
    Primary(File),
    /// Another launch holds the lock. Forward the argument and exit.
    Secondary,
}

pub fn request_path(dir: &Path) -> PathBuf {
    dir.join(REQUEST_FILE)
}

pub fn start(dir: &Path) -> io::Result<Start> {
    fs::create_dir_all(dir)?;
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // No sharing: a second open fails while this handle lives, and only while it lives.
        options.share_mode(0);
    }
    match options.open(dir.join(LOCK_FILE)) {
        Ok(file) => Ok(Start::Primary(file)),
        // ERROR_SHARING_VIOLATION: another process holds the handle. Access denied is a real
        // problem with the folder, not a second launch, so it is reported instead.
        Err(err) if err.raw_os_error() == Some(32) => Ok(Start::Secondary),
        Err(err) => Err(err),
    }
}

/// Writes the path (or an empty request, which just brings the window forward) for the first instance.
pub fn forward(dir: &Path, path: Option<&Path>) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let text = path.map(|path| path.to_string_lossy().into_owned()).unwrap_or_default();
    files::write_atomically(&request_path(dir), text.as_bytes())
}

/// Reads and removes a pending request. `Some(None)` means "focus the window".
pub fn take_request(dir: &Path) -> Option<Option<PathBuf>> {
    let request = request_path(dir);
    let text = fs::read_to_string(&request).ok()?;
    let _ = fs::remove_file(&request);
    let text = text.trim();
    Some((!text.is_empty()).then(|| PathBuf::from(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("marky-gpui-instance-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_first_start_holds_the_lock_and_the_second_is_secondary() {
        let dir = temp_dir("lock");
        let first = start(&dir);
        assert!(matches!(first, Ok(Start::Primary(_))));
        assert!(matches!(start(&dir), Ok(Start::Secondary)));
        drop(first);
        assert!(matches!(start(&dir), Ok(Start::Primary(_))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_forwarded_path_is_taken_once() {
        let dir = temp_dir("request");
        forward(&dir, Some(Path::new(r"C:\docs\a.md"))).unwrap();
        assert_eq!(take_request(&dir), Some(Some(PathBuf::from(r"C:\docs\a.md"))));
        assert_eq!(take_request(&dir), None);
        forward(&dir, None).unwrap();
        assert_eq!(take_request(&dir), Some(None));
        let _ = fs::remove_dir_all(&dir);
    }
}
