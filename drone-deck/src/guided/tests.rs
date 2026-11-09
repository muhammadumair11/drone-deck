use super::*;
use crate::recordings::{Session, Topic};

/// Guided setup at First run for `kind`, with `kind` as the active system.
fn first_run(kind: usize, task: usize) -> State {
    let mut s = State::new();
    s.systems.active = kind;
    s.guided.choice = kind;
    s.guided.chapter = 3;
    s.guided.task = task;
    s
}

fn topic(name: &str) -> Topic {
    Topic {
        name: name.into(),
        ..Default::default()
    }
}

#[test]
fn changing_the_system_drops_a_tool_check_queued_for_another_preset() {
    let mut s = State::new();
    s.setup.workflow = PRESET[systems::ROS];
    s.setup.queue = setup::workflow::ids(&s)
        .iter()
        .filter_map(|id| s.setup.recipe_index(id))
        .collect();
    s.guided.choice = systems::ROS;
    s.guided.check = Check::Tools;
    set_choice(&mut s, systems::AI);
    assert!(s.setup.queue.is_empty());
    assert_eq!(s.guided.check, Check::Idle);
    use_preset(&mut s);
    assert_eq!(s.setup.workflow, PRESET[systems::AI]);
}

#[test]
fn a_refused_switch_keeps_guided_setup_open_and_first_run_inactive() {
    let mut s = State::new();
    s.systems.active = systems::SIMULATION;
    s.status.insert("sih".into(), "Running".into());
    s.guided.choice = systems::ROS;
    s.guided.name = TextEdit::new("Lab");
    s.page = ui::Page::Setup;
    open_workspace(&mut s);
    assert_eq!(s.page, ui::Page::Setup);
    assert_eq!(s.systems.active, systems::SIMULATION);
    assert!(s.systems.records[systems::ROS].prepared);
    assert!(
        s.guided
            .prep_error
            .contains("Stop the running session to switch to Lab; it was saved as prepared.")
    );
    s.guided.chapter = 3;
    s.guided.prep_error.clear();
    run::act(&mut s);
    assert!(!s.guided.awaiting);
    assert!(s.guided.prep_error.contains("is not the active system"));
}

#[test]
fn ros_publishers_need_application_topics_from_a_ready_px4_and_dds_agent() {
    let mut s = first_run(systems::ROS, 0);
    s.guided.awaiting = true;
    s.recorder.discovered = vec![
        topic("/rosout"),
        topic("/parameter_events"),
        topic("/clock"),
        topic("/tf_static"),
    ];
    s.status.insert("sih".into(), "Running".into());
    s.status.insert("dds".into(), "Running".into());
    s.readiness.insert("sih".into(), "Ready".into());
    assert!(run::evidence(&s).is_none(), "infrastructure topics only");
    s.recorder.discovered.push(topic("/fmu/out/vehicle_status"));
    assert!(run::evidence(&s).is_some());
    s.status.insert("dds".into(), "Starting".into());
    assert!(run::evidence(&s).is_none(), "DDS agent not running yet");
    s.status.insert("dds".into(), "Running".into());
    s.readiness.remove("sih");
    assert!(run::evidence(&s).is_none(), "PX4 not ready yet");

    s.guided.task = 1;
    s.recorder.topics = TextEdit::new("");
    run::act(&mut s);
    assert_eq!(s.recorder.topics.text, "/fmu/out/vehicle_status");
}

#[test]
fn ros_finalize_needs_a_finalized_recording_with_messages() {
    let mut s = first_run(systems::ROS, 2);
    s.guided.awaiting = true;
    s.recorder.selected = Some("flight".into());
    s.recorder.sessions = vec![Session {
        id: "flight".into(),
        status: "Recording".into(),
        messages: 40,
        ..Default::default()
    }];
    assert!(run::evidence(&s).is_none());
    assert!(run::observed(&s).contains("Recording"));
    s.recorder.sessions[0].status = "Finalized".into();
    s.recorder.sessions[0].messages = 0;
    assert!(run::evidence(&s).is_none());
    s.recorder.sessions[0].messages = 40;
    assert!(run::evidence(&s).is_some());
}

#[test]
fn a_running_queue_selects_the_system_it_installs_for() {
    let mut s = State::new();
    s.systems.active = systems::SIMULATION;
    s.setup.workflow = PRESET[systems::ROS];
    setup::batch::preview(&mut s);
    assert!(s.setup.batch.is_some());
    open(&mut s);
    assert_eq!(s.guided.choice, systems::ROS);
    assert!(s.guided.applied);
}

#[test]
fn a_stale_exit_status_is_not_first_run_evidence() {
    let mut s = first_run(systems::SIMULATION, 1);
    s.status.insert("qgc".into(), "Exited 0".into());
    run::act(&mut s);
    assert!(!s.guided.awaiting, "headless start requests no launch");
    s.guided.awaiting = true;
    assert!(run::evidence(&s).is_none());
    s.guided.launched = Some("qgc".into());
    assert!(run::evidence(&s).is_some());

    let mut s = first_run(systems::AI, 1);
    s.guided.awaiting = true;
    s.status.insert("project-old".into(), "Exited 0".into());
    assert!(run::evidence(&s).is_none());
    s.guided.launched = Some("project-old".into());
    assert!(run::evidence(&s).is_some());
}
