//! A real ArduPilot flight brought in for re-simulation, from its DataFlash log (`.bin`), the
//! counterpart of `import` for PX4. Drone Deck writes what a run needs: the commands the vehicle
//! took from outside (`commands.jsonl`), what it reported (`evidence.jsonl`, as /ap/status and
//! /ap/pose/filtered messages for the comparison), its parameters, home position and version, and
//! a recipe that flies it in ArduPilot SITL at that home.
//!
//! What counts as a command from outside:
//! - a Guided velocity target (GUIP of the VelAccel kind, logged for each MAVLink or ROS 2
//!   velocity command while in Guided), replayed as `/ap/cmd_vel` in the map frame;
//! - a mode change the pilot, a ground station or companion, or ROS 2 asked for (MODE with reason
//!   RC_COMMAND, GCS_COMMAND or DDS_COMMAND), replayed as a `/ap/mode_switch` call;
//! - arming by the pilot, MAVLink or ROS 2 (ARM with method RUDDER, MAVLINK, AUXSWITCH or DDS),
//!   replayed as `/ap/arm_motors`;
//! - a copter taking off in Guided, or switched into Guided while already flying, replayed as a
//!   `/ap/experimental/takeoff` call to the height it reached (the log keeps no takeoff call).
//!
//! Not replayed from a log: Guided position targets (an MCAP of the drone keeps them as
//! `/ap/cmd_gps_pose`), yaw rates (GUIP has none), attitude targets, RC sticks and disarming.
//! Copter 4.6 and earlier log guided targets north-east-up in centimetres, later versions
//! north-east-down in metres; the version in the log decides which.
use super::import::{Export, line};
use super::recipe::{Home, Recipe, SCHEMA};
use crate::dataflash::Log;
use crate::mavlink::{Param, ParamSet, ParamValue};
use crate::systems::adapter::AutopilotKind;
use crate::systems::model::World;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The messages an import reads.
pub(crate) const WANTED: &[&str] = &["PARM", "MODE", "ARM", "EV", "ORGN", "POS", "GUIP", "MSG"];
pub(crate) const MANUAL: &str = "This flight has no ROS 2 or Guided commands to replay: it was flown by hand or by ArduPilot's own modes.";

const STATUS: &str = "/ap/status";
const POSE: &str = "/ap/pose/filtered";
const CMD_VEL: &str = "/ap/cmd_vel";
const ARM: &str = "/ap/arm_motors";
const MODE: &str = "/ap/mode_switch";
const TAKEOFF: &str = "/ap/experimental/takeoff";
/// ModeReason of changes asked for from outside: RC_COMMAND, GCS_COMMAND, DDS_COMMAND.
const ASKED_MODE: [i64; 3] = [1, 2, 52];
/// ModeReason of changes a failsafe made (radio, battery, GCS, EKF, GPS glitch, fence, terrain,
/// crash, general, leak, dead reckoning): the comparison shows them as failsafes.
const FAILSAFE_MODE: [i64; 11] = [3, 4, 5, 6, 7, 10, 11, 19, 25, 29, 50];
/// AP_Arming::Method of arming by the pilot or from outside: RUDDER, MAVLINK, AUXSWITCH, DDS.
const ASKED_ARM: [i64; 4] = [0, 1, 2, 35];
/// LogEvent ids: the vehicle landed, and left the ground.
const EV_LAND_COMPLETE: i64 = 18;
const EV_NOT_LANDED: i64 = 28;
/// Copter's Guided mode, and Guided's velocity submode as GUIP logs it.
const GUIDED: i64 = 4;
const SUB_VEL_ACCEL: i64 = 4;
const TAKEOFF_SEARCH_US: u64 = 30_000_000;
const DEFAULT_TAKEOFF_M: f64 = 2.5;
/// Metres per degree of latitude (and of longitude at the equator).
const M_PER_DEG: f64 = 111_319.49;

