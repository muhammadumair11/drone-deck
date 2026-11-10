//! The launch through virtual stages (fake agent + fake vehicle on loopback), the safety gate,
//! exclusive simulators, physical read-only links and headless previews.
use super::*;
use crate::agent::{self, fake};
use crate::mavlink::fake_vehicle::{FakeVehicle, is_home_request, wait_for};
use crate::mavlink::{
    Authority, CommandKind, CommandResult, Msg, Refusal, SimulationEvidence, dialect,
};
use crate::{session, setup, systems};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(10);

/// A loopback port nothing listens on (bound once, then released).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("free port")
        .port()
}

/// A state that may connect: not headless, WSL 2 `distro` ready, no backend (nothing spawns).
/// Its simulator port is a free loopback port, so the port probe never depends on what this
/// machine runs on TCP 5760.
fn live_state(distro: &str) -> State {
    let mut s = State::new();
    setup::preview(&mut s);
    s.headless = false;
    s.config.distro = distro.into();
    s.setup.host.as_mut().unwrap().distros[0].name = distro.into();
    s.vehicle.sim_port = free_port();
    s
}

/// The router reports the app's link as its client, as `router.py` does once it connects.
fn link_is_router_client(s: &mut State) {
    if let Some(router) = s.agent.router.as_mut() {
        router.clients = 1;
    }
}

/// A fake agent whose router starts and stops like `scripts/agent/router.py`.
fn router_agent(distro: &str) -> fake::Seen {
    let (connector, seen) =
        fake::connector(Arc::new(|request, out| match request["method"].as_str() {
            Some("router.start") => {
                let status = json!({"state":"listening","clients":0,"packets":0,"to_vehicle":0,
                    "dropped":0,"vehicle":null,"udp_listen":14550,"tcp_port":5760});
                out.reply(request, status.clone());
                let mut event = status;
                event["event"] = json!("router");
                out.send(event);
            }
            Some("router.stop") => out.reply(request, json!({"state":"stopped"})),
            _ => out.reply(request, json!({})),
        }));
    agent::install(distro, connector);
    seen
}

/// The router up with the app's link as its one client.
fn listening() -> crate::agent::RouterStatus {
    crate::agent::RouterStatus {
        state: "listening".into(),
        clients: 1,
        ..Default::default()
    }
}

/// Polls until the link is up. A fake TCP vehicle waits in `accept` until the service
/// connects, and dropping it joins that thread, so every test lets the link connect.
fn wait_until_up(s: &mut State) {
    wait_for(LIMIT, || {
        poll(s);
        s.vehicle.up().then_some(())
    })
    .expect("link up");
}

fn advance_until(s: &mut State, stage: &str) {
    wait_for(LIMIT, || {
        poll(s);
        session::poll(s);
        (s.session.as_ref()?.current() == stage).then_some(())
    })
    .unwrap_or_else(|| panic!("launch never reached {stage}: {}", s.notice));
}

#[test]
fn a_launch_waits_for_the_router_then_the_link_and_stop_session_closes_both() {
    let distro = "fake-b1-launch";
    let seen = router_agent(distro);
    let mut s = live_state(distro);
    let sim = s.systems.active_id().to_string();
    assert!(s.bundle(systems::session_stages(s.systems.current())));
    // The Windows port is checked off the UI thread: the first frame only starts the probe.
    assert!(!session::poll(&mut s));
    assert!(!s.status.contains_key("sih"));
    wait_for(LIMIT, || {
        poll(&mut s);
        session::poll(&mut s);
        s.status.contains_key("sih").then_some(())
    })
    .expect("SIH starts once the port is known to be free");
    assert_eq!(s.status.get("sih").map(String::as_str), Some("Starting"));
    s.status.insert("sih".into(), "Running".into());
    session::poll(&mut s);
    assert!(
        !seen.methods().contains(&"router.start".to_string()),
        "the router waits for SIH readiness"
    );
    s.readiness.insert("sih".into(), "Ready".into());
    advance_until(&mut s, "router");
    advance_until(&mut s, "link");
    assert!(seen.methods().contains(&"router.start".to_string()));
    assert!(s.vehicle.router.started && router_running(&s));
    // The simulator port was free until the router started; now it listens (the link
    // retries until it does).
    let fake = FakeVehicle::tcp_at(([127, 0, 0, 1], s.vehicle.sim_port).into());
    advance_until(&mut s, "qgc");
    assert!(s.vehicle.up() && s.vehicle.owned_by(&sim));
    assert_eq!(
        s.vehicle.endpoint,
        format!("tcp:127.0.0.1:{}", fake.addr.port())
    );

    // The app's own router link to its own running SIH: commands are allowed.
    wait_for(LIMIT, || {
        poll(&mut s);
        s.vehicle.fresh().then_some(())
    })
    .expect("heartbeat");
    let reason = gate_reason(&s).expect("no router client yet");
    assert!(reason.contains("reports no client"), "{reason}");
    link_is_router_client(&mut s);
    assert_eq!(gate_reason(&s), None);
    let id = command(&mut s, CommandKind::Arm).expect("granted");
    let result = wait_for(LIMIT, || {
        poll(&mut s);
        s.vehicle
            .commands
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.result.clone())
    });
    assert_eq!(result, Some(CommandResult::Accepted));
    assert!(s.activity.entries.iter().any(|e| e.text.contains("Arm")));

    s.stop_all();
    assert!(s.vehicle.link.is_none() && !router_running(&s));
    wait_for(LIMIT, || {
        seen.methods()
            .contains(&"router.stop".to_string())
            .then_some(())
    })
    .expect("Stop session stops the router the app started");
}

