//! Chapter 3, Preparation: check this computer, review the plan, apply it and follow the
//! installation queue until every tool is verified and configured.
use super::view::{Frame, frame, next};
use super::*;
use crate::setup::{batch, workflow};
use crate::ui::kit::*;
use crate::ui::theme::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    Start,
    Checking,
    Environment,
    Review,
    Applying,
    Attention,
    Prepared,
}

pub(super) fn phase(s: &State) -> Phase {
    let g = &s.guided;
    let preset = s.setup.workflow == PRESET[g.choice];
    if s.setup.batch.is_some() {
        Phase::Applying
    } else if g.applied && preset && workflow::verified_all(s) {
        Phase::Prepared
    } else if g.applied {
        Phase::Attention
    } else if g.check != Check::Idle {
        Phase::Checking
    } else if g.choice != systems::PHYSICAL && s.setup.host.is_some() && !s.wsl_ready() {
        Phase::Environment
    } else if preset && workflow::checked_all(s) {
        Phase::Review
    } else {
        Phase::Start
    }
}

fn start_check(s: &mut State) {
    if s.headless {
        s.guided.prep_error =
            "Headless preview: checking this computer is disabled; nothing was read or changed."
                .into();
        return;
    }
    s.guided.prep_error.clear();
    s.guided.applied = false;
    s.guided.consent = false;
    use_preset(s);
    s.setup.batch_message.clear();
    crate::setup::request(s, "scan");
    s.guided.check = Check::Host;
    let kind = systems::KINDS[s.guided.choice];
    activity::record(s, format!("Checked this computer for {kind}"));
}

fn apply(s: &mut State) {
    if !s.guided.consent {
        s.guided.prep_error = "Review the changes and check the acknowledgement first.".into();
        return;
    }
    use_preset(s);
    batch::begin(s);
    if s.setup.batch.is_some() {
        s.guided.applied = true;
        s.guided.consent = false;
        s.guided.prep_error.clear();
        let kind = systems::KINDS[s.guided.choice];
        activity::record(
            s,
            format!("Applied the reviewed preparation plan for {kind}"),
        );
    } else {
        s.guided.prep_error = s.setup.message.clone();
    }
}

fn plan_summary(s: &State) -> String {
    let (mut reuse, mut install) = (Vec::new(), Vec::new());
    for id in workflow::ids(s) {
        let Some(recipe) = s.setup.recipes.iter().find(|r| r.id == *id) else {
            continue;
        };
        if workflow::valid(s, id).is_some_and(|c| c.compatible) {
            reuse.push(recipe.name.as_str());
        } else {
            install.push(recipe.name.as_str());
        }
    }
    let mut text = String::new();
    if !reuse.is_empty() {
        text += &format!(
            "This plan reuses {} and saves its launch configuration. ",
            reuse.join(", ")
        );
    }
    if install.is_empty() {
        text += "Nothing will be installed.";
    } else {
        text += &format!(
            "It installs {} with reviewed recipes, one job at a time, and configures each tool only after it verifies. Shared Linux packages may require root; Windows installers may request input or a restart. Size and duration are unknown.",
            install.join(", ")
        );
    }
    text
}

fn close_app(s: &State) -> El {
    disclosure(s, "close-app", "What happens if I close the app?", || {
        p(
            "Installation jobs continue and recover their status when you return. Cancel at a safe boundary from the job controls. Simulation and recording processes stop on normal application close. Stopping a session does not land a vehicle.",
            12.,
        )
        .max_w(860.)
    })
}

fn location_note(s: &State) -> El {
    let tool = primary_tool(s.guided.choice);
    match s.setup.overrides.get(tool) {
        Some(path) => notice(format!(
            "Requested existing location: {path}. Discovery validates it before reuse."
        )),
        None => el(),
    }
}

fn error_line(s: &State) -> El {
    let e = &s.guided.prep_error;
    body(e.clone(), 11., danger())
        .key("preperror")
        .role("status")
        .w_full()
}

fn jobs(s: &State) -> El {
    if s.setup.jobs.is_empty() {
        return el();
    }
    disclosure(s, "guided-jobs", "Installation jobs and logs", || {
        crate::setup::jobs_panel(s)
    })
}

/// The selected job when it stopped short of a verified result (not by cancellation).
fn stopped_job(s: &State) -> Option<&crate::setup::Job> {
    s.setup
        .selected_job
        .as_ref()
        .and_then(|id| s.setup.jobs.iter().find(|j| &j.id == id))
        .filter(|j| {
            !j.alive
                && !matches!(
                    j.status.as_str(),
                    "Ready" | "Existing installation" | "Cancelled"
                )
        })
}

fn failed_job(s: &State) -> El {
    let Some(job) = stopped_job(s) else {
        return el();
    };
    let restart = job.status == "Needs restart";
    col()
        .key("failed-job")
        .w_full()
        .min_w(0.)
        .gap(10.)
        .pt(18.)
        .border_sides([1., 0., 0., 0.], danger())
        .child(
            row()
                .gaps(4., 16.)
                .center_y()
                .flex_wrap()
                .child(h3(&job.tool, 19.))
                .child(meta_sized(job.status.clone(), 10., fg())),
        )
        .child(p(&job.message, 12.))
        .child(if restart {
            notice("Restart Windows, reopen Drone Deck and check this computer again. The reviewed queue resumes from its saved checkpoint.")
        } else {
            el()
        })
        .child(crate::setup::job_log(job, 220.))
}

