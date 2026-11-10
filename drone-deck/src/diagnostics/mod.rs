//! Crash reports, the rolling app log and the diagnostics bundle.
//!
//! - `logs_dir()`: `%LOCALAPPDATA%\DroneDeck\logs` beside the default data root, or `<data
//!   root>\logs` when the data root is overridden or portable.
//! - `app.log` rolls at [`LOG_CAP`] bytes into `app.1.log` … `app.<LOG_KEEP>.log`.
//! - A panic writes `crash-<YYYYMMDD-HHMMSS>.txt`; the next launch offers the newest unseen one
//!   in the startup notice, the activity history and a strip on the Activity page.
//! - `export` writes `<data root>\exports\drone-deck-diagnostics-<stamp>.zip` with redacted
//!   profiles, state and recent logs.
//!
//! Nothing is written in headless mode or under `cfg(test)`, where `startup` is never enabled.
pub(crate) mod redact;
pub(crate) mod zip;

use crate::{State, activity, backend};
use std::{
    ffi::OsString,
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

#[cfg(test)]
mod tests;

/// `app.log` rolls over at this size.
pub(crate) const LOG_CAP: u64 = 1024 * 1024;
/// Rolled logs kept beside `app.log`.
pub(crate) const LOG_KEEP: usize = 2;
/// Crash reports kept in the logs folder.
const CRASH_KEEP: usize = 10;
/// Crash reports one process writes at most (a panic storm must not fill the disk).
const CRASH_PER_RUN: u32 = 3;
/// Bytes kept from the end of each log file in a bundle.
const TAIL: usize = 512 * 1024;
const ACKNOWLEDGED: &str = "crash-acknowledged.txt";
const VERSION: &str = env!("CARGO_PKG_VERSION");

static ENABLED: AtomicBool = AtomicBool::new(false);
static WRITING: Mutex<()> = Mutex::new(());
static CRASHES: AtomicU32 = AtomicU32::new(0);
#[cfg(not(test))]
static PENDING: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(test)]
thread_local! {
    /// Per test thread, so a test that sets a crash report cannot show the strip in another
    /// test's Activity page.
    static PENDING: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}
/// Runs `f` on the crash report offered this session.
fn with_pending<R>(f: impl FnOnce(&mut Option<PathBuf>) -> R) -> R {
    #[cfg(not(test))]
    {
        let mut pending = PENDING.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut pending)
    }
    #[cfg(test)]
    {
        PENDING.with(|p| f(&mut p.borrow_mut()))
    }
}

/// The logs folder for a data root (see the module docs).
pub(crate) fn logs_for(root: &Path, local_app_data: Option<OsString>) -> PathBuf {
    if let Some(base) = local_app_data.filter(|d| !d.is_empty()) {
        let app = PathBuf::from(base).join("DroneDeck");
        if root == app.join("workspace") {
            return app.join("logs");
        }
    }
    root.join("logs")
}
pub(crate) fn logs_dir() -> PathBuf {
    logs_for(&backend::root(), std::env::var_os("LOCALAPPDATA"))
}

/// `YYYYMMDD-HHMMSS` in local time.
fn stamp() -> String {
    let digits: String = activity::now()
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    if digits.len() == 14 {
        format!("{}-{}", &digits[..8], &digits[8..])
    } else {
        digits
    }
}

/// `app.1.log` for `app.log`.
fn rolled(path: &Path, n: usize) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path.extension().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{stem}.{n}.{ext}"))
}
/// Rolls `path` over when it holds `cap` bytes or more: `path` → `.1`, `.1` → `.2` …, keeping
/// `keep` rolled files. Returns whether it rolled.
pub(crate) fn rotate(path: &Path, cap: u64, keep: usize) -> io::Result<bool> {
    if fs::metadata(path).map_or(true, |m| m.len() < cap) {
        return Ok(false);
    }
    if keep == 0 {
        fs::remove_file(path)?;
        return Ok(true);
    }
    let _ = fs::remove_file(rolled(path, keep));
    for n in (1..keep).rev() {
        let from = rolled(path, n);
        if from.exists() {
            fs::rename(&from, rolled(path, n + 1))?;
        }
    }
    fs::rename(path, rolled(path, 1))?;
    Ok(true)
}
/// Appends one line to `path`, rolling it first when full. When rolling fails (a scanner or a
/// viewer holding a rolled file), the line still goes into `path` until it reaches twice `cap`;
/// past that, lines are dropped rather than letting the log grow without bound.
pub(crate) fn append(path: &Path, line: &str, cap: u64, keep: usize) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if let Err(e) = rotate(path, cap, keep) {
        if fs::metadata(path).is_ok_and(|m| m.len() >= cap.saturating_mul(2)) {
            return Err(e);
        }
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line: String = line.chars().take(4096).collect();
    writeln!(file, "{}", line.trim_end())
}

