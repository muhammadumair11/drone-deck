//! Bounds the telemetry captures in `<data root>\logs\tlog`: every link writes a new `.tlog`
//! of up to 2 GB, so before a link opens the oldest captures go once the folder holds more
//! than [`KEEP`] of them or more than [`BUDGET`] bytes. Only `.tlog` files directly in that
//! folder are touched; nothing else there is read or removed.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Captures kept, the one about to be written included.
pub(crate) const KEEP: usize = 100;
/// Bytes the kept captures may take together, before the new one starts.
pub(crate) const BUDGET: u64 = 2_000_000_000;

/// Deletes the oldest `.tlog` files in `dir` until at most `keep - 1` remain (room for the
/// capture about to start) and they total at most `budget` bytes. Returns the paths removed;
/// a file that can't be removed is left and skipped.
pub(crate) fn prune(dir: &Path, keep: usize, budget: u64) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut captures: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let tlog = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tlog"));
            let meta = entry.metadata().ok().filter(|m| m.is_file() && tlog)?;
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, meta.len(), path))
        })
        .collect();
    // Newest first; ties fall back to the name (a UTC stamp), newest name first.
    captures.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.2.cmp(&a.2)));
    let mut kept = 0usize;
    let mut bytes = 0u64;
    let mut removed = Vec::new();
    for (_, len, path) in captures {
        if kept + 1 < keep && bytes + len <= budget {
            kept += 1;
            bytes += len;
        } else if fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;
