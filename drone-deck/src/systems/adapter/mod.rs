//! Simulator adapters: one entry per supported (autopilot, world) pair.
//!
//! Everything that depends on which autopilot runs in which world reads this table: the launch
//! stages and how each one reports readiness, the MAVLink route to Windows, the ROS 2 bridge
//! and its topic prefix, the environment a launch passes, and which profiles are simulators
//! (they share UDP 14550 and TCP 5760, so one runs at a time). Adding a simulator is one entry
//! here plus its catalog recipe and launch profile.
//!
//! The table is `'static` because the launch machinery names stages by `&'static str`. A
//! later pack loader can deserialize an owned spec and leak it once at startup into this shape.
mod table;
#[cfg(test)]
mod tests;

use super::System;
use crate::State;
pub(crate) use table::ALL;

/// The flight stacks Drone Deck knows how to start and talk to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum AutopilotKind {
    #[serde(rename = "px4")]
    Px4,
    #[serde(rename = "ardupilot")]
    ArduPilot,
}
/// The autopilots in the order the editor offers them.
pub(crate) const AUTOPILOTS: [AutopilotKind; 2] = [AutopilotKind::Px4, AutopilotKind::ArduPilot];

pub(crate) fn autopilot_name(kind: AutopilotKind) -> &'static str {
    match kind {
        AutopilotKind::Px4 => "PX4",
        AutopilotKind::ArduPilot => "ArduPilot",
    }
}

/// The worlds systems can name: id, name, one line about it. A world is offered for every
/// autopilot; the ones without an adapter for it say so instead of being hidden.
pub(crate) const WORLDS: [(&str, &str, &str); 2] = [
    (
        "internal",
        "Built-in physics",
        "The autopilot's own simulation (PX4 SIH, ArduPilot SITL). No world to install.",
    ),
    (
        "gazebo",
        "Gazebo Harmonic",
        "A 3D world with sensors such as cameras. Needs Gazebo in WSL.",
    ),
];

/// Why `autopilot` cannot fly in `world` in this version, or the adapter that runs it.
pub(crate) fn world_for(autopilot: AutopilotKind, world: &str) -> Result<&'static Adapter, String> {
    find(autopilot, world).ok_or_else(|| {
        let others: Vec<&str> = ALL
            .iter()
            .filter(|a| a.world == world)
            .map(|a| autopilot_name(a.autopilot))
            .collect();
        if others.is_empty() {
            format!(
                "No {} adapter for this world yet.",
                autopilot_name(autopilot)
            )
        } else {
            format!("{} only in this version.", others.join(", "))
        }
    })
}

/// How the staged launch knows a stage is ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Readiness {
    /// The agent saw this px4 process (started with `PX4_SIMULATOR=<simulator>`) publish
    /// vehicle status (`scripts/agent/sih.py`). A running process alone is not enough.
    Px4Status { simulator: &'static str },
    /// The process runs.
    ProcessRunning,
}
impl Readiness {
    /// The readiness check the WSL agent runs for the process, if any.
    pub(crate) fn agent_kind(self) -> Option<String> {
        match self {
            Readiness::Px4Status { simulator } => Some(format!("px4-status:{simulator}")),
            Readiness::ProcessRunning => None,
        }
    }
    /// What the staged launch waits for, in user words.
    pub(crate) fn awaited(self) -> Option<&'static str> {
        match self {
            Readiness::Px4Status { .. } => Some("vehicle status"),
            Readiness::ProcessRunning => None,
        }
    }
}

/// How MAVLink from the simulator reaches the native vehicle link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Link {
    /// The autopilot sends to UDP 14550 in WSL; the app's router bridges it to TCP 5760.
    Router,
    /// The simulator serves TCP 5760 itself.
    Direct,
}

/// One process the launch starts, in order.
#[derive(Debug)]
pub(crate) struct Stage {
    pub profile: &'static str,
    pub readiness: Readiness,
}

/// The ROS 2 bridge of a simulator: the transport profile, the topic prefix that proves it
/// carries the vehicle, and further profiles started with it (the Gazebo camera bridge).
#[derive(Debug)]
pub(crate) struct Ros {
    pub agent: &'static str,
    pub topic_prefix: &'static str,
    pub extra: &'static [&'static str],
}

