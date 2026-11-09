//! The four fixed systems from the design and the components behind each one.
//!
//! A system is a saved context (name, whether guided preparation finished) over the shared
//! tool registry. Components map design stations to real profiles or subsystems, so every
//! label shown for them comes from observed state.
use crate::*;
use serde::{Deserialize, Serialize};

pub(crate) const SIMULATION: usize = 0;
pub(crate) const ROS: usize = 1;
pub(crate) const AI: usize = 2;
pub(crate) const PHYSICAL: usize = 3;
pub(crate) const KINDS: [&str; 4] = [
    "Local simulation",
    "ROS & sensor capture",
    "Offline AI workspace",
    "Physical vehicle",
];

/// What a component is backed by in the running app.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Backing {
    /// A launch profile in `config.profiles`, started with `State::start`.
    Profile(&'static str),
    /// The native MAVLink monitor (`hardware`).
    Monitor,
    /// The vehicle endpoint text (`hardware.endpoint`).
    Endpoint,
    /// rosbag2 / MCAP capture (`recorder`).
    Recorder,
    /// Editable projects (`studio` library).
    Projects,
    /// Evaluation reports and project artifacts.
    Artifacts,
    /// Native `.tlog` captures (`hardware.path`).
    Tlog,
}

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

const PX4: Component = comp(
    "sih",
    "PX4 / SIH",
    "Autopilot + internal physics",
    "Ubuntu / WSL",
    "Simulation",
    Backing::Profile("sih"),
    "SIH readiness is reported only after PX4 publishes vehicle status; a running process alone is not readiness.",
);
const RELAY: Component = comp(
    "relay",
    "Display relay",
    "Vehicle state to Windows",
    "Ubuntu / WSL",
    "Transport",
    Backing::Profile("relay"),
    "Forwards UDP 19410 to the Windows gateway. The firewall rule must allow the current WSL gateway.",
);
const JMAVSIM: Component = comp(
    "jmavsim",
    "jMAVSim",
    "Native vehicle visualizer",
    "Windows / Java",
    "Simulation",
    Backing::Profile("jmavsim"),
    "Display only in this workflow. It needs a native Java runtime and the relay's live state.",
);
const QGC: Component = comp(
    "qgc",
    "QGroundControl",
    "External flight controls",
    "Windows",
    "Ground station",
    Backing::Profile("qgc"),
    "Vehicle setup, missions and flight control stay in QGroundControl. Drone Deck sends no flight commands.",
);
const MAVSDK: Component = comp(
    "telemetry",
    "MAVSDK telemetry",
    "Read-only vehicle data",
    "Ubuntu / WSL",
    "Telemetry",
    Backing::Profile("telemetry"),
    "Avoid another listener on UDP 14540. Telemetry becomes stale after five seconds without updates.",
);
const DDS: Component = comp(
    "dds",
    "DDS Agent",
    "PX4 to ROS transport",
    "Ubuntu / WSL",
    "Transport",
    Backing::Profile("dds"),
    "Match the Agent, firmware DDS client and px4_msgs versions; installation alone does not establish ROS routing.",
);
const ROS2: Component = comp(
    "ros2",
    "ROS 2 Jazzy",
    "Matching messages + publishers",
    "Ubuntu / WSL",
    "Data",
    Backing::Profile("ros2"),
    "Installed packages do not prove topics are live. Discover topics before recording.",
);
const MCAP: Component = comp(
    "mcap",
    "rosbag2 / MCAP",
    "Record and replay topics",
    "Ubuntu / WSL",
    "Data",
    Backing::Recorder,
    "Capture domain 0, replay on a separate domain (normally 77). Stop gracefully to finalize metadata.",
);
const ONNX: Component = comp(
    "onnx",
    "ONNX CPU runtime",
    "Local inference",
    "Ubuntu / WSL",
    "AI",
    Backing::Profile("onnx"),
    "A private Python environment. A prepared runtime does not establish model compatibility.",
);
const PROJECT: Component = comp(
    "project",
    "Inference project",
    "User model + prepared inputs",
    "Ubuntu / WSL",
    "AI",
    Backing::Projects,
    "Editable project source. Input preparation is task-specific and not automatic.",
);
const REPORT: Component = comp(
    "report",
    "Run artifacts",
    "Outputs + version hashes",
    "Local files",
    "Data",
    Backing::Artifacts,
    "Runs retain model/input hashes and outputs. Evaluation needs supplied targets.",
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
const MONITOR: Component = comp(
    "hardware",
    "MAVLink monitor",
    "Native read-only monitoring",
    "Windows",
    "Telemetry",
    Backing::Monitor,
    "Identity and a fresh heartbeat are separate from port availability. No flight commands are sent.",
);
const TLOG: Component = comp(
    "tlog",
    "Telemetry logs",
    "Local MAVLink capture",
    "Windows",
    "Data",
    Backing::Tlog,
    "Timestamped .tlog captures and read-only parameter backups under workspace/hardware.",
);

const GROUPS: [&[Component]; 4] = [
    &[PX4, RELAY, JMAVSIM, QGC, MAVSDK],
    &[PX4, DDS, ROS2, MCAP, QGC],
    &[ONNX, PROJECT, REPORT],
    &[ENDPOINT, MONITOR, TLOG, QGC],
];
/// Stations shown in the System connections chain.
const CHAINS: [&[&str]; 4] = [
    &["sih", "relay", "jmavsim"],
    &["sih", "dds", "ros2", "mcap"],
    &["onnx", "project", "report"],
    &["endpoint", "hardware", "tlog"],
];
/// Profiles a workspace session starts, in order.
pub(crate) const SESSIONS: [&[&str]; 4] = [&["sih", "relay", "jmavsim"], &["sih", "dds"], &[], &[]];

pub(crate) fn components(kind: usize) -> &'static [Component] {
    GROUPS[kind.min(3)]
}
pub(crate) fn chain(kind: usize) -> impl Iterator<Item = &'static Component> {
    CHAINS[kind.min(3)]
        .iter()
        .filter_map(move |id| components(kind).iter().find(|c| c.id == *id))
}
/// Every component across systems, without duplicates.
pub(crate) fn all_components() -> Vec<&'static Component> {
    let mut seen = Vec::<&'static Component>::new();
    for group in GROUPS {
        for c in group {
            if !seen.iter().any(|x| x.id == c.id) {
                seen.push(c);
            }
        }
    }
    seen
}
/// The design filter for a profile that is not part of a system catalog.
pub(crate) fn filter_for(profile: &backend::Profile) -> &'static str {
    if let Some(c) = all_components()
        .into_iter()
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
/// Profile ids that belong to the system's catalog.
pub(crate) fn profile_ids(kind: usize) -> impl Iterator<Item = &'static str> {
    components(kind).iter().filter_map(|c| match c.backing {
        Backing::Profile(id) => Some(id),
        _ => None,
    })
}

