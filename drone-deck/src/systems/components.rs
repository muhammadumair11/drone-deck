//! The stations behind a system, derived from its source and consumers. Each one is backed by
//! a launch profile or a subsystem, so every label shown for it comes from observed state.
use super::model::{Source, System};

/// What a component is backed by in the running app.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Backing {
    /// A launch profile in `config.profiles`, started with `State::start`.
    Profile(&'static str),
    /// The agent's MAVLink router in WSL (a virtual launch stage, `vehicle::stage`).
    Router,
    /// The native MAVLink service (`vehicle`), also a virtual launch stage.
    Link,
    /// The scripts fan-out peer of the link (`consumers.scripts_udp`).
    Scripts,
    /// The vehicle endpoint text (`hardware.endpoint`).
    Endpoint,
    /// rosbag2 / MCAP capture (`recorder`).
    Recorder,
    /// Editable projects (`studio` library).
    Projects,
    /// Frame extractions and inference runs (`studio.pipes`).
    Pipelines,
    /// The link's `.tlog` capture.
    Tlog,
}

#[derive(Debug)]
pub(crate) struct Component {
    pub id: &'static str,
    pub name: &'static str,
    pub role: &'static str,
    pub target: &'static str,
    /// Tools & config filter: Simulation, Transport, Telemetry, Ground station, Data, AI.
    pub filter: &'static str,
    pub backing: Backing,
    /// One line on what must be true beyond installation.
    pub guidance: &'static str,
}

const fn comp(
    id: &'static str,
    name: &'static str,
    role: &'static str,
    target: &'static str,
    filter: &'static str,
    backing: Backing,
    guidance: &'static str,
) -> Component {
    Component {
        id,
        name,
        role,
        target,
        filter,
        backing,
        guidance,
    }
}

const WSL: &str = "Ubuntu / WSL";

