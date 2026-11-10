//! The live vehicle link: one `mavlink::Service` at a time, owned by the system that opened
//! it. A simulated system reaches its vehicle through the app's own loopback link
//! (`tcp:127.0.0.1:5760`: the WSL router for PX4, ArduPilot SITL's own port); a physical
//! system through its endpoint, read-only and without a GCS heartbeat.
//!
//! The service fans packets out to QGroundControl (`udpout 127.0.0.1:14550`) and user scripts
//! (`scripts_udp`), records a `.tlog` under `<data root>\logs\tlog\` (the newest 100 captures,
//! at most 2 GB before a new one starts; see `retention`), and hands the UI a
//! coalesced [`VehicleSnapshot`] plus events, which [`poll`] folds into [`Vehicle`].
//!
//! Flight commands and the console go through [`authority`]: only an app-owned simulation
//! that is running gets `Authority::OwnedSimulation` (see `gate`). Headless previews never
//! connect; [`preview`] installs a fake snapshot instead.
use crate::mavlink::{
    self, CommandKind, CommandResult, Endpoint, Event, LinkOrigin, LinkState, LogEntry, ParamSet,
    Service, StatusText, VehicleSnapshot, Waker,
};
use crate::{State, activity, backend};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

mod gate;
mod preview;
mod retention;
mod stage;
#[cfg(test)]
mod tests;
pub(crate) mod view;

#[cfg(test)]
pub(crate) use gate::evidence;
pub(crate) use gate::{authority, gate_reason, simulator_conflict};
#[cfg(test)]
pub(crate) use preview::fake_snapshot;
pub(crate) use preview::preview;
pub(crate) use stage::{Stage, Step, awaited, router_running, stage, stop_router};

/// Where the app's simulator link listens on this computer (WSL forwards it from the router,
/// or ArduPilot SITL serves it directly).
pub(crate) const SIM_TCP_PORT: u16 = 5760;
/// The UDP port the router listens on inside WSL (PX4's default GCS target).
pub(crate) const ROUTER_UDP_PORT: u16 = 14550;
/// QGroundControl listens here and connects to whoever sends to it.
pub(crate) const QGC_PEER: &str = "127.0.0.1:14550";

/// Bounded rings.
pub(crate) const STATUS_RING: usize = 200;
pub(crate) const COMMAND_RING: usize = 50;
pub(crate) const CONSOLE_BYTES: usize = 64 * 1024;

/// One flight command sent to the vehicle and how it ended (`None` while pending).
#[derive(Clone, Debug)]
pub(crate) struct CommandRecord {
    pub id: u64,
    pub kind: CommandKind,
    pub result: Option<CommandResult>,
    pub sent: Instant,
}

/// The live link and what it reported. `State::vehicle`.
#[derive(Default)]
pub(crate) struct Vehicle {
    pub link: Option<Service>,
    /// Id of the system that opened the link.
    pub owner: Option<String>,
    /// The endpoint the link uses, as text (`tcp:127.0.0.1:5760`, `serial:COM4:57600`).
    pub endpoint: String,
    /// The latest snapshot of the open link; `None` when no link is open. Check
    /// `snapshot.fresh` before showing values as live.
    pub snapshot: Option<VehicleSnapshot>,
    /// STATUSTEXT messages, newest last (at most [`STATUS_RING`]).
    pub status: VecDeque<StatusText>,
    /// Flight commands, newest last (at most [`COMMAND_RING`]).
    pub commands: VecDeque<CommandRecord>,
    /// Console output (MAVLink shell), at most [`CONSOLE_BYTES`].
    pub console: String,
    pub params: Option<ParamSet>,
    /// Parameter indices the vehicle never answered in the last download.
    pub params_missing: Vec<u16>,
    /// (received, total) while a parameter download runs.
    pub params_progress: Option<(usize, usize)>,
    pub logs: Vec<LogEntry>,
    /// The last log list answered every entry.
    pub logs_complete: bool,
    /// A log list request is in flight.
    pub listing: bool,
    /// (log id, bytes received, size) while a log downloads.
    pub log_progress: Option<(u16, u32, u32)>,
    /// The last log downloaded into `<data root>\logs\vehicle\`.
    pub downloaded: Option<PathBuf>,
    /// The last failure (link down, refused request, failed download), user text.
    pub error: Option<String>,
    /// Opened by `connect_sim` on the app's own loopback simulator link.
    pub(crate) sim: bool,
    /// Simulator link port (`SIM_TCP_PORT`; tests point it at a fake vehicle).
    pub(crate) sim_port: u16,
    pub(crate) router: stage::Router,
    /// Wakes the UI when the service has news (set by `main` once the window is ready).
    pub(crate) waker: Option<Waker>,
    /// Headless scenes only: evidence shown in place of a link's (see `preview`).
    pub(crate) preview: Option<mavlink::SimulationEvidence>,
    /// View state of the live view, flight actions and console (`view::VehicleUi`).
    pub(crate) ui: view::VehicleUi,
}

