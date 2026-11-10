//! End-to-end service tests against the fake vehicle on loopback sockets.
use super::fake_vehicle::{FakeVehicle, is_home_request, scratch, wait_for};
use super::frame::{Encoder, Framer, Msg, dialect};
use super::*;
use std::net::UdpSocket;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(10);

fn tcp(fake: &FakeVehicle) -> Endpoint {
    format!("tcp:127.0.0.1:{}", fake.addr.port())
        .parse()
        .expect("endpoint")
}

fn sim() -> Authority {
    Authority::from_evidence(SimulationEvidence {
        app_router_link: true,
        simulator_source: true,
        stack_owned_running: true,
    })
}

/// Polls until a snapshot satisfies `check`; events are discarded.
fn snapshot_until(
    service: &mut Service,
    check: impl Fn(&VehicleSnapshot) -> bool,
) -> VehicleSnapshot {
    wait_for(LIMIT, || {
        let update = service.poll();
        update.snapshot.filter(|s| check(s))
    })
    .expect("snapshot condition")
}

/// Polls until an event matches; earlier events are discarded.
fn event_until<T>(service: &mut Service, pick: impl Fn(&Event) -> Option<T>) -> T {
    wait_for(LIMIT, || service.poll().events.iter().find_map(&pick)).expect("event")
}

fn connected(fake: &FakeVehicle, origin: LinkOrigin) -> Service {
    let mut service = Service::start(Config::new(tcp(fake), origin)).expect("start");
    snapshot_until(&mut service, |s| s.fresh);
    service
}

#[test]
fn tcp_link_decodes_vehicle_state() {
    let fake = FakeVehicle::tcp();
    let mut service =
        Service::start(Config::new(tcp(&fake), LinkOrigin::SimulatorRouter)).expect("start");
    let snap = snapshot_until(&mut service, |s| {
        s.vehicle.as_ref().is_some_and(|v| {
            v.landed_state == 1 && v.battery_voltage.is_some() && !v.message_rates.is_empty()
        })
    });
    assert_eq!(snap.link.state, LinkState::Connected);
    assert!(snap.fresh);
    let v = snap.vehicle.expect("vehicle");
    assert_eq!((v.system_id, v.component_id), (1, 1));
    assert_eq!(v.autopilot, Autopilot::Px4);
    assert_eq!(v.vehicle_type_name(), "Quadrotor");
    assert_eq!(v.mode, "Position");
    assert!(!v.armed);
    assert!((v.lat.expect("lat") - 47.397742).abs() < 1e-6);
    assert!((v.relative_alt.expect("rel") - 12.5).abs() < 1e-3);
    assert!((v.alt_msl.expect("msl") - 488.0).abs() < 1e-3);
    assert!((v.groundspeed.expect("gs") - 5.0).abs() < 1e-3);
    assert!((v.climb.expect("climb") - 1.0).abs() < 1e-3);
    assert!((v.heading.expect("hdg") - 90.0).abs() < 1e-3);
    assert!((v.roll.expect("roll") - 0.1f32.to_degrees()).abs() < 1e-3);
    assert_eq!(v.battery_remaining, Some(87));
    assert!((v.battery_voltage.expect("volts") - 16.2).abs() < 1e-3);
    assert_eq!((v.gps_fix_name(), v.satellites), ("3D fix", Some(14)));
    assert_eq!(v.unhealthy_sensors(), 0b100);
    assert_eq!(v.landed_name(), "On ground");
    assert!(
        v.message_rates
            .iter()
            .any(|(n, r)| *n == "HEARTBEAT" && *r > 1.0)
    );
    assert!(v.status.iter().any(|s| s.text == "Fake vehicle ready"));
    assert!(
        snap.systems
            .iter()
            .any(|s| s.system_id == 1 && s.autopilot == 12)
    );
}

