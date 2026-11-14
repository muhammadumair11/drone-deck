//! The recipe of a recorded flight: what a fresh simulator needs to fly it again. A recording
//! of a simulated system keeps it as `recipe.json` beside its MCAP (`scripts/recordings.py`);
//! the parameters follow as `params.params` when the recording stops.
use crate::State;
use crate::systems::adapter::{AutopilotKind, Ros};
use crate::systems::model::World;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const SCHEMA: u32 = 1;

/// The home position the vehicle reported: degrees and metres above mean sea level.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Home {
    pub lat: f64,
    pub lon: f64,
    pub alt: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Recipe {
    pub schema: u32,
    /// `simulated` for a flight a Drone Deck simulator flew.
    pub origin: String,
    /// The system's name when it recorded.
    pub system: String,
    /// The adapter that flew it (`px4-gazebo`, `ardupilot-sitl`…).
    pub adapter: String,
    pub autopilot: AutopilotKind,
    pub world: World,
    /// The ArduPilot vehicle (copter, plane…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vehicle: Option<String>,
    /// The autopilot version setup detected, when it did.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub autopilot_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<Home>,
    /// The parameters file beside the recording, once written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<String>,
    /// Recorded topics that command the autopilot: what a re-simulation replays.
    pub command_topics: Vec<String>,
    /// Recorded topics the autopilot reports: what a re-simulation compares against.
    pub evidence_topics: Vec<String>,
    /// Where replay timing starts.
    pub anchor: String,
    pub drone_deck: String,
    /// Where the commands are: empty for the recording's MCAP, `ulog` for `commands.jsonl` and
    /// `evidence.jsonl` exported from a flight log (`import`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// ROS message types of the command topics, when no MCAP carries them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub command_types: BTreeMap<String, String>,
}

/// Recorded topics split into (commands, evidence) by the adapter's ROS prefixes. Topics
/// outside the autopilot's prefix (cameras, your nodes) are in neither list.
pub(crate) fn split(topics: &[String], ros: &Ros) -> (Vec<String>, Vec<String>) {
    let (mut commands, mut evidence) = (Vec::new(), Vec::new());
    for t in topics {
        if ros.commands.iter().any(|p| t.starts_with(p)) {
            commands.push(t.clone());
        } else if t.starts_with(ros.topic_prefix) {
            evidence.push(t.clone());
        }
    }
    (commands, evidence)
}

/// Discovered topics of the autopilot that a re-simulable recording always includes, even
/// when they were not selected: its commands and what it reports.
pub(crate) fn wanted_topics(s: &State) -> Vec<String> {
    let Some(ros) = s.systems.current().adapter().and_then(|a| a.ros.as_ref()) else {
        return Vec::new();
    };
    s.recorder
        .discovered
        .iter()
        .map(|t| t.name.clone())
        .filter(|n| n.starts_with(ros.topic_prefix))
        .collect()
}

/// The recipe for recording `topics` from the active system, or `None` when that system is
/// not a simulator Drone Deck flies (a physical vehicle or a replay records no recipe here).
pub(crate) fn capture(s: &State, topics: &[String]) -> Option<Recipe> {
    let sys = s.systems.current();
    let a = sys.adapter()?;
    let (command_topics, evidence_topics) = a
        .ros
        .as_ref()
        .map(|ros| split(topics, ros))
        .unwrap_or_default();
    let world = match &sys.vehicle {
        crate::systems::Vehicle::Simulated { world, .. } => world.clone(),
        _ => return None,
    };
    let home = s
        .vehicle
        .shown_for(&sys.id)
        .and_then(|snap| snap.vehicle.as_ref())
        .and_then(|v| v.home)
        .map(|(lat, lon, alt)| Home { lat, lon, alt });
    let autopilot_version = a
        .recipes
        .first()
        .and_then(|id| s.setup.checks.get(*id))
        .map(|c| c.version.clone())
        .unwrap_or_default();
    Some(Recipe {
        schema: SCHEMA,
        origin: "simulated".into(),
        system: sys.label().to_string(),
        adapter: a.id.into(),
        autopilot: a.autopilot,
        world,
        vehicle: (a.autopilot == AutopilotKind::ArduPilot)
            .then(|| sys.param("vehicle").map(str::to_string))
            .flatten(),
        autopilot_version,
        home,
        params: None,
        command_topics,
        evidence_topics,
        anchor: match a.autopilot {
            AutopilotKind::Px4 => "first vehicle status after the autopilot is ready",
            AutopilotKind::ArduPilot => {
                "first /ap/status after the autopilot passes its pre-arm checks"
            }
        }
        .into(),
        drone_deck: env!("CARGO_PKG_VERSION").into(),
        source: String::new(),
        command_types: BTreeMap::new(),
    })
}

/// The parameters to keep with the recording when it stops: the active system's link has
/// downloaded them (`vehicle::fetch_params`), in QGroundControl's `.params` format.
pub(crate) fn params_text(s: &State) -> Option<String> {
    let sys = s.systems.current();
    if !sys.simulated() {
        return None;
    }
    s.vehicle
        .owned_by(&sys.id)
        .then(|| s.vehicle.params.as_ref().map(|p| p.to_qgc()))
        .flatten()
}
