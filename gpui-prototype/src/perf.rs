//! Timing lines in the format `scripts/bench.ps1` reads from `MARKY_PERF_LOG`.
//! A release build has no console, so the file is the only place they can go.

use std::fs::OpenOptions;
use std::io::Write;

pub fn log(line: &str) {
    eprintln!("{line}");
    let Some(path) = std::env::var_os("MARKY_PERF_LOG") else { return };
    let file = OpenOptions::new().create(true).append(true).open(path);
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{line}");
    }
}
