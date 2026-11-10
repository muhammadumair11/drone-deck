//! What reaches the vehicle, and from whom: fan-out peers only forward their own datagrams, a
//! UDP listener stays with its vehicle, a capture problem never costs the link, a stopped
//! service frees its port at once, and one-shot events are never dropped.
use super::engine::Core;
use super::fake_vehicle::{FakeVehicle, scratch, udp_listen_port, wait_for};
use super::frame::{Encoder, Msg, dialect};
use super::service::Shared;
use super::*;
use std::collections::VecDeque;
use std::net::UdpSocket;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(10);

fn snapshot_until(
    service: &mut Service,
    check: impl Fn(&VehicleSnapshot) -> bool,
) -> VehicleSnapshot {
    wait_for(LIMIT, || service.poll().snapshot.filter(|s| check(s))).expect("snapshot condition")
}

fn request_message(param1: f32) -> Msg {
    Msg::COMMAND_LONG(dialect::COMMAND_LONG_DATA {
        command: dialect::MavCmd::MAV_CMD_REQUEST_MESSAGE,
        param1,
        target_system: 1,
        target_component: 1,
        ..Default::default()
    })
}

#[test]
fn a_peer_socket_forwards_only_its_own_peer() {
    let fake = FakeVehicle::tcp();
    let peer = UdpSocket::bind("127.0.0.1:0").expect("peer");
    peer.set_read_timeout(Some(Duration::from_millis(200)))
        .expect("timeout");
    let endpoint: Endpoint = format!("tcp:127.0.0.1:{}", fake.addr.port())
        .parse()
        .expect("endpoint");
    let config = Config::new(endpoint, LinkOrigin::Physical).peer(peer.local_addr().expect("addr"));
    let mut service = Service::start(config).expect("start");
    snapshot_until(&mut service, |s| s.fresh);
    let mut buf = [0u8; 2048];
    let (_, service_port) = peer.recv_from(&mut buf).expect("fan-out packet");

    // Another program that finds the peer socket's port is ignored.
    let stray = UdpSocket::bind("127.0.0.1:0").expect("stray");
    let mut other = Encoder::new(200, 1);
    stray
        .send_to(&other.encode(&request_message(111.0)).bytes, service_port)
        .expect("send");
    let mut qgc = Encoder::new(255, 190);
    peer.send_to(&qgc.encode(&request_message(148.0)).bytes, service_port)
        .expect("send");
    let forwarded = wait_for(LIMIT, || {
        let got = fake.received(|m| matches!(m, Msg::COMMAND_LONG(c) if c.param1 == 148.0));
        (!got.is_empty()).then_some(())
    });
    assert!(forwarded.is_some(), "the peer's own command is bridged");
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        fake.received(|m| matches!(m, Msg::COMMAND_LONG(c) if c.param1 == 111.0))
            .is_empty(),
        "a stray sender never reaches the vehicle"
    );
    let snap = snapshot_until(&mut service, |s| s.link.peers[0].rx_frames > 0);
    assert_eq!(snap.link.peer_to_vehicle_frames, 1);
}

#[test]
fn a_udp_listener_stays_with_its_vehicle() {
    let (_one_at_a_time, port) = udp_listen_port();
    let endpoint: Endpoint = format!("udp:127.0.0.1:{port}").parse().expect("endpoint");
    let mut service = Service::start(Config::new(endpoint, LinkOrigin::Physical)).expect("start");
    let fake = FakeVehicle::udp(([127, 0, 0, 1], port).into());
    snapshot_until(&mut service, |s| s.fresh);

    // A second sender, noise first and then a well-formed autopilot heartbeat.
    let stray = UdpSocket::bind("127.0.0.1:0").expect("stray");
    stray
        .set_read_timeout(Some(Duration::from_millis(100)))
        .expect("timeout");
    stray
        .send_to(b"not mavlink", ("127.0.0.1", port))
        .expect("send");
    let beat = Msg::HEARTBEAT(dialect::HEARTBEAT_DATA {
        mavtype: dialect::MavType::MAV_TYPE_QUADROTOR,
        autopilot: dialect::MavAutopilot::MAV_AUTOPILOT_PX4,
        ..Default::default()
    });
    stray
        .send_to(&Encoder::new(7, 1).encode(&beat).bytes, ("127.0.0.1", port))
        .expect("send");
    let snap = snapshot_until(&mut service, |s| s.link.bad_frames >= 2);
    assert_eq!(
        snap.vehicle.as_ref().map(|v| v.system_id),
        Some(1),
        "the stray heartbeat is not the vehicle"
    );
    assert!(snap.systems.iter().all(|s| s.system_id != 7));

    // Replies still go to the vehicle, never to the stray sender.
    let asked = fake
        .received(|m| matches!(m, Msg::PARAM_REQUEST_LIST(_)))
        .len();
    service.fetch_params().expect("queued");
    let reached = wait_for(LIMIT, || {
        let n = fake
            .received(|m| matches!(m, Msg::PARAM_REQUEST_LIST(_)))
            .len();
        (n > asked).then_some(())
    });
    assert!(
        reached.is_some(),
        "the parameter request reached the vehicle"
    );
    let mut buf = [0u8; 2048];
    assert!(
        stray.recv_from(&mut buf).is_err(),
        "nothing is sent to the stray sender"
    );
}

