//! The vehicle panels: instrument geometry, the position trace, pre-arm readiness, the
//! takeoff field, the console history, the gate's disabled state, the Arm confirmation (a
//! cancel sends nothing; a confirm sends one COMMAND_LONG to a fake vehicle), and compact
//! layouts.
use super::actions::takeoff_alt;
use super::console::{history_key, tail};
use super::health::{PREARM_BIT, Readiness, readiness};
use super::instruments::{clip, ground, horizon_axes};
use super::track::{Frame, Track, thin};
use super::*;
use crate::mavlink::fake_vehicle::{FakeVehicle, is_home_request, wait_for};
use crate::mavlink::{Msg, StatusText, dialect};
use crate::{setup, vehicle, view};
use rune::{Env, Key, KeyEvent, Mods, Runtime};
use std::time::{Duration, SystemTime};

const LIMIT: Duration = Duration::from_secs(10);

fn runtime(state: State, size: (f32, f32)) -> Runtime<State> {
    let mut rt = Runtime::new(
        state,
        view,
        size,
        Env {
            system_dark: true,
            scale: 1.,
        },
    );
    rt.frame(0.);
    rt
}

fn tap(rt: &mut Runtime<State>, key: &str) {
    let target = key.to_string();
    rt.update(move |_, tx| tx.scroll_into_view(&target));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("{key} is not on screen"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
}

fn assert_inside(rt: &mut Runtime<State>, key: &str, w: f32, context: &str) {
    let target = key.to_string();
    rt.update(move |_, tx| tx.scroll_into_view(&target));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    let (r, _) = rt
        .presented(key)
        .unwrap_or_else(|| panic!("Missing {key} ({context})"));
    assert!(
        r[0] >= -1. && r[0] + r[2] <= w + 1.,
        "{key} overflows width {w} ({context}): {r:?}"
    );
}

fn scene(f: fn(&mut State)) -> State {
    let mut s = State::new();
    f(&mut s);
    s
}

fn area(poly: &[[f32; 2]]) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        .abs()
        / 2.
}

#[test]
fn the_horizon_rolls_and_pitches_the_right_way() {
    let size = 180f32;
    let half = std::f32::consts::PI * (size / 2. - 2.).powi(2) / 2.;
    let level = ground(0., 0., size);
    assert!(
        (area(&level) - half).abs() / half < 0.02,
        "level: half the dial is ground"
    );
    assert!(level.iter().all(|p| p[1] >= size / 2. - 0.01));
    // Nose up: the horizon drops, less ground shows.
    assert!(area(&ground(0., 15., size)) < area(&level) * 0.8);
    assert!(area(&ground(0., -15., size)) > area(&level) * 1.2);
    // Rolling right raises the right end of the horizon: a point right of centre, just above
    // it, is ground.
    let rolled = ground(20., 0., size);
    let probe = [size / 2. + 60., size / 2. - 8.];
    assert!(clip(&rolled, |q| q[0] - probe[0]).len() >= 3);
    let (along, up) = horizon_axes(20.);
    assert!(along[1] < 0. && up[1] < 0.);
    // A half-plane clip keeps the right side of a square.
    let square = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
    assert!((area(&clip(&square, |q| 5. - q[0])) - 50.).abs() < 1e-3);
}

#[test]
fn the_trace_starts_at_the_first_fix_and_ignores_jitter() {
    let mut t = Track::default();
    assert!(!t.push_fix(0., 0.), "0,0 is no fix");
    assert!(t.push_fix(47.0, 8.0));
    assert!(!t.push_fix(47.0, 8.0 + 1e-7), "under 25 cm is jitter");
    // About 11 m north.
    assert!(t.push_fix(47.0001, 8.0));
    let p = t.local(47.0001, 8.0).unwrap();
    assert!(p[0].abs() < 0.01 && (p[1] - 11.12).abs() < 0.05, "{p:?}");
    assert_eq!(t.len(), 2);
    for i in 0..(track::TRACK_POINTS + 10) {
        t.push_fix(47.0 + i as f64 * 1e-5, 8.0);
    }
    assert_eq!(t.len(), track::TRACK_POINTS, "bounded");
    t.clear();
    assert!(t.is_empty() && t.local(47.0, 8.0).is_none());

    let f = Frame::fit(&[[0., 0.], [100., 40.]], 400., 200.);
    let a = f.to_screen([0., 0.]);
    let b = f.to_screen([100., 40.]);
    assert!(a[0] < b[0] && a[1] > b[1], "north is up");
    assert!(a[0] >= 0. && b[0] <= 400. && b[1] >= 0. && a[1] <= 200.);
    assert!([10., 20., 50.].contains(&f.scale_bar()));
    let thinned = thin((0..100).map(|i| [i as f32 * 0.5, 0.]));
    assert!(thinned.len() < 20 && thinned.last() == Some(&[49.5, 0.]));
}

