//! A system: a named launch setup made of one vehicle source and the consumers fed by it.
//! Templates reproduce the four 0.8 systems; everything else derives from source + consumers.
use serde::{Deserialize, Serialize};

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
/// Labels for the source choices in the editor, in `Source::index` order.
pub(crate) const SOURCES: [&str; 5] = [
    "PX4 SIH",
    "PX4 + Gazebo",
    "ArduPilot SITL",
    "Physical vehicle",
    "Recording replay",
];
pub(crate) const DEFAULT_WORLD: &str = "default";
pub(crate) const DEFAULT_MODEL: &str = "x500_mono_cam";
pub(crate) const DEFAULT_VEHICLE: &str = "copter";
pub(crate) const DEFAULT_DOMAIN: u32 = 0;
pub(crate) const DEFAULT_SCRIPTS_UDP: &str = "127.0.0.1:14540";
pub(crate) const MAX_NAME: usize = 60;

/// Where vehicle state comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Source {
    /// PX4 software-in-the-loop with its internal physics.
    Sih,
    /// PX4 with a Gazebo Harmonic world and model.
    Gazebo { world: String, model: String },
    /// ArduPilot SITL (`vehicle`: copter, plane, rover…).
    #[serde(rename = "ardupilot")]
    ArduPilot { vehicle: String },
    /// A flight controller over `serial:COMx:baud`, `udp:host:port` or `tcp:host:port`.
    Physical { endpoint: String },
    /// A recorded session played back instead of a live vehicle.
    Replay {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recording: Option<String>,
    },
}
impl Source {
    pub fn index(&self) -> usize {
        match self {
            Source::Sih => 0,
            Source::Gazebo { .. } => 1,
            Source::ArduPilot { .. } => 2,
            Source::Physical { .. } => 3,
            Source::Replay { .. } => 4,
        }
    }
    /// A source of the given `index` with default details.
    pub fn with_index(index: usize) -> Self {
        match index {
            1 => Source::Gazebo {
                world: DEFAULT_WORLD.into(),
                model: DEFAULT_MODEL.into(),
            },
            2 => Source::ArduPilot {
                vehicle: DEFAULT_VEHICLE.into(),
            },
            3 => Source::Physical {
                endpoint: String::new(),
            },
            4 => Source::Replay { recording: None },
            _ => Source::Sih,
        }
    }
    /// A simulator that Drone Deck starts and owns.
    pub fn simulated(&self) -> bool {
        matches!(
            self,
            Source::Sih | Source::Gazebo { .. } | Source::ArduPilot { .. }
        )
    }
    /// One line for lists: the source and its details.
    pub fn summary(&self) -> String {
        match self {
            Source::Sih => "PX4 SIH".into(),
            Source::Gazebo { world, model } => format!("PX4 + Gazebo / {world} / {model}"),
            Source::ArduPilot { vehicle } => format!("ArduPilot SITL / {vehicle}"),
            Source::Physical { endpoint } if endpoint.trim().is_empty() => {
                "Physical vehicle / no endpoint yet".into()
            }
            Source::Physical { endpoint } => format!("Physical vehicle / {endpoint}"),
            Source::Replay { recording: None } => "Recording replay / chosen at replay".into(),
            Source::Replay { recording: Some(r) } => format!("Recording replay / {r}"),
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
    pub source: Source,
    #[serde(default)]
    pub consumers: Consumers,
}
impl System {
    /// The template family it behaves like, from its source and consumers.
    pub fn kind(&self) -> usize {
        match self.source {
            Source::Physical { .. } => PHYSICAL,
            Source::Replay { .. } => AI,
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
                match self.source {
                    Source::Physical { .. } => "read-only live view",
                    _ => "live view",
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
    let (source, consumers) = match kind {
        SIMULATION => (
            Source::Sih,
            Consumers {
                qgc: true,
                live_view: true,
                scripts_udp: Some(DEFAULT_SCRIPTS_UDP.into()),
                ..Default::default()
            },
        ),
        ROS => (
            Source::with_index(1),
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
            Source::Replay { recording: None },
            Consumers {
                inference: Some(Inference::default()),
                foxglove: true,
                ..Default::default()
            },
        ),
        _ => (
            Source::with_index(3),
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
        source,
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

/// The workflow preset (`setup::workflow::PRESETS`) that prepares this system.
pub(crate) fn preset(sys: &System) -> usize {
    match sys.source {
        Source::Physical { .. } => 4,
        Source::Replay { .. } => 3,
        Source::ArduPilot { .. } => 2,
        Source::Gazebo { .. } => 1,
        _ if sys.consumers.ros.agent => 5,
        _ => 0,
    }
}