/// The vehicle, its ardupilot_msgs/Status type, and the firmware version, from the log's
/// startup message ("ArduCopter V4.6.3 (…)").
fn firmware(log: &Log) -> (String, u8, String) {
    let text = log
        .rows("MSG")
        .filter_map(|r| r.text("Message").map(str::to_string))
        .find(|m| m.starts_with("Ardu") || m.starts_with("Rover"))
        .unwrap_or_default();
    let mut words = text.split_whitespace();
    let name = words.next().unwrap_or("");
    let version = words.next().unwrap_or("").to_string();
    let (vehicle, kind) = match name {
        "ArduPlane" => ("plane", 3),
        "ArduRover" | "Rover" => ("rover", 1),
        "ArduSub" => ("sub", 7),
        _ => ("copter", 2),
    };
    (vehicle.into(), kind, version)
}

/// Whether `version` ("V4.6.3") logs guided targets north-east-down in metres (4.7 and later).
fn ned_targets(version: &str) -> bool {
    let mut parts = version
        .trim_start_matches(['V', 'v'])
        .split('.')
        .map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    (major, minor) >= (4, 7)
}

fn stamp(t_us: u64) -> Value {
    json!({"sec": t_us / 1_000_000, "nanosec": (t_us % 1_000_000) * 1000})
}

/// The vehicle's state at each change of mode, arming or landing, in time order.
#[derive(Clone, Copy, Default)]
struct State {
    mode: i64,
    armed: bool,
    flying: bool,
    failsafe: Option<i64>,
}

/// (time, what changed) of the mode, arming and landing messages, in time order.
enum Change {
    Mode { mode: i64, reason: i64 },
    Arm { armed: bool, method: i64 },
    Flying(bool),
}

fn changes(log: &Log) -> Vec<(u64, Change)> {
    let mut out: Vec<(u64, Change)> = Vec::new();
    for r in log.rows("MODE") {
        let mode = r.i64("ModeNum").or_else(|| r.i64("Mode"));
        if let (Some(t), Some(mode)) = (r.time_us(), mode) {
            let reason = r.i64("Rsn").unwrap_or(0);
            out.push((t, Change::Mode { mode, reason }));
        }
    }
    for r in log.rows("ARM") {
        if let (Some(t), Some(state)) = (r.time_us(), r.i64("ArmState")) {
            let method = r.i64("Method").unwrap_or(-1);
            out.push((
                t,
                Change::Arm {
                    armed: state != 0,
                    method,
                },
            ));
        }
    }
    for r in log.rows("EV") {
        match (r.time_us(), r.i64("Id")) {
            (Some(t), Some(EV_NOT_LANDED)) => out.push((t, Change::Flying(true))),
            (Some(t), Some(EV_LAND_COMPLETE)) => out.push((t, Change::Flying(false))),
            _ => {}
        }
    }
    out.sort_by_key(|(t, _)| *t);
    out
}

/// The origin positions are measured from: the EKF origin (ORGN type 0), else the first one.
fn origin(log: &Log) -> Option<(f64, f64, f64)> {
    let rows: Vec<_> = log.rows("ORGN").collect();
    let pick = rows
        .iter()
        .find(|r| r.i64("Type") == Some(0))
        .or(rows.first())?;
    Some((pick.f64("Lat")?, pick.f64("Lng")?, pick.f64("Alt")?))
}

fn home(log: &Log) -> Option<Home> {
    let rows: Vec<_> = log.rows("ORGN").collect();
    let pick = rows
        .iter()
        .find(|r| r.i64("Type") == Some(1))
        .or(rows.first())?;
    let (lat, lon, alt) = (pick.f64("Lat")?, pick.f64("Lng")?, pick.f64("Alt")?);
    (lat != 0. || lon != 0.).then_some(Home {
        lat,
        lon,
        alt: alt as f32,
    })
}

