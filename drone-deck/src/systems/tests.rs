use super::model::{self, Consumers, Inference, Ros, TEMPLATE_IDS, template};
use super::*;

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
    assert_eq!(f.schema, 2);
    assert_eq!(f.systems.len(), 4);
    let names: Vec<_> = f.systems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["Lab", "ROS & sensor capture", "Models", "Bench quad"]
    );
    let prepared: Vec<_> = f.systems.iter().map(|s| s.prepared).collect();
    assert_eq!(prepared, [true, true, false, true]);
    assert_eq!(f.active, "physical-vehicle");
    let kinds: Vec<_> = f.systems.iter().map(System::kind).collect();
    assert_eq!(kinds, [SIMULATION, ROS, AI, PHYSICAL]);
    // The 0.8 ROS system ran SIH with the DDS Agent; a migrated one keeps working that way.
    assert_eq!(f.systems[ROS].source, Source::Sih);
    assert_eq!(
        session_stages(&f.systems[ROS]),
        ["sih", "router", "link", "dds", "qgc"]
    );
    for (sys, template) in f.systems.iter().zip(TEMPLATE_IDS) {
        assert_eq!(sys.template, template);
        assert_eq!(sys.id, template);
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
fn schema_2_round_trips_and_newer_schemas_are_refused() {
    let mut systems = Systems::default();
    let id = systems.create(ROS, "Camera lab");
    systems.set_active(&id);
    let json = serde_json::to_vec(&systems.to_file()).unwrap();
    let text = String::from_utf8(json.clone()).unwrap();
    assert!(text.contains(r#""schema":2"#), "{text}");
    assert!(text.contains(r#""kind":"gazebo""#), "{text}");
    assert!(text.contains(r#""model":"x500_mono_cam""#), "{text}");
    let (back, migrated) = file::parse(&json).unwrap().unwrap();
    assert!(!migrated);
    assert_eq!(back, systems.to_file());
    let restored = Systems::from_file(back);
    assert_eq!(restored.active_id(), "camera-lab");
    assert_eq!(restored.active, ROS);

    let err = file::parse(br#"{"schema":3,"active":"x","systems":[]}"#)
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
    let body = br#"{"schema":3,"active":"x","systems":[{"id":"x"}]}"#;
    std::fs::write(&path, body).unwrap();
    let systems = Systems::load_at(&path);
    assert_eq!(systems.list, model::defaults(), "the defaults run");
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
    assert_eq!(f.systems[2].source, Source::Replay { recording: None });
    let (empty, _) = file::parse(br#"{"schema":2,"active":"","systems":[]}"#)
        .unwrap()
        .unwrap();
    assert_eq!(
        empty.systems.len(),
        4,
        "an empty list falls back to the templates"
    );
}

#[test]
fn templates_reproduce_the_four_systems() {
    let sim = template(SIMULATION, "a", "A");
    assert_eq!(sim.source, Source::Sih);
    assert!(sim.consumers.qgc && sim.consumers.live_view);
    assert_eq!(ids(&sim), ["sih", "router", "link", "qgc", "scripts"]);
    assert_eq!(chain_ids(&sim), ["sih", "router", "link", "qgc"]);
    assert_eq!(session_stages(&sim), ["sih", "router", "link", "qgc"]);
    assert_eq!(session_stages(&sim), SESSIONS[SIMULATION]);
    assert_eq!(preset(&sim), 0);

    let ros = template(ROS, "b", "B");
    assert_eq!(
        ros.source,
        Source::Gazebo {
            world: "default".into(),
            model: "x500_mono_cam".into()
        }
    );
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
        ["gazebo", "router", "link", "dds", "rosgz", "qgc"]
    );
    assert_eq!(session_stages(&ros), SESSIONS[ROS]);
    assert_eq!(preset(&ros), 1);

    let ai = template(AI, "c", "C");
    assert_eq!(ai.source, Source::Replay { recording: None });
    assert_eq!(ai.consumers.inference, Some(Inference::default()));
    assert_eq!(chain_ids(&ai), ["onnx", "project", "pipelines"]);
    assert!(session_stages(&ai).is_empty());
    assert_eq!(preset(&ai), 3);

    let physical = template(PHYSICAL, "d", "D");
    assert!(!physical.consumers.qgc, "QGroundControl is optional");
    assert_eq!(chain_ids(&physical), ["endpoint", "link", "tlog"]);
    assert!(
        session_stages(&physical).is_empty(),
        "a physical link is no launch"
    );
    assert_eq!(preset(&physical), 4);
    assert_eq!(
        [sim.kind(), ros.kind(), ai.kind(), physical.kind()],
        [SIMULATION, ROS, AI, PHYSICAL]
    );
}

#[test]
fn components_chain_and_stages_follow_source_and_consumers() {
    let mut sys = template(SIMULATION, "x", "X");
    sys.consumers.live_view = false;
    sys.consumers.scripts_udp = None;
    assert_eq!(chain_ids(&sys), ["sih", "router", "link", "qgc"]);
    sys.consumers.qgc = false;
    assert_eq!(session_stages(&sys), ["sih", "router", "link"]);
    sys.consumers.ros = Ros {
        agent: true,
        recorder: false,
        domain: 7,
    };
    assert_eq!(sys.kind(), ROS);
    assert_eq!(chain_ids(&sys), ["sih", "dds", "ros2"]);
    assert_eq!(session_stages(&sys), ["sih", "router", "link", "dds"]);
    assert_eq!(preset(&sys), 5, "SIH with ROS needs no Gazebo");
    sys.consumers.live_view = true;
    assert_eq!(
        session_stages(&sys),
        ["sih", "router", "link", "dds"],
        "the live view needs no process"
    );

    sys.source = Source::ArduPilot {
        vehicle: "plane".into(),
    };
    assert_eq!(vehicle_profile(&sys), Some("ardupilot"));
    assert_eq!(
        session_stages(&sys),
        ["ardupilot", "link"],
        "ArduPilot SITL serves TCP 5760 itself; no router"
    );
    assert_eq!(chain_ids(&sys), ["ardupilot", "dds", "ros2"]);
    assert_eq!(preset(&sys), 2);
    assert!(!ids(&sys).contains(&"router"));
    sys.consumers.ros.agent = false;
    sys.consumers.qgc = true;
    assert_eq!(chain_ids(&sys), ["ardupilot", "link", "qgc"]);
    assert_eq!(session_stages(&sys), ["ardupilot", "link", "qgc"]);

    sys.source = Source::Physical {
        endpoint: "udp:0.0.0.0:14550".into(),
    };
    sys.consumers = Consumers {
        qgc: true,
        ..Default::default()
    };
    assert_eq!(chain_ids(&sys), ["endpoint", "link", "tlog"]);
    assert_eq!(ids(&sys), ["endpoint", "link", "tlog", "qgc"]);
    assert!(session_stages(&sys).is_empty());

    sys.source = Source::Replay { recording: None };
    sys.consumers.ros.recorder = true;
    assert_eq!(ids(&sys).iter().filter(|i| **i == "mcap").count(), 1);
    assert_eq!(chain_ids(&sys), ["mcap"]);
    // Chains only list stations that are components.
    for k in 0..4 {
        let t = template(k, "t", "T");
        let comps = ids(&t);
        assert!(chain_ids(&t).iter().all(|c| comps.contains(c)));
    }
}

#[test]
fn create_rename_duplicate_and_delete() {
    let mut systems = Systems::default();
    assert_eq!(systems.list.len(), 4);
    let a = systems.create(SIMULATION, "  My Lab  ");
    assert_eq!(a, "my-lab");
    let b = systems.create(SIMULATION, "My lab");
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
    assert!(systems.remove(&last).is_err(), "the last system stays");
}

#[test]
fn kind_lookups_answer_for_the_active_system() {
    let mut systems = Systems::default();
    let id = systems.create(PHYSICAL, "Rig");
    systems.get_mut(&id).unwrap().prepared = true;
    systems.set_active(&id);
    assert_eq!(systems.active, PHYSICAL);
    assert_eq!(systems.name(PHYSICAL), "Rig");
    assert!(systems.prepared(PHYSICAL));
    assert_eq!(systems.name(AI), "Offline AI workspace");
    assert_eq!(
        systems.resolve(Pick::Kind(PHYSICAL)).as_deref(),
        Some("rig")
    );
    assert_eq!(
        systems.resolve(Pick::Kind(AI)).as_deref(),
        Some("offline-ai")
    );
    assert_eq!(systems.resolve(Pick::Id("nope")), None);
    systems.get_mut(&id).unwrap().source = Source::Sih;
    systems.changed();
    assert_eq!(
        systems.active, SIMULATION,
        "the kind follows an edited source"
    );
}

#[test]
fn the_editor_validates_source_details_and_consumers() {
    let base = template(SIMULATION, "lab", "Lab");
    let mut d = Draft::of(&base);
    assert_eq!(d.apply(&base).unwrap(), base);
    d.source = 1;
    d.world = rune::TextEdit::new("baylands");
    let gz = d.apply(&base).unwrap();
    assert_eq!(
        gz.source,
        Source::Gazebo {
            world: "baylands".into(),
            model: "x500_mono_cam".into()
        }
    );
    d.model = rune::TextEdit::new("x500 cam");
    assert!(
        d.apply(&base)
            .unwrap_err()
            .contains("Gazebo world and model")
    );

    d.source = 3;
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
    d.source = 0;
    d.ros_agent = true;
    d.domain = rune::TextEdit::new("300");
    assert!(d.apply(&base).unwrap_err().contains("ROS domain"));
    d.domain = rune::TextEdit::new("42");
    d.scripts = true;
    d.scripts_udp = rune::TextEdit::new("localhost");
    assert!(d.apply(&base).unwrap_err().contains("host:port"));
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

    assert!(s.select_system(ROS), "switching is allowed while work runs");
    assert_eq!(s.systems.active, ROS);
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
    let id = s.systems.create(PHYSICAL, "Rig");
    s.systems.get_mut(&id).unwrap().source = Source::Physical {
        endpoint: "serial:COM7:57600".into(),
    };
    assert!(s.select_system(&id));
    assert_eq!(s.hardware.mode, 1);
    assert_eq!(s.hardware.endpoint.text, "serial:COM7:57600");
    assert!(s.select_system(SIMULATION));
    assert_eq!(s.hardware.mode, 0);
}

#[test]
fn the_editor_drops_consumers_it_hides_for_the_source() {
    let base = template(SIMULATION, "lab", "Lab");
    let mut d = Draft::of(&base);
    d.ros_agent = true;
    d.domain = rune::TextEdit::new("not a number");
    // Physical hides the ROS bridge; Replay hides live view and scripts.
    d.source = 3;
    let sys = d.apply(&base).expect("a hidden domain is not validated");
    assert!(
        !sys.consumers.ros.agent,
        "a physical source has no ROS bridge"
    );
    assert!(sys.consumers.live_view, "physical keeps its live view");
    assert_eq!(sys.consumers.ros.domain, base.consumers.ros.domain);
    d.source = 4;
    let sys = d.apply(&base).unwrap();
    assert!(!sys.consumers.live_view && sys.consumers.scripts_udp.is_none());
    assert_eq!(sys.consumers_summary(), "QGroundControl");
    d.source = 1;
    assert!(
        d.apply(&base).unwrap_err().contains("ROS domain"),
        "a shown domain is validated"
    );
    d.domain = rune::TextEdit::new("5");
    let sys = d.apply(&base).unwrap();
    assert!(!sys.consumers.live_view, "Gazebo has no live-view toggle");
    assert!(sys.consumers.ros.agent);
    assert_eq!(sys.consumers.ros.domain, 5);
}

#[test]
fn source_details_reject_paths_options_and_bad_ports() {
    let base = template(SIMULATION, "lab", "Lab");
    let mut d = Draft::of(&base);
    d.source = 1;
    for bad in ["..", "-world", "../etc", ".hidden", ""] {
        d.world = rune::TextEdit::new(bad);
        assert!(d.apply(&base).is_err(), "world {bad:?}");
    }
    d.world = rune::TextEdit::new("default");
    d.source = 3;
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
    assert!(s.select_system(ROS));
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
    assert!(s.select_system(ROS));
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
    assert!(s.select_system(PHYSICAL));
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
