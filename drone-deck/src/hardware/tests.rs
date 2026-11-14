use super::*;
/// The physical bridge is transparent, so the panel splits what reached the vehicle: Drone
/// Deck's own read requests and what QGroundControl or scripts sent through it.
#[test]
fn the_connection_panel_shows_what_peers_sent_a_physical_vehicle() {
    use rune::{Env, Runtime};
    let texts = |peers: bool, bridged: u64| {
        let mut s = State::new();
        s.select_starter(systems::PHYSICAL);
        let id = s.systems.active_id().to_string();
        crate::vehicle::preview(&mut s, &id, true, false);
        let link = &mut s.vehicle.snapshot.as_mut().expect("snapshot").link;
        if peers {
            link.peers.push(crate::mavlink::PeerStatus {
                addr: "127.0.0.1:14550".parse().ok(),
                rx_frames: bridged,
                ..Default::default()
            });
        }
        link.tx_frames = 3 + bridged;
        link.peer_to_vehicle_frames = bridged;
        let mut rt = Runtime::new(
            s,
            crate::view,
            (1440., 900.),
            Env {
                scale: 1.,
                system_dark: true,
            },
        );
        rt.update(|_, tx| tx.scroll_into_view("hardware-readings"));
        for _ in 0..10 {
            rt.frame(0.1);
        }
        let shown = rt.find("link-peer-to-vehicle").is_some();
        let texts: Vec<String> = rt.elements().into_iter().filter_map(|e| e.text).collect();
        (shown, texts)
    };
    let (shown, t) = texts(true, 42);
    assert!(shown);
    assert!(t.iter().any(|x| x == "Sent by QGroundControl/scripts"));
    assert!(t.iter().any(|x| x == "42 frames"));
    assert!(t.iter().any(|x| x == "Sent by Drone Deck"));
    assert!(t.iter().any(|x| x == "3 read requests"), "{t:?}");
    let (shown, t) = texts(false, 0);
    assert!(!shown, "no peers, no bridge figure");
    assert!(t.iter().any(|x| x == "3 read requests"));
}

#[test]
fn endpoints_are_built_from_a_port_and_baud() {
    assert_eq!(ports::endpoint("COM4", 115_200), "serial:COM4:115200");
    assert_eq!(
        ports::serial_of("serial:/dev/ttyACM0:57600"),
        Some(("/dev/ttyACM0".into(), 57_600))
    );
    assert_eq!(ports::serial_of("udp:0.0.0.0:14550"), None);
    for (_, preset) in ports::PRESETS {
        assert!(systems::endpoint_ok(preset), "{preset}");
    }
}

/// Choosing a port, then a baud rate, then a network preset fills the endpoint; nothing is
/// typed and nothing connects.
#[test]
fn the_endpoint_is_chosen_from_ports_bauds_and_presets() {
    use rune::{Env, Runtime};
    let mut s = State::new();
    s.select_starter(systems::PHYSICAL);
    s.hardware.ports.found = Some(vec![ports::Port {
        name: "COM7".into(),
        detail: "Pixhawk 6C / Holybro".into(),
    }]);
    let mut rt = Runtime::new(
        s,
        crate::view,
        (1440., 1400.),
        Env {
            scale: 1.,
            system_dark: true,
        },
    );
    rt.frame(0.);
    let tap = |rt: &mut Runtime<State>, key: &str| {
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
    };
    tap(&mut rt, "endpoint-port-COM7");
    assert_eq!(rt.state.hardware.endpoint.text, "serial:COM7:115200");
    tap(&mut rt, "endpoint-baud-57600");
    assert_eq!(rt.state.hardware.endpoint.text, "serial:COM7:57600");
    tap(&mut rt, "endpoint-preset-0");
    assert_eq!(rt.state.hardware.endpoint.text, "udp:0.0.0.0:14550");
    assert!(
        rt.find("endpoint-baud-57600").is_none(),
        "no baud for a network endpoint"
    );
    assert!(rt.state.vehicle.link.is_none());
}

#[test]
fn every_endpoint_check_refuses_the_same_text() {
    let mut s = State::new();
    s.select_starter(systems::PHYSICAL);
    s.hardware.endpoint = TextEdit::new("udpout:127.0.0.1:14550");
    save_endpoint(&mut s);
    assert!(
        s.hardware.message.contains("is not an endpoint"),
        "{}",
        s.hardware.message
    );
    assert!(
        !systems::endpoint_ok("udpout:127.0.0.1:14550"),
        "Settings refuses it too"
    );
}
