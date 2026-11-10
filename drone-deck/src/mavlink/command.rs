//! Vehicle commands: the authority gate, COMMAND_LONG encoding per flight stack, and
//! COMMAND_ACK tracking with retries.
use super::frame::{Msg, dialect};
use super::modes::{self, Autopilot};
use super::state::VehicleState;
use std::time::{Duration, Instant};

/// A flight command the UI can ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandKind {
    Arm,
    Disarm,
    /// Climb to `alt` metres above home.
    Takeoff {
        alt: f32,
    },
    Land,
    ReturnToLaunch,
    /// PX4 Hold, ArduPilot Loiter.
    Hold,
    /// A mode name from `modes::available`.
    SetMode(String),
}

impl CommandKind {
    pub fn label(&self) -> String {
        match self {
            Self::Arm => "Arm".into(),
            Self::Disarm => "Disarm".into(),
            Self::Takeoff { alt } => format!("Takeoff to {alt:.0} m"),
            Self::Land => "Land".into(),
            Self::ReturnToLaunch => "Return to launch".into(),
            Self::Hold => "Hold".into(),
            Self::SetMode(name) => format!("Mode {name}"),
        }
    }
}

/// Facts the UI layer checks before it may fly a vehicle. All three must hold.
#[derive(Clone, Copy, Debug, Default)]
pub struct SimulationEvidence {
    /// The service's link is the app-owned simulator router (not a physical endpoint).
    pub app_router_link: bool,
    /// The active system's source is a simulator.
    pub simulator_source: bool,
    /// Its PX4 / ArduPilot process was started by this app and is running.
    pub stack_owned_running: bool,
}

/// Proof that `SimulationEvidence` held; only `Authority::from_evidence` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationGrant(());

/// What a caller may do on a link. Flight commands and the shell need `OwnedSimulation`;
/// a service whose link is `LinkOrigin::Physical` refuses them whatever the authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authority {
    ReadOnly,
    OwnedSimulation(SimulationGrant),
}

impl Authority {
    pub fn from_evidence(evidence: SimulationEvidence) -> Self {
        if evidence.app_router_link && evidence.simulator_source && evidence.stack_owned_running {
            Self::OwnedSimulation(SimulationGrant(()))
        } else {
            Self::ReadOnly
        }
    }
    pub fn may_fly(self) -> bool {
        matches!(self, Self::OwnedSimulation(_))
    }
}

/// Why a request was refused before anything was sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The authority is read-only.
    ReadOnly,
    /// The link is a physical vehicle; it is read-only in this app.
    PhysicalLink,
    /// The service thread has stopped.
    Stopped,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ReadOnly => "Commands need an app-owned simulation that is running",
            Self::PhysicalLink => "Physical vehicle links are read-only",
            Self::Stopped => "The MAVLink service has stopped",
        })
    }
}

/// How a sent command ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandResult {
    Accepted,
    /// The vehicle answered with a MAV_RESULT other than accepted / in progress.
    Rejected {
        result: u8,
        reason: &'static str,
    },
    /// No COMMAND_ACK after every attempt.
    TimedOut {
        attempts: u32,
    },
    /// No vehicle heartbeat yet.
    NoVehicle,
    /// The same MAVLink command is still waiting for its acknowledgement.
    Busy,
    /// This flight stack has no such mode or command here.
    Unsupported,
    /// PX4 takes off to an altitude above sea level, and the vehicle hasn't reported where
    /// home is yet, so the requested height can't be expressed.
    NoHome,
}

pub fn result_name(result: u8) -> &'static str {
    match result {
        0 => "Accepted",
        1 => "Temporarily rejected",
        2 => "Denied",
        3 => "Unsupported",
        4 => "Failed",
        5 => "In progress",
        6 => "Cancelled",
        _ => "Rejected",
    }
}

/// Whether `kind` is a Takeoff PX4 can't be given yet: it takes an altitude above sea level,
/// and the home altitude isn't known (sending NaN would climb to PX4's default height, not
/// the one the user confirmed).
pub fn takeoff_needs_home(kind: &CommandKind, vehicle: &VehicleState) -> bool {
    matches!(kind, CommandKind::Takeoff { .. })
        && vehicle.autopilot == Autopilot::Px4
        && vehicle.home_alt_msl().is_none()
}

