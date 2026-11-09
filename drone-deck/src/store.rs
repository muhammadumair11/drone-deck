//! Durable files in the data root. A write lands in a temporary file beside its target, is
//! flushed and renamed over it, and the previous version stays as `<name>.bak`. A file that
//! fails to parse is moved aside as `<stem>.corrupt-<YYYYMMDD-HHMMSS>.<ext>`, its backup is
//! restored when it parses, and the user is told either way.
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

static NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static TEMPORARY: AtomicU32 = AtomicU32::new(0);

/// Queues a message for the startup notice and the activity history.
pub(crate) fn note(text: impl Into<String>) {
    if let Ok(mut notices) = NOTICES.lock() {
        notices.push(text.into());
    }
}
/// Messages queued since the last call.
pub(crate) fn take_notices() -> Vec<String> {
    NOTICES
        .lock()
        .map(|mut n| std::mem::take(&mut *n))
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
/// `path` with `suffix` added to its file name.
pub(crate) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    path.with_file_name(format!("{}{suffix}", file_name(path)))
}
/// `profiles.json` → `profiles.json.bak`.
pub(crate) fn backup_path(path: &Path) -> PathBuf {
    sibling(path, ".bak")
}

/// Replaces `path` with `bytes` atomically, keeping the previous file as `<name>.bak`.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace(path, bytes, true)
}
/// Moves `from` over `to`. Windows refuses for a moment while a scanner or indexer has the
/// target open, so an access or sharing error is retried briefly.
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Err(e)
                if attempt < 5
                    && (e.kind() == io::ErrorKind::PermissionDenied
                        || e.raw_os_error() == Some(32)) =>
            {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(20 * attempt));
            }
            result => return result,
        }
    }
}
fn replace(path: &Path, bytes: &[u8], keep_backup: bool) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(
        ".{}.{}-{}.tmp",
        file_name(path),
        std::process::id(),
        TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if keep_backup && path.is_file() {
            fs::copy(path, backup_path(path))?;
        }
        rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
/// Writes `value` as pretty JSON through [`write`].
pub(crate) fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    write(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The outcome of reading a stored file.
pub(crate) enum Read<T> {
    Missing,
    Loaded(T),
    /// The file was damaged; its backup was restored. Carries the user notice.
    Recovered(T, String),
    /// The file could not be read or parsed and no backup parsed. Carries the user notice.
    Lost(String),
}

/// `YYYYMMDD-HHMMSS` in local time.
fn stamp() -> String {
    let digits: String = crate::activity::now()
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    if digits.len() == 14 {
        format!("{}-{}", &digits[..8], &digits[8..])
    } else {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("t{secs}")
    }
}
/// A free `<stem>.corrupt-<stamp>.<ext>` name beside `path`.
fn quarantine_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "json".into());
    let stamp = stamp();
    let mut candidate = path.with_file_name(format!("{stem}.corrupt-{stamp}.{ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = path.with_file_name(format!("{stem}.corrupt-{stamp}-{n}.{ext}"));
        n += 1;
    }
    candidate
}

/// Reads and parses `path`. A damaged file is quarantined and its backup tried; nothing is
/// queued as a notice (see [`read`]).
pub(crate) fn load<T>(path: &Path, parse: impl Fn(&[u8]) -> Result<T, String>) -> Read<T> {
    let name = file_name(path);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Read::Missing,
        Err(e) => {
            return Read::Lost(format!(
                "{name} could not be read ({e}); defaults are in use until it is readable."
            ));
        }
    };
    let error = match parse(&bytes) {
        Ok(value) => return Read::Loaded(value),
        Err(e) => e,
    };
    let quarantine = quarantine_path(path);
    let kept = file_name(&quarantine);
    if rename(path, &quarantine).is_err() && fs::copy(path, &quarantine).is_err() {
        return Read::Lost(format!(
            "{name} is damaged ({error}) and could not be set aside; defaults are in use and the file was left unchanged."
        ));
    }
    let backup = backup_path(path);
    if let Some(value) = fs::read(&backup)
        .ok()
        .and_then(|bytes| parse(&bytes).ok().map(|value| (value, bytes)))
        // No new backup: a damaged file that could only be copied aside is still in place
        // and must not overwrite the good `.bak`.
        .and_then(|(value, bytes)| replace(path, &bytes, false).ok().map(|_| value))
    {
        return Read::Recovered(
            value,
            format!(
                "{name} was damaged ({error}). It was kept as {kept} and the previous version was restored from {}.",
                file_name(&backup)
            ),
        );
    }
    Read::Lost(format!(
        "{name} was damaged ({error}). It was kept as {kept}; no usable backup was found, so defaults are in use."
    ))
}
/// [`load`], queueing any recovery or loss as a user notice. `None` means use defaults.
pub(crate) fn read<T>(path: &Path, parse: impl Fn(&[u8]) -> Result<T, String>) -> Option<T> {
    match load(path, parse) {
        Read::Missing => None,
        Read::Loaded(value) => Some(value),
        Read::Recovered(value, notice) => {
            note(notice);
            Some(value)
        }
        Read::Lost(notice) => {
            note(notice);
            None
        }
    }
}
/// [`read`] for a JSON file of type `T`.
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    read(path, |bytes| {
        serde_json::from_slice(bytes).map_err(|e| e.to_string())
    })
}
