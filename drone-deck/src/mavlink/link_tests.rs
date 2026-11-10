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