impl Vehicle {
    /// Whether a link is open (connecting, up or down and retrying).
    pub(crate) fn connected(&self) -> bool {
        self.link.is_some()
    }
    /// Whether the open link belongs to system `id`.
    pub(crate) fn owned_by(&self, id: &str) -> bool {
        self.link.is_some() && self.owner.as_deref() == Some(id)
    }
    /// The link is up (TCP connected, UDP bound, serial open).
    pub(crate) fn up(&self) -> bool {
        self.link.is_some()
            && self
                .snapshot
                .as_ref()
                .is_some_and(|s| s.link.state == LinkState::Connected)
    }
    /// A vehicle heartbeat arrived within the stale window.
    pub(crate) fn fresh(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|s| s.fresh)
    }
    /// The snapshot to show on system `id`'s page: only the owner's (live or preview).
    pub(crate) fn shown_for(&self, id: &str) -> Option<&VehicleSnapshot> {
        self.snapshot
            .as_ref()
            .filter(|_| self.owner.as_deref() == Some(id))
    }
    /// The `.tlog` the link is writing.
    pub(crate) fn tlog(&self) -> Option<&Path> {
        self.snapshot.as_ref()?.link.tlog.as_deref()
    }
    fn reset_link_state(&mut self) {
        self.snapshot = None;
        // The position trace is "since the link came up": a new link starts a new one.
        self.ui.track.clear();
        self.params_progress = None;
        self.log_progress = None;
        self.listing = false;
    }
}

/// The port of the app's simulator link: [`SIM_TCP_PORT`] unless a test points it elsewhere.
pub(crate) fn sim_port(s: &State) -> u16 {
    if s.vehicle.sim_port == 0 {
        SIM_TCP_PORT
    } else {
        s.vehicle.sim_port
    }
}

/// `<data root>\logs\tlog`.
pub(crate) fn tlog_dir() -> PathBuf {
    backend::root().join("logs").join("tlog")
}
/// `<data root>\logs\vehicle`, where downloaded onboard logs go. PX4 ULog files there are listed
/// in Flight logs; ArduPilot DataFlash `.bin` files are reached through their folder.
pub(crate) fn log_dir() -> PathBuf {
    crate::ulog::vehicle_dir(&backend::root())
}

fn owner_name(s: &State, id: &str) -> String {
    s.systems
        .get(id)
        .map_or_else(|| "another system".to_string(), |x| x.label().to_string())
}

/// Refuses a second link: one link at a time, owned by one system.
fn check_free(s: &State, owner: &str) -> Result<(), String> {
    match s.vehicle.owner.as_deref() {
        Some(other) if s.vehicle.link.is_some() && other != owner => Err(format!(
            "The live link belongs to {}. Disconnect it (or stop its session) first.",
            owner_name(s, other)
        )),
        _ => Ok(()),
    }
}

fn fail(s: &mut State, message: String) -> Result<(), String> {
    s.vehicle.error = Some(message.clone());
    s.notice = message.clone();
    Err(message)
}

