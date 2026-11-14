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
fn simulators_are_the_stage_profiles_and_flight_stacks_are_exclusive() {
    assert_eq!(
        simulator_profiles().collect::<Vec<_>>(),
        [
            "sih",
            "gazebo",
            "ardupilot",
            "jmavsim",
            "px4-none",
            "xplane",
            "flightgear",
            "ardupilot-webots",
            "webots",
            "realflight",
            "ardupilot-realflight",
            "ardupilot-flightgear",
            "fgview"
        ]
    );
    // The backend runs one flight stack at a time; X-Plane joins its PX4 instead.
    for p in crate::backend::defaults().profiles {
        assert_eq!(p.exclusive, is_vehicle_profile(&p.id), "{}", p.id);
    }
    assert!(is_simulator("xplane") && !is_vehicle_profile("xplane"));
    assert!(same_adapter("px4-none", "xplane") && !same_adapter("sih", "xplane"));
}

#[test]
fn the_new_worlds_run_px4_through_the_router_with_their_recipes() {
    let jmavsim = find(AutopilotKind::Px4, "jmavsim").unwrap();
    assert_eq!(jmavsim.recipes, ["px4", "jmavsim"]);
    assert_eq!(jmavsim.link, Link::Router);
    assert_eq!(
        readiness_of("jmavsim").agent_kind().as_deref(),
        Some("px4-status")
    );
    let xplane = find(AutopilotKind::Px4, "xplane").unwrap();
    let stages: Vec<_> = xplane.stages.iter().map(|s| s.profile).collect();
    assert_eq!(stages, ["px4-none", "xplane"]);
    assert_eq!(xplane.vehicle_profile(), "px4-none", "PX4 is the vehicle");
    assert!(
        world_for(AutopilotKind::ArduPilot, "jmavsim")
            .unwrap_err()
            .contains("PX4 only")
    );
    let recipes = crate::setup::catalog::load();
    for a in &ALL {
        for id in a.recipes {
            assert!(recipes.iter().any(|r| r.id == *id), "{}: {id}", a.id);
        }
    }
    for (id, ..) in WORLDS {
        assert!(ALL.iter().any(|a| a.world == id), "no adapter offers {id}");
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

#[test]
fn each_world_offers_only_the_autopilots_that_fly_in_it() {
    use AutopilotKind::{ArduPilot, Px4};
    assert_eq!(
        world_for(Px4, "flightgear").map(|a| a.id),
        Ok("px4-flightgear")
    );
    assert_eq!(
        world_for(ArduPilot, "flightgear").map(|a| a.id),
        Ok("ardupilot-flightgear")
    );
    for world in ["webots", "realflight"] {
        assert!(world_for(ArduPilot, world).is_ok(), "{world}");
        assert!(
            world_for(Px4, world)
                .unwrap_err()
                .contains("ArduPilot only"),
            "{world}"
        );
    }
}

#[test]
fn ardupilot_in_flightgear_is_sitl_drawn_by_flightgear_with_the_ros_bridge() {
    let a = find(AutopilotKind::ArduPilot, "flightgear").unwrap();
    let stages: Vec<_> = a.stages.iter().map(|s| s.profile).collect();
    assert_eq!(stages, ["ardupilot-flightgear", "fgview"]);
    assert_eq!(a.vehicle_profile(), "ardupilot-flightgear");
    assert!(is_vehicle_profile("ardupilot-flightgear") && !is_vehicle_profile("fgview"));
    assert_eq!(a.link, Link::Direct);
    assert_eq!(a.recipes, ["ardupilot", "flightgear"]);
    let ros = a.ros.as_ref().unwrap();
    assert_eq!((ros.agent, ros.topic_prefix), ("dds-ap", "/ap/"));
    assert_eq!(ros.recipes, ["dds", "ardupilot-dds"]);
    assert!(
        a.home.is_some(),
        "re-simulation starts it at the recorded home"
    );

    let defaults = crate::backend::defaults();
    let args = |id: &str| {
        defaults
            .profiles
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .args
            .join(" ")
    };
    assert!(args("ardupilot-flightgear").contains(" --enable-fgview "));
    assert!(
        args("fgview").contains("fg_quad_view.sh") && args("fgview").contains("fg_plane_view.sh")
    );
    assert!(args("dds-ap").ends_with("udp4 -p 2019"));
    // Every ArduPilot SITL command takes what systems and re-simulations pass it.
    for id in ["ardupilot", "ardupilot-flightgear"] {
        let command = args(id);
        for name in [
            "DRONE_DECK_AP_DOMAIN",
            "DRONE_DECK_AP_HOME",
            "DRONE_DECK_AP_DIR",
            "DRONE_DECK_AP_WIPE",
        ] {
            assert!(command.contains(name), "{id}: {name}");
        }
    }
}

#[test]
fn home_reaches_px4_as_three_variables_and_ardupilot_as_one() {
    let px4 = find(AutopilotKind::Px4, "internal").unwrap();
    let env = px4.home.as_ref().unwrap().env("47.1", "8.5", "490");
    assert_eq!(
        env,
        [
            ("PX4_HOME_LAT".to_string(), "47.1".to_string()),
            ("PX4_HOME_LON".into(), "8.5".into()),
            ("PX4_HOME_ALT".into(), "490".into())
        ]
    );
    let ap = find(AutopilotKind::ArduPilot, "internal").unwrap();
    assert_eq!(
        ap.home.as_ref().unwrap().env("-35.3", "149.1", "584"),
        [(
            "DRONE_DECK_AP_HOME".to_string(),
            "-35.3,149.1,584,0".to_string()
        )]
    );
}

#[test]
fn realflight_starts_before_the_autopilot_that_drives_it() {
    let a = find(AutopilotKind::ArduPilot, "realflight").unwrap();
    let stages: Vec<_> = a.stages.iter().map(|s| s.profile).collect();
    assert_eq!(stages, ["realflight", "ardupilot-realflight"]);
    assert_eq!(
        a.vehicle_profile(),
        "ardupilot-realflight",
        "ArduPilot is the flight stack"
    );
    assert!(is_vehicle_profile("ardupilot-realflight") && !is_vehicle_profile("realflight"));
    assert_eq!(a.link, Link::Direct);

    let mut sys = template(systems::SIMULATION, "rf", "RF");
    sys.vehicle = crate::systems::Vehicle::Simulated {
        autopilot: crate::systems::model::Autopilot::ArduPilot {
            vehicle: "plane".into(),
        },
        world: crate::systems::model::World::new("realflight", &[("host", "172.20.0.1")]),
    };
    let env = launch_env(&sys, "ardupilot-realflight");
    assert!(env.contains(&("DRONE_DECK_FLIGHTAXIS_HOST".into(), "172.20.0.1".into())));
    assert!(env.contains(&("DRONE_DECK_AP_VEHICLE".into(), "ArduPlane".into())));
    assert_eq!(
        crate::systems::session_stages(&sys),
        ["realflight", "ardupilot-realflight", "link", "qgc"]
    );
}
