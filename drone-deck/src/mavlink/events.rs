//! PX4's health and arming check report, read passively from the MAVLink events protocol
//! (EVENT, id 410, and CURRENT_EVENT_SEQUENCE, id 411). Nothing is sent: a missed event is
//! not asked for again with REQUEST_EVENT, so the report kept is the last complete one this
//! link carried. PX4 sends it when a result changes, not periodically, so it describes the
//! vehicle until the next report, a sequence reset (reboot) or another vehicle.
//!
//! Because a lost event is not recovered, the report is marked possibly outdated once an event
//! after it went missing: an EVENT sequence that skips ahead, or a periodic
//! CURRENT_EVENT_SEQUENCE (the sequence of the vehicle's latest event, `get_latest_sequence` in
//! <https://github.com/PX4/PX4-Autopilot/blob/main/src/modules/mavlink/mavlink_events.cpp>)
//! ahead of the last EVENT received. Sequences compare with wrap-around, as libevents'
//! `compareSequence` does in
//! <https://github.com/mavlink/libevents/blob/main/libs/cpp/protocol/receive.h>.
//!
//! Layout (PX4 v1.14 and later), from the public PX4 and libevents sources:
//! - An event id is `(1 << 24) | (fnv1a32(name) & 0xff_ffff)`, the autopilot component in the
//!   top byte (`events::ID` in
//!   <https://github.com/PX4/PX4-Autopilot/blob/main/platforms/common/include/px4_platform_common/events.h>).
//!   Arguments are packed back to back, little-endian, from byte 0 of `arguments`.
//! - `commander_arming_check_summary`: u8 chunk, u32 error components, u32 warning components,
//!   u32 can-arm mode groups, u32 can-run mode groups. `commander_health_summary`: u8 chunk,
//!   u32 present, u32 error and u32 warning components. Arming comes first, then the
//!   individual checks, then health (`Report::report` in
//!   <https://github.com/PX4/PX4-Autopilot/blob/main/src/modules/commander/HealthAndArmingChecks/Common.cpp>;
//!   the same pairing as libevents' `HealthAndArmingChecks`, which QGroundControl uses:
//!   <https://github.com/mavlink/libevents/blob/main/libs/cpp/parse/health_and_arming_checks.cpp>).
//! - Component bits are `health_component_t` and mode groups `navigation_mode_group_t` in
//!   <https://github.com/PX4/PX4-Autopilot/blob/main/src/lib/events/enums.json>. A mode group
//!   is `1 << nav_state` (`Report::getModeGroup`), and the heartbeat's custom mode maps to a
//!   nav_state as in `get_px4_custom_mode` in
//!   <https://github.com/PX4/PX4-Autopilot/blob/main/src/modules/commander/px4_custom_mode.h>.
//!
//! The individual check events carry their text only in the vehicle's event metadata (a
//! component-information download), so they are not decoded here.
use super::frame::dialect;
use super::modes::Autopilot;

/// FNV-1a, 32 bits, as PX4 hashes event names.
const fn fnv1a(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut hash: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

/// The id PX4's autopilot component gives the event `name`.
pub const fn px4_event_id(name: &str) -> u32 {
    (1 << 24) | (fnv1a(name) & 0x00ff_ffff)
}

pub const ARMING_SUMMARY_ID: u32 = px4_event_id("commander_arming_check_summary");
pub const HEALTH_SUMMARY_ID: u32 = px4_event_id("commander_health_summary");

/// The most events that may sit between the two summaries of one report. PX4 sends fewer
/// (its event queue is short); a health summary further away belongs to another report.
const MAX_GAP: u16 = 128;

/// `health_component_t` bits, as labels.
const COMPONENTS: [&str; 32] = [
    "General",
    "Absolute pressure",
    "Differential pressure",
    "GPS",
    "Optical flow",
    "Vision position",
    "Distance sensor",
    "Remote control",
    "Motors / ESCs",
    "UTM",
    "Logging",
    "Battery",
    "Communication links",
    "Rate controller",
    "Attitude controller",
    "Position controller",
    "Attitude estimate",
    "Local position estimate",
    "Mission",
    "Avoidance",
    "System",
    "Camera",
    "Gimbal",
    "Payload",
    "Global position estimate",
    "Storage",
    "Parachute",
    "Magnetometer",
    "Accelerometer",
    "Gyroscope",
    "Open Drone ID",
    "Traffic avoidance",
];

/// `navigation_mode_group_t` bits (= nav_state), named like the heartbeat modes; `None`
/// where PX4 defines no group.
const MODE_GROUPS: [Option<&str>; 31] = [
    Some("Manual"),
    Some("Altitude"),
    Some("Position"),
    Some("Mission"),
    Some("Hold"),
    Some("Return"),
    Some("Position Slow"),
    Some("Course"),
    Some("Altitude Cruise"),
    Some("Manual Parking"),
    Some("Acro"),
    None,
    None,
    None,
    Some("Offboard"),
    Some("Stabilized"),
    None,
    Some("Takeoff"),
    Some("Land"),
    Some("Follow Me"),
    Some("Precision Land"),
    Some("Orbit"),
    Some("VTOL Takeoff"),
    Some("External 1"),
    Some("External 2"),
    Some("External 3"),
    Some("External 4"),
    Some("External 5"),
    Some("External 6"),
    Some("External 7"),
    Some("External 8"),
];

/// Labels of the `health_component_t` bits set in `mask`, lowest bit first.
pub fn component_names(mask: u32) -> Vec<&'static str> {
    (0..32)
        .filter(|bit| mask & (1 << bit) != 0)
        .map(|bit| COMPONENTS[bit])
        .collect()
}

