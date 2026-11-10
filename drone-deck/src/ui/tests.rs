use super::*;
use rune::{Env, Runtime};

fn texts(s: State, page: Page, scroll_to: &str) -> Vec<String> {
    let mut s = s;
    s.navigate(page);
    let mut rt = Runtime::new(
        s,
        view,
        (1440., 900.),
        Env {
            scale: 1.,
            system_dark: true,
        },
    );
    let target = scroll_to.to_string();
    rt.update(move |_, tx| tx.scroll_into_view(&target));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    rt.elements().into_iter().filter_map(|e| e.text).collect()
}

/// A physical system with QGroundControl enabled.
fn bridged() -> State {
    let mut s = State::new();
    assert!(s.select_system(systems::PHYSICAL));
    let id = s.systems.active_id().to_string();
    s.systems.get_mut(&id).unwrap().consumers.qgc = true;
    s
}

#[test]
fn the_rail_and_the_top_bar_name_the_environment_alike() {
    let s = State::new();
    assert!(s.setup.host.is_none());
    assert_eq!(env_label(&s, false), "Environment not checked");
    assert_eq!(env_label(&s, true), "Environment not checked");
    let t = texts(s, Page::System, "environment");
    assert!(!t.iter().any(|x| x.contains("not ready")), "{t:?}");
    let mut s = State::new();
    assert!(s.select_system(systems::PHYSICAL));
    assert_eq!(env_label(&s, false), "Windows native");
    assert_eq!(env_label(&s, true), "Windows native / no WSL required");
}

/// Stop session says that it closes the vehicle link, first and with the failsafe risk when
/// the link runs to a physical vehicle.
#[test]
fn stop_session_names_the_vehicle_link_it_closes() {
    let s = State::new();
    let body = stop_body(&s);
    assert!(
        body.starts_with("It stops the work of every system"),
        "{body}"
    );
    assert!(!body.contains("vehicle link"), "{body}");

    let mut s = bridged();
    let id = s.systems.active_id().to_string();
    vehicle::preview(&mut s, &id, true, false);
    s.status.insert("qgc".into(), "Running".into());
    let name = s.systems.current().label().to_string();
    let body = stop_body(&s);
    assert!(
        body.starts_with(&format!(
            "It closes the vehicle link of {name}, and QGroundControl, which Drone Deck started."
        )),
        "{body}"
    );
    assert!(body.contains("link-loss failsafe"), "{body}");

    let mut s = State::new();
    let id = s.systems.active_id().to_string();
    vehicle::preview(&mut s, &id, true, true);
    let body = stop_body(&s);
    assert!(
        body.starts_with("It stops the work of every system"),
        "{body}"
    );
    assert!(
        body.contains("It also closes the vehicle link of"),
        "{body}"
    );
    assert!(!body.contains("failsafe"), "{body}");
}

/// Wherever a physical system's QGroundControl or scripts consumer is enabled or shown, the
/// page says the bridge is two-way and can command the vehicle.
#[test]
fn a_bridged_physical_system_never_reads_as_read_only() {
    let mut s = bridged();
    let id = s.systems.active_id().to_string();
    vehicle::preview(&mut s, &id, true, false);
    assert_eq!(session_label(&s), "vehicle link (bridged)");

    let t = texts(bridged(), Page::System, "hardware-peers");
    assert!(t.iter().any(|x| x == "Connect (bridged)"), "{t:?}");
    assert!(
        t.iter()
            .any(|x| x.starts_with("Once connected, the link forwards to QGroundControl")),
        "the bridge is named before connecting: {t:?}"
    );
    assert!(
        t.iter().any(|x| x.contains("can command the vehicle")),
        "topology caveat and branch: {t:?}"
    );
    assert!(!t.iter().any(|x| x.contains("read-only")), "{t:?}");

    let mut s = bridged();
    s.navigate(Page::Settings);
    let sys = s.systems.current().clone();
    s.systems.editor = Some(crate::systems::Draft::of(&sys));
    let t = texts(s, Page::Settings, "system-bridge-warning");
    assert!(
        t.iter()
            .any(|x| x.contains("can command this vehicle and write parameters")),
        "{t:?}"
    );
    assert!(
        t.iter()
            .any(|x| x == "User scripts / MAVLink over UDP (two-way)"),
        "{t:?}"
    );
}