#[test]
fn ready_to_arm_comes_only_from_the_vehicle() {
    let snap = vehicle::fake_snapshot(true);
    let mut v = snap.vehicle.unwrap();
    v.armed = false;
    let text = |severity, t: &str| StatusText {
        severity,
        text: t.into(),
        at: SystemTime::now(),
    };
    assert_eq!(readiness(&v, &[]), Readiness::NotReported);
    let info = text(6, "Ready for takeoff!");
    assert_eq!(readiness(&v, &[&info]), Readiness::NotReported);
    let fail = text(4, "Preflight Fail: Accel uncalibrated");
    assert_eq!(
        readiness(&v, &[&fail]),
        Readiness::NotReady(Some("Preflight Fail: Accel uncalibrated".into()))
    );
    v.sensors_present |= PREARM_BIT;
    v.sensors_enabled |= PREARM_BIT;
    assert_eq!(
        readiness(&v, &[]),
        Readiness::NotReady(None),
        "pre-arm bit present but unhealthy"
    );
    v.sensors_health |= PREARM_BIT;
    assert_eq!(readiness(&v, &[]), Readiness::Ready);
    v.armed = true;
    assert_eq!(readiness(&v, &[]), Readiness::Armed);
}

#[test]
fn stale_data_is_labelled_not_live() {
    let snap = vehicle::fake_snapshot(false);
    let (line, fresh) = freshness(&snap, snap.vehicle.as_ref().unwrap());
    assert!(!fresh && line.contains("No heartbeat for 9 s"), "{line}");
    let snap = vehicle::fake_snapshot(true);
    let (line, fresh) = freshness(&snap, snap.vehicle.as_ref().unwrap());
    assert!(fresh && line.starts_with("Heartbeat fresh"), "{line}");
    // A stale snapshot adds nothing to the trace.
    let mut t = Track::default();
    assert!(!t.push(snap.vehicle.as_ref().unwrap(), false));
    assert!(t.push(snap.vehicle.as_ref().unwrap(), true));
}

#[test]
fn takeoff_altitude_and_console_helpers() {
    assert_eq!(takeoff_alt(&TextEdit::new("10")), Ok(10.));
    assert_eq!(takeoff_alt(&TextEdit::new(" 25 m")), Ok(25.));
    for bad in ["", "0", "abc", "250", "-5"] {
        assert!(takeoff_alt(&TextEdit::new(bad)).is_err(), "{bad}");
    }
    assert_eq!(tail("a\nb\nc\n", 2), "b\nc\n");
    assert_eq!(tail("a\nb\nc", 2), "b\nc");
    assert_eq!(tail("a\nb", 5), "a\nb");

    let mut s = State::new();
    for line in ["ver all", "commander status"] {
        s.vehicle.ui.history.push_back(line.into());
    }
    let key = |k| KeyEvent {
        key: k,
        mods: Mods::default(),
    };
    let (state, tx) = (&mut s, &mut rune::Tx::default());
    {
        assert!(history_key(state, &key(Key::Up), tx));
        assert_eq!(state.vehicle.ui.console_input.text, "commander status");
        history_key(state, &key(Key::Up), tx);
        history_key(state, &key(Key::Up), tx);
        assert_eq!(state.vehicle.ui.console_input.text, "ver all");
        history_key(state, &key(Key::Down), tx);
        assert_eq!(state.vehicle.ui.console_input.text, "commander status");
        history_key(state, &key(Key::Down), tx);
        assert_eq!(state.vehicle.ui.console_input.text, "");
        assert!(!history_key(state, &key(Key::Left), tx));
    }
}

