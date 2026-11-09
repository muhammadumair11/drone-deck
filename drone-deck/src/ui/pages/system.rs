//! 01 System: the active system's session, connections, live evidence and runtime.
use super::*;
use systems::{AI, PHYSICAL};
mod runtime;
mod topology;

const DESC: &str = "Manage the tools behind your UAV workflow. Configuration, process state and live connection evidence stay distinct.";

pub(super) fn page(s: &State, vp: &Viewport) -> El {
    let kind = s.systems.active;
    let compact = vp.compact();
    let eyebrow = format!(
        "{} / {}",
        s.systems.name(kind),
        if s.systems.prepared(kind) {
            "Configured"
        } else {
            "Preparation required"
        }
    );
    let mut page = col().key("system-page").w_full().min_w(0.).child(mast(
        &eyebrow,
        "Your system. In one place.",
        DESC,
        Some(row().child(session_action(s))),
        compact,
    ));
    if let Some(launch) = &s.session {
        page = page.child(launch_progress(s, launch, compact));
    }
    page = page
        .child(guidance(s, compact))
        .child(topology::section(s, vp));
    if kind == PHYSICAL {
        page = page.child(hardware::monitor(s, vp));
    } else if kind != AI && (systems::running(s, "telemetry") || !s.metrics.is_empty()) {
        page = page.child(telemetry(s, compact));
    }
    page.child(runtime::section(s, vp))
}

fn session_action(s: &State) -> El {
    let kind = s.systems.active;
    if systems::active_work(s) {
        return pill("start-session", "Stop session", true, |s| {
            s.confirm = Some("stop".into())
        });
    }
    let name = match kind {
        AI => "Open projects",
        PHYSICAL => "Start monitoring",
        _ => "Start workspace",
    };
    // Direct controls stay available without guided preparation; `bundle` and `start` still
    // refuse when WSL is not ready or an installation is running.
    pill("start-session", name, true, systems::start_workspace)
}

fn environment_note(s: &State) -> Option<String> {
    if s.systems.active == PHYSICAL || s.wsl_ready() {
        return None;
    }
    Some(if s.headless {
        "Headless preview: the Linux environment is not checked and no tool starts.".into()
    } else if s.setup.host.is_none() {
        "Checking the Linux environment. Linux tools start once WSL 2 is ready.".into()
    } else {
        format!(
            "{} / WSL 2 is not ready, so Linux tools cannot start. Prepare it in Settings → Environment.",
            s.config.distro
        )
    })
}

fn guidance(s: &State, compact: bool) -> El {
    let kind = s.systems.active;
    let env = environment_note(s);
    if !s.systems.prepared(kind) {
        let mut text = "Choose what you want to do. Guided setup finds existing tools, explains the connections and prepares only what this system needs.".to_string();
        if let Some(e) = env {
            text = format!("{text} {e}");
        }
        return helpstrip(
            "guided-entry",
            Some("First time here? Build the foundation."),
            &text,
            pill(
                "setup-entry",
                "Prepare a system →",
                true,
                crate::guided::open,
            ),
            compact,
        );
    }
    let text = env.unwrap_or_else(|| {
        "Configuration saved. Use the direct controls below, or return to guided setup when your system changes.".into()
    });
    helpstrip(
        "guided-entry",
        None,
        &text,
        ghost("review-setup", "Review setup ↗", true, crate::guided::open),
        compact,
    )
}

/// What the staged launch waits for before starting the next tool.
fn awaited(id: &str) -> &'static str {
    match id {
        "sih" => "vehicle status",
        "relay" => "first forwarded packet",
        _ => "process start",
    }
}

fn launch_progress(s: &State, launch: &session::Launch, compact: bool) -> El {
    let stages = launch.stages();
    let at = launch.stage();
    let mut steps = if compact {
        col().gap(12.)
    } else {
        row().gap(32.).flex_wrap()
    };
    for (i, id) in stages.iter().enumerate() {
        let name = systems::all_components()
            .into_iter()
            .find(|c| matches!(c.backing, systems::Backing::Profile(p) if p == *id))
            .map(|c| c.name.to_string())
            .or_else(|| {
                s.config
                    .profiles
                    .iter()
                    .find(|p| p.id == *id)
                    .map(|p| p.name.clone())
            })
            .unwrap_or_else(|| id.to_string());
        let (label, color) = if i < at {
            let (label, tone) = systems::profile_state(s, id);
            (label, systems::tone_color(tone))
        } else if i == at {
            (
                format!(
                    "Waiting for {} / {}s of 60s",
                    awaited(id),
                    launch.elapsed().as_secs()
                ),
                fg(),
            )
        } else {
            ("Queued".to_string(), muted())
        };
        steps = steps.child(
            col()
                .key(format!("launch-stage-{id}"))
                .min_w(0.)
                .gap(6.)
                .child(meta_sized(format!("{:02}", i + 1), 9., muted()))
                .child(caps(name, 12., 700., 0.045, if i > at { muted() } else { fg() }).wrap())
                .child(
                    row()
                        .gap(8.)
                        .center_y()
                        .child(dot(color))
                        .child(meta_sized(label, 9., fg()).wrap()),
                ),
        );
    }
    col()
        .key("launch-progress")
        .role("status")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .py(18.)
        .mb(8.)
        .border_sides([1., 0., 1., 0.], soft())
        .child(
            row()
                .w_full()
                .gaps(6., 24.)
                .flex_wrap()
                .justify_between()
                .child(h3("Workspace launch", 15.))
                .child(meta("Each tool waits for the previous one's readiness")),
        )
        .child(steps)
}

fn reading(label: &str, value: El) -> El {
    col()
        .w(140.)
        .min_w(0.)
        .gap(6.)
        .child(meta_sized(label, 9., muted()))
        .child(value)
}

fn telemetry(s: &State, compact: bool) -> El {
    let link =
        s.metrics
            .get("link")
            .map(String::as_str)
            .unwrap_or(if systems::running(s, "telemetry") {
                "No data yet"
            } else {
                "Offline"
            });
    let tone = match link {
        "Connected" => success(),
        "Stale" => warn(),
        _ => muted(),
    };
    let value = |key: &str| {
        meta_sized(
            s.metrics
                .get(key)
                .cloned()
                .unwrap_or_else(|| "No data".into()),
            12.,
            fg(),
        )
        .wrap()
    };
    let readings = row()
        .key("telemetry-readings")
        .w_full()
        .min_w(0.)
        .gaps(18., 24.)
        .flex_wrap()
        .py(18.)
        .border_sides([1., 0., 1., 0.], soft())
        .child(reading(
            "Link",
            row()
                .gap(8.)
                .center_y()
                .child(dot(tone))
                .child(meta_sized(link, 12., fg())),
        ))
        .child(reading("Altitude", value("altitude")))
        .child(reading("Flight mode", value("mode")))
        .child(reading("Latitude", value("latitude")))
        .child(reading("Longitude", value("longitude")))
        .child(reading("Local position", value("position")));
    col()
        .key("vehicle-telemetry")
        .w_full()
        .min_w(0.)
        .pt(18.)
        .pb(26.)
        .gap(12.)
        .child(section_head(
            "Vehicle telemetry",
            meta("Read-only MAVSDK / UDP 14540"),
            compact,
        ))
        .child(readings)
        .child(notice(
            "Values become stale after five seconds without updates. Vehicle setup and flight control stay in QGroundControl.",
        ))
}
