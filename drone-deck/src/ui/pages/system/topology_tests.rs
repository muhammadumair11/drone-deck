use super::*;
use crate::recordings::Topic;

fn topic(name: &str) -> Topic {
    Topic {
        name: name.into(),
        ..Default::default()
    }
}

/// Running processes are not traffic: the simulator → DDS wire needs the vehicle's own topics,
/// and the camera bridge needs camera topics.
#[test]
fn simulator_wires_need_discovered_topics_not_running_processes() {
    let mut s = State::new();
    assert!(s.select_system(systems::ROS));
    for id in ["gazebo", "dds", "rosgz"] {
        s.status.insert(id.into(), "Running".into());
    }
    assert!(!observed(&s, "gazebo", "dds"), "both run, nothing seen");
    assert!(!observed(&s, "rosgz", "ros2"));
    s.recorder.discovered = vec![topic("/ap/status")];
    assert!(
        !observed(&s, "gazebo", "dds"),
        "ArduPilot topics are not PX4's"
    );
    s.recorder.discovered = vec![topic("/fmu/out/vehicle_status"), topic("/camera/image_raw")];
    assert!(observed(&s, "gazebo", "dds"));
    assert!(observed(&s, "rosgz", "ros2"));
    s.status.insert("rosgz".into(), "Stopped".into());
    assert!(
        !observed(&s, "rosgz", "ros2"),
        "a stale topic list is not the bridge"
    );
    s.status.insert("gazebo".into(), "Stopped".into());
    assert!(!observed(&s, "gazebo", "dds"));
}

/// A physical system says the link is read-only only while no QGroundControl or scripts
/// consumer is bridged through it.
#[test]
fn the_physical_caveat_names_bridged_consumers() {
    let mut s = State::new();
    assert!(s.select_system(systems::PHYSICAL));
    assert!(
        caveat(&s).ends_with("The link is read-only."),
        "{}",
        caveat(&s)
    );
    let id = s.systems.active_id().to_string();
    s.systems.get_mut(&id).unwrap().consumers.qgc = true;
    let text = caveat(&s);
    assert!(!text.contains("read-only"), "{text}");
    assert!(text.contains("can command the vehicle"), "{text}");
}