/// Peers for the system's consumers. A peer that would send to the link's own UDP listen
/// port is left out (a physical vehicle on UDP 14550 and QGroundControl can't share it).
fn peers(s: &State, owner: &str, endpoint: &Endpoint) -> Vec<SocketAddr> {
    let Some(sys) = s.systems.get(owner) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if sys.consumers.qgc {
        out.extend(QGC_PEER.parse::<SocketAddr>());
    }
    if let Some(addr) = &sys.consumers.scripts_udp {
        out.extend(addr.trim().parse::<SocketAddr>());
    }
    let listen = match endpoint {
        Endpoint::UdpListen { port, .. } => Some(*port),
        _ => None,
    };
    out.retain(|p| Some(p.port()) != listen);
    out.dedup();
    out
}

fn open(s: &mut State, owner: &str, endpoint: Endpoint, origin: LinkOrigin) -> Result<(), String> {
    let removed = retention::prune(&tlog_dir(), retention::KEEP, retention::BUDGET);
    if !removed.is_empty() {
        s.log(format!(
            "[link] Removed {} old telemetry capture(s): logs\\tlog keeps {} files and {} GB",
            removed.len(),
            retention::KEEP,
            retention::BUDGET / 1_000_000_000
        ));
    }
    let mut config = mavlink::Config::new(endpoint.clone(), origin)
        .tlog(mavlink::default_tlog_path(&tlog_dir(), SystemTime::now()))
        .heartbeat(origin == LinkOrigin::SimulatorRouter)
        .stale_after(std::time::Duration::from_secs(5));
    for peer in peers(s, owner, &endpoint) {
        config = config.peer(peer);
    }
    if let Some(waker) = &s.vehicle.waker {
        config = config.waker(waker.clone());
    }
    let service = match Service::start(config) {
        Ok(service) => service,
        Err(e) => return fail(s, format!("The vehicle link could not start: {e}")),
    };
    let v = &mut s.vehicle;
    v.reset_link_state();
    if v.owner.as_deref() != Some(owner) {
        v.params = None;
        v.params_missing.clear();
        v.logs.clear();
        v.logs_complete = false;
        v.downloaded = None;
        v.console.clear();
    }
    v.sim = origin == LinkOrigin::SimulatorRouter;
    v.endpoint = endpoint.to_string();
    v.owner = Some(owner.to_string());
    v.error = None;
    v.link = Some(service);
    let name = owner_name(s, owner);
    let kind = if origin == LinkOrigin::Physical {
        "read-only link"
    } else {
        "simulator link"
    };
    s.log(format!("[link] {name}: {kind} on {endpoint}"));
    activity::record(s, format!("Opened the {kind} for {name} ({endpoint})"));
    Ok(())
}

/// Opens the app's own simulator link (`tcp:127.0.0.1:5760`) for system `owner`, which must
/// have a simulated source. Fans out to its QGroundControl and scripts consumers. A no-op
/// when that system's simulator link is already open.
pub(crate) fn connect_sim(s: &mut State, owner: &str) -> Result<(), String> {
    if s.headless {
        return fail(s, "Preview: native connections disabled.".into());
    }
    check_free(s, owner).or_else(|e| fail(s, e))?;
    if s.vehicle.owned_by(owner) && s.vehicle.sim {
        return Ok(());
    }
    if !s.systems.get(owner).is_some_and(|x| x.source.simulated()) {
        return fail(
            s,
            "Only a simulated system opens the simulator link.".into(),
        );
    }
    if s.vehicle.link.is_some() {
        disconnect(s);
    }
    let endpoint = Endpoint::Tcp {
        host: "127.0.0.1".into(),
        port: sim_port(s),
    };
    open(s, owner, endpoint, LinkOrigin::SimulatorRouter)
}