/// (time, east, north, up) of each POS, from the origin.
fn positions(log: &Log) -> Vec<(u64, f64, f64, f64)> {
    let Some((lat0, lon0, alt0)) = origin(log).or_else(|| {
        let r = log.rows("POS").next()?;
        Some((r.f64("Lat")?, r.f64("Lng")?, r.f64("Alt")?))
    }) else {
        return Vec::new();
    };
    log.rows("POS")
        .filter_map(|r| {
            let (t, lat, lon) = (r.time_us()?, r.f64("Lat")?, r.f64("Lng")?);
            let up = r
                .f64("RelOriginAlt")
                .filter(|v| v.is_finite())
                .or_else(|| r.f64("Alt").map(|a| a - alt0))?;
            let north = (lat - lat0) * M_PER_DEG;
            let east = (lon - lon0) * M_PER_DEG * lat0.to_radians().cos();
            Some((t, east, north, up))
        })
        .collect()
}

/// The height above home at `t`, the highest one up to `until` (`None`: at `t` only).
fn height(log: &Log, t: u64, until: Option<u64>) -> f64 {
    let heights: Vec<f64> = log
        .rows("POS")
        .filter_map(|r| Some((r.time_us()?, r.f64("RelHomeAlt")?)))
        .filter(|(at, h)| h.is_finite() && *at >= t && *at <= until.unwrap_or(t + 500_000))
        .map(|(_, h)| h)
        .collect();
    let best = match until {
        Some(_) => heights.iter().copied().fold(f64::NAN, f64::max),
        None => heights.first().copied().unwrap_or(f64::NAN),
    };
    if best.is_finite() {
        (best.max(1.0) * 10.).round() / 10.
    } else {
        DEFAULT_TAKEOFF_M
    }
}

fn params(log: &Log) -> String {
    let mut set = ParamSet {
        system_id: 1,
        component_id: 1,
        stack: "ArduPilot".into(),
        vehicle: "from a flight log".into(),
        ..Default::default()
    };
    for r in log.rows("PARM") {
        if let (Some(name), Some(value)) = (r.text("Name"), r.f64("Value")) {
            set.params.insert(
                name.to_string(),
                Param {
                    name: name.to_string(),
                    value: ParamValue::Real(value),
                    kind: 9,
                },
            );
        }
    }
    set.to_qgc()
}

