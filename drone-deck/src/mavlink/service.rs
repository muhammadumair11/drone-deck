//! The MAVLink service: one thread that owns the vehicle link, fans packets out to local UDP
//! peers (QGroundControl, user scripts) and back, writes the `.tlog`, keeps `VehicleState`,
//! and runs commands, parameter and log transfers. The UI talks to it through `Service`.
//!
//! What the service writes to the vehicle on its own: a 1 Hz GCS heartbeat when
//! `Config::heartbeat` is on, and one HOME_POSITION request when a vehicle is first found
//! (`home`; read-only, so it goes out on physical links too). Everything else is a request the
//! caller made, and `command` and `shell` refuse unless the link is the app's simulator router
//! and the caller holds `Authority::OwnedSimulation`.
use super::command::{Authority, CommandKind, CommandResult, Refusal};
use super::engine::Core;
use super::link::{self, Endpoint, Writer};
use super::logs::LogEntry;
use super::params::ParamSet;
use super::state::{StatusText, VehicleState};
use std::collections::VecDeque;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Where the link comes from. Physical links are read-only whatever `Authority` says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkOrigin {
    /// The app-owned router in front of a simulator.
    SimulatorRouter,
    /// A real vehicle (serial radio, network bridge).
    Physical,
}

pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// How to run a service. Build with `Config::new` and the builder methods.
#[derive(Clone)]
pub struct Config {
    pub(super) endpoint: Endpoint,
    pub(super) origin: LinkOrigin,
    pub(super) peers: Vec<SocketAddr>,
    pub(super) tlog: Option<PathBuf>,
    pub(super) tlog_limit: u64,
    pub(super) gcs: (u8, u8),
    pub(super) heartbeat: bool,
    pub(super) stale_after: Duration,
    pub(super) command_timeout: Duration,
    pub(super) command_attempts: u32,
    pub(super) waker: Option<Waker>,
}

/// Whether `endpoint` could be the app-owned simulator router, which is always reached on
/// this machine over TCP or UDP (WSL forwards it to localhost).
fn router_capable(endpoint: &Endpoint) -> bool {
    match endpoint {
        Endpoint::Serial { .. } => false,
        Endpoint::Tcp { host, .. }
        | Endpoint::UdpListen { host, .. }
        | Endpoint::UdpOut { host, .. } => {
            host.eq_ignore_ascii_case("localhost")
                || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
        }
    }
}

