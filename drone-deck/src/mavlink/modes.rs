//! Flight-mode names: decoding a heartbeat's custom mode and encoding a named mode for
//! MAV_CMD_DO_SET_MODE, for PX4 and ArduPilot Copter.

/// Flight stack family, from HEARTBEAT.autopilot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autopilot {
    Px4,
    ArduPilot,
    Other(u8),
}

impl Autopilot {
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            12 => Self::Px4,
            3 => Self::ArduPilot,
            other => Self::Other(other),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Px4 => "PX4",
            Self::ArduPilot => "ArduPilot",
            Self::Other(_) => "Other autopilot",
        }
    }
}

/// Whether a heartbeat comes from a flight controller: a real autopilot, and not a ground
/// station or a peripheral (camera, gimbal, companion computer, ADS-B, battery…) that some
/// stacks announce with their autopilot id.
pub fn is_vehicle_heartbeat(autopilot: u8, mavtype: u8) -> bool {
    const NOT_VEHICLES: &[u8] = &[
        5, 6, 18, 26, 27, 30, 31, 32, 33, 34, 36, 37, 38, 39, 40, 41, 42, 44,
    ];
    autopilot != 8 && !NOT_VEHICLES.contains(&mavtype)
}

/// MAV_TYPE values flown by ArduCopter.
pub fn is_copter(vehicle_type: u8) -> bool {
    matches!(vehicle_type, 2 | 3 | 4 | 13 | 14 | 15 | 29)
}

pub fn vehicle_type_name(vehicle_type: u8) -> &'static str {
    match vehicle_type {
        1 => "Fixed wing",
        2 => "Quadrotor",
        3 => "Coaxial",
        4 => "Helicopter",
        10 => "Ground rover",
        11 => "Surface boat",
        12 => "Submarine",
        13 => "Hexarotor",
        14 => "Octorotor",
        15 => "Tricopter",
        19..=25 => "VTOL",
        29 => "Dodecarotor",
        _ => "Generic",
    }
}

const PX4_MODES: &[(&str, u8, u8)] = &[
    ("Manual", 1, 0),
    ("Altitude", 2, 0),
    ("Position", 3, 0),
    ("Orbit", 3, 1),
    ("Position Slow", 3, 2),
    ("Ready", 4, 1),
    ("Takeoff", 4, 2),
    ("Hold", 4, 3),
    ("Mission", 4, 4),
    ("Return", 4, 5),
    ("Land", 4, 6),
    ("Follow Me", 4, 8),
    ("Precision Land", 4, 9),
    ("VTOL Takeoff", 4, 10),
    ("Acro", 5, 0),
    ("Offboard", 6, 0),
    ("Stabilized", 7, 0),
    ("Rattitude", 8, 0),
    ("Termination", 10, 0),
];

const COPTER_MODES: &[(&str, u32)] = &[
    ("Stabilize", 0),
    ("Acro", 1),
    ("Altitude Hold", 2),
    ("Auto", 3),
    ("Guided", 4),
    ("Loiter", 5),
    ("RTL", 6),
    ("Circle", 7),
    ("Land", 9),
    ("Drift", 11),
    ("Sport", 13),
    ("Flip", 14),
    ("Autotune", 15),
    ("Position Hold", 16),
    ("Brake", 17),
    ("Throw", 18),
    ("Avoid ADSB", 19),
    ("Guided No GPS", 20),
    ("Smart RTL", 21),
    ("Flow Hold", 22),
    ("Follow", 23),
    ("ZigZag", 24),
    ("System ID", 25),
    ("Heli Autorotate", 26),
    ("Auto RTL", 27),
    ("Turtle", 28),
];

/// The name of the mode a heartbeat reports.
pub fn mode_name(
    autopilot: Autopilot,
    vehicle_type: u8,
    base_mode: u8,
    custom_mode: u32,
) -> String {
    if base_mode & 1 == 0 {
        return if base_mode & 4 != 0 {
            "Auto".into()
        } else if base_mode & 8 != 0 {
            "Guided".into()
        } else if base_mode & 64 != 0 {
            "Manual".into()
        } else {
            "Unknown".into()
        };
    }
    match autopilot {
        Autopilot::Px4 => {
            let main = ((custom_mode >> 16) & 0xff) as u8;
            let sub = ((custom_mode >> 24) & 0xff) as u8;
            let sub = if main == 4 || main == 3 { sub } else { 0 };
            PX4_MODES
                .iter()
                .find(|(_, m, s)| *m == main && *s == sub)
                .map(|(name, ..)| (*name).to_string())
                .unwrap_or_else(|| format!("Mode {main}.{sub}"))
        }
        Autopilot::ArduPilot if is_copter(vehicle_type) => COPTER_MODES
            .iter()
            .find(|(_, number)| *number == custom_mode)
            .map(|(name, _)| (*name).to_string())
            .unwrap_or_else(|| format!("Mode {custom_mode}")),
        _ => format!("Mode {custom_mode}"),
    }
}

/// DO_SET_MODE parameters (param1 base mode, param2 custom main mode, param3 sub mode).
pub fn encode_mode(autopilot: Autopilot, vehicle_type: u8, name: &str) -> Option<[f32; 3]> {
    if !selectable(name) {
        return None;
    }
    match autopilot {
        Autopilot::Px4 => PX4_MODES
            .iter()
            .find(|(n, ..)| n.eq_ignore_ascii_case(name))
            .map(|&(_, main, sub)| [1.0, main as f32, sub as f32]),
        Autopilot::ArduPilot if is_copter(vehicle_type) => COPTER_MODES
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|&(_, number)| [1.0, number as f32, 0.0]),
        _ => None,
    }
}

/// Mode names the UI can offer for this vehicle.
pub fn available(autopilot: Autopilot, vehicle_type: u8) -> Vec<&'static str> {
    match autopilot {
        Autopilot::Px4 => PX4_MODES
            .iter()
            .map(|(n, ..)| *n)
            .filter(|n| selectable(n))
            .collect(),
        Autopilot::ArduPilot if is_copter(vehicle_type) => {
            COPTER_MODES.iter().map(|(n, _)| *n).collect()
        }
        _ => Vec::new(),
    }
}

/// Modes the app never sets: flight termination (it cuts the motors) and states a flight
/// stack enters on its own.
fn selectable(name: &str) -> bool {
    !["Termination", "Ready"]
        .iter()
        .any(|n| n.eq_ignore_ascii_case(name))
}

/// The "hold position" mode for this stack.
pub fn hold_mode(autopilot: Autopilot) -> &'static str {
    match autopilot {
        Autopilot::ArduPilot => "Loiter",
        _ => "Hold",
    }
}

pub fn gps_fix_name(fix: u8) -> &'static str {
    match fix {
        0 => "No GPS",
        1 => "No fix",
        2 => "2D fix",
        3 => "3D fix",
        4 => "DGPS",
        5 => "RTK float",
        6 => "RTK fixed",
        7 => "Static",
        8 => "PPP",
        _ => "Unknown",
    }
}

pub fn landed_name(state: u8) -> &'static str {
    match state {
        1 => "On ground",
        2 => "In air",
        3 => "Taking off",
        4 => "Landing",
        _ => "Unknown",
    }
}

#[cfg(test)]
#[path = "modes_tests.rs"]
mod tests;