#[test]
fn snapshots_are_coalesced_to_ten_hertz() {
    let fake = FakeVehicle::tcp();
    let mut service = connected(&fake, LinkOrigin::SimulatorRouter);
    let start = std::time::Instant::now();
    let mut count = 0;
    while start.elapsed() < Duration::from_millis(1500) {
        if service.poll().snapshot.is_some() {
            count += 1;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // The fake sends six messages every 100 ms; at most ~15 publishes fit in 1.5 s.
    assert!((3..=17).contains(&count), "{count} snapshots in 1.5 s");
}

#[test]
fn fan_out_is_bidirectional_and_logged() {
    let fake = FakeVehicle::tcp();
    let peer = UdpSocket::bind("127.0.0.1:0").expect("peer");
    peer.set_read_timeout(Some(Duration::from_millis(200)))
        .expect("timeout");
    let dir = scratch("fanout");
    let tlog_path = dir.join("session.tlog");
    let config = Config::new(tcp(&fake), LinkOrigin::Physical)
        .peer(peer.local_addr().expect("addr"))
        .tlog(tlog_path.clone());
    let mut service = Service::start(config).expect("start");
    snapshot_until(&mut service, |s| s.fresh);

    // Vehicle → peer: raw frames arrive unchanged.
    let mut buf = [0u8; 2048];
    let (n, from) = peer.recv_from(&mut buf).expect("fan-out packet");
    let mut frames = Vec::new();
    Framer::new().push(&buf[..n], &mut frames);
    assert!(!frames.is_empty() && frames.iter().all(|f| f.system_id() == 1));

    // Peer → vehicle: a QGroundControl command reaches the fake vehicle.
    let mut qgc = Encoder::new(255, 190);
    let cmd = Msg::COMMAND_LONG(dialect::COMMAND_LONG_DATA {
        command: dialect::MavCmd::MAV_CMD_REQUEST_MESSAGE,
        param1: 148.0,
        target_system: 1,
        target_component: 1,
        ..Default::default()
    });
    peer.send_to(&qgc.encode(&cmd).bytes, from).expect("send");
    let forwarded = wait_for(LIMIT, || {
        let got = fake.received(|m| matches!(m, Msg::COMMAND_LONG(c) if c.param1 == 148.0));
        (!got.is_empty()).then_some(got)
    });
    assert!(forwarded.is_some(), "peer packet never reached the vehicle");
    let snap = snapshot_until(&mut service, |s| s.link.peers[0].rx_frames > 0);
    assert!(snap.link.peers[0].tx_frames > 0);
    // The physical bridge counts what the peer sent the vehicle: exactly the one command.
    assert_eq!(snap.link.peer_to_vehicle_frames, 1);
    assert!(snap.link.tx_frames > snap.link.peer_to_vehicle_frames);

    drop(service);
    let summary = summarize_tlog(&tlog_path).expect("tlog");
    assert!(summary.frames > 5);
    assert_eq!(summary.vehicle.as_ref().map(|v| v.0), Some(1));
    assert!(summary.message_counts.contains_key("COMMAND_LONG"));
    assert!(summary.end_us >= summary.start_us && summary.start_us > 1_600_000_000_000_000);
    assert_eq!(summary.max_relative_alt, Some(12.5));
}

#[test]
fn commands_are_acknowledged() {
    let fake = FakeVehicle::tcp();
    let mut service = connected(&fake, LinkOrigin::SimulatorRouter);
    let id = service.command(CommandKind::Arm, sim()).expect("allowed");
    let result = event_until(&mut service, |e| match e {
        Event::Command {
            id: got, result, ..
        } if *got == id => Some(result.clone()),
        _ => None,
    });
    assert_eq!(result, CommandResult::Accepted);
    let arm = fake.received(|m| matches!(m, Msg::COMMAND_LONG(c) if c.command == dialect::MavCmd::MAV_CMD_COMPONENT_ARM_DISARM));
    let Msg::COMMAND_LONG(c) = &arm[0] else {
        unreachable!()
    };
    assert_eq!((c.param1, c.target_system, c.target_component), (1.0, 1, 1));

    let id = service
        .command(CommandKind::SetMode("Mission".into()), sim())
        .expect("allowed");
    let result = event_until(&mut service, |e| match e {
        Event::Command {
            id: got, result, ..
        } if *got == id => Some(result.clone()),
        _ => None,
    });
    assert_eq!(result, CommandResult::Accepted);
    let modes = fake.received(
        |m| matches!(m, Msg::COMMAND_LONG(c) if c.command == dialect::MavCmd::MAV_CMD_DO_SET_MODE),
    );
    let Msg::COMMAND_LONG(c) = &modes[0] else {
        unreachable!()
    };
    assert_eq!((c.param1, c.param2, c.param3), (1.0, 4.0, 4.0));

    fake.with(|s| s.ack = Some(dialect::MavResult::MAV_RESULT_DENIED));
    let id = service.command(CommandKind::Land, sim()).expect("allowed");
    let result = event_until(&mut service, |e| match e {
        Event::Command {
            id: got, result, ..
        } if *got == id => Some(result.clone()),
        _ => None,
    });
    assert_eq!(
        result,
        CommandResult::Rejected {
            result: 2,
            reason: "Denied"
        }
    );
}

#[test]
fn unacknowledged_commands_retry_then_time_out() {
    let fake = FakeVehicle::tcp();
    fake.with(|s| s.ack = None);
    let config = Config::new(tcp(&fake), LinkOrigin::SimulatorRouter)
        .command_retry(Duration::from_millis(200), 3);
    let mut service = Service::start(config).expect("start");
    snapshot_until(&mut service, |s| s.fresh);
    let id = service
        .command(CommandKind::ReturnToLaunch, sim())
        .expect("allowed");
    let busy = service
        .command(CommandKind::ReturnToLaunch, sim())
        .expect("allowed");
    let mut results = Vec::new();
    wait_for(LIMIT, || {
        for e in service.poll().events {
            if let Event::Command { id, result, .. } = e {
                results.push((id, result));
            }
        }
        (results.len() == 2).then_some(())
    })
    .expect("two results");
    assert!(results.contains(&(busy, CommandResult::Busy)));
    assert!(results.contains(&(id, CommandResult::TimedOut { attempts: 3 })));
    let sent = fake.received(|m| matches!(m, Msg::COMMAND_LONG(c) if c.command == dialect::MavCmd::MAV_CMD_NAV_RETURN_TO_LAUNCH));
    let confirmations: Vec<u8> = sent
        .iter()
        .map(|m| match m {
            Msg::COMMAND_LONG(c) => c.confirmation,
            _ => 99,
        })
        .collect();
    assert_eq!(confirmations, vec![0, 1, 2]);
}

#[test]
fn authority_gates_flight_commands_and_the_shell() {
    let fake = FakeVehicle::tcp();
    let physical = connected(&fake, LinkOrigin::Physical);
    assert_eq!(
        physical.command(CommandKind::Arm, Authority::ReadOnly),
        Err(Refusal::PhysicalLink)
    );
    assert_eq!(
        physical.command(CommandKind::Arm, sim()),
        Err(Refusal::PhysicalLink)
    );
    assert_eq!(
        physical.shell("reboot\n", sim()),
        Err(Refusal::PhysicalLink)
    );
    assert!(physical.fetch_params().is_ok());
    assert!(physical.list_logs().is_ok());
    drop(physical);
    drop(fake);

    let fake = FakeVehicle::tcp();
    let sim_link = connected(&fake, LinkOrigin::SimulatorRouter);
    assert_eq!(
        sim_link.command(CommandKind::Arm, Authority::ReadOnly),
        Err(Refusal::ReadOnly)
    );
    let partial = Authority::from_evidence(SimulationEvidence {
        app_router_link: true,
        simulator_source: true,
        stack_owned_running: false,
    });
    assert_eq!(
        sim_link.command(CommandKind::Takeoff { alt: 5.0 }, partial),
        Err(Refusal::ReadOnly)
    );
    assert_eq!(sim_link.shell("ls\n", partial), Err(Refusal::ReadOnly));
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        fake.received(|m| match m {
            Msg::COMMAND_LONG(_) => !is_home_request(m),
            Msg::SERIAL_CONTROL(_) => true,
            _ => false,
        })
        .is_empty(),
        "a refused request reached the vehicle"
    );
}

fn requests_home(c: &dialect::COMMAND_LONG_DATA) -> bool {
    c.command == dialect::MavCmd::MAV_CMD_REQUEST_MESSAGE && c.param1 == 242.0
}

#[allow(deprecated)]
const GET_HOME: dialect::MavCmd = dialect::MavCmd::MAV_CMD_GET_HOME_POSITION;

fn home_of(snap: &VehicleSnapshot) -> Option<(f64, f64, f32)> {
    snap.vehicle.as_ref().and_then(|v| v.home)
}

#[test]
fn a_found_vehicle_is_asked_for_home_once_even_on_a_physical_link() {
    let fake = FakeVehicle::tcp();
    let mut service = connected(&fake, LinkOrigin::Physical);
    let snap = snapshot_until(&mut service, |s| home_of(s).is_some());
    let (lat, lon, alt) = home_of(&snap).expect("home");
    assert!((lat - 47.3977).abs() < 1e-9 && (lon - 8.5456).abs() < 1e-9);
    assert!((alt - 498.0).abs() < 1e-3);
    std::thread::sleep(Duration::from_millis(400));
    let asked = fake.received(|m| matches!(m, Msg::COMMAND_LONG(c) if requests_home(c)));
    assert_eq!(asked.len(), 1, "asked once: {asked:?}");
    let Msg::COMMAND_LONG(c) = &asked[0] else {
        unreachable!()
    };
    assert_eq!((c.target_system, c.target_component), (1, 1));
    assert!(
        fake.received(|m| matches!(m, Msg::COMMAND_LONG(c) if c.command == GET_HOME))
            .is_empty()
    );
    // The read request opens nothing else: flight commands and the console stay refused.
    assert_eq!(
        service.command(CommandKind::Arm, sim()),
        Err(Refusal::PhysicalLink)
    );
    assert_eq!(service.shell("ls\n", sim()), Err(Refusal::PhysicalLink));
    assert_eq!(snap.link.peer_to_vehicle_frames, 0);
}

#[test]
fn a_vehicle_that_refuses_request_message_gets_one_get_home_position() {
    let fake = FakeVehicle::tcp();
    fake.with(|s| s.request_message = false);
    let mut service = connected(&fake, LinkOrigin::Physical);
    snapshot_until(&mut service, |s| home_of(s).is_some());
    std::thread::sleep(Duration::from_millis(400));
    let fallback = fake.received(|m| matches!(m, Msg::COMMAND_LONG(c) if c.command == GET_HOME));
    assert_eq!(fallback.len(), 1);
    assert_eq!(
        fake.received(|m| matches!(m, Msg::COMMAND_LONG(c) if requests_home(c)))
            .len(),
        1
    );
}

#[test]
fn a_vehicle_without_a_home_is_asked_once_and_left_alone() {
    let fake = FakeVehicle::tcp();
    fake.with(|s| s.home = None);
    let mut service = connected(&fake, LinkOrigin::SimulatorRouter);
    wait_for(LIMIT, || {
        (!fake
            .received(|m| matches!(m, Msg::COMMAND_ACK(_) | Msg::COMMAND_LONG(_)))
            .is_empty())
        .then_some(())
    })
    .expect("home request");
    std::thread::sleep(Duration::from_millis(500));
    service.poll();
    assert!(home_of(&service.snapshot()).is_none(), "no home made up");
    let asked = fake.received(|m| matches!(m, Msg::COMMAND_LONG(_)));
    assert_eq!(
        asked.len(),
        1,
        "an accepted request is not repeated: {asked:?}"
    );
}

#[test]
fn peers_on_a_simulator_link_are_not_counted_as_physical_bridge_traffic() {
    let fake = FakeVehicle::tcp();
    let peer = UdpSocket::bind("127.0.0.1:0").expect("peer");
    peer.set_read_timeout(Some(Duration::from_millis(200)))
        .expect("timeout");
    let config =
        Config::new(tcp(&fake), LinkOrigin::SimulatorRouter).peer(peer.local_addr().expect("addr"));
    let mut service = Service::start(config).expect("start");
    snapshot_until(&mut service, |s| s.fresh);
    let mut buf = [0u8; 2048];
    let (_, from) = peer.recv_from(&mut buf).expect("fan-out packet");
    let beat = Msg::HEARTBEAT(dialect::HEARTBEAT_DATA {
        mavtype: dialect::MavType::MAV_TYPE_GCS,
        autopilot: dialect::MavAutopilot::MAV_AUTOPILOT_INVALID,
        ..Default::default()
    });
    peer.send_to(&Encoder::new(255, 190).encode(&beat).bytes, from)
        .expect("send");
    let snap = snapshot_until(&mut service, |s| s.link.peers[0].rx_frames > 0);
    assert_eq!(snap.link.peer_to_vehicle_frames, 0);
}

#[test]
fn params_fill_gaps_and_export() {
    let fake = FakeVehicle::tcp();
    fake.with(|s| s.drop_params = [3u16, 17, 42].into_iter().collect());
    let mut service = connected(&fake, LinkOrigin::Physical);
    service.fetch_params().expect("fetch");
    let (set, missing) = event_until(&mut service, |e| match e {
        Event::Params { set, missing } => Some((set.clone(), missing.clone())),
        _ => None,
    });
    assert!(missing.is_empty());
    assert_eq!(set.params.len(), 50);
    assert_eq!(set.params["PARAM_04"].value, ParamValue::Int(12));
    assert_eq!(set.params["PARAM_17"].value, ParamValue::Real(8.5));
    let reads = fake.received(|m| matches!(m, Msg::PARAM_REQUEST_READ(_)));
    let mut indices: Vec<i16> = reads
        .iter()
        .filter_map(|m| match m {
            Msg::PARAM_REQUEST_READ(r) => Some(r.param_index),
            _ => None,
        })
        .collect();
    indices.sort();
    assert_eq!(indices, vec![3, 17, 42]);

    let text = set.to_qgc();
    assert!(text.contains("# Stack: PX4 Pro"));
    assert!(text.contains("1\t1\tPARAM_04\t12\t6\n"));
    let (parsed, bad) = ParamSet::from_qgc(&text);
    assert!(bad.is_empty());
    assert!(diff_params(&set, &parsed).is_empty());
}

#[test]
fn log_download_resumes_after_an_interruption() {
    let fake = FakeVehicle::tcp();
    let dir = scratch("logs");
    let mut service = connected(&fake, LinkOrigin::Physical);
    service.list_logs().expect("list");
    let (entries, complete) = event_until(&mut service, |e| match e {
        Event::LogList { entries, complete } => Some((entries.clone(), *complete)),
        _ => None,
    });
    assert!(complete);
    assert_eq!(entries.iter().map(|e| e.id).collect::<Vec<_>>(), vec![1, 2]);
    let entry = entries[1].clone();
    assert_eq!(entry.size, 20_000);

    // First attempt: the link dies at 9,000 bytes; one chunk before it is lost and re-asked.
    fake.with(|s| {
        s.log_cut = Some(9_000);
        s.log_drop_once = [900u32].into_iter().collect();
    });
    service
        .download_log(entry.clone(), dir.clone())
        .expect("download");
    snapshot_until(&mut service, |s| {
        s.log_progress.is_some_and(|(_, got, _)| got >= 9_000)
    });
    service.cancel_log().expect("cancel");
    let reason = event_until(&mut service, |e| match e {
        Event::LogFailed { reason, .. } => Some(reason.clone()),
        _ => None,
    });
    assert!(reason.contains("resume"));
    let part = std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with(".part"))
        .expect("part file");
    assert_eq!(part.metadata().expect("meta").len(), 9_000);

    // Second attempt starts at the end of the .part file and finishes.
    fake.with(|s| {
        s.log_cut = None;
        s.received.clear();
    });
    service
        .download_log(entry.clone(), dir.clone())
        .expect("download");
    let path = event_until(&mut service, |e| match e {
        Event::LogDone { path, .. } => Some(path.clone()),
        Event::LogFailed { reason, .. } => panic!("{reason}"),
        _ => None,
    });
    let first = fake
        .received(|m| matches!(m, Msg::LOG_REQUEST_DATA(_)))
        .into_iter()
        .find_map(|m| match m {
            Msg::LOG_REQUEST_DATA(r) => Some(r.ofs),
            _ => None,
        });
    assert_eq!(first, Some(9_000));
    assert!(path.to_string_lossy().ends_with(".ulg"));
    let bytes = std::fs::read(&path).expect("log");
    assert_eq!(bytes, fake.with(|s| s.log.clone()));
    assert!(
        !fake
            .received(|m| matches!(m, Msg::LOG_REQUEST_END(_)))
            .is_empty()
    );
}

