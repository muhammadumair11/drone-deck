//! Preflight and health (read-only): what the vehicle itself reports in its health and arming
//! check report (PX4 events), SYS_STATUS, GPS, battery, landed state and STATUSTEXT. "Ready to
//! arm" comes only from the vehicle: the event report when one arrived, else the SYS_STATUS
//! pre-arm bit, else a pre-arm STATUSTEXT, else "Not reported". PX4 sends the event report only
//! when a result changes, so the periodic SYS_STATUS bit checks it: when the two disagree the
//! stricter is shown, and once events were lost after the report the SYS_STATUS bit decides
//! (or, without one, the report is shown as possibly out of date).
use crate::State;
use crate::mavlink::{
    HealthReport, StatusText, VehicleState, component_names, mode_group, mode_group_names,
};
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
    /// From PX4's health and arming check report.
    Checks(Checks),
    NotReported,
}

/// What a PX4 health and arming check report says about arming.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checks {
    /// The heartbeat mode's group, when it has one.
    pub mode: Option<&'static str>,
    /// Whether the vehicle may arm in that mode; `None` when the mode has no group.
    pub can_arm: Option<bool>,
    /// Mode groups the vehicle may arm in.
    pub arm_modes: Vec<&'static str>,
    /// Components whose arming checks fail, and those that only warn.
    pub errors: Vec<&'static str>,
    pub warnings: Vec<&'static str>,
    /// The SYS_STATUS pre-arm bit for the current mode, when the vehicle sends it. PX4 derives
    /// both from the same checks; the stricter of the two is shown when they disagree.
    pub prearm: Option<bool>,
    /// Events went missing after the report, so a newer one may have been lost.
    pub outdated: bool,
}

/// The SYS_STATUS pre-arm bit, when the vehicle sends it.
fn prearm_bit(v: &VehicleState) -> Option<bool> {
    (v.sensors_present & PREARM_BIT != 0).then_some(v.sensors_health & PREARM_BIT != 0)
}

fn checks(v: &VehicleState, r: &HealthReport) -> Checks {
    let group = mode_group(v.autopilot, v.base_mode, v.custom_mode);
    Checks {
        mode: group.map(|(_, name)| name),
        can_arm: group.map(|(bit, _)| r.can_arm_in(bit)),
        arm_modes: mode_group_names(r.can_arm),
        errors: component_names(r.arming_errors),
        warnings: component_names(r.arming_warnings & !r.arming_errors),
        prearm: prearm_bit(v),
        outdated: v.health_events.outdated(),
    }
}

/// Whether the report and the SYS_STATUS pre-arm bit disagree about the current mode.
fn disagree(c: &Checks) -> bool {
    matches!((c.can_arm, c.prearm), (Some(a), Some(b)) if a != b)
}

/// The readiness line for a report, and its tone.
fn checks_line(c: &Checks) -> (String, Color) {
    let list = |names: &[&str]| names.join(", ");
    let issues = match (c.errors.is_empty(), c.warnings.is_empty()) {
        (true, true) => "no arming check errors or warnings".to_string(),
        (true, false) => format!("warnings: {}", list(&c.warnings)),
        (false, true) => format!("errors: {}", list(&c.errors)),
        (false, false) => format!(
            "errors: {} / warnings: {}",
            list(&c.errors),
            list(&c.warnings)
        ),
    };
    let (mut line, mut tone) = report_verdict(c, &issues);
    match (c.can_arm, c.prearm) {
        (Some(false), Some(true)) => {
            line.push_str(" / SYS_STATUS pre-arm check passes, the stricter report is shown")
        }
        (None, Some(pass)) => {
            line.push_str(if pass {
                " / SYS_STATUS pre-arm check passes"
            } else {
                " / SYS_STATUS pre-arm check fails"
            });
            if !pass {
                tone = danger();
            }
        }
        _ => {}
    }
    if c.outdated {
        line.push_str(" / events were missed since this report, so it may be out of date");
        if tone == success() {
            tone = warn();
        }
    }
    (line, tone)
}

fn report_verdict(c: &Checks, issues: &str) -> (String, Color) {
    let list = |names: &[&str]| names.join(", ");
    match (c.can_arm, c.mode) {
        // The report says ready but SYS_STATUS, sent every second or so, says the pre-arm
        // checks fail: one of them is out of date, so the stricter wins.
        (Some(true), Some(mode)) if c.prearm == Some(false) => (
            format!(
                "Not ready to arm in {mode} / SYS_STATUS says the pre-arm checks fail, \
                 the check report says ready / {issues}"
            ),
            danger(),
        ),
        (Some(true), Some(mode)) => (format!("Ready to arm in {mode} / {issues}"), success()),
        (Some(false), Some(mode)) if c.arm_modes.is_empty() => (
            format!("Not ready to arm in {mode} or any other mode / {issues}"),
            danger(),
        ),
        (Some(false), Some(mode)) => (
            format!(
                "Not ready to arm in {mode} / {issues} / can arm in {}",
                list(&c.arm_modes)
            ),
            danger(),
        ),
        _ if c.arm_modes.is_empty() => {
            (format!("Not ready to arm in any mode / {issues}"), danger())
        }
        _ => (
            format!(
                "Can arm in {} / the report does not cover the current mode / {issues}",
                list(&c.arm_modes)
            ),
            warn(),
        ),
    }
}

