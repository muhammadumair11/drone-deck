//! Flight summary from a parsed ULog. Every figure comes from a logged topic; a topic that
//! was not logged leaves its figure `None` ("Not logged"), never zero.
use super::parse::{Issues, LogMessage, Ulog};

/// PX4 `vehicle_status.arming_state` value for armed (same in old and new message sets).
const ARMED: f64 = 2.;
/// A horizontal step longer than this between two samples is an estimator reset, not travel.
const RESET_STEP_M: f64 = 100.;
/// Warnings and errors kept for display.
pub(crate) const MAX_MESSAGES: usize = 200;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Estimator {
    /// Samples of `estimator_status` and how many had a filter fault or a failed innovation check.
    pub samples: usize,
    pub fault_samples: usize,
    pub fault_flags: u64,
    pub innovation_fail_samples: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Summary {
    /// Seconds between the first and the last data sample.
    pub duration_s: f64,
    /// First data timestamp (µs since boot); every `*_s` time below is relative to it.
    pub first_us: u64,
    /// Wall-clock start (µs since the Unix epoch, UTC) when GPS time was logged.
    pub utc_start_us: Option<u64>,
    /// `(label, value)`: system name, hardware, software, OS.
    pub vehicle: Vec<(&'static str, String)>,
    /// Highest point above the local origin (`-vehicle_local_position.z`).
    pub max_alt_rel_m: Option<f64>,
    /// Highest `vehicle_global_position.alt` (AMSL).
    pub max_alt_amsl_m: Option<f64>,
    /// Horizontal path length from local position.
    pub distance_m: Option<f64>,
    /// Highest horizontal ground speed.
    pub max_speed_ms: Option<f64>,
    pub battery_min_v: Option<f64>,
    /// Lowest remaining charge, 0–100.
    pub battery_min_pct: Option<f64>,
    /// Times (s) `vehicle_status.failsafe` turned on; `None` when the field was not logged.
    pub failsafes: Option<Vec<f64>>,
    /// Armed intervals (s); an interval still open at the end closes at the last sample.
    pub armed: Option<Vec<(f64, f64)>>,
    pub warnings: usize,
    pub errors: usize,
    /// Warnings and errors (level ≤ 4), oldest first, at most `MAX_MESSAGES`.
    pub messages: Vec<LogMessage>,
    pub dropouts: usize,
    pub dropout_ms: u64,
    pub estimator: Option<Estimator>,
    pub topics: usize,
    pub params: usize,
    /// ULog file format version from the header.
    pub format_version: u8,
    pub issues: Issues,
}

fn rel(log_first: u64, t: u64) -> f64 {
    t.saturating_sub(log_first) as f64 / 1e6
}
fn max_finite(v: impl Iterator<Item = f64>) -> Option<f64> {
    v.filter(|x| x.is_finite())
        .fold(None, |m, x| Some(m.map_or(x, |m: f64| m.max(x))))
}
fn min_finite(v: impl Iterator<Item = f64>) -> Option<f64> {
    v.filter(|x| x.is_finite())
        .fold(None, |m, x| Some(m.map_or(x, |m: f64| m.min(x))))
}

/// `ver_sw_release` is `0xMMmmppTT`: major, minor, patch, type (255 = release).
fn release(v: f64) -> String {
    let v = v as u32;
    let (major, minor, patch, kind) = (v >> 24, (v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff);
    let tag = match kind {
        255 => "",
        192..=254 => " rc",
        128..=191 => " beta",
        64..=127 => " alpha",
        _ => " dev",
    };
    format!("v{major}.{minor}.{patch}{tag}")
}

fn vehicle(log: &Ulog) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for (label, key) in [
        ("System", "sys_name"),
        ("Hardware", "ver_hw"),
        ("OS", "sys_os_name"),
    ] {
        if let Some(v) = log.info_text(key).filter(|v| !v.is_empty()) {
            out.push((label, v));
        }
    }
    let release = log
        .info
        .get("ver_sw_release")
        .and_then(|v| v.as_f64())
        .filter(|&v| v > 0.)
        .map(release);
    let hash = log
        .info_text("ver_sw")
        .map(|h| h.chars().take(8).collect::<String>());
    match (release, hash) {
        (Some(r), Some(h)) => out.push(("Software", format!("{r} ({h})"))),
        (Some(r), None) => out.push(("Software", r)),
        (None, Some(h)) => out.push(("Software", h)),
        _ => {}
    }
    out
}

fn transitions_on(t: &[u64], v: &[f64], first: u64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut prev = false;
    for (&t, &v) in t.iter().zip(v) {
        let on = v != 0.;
        if on && !prev {
            out.push(rel(first, t));
        }
        prev = on;
    }
    out
}
fn periods(t: &[u64], on: impl Fn(f64) -> bool, v: &[f64], first: u64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut open: Option<f64> = None;
    for (&t, &v) in t.iter().zip(v) {
        match (on(v), open) {
            (true, None) => open = Some(rel(first, t)),
            (false, Some(s)) => {
                out.push((s, rel(first, t)));
                open = None;
            }
            _ => {}
        }
    }
    if let (Some(s), Some(&last)) = (open, t.last()) {
        out.push((s, rel(first, last)));
    }
    out
}

fn utc_start(log: &Ulog, first: u64) -> Option<u64> {
    for topic in ["vehicle_gps_position", "sensor_gps"] {
        for i in log.instances(topic) {
            let Some(s) = log.series(topic, i, "time_utc_usec") else {
                continue;
            };
            if let Some((t, utc)) = s.t_us.iter().zip(&s.v).find(|(_, u)| **u > 1e15) {
                return Some((*utc as u64).saturating_sub(t.saturating_sub(first)));
            }
        }
    }
    None
}

fn local_motion(log: &Ulog, s: &mut Summary) {
    let Some(i) = log.first_instance("vehicle_local_position", &["z"]) else {
        return;
    };
    if let Some(z) = log.series("vehicle_local_position", i, "z") {
        s.max_alt_rel_m = max_finite(z.v.iter().map(|z| -z));
    }
    let has_valid = log
        .first_instance("vehicle_local_position", &["xy_valid"])
        .is_some();
    let fields: &[&str] = if has_valid {
        &["x", "y", "vx", "vy", "xy_valid"]
    } else {
        &["x", "y", "vx", "vy"]
    };
    let Some((_, c)) = log.series_many("vehicle_local_position", i, fields) else {
        return;
    };
    let valid = |k: usize| !has_valid || c[4][k] != 0.;
    s.max_speed_ms = max_finite(
        (0..c[0].len())
            .filter(|&k| valid(k))
            .map(|k| c[2][k].hypot(c[3][k])),
    );
    let mut distance = 0.;
    let mut last: Option<(f64, f64)> = None;
    for k in (0..c[0].len()).filter(|&k| valid(k)) {
        let (x, y) = (c[0][k], c[1][k]);
        if !(x.is_finite() && y.is_finite()) {
            continue;
        }
        if let Some((px, py)) = last {
            let step = (x - px).hypot(y - py);
            if step < RESET_STEP_M {
                distance += step;
            }
        }
        last = Some((x, y));
    }
    s.distance_m = Some(distance);
}

fn battery(log: &Ulog, s: &mut Summary) {
    for i in log.instances("battery_status") {
        let connected = log.series("battery_status", i, "connected");
        if let Some(v) = log.series("battery_status", i, "voltage_v") {
            let ok = |k: usize| {
                connected
                    .as_ref()
                    .is_none_or(|c| c.v.get(k).is_some_and(|&c| c != 0.))
            };
            let min = min_finite(
                (0..v.v.len())
                    .filter(|&k| ok(k) && v.v[k] > 0.5)
                    .map(|k| v.v[k]),
            );
            s.battery_min_v = match (s.battery_min_v, min) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        if let Some(r) = log.series("battery_status", i, "remaining") {
            let min = min_finite(r.v.iter().copied().filter(|&r| (0. ..=1.).contains(&r)));
            s.battery_min_pct = match (s.battery_min_pct, min.map(|r| r * 100.)) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
    }
}

fn status(log: &Ulog, s: &mut Summary, first: u64) {
    if let Some(i) = log.first_instance("vehicle_status", &["failsafe"])
        && let Some(f) = log.series("vehicle_status", i, "failsafe")
    {
        s.failsafes = Some(transitions_on(&f.t_us, &f.v, first));
    }
    if let Some(i) = log.first_instance("vehicle_status", &["arming_state"])
        && let Some(a) = log.series("vehicle_status", i, "arming_state")
    {
        s.armed = Some(periods(&a.t_us, |v| v == ARMED, &a.v, first));
    } else if let Some(i) = log.first_instance("actuator_armed", &["armed"])
        && let Some(a) = log.series("actuator_armed", i, "armed")
    {
        s.armed = Some(periods(&a.t_us, |v| v != 0., &a.v, first));
    }
}

fn estimator(log: &Ulog) -> Option<Estimator> {
    let i = log.first_instance("estimator_status", &["filter_fault_flags"])?;
    let f = log.series("estimator_status", i, "filter_fault_flags")?;
    let mut e = Estimator {
        samples: f.v.len(),
        ..Default::default()
    };
    for &v in &f.v {
        if v != 0. {
            e.fault_samples += 1;
            e.fault_flags |= v as u64;
        }
    }
    if let Some(inn) = log.series("estimator_status", i, "innovation_check_flags") {
        e.innovation_fail_samples = inn.v.iter().filter(|&&v| v != 0.).count();
    }
    Some(e)
}

pub(crate) fn summarize(log: &Ulog) -> Summary {
    let first = log.first_us.unwrap_or(log.start_us);
    let mut s = Summary {
        duration_s: match (log.first_us, log.last_us) {
            (Some(a), Some(b)) => rel(a, b),
            _ => 0.,
        },
        first_us: first,
        utc_start_us: utc_start(log, first),
        vehicle: vehicle(log),
        dropouts: log.dropouts.len(),
        dropout_ms: log.dropouts.iter().map(|d| d.ms as u64).sum(),
        estimator: estimator(log),
        topics: log.topics.iter().filter(|t| t.len() > 0).count(),
        params: log.params.len(),
        format_version: log.version,
        issues: log.issues.clone(),
        ..Default::default()
    };
    local_motion(log, &mut s);
    if let Some(i) = log.first_instance("vehicle_global_position", &["alt"])
        && let Some(a) = log.series("vehicle_global_position", i, "alt")
    {
        s.max_alt_amsl_m = max_finite(a.v.into_iter());
    }
    battery(log, &mut s);
    status(log, &mut s, first);
    for m in &log.messages {
        match m.level {
            0..=3 => s.errors += 1,
            4 => s.warnings += 1,
            _ => continue,
        }
        if s.messages.len() < MAX_MESSAGES {
            s.messages.push(m.clone());
        }
    }
    s
}
