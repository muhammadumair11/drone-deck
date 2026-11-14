use super::*;
use crate::systems::model::{AI, PHYSICAL, ROS, SIMULATION, starter};
use Panel::*;

#[test]
fn each_system_gets_exactly_the_panels_of_its_pieces() {
    let sim = starter(SIMULATION, "", "");
    assert_eq!(
        panels(&sim),
        [
            Connections,
            Vehicle,
            LiveView,
            GroundStation,
            Scripts,
            Processes
        ]
    );
    let gazebo = starter(ROS, "", "");
    assert_eq!(
        panels(&gazebo),
        [
            Connections,
            Vehicle,
            GroundStation,
            Ros,
            Recorder,
            Foxglove,
            Processes
        ]
    );
    let replay = starter(AI, "", "");
    assert_eq!(
        panels(&replay),
        [Connections, Vehicle, Foxglove, Inference, Processes]
    );
    let mut real = starter(PHYSICAL, "", "");
    real.consumers.qgc = true;
    real.consumers.scripts_udp = Some("127.0.0.1:14540".into());
    assert_eq!(
        panels(&real),
        [
            Connections,
            Vehicle,
            LiveView,
            GroundStation,
            Scripts,
            Processes
        ]
    );
    assert_eq!(
        unused(&real),
        ["ROS 2", "Recorder", "Foxglove", "inference"]
    );
}

#[test]
fn turning_a_listener_off_removes_its_panel() {
    let mut sim = starter(SIMULATION, "", "");
    sim.consumers.live_view = false;
    sim.consumers.qgc = false;
    assert!(
        !panels(&sim).contains(&LiveView),
        "live view off: no live panel"
    );
    assert!(!panels(&sim).contains(&GroundStation));
    assert_eq!(unused(&sim)[..2], ["live view", "QGroundControl"]);
    sim.consumers.ros.recorder = true;
    assert!(panels(&sim).contains(&Ros) && panels(&sim).contains(&Recorder));
    let mut replay = starter(AI, "", "");
    replay.consumers.qgc = true;
    replay.consumers.live_view = true;
    assert!(
        !panels(&replay).contains(&GroundStation) && !panels(&replay).contains(&LiveView),
        "a recording has no live vehicle"
    );
}

fn keys_on_screen(combo: &str) -> Vec<&'static str> {
    let mut s = State::new();
    scene(&mut s, combo);
    let mut rt = rune::Runtime::new(
        s,
        crate::view,
        (1440., 6000.),
        rune::Env {
            scale: 1.,
            system_dark: true,
        },
    );
    rt.frame(0.1);
    [
        "panel-connections",
        "panel-vehicle",
        "panel-qgc",
        "panel-scripts",
        "panel-ros",
        "panel-recorder",
        "panel-foxglove",
        "panel-inference",
        "branch-scripts-link",
        "vehicle-actions",
    ]
    .into_iter()
    .filter(|k| rt.find(k).is_some())
    .collect()
}

#[test]
fn the_page_draws_a_panel_exactly_when_the_system_has_the_piece() {
    assert_eq!(
        keys_on_screen("jmavsim"),
        [
            "panel-connections",
            "panel-vehicle",
            "panel-ros",
            "panel-recorder",
            "vehicle-actions"
        ]
    );
    assert_eq!(
        keys_on_screen("real"),
        [
            "panel-connections",
            "panel-vehicle",
            "panel-qgc",
            "panel-scripts",
            "branch-scripts-link"
        ],
        "a real drone: both bridges shown, never flight controls"
    );
    assert_eq!(
        keys_on_screen("recording"),
        [
            "panel-connections",
            "panel-vehicle",
            "panel-foxglove",
            "panel-inference"
        ]
    );
}

#[test]
fn the_scripts_branch_stays_when_ros_is_on() {
    let mut s = State::new();
    scene(&mut s, "gazebo");
    let id = s.systems.active_id().to_string();
    s.systems.get_mut(&id).unwrap().consumers.scripts_udp = Some("127.0.0.1:14540".into());
    let mut rt = rune::Runtime::new(
        s,
        crate::view,
        (1440., 6000.),
        rune::Env {
            scale: 1.,
            system_dark: true,
        },
    );
    rt.frame(0.1);
    assert!(rt.find("branch-scripts-link").is_some());
    assert!(rt.find("panel-scripts").is_some() && rt.find("panel-ros").is_some());
}
