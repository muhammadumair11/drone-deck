//! The service thread: routes frames, updates state, drives transfers, publishes snapshots.
use super::command::{CommandStep, Commands};
use super::frame::{Encoder, Frame, Framer, Msg, dialect};
use super::home::HomeRequest;
use super::link::{Endpoint, Writer};
use super::logs::{DownloadStep, ListStep, LogDownload, LogList};
use super::modes;
use super::params::{self, FetchStep, ParamFetch};
use super::service::{
    Config, Event, Input, LinkOrigin, LinkState, LinkStatus, PeerStatus, Request, Shared,
    SystemSeen, VehicleSnapshot,
};
use super::shell;
use super::state::{StatusAssembler, VehicleState};
use super::tlog::TlogWriter;
use mavlink::Message;
use std::collections::{BTreeMap, HashMap};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

const PUBLISH_EVERY: Duration = Duration::from_millis(100);
const TICK: Duration = Duration::from_secs(1);

struct Peer {
    addr: SocketAddr,
    socket: UdpSocket,
    framer: Framer,
}

pub(super) struct Core {
    config: Config,
    encoder: Encoder,
    framer: Framer,
    datagrams: bool,
    listening: bool,
    /// A UDP listener's vehicle: the source of the first autopilot heartbeat. Datagrams from
    /// anywhere else are dropped (counted as bad frames) until the link goes down, so a stray
    /// sender can neither take the replies nor pose as the vehicle.
    locked: Option<SocketAddr>,
    /// Datagrams dropped because they came from another source than `locked`.
    foreign: u64,
    writer: Option<Writer>,
    peers: Vec<Peer>,
    tlog: Option<TlogWriter>,
    vehicle: Option<VehicleState>,
    systems: BTreeMap<(u8, u8), SystemSeen>,
    status: StatusAssembler,
    link: LinkStatus,
    commands: Commands,
    home: HomeRequest,
    fetch: Option<ParamFetch>,
    list: Option<LogList>,
    download: Option<LogDownload>,
    shell_bytes: Vec<u8>,
    /// New console bytes arrived since the last publish (a split UTF-8 tail alone doesn't count).
    shell_pending: bool,
    /// The console was opened by `Request::Shell`; only then is a close sent.
    shell_open: bool,
    counts: HashMap<&'static str, u32>,
    counted_since: Instant,
    fresh: bool,
    dirty: bool,
    progress: bool,
    last_publish: Instant,
    next_tick: Instant,
    shared: Option<Arc<Shared>>,
}

impl Core {
    /// A capture that can't be created leaves the link without one (`tlog_error` says why):
    /// a recording problem never costs the vehicle link.
    pub fn new(config: &Config, peers: Vec<UdpSocket>) -> (Core, VehicleSnapshot) {
        let now = Instant::now();
        let (tlog, tlog_error) = match &config.tlog {
            Some(path) => match TlogWriter::create(path, config.tlog_limit) {
                Ok(writer) => (Some(writer), None),
                Err(e) => (
                    None,
                    Some(format!("Capture not started: {}: {e}", path.display())),
                ),
            },
            None => (None, None),
        };
        let link = LinkStatus {
            endpoint: config.endpoint.to_string(),
            state: LinkState::Connecting,
            rx_bytes: 0,
            rx_frames: 0,
            tx_frames: 0,
            peer_to_vehicle_frames: 0,
            bad_frames: 0,
            dropped_events: 0,
            peers: config
                .peers
                .iter()
                .map(|a| PeerStatus {
                    addr: Some(*a),
                    ..Default::default()
                })
                .collect(),
            tlog: tlog.as_ref().map(|t| t.path().to_path_buf()),
            tlog_bytes: 0,
            tlog_error,
        };
        let core = Core {
            encoder: Encoder::new(config.gcs.0, config.gcs.1),
            framer: Framer::new(),
            datagrams: matches!(
                config.endpoint,
                Endpoint::UdpListen { .. } | Endpoint::UdpOut { .. }
            ),
            listening: matches!(config.endpoint, Endpoint::UdpListen { .. }),
            locked: None,
            foreign: 0,
            writer: None,
            peers: config
                .peers
                .iter()
                .zip(peers)
                .map(|(addr, socket)| Peer {
                    addr: *addr,
                    socket,
                    framer: Framer::new(),
                })
                .collect(),
            tlog,
            vehicle: None,
            systems: BTreeMap::new(),
            status: StatusAssembler::default(),
            link,
            commands: Commands::new(config.command_timeout, config.command_attempts),
            home: HomeRequest::default(),
            fetch: None,
            list: None,
            download: None,
            shell_bytes: Vec::new(),
            shell_pending: false,
            shell_open: false,
            counts: HashMap::new(),
            counted_since: now,
            fresh: false,
            dirty: false,
            progress: false,
            last_publish: now,
            next_tick: now + TICK,
            shared: None,
            config: config.clone(),
        };
        let snapshot = core.snapshot(now);
        (core, snapshot)
    }

