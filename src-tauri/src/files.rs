//! File I/O for Markdown documents: UTF-8 decoding with BOM and line-ending
//! detection on read, and crash-safe atomic replacement on write.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    Lf,
    Crlf,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    pub bom: bool,
    pub line_ending: LineEnding,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFile {
    pub path: String,
    /// LF-normalised; the original ending is restored on save from `meta`.
    pub text: String,
    pub meta: FileMeta,
    pub mtime_ms: u64,
    /// The file was not valid UTF-8 and was decoded lossily; saving would corrupt it.
    pub read_only: bool,
}

pub fn read(path: &Path) -> io::Result<OpenedFile> {
    let bytes = fs::read(path)?;
    let (bom, body) = match bytes.strip_prefix(BOM) {
        Some(rest) => (true, rest),
        None => (false, &bytes[..]),
    };
    let (text, read_only) = match std::str::from_utf8(body) {
        Ok(s) => (s.to_owned(), false),
        Err(_) => (String::from_utf8_lossy(body).into_owned(), true),
    };

    // Mixed files are normalised to whichever ending dominates.
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    let line_ending = if crlf > lf { LineEnding::Crlf } else { LineEnding::Lf };
    let text = if crlf > 0 { text.replace("\r\n", "\n") } else { text };

    Ok(OpenedFile {
        path: path.to_string_lossy().into_owned(),
        text,
        meta: FileMeta { bom, line_ending },
        mtime_ms: mtime_ms(path)?,
        read_only,
    })
}

/// Encode `text` (LF) with the file's BOM and line ending, write it to a temp
/// file beside the target, flush it to disk, then atomically swap it in.
/// Returns the new mtime so the watcher can ignore our own write.
pub fn write(path: &Path, text: &str, meta: FileMeta) -> io::Result<u64> {
    let target = resolve_symlink(path)?;
    let bytes = encode(text, meta);

    let dir = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent folder"))?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.marky-{}.tmp", std::process::id()));

    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        replace(&target, &tmp)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    mtime_ms(&target)
}

fn encode(text: &str, meta: FileMeta) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + text.len() / 32 + BOM.len());
    if meta.bom {
        out.extend_from_slice(BOM);
    }
    match meta.line_ending {
        LineEnding::Lf => out.extend_from_slice(text.as_bytes()),
        LineEnding::Crlf => {
            for (i, line) in text.split('\n').enumerate() {
                if i > 0 {
                    out.extend_from_slice(b"\r\n");
                }
                out.extend_from_slice(line.as_bytes());
            }
        }
    }
    out
}

/// Saving through a symlink must update the link target, not replace the link.
fn resolve_symlink(path: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => fs::canonicalize(path),
        _ => Ok(path.to_path_buf()),
    }
}

/// A single rename-over is atomic on NTFS (std uses POSIX-semantics replace).
/// `ReplaceFileW` would keep ACLs and alternate data streams, but it renames the
/// original aside first, so a crash mid-call left no file at all (caught by the kill test).
fn replace(target: &Path, tmp: &Path) -> io::Result<()> {
    fs::rename(tmp, target)
}

pub fn mtime_ms(path: &Path) -> io::Result<u64> {
    let modified = fs::metadata(path)?.modified()?;
    Ok(modified.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One folder per test: tests run in parallel and must not see each other's temp files.
    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("marky-test-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn round_trips_bom_and_crlf_byte_for_byte() {
        let path = temp_path("crlf-bom.md");
        let original = b"\xEF\xBB\xBF# Title\r\n\r\n- a\r\n- b\r\n";
        fs::write(&path, original).unwrap();

        let opened = read(&path).unwrap();
        assert_eq!(opened.text, "# Title\n\n- a\n- b\n");
        assert_eq!(opened.meta, FileMeta { bom: true, line_ending: LineEnding::Crlf });
        assert!(!opened.read_only);

        write(&path, &opened.text, opened.meta).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn round_trips_plain_lf() {
        let path = temp_path("lf.md");
        fs::write(&path, "a\nb\n").unwrap();
        let opened = read(&path).unwrap();
        assert_eq!(opened.meta, FileMeta { bom: false, line_ending: LineEnding::Lf });
        write(&path, "a\nb\nc\n", opened.meta).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"a\nb\nc\n");
    }

    #[test]
    fn marks_invalid_utf8_read_only() {
        let path = temp_path("latin1.md");
        fs::write(&path, b"caf\xE9\n").unwrap();
        assert!(read(&path).unwrap().read_only);
    }

    /// 512 KB so each save takes long enough to be interrupted mid-write.
    fn save_loop_version(n: usize) -> String {
        n.to_string().repeat(512 * 1024)
    }

    const LF: FileMeta = FileMeta { bom: false, line_ending: LineEnding::Lf };
    const SAVE_LOOP_ENV: &str = "MARKY_SAVE_LOOP_PATH";

    /// Helper process for `killed_mid_save_leaves_old_or_new_content`: saves until killed.
    #[test]
    #[ignore = "spawned by killed_mid_save_leaves_old_or_new_content"]
    fn save_loop_until_killed() {
        let Some(path) = std::env::var_os(SAVE_LOOP_ENV) else { return };
        let path = PathBuf::from(path);
        let versions = [save_loop_version(0), save_loop_version(1)];
        for content in versions.iter().cycle() {
            write(&path, content, LF).unwrap();
        }
    }

    /// M1 exit gate: a process killed at any point during a save never leaves a torn file.
    #[test]
    fn killed_mid_save_leaves_old_or_new_content() {
        use std::process::{Command, Stdio};
        use std::time::Duration;

        let path = temp_path("kill.md");
        write(&path, &save_loop_version(0), LF).unwrap();
        let valid = [save_loop_version(0), save_loop_version(1)];

        for round in 0..15u64 {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "files::tests::save_loop_until_killed", "--ignored"])
                .env(SAVE_LOOP_ENV, &path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            // Vary the kill point so it lands at different phases of the write.
            std::thread::sleep(Duration::from_millis(150 + round * 17));
            child.kill().unwrap();
            child.wait().unwrap();

            let on_disk = fs::read_to_string(&path).unwrap();
            assert!(valid.contains(&on_disk), "round {round}: torn file ({} bytes)", on_disk.len());
        }
    }

    #[test]
    fn creates_new_file_and_leaves_no_temp_behind() {
        let path = temp_path("new.md");
        let _ = fs::remove_file(&path);
        write(&path, "hello\n", FileMeta { bom: false, line_ending: LineEnding::Crlf }).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello\r\n");
        let leftovers = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }
}