#[test]
fn the_trace_follows_fresh_snapshots_and_resets_without_a_link() {
    let mut s = State::new();
    s.vehicle.snapshot = Some(vehicle::fake_snapshot(true));
    assert!(follow(&mut s), "first fix and heartbeat age");
    assert_eq!(s.vehicle.ui.track.len(), 1);
    assert!(!follow(&mut s), "nothing moved, the age did not tick");
    s.vehicle.snapshot = None;
    assert!(follow(&mut s));
    assert!(s.vehicle.ui.track.is_empty());
}

#[test]
fn a_physical_system_never_shows_flight_actions_or_the_console() {
    let rt = runtime(scene(scene_readonly), (1440., 900.));
    for key in [
        "vehicle-live",
        "vehicle-attitude",
        "vehicle-track",
        "vehicle-health",
    ] {
        assert!(rt.presented(key).is_some(), "{key}");
    }
    for key in [
        "vehicle-actions",
        "flight-arm",
        "flight-takeoff",
        "vehicle-console",
        "console-input",
    ] {
        assert!(rt.find(key).is_none(), "{key} on a physical system");
    }
}

#[test]
fn without_the_grant_actions_and_console_are_disabled_with_the_reason() {
    let mut rt = runtime(scene(scene_gated), (1440., 900.));
    let reason = vehicle::gate_reason(&rt.state).expect("not granted");
    assert!(reason.contains("not running"), "{reason}");
    assert!(rt.find("flight-gate").is_some() && rt.find("console-gate").is_some());
    for key in [
        "flight-arm",
        "flight-takeoff",
        "flight-land",
        "console-send",
    ] {
        tap(&mut rt, key);
    }
    assert!(rt.state.confirm.is_none(), "a disabled Arm asks nothing");
    assert!(rt.state.vehicle.ui.pending.is_none());
    assert!(rt.state.vehicle.commands.is_empty());
}

#[test]
fn the_vehicle_panels_fit_compact_windows() {
    for (w, h) in [(390., 844.), (360., 800.)] {
        let mut rt = runtime(scene(scene_console), (w, h));
        for key in [
            "vehicle-attitude",
            "vehicle-heading",
            "vehicle-track",
            "vehicle-readings",
            "vehicle-health",
            "flight-bar",
            "flight-takeoff",
            "flight-mode",
            "console-output",
            "console-input",
            "console-send",
        ] {
            assert_inside(&mut rt, key, w, &format!("{w}x{h}"));
        }
        let mut rt = runtime(scene(scene_actions), (w, h));
        for key in ["flight-mode-list", "mode-0", "flight-results"] {
            assert_inside(&mut rt, key, w, &format!("actions {w}x{h}"));
        }
    }
}

/// A live, granted link to a fake PX4 on loopback: the app's router reports itself up, SIH
/// runs and is owned by the active system.
fn granted(fake: &FakeVehicle) -> State {
    let mut s = State::new();
    setup::preview(&mut s);
    s.navigate(crate::ui::Page::System);
    s.headless = false;
    s.config.distro = "fake-b2-flight".into();
    s.setup.host.as_mut().unwrap().distros[0].name = s.config.distro.clone();
    s.vehicle.sim_port = fake.addr.port();
    let sim = s.systems.active_id().to_string();
    vehicle::connect_sim(&mut s, &sim).expect("link");
    wait_for(LIMIT, || {
        vehicle::poll(&mut s);
        s.vehicle.fresh().then_some(())
    })
    .expect("heartbeat");
    s.vehicle.router.started = true;
    s.agent.router = Some(crate::agent::RouterStatus {
        state: "listening".into(),
        clients: 1,
        ..Default::default()
    });
    s.systems.claim("sih");
    s.status.insert("sih".into(), "Running".into());
    assert_eq!(vehicle::gate_reason(&s), None);
    s
}

/// Flight commands the vehicle received (the link-up HOME_POSITION request is not one).
fn commands_sent(fake: &FakeVehicle) -> usize {
    fake.received(|m| matches!(m, Msg::COMMAND_LONG(_)) && !is_home_request(m))
        .len()
}

