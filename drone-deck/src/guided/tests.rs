use super::tasks::{Task, tasks};
use super::*;
use crate::recordings::{Session, Topic};

fn ready_checks(s: &mut State) {
    use_preset(s);
    for id in s.setup.workflow.recipes.clone() {
        s.setup.checks.insert(
            id.clone(),
            setup::Candidate {
                tool: id,
                found: true,
                compatible: true,
                ..Default::default()
            },
        );
    }
}
fn first_run(kind: usize, task: Task) -> State {
    let mut s = State::new();
    s.select_starter(kind);
    open(&mut s);
    s.guided.chapter = RUN;
    s.guided.task = tasks(&s)
        .iter()
        .position(|t| *t == task)
        .expect("task exists");
    s
}
#[test]
fn new_system_is_unsaved_until_review_and_saved_only_once() {
    let mut s = State::new();
    new_system(&mut s);
    s.guided.draft.name = TextEdit::new("Lab");
    s.guided.draft.world = "jmavsim".into();
    go(&mut s, REVIEW);
    assert_eq!(s.systems.list.len(), 4);
    assert_eq!(prep::phase(&s), prep::Phase::Unsaved);
    save_draft(&mut s);
    assert!(saved(&s));
    assert_eq!(s.systems.list.len(), 5);
    assert_eq!(target(&s).unwrap().adapter().unwrap().id, "px4-jmavsim");
    save_draft(&mut s);
    assert_eq!(s.systems.list.len(), 5);
    assert!(s.setup.jobs.is_empty());
}

#[test]
fn save_failures_rollback_new_and_existing_systems_and_keep_the_draft() {
    let mut s = State::new();
    new_system(&mut s);
    s.guided.draft.name = TextEdit::new("Unsaved lab");
    lifecycle::save_with(&mut s, |_, _| Err("disk full".into()));
    assert_eq!(s.systems.list.len(), 4);
    assert!(s.guided.existing.is_none());
    assert_eq!(s.guided.draft.name.text, "Unsaved lab");
    assert!(s.guided.prep_error.contains("disk full"));
    s.guided.opened_for.clear();
    open(&mut s);
    let old = target(&s).unwrap().clone();
    s.guided.draft.vehicle = 1;
    lifecycle::save_with(&mut s, |_, _| Err("read only".into()));
    assert_eq!(target(&s), Some(&old));
    assert_eq!(s.guided.draft.vehicle, 1);
    assert!(!saved(&s));
}

#[test]
fn configuration_change_invalidates_prepared_but_rename_does_not() {
    let mut s = State::new();
    s.systems.list[0].prepared = true;
    open(&mut s);
    s.guided.draft.name = TextEdit::new("Renamed");
    save_draft(&mut s);
    assert!(target(&s).unwrap().prepared);
    s.guided.draft.qgc = false;
    save_draft(&mut s);
    assert!(!target(&s).unwrap().prepared);
}

#[test]
fn switching_active_system_loads_its_saved_configuration() {
    let mut s = State::new();
    open(&mut s);
    s.guided.draft.name = TextEdit::new("Unfinished");
    s.select_starter(systems::PHYSICAL);
    open(&mut s);
    assert_eq!(s.guided.existing.as_deref(), Some("physical-vehicle"));
    assert_eq!(s.guided.draft.vehicle, 1);
}

