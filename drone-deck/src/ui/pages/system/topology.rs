//! System connections: the planned chain, with a solid wire only where traffic was observed.
use super::*;
use systems::Tone;
use systems::adapter::{self, Link, Readiness};
#[cfg(test)]
#[path = "topology_tests.rs"]
mod tests;

fn project_running(s: &State) -> bool {
    s.status
        .iter()
        .any(|(k, v)| k.starts_with("project-") && v == "Running")
}

/// Autopilots with a fresh heartbeat on the active system's link (0 or 1).
fn fresh_heartbeats(s: &State) -> usize {
    usize::from(
        s.vehicle
            .shown_for(s.systems.active_id())
            .is_some_and(|snap| snap.fresh),
    )
}

fn router_forwarding(s: &State) -> bool {
    vehicle::router_running(s)
        && s.agent
            .router
            .as_ref()
            .is_some_and(|r| r.state == "forwarding")
}

/// The active system's link is up (a headless scene's preview link counts: its snapshot is
/// the only one, and the window says it is a preview).
fn link_up(s: &State) -> bool {
    s.vehicle
        .shown_for(s.systems.active_id())
        .is_some_and(|snap| snap.link.state == crate::mavlink::LinkState::Connected)
        && (s.vehicle.link.is_some() || s.headless)
}

/// The simulated vehicle is up by its adapter's readiness rule.
fn vehicle_up(s: &State) -> bool {
    systems::vehicle_profile(s.systems.current()).is_some_and(|id| adapter::ready(s, id))
}

/// Whether ROS topics under `prefix` were discovered.
fn topics(s: &State, prefix: &str) -> bool {
    s.recorder
        .discovered
        .iter()
        .any(|t| t.name.starts_with(prefix))
}

/// Whether there is live evidence for the link `from → to`.
fn observed(s: &State, from: &str, to: &str) -> bool {
    let up = |id: &str| s.status.get(id).is_some_and(|v| v == "Running");
    let sim = adapter::by_profile(from);
    let ros = sim.and_then(|a| a.ros.as_ref());
    match (from, to) {
        (_, "router") if sim.is_some_and(|a| a.link == Link::Router) => {
            vehicle_up(s) && router_forwarding(s)
        }
        ("router", "link") => router_forwarding(s) && link_up(s) && fresh_heartbeats(s) > 0,
        (_, "link") if sim.is_some_and(|a| a.link == Link::Direct) => {
            up(from) && link_up(s) && fresh_heartbeats(s) > 0
        }
        ("link", "qgc") => {
            link_up(s)
                && up("qgc")
                && s.vehicle.snapshot.as_ref().is_some_and(|snap| {
                    snap.link.peers.iter().any(|p| {
                        p.rx_frames > 0
                            && p.addr.is_some_and(|a| a.to_string() == vehicle::QGC_PEER)
                    })
                })
        }
        // A simulator whose readiness is its vehicle status proves the bridge with the Agent
        // running; for the others a running simulator and Agent prove nothing until its
        // topics are seen.
        (_, _) if ros.is_some_and(|r| r.agent == to) => {
            let status = matches!(adapter::readiness_of(from), Readiness::Px4Status { .. });
            vehicle_up(s) && up(to) && (status || ros.is_some_and(|r| topics(s, r.topic_prefix)))
        }
        // The camera bridge counts once its camera topics are discovered.
        (_, "rosgz") if sim.is_some() => up("rosgz") && topics(s, "/camera/"),
        ("rosgz", "ros2") => up("rosgz") && topics(s, "/camera/"),
        // Topic discovery counts only while the vehicle is ready and the Agent runs.
        ("dds", "ros2") => vehicle_up(s) && up("dds") && topics(s, "/fmu/"),
        ("ros2", "mcap") => s.recorder.recording,
        ("onnx", "project") => project_running(s),
        ("endpoint", "link") => link_up(s) && fresh_heartbeats(s) > 0,
        ("link", "tlog") => {
            link_up(s)
                && s.vehicle
                    .snapshot
                    .as_ref()
                    .is_some_and(|snap| snap.link.tlog_bytes > 0)
        }
        _ => false,
    }
}