    pub fn run(mut self, rx: Receiver<Input>, shared: Arc<Shared>) {
        /// Marks the service stopped however this thread ends (a panic included), so
        /// `Service` refuses requests instead of queueing them for nobody.
        struct Exit(Arc<Shared>);
        impl Drop for Exit {
            fn drop(&mut self) {
                self.0.stop.store(true, Ordering::Relaxed);
            }
        }
        let _exit = Exit(shared.clone());
        self.shared = Some(shared.clone());
        while !shared.stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            let wait = self.next_deadline().saturating_duration_since(now);
            match rx.recv_timeout(wait) {
                Ok(input) => self.handle(input),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let requests: Vec<Request> = match shared.requests.lock() {
                Ok(mut queue) => queue.drain(..).collect(),
                Err(_) => break,
            };
            for request in requests {
                self.request(request);
            }
            self.timers(Instant::now());
        }
        if let Some(mut download) = self.download.take() {
            let end = download.end_message();
            self.send(&end);
            download.cancel();
        }
        if let Some(tlog) = self.tlog.as_mut() {
            let _ = tlog.flush();
        }
    }

    fn next_deadline(&self) -> Instant {
        let mut at = self.next_tick;
        if self.dirty || self.progress || self.shell_pending {
            at = at.min(self.last_publish + PUBLISH_EVERY);
        }
        let timers = [
            self.commands.next_deadline(),
            self.fetch.as_ref().map(|f| f.next_deadline()),
            self.list.as_ref().map(|l| l.next_deadline()),
            self.download.as_ref().map(|d| d.next_deadline()),
        ];
        timers.into_iter().flatten().fold(at, Instant::min)
    }

    /// Queues an event for `Service::poll`. Only progress, status text and console output
    /// can be dropped (when the UI falls behind); results and link changes always arrive.
    fn emit(&mut self, event: Event) {
        let Some(shared) = &self.shared else {
            return;
        };
        if shared.push_event(event) {
            self.wake();
        } else {
            self.link.dropped_events += 1;
        }
    }
    fn wake(&self) {
        if let Some(waker) = &self.config.waker {
            waker();
        }
    }

    fn handle(&mut self, input: Input) {
        let now = Instant::now();
        match input {
            Input::Up(writer) => {
                self.writer = Some(writer);
                self.set_link(LinkState::Connected);
            }
            Input::Down(reason) => {
                self.writer = None;
                self.locked = None;
                self.framer.reset();
                self.set_link(LinkState::Down(reason));
            }
            Input::Main { data, from } => {
                if self.listening && self.locked.is_some() && from != self.locked {
                    self.foreign += 1;
                    self.link.bad_frames = self.framer.bad_frames + self.foreign;
                    self.dirty = true;
                    return;
                }
                self.link.rx_bytes += data.len() as u64;
                let mut frames = Vec::new();
                self.framer.push(&data, &mut frames);
                if self.datagrams {
                    self.framer.reset();
                }
                self.link.bad_frames = self.framer.bad_frames + self.foreign;
                // A UDP listener answers the last sender of valid MAVLink, never of noise.
                if !frames.is_empty()
                    && let Some(writer) = self.writer.as_mut()
                {
                    writer.learn(from, self.listening);
                }
                for frame in frames {
                    self.on_vehicle_frame(frame, from, now);
                }
            }
            Input::Peer { index, data } => {
                let Some(peer) = self.peers.get_mut(index) else {
                    return;
                };
                let mut frames = Vec::new();
                peer.framer.push(&data, &mut frames);
                peer.framer.reset();
                self.link.peers[index].rx_frames += frames.len() as u64;
                let physical = self.config.origin == LinkOrigin::Physical;
                for frame in frames {
                    if self.forward_to_vehicle(&frame) && physical {
                        // The bridge is transparent: make what a peer sends a real vehicle visible.
                        self.link.peer_to_vehicle_frames += 1;
                        self.dirty = true;
                    }
                }
            }
            Input::PeerError { index, reason } => {
                if let Some(peer) = self.link.peers.get_mut(index) {
                    peer.error = Some(reason);
                    self.dirty = true;
                }
            }
            Input::Wake => {}
        }
    }

