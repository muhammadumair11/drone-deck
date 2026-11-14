//! 01 System: the active system's session, connections, live evidence and runtime.
use super::*;
mod details;
mod listeners;
pub(crate) mod panels;
mod runtime;
mod topology;

const DESC: &str = "Manage the tools behind your UAV workflow. Configuration, process state and live connection evidence stay distinct.";

/// A fresh install: no system yet, so the page offers the wizard.
fn first_system(compact: bool) -> El {
    col()
        .key("system-page")
        .w_full()
        .min_w(0.)
        .child(mast(
            "No system yet",
            "Build your first system.",
            "A system is three pieces you choose: the vehicle, the world it flies in, and what listens to it. Drone Deck finds the tools you already have, installs only what is missing, and then shows a workspace with exactly those pieces.",
            Some(row().child(pill(
                "first-system",
                "Start: choose a vehicle",
                true,
                crate::guided::new_system,
            ))),
            compact,
        ))
}

pub(super) fn page(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    if s.systems.is_empty() {
        return first_system(compact);
    }
    let current = s.systems.current();
    let vehicle = if current.is_physical() {
        "Real drone"
    } else if current.is_replay() {
        "Recording"
    } else {
        "Simulated"
    };
    let eyebrow = format!(
        "{vehicle} / {}",
        if current.prepared {
            "Configured"
        } else {
            "Preparation required"
        }
    );
    let mut page = col().key("system-page").w_full().min_w(0.).child(mast(
        &eyebrow,
        current.label(),
        DESC,
        Some(row().child(session_action(s))),
        compact,
    ));
    if let Some(launch) = &s.session {
        page = page.child(launch_progress(s, launch, compact));
    }
    if let Some((id, why)) = &s.launch_failure {
        page = page.child(launch_failure(id, why));
    }
    page = page.child(guidance(s, compact)).child(pieces(current));
    // One panel per piece the system has (`panels`); flight controls only in the vehicle
    // panel of a simulator Drone Deck started, never for a real drone.
    for panel in panels::panels(current) {
        page = page.child(panels::view(s, vp, panel));
    }
    let unused = panels::unused(current);
    if !unused.is_empty() {
        page = page.child(
            body(
                format!(
                    "Not shown because this system doesn't use them: {}.",
                    unused.join(", ")
                ),
                11.,
                muted(),
            )
            .key("system-unused")
            .wrap()
            .pt(16.),
        );
    }
    page
}

/// The system's pieces in one line under the title: vehicle and world, then its listeners.
fn pieces(sys: &systems::System) -> El {
    let mut parts = vec![sys.summary()];
    let listeners = sys.consumers_summary();
    if !listeners.is_empty() {
        parts.push(listeners);
    }
    row()
        .key("system-pieces")
        .w_full()
        .min_w(0.)
        .gaps(4., 12.)
        .flex_wrap()
        .center_y()
        .child(code(parts.join(" / "), 12., fg()).wrap_anywhere())
        .child(ghost(
            "system-edit-pieces",
            "Edit pieces ↗",
            true,
            crate::guided::open,
        ))
}

fn session_action(s: &State) -> El {
    let current = s.systems.current();
    if systems::active_work(s) {
        return pill("start-session", "Stop session", true, |s| {
            s.confirm = Some("stop".into())
        });
    }
    let name = if current.is_replay() && current.consumers.inference.is_some() {
        "Open projects"
    } else if current.is_replay() {
        "Open recordings"
    } else if systems::physical_bridge(current) {
        "Connect (bridged)"
    } else if current.is_physical() {
        "Connect read-only"
    } else {
        "Start workspace"
    };
    // Direct controls stay available without guided preparation; `bundle` and `start` still
    // refuse when WSL is not ready or an installation is running.
    pill("start-session", name, true, systems::start_workspace)
}

fn environment_note(s: &State) -> Option<String> {
    if s.systems.current().is_physical() || s.wsl_ready() {
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
    // While a session runs the large first-use strip would push the live view down; a quiet
    // line keeps the way back to guided setup.
    if !s.systems.current().prepared && systems::active_work(s) {
        let text = env.unwrap_or_else(|| {
            "This system runs with its current configuration; guided setup has not prepared it yet.".into()
        });
        return helpstrip(
            "guided-entry",
            None,
            &text,
            ghost(
                "review-setup",
                "Prepare in guided setup ↗",
                true,
                crate::guided::open,
            ),
            compact,
        );
    }
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

/// Where and why the last workspace launch stopped, kept on the page (the status bar line is
/// replaced by the next message) with the way to that tool's output.
fn launch_failure(id: &str, why: &str) -> El {
    let name = systems::component(id).map_or_else(|| id.to_string(), |c| c.name.to_string());
    let output_id = id.to_string();
    let message = format!("Workspace stopped at {name}: {why}");
    col()
        .key("launch-failure")
        .role("alert")
        .label(message.clone())
        .w_full()
        .min_w(0.)
        .mt(8.)
        .mb(16.)
        .px(16.)
        .py(12.)
        .gap(6.)
        .fill(danger().alpha(0.12))
        .border_sides([0., 0., 0., 4.], danger())
        .child(
            caps(
                format!("Workspace stopped at {name}"),
                11.,
                700.,
                0.08,
                danger(),
            )
            .wrap(),
        )
        .child(body(why, 13., fg()))
        .child(body(
            "Later tools were not started. Nothing was sent to a vehicle.",
            12.,
            muted(),
        ))
        .child(
            row()
                .gaps(0., 20.)
                .flex_wrap()
                .center_y()
                .child(small_link(
                    "launch-failure-output",
                    "Show output ↗",
                    move |s| runtime::show_output(s, &output_id),
                ))
                .child(
                    small_link("launch-failure-copy", "Copy message", |_| {}).on_press_tx(
                        move |s: &mut State, tx| {
                            tx.copy(message.clone());
                            s.notice = "Failure message copied.".into();
                        },
                    ),
                )
                .child(small_link("launch-failure-dismiss", "Dismiss", |s| {
                    s.launch_failure = None
                })),
        )
}

/// What the staged launch waits for before starting the next tool.
fn awaited(id: &str) -> &'static str {
    systems::adapter::readiness_of(id)
        .awaited()
        .or_else(|| vehicle::awaited(id))
        .unwrap_or("process start")
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