/// Writes `time [level] message` to `app.log`. A no-op until `startup` enables logging, so
/// headless runs and tests never write.
pub(crate) fn log(level: &str, message: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    // try_lock with a short wait: a panic raised while this thread holds the lock must not
    // deadlock the panic hook that logs it.
    let guard = (0..50).find_map(|_| match WRITING.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => {
            std::thread::sleep(std::time::Duration::from_millis(1));
            None
        }
    });
    if guard.is_none() {
        return;
    }
    let _ = append(
        &logs_dir().join("app.log"),
        &format!("{} [{level}] {message}", activity::now()),
        LOG_CAP,
        LOG_KEEP,
    );
}

/// The text of a crash report.
pub(crate) fn crash_text(message: &str, location: &str, thread: &str, backtrace: &str) -> String {
    format!(
        "Drone Deck {VERSION} crash report\r\nTime: {}\r\nOS: {}\r\nThread: {thread}\r\nLocation: {location}\r\n\r\n{message}\r\n\r\nBacktrace:\r\n{backtrace}\r\n",
        activity::now(),
        os_version(),
    )
}
/// Writes a crash report into `dir` under a free `crash-<stamp>.txt` name and prunes old ones.
pub(crate) fn write_crash(dir: &Path, text: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let stamp = stamp();
    let mut path = dir.join(format!("crash-{stamp}.txt"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("crash-{stamp}-{n}.txt"));
        n += 1;
    }
    fs::write(&path, text)?;
    for old in crash_files(dir).into_iter().rev().skip(CRASH_KEEP) {
        let _ = fs::remove_file(old);
    }
    Ok(path)
}
/// `crash-*.txt` in `dir`, oldest first.
pub(crate) fn crash_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().unwrap_or_default().to_string_lossy();
                    name.starts_with("crash-") && name.ends_with(".txt") && name != ACKNOWLEDGED
                })
                .collect()
        })
        .unwrap_or_default();
    // Stems sort by time: `crash-<stamp>` before `crash-<stamp>-2`.
    files.sort_by_key(|p| p.file_stem().unwrap_or_default().to_os_string());
    files
}
/// The newest crash report in `dir` not yet offered, marking it offered.
pub(crate) fn take_unseen_crash(dir: &Path) -> Option<PathBuf> {
    let newest = crash_files(dir).pop()?;
    let stem = newest.file_stem()?.to_string_lossy().into_owned();
    let seen = fs::read_to_string(dir.join(ACKNOWLEDGED)).unwrap_or_default();
    if seen.trim() >= stem.as_str() {
        return None;
    }
    let _ = fs::write(dir.join(ACKNOWLEDGED), &stem);
    Some(newest)
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(panic payload is not text)".into())
}
/// Chains a hook that writes a crash report (once logging is enabled) before the default
/// hook runs. Call once, at the top of `main`.
pub(crate) fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if ENABLED.load(Ordering::Relaxed)
            && CRASHES.fetch_add(1, Ordering::Relaxed) < CRASH_PER_RUN
        {
            let message = panic_message(info.payload());
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_default();
            let thread = std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string();
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            let text = crash_text(&message, &location, &thread, &backtrace);
            match write_crash(&logs_dir(), &text) {
                Ok(path) => log(
                    "error",
                    &format!(
                        "Panic on thread {thread} at {location}: {message} (report {})",
                        path.display()
                    ),
                ),
                Err(e) => log(
                    "error",
                    &format!("Panic at {location}: {message}; report not written: {e}"),
                ),
            }
        }
        previous(info);
    }));
}