/// Observed state of a component, as a label and a tone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tone {
    Idle,
    Active,
    Good,
    Warn,
    Bad,
}
pub(crate) fn running(s: &State, id: &str) -> bool {
    s.status
        .get(id)
        .is_some_and(|v| v == "Running" || v == "Starting")
}
pub(crate) fn state(s: &State, c: &Component) -> (String, Tone) {
    match c.backing {
        Backing::Profile(id) => profile_state(s, id),
        Backing::Monitor => {
            let status = s.status.get("hardware").cloned().unwrap_or_default();
            if status == "Running" {
                let fresh = s
                    .hardware
                    .seen
                    .values()
                    .filter(|t| t.elapsed() < Duration::from_secs(10))
                    .count();
                if fresh > 0 {
                    (format!("{fresh} vehicle heartbeat(s) fresh"), Tone::Good)
                } else {
                    ("Listening / no heartbeat".into(), Tone::Warn)
                }
            } else if status == "Starting" {
                ("Starting".into(), Tone::Active)
            } else if status.starts_with("Exited") || status == "Failed" {
                (status, Tone::Bad)
            } else {
                ("Not monitoring".into(), Tone::Idle)
            }
        }
        Backing::Endpoint => {
            let e = s.hardware.endpoint.text.trim();
            if e.is_empty() {
                ("No endpoint".into(), Tone::Warn)
            } else {
                (e.to_string(), Tone::Idle)
            }
        }
        Backing::Recorder => {
            let r = &s.recorder;
            if r.recording {
                ("Recording".into(), Tone::Active)
            } else if r.playing {
                ("Replaying".into(), Tone::Active)
            } else if r.ready {
                (
                    format!("Ready / {} recordings", r.sessions.len()),
                    Tone::Good,
                )
            } else {
                ("Not checked".into(), Tone::Idle)
            }
        }
        Backing::Projects => {
            let count = s.studio.library["projects"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0);
            let active = s
                .status
                .iter()
                .any(|(k, v)| k.starts_with("project-") && (v == "Running" || v == "Starting"));
            if active {
                ("Project running".into(), Tone::Active)
            } else if count > 0 {
                (format!("{count} project(s)"), Tone::Idle)
            } else {
                ("No project".into(), Tone::Idle)
            }
        }
        Backing::Artifacts => {
            let count = s.studio.library["reports"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0);
            if count > 0 {
                (format!("{count} report(s)"), Tone::Good)
            } else {
                ("No run artifacts".into(), Tone::Idle)
            }
        }
        Backing::Tlog => {
            if s.hardware.path.is_empty() {
                ("No capture yet".into(), Tone::Idle)
            } else {
                ("Capture folder ready".into(), Tone::Good)
            }
        }
    }
}
pub(crate) fn profile_state(s: &State, id: &str) -> (String, Tone) {
    if !s.config.profiles.iter().any(|p| p.id == id) {
        return ("Not configured".into(), Tone::Warn);
    }
    let status = s.status.get(id).map(String::as_str).unwrap_or("");
    match status {
        "Running" => match s.readiness.get(id).map(String::as_str) {
            Some("Ready") => ("Ready".into(), Tone::Good),
            Some("Forwarding") => ("Forwarding".into(), Tone::Good),
            Some("Failed") => ("Running / not ready".into(), Tone::Bad),
            _ if id == "telemetry" => match s.metrics.get("link").map(String::as_str) {
                Some("Connected") => ("Running / live data".into(), Tone::Good),
                Some("Stale") => ("Running / stale".into(), Tone::Warn),
                _ => ("Running / no data".into(), Tone::Active),
            },
            _ => ("Running".into(), Tone::Active),
        },
        "Starting" => ("Starting".into(), Tone::Active),
        "Blocked" | "Failed" | "Configure command" => (status.into(), Tone::Bad),
        s2 if s2.starts_with("Exited") => {
            if s2 == "Exited 0" {
                (s2.into(), Tone::Idle)
            } else {
                (s2.into(), Tone::Bad)
            }
        }
        _ => match s.detected.get(id) {
            Some(true) => ("Path found".into(), Tone::Idle),
            Some(false) => ("Not detected".into(), Tone::Warn),
            None => ("Not checked".into(), Tone::Idle),
        },
    }
}
pub(crate) fn tone_color(t: Tone) -> rune::Color {
    use crate::ui::theme::*;
    match t {
        Tone::Idle => muted(),
        Tone::Active => fg(),
        Tone::Good => success(),
        Tone::Warn => warn(),
        Tone::Bad => danger(),
    }
}

