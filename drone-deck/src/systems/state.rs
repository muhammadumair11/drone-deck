//! Observed state of components and profiles, as a label and a tone.
use super::components::{Backing, Component};
use crate::mavlink::LinkState;
use crate::*;

/// Observed state of a component, as a label and a tone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tone {
    Idle,
    Active,
    Good,
    Warn,
    Bad,
}
/// Whether `id` runs or starts: a profile's process, or the virtual `router` / `link` stages.
pub(crate) fn running(s: &State, id: &str) -> bool {
    match id {
        "link" => s.vehicle.link.is_some(),
        "router" => vehicle::router_running(s),
        _ => s
            .status
            .get(id)
            .is_some_and(|v| v == "Running" || v == "Starting"),
    }
}
fn bytes(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", n.div_ceil(1024))
    }
}
fn router_state(s: &State) -> (String, Tone) {
    if let Some(e) = &s.vehicle.router.error {
        return (e.clone(), Tone::Bad);
    }
    if !vehicle::router_running(s) {
        return ("Not started".into(), Tone::Idle);
    }
    match s.agent.router.as_ref() {
        Some(r) if r.state == "forwarding" => {
            (format!("Forwarding / {} client(s)", r.clients), Tone::Good)
        }
        Some(r) if r.state == "listening" => ("Listening / no vehicle packets".into(), Tone::Warn),
        Some(r) if r.state == "failed" => ("Failed".into(), Tone::Bad),
        _ => ("Starting".into(), Tone::Active),
    }
}
fn link_state(s: &State) -> (String, Tone) {
    let v = &s.vehicle;
    if v.link.is_none() && v.preview.is_none() {
        return ("Not connected".into(), Tone::Idle);
    }
    let owner = v
        .owner
        .as_deref()
        .filter(|o| *o != s.systems.active_id())
        .and_then(|o| s.systems.get(o))
        .map(|x| format!(" / {}", x.label()))
        .unwrap_or_default();
    let Some(snap) = &v.snapshot else {
        return (format!("Connecting{owner}"), Tone::Active);
    };
    match &snap.link.state {
        LinkState::Down(reason) => (format!("Down / {reason}"), Tone::Bad),
        LinkState::Connecting => (format!("Connecting{owner}"), Tone::Active),
        LinkState::Connected if snap.fresh => {
            let mode = snap.vehicle.as_ref().map_or("", |x| x.mode.as_str());
            (format!("Heartbeat fresh / {mode}{owner}"), Tone::Good)
        }
        LinkState::Connected => (format!("Up / no heartbeat{owner}"), Tone::Warn),
    }
}
fn scripts_state(s: &State) -> (String, Tone) {
    let addr = s.systems.current().consumers.scripts_udp.clone();
    let Some(addr) = addr else {
        return ("Not configured".into(), Tone::Idle);
    };
    let peer = s.vehicle.snapshot.as_ref().and_then(|snap| {
        snap.link
            .peers
            .iter()
            .find(|p| p.addr.is_some_and(|a| a.to_string() == addr.trim()))
    });
    match peer {
        Some(p) if p.error.is_some() => (
            format!("UDP {addr} / {}", p.error.clone().unwrap_or_default()),
            Tone::Bad,
        ),
        Some(p) if p.rx_frames > 0 => (format!("UDP {addr} / script answering"), Tone::Good),
        Some(_) => (format!("UDP {addr} / sending"), Tone::Active),
        None => (format!("UDP {addr}"), Tone::Idle),
    }
}
pub(crate) fn state(s: &State, c: &Component) -> (String, Tone) {
    match c.backing {
        Backing::Profile(id) => profile_state(s, id),
        Backing::Router => router_state(s),
        Backing::Link => link_state(s),
        Backing::Scripts => scripts_state(s),
        Backing::Endpoint => {
            let e = s.hardware.endpoint.text.trim();
            if e.is_empty() {
                ("No endpoint".into(), Tone::Warn)
            } else {
                (e.to_string(), Tone::Idle)
            }
        }
        Backing::Recorder => {
            let r = &s.recorder;
            if r.recording {
                ("Recording".into(), Tone::Active)
            } else if r.playing {
                ("Replaying".into(), Tone::Active)
            } else if r.ready {
                (
                    format!("Ready / {} recordings", r.sessions.len()),
                    Tone::Good,
                )
            } else {
                ("Not checked".into(), Tone::Idle)
            }
        }
        Backing::Projects => {
            let count = s.studio.library["projects"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0);
            let active = s
                .status
                .iter()
                .any(|(k, v)| k.starts_with("project-") && (v == "Running" || v == "Starting"));
            if active {
                ("Project running".into(), Tone::Active)
            } else if count > 0 {
                (format!("{count} project(s)"), Tone::Idle)
            } else {
                ("No project".into(), Tone::Idle)
            }
        }
        Backing::Pipelines => pipelines_state(&s.studio.pipes),
        Backing::Tlog => match s.vehicle.snapshot.as_ref().map(|x| &x.link) {
            Some(l) if l.tlog_error.is_some() => {
                (l.tlog_error.clone().unwrap_or_default(), Tone::Bad)
            }
            Some(l) if l.tlog.is_some() => {
                (format!("Recording / {}", bytes(l.tlog_bytes)), Tone::Good)
            }
            _ => ("No capture yet".into(), Tone::Idle),
        },
    }
}
/// The pipeline job in flight, else what `pipe.list` returned. Runs are files next to their
/// recordings, so a listed run is never shown as live.
fn pipelines_state(p: &crate::studio::pipelines::Pipes) -> (String, Tone) {
    if let Some(job) = &p.job {
        let label = if job.cancelling() {
            format!("Cancelling {}", job.kind.name().to_lowercase())
        } else {
            format!("{} running", job.kind.name())
        };
        return (label, Tone::Active);
    }
    if !p.listed {
        return ("Runs not listed yet".into(), Tone::Idle);
    }
    let infer = p.inference_runs().count();
    let extract = p.runs.iter().filter(|r| r.kind == "extract").count();
    if infer + extract == 0 {
        return ("No pipeline runs".into(), Tone::Idle);
    }
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    (
        format!(
            "{} / {}",
            plural(infer, "inference run", "inference runs"),
            plural(extract, "extraction", "extractions")
        ),
        Tone::Idle,
    )
}
pub(crate) fn profile_state(s: &State, id: &str) -> (String, Tone) {
    if !s.config.profiles.iter().any(|p| p.id == id) {
        return ("Not configured".into(), Tone::Warn);
    }
    let status = s.status.get(id).map(String::as_str).unwrap_or("");
    match status {
        "Running" => match s.readiness.get(id).map(String::as_str) {
            Some("Ready") => ("Ready".into(), Tone::Good),
            Some("Failed") => ("Running / not ready".into(), Tone::Bad),
            _ => ("Running".into(), Tone::Active),
        },
        "Starting" => ("Starting".into(), Tone::Active),
        "Blocked" | "Failed" | "Configure command" => (status.into(), Tone::Bad),
        s2 if s2.starts_with("Exited") => {
            if s2 == "Exited 0" {
                (s2.into(), Tone::Idle)
            } else {
                (s2.into(), Tone::Bad)
            }
        }
        _ => match s.detected.get(id) {
            Some(true) => ("Path found".into(), Tone::Idle),
            Some(false) => ("Not detected".into(), Tone::Warn),
            None => ("Not checked".into(), Tone::Idle),
        },
    }
}
pub(crate) fn tone_color(t: Tone) -> rune::Color {
    use crate::ui::theme::*;
    match t {
        Tone::Idle => muted(),
        Tone::Active => fg(),
        Tone::Good => success(),
        Tone::Warn => warn(),
        Tone::Bad => danger(),
    }
}
