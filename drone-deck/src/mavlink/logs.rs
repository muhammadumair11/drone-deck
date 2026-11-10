//! Onboard logs: LOG_REQUEST_LIST into entries, and LOG_REQUEST_DATA downloads that write a
//! contiguous `.part` file and resume from its length.
use super::frame::{Msg, dialect};
use super::modes::Autopilot;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// LOG_DATA carries at most this many bytes.
pub const CHUNK: u32 = 90;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub id: u16,
    /// Bytes.
    pub size: u32,
    /// UTC seconds since 1970, 0 when the vehicle has no clock.
    pub time_utc: u32,
}

/// What the list collector wants next.
#[derive(Debug, PartialEq)]
pub enum ListStep {
    Wait,
    Send(Msg),
    Done {
        entries: Vec<LogEntry>,
        complete: bool,
    },
}

/// Collects LOG_ENTRY answers until every log is listed (re-asking on silence).
pub struct LogList {
    target: (u8, u8),
    entries: BTreeMap<u16, LogEntry>,
    expected: Option<u16>,
    last_rx: Instant,
    requests: u32,
    pub idle: Duration,
}

impl LogList {
    pub fn new(target: (u8, u8), now: Instant) -> (Self, Msg) {
        let list = Self {
            target,
            entries: BTreeMap::new(),
            expected: None,
            last_rx: now,
            requests: 1,
            idle: Duration::from_millis(1000),
        };
        let msg = list.request();
        (list, msg)
    }
    fn request(&self) -> Msg {
        Msg::LOG_REQUEST_LIST(dialect::LOG_REQUEST_LIST_DATA {
            start: 0,
            end: u16::MAX,
            target_system: self.target.0,
            target_component: self.target.1,
        })
    }
    pub fn on_entry(&mut self, e: &dialect::LOG_ENTRY_DATA, now: Instant) {
        self.last_rx = now;
        self.expected = Some(e.num_logs);
        if e.num_logs > 0 {
            self.entries.insert(
                e.id,
                LogEntry {
                    id: e.id,
                    size: e.size,
                    time_utc: e.time_utc,
                },
            );
        }
    }
    pub fn next_deadline(&self) -> Instant {
        self.last_rx + self.idle
    }
    pub fn poll(&mut self, now: Instant) -> ListStep {
        if self
            .expected
            .is_some_and(|n| self.entries.len() >= n as usize)
        {
            return self.finish(true);
        }
        if now < self.next_deadline() {
            return ListStep::Wait;
        }
        if self.requests >= 3 {
            return self.finish(false);
        }
        self.requests += 1;
        self.last_rx = now;
        ListStep::Send(self.request())
    }
    fn finish(&mut self, complete: bool) -> ListStep {
        ListStep::Done {
            entries: std::mem::take(&mut self.entries).into_values().collect(),
            complete,
        }
    }
}

/// `log_<id>_<YYYY-MM-DD_HH-MM-SS>.<ulg|bin>` (`log_<id>_<size>B.<ext>` without a vehicle
/// clock); the same entry always maps to the same name.|bin>`; the same entry always maps to the same name.
pub fn file_name(entry: &LogEntry, autopilot: Autopilot) -> String {
    let ext = if autopilot == Autopilot::Px4 {
        "ulg"
    } else {
        "bin"
    };
    if entry.time_utc == 0 {
        // No vehicle clock. PX4 ids are positions in the onboard listing and get reused, so
        // the size keeps one log from resuming into or standing in for another.
        return format!("log_{}_{}B.{ext}", entry.id, entry.size);
    }
    format!(
        "log_{}_{}.{ext}",
        entry.id,
        utc_stamp(entry.time_utc as u64)
    )
}

