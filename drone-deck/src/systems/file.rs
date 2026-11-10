//! `systems.json`: schema 2 (`{schema, active, systems}`), written through `store.rs`.
//! A schema 1 file (four fixed records and an active index) is migrated on read.
use super::model::{self, KINDS, ROS, Source, System, TEMPLATE_IDS};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const SCHEMA: u32 = 2;

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

/// Parses either schema: the file and whether it was migrated from schema 1. The outer error
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
            let file: File = serde_json::from_value(value).map_err(|e| e.to_string())?;
            Ok(Ok((normalize(file), false)))
        }
        n => Ok(Err(format!(
            "systems.json uses schema {n}, written by a newer Drone Deck; this version reads schema {SCHEMA}"
        ))),
    }
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
            sys.source = Source::Sih;
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
/// an active id that does not exist.
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