/// Startup: enables logging (not in headless mode), records the session start and offers a
/// crash report left by the previous session.
pub(crate) fn startup(s: &mut State) {
    if s.headless {
        return;
    }
    ENABLED.store(true, Ordering::Relaxed);
    log(
        "info",
        &format!(
            "Drone Deck {VERSION} started on {}; data root {}",
            os_version(),
            backend::root().display()
        ),
    );
    if !s.notice.is_empty() {
        log("info", &format!("Startup notice: {}", s.notice));
    }
    let Some(path) = take_unseen_crash(&logs_dir()) else {
        return;
    };
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    with_pending(|pending| *pending = Some(path));
    // A panic on a worker thread does not always end the app, so say "hit", not "stopped".
    s.notice = format!(
        "Drone Deck hit an internal error last session; the report is in Activity. {}",
        s.notice
    )
    .trim_end()
    .to_string();
    activity::record(
        s,
        format!("Internal error report from the previous session saved as {name}"),
    );
}
/// The crash report offered this session, until dismissed.
pub(crate) fn pending_crash() -> Option<PathBuf> {
    with_pending(|p| p.clone())
}
pub(crate) fn dismiss_crash() {
    with_pending(|p| *p = None);
}
#[cfg(test)]
pub(crate) fn set_pending_crash(path: Option<PathBuf>) {
    with_pending(|p| *p = path);
}

/// `Windows 10.0.19045` from the kernel (not the compatibility-shimmed version).
pub(crate) fn os_version() -> String {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Info {
            size: u32,
            major: u32,
            minor: u32,
            build: u32,
            platform: u32,
            service_pack: [u16; 128],
        }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn RtlGetVersion(info: *mut Info) -> i32;
        }
        let mut info = Info {
            size: size_of::<Info>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform: 0,
            service_pack: [0; 128],
        };
        if unsafe { RtlGetVersion(&mut info) } == 0 {
            return format!("Windows {}.{}.{}", info.major, info.minor, info.build);
        }
        "Windows (version unavailable)".into()
    }
    #[cfg(not(windows))]
    {
        std::env::consts::OS.to_string()
    }
}

/// The last `max` bytes of `bytes` as text, starting on a whole line when cut.
fn tail(bytes: &[u8], max: usize) -> String {
    if bytes.len() <= max {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let cut = &bytes[bytes.len() - max..];
    let start = cut.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
    format!(
        "[earlier lines omitted]\n{}",
        String::from_utf8_lossy(&cut[start..])
    )
}
/// [`tail`] of the file at `path`, reading only its last `TAIL + 1` bytes: the extra byte
/// tells whether the cut falls on a line start, so the result equals `tail` of the whole file.
fn read_tail(path: &Path) -> Option<String> {
    read_tail_of(path, TAIL)
}
fn read_tail_of(path: &Path, max: usize) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let want = max as u64 + 1;
    let len = file.metadata().ok()?.len();
    if len > want {
        file.seek(SeekFrom::Start(len - want)).ok()?;
    }
    let mut bytes = Vec::with_capacity(len.min(want) as usize);
    file.take(want).read_to_end(&mut bytes).ok()?;
    Some(tail(&bytes, max))
}

