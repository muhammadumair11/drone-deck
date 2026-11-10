//! The Windows simulator-port check, off the UI thread. Before a simulator or the app's router
//! starts, nothing else on Windows may accept connections on TCP 5760 (MAVProxy,
//! mavlink-router, Mission Planner…): WSL's localhost forwarding could not take the port and
//! the link would reach that program instead of the simulator. On a free port the probe waits
//! up to [`PROBE_TIMEOUT`] for a refused connect, so it runs on a worker thread; the staged
//! launch waits for its answer and a direct start continues once it arrives.
use super::gate::SIMULATORS;
use super::{sim_port, stage};
use crate::{State, systems};
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// A listener on loopback accepts at once; this only bounds the wait on a free port.
const PROBE_TIMEOUT: Duration = Duration::from_millis(150);
/// A probe answer this recent stands for the port now. The launch and a waiting direct start
/// use it within the same frame it arrives; it only keeps a start from probing twice.
const FRESH: Duration = Duration::from_secs(1);

/// The probe in flight, its last answer, and the profile waiting for it (`Vehicle::port`).
#[derive(Default)]
pub(crate) struct PortCheck {
    probe: Option<(u16, Receiver<bool>)>,
    /// (port, taken, when answered).
    answer: Option<(u16, bool, Instant)>,
    /// A profile started directly (Tools, the runtime list) that starts once the port is free.
    then: Option<Waiting>,
}

/// A direct start waiting for the probe, and the system it was started for.
struct Waiting {
    profile: String,
    system: String,
}

/// What the check knows about the simulator port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Port {
    Free,
    Taken(String),
    /// A probe runs; ask again on a later frame.
    Checking,
}

/// Blocking: whether something on this computer accepts connections on loopback `port`.
/// Windows may let a specific bind succeed while another socket holds 0.0.0.0, so a free bind
/// is followed by a connect.
pub(crate) fn probe(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    match TcpListener::bind(addr) {
        Err(e) => e.kind() == ErrorKind::AddrInUse,
        Ok(listener) => {
            drop(listener);
            TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok()
        }
    }
}

pub(crate) fn taken(port: u16) -> String {
    format!(
        "Something on Windows already listens on TCP {port} (MAVProxy, mavlink-router, Mission Planner…). Stop it first: Drone Deck would connect to it instead of the simulator."
    )
}

/// Whether stage or profile `id` needs the port free before it starts: a simulator that is not
/// already running (ArduPilot serves the port itself) while the app's router does not hold it,
/// or the router unless the agent already runs one (it is joined, and the gate checks that the
/// link is its client). Headless previews never probe.
fn needed(s: &State, id: &str) -> bool {
    if s.headless {
        return false;
    }
    if id == "router" {
        return !stage::router_listening(s);
    }
    SIMULATORS.contains(&id) && !systems::running(s, id) && !stage::router_up(s)
}

/// The port as far as stage or profile `id` is concerned. Starts a probe on a worker thread
/// when no fresh answer exists; `Port::Checking` until it answers.
pub(crate) fn check(s: &mut State, id: &str) -> Port {
    if !needed(s, id) {
        return Port::Free;
    }
    let port = sim_port(s);
    let waker = s.vehicle.waker.clone();
    let c = &mut s.vehicle.port;
    if let Some((p, busy, at)) = c.answer
        && p == port
        && at.elapsed() < FRESH
    {
        return if busy {
            Port::Taken(taken(port))
        } else {
            Port::Free
        };
    }
    if c.probe.as_ref().is_none_or(|(p, _)| *p != port) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(probe(port));
            if let Some(wake) = waker {
                wake();
            }
        });
        c.probe = Some((port, rx));
    }
    Port::Checking
}

/// Why profile `id` must not start now for system `for_system`: another simulator runs
/// (`simulator_conflict`) or the simulator port is taken. While the port is being checked the
/// start is refused with a note and made again once the answer says the port is free.
pub(crate) fn start_conflict(s: &mut State, id: &str, for_system: &str) -> Option<String> {
    if let Some(conflict) = super::simulator_conflict(s, id, for_system) {
        return Some(conflict);
    }
    match check(s, id) {
        Port::Free => None,
        Port::Taken(why) => Some(why),
        Port::Checking => {
            s.vehicle.port.then = Some(Waiting {
                profile: id.to_string(),
                system: for_system.to_string(),
            });
            let name = systems::component(id).map_or(id, |c| c.name);
            Some(format!(
                "Checking that nothing on Windows listens on TCP {} before {name} starts.",
                sim_port(s)
            ))
        }
    }
}

/// Drops a pending direct start (`Stop session`).
pub(crate) fn forget(s: &mut State) {
    s.vehicle.port.then = None;
}

/// Takes the probe's answer and makes a waiting direct start. True when the notice changed.
pub(super) fn poll(s: &mut State) -> bool {
    let c = &mut s.vehicle.port;
    let Some((port, rx)) = &c.probe else {
        return false;
    };
    let port = *port;
    let busy = match rx.try_recv() {
        Ok(busy) => busy,
        Err(TryRecvError::Empty) => return false,
        // The worker died without an answer: treat the port as taken rather than guess.
        Err(TryRecvError::Disconnected) => true,
    };
    c.probe = None;
    c.answer = Some((port, busy, Instant::now()));
    let Some(waiting) = c.then.take() else {
        return false;
    };
    if busy {
        s.notice = taken(port);
        return true;
    }
    if let Some(why) = system_changed(s, &waiting) {
        s.notice = why;
        return true;
    }
    if let Some(index) = s
        .config
        .profiles
        .iter()
        .position(|p| p.id == waiting.profile)
    {
        s.start(index);
    }
    true
}

/// Why a waiting direct start is dropped: the system it was started for was deleted, or
/// another system became active, so `State::start` would claim the simulator for the wrong
/// one. `None` when it may start under the same system.
fn system_changed(s: &State, waiting: &Waiting) -> Option<String> {
    let name = systems::component(&waiting.profile).map_or(waiting.profile.as_str(), |c| c.name);
    let Some(sys) = s.systems.get(&waiting.system) else {
        return Some(format!(
            "{name} was not started: the system it was started for no longer exists."
        ));
    };
    // `State::start` keeps the owner of a stage of the staged launch; anything else it
    // claims for the active system.
    let would = if systems::staged(s, &waiting.profile) {
        s.systems
            .owner(&waiting.profile)
            .unwrap_or(s.systems.active_id())
    } else {
        s.systems.active_id()
    };
    (would != waiting.system).then(|| {
        format!(
            "{name} was not started: the active system changed while the port was checked. Select {} and start it again.",
            sys.label()
        )
    })
}
