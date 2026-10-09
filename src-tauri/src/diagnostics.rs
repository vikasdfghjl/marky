//! `marky.log` in the app's local data folder: panics, uncaught frontend errors and failed
//! background writes. Release builds abort on panic and have no console, so this is the only
//! trace a crash leaves. The file never leaves the machine; users attach it to bug reports.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FILE: &str = "marky.log";
const PREVIOUS_LOG_FILE: &str = "marky.previous.log";
/// Checked at startup; one previous file is kept.
const ROTATE_AT_BYTES: u64 = 512 * 1024;
/// A runaway error loop in the webview must not fill the disk.
const MAX_LINES_PER_SESSION: usize = 200;
const MAX_LINE_BYTES: usize = 4 * 1024;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LINES_WRITTEN: AtomicUsize = AtomicUsize::new(0);

/// Opens the log in `dir`, rotating an oversized one, and records panics there from now on.
pub fn init(dir: &Path, version: &str) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join(LOG_FILE);
    if fs::metadata(&path).is_ok_and(|m| m.len() > ROTATE_AT_BYTES) {
        let _ = fs::rename(&path, dir.join(PREVIOUS_LOG_FILE));
    }
    let _ = LOG_PATH.set(path);
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        write(&format!("panic in pid {} on thread '{}': {info}", std::process::id(), thread.name().unwrap_or("unnamed")));
    }));
    // The pid tells sessions apart: a second launch also starts, forwards its file and exits.
    write(&format!("started Marky {version} (pid {})", std::process::id()));
}

/// Appends one timestamped line. Best effort: logging must never fail the caller.
pub fn write(message: &str) {
    let Some(path) = LOG_PATH.get() else { return };
    if LINES_WRITTEN.fetch_add(1, Ordering::Relaxed) >= MAX_LINES_PER_SESSION {
        return;
    }
    let message = truncate(message.trim(), MAX_LINE_BYTES);
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else { return };
    let _ = writeln!(file, "{} {message}", utc_timestamp(SystemTime::now()));
}

fn truncate(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `2026-10-08T18:00:07Z`. UTC, so lines from different machines in a bug report compare directly.
fn utc_timestamp(time: SystemTime) -> String {
    let seconds = time.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (year, month, day) = civil_date(seconds / 86_400);
    let time_of_day = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time_of_day / 3600,
        time_of_day / 60 % 60,
        time_of_day % 60
    )
}

/// Year, month and day of a count of days since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_date(days_since_epoch: u64) -> (u64, u64, u64) {
    let days = days_since_epoch + 719_468; // shift the epoch to 0000-03-01
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153; // March = 0
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = era * 400 + year_of_era + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_utc_timestamps() {
        let at = |seconds| utc_timestamp(UNIX_EPOCH + Duration::from_secs(seconds));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z"); // leap day in a 400-year leap year
        assert_eq!(at(1_791_482_407), "2026-10-08T18:00:07Z");
        assert_eq!(at(4_102_444_799), "2099-12-31T23:59:59Z");
    }

    #[test]
    fn truncates_on_a_character_boundary() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("日本語", 4), "日");
    }
}
