use super::*;
#[test]
fn headless_previews_never_connect_a_physical_vehicle() {
    let mut s = State::new();
    s.select_system(systems::PHYSICAL);
    s.hardware.endpoint = TextEdit::new("udp:127.0.0.1:0");
    toggle(&mut s);
    assert!(s.vehicle.link.is_none());
    assert!(s.hardware.message.contains("Preview"));
    assert!(s.status.is_empty());
}

/// The physical bridge is transparent, so the panel splits what reached the vehicle: Drone
/// Deck's own read requests and what QGroundControl or scripts sent through it.
#[test]
fn the_connection_panel_shows_what_peers_sent_a_physical_vehicle() {
    use rune::{Env, Runtime};
    let texts = |peers: bool, bridged: u64| {
        let mut s = State::new();
        s.select_system(systems::PHYSICAL);
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
