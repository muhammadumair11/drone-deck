//! What the app knows about the vehicle, built from decoded messages.
use super::events::{HealthEvents, HealthReport};
use super::frame::{Msg, text};
use super::modes::{self, Autopilot};
use std::collections::VecDeque;
use std::time::{Instant, SystemTime};

/// Recent STATUSTEXT entries kept on the vehicle.
pub const STATUS_RING: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub struct StatusText {
    /// MAV_SEVERITY (0 emergency … 7 debug).
    pub severity: u8,
    pub text: String,
    pub at: SystemTime,
}

impl StatusText {
    pub fn severity_name(&self) -> &'static str {
        match self.severity {
            0 => "Emergency",
            1 => "Alert",
            2 => "Critical",
            3 => "Error",
            4 => "Warning",
            5 => "Notice",
            6 => "Info",
            _ => "Debug",
        }
    }
}

/// One vehicle (the first autopilot heartbeat seen on the link).
#[derive(Clone, Debug)]
pub struct VehicleState {
    pub system_id: u8,
    pub component_id: u8,
    pub autopilot: Autopilot,
    /// MAV_TYPE.
    pub vehicle_type: u8,
    pub armed: bool,
    pub base_mode: u8,
    pub custom_mode: u32,
    pub mode: String,
    /// MAV_STATE.
    pub system_status: u8,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Metres above mean sea level.
    pub alt_msl: Option<f32>,
    /// Metres above home.
    pub relative_alt: Option<f32>,
    /// Degrees, 0..360.
    pub heading: Option<f32>,
    /// Degrees.
    pub roll: Option<f32>,
    pub pitch: Option<f32>,
    pub yaw: Option<f32>,
    /// Metres per second.
    pub groundspeed: Option<f32>,
    pub climb: Option<f32>,
    /// Volts.
    pub battery_voltage: Option<f32>,
    /// Percent.
    pub battery_remaining: Option<i8>,
    /// GPS_FIX_TYPE.
    pub gps_fix: u8,
    pub satellites: Option<u8>,
    /// Horizontal dilution, metres-ish (EPH / 100).
    pub eph: Option<f32>,
    /// MAV_SYS_STATUS_SENSOR bits.
    pub sensors_present: u32,
    pub sensors_enabled: u32,
    pub sensors_health: u32,
    /// MAV_LANDED_STATE (0 when the vehicle doesn't send EXTENDED_SYS_STATE).
    pub landed_state: u8,
    pub vtol_state: u8,
    /// HOME_POSITION: latitude, longitude (degrees) and altitude MSL (metres).
    pub home: Option<(f64, f64, f32)>,
    /// PX4's health and arming check report (events protocol), when the vehicle sent one.
    pub health_events: HealthEvents,
    pub last_heartbeat: Instant,
    /// Messages per second by name, sorted by name.
    pub message_rates: Vec<(&'static str, f32)>,
    pub status: VecDeque<StatusText>,
}

impl VehicleState {
    pub fn new(system_id: u8, component_id: u8, now: Instant) -> Self {
        Self {
            system_id,
            component_id,
            autopilot: Autopilot::Other(0),
            vehicle_type: 0,
            armed: false,
            base_mode: 0,
            custom_mode: 0,
            mode: "Unknown".into(),
            system_status: 0,
            lat: None,
            lon: None,
            alt_msl: None,
            relative_alt: None,
            heading: None,
            roll: None,
            pitch: None,
            yaw: None,
            groundspeed: None,
            climb: None,
            battery_voltage: None,
            battery_remaining: None,
            gps_fix: 0,
            satellites: None,
            eph: None,
            sensors_present: 0,
            sensors_enabled: 0,
            sensors_health: 0,
            landed_state: 0,
            vtol_state: 0,
            home: None,
            health_events: HealthEvents::default(),
            last_heartbeat: now,
            message_rates: Vec::new(),
            status: VecDeque::new(),
        }
    }
    pub fn vehicle_type_name(&self) -> &'static str {
        modes::vehicle_type_name(self.vehicle_type)
    }
    pub fn gps_fix_name(&self) -> &'static str {
        modes::gps_fix_name(self.gps_fix)
    }
    pub fn landed_name(&self) -> &'static str {
        modes::landed_name(self.landed_state)
    }
    /// Present and enabled sensors that report unhealthy, as MAV_SYS_STATUS_SENSOR bits.
    pub fn unhealthy_sensors(&self) -> u32 {
        self.sensors_present & self.sensors_enabled & !self.sensors_health
    }
    /// The last complete health and arming check report.
    pub fn health_report(&self) -> Option<&HealthReport> {
        self.health_events.report.as_ref()
    }
    pub fn is_fresh(&self, now: Instant, stale_after: std::time::Duration) -> bool {
        now.saturating_duration_since(self.last_heartbeat) < stale_after
    }
    /// Home altitude above mean sea level, when both altitudes are known.
    pub fn home_alt_msl(&self) -> Option<f32> {
        Some(self.alt_msl? - self.relative_alt?)
    }
    pub fn push_status(&mut self, entry: StatusText) {
        if self.status.len() == STATUS_RING {
            self.status.pop_front();
        }
        self.status.push_back(entry);
    }

    /// Applies one message from this vehicle. Returns true when a visible value changed.
    pub fn apply(&mut self, msg: &Msg, now: Instant) -> bool {
        match msg {
            Msg::HEARTBEAT(h) => {
                self.last_heartbeat = now;
                self.autopilot = Autopilot::from_raw(h.autopilot as u8);
                self.vehicle_type = h.mavtype as u8;
                self.base_mode = h.base_mode.bits();
                self.custom_mode = h.custom_mode;
                self.armed = self.base_mode & 128 != 0;
                self.system_status = h.system_status as u8;
                self.mode = modes::mode_name(
                    self.autopilot,
                    self.vehicle_type,
                    self.base_mode,
                    self.custom_mode,
                );
            }
            Msg::ATTITUDE(a) => {
                self.roll = Some(a.roll.to_degrees());
                self.pitch = Some(a.pitch.to_degrees());
                self.yaw = Some(a.yaw.to_degrees());
            }
            Msg::GLOBAL_POSITION_INT(p) => {
                self.lat = Some(p.lat as f64 / 1e7);
                self.lon = Some(p.lon as f64 / 1e7);
                self.alt_msl = Some(p.alt as f32 / 1000.0);
                self.relative_alt = Some(p.relative_alt as f32 / 1000.0);
                if p.hdg != u16::MAX {
                    self.heading = Some(p.hdg as f32 / 100.0);
                }
                let (vx, vy) = (p.vx as f32 / 100.0, p.vy as f32 / 100.0);
                self.groundspeed = Some((vx * vx + vy * vy).sqrt());
                self.climb = Some(-(p.vz as f32) / 100.0);
            }
            Msg::VFR_HUD(v) => {
                self.groundspeed = Some(v.groundspeed);
                self.climb = Some(v.climb);
                self.heading = Some(v.heading as f32);
            }
            Msg::SYS_STATUS(s) => {
                self.sensors_present = s.onboard_control_sensors_present.bits();
                self.sensors_enabled = s.onboard_control_sensors_enabled.bits();
                self.sensors_health = s.onboard_control_sensors_health.bits();
                if s.voltage_battery != u16::MAX {
                    self.battery_voltage = Some(s.voltage_battery as f32 / 1000.0);
                }
                if s.battery_remaining >= 0 {
                    self.battery_remaining = Some(s.battery_remaining);
                }
            }
            Msg::BATTERY_STATUS(b) if b.id == 0 => {
                if b.battery_remaining >= 0 {
                    self.battery_remaining = Some(b.battery_remaining);
                }
            }
            Msg::GPS_RAW_INT(g) => {
                self.gps_fix = g.fix_type as u8;
                self.satellites = (g.satellites_visible != u8::MAX).then_some(g.satellites_visible);
                self.eph = (g.eph != u16::MAX).then_some(g.eph as f32 / 100.0);
            }
            Msg::EXTENDED_SYS_STATE(e) => {
                self.landed_state = e.landed_state as u8;
                self.vtol_state = e.vtol_state as u8;
            }
            Msg::HOME_POSITION(h) => {
                self.home = Some((
                    h.latitude as f64 / 1e7,
                    h.longitude as f64 / 1e7,
                    h.altitude as f32 / 1000.0,
                ));
            }
            Msg::EVENT(e) => return self.health_events.on_event(e),
            Msg::CURRENT_EVENT_SEQUENCE(c) => return self.health_events.on_sequence(c),
            _ => return false,
        }
        true
    }
}