/// Saved per-system context.
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Record {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub prepared: bool,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Systems {
    pub active: usize,
    pub records: [Record; 4],
}
impl Default for Systems {
    fn default() -> Self {
        Self {
            active: SIMULATION,
            records: Default::default(),
        }
    }
}
impl Systems {
    fn path() -> std::path::PathBuf {
        backend::root().join("systems.json")
    }
    pub fn load(headless: bool) -> Self {
        if headless {
            return Self::default();
        }
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice::<Systems>(&b).ok())
            .map(|mut s| {
                s.active = s.active.min(3);
                s
            })
            .unwrap_or_default()
    }
    pub fn save(&self, headless: bool) -> Result<(), String> {
        if headless {
            return Ok(());
        }
        std::fs::create_dir_all(backend::root()).map_err(|e| e.to_string())?;
        std::fs::write(Self::path(), serde_json::to_vec_pretty(self).unwrap())
            .map_err(|e| e.to_string())
    }
    /// The saved name, or the system type when none was given.
    pub fn name(&self, kind: usize) -> &str {
        let n = self.records[kind.min(3)].name.trim();
        if n.is_empty() { KINDS[kind.min(3)] } else { n }
    }
    pub fn prepared(&self, kind: usize) -> bool {
        self.records[kind.min(3)].prepared
    }
    pub fn any_prepared(&self) -> bool {
        self.records.iter().any(|r| r.prepared)
    }
}

/// Whether the active system's processes or captures are running.
pub(crate) fn active_work(s: &State) -> bool {
    s.session.is_some()
        || s.recorder.recording
        || s.recorder.playing
        || s.status.values().any(|v| v == "Running" || v == "Starting")
}

/// Starts the active system's workspace: a staged profile launch, the vehicle monitor, or
/// (for offline AI) the project list.
pub(crate) fn start_workspace(s: &mut State) {
    let kind = s.systems.active;
    match kind {
        // The monitor records its own start and refusal.
        PHYSICAL => hardware::toggle(s),
        AI => {
            s.ui.data_tab = 3;
            s.navigate(ui::Page::Data);
            s.notice = "Run a project from Data & models → Projects. No model is bundled.".into();
        }
        _ => {
            s.bundle(SESSIONS[kind]);
            if s.session.is_some() {
                activity::record(s, format!("Requested {} workspace startup", KINDS[kind]));
            }
        }
    }
}
