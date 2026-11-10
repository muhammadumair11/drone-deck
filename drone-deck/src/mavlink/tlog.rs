//! Telemetry logs in the QGroundControl `.tlog` layout: each record is an 8-byte big-endian
//! timestamp (microseconds since 1970, UTC) followed by one raw MAVLink frame.
use super::frame::{Frame, Framer, MAX_FRAME, Msg, frame_len, is_start};
use super::modes;
use super::state::{StatusAssembler, VehicleState};
use mavlink::MavlinkVersion;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn micros(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// `<dir>/<YYYY-MM-DD_HH-MM-SS>.tlog` for a session that starts at `at` (UTC).
pub fn default_path(dir: &Path, at: SystemTime) -> PathBuf {
    let secs = at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    dir.join(format!("{}.tlog", super::logs::utc_stamp(secs)))
}

/// Creates `path`, or `<stem>-2.<ext>`, `-3`… when it exists: an earlier session's log (two
/// sessions started in the same second share a default name) is never overwritten.
fn create_unique(path: &Path) -> io::Result<(PathBuf, File)> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("tlog");
    for n in 1..1000 {
        let candidate = if n == 1 {
            path.to_path_buf()
        } else {
            path.with_file_name(format!("{stem}-{n}.{ext}"))
        };
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{} and 998 numbered copies exist", path.display()),
    ))
}

/// Appends records; stops itself at `limit` bytes.
pub struct TlogWriter {
    path: PathBuf,
    out: BufWriter<File>,
    written: u64,
    limit: u64,
}

impl TlogWriter {
    /// Creates a new log at `path` (or a numbered sibling when `path` exists; see `path()`).
    pub fn create(path: &Path, limit: u64) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let (path, file) = create_unique(path)?;
        Ok(Self {
            path,
            out: BufWriter::with_capacity(64 * 1024, file),
            written: 0,
            limit,
        })
    }
    /// The file actually written.
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn written(&self) -> u64 {
        self.written
    }
    /// Writes one record. `Err` means the log should stop (write error or size limit).
    pub fn write(&mut self, at: SystemTime, frame: &Frame) -> Result<(), String> {
        let len = 8 + frame.bytes.len() as u64;
        if self.written + len > self.limit {
            let _ = self.out.flush();
            return Err(format!(
                "Telemetry log reached its {} MB limit",
                self.limit / 1_000_000
            ));
        }
        self.out
            .write_all(&micros(at).to_be_bytes())
            .and_then(|_| self.out.write_all(&frame.bytes))
            .map_err(|e| format!("Couldn't write {}: {e}", self.path.display()))?;
        self.written += len;
        Ok(())
    }
    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

