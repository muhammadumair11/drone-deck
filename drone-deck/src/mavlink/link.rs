//! Link endpoints and their I/O threads. Each reader blocks with a short timeout (no polling
//! loops), reconnects on its own, and hands bytes to the service over a bounded channel.
use super::service::Input;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, SyncSender};
use std::thread;
use std::time::Duration;

/// How long a blocking read waits before checking whether to stop.
pub const READ_TIMEOUT: Duration = Duration::from_millis(250);
const RETRY: Duration = Duration::from_secs(1);

/// A MAVLink endpoint, written like `tcp:127.0.0.1:5760`, `udp:0.0.0.0:14550` (listen),
/// `udpout:127.0.0.1:14550` (send to) or `serial:COM3:57600`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    /// Connects to a TCP server (the WSL router, a vehicle's TCP port).
    Tcp {
        host: String,
        port: u16,
    },
    /// Listens on a UDP port and replies to the last sender.
    UdpListen {
        host: String,
        port: u16,
    },
    /// Sends to a fixed UDP address and accepts replies on the same socket.
    UdpOut {
        host: String,
        port: u16,
    },
    Serial {
        port: String,
        baud: u32,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct BadEndpoint(pub String);

impl fmt::Display for BadEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "'{}' isn't an endpoint; use tcp:host:port, udp:host:port, udpout:host:port or serial:PORT:baud",
            self.0
        )
    }
}

impl FromStr for Endpoint {
    type Err = BadEndpoint;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || BadEndpoint(s.to_string());
        let (scheme, rest) = s.trim().split_once(':').ok_or_else(bad)?;
        let (head, tail) = rest.rsplit_once(':').ok_or_else(bad)?;
        if head.is_empty() {
            return Err(bad());
        }
        let host = head
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let port = || tail.parse::<u16>().ok().filter(|&p| p > 0).ok_or_else(bad);
        Ok(match scheme.to_ascii_lowercase().as_str() {
            "tcp" => Self::Tcp {
                host,
                port: port()?,
            },
            "udp" | "udpin" => Self::UdpListen {
                host,
                port: port()?,
            },
            "udpout" => Self::UdpOut {
                host,
                port: port()?,
            },
            "serial" => Self::Serial {
                port: head.to_string(),
                baud: tail.parse().ok().filter(|&b| b > 0).ok_or_else(bad)?,
            },
            _ => return Err(bad()),
        })
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let host = |h: &str| {
            if h.contains(':') {
                format!("[{h}]")
            } else {
                h.to_string()
            }
        };
        match self {
            Self::Tcp { host: h, port } => write!(f, "tcp:{}:{port}", host(h)),
            Self::UdpListen { host: h, port } => write!(f, "udp:{}:{port}", host(h)),
            Self::UdpOut { host: h, port } => write!(f, "udpout:{}:{port}", host(h)),
            Self::Serial { port, baud } => write!(f, "serial:{port}:{baud}"),
        }
    }
}

