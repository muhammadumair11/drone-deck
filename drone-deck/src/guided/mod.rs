//! Guided setup: Vehicle → World → Listeners → Review & install → First run.
//!
//! A focused route for creating or editing a system on this computer. The first three
//! chapters edit a draft with the same controls as Settings → Systems (`steps`). Review &
//! install drives the real setup backend (host scan, workflow detection, reviewed batch
//! installation, guided manual installs); First run collects evidence that it works.
use crate::*;
pub(crate) mod lifecycle;
mod prep;
mod run;
mod steps;
mod tasks;
#[cfg(test)]
mod tests;
mod view;
use setup::presets::{self, Preset};
pub(crate) use view::{header, page};

pub(crate) const CHAPTERS: [&str; 5] = [
    "Vehicle",
    "World",
    "Listeners",
    "Review & install",
    "First run",
];
/// The chapter that checks this computer and installs what the system needs.
pub(crate) const REVIEW: usize = 3;
/// The chapter that tries the prepared system.
pub(crate) const RUN: usize = 4;

/// The computer check in Preparation: host scan, then tool detection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Check {
    Idle,
    Host,
    Tools,
}

pub(crate) struct Guided {
    /// 0..5, see `CHAPTERS`.
    pub chapter: usize,
    /// The name the wizard last suggested from the pieces; while the name field still holds
    /// it, it follows the pieces (`follow_name`).
    pub suggested: String,
    /// The one working copy, applied only by Save system in Review.
    pub draft: systems::Draft,
    /// Stable editing identity; None until an explicitly new system is saved.
    pub existing: Option<String>,
    pub fingerprint: String,
    pub saved_fingerprint: String,
    /// The reviewed-changes acknowledgement in Preparation.
    pub consent: bool,
    pub prep_error: String,
    pub check: Check,
    /// The reviewed plan was applied (or a saved queue resumed) in this preparation.
    pub applied: bool,
    pub queue_unbound: bool,
    /// First run: current task, whether its action was requested, and completion.
    pub task: usize,
    pub awaiting: bool,
    pub done: bool,
    /// Observations confirmed in First run, for the receipt.
    pub evidence: Vec<String>,
    /// The process (profile id or `project-<name>`) seen Starting or Running after a First
    /// run step requested it; a finished status counts only for this process.
    pub launched: Option<String>,
    pub prior_runs: Vec<String>,
    pub run_recording: Option<String>,
    pub playback_seen: bool,
    /// ROS publisher discovery: when topics were last requested, and how many requests were
    /// made once PX4 and the DDS agent were both up.
    pub last_query: Option<Instant>,
    pub queries: u8,
    /// Where "Drone Deck / Workspace" returns to.
    pub came_from: ui::Page,
    /// The active system id when guided setup was opened.
    pub opened_for: String,
}
impl Default for Guided {
    fn default() -> Self {
        Self {
            chapter: 0,
            suggested: String::new(),
            draft: systems::Draft::default(),
            existing: None,
            fingerprint: String::new(),
            saved_fingerprint: String::new(),
            consent: false,
            prep_error: String::new(),
            check: Check::Idle,
            applied: false,
            queue_unbound: false,
            task: 0,
            awaiting: false,
            done: false,
            evidence: Vec::new(),
            launched: None,
            prior_runs: Vec::new(),
            run_recording: None,
            playback_seen: false,
            last_query: None,
            queries: 0,
            came_from: ui::Page::System,
            opened_for: String::new(),
        }
    }
}

#[cfg(test)]
use lifecycle::batch_system;
use lifecycle::{candidate, follow_batch, reconcile, save_draft};
pub(crate) use lifecycle::{
    ensure_target, mark_prepared, new_system, open, open_workspace, restart,
};
pub(crate) use lifecycle::{saved, target};

/// Setup requirements always follow the current draft.
pub(crate) fn preset(s: &State) -> Preset {
    candidate(s).map_or_else(
        |_| Preset {
            name: "Invalid configuration".into(),
            recipes: vec![],
        },
        |sys| presets::for_system(&sys),
    )
}

#[cfg(test)]
pub(crate) fn template_preset(kind: usize) -> Preset {
    presets::for_system(&systems::model::starter(kind, "", ""))
}

pub(super) fn use_preset(s: &mut State) {
    if s.setup.active() {
        return;
    }
    let wanted = preset(s);
    if s.setup.workflow != wanted {
        s.setup.queue.clear();
    }
    s.setup.workflow = wanted;
}