fn chain(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    let stations: Vec<&systems::Component> = systems::chain(s.systems.current()).collect();
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

/// Links beside the chain: QGroundControl and scripts (MAVLink over UDP, two-way; for a
/// physical vehicle too, since they can command it), the Gazebo camera bridge and the MCAP
/// recorder, each only when the system has it.
fn branches(s: &State, vp: &Viewport) -> Option<El> {
    let compact = vp.compact();
    let sys = s.systems.current();
    let c = &sys.consumers;
    let physical = sys.is_physical();
    // A physical vehicle is named by the flight stack the user said it runs, if any.
    let autopilot = match sys.autopilot() {
        Some(kind) => adapter::autopilot_name(kind),
        None if physical => "Vehicle",
        None => "PX4",
    };
    // With ROS the chain shows the DDS path; MAVLink still runs through the router and link.
    let mavlink = if physical {
        "MAVLink through the vehicle link, two-way: it can command the vehicle and write parameters"
    } else if c.ros.agent && systems::routed(sys) {
        "MAVLink through the router and vehicle link / external vehicle setup and flight controls"
    } else {
        "MAVLink / external vehicle setup and flight controls"
    };
    let mut links: Vec<El> = Vec::new();
    if c.qgc {
        let (qgc_state, qgc_action) = profile_branch(
            s,
            "branch-qgc",
            "qgc",
            "Open QGroundControl ↗",
            "Stop QGroundControl",
        );
        links.push(branch(
            "branch-qgc-link",
            &format!("{autopilot} ↔ QGroundControl"),
            mavlink,
            Some(qgc_state),
            qgc_action,
            compact,
        ));
    }
    if let Some(addr) = c.scripts_udp.as_ref() {
        let (label, tone) = systems::component("scripts")
            .map(|c| systems::state(s, c))
            .unwrap_or_else(|| ("Not checked".into(), Tone::Idle));
        links.push(branch(
            "branch-scripts-link",
            &format!("{autopilot} ↔ your scripts"),
            &if physical {
                format!(
                    "MAVLink over UDP {addr} / two-way through the vehicle link: scripts can command the vehicle"
                )
            } else {
                format!("MAVLink over UDP {addr} / two-way through the vehicle link")
            },
            Some(status_line(label, tone)),
            el(),
            compact,
        ));
    }
    let camera = sys
        .adapter()
        .and_then(|a| a.ros.as_ref())
        .is_some_and(|r| r.extra.contains(&"rosgz"));
    if c.ros.agent && camera {
        let (label, tone) = systems::profile_state(s, "rosgz");
        let seen = observed(s, "rosgz", "ros2");
        let (_, action) = profile_branch(
            s,
            "branch-rosgz",
            "rosgz",
            "Start camera bridge",
            "Stop camera bridge",
        );
        links.push(branch(
            "branch-rosgz-link",
            "Gazebo camera → ROS 2",
            "ROS–Gazebo bridge / /camera/image_raw and /camera/camera_info",
            Some(status_line(
                format!(
                    "{label} / camera topics {}",
                    if seen { "observed" } else { "unverified" }
                ),
                if seen { Tone::Good } else { tone },
            )),
            action,
            compact,
        ));
    }
    if c.ros.recorder {
        let (label, tone) = systems::component("mcap")
            .map(|c| systems::state(s, c))
            .unwrap_or_else(|| ("Not checked".into(), Tone::Idle));
        links.push(branch(
            "branch-mcap-link",
            "ROS 2 → MCAP",
            "Live topics / sensor recording",
            Some(status_line(label, tone)),
            ghost("open-recordings", "Open recordings ↗", true, |s| {
                s.ui.data_tab = 0;
                s.navigate(Page::Data)
            }),
            compact,
        ));
    }
    if links.is_empty() {
        return None;
    }
    let mut wrap = if compact {
        col().gap(20.)
    } else {
        row().gap(if vp.medium() { 24. } else { 40. }).items_start()
    };
    for link in links {
        wrap = wrap.child(link);
    }
    Some(
        wrap.key("system-branches")
            .w_full()
            .min_w(0.)
            .mt(8.)
            .mb(26.),
    )
}

fn caveat(s: &State) -> String {
    let sys = s.systems.current();
    if sys.is_physical() {
        let link = if systems::physical_bridge(sys) {
            "Drone Deck only sends read requests; QGroundControl/scripts enabled for this system can command the vehicle through the link."
        } else {
            "The link is read-only."
        };
        match fresh_heartbeats(s) {
            0 => format!(
                "No physical vehicle identified. A fresh autopilot heartbeat is required. {link}"
            ),
            n => format!(
                "{n} vehicle{} identified by a fresh autopilot heartbeat. {link}",
                if n == 1 { "" } else { "s" }
            ),
        }
    } else if sys.is_replay() {
        "A prepared runtime does not establish model compatibility. Choose a model and prepared inputs for a project in Data & models.".into()
    } else {
        "Dashed lines describe intended routing; a solid line marks observed traffic. A running process does not prove vehicle readiness or message arrival.".into()
    }
}

pub(super) fn section(s: &State, vp: &Viewport) -> El {
    let sys = s.systems.current();
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
        .child(
            meta_sized(format!("Source / {}", sys.summary()), 9., muted())
                .key("system-source")
                .wrap()
                .pb(12.),
        )
        .child(chain(s, vp));
    if !sys.is_replay() {
        if let Some(b) = branches(s, vp) {
            section = section.child(b);
        }
    }
    section.child(notice(caveat(s)).key("topology-notice"))
}
