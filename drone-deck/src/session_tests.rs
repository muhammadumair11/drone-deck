use crate::*;
#[test]
fn workspace_does_not_start_later_steps_before_readiness() {
    let mut s = State::new();
    assert!(s.bundle(["sih", "router", "link", "qgc"]));
    session::poll(&mut s);
    s.status.insert("sih".into(), "Running".into());
    session::poll(&mut s);
    assert_eq!(s.session.as_ref().map(|l| l.current()), Some("sih"));
    s.readiness.insert("sih".into(), "Ready".into());
    session::poll(&mut s);
    assert_eq!(s.session.as_ref().map(|l| l.current()), Some("router"));
    // Previews have no Linux agent, so the router stage fails with its reason and the link
    // and QGroundControl are never started.
    session::poll(&mut s);
    assert!(s.session.is_none());
    assert!(s.notice.contains("stopped at router"), "{}", s.notice);
    assert!(s.vehicle.link.is_none());
    assert!(!s.status.contains_key("qgc"));
}

/// The launch's seconds count is drawn on the System page only, so it redraws nothing on
/// other pages.
#[test]
fn the_launch_wait_ticks_only_on_the_system_page() {
    let mut s = State::new();
    s.session = Some(session::Launch::new(["dds"]));
    assert!(session::poll(&mut s), "the stage is requested");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    s.page = ui::Page::Activity;
    assert!(!session::poll(&mut s), "another page: no tick");
    std::thread::sleep(std::time::Duration::from_millis(1000));
    s.page = ui::Page::System;
    assert!(session::poll(&mut s), "the System page ticks");
    assert!(!session::poll(&mut s), "same second");
}

/// PX4 can fail readiness ("did not publish vehicle status") while its process still runs; the
/// launch stops it, or the next Start would be refused because a px4 is already running.
#[test]
fn a_stage_that_fails_readiness_while_running_is_stopped() {
    let mut s = State::new();
    let (backend, requests) = backend::Backend::capture();
    s.backend = Some(backend);
    s.session = Some(session::Launch::new(["sih", "qgc"]));
    session::poll(&mut s);
    s.status.insert("sih".into(), "Running".into());
    s.readiness.insert("sih".into(), "Failed".into());
    session::poll(&mut s);
    assert!(s.session.is_none());
    let stops: Vec<_> = requests
        .try_iter()
        .filter_map(|r| match r {
            backend::Request::Stop(id) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(stops, ["sih"]);
    assert!(s.notice.contains("stopped sih"), "{}", s.notice);
    assert!(!s.status.contains_key("qgc"));
}

#[test]
fn a_failed_profile_stage_stops_the_launch() {
    let mut s = State::new();
    let (backend, requests) = backend::Backend::capture();
    s.backend = Some(backend);
    s.session = Some(session::Launch::new(["dds", "qgc"]));
    session::poll(&mut s);
    s.status.insert("dds".into(), "Exited 2".into());
    session::poll(&mut s);
    assert!(s.session.is_none());
    assert!(
        s.notice.contains("stopped at dds: Exited 2."),
        "{}",
        s.notice
    );
    assert!(!s.status.contains_key("qgc"));
    // A process that already exited has nothing to stop.
    assert!(
        requests
            .try_iter()
            .all(|r| !matches!(r, backend::Request::Stop(_)))
    );
}
