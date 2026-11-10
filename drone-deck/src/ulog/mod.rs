//! Flight logs: a ULog parser with flight summaries and plot series, a `.tlog` summary from the
//! MAVLink service's decoder (`mavlink::summarize_tlog`, CRC-checked and streamed), log discovery (PX4 SITL in WSL, vehicle downloads, telemetry captures) and the state behind
//! Data & models → Flight logs. Files are read and parsed on a worker thread; the UI keeps only
//! summaries and reduced plot series, in a bounded cache.
mod fixture;
pub(crate) mod parse;
pub(crate) mod plots;
pub(crate) mod sources;
pub(crate) mod summary;
mod worker;

pub(crate) use parse::{Error, parse};
pub(crate) use plots::Plot;
pub(crate) use sources::{Locations, LogFile, Source, modified_utc, utc, vehicle_dir};
pub(crate) use summary::{Summary, summarize};

use crate::mavlink::{TlogSummary, summarize_tlog, summarize_tlog_bytes};

use crate::State;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use worker::{Done, Job, Worker};

/// `ui.data_tab` of Data & models → Flight logs.
pub(crate) const LOGS_TAB: usize = 4;
/// Larger ULog files are not read into memory; Flight Review handles them. Tlogs are streamed,
/// so any size is summarized.
pub(crate) const MAX_BYTES: u64 = 512 * 1024 * 1024;
/// PX4 Flight Review's upload page.
pub(crate) const FLIGHT_REVIEW: &str = "https://logs.px4.io/upload";
/// Analyses kept (each is a summary plus a few reduced plot lines).
const CACHE_ENTRIES: usize = 48;
/// After a scan, this many of the newest logs are analyzed in the background so the list can
/// show durations; bigger files wait until they are selected.
const BACKGROUND: usize = 12;
const BACKGROUND_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// What a file yielded.
#[derive(Debug)]
pub(crate) enum Report {
    Ulog { summary: Summary, plots: Vec<Plot> },
    Tlog(TlogSummary),
}
impl Report {
    pub(crate) fn duration_s(&self) -> f64 {
        match self {
            Report::Ulog { summary, .. } => summary.duration_s,
            Report::Tlog(t) => t.duration_s(),
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) enum Analysis {
    Pending,
    Ready(Arc<Report>),
    Failed(String),
}

/// File size and modification time: a cached analysis is reused only while both match.
type Stamp = (u64, Option<SystemTime>);
fn stamp(f: &LogFile) -> Stamp {
    (f.size, f.modified)
}

#[derive(Default)]
pub(crate) struct FlightLogs {
    pub files: Vec<LogFile>,
    /// SITL log folders the last scan searched.
    pub searched: Vec<PathBuf>,
    /// The last scan included WSL and its file system answered.
    pub wsl_reachable: bool,
    /// The last scan was allowed to look inside WSL (the distribution was ready).
    pub wsl_scanned: bool,
    pub scanned: bool,
    pub scanning: bool,
    pub selected: Option<PathBuf>,
    /// `(distro, home)` reported by the WSL agent; until then homes are guessed from `/home/*`.
    pub home: Option<(String, String)>,
    /// The list is the headless preview fixture.
    pub preview: bool,
    cache: HashMap<PathBuf, (Stamp, Analysis)>,
    order: VecDeque<PathBuf>,
    worker: Option<Worker>,
}
impl FlightLogs {
    pub(crate) fn analysis(&self, f: &LogFile) -> Option<&Analysis> {
        self.cache
            .get(&f.path)
            .filter(|(s, _)| *s == stamp(f))
            .map(|(_, a)| a)
    }
    pub(crate) fn selected_file(&self) -> Option<&LogFile> {
        let sel = self.selected.as_ref()?;
        self.files.iter().find(|f| &f.path == sel)
    }
    fn insert(&mut self, path: PathBuf, stamp: Stamp, a: Analysis) {
        self.order.retain(|p| p != &path);
        self.order.push_back(path.clone());
        self.cache.insert(path, (stamp, a));
        while self.order.len() > CACHE_ENTRIES {
            let Some(old) = self
                .order
                .iter()
                .position(|p| Some(p) != self.selected.as_ref())
            else {
                break;
            };
            if let Some(p) = self.order.remove(old) {
                self.cache.remove(&p);
            }
        }
    }
    fn send(&mut self, job: Job) {
        self.worker.get_or_insert_with(Worker::spawn).send(job);
    }
    fn request(&mut self, f: &LogFile, urgent: bool) {
        match self.analysis(f) {
            Some(Analysis::Ready(_)) => return,
            Some(Analysis::Pending) => {
                // Queued in the background: the user is waiting on it now.
                if urgent {
                    self.send(Job::Promote(f.path.clone()));
                }
                return;
            }
            _ => {}
        }
        if f.size > MAX_BYTES && !f.is_tlog() {
            let msg = format!(
                "{} MB is more than Drone Deck reads into memory ({} MB). Open it in PX4 Flight Review.",
                f.size / (1024 * 1024),
                MAX_BYTES / (1024 * 1024)
            );
            self.insert(f.path.clone(), stamp(f), Analysis::Failed(msg));
            return;
        }
        self.insert(f.path.clone(), stamp(f), Analysis::Pending);
        self.send(Job::Analyze {
            path: f.path.clone(),
            urgent,
        });
    }
}

/// Reads and analyzes one file (runs on the worker thread).
pub(crate) fn analyze_file(path: &Path) -> Result<Report, String> {
    let name = path.display();
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tlog"))
    {
        return summarize_tlog(path)
            .map(Report::Tlog)
            .map_err(|e| format!("Could not read {name}: {e}"));
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("Could not read {name}: {e}"))?;
    if meta.len() > MAX_BYTES {
        return Err(format!(
            "{name} is too large to read here; open it in PX4 Flight Review."
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read {name}: {e}"))?;
    analyze_bytes(bytes).map_err(|e| e.to_string())
}
pub(crate) fn analyze_bytes(bytes: Vec<u8>) -> Result<Report, Error> {
    let log = parse(bytes)?;
    let summary = summarize(&log);
    let plots = plots::extract(&log, summary.first_us);
    Ok(Report::Ulog { summary, plots })
}

/// Where to look, from the profiles, the distribution and the data root.
pub(crate) fn locations(s: &State) -> Locations {
    let distro = s.config.distro.clone();
    Locations {
        home: s
            .flight_logs
            .home
            .as_ref()
            .filter(|(d, _)| *d == distro)
            .map(|(_, h)| h.clone()),
        distro: s.wsl_ready().then_some(distro),
        px4_dirs: sources::px4_dirs(&s.config.profiles),
        data_root: crate::backend::root(),
    }
}

/// Rescans every source. Headless previews never touch the file system.
pub(crate) fn refresh(s: &mut State) {
    if s.headless {
        s.notice = "Headless preview · flight-log discovery is disabled.".into();
        return;
    }
    let loc = locations(s);
    s.flight_logs.wsl_scanned = loc.distro.is_some();
    s.flight_logs.scanned = true;
    s.flight_logs.scanning = true;
    s.flight_logs.send(Job::Scan(loc));
}

/// Selects a log and asks for its analysis unless a current one is cached.
pub(crate) fn select(s: &mut State, path: &Path) {
    let logs = &mut s.flight_logs;
    logs.selected = Some(path.to_path_buf());
    let Some(f) = logs.files.iter().find(|f| f.path == path).cloned() else {
        return;
    };
    if s.headless {
        if logs.analysis(&f).is_none() {
            logs.insert(
                f.path.clone(),
                stamp(&f),
                Analysis::Failed("Headless preview · logs are not read.".into()),
            );
        }
        return;
    }
    logs.request(&f, true);
}

/// The WSL agent's `hello` reports the Linux home; a later scan uses it instead of `/home/*`.
pub(crate) fn set_home(s: &mut State, distro: &str, home: &str) {
    let new = Some((distro.to_string(), home.to_string()));
    if s.flight_logs.home != new {
        s.flight_logs.home = new;
        s.flight_logs.scanned = false;
    }
}

/// The single `explorer.exe` argument that shows `path`: `/select,<path>` highlights the file,
/// but Explorer splits that argument at commas, and a path with spaces would reach it
/// whole-quoted (`"/select,C:\a b\x.ulg"`), a form Explorer does not reliably accept. Such
/// paths open the containing folder instead, which Explorer takes quoted.
pub(crate) fn explorer_target(path: &Path) -> String {
    let text = path.display().to_string();
    if text.contains([',', ' ', '\t', '"']) {
        path.parent().unwrap_or(path).display().to_string()
    } else {
        format!("/select,{text}")
    }
}

/// Opens Explorer with the file selected (or its folder; see `explorer_target`).
pub(crate) fn show_in_folder(s: &mut State, path: &Path) {
    if s.headless {
        s.notice = "Headless preview · opening folders is disabled.".into();
        return;
    }
    s.open(explorer_target(path));
}

/// Opens Flight Review's upload page and shows the file, so it can be dropped on the page.
/// Drone Deck uploads nothing itself.
pub(crate) fn flight_review(s: &mut State, path: &Path) {
    if s.headless {
        s.notice = "Headless preview · opening the browser is disabled.".into();
        return;
    }
    s.open(FLIGHT_REVIEW.into());
    s.open(explorer_target(path));
    s.notice = format!(
        "Flight Review opened in your browser. Upload {} there; Drone Deck does not upload logs.",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
}

/// Scans on the first visit of the tab (and again once WSL becomes ready), then applies
/// worker results. Returns whether visible state changed.
pub(crate) fn poll(s: &mut State) -> bool {
    let on_tab = s.page == crate::ui::Page::Data && s.ui.data_tab == LOGS_TAB;
    let mut changed = false;
    if on_tab
        && !s.headless
        && !s.flight_logs.scanning
        && !s.flight_logs.preview
        && (!s.flight_logs.scanned || (!s.flight_logs.wsl_scanned && s.wsl_ready()))
    {
        refresh(s);
        changed = true;
    }
    let done: Vec<Done> = match &s.flight_logs.worker {
        Some(w) => w.rx.try_iter().take(32).collect(),
        None => return changed,
    };
    for d in done {
        changed = true;
        apply(&mut s.flight_logs, d);
    }
    changed
}

fn apply(logs: &mut FlightLogs, d: Done) {
    match d {
        Done::Scanned(found) => {
            logs.scanning = false;
            logs.files = found.files;
            logs.searched = found.searched;
            logs.wsl_reachable = found.wsl_reachable;
            // The open log keeps its analysis current (it may have grown since it was read).
            match logs.selected_file().cloned() {
                Some(f) => logs.request(&f, true),
                None => logs.selected = None,
            }
            let background: Vec<LogFile> = logs
                .files
                .iter()
                .filter(|f| f.size <= BACKGROUND_MAX_BYTES)
                .take(BACKGROUND)
                .cloned()
                .collect();
            for f in background {
                logs.request(&f, false);
            }
        }
        Done::Analyzed(path, result) => {
            let Some(f) = logs.files.iter().find(|f| f.path == path).cloned() else {
                return;
            };
            let a = match result {
                Ok(r) => Analysis::Ready(Arc::new(r)),
                Err(e) => Analysis::Failed(e),
            };
            logs.insert(path, stamp(&f), a);
        }
    }
}

/// Headless `flight-logs` scene: the synthetic flight, a truncated copy and a tlog.
pub(crate) fn preview(s: &mut State) {
    let full = fixture::flight();
    let cut = full[..full.len() * 3 / 5].to_vec();
    let tlog = fixture::tlog();
    let base = PathBuf::from("preview");
    let entries = [
        (
            base.join("synthetic-flight.ulg"),
            Source::Simulation("px4_sitl_sih".into()),
            full,
        ),
        (base.join("synthetic-truncated.ulg"), Source::Vehicle, cut),
        (base.join("synthetic-capture.tlog"), Source::Telemetry, tlog),
    ];
    let logs = &mut s.flight_logs;
    logs.preview = true;
    logs.scanned = true;
    for (path, source, bytes) in entries {
        let f = LogFile {
            path,
            source,
            size: bytes.len() as u64,
            modified: Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_793_000_000)),
        };
        let report = if f.is_tlog() {
            Ok(Report::Tlog(summarize_tlog_bytes(&bytes)))
        } else {
            analyze_bytes(bytes).map_err(|e| e.to_string())
        };
        let a = match report {
            Ok(r) => Analysis::Ready(Arc::new(r)),
            Err(e) => Analysis::Failed(e),
        };
        logs.insert(f.path.clone(), stamp(&f), a);
        logs.files.push(f);
    }
    logs.selected = logs.files.first().map(|f| f.path.clone());
    s.ui.data_tab = LOGS_TAB;
    s.navigate(crate::ui::Page::Data);
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
