use super::*;
use rune::{Env, Runtime};

#[test]
fn active_capture_prevents_switching_to_another_distribution() {
    let mut s = State::new();
    s.recorder.recording = true;
    let mut config = s.config.clone();
    config.distro = "Another-Ubuntu".into();
    s.editor = TextEdit::new(&serde_json::to_string(&config).unwrap());
    s.apply();
    assert_ne!(s.config.distro, "Another-Ubuntu");
    assert!(s.notice.contains("before changing the WSL distribution"));
}

#[test]
fn replay_requires_a_separate_domain_and_a_valid_rate() {
    let mut state = State::new();
    state.recorder.selected = Some("flight1".into());
    state.recorder.replay_domain = TextEdit::new("0");
    assert!(payload(&state, "replay").unwrap_err().contains("different"));
    state.recorder.replay_domain = TextEdit::new("77");
    for rate in ["NaN", "inf", "0", "100", "garbage"] {
        state.recorder.rate = TextEdit::new(rate);
        assert!(payload(&state, "replay").is_err());
    }
    state.recorder.rate = TextEdit::new("0.5");
    let value = payload(&state, "replay").unwrap();
    assert_eq!(value["domain"], 77);
    assert_eq!(value["rate"], 0.5);
    state.recorder.capture_domain = TextEdit::new("invalid");
    assert!(
        payload(&state, "stop").is_ok(),
        "Stop cannot depend on an edited form"
    );
}

#[test]
fn topic_selection_and_worker_events_drive_the_recording_ui() {
    let mut s = State::new();
    assert!(payload(&s, "record").is_err());
    toggle_topic(&mut s, "/imu");
    toggle_topic(&mut s, "/camera/image_raw");
    toggle_topic(&mut s, "/imu");
    assert_eq!(
        payload(&s, "record").unwrap()["topics"],
        json!(["/camera/image_raw"])
    );
    apply_event(
        &mut s,
        json!({"kind":"result","action":"check","ok":true,
        "data":{"ready":true,"root":"/home/test/recordings"}}),
    );
    assert!(s.recorder.ready);
    apply_event(
        &mut s,
        json!({"kind":"activity","recording":true,"playing":false,"message":"started"}),
    );
    assert!(s.recorder.recording);
    apply_event(
        &mut s,
        json!({"kind":"result","action":"record","ok":false,"error":"disk full"}),
    );
    assert_eq!(s.recorder.message, "disk full");
    send(&mut s, "record");
    assert!(
        s.recorder.bridge.is_none(),
        "Headless UI must never launch ROS processes"
    );
}

#[test]
fn recordings_are_reachable_from_navigation_and_ai_page() {
    let mut rt = Runtime::new(
        State::new(),
        view,
        (1216., 700.),
        Env {
            system_dark: false,
            scale: 1.,
        },
    );
    rt.frame(0.);
    let (x, y) = rt.find("rail-data").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert_eq!(rt.state.page, crate::ui::Page::Data);
    assert_eq!(rt.state.ui.data_tab, 0, "recordings is the first data tab");
    assert!(rt.find("capture-panel").is_some());
    rt.update(|s, _| s.ui.data_tab = 1);
    rt.frame(0.2);
    assert!(rt.find("capture-panel").is_none());
    let (x, y) = rt.find("data-tab-0").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert!(rt.find("capture-panel").is_some());
}

#[test]
fn the_capture_note_names_the_active_system_and_follows_its_ros_consumers() {
    let mut rt = Runtime::new(
        State::new(),
        view,
        (1216., 900.),
        Env {
            system_dark: false,
            scale: 1.,
        },
    );
    rt.update(|s, _| {
        let id = s
            .systems
            .add_starter(crate::systems::SIMULATION, "Bench sim");
        s.select_system(&id);
        s.navigate(crate::ui::Page::Data);
    });
    rt.frame(0.2);
    assert!(rt.find("recording-system-note").is_some());
    assert!(
        rt.elements()
            .into_iter()
            .filter_map(|e| e.text)
            .any(|t| t.contains("The active system is Bench sim."))
    );
    rt.update(|s, _| {
        let id = s.systems.active_id().to_string();
        s.systems.get_mut(&id).unwrap().consumers.ros.recorder = true;
    });
    rt.frame(0.2);
    assert!(rt.find("recording-system-note").is_none());
}