#[test]
fn vehicle_goes_stale_without_heartbeats() {
    let fake = FakeVehicle::tcp();
    let config =
        Config::new(tcp(&fake), LinkOrigin::Physical).stale_after(Duration::from_millis(600));
    let mut service = Service::start(config).expect("start");
    snapshot_until(&mut service, |s| s.fresh);
    fake.with(|s| s.heartbeats = false);
    let snap = snapshot_until(&mut service, |s| !s.fresh);
    assert!(snap.vehicle.is_some(), "the last known state stays visible");
    fake.with(|s| s.heartbeats = true);
    snapshot_until(&mut service, |s| s.fresh);
}

#[test]
fn shell_round_trip() {
    let fake = FakeVehicle::tcp();
    let mut service = connected(&fake, LinkOrigin::SimulatorRouter);
    service.shell("ver all\n", sim()).expect("allowed");
    let text = event_until(&mut service, |e| match e {
        Event::Shell(t) => Some(t.clone()),
        _ => None,
    });
    assert_eq!(text, "echo: ver all\n");
    let sent = fake.received(|m| matches!(m, Msg::SERIAL_CONTROL(_)));
    let Msg::SERIAL_CONTROL(c) = &sent[0] else {
        unreachable!()
    };
    assert_eq!(
        c.device,
        dialect::SerialControlDev::SERIAL_CONTROL_DEV_SHELL
    );
    assert_eq!(&c.data[..c.count as usize], b"ver all\n");
}