#[test]
fn arm_asks_first_and_a_cancel_sends_nothing() {
    let fake = FakeVehicle::tcp();
    let mut rt = runtime(granted(&fake), (1440., 900.));
    tap(&mut rt, "flight-arm");
    assert_eq!(rt.state.confirm.as_deref(), Some("flight"));
    assert!(rt.presented("confirmation").is_some());
    tap(&mut rt, "cancel-action");
    assert!(rt.state.confirm.is_none() && rt.state.vehicle.ui.pending.is_none());
    assert!(rt.state.notice.contains("Nothing was sent"));
    std::thread::sleep(Duration::from_millis(400));
    vehicle::poll(&mut rt.state);
    assert_eq!(commands_sent(&fake), 0, "a cancelled Arm sends nothing");
    assert!(rt.state.vehicle.commands.is_empty());

    // Takeoff asks too; Escape dismisses like Cancel.
    tap(&mut rt, "flight-takeoff");
    assert!(matches!(
        rt.state.vehicle.ui.pending,
        Some(CommandKind::Takeoff { alt }) if alt == 10.
    ));
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    rt.frame(0.2);
    assert!(rt.state.confirm.is_none() && rt.state.vehicle.ui.pending.is_none());
    assert_eq!(commands_sent(&fake), 0);

    tap(&mut rt, "flight-arm");
    tap(&mut rt, "confirm-action");
    assert_eq!(rt.state.vehicle.commands.len(), 1);
    let arm = wait_for(LIMIT, || {
        vehicle::poll(&mut rt.state);
        fake.received(|m| {
            matches!(m, Msg::COMMAND_LONG(c)
                if c.command == dialect::MavCmd::MAV_CMD_COMPONENT_ARM_DISARM && c.param1 == 1.)
        })
        .first()
        .cloned()
    });
    assert!(arm.is_some(), "the confirmed Arm reached the vehicle");
    let result = wait_for(LIMIT, || {
        vehicle::poll(&mut rt.state);
        rt.state.vehicle.commands.back()?.result.clone()
    });
    assert_eq!(result, Some(crate::mavlink::CommandResult::Accepted));
    rt.frame(0.1);
    let id = rt.state.vehicle.commands.back().unwrap().id;
    assert!(
        rt.find(&format!("flight-result-{id}")).is_some(),
        "shown inline"
    );
    assert!(
        rt.state
            .activity
            .entries
            .iter()
            .any(|e| e.text.contains("Arm accepted"))
    );
    vehicle::disconnect(&mut rt.state);
}

#[test]
fn another_systems_connecting_link_blocks_this_page_and_a_new_link_starts_a_new_trace() {
    let fake = FakeVehicle::tcp();
    let mut s = granted(&fake);
    let fixed = wait_for(LIMIT, || {
        vehicle::poll(&mut s);
        (!s.vehicle.ui.track.is_empty()).then_some(())
    });
    assert!(
        fixed.is_some(),
        "the fake vehicle's position starts the trace"
    );
    let owner = s.systems.active_id().to_string();

    // Reconnecting starts a new trace, even before the first snapshot arrives.
    vehicle::disconnect(&mut s);
    assert!(s.vehicle.ui.track.is_empty());
    vehicle::connect_sim(&mut s, &owner).expect("link");
    assert!(s.vehicle.snapshot.is_none() && s.vehicle.link.is_some());

    // Another simulated system is active while the owner's link still connects: its page
    // names the owner and its bar sends nothing.
    let other = s
        .systems
        .list
        .iter()
        .find(|x| x.id != owner && x.source.simulated())
        .map(|x| x.id.clone())
        .expect("a second simulated system");
    assert!(s.systems.set_active(&other));
    assert!(owner_note(&s).is_some(), "connecting link has an owner");
    assert!(actions::blocked(&s).is_some());
    s.vehicle.ui.console_input = TextEdit::new("ver all");
    console::submit(&mut s);
    assert!(s.notice.starts_with("Console:"), "{}", s.notice);
    assert!(s.vehicle.ui.history.is_empty(), "nothing was sent");
    vehicle::disconnect(&mut s);
}

