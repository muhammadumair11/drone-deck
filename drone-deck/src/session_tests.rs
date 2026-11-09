use crate::*;
#[test]
fn workspace_does_not_start_later_steps_before_readiness() {
    let mut s = State::new();
    s.bundle();
    session::poll(&mut s);
    s.status.insert("sih".into(), "Running".into());
    session::poll(&mut s);
    assert!(!s.status.contains_key("relay"));
    s.readiness.insert("sih".into(), "Ready".into());
    session::poll(&mut s);
    session::poll(&mut s);
    s.status.insert("relay".into(), "Exited 2".into());
    session::poll(&mut s);
    assert!(s.session.is_none());
    assert!(s.notice.contains("stopped at relay"));
    assert!(!s.status.contains_key("jmavsim"));
}
