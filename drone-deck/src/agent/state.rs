//! The UI side of the agent: connection notices, router status and startup reconciliation.
use super::{AgentClient, AgentEvent, Hello, RouterStatus, Subscription, Ticket, shared};
use crate::{State, activity, backend::Request};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

/// Restarts allowed within [`RECONNECT_WINDOW`] before the app stops trying.
const RECONNECTS: usize = 3;
const RECONNECT_WINDOW: Duration = Duration::from_secs(60);

/// A process an earlier agent (or an earlier app session) started and nobody follows now.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Leftover {
    pub key: String,
    pub pid: u64,
    /// Start time, seconds since the Unix epoch (WSL clock).
    pub since: Option<f64>,
    /// Start time on this computer's monotonic clock, from the agent's `age_s`.
    pub started: Option<Instant>,
    pub command: String,
    /// Whether its output can be followed after adopting (false for `supervisor.py` processes).
    pub output: bool,
    /// An adopt or stop request is in flight.
    pub busy: bool,
}

#[derive(Default)]
pub(crate) struct AgentState {
    pub distro: String,
    pub hello: Option<Hello>,
    pub router: Option<RouterStatus>,
    pub leftovers: Vec<Leftover>,
    client: Option<Arc<AgentClient>>,
    events: Option<Subscription>,
    listing: Option<Ticket>,
    stops: Vec<(String, Ticket)>,
    restarts: VecDeque<Instant>,
    /// The agent could not be (re)started; no further automatic attempts.
    pub failed: Option<String>,
}

/// Rows of a `list` reply that are alive and not followed by the current agent.
pub(crate) fn leftovers_from(rows: &Value) -> Vec<Leftover> {
    rows.as_array()
        .into_iter()
        .flatten()
        .filter(|row| {
            row["alive"].as_bool() == Some(true) && row["attached"].as_bool() != Some(true)
        })
        .filter_map(|row| {
            Some(Leftover {
                key: row["key"].as_str()?.to_owned(),
                pid: row["pid"].as_u64().unwrap_or(0),
                since: row["since"].as_f64(),
                started: row["age_s"]
                    .as_f64()
                    .filter(|a| a.is_finite() && *a >= 0.)
                    .and_then(|a| Instant::now().checked_sub(Duration::from_secs_f64(a))),
                command: row["command"].as_str().unwrap_or("").to_owned(),
                output: row["output"].as_bool().unwrap_or(false),
                busy: false,
            })
        })
        .collect()
}

fn running(s: &State, key: &str) -> bool {
    s.status
        .get(key)
        .is_some_and(|v| v == "Running" || v == "Starting")
}

/// Splits a fresh `list` into processes this app session already owns and leftovers the user
/// decides about. After an agent restart the backend's [`super::Launcher`] re-adopts what it
/// owns (and reports what ended meanwhile), so owned rows are only noted here.
pub(super) fn reconcile(s: &mut State, rows: &Value) {
    let mut leftovers = Vec::new();
    for row in leftovers_from(rows) {
        if running(s, &row.key) {
            s.log(format!(
                "[agent] Reattaching to {} (pid {}) after the agent restarted.",
                row.key, row.pid
            ));
        } else {
            leftovers.push(row);
        }
    }
    if !leftovers.is_empty() {
        let keys: Vec<_> = leftovers.iter().map(|l| l.key.as_str()).collect();
        s.notice = format!(
            "{} process{} from a previous session still run in {} ({}). Adopt or stop {}.",
            leftovers.len(),
            if leftovers.len() == 1 { "" } else { "es" },
            s.config.distro,
            keys.join(", "),
            if leftovers.len() == 1 { "it" } else { "them" }
        );
        for row in &leftovers {
            s.log(format!(
                "[agent] Running (previous session): {} · pid {} · {}",
                row.key,
                row.pid,
                if row.command.is_empty() {
                    "command not recorded"
                } else {
                    &row.command
                }
            ));
        }
    }
    s.agent.leftovers = leftovers;
}

fn profile_exclusive(s: &State, key: &str) -> bool {
    s.config
        .profiles
        .iter()
        .find(|p| p.id == key)
        .is_some_and(|p| p.exclusive)
}

/// Follows a leftover process as if this session had started it: output, exit and Stop.
pub(crate) fn adopt(s: &mut State, key: &str) {
    let Some(row) = s.agent.leftovers.iter().position(|l| l.key == key) else {
        return;
    };
    let row = s.agent.leftovers.remove(row);
    s.status.insert(row.key.clone(), "Starting".into());
    let exclusive = profile_exclusive(s, &row.key);
    s.send(Request::Adopt(
        row.key.clone(),
        s.config.distro.clone(),
        exclusive,
    ));
    s.notice = format!("Adopting {} (pid {}).", row.key, row.pid);
    activity::record(s, format!("Adopted {} from a previous session", row.key));
}

/// Stops a leftover process group through the agent (a process stop, not a vehicle command).
pub(crate) fn stop_leftover(s: &mut State, key: &str) {
    let Some(client) = s.agent.client.clone() else {
        return;
    };
    let Some(row) = s
        .agent
        .leftovers
        .iter_mut()
        .find(|l| l.key == key && !l.busy)
    else {
        return;
    };
    row.busy = true;
    let ticket = client.request(
        "stop",
        json!({"key":key,"grace_s":3}),
        Duration::from_secs(20),
    );
    s.agent.stops.push((key.to_owned(), ticket));
    s.notice = format!("Stopping {key} from a previous session.");
    activity::record(s, format!("Stopped {key} from a previous session"));
}