#[test]
fn a_router_that_cannot_start_ends_the_launch_with_its_reason() {
    let distro = "fake-b1-router-busy";
    let (connector, _) = fake::connector(Arc::new(|request, out| {
        if request["method"] == "router.start" {
            out.error(
                request,
                "UDP 14550 is in use inside WSL (Address already in use).",
            );
        }
    }));
    agent::install(distro, connector);
    let mut s = live_state(distro);
    s.session = Some(session::Launch::new(["router", "link"]));
    session::poll(&mut s);
    wait_for(LIMIT, || {
        poll(&mut s);
        session::poll(&mut s);
        s.session.is_none().then_some(())
    })
    .expect("the launch ends");
    assert!(s.notice.contains("stopped at router"), "{}", s.notice);
    assert!(s.notice.contains("14550 is in use"), "{}", s.notice);
    assert!(s.vehicle.link.is_none(), "the link never opened");
}

#[test]
fn authority_needs_every_piece_of_evidence() {
    for bits in 0..8u8 {
        let e = SimulationEvidence {
            app_router_link: bits & 1 != 0,
            simulator_source: bits & 2 != 0,
            stack_owned_running: bits & 4 != 0,
        };
        assert_eq!(
            Authority::from_evidence(e).may_fly(),
            bits == 7,
            "evidence {e:?}"
        );
    }
}

