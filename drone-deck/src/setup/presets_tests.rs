use super::*;
use crate::systems::{self, Vehicle, model::template};

#[test]
fn combinations_outside_the_earlier_list_get_a_preset_and_a_name() {
    let mut gz = template(systems::SIMULATION, "g", "G");
    gz.vehicle = Vehicle::gazebo("default", "x500");
    let p = for_system(&gz);
    assert_eq!(p.recipes, ["px4", "gazebo", "qgc"]);
    assert_eq!(p.name, "PX4 + Gazebo with QGroundControl");

    gz.consumers.inference = Some(Default::default());
    gz.consumers.ros.agent = true;
    let p = for_system(&gz);
    assert_eq!(
        p.recipes,
        [
            "px4",
            "gazebo",
            "ros2",
            "ros-gz",
            "dds",
            "ros-workspace",
            "onnx",
            "qgc"
        ]
    );
    assert_eq!(
        p.name,
        "PX4 + Gazebo with ROS 2, inference and QGroundControl"
    );
}

#[test]
fn ardupilot_gets_its_own_ros_stack_and_a_replay_with_qgc_is_not_physical() {
    let mut ap = template(systems::ROS, "a", "A");
    ap.vehicle = Vehicle::ardupilot("plane");
    let p = for_system(&ap);
    assert_eq!(
        p.recipes,
        ["ardupilot", "ros2", "dds", "ardupilot-dds", "qgc"],
        "ArduPilot's agent and DDS build, not px4_msgs"
    );
    assert_eq!(p.name, "ArduPilot SITL with ROS 2 and QGroundControl");
    ap.consumers.ros.agent = false;
    assert_eq!(for_system(&ap).recipes, ["ardupilot", "qgc"]);
    let mut replay = template(systems::AI, "r", "R");
    replay.consumers.inference = None;
    replay.consumers.qgc = true;
    let p = for_system(&replay);
    assert_eq!(p.recipes, ["qgc"]);
    assert_eq!(p.name, "Recording replay with QGroundControl");
}

#[test]
fn a_checkpoint_recipe_list_wins_over_its_name() {
    let p = resolve("Anything", Some(vec!["onnx".into()])).unwrap();
    assert_eq!(
        p.name, "Offline CPU inference",
        "known lists keep their name"
    );
    let p = resolve("My mix", Some(vec!["px4".into(), "onnx".into()])).unwrap();
    assert_eq!((p.name.as_str(), p.recipes.len()), ("My mix", 2));
    assert!(resolve("My mix", None).is_none());
}
