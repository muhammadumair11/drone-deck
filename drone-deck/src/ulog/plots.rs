//! Plot-ready series: altitude, battery, attitude and vibration, reduced to a few hundred
//! points so a cached analysis stays small whatever the log size.
use super::parse::Ulog;

/// Buckets per line; each keeps its minimum and maximum, so spikes survive reduction.
const BUCKETS: usize = 240;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Line {
    pub label: &'static str,
    /// `[seconds from the first sample, value]`; a non-finite value breaks the line.
    pub points: Vec<[f32; 2]>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plot {
    /// Stable key suffix: `altitude`, `battery`, `attitude`, `vibration`.
    pub id: &'static str,
    pub title: &'static str,
    pub unit: &'static str,
    /// Where the data came from, e.g. `vehicle_local_position.z`.
    pub source: &'static str,
    pub lines: Vec<Line>,
}
impl Plot {
    /// Finite value range over every line.
    pub(crate) fn range(&self) -> Option<(f32, f32)> {
        let mut r: Option<(f32, f32)> = None;
        for p in self.lines.iter().flat_map(|l| &l.points) {
            if p[1].is_finite() {
                r = Some(r.map_or((p[1], p[1]), |(a, b)| (a.min(p[1]), b.max(p[1]))));
            }
        }
        r
    }
}

/// Min/max decimation that keeps time order.
pub(crate) fn reduce(t: &[f64], v: &[f64]) -> Vec<[f32; 2]> {
    let n = t.len().min(v.len());
    if n <= BUCKETS * 2 {
        return (0..n).map(|k| [t[k] as f32, v[k] as f32]).collect();
    }
    let mut out = Vec::with_capacity(BUCKETS * 2);
    for b in 0..BUCKETS {
        let (lo, hi) = (
            b * n / BUCKETS,
            ((b + 1) * n / BUCKETS).max(b * n / BUCKETS + 1),
        );
        let slice = lo..hi.min(n);
        let finite = || slice.clone().filter(|&k| v[k].is_finite());
        let (Some(min), Some(max)) = (
            finite().min_by(|&a, &b| v[a].total_cmp(&v[b])),
            finite().max_by(|&a, &b| v[a].total_cmp(&v[b])),
        ) else {
            out.push([t[lo] as f32, f32::NAN]);
            continue;
        };
        for k in if min <= max { [min, max] } else { [max, min] } {
            out.push([t[k] as f32, v[k] as f32]);
        }
    }
    out
}

fn seconds(t: &[u64], first: u64) -> Vec<f64> {
    t.iter()
        .map(|&t| t.saturating_sub(first) as f64 / 1e6)
        .collect()
}

fn line(
    log: &Ulog,
    first: u64,
    topic: &str,
    field: &str,
    label: &'static str,
    f: impl Fn(f64) -> f64,
) -> Option<Line> {
    let i = log.first_instance(topic, &[field])?;
    let s = log.series(topic, i, field)?;
    if s.is_empty() {
        return None;
    }
    let v: Vec<f64> = s.v.iter().map(|&v| f(v)).collect();
    Some(Line {
        label,
        points: reduce(&seconds(&s.t_us, first), &v),
    })
}

fn attitude(log: &Ulog, first: u64) -> Option<Plot> {
    let fields = ["q[0]", "q[1]", "q[2]", "q[3]"];
    let i = log.first_instance("vehicle_attitude", &fields)?;
    let (t, q) = log.series_many("vehicle_attitude", i, &fields)?;
    if t.is_empty() {
        return None;
    }
    let (mut roll, mut pitch) = (Vec::with_capacity(t.len()), Vec::with_capacity(t.len()));
    for k in 0..t.len() {
        let (w, x, y, z) = (q[0][k], q[1][k], q[2][k], q[3][k]);
        roll.push(
            (2. * (w * x + y * z))
                .atan2(1. - 2. * (x * x + y * y))
                .to_degrees(),
        );
        pitch.push((2. * (w * y - z * x)).clamp(-1., 1.).asin().to_degrees());
    }
    let ts = seconds(&t, first);
    Some(Plot {
        id: "attitude",
        title: "Attitude",
        unit: "deg",
        source: "vehicle_attitude.q",
        lines: vec![
            Line {
                label: "Roll",
                points: reduce(&ts, &roll),
            },
            Line {
                label: "Pitch",
                points: reduce(&ts, &pitch),
            },
        ],
    })
}

/// The plots this log can draw, in display order; topics that were not logged are left out.
pub(crate) fn extract(log: &Ulog, first: u64) -> Vec<Plot> {
    let mut plots = Vec::new();
    if let Some(l) = line(
        log,
        first,
        "vehicle_local_position",
        "z",
        "Above origin",
        |z| -z,
    ) {
        plots.push(Plot {
            id: "altitude",
            title: "Altitude",
            unit: "m",
            source: "vehicle_local_position.z",
            lines: vec![l],
        });
    } else if let Some(l) = line(log, first, "vehicle_global_position", "alt", "AMSL", |a| a) {
        plots.push(Plot {
            id: "altitude",
            title: "Altitude",
            unit: "m",
            source: "vehicle_global_position.alt",
            lines: vec![l],
        });
    }
    if let Some(l) = line(log, first, "battery_status", "voltage_v", "Voltage", |v| v) {
        plots.push(Plot {
            id: "battery",
            title: "Battery",
            unit: "V",
            source: "battery_status.voltage_v",
            lines: vec![l],
        });
    }
    plots.extend(attitude(log, first));
    if let Some(l) = line(
        log,
        first,
        "vehicle_imu_status",
        "accel_vibration_metric",
        "Accel",
        |v| v,
    ) {
        plots.push(Plot {
            id: "vibration",
            title: "Vibration",
            unit: "m/s²",
            source: "vehicle_imu_status.accel_vibration_metric",
            lines: vec![l],
        });
    }
    plots
}