pub fn resolve(host: &str, port: u16) -> io::Result<SocketAddr> {
    (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{host} has no address")))
}

/// A local UDP socket suited to reaching `target` (loopback stays on loopback).
pub fn udp_for(target: SocketAddr) -> io::Result<UdpSocket> {
    let local: SocketAddr = match (target.ip().is_loopback(), target.is_ipv4()) {
        (true, true) => ([127, 0, 0, 1], 0).into(),
        (false, true) => ([0, 0, 0, 0], 0).into(),
        (true, false) => (std::net::Ipv6Addr::LOCALHOST, 0).into(),
        (false, false) => (std::net::Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    let socket = UdpSocket::bind(local)?;
    socket.set_read_timeout(Some(READ_TIMEOUT))?;
    Ok(socket)
}

/// The service's write half of the vehicle link.
pub enum Writer {
    Tcp(TcpStream),
    Udp {
        socket: UdpSocket,
        peer: Option<SocketAddr>,
    },
    Serial(Box<dyn serialport::SerialPort>),
}

impl Writer {
    pub fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.write_all(bytes).inspect_err(|_| {
                // A write that timed out may have sent part of a frame, and a peer that
                // stopped reading would stall every later write: drop the connection so the
                // reader reports it and reconnects with a fresh writer.
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }),
            Self::Udp { socket, peer } => match peer {
                Some(peer) => socket.send_to(bytes, *peer).map(|_| ()),
                None => Ok(()),
            },
            Self::Serial(port) => port.write_all(bytes),
        }
    }
    /// A UDP listener answers whoever spoke last.
    pub fn learn(&mut self, from: Option<SocketAddr>, listening: bool) {
        if let (Self::Udp { peer, .. }, Some(from), true) = (self, from, listening) {
            *peer = Some(from);
        }
    }
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Sleeps up to `total`, waking early when asked to stop. Returns false when stopping.
fn pause(stop: &AtomicBool, total: Duration) -> bool {
    let step = Duration::from_millis(50);
    let mut left = total;
    while !left.is_zero() {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let nap = left.min(step);
        thread::sleep(nap);
        left -= nap;
    }
    !stop.load(Ordering::Relaxed)
}

/// Starts the reader thread for the vehicle link. `exit` is dropped once the thread has
/// closed its socket or port, so the service can wait for it to be free again.
pub fn spawn_main(
    endpoint: Endpoint,
    tx: SyncSender<Input>,
    stop: Arc<AtomicBool>,
    exit: Sender<()>,
) -> io::Result<()> {
    thread::Builder::new()
        .name("mavlink-link".into())
        .spawn(move || {
            let _exit = exit;
            while !stop.load(Ordering::Relaxed) {
                let reason = match run_once(&endpoint, &tx, &stop) {
                    Ok(()) => return,
                    Err(e) => e,
                };
                if tx.send(Input::Down(reason)).is_err() || !pause(&stop, RETRY) {
                    return;
                }
            }
        })
        .map(drop)
}

/// Connects once and reads until the link fails (`Err(reason)`) or the service stops (`Ok`).
fn run_once(endpoint: &Endpoint, tx: &SyncSender<Input>, stop: &AtomicBool) -> Result<(), String> {
    let describe = |e: io::Error| format!("{endpoint}: {e}");
    match endpoint {
        Endpoint::Tcp { host, port } => {
            let addr = resolve(host, *port).map_err(describe)?;
            let stream =
                TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(describe)?;
            stream.set_nodelay(true).map_err(describe)?;
            stream
                .set_read_timeout(Some(READ_TIMEOUT))
                .map_err(describe)?;
            let writer = stream.try_clone().map_err(describe)?;
            writer
                .set_write_timeout(Some(Duration::from_secs(2)))
                .map_err(describe)?;
            if tx.send(Input::Up(Writer::Tcp(writer))).is_err() {
                return Ok(());
            }
            read_stream(stream, tx, stop)
                .map_err(|e| e.unwrap_or_else(|| format!("{endpoint}: closed by peer")))
        }
        Endpoint::Serial { port, baud } => {
            let serial = serialport::new(port, *baud)
                .timeout(READ_TIMEOUT)
                .open()
                .map_err(|e| format!("{endpoint}: {e}"))?;
            let writer = serial.try_clone().map_err(|e| format!("{endpoint}: {e}"))?;
            if tx.send(Input::Up(Writer::Serial(writer))).is_err() {
                return Ok(());
            }
            read_stream(serial, tx, stop)
                .map_err(|e| e.unwrap_or_else(|| format!("{endpoint}: closed")))
        }
        Endpoint::UdpListen { host, port } => {
            let addr = resolve(host, *port).map_err(describe)?;
            let socket = UdpSocket::bind(addr).map_err(describe)?;
            socket
                .set_read_timeout(Some(READ_TIMEOUT))
                .map_err(describe)?;
            let writer = socket.try_clone().map_err(describe)?;
            let up = Writer::Udp {
                socket: writer,
                peer: None,
            };
            if tx.send(Input::Up(up)).is_err() {
                return Ok(());
            }
            read_datagrams(&socket, tx, stop, |data, from| {
                Some(Input::Main {
                    data,
                    from: Some(from),
                })
            })
            .map_err(describe)
        }
        Endpoint::UdpOut { host, port } => {
            let addr = resolve(host, *port).map_err(describe)?;
            let socket = udp_for(addr).map_err(describe)?;
            let writer = socket.try_clone().map_err(describe)?;
            let up = Writer::Udp {
                socket: writer,
                peer: Some(addr),
            };
            if tx.send(Input::Up(up)).is_err() {
                return Ok(());
            }
            read_datagrams(&socket, tx, stop, |data, from| {
                Some(Input::Main {
                    data,
                    from: Some(from),
                })
            })
            .map_err(describe)
        }
    }
}

/// Reads a byte stream. `Err(None)` = closed by the peer, `Err(Some(reason))` = error.
fn read_stream(
    mut source: impl Read,
    tx: &SyncSender<Input>,
    stop: &AtomicBool,
) -> Result<(), Option<String>> {
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        match source.read(&mut buf) {
            Ok(0) => return Err(None),
            Ok(n) => {
                let input = Input::Main {
                    data: buf[..n].to_vec(),
                    from: None,
                };
                if tx.send(input).is_err() {
                    return Ok(());
                }
            }
            Err(e) if is_timeout(&e) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(Some(e.to_string())),
        }
    }
    Ok(())
}

/// Reads datagrams until stopped; `wrap` returns `None` for a datagram to drop. Windows
/// reports ICMP "port unreachable" from an earlier send as `ConnectionReset` on the next
/// receive; that is not a failure of this socket.
pub fn read_datagrams(
    socket: &UdpSocket,
    tx: &SyncSender<Input>,
    stop: &AtomicBool,
    wrap: impl Fn(Vec<u8>, SocketAddr) -> Option<Input>,
) -> io::Result<()> {
    let mut buf = [0u8; 65_536];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                let Some(input) = wrap(buf[..n].to_vec(), from) else {
                    continue;
                };
                if tx.send(input).is_err() {
                    return Ok(());
                }
            }
            Err(e) if is_timeout(&e) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Starts the reader for one fan-out peer socket. Only datagrams from `addr`, the peer
/// itself, are passed on: anything else that finds the socket's port never reaches the
/// vehicle. `exit` is dropped once the socket is closed (see `spawn_main`).
pub fn spawn_peer(
    index: usize,
    addr: SocketAddr,
    socket: UdpSocket,
    tx: SyncSender<Input>,
    stop: Arc<AtomicBool>,
    exit: Sender<()>,
) -> io::Result<()> {
    thread::Builder::new()
        .name(format!("mavlink-peer-{index}"))
        .spawn(move || {
            let _exit = exit;
            let result = read_datagrams(&socket, &tx, &stop, |data, from| {
                (from == addr).then_some(Input::Peer { index, data })
            });
            drop(socket);
            if let Err(e) = result {
                let _ = tx.send(Input::PeerError {
                    index,
                    reason: e.to_string(),
                });
            }
        })
        .map(drop)
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
