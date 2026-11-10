use super::*;

const CUSTOM: u8 = 1;

#[test]
fn decodes_px4_modes() {
    let px4 =
        |main: u32, sub: u32| mode_name(Autopilot::Px4, 2, CUSTOM, (main << 16) | (sub << 24));
    assert_eq!(px4(3, 0), "Position");
    assert_eq!(px4(4, 3), "Hold");
    assert_eq!(px4(4, 4), "Mission");
    assert_eq!(px4(4, 5), "Return");
    assert_eq!(px4(6, 0), "Offboard");
    assert_eq!(px4(7, 0), "Stabilized");
    assert_eq!(px4(4, 99), "Mode 4.99");
}

#[test]
fn decodes_arducopter_modes() {
    assert_eq!(mode_name(Autopilot::ArduPilot, 2, CUSTOM, 4), "Guided");
    assert_eq!(
        mode_name(Autopilot::ArduPilot, 13, CUSTOM, 16),
        "Position Hold"
    );
    assert_eq!(mode_name(Autopilot::ArduPilot, 2, CUSTOM, 6), "RTL");
    // ArduPlane numbers differ; they are not guessed.
    assert_eq!(mode_name(Autopilot::ArduPilot, 1, CUSTOM, 6), "Mode 6");
}

#[test]
fn falls_back_without_custom_mode() {
    assert_eq!(mode_name(Autopilot::Px4, 2, 4, 0), "Auto");
    assert_eq!(mode_name(Autopilot::Px4, 2, 0, 0), "Unknown");
}

#[test]
fn encodes_modes_for_set_mode() {
    assert_eq!(
        encode_mode(Autopilot::Px4, 2, "hold"),
        Some([1.0, 4.0, 3.0])
    );
    assert_eq!(
        encode_mode(Autopilot::Px4, 2, "Position"),
        Some([1.0, 3.0, 0.0])
    );
    assert_eq!(
        encode_mode(Autopilot::ArduPilot, 2, "Loiter"),
        Some([1.0, 5.0, 0.0])
    );
    assert_eq!(encode_mode(Autopilot::ArduPilot, 1, "Loiter"), None);
    assert_eq!(encode_mode(Autopilot::Px4, 2, "Nope"), None);
    for name in available(Autopilot::Px4, 2) {
        let [_, main, sub] = encode_mode(Autopilot::Px4, 2, name).expect("encodable");
        let custom = ((main as u32) << 16) | ((sub as u32) << 24);
        assert_eq!(mode_name(Autopilot::Px4, 2, CUSTOM, custom), name);
    }
}

#[test]
fn termination_is_never_offered_or_encoded() {
    assert!(!available(Autopilot::Px4, 2).contains(&"Termination"));
    assert!(!available(Autopilot::Px4, 2).contains(&"Ready"));
    assert_eq!(encode_mode(Autopilot::Px4, 2, "termination"), None);
    // Still decoded when the vehicle reports it.
    assert_eq!(
        mode_name(Autopilot::Px4, 2, CUSTOM, 10 << 16),
        "Termination"
    );
}

#[test]
fn peripherals_are_not_vehicles() {
    assert!(is_vehicle_heartbeat(12, 2), "PX4 quadrotor");
    assert!(is_vehicle_heartbeat(3, 1), "ArduPlane");
    assert!(!is_vehicle_heartbeat(8, 2), "no autopilot");
    assert!(!is_vehicle_heartbeat(12, 6), "ground station");
    assert!(
        !is_vehicle_heartbeat(12, 30),
        "camera announced with the PX4 id"
    );
    assert!(!is_vehicle_heartbeat(12, 26), "gimbal");
    assert!(!is_vehicle_heartbeat(12, 18), "companion computer");
}
