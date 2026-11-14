use super::*;
#[test]
fn failed_workflow_step_pauses_without_starting_later_installations() {
    let mut s = State::new();
    let before = serde_json::to_string(&s.config).unwrap();
    s.setup.batch = Some(Batch {
        remaining: vec![2],
        waiting: Some("fixture".into()),
        distro: s.setup.distro.text.clone(),
    });
    s.setup.jobs.push(Job {
        id: "fixture".into(),
        tool: "PX4".into(),
        status: "Failed".into(),
        message: "fixture failure".into(),
        ..Default::default()
    });
    assert!(poll(&mut s));
    assert!(s.setup.batch.is_none());
    assert!(s.setup.batch_message.contains("paused at PX4"));
    assert_eq!(before, serde_json::to_string(&s.config).unwrap());
    assert!(!s.setup.pending);
}
#[test]
fn workflow_review_is_noninstalling_in_preview() {
    let mut s = State::new();
    begin(&mut s);
    assert!(s.setup.batch.is_none());
    assert!(!s.setup.pending);
}
#[test]
fn interrupted_handoff_does_not_duplicate_an_installation() {
    let mut s = State::new();
    s.setup.batch = Some(Batch {
        remaining: vec![2],
        waiting: Some("starting:px4".into()),
        distro: s.setup.distro.text.clone(),
    });
    assert!(poll(&mut s));
    assert!(s.setup.batch.is_none());
    assert!(!s.setup.pending);
    assert!(s.setup.batch_message.contains("interrupted job handoff"));
}
#[test]
fn ready_job_waits_for_configuration_before_next_component() {
    let mut s = State::new();
    s.setup.batch = Some(Batch {
        remaining: vec![2],
        waiting: Some("fixture".into()),
        distro: s.setup.distro.text.clone(),
    });
    s.setup.jobs.push(Job {
        id: "fixture".into(),
        status: "Ready".into(),
        configure_after: true,
        ..Default::default()
    });
    assert!(!poll(&mut s));
    assert!(s.setup.batch.is_some());
    assert!(!s.setup.pending);
}
#[test]
fn checkpoints_name_recipes_and_presets_and_refuse_unknown_ones() {
    let mut s = State::new();
    let position = |s: &State, id: &str| s.setup.recipes.iter().position(|r| r.id == id).unwrap();
    s.setup.workflow = crate::guided::template_preset(crate::systems::ROS);
    s.setup.batch = Some(Batch {
        remaining: vec![position(&s, "ros-gz"), position(&s, "ros-workspace")],
        waiting: Some("20260101T120000-0123abcd".into()),
        distro: "Ubuntu-24.04".into(),
    });
    let saved = checkpoint(&s);
    assert_eq!(
        saved["batch"]["remaining"],
        json!(["ros-gz", "ros-workspace"])
    );
    assert_eq!(saved["workflow"], "PX4 + Gazebo with ROS 2 and recordings");
    assert_eq!(saved["recipes"][1], "gazebo");

    let mut back = State::new();
    resume(&mut back, &saved);
    let batch = back.setup.batch.as_ref().expect("resumed");
    assert_eq!(
        batch.remaining(),
        [position(&back, "ros-gz"), position(&back, "ros-workspace")]
    );
    assert_eq!(batch.waiting(), Some("20260101T120000-0123abcd"));
    assert_eq!(back.setup.workflow, s.setup.workflow);

    // A recipe the catalog no longer has (queued or in the preset), an unknown preset name
    // without a recipe list and the older format that kept catalog positions all ask for a
    // review and resume nothing.
    let mut unknown = saved.clone();
    unknown["batch"]["remaining"] = json!(["warp-drive"]);
    let mut unknown_preset = saved.clone();
    unknown_preset["recipes"] = json!(["px4", "warp-drive"]);
    let mut renamed = saved.clone();
    renamed["workflow"] = json!("Lightweight visual");
    renamed.as_object_mut().unwrap().remove("recipes");
    let mut positions = saved.clone();
    positions["batch"]["remaining"] = json!([11]);
    positions["workflow"] = json!(1);
    for value in [unknown, unknown_preset, renamed, positions] {
        let mut s = State::new();
        resume(&mut s, &value);
        assert!(s.setup.batch.is_none(), "{value}");
        assert!(s.setup.batch_message.contains("Recheck/review"), "{value}");
    }
}
