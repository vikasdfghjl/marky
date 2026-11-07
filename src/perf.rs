//! Timing lines in the format `scripts/bench.ps1` reads from `MARKY_PERF_LOG`.
//! A release build has no console, so the file is the only place they can go.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static STARTED: OnceLock<Instant> = OnceLock::new();
static MARKS: Mutex<Vec<(&'static str, f64)>> = Mutex::new(Vec::new());
static STARTUP: OnceLock<f64> = OnceLock::new();

/// Call first thing in `main`: every mark and `startup_ms` count from here, like the app's.
pub fn start() -> Instant {
    *STARTED.get_or_init(Instant::now)
}

pub fn since_start_ms() -> f64 {
    start().elapsed().as_secs_f64() * 1000.0
}

/// Records a startup phase. The marks are written on the `startup_ms` line, like the app's `js:` marks.
pub fn mark(name: &'static str) {
    let ms = since_start_ms();
    if let Ok(mut marks) = MARKS.lock() {
        marks.push((name, ms));
    }
}

/// `startup_ms=651 marks: read=1 gpui=40 window=310 render=330 painted=651`
pub fn log_startup() {
    let total = since_start_ms();
    let _ = STARTUP.set(total);
    let marks = MARKS.lock().map(|marks| marks.iter().map(|(name, ms)| format!("{name}={ms:.0}")).collect::<Vec<_>>().join(" "));
    log(&format!("startup_ms={total:.0} marks: {}", marks.unwrap_or_default()));
}

pub fn log(line: &str) {
    eprintln!("{line}");
    let Some(path) = std::env::var_os("MARKY_PERF_LOG") else { return };
    let file = OpenOptions::new().create(true).append(true).open(path);
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{line}");
    }
}

/// The start-up time once the first frame has painted, for the status bar (the app shows it too).
pub fn startup_ms() -> Option<f64> {
    STARTUP.get().copied()
}