/// Connects to the selected distribution's agent once WSL is ready, then applies its events.
pub(crate) fn poll(s: &mut State) -> bool {
    if s.headless {
        return false;
    }
    let mut changed = false;
    if s.agent.distro != s.config.distro && s.wsl_ready() {
        changed |= attach(s);
    }
    let events = s
        .agent
        .events
        .as_ref()
        .map(|e| e.drain(200))
        .unwrap_or_default();
    for event in events {
        changed = true;
        apply(s, event);
    }
    if let Some(reply) = s.agent.listing.as_mut().and_then(Ticket::try_take) {
        s.agent.listing = None;
        changed = true;
        match reply {
            Ok(rows) => reconcile(s, &rows),
            Err(e) => s.log(format!("[agent] Could not list owned processes: {e}")),
        }
    }
    let mut finished = Vec::new();
    s.agent
        .stops
        .retain_mut(|(key, ticket)| match ticket.try_take() {
            Some(reply) => {
                finished.push((key.clone(), reply));
                false
            }
            None => true,
        });
    for (key, reply) in finished {
        changed = true;
        // `running: true` means it survived SIGTERM and SIGKILL: never report that as stopped.
        let still_running = match &reply {
            Ok(value) => value["running"].as_bool() == Some(true),
            Err(_) => true,
        };
        if !still_running {
            s.agent.leftovers.retain(|l| l.key != key);
            s.log(format!("[agent] Stopped {key} from a previous session."));
            s.notice = format!("Stopped {key}. This is a process stop, not a vehicle command.");
            continue;
        }
        if let Some(row) = s.agent.leftovers.iter_mut().find(|l| l.key == key) {
            row.busy = false;
        }
        s.notice = match reply {
            Err(e) => format!("Could not stop {key}: {e}"),
            Ok(_) => format!("{key} is still running after the stop signals. Try Stop again."),
        };
        s.log(format!("[agent] {}", s.notice));
    }
    changed
}

fn attach(s: &mut State) -> bool {
    let distro = s.config.distro.clone();
    // Requests to the previous distribution's agent are abandoned.
    let previous = std::mem::take(&mut s.agent);
    previous.listing.into_iter().for_each(Ticket::cancel);
    previous.stops.into_iter().for_each(|(_, t)| t.cancel());
    s.agent = AgentState::default();
    // Not available yet (agents are enabled when the window is ready): try again next poll.
    let Some(client) = shared(&distro) else {
        return false;
    };
    s.agent.distro = distro;
    // Process output belongs to the backend; this view only needs the agent's own events.
    s.agent.events = Some(client.subscribe_filtered(|e| {
        !matches!(e, AgentEvent::Output { .. } | AgentEvent::Exited { .. })
    }));
    s.agent.client = Some(client.clone());
    if let Some(hello) = client.hello() {
        s.agent.hello = Some(hello);
        list(s);
    }
    if let Err(e) = client.connect() {
        s.agent.failed = Some(e.to_string());
        s.notice = e.to_string();
        s.log(format!("[agent] {e}"));
    }
    true
}

fn list(s: &mut State) {
    if let Some(client) = &s.agent.client {
        s.agent.listing = Some(client.request("list", json!({}), Duration::from_secs(20)));
    }
}

pub(super) fn apply(s: &mut State, event: AgentEvent) {
    match event {
        AgentEvent::Hello(hello) => {
            s.log(format!(
                "[agent] Linux agent ready in {} (Python {}, user {}).",
                if hello.distro.is_empty() {
                    &s.config.distro
                } else {
                    &hello.distro
                },
                hello.python,
                hello.user
            ));
            // Flight logs then search this home for SITL logs instead of every `/home/*`.
            if !hello.home.is_empty() {
                let distro = s.agent.distro.clone();
                crate::ulog::set_home(s, &distro, &hello.home);
            }
            s.agent.hello = Some(hello);
            s.agent.failed = None;
            list(s);
        }
        AgentEvent::Router(status) => s.agent.router = Some(status),
        AgentEvent::Log { level, message } => {
            let line = if level == "error" || level == "stderr" {
                format!("[agent] Error: {message}")
            } else {
                format!("[agent] {message}")
            };
            s.log(line);
        }
        AgentEvent::Dropped(n) => s.log(format!(
            "[agent] {n} agent messages were dropped while the app was busy."
        )),
        AgentEvent::Disconnected { expected: true, .. } => {
            s.agent.hello = None;
            s.agent.router = None;
        }
        AgentEvent::Disconnected { code, .. } => {
            s.agent.hello = None;
            s.agent.router = None;
            s.agent.leftovers.clear();
            s.agent.listing = None;
            let code = code.map_or_else(|| "unknown".to_owned(), |c| c.to_string());
            let now = Instant::now();
            s.agent
                .restarts
                .retain(|t| now.duration_since(*t) < RECONNECT_WINDOW);
            if s.agent.restarts.len() >= RECONNECTS {
                let message = format!(
                    "The Linux agent stopped again (exit code {code}). Linux tools are unavailable; check Activity → Process output, then restart Drone Deck."
                );
                s.log(format!("[agent] {message}"));
                s.notice = message.clone();
                s.agent.failed = Some(message);
                return;
            }
            s.agent.restarts.push_back(now);
            s.notice = format!("The Linux agent stopped (exit code {code}). Reconnecting…");
            s.log(format!(
                "[agent] The Linux agent stopped (exit code {code}). Reconnecting; running processes are reattached."
            ));
            if let Some(Err(e)) = s.agent.client.as_ref().map(|c| c.connect()) {
                s.notice = e.to_string();
                s.agent.failed = Some(e.to_string());
            }
        }
        AgentEvent::Connected
        | AgentEvent::Output { .. }
        | AgentEvent::Exited { .. }
        | AgentEvent::Readiness { .. }
        | AgentEvent::Rec(_)
        | AgentEvent::Other(..) => {}
    }
}
