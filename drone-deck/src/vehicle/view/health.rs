//! Preflight and health (read-only): what the vehicle itself reports in SYS_STATUS, GPS,
//! battery, landed state and STATUSTEXT. "Ready to arm" comes only from the vehicle.
use crate::State;
use crate::mavlink::{StatusText, VehicleState};
use crate::ui::{kit::*, theme::*};
use rune::{Color, El, col, row};

/// MAV_SYS_STATUS_SENSOR bit names.
const SENSORS: [&str; 31] = [
    "Gyroscope",
    "Accelerometer",
    "Magnetometer",
    "Absolute pressure",
    "Differential pressure",
    "GPS",
    "Optical flow",
    "Vision position",
    "Laser position",
    "External ground truth",
    "Angular rate control",
    "Attitude stabilization",
    "Yaw position",
    "Altitude control",
    "Position control",
    "Motor outputs",
    "RC receiver",
    "Gyroscope 2",
    "Accelerometer 2",
    "Magnetometer 2",
    "Geofence",
    "AHRS",
    "Terrain",
    "Reverse motor",
    "Logging",
    "Battery",
    "Proximity",
    "Satellite comms",
    "Pre-arm check",
    "Obstacle avoidance",
    "Propulsion",
];
/// MAV_SYS_STATUS_PREARM_CHECK: healthy when the flight stack's pre-arm checks pass.
pub(crate) const PREARM_BIT: u32 = 1 << 28;
/// Messages shown, newest first.
const MESSAGES: usize = 8;

/// Whether the vehicle may arm, in its own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Readiness {
    Armed,
    Ready,
    NotReady(Option<String>),
    NotReported,
}

fn prearm_text(t: &StatusText) -> bool {
    let lower = t.text.to_ascii_lowercase();
    [
        "prearm",
        "pre-arm",
        "preflight fail",
        "arming denied",
        "arm denied",
    ]
    .iter()
    .any(|w| lower.contains(w))
}

/// The SYS_STATUS pre-arm bit when the vehicle sends it; otherwise a pre-arm failure
/// STATUSTEXT means "not ready"; nothing else is read as ready.
pub(crate) fn readiness(v: &VehicleState, status: &[&StatusText]) -> Readiness {
    if v.armed {
        return Readiness::Armed;
    }
    let failure = status
        .iter()
        .rev()
        .find(|t| t.severity <= 4 && prearm_text(t))
        .map(|t| t.text.clone());
    if v.sensors_present & PREARM_BIT != 0 {
        return if v.sensors_health & PREARM_BIT != 0 {
            Readiness::Ready
        } else {
            Readiness::NotReady(failure)
        };
    }
    match failure {
        Some(text) => Readiness::NotReady(Some(text)),
        None => Readiness::NotReported,
    }
}

/// STATUSTEXT colour by MAV_SEVERITY.
pub(crate) fn severity_color(severity: u8) -> Color {
    match severity {
        0..=3 => danger(),
        4 => warn(),
        5 => fg(),
        _ => muted(),
    }
}

fn cell(t: impl Into<String>, color: Color) -> El {
    meta_sized(t, 9., color).wrap()
}

fn sensor_rows(v: &VehicleState, compact: bool) -> El {
    let mut list = col().key("health-sensors").w_full().min_w(0.);
    if v.sensors_present == 0 {
        return list.child(notice("No SYS_STATUS sensor report yet."));
    }
    if !compact {
        list = list.child(
            row()
                .w_full()
                .py(8.)
                .gap(12.)
                .a11y_hidden()
                .child(cell("Sensor", muted()).grow(1.4))
                .child(cell("Present", muted()).grow(1.))
                .child(cell("Enabled", muted()).grow(1.))
                .child(cell("Health", muted()).grow(1.)),
        );
    }
    for (bit, name) in SENSORS.iter().enumerate() {
        let mask = 1u32 << bit;
        if v.sensors_present & mask == 0 || mask == PREARM_BIT {
            continue;
        }
        let enabled = v.sensors_enabled & mask != 0;
        let healthy = v.sensors_health & mask != 0;
        let (health, tone) = match (enabled, healthy) {
            (false, _) => ("Not enabled", muted()),
            (true, true) => ("Healthy", success()),
            (true, false) => ("Unhealthy", danger()),
        };
        let state = row()
            .gap(8.)
            .center_y()
            .child(dot(tone))
            .child(cell(health, fg()));
        let line = ruled(false)
            .key(format!("health-sensor-{bit}"))
            .py(8.)
            .gap(12.)
            .center_y()
            .child(caps(*name, 10., 500., 0.045, fg()).wrap().grow(1.4));
        list = list.child(if compact {
            line.child(state)
        } else {
            line.child(cell("Present", muted()).grow(1.))
                .child(cell(if enabled { "Enabled" } else { "Disabled" }, muted()).grow(1.))
                .child(state.grow(1.))
        });
    }
    list
}