const PX4: Component = comp(
    "sih",
    "PX4 / SIH",
    "Autopilot + internal physics",
    WSL,
    "Simulation",
    Backing::Profile("sih"),
    "SIH readiness is reported only after PX4 publishes vehicle status; a running process alone is not readiness.",
);
const GAZEBO: Component = comp(
    "gazebo",
    "PX4 / Gazebo",
    "Autopilot + Gazebo world",
    WSL,
    "Simulation",
    Backing::Profile("gazebo"),
    "Gazebo Harmonic runs the system's world and model; a running process does not prove the model spawned or sensors publish.",
);
const ARDUPILOT: Component = comp(
    "ardupilot",
    "ArduPilot SITL",
    "Autopilot + SITL physics",
    WSL,
    "Simulation",
    Backing::Profile("ardupilot"),
    "Serves MAVLink on TCP 5760 itself. One simulator runs at a time: they share UDP 14550 and TCP 5760.",
);
const ROUTER: Component = comp(
    "router",
    "MAVLink router",
    "UDP 14550 ⇄ TCP 5760",
    WSL,
    "Transport",
    Backing::Router,
    "Started by the workspace through the Linux agent. Windows reaches it over WSL localhost forwarding; no firewall rule is needed.",
);
const LINK: Component = comp(
    "link",
    "Vehicle link",
    "Native MAVLink service",
    "Windows",
    "Telemetry",
    Backing::Link,
    "Owns the vehicle connection, fans it out to QGroundControl and scripts, and records a .tlog. An open link is not a fresh heartbeat.",
);
const QGC: Component = comp(
    "qgc",
    "QGroundControl",
    "External flight controls",
    "Windows",
    "Ground station",
    Backing::Profile("qgc"),
    "Connects automatically on UDP 14550 through the vehicle link. Vehicle setup and missions stay in QGroundControl.",
);
const SCRIPTS: Component = comp(
    "scripts",
    "User scripts",
    "MAVLink over UDP for MAVSDK / pymavlink",
    "Windows",
    "Telemetry",
    Backing::Scripts,
    "The vehicle link sends every packet to this UDP address and forwards replies to the vehicle. Point your script at it.",
);
const DDS: Component = comp(
    "dds",
    "DDS Agent",
    "PX4 to ROS transport",
    WSL,
    "Transport",
    Backing::Profile("dds"),
    "Match the Agent, firmware DDS client and px4_msgs versions; installation alone does not establish ROS routing.",
);
const ROSGZ: Component = comp(
    "rosgz",
    "ROS–Gazebo bridge",
    "Camera image + camera_info into ROS",
    WSL,
    "Transport",
    Backing::Profile("rosgz"),
    "Bridges the x500_mono_cam camera topics of the default world. Another world or model needs its own topic names.",
);
const ROS2: Component = comp(
    "ros2",
    "ROS 2 Jazzy",
    "Matching messages + publishers",
    WSL,
    "Data",
    Backing::Profile("ros2"),
    "Installed packages do not prove topics are live. Discover topics before recording.",
);
const MCAP: Component = comp(
    "mcap",
    "rosbag2 / MCAP",
    "Record and replay topics",
    WSL,
    "Data",
    Backing::Recorder,
    "Capture domain 0, replay on a separate domain (normally 77). Stop gracefully to finalize metadata.",
);
const ONNX: Component = comp(
    "onnx",
    "ONNX CPU runtime",
    "Local inference",
    WSL,
    "AI",
    Backing::Profile("onnx"),
    "A private Python environment. A prepared runtime does not establish model compatibility.",
);
const PROJECT: Component = comp(
    "project",
    "Inference project",
    "User model + prepared inputs",
    WSL,
    "AI",
    Backing::Projects,
    "Editable project source. Input preparation is task-specific and not automatic.",
);
const PIPELINES: Component = comp(
    "pipelines",
    "Pipelines",
    "Extract frames + infer on replay",
    WSL,
    "AI",
    Backing::Pipelines,
    "Writes next to the recording with a manifest of source and model hashes. Detections are model output; no accuracy is computed.",
);
const FOXGLOVE: Component = comp(
    "foxglove",
    "Foxglove",
    "Inspect recordings and live topics",
    "Windows + WSL bridge",
    "Data",
    Backing::Profile("foxglove"),
    "Opens recordings locally; a live view needs the bridge process (port 8765) running in WSL.",
);
const ENDPOINT: Component = comp(
    "endpoint",
    "Vehicle endpoint",
    "Serial / UDP / TCP source",
    "Windows",
    "Telemetry",
    Backing::Endpoint,
    "Use a dedicated endpoint so tools do not compete for the same port.",
);
const TLOG: Component = comp(
    "tlog",
    "Telemetry logs",
    "Local MAVLink capture",
    "Windows",
    "Data",
    Backing::Tlog,
    "The vehicle link records every packet to a timestamped .tlog under logs/tlog in the data folder.",
);

/// Every known component, in catalog order.
const ALL: [&Component; 17] = [
    &PX4, &GAZEBO, &ARDUPILOT, &ROUTER, &LINK, &QGC, &SCRIPTS, &DDS, &ROSGZ, &ROS2, &MCAP, &ONNX,
    &PROJECT, &PIPELINES, &FOXGLOVE, &ENDPOINT, &TLOG,
];

/// Every component across systems, without duplicates.
pub(crate) fn all_components() -> Vec<&'static Component> {
    ALL.to_vec()
}
/// The component with `id`.
pub(crate) fn component(id: &str) -> Option<&'static Component> {
    ALL.iter().copied().find(|c| c.id == id)
}

/// The launch profile that runs the vehicle source, if Drone Deck starts one.
pub(crate) fn vehicle_profile(sys: &System) -> Option<&'static str> {
    match sys.source {
        Source::Sih => Some("sih"),
        Source::Gazebo { .. } => Some("gazebo"),
        Source::ArduPilot { .. } => Some("ardupilot"),
        _ => None,
    }
}

/// Whether the source reaches Windows through the app's router (PX4 in WSL). ArduPilot SITL
/// serves TCP 5760 itself.
pub(crate) fn routed(sys: &System) -> bool {
    matches!(sys.source, Source::Sih | Source::Gazebo { .. })
}