    fn set_link(&mut self, state: LinkState) {
        if self.link.state != state {
            self.link.state = state.clone();
            self.dirty = true;
            self.emit(Event::Link(state));
        }
    }

    fn log(&mut self, frame: &Frame) {
        let Some(tlog) = self.tlog.as_mut() else {
            return;
        };
        match tlog.write(SystemTime::now(), frame) {
            Ok(()) => self.link.tlog_bytes = tlog.written(),
            Err(reason) => {
                self.tlog = None;
                self.link.tlog_error = Some(reason.clone());
                self.dirty = true;
                self.emit(Event::TlogStopped(reason));
            }
        }
    }

    /// Writes a frame to the vehicle link (and the tlog). Returns whether the link took it.
    fn forward_to_vehicle(&mut self, frame: &Frame) -> bool {
        let Some(writer) = self.writer.as_mut() else {
            return false;
        };
        let sent = writer.send(&frame.bytes).is_ok();
        if sent {
            self.link.tx_frames += 1;
        }
        self.log(frame);
        sent
    }
    fn send(&mut self, msg: &Msg) {
        let frame = self.encoder.encode(msg);
        self.forward_to_vehicle(&frame);
    }

    fn on_vehicle_frame(&mut self, frame: Frame, from: Option<SocketAddr>, now: Instant) {
        self.link.rx_frames += 1;
        self.log(&frame);
        for (peer, status) in self.peers.iter().zip(self.link.peers.iter_mut()) {
            match peer.socket.send_to(&frame.bytes, peer.addr) {
                Ok(_) => status.tx_frames += 1,
                Err(e) => status.error = Some(e.to_string()),
            }
        }
        let Some(msg) = frame.decode() else { return };
        let (sys, comp) = (frame.system_id(), frame.component_id());
        if let Msg::HEARTBEAT(h) = &msg {
            let seen = SystemSeen {
                system_id: sys,
                component_id: comp,
                vehicle_type: h.mavtype as u8,
                autopilot: h.autopilot as u8,
            };
            if self.systems.get(&(sys, comp)) != Some(&seen) {
                self.systems.insert((sys, comp), seen);
                self.dirty = true;
            }
            let autopilot = modes::is_vehicle_heartbeat(h.autopilot as u8, h.mavtype as u8);
            let replace = match &self.vehicle {
                None => true,
                Some(v) => v.system_id != sys && !v.is_fresh(now, self.config.stale_after),
            };
            if autopilot && replace {
                if self.listening {
                    self.locked = from;
                }
                self.vehicle = Some(VehicleState::new(sys, comp, now));
                self.emit(Event::VehicleFound {
                    system_id: sys,
                    component_id: comp,
                });
                let request = self.home.start((sys, comp));
                self.send(&request);
            }
        }
        let Some(vehicle) = self.vehicle.as_mut() else {
            return;
        };
        if vehicle.system_id != sys {
            return;
        }
        *self.counts.entry(msg.message_name()).or_default() += 1;
        let primary = vehicle.component_id == comp;
        if primary && vehicle.apply(&msg, now) {
            self.dirty = true;
        }
        match &msg {
            Msg::STATUSTEXT(t) => {
                for entry in self.status.push(t, SystemTime::now()) {
                    if let Some(v) = self.vehicle.as_mut() {
                        v.push_status(entry.clone());
                    }
                    self.dirty = true;
                    self.emit(Event::StatusText(entry));
                }
            }
            Msg::COMMAND_ACK(ack) => {
                let ours = ack.target_system == 0 || ack.target_system == self.config.gcs.0;
                let home_known = self.vehicle.as_ref().is_some_and(|v| v.home.is_some());
                if ours && let Some(fallback) = self.home.on_ack(ack, home_known) {
                    self.send(&fallback);
                }
                if ours && let Some(step) = self.commands.on_ack(ack, now) {
                    self.command_step(step);
                }
            }
            Msg::PARAM_VALUE(v) if primary => {
                if let Some(fetch) = self.fetch.as_mut()
                    && fetch.on_value(v, now)
                {
                    self.progress = true;
                }
            }
            Msg::LOG_ENTRY(e) if primary => {
                if let Some(list) = self.list.as_mut() {
                    list.on_entry(e, now);
                }
            }
            Msg::LOG_DATA(d) if primary => {
                let step = self.download.as_mut().and_then(|dl| dl.on_data(d, now));
                self.progress = true;
                if let Some(step) = step {
                    self.download_step(step);
                }
            }
            Msg::SERIAL_CONTROL(s) => {
                if let Some(bytes) = shell::output(s)
                    && self.shell_bytes.len() < 64 * 1024
                {
                    self.shell_bytes.extend_from_slice(bytes);
                    self.shell_pending = true;
                }
            }
            _ => {}
        }
    }