#[test]
fn a_capture_that_cannot_be_created_leaves_the_link_open() {
    let fake = FakeVehicle::tcp();
    let dir = scratch("tlog-blocked");
    let blocker = dir.join("not-a-folder");
    std::fs::write(&blocker, b"a file where the capture folder would go").expect("seed");
    let endpoint: Endpoint = format!("tcp:127.0.0.1:{}", fake.addr.port())
        .parse()
        .expect("endpoint");
    let config = Config::new(endpoint, LinkOrigin::Physical).tlog(blocker.join("x.tlog"));
    let mut service = Service::start(config).expect("the link starts without a capture");
    let snap = snapshot_until(&mut service, |s| s.fresh);
    assert!(snap.link.tlog.is_none());
    let error = snap
        .link
        .tlog_error
        .expect("the capture problem is reported");
    assert!(error.starts_with("Capture not started"), "{error}");
}

#[test]
fn a_stopped_udp_listener_frees_its_port_for_the_next_link() {
    let (_one_at_a_time, port) = udp_listen_port();
    let endpoint: Endpoint = format!("udp:127.0.0.1:{port}").parse().expect("endpoint");
    let mut first =
        Service::start(Config::new(endpoint.clone(), LinkOrigin::Physical)).expect("start");
    snapshot_until(&mut first, |s| s.link.state == LinkState::Connected);
    first.stop();
    let mut second = Service::start(Config::new(endpoint, LinkOrigin::Physical)).expect("start");
    let state = wait_for(LIMIT, || {
        second.poll().events.into_iter().find_map(|e| match e {
            Event::Link(state @ (LinkState::Connected | LinkState::Down(_))) => Some(state),
            _ => None,
        })
    });
    assert_eq!(
        state,
        Some(LinkState::Connected),
        "no 'address in use' retry"
    );
}

#[test]
fn one_shot_events_survive_a_full_queue() {
    let endpoint: Endpoint = "tcp:127.0.0.1:9".parse().expect("endpoint");
    let (_, snapshot) = Core::new(&Config::new(endpoint, LinkOrigin::Physical), Vec::new());
    let shared = Shared {
        snapshot: Mutex::new(snapshot),
        version: AtomicU64::new(0),
        requests: Mutex::new(VecDeque::new()),
        events: Mutex::new(VecDeque::new()),
        stop: AtomicBool::new(false),
    };
    let progress = || Event::ParamProgress {
        received: 1,
        total: 2,
    };
    let mut accepted = 0;
    while shared.push_event(progress()) {
        accepted += 1;
        assert!(accepted <= 10_000, "progress is bounded");
    }
    let done = Event::Params {
        set: ParamSet::default(),
        missing: Vec::new(),
    };
    assert!(
        shared.push_event(done),
        "a finished download is never dropped"
    );
    assert!(shared.push_event(Event::LogFailed {
        id: 1,
        reason: "cut".into()
    }));
    assert!(!shared.push_event(progress()));
    let queue = shared.events.lock().unwrap();
    assert!(matches!(queue.back(), Some(Event::LogFailed { .. })));
}
