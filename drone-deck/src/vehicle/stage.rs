//! Virtual launch stages: `router` (the agent's MAVLink router in WSL, UDP 14550 ⇄ TCP 5760)
//! and `link` (the native service on `tcp:127.0.0.1:5760`). `session.rs` asks [`stage`] for
//! them instead of starting a profile.
use super::{ROUTER_UDP_PORT, SIM_TCP_PORT, connect_sim};
use crate::State;
use crate::agent::{RouterStatus, Ticket, shared};
use serde_json::json;
use std::time::Duration;

const ROUTER_TIMEOUT: Duration = Duration::from_secs(15);

/// The router as this app controls it.
#[derive(Default)]
pub(crate) struct Router {
    /// `router.start` succeeded: this app started (or joined) the router and stops it.
    pub(crate) started: bool,
    pending: Option<Ticket>,
    pub(crate) error: Option<String>,
}

/// What `session.rs` asks of a virtual stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Start,
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Waiting,
    Ready,
    Failed(String),
}

fn router_state(s: &State) -> &str {
    s.agent.router.as_ref().map_or("", |r| r.state.as_str())
}

/// The router this app started reports itself up (listening or forwarding). False once the
/// agent restarted or the router stopped or failed, whatever `started` still says.
pub(crate) fn router_up(s: &State) -> bool {
    s.vehicle.router.started && router_listening(s)
}

/// The router reports a TCP client. While the app's link is up that client is the link, so
/// none means the link reached some other program listening on the simulator port.
pub(crate) fn router_has_client(s: &State) -> bool {
    s.agent.router.as_ref().is_some_and(|r| r.clients >= 1)
}

/// The agent runs a router that is up, whoever started it.
pub(crate) fn router_listening(s: &State) -> bool {
    matches!(router_state(s), "listening" | "forwarding")
}

/// The app's router is starting or running.
pub(crate) fn router_running(s: &State) -> bool {
    s.vehicle.router.pending.is_some() || router_up(s)
}

fn start_router(s: &mut State) -> Stage {
    if s.vehicle.router.pending.is_some() {
        return Stage::Waiting;
    }
    let Some(client) = shared(&s.config.distro) else {
        let message =
            "The Linux agent is not available, so the MAVLink router cannot start. Prepare WSL in Settings → Environment."
                .to_string();
        s.vehicle.router.error = Some(message.clone());
        return Stage::Failed(message);
    };
    // `session.rs` checked the Windows port first (`port::check`). A router the agent already
    // runs is joined: it holds the port itself, and the gate checks that the link is its
    // client.
    s.vehicle.router.error = None;
    s.vehicle.router.pending = Some(client.request(
        "router.start",
        json!({"udp_listen": ROUTER_UDP_PORT, "tcp_port": SIM_TCP_PORT}),
        ROUTER_TIMEOUT,
    ));
    s.log(format!(
        "[router] Starting the MAVLink router in {} (UDP {ROUTER_UDP_PORT} ⇄ TCP {SIM_TCP_PORT})",
        s.config.distro
    ));
    Stage::Waiting
}

fn router_ready(s: &State) -> Stage {
    let r = &s.vehicle.router;
    if let Some(e) = &r.error {
        return Stage::Failed(e.clone());
    }
    if !r.started {
        return Stage::Waiting;
    }
    match router_state(s) {
        "listening" | "forwarding" => Stage::Ready,
        "failed" => Stage::Failed("The MAVLink router failed inside WSL.".into()),
        "stopped" => Stage::Failed("The MAVLink router stopped.".into()),
        _ => Stage::Waiting,
    }
}

fn link_owner(s: &State) -> String {
    s.systems
        .owner("session")
        .unwrap_or(s.systems.active_id())
        .to_string()
}

fn link_ready(s: &State) -> Stage {
    if s.vehicle.link.is_none() {
        return Stage::Failed(
            s.vehicle
                .error
                .clone()
                .unwrap_or_else(|| "The vehicle link closed.".into()),
        );
    }
    if s.vehicle.up() {
        Stage::Ready
    } else {
        Stage::Waiting
    }
}

/// Starts or checks virtual stage `id`; `None` when `id` is a launch profile.
pub(crate) fn stage(s: &mut State, id: &str, step: Step) -> Option<Stage> {
    Some(match (id, step) {
        ("router", Step::Start) => start_router(s),
        ("router", Step::Ready) => router_ready(s),
        ("link", Step::Start) => {
            let owner = link_owner(s);
            match connect_sim(s, &owner) {
                Ok(()) => Stage::Waiting,
                Err(e) => Stage::Failed(e),
            }
        }
        ("link", Step::Ready) => link_ready(s),
        _ => return None,
    })
}

/// Waiting-for text for a virtual stage, for the launch progress.
pub(crate) fn awaited(id: &str) -> Option<&'static str> {
    match id {
        "router" => Some("router listening"),
        "link" => Some("link up"),
        _ => None,
    }
}

/// Why the router this app started is gone (the agent restarted, or the router stopped or
/// failed without being asked); `None` while it is up or was never started.
fn lost(s: &State) -> Option<&'static str> {
    let r = &s.vehicle.router;
    if !r.started || r.pending.is_some() || r.error.is_some() {
        return None;
    }
    match s.agent.router.as_ref().map(|x| x.state.as_str()) {
        None => Some("The Linux agent restarted, so the MAVLink router stopped."),
        Some("failed") => Some("The MAVLink router failed inside WSL."),
        Some("stopped") => Some("The MAVLink router stopped."),
        _ => None,
    }
}

/// Applies the `router.start` reply, or notices the router is gone. True on a change.
pub(super) fn poll(s: &mut State) -> bool {
    if let Some(why) = lost(s) {
        let message = format!("{why} Stop the session and start it again.");
        s.log(format!("[router] {message}"));
        s.vehicle.router.error = Some(message);
        return true;
    }
    let Some(reply) = s.vehicle.router.pending.as_mut().and_then(Ticket::try_take) else {
        return false;
    };
    s.vehicle.router.pending = None;
    match reply {
        Ok(value) => {
            s.vehicle.router.started = true;
            let status: RouterStatus = serde_json::from_value(value).unwrap_or_default();
            s.log(format!(
                "[router] {} on UDP {} / TCP {}",
                status.state,
                status.udp_listen.unwrap_or(ROUTER_UDP_PORT),
                status.tcp_port.unwrap_or(SIM_TCP_PORT)
            ));
            s.agent.router = Some(status);
        }
        Err(e) => {
            let message = format!("The MAVLink router did not start: {e}");
            s.log(format!("[router] {message}"));
            s.vehicle.router.error = Some(message);
        }
    }
    true
}

/// Stops the router if this app started it (a process stop inside WSL, not a vehicle command).
pub(crate) fn stop_router(s: &mut State) {
    let r = &mut s.vehicle.router;
    let pending = r.pending.take();
    let ours = r.started || pending.is_some();
    r.started = false;
    r.error = None;
    if let Some(ticket) = pending {
        ticket.cancel();
    }
    if !ours {
        return;
    }
    if let Some(client) = shared(&s.config.distro) {
        // Fire and forget: the reply (the stopped status) also arrives as a `router` event.
        drop(client.request("router.stop", json!({}), ROUTER_TIMEOUT));
    }
    s.log("[router] Stopping the MAVLink router".into());
}