    fn target(&self) -> Option<(u8, u8)> {
        self.vehicle.as_ref().map(|v| (v.system_id, v.component_id))
    }

    fn request(&mut self, request: Request) {
        let now = Instant::now();
        let no_vehicle = || Event::Unavailable("No vehicle heartbeat on the link yet".into());
        match request {
            Request::Command { id, kind } => {
                let step = self.commands.start(id, kind, self.vehicle.as_ref(), now);
                self.command_step(step);
            }
            Request::FetchParams => {
                if self.fetch.is_some() {
                    return;
                }
                let (Some(target), Some(v)) = (self.target(), self.vehicle.as_ref()) else {
                    return self.emit(no_vehicle());
                };
                let (fetch, msg) = ParamFetch::new(target, v.autopilot, now);
                self.fetch = Some(fetch);
                self.progress = true;
                self.send(&msg);
            }
            Request::ListLogs => {
                if self.list.is_some() {
                    return;
                }
                let Some(target) = self.target() else {
                    return self.emit(no_vehicle());
                };
                let (list, msg) = LogList::new(target, now);
                self.list = Some(list);
                self.send(&msg);
            }
            Request::Download { entry, dir } => {
                let id = entry.id;
                if self.download.is_some() {
                    let reason = "Another log is downloading".into();
                    return self.emit(Event::LogFailed { id, reason });
                }
                let (Some(target), Some(v)) = (self.target(), self.vehicle.as_ref()) else {
                    return self.emit(no_vehicle());
                };
                match LogDownload::start(entry, target, v.autopilot, &dir, now) {
                    Ok((download, step)) => {
                        self.download = Some(download);
                        self.progress = true;
                        self.download_step(step);
                    }
                    Err(e) => {
                        let reason = format!("Couldn't open {}: {e}", dir.display());
                        self.emit(Event::LogFailed { id, reason });
                    }
                }
            }
            Request::CancelDownload => {
                if let Some(mut download) = self.download.take() {
                    let end = download.end_message();
                    self.send(&end);
                    download.cancel();
                    let reason = "Cancelled; download again to resume".into();
                    self.progress = true;
                    self.emit(Event::LogFailed {
                        id: download.entry.id,
                        reason,
                    });
                }
            }
            Request::Shell(text) => {
                let Some(target) = self.target() else {
                    return self.emit(no_vehicle());
                };
                self.shell_open = true;
                for msg in shell::send(target, &text) {
                    self.send(&msg);
                }
            }
            Request::ShellClose => {
                // Only `Request::Shell` (gated by authority) opens the console, so a close
                // never writes to a link the app didn't already command.
                if self.shell_open
                    && let Some(target) = self.target()
                {
                    self.shell_open = false;
                    self.send(&shell::close(target));
                }
            }
        }
    }

    fn command_step(&mut self, step: CommandStep) {
        match step {
            CommandStep::Send(msg) => self.send(&msg),
            CommandStep::Done { id, kind, result } => {
                self.emit(Event::Command { id, kind, result })
            }
        }
    }

    fn download_step(&mut self, step: DownloadStep) {
        match step {
            DownloadStep::Wait => {}
            DownloadStep::Send(msg) => self.send(&msg),
            DownloadStep::Done(path) => {
                if let Some(download) = self.download.take() {
                    let end = download.end_message();
                    self.send(&end);
                    let id = download.entry.id;
                    self.emit(Event::LogDone { id, path });
                }
            }
            DownloadStep::Failed(reason) => {
                if let Some(download) = self.download.take() {
                    let end = download.end_message();
                    self.send(&end);
                    let id = download.entry.id;
                    self.emit(Event::LogFailed { id, reason });
                }
            }
        }
    }

    fn timers(&mut self, now: Instant) {
        for step in self.commands.poll(self.vehicle.as_ref(), now) {
            self.command_step(step);
        }
        if let Some(fetch) = self.fetch.as_mut() {
            match fetch.poll(now) {
                FetchStep::Wait => {}
                FetchStep::Send(msgs) => {
                    for msg in msgs {
                        self.send(&msg);
                    }
                }
                FetchStep::Complete => self.finish_params(),
                FetchStep::Incomplete(_) if fetch.progress().1 == 0 => {
                    // Not one PARAM_VALUE: an empty `Params` would read as "complete".
                    self.fetch = None;
                    self.progress = true;
                    self.emit(Event::Unavailable(
                        "The vehicle sent no parameters; try again".into(),
                    ));
                }
                FetchStep::Incomplete(_) => self.finish_params(),
            }
        }
        if let Some(list) = self.list.as_mut() {
            match list.poll(now) {
                ListStep::Wait => {}
                ListStep::Send(msg) => self.send(&msg),
                ListStep::Done { entries, complete } => {
                    self.list = None;
                    self.emit(Event::LogList { entries, complete });
                }
            }
        }
        if let Some(download) = self.download.as_mut() {
            let step = download.poll(now);
            self.download_step(step);
        }
        if now >= self.next_tick {
            self.tick(now);
        }
        if (self.dirty || self.progress || self.shell_pending)
            && now >= self.last_publish + PUBLISH_EVERY
        {
            self.publish(now);
        }
    }