/// Where a system keeps a `Param`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// In the world (`World::params`), such as the Gazebo world and model.
    World,
    /// On the autopilot, such as the ArduPilot vehicle.
    Autopilot,
}

/// A world or vehicle detail the system chooses, passed to the launch as `env`.
#[derive(Debug)]
pub(crate) struct Param {
    pub key: &'static str,
    pub label: &'static str,
    pub scope: Scope,
    pub default: &'static str,
    pub env: &'static str,
    /// The value as the launch command expects it.
    pub map: fn(&str) -> String,
}

#[derive(Debug)]
pub(crate) struct Adapter {
    pub id: &'static str,
    /// How lists name a system on this adapter.
    pub name: &'static str,
    pub autopilot: AutopilotKind,
    /// The world id systems store (`internal` is the autopilot's own physics).
    pub world: &'static str,
    pub stages: &'static [Stage],
    pub link: Link,
    pub ros: Option<Ros>,
    pub params: &'static [Param],
    /// Setup recipes (`scripts/setup/catalog.json`) the simulator needs, in install order.
    pub recipes: &'static [&'static str],
    /// Whether the autopilot answers the MAVLink shell (PX4 does, ArduPilot does not).
    pub shell: bool,
}
impl Adapter {
    /// The profile that runs the vehicle: the first stage.
    pub(crate) fn vehicle_profile(&self) -> &'static str {
        self.stages[0].profile
    }
    /// Profiles that receive this adapter's `params` as environment: its stages and the
    /// extra ROS profiles started with it.
    fn takes_params(&self, profile: &str) -> bool {
        self.stages.iter().any(|st| st.profile == profile)
            || self
                .ros
                .as_ref()
                .is_some_and(|r| r.extra.contains(&profile))
    }
}

/// The adapter for `autopilot` in `world`.
pub(crate) fn find(autopilot: AutopilotKind, world: &str) -> Option<&'static Adapter> {
    ALL.iter()
        .find(|a| a.autopilot == autopilot && a.world == world)
}

/// The adapter one of whose stages is profile `id`.
pub(crate) fn by_profile(id: &str) -> Option<&'static Adapter> {
    ALL.iter()
        .find(|a| a.stages.iter().any(|st| st.profile == id))
}

/// Whether profile `id` runs a simulator.
pub(crate) fn is_simulator(id: &str) -> bool {
    by_profile(id).is_some()
}

/// Every simulator profile, in table order.
pub(crate) fn simulator_profiles() -> impl Iterator<Item = &'static str> {
    ALL.iter()
        .flat_map(|a| a.stages.iter().map(|st| st.profile))
}

/// How profile `id` reports readiness; `ProcessRunning` for any profile that is not a stage.
pub(crate) fn readiness_of(id: &str) -> Readiness {
    by_profile(id)
        .and_then(|a| a.stages.iter().find(|st| st.profile == id))
        .map_or(Readiness::ProcessRunning, |st| st.readiness)
}

/// Whether profile `id` is ready: it runs and, where its stage reports more, has reported it.
/// The one readiness rule the launch, the System page and First run share.
pub(crate) fn ready(s: &State, id: &str) -> bool {
    let running = s.status.get(id).is_some_and(|v| v == "Running");
    match readiness_of(id) {
        Readiness::Px4Status { .. } => running && s.readiness.get(id).is_some_and(|r| r == "Ready"),
        Readiness::ProcessRunning => running,
    }
}

/// Environment for profile `id` started for `sys`: the world and vehicle details its adapter
/// takes. Values reach the process through the agent's environment, never the command line.
pub(crate) fn launch_env(sys: &System, id: &str) -> Vec<(String, String)> {
    let Some(a) = sys.adapter().filter(|a| a.takes_params(id)) else {
        return Vec::new();
    };
    a.params
        .iter()
        .map(|p| {
            let value = sys.param(p.key).unwrap_or(p.default);
            (p.env.to_string(), (p.map)(value))
        })
        .collect()
}