#[test]
fn the_gate_follows_the_observed_link_router_and_stack() {
    let fake = FakeVehicle::tcp();
    let mut s = live_state("fake-b1-gate");
    s.vehicle.sim_port = fake.addr.port();
    let sim = s.systems.active_id().to_string();
    assert!(gate_reason(&s).unwrap().contains("No live link"));
    connect_sim(&mut s, &sim).expect("link");
    wait_until_up(&mut s);
    let e = evidence(&s);
    assert!(e.simulator_source && !e.app_router_link && !e.stack_owned_running);
    assert!(
        gate_reason(&s)
            .unwrap()
            .contains("not the app's own simulator link")
    );
    s.vehicle.router.started = true;
    assert!(
        !authority(&s).may_fly() && !evidence(&s).app_router_link,
        "a router that never reported itself up is no evidence"
    );
    s.agent.router = Some(listening());
    assert!(
        gate_reason(&s)
            .unwrap()
            .contains("PX4 / SIH is not running")
    );
    // Running, but started by another system: still read-only.
    let other = s.systems.create(systems::SIMULATION, "Other lab");
    s.systems.set_active(&other);
    s.systems.claim("sih");
    s.status.insert("sih".into(), "Running".into());
    assert!(!authority(&s).may_fly());
    s.systems.set_active(&sim);
    s.systems.claim("sih");
    assert!(authority(&s).may_fly());
    // A router without a client never saw the app's link: whatever answered on the Windows
    // port is not the simulator.
    s.agent.router.as_mut().unwrap().clients = 0;
    assert!(!authority(&s).may_fly() && !evidence(&s).app_router_link);
    let reason = gate_reason(&s).unwrap();
    assert!(reason.contains("reports no client"), "{reason}");
    s.agent.router.as_mut().unwrap().clients = 1;
    assert!(authority(&s).may_fly());
    // The agent restarts: its router is gone, and so is the grant, even though TCP 5760 may
    // answer again later.
    s.agent.router = None;
    assert!(poll(&mut s), "the lost router is noticed");
    assert!(!authority(&s).may_fly() && !router_running(&s));
    let reason = gate_reason(&s).unwrap();
    assert!(reason.contains("agent restarted"), "{reason}");
    assert!(command(&mut s, CommandKind::Arm).is_err());
    assert!(matches!(
        stage(&mut s, "router", Step::Ready),
        Some(Stage::Failed(_))
    ));
    s.vehicle.router.error = None;
    s.agent.router = Some(listening());
    assert!(authority(&s).may_fly());
    // A second UDP sender reached the router (say a mavproxy for a real flight controller).
    s.agent.router.as_mut().unwrap().foreign = 3;
    assert!(!authority(&s).may_fly());
    let reason = gate_reason(&s).unwrap();
    assert!(reason.contains("3 datagrams ignored"), "{reason}");
    s.agent.router = Some(listening());
    // Two autopilot system ids on one link: read-only too.
    let snapshot = s.vehicle.snapshot.as_mut().expect("snapshot");
    let first = snapshot.systems[0].clone();
    snapshot.systems.push(crate::mavlink::SystemSeen {
        system_id: first.system_id.wrapping_add(1),
        ..first
    });
    assert!(!authority(&s).may_fly());
    let reason = gate_reason(&s).unwrap();
    assert!(reason.contains("system ids"), "{reason}");
    s.vehicle.snapshot.as_mut().unwrap().systems.truncate(1);
    assert!(authority(&s).may_fly());
    s.status.insert("sih".into(), "Exited 1".into());
    assert!(!authority(&s).may_fly());
    let refused = command(&mut s, CommandKind::Takeoff { alt: 5. }).unwrap_err();
    assert!(refused.contains("not running"), "{refused}");
    assert!(console_send(&mut s, "commander status").is_err());
    disconnect(&mut s);
    assert!(s.vehicle.link.is_none() && s.vehicle.snapshot.is_none());
}

/// Something on Windows already listening on the simulator port (MAVProxy, a bridge to a real
/// flight controller) would take the app's link in place of the simulator: no simulator or
/// router starts while it does.
#[test]
fn a_foreign_listener_on_the_simulator_port_refuses_the_launch() {
    let distro = "fake-b1-foreign";
    let seen = router_agent(distro);
    let mut s = live_state(distro);
    let foreign = std::net::TcpListener::bind(("127.0.0.1", s.vehicle.sim_port)).expect("bind");
    let until_ended = |s: &mut State| {
        wait_for(LIMIT, || {
            poll(s);
            session::poll(s);
            s.session.is_none().then_some(())
        })
        .expect("the launch ends");
    };
    // The launch begins and its first stage, checked off the UI thread, ends it.
    assert!(s.bundle(systems::session_stages(s.systems.current())));
    until_ended(&mut s);
    assert!(s.notice.contains("stopped at sih"), "{}", s.notice);
    assert!(s.notice.contains("already listens on TCP"), "{}", s.notice);
    assert!(!s.status.contains_key("sih"), "the simulator did not start");
    let sih = s
        .config
        .profiles
        .iter()
        .position(|p| p.id == "sih")
        .unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    s.start(sih);
    assert!(s.notice.contains("Checking that nothing"), "{}", s.notice);
    wait_for(LIMIT, || {
        poll(&mut s);
        s.notice.contains("already listens on TCP").then_some(())
    })
    .expect("a direct start says why once the probe answers");
    assert!(!s.status.contains_key("sih"), "nor on its own");

    // The router stage refuses too (SIH already running, started elsewhere).
    s.session = Some(session::Launch::new(["router", "link"]));
    until_ended(&mut s);
    assert!(s.notice.contains("stopped at router"), "{}", s.notice);
    assert!(s.notice.contains("already listens on TCP"), "{}", s.notice);
    assert!(!seen.methods().contains(&"router.start".to_string()));
    assert!(s.vehicle.link.is_none());
    // The router station and the gate name the taken port, not "Not started" or a closed link.
    let router = s.vehicle.router.error.clone().expect("router error");
    assert!(router.contains("already listens on TCP"), "{router}");
    let station = systems::state(&s, systems::component("router").unwrap());
    assert_eq!(station, (router, systems::Tone::Bad));
    let reason = gate_reason(&s).unwrap();
    assert!(reason.contains("already listens on TCP"), "{reason}");
    s.stop_all();
    assert!(s.vehicle.router.error.is_none(), "Stop session clears it");

    // Once it is gone, the router may start.
    drop(foreign);
    assert!(!port::probe(s.vehicle.sim_port));
}

