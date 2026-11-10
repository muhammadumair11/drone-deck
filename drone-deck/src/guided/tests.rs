use super::*;
use crate::recordings::{Session, Topic};

/// Guided setup at First run for `kind`, with `kind` as the active system.
fn first_run(kind: usize, task: usize) -> State {
    let mut s = State::new();
    assert!(s.select_system(kind));
    s.guided.choice = kind;
    s.guided.base = Some(s.systems.active_id().to_string());
    s.guided.existing = s.guided.base.clone();
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
fn preparing_while_work_runs_switches_without_stopping_it() {
    let mut s = State::new();
    s.systems.claim("sih");
    s.status.insert("sih".into(), "Running".into());
    s.page = ui::Page::System;
    open(&mut s);
    assert_eq!(s.guided.existing.as_deref(), Some("local-simulation"));
    set_choice(&mut s, systems::ROS);
    assert!(
        s.guided.existing.is_none(),
        "another template prepares a new system"
    );
    s.guided.name = TextEdit::new("Lab");
    open_workspace(&mut s);
    assert_eq!(s.page, ui::Page::System);
    let lab = s.systems.get("lab").expect("created from the ROS template");
    assert!(lab.prepared);
    assert_eq!(lab.template, "ros-sensor-capture");
    assert_eq!(s.systems.active_id(), "lab");
    assert_eq!(s.status.get("sih").map(String::as_str), Some("Running"));
    assert_eq!(s.systems.owner("sih"), Some("local-simulation"));
    assert!(
        !s.systems.get("ros-sensor-capture").unwrap().prepared,
        "the template system was not touched"
    );
}

#[test]
fn first_run_needs_its_system_active() {
    let mut s = first_run(systems::ROS, 0);
    assert!(s.select_system(systems::SIMULATION));
    run::act(&mut s);
    assert!(!s.guided.awaiting);
    assert!(s.guided.prep_error.contains("is not the active system"));
}

#[test]
fn reopening_for_the_same_template_updates_the_existing_system() {
    let mut s = State::new();
    assert!(s.select_system(systems::PHYSICAL));
    open(&mut s);
    assert_eq!(s.guided.choice, systems::PHYSICAL);
    set_choice(&mut s, systems::AI);
    set_choice(&mut s, systems::PHYSICAL);
    assert_eq!(s.guided.existing.as_deref(), Some("physical-vehicle"));
    assert_eq!(preset(&s), 4);
    s.guided.name = TextEdit::new("Bench quad");
    mark_prepared(&mut s);
    assert_eq!(s.systems.list.len(), 4, "no new system");
    let sys = s.systems.current();
    assert_eq!((sys.name.as_str(), sys.prepared), ("Bench quad", true));
}

#[test]
fn the_physical_endpoint_from_first_run_is_saved_with_the_system() {
    let mut s = first_run(systems::PHYSICAL, 0);
    s.hardware.endpoint = TextEdit::new("udp:0.0.0.0:14550");
    run::act(&mut s);
    assert!(s.guided.awaiting);
    assert_eq!(
        s.systems.current().source,
        systems::Source::Physical {
            endpoint: "udp:0.0.0.0:14550".into()
        }
    );
}

#[test]
fn ros_publishers_need_application_topics_from_a_ready_px4_and_dds_agent() {
    let mut s = first_run(systems::ROS, 0);
    // The template runs Gazebo: a running Gazebo vehicle and DDS agent are enough.
    s.status.insert("gazebo".into(), "Running".into());
    s.status.insert("dds".into(), "Running".into());
    s.guided.awaiting = true;
    s.recorder.discovered = vec![topic("/fmu/out/vehicle_status")];
    assert!(run::evidence(&s).is_some());
    s.status.insert("gazebo".into(), "Exited 1".into());
    assert!(run::evidence(&s).is_none(), "Gazebo stopped");
    s.status.clear();

    // A migrated 0.8 ROS system runs PX4 SIH, which must also report ready.
    let id = s.systems.active_id().to_string();
    s.systems.get_mut(&id).unwrap().source = systems::Source::Sih;
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

#[test]
fn the_ai_outputs_step_counts_completed_inference_runs_from_pipelines() {
    use crate::studio::pipelines::{Job, JobKind};
    let mut s = first_run(systems::AI, 2);
    let pipelines = systems::component("pipelines").unwrap();
    assert_eq!(systems::state(&s, pipelines).0, "Runs not listed yet");
    run::act(&mut s);
    assert!(s.guided.awaiting);
    assert_eq!(
        (s.page, s.ui.data_tab),
        (ui::Page::Data, 2),
        "opens Pipelines"
    );
    assert!(run::evidence(&s).is_none(), "nothing listed yet");

    crate::studio::pipelines::preview(&mut s);
    assert_eq!(
        systems::state(&s, pipelines).0,
        "1 inference run / 1 extraction"
    );
    let found = run::evidence(&s).expect("a completed inference run is listed");
    assert!(
        found.contains("1 completed inference run(s)") && found.contains("612 detection"),
        "{found}"
    );
    s.studio.pipes.runs[0].detections = None;
    assert!(
        run::evidence(&s)
            .unwrap()
            .contains("no detection count recorded")
    );

    s.studio.pipes.runs[0].status = "Failed".into();
    assert!(run::evidence(&s).is_none(), "a failed run is no output");
    s.studio.pipes.runs[0].status = "Completed".into();
    s.studio.pipes.job = Some(Job::fake(JobKind::Infer));
    assert!(run::evidence(&s).is_none(), "waits for the running job");
    assert_eq!(
        systems::state(&s, pipelines).0,
        "Inference on replay running"
    );
    assert!(run::observed(&s).starts_with("Pipelines: Inference on replay running"));
    s.studio.pipes.job = None;
    s.guided.awaiting = false;
    assert!(run::evidence(&s).is_none(), "only while the step waits");
}