fn fact(key: &str, label: &str, value: String, color: Color) -> El {
    ruled(false)
        .key(key.to_string())
        .py(10.)
        .gap(12.)
        .center_y()
        .justify_between()
        .child(meta_sized(label, 9., muted()))
        .child(meta_sized(value, 10., color).wrap())
}

fn messages(s: &State) -> El {
    let mut list = col()
        .key("vehicle-messages")
        .w_full()
        .min_w(0.)
        .gap(2.)
        .child(meta("Vehicle messages / newest first"));
    if s.vehicle.status.is_empty() {
        return list.child(notice("No STATUSTEXT received on this link."));
    }
    for (i, t) in s.vehicle.status.iter().rev().take(MESSAGES).enumerate() {
        list = list.child(
            ruled(false)
                .key(format!("vehicle-message-{i}"))
                .py(8.)
                .gap(12.)
                .items_start()
                .child(meta_sized(t.severity_name(), 9., severity_color(t.severity)).w(74.))
                .child(code(&t.text, 11., fg()).wrap_anywhere().grow(1.)),
        );
    }
    list
}

/// The read-only preflight and health block (key `vehicle-health`).
pub(crate) fn health(s: &State, v: &VehicleState, fresh: bool, vp: &Viewport) -> El {
    let compact = vp.compact();
    let status: Vec<&StatusText> = s.vehicle.status.iter().collect();
    let (ready, tone) = match readiness(v, &status) {
        Readiness::Armed => ("Armed".to_string(), fg()),
        Readiness::Ready => (
            "Ready to arm / the vehicle reports its pre-arm checks pass".to_string(),
            success(),
        ),
        Readiness::NotReady(Some(why)) => (format!("Not ready to arm / {why}"), danger()),
        Readiness::NotReady(None) => (
            "Not ready to arm / the vehicle reports failing pre-arm checks".to_string(),
            danger(),
        ),
        Readiness::NotReported => (
            "Not reported / the vehicle has not said whether it can arm".to_string(),
            muted(),
        ),
    };
    let gps = format!(
        "{} / {} / eph {}",
        v.gps_fix_name(),
        v.satellites
            .map_or_else(|| "sats not reported".into(), |n| format!("{n} sats")),
        v.eph
            .map_or_else(|| "not reported".into(), |e| format!("{e:.1} m"))
    );
    let battery = match (v.battery_voltage, v.battery_remaining) {
        (None, None) => "Not reported".to_string(),
        (volts, pct) => format!(
            "{} / {}",
            volts.map_or_else(|| "V not reported".into(), |b| format!("{b:.1} V")),
            pct.map_or_else(|| "% not reported".into(), |p| format!("{p} %"))
        ),
    };
    let landed = if v.landed_state == 0 {
        "Not reported".to_string()
    } else {
        v.landed_name().to_string()
    };
    let facts = col()
        .w_full()
        .min_w(0.)
        .child(
            row()
                .key("vehicle-ready")
                .role("status")
                .w_full()
                .min_w(0.)
                .py(10.)
                .gap(10.)
                .center_y()
                .child(dot(tone))
                .child(meta_sized(ready, 10., fg()).wrap().grow(1.)),
        )
        .child(fact("health-gps", "GPS", gps, fg()))
        .child(fact("health-battery", "Battery", battery, fg()))
        .child(fact("health-landed", "Landed state", landed, fg()))
        .child(sensor_rows(v, compact));
    let mut block = col()
        .key("vehicle-health")
        .w_full()
        .min_w(0.)
        .pt(18.)
        .child(
            row()
                .w_full()
                .gaps(6., 24.)
                .flex_wrap()
                .justify_between()
                .pb(8.)
                .child(h3("Preflight & health", 15.))
                .child(meta(match v.unhealthy_sensors().count_ones() {
                    0 => "Read-only / as reported by the vehicle".to_string(),
                    n => format!(
                        "Read-only / {n} unhealthy sensor{}",
                        if n == 1 { "" } else { "s" }
                    ),
                })),
        )
        .child(asset(facts, messages(s), compact).pt(4.));
    if !fresh {
        block = block.dim(0.45);
    }
    block
}