/// A direct start (Tools, the runtime list) probes the port off the UI thread and starts once
/// it is free; Stop session drops a start still waiting.
#[test]
fn a_direct_simulator_start_waits_for_the_port_check() {
    let mut s = live_state("fake-b1-direct");
    let sih = s
        .config
        .profiles
        .iter()
        .position(|p| p.id == "sih")
        .unwrap();
    s.start(sih);
    assert!(
        s.notice
            .contains("Checking that nothing on Windows listens on TCP"),
        "{}",
        s.notice
    );
    assert!(!s.status.contains_key("sih"));
    wait_for(LIMIT, || {
        poll(&mut s);
        s.status.contains_key("sih").then_some(())
    })
    .expect("started once the port is free");
    assert_eq!(s.status.get("sih").map(String::as_str), Some("Starting"));

    let mut s = live_state("fake-b1-direct-stop");
    s.start(sih);
    s.stop_all();
    std::thread::sleep(Duration::from_millis(400));
    for _ in 0..5 {
        poll(&mut s);
    }
    assert!(!s.status.contains_key("sih"), "Stop session dropped it");
}

/// A direct start waiting for the port check stays with the system it was started for: once
/// another system is active (or the system is gone) it is dropped with a notice instead of
/// being claimed for whichever system is active when the probe answers.
#[test]
fn a_waiting_direct_start_is_dropped_when_the_system_changes() {
    let until_answered = |s: &mut State| {
        wait_for(LIMIT, || {
            poll(s);
            s.notice.contains("was not started").then_some(())
        })
        .unwrap_or_else(|| panic!("the probe answers: {}", s.notice));
    };
    let mut s = live_state("fake-b1-direct-switch");
    let sih = s
        .config
        .profiles
        .iter()
        .position(|p| p.id == "sih")
        .unwrap();
    let home = s.systems.active_id().to_string();
    let lab = s.systems.create(systems::SIMULATION, "Second lab");
    s.start(sih);
    assert!(s.notice.contains("Checking that nothing"), "{}", s.notice);
    s.systems.set_active(&lab);
    until_answered(&mut s);
    assert!(
        s.notice.contains("active system changed")
            && s.notice.contains(s.systems.get(&home).unwrap().label()),
        "{}",
        s.notice
    );
    assert!(!s.status.contains_key("sih"), "nothing started");

    // Started for the second lab, which is then deleted.
    std::thread::sleep(Duration::from_millis(1100));
    s.start(sih);
    assert!(s.notice.contains("Checking that nothing"), "{}", s.notice);
    s.systems.set_active(&home);
    s.systems.remove(&lab).expect("removed");
    until_answered(&mut s);
    assert!(s.notice.contains("no longer exists"), "{}", s.notice);
    assert!(!s.status.contains_key("sih"));
}

