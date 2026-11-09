use super::*;
#[test]
fn vehicle_identity_requires_an_autopilot_heartbeat_and_stays_separate_from_simulation() {
    let mut s = State::new();
    event(
        &mut s,
        json!({"kind":"packet","system":1,"component":1,"message_id":0,"fields":{"type":2,"autopilot":12,"armed":false}}),
    );
    assert_eq!(s.hardware.vehicles.len(), 1);
    assert!(s.metrics.is_empty());
    event(
        &mut s,
        json!({"kind":"packet","system":1,"component":1,"message_id":33,"fields":{"latitude":12.5}}),
    );
    event(
        &mut s,
        json!({"kind":"packet","system":1,"component":1,"message_id":0,"fields":{"type":2,"autopilot":12,"armed":true}}),
    );
    assert_eq!(s.hardware.vehicles["1:1"]["fields"]["latitude"], 12.5);
    assert_eq!(s.hardware.vehicles["1:1"]["fields"]["armed"], true);
    event(
        &mut s,
        json!({"kind":"packet","system":255,"component":190,"message_id":0,"fields":{"type":6,"autopilot":8}}),
    );
    assert_eq!(s.hardware.vehicles.len(), 1);
    s.hardware.mode = 1;
    start(&mut s);
    assert!(s.backend.is_none());
    assert!(s.status.is_empty());
}
