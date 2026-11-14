use super::*;
use crate::systems::{self, model::template};

#[test]
fn every_stage_and_ros_profile_is_a_built_in() {
    let defaults = crate::backend::defaults();
    let known = |id: &str| defaults.profiles.iter().any(|p| p.id == id);
    for a in &ALL {
        for st in a.stages {
            assert!(known(st.profile), "{}: {}", a.id, st.profile);
        }
        if let Some(ros) = &a.ros {
            assert!(known(ros.agent), "{}: {}", a.id, ros.agent);
            for id in ros.extra {
                assert!(known(id), "{}: {id}", a.id);
            }
        }
    }
}

#[test]
fn simulators_are_the_stage_profiles_and_are_exclusive() {
    assert_eq!(
        simulator_profiles().collect::<Vec<_>>(),
        ["sih", "gazebo", "ardupilot"]
    );
    for p in crate::backend::defaults().profiles {
        assert_eq!(p.exclusive, is_simulator(&p.id), "{}", p.id);
    }
}

#[test]
fn pairs_name_their_route_bridge_and_shell() {
    let gazebo = find(AutopilotKind::Px4, "gazebo").unwrap();
    assert_eq!(gazebo.link, Link::Router);
    assert_eq!(gazebo.ros.as_ref().unwrap().extra, ["rosgz"]);
    let ardupilot = find(AutopilotKind::ArduPilot, "internal").unwrap();
    assert_eq!(ardupilot.link, Link::Direct);
    assert_eq!(ardupilot.ros.as_ref().unwrap().topic_prefix, "/ap/");
    assert!(!ardupilot.shell);
    assert!(find(AutopilotKind::ArduPilot, "gazebo").is_none());
    assert_eq!(by_profile("rosgz").map(|a| a.id), None);
}

#[test]
fn systems_find_their_adapter_from_the_source() {
    let sim = template(systems::SIMULATION, "s", "S");
    assert_eq!(sim.adapter().map(|a| a.id), Some("px4-sih"));
    assert_eq!(sim.autopilot(), Some(AutopilotKind::Px4));
    let ros = template(systems::ROS, "r", "R");
    assert_eq!(ros.adapter().map(|a| a.id), Some("px4-gazebo"));
    for kind in [systems::AI, systems::PHYSICAL] {
        let sys = template(kind, "x", "X");
        assert!(sys.adapter().is_none() && !sys.simulated());
    }
}

#[test]
fn readiness_needs_vehicle_status_only_where_the_stage_reports_it() {
    assert_eq!(
        readiness_of("sih").agent_kind().as_deref(),
        Some("px4-status:sihsim")
    );
    assert_eq!(readiness_of("gazebo").agent_kind(), None);
    assert_eq!(readiness_of("qgc"), Readiness::ProcessRunning);
    let mut s = crate::State::new();
    for id in ["sih", "gazebo", "qgc"] {
        s.status.insert(id.into(), "Running".into());
    }
    assert!(
        !ready(&s, "sih"),
        "a running SIH has not reported vehicle status"
    );
    assert!(ready(&s, "gazebo") && ready(&s, "qgc"));
    s.readiness.insert("sih".into(), "Ready".into());
    assert!(ready(&s, "sih"));
    s.status.insert("sih".into(), "Exited 1".into());
    assert!(
        !ready(&s, "sih"),
        "an exited SIH is not ready, whatever it reported"
    );
}

#[test]
fn launch_env_gives_defaults_for_missing_details() {
    let ros = template(systems::ROS, "r", "R");
    let env = launch_env(&ros, "rosgz");
    assert!(env.contains(&("DRONE_DECK_GZ_MODEL".into(), "x500_mono_cam".into())));
    assert!(
        launch_env(&ros, "dds").is_empty(),
        "not a stage or extra of Gazebo"
    );
    let sim = template(systems::SIMULATION, "s", "S");
    assert!(launch_env(&sim, "sih").is_empty());
}