/// While the active system's launch has not finished its link stage, the gate says the launch
/// is under way; another system's launch and a link closed under a running stack are named.
#[test]
fn the_gate_names_a_launch_under_way_and_a_closed_link() {
    let mut s = State::new();
    let sim = s.systems.active_id().to_string();
    assert_eq!(
        gate_reason(&s).unwrap(),
        "No live link. Start the workspace of a simulated system first."
    );
    s.session = Some(session::Launch::new(systems::SESSIONS[systems::SIMULATION]));
    let reason = gate_reason(&s).unwrap();
    assert!(
        reason.starts_with("Workspace launch under way: starting PX4 / SIH (stage 1 of 4)"),
        "{reason}"
    );
    assert!(command(&mut s, CommandKind::Arm).is_err(), "still refused");

    // The link stage: the preview link connects, the router has no client yet.
    s.session = Some(session::Launch::new(["link", "qgc"]));
    preview(&mut s, &sim, true, false);
    s.vehicle.preview = Some(SimulationEvidence {
        app_router_link: false,
        simulator_source: true,
        stack_owned_running: true,
    });
    let reason = gate_reason(&s).unwrap();
    assert!(reason.starts_with("Workspace launch under way"), "{reason}");
    // A foreign vehicle still wins over the launch note.
    s.agent.router.as_mut().unwrap().foreign = 2;
    assert!(gate_reason(&s).unwrap().contains("2 datagrams ignored"));

    // Another system's launch.
    let mut s = State::new();
    let other = s.systems.create(systems::SIMULATION, "Other lab");
    s.systems.set_active(&other);
    s.systems.claim("session");
    s.systems.set_active(&sim);
    s.session = Some(session::Launch::new(["sih"]));
    let reason = gate_reason(&s).unwrap();
    assert!(
        reason.contains("A workspace launch for Other lab is under way"),
        "{reason}"
    );

    // The stack runs and no link was ever opened (a direct start from Tools).
    let mut s = State::new();
    let sim = s.systems.active_id().to_string();
    s.systems.claim("sih");
    s.status.insert("sih".into(), "Running".into());
    let reason = gate_reason(&s).unwrap();
    assert_eq!(
        reason,
        "No live link: PX4 / SIH runs, but no vehicle link was opened for it. Start the workspace to open the link."
    );
    // Its link was opened, then closed on its own.
    s.vehicle.owner = Some(sim.clone());
    s.vehicle.closed = true;
    let reason = gate_reason(&s).unwrap();
    assert!(
        reason.contains("the vehicle link was closed while PX4 / SIH still runs"),
        "{reason}"
    );
    // Another system's closed link says nothing about this one.
    s.vehicle.owner = Some("other".into());
    assert!(
        gate_reason(&s)
            .unwrap()
            .contains("no vehicle link was opened")
    );
    // A launch that stopped at the router names the router's reason.
    s.vehicle.router.error = Some(port::taken(5760));
    let reason = gate_reason(&s).unwrap();
    assert!(
        reason.starts_with("No live link. Something on Windows already listens on TCP 5760"),
        "{reason}"
    );
}

/// ArduPilot SITL has no MAVLink shell: a console line is refused before it reaches the link.
#[test]
fn the_console_refuses_an_ardupilot_link() {
    let fake = FakeVehicle::tcp();
    let mut s = live_state("fake-b1-ardupilot");
    s.vehicle.sim_port = fake.addr.port();
    let ap = s.systems.create(systems::SIMULATION, "ArduPilot lab");
    s.systems.get_mut(&ap).unwrap().source = systems::Source::ArduPilot {
        vehicle: "copter".into(),
    };
    let px4 = s
        .systems
        .list
        .iter()
        .find(|x| matches!(x.source, systems::Source::Sih))
        .map(|x| x.id.clone())
        .unwrap();
    assert!(
        shell_unsupported(&s, &ap).is_some(),
        "known from the source"
    );
    assert!(shell_unsupported(&s, &px4).is_none());
    connect_sim(&mut s, &ap).expect("link");
    wait_until_up(&mut s);
    let refused = console_send(&mut s, "ver all").unwrap_err();
    assert!(refused.contains("no MAVLink shell"), "{refused}");
    // Ending the shell session is refused the same way, whatever the UI button allows.
    let refused = console_close(&mut s).unwrap_err();
    assert!(refused.contains("no MAVLink shell"), "{refused}");
    std::thread::sleep(Duration::from_millis(300));
    poll(&mut s);
    assert!(
        fake.received(|m| matches!(m, Msg::SERIAL_CONTROL(_)))
            .is_empty()
    );
    disconnect(&mut s);
}

/// A simulation link that keeps retrying (its router is gone) can be closed on its own:
/// the simulator keeps running and the gate says how to reconnect.
#[test]
fn a_retrying_simulation_link_disconnects_on_its_own() {
    let mut s = live_state("fake-b1-retrying");
    let sim = s.systems.active_id().to_string();
    s.systems.claim("sih");
    s.status.insert("sih".into(), "Running".into());
    connect_sim(&mut s, &sim).expect("link");
    std::thread::sleep(Duration::from_millis(200));
    poll(&mut s);
    assert!(
        view::sim_link_stuck(&s),
        "nothing listens: the link retries"
    );
    s.session = Some(session::Launch::new(["qgc"]));
    assert!(!view::sim_link_stuck(&s), "not while a launch runs");
    s.session = None;
    view::disconnect_link(&mut s);
    assert!(s.vehicle.link.is_none());
    assert!(s.notice.starts_with("Vehicle link closed"), "{}", s.notice);
    assert_eq!(s.status.get("sih").map(String::as_str), Some("Running"));
    assert!(!view::sim_link_stuck(&s));
    let reason = gate_reason(&s).unwrap();
    assert!(
        reason.contains("closed while PX4 / SIH still runs"),
        "{reason}"
    );
}