/// The commands, evidence, parameters and recipe of an ArduPilot flight log, or why it cannot be
/// re-simulated.
pub(crate) fn export(log: &Log, label: &str) -> Result<Export, String> {
    let (vehicle, vehicle_type, version) = firmware(log);
    let copter = vehicle == "copter";
    let ned = ned_targets(&version);
    let guip: Vec<(u64, f64, f64, f64)> = log
        .rows("GUIP")
        .filter(|r| r.i64("Type") == Some(SUB_VEL_ACCEL))
        .filter_map(|r| Some((r.time_us()?, r.f64("vX")?, r.f64("vY")?, r.f64("vZ")?)))
        .collect();
    let mut rows: Vec<(u64, &str, Value)> = Vec::new();
    let mut evidence = String::new();
    let mut state = State::default();
    let mut guided_at: Vec<(u64, u64)> = Vec::new(); // Guided periods [start, end)
    let mut since: Option<u64> = None;
    for (t, change) in changes(log) {
        state.failsafe = None;
        match change {
            Change::Mode { mode, reason } => {
                if ASKED_MODE.contains(&reason) {
                    rows.push((t, MODE, json!({"mode": mode})));
                    if mode == GUIDED && state.flying && copter && state.mode != GUIDED {
                        // Switched into Guided in the air (a takeoff by hand): the fresh vehicle,
                        // which no stick lifted, arms and climbs there first.
                        rows.push((t, ARM, json!({"arm": true})));
                        rows.push((t, TAKEOFF, json!({"alt": height(log, t, None)})));
                    }
                } else if FAILSAFE_MODE.contains(&reason) {
                    state.failsafe = Some(reason);
                }
                match (state.mode == GUIDED, mode == GUIDED) {
                    (false, true) => since = Some(t),
                    (true, false) => guided_at.extend(since.take().map(|s| (s, t))),
                    _ => {}
                }
                state.mode = mode;
            }
            Change::Arm { armed, method } => {
                if armed && !state.armed && ASKED_ARM.contains(&method) {
                    rows.push((t, ARM, json!({"arm": true})));
                }
                state.armed = armed;
            }
            Change::Flying(flying) => {
                if flying && !state.flying && state.armed && state.mode == GUIDED && copter {
                    let next = guip.iter().map(|g| g.0).find(|g| *g > t);
                    let until =
                        next.map_or(t + TAKEOFF_SEARCH_US, |n| n.min(t + TAKEOFF_SEARCH_US));
                    rows.push((t, TAKEOFF, json!({"alt": height(log, t, Some(until))})));
                }
                state.flying = flying;
            }
        }
        let message = json!({"header": {"stamp": stamp(t), "frame_id": ""}, "vehicle_type": vehicle_type,
            "armed": state.armed, "mode": state.mode, "flying": state.flying, "external_control": true,
            "failsafe": state.failsafe.map_or(vec![], |f| vec![f])});
        evidence.push_str(&line(STATUS, t, message));
    }
    guided_at.extend(since.map(|s| (s, u64::MAX)));
    let in_guided = |t: u64| guided_at.iter().any(|&(a, b)| a <= t && t < b);
    let mut velocities = 0usize;
    for &(t, vx, vy, vz) in guip.iter().filter(|g| in_guided(g.0)) {
        let up = if ned { -vz } else { vz };
        rows.push((
            t,
            CMD_VEL,
            json!({"header": {"stamp": stamp(t), "frame_id": "map"},
                "twist": {"linear": {"x": vy, "y": vx, "z": up}, "angular": {"x": 0.0, "y": 0.0, "z": 0.0}}}),
        ));
        velocities += 1;
    }
    if velocities == 0 {
        return Err(MANUAL.into());
    }
    if evidence.is_empty() {
        return Err(
            "The flight log has no mode or arming messages to time the replay from.".into(),
        );
    }
    let mut poses = 0usize;
    for (t, east, north, up) in positions(log) {
        evidence.push_str(&line(
            POSE,
            t,
            json!({"header": {"stamp": stamp(t), "frame_id": "map"},
                "pose": {"position": {"x": east, "y": north, "z": up},
                         "orientation": {"x": 0.0, "y": 0.0, "z": 0.0, "w": 1.0}}}),
        ));
        poses += 1;
    }
    rows.sort_by_key(|r| r.0);
    let mut counts = BTreeMap::new();
    for (_, topic, _) in &rows {
        *counts.entry(topic.to_string()).or_insert(0) += 1;
    }
    let commands: String = rows
        .into_iter()
        .map(|(t, topic, m)| line(topic, t, m))
        .collect();
    let mut evidence_topics = vec![STATUS.to_string()];
    if poses > 0 {
        evidence_topics.push(POSE.into());
    }
    let recipe = Recipe {
        schema: SCHEMA,
        origin: "real".into(),
        system: label.to_string(),
        adapter: "ardupilot-sitl".into(),
        autopilot: AutopilotKind::ArduPilot,
        world: World {
            id: "internal".into(),
            params: Default::default(),
        },
        vehicle: Some(vehicle),
        autopilot_version: version,
        home: home(log),
        params: (log.count("PARM") > 0).then(|| "params.params".into()),
        command_topics: counts.keys().cloned().collect(),
        evidence_topics,
        anchor: "first status in the flight log".into(),
        drone_deck: env!("CARGO_PKG_VERSION").into(),
        source: "dataflash".into(),
        command_types: [(
            CMD_VEL.to_string(),
            "geometry_msgs/msg/TwistStamped".to_string(),
        )]
        .into(),
    };
    Ok(Export {
        recipe,
        commands,
        evidence,
        params: params(log),
        counts,
    })
}

#[cfg(test)]
#[path = "import_ap_tests.rs"]
mod tests;
