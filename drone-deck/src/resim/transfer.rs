//! Which recorded parameters a fresh simulator takes. Calibration, hardware ports, batteries
//! and the airframe the launch picked are never copied; a simulator's own parameters are
//! copied only when the run uses the same world. Every parameter left out is reported with
//! the reason.
use crate::mavlink::{Param, ParamSet};

/// Prefixes of parameters that describe one physical board or its wiring.
const HARDWARE: &[&str] = &[
    "CAL_", "SENS_", "PWM_", "SER_", "BAT", "BATT", "INS_", "COMPASS_", "SERIAL", "BRD_", "RC_MAP_",
];
/// Parameters the launch chooses for the world (the airframe, and the DDS client's domain and
/// namespace, `UXRCE_DDS_*`), not the flight.
const LAUNCH: &[&str] = &["SYS_AUTOSTART", "SYS_AUTOCONFIG", "MAV_SYS_ID"];
/// Prefixes of a simulator's own parameters.
const SIMULATOR: &[&str] = &["SIM_", "SIH_"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Skip {
    /// Calibration, ports, battery: belongs to one board.
    Hardware,
    /// Chosen by the launch for this world.
    Launch,
    /// Another simulator's own setting.
    OtherSimulator,
    /// The fresh autopilot has no parameter of that name.
    Unknown,
}

impl Skip {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Hardware => "hardware calibration or wiring",
            Self::Launch => "set by the launch for this world",
            Self::OtherSimulator => "a setting of the recorded simulator",
            Self::Unknown => "not a parameter of this autopilot",
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Transfer {
    /// Parameters to write, in name order, with the fresh autopilot's types when known.
    pub apply: Vec<Param>,
    pub skipped: Vec<(String, Skip)>,
    /// Recorded values the fresh autopilot already has.
    pub unchanged: usize,
}

fn is_mav_config(name: &str) -> bool {
    name.strip_prefix("MAV_")
        .and_then(|r| r.strip_suffix("_CONFIG"))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// Why `name` is not copied, or `None` when it is.
pub(crate) fn skip(name: &str, same_world: bool) -> Option<Skip> {
    if LAUNCH.contains(&name) || name.starts_with("UXRCE_DDS_") {
        Some(Skip::Launch)
    } else if HARDWARE.iter().any(|p| name.starts_with(p))
        || is_mav_config(name)
        || (name.starts_with("RC") && name[2..].starts_with(|c: char| c.is_ascii_digit()))
    {
        Some(Skip::Hardware)
    } else if !same_world && SIMULATOR.iter().any(|p| name.starts_with(p)) {
        Some(Skip::OtherSimulator)
    } else {
        None
    }
}

/// The parameters to write from `recorded` into a fresh autopilot whose own set is `fresh`
/// (downloaded after it started; `None` writes every allowed parameter).
pub(crate) fn plan(recorded: &ParamSet, fresh: Option<&ParamSet>, same_world: bool) -> Transfer {
    let mut t = Transfer::default();
    for (name, p) in &recorded.params {
        if let Some(why) = skip(name, same_world) {
            t.skipped.push((name.clone(), why));
            continue;
        }
        let Some(fresh) = fresh else {
            t.apply.push(p.clone());
            continue;
        };
        match fresh.params.get(name) {
            None => t.skipped.push((name.clone(), Skip::Unknown)),
            Some(now) if now.value.as_f64() == p.value.as_f64() => t.unchanged += 1,
            Some(now) => t.apply.push(Param {
                kind: now.kind,
                ..p.clone()
            }),
        }
    }
    t
}

#[cfg(test)]
#[path = "transfer_tests.rs"]
mod tests;
