//! A system: a named launch setup made of one vehicle (a simulated autopilot in a world, a
//! physical vehicle or a recording) and the consumers fed by it. Templates reproduce the four
//! 0.8 systems; everything else derives from vehicle + consumers.
pub(crate) use super::adapter::AutopilotKind;
use super::adapter::{self, Adapter};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Template families. `System::kind` maps any system onto one of them, so pages that only
/// need "is this a physical / AI / ROS system" keep a single index.
pub(crate) const SIMULATION: usize = 0;
pub(crate) const ROS: usize = 1;
pub(crate) const AI: usize = 2;
pub(crate) const PHYSICAL: usize = 3;
/// Template names, indexed by kind.
pub(crate) const KINDS: [&str; 4] = [
    "Local simulation",
    "ROS & sensor capture",
    "Offline AI workspace",
    "Physical vehicle",
];
/// Template ids stored in `System::template`, indexed by kind.
pub(crate) const TEMPLATE_IDS: [&str; 4] = [
    "local-simulation",
    "ros-sensor-capture",
    "offline-ai",
    "physical-vehicle",
];
/// Labels for the vehicle kinds in the editor, in `Vehicle::index` order.
pub(crate) const VEHICLES: [&str; 3] = ["Simulated", "Physical vehicle", "Recording replay"];
pub(crate) const DEFAULT_DOMAIN: u32 = 0;
pub(crate) const DEFAULT_SCRIPTS_UDP: &str = "127.0.0.1:14540";
pub(crate) const MAX_NAME: usize = 60;

/// The flight stack of a simulated vehicle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Autopilot {
    Px4,
    /// ArduPilot SITL (`vehicle`: copter, plane, rover…).
    #[serde(rename = "ardupilot")]
    ArduPilot {
        vehicle: String,
    },
}
impl Autopilot {
    pub fn kind(&self) -> AutopilotKind {
        match self {
            Autopilot::Px4 => AutopilotKind::Px4,
            Autopilot::ArduPilot { .. } => AutopilotKind::ArduPilot,
        }
    }
}

/// The physics a simulated vehicle flies in: `internal` (the autopilot's own) or a simulator
/// such as `gazebo`, with the details its adapter takes (Gazebo: `world`, `model`). Worlds are
/// data, so a world this version has no adapter for is kept as it was written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct World {
    pub id: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}
