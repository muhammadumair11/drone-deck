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
        let id = s.systems.create(crate::systems::SIMULATION, "Bench sim");
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
