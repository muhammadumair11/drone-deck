//! A real PX4 flight brought in for re-simulation. From its flight log (ULog) Drone Deck writes
//! what a run needs: the commands the autopilot received from outside (`commands.jsonl`), what
//! it reported (`evidence.jsonl`, for the comparison), its parameters, home position and version,
//! and a recipe that flies it in PX4's built-in physics at that home. The agent then files it in
//! the recording library (`resim.import`), with the log itself and, from a ROS 2 recording, the
//! MCAP (whose commands are preferred).
//!
//! What counts as a command from outside:
//! - `offboard_control_mode`, which only offboard control publishes;
//! - `trajectory_setpoint` while the vehicle was in Offboard mode (other modes publish it from
//!   PX4's own flight tasks);
//! - `vehicle_command` marked `from_external` (MAVLink or DDS, not PX4 itself).
//!
//! A flight with none of these was flown by hand (RC or QGroundControl sticks) and has nothing
//! to replay.
use super::recipe::{Home, Recipe, SCHEMA};
use crate::mavlink::{Param, ParamSet, ParamValue};
use crate::systems::adapter::AutopilotKind;
use crate::systems::model::World;
use crate::ulog::parse::{Base, Ulog};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// PX4's Offboard navigation state.
const OFFBOARD: f64 = 14.;
pub(crate) const MANUAL: &str = "This flight has no ROS 2 or offboard commands to replay: it was flown by hand or by PX4's own modes.";

/// (ULog topic, ROS topic, ROS type) of the commands a flight log can give back.
const COMMANDS: [(&str, &str, &str); 3] = [
    (
        "offboard_control_mode",
        "/fmu/in/offboard_control_mode",
        "px4_msgs/msg/OffboardControlMode",
    ),
    (
        "trajectory_setpoint",
        "/fmu/in/trajectory_setpoint",
        "px4_msgs/msg/TrajectorySetpoint",
    ),
    (
        "vehicle_command",
        "/fmu/in/vehicle_command",
        "px4_msgs/msg/VehicleCommand",
    ),
];
/// (ULog topic, ROS topic, fields kept) of what the comparison reads.
const EVIDENCE: [(&str, &str, &[&str]); 3] = [
    (
        "vehicle_status",
        "/fmu/out/vehicle_status",
        &["arming_state", "nav_state", "failsafe"],
    ),
    (
        "vehicle_local_position",
        "/fmu/out/vehicle_local_position",
        &["x", "y", "z"],
    ),
    (
        "vehicle_land_detected",
        "/fmu/out/vehicle_land_detected",
        &["landed"],
    ),
];

/// What an import writes, as file contents.
#[derive(Debug)]
pub(crate) struct Export {
    pub recipe: Recipe,
    pub commands: String,
    pub evidence: String,
    pub params: String,
    /// Messages per command topic.
    pub counts: BTreeMap<String, usize>,
}

/// One message as px4_msgs fields: `name[i]` back into arrays, integers as integers, booleans
/// as booleans.
fn message(time: u64, fields: &[(&str, Base, f64)], keep: Option<&[&str]>) -> Value {
    let mut out = Map::new();
    out.insert("timestamp".into(), json!(time));
    for &(name, base, v) in fields {
        if name == "timestamp" {
            continue;
        }
        let (key, index) = match name.split_once('[') {
            Some((k, rest)) => (k, rest.trim_end_matches(']').parse::<usize>().ok()),
            None => (name, None),
        };
        if keep.is_some_and(|k| !k.contains(&key)) {
            continue;
        }
        let value = match base {
            Base::Bool => json!(v != 0.),
            Base::F32 | Base::F64 => {
                if v.is_finite() {
                    json!(v)
                } else {
                    json!(if v.is_nan() {
                        "nan"
                    } else if v > 0. {
                        "inf"
                    } else {
                        "-inf"
                    })
                }
            }
            _ => json!(v as i64),
        };
        match index {
            Some(i) => {
                let list = out.entry(key).or_insert_with(|| json!([]));
                if let Some(a) = list.as_array_mut() {
                    a.resize(a.len().max(i + 1), Value::Null);
                    a[i] = value;
                }
            }
            None => {
                out.insert(key.into(), value);
            }
        }
    }
    Value::Object(out)
}

fn line(topic: &str, time: u64, message: Value) -> String {
    format!(
        "{}\n",
        json!({"topic": topic, "t_ns": time * 1000, "message": message})
    )
}