/// Opens a read-only link to a physical vehicle on `endpoint` (`serial:COM4:57600`,
/// `udp:0.0.0.0:14550`, `tcp:192.168.1.10:5760`) for system `owner`. No GCS heartbeat is sent
/// and flight commands are refused whatever the caller passes.
pub(crate) fn connect_physical(s: &mut State, owner: &str, endpoint: &str) -> Result<(), String> {
    if s.headless {
        return fail(s, "Preview: native connections disabled.".into());
    }
    check_free(s, owner).or_else(|e| fail(s, e))?;
    let endpoint: Endpoint = match endpoint.trim().parse() {
        Ok(e) => e,
        Err(e) => return fail(s, e.to_string()),
    };
    if s.vehicle.link.is_some() {
        disconnect(s);
    }
    open(s, owner, endpoint, LinkOrigin::Physical)
}

/// Closes the link (a connection stop, not a vehicle command). Parameters and the log list
/// stay for export until another system connects.
pub(crate) fn disconnect(s: &mut State) {
    let Some(link) = s.vehicle.link.take() else {
        return;
    };
    link.stop();
    s.vehicle.reset_link_state();
    s.vehicle.sim = false;
    let name = s
        .vehicle
        .owner
        .clone()
        .map_or_else(String::new, |id| owner_name(s, &id));
    s.log(format!("[link] Closed the vehicle link of {name}"));
    activity::record(
        s,
        format!("Closed the vehicle link of {name} (not a vehicle command)"),
    );
}

/// Closes the link and stops the router this app started. `Stop session` and app exit.
pub(crate) fn stop(s: &mut State) {
    disconnect(s);
    stop_router(s);
}

/// Whether the link or the app's router is up (dependent work for `State::busy`). A headless
/// scene's preview link counts, so its page reads like the live one.
pub(crate) fn active(s: &State) -> bool {
    s.vehicle.link.is_some() || router_running(s) || (s.headless && s.vehicle.preview.is_some())
}

/// Sends a flight command with the authority the evidence grants. The result arrives later in
/// [`Vehicle::commands`] and Activity. Returns the request id.
pub(crate) fn command(s: &mut State, kind: CommandKind) -> Result<u64, String> {
    let granted = authority(s);
    let Some(link) = &s.vehicle.link else {
        return Err("No live link. Start the workspace first.".into());
    };
    match link.command(kind.clone(), granted) {
        Ok(id) => {
            s.vehicle.commands.push_back(CommandRecord {
                id,
                kind: kind.clone(),
                result: None,
                sent: Instant::now(),
            });
            while s.vehicle.commands.len() > COMMAND_RING {
                s.vehicle.commands.pop_front();
            }
            activity::record(s, format!("Sent {} to the simulated vehicle", kind.label()));
            Ok(id)
        }
        Err(refusal) => {
            let reason = gate_reason(s).unwrap_or_else(|| refusal.to_string());
            s.notice = format!("{} not sent: {reason}", kind.label());
            Err(reason)
        }
    }
}

/// Types `line` into the vehicle console (MAVLink shell) and runs it.
pub(crate) fn console_send(s: &mut State, line: &str) -> Result<(), String> {
    let granted = authority(s);
    let Some(link) = &s.vehicle.link else {
        return Err("No live link.".into());
    };
    link.shell(
        &format!("{}\n", line.trim_end_matches(['\r', '\n'])),
        granted,
    )
    .map_err(|refusal| gate_reason(s).unwrap_or_else(|| refusal.to_string()))
}

fn request(
    s: &mut State,
    what: &str,
    call: impl FnOnce(&Service) -> Result<(), mavlink::Refusal>,
) -> Result<(), String> {
    let Some(link) = &s.vehicle.link else {
        return Err(format!("No live link to {what}."));
    };
    call(link).map_err(|e| {
        let text = e.to_string();
        s.vehicle.error = Some(text.clone());
        text
    })
}

/// Downloads every parameter (read-only, any link). Progress in `params_progress`.
pub(crate) fn fetch_params(s: &mut State) -> Result<(), String> {
    request(s, "read parameters", |l| l.fetch_params())?;
    s.vehicle.params_progress = Some((0, 0));
    Ok(())
}