/// Names of the mode groups set in `mask`, lowest bit first (bits without a group skipped).
pub fn mode_group_names(mask: u32) -> Vec<&'static str> {
    (0..31)
        .filter(|bit| mask & (1 << bit) != 0)
        .filter_map(|bit| MODE_GROUPS[bit])
        .collect()
}

/// PX4's nav_state for a heartbeat custom mode (main mode in bits 16..24, sub mode above).
fn px4_nav_state(custom_mode: u32) -> Option<u8> {
    let main = (custom_mode >> 16) & 0xff;
    let sub = (custom_mode >> 24) & 0xff;
    Some(match (main, sub) {
        (1, _) => 0,
        (2, _) => 1,
        (3, 0) => 2,
        (3, 1) => 21,
        (3, 2) => 6,
        (4, 2) => 17,
        (4, 3) => 4,
        (4, 4) => 3,
        (4, 5) => 5,
        (4, 6) => 18,
        (4, 8) => 19,
        (4, 9) => 20,
        (4, 10) => 22,
        (4, n @ 11..=18) => 23 + (n as u8 - 11),
        (4, 19) => 7,
        (4, 20) => 12,
        (5, _) => 10,
        (6, _) => 14,
        (7, _) => 15,
        (10, _) => 13,
        (11, _) => 8,
        (12, _) => 9,
        _ => return None,
    })
}

/// The current mode's group bit and name, for a PX4 heartbeat in custom mode whose mode has
/// a group. `None` otherwise: the report can't then say whether *this* mode may arm.
pub fn mode_group(
    autopilot: Autopilot,
    base_mode: u8,
    custom_mode: u32,
) -> Option<(u32, &'static str)> {
    if autopilot != Autopilot::Px4 || base_mode & 1 == 0 {
        return None;
    }
    let state = px4_nav_state(custom_mode)?;
    let name = MODE_GROUPS.get(state as usize).copied().flatten()?;
    Some((1 << state, name))
}

/// One complete health and arming check report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HealthReport {
    /// Components whose arming checks fail (`health_component_t` bits).
    pub arming_errors: u32,
    pub arming_warnings: u32,
    /// Mode groups the vehicle may arm in (`navigation_mode_group_t` bits).
    pub can_arm: u32,
    /// Mode groups the vehicle may stay in once armed.
    pub can_run: u32,
    /// Components the health report covers.
    pub present: u32,
    pub health_errors: u32,
    pub health_warnings: u32,
}