/// The components of `sys`: its source first, then its consumers.
pub(crate) fn components(sys: &System) -> Vec<&'static Component> {
    let c = &sys.consumers;
    let mut out: Vec<&'static Component> = Vec::new();
    match sys.source {
        Source::Sih => out.extend([&PX4, &ROUTER, &LINK]),
        Source::Gazebo { .. } => out.extend([&GAZEBO, &ROUTER, &LINK]),
        Source::ArduPilot { .. } => out.extend([&ARDUPILOT, &LINK]),
        Source::Physical { .. } => out.extend([&ENDPOINT, &LINK, &TLOG]),
        Source::Replay { .. } => out.push(&MCAP),
    }
    if c.ros.agent && sys.source.simulated() {
        out.extend([&DDS, &ROS2]);
        if matches!(sys.source, Source::Gazebo { .. }) {
            out.push(&ROSGZ);
        }
    }
    if c.ros.recorder && !out.iter().any(|x| x.id == "mcap") {
        out.push(&MCAP);
    }
    if c.inference.is_some() {
        out.extend([&ONNX, &PROJECT, &PIPELINES]);
    }
    if c.qgc {
        out.push(&QGC);
    }
    if c.scripts_udp.is_some() && !matches!(sys.source, Source::Replay { .. }) {
        out.push(&SCRIPTS);
    }
    if c.foxglove {
        out.push(&FOXGLOVE);
    }
    out
}

/// Station ids shown in the System connections chain, in flow order.
fn chain_ids(sys: &System) -> Vec<&'static str> {
    let c = &sys.consumers;
    if let Some(vehicle) = vehicle_profile(sys) {
        let mut ids = vec![vehicle];
        if c.ros.agent {
            ids.extend(["dds", "ros2"]);
            if c.ros.recorder {
                ids.push("mcap");
            }
            return ids;
        }
        if routed(sys) {
            ids.push("router");
        }
        ids.push("link");
        if c.qgc {
            ids.push("qgc");
        }
        return ids;
    }
    match sys.source {
        Source::Physical { .. } => vec!["endpoint", "link", "tlog"],
        _ if c.inference.is_some() => vec!["onnx", "project", "pipelines"],
        _ => vec!["mcap"],
    }
}

/// Stations shown in the System connections chain.
pub(crate) fn chain(sys: &System) -> impl Iterator<Item = &'static Component> {
    let comps = components(sys);
    chain_ids(sys)
        .into_iter()
        .filter_map(move |id| comps.iter().copied().find(|c| c.id == id))
}

/// Stages a workspace session starts for `sys`, in order; each waits for the previous one's
/// readiness (`session.rs`). `router` and `link` are virtual stages (`vehicle::stage`).
pub(crate) fn session_stages(sys: &System) -> Vec<&'static str> {
    let c = &sys.consumers;
    let mut stages: Vec<&'static str> = match sys.source {
        Source::Sih => vec!["sih", "router", "link"],
        Source::Gazebo { .. } => vec!["gazebo", "router", "link"],
        Source::ArduPilot { .. } => vec!["ardupilot", "link"],
        _ => return Vec::new(),
    };
    if c.ros.agent && routed(sys) {
        stages.push("dds");
        if matches!(sys.source, Source::Gazebo { .. }) {
            stages.push("rosgz");
        }
    }
    if c.qgc {
        stages.push("qgc");
    }
    stages
}

/// Session stages of the four templates, indexed by kind.
pub(crate) const SESSIONS: [&[&str]; 4] = [
    &["sih", "router", "link", "qgc"],
    &["gazebo", "router", "link", "dds", "rosgz", "qgc"],
    &[],
    &[],
];

/// Profile ids in the components of template `kind`. Used where only the active kind is at
/// hand; prefer [`components`] with the system itself.
pub(crate) fn profile_ids(kind: usize) -> impl Iterator<Item = &'static str> {
    let sys = super::model::template(kind, "", "");
    components(&sys)
        .into_iter()
        .filter_map(|c| match c.backing {
            Backing::Profile(id) => Some(id),
            _ => None,
        })
        .collect::<Vec<_>>()
        .into_iter()
}

/// The design filter for a profile that is not part of a system catalog.
pub(crate) fn filter_for(profile: &crate::backend::Profile) -> &'static str {
    if let Some(c) = ALL
        .iter()
        .find(|c| matches!(c.backing, Backing::Profile(p) if p == profile.id))
    {
        return c.filter;
    }
    match profile.category.as_str() {
        "Simulation" => "Simulation",
        "Infrastructure" => "Transport",
        "Flight operations" => "Ground station",
        "AI & data" => "AI",
        _ => "Data",
    }
}