/// `<data root>\exports\params\<system>-<UTC stamp>.params` from the last download.
pub(crate) fn export_params(s: &State) -> Result<PathBuf, String> {
    let params = s
        .vehicle
        .params
        .as_ref()
        .ok_or("Fetch the parameters first.")?;
    let system = s.vehicle.owner.as_deref().unwrap_or("vehicle");
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let path = backend::root()
        .join("exports")
        .join("params")
        .join(format!("{system}-{}.params", mavlink::utc_stamp(secs)));
    crate::store::write(&path, params.to_qgc().as_bytes())
        .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// Reads a QGroundControl `.params` file. The `Vec` lists line numbers that did not parse.
pub(crate) fn load_params_file(path: &Path) -> Result<(ParamSet, Vec<usize>), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    Ok(ParamSet::from_qgc(&text))
}

/// Asks for the onboard log list (read-only). Entries arrive in `logs`.
pub(crate) fn list_logs(s: &mut State) -> Result<(), String> {
    request(s, "list logs", |l| l.list_logs())?;
    s.vehicle.listing = true;
    Ok(())
}

/// Downloads one onboard log into `<data root>\logs\vehicle\` (resuming a `.part`).
pub(crate) fn download_log(s: &mut State, entry: LogEntry) -> Result<(), String> {
    let dir = log_dir();
    let size = entry.size;
    let id = entry.id;
    request(s, "download a log", move |l| l.download_log(entry, dir))?;
    s.vehicle.log_progress = Some((id, 0, size));
    Ok(())
}

pub(crate) fn cancel_log(s: &mut State) -> Result<(), String> {
    request(s, "cancel", |l| l.cancel_log())?;
    s.vehicle.log_progress = None;
    Ok(())
}

/// Ends the console session on the vehicle (sent on any link; harmless when none is open).
pub(crate) fn console_close(s: &mut State) -> Result<(), String> {
    request(s, "close the console", |l| l.shell_close())
}

/// Mode names the vehicle's flight stack accepts in `CommandKind::SetMode` (empty without a
/// vehicle heartbeat).
pub(crate) fn available_modes(s: &State) -> Vec<&'static str> {
    s.vehicle
        .snapshot
        .as_ref()
        .and_then(|snap| snap.vehicle.as_ref())
        .map_or_else(Vec::new, |v| {
            mavlink::available_modes(v.autopilot, v.vehicle_type)
        })
}

/// Parameters added, removed or changed from `before` (a loaded file) to `after`.
pub(crate) fn diff_params(before: &ParamSet, after: &ParamSet) -> Vec<mavlink::ParamChange> {
    mavlink::diff_params(before, after)
}

fn push_console(console: &mut String, text: &str) {
    console.push_str(text);
    if console.len() > CONSOLE_BYTES {
        let mut cut = console.len() - CONSOLE_BYTES;
        while !console.is_char_boundary(cut) {
            cut += 1;
        }
        console.drain(..cut);
    }
}

fn result_text(result: &CommandResult) -> String {
    match result {
        CommandResult::Accepted => "accepted".into(),
        CommandResult::Rejected { reason, .. } => format!("rejected ({reason})"),
        CommandResult::TimedOut { attempts } => {
            format!("timed out after {attempts} attempts")
        }
        CommandResult::NoVehicle => "not sent: no vehicle heartbeat yet".into(),
        CommandResult::Busy => "not sent: the same command is still waiting".into(),
        CommandResult::Unsupported => "unsupported by this flight stack".into(),
        CommandResult::NoHome => "not sent: the vehicle has not reported its home yet".into(),
    }
}