pub(super) fn view(s: &State, f: &Frame) -> El {
    let g = &s.guided;
    let compact = f.narrow();
    let story = &STORIES[g.choice];
    let phase = phase(s);
    let deps = workflow::dependencies(s, compact, story.builtin);
    let groundwork = "Let us do the groundwork.";
    let (title, lead, note): (&str, String, &str) = match phase {
        Phase::Start | Phase::Checking => (
            groundwork,
            "First, check this computer for compatible tools. Existing installations stay intact. Nothing installs as a result of checking.".into(),
            "Checking reads this computer; it changes nothing.",
        ),
        Phase::Environment => (
            "First, a Linux environment.",
            "This system runs its Linux tools inside WSL 2. Choose an installed distribution or review an installation. Tool checks continue once the environment is ready.".into(),
            "Environment installs need Windows approval and may need a restart.",
        ),
        Phase::Review => (
            groundwork,
            "Discovery results are shown below. Review what will be reused, installed and configured.".into(),
            "Nothing changes until you apply the reviewed plan.",
        ),
        Phase::Applying => (
            "Preparing your tools.",
            "Installations run one at a time and continue if you close Drone Deck. Each tool is configured only after it verifies.".into(),
            "Jobs keep their logs and survive app restarts.",
        ),
        Phase::Attention => (
            if stopped_job(s).is_some() {
                "One dependency needs attention."
            } else {
                "Preparation paused."
            },
            if s.setup.batch_message.is_empty() {
                s.setup.message.clone()
            } else {
                s.setup.batch_message.clone()
            },
            "Later jobs are paused. Completed tools are kept; nothing is rolled back.",
        ),
        Phase::Prepared => (
            "The tools are prepared.",
            "Every tool is verified and configured. Connections are still unverified. Return to the system workspace for direct controls, tool configuration and session history.".into(),
            "Configured is not connected: run the system to verify its links.",
        ),
    };
    let mut content = col().w_full().min_w(0.).gap(16.);
    match phase {
        Phase::Start => {
            content = content.child(deps).child(location_note(s)).child(jobs(s));
        }
        Phase::Checking => {
            let progress = match g.check {
                Check::Tools => workflow::progress(s),
                _ => None,
            };
            let label = match progress {
                Some((i, n)) => {
                    let id = s
                        .setup
                        .recipes
                        .get(s.setup.selected)
                        .map(|r| r.name.as_str())
                        .unwrap_or("tools");
                    format!("Checking {id} ({} of {n})", i.max(1))
                }
                None => "Checking Windows and WSL…".into(),
            };
            content = content
                .child(
                    meta_sized(label, 10., fg())
                        .key("check-progress")
                        .role("status"),
                )
                .child(deps)
                .child(location_note(s));
        }
        Phase::Environment => {
            let vp = ui::theme::Viewport { w: f.w, h: 800. };
            content = content
                .child(crate::setup::environment(s, &vp))
                .child(jobs(s))
                .child(meta("Then: tools for this system").mt(24.))
                .child(deps);
        }
        Phase::Review => {
            content = content
                .child(deps)
                .child(location_note(s))
                .child(notice(plan_summary(s)).max_w(860.).mt(8.))
                .child(checkbox(
                    "setup-consent",
                    "I reviewed the changes for this system.",
                    g.consent,
                    |s| {
                        s.guided.consent = !s.guided.consent;
                        s.guided.prep_error.clear();
                    },
                ))
                .child(row().child(small_link(
                    "recheck",
                    "Check this computer again",
                    start_check,
                )))
                .child(close_app(s))
                .child(jobs(s));
        }
        Phase::Applying => {
            if s.setup.workflow != PRESET[g.choice] {
                let preset = workflow::PRESETS[s.setup.workflow.min(workflow::PRESETS.len() - 1)].0;
                content = content.child(notice(format!(
                    "This queue was started for {preset}. It must finish or be cancelled before this system can be prepared."
                )));
            }
            content = content
                .child(batch::progress(s, compact))
                .child(deps.mt(16.))
                .child(close_app(s));
        }
        Phase::Attention => {
            content = content
                .child(failed_job(s))
                .child(notice(
                    "Retry checks this computer again before anything else installs; tools that already verified are reused.",
                ))
                .child(deps)
                .child(jobs(s));
        }
        Phase::Prepared => {
            content = content.child(deps).child(row().child(small_link(
                "continue-first-run",
                "Continue to first run",
                |s| {
                    mark_prepared(s);
                    go(s, 3);
                },
            )));
        }
    }
    content = content.child(error_line(s));
    let action = match phase {
        Phase::Start | Phase::Environment => next("Check this computer", true, start_check),
        Phase::Checking => next("Checking…", false, |_| {}),
        Phase::Review => next("Apply reviewed plan", true, apply),
        Phase::Applying => next("Applying reviewed plan…", false, |_| {}),
        Phase::Attention => next("Retry", !s.setup.pending && !s.headless, start_check),
        Phase::Prepared => next("Open system workspace →", true, open_workspace),
    };
    frame(
        s,
        f,
        "03 / Prepare only what you need",
        title,
        &lead,
        content,
        note,
        action,
    )
}
