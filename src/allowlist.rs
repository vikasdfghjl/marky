//! Where a relative link in a document may point: only an existing .md, .markdown or .txt file.

use std::path::{Path, PathBuf};

use crate::files::MARKDOWN_EXTS;

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
