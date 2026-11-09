//! System connections: the planned chain, with a solid wire only where traffic was observed.
use super::*;
use systems::{ROS, SIMULATION, Tone};

fn project_running(s: &State) -> bool {
    s.status
        .iter()
        .any(|(k, v)| k.starts_with("project-") && v == "Running")
}

fn fresh_heartbeats(s: &State) -> usize {
    s.hardware
        .seen
        .values()
        .filter(|t| t.elapsed() < Duration::from_secs(10))
        .count()
}

/// Whether there is live evidence for the link `from → to`.
fn observed(s: &State, from: &str, to: &str) -> bool {
    let ready = |id: &str, value: &str| s.readiness.get(id).is_some_and(|r| r == value);
    let up = |id: &str| s.status.get(id).is_some_and(|v| v == "Running");
    match (from, to) {
        ("sih", "relay" | "dds") => ready("sih", "Ready") && up(to),
        ("relay", "jmavsim") => ready("relay", "Forwarding") && up("jmavsim"),
        // Topic discovery counts only while PX4 is ready and the Agent runs.
        ("dds", "ros2") => {
            ready("sih", "Ready")
                && up("dds")
                && s.recorder
                    .discovered
                    .iter()
                    .any(|t| t.name.starts_with("/fmu/"))
        }
        ("ros2", "mcap") => s.recorder.recording,
        ("onnx", "project") => project_running(s),
        ("endpoint", "hardware") => up("hardware") && fresh_heartbeats(s) > 0,
        ("hardware", "tlog") => {
            up("hardware") && fresh_heartbeats(s) > 0 && !s.hardware.path.is_empty()
        }
        _ => false,
    }
}

fn chain(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    let stations: Vec<&systems::Component> = systems::chain(s.systems.active).collect();
    let mut chain = if compact {
        col().w_full().gap(6.)
    } else {
        row()
            .w_full()
            .gap(if vp.medium() { 14. } else { 18. })
            .items_start()
            .min_h(if vp.wide() { 240. } else { 150. })
    };
    for (i, c) in stations.iter().enumerate() {
        if i > 0 {
            let seen = observed(s, stations[i - 1].id, c.id);
            chain = chain.child(
                wire(if seen { "Observed" } else { "Unverified" }, seen, compact).key(format!(
                    "wire-{}-{}",
                    stations[i - 1].id,
                    c.id
                )),
            );
        }
        let (label, tone) = systems::state(s, c);
        let st = station(
            format!("station-{}", c.id),
            c.target,
            c.name,
            c.role,
            &label,
            systems::tone_color(tone),
            !vp.medium(),
        );
        chain = chain.child(if compact { st.w_full() } else { st });
    }
    chain
}

fn status_line(label: String, tone: Tone) -> El {
    row()
        .gap(8.)
        .center_y()
        .child(dot(systems::tone_color(tone)))
        .child(meta_sized(label, 9., fg()).wrap())
}

fn branch(key: &str, title: &str, desc: &str, status: Option<El>, action: El, compact: bool) -> El {
    let mut words = col()
        .grow(if compact { 0. } else { 1. })
        .min_w(0.)
        .gap(4.)
        .child(h3(title, 15.))
        .child(p(desc, 10.));
    if let Some(st) = status {
        words = words.child(st);
    }
    let b = if compact {
        col().w_full().gap(4.).items_start()
    } else {
        row().grow(1.).gap(20.).center_y().justify_between()
    };
    b.key(key).min_w(0.).child(words).child(action)
}

fn profile_branch(s: &State, key: &str, id: &'static str, start: &str, stop: &str) -> (El, El) {
    let (label, tone) = systems::profile_state(s, id);
    let configured = s.config.profiles.iter().any(|p| p.id == id);
    let on = systems::running(s, id);
    let action = if configured {
        ghost(key, if on { stop } else { start }, true, move |s| {
            super::runtime::toggle_profile(s, id)
        })
    } else {
        ghost(key, "Configure ↗", true, |s| s.navigate(Page::Tools))
    };
    (status_line(label, tone), action)
}

fn branches(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    let kind = s.systems.active;
    let (qgc_state, qgc_action) = profile_branch(
        s,
        "branch-qgc",
        "qgc",
        "Open QGroundControl ↗",
        "Stop QGroundControl",
    );
    let qgc = branch(
        "branch-qgc-link",
        "PX4 ↔ QGroundControl",
        "MAVLink / external vehicle setup and flight controls",
        Some(qgc_state),
        qgc_action,
        compact,
    );
    let second = if kind == SIMULATION {
        let (state, action) = profile_branch(
            s,
            "branch-telemetry",
            "telemetry",
            "Start telemetry",
            "Stop telemetry",
        );
        branch(
            "branch-telemetry-link",
            "PX4 → MAVSDK",
            "UDP 14540 / read-only telemetry",
            Some(state),
            action,
            compact,
        )
    } else {
        let (label, tone) = systems::components(ROS)
            .iter()
            .find(|c| c.id == "mcap")
            .map(|c| systems::state(s, c))
            .unwrap_or_else(|| ("Not checked".into(), Tone::Idle));
        branch(
            "branch-mcap-link",
            "ROS 2 → MCAP",
            "Live topics / sensor recording",
            Some(status_line(label, tone)),
            ghost("open-recordings", "Open recordings ↗", true, |s| {
                s.ui.data_tab = 0;
                s.navigate(Page::Data)
            }),
            compact,
        )
    };
    let wrap = if compact {
        col().gap(20.)
    } else {
        row().gap(if vp.medium() { 24. } else { 40. }).items_start()
    };
    wrap.key("system-branches")
        .w_full()
        .min_w(0.)
        .mt(8.)
        .mb(26.)
        .child(qgc)
        .child(second)
}

fn caveat(s: &State) -> String {
    match s.systems.active {
        systems::PHYSICAL => match fresh_heartbeats(s) {
            0 => "No physical vehicle identified. A fresh autopilot heartbeat is required; no flight commands are sent.".into(),
            n => format!(
                "{n} vehicle{} identified by a fresh autopilot heartbeat. No flight commands are sent.",
                if n == 1 { "" } else { "s" }
            ),
        },
        systems::AI => "A prepared runtime does not establish model compatibility. Choose a model and prepared inputs for a project in Data & models.".into(),
        _ => "Dashed lines describe intended routing; a solid line marks observed traffic. A running process does not prove vehicle readiness or message arrival.".into(),
    }
}

pub(super) fn section(s: &State, vp: &Viewport) -> El {
    let kind = s.systems.active;
    let mut section = col()
        .key("system-topology")
        .w_full()
        .min_w(0.)
        .pt(24.)
        .pb(8.)
        .child(section_head(
            "System connections",
            meta("Planned topology / live evidence where observed").wrap(),
            vp.compact(),
        ))
        .child(chain(s, vp));
    if kind == SIMULATION || kind == ROS {
        section = section.child(branches(s, vp));
    }
    section.child(notice(caveat(s)).key("topology-notice"))
}