#[test]
fn all_simulator_variants_compute_requirements_from_the_draft() {
    for (pilot, world, required) in [
        (systems::adapter::AutopilotKind::Px4, "internal", "px4"),
        (systems::adapter::AutopilotKind::Px4, "gazebo", "gazebo"),
        (systems::adapter::AutopilotKind::Px4, "jmavsim", "jmavsim"),
        (
            systems::adapter::AutopilotKind::ArduPilot,
            "internal",
            "ardupilot",
        ),
    ] {
        let mut s = State::new();
        new_system(&mut s);
        s.guided.draft.autopilot = pilot;
        s.guided.draft.world = world.into();
        assert!(preset(&s).recipes.contains(&required.to_string()));
        assert!(s.guided.existing.is_none());
    }
}
#[test]
fn editing_vehicle_keeps_identity_and_preserves_user_name() {
    let mut s = State::new();
    open(&mut s);
    let id = s.systems.active_id().to_string();
    s.guided.draft.vehicle = 1;
    s.guided.draft.name = TextEdit::new("Bench quad");
    save_draft(&mut s);
    assert_eq!(s.guided.existing.as_deref(), Some(id.as_str()));
    assert_eq!(s.systems.list.len(), 4);
    assert!(target(&s).unwrap().is_physical());
    assert_eq!(target(&s).unwrap().name, "Bench quad");
}
#[test]
fn configuration_changes_clear_evidence_consent_and_pending_checks() {
    let mut s = State::new();
    open(&mut s);
    s.guided.consent = true;
    s.guided.done = true;
    s.guided.evidence.push("old".into());
    s.guided.check = Check::Tools;
    s.setup.pending = true;
    s.setup.queue.push(0);
    s.guided.draft.world = "jmavsim".into();
    reconcile(&mut s);
    assert!(!s.guided.consent && !s.guided.done);
    assert!(s.guided.evidence.is_empty() && s.setup.queue.is_empty());
    assert!(s.setup.discard_detect);
    assert!(!saved(&s));
}
#[test]
fn rename_preserves_verification_but_requires_an_explicit_save() {
    let mut s = State::new();
    open(&mut s);
    s.guided.evidence.push("fresh heartbeat".into());
    s.guided.consent = true;
    s.guided.draft.name = TextEdit::new("Renamed");
    assert!(!reconcile(&mut s));
    assert!(s.guided.consent && !s.guided.evidence.is_empty());
    assert!(!saved(&s));
    save_draft(&mut s);
    assert!(saved(&s));
}
#[test]
fn back_reopen_and_restart_have_predictable_identity() {
    let mut s = State::new();
    open(&mut s);
    s.guided.draft.world = "jmavsim".into();
    s.guided.draft.name = TextEdit::new("Unfinished");
    go(&mut s, 0);
    s.navigate(ui::Page::Data);
    open(&mut s);
    assert_eq!(s.guided.draft.name.text, "Unfinished");
    assert_eq!(s.guided.draft.world, "jmavsim");
    restart(&mut s);
    assert_eq!(s.guided.draft.world, "internal");
    new_system(&mut s);
    s.guided.draft.vehicle = 2;
    go(&mut s, 1);
    assert_eq!(s.guided.chapter, 2);
    restart(&mut s);
    assert!(s.guided.existing.is_none());
    assert_eq!(s.guided.draft.vehicle, 0);
}
#[test]
fn temporarily_switching_vehicle_preserves_world_and_endpoint_fields() {
    let mut s = State::new();
    new_system(&mut s);
    s.guided.draft.world = "gazebo".into();
    s.guided.draft.endpoint = TextEdit::new("serial:COM4:115200");
    s.guided.draft.vehicle = 1;
    reconcile(&mut s);
    s.guided.draft.vehicle = 0;
    reconcile(&mut s);
    assert_eq!(s.guided.draft.world, "gazebo");
    assert_eq!(s.guided.draft.endpoint.text, "serial:COM4:115200");
}
#[test]
fn running_vehicle_blocks_reconfiguration_without_stopping_processes() {
    let mut s = State::new();
    open(&mut s);
    s.systems.claim("sih");
    s.status.insert("sih".into(), "Running".into());
    s.guided.draft.vehicle = 1;
    save_draft(&mut s);
    assert!(target(&s).unwrap().simulated());
    assert!(s.guided.prep_error.contains("Stop"));
    assert_eq!(s.status["sih"], "Running");
}
#[test]
fn missing_target_does_not_recreate_a_deleted_system() {
    let mut s = State::new();
    s.select_starter(systems::PHYSICAL);
    open(&mut s);
    s.systems.remove("physical-vehicle").unwrap();
    save_draft(&mut s);
    assert!(s.guided.prep_error.contains("no longer exists"));
    assert_eq!(s.systems.list.len(), 3);
}
#[test]
fn preparation_requires_saved_verified_configuration() {
    let mut s = State::new();
    new_system(&mut s);
    assert!(!mark_prepared(&mut s));
    save_draft(&mut s);
    assert!(!mark_prepared(&mut s));
    ready_checks(&mut s);
    assert!(mark_prepared(&mut s));
    assert!(target(&s).unwrap().prepared);
}
#[test]
fn first_run_tasks_follow_selected_listeners_and_not_historical_template() {
    let mut s = State::new();
    open(&mut s);
    s.guided.draft.qgc = false;
    s.guided.draft.ros_agent = true;
    s.guided.draft.recorder = false;
    save_draft(&mut s);
    let list = tasks(&s);
    assert!(list.contains(&Task::Ros));
    assert!(!list.contains(&Task::GroundStation));
    assert!(!list.contains(&Task::Record));
    assert!(!list.contains(&Task::Infer));
    assert_eq!(target(&s).unwrap().id, "local-simulation");
}
#[test]
fn replay_without_inference_has_only_recording_and_playback_tasks() {
    let mut s = State::new();
    new_system(&mut s);
    s.guided.draft.vehicle = 2;
    s.guided.draft.inference = false;
    s.guided.draft.qgc = false;
    save_draft(&mut s);
    assert_eq!(tasks(&s), [Task::Recording, Task::Playback]);
    assert!(!preset(&s).recipes.contains(&"onnx".to_string()));
    assert!(preset(&s).recipes.contains(&"ros2".to_string()));
}
#[test]
fn first_run_needs_its_saved_system_active() {
    let mut s = first_run(systems::ROS, Task::Ros);
    s.select_starter(systems::SIMULATION);
    run::act(&mut s);
    assert!(!s.guided.awaiting);
    assert!(s.guided.prep_error.contains("active"));
}
#[test]
fn ros_publishers_need_adapter_readiness_and_application_topics() {
    let mut s = first_run(systems::ROS, Task::Ros);
    s.status.insert("gazebo".into(), "Running".into());
    s.status.insert("dds".into(), "Running".into());
    s.guided.awaiting = true;
    s.recorder.discovered = vec![Topic {
        name: "/fmu/out/vehicle_status".into(),
        ..Default::default()
    }];
    assert!(run::evidence(&s).is_some());
    s.status.insert("gazebo".into(), "Exited 1".into());
    assert!(run::evidence(&s).is_none());
    s.status.insert("gazebo".into(), "Running".into());
    s.recorder.discovered[0].name = "/rosout".into();
    assert!(run::evidence(&s).is_none());
}
#[test]
fn recording_finalize_requires_finalized_nonempty_data() {
    let mut s = first_run(systems::ROS, Task::Finalize);
    s.guided.awaiting = true;
    s.recorder.selected = Some("flight".into());
    s.guided.run_recording = Some("flight".into());
    s.recorder.sessions = vec![Session {
        id: "flight".into(),
        status: "Recording".into(),
        messages: 40,
        ..Default::default()
    }];
    assert!(run::evidence(&s).is_none());
    s.recorder.sessions[0].status = "Finalized".into();
    s.recorder.sessions[0].messages = 0;
    assert!(run::evidence(&s).is_none());
    s.recorder.sessions[0].messages = 40;
    assert!(run::evidence(&s).is_some());
}
#[test]
fn a_stale_process_exit_is_not_first_run_evidence() {
    let mut s = first_run(systems::SIMULATION, Task::GroundStation);
    s.status.insert("qgc".into(), "Exited 0".into());
    s.guided.awaiting = true;
    assert!(run::evidence(&s).is_none());
    s.guided.launched = Some("qgc".into());
    assert!(run::evidence(&s).is_some());
}
#[test]
fn legacy_installation_ownership_requires_a_unique_match() {
    let mut s = State::new();
    s.setup.workflow = template_preset(systems::ROS);
    setup::batch::preview(&mut s);
    assert_eq!(batch_system(&s).as_deref(), Some("ros-sensor-capture"));
    s.systems.duplicate("ros-sensor-capture");
    assert!(batch_system(&s).is_none());
    s.setup.batch.as_mut().unwrap().system = Some("ros-sensor-capture".into());
    assert_eq!(batch_system(&s).as_deref(), Some("ros-sensor-capture"));
    open(&mut s);
    assert_eq!(s.guided.existing.as_deref(), Some("ros-sensor-capture"));
    assert_eq!(s.guided.chapter, REVIEW);
}