    fn finish_params(&mut self) {
        let Some(fetch) = self.fetch.take() else {
            return;
        };
        let missing = fetch.missing();
        let (stack, vehicle) = self
            .vehicle
            .as_ref()
            .map(|v| params::header_names(v.autopilot, v.vehicle_type))
            .unwrap_or(("Generic", "Generic"));
        self.progress = true;
        let set = fetch.into_set(stack, vehicle);
        self.emit(Event::Params { set, missing });
    }

    fn tick(&mut self, now: Instant) {
        self.next_tick = now + TICK;
        if self.config.heartbeat && self.writer.is_some() {
            let heartbeat = Msg::HEARTBEAT(dialect::HEARTBEAT_DATA {
                custom_mode: 0,
                mavtype: dialect::MavType::MAV_TYPE_GCS,
                autopilot: dialect::MavAutopilot::MAV_AUTOPILOT_INVALID,
                base_mode: dialect::MavModeFlag::empty(),
                system_status: dialect::MavState::MAV_STATE_ACTIVE,
                mavlink_version: 3,
            });
            self.send(&heartbeat);
        }
        let seconds = now
            .duration_since(self.counted_since)
            .as_secs_f32()
            .max(0.001);
        self.counted_since = now;
        let mut rates: Vec<(&'static str, f32)> = self
            .counts
            .drain()
            .map(|(name, n)| (name, n as f32 / seconds))
            .collect();
        rates.sort_by(|a, b| a.0.cmp(b.0));
        if let Some(v) = self.vehicle.as_mut()
            && v.message_rates != rates
        {
            v.message_rates = rates;
            self.dirty = true;
        }
        let fresh = self
            .vehicle
            .as_ref()
            .is_some_and(|v| v.is_fresh(now, self.config.stale_after));
        if fresh != self.fresh {
            self.dirty = true;
        }
        if let Some(tlog) = self.tlog.as_mut() {
            let _ = tlog.flush();
        }
    }

    fn snapshot(&self, now: Instant) -> VehicleSnapshot {
        VehicleSnapshot {
            link: self.link.clone(),
            vehicle: self.vehicle.clone(),
            fresh: self
                .vehicle
                .as_ref()
                .is_some_and(|v| v.is_fresh(now, self.config.stale_after)),
            systems: self.systems.values().cloned().collect(),
            params_progress: self.fetch.as_ref().map(|f| f.progress()),
            log_progress: self
                .download
                .as_ref()
                .map(|d| (d.entry.id, d.received(), d.entry.size)),
        }
    }

    fn publish(&mut self, now: Instant) {
        self.last_publish = now;
        if self.progress {
            self.progress = false;
            if let Some((received, total)) = self.fetch.as_ref().map(|f| f.progress()) {
                self.emit(Event::ParamProgress { received, total });
            }
            if let Some(d) = self.download.as_ref() {
                let (id, received, size) = (d.entry.id, d.received(), d.entry.size);
                self.emit(Event::LogProgress { id, received, size });
            }
            self.dirty = true;
        }
        if self.shell_pending {
            self.shell_pending = false;
            let text = take_utf8(&mut self.shell_bytes);
            if !text.is_empty() {
                self.emit(Event::Shell(text));
            }
        }
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let snapshot = self.snapshot(now);
        self.fresh = snapshot.fresh;
        if let Some(shared) = &self.shared {
            if let Ok(mut slot) = shared.snapshot.lock() {
                *slot = snapshot;
            }
            shared.version.fetch_add(1, Ordering::Release);
        }
        self.wake();
    }
}

/// Takes the complete UTF-8 prefix of `bytes`, leaving a split character for later.
fn take_utf8(bytes: &mut Vec<u8>) -> String {
    let keep = match std::str::from_utf8(bytes) {
        Ok(_) => bytes.len(),
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => bytes.len(),
    };
    let rest = bytes.split_off(keep);
    let text = String::from_utf8_lossy(bytes).into_owned();
    *bytes = rest;
    text
}