impl HealthReport {
    /// Whether arming is allowed in the mode group `group` (a single bit).
    pub fn can_arm_in(&self, group: u32) -> bool {
        self.can_arm & group != 0
    }
    /// Components with an arming or health error.
    pub fn error_components(&self) -> u32 {
        self.arming_errors | self.health_errors
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ArmingSummary {
    sequence: u16,
    chunk: u8,
    errors: u32,
    warnings: u32,
    can_arm: u32,
    can_run: u32,
}

/// Pairs the two summaries of a report, keeping the last complete one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HealthEvents {
    pending: Option<ArmingSummary>,
    pub report: Option<HealthReport>,
    /// Sequence of the last EVENT received (or announced, before any arrived).
    last_sequence: Option<u16>,
    /// An event went missing after the report, so a newer report may have been lost.
    missed: bool,
}

/// Whether `incoming` is ahead of `expected`, with wrap-around (libevents' "newer").
fn newer(incoming: u16, expected: u16) -> bool {
    let diff = incoming.wrapping_sub(expected);
    diff != 0 && diff <= u16::MAX / 2
}

fn u32_at(args: &[u8; 40], at: usize) -> u32 {
    u32::from_le_bytes([args[at], args[at + 1], args[at + 2], args[at + 3]])
}

impl HealthEvents {
    /// Whether events were lost since the report, so it may no longer describe the vehicle.
    pub fn outdated(&self) -> bool {
        self.report.is_some() && self.missed
    }
    /// As if an event after the report went missing (for view tests).
    #[cfg(test)]
    pub fn assume_missed(&mut self) {
        self.missed = true;
    }
    /// Marks the report possibly outdated. Returns true when that is news.
    fn mark_missed(&mut self) -> bool {
        let news = self.report.is_some() && !self.missed;
        self.missed |= self.report.is_some();
        news
    }
    /// Feeds one EVENT. Returns true when the report, or whether it may be outdated, changed.
    pub fn on_event(&mut self, e: &dialect::EVENT_DATA) -> bool {
        let mut changed = false;
        match self.last_sequence {
            // A duplicate or a late, reordered event: the sequence stays.
            Some(last) if !newer(e.sequence, last) && last.wrapping_sub(e.sequence) <= MAX_GAP => {}
            Some(last) if newer(e.sequence, last) => {
                if e.sequence != last.wrapping_add(1) {
                    changed = self.mark_missed();
                }
                self.last_sequence = Some(e.sequence);
            }
            // First event, or far behind (a reboot whose reset went unseen): start over.
            _ => self.last_sequence = Some(e.sequence),
        }
        self.on_summary(e) | changed
    }
    fn on_summary(&mut self, e: &dialect::EVENT_DATA) -> bool {
        let a = &e.arguments;
        match e.id {
            ARMING_SUMMARY_ID => {
                self.pending = Some(ArmingSummary {
                    sequence: e.sequence,
                    chunk: a[0],
                    errors: u32_at(a, 1),
                    warnings: u32_at(a, 5),
                    can_arm: u32_at(a, 9),
                    can_run: u32_at(a, 13),
                });
                false
            }
            HEALTH_SUMMARY_ID => {
                let Some(arming) = self.pending.take() else {
                    return false;
                };
                let gap = e.sequence.wrapping_sub(arming.sequence);
                if arming.chunk != a[0] || gap == 0 || gap > MAX_GAP {
                    return false;
                }
                let report = HealthReport {
                    arming_errors: arming.errors,
                    arming_warnings: arming.warnings,
                    can_arm: arming.can_arm,
                    can_run: arming.can_run,
                    present: u32_at(a, 1),
                    health_errors: u32_at(a, 5),
                    health_warnings: u32_at(a, 9),
                };
                let changed = self.report != Some(report) || self.missed;
                self.report = Some(report);
                self.missed = false;
                changed
            }
            _ => false,
        }
    }
    /// Feeds one CURRENT_EVENT_SEQUENCE. A sequence ahead of the last EVENT received means
    /// events were lost, so the report may be outdated. A reset (the vehicle rebooted, or has
    /// sent no event since boot) drops the report, which described the vehicle before it.
    /// Returns true when the report, or whether it may be outdated, changed.
    pub fn on_sequence(&mut self, s: &dialect::CURRENT_EVENT_SEQUENCE_DATA) -> bool {
        let reset = s.flags.contains(
            dialect::MavEventCurrentSequenceFlags::MAV_EVENT_CURRENT_SEQUENCE_FLAGS_RESET,
        );
        if !reset {
            return match self.last_sequence {
                Some(last) if newer(s.sequence, last) => self.mark_missed(),
                Some(_) => false,
                None => {
                    self.last_sequence = Some(s.sequence);
                    false
                }
            };
        }
        self.last_sequence = Some(s.sequence);
        self.missed = false;
        // PX4 may announce the reset just after the first events of the new boot: keep an
        // arming summary at or just before the announced sequence, drop an older one.
        if self
            .pending
            .is_some_and(|p| s.sequence.wrapping_sub(p.sequence) > MAX_GAP)
        {
            self.pending = None;
        }
        self.report.take().is_some()
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