impl Drop for TlogWriter {
    /// A capture that never received a frame (a link that never came up) is removed rather
    /// than left as an empty file in the log list.
    fn drop(&mut self) {
        let _ = self.out.flush();
        if self.written == 0 {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Timestamps outside 2001–2100 are not tlog timestamps. Requiring one keeps the reader from
/// locking onto bytes inside a damaged record.
const PLAUSIBLE_US: std::ops::Range<u64> = 978_307_200_000_000..4_102_444_800_000_000;

/// Splits tlog bytes into records, resynchronising after damage, and counts the damage.
#[derive(Default)]
struct Scanner {
    framer: Framer,
    frames: Vec<Frame>,
    skipped_bytes: u64,
    bad_frames: u64,
    truncated: bool,
}

impl Scanner {
    /// Calls `each` for every record in `bytes`. With `more` (the file goes on), it stops where
    /// a record could still be cut off and returns that offset so the caller can read on from
    /// there. Without it, a record cut off at the end marks the log truncated.
    fn scan(&mut self, bytes: &[u8], more: bool, each: &mut impl FnMut(u64, &Frame)) -> usize {
        let mut pos = 0;
        while pos < bytes.len() {
            let rest = &bytes[pos..];
            if more && rest.len() < 8 + MAX_FRAME {
                return pos;
            }
            if rest.len() < 9 {
                self.truncated = true;
                break;
            }
            let stamp = u64::from_be_bytes(rest[..8].try_into().unwrap_or_default());
            let packet = &rest[8..];
            if PLAUSIBLE_US.contains(&stamp) && is_start(packet[0]) {
                match frame_len(packet) {
                    Some(len) if packet.len() >= len => {
                        if let Some(frame) = self.frame(&packet[..len]) {
                            each(stamp, &frame);
                            pos += 8 + len;
                            continue;
                        }
                        self.bad_frames += 1;
                    }
                    // Only reachable at the end: with `more`, a whole frame is always in hand.
                    _ => {
                        self.truncated = true;
                        break;
                    }
                }
            }
            self.skipped_bytes += 1;
            pos += 1;
        }
        bytes.len()
    }

    /// The record's packet when it is exactly one frame that passes the CRC check.
    fn frame(&mut self, packet: &[u8]) -> Option<Frame> {
        self.framer.reset();
        self.frames.clear();
        self.framer.push(packet, &mut self.frames);
        let frame = self.frames.pop()?;
        (self.frames.is_empty() && frame.bytes.len() == packet.len()).then_some(frame)
    }
}

/// What a tlog shows at a glance.
#[derive(Clone, Debug, Default)]
pub struct TlogSummary {
    /// Records whose packet passed the CRC check (or is outside the dialect; see
    /// `unknown_frames`).
    pub frames: usize,
    pub v1: usize,
    pub v2: usize,
    pub start_us: u64,
    pub end_us: u64,
    /// `(system id, autopilot, vehicle type)` of the first autopilot heartbeat.
    pub vehicle: Option<(u8, String, String)>,
    pub max_relative_alt: Option<f32>,
    pub min_battery_voltage: Option<f32>,
    pub armed_seconds: f64,
    /// Mode changes in order, at most `MAX_LINES`.
    pub modes: Vec<String>,
    /// The vehicle's STATUSTEXT `(MAV_SEVERITY, text)`, oldest first, at most `MAX_LINES`.
    pub status: Vec<(u8, String)>,
    /// Decoded messages by name.
    pub message_counts: BTreeMap<&'static str, u64>,
    /// Frames with message ids outside the dialect: framed, but their CRC can't be checked.
    pub unknown_frames: usize,
    /// `(system id, component id)` → frames.
    pub components: BTreeMap<(u8, u8), usize>,
    /// Well-framed records whose packet failed the CRC check.
    pub bad_frames: u64,
    /// Bytes skipped while resynchronising.
    pub skipped_bytes: u64,
    /// The log ends inside a record.
    pub truncated: bool,
}

/// Cap on the summary's mode and status lists.
pub const MAX_LINES: usize = 200;

impl TlogSummary {
    pub fn duration_s(&self) -> f64 {
        self.end_us.saturating_sub(self.start_us) as f64 / 1e6
    }
    pub fn heartbeats(&self) -> u64 {
        self.message_counts.get("HEARTBEAT").copied().unwrap_or(0)
    }
    pub fn damaged(&self) -> bool {
        self.truncated || self.bad_frames > 0 || self.skipped_bytes > 0
    }
}

/// Summarises a `.tlog` using the same decoder as the live link. Reads the file in chunks,
/// so a log of any size (they grow to gigabytes) needs only a few megabytes.
pub fn summarize(path: &Path) -> io::Result<TlogSummary> {
    const CHUNK: usize = 4 << 20;
    let mut file = File::open(path)?;
    let mut summary = Summarizer::default();
    let mut scanner = Scanner::default();
    let mut buf: Vec<u8> = Vec::with_capacity(CHUNK + 8 + MAX_FRAME);
    loop {
        let have = buf.len();
        buf.resize(have + CHUNK, 0);
        let n = loop {
            match file.read(&mut buf[have..]) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                other => break other?,
            }
        };
        buf.truncate(have + n);
        let more = n > 0;
        let used = scanner.scan(&buf, more, &mut |stamp, frame| summary.record(stamp, frame));
        buf.drain(..used);
        if !more {
            return Ok(summary.finish(scanner));
        }
    }
}

/// `summarize` for a log already in memory.
pub fn summarize_bytes(bytes: &[u8]) -> TlogSummary {
    let mut summary = Summarizer::default();
    let mut scanner = Scanner::default();
    scanner.scan(bytes, false, &mut |stamp, frame| {
        summary.record(stamp, frame)
    });
    summary.finish(scanner)
}

#[derive(Default)]
struct Summarizer {
    s: TlogSummary,
    vehicle: Option<VehicleState>,
    assembler: StatusAssembler,
    armed_since: Option<u64>,
}

impl Summarizer {
    fn record(&mut self, stamp: u64, frame: &Frame) {
        let s = &mut self.s;
        if s.frames == 0 {
            s.start_us = stamp;
        }
        s.frames += 1;
        if frame.version() == MavlinkVersion::V1 {
            s.v1 += 1;
        } else {
            s.v2 += 1;
        }
        s.end_us = s.end_us.max(stamp);
        *s.components
            .entry((frame.system_id(), frame.component_id()))
            .or_default() += 1;
        let Some(msg) = frame.decode() else {
            s.unknown_frames += 1;
            return;
        };
        use mavlink::Message;
        *s.message_counts.entry(msg.message_name()).or_default() += 1;
        let now = Instant::now();
        if let Msg::HEARTBEAT(h) = &msg
            && self.vehicle.is_none()
            && modes::is_vehicle_heartbeat(h.autopilot as u8, h.mavtype as u8)
        {
            self.vehicle = Some(VehicleState::new(
                frame.system_id(),
                frame.component_id(),
                now,
            ));
        }
        let Some(v) = self.vehicle.as_mut() else {
            return;
        };
        if frame.system_id() != v.system_id {
            return;
        }
        if let Msg::STATUSTEXT(t) = &msg {
            let at = UNIX_EPOCH + Duration::from_micros(stamp);
            for entry in self.assembler.push(t, at) {
                if s.status.len() < MAX_LINES {
                    s.status.push((entry.severity, entry.text));
                }
            }
        }
        if frame.component_id() != v.component_id {
            return;
        }
        let was_armed = v.armed;
        v.apply(&msg, now);
        match (was_armed, v.armed) {
            (false, true) => self.armed_since = Some(stamp),
            (true, false) => {
                if let Some(start) = self.armed_since.take() {
                    s.armed_seconds += stamp.saturating_sub(start) as f64 / 1e6;
                }
            }
            _ => {}
        }
        if s.modes.len() < MAX_LINES && s.modes.last() != Some(&v.mode) && v.mode != "Unknown" {
            s.modes.push(v.mode.clone());
        }
        if let Some(alt) = v.relative_alt {
            s.max_relative_alt = Some(s.max_relative_alt.map_or(alt, |m| m.max(alt)));
        }
        if let Some(volts) = v.battery_voltage {
            s.min_battery_voltage = Some(s.min_battery_voltage.map_or(volts, |m| m.min(volts)));
        }
    }

    fn finish(self, scanner: Scanner) -> TlogSummary {
        let mut s = self.s;
        if let Some(start) = self.armed_since {
            s.armed_seconds += s.end_us.saturating_sub(start) as f64 / 1e6;
        }
        s.vehicle = self.vehicle.map(|v| {
            (
                v.system_id,
                v.autopilot.name().to_string(),
                modes::vehicle_type_name(v.vehicle_type).to_string(),
            )
        });
        s.bad_frames = scanner.bad_frames;
        s.skipped_bytes = scanner.skipped_bytes;
        s.truncated = scanner.truncated;
        s
    }
}

#[cfg(test)]
#[path = "tlog_tests.rs"]
mod tests;