impl Config {
    /// `origin` is kept only when the endpoint could be the simulator router: a serial port
    /// or a non-loopback address is always `Physical` (read-only), whatever the caller says.
    pub fn new(endpoint: Endpoint, origin: LinkOrigin) -> Self {
        let origin = if router_capable(&endpoint) {
            origin
        } else {
            LinkOrigin::Physical
        };
        Self {
            endpoint,
            origin,
            peers: Vec::new(),
            tlog: None,
            tlog_limit: 2_000_000_000,
            // This station's MAVLink ids, apart from QGroundControl's 255.
            gcs: (246, 190),
            heartbeat: true,
            stale_after: Duration::from_secs(5),
            command_timeout: Duration::from_millis(1500),
            command_attempts: 3,
            waker: None,
        }
    }
    /// Adds a fan-out peer (`udpout`): every vehicle packet goes there, and its packets go
    /// to the vehicle. QGroundControl auto-connects on `127.0.0.1:14550`.
    pub fn peer(mut self, addr: SocketAddr) -> Self {
        self.peers.push(addr);
        self
    }
    /// Records every packet in both directions to this `.tlog` file. A file that can't be
    /// created doesn't stop the link: it opens without a capture and says why in
    /// `LinkStatus::tlog_error`.
    pub fn tlog(mut self, path: PathBuf) -> Self {
        self.tlog = Some(path);
        self
    }
    /// Whether to send a 1 Hz GCS heartbeat (default on).
    pub fn heartbeat(mut self, on: bool) -> Self {
        self.heartbeat = on;
        self
    }
    /// Silence after which the vehicle is shown stale (default 5 s).
    pub fn stale_after(mut self, after: Duration) -> Self {
        self.stale_after = after;
        self
    }
    /// Per-attempt acknowledgement timeout and attempt count (default 1.5 s × 3).
    #[cfg(test)]
    pub fn command_retry(mut self, timeout: Duration, attempts: u32) -> Self {
        self.command_timeout = timeout;
        self.command_attempts = attempts;
        self
    }
    /// Called (from the service thread) when a new snapshot or event is ready.
    pub fn waker(mut self, waker: Waker) -> Self {
        self.waker = Some(waker);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Connected,
    /// The last failure; the service keeps retrying.
    Down(String),
}

#[derive(Clone, Debug, Default)]
pub struct PeerStatus {
    pub addr: Option<SocketAddr>,
    pub rx_frames: u64,
    pub tx_frames: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LinkStatus {
    pub endpoint: String,
    pub state: LinkState,
    pub rx_bytes: u64,
    pub rx_frames: u64,
    /// Everything written to the vehicle: this app's requests and what peers forwarded.
    pub tx_frames: u64,
    /// Frames fan-out peers (QGroundControl, scripts) sent through to a `Physical` vehicle;
    /// always 0 on a simulator link. Part of `tx_frames`.
    pub peer_to_vehicle_frames: u64,
    pub bad_frames: u64,
    /// Progress, status text and console events dropped because the UI didn't drain them in
    /// time. One-shot events (link changes, results, finished transfers) are never dropped.
    pub dropped_events: u64,
    pub peers: Vec<PeerStatus>,
    pub tlog: Option<PathBuf>,
    pub tlog_bytes: u64,
    pub tlog_error: Option<String>,
}

/// A component seen on the link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemSeen {
    pub system_id: u8,
    pub component_id: u8,
    pub vehicle_type: u8,
    pub autopilot: u8,
}

/// What the UI draws, published at most 10 times a second.
#[derive(Clone, Debug)]
pub struct VehicleSnapshot {
    pub link: LinkStatus,
    pub vehicle: Option<VehicleState>,
    /// A heartbeat arrived within `stale_after`.
    pub fresh: bool,
    pub systems: Vec<SystemSeen>,
    /// Parameters received / expected while a download runs.
    pub params_progress: Option<(usize, usize)>,
    /// (log id, bytes received, size) while a log downloads.
    pub log_progress: Option<(u16, u32, u32)>,
}

/// Discrete things the UI shows once.
#[derive(Clone, Debug)]
pub enum Event {
    Link(LinkState),
    VehicleFound {
        system_id: u8,
        component_id: u8,
    },
    StatusText(StatusText),
    Command {
        id: u64,
        kind: CommandKind,
        result: CommandResult,
    },
    ParamProgress {
        received: usize,
        total: usize,
    },
    /// A finished (or abandoned) parameter download. `missing` lists indices never received.
    Params {
        set: ParamSet,
        missing: Vec<u16>,
    },
    LogList {
        entries: Vec<LogEntry>,
        complete: bool,
    },
    LogProgress {
        id: u16,
        received: u32,
        size: u32,
    },
    LogDone {
        id: u16,
        path: PathBuf,
    },
    /// The download stopped; its `.part` file stays for a resume.
    LogFailed {
        id: u16,
        reason: String,
    },
    /// Console output (coalesced, ≤ 10 Hz).
    Shell(String),
    /// A request that needs a vehicle arrived before one was seen.
    Unavailable(String),
    TlogStopped(String),
}

impl Event {
    /// Superseded by a later event or the snapshot, so it may be dropped when the UI falls
    /// behind. Every other event ends something the UI waits for and is always delivered.
    fn droppable(&self) -> bool {
        matches!(
            self,
            Self::ParamProgress { .. }
                | Self::LogProgress { .. }
                | Self::StatusText(_)
                | Self::Shell(_)
        )
    }
}

/// What `Service::poll` returns.
#[derive(Default)]
pub struct Update {
    pub snapshot: Option<VehicleSnapshot>,
    pub events: Vec<Event>,
}

impl Update {
    pub fn is_empty(&self) -> bool {
        self.snapshot.is_none() && self.events.is_empty()
    }
}

pub(super) enum Request {
    Command { id: u64, kind: CommandKind },
    FetchParams,
    ListLogs,
    Download { entry: LogEntry, dir: PathBuf },
    CancelDownload,
    Shell(String),
    ShellClose,
}

pub(super) enum Input {
    Up(Writer),
    Down(String),
    Main {
        data: Vec<u8>,
        from: Option<SocketAddr>,
    },
    Peer {
        index: usize,
        data: Vec<u8>,
    },
    PeerError {
        index: usize,
        reason: String,
    },
    Wake,
}

pub(super) struct Shared {
    pub snapshot: Mutex<VehicleSnapshot>,
    pub version: AtomicU64,
    pub requests: Mutex<VecDeque<Request>>,
    /// Events in order. Droppable ones stop being queued at `EVENT_QUEUE`; the others come
    /// one per link change, request or transfer, so they stay few.
    pub events: Mutex<VecDeque<Event>>,
    pub stop: AtomicBool,
}

impl Shared {
    /// Queues `event`; false when it was dropped because the queue is full.
    pub fn push_event(&self, event: Event) -> bool {
        let mut queue = match self.events.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        if queue.len() >= EVENT_QUEUE && event.droppable() {
            return false;
        }
        queue.push_back(event);
        true
    }
}

/// Bounded queue sizes.
const INPUT_QUEUE: usize = 1024;
const EVENT_QUEUE: usize = 512;
const REQUEST_QUEUE: usize = 256;
/// How long `stop` waits for the readers to close their socket or serial port: one blocking
/// read (`link::READ_TIMEOUT`), one retry pause step and some slack. A reader still connecting
/// a TCP socket, which holds nothing a new link needs, is left to finish on its own.
const READER_EXIT: Duration = Duration::from_millis(500);

/// The running service. Dropping it stops the thread and closes the link and the tlog.
pub struct Service {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    input: SyncSender<Input>,
    core: Option<JoinHandle<()>>,
    /// Disconnects once every reader thread has closed its socket or port.
    readers: Option<Receiver<()>>,
    origin: LinkOrigin,
    next_id: AtomicU64,
    seen: u64,
}

impl Service {
    /// Starts the threads. Fails when a peer socket can't be opened; link failures don't
    /// fail here, they show as `LinkState::Down` and are retried.
    pub fn start(config: Config) -> io::Result<Service> {
        let (input, rx) = mpsc::sync_channel(INPUT_QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let mut readers = Vec::new();
        let mut writers = Vec::new();
        for addr in &config.peers {
            let socket = link::udp_for(*addr)?;
            writers.push(socket.try_clone()?);
            readers.push((*addr, socket));
        }
        let (core, snapshot) = Core::new(&config, writers);
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(snapshot),
            version: AtomicU64::new(0),
            requests: Mutex::new(VecDeque::new()),
            events: Mutex::new(VecDeque::new()),
            stop: AtomicBool::new(false),
        });
        let core_shared = shared.clone();
        let core = thread::Builder::new()
            .name("mavlink-service".into())
            .spawn(move || core.run(rx, core_shared))?;
        let (exit, exited) = mpsc::channel();
        let started = (|| {
            let endpoint = config.endpoint.clone();
            link::spawn_main(endpoint, input.clone(), stop.clone(), exit.clone())?;
            for (index, (addr, socket)) in readers.into_iter().enumerate() {
                let (tx, stop) = (input.clone(), stop.clone());
                link::spawn_peer(index, addr, socket, tx, stop, exit.clone())?;
            }
            io::Result::Ok(())
        })();
        drop(exit);
        let mut service = Service {
            shared,
            stop,
            input,
            core: Some(core),
            readers: Some(exited),
            origin: config.origin,
            next_id: AtomicU64::new(1),
            seen: 0,
        };
        if let Err(e) = started {
            // Don't leave the service thread or a started reader running for nobody.
            service.shutdown();
            return Err(e);
        }
        Ok(service)
    }

