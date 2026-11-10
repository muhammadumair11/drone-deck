//! 01 System: the active system's session, connections, live evidence and runtime.
use super::*;
use systems::{AI, PHYSICAL};
mod runtime;
mod topology;

const DESC: &str = "Manage the tools behind your UAV workflow. Configuration, process state and live connection evidence stay distinct.";

pub(super) fn page(s: &State, vp: &Viewport) -> El {
    let kind = s.systems.active;
    let compact = vp.compact();
    let current = s.systems.current();
    let eyebrow = format!(
        "{} / {}",
        current.label(),
        if current.prepared {
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
    }
    if kind != AI {
        page = page.child(vehicle::view::live(s, vp));
    }
    // Flight actions and the console exist only for an app-owned simulation; a physical
    // vehicle is read-only and never shows them.
    if current.source.simulated() {
        page = page
            .child(vehicle::view::actions(s, vp))
            .child(vehicle::view::console(s, vp));
    }
    if kind == PHYSICAL {
        page = page.child(vehicle::view::tools(s, vp));
    }
    page.child(runtime::section(s, vp))
}

fn session_action(s: &State) -> El {
    let current = s.systems.current();
    if systems::active_work(s) {
        return pill("start-session", "Stop session", true, |s| {
            s.confirm = Some("stop".into())
        });
    }
    let name = match &current.source {
        systems::Source::Replay { .. } if current.consumers.inference.is_some() => "Open projects",
        systems::Source::Replay { .. } => "Open recordings",
        systems::Source::Physical { .. } if systems::physical_bridge(current) => {
            "Connect (bridged)"
        }
        systems::Source::Physical { .. } => "Connect read-only",
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
    let env = environment_note(s);
    if !s.systems.current().prepared {
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
        _ => vehicle::awaited(id).unwrap_or("process start"),
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
        let name = systems::component(id)
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
            let (label, tone) = systems::component(id)
                .map(|c| systems::state(s, c))
                .unwrap_or_else(|| systems::profile_state(s, id));
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
                .child(meta(match s.systems.owner("session") {
                    // Switching never stops a launch; say whose it is when it is not this one.
                    Some(id) if id != s.systems.active_id() => format!(
                        "Started by {} / each tool waits for the previous one's readiness",
                        s.systems.get(id).map_or("another system", |x| x.label())
                    ),
                    _ => "Each tool waits for the previous one's readiness".into(),
                })),
        )
        .child(steps)
}
