use super::model::{self, Consumers, Inference, Ros, STARTER_IDS, starter};
use super::*;

fn set(d: &mut Draft, key: &str, value: &str) {
    *d.params.get_mut(key).unwrap() = rune::TextEdit::new(value);
}

fn ids(sys: &System) -> Vec<&'static str> {
    components(sys).iter().map(|c| c.id).collect()
}
fn chain_ids(sys: &System) -> Vec<&'static str> {
    chain(sys).map(|c| c.id).collect()
}

#[test]
fn schema_1_migrates_to_four_systems_keeping_names_prepared_flags_and_active() {
    let v1 = br#"{"active":3,"records":[{"name":"Lab","prepared":true},{"name":"","prepared":true},{"name":"Models","prepared":false},{"name":"Bench quad","prepared":true}]}"#;
    let (f, migrated) = file::parse(v1).unwrap().unwrap();
    assert!(migrated);
    assert_eq!(f.schema, file::SCHEMA);
    assert_eq!(f.systems.len(), 4);
    let names: Vec<_> = f.systems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["Lab", "ROS & sensor capture", "Models", "Bench quad"]
    );
    let prepared: Vec<_> = f.systems.iter().map(|s| s.prepared).collect();
    assert_eq!(prepared, [true, true, false, true]);
    assert_eq!(f.active, "physical-vehicle");
    assert!(f.systems[PHYSICAL].is_physical() && f.systems[AI].is_replay());
    // The 0.8 ROS system ran SIH with the DDS Agent; a migrated one keeps working that way.
    assert_eq!(f.systems[ROS].vehicle, Vehicle::sih());
    assert_eq!(
        session_stages(&f.systems[ROS]),
        ["sih", "router", "link", "dds", "foxglove", "qgc"]
    );
    for (sys, id) in f.systems.iter().zip(STARTER_IDS) {
        assert_eq!(sys.id, id);
    }
}