/// `YYYY-MM-DD_HH-MM-SS` for UTC seconds since 1970.
pub fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}_{:02}-{:02}-{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// What a download wants next.
#[derive(Debug, PartialEq)]
pub enum DownloadStep {
    Wait,
    Send(Msg),
    Done(PathBuf),
    Failed(String),
}

/// One log download. Data lands in `<final>.part` only in order, so the `.part` length is
/// always a valid prefix and a later download resumes from it.
pub struct LogDownload {
    pub entry: LogEntry,
    target: (u8, u8),
    part: PathBuf,
    path: PathBuf,
    file: Option<File>,
    next: u32,
    /// Out-of-order chunks inside the current window, by offset.
    early: BTreeMap<u32, Vec<u8>>,
    window_end: u32,
    last_rx: Instant,
    stalls: u32,
    pub window: u32,
    pub idle: Duration,
    pub max_stalls: u32,
}

impl LogDownload {
    /// Opens (or resumes) the download into `dir`. Returns the first request, or `Done`
    /// when the finished file already exists with the listed size (a file of another size is
    /// downloaded again and replaced).
    pub fn start(
        entry: LogEntry,
        target: (u8, u8),
        autopilot: Autopilot,
        dir: &Path,
        now: Instant,
    ) -> io::Result<(Self, DownloadStep)> {
        fs::create_dir_all(dir)?;
        let path = dir.join(file_name(&entry, autopilot));
        let part = path.with_extension(format!(
            "{}.part",
            path.extension().and_then(|e| e.to_str()).unwrap_or("log")
        ));
        let mut download = Self {
            entry,
            target,
            part,
            path,
            file: None,
            next: 0,
            early: BTreeMap::new(),
            window_end: 0,
            last_rx: now,
            stalls: 0,
            window: CHUNK * 512,
            idle: Duration::from_millis(500),
            max_stalls: 10,
        };
        let finished = fs::metadata(&download.path).map(|m| m.len());
        if finished
            .as_ref()
            .is_ok_and(|&len| len == download.entry.size as u64)
        {
            let path = download.path.clone();
            return Ok((download.closed(), DownloadStep::Done(path)));
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&download.part)?;
        let mut have = file.metadata()?.len();
        if have > download.entry.size as u64 {
            file.set_len(0)?;
            have = 0;
        }
        download.next = have as u32;
        download.file = Some(file);
        let step = download.request_window(now);
        Ok((download, step))
    }
    fn closed(mut self) -> Self {
        self.file = None;
        self
    }
    pub fn received(&self) -> u32 {
        self.next
    }
    #[cfg(test)]
    pub fn part_path(&self) -> &Path {
        &self.part
    }
    fn request_window(&mut self, now: Instant) -> DownloadStep {
        if self.next >= self.entry.size {
            return self.finish();
        }
        let count = self.window.min(self.entry.size - self.next);
        self.window_end = self.next + count;
        self.early.clear();
        self.last_rx = now;
        DownloadStep::Send(Msg::LOG_REQUEST_DATA(dialect::LOG_REQUEST_DATA_DATA {
            ofs: self.next,
            count,
            id: self.entry.id,
            target_system: self.target.0,
            target_component: self.target.1,
        }))
    }
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.write_all(bytes)?;
        }
        self.next += bytes.len() as u32;
        Ok(())
    }
    /// Takes one LOG_DATA. Returns a step when the download finished, failed, or should ask
    /// for the next window.
    pub fn on_data(&mut self, d: &dialect::LOG_DATA_DATA, now: Instant) -> Option<DownloadStep> {
        if d.id != self.entry.id || self.file.is_none() {
            return None;
        }
        self.last_rx = now;
        let count = (d.count as u32).min(CHUNK);
        if count == 0 {
            // Past the end: the vehicle's size was an over-estimate.
            if d.ofs <= self.next {
                self.entry.size = self.next;
                return Some(self.finish());
            }
            return None;
        }
        // Saturating: a malformed offset near u32::MAX must not wrap into the window.
        if d.ofs.saturating_add(count) <= self.next || d.ofs >= self.window_end {
            return None;
        }
        if d.ofs > self.next {
            self.early.insert(d.ofs, d.data[..count as usize].to_vec());
            return None;
        }
        let skip = (self.next - d.ofs) as usize;
        if let Err(e) = self.write(&d.data[skip..count as usize]) {
            return Some(self.fail(format!("Couldn't write {}: {e}", self.part.display())));
        }
        while let Some(chunk) = self.early.remove(&self.next) {
            if let Err(e) = self.write(&chunk) {
                return Some(self.fail(format!("Couldn't write {}: {e}", self.part.display())));
            }
        }
        self.early.retain(|&ofs, _| ofs > self.next);
        self.stalls = 0;
        if self.next >= self.entry.size {
            return Some(self.finish());
        }
        if self.next >= self.window_end {
            return Some(self.request_window(now));
        }
        None
    }
    pub fn next_deadline(&self) -> Instant {
        self.last_rx + self.idle
    }
    /// Re-asks from the first missing byte after silence; fails after `max_stalls`.
    pub fn poll(&mut self, now: Instant) -> DownloadStep {
        if self.file.is_none() || now < self.next_deadline() {
            return DownloadStep::Wait;
        }
        self.stalls += 1;
        if self.stalls > self.max_stalls {
            return self.fail(format!(
                "The vehicle stopped sending log data at {} of {} bytes; download again to resume",
                self.next, self.entry.size
            ));
        }
        self.request_window(now)
    }
    /// LOG_REQUEST_END, sent when a download ends for any reason.
    pub fn end_message(&self) -> Msg {
        Msg::LOG_REQUEST_END(dialect::LOG_REQUEST_END_DATA {
            target_system: self.target.0,
            target_component: self.target.1,
        })
    }
    /// Flushes and keeps the `.part` file for a later resume.
    pub fn cancel(&mut self) {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush();
        }
    }
    fn fail(&mut self, reason: String) -> DownloadStep {
        self.cancel();
        DownloadStep::Failed(reason)
    }
    fn finish(&mut self) -> DownloadStep {
        let Some(file) = self.file.take() else {
            return DownloadStep::Done(self.path.clone());
        };
        let synced = file.sync_all();
        drop(file);
        let result = synced.and_then(|()| fs::rename(&self.part, &self.path));
        match result {
            Ok(()) => DownloadStep::Done(self.path.clone()),
            Err(e) => DownloadStep::Failed(format!("Couldn't finish {}: {e}", self.path.display())),
        }
    }
}

#[cfg(test)]
#[path = "logs_tests.rs"]
mod tests;
