//! The vehicle model (schema 3 and later): migration from schema 2, worlds without an adapter, and the
//! consumers a vehicle can feed.
use super::model::{World, starter};
use super::*;

/// A schema 2 file with one system of each source.
const V2: &[u8] = br#"{"schema":2,"active":"gz","systems":[
    {"id":"sih","name":"SIH","template":"local-simulation","prepared":true,"source":{"kind":"sih"},
     "consumers":{"qgc":true,"live_view":true,"scripts_udp":"127.0.0.1:14540"}},
    {"id":"gz","name":"Gazebo","source":{"kind":"gazebo","world":"baylands","model":"x500_depth"},
     "consumers":{"qgc":true,"ros":{"agent":true,"recorder":true,"domain":3}}},
    {"id":"ap","name":"ArduPilot","source":{"kind":"ardupilot","vehicle":"plane"}},
    {"id":"rig","name":"Rig","source":{"kind":"physical","endpoint":"serial:COM3:57600"}},
    {"id":"ai","name":"AI","source":{"kind":"replay","recording":"flight-1"}}]}"#;

#[test]
fn schema_2_sources_become_vehicles_with_the_same_launch() {
    let (f, migrated) = file::parse(V2).unwrap().unwrap();
    assert!(migrated, "written back as schema 4");
    assert_eq!(f.schema, file::SCHEMA);
    assert_eq!(f.active, "gz");
    let vehicles: Vec<_> = f.systems.iter().map(|s| s.vehicle.clone()).collect();
    assert_eq!(
        vehicles,
        [
            Vehicle::sih(),
            Vehicle::gazebo("baylands", "x500_depth"),
            Vehicle::ardupilot("plane"),
            Vehicle::physical("serial:COM3:57600"),
            Vehicle::replay(Some("flight-1")),
        ]
    );
    let stages: Vec<Vec<&str>> = f.systems.iter().map(session_stages).collect();
    assert_eq!(stages[0], ["sih", "router", "link", "qgc"]);
    assert_eq!(
        stages[1],
        ["gazebo", "router", "link", "dds", "rosgz", "qgc"]
    );
    assert_eq!(stages[2], ["ardupilot", "link"]);
    assert!(stages[3].is_empty() && stages[4].is_empty());
    assert!(
        f.systems[0].prepared && f.systems[0].id == "sih",
        "an old template key is ignored"
    );
    assert_eq!(f.systems[1].consumers.ros.domain, 3);
    assert_eq!(
        f.systems[1].summary(),
        "PX4 + Gazebo / baylands / x500_depth"
    );
    assert_eq!(f.systems[2].summary(), "ArduPilot SITL / plane");

    // Written back, it reads as schema 3 without another migration.
    let json = serde_json::to_vec(&f).unwrap();
    let (back, again) = file::parse(&json).unwrap().unwrap();
    assert!(!again);
    assert_eq!(back, f);
}

#[test]
fn a_schema_2_file_with_an_unknown_source_is_damaged() {
    let bad =
        br#"{"schema":2,"active":"x","systems":[{"id":"x","name":"X","source":{"kind":"warp"}}]}"#;
    assert!(file::parse(bad).is_err());
}

#[test]
fn a_world_without_an_adapter_is_kept_and_refuses_to_start() {
    let json = br#"{"schema":3,"active":"x","systems":[{"id":"x","name":"Future",
        "vehicle":{"kind":"simulated","autopilot":{"kind":"px4"},"world":{"id":"AirSim","params":{"map":"city","empty":" "}}}}]}"#;
    let (f, _) = file::parse(json).unwrap().unwrap();
    let sys = &f.systems[0];
    let Vehicle::Simulated { world, .. } = &sys.vehicle else {
        panic!("still simulated");
    };
    assert_eq!(world.id, "airsim", "normalized, not rewritten");
    assert_eq!(world.params.len(), 1, "empty details dropped");
    assert!(sys.adapter().is_none() && !sys.simulated());
    assert!(session_stages(sys).is_empty());
    assert!(sys.summary().contains("no adapter"), "{}", sys.summary());

    // Saving the editor unchanged keeps it; naming a known world replaces it.
    let mut d = Draft::of(sys);
    assert_eq!(d.apply(sys).unwrap().vehicle, sys.vehicle);
    d.world = "internal".into();
    assert_eq!(d.apply(sys).unwrap().vehicle, Vehicle::sih());

    let mut s = State::new();
    s.systems.list.push(sys.clone());
    s.select_system("x");
    start_workspace(&mut s);
    assert!(s.session.is_none(), "nothing starts");
    assert!(s.notice.contains("no adapter for"), "{}", s.notice);
}

#[test]
fn ardupilot_cannot_pick_gazebo_and_says_why() {
    let base = starter(SIMULATION, "lab", "Lab");
    let mut d = Draft::of(&base);
    d.autopilot = adapter::AutopilotKind::ArduPilot;
    d.world = "gazebo".into();
    let e = d.apply(&base).unwrap_err();
    assert_eq!(e, "PX4 only in this version.");
    d.world = "internal".into();
    assert_eq!(
        d.apply(&base).unwrap().vehicle,
        Vehicle::ardupilot("copter")
    );
    // ArduPilot SITL has a ROS bridge entry, so the toggle stays offered.
    assert!(d.feeds(compat::Consumer::RosAgent));
}

#[test]
fn consumers_follow_what_the_vehicle_can_feed() {
    let mut sys = starter(SIMULATION, "x", "X");
    sys.consumers.ros.agent = true;
    sys.vehicle = Vehicle::Simulated {
        autopilot: model::Autopilot::Px4,
        world: World::new("unknown", &[]),
    };
    compat::prune(&mut sys);
    assert!(!sys.consumers.ros.agent, "no adapter, no ROS bridge");
    assert!(sys.consumers.live_view && sys.consumers.scripts_udp.is_some());
    sys.vehicle = Vehicle::replay(None);
    compat::prune(&mut sys);
    assert!(!sys.consumers.live_view && sys.consumers.scripts_udp.is_none());
    assert!(sys.consumers.qgc, "QGroundControl is never pruned");
}

#[test]
fn only_a_simulator_drone_deck_owns_may_be_commanded() {
    for kind in [SIMULATION, ROS] {
        assert!(compat::may_command(&starter(kind, "s", "S")), "{kind}");
    }
    for kind in [AI, PHYSICAL] {
        assert!(!compat::may_command(&starter(kind, "x", "X")), "{kind}");
    }
    let mut unknown = starter(SIMULATION, "u", "U");
    unknown.vehicle = Vehicle::Simulated {
        autopilot: model::Autopilot::Px4,
        world: World::new("future-sim", &[]),
    };
    assert!(
        !compat::may_command(&unknown),
        "a world without an adapter is not a simulator Drone Deck starts"
    );
}

#[test]
fn a_physical_vehicle_keeps_the_flight_stack_the_user_names() {
    let base = starter(PHYSICAL, "rig", "Rig");
    let mut d = Draft::of(&base);
    assert_eq!(d.hint, None);
    d.endpoint = rune::TextEdit::new("serial:COM3:57600");
    d.hint = Some(adapter::AutopilotKind::ArduPilot);
    let sys = d.apply(&base).unwrap();
    assert_eq!(sys.autopilot(), Some(adapter::AutopilotKind::ArduPilot));
    assert_eq!(
        sys.summary(),
        "Physical vehicle (ArduPilot) / serial:COM3:57600"
    );
    assert!(!compat::may_command(&sys), "a hint never grants commands");
    assert_eq!(
        Draft::of(&sys).hint,
        Some(adapter::AutopilotKind::ArduPilot)
    );
}