fn apply(s: &mut State, event: Event) {
    match event {
        Event::Link(LinkState::Down(reason)) => {
            s.log(format!("[link] Down: {reason}. Retrying."));
            s.vehicle.error = Some(format!("Link down: {reason}"));
        }
        Event::Link(LinkState::Connected) => {
            s.log(format!("[link] Connected to {}", s.vehicle.endpoint));
            s.vehicle.error = None;
        }
        Event::Link(LinkState::Connecting) => {}
        Event::VehicleFound {
            system_id,
            component_id,
        } => s.log(format!(
            "[link] Vehicle {system_id}:{component_id} heartbeat"
        )),
        Event::StatusText(text) => {
            s.log(format!("[vehicle] {}: {}", text.severity_name(), text.text));
            s.vehicle.status.push_back(text);
            while s.vehicle.status.len() > STATUS_RING {
                s.vehicle.status.pop_front();
            }
        }
        Event::Command { id, kind, result } => {
            let text = format!("{} {}", kind.label(), result_text(&result));
            match s.vehicle.commands.iter_mut().find(|c| c.id == id) {
                Some(record) => record.result = Some(result),
                None => {
                    s.vehicle.commands.push_back(CommandRecord {
                        id,
                        kind,
                        result: Some(result),
                        sent: Instant::now(),
                    });
                    while s.vehicle.commands.len() > COMMAND_RING {
                        s.vehicle.commands.pop_front();
                    }
                }
            }
            s.notice = format!("{text}.");
            activity::record(s, format!("Simulated vehicle: {text}"));
        }
        Event::ParamProgress { received, total } => {
            s.vehicle.params_progress = Some((received, total));
        }
        Event::Params { set, missing } => {
            s.notice = if missing.is_empty() {
                format!("Read {} parameters (read-only).", set.params.len())
            } else {
                format!(
                    "Read {} parameters; {} never answered.",
                    set.params.len(),
                    missing.len()
                )
            };
            s.vehicle.params = Some(set);
            s.vehicle.params_missing = missing;
            s.vehicle.params_progress = None;
        }
        Event::LogList { entries, complete } => {
            s.vehicle.logs = entries;
            s.vehicle.logs_complete = complete;
            s.vehicle.listing = false;
        }
        Event::LogProgress { id, received, size } => {
            s.vehicle.log_progress = Some((id, received, size));
        }
        Event::LogDone { id, path } => {
            s.vehicle.log_progress = None;
            s.notice = format!(
                "Log {id} saved to {}. It is listed in Data & models → Flight logs.",
                path.display()
            );
            activity::record(s, format!("Downloaded onboard log {id}"));
            s.vehicle.downloaded = Some(path);
        }
        Event::LogFailed { id, reason } => {
            s.vehicle.log_progress = None;
            s.vehicle.error = Some(format!("Log {id}: {reason}"));
        }
        Event::Shell(text) => push_console(&mut s.vehicle.console, &text),
        Event::Unavailable(reason) => {
            s.vehicle.listing = false;
            s.vehicle.params_progress = None;
            // A download refused for want of a vehicle never started; one the service still
            // runs shows in its snapshot.
            if s.vehicle
                .snapshot
                .as_ref()
                .is_none_or(|snap| snap.log_progress.is_none())
            {
                s.vehicle.log_progress = None;
            }
            s.vehicle.error = Some(reason);
        }
        Event::TlogStopped(reason) => {
            s.log(format!("[link] Telemetry log stopped: {reason}"));
            s.vehicle.error = Some(format!("Telemetry log stopped: {reason}"));
        }
    }
}

/// Folds the service's snapshot and events into `s.vehicle`, and advances the router
/// request; the live view follows (position trace, heartbeat age). True only when something
/// visible changed.
pub(crate) fn poll(s: &mut State) -> bool {
    let changed = stage::poll(s) | poll_link(s);
    view::follow(s) | changed
}

fn poll_link(s: &mut State) -> bool {
    let Some(link) = s.vehicle.link.as_mut() else {
        return false;
    };
    let update = link.poll();
    if update.is_empty() {
        return false;
    }
    if let Some(snapshot) = update.snapshot {
        // Transfers in progress also show in the coalesced snapshot.
        if snapshot.params_progress.is_some() {
            s.vehicle.params_progress = snapshot.params_progress;
        }
        if snapshot.log_progress.is_some() {
            s.vehicle.log_progress = snapshot.log_progress;
        }
        s.vehicle.snapshot = Some(snapshot);
    }
    for event in update.events {
        apply(s, event);
    }
    true
}