impl World {
    pub fn new(id: &str, params: &[(&str, &str)]) -> Self {
        Self {
            id: id.into(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

/// Where vehicle state comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Vehicle {
    /// An autopilot that Drone Deck starts, flying in a world.
    Simulated { autopilot: Autopilot, world: World },
    /// A flight controller over `serial:COMx:baud`, `udp:host:port` or `tcp:host:port`.
    Physical {
        endpoint: String,
        /// The flight stack the user said it runs; only the heartbeat identifies it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        autopilot_hint: Option<AutopilotKind>,
    },
    /// A recorded session played back instead of a live vehicle.
    Replay {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recording: Option<String>,
    },
}
impl Vehicle {
    /// PX4 with its internal SIH physics.
    pub fn sih() -> Self {
        Vehicle::Simulated {
            autopilot: Autopilot::Px4,
            world: World::new("internal", &[]),
        }
    }
    pub fn gazebo(world: &str, model: &str) -> Self {
        Vehicle::Simulated {
            autopilot: Autopilot::Px4,
            world: World::new("gazebo", &[("world", world), ("model", model)]),
        }
    }
    pub fn ardupilot(vehicle: &str) -> Self {
        Vehicle::Simulated {
            autopilot: Autopilot::ArduPilot {
                vehicle: vehicle.into(),
            },
            world: World::new("internal", &[]),
        }
    }
    pub fn physical(endpoint: &str) -> Self {
        Vehicle::Physical {
            endpoint: endpoint.into(),
            autopilot_hint: None,
        }
    }
    pub fn replay(recording: Option<&str>) -> Self {
        Vehicle::Replay {
            recording: recording.map(str::to_string),
        }
    }
    /// Index into `VEHICLES`.
    pub fn index(&self) -> usize {
        match self {
            Vehicle::Simulated { .. } => 0,
            Vehicle::Physical { .. } => 1,
            Vehicle::Replay { .. } => 2,
        }
    }
}

/// ROS 2 consumers: the PX4 ↔ ROS 2 bridge, the MCAP recorder and the live ROS domain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Ros {
    #[serde(default)]
    pub agent: bool,
    #[serde(default)]
    pub recorder: bool,
    #[serde(default)]
    pub domain: u32,
}
impl Default for Ros {
    fn default() -> Self {
        Self {
            agent: false,
            recorder: false,
            domain: DEFAULT_DOMAIN,
        }
    }
}

/// Local inference over the source's data, optionally tied to one project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Inference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// What the source feeds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Consumers {
    /// QGroundControl, the flight-control interface.
    #[serde(default)]
    pub qgc: bool,
    /// The native live vehicle view on the System page (the vehicle link needs no extra process).
    #[serde(default)]
    pub live_view: bool,
    #[serde(default)]
    pub ros: Ros,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<Inference>,
    #[serde(default)]
    pub foxglove: bool,
    /// Two-way MAVLink bridge for user scripts (MAVSDK, pymavlink) on this Windows UDP address:
    /// the vehicle link forwards what they send to the vehicle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts_udp: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct System {
    /// Stable slug; renaming keeps it.
    pub id: String,
    pub name: String,
    /// The template id it was created from (`TEMPLATE_IDS`), kept for guided setup.
    #[serde(default)]
    pub template: String,
    /// Guided preparation finished for it on this computer.
    #[serde(default)]
    pub prepared: bool,
    pub vehicle: Vehicle,
    #[serde(default)]
    pub consumers: Consumers,
}
impl System {
    /// The simulator adapter of a simulated vehicle; `None` for a physical vehicle, a replay,
    /// or a world this version has no adapter for.
    pub fn adapter(&self) -> Option<&'static Adapter> {
        match &self.vehicle {
            Vehicle::Simulated { autopilot, world } => adapter::find(autopilot.kind(), &world.id),
            _ => None,
        }
    }
    /// A simulator that Drone Deck starts and owns (a world this version has an adapter for).
    pub fn simulated(&self) -> bool {
        self.adapter().is_some()
    }
    pub fn is_physical(&self) -> bool {
        matches!(self.vehicle, Vehicle::Physical { .. })
    }
    pub fn is_replay(&self) -> bool {
        matches!(self.vehicle, Vehicle::Replay { .. })
    }
    /// The flight stack: a simulated one, or the hint saved for a physical vehicle.
    pub fn autopilot(&self) -> Option<AutopilotKind> {
        match &self.vehicle {
            Vehicle::Simulated { autopilot, .. } => Some(autopilot.kind()),
            Vehicle::Physical { autopilot_hint, .. } => *autopilot_hint,
            Vehicle::Replay { .. } => None,
        }
    }
    /// A detail named by an adapter `Param` key: the ArduPilot vehicle, or a world detail.
    pub fn param(&self, key: &str) -> Option<&str> {
        match (&self.vehicle, key) {
            (
                Vehicle::Simulated {
                    autopilot: Autopilot::ArduPilot { vehicle },
                    ..
                },
                "vehicle",
            ) => Some(vehicle),
            (Vehicle::Simulated { world, .. }, _) => world.params.get(key).map(String::as_str),
            _ => None,
        }
    }
    /// The endpoint of a physical vehicle.
    pub fn endpoint(&self) -> Option<&str> {
        match &self.vehicle {
            Vehicle::Physical { endpoint, .. } => Some(endpoint),
            _ => None,
        }
    }
    /// The saved endpoint of a physical vehicle, to change it.
    pub fn endpoint_mut(&mut self) -> Option<&mut String> {
        match &mut self.vehicle {
            Vehicle::Physical { endpoint, .. } => Some(endpoint),
            _ => None,
        }
    }
    /// The vehicle in one line for lists: the adapter's name and its details.
    pub fn summary(&self) -> String {
        match &self.vehicle {
            Vehicle::Simulated { autopilot, world } => match self.adapter() {
                Some(a) => std::iter::once(a.name.to_string())
                    .chain(
                        a.params
                            .iter()
                            .map(|p| self.param(p.key).unwrap_or(p.default).to_string()),
                    )
                    .collect::<Vec<_>>()
                    .join(" / "),
                None => format!(
                    "{} + {} / no adapter in this version",
                    adapter::autopilot_name(autopilot.kind()),
                    world.id
                ),
            },
            Vehicle::Physical {
                endpoint,
                autopilot_hint,
            } => {
                let what = autopilot_hint.map_or("Physical vehicle".to_string(), |k| {
                    format!("Physical vehicle ({})", adapter::autopilot_name(k))
                });
                if endpoint.trim().is_empty() {
                    format!("{what} / no endpoint yet")
                } else {
                    format!("{what} / {endpoint}")
                }
            }
            Vehicle::Replay { recording: None } => "Recording replay / chosen at replay".into(),
            Vehicle::Replay { recording: Some(r) } => format!("Recording replay / {r}"),
        }
    }
    /// The template family it behaves like, from its vehicle and consumers.
    pub fn kind(&self) -> usize {
        match self.vehicle {
            Vehicle::Physical { .. } => PHYSICAL,
            Vehicle::Replay { .. } => AI,
            _ if self.consumers.ros.agent || self.consumers.ros.recorder => ROS,
            _ => SIMULATION,
        }
    }
    /// The template index guided setup uses: the stored template, else the kind.
    pub fn template_index(&self) -> usize {
        TEMPLATE_IDS
            .iter()
            .position(|t| *t == self.template)
            .unwrap_or_else(|| self.kind())
    }
    /// The display name, or the template name when none was given.
    pub fn label(&self) -> &str {
        let n = self.name.trim();
        if n.is_empty() {
            KINDS[self.template_index()]
        } else {
            n
        }
    }
    /// Consumers in a short line for lists.
    pub fn consumers_summary(&self) -> String {
        let c = &self.consumers;
        let mut parts = Vec::new();
        if c.qgc {
            parts.push("QGroundControl".to_string());
        }
        if c.live_view {
            parts.push(
                if self.is_physical() {
                    "read-only live view"
                } else {
                    "live view"
                }
                .into(),
            );
        }
        if c.ros.agent {
            parts.push(format!("ROS 2 bridge / domain {}", c.ros.domain));
        }
        if c.ros.recorder {
            parts.push("MCAP recorder".into());
        }
        if let Some(i) = &c.inference {
            parts.push(match &i.project {
                Some(p) => format!("inference / {p}"),
                None => "inference".into(),
            });
        }
        if c.foxglove {
            parts.push("Foxglove".into());
        }
        if let Some(addr) = &c.scripts_udp {
            parts.push(format!("scripts / UDP {addr}"));
        }
        if parts.is_empty() {
            "No consumers".into()
        } else {
            parts.join(" / ")
        }
    }
}