#[test]
fn physical_links_are_read_only_and_send_no_heartbeat() {
    let fake = FakeVehicle::tcp();
    let mut s = live_state("fake-b1-physical");
    let id = s.systems.create(systems::PHYSICAL, "Bench quad");
    s.select_system(&id);
    let endpoint = format!("tcp:127.0.0.1:{}", fake.addr.port());
    connect_physical(&mut s, &id, &endpoint).expect("link");
    wait_for(LIMIT, || {
        poll(&mut s);
        s.vehicle.fresh().then_some(())
    })
    .expect("vehicle heartbeat");
    // Even forged evidence does not open a physical link.
    s.vehicle.sim = true;
    s.vehicle.router.started = true;
    assert!(!authority(&s).may_fly());
    assert!(gate_reason(&s).unwrap().contains("read-only"));
    assert!(command(&mut s, CommandKind::Arm).is_err());
    let granted = Authority::from_evidence(SimulationEvidence {
        app_router_link: true,
        simulator_source: true,
        stack_owned_running: true,
    });
    let link = s.vehicle.link.as_ref().unwrap();
    assert_eq!(
        link.command(CommandKind::Arm, granted),
        Err(Refusal::PhysicalLink)
    );
    assert_eq!(link.shell("ls\n", granted), Err(Refusal::PhysicalLink));
    // Read-only requests still work.
    fetch_params(&mut s).expect("parameters are read-only");
    std::thread::sleep(Duration::from_millis(1500));
    poll(&mut s);
    let ours = fake.received(|m| {
        matches!(m, Msg::HEARTBEAT(h) if h.mavtype == dialect::MavType::MAV_TYPE_GCS)
            || (matches!(m, Msg::COMMAND_LONG(_)) && !is_home_request(m))
    });
    assert!(
        ours.is_empty(),
        "no GCS heartbeat or command reached the vehicle: {ours:?}"
    );
    // The one read request it does send: HOME_POSITION, once.
    assert_eq!(fake.received(is_home_request).len(), 1);
    s.vehicle.sim = false;
    assert!(crate::hardware::connected(&s));
    crate::hardware::toggle(&mut s);
    assert!(s.vehicle.link.is_none());
    assert_eq!(
        s.hardware.message,
        "Disconnected. This only closed the link; nothing was commanded."
    );
    // Connecting says what Drone Deck sends; with QGroundControl enabled, that it bridges it.
    s.systems.get_mut(&id).unwrap().consumers.qgc = true;
    s.hardware.endpoint = crate::TextEdit::new(endpoint);
    crate::hardware::toggle(&mut s);
    assert!(
        s.hardware
            .message
            .contains("bridged both ways and can command the vehicle"),
        "{}",
        s.hardware.message
    );
    crate::hardware::toggle(&mut s);
    assert!(!crate::hardware::DEFAULT_MESSAGE.contains("Nothing is sent"));
}

#[test]
fn one_link_at_a_time_and_one_simulator_at_a_time() {
    let fake = FakeVehicle::tcp();
    let mut s = live_state("fake-b1-exclusive");
    s.vehicle.sim_port = fake.addr.port();
    let sim = s.systems.active_id().to_string();
    s.systems.claim("sih");
    s.status.insert("sih".into(), "Running".into());
    connect_sim(&mut s, &sim).expect("link");
    wait_until_up(&mut s);

    // Gazebo for the ROS system would share UDP 14550 / TCP 5760.
    assert!(s.select_system(systems::ROS));
    let ros = s.systems.active_id().to_string();
    assert!(!s.bundle(systems::session_stages(s.systems.current())));
    assert!(
        s.notice
            .contains("PX4 / SIH is running for Local simulation"),
        "{}",
        s.notice
    );
    let gazebo = s
        .config
        .profiles
        .iter()
        .position(|p| p.id == "gazebo")
        .unwrap();
    s.start(gazebo);
    assert!(
        !s.status.contains_key("gazebo"),
        "the second simulator is refused"
    );
    assert!(s.notice.contains("Local simulation"));
    let refused = connect_sim(&mut s, &ros).unwrap_err();
    assert!(refused.contains("belongs to Local simulation"), "{refused}");

    // The same simulator for another SIH system is refused too.
    let twin = s.systems.create(systems::SIMULATION, "Twin lab");
    assert!(s.select_system(&twin));
    assert!(!s.bundle(systems::session_stages(s.systems.current())));
    assert!(
        s.notice.contains("already runs for Local simulation"),
        "{}",
        s.notice
    );
    assert!(s.vehicle.owned_by(&sim), "nothing was taken over");
}

