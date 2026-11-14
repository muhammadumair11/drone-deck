//! `systems.json`: schema 3 (`{schema, active, systems}`), written through `store.rs`.
//! Older files are migrated on read: schema 1 (four fixed records and an active index) and
//! schema 2 (one `source` per system, such as `{"kind":"gazebo","world":..,"model":..}`).
use super::model::{self, KINDS, ROS, System, TEMPLATE_IDS, Vehicle};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) const SCHEMA: u32 = 3;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct File {
    pub schema: u32,
    /// Id of the active system.
    pub active: String,
    pub systems: Vec<System>,
}

/// Schema 1: per-kind `{name, prepared}` and the active kind.
#[derive(Deserialize)]
struct V1 {
    #[serde(default)]
    active: usize,
    #[serde(default)]
    records: Vec<V1Record>,
}
#[derive(Deserialize, Default)]
struct V1Record {
    #[serde(default)]
    name: String,
    #[serde(default)]
    prepared: bool,
}

/// The four template systems with the first one active.
pub(crate) fn defaults() -> File {
    let systems = model::defaults();
    File {
        schema: SCHEMA,
        active: systems[0].id.clone(),
        systems,
    }
}

/// Parses any schema: the file and whether it was migrated from an older one. The outer error
/// marks a damaged file; the inner one a readable file of a newer schema, which this version
/// must neither use nor replace.
pub(crate) fn parse(bytes: &[u8]) -> Result<Result<(File, bool), String>, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let schema = match value.get("schema") {
        None => 1,
        Some(v) => v
            .as_u64()
            .ok_or_else(|| "schema is not a number".to_string())?,
    };
    match schema {
        1 => {
            let v1: V1 = serde_json::from_value(value).map_err(|e| e.to_string())?;
            Ok(Ok((migrate(v1), true)))
        }
        2 => {
            let file: File =
                serde_json::from_value(migrate_v2(value)?).map_err(|e| e.to_string())?;
            Ok(Ok((normalize(file), true)))
        }
        3 => {
            let file: File = serde_json::from_value(value).map_err(|e| e.to_string())?;
            Ok(Ok((normalize(file), false)))
        }
        n => Ok(Err(format!(
            "systems.json uses schema {n}, written by a newer Drone Deck; this version reads schema {SCHEMA}"
        ))),
    }
}

/// Schema 2 → 3: each system's `source` becomes a `vehicle`: PX4 SIH and ArduPilot SITL fly in
/// their autopilot's `internal` physics, PX4 + Gazebo in the `gazebo` world with its world and
/// model; physical and replay sources keep their details.
fn migrate_v2(mut value: Value) -> Result<Value, String> {
    let Some(systems) = value.get_mut("systems").and_then(Value::as_array_mut) else {
        return Ok(value);
    };
    for sys in systems {
        let Some(obj) = sys.as_object_mut() else {
            return Err("a system is not an object".into());
        };
        let source = obj.remove("source").unwrap_or(Value::Null);
        let text = |key: &str| source.get(key).and_then(Value::as_str).unwrap_or("");
        let vehicle = match source.get("kind").and_then(Value::as_str) {
            Some("sih") => json!({"kind": "simulated", "autopilot": {"kind": "px4"},
                "world": {"id": "internal"}}),
            Some("gazebo") => json!({"kind": "simulated", "autopilot": {"kind": "px4"},
                "world": {"id": "gazebo", "params": {"world": text("world"), "model": text("model")}}}),
            Some("ardupilot") => json!({"kind": "simulated",
                "autopilot": {"kind": "ardupilot", "vehicle": text("vehicle")},
                "world": {"id": "internal"}}),
            Some("physical") => json!({"kind": "physical", "endpoint": text("endpoint")}),
            Some("replay") => match source.get("recording") {
                Some(r) => json!({"kind": "replay", "recording": r}),
                None => json!({"kind": "replay"}),
            },
            other => return Err(format!("unknown source {}", other.unwrap_or("(none)"))),
        };
        obj.insert("vehicle".into(), vehicle);
    }
    Ok(value)
}

/// Four systems from the templates, keeping each record's name and prepared flag. The 0.8
/// ROS system ran PX4 SIH with the DDS Agent, so its migrated source stays SIH.
fn migrate(v1: V1) -> File {
    let mut systems = Vec::new();
    for (kind, template) in TEMPLATE_IDS.iter().enumerate() {
        let record = v1.records.get(kind);
        let name = record
            .map(|r| r.name.trim())
            .filter(|n| !n.is_empty())
            .unwrap_or(KINDS[kind]);
        let mut sys = model::template(kind, template, name);
        sys.prepared = record.is_some_and(|r| r.prepared);
        if kind == ROS {
            sys.vehicle = Vehicle::sih();
        }
        systems.push(sys);
    }
    File {
        schema: SCHEMA,
        active: TEMPLATE_IDS[v1.active.min(3)].into(),
        systems,
    }
}

/// Repairs what a hand-edited file can break: no systems, bad or duplicate ids, long names,
/// an active id that does not exist, a world id in another case or with empty details.
pub(crate) fn normalize(mut file: File) -> File {
    if file.systems.is_empty() {
        return defaults();
    }
    let mut seen: Vec<String> = Vec::new();
    for sys in &mut file.systems {
        let base = if model::valid_id(&sys.id) {
            sys.id.clone()
        } else {
            model::slug(&sys.name)
        };
        sys.id = unique(&base, &seen);
        seen.push(sys.id.clone());
        if sys.name.chars().count() > model::MAX_NAME {
            sys.name = sys.name.chars().take(model::MAX_NAME).collect();
        }
        if let Vehicle::Simulated { world, .. } = &mut sys.vehicle {
            world.id = world.id.trim().to_ascii_lowercase();
            world.params.retain(|_, v| !v.trim().is_empty());
        }
    }
    if !file.systems.iter().any(|s| s.id == file.active) {
        file.active = file.systems[0].id.clone();
    }
    file.schema = SCHEMA;
    file
}

/// `base`, or `base-2`, `base-3`… when taken.
pub(crate) fn unique(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.into();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|c| !taken.iter().any(|t| t == c))
        .unwrap_or_default()
}
