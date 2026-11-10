use super::*;
use crate::mavlink::frame::dialect;
use std::time::Duration;

#[test]
fn heartbeat_sets_mode_and_arming() {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    let beat = Msg::HEARTBEAT(dialect::HEARTBEAT_DATA {
        custom_mode: (4 << 16) | (4 << 24),
        mavtype: dialect::MavType::MAV_TYPE_HEXAROTOR,
        autopilot: dialect::MavAutopilot::MAV_AUTOPILOT_PX4,
        base_mode: dialect::MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED
            | dialect::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED,
        system_status: dialect::MavState::MAV_STATE_ACTIVE,
        mavlink_version: 3,
    });
    let later = now + Duration::from_secs(2);
    assert!(v.apply(&beat, later));
    assert!(v.armed);
    assert_eq!(v.mode, "Mission");
    assert_eq!(v.vehicle_type_name(), "Hexarotor");
    assert!(v.is_fresh(later + Duration::from_secs(4), Duration::from_secs(5)));
    assert!(!v.is_fresh(later + Duration::from_secs(5), Duration::from_secs(5)));
}

#[test]
fn unknown_values_stay_unknown() {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    v.apply(
        &Msg::SYS_STATUS(dialect::SYS_STATUS_DATA {
            voltage_battery: u16::MAX,
            battery_remaining: -1,
            ..Default::default()
        }),
        now,
    );
    v.apply(
        &Msg::GPS_RAW_INT(dialect::GPS_RAW_INT_DATA {
            eph: u16::MAX,
            satellites_visible: u8::MAX,
            ..Default::default()
        }),
        now,
    );
    assert_eq!((v.battery_voltage, v.battery_remaining), (None, None));
    assert_eq!((v.eph, v.satellites), (None, None));
    assert!(!v.apply(&Msg::PARAM_REQUEST_LIST(Default::default()), now));
}

#[test]
fn status_ring_is_bounded() {
    let mut v = VehicleState::new(1, 1, Instant::now());
    for i in 0..(STATUS_RING + 5) {
        v.push_status(StatusText {
            severity: 6,
            text: i.to_string(),
            at: SystemTime::now(),
        });
    }
    assert_eq!(v.status.len(), STATUS_RING);
    assert_eq!(v.status.front().map(|s| s.text.as_str()), Some("5"));
}

fn chunk(id: u16, seq: u8, text: &str) -> dialect::STATUSTEXT_DATA {
    dialect::STATUSTEXT_DATA {
        severity: dialect::MavSeverity::MAV_SEVERITY_WARNING,
        text: text.into(),
        id,
        chunk_seq: seq,
    }
}

#[test]
fn reassembles_chunked_status_text() {
    let mut a = StatusAssembler::default();
    let at = SystemTime::now();
    let first = "x".repeat(50);
    assert!(a.push(&chunk(7, 0, &first), at).is_empty());
    let done = a.push(&chunk(7, 1, "tail"), at);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].text, format!("{first}tail"));
    assert_eq!(done[0].severity_name(), "Warning");
    let single = a.push(&chunk(0, 0, "plain"), at);
    assert_eq!(single[0].text, "plain");
}

#[test]
fn unterminated_chunk_runs_stay_bounded() {
    let mut a = StatusAssembler::default();
    let at = SystemTime::now();
    let full = "y".repeat(50);
    let mut emitted = Vec::new();
    for seq in 0..2000u32 {
        emitted.extend(a.push(&chunk(3, seq as u8, &full), at));
    }
    assert!(!emitted.is_empty(), "long runs are cut into entries");
    assert!(emitted.iter().all(|e| e.text.len() <= MAX_STATUS_TEXT + 50));
}

#[test]
fn home_position_is_kept_in_degrees_and_metres() {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    assert!(v.home.is_none());
    assert!(v.apply(
        &Msg::HOME_POSITION(dialect::HOME_POSITION_DATA {
            latitude: 473_977_420,
            longitude: 85_455_940,
            altitude: 488_000,
            ..Default::default()
        }),
        now,
    ));
    let (lat, lon, alt) = v.home.unwrap();
    assert!((lat - 47.397_742).abs() < 1e-6 && (lon - 8.545_594).abs() < 1e-6);
    assert!((alt - 488.0).abs() < 1e-3);
}
