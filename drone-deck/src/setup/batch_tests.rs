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