fn about(s: &State) -> String {
    let mut t = format!(
        "Drone Deck {VERSION}\nExported: {}\nOS: {}\nData root: {}\nLogs: {}\nConfigured WSL distribution: {}\n{}\n",
        activity::now(),
        os_version(),
        backend::root().display(),
        logs_dir().display(),
        s.config.distro,
        crate::winjob::describe(),
    );
    match &s.setup.host {
        None => t.push_str("\nHost scan: not run in this session\n"),
        Some(h) => {
            t.push_str(&format!(
                "\nHost scan\n  Windows: {} (build {}, {})\n  Virtualization: {}\n  Memory: {:.1} GiB, free disk: {:.1} GiB\n  GPU: {}\n  WSL engine: {}, WSL ready: {}\n",
                h.windows, h.build, h.architecture, h.virtualization, h.ram_gib, h.disk_gib, h.gpu, h.wsl_engine, h.wsl_ready
            ));
            for d in &h.distros {
                t.push_str(&format!(
                    "  Distribution: {} (WSL {}, {}{})\n",
                    d.name,
                    d.version,
                    d.state,
                    if d.support.is_empty() {
                        String::new()
                    } else {
                        format!(", {}", d.support)
                    }
                ));
            }
        }
    }
    t
}
fn state(s: &State) -> String {
    let mut t = String::from("Owned processes (status / readiness)\n");
    let mut ids: Vec<&String> = s.status.keys().collect();
    ids.sort();
    if ids.is_empty() {
        t.push_str("  none this session\n");
    }
    for id in ids {
        t.push_str(&format!(
            "  {id}: {} / {}\n",
            s.status[id],
            s.readiness.get(id).map_or("-", String::as_str)
        ));
    }
    t.push_str("\nInstallation jobs\n");
    if s.setup.jobs.is_empty() {
        t.push_str("  none known\n");
    }
    for j in &s.setup.jobs {
        t.push_str(&format!(
            "  {} {}: {} step {}/{}{} - {}\n",
            j.id,
            j.tool,
            j.status,
            j.step,
            j.total,
            if j.alive { ", worker alive" } else { "" },
            j.message
        ));
    }
    t.push_str(&format!("\n{}\n", crate::winjob::describe()));
    t
}
/// Installation job folders under `<data root>\setup\jobs`, newest first, at most `n`.
fn recent_job_dirs(n: usize) -> Vec<PathBuf> {
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> =
        fs::read_dir(backend::root().join("setup").join("jobs"))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|e| {
                        let time = e
                            .metadata()
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::UNIX_EPOCH);
                        (time, e.path())
                    })
                    .collect()
            })
            .unwrap_or_default();
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.into_iter().take(n).map(|(_, p)| p).collect()
}

/// The bundle's files (name, text) before redaction.
pub(crate) fn entries(s: &State) -> Vec<(String, String)> {
    let mut files = vec![
        ("about.txt".to_string(), about(s)),
        ("state.txt".to_string(), state(s)),
        (
            "profiles.json".to_string(),
            serde_json::to_string_pretty(&s.config).unwrap_or_default(),
        ),
        (
            "activity.json".to_string(),
            serde_json::to_string_pretty(&s.activity.entries).unwrap_or_default(),
        ),
        (
            "process-output.log".to_string(),
            s.logs.iter().cloned().collect::<Vec<_>>().join("\n"),
        ),
    ];
    let root = backend::root();
    for name in ["systems.json", "ui-preferences.json"] {
        if let Some(text) = read_tail(&root.join(name)) {
            files.push((format!("data/{name}"), text));
        }
    }
    let logs = logs_dir();
    let app = logs.join("app.log");
    for path in std::iter::once(app.clone()).chain((1..=LOG_KEEP).map(|n| rolled(&app, n))) {
        if let Some(text) = read_tail(&path) {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            files.push((format!("logs/{name}"), text));
        }
    }
    for path in crash_files(&logs).into_iter().rev().take(5) {
        if let Some(text) = read_tail(&path) {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            files.push((format!("logs/{name}"), text));
        }
    }
    for dir in recent_job_dirs(5) {
        let id = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        for name in ["plan.json", "state.json", "output.log"] {
            if let Some(text) = read_tail(&dir.join(name)) {
                files.push((format!("setup-jobs/{id}/{name}"), text));
            }
        }
    }
    files
}

/// Writes the redacted bundle to `<data root>\exports\` and returns its path.
pub(crate) fn export(s: &State) -> Result<PathBuf, String> {
    let redactor = redact::Redactor::from_env();
    // Every entry is capped, including in-memory ones and any added to `entries` later.
    let files: Vec<(String, Vec<u8>)> = entries(s)
        .into_iter()
        .map(|(name, text)| {
            (
                name,
                redactor.apply(&tail(text.as_bytes(), TAIL)).into_bytes(),
            )
        })
        .collect();
    let bytes = zip::store(&files, zip::dos_time(&activity::now()));
    let path = backend::root()
        .join("exports")
        .join(format!("drone-deck-diagnostics-{}.zip", stamp()));
    crate::store::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    log(
        "info",
        &format!("Diagnostics exported to {}", path.display()),
    );
    Ok(path)
}