pub(crate) fn go(s: &mut State, chapter: usize) {
    reconcile(s);
    if chapter == REVIEW {
        use_preset(s);
    }
    let chapter = chapter.min(RUN);
    s.guided.chapter = if chapter == 1 && s.guided.draft.vehicle != 0 {
        2
    } else {
        chapter
    };
    s.ui.reset_scroll = true;
}

/// Per-frame work: sequences the computer check (host scan → tool detection) and notices a
/// workflow queue started or resumed outside this view.
/// A new system's name follows its pieces ("PX4 + jMAVSim + ROS 2") until a name is typed.
fn follow_name(s: &mut State) -> bool {
    if s.guided.existing.is_some() || s.guided.draft.name.text != s.guided.suggested {
        return false;
    }
    let Ok(next) = lifecycle::candidate(s) else {
        return false;
    };
    let name = s.systems.suggest_name(&next.suggested_name());
    if name == s.guided.suggested {
        return false;
    }
    s.guided.draft.name = TextEdit::new(&name);
    s.guided.suggested = name;
    true
}

pub(crate) fn poll(s: &mut State) -> bool {
    let mut changed = reconcile(s) | follow_name(s);
    if s.setup.batch.is_some() && !s.guided.applied {
        follow_batch(s);
        changed = true;
    }
    match s.guided.check {
        Check::Idle => {}
        Check::Host if !s.setup.pending => {
            changed = true;
            if s.setup.host.is_none() {
                s.guided.check = Check::Idle;
                s.guided.prep_error = s.setup.message.clone();
            } else {
                use_preset(s);
                if setup::workflow::needs_wsl(s) && !s.wsl_ready() {
                    s.guided.check = Check::Idle;
                } else {
                    setup::workflow::scan(s);
                    s.guided.check = Check::Tools;
                }
            }
        }
        Check::Tools if !s.setup.pending && s.setup.queue.is_empty() => {
            s.guided.check = Check::Idle;
            if !setup::workflow::checked_all(s) {
                s.guided.prep_error = s.setup.message.clone();
            }
            changed = true;
        }
        _ => {}
    }
    changed | run::poll(s)
}

/// Headless fixtures for Preparation: `env` (no WSL 2), `review` (checked tools),
/// `applying` (a running queue) and `attention` (a paused queue).
pub(crate) fn preview(s: &mut State, phase: &str) {
    setup::preview(s);
    open(s);
    s.guided.chapter = REVIEW;
    if s.guided.draft.name.text.trim().is_empty() {
        s.guided.draft.name = TextEdit::new("My first system");
    }
    use_preset(s);
    let found = |tool: &str, ok: bool| setup::Candidate {
        tool: tool.into(),
        found: ok,
        compatible: ok,
        reason: if ok {
            "Headless preview fixture: compatible installation.".into()
        } else {
            "Headless preview fixture: not found.".into()
        },
        ..Default::default()
    };
    match phase {
        "env" => {
            let host = s.setup.host.as_mut().unwrap();
            host.distros.clear();
            host.wsl_ready = false;
        }
        "review" | "attention" => {
            for (i, id) in setup::workflow::ids(s).to_vec().iter().enumerate() {
                s.setup.checks.insert(id.to_string(), found(id, i == 0));
            }
            if phase == "attention" {
                let tool = setup::workflow::ids(s)
                    .get(1)
                    .and_then(|id| s.setup.recipes.iter().find(|r| r.id == *id))
                    .map(|r| r.name.clone())
                    .unwrap_or_default();
                let id = "20260101T120000-0456cdef".to_string();
                s.setup.jobs = vec![setup::Job {
                    id: id.clone(),
                    tool: tool.clone(),
                    status: "Failed".into(),
                    message: "Headless preview fixture: no installer ran.".into(),
                    step: 4,
                    total: 6,
                    log_tail: "[preview] fixture log; no command was executed".into(),
                    ..Default::default()
                }];
                s.setup.selected_job = Some(id);
                s.guided.applied = true;
                s.setup.batch_message = format!(
                    "Workflow paused at {tool}: Headless preview fixture: no installer ran. Completed components were preserved; review this job, then check the workflow again."
                );
            }
        }
        _ => {
            setup::batch::preview(s);
            s.guided.applied = true;
        }
    }
}
