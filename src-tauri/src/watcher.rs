//! Watches the open file for changes made by other programs (git, AI agents, other editors).

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;

/// Calls `on_change` for every create/modify/remove event touching `path`.
/// Events arrive in bursts; the frontend debounces and compares mtimes.
///
/// Watches the parent folder rather than the file: saves (ours and most editors')
/// replace the file by rename, which would orphan a watch on the file itself.
pub fn watch(path: &Path, on_change: impl Fn() + Send + 'static) -> notify::Result<RecommendedWatcher> {
    let target = path.to_path_buf();
    let folder = path
        .parent()
        .ok_or_else(|| notify::Error::generic("file has no parent folder"))?
        .to_path_buf();

    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        if event.paths.iter().any(|p| is_same_path(p, &target)) {
            on_change();
        }
    })?;
    watcher.watch(&folder, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

/// Windows paths are case-insensitive.
fn is_same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}