#[test]
fn headless_previews_never_connect_and_show_a_fake_snapshot() {
    let mut s = State::new();
    let sim = s.systems.active_id().to_string();
    assert!(connect_sim(&mut s, &sim).is_err());
    assert!(connect_physical(&mut s, &sim, "udp:127.0.0.1:14550").is_err());
    assert_eq!(
        stage(&mut s, "link", Step::Start),
        Some(Stage::Failed(
            "Preview: native connections disabled.".into()
        ))
    );
    assert!(matches!(
        stage(&mut s, "router", Step::Start),
        Some(Stage::Failed(_))
    ));
    assert_eq!(
        stage(&mut s, "sih", Step::Start),
        None,
        "profiles are not virtual"
    );
    assert!(s.vehicle.link.is_none());

    preview(&mut s, &sim, false, true);
    assert!(s.vehicle.shown_for(&sim).is_some_and(|snap| !snap.fresh));
    assert!(s.vehicle.shown_for("other").is_none());
    assert!(authority(&s).may_fly(), "a scene can draw granted actions");
    assert!(
        command(&mut s, CommandKind::Land).is_err(),
        "but nothing is sent"
    );
    assert!(router_running(&s), "a PX4 preview shows its router up");
    assert!(
        !crate::hardware::connected(&s),
        "a simulated link is no physical link"
    );
    preview(&mut s, &sim, true, false);
    assert!(gate_reason(&s).is_some());
    assert!(s.select_system(systems::PHYSICAL));
    let physical = s.systems.active_id().to_string();
    preview(&mut s, &physical, true, false);
    assert!(
        crate::hardware::connected(&s),
        "the physical page matches its preview link"
    );
    assert_eq!(s.hardware.endpoint.text, s.vehicle.endpoint);
    crate::hardware::toggle(&mut s);
    assert!(s.hardware.message.contains("Preview") && s.vehicle.snapshot.is_some());
    let mut v = fake_snapshot(true).vehicle.unwrap();
    assert!(v.status.len() == 2 && v.relative_alt.is_some());
    v.status.clear();
}

#[test]
fn launch_env_carries_world_model_vehicle_and_domain() {
    let mut ros = systems::model::template(systems::ROS, "r", "R");
    ros.source = systems::Source::Gazebo {
        world: "baylands".into(),
        model: "x500_depth".into(),
    };
    ros.consumers.ros.domain = 7;
    let env = backend::launch_env("gazebo", &ros);
    assert!(env.contains(&("DRONE_DECK_GZ_WORLD".into(), "baylands".into())));
    assert!(env.contains(&("DRONE_DECK_GZ_MODEL".into(), "x500_depth".into())));
    assert!(backend::launch_env("rosgz", &ros).contains(&("ROS_DOMAIN_ID".into(), "7".into())));
    assert!(backend::launch_env("qgc", &ros).is_empty());
    let mut ap = systems::model::template(systems::SIMULATION, "a", "A");
    ap.source = systems::Source::ArduPilot {
        vehicle: "plane".into(),
    };
    assert_eq!(
        backend::launch_env("ardupilot", &ap),
        [("DRONE_DECK_AP_VEHICLE".to_string(), "ArduPlane".to_string())]
    );
    let defaults = backend::defaults();
    let args = |id: &str| defaults.profiles.iter().find(|p| p.id == id).unwrap().args[0].clone();
    assert!(args("gazebo").contains("PX4_GZ_WORLD=\"${DRONE_DECK_GZ_WORLD:-default}\""));
    assert!(args("gazebo").contains("gz_${DRONE_DECK_GZ_MODEL:-x500_mono_cam}"));
    assert!(args("ardupilot").contains("-v \"${DRONE_DECK_AP_VEHICLE:-ArduCopter}\""));
    assert!(args("rosgz").contains("/link/camera_link/sensor/camera"));
    assert!(args("foxglove").contains("port:=8765"));
    for retired in backend::RETIRED {
        assert!(
            defaults.profiles.iter().all(|p| p.id != retired),
            "{retired}"
        );
    }
}