/// Longest reassembled STATUSTEXT kept; a longer chunk run is cut into several entries.
pub const MAX_STATUS_TEXT: usize = 2048;

/// Reassembles STATUSTEXT messages, including chunked ones (non-zero `id`).
#[derive(Default)]
pub struct StatusAssembler {
    pending: Option<(u16, u8, String)>,
}

impl StatusAssembler {
    /// Feeds one STATUSTEXT; returns a complete entry when one is ready.
    pub fn push(
        &mut self,
        data: &super::frame::dialect::STATUSTEXT_DATA,
        at: SystemTime,
    ) -> Vec<StatusText> {
        let chunk = text(&data.text);
        let severity = data.severity as u8;
        let last_chunk = data.text.contains(&0);
        let mut out = Vec::new();
        if data.id == 0 {
            out.push(StatusText {
                severity,
                text: chunk,
                at,
            });
            return out;
        }
        match self.pending.take() {
            Some((id, sev, mut acc)) if id == data.id && acc.len() < MAX_STATUS_TEXT => {
                acc.push_str(&chunk);
                self.pending = Some((id, sev, acc));
            }
            Some((_, sev, acc)) => {
                out.push(StatusText {
                    severity: sev,
                    text: acc,
                    at,
                });
                self.pending = Some((data.id, severity, chunk));
            }
            None => self.pending = Some((data.id, severity, chunk)),
        }
        if last_chunk && let Some((_, sev, acc)) = self.pending.take() {
            out.push(StatusText {
                severity: sev,
                text: acc,
                at,
            });
        }
        out
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