/// COMMAND_LONG fields for a command on this vehicle; `None` when it can't be encoded (an
/// unknown mode, or a PX4 takeoff before home is known).
pub fn encode(kind: &CommandKind, vehicle: &VehicleState) -> Option<(dialect::MavCmd, [f32; 7])> {
    use dialect::MavCmd::*;
    let px4 = vehicle.autopilot == Autopilot::Px4;
    let mode = |name: &str| {
        modes::encode_mode(vehicle.autopilot, vehicle.vehicle_type, name)
            .map(|[a, b, c]| (MAV_CMD_DO_SET_MODE, [a, b, c, 0.0, 0.0, 0.0, 0.0]))
    };
    Some(match kind {
        CommandKind::Arm => (
            MAV_CMD_COMPONENT_ARM_DISARM,
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        CommandKind::Disarm => (MAV_CMD_COMPONENT_ARM_DISARM, [0.0; 7]),
        CommandKind::Takeoff { alt } => {
            let target = if px4 {
                // PX4 takes an AMSL altitude.
                vehicle.home_alt_msl()? + alt
            } else {
                *alt
            };
            let nan = f32::NAN;
            let yaw = if px4 { nan } else { 0.0 };
            let (lat, lon) = if px4 { (nan, nan) } else { (0.0, 0.0) };
            (MAV_CMD_NAV_TAKEOFF, [0.0, 0.0, 0.0, yaw, lat, lon, target])
        }
        CommandKind::Land => {
            let v = if px4 { f32::NAN } else { 0.0 };
            (MAV_CMD_NAV_LAND, [0.0, 0.0, 0.0, v, v, v, v])
        }
        CommandKind::ReturnToLaunch => (MAV_CMD_NAV_RETURN_TO_LAUNCH, [0.0; 7]),
        CommandKind::Hold => return mode(modes::hold_mode(vehicle.autopilot)),
        CommandKind::SetMode(name) => return mode(name),
    })
}

/// Builds the COMMAND_LONG message for one attempt.
pub fn command_long(
    cmd: dialect::MavCmd,
    params: [f32; 7],
    vehicle: &VehicleState,
    confirmation: u8,
) -> Msg {
    Msg::COMMAND_LONG(dialect::COMMAND_LONG_DATA {
        param1: params[0],
        param2: params[1],
        param3: params[2],
        param4: params[3],
        param5: params[4],
        param6: params[5],
        param7: params[6],
        command: cmd,
        target_system: vehicle.system_id,
        target_component: vehicle.component_id,
        confirmation,
    })
}

struct Pending {
    id: u64,
    kind: CommandKind,
    cmd: dialect::MavCmd,
    params: [f32; 7],
    attempts: u32,
    deadline: Instant,
    /// The (system, component) the first attempt went to. A retry never goes anywhere else.
    target: (u8, u8),
    /// The vehicle answered IN_PROGRESS: it has the command, so it is never resent.
    in_progress: bool,
}

/// Commands waiting for COMMAND_ACK. One per MAV_CMD at a time.
pub struct Commands {
    pending: Vec<Pending>,
    pub timeout: Duration,
    pub max_attempts: u32,
}

/// What the tracker wants the service to do.
pub enum CommandStep {
    Send(Msg),
    Done {
        id: u64,
        kind: CommandKind,
        result: CommandResult,
    },
}

impl Commands {
    pub fn new(timeout: Duration, max_attempts: u32) -> Self {
        Self {
            pending: Vec::new(),
            timeout,
            max_attempts: max_attempts.max(1),
        }
    }
    /// Starts a command; returns the first COMMAND_LONG or an immediate result.
    pub fn start(
        &mut self,
        id: u64,
        kind: CommandKind,
        vehicle: Option<&VehicleState>,
        now: Instant,
    ) -> CommandStep {
        let done = |result| CommandStep::Done {
            id,
            kind: kind.clone(),
            result,
        };
        let Some(vehicle) = vehicle else {
            return done(CommandResult::NoVehicle);
        };
        if takeoff_needs_home(&kind, vehicle) {
            return done(CommandResult::NoHome);
        }
        let Some((cmd, params)) = encode(&kind, vehicle) else {
            return done(CommandResult::Unsupported);
        };
        if self.pending.iter().any(|p| p.cmd == cmd) {
            return done(CommandResult::Busy);
        }
        self.pending.push(Pending {
            id,
            kind,
            cmd,
            params,
            attempts: 1,
            deadline: now + self.timeout,
            target: (vehicle.system_id, vehicle.component_id),
            in_progress: false,
        });
        CommandStep::Send(command_long(cmd, params, vehicle, 0))
    }
    /// Handles a COMMAND_ACK addressed to us.
    pub fn on_ack(&mut self, ack: &dialect::COMMAND_ACK_DATA, now: Instant) -> Option<CommandStep> {
        let index = self.pending.iter().position(|p| p.cmd == ack.command)?;
        let result = ack.result as u8;
        if result == 5 {
            // In progress: keep waiting without resending.
            self.pending[index].in_progress = true;
            self.pending[index].deadline = now + self.timeout * 3;
            return None;
        }
        let p = self.pending.remove(index);
        let result = if result == 0 {
            CommandResult::Accepted
        } else {
            CommandResult::Rejected {
                result,
                reason: result_name(result),
            }
        };
        Some(CommandStep::Done {
            id: p.id,
            kind: p.kind,
            result,
        })
    }
    /// Resends or times out commands whose deadline passed. A command whose vehicle was replaced
    /// by another system or component times out instead of reaching the new one.
    pub fn poll(&mut self, vehicle: Option<&VehicleState>, now: Instant) -> Vec<CommandStep> {
        let mut steps = Vec::new();
        let max = self.max_attempts;
        let timeout = self.timeout;
        self.pending.retain_mut(|p| {
            if now < p.deadline {
                return true;
            }
            match vehicle {
                Some(v)
                    if p.attempts < max
                        && !p.in_progress
                        && (v.system_id, v.component_id) == p.target =>
                {
                    p.attempts += 1;
                    p.deadline = now + timeout;
                    let confirmation = (p.attempts - 1).min(255) as u8;
                    steps.push(CommandStep::Send(command_long(
                        p.cmd,
                        p.params,
                        v,
                        confirmation,
                    )));
                    true
                }
                _ => {
                    steps.push(CommandStep::Done {
                        id: p.id,
                        kind: p.kind.clone(),
                        result: CommandResult::TimedOut {
                            attempts: p.attempts,
                        },
                    });
                    false
                }
            }
        });
        steps
    }
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.iter().map(|p| p.deadline).min()
    }
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