/// Where the readiness line comes from.
fn source(v: &VehicleState, ready: &Readiness) -> &'static str {
    match ready {
        Readiness::Armed => "HEARTBEAT armed flag",
        Readiness::Checks(c) if disagree(c) => {
            "PX4 health and arming check report (EVENT), disagrees with SYS_STATUS"
        }
        Readiness::Checks(_) => "PX4 health and arming check report (EVENT)",
        _ if v.sensors_present & PREARM_BIT != 0 && v.health_events.outdated() => {
            "SYS_STATUS pre-arm check bit / the EVENT report may be out of date"
        }
        _ if v.sensors_present & PREARM_BIT != 0 => "SYS_STATUS pre-arm check bit",
        Readiness::NotReady(Some(_)) => "STATUSTEXT pre-arm message",
        _ => "Nothing received yet",
    }
}

/// The health half of the report: which reported components have errors or warnings.
fn report_line(r: &HealthReport, outdated: bool) -> (String, Color) {
    let n = r.present.count_ones();
    let reported = format!("{n} component{} reported", if n == 1 { "" } else { "s" });
    let errors = component_names(r.health_errors);
    let warnings = component_names(r.health_warnings & !r.health_errors);
    let mut line = reported;
    if !errors.is_empty() {
        line.push_str(&format!(" / errors: {}", errors.join(", ")));
    }
    if !warnings.is_empty() {
        line.push_str(&format!(" / warnings: {}", warnings.join(", ")));
    }
    let mut tone = match (errors.is_empty(), warnings.is_empty()) {
        (false, _) => danger(),
        (true, false) => warn(),
        (true, true) => {
            line.push_str(" / no errors or warnings");
            fg()
        }
    };
    if outdated {
        line.push_str(" / events were missed since, it may be out of date");
        if tone == fg() {
            tone = warn();
        }
    }
    (line, tone)
}

/// A disarmed PX4 vehicle in Hold whose report blocks arming on a magnetometer error and warns
/// about the battery, still able to arm in Manual and Stabilized. For a headless scene of the
/// event-based readiness (`vehicle-health-checks` in `scenes.rs`).
pub(crate) fn px4_checks_fixture(v: &mut VehicleState) {
    const MAGNETOMETER: u32 = 1 << 27;
    const BATTERY: u32 = 1 << 11;
    const GPS: u32 = 1 << 3;
    v.armed = false;
    v.base_mode = 1;
    v.custom_mode = (4 << 16) | (3 << 24);
    v.mode = "Hold".into();
    // SYS_STATUS agrees: the pre-arm checks fail in Hold, and the magnetometer is unhealthy.
    const SYS_STATUS_MAG: u32 = 1 << 2;
    v.sensors_present |= PREARM_BIT;
    v.sensors_health &= !(PREARM_BIT | SYS_STATUS_MAG);
    v.status
        .retain(|t| !t.text.starts_with("Ready for takeoff"));
    v.health_events.report = Some(HealthReport {
        arming_errors: MAGNETOMETER,
        arming_warnings: BATTERY,
        can_arm: 1 | (1 << 15),
        can_run: 1 | (1 << 15),
        present: GPS | BATTERY | MAGNETOMETER | (1 << 28) | (1 << 29),
        health_errors: MAGNETOMETER,
        health_warnings: BATTERY,
    });
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

/// PX4's arming check report when the vehicle sent one (unless events were lost since and a
/// SYS_STATUS pre-arm bit is there to go by); otherwise the SYS_STATUS pre-arm bit
/// when the vehicle sends it; otherwise a pre-arm failure STATUSTEXT means "not ready";
/// nothing else is read as ready.
pub(crate) fn readiness(v: &VehicleState, status: &[&StatusText]) -> Readiness {
    if v.armed {
        return Readiness::Armed;
    }
    if let Some(r) = v.health_report() {
        // A report events were lost after yields to a SYS_STATUS bit the vehicle keeps sending.
        if !(v.health_events.outdated() && prearm_bit(v).is_some()) {
            return Readiness::Checks(checks(v, r));
        }
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
        .child(meta_sized(value, 10., color).wrap().shrink())
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
    let readiness = readiness(v, &status);
    let source = source(v, &readiness);
    let (ready, tone) = match readiness {
        Readiness::Armed => ("Armed".to_string(), fg()),
        Readiness::Checks(c) => checks_line(&c),
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
    let mut facts = col()
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
        .child(fact(
            "health-source",
            "Arming source",
            source.into(),
            muted(),
        ));
    if let Some(r) = v.health_report() {
        let (line, tone) = report_line(r, v.health_events.outdated());
        facts = facts.child(fact("health-report", "Health report", line, tone));
    }
    let facts = facts
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
                .child(meta(
                    match (v.health_report(), v.unhealthy_sensors().count_ones()) {
                        (Some(r), _) if r.error_components() != 0 => {
                            let n = r.error_components().count_ones();
                            format!(
                                "Read-only / {n} component{} with errors",
                                if n == 1 { "" } else { "s" }
                            )
                        }
                        (_, 0) => "Read-only / as reported by the vehicle".to_string(),
                        (_, n) => format!(
                            "Read-only / {n} unhealthy sensor{}",
                            if n == 1 { "" } else { "s" }
                        ),
                    },
                )),
        )
        .child(asset(facts, messages(s), compact).pt(4.));
    if !fresh {
        block = block.dim(0.45);
    }
    block
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