    /// The latest snapshot when it changed since the last poll, and pending events (bounded).
    pub fn poll(&mut self) -> Update {
        let mut update = Update::default();
        let version = self.shared.version.load(Ordering::Acquire);
        if version != self.seen {
            self.seen = version;
            update.snapshot = Some(self.snapshot());
        }
        let mut events = match self.shared.events.lock() {
            Ok(events) => events,
            Err(poisoned) => poisoned.into_inner(),
        };
        update.events.extend(events.drain(..));
        drop(events);
        update
    }
    /// The latest snapshot, whether or not it changed.
    pub fn snapshot(&self) -> VehicleSnapshot {
        match self.shared.snapshot.lock() {
            Ok(s) => s.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
    pub fn origin(&self) -> LinkOrigin {
        self.origin
    }

    fn request(&self, request: Request) -> Result<(), Refusal> {
        if self.shared.stop.load(Ordering::Relaxed) {
            return Err(Refusal::Stopped);
        }
        {
            let mut queue = self.shared.requests.lock().map_err(|_| Refusal::Stopped)?;
            if queue.len() >= REQUEST_QUEUE {
                queue.pop_front();
            }
            queue.push_back(request);
        }
        match self.input.try_send(Input::Wake) {
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => Err(Refusal::Stopped),
        }
    }
    fn check_flight(&self, authority: Authority) -> Result<(), Refusal> {
        if self.origin == LinkOrigin::Physical {
            return Err(Refusal::PhysicalLink);
        }
        if !authority.may_fly() {
            return Err(Refusal::ReadOnly);
        }
        Ok(())
    }

    /// Sends a flight command. The result arrives later as `Event::Command` with this id.
    pub fn command(&self, kind: CommandKind, authority: Authority) -> Result<u64, Refusal> {
        self.check_flight(authority)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.request(Request::Command { id, kind })?;
        Ok(id)
    }
    /// Downloads every parameter (read-only; allowed on any link).
    pub fn fetch_params(&self) -> Result<(), Refusal> {
        self.request(Request::FetchParams)
    }
    /// Lists onboard logs (read-only).
    pub fn list_logs(&self) -> Result<(), Refusal> {
        self.request(Request::ListLogs)
    }
    /// Downloads one log into `dir` (normally `<data root>/logs/vehicle`), resuming a `.part`.
    pub fn download_log(&self, entry: LogEntry, dir: PathBuf) -> Result<(), Refusal> {
        self.request(Request::Download { entry, dir })
    }
    pub fn cancel_log(&self) -> Result<(), Refusal> {
        self.request(Request::CancelDownload)
    }
    /// Types into the vehicle console. Include the trailing "\n" to run a line.
    pub fn shell(&self, text: &str, authority: Authority) -> Result<(), Refusal> {
        self.check_flight(authority)?;
        self.request(Request::Shell(text.to_string()))
    }
    pub fn shell_close(&self) -> Result<(), Refusal> {
        self.request(Request::ShellClose)
    }
    /// Stops the threads, waits for the service thread, and waits briefly for the readers to
    /// close the link's socket or serial port, so a new link can open it at once.
    pub fn stop(mut self) {
        self.shutdown();
    }
    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.shared.stop.store(true, Ordering::Relaxed);
        let _ = self.input.try_send(Input::Wake);
        if let Some(core) = self.core.take() {
            let _ = core.join();
        }
        if let Some(readers) = self.readers.take() {
            // Nothing is ever sent: this returns when the last reader has exited (the channel
            // disconnects) or at the deadline.
            let _ = readers.recv_timeout(READER_EXIT);
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}