#[test]
fn schema_1_without_records_and_an_out_of_range_index_still_migrates() {
    let (f, migrated) = file::parse(br#"{"active":9}"#).unwrap().unwrap();
    assert!(migrated);
    assert_eq!(f.active, "physical-vehicle");
    assert!(f.systems.iter().all(|s| !s.prepared));
    assert_eq!(f.systems[0].name, "Local simulation");
}

#[test]
fn schema_4_round_trips_and_newer_schemas_are_refused() {
    let mut systems = Systems::starters();
    let id = systems.add_starter(ROS, "Camera lab");
    systems.set_active(&id);
    let json = serde_json::to_vec(&systems.to_file()).unwrap();
    let text = String::from_utf8(json.clone()).unwrap();
    assert!(text.contains(r#""schema":4"#), "{text}");
    assert!(text.contains(r#""kind":"simulated""#), "{text}");
    assert!(text.contains(r#""id":"gazebo""#), "{text}");
    assert!(text.contains(r#""model":"x500_mono_cam""#), "{text}");
    let (back, migrated) = file::parse(&json).unwrap().unwrap();
    assert!(!migrated);
    assert_eq!(back, systems.to_file());
    let restored = Systems::from_file(back);
    assert_eq!(restored.active_id(), "camera-lab");

    let err = file::parse(br#"{"schema":5,"active":"x","systems":[]}"#)
        .expect("readable, not damaged")
        .unwrap_err();
    assert!(err.contains("newer Drone Deck"), "{err}");
    assert!(file::parse(b"{not json").is_err());
}

#[test]
fn a_newer_systems_file_is_kept_in_place_and_never_overwritten() {
    let dir = std::env::temp_dir().join(format!("drone-deck-systems-newer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("systems.json");
    let body = br#"{"schema":5,"active":"x","systems":[{"id":"x"}]}"#;
    std::fs::write(&path, body).unwrap();
    let systems = Systems::load_at(&path);
    assert!(systems.is_empty(), "nothing from it is used");
    let err = systems.save_at(&path).unwrap_err();
    assert!(err.contains("newer Drone Deck"), "{err}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        body,
        "not quarantined or replaced"
    );
    assert_eq!(std::fs::read(dir.join("systems.json.kept")).unwrap(), body);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["systems.json", "systems.json.kept"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_hand_edited_file_is_repaired() {
    let json = br#"{"schema":2,"active":"gone","systems":[
        {"id":"Bad Id!","name":"Rig","source":{"kind":"sih"}},
        {"id":"rig","name":"Rig two","source":{"kind":"physical","endpoint":"serial:COM3:57600"}},
        {"id":"rig","name":"Rig three","source":{"kind":"replay"}}]}"#;
    let (f, _) = file::parse(json).unwrap().unwrap();
    let ids: Vec<_> = f.systems.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["rig", "rig-2", "rig-3"]);
    assert_eq!(f.active, "rig");
    assert_eq!(f.systems[0].consumers, Consumers::default());
    assert_eq!(f.systems[2].vehicle, Vehicle::replay(None));
    let (empty, _) = file::parse(br#"{"schema":2,"active":"","systems":[]}"#)
        .unwrap()
        .unwrap();
    assert!(
        empty.systems.is_empty() && empty.active.is_empty(),
        "an empty list stays empty: the wizard is offered"
    );
}

#[test]
fn templates_reproduce_the_four_systems() {
    let sim = starter(SIMULATION, "a", "A");
    assert_eq!(sim.vehicle, Vehicle::sih());
    assert!(sim.consumers.qgc && sim.consumers.live_view);
    assert_eq!(ids(&sim), ["sih", "router", "link", "qgc", "scripts"]);
    assert_eq!(chain_ids(&sim), ["sih", "router", "link", "qgc"]);
    assert_eq!(session_stages(&sim), ["sih", "router", "link", "qgc"]);
    assert_eq!(session_stages(&sim), ["sih", "router", "link", "qgc"]);
    assert_eq!(
        crate::setup::presets::for_system(&sim).name,
        "PX4 SIH simulation"
    );

    let ros = starter(ROS, "b", "B");
    assert_eq!(ros.vehicle, Vehicle::gazebo("default", "x500_mono_cam"));
    assert!(ros.consumers.ros.agent && ros.consumers.ros.recorder && ros.consumers.foxglove);
    assert_eq!(
        ids(&ros),
        [
            "gazebo", "router", "link", "dds", "ros2", "rosgz", "mcap", "qgc", "foxglove"
        ]
    );
    assert_eq!(chain_ids(&ros), ["gazebo", "dds", "ros2", "mcap"]);
    assert_eq!(
        session_stages(&ros),
        [
            "gazebo", "router", "link", "dds", "rosgz", "foxglove", "qgc"
        ]
    );
    assert_eq!(
        session_stages(&ros),
        [
            "gazebo", "router", "link", "dds", "rosgz", "foxglove", "qgc"
        ]
    );
    assert_eq!(
        crate::setup::presets::for_system(&ros).name,
        "PX4 + Gazebo with ROS 2 and recordings"
    );

    let ai = starter(AI, "c", "C");
    assert_eq!(ai.vehicle, Vehicle::replay(None));
    assert_eq!(ai.consumers.inference, Some(Inference::default()));
    assert_eq!(chain_ids(&ai), ["onnx", "project", "pipelines"]);
    assert!(session_stages(&ai).is_empty());
    assert_eq!(
        crate::setup::presets::for_system(&ai).name,
        "Offline CPU inference"
    );

    let physical = starter(PHYSICAL, "d", "D");
    assert!(!physical.consumers.qgc, "QGroundControl is optional");
    assert_eq!(chain_ids(&physical), ["endpoint", "link", "tlog"]);
    assert!(
        session_stages(&physical).is_empty(),
        "a physical link is no launch"
    );
    assert_eq!(
        crate::setup::presets::for_system(&physical).name,
        "Physical vehicle"
    );
}

#[test]
fn components_chain_and_stages_follow_vehicle_and_consumers() {
    let mut sys = starter(SIMULATION, "x", "X");
    sys.consumers.live_view = false;
    sys.consumers.scripts_udp = None;
    assert_eq!(chain_ids(&sys), ["sih", "router", "link", "qgc"]);
    sys.consumers.qgc = false;
    assert_eq!(session_stages(&sys), ["sih", "router", "link"]);
    sys.consumers.ros = Ros {
        agent: true,
        recorder: false,
        domain: 7,
        ..Default::default()
    };
    assert_eq!(chain_ids(&sys), ["sih", "dds", "ros2"]);
    assert_eq!(session_stages(&sys), ["sih", "router", "link", "dds"]);
    assert_eq!(
        crate::setup::presets::for_system(&sys).recipes,
        ["px4", "ros2", "dds", "ros-workspace"],
        "SIH with ROS needs no Gazebo, and QGroundControl is off here"
    );
    sys.consumers.live_view = true;
    assert_eq!(
        session_stages(&sys),
        ["sih", "router", "link", "dds"],
        "the live view needs no process"
    );

    sys.vehicle = Vehicle::ardupilot("plane");
    assert_eq!(vehicle_profile(&sys), Some("ardupilot"));
    assert_eq!(
        session_stages(&sys),
        ["ardupilot", "link", "dds-ap"],
        "ArduPilot SITL serves TCP 5760 itself (no router); its own DDS agent on UDP 2019"
    );
    assert_eq!(chain_ids(&sys), ["ardupilot", "dds-ap", "ros2"]);
    assert_eq!(
        crate::setup::presets::for_system(&sys).recipes,
        ["ardupilot", "ros2", "dds", "ardupilot-dds"],
        "no QGroundControl consumer here"
    );
    assert!(!ids(&sys).contains(&"router"));
    sys.consumers.ros.agent = false;
    sys.consumers.qgc = true;
    assert_eq!(chain_ids(&sys), ["ardupilot", "link", "qgc"]);
    assert_eq!(session_stages(&sys), ["ardupilot", "link", "qgc"]);

    sys.vehicle = Vehicle::physical("udp:0.0.0.0:14550");
    sys.consumers = Consumers {
        qgc: true,
        ..Default::default()
    };
    assert_eq!(chain_ids(&sys), ["endpoint", "link", "tlog"]);
    assert_eq!(ids(&sys), ["endpoint", "link", "tlog", "qgc"]);
    assert!(session_stages(&sys).is_empty());

    sys.vehicle = Vehicle::replay(None);
    sys.consumers.ros.recorder = true;
    assert_eq!(ids(&sys).iter().filter(|i| **i == "mcap").count(), 1);
    assert_eq!(chain_ids(&sys), ["mcap"]);
    // Chains only list stations that are components.
    for k in 0..4 {
        let t = starter(k, "t", "T");
        let comps = ids(&t);
        assert!(chain_ids(&t).iter().all(|c| comps.contains(c)));
    }
}

#[test]
fn create_rename_duplicate_and_delete() {
    let mut systems = Systems::starters();
    assert_eq!(systems.list.len(), 4);
    let a = systems.add_starter(SIMULATION, "  My Lab  ");
    assert_eq!(a, "my-lab");
    let b = systems.add_starter(SIMULATION, "My lab");
    assert_eq!(b, "my-lab-2", "ids stay unique");
    assert_eq!(systems.get(&a).unwrap().name, "My Lab");

    assert!(systems.rename(&a, "").is_err());
    assert!(systems.rename(&a, &"x".repeat(61)).is_err());
    systems.rename(&a, "Bench").unwrap();
    assert_eq!(systems.get(&a).unwrap().name, "Bench");
    assert_eq!(
        systems.get(&a).unwrap().id,
        "my-lab",
        "renaming keeps the id"
    );

    let copy = systems.duplicate(&a).unwrap();
    assert_eq!(systems.get(&copy).unwrap().name, "Bench copy");
    assert_eq!(systems.position(&copy), systems.position(&a).map(|i| i + 1));
    let again = systems.duplicate(&a).unwrap();
    assert_eq!(systems.get(&again).unwrap().name, "Bench copy 2");
    assert_eq!(systems.suggest_name("Bench copy 2"), "Bench copy 2 2");

    systems.set_active(&a);
    systems.remove(&a).unwrap();
    assert!(systems.get(&a).is_none());
    assert_eq!(
        systems.active_id(),
        systems.list[0].id,
        "deleting the active system activates the first one"
    );
    while systems.list.len() > 1 {
        let id = systems.list[0].id.clone();
        systems.remove(&id).unwrap();
    }
    let last = systems.list[0].id.clone();
    systems.remove(&last).unwrap();
    assert!(
        systems.is_empty(),
        "the last system can go: the wizard is offered"
    );
    assert_eq!(systems.current().label(), "No system yet");
    assert!(systems.current().is_replay() && session_stages(systems.current()).is_empty());
}

#[test]
fn a_fresh_library_is_empty_and_the_first_system_added_is_active() {
    let mut systems = Systems::default();
    assert!(systems.is_empty());
    assert_eq!(systems.active_id(), "");
    assert_eq!(systems.find(System::is_physical), None);
    let mut rig = starter(PHYSICAL, "", "");
    rig.name = "  ".into();
    let id = systems.add(rig);
    assert_eq!(
        (id.as_str(), systems.active_id()),
        ("real-drone", "real-drone"),
        "named from its pieces"
    );
    let sim = systems.add(starter(SIMULATION, "", "Lab"));
    assert_eq!(
        systems.active_id(),
        "real-drone",
        "only the first becomes active"
    );
    assert_eq!(
        systems.find(System::simulated).as_deref(),
        Some(sim.as_str())
    );
    assert_eq!(
        systems.find(System::is_physical).as_deref(),
        Some("real-drone"),
        "the active one first"
    );
    let f = systems.to_file();
    assert_eq!((f.schema, f.systems.len()), (file::SCHEMA, 2));
}

#[test]
fn names_come_from_the_pieces() {
    assert_eq!(starter(SIMULATION, "", "").suggested_name(), "PX4 SIH");
    assert_eq!(
        starter(ROS, "", "").suggested_name(),
        "PX4 + Gazebo + ROS 2"
    );
    assert_eq!(
        starter(AI, "", "").suggested_name(),
        "Recording replay + inference"
    );
    let mut real = starter(PHYSICAL, "", "");
    assert_eq!(real.suggested_name(), "Real drone");
    real.vehicle = Vehicle::physical("serial:COM7:57600");
    assert_eq!(real.suggested_name(), "Real drone · COM7");
    real.vehicle = Vehicle::physical("udp:0.0.0.0:14550");
    assert_eq!(real.suggested_name(), "Real drone · UDP 14550");
    let mut jmav = starter(SIMULATION, "", "");
    jmav.vehicle = Vehicle::Simulated {
        autopilot: model::Autopilot::Px4,
        world: model::World::new("jmavsim", &[]),
    };
    jmav.consumers.ros.agent = true;
    assert_eq!(jmav.suggested_name(), "PX4 + jMAVSim + ROS 2");
}

#[test]
fn an_old_starter_name_that_no_longer_fits_is_renamed_once() {
    let mut jmav = starter(SIMULATION, "local-simulation", "Local simulation");
    jmav.vehicle = Vehicle::Simulated {
        autopilot: model::Autopilot::Px4,
        world: model::World::new("jmavsim", &[]),
    };
    let untouched = starter(PHYSICAL, "physical-vehicle", "Physical vehicle");
    let mut mine = starter(SIMULATION, "mine", "My bench");
    mine.vehicle = Vehicle::ardupilot("plane");
    let v3 = serde_json::json!({"schema": 3, "active": "local-simulation",
        "systems": [jmav, untouched, mine]});
    let (f, migrated) = file::parse(v3.to_string().as_bytes()).unwrap().unwrap();
    assert!(migrated);
    let names: Vec<&str> = f.systems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["PX4 + jMAVSim", "Physical vehicle", "My bench"]);
    assert_eq!(f.systems[0].id, "local-simulation", "the id stays");
    let again = serde_json::to_vec(&f).unwrap();
    let (f2, migrated) = file::parse(&again).unwrap().unwrap();
    assert!(!migrated, "schema 4 is not renamed again");
    assert_eq!(f2, f);
}

#[test]
fn the_editor_validates_vehicle_details_and_consumers() {
    let base = starter(SIMULATION, "lab", "Lab");
    let mut d = Draft::of(&base);
    assert_eq!(d.apply(&base).unwrap(), base);
    d.world = "gazebo".into();
    set(&mut d, "world", "baylands");
    let gz = d.apply(&base).unwrap();
    assert_eq!(gz.vehicle, Vehicle::gazebo("baylands", "x500_mono_cam"));
    set(&mut d, "model", "x500 cam");
    assert!(
        d.apply(&base)
            .unwrap_err()
            .contains("Vehicle model is a name such as x500_mono_cam")
    );

    d.vehicle = 1;
    d.endpoint = rune::TextEdit::new("COM4");
    assert!(d.apply(&base).is_err());
    for ok in [
        "",
        "serial:COM4:115200",
        "udp:0.0.0.0:14550",
        "tcp:192.168.4.1:5760",
    ] {
        d.endpoint = rune::TextEdit::new(ok);
        assert!(d.apply(&base).is_ok(), "{ok}");
    }
    d.vehicle = 0;
    d.world = "internal".into();
    d.ros_agent = true;
    d.domain = rune::TextEdit::new("300");
    assert!(d.apply(&base).unwrap_err().contains("ROS domain"));
    d.domain = rune::TextEdit::new("42");
    d.scripts = true;
    d.scripts_udp = rune::TextEdit::new("localhost");
    assert!(d.apply(&base).unwrap_err().contains("ip:port"));
    d.scripts_udp = rune::TextEdit::new("127.0.0.1:14541");
    d.inference = true;
    d.project = rune::TextEdit::new("detector");
    let sys = d.apply(&base).unwrap();
    assert_eq!(sys.consumers.ros.domain, 42);
    assert_eq!(
        sys.consumers.scripts_udp.as_deref(),
        Some("127.0.0.1:14541")
    );
    assert_eq!(
        sys.consumers.inference,
        Some(Inference {
            project: Some("detector".into())
        })
    );
    d.name = rune::TextEdit::new(" ");
    assert!(d.apply(&base).is_err());
}

#[test]
fn slugs_are_stable_ascii() {
    assert_eq!(model::slug("ROS & sensor capture"), "ros-sensor-capture");
    assert_eq!(model::slug("  Über drone!! "), "ber-drone");
    assert_eq!(model::slug("!!!"), "system");
    assert!(model::valid_id(&model::slug(&"long name ".repeat(20))));
}

#[test]
fn switching_while_work_runs_keeps_it_with_the_system_that_started_it() {
    let mut s = State::new();
    let sim = s.systems.active_id().to_string();
    s.bundle(session_stages(s.systems.current()));
    assert_eq!(s.systems.owner("sih"), Some(sim.as_str()));
    assert_eq!(s.systems.owner("link"), Some(sim.as_str()));
    assert!(owns_work(&s, &sim), "a staged launch is work of its system");
    s.session = None;
    s.status.insert("sih".into(), "Running".into());
    poll(&mut s);
    assert!(owns_work(&s, &sim));

    assert!(
        s.select_starter(ROS),
        "switching is allowed while work runs"
    );
    assert_eq!(s.systems.active_id(), "ros-sensor-capture");
    assert!(s.notice.contains("stays with the system"), "{}", s.notice);
    assert_eq!(s.status.get("sih").map(String::as_str), Some("Running"));
    assert!(owns_work(&s, &sim));
    assert!(!owns_work(&s, s.systems.active_id()));

    // Work nobody claimed belongs to the system active when it is first seen.
    s.recorder.recording = true;
    poll(&mut s);
    assert_eq!(s.systems.owner("recorder"), Some("ros-sensor-capture"));
    s.recorder.recording = false;
    s.status.insert("sih".into(), "Exited 0".into());
    poll(&mut s);
    assert!(s.systems.owner("sih").is_none() && s.systems.owner("recorder").is_none());
    assert!(!owns_work(&s, &sim));
    assert!(!s.select_system("missing"));
    assert!(s.select_system(&sim));
}

#[test]
fn a_physical_system_fills_the_monitor_endpoint_when_selected() {
    let mut s = State::new();
    let id = s.systems.add_starter(PHYSICAL, "Rig");
    s.systems.get_mut(&id).unwrap().vehicle = Vehicle::physical("serial:COM7:57600");
    assert!(s.select_system(&id));
    assert_eq!(s.hardware.mode, 1);
    assert_eq!(s.hardware.endpoint.text, "serial:COM7:57600");
    assert!(s.select_starter(SIMULATION));
    assert_eq!(s.hardware.mode, 0);
}

#[test]
fn the_editor_drops_consumers_it_hides_for_the_vehicle() {
    let base = starter(SIMULATION, "lab", "Lab");
    let mut d = Draft::of(&base);
    d.ros_agent = true;
    d.domain = rune::TextEdit::new("not a number");
    // Physical hides the ROS bridge; Replay hides live view and scripts.
    d.vehicle = 1;
    let sys = d.apply(&base).expect("a hidden domain is not validated");
    assert!(
        !sys.consumers.ros.agent,
        "a physical source has no ROS bridge"
    );
    assert!(sys.consumers.live_view, "physical keeps its live view");
    assert_eq!(sys.consumers.ros.domain, base.consumers.ros.domain);
    d.vehicle = 2;
    let sys = d.apply(&base).unwrap();
    assert!(!sys.consumers.live_view && sys.consumers.scripts_udp.is_none());
    assert_eq!(sys.consumers_summary(), "QGroundControl");
    d.vehicle = 0;
    d.world = "gazebo".into();
    assert!(
        d.apply(&base).unwrap_err().contains("ROS domain"),
        "a shown domain is validated"
    );
    d.domain = rune::TextEdit::new("5");
    let sys = d.apply(&base).unwrap();
    assert!(
        sys.consumers.live_view,
        "every live vehicle can feed the live view"
    );
    assert!(sys.consumers.ros.agent);
    assert_eq!(sys.consumers.ros.domain, 5);
}

#[test]
fn vehicle_details_reject_paths_options_and_bad_ports() {
    let base = starter(SIMULATION, "lab", "Lab");
    let mut d = Draft::of(&base);
    d.world = "gazebo".into();
    for bad in ["..", "-world", "../etc", ".hidden", ""] {
        set(&mut d, "world", bad);
        assert!(d.apply(&base).is_err(), "world {bad:?}");
    }
    set(&mut d, "world", "default");
    d.vehicle = 1;
    for bad in [
        "serial:COM4;calc:57600",
        "serial:COM4:0",
        "serial::57600",
        "serial:-x:57600",
        "udp:0.0.0.0:0",
        "tcp:host",
        "http://x",
    ] {
        d.endpoint = rune::TextEdit::new(bad);
        assert!(d.apply(&base).is_err(), "endpoint {bad:?}");
    }
    for ok in [
        "serial:/dev/ttyUSB0:921600",
        "serial:COM12:57600",
        "tcp:vehicle.local:5760",
    ] {
        d.endpoint = rune::TextEdit::new(ok);
        assert!(d.apply(&base).is_ok(), "endpoint {ok:?}");
    }
}

#[test]
fn a_second_launch_never_replaces_one_in_progress() {
    let mut s = State::new();
    let sim = s.systems.active_id().to_string();
    assert!(s.bundle(session_stages(s.systems.current())));
    assert!(s.select_starter(ROS));
    let ros = s.systems.active_id().to_string();
    assert!(
        !s.bundle(session_stages(s.systems.current())),
        "the first launch is kept"
    );
    assert!(s.notice.contains("Local simulation"), "{}", s.notice);
    let launch = s.session.as_ref().unwrap();
    assert_eq!(launch.stages(), ["sih", "router", "link", "qgc"]);
    assert_eq!(s.systems.owner("session"), Some(sim.as_str()));
    assert_eq!(
        s.systems.owner("gazebo"),
        None,
        "nothing was claimed for the refused launch"
    );
    assert!(owns_work(&s, &sim) && !owns_work(&s, &ros));
}

#[test]
fn claims_of_a_launch_that_stopped_early_are_forgotten() {
    let mut s = State::new();
    let sim = s.systems.active_id().to_string();
    s.bundle(session_stages(s.systems.current()));
    s.status.insert("sih".into(), "Exited 1".into());
    poll(&mut s);
    assert_eq!(
        s.systems.owner("router"),
        Some(sim.as_str()),
        "a pending stage stays claimed"
    );
    // The launch stops at sih: router, link and qgc never start.
    s.session = None;
    poll(&mut s);
    for key in ["sih", "router", "link", "qgc", "session"] {
        assert_eq!(s.systems.owner(key), None, "{key}");
    }
    assert!(s.select_starter(ROS));
    s.status.insert("dds".into(), "Running".into());
    poll(&mut s);
    assert_eq!(s.systems.owner("dds"), Some(s.systems.active_id()));
}

#[test]
fn a_running_pipeline_job_is_owned_work_that_stop_session_cancels() {
    use crate::studio::pipelines::{Job, JobKind};
    let mut s = State::new();
    assert!(!active_work(&s) && !s.busy());
    s.studio.pipes.job = Some(Job::fake(JobKind::Infer));
    assert!(active_work(&s), "the System page offers Stop session");
    assert!(s.busy(), "installs and environment changes wait for it");
    assert_eq!(work_labels(&s), ["pipeline job"]);
    assert_eq!(crate::ui::session_label(&s), "pipeline job");

    // Switching keeps the job; the notice says the work stays.
    assert!(s.select_starter(PHYSICAL));
    assert!(s.notice.contains("Running work stays"), "{}", s.notice);
    assert!(s.studio.pipes.job.is_some());

    let mut next = s.config.clone();
    next.distro = "AnotherDistribution".into();
    s.editor = rune::TextEdit::new(&serde_json::to_string(&next).unwrap());
    s.apply();
    assert_ne!(s.config.distro, "AnotherDistribution");
    assert!(s.notice.contains("pipeline jobs"), "{}", s.notice);

    s.stop_all();
    assert!(s.studio.pipes.job.as_ref().is_some_and(Job::cancelling));
}

/// UI-02: a physical system whose scripts consumer sends to another computer is warned about,
/// never refused: that computer gets the telemetry and can command the vehicle.
#[test]
fn a_physical_scripts_address_off_this_computer_is_cautioned_not_refused() {
    use super::edit::off_this_computer;
    // Only a real IP on another computer counts: loopback, the unspecified address and host
    // names (refused, never forwarded) do not.
    for not_remote in [
        "127.0.0.1:14540",
        "127.8.0.3:9",
        "[::1]:14540",
        "localhost:14540",
        "ground-pc:14540",
        "0.0.0.0:14540",
        "[::]:14540",
    ] {
        assert!(!off_this_computer(not_remote), "{not_remote}");
    }
    for remote in ["192.168.1.20:14540", "10.0.0.5:1", "[fd00::5]:14540"] {
        assert!(off_this_computer(remote), "{remote}");
    }

    let base = starter(PHYSICAL, "rig", "Rig");
    let mut d = Draft::of(&base);
    d.vehicle = 1;
    d.scripts = true;
    d.scripts_udp = rune::TextEdit::new("127.0.0.1:14540");
    assert_eq!(d.caution(), None);
    d.scripts_udp = rune::TextEdit::new("192.168.1.20:14540");
    let caution = d.caution().expect("a network address is cautioned");
    assert!(caution.starts_with("192.168.1.20:14540 is not this computer"));
    assert!(caution.contains("can command the vehicle"));
    let sys = d.apply(&base).expect("saving is not blocked");
    assert_eq!(
        sys.consumers.scripts_udp.as_deref(),
        Some("192.168.1.20:14540")
    );

    d.scripts = false;
    assert_eq!(d.caution(), None, "scripts off");
    d.scripts = true;
    d.vehicle = 0;
    assert_eq!(
        d.caution(),
        None,
        "a simulation is not a vehicle in the air"
    );
    d.vehicle = 1;
    d.scripts_udp = rune::TextEdit::new("not an address");
    assert_eq!(d.caution(), None, "the address error comes first");
}

/// UI-08: the vehicle link forwards only to an `ip:port`, so a host name or the unspecified
/// address is refused with its own reason instead of being saved and silently not forwarded.
#[test]
fn a_scripts_address_the_link_cannot_forward_to_is_refused_with_its_reason() {
    let base = starter(PHYSICAL, "rig", "Rig");
    let mut d = Draft::of(&base);
    d.vehicle = 1;
    d.scripts = true;
    for host in ["ground-pc:14540", "localhost:14540"] {
        d.scripts_udp = rune::TextEdit::new(host);
        let e = d.apply(&base).unwrap_err();
        assert!(e.contains("Host names are not resolved"), "{host}: {e}");
        assert!(e.contains("Use an IP address"), "{e}");
        assert_eq!(d.scripts_problem(), Some(e));
        assert_eq!(
            d.caution(),
            None,
            "{host} is never forwarded, so never cautioned"
        );
    }
    for unspecified in ["0.0.0.0:14540", "[::]:14540"] {
        d.scripts_udp = rune::TextEdit::new(unspecified);
        let e = d.apply(&base).unwrap_err();
        assert!(e.contains("listen address, not a destination"), "{e}");
        assert_eq!(d.caution(), None);
    }
    d.scripts_udp = rune::TextEdit::new("127.0.0.1:0");
    assert!(d.apply(&base).unwrap_err().contains("1 to 65535"));
    d.scripts_udp = rune::TextEdit::new("ground pc");
    assert!(d.apply(&base).unwrap_err().contains("ip:port"));
    for ok in ["127.0.0.1:14540", " [::1]:14540 ", "192.168.1.20:14540"] {
        d.scripts_udp = rune::TextEdit::new(ok);
        assert_eq!(d.scripts_problem(), None, "{ok}");
        assert!(d.apply(&base).is_ok(), "{ok}");
    }
    // Scripts off: the hidden address is not checked.
    d.scripts = false;
    d.scripts_udp = rune::TextEdit::new("ground-pc:14540");
    assert!(d.apply(&base).is_ok());
}

/// UI-02 in the editor: the caution shows under the scripts address and is repeated on save.
#[test]
fn the_system_editor_cautions_a_network_scripts_address_on_a_physical_system() {
    use crate::ui::Page;
    use rune::{Env, Runtime};
    let mut s = State::new();
    s.navigate(Page::Settings);
    let id = s.systems.add_starter(PHYSICAL, "Bench rig");
    let mut d = s.systems.get(&id).map(Draft::of).unwrap();
    d.scripts = true;
    d.scripts_udp = rune::TextEdit::new("192.168.1.20:14540");
    s.systems.editor = Some(d);
    let mut rt = Runtime::new(
        s,
        crate::view,
        (1440., 900.),
        Env {
            scale: 1.,
            system_dark: true,
        },
    );
    rt.update(|_, tx| tx.scroll_into_view("system-scripts"));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    assert!(rt.find("system-scripts-network").is_some());
    assert!(rt.find("system-bridge-warning").is_some());
    rt.update(|_, tx| tx.scroll_into_view("system-save"));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    let (x, y) = rt.find("system-save").expect("save");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.1);
    assert!(rt.state.systems.editor.is_none(), "saved");
    assert!(
        rt.state.notice.contains("is not this computer"),
        "{}",
        rt.state.notice
    );
    assert_eq!(
        rt.state
            .systems
            .get(&id)
            .unwrap()
            .consumers
            .scripts_udp
            .as_deref(),
        Some("192.168.1.20:14540")
    );
}

#[test]
fn foxglove_and_automatic_recording_start_with_the_session() {
    let mut sys = starter(ROS, "lab", "Lab");
    sys.consumers.qgc = false;
    assert_eq!(
        session_stages(&sys),
        ["gazebo", "router", "link", "dds", "rosgz", "foxglove"],
        "Foxglove starts with the session when chosen"
    );
    sys.consumers.ros.record_on_start = true;
    assert_eq!(
        session_stages(&sys).last(),
        Some(&"mcap"),
        "recording waits for the others"
    );
    sys.consumers.ros.recorder = false;
    assert!(
        !session_stages(&sys).contains(&"mcap"),
        "no recorder, no automatic recording"
    );
    sys.consumers.foxglove = false;
    assert!(!session_stages(&sys).contains(&"foxglove"));
}

#[test]
fn record_on_start_is_kept_only_for_a_simulated_system_with_the_recorder() {
    let base = starter(ROS, "lab", "Lab");
    let mut d = Draft::of(&base);
    d.record_on_start = true;
    assert!(d.apply(&base).unwrap().consumers.ros.record_on_start);
    d.recorder = false;
    assert!(!d.apply(&base).unwrap().consumers.ros.record_on_start);
    d.recorder = true;
    d.vehicle = 1;
    d.endpoint = rune::TextEdit::new("udp:0.0.0.0:14550");
    assert!(
        !d.apply(&base).unwrap().consumers.ros.record_on_start,
        "a real drone has no launch to record from"
    );
    let mut saved = base.clone();
    saved.consumers.ros.topics = vec!["/imu".into()];
    let d = Draft::of(&saved);
    assert_eq!(
        d.apply(&saved).unwrap().consumers.ros.topics,
        ["/imu"],
        "editing keeps the topics"
    );
}