#[test]
fn a_malformed_attitude_or_far_home_draws_without_panicking() {
    let mut s = scene(scene_live);
    if let Some(v) = s.vehicle.snapshot.as_mut().and_then(|x| x.vehicle.as_mut()) {
        v.roll = Some(f32::NAN);
        v.pitch = Some(f32::INFINITY);
        v.home = Some((0., 0., 0.));
    }
    let rt = runtime(s, (1440., 900.));
    assert!(rt.presented("vehicle-attitude").is_some());
    let mut s = scene(scene_live);
    if let Some(v) = s.vehicle.snapshot.as_mut().and_then(|x| x.vehicle.as_mut()) {
        v.home = Some((40., -100., 0.));
    }
    let rt = runtime(s, (390., 844.));
    assert!(rt.presented("vehicle-track").is_some());
}

/// The age label ticks once a second only while it is drawn (the owner's System page), and
/// stops past a minute of silence, so a vehicle that went quiet never keeps an idle window
/// redrawing.
#[test]
fn the_heartbeat_age_ticks_only_while_it_is_drawn() {
    let mut s = State::new();
    let id = s.systems.active_id().to_string();
    s.vehicle.owner = Some(id);
    s.vehicle.snapshot = Some(vehicle::fake_snapshot(false));
    s.page = crate::ui::Page::Activity;
    assert!(!follow(&mut s), "another page: nothing drawn ticks");
    s.page = crate::ui::Page::System;
    assert!(follow(&mut s), "the System page draws the age");
    assert!(!follow(&mut s), "same second");
    let silent = |s: &mut State, secs: u64| {
        let v = s
            .vehicle
            .snapshot
            .as_mut()
            .unwrap()
            .vehicle
            .as_mut()
            .unwrap();
        v.last_heartbeat = std::time::Instant::now() - Duration::from_secs(secs);
    };
    silent(&mut s, 120);
    assert!(follow(&mut s), "past the cap once");
    silent(&mut s, 130);
    assert!(!follow(&mut s), "and no more ticks after it");
    let snap = s.vehicle.snapshot.as_ref().unwrap();
    let (line, _) = freshness(snap, snap.vehicle.as_ref().unwrap());
    assert!(line.contains("No heartbeat for over a minute"), "{line}");
    s.vehicle.owner = Some("someone-else".into());
    assert!(!follow(&mut s), "another system's link is not drawn here");
}

/// No page but System draws the age, so an open link with a silent vehicle never ticks the
/// window elsewhere; neither does an AI system's page, which has no live view.
#[test]
fn the_heartbeat_age_never_ticks_off_the_system_page() {
    use crate::ui::Page;
    let mut s = State::new();
    let id = s.systems.active_id().to_string();
    s.vehicle.owner = Some(id);
    s.vehicle.snapshot = Some(vehicle::fake_snapshot(false));
    for page in [
        Page::Tools,
        Page::Data,
        Page::Activity,
        Page::Settings,
        Page::Setup,
    ] {
        s.page = page;
        assert!(!follow(&mut s), "only the System page draws the age");
    }
    assert!(s.select_system(crate::systems::AI));
    let ai = s.systems.active_id().to_string();
    s.vehicle.owner = Some(ai);
    s.page = Page::System;
    assert!(!follow(&mut s), "an AI system's page draws no live view");
}

fn texts(rt: &Runtime<State>) -> Vec<String> {
    rt.elements().into_iter().filter_map(|e| e.text).collect()
}

/// While the workspace launch runs, the actions bar and the console name the stage instead of
/// the general "No live link", and send nothing.
#[test]
fn a_launch_under_way_disables_actions_with_its_stage() {
    let mut rt = runtime(scene(scene_launching), (1440., 900.));
    let reason = actions::blocked(&rt.state).expect("not granted");
    assert!(
        reason.starts_with("Workspace launch under way: starting PX4 / SIH"),
        "{reason}"
    );
    tap(&mut rt, "flight-arm");
    assert!(rt.state.confirm.is_none() && rt.state.vehicle.ui.pending.is_none());
    rt.update(|_, tx| tx.scroll_into_view("flight-gate"));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    let t = texts(&rt);
    assert!(
        t.iter()
            .any(|x| x.starts_with("Workspace launch under way")),
        "{t:?}"
    );
    assert!(!t.iter().any(|x| x.contains("Start the workspace of a")));
}