/// Offboard periods `[start, end)` in µs from vehicle_status.
fn offboard(log: &Ulog) -> Vec<(u64, u64)> {
    let Some(i) = log.first_instance("vehicle_status", &["nav_state"]) else {
        return Vec::new();
    };
    let Some(s) = log.series("vehicle_status", i, "nav_state") else {
        return Vec::new();
    };
    let mut out: Vec<(u64, u64)> = Vec::new();
    let mut start = None;
    for (t, v) in s.t_us.iter().zip(&s.v) {
        match (start, *v == OFFBOARD) {
            (None, true) => start = Some(*t),
            (Some(a), false) => {
                out.push((a, *t));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(a) = start {
        out.push((a, u64::MAX));
    }
    out
}

fn params(log: &Ulog) -> String {
    let mut set = ParamSet {
        system_id: 1,
        component_id: 1,
        stack: "PX4 Pro".into(),
        vehicle: "from a flight log".into(),
        ..Default::default()
    };
    for (name, v) in &log.params {
        let (value, kind) = match v {
            crate::ulog::parse::Value::Int(i) => (ParamValue::Int(*i), 6),
            crate::ulog::parse::Value::Float(f) => (ParamValue::Real(*f), 9),
            _ => continue,
        };
        set.params.insert(
            name.clone(),
            Param {
                name: name.clone(),
                value,
                kind,
            },
        );
    }
    set.to_qgc()
}

fn home(log: &Ulog) -> Option<Home> {
    let i = log.first_instance("home_position", &["lat", "lon", "alt"])?;
    let (_, cols) = log.series_many("home_position", i, &["lat", "lon", "alt"])?;
    let at = cols[0].iter().position(|v| v.is_finite() && *v != 0.)?;
    Some(Home {
        lat: cols[0][at],
        lon: cols[1][at],
        alt: cols[2][at] as f32,
    })
}

/// The commands, evidence, parameters and recipe of a PX4 flight log, or why it cannot be
/// re-simulated.
pub(crate) fn export(log: &Ulog, label: &str) -> Result<Export, String> {
    let periods = offboard(log);
    let in_offboard = |t: u64| periods.iter().any(|&(a, b)| a <= t && t < b);
    let mut rows: Vec<(u64, &str, Value)> = Vec::new();
    let mut counts = BTreeMap::new();
    let mut types = BTreeMap::new();
    for (name, topic, ty) in COMMANDS {
        for instance in log.instances(name) {
            for (time, fields) in log.rows(name, instance) {
                let keep = match name {
                    "trajectory_setpoint" => in_offboard(time),
                    "vehicle_command" => fields
                        .iter()
                        .any(|(f, _, v)| *f == "from_external" && *v != 0.),
                    _ => true,
                };
                if keep {
                    rows.push((time, topic, message(time, &fields, None)));
                    *counts.entry(topic.to_string()).or_insert(0) += 1;
                    types.insert(topic.to_string(), ty.to_string());
                }
            }
        }
    }
    let offboard_used = counts.contains_key("/fmu/in/offboard_control_mode");
    if !offboard_used && !counts.contains_key("/fmu/in/vehicle_command") {
        return Err(MANUAL.into());
    }
    rows.sort_by_key(|r| r.0);
    let commands: String = rows
        .into_iter()
        .map(|(t, topic, m)| line(topic, t, m))
        .collect();
    let mut evidence = String::new();
    let mut evidence_topics = Vec::new();
    for (name, topic, keep) in EVIDENCE {
        let Some(instance) = log.instances(name).first().copied() else {
            continue;
        };
        let lines: Vec<String> = log
            .rows(name, instance)
            .into_iter()
            .map(|(t, fields)| line(topic, t, message(t, &fields, Some(keep))))
            .collect();
        if !lines.is_empty() {
            evidence_topics.push(topic.to_string());
            evidence.extend(lines);
        }
    }
    if !evidence_topics
        .iter()
        .any(|t| t.ends_with("vehicle_status"))
    {
        return Err("The flight log has no vehicle status to time the replay from.".into());
    }
    let recipe = Recipe {
        schema: SCHEMA,
        origin: "real".into(),
        system: label.to_string(),
        adapter: "px4-sih".into(),
        autopilot: AutopilotKind::Px4,
        world: World {
            id: "internal".into(),
            params: Default::default(),
        },
        vehicle: None,
        autopilot_version: log
            .info_text("ver_sw_release")
            .or_else(|| log.info_text("ver_sw"))
            .unwrap_or_default(),
        home: home(log),
        params: (!log.params.is_empty()).then(|| "params.params".into()),
        command_topics: counts.keys().cloned().collect(),
        evidence_topics,
        anchor: "first vehicle status in the flight log".into(),
        drone_deck: env!("CARGO_PKG_VERSION").into(),
        source: "ulog".into(),
        command_types: types,
    };
    Ok(Export {
        recipe,
        commands,
        evidence,
        params: params(log),
        counts,
    })
}

/// "30 trajectory_setpoint, 1 vehicle_command" from an import's command counts.
pub(crate) fn counted(counts: &Value) -> String {
    counts
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(topic, n)| format!("{n} {}", topic.rsplit('/').next().unwrap_or(topic)))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// The largest flight log an import reads into memory.
const LOG_LIMIT: u64 = 512 * 1024 * 1024;

/// Writes `e` and the log to `<data root>\imports\<stamp>` for the agent to file.
fn write(e: &Export, log: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stamp = crate::mavlink::utc_stamp(secs);
    let folder = crate::backend::root().join("imports").join(&stamp);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let recipe = serde_json::to_string_pretty(&e.recipe).map_err(|e| e.to_string())?;
    for (name, text) in [
        ("recipe.json", recipe.as_str()),
        ("commands.jsonl", e.commands.as_str()),
        ("evidence.jsonl", e.evidence.as_str()),
        ("params.params", e.params.as_str()),
    ] {
        std::fs::write(folder.join(name), text).map_err(|e| format!("{name}: {e}"))?;
    }
    std::fs::copy(log, folder.join("flight.ulg")).map_err(|e| format!("flight.ulg: {e}"))?;
    Ok(folder)
}

/// Imports a real flight from a PX4 flight log (`.ulg`) or the drone's ROS 2 recording
/// (`.mcap`); the result arrives as `Msg::Imported`.
pub(crate) fn start(s: &mut crate::State, path: std::path::PathBuf) {
    if s.headless || !s.wsl_ready() {
        s.resim.message =
            "Select a working WSL 2 distribution in Settings → Environment first.".into();
        return;
    }
    if s.resim.importing {
        return;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "ulg" && ext != "mcap" {
        s.resim.message = "Choose a PX4 flight log (.ulg) or a ROS 2 recording (.mcap).".into();
        return;
    }
    let label = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("Imported flight")
        .to_string();
    s.resim.importing = true;
    s.resim.message = format!("Importing {label}…");
    let (tx, distro) = (s.resim.tx.clone(), s.config.distro.clone());
    let mount = s.agent.hello.as_ref().map(|h| h.automount_root.clone());
    std::thread::spawn(move || {
        let linux = |p: &std::path::Path| {
            crate::studio::linux_model_path(&p.display().to_string(), &distro, mount.as_deref())
        };
        let mut counts = BTreeMap::new();
        let params = if ext == "ulg" {
            std::fs::metadata(&path)
                .map_err(|e| e.to_string())
                .and_then(|m| {
                    (m.len() <= LOG_LIMIT).then_some(()).ok_or_else(|| {
                        "Flight logs over 512 MB are not read into memory.".to_string()
                    })
                })
                .and_then(|_| std::fs::read(&path).map_err(|e| e.to_string()))
                .and_then(|bytes| crate::ulog::parse(bytes).map_err(|e| e.to_string()))
                .and_then(|log| export(&log, &label))
                .and_then(|e| {
                    counts = e.counts.clone();
                    write(&e, &path)
                })
                .and_then(|folder| linux(&folder))
                .map(|folder| json!({"label": label, "folder": folder}))
        } else {
            linux(&path).map(|mcap| json!({"label": label, "mcap": mcap}))
        };
        let reply = params.and_then(|params| match crate::agent::shared(&distro) {
            Some(client) => client
                .call("resim.import", params, std::time::Duration::from_secs(600))
                .map_err(|e| e.to_string()),
            None => Err("The Linux agent is not available in this mode.".into()),
        });
        let reply = reply.map(|mut v| {
            v["counts"] = json!(counts);
            v
        });
        let _ = tx.send(super::Msg::Imported(reply));
    });
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
