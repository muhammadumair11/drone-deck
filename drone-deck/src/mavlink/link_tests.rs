use super::*;

#[test]
fn parses_and_prints_endpoints() {
    let cases = [
        (
            "tcp:127.0.0.1:5760",
            Endpoint::Tcp {
                host: "127.0.0.1".into(),
                port: 5760,
            },
        ),
        (
            "udp:0.0.0.0:14550",
            Endpoint::UdpListen {
                host: "0.0.0.0".into(),
                port: 14550,
            },
        ),
        (
            "udpout:127.0.0.1:14540",
            Endpoint::UdpOut {
                host: "127.0.0.1".into(),
                port: 14540,
            },
        ),
        (
            "serial:COM3:57600",
            Endpoint::Serial {
                port: "COM3".into(),
                baud: 57600,
            },
        ),
        (
            "serial:/dev/ttyACM0:115200",
            Endpoint::Serial {
                port: "/dev/ttyACM0".into(),
                baud: 115200,
            },
        ),
        (
            "tcp:[::1]:5760",
            Endpoint::Tcp {
                host: "::1".into(),
                port: 5760,
            },
        ),
    ];
    for (text, endpoint) in cases {
        let parsed: Endpoint = text.parse().expect(text);
        assert_eq!(parsed, endpoint);
        assert_eq!(parsed.to_string(), text);
    }
    assert_eq!(
        "udpin:0.0.0.0:14550"
            .parse::<Endpoint>()
            .map(|e| e.to_string()),
        Ok("udp:0.0.0.0:14550".into())
    );
    for bad in [
        "",
        "tcp",
        "tcp:5760",
        "tcp:host:0",
        "tcp:host:70000",
        "ftp:host:21",
        "serial:COM3:fast",
        "udp::14550",
    ] {
        assert!(bad.parse::<Endpoint>().is_err(), "{bad}");
    }
}

#[test]
fn udp_writer_answers_the_last_sender() {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
    let mut w = Writer::Udp { socket, peer: None };
    assert!(w.send(b"nobody yet").is_ok());
    let from: SocketAddr = "127.0.0.1:9".parse().expect("addr");
    w.learn(Some(from), false);
    assert!(
        matches!(&w, Writer::Udp { peer: None, .. }),
        "udpout keeps its fixed peer"
    );
    w.learn(Some(from), true);
    assert!(matches!(&w, Writer::Udp { peer: Some(p), .. } if *p == from));
}

#[test]
fn a_broadcast_peer_takes_answers_from_the_network_but_not_from_this_computer() {
    let broadcast: SocketAddr = "255.255.255.255:14550".parse().unwrap();
    let scripts: SocketAddr = "127.0.0.1:14540".parse().unwrap();
    let phone: SocketAddr = "192.168.100.4:14550".parse().unwrap();
    let here: SocketAddr = "192.168.100.10:14550".parse().unwrap();
    let own = |ip: IpAddr| ip == here.ip() || ip.is_loopback();
    assert!(is_broadcast(broadcast) && !is_broadcast(scripts));
    assert!(
        peer_accepts(broadcast, phone, own),
        "a phone that picked up the broadcast"
    );
    assert!(
        !peer_accepts(broadcast, here, own),
        "QGroundControl's echo from this computer"
    );
    assert!(!peer_accepts(
        broadcast,
        "[::1]:14550".parse().unwrap(),
        own
    ));
    assert!(
        peer_accepts(scripts, scripts, own),
        "a unicast peer answers from its own address"
    );
    assert!(!peer_accepts(scripts, phone, own));
}

#[test]
fn a_broadcast_peer_socket_may_send_broadcasts() {
    let socket = udp_for("255.255.255.255:14550".parse().unwrap()).unwrap();
    assert!(socket.broadcast().unwrap());
    let local = socket.local_addr().unwrap().ip();
    assert!(!local.is_loopback(), "reaches the network");
    if let Some(lan) = lan_address() {
        assert_eq!(
            local,
            IpAddr::V4(lan),
            "bound to the default route's network, not a virtual adapter"
        );
    }
    let scripts = udp_for("127.0.0.1:14540".parse().unwrap()).unwrap();
    assert!(!scripts.broadcast().unwrap());
    assert!(is_own("127.0.0.1".parse().unwrap()));
    assert!(
        !is_own("203.0.113.7".parse().unwrap()),
        "a documentation address is never ours"
    );
}

#[test]
fn a_broadcast_peer_has_a_loopback_sink_on_its_port() {
    // QGroundControl here answers the broadcast at 127.0.0.1 on this port; unbound, Windows
    // answers "port unreachable" and QGroundControl drops its vehicle.
    let broadcast = udp_for("255.255.255.255:14550".parse().unwrap()).unwrap();
    let sink = loopback_sink(&broadcast).unwrap();
    let (b, s) = (broadcast.local_addr().unwrap(), sink.local_addr().unwrap());
    assert_eq!(s.port(), b.port());
    assert!(s.ip().is_loopback());
    let qgc = UdpSocket::bind("127.0.0.1:0").unwrap();
    qgc.send_to(b"hb", s).unwrap();
    let mut buf = [0u8; 8];
    assert_eq!(
        sink.recv_from(&mut buf).unwrap().0,
        2,
        "the answer lands instead of bouncing"
    );
}
