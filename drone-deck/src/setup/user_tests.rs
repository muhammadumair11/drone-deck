use super::*;

fn scan(s: &mut State, user: serde_json::Value) {
    apply_event(
        s,
        json!({"ok":true,"action":"scan","data":{
            "build":26100,"windows":"Windows fixture","architecture":"64-bit","eligible":true,
            "virtualization":"Available","ram_gib":16.,"gpu":"Fixture","wsl_ready":true,
            "distros":[{"name":"Ubuntu-24.04","state":"Running","version":2,"regular_user":user}]}}),
    );
}

fn select(s: &mut State, id: &str) {
    let index = s.setup.recipe_index(id).unwrap();
    choose(s, index);
}

#[test]
fn a_distribution_with_only_root_is_missing_its_user_and_blocks_linux_tools() {
    let mut s = State::new();
    scan(&mut s, json!(false));
    assert!(user::missing(&s));
    assert!(user::typed_missing(&s.setup));
    select(&mut s, "px4");
    assert!(user::blocks_install(&s));
    s.headless = false;
    request(&mut s, "start");
    assert_eq!(s.setup.message, user::BLOCKED);
    assert!(!s.setup.pending, "no helper is started");
    assert!(user::BLOCKED.contains("Initialize Linux user"));
}

#[test]
fn windows_tools_and_the_distribution_itself_are_never_blocked_by_the_missing_user() {
    let mut s = State::new();
    scan(&mut s, json!(false));
    for id in ["wsl", "distro", "qgc"] {
        select(&mut s, id);
        assert!(!user::blocks_install(&s), "{id} is a Windows-side step");
    }
}

#[test]
fn an_unknown_or_present_user_blocks_nothing() {
    let mut s = State::new();
    select(&mut s, "px4");
    scan(&mut s, json!(null));
    assert!(!user::missing(&s), "a failed probe is unknown, not blocked");
    scan(&mut s, json!(true));
    assert!(!user::missing(&s));
    assert!(!user::blocks_install(&s));
}

#[test]
fn a_finished_wsl_or_distribution_job_asks_for_a_rescan() {
    let mut r = Setup::default();
    for (tool, status, wanted) in [
        ("wsl", "Needs restart", true),
        ("wsl", "Ready", true),
        ("distro", "Needs input", true),
        ("distro", "Failed", true),
        ("distro", "Downloading", false),
        ("px4", "Ready", false),
    ] {
        r.rescan_wanted = false;
        user::job_changed(&mut r, tool, status);
        assert_eq!(r.rescan_wanted, wanted, "{tool} {status}");
    }
}

#[test]
fn watching_stops_once_the_user_exists() {
    let mut s = State::new();
    scan(&mut s, json!(false));
    user::begin_watch(&mut s.setup);
    user::saw_host(&mut s.setup);
    assert!(s.setup.user_watch.is_some(), "still only root");
    scan(&mut s, json!(true));
    assert!(s.setup.user_watch.is_none());
}

#[test]
fn the_rescan_waits_for_a_quiet_moment() {
    let mut s = State::new();
    s.headless = false;
    s.setup.rescan_wanted = true;
    s.setup.pending = true;
    assert!(!user::tick(&mut s), "nothing starts while a request is out");
    assert!(s.setup.rescan_wanted, "the wish is kept for later");
    s.headless = true;
    s.setup.pending = false;
    assert!(!user::tick(&mut s), "previews never run helpers");
}

#[test]
fn a_background_rescan_never_starves_the_wizards_own_check() {
    let mut s = State::new();
    s.setup.rescan_wanted = true;
    s.guided.check = crate::guided::Check::Host;
    assert!(
        !user::due(&mut s),
        "the wizard's scan has the host to itself"
    );
    assert!(s.setup.rescan_wanted, "the wish is kept for later");
    s.guided.check = crate::guided::Check::Idle;
    assert!(user::due(&mut s));
    assert!(!s.setup.rescan_wanted, "and is spent once it starts");
}

#[test]
fn the_watch_waits_between_scans_and_counts_from_the_last_result() {
    let mut s = State::new();
    scan(&mut s, json!(false));
    s.setup.user_watch = Some((
        Instant::now(),
        Instant::now() - std::time::Duration::from_secs(10),
    ));
    assert!(user::due(&mut s), "long since the last scan");
    assert!(
        !user::due(&mut s),
        "a scan just started; the next one waits"
    );
    // The scan's result restarts the wait, even if the scan was slow.
    s.setup.user_watch = Some((
        Instant::now() - std::time::Duration::from_secs(60),
        Instant::now() - std::time::Duration::from_secs(10),
    ));
    user::saw_host(&mut s.setup);
    assert!(
        !user::due(&mut s),
        "counted from the result, not the request"
    );
}