/// A new system from template `kind` (clamped to the four templates).
pub(crate) fn template(kind: usize, id: &str, name: &str) -> System {
    let kind = kind.min(PHYSICAL);
    let (vehicle, consumers) = match kind {
        SIMULATION => (
            Vehicle::sih(),
            Consumers {
                qgc: true,
                live_view: true,
                scripts_udp: Some(DEFAULT_SCRIPTS_UDP.into()),
                ..Default::default()
            },
        ),
        ROS => (
            Vehicle::gazebo("default", "x500_mono_cam"),
            Consumers {
                qgc: true,
                ros: Ros {
                    agent: true,
                    recorder: true,
                    domain: DEFAULT_DOMAIN,
                },
                foxglove: true,
                ..Default::default()
            },
        ),
        AI => (
            Vehicle::replay(None),
            Consumers {
                inference: Some(Inference::default()),
                foxglove: true,
                ..Default::default()
            },
        ),
        _ => (
            Vehicle::physical(""),
            Consumers {
                live_view: true,
                ..Default::default()
            },
        ),
    };
    System {
        id: id.into(),
        name: name.into(),
        template: TEMPLATE_IDS[kind].into(),
        prepared: false,
        vehicle,
        consumers,
    }
}

/// The four template systems, as a first start and a schema 1 file describe them.
pub(crate) fn defaults() -> Vec<System> {
    (0..4)
        .map(|k| template(k, TEMPLATE_IDS[k], KINDS[k]))
        .collect()
}

/// `name` as an id: lowercase ASCII letters and digits joined by single dashes.
pub(crate) fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    let out: String = out.chars().take(40).collect();
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "system".into()
    } else {
        out.into()
    }
}

/// Whether `id` is a well-formed slug.
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