#[test]
fn udp_listen_link_learns_the_vehicle_address() {
    let (_one_at_a_time, port) = super::fake_vehicle::udp_listen_port();
    let endpoint: Endpoint = format!("udp:127.0.0.1:{port}").parse().expect("endpoint");
    let mut service =
        Service::start(Config::new(endpoint, LinkOrigin::SimulatorRouter)).expect("start");
    let fake = FakeVehicle::udp(([127, 0, 0, 1], port).into());
    snapshot_until(&mut service, |s| s.fresh);
    let id = service.command(CommandKind::Hold, sim()).expect("allowed");
    let result = event_until(&mut service, |e| match e {
        Event::Command {
            id: got, result, ..
        } if *got == id => Some(result.clone()),
        _ => None,
    });
    assert_eq!(result, CommandResult::Accepted);
    let heartbeats = wait_for(LIMIT, || {
        let got = fake.received(
            |m| matches!(m, Msg::HEARTBEAT(h) if h.mavtype == dialect::MavType::MAV_TYPE_GCS),
        );
        (!got.is_empty()).then_some(got)
    });
    assert!(
        heartbeats.is_some(),
        "the service's GCS heartbeat reached the vehicle"
    );
}

#[test]
fn unreachable_link_reports_down_and_keeps_trying() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("free port")
        .port();
    let endpoint: Endpoint = format!("tcp:127.0.0.1:{port}").parse().expect("endpoint");
    let mut service = Service::start(Config::new(endpoint, LinkOrigin::Physical)).expect("start");
    let state = event_until(&mut service, |e| match e {
        Event::Link(LinkState::Down(reason)) => Some(reason.clone()),
        _ => None,
    });
    assert!(state.contains(&port.to_string()));
    let unavailable = {
        service.fetch_params().expect("queued");
        event_until(&mut service, |e| match e {
            Event::Unavailable(r) => Some(r.clone()),
            _ => None,
        })
    };
    assert!(unavailable.contains("No vehicle"));
}

