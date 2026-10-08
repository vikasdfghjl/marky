//! What the webview may touch: only files the user chose (dialog, drag-drop, CLI arg, Explorer,
//! recent files, crash recovery or a followed link), so a compromised preview cannot reach others.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::files::MARKDOWN_EXTS;

#[derive(Default)]
pub struct UserChosenPaths {
    paths: Mutex<HashSet<PathBuf>>,
}

impl UserChosenPaths {
    /// Returns the path made absolute, the form `require` compares against.
    pub fn remember(&self, path: PathBuf) -> PathBuf {
        let path = std::path::absolute(&path).unwrap_or(path);
        self.paths.lock().unwrap().insert(path.clone());
        path
    }

    /// Exact match, case included: a folder with per-directory case sensitivity (WSL) can hold
    /// both `a.md` and `A.md`, so folding case could authorise a file the user never chose.
    pub fn require(&self, path: &str) -> Result<PathBuf, String> {
        let path = std::path::absolute(path).map_err(|e| e.to_string())?;
        if !self.paths.lock().unwrap().contains(&path) {
            return Err("path was not selected by the user".into());
        }
        Ok(path)
    }
}

/// Where a relative link in the document `from` points, if Marky may follow it: an existing
/// .md, .markdown or .txt file. Clicking it is the user's choice, so the caller remembers it.
pub fn link_target(from: &Path, target: &str) -> Result<PathBuf, String> {
    let folder = from.parent().ok_or("the document has no folder")?;
    // absolute() normalises `.` and `..` on Windows.
    let path = std::path::absolute(folder.join(target)).map_err(|e| e.to_string())?;
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !MARKDOWN_EXTS.contains(&extension.as_str()) {
        return Err(format!("Marky follows links to .md, .markdown and .txt files only: {target}"));
    }
    if !path.is_file() {
        return Err(format!("Linked file not found: {}", path.to_string_lossy()));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("marky-allowlist-{}-{name}", std::process::id()));
        fs::create_dir_all(dir.join("sub")).unwrap();
        dir
    }

    #[test]
    fn refuses_paths_the_user_never_chose() {
        let chosen = UserChosenPaths::default();
        chosen.remember(PathBuf::from(r"C:\docs\a.md"));
        assert!(chosen.require(r"C:\Windows\win.ini").is_err());
        assert!(chosen.require(r"C:\docs\b.md").is_err());
    }

    #[test]
    fn accepts_a_chosen_path_however_it_is_spelled() {
        let chosen = UserChosenPaths::default();
        let remembered = chosen.remember(PathBuf::from(r"C:\docs\a.md"));
        assert_eq!(chosen.require(r"C:\docs\a.md").unwrap(), remembered);
        assert_eq!(chosen.require(r"C:\docs\sub\..\.\a.md").unwrap(), remembered);
    }

    #[test]
    fn treats_a_different_case_as_a_different_file() {
        let chosen = UserChosenPaths::default();
        chosen.remember(PathBuf::from(r"C:\docs\a.md"));
        assert!(chosen.require(r"C:\docs\A.md").is_err());
    }

    #[test]
    fn follows_links_to_existing_markdown_files() {
        let dir = temp_dir("follow");
        fs::write(dir.join("other.md"), "").unwrap();
        fs::write(dir.join("sub").join("notes.TXT"), "").unwrap();
        let from = dir.join("sub").join("doc.md");
        assert_eq!(link_target(&from, "../other.md").unwrap(), dir.join("other.md"));
        assert_eq!(link_target(&from, "notes.TXT").unwrap(), dir.join("sub").join("notes.TXT"));
    }

    #[test]
    fn refuses_other_link_targets() {
        let dir = temp_dir("refuse");
        fs::write(dir.join("secret.ini"), "").unwrap();
        let from = dir.join("doc.md");
        assert!(link_target(&from, "secret.ini").unwrap_err().contains(".md, .markdown and .txt files only"));
        assert!(link_target(&from, "missing.md").unwrap_err().starts_with("Linked file not found"));
    }
}