#[test]
fn a_simulated_recording_carries_a_recipe_and_the_autopilot_topics() {
    let mut s = State::new();
    s.recorder.topics = TextEdit::new("/camera/image_raw");
    s.recorder.discovered = [
        "/fmu/in/vehicle_command",
        "/fmu/out/vehicle_status_v1",
        "/camera/image_raw",
    ]
    .iter()
    .map(|n| Topic {
        name: (*n).into(),
        ..Default::default()
    })
    .collect();
    let value = payload(&s, "record").unwrap();
    let topics: Vec<&str> = value["topics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t.as_str())
        .collect();
    assert_eq!(
        topics,
        [
            "/camera/image_raw",
            "/fmu/in/vehicle_command",
            "/fmu/out/vehicle_status_v1"
        ],
        "the autopilot's topics are added to the selection"
    );
    assert_eq!(value["recipe"]["adapter"], "px4-sih");
    assert_eq!(
        value["recipe"]["command_topics"],
        serde_json::json!(["/fmu/in/vehicle_command"])
    );
    assert_eq!(
        value["recipe"]["evidence_topics"],
        serde_json::json!(["/fmu/out/vehicle_status_v1"])
    );
    assert!(
        payload(&s, "stop").unwrap().get("params").is_none(),
        "no link, no parameters"
    );

    assert!(s.select_starter(crate::systems::PHYSICAL));
    let value = payload(&s, "record").unwrap();
    assert!(
        value["recipe"].is_null(),
        "a physical vehicle records no recipe here"
    );
    assert_eq!(value["topics"], serde_json::json!(["/camera/image_raw"]));
}

#[test]
fn the_library_says_which_recordings_can_be_flown_again() {
    let mut session = Session::default();
    assert_eq!(view::resimulable(&session), None);
    session.recipe =
        Some(serde_json::json!({"adapter": "px4-gazebo", "commands": 2, "params": true}));
    assert_eq!(
        view::resimulable(&session).as_deref(),
        Some("Re-simulable / PX4 + Gazebo / 2 command topics")
    );
    session.recipe = Some(serde_json::json!({"adapter": "ardupilot-sitl", "commands": 0}));
    assert_eq!(
        view::resimulable(&session).as_deref(),
        Some("Re-simulable / ArduPilot SITL / no command topics recorded / parameters not kept")
    );
}

#[test]
fn a_system_with_ros_records_on_its_own_domain_and_keeps_its_topics() {
    let mut s = State::new();
    let id = s.systems.active_id().to_string();
    {
        let ros = &mut s.systems.get_mut(&id).unwrap().consumers.ros;
        ros.agent = true;
        ros.domain = 5;
        ros.topics = vec!["/camera/image_raw".into()];
    }
    s.recorder.capture_domain = TextEdit::new("0");
    assert_eq!(
        capture_domain(&s),
        Ok(5),
        "the system's domain, not the typed one"
    );
    follow_system(&mut s);
    assert_eq!(s.recorder.capture_domain.text, "5");
    assert_eq!(s.recorder.topics.text, "/camera/image_raw");
    let value = payload(&s, "record").unwrap();
    assert_eq!(
        (value["domain"].as_u64(), value["system"].as_str()),
        (Some(5), Some(id.as_str()))
    );

    // A confirmed recording saves the topics it was asked for to the system that asked.
    s.recorder.topics = TextEdit::new("/camera/image_raw /imu");
    s.recorder.saving = Some((id.clone(), topic_names(&s.recorder.topics.text)));
    apply_event(
        &mut s,
        serde_json::json!({"kind":"result","action":"record","ok":true,"data":{"id":"rec-1"}}),
    );
    assert_eq!(
        s.systems.get(&id).unwrap().consumers.ros.topics,
        ["/camera/image_raw", "/imu"]
    );
    assert!(s.recorder.saving.is_none());

    // A refused one keeps nothing.
    s.recorder.saving = Some((id.clone(), vec!["/other".into()]));
    apply_event(
        &mut s,
        serde_json::json!({"kind":"result","action":"record","ok":false,"error":"no"}),
    );
    assert_eq!(
        s.systems.get(&id).unwrap().consumers.ros.topics,
        ["/camera/image_raw", "/imu"]
    );

    // A recording in progress keeps its settings when the system changes.
    s.recorder.recording = true;
    s.systems.get_mut(&id).unwrap().consumers.ros.domain = 9;
    follow_system(&mut s);
    assert_eq!(s.recorder.capture_domain.text, "5");
}

#[test]
fn without_ros_the_typed_domain_is_used_and_a_recording_vehicle_selects_its_recording() {
    let mut s = State::new();
    assert_eq!(system_domain(&s), None);
    s.recorder.capture_domain = TextEdit::new("12");
    assert_eq!(capture_domain(&s), Ok(12));
    let id = s.systems.active_id().to_string();
    s.systems.get_mut(&id).unwrap().vehicle =
        systems::Vehicle::replay(Some("20261114T101500Z-offboard"));
    systems::start_workspace(&mut s);
    assert_eq!(
        s.recorder.selected.as_deref(),
        Some("20261114T101500Z-offboard")
    );
    assert_eq!(
        s.studio.pipes.selected.as_deref(),
        Some("20261114T101500Z-offboard")
    );
}