#[test]
fn only_local_network_links_can_be_simulator_routers() {
    let origin = |text: &str| {
        let endpoint: Endpoint = text.parse().expect("endpoint");
        Config::new(endpoint, LinkOrigin::SimulatorRouter).origin
    };
    assert_eq!(origin("tcp:127.0.0.1:5760"), LinkOrigin::SimulatorRouter);
    assert_eq!(origin("tcp:localhost:5760"), LinkOrigin::SimulatorRouter);
    assert_eq!(origin("tcp:[::1]:5760"), LinkOrigin::SimulatorRouter);
    assert_eq!(origin("serial:COM3:57600"), LinkOrigin::Physical);
    assert_eq!(origin("tcp:192.168.4.1:5760"), LinkOrigin::Physical);
    assert_eq!(origin("udp:0.0.0.0:14550"), LinkOrigin::Physical);
}

#[test]
fn shell_close_writes_only_after_the_shell_was_opened() {
    let fake = FakeVehicle::tcp();
    let physical = connected(&fake, LinkOrigin::Physical);
    physical.shell_close().expect("queued");
    drop(physical);
    drop(fake);

    let fake = FakeVehicle::tcp();
    let mut service = connected(&fake, LinkOrigin::SimulatorRouter);
    service.shell_close().expect("queued");
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        fake.received(|m| matches!(m, Msg::SERIAL_CONTROL(_)))
            .is_empty(),
        "close sent to a shell nobody opened"
    );
    service.shell("ver\n", sim()).expect("allowed");
    event_until(&mut service, |e| matches!(e, Event::Shell(_)).then_some(()));
    service.shell_close().expect("queued");
    let closes = wait_for(LIMIT, || {
        let got = fake.received(|m| matches!(m, Msg::SERIAL_CONTROL(c) if c.count == 0));
        (!got.is_empty()).then_some(got.len())
    });
    assert_eq!(closes, Some(1));
}

#[test]
fn a_vehicle_without_parameters_is_not_reported_complete() {
    let fake = FakeVehicle::tcp();
    fake.with(|s| s.params.clear());
    let mut service = connected(&fake, LinkOrigin::Physical);
    service.fetch_params().expect("fetch");
    let outcome = event_until(&mut service, |e| match e {
        Event::Params { set, missing } => Some(Err((set.params.len(), missing.len()))),
        Event::Unavailable(reason) => Some(Ok(reason.clone())),
        _ => None,
    });
    assert!(
        outcome.is_ok_and(|r| r.contains("no parameters")),
        "an empty set read as complete"
    );
}
