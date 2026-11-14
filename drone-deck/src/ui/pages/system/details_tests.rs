use super::*;
use crate::systems::{self, Vehicle, model::starter};

fn values(sys: &systems::System) -> Vec<(&'static str, String)> {
    lines(sys, 5760)
        .into_iter()
        .map(|l| (l.key, l.value))
        .collect()
}

#[test]
fn a_simulation_lists_its_link_scripts_and_ground_station() {
    let sim = starter(systems::SIMULATION, "s", "S");
    assert_eq!(
        values(&sim),
        [
            ("link", "tcp:127.0.0.1:5760".to_string()),
            ("mavsdk", "udpin://127.0.0.1:14540".to_string()),
            ("pymavlink", "udpin:127.0.0.1:14540".to_string()),
            ("qgc", "udp:127.0.0.1:14550".to_string()),
        ]
    );
}

#[test]
fn ros_and_foxglove_lines_follow_the_consumers() {
    let mut ros = starter(systems::ROS, "r", "R");
    ros.consumers.ros.domain = 7;
    let v = values(&ros);
    assert!(v.contains(&("ros", "export ROS_DOMAIN_ID=7".to_string())));
    assert!(v.contains(&("foxglove", "ws://localhost:8765".to_string())));
    assert!(
        !v.iter().any(|(k, _)| *k == "mavsdk"),
        "no scripts consumer"
    );
}

#[test]
fn a_physical_vehicle_shows_its_endpoint_and_a_replay_no_vehicle_lines() {
    let mut rig = starter(systems::PHYSICAL, "p", "P");
    assert!(values(&rig).is_empty(), "no endpoint yet, nothing to copy");
    rig.vehicle = Vehicle::physical("serial:COM7:57600");
    assert_eq!(values(&rig), [("link", "serial:COM7:57600".to_string())]);
    let mut replay = starter(systems::AI, "a", "A");
    replay.consumers.qgc = true;
    replay.consumers.scripts_udp = Some("127.0.0.1:14540".into());
    assert_eq!(
        values(&replay),
        [("foxglove", "ws://localhost:8765".to_string())],
        "a recording has no live vehicle to reach"
    );
}