/// ArduPilot has no MAVLink shell: its console is disabled with the reason, flight actions
/// stay granted, and PX4's console is unchanged.
#[test]
fn the_ardupilot_console_is_disabled_and_px4s_is_not() {
    let mut rt = runtime(scene(scene_ardupilot_console), (1440., 900.));
    assert_eq!(
        actions::blocked(&rt.state),
        None,
        "flight actions are granted"
    );
    let reason = console::console_blocked(&rt.state).expect("console off");
    assert!(reason.contains("no MAVLink shell"), "{reason}");
    assert!(rt.find("console-gate").is_some());
    rt.state.vehicle.ui.console_input = TextEdit::new("ver all");
    tap(&mut rt, "console-send");
    console::submit(&mut rt.state);
    assert!(rt.state.vehicle.ui.history.is_empty(), "nothing was sent");
    assert!(
        rt.state.notice.contains("no MAVLink shell"),
        "{}",
        rt.state.notice
    );

    let s = scene(scene_console);
    assert_eq!(console::console_blocked(&s), None, "PX4 keeps its console");
}

/// A simulation link that keeps retrying shows Disconnect, which closes only the link; a
/// connected link and a launch under way do not show it.
#[test]
fn a_retrying_simulation_link_offers_disconnect() {
    let mut rt = runtime(scene(scene_link_retrying), (1440., 900.));
    assert!(sim_link_stuck(&rt.state));
    rt.update(|_, tx| tx.scroll_into_view("vehicle-disconnect"));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    assert!(rt.find("link-disconnect").is_some());
    let t = texts(&rt);
    assert!(t.iter().any(|x| x == "Disconnect link"), "{t:?}");
    assert!(
        t.iter().any(|x| x == "Stop session"),
        "Stop session stays: {t:?}"
    );
    tap(&mut rt, "link-disconnect");
    assert!(
        rt.state.notice.starts_with("Vehicle link closed"),
        "{}",
        rt.state.notice
    );
    assert!(
        rt.state.status.get("sih").is_some_and(|v| v == "Running"),
        "the simulator keeps running"
    );

    let rt = runtime(scene(scene_live), (1440., 900.));
    assert!(rt.find("link-disconnect").is_none(), "a connected link");
    let mut s = scene(scene_link_retrying);
    s.session = Some(crate::session::Launch::new(["qgc"]));
    assert!(!sim_link_stuck(&s), "the launch handles its own link");
}

/// PX4 takes off to an altitude above sea level: without a reported home the confirmed
/// height can't be sent, so Takeoff is refused before the dialog instead of climbing to
/// PX4's own default.
#[test]
fn a_px4_takeoff_waits_for_home() {
    let fake = FakeVehicle::tcp();
    let mut rt = runtime(granted(&fake), (1440., 900.));
    {
        let v = rt.state.vehicle.snapshot.as_mut().unwrap();
        v.vehicle.as_mut().unwrap().alt_msl = None;
    }
    tap(&mut rt, "flight-takeoff");
    assert!(rt.state.vehicle.ui.pending.is_none() && rt.state.confirm.is_none());
    assert!(
        rt.state.notice.contains("Home not reported yet"),
        "{}",
        rt.state.notice
    );
    std::thread::sleep(Duration::from_millis(300));
    vehicle::poll(&mut rt.state);
    assert_eq!(commands_sent(&fake), 0);
    vehicle::disconnect(&mut rt.state);
}

#[test]
fn a_launching_workspace_says_so_in_place_of_no_link() {
    let mut s = scene(|s| {
        s.session = Some(crate::session::Launch::new(
            crate::systems::SESSIONS[crate::systems::SIMULATION],
        ));
    });
    s.navigate(crate::ui::Page::System);
    let mut rt = runtime(s, (1440., 900.));
    rt.update(|_, tx| tx.scroll_into_view("vehicle-live"));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    let texts: Vec<String> = rt.elements().into_iter().filter_map(|e| e.text).collect();
    assert!(
        texts.iter().any(|t| t.starts_with("Workspace starting")),
        "{texts:?}"
    );
    assert!(
        !texts
            .iter()
            .any(|t| t.contains("Start the workspace to see"))
    );
}
