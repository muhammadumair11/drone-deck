//! Guided setup: Intention → The system → Preparation → First run.
//!
//! A focused route for creating a system from a template, or preparing an existing one, on
//! this computer. It drives the real setup backend (host scan, workflow detection, reviewed
//! batch installation) and ends by saving the system and returning to the management
//! workspace.
use crate::*;
mod content;
mod prep;
mod run;
#[cfg(test)]
mod tests;
mod view;
pub(crate) use content::{PRESET, STORIES};
pub(crate) use view::{header, page};

pub(crate) const CHAPTERS: [&str; 4] = ["Intention", "The system", "Preparation", "First run"];

/// The computer check in Preparation: host scan, then tool detection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Check {
    Idle,
    Host,
    Tools,
}

pub(crate) struct Guided {
    /// 0..4, see `CHAPTERS`.
    pub chapter: usize,
    /// The template being prepared (index into `systems::KINDS`).
    pub choice: usize,
    /// The saved system guided setup was opened to update; None for "New system…".
    pub base: Option<String>,
    /// The saved system this preparation updates: `base` while its template is chosen, or
    /// the system created from the template. None until a new system is created.
    pub existing: Option<String>,
    pub name: TextEdit,
    pub name_error: String,
    /// Optional existing location for the system's primary tool.
    pub path: TextEdit,
    pub path_error: bool,
    /// The reviewed-changes acknowledgement in Preparation.
    pub consent: bool,
    pub prep_error: String,
    pub check: Check,
    /// The reviewed plan was applied (or a saved queue resumed) in this preparation.
    pub applied: bool,
    /// First run: current task, whether its action was requested, and completion.
    pub task: usize,
    pub awaiting: bool,
    pub done: bool,
    /// Observations confirmed in First run, for the receipt.
    pub evidence: Vec<String>,
    /// The process (profile id or `project-<name>`) seen Starting or Running after a First
    /// run step requested it; a finished status counts only for this process.
    pub launched: Option<String>,
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
            choice: systems::SIMULATION,
            base: None,
            existing: None,
            name: TextEdit::new(""),
            name_error: String::new(),
            path: TextEdit::new(""),
            path_error: false,
            consent: false,
            prep_error: String::new(),
            check: Check::Idle,
            applied: false,
            task: 0,
            awaiting: false,
            done: false,
            evidence: Vec::new(),
            launched: None,
            last_query: None,
            queries: 0,
            came_from: ui::Page::System,
            opened_for: String::new(),
        }
    }
}

/// The system whose preset the running workflow queue installs, if a queue runs and one
/// matches.
pub(crate) fn batch_system(s: &State) -> Option<usize> {
    s.setup.batch.as_ref()?;
    (0..PRESET.len()).find(|&k| PRESET[k] == s.setup.workflow)
}

/// The saved system this preparation updates, if any.
pub(crate) fn target(s: &State) -> Option<&systems::System> {
    s.guided
        .existing
        .as_deref()
        .and_then(|id| s.systems.get(id))
}

/// The workflow preset for this preparation: the target system's source and consumers, or
/// the chosen template for a new system.
pub(crate) fn preset(s: &State) -> usize {
    target(s).map_or(PRESET[s.guided.choice.min(3)], systems::preset)
}

/// Preparation for the active system, unless a running queue installs another template.
fn fresh(s: &State) -> Guided {
    let current = s.systems.current();
    let mut g = Guided {
        choice: current.template_index(),
        name: TextEdit::new(current.label()),
        base: Some(current.id.clone()),
        existing: Some(current.id.clone()),
        opened_for: current.id.clone(),
        ..Guided::default()
    };
    if let Some(kind) = batch_system(s).filter(|k| *k != g.choice) {
        g.choice = kind;
        g.existing = None;
        g.name = TextEdit::new(s.systems.suggest_name(systems::KINDS[kind]));
    }
    g
}

/// Changes the template being prepared, resetting everything tied to the previous choice.
/// The system guided setup was opened for stays the target only for its own template;
/// another template prepares a new system.
pub(super) fn set_choice(s: &mut State, i: usize) {
    if s.guided.choice == i {
        return;
    }
    let g = &mut s.guided;
    g.choice = i;
    g.path = TextEdit::new("");
    g.path_error = false;
    g.consent = false;
    g.applied = false;
    g.prep_error.clear();
    g.check = Check::Idle;
    g.task = 0;
    g.awaiting = false;
    g.done = false;
    g.evidence.clear();
    g.launched = None;
    g.last_query = None;
    g.queries = 0;
    s.setup.queue.clear();
    let base = s
        .guided
        .base
        .as_deref()
        .and_then(|id| s.systems.get(id))
        .map(|b| (b.id.clone(), b.label().to_string(), b.template_index()));
    let typed = s.guided.name.text.trim().to_string();
    match base {
        Some((id, label, template)) if template == i => {
            s.guided.existing = Some(id);
            s.guided.name = TextEdit::new(label);
        }
        base => {
            s.guided.existing = None;
            let suggested = systems::KINDS
                .iter()
                .any(|k| typed.starts_with(k) || s.systems.suggest_name(k) == typed);
            if typed.is_empty() || suggested || base.is_some_and(|(_, label, _)| label == typed) {
                s.guided.name = TextEdit::new(s.systems.suggest_name(systems::KINDS[i]));
            }
        }
    }
}

/// A running workflow queue owns the preparation: follow the system it installs for.
fn follow_batch(s: &mut State) {
    if s.setup.batch.is_none() {
        return;
    }
    if let Some(kind) = batch_system(s) {
        set_choice(s, kind);
    }
    s.guided.applied = true;
}

/// Points the workflow at this preparation's preset. A tool check queued for another
/// preset is dropped, so its progress never refers to the wrong tool list.
pub(super) fn use_preset(s: &mut State) {
    s.setup.queue.clear();
    s.setup.workflow = preset(s);
}

/// Opens guided setup for the active system. A preparation already under way for the same
/// active system is kept. A running workflow queue selects the system it installs for.
pub(crate) fn open(s: &mut State) {
    let from = if s.page == ui::Page::Setup {
        s.guided.came_from
    } else {
        s.page
    };
    if s.guided.chapter == 0 || s.guided.opened_for != s.systems.active_id() {
        s.guided = fresh(s);
    }
    follow_batch(s);
    s.guided.came_from = from;
    s.navigate(ui::Page::Setup);
}

/// "New system…": guided setup from the first chapter for a system that does not exist yet.
/// It is created from the chosen template when its name is confirmed in The system.
pub(crate) fn new_system(s: &mut State) {
    let from = if s.page == ui::Page::Setup {
        s.guided.came_from
    } else {
        s.page
    };
    s.guided = Guided {
        name: TextEdit::new(s.systems.suggest_name(systems::KINDS[systems::SIMULATION])),
        opened_for: s.systems.active_id().to_string(),
        ..Guided::default()
    };
    follow_batch(s);
    s.guided.came_from = from;
    s.navigate(ui::Page::Setup);
}

/// Starts over with the same system (or a new one), keeping where guided setup returns to.
pub(crate) fn restart(s: &mut State) {
    let from = s.guided.came_from;
    let base = s.guided.base.clone();
    s.guided = fresh(s);
    if base.is_none() {
        s.guided.base = None;
        s.guided.existing = None;
        s.guided.choice = systems::SIMULATION;
        s.guided.name = TextEdit::new(s.systems.suggest_name(systems::KINDS[0]));
    }
    s.guided.came_from = from;
    s.ui.reset_scroll = true;
}

pub(crate) fn go(s: &mut State, chapter: usize) {
    s.guided.chapter = chapter.min(3);
    s.ui.reset_scroll = true;
}

/// The primary tool's recipe id, which the optional existing location applies to.
pub(crate) fn primary_tool(s: &State) -> &'static str {
    let presets = setup::workflow::PRESETS;
    presets[preset(s).min(presets.len() - 1)].1[0]
}

/// The target system's id, creating it from the chosen template under the entered name
/// when this preparation is for a new system.
pub(crate) fn ensure_target(s: &mut State) -> String {
    if let Some(sys) = target(s) {
        return sys.id.clone();
    }
    let name = s.guided.name.text.trim().to_string();
    let id = s.systems.create(s.guided.choice, &name);
    s.guided.existing = Some(id.clone());
    s.guided.base = Some(id.clone());
    let label = s
        .systems
        .get(&id)
        .map(|x| x.label().to_string())
        .unwrap_or_default();
    if let Err(e) = s.systems.save(s.headless) {
        s.notice = format!("{label} was created, but it could not be saved: {e}");
    }
    activity::record(
        s,
        format!(
            "Created system {label} from the {} template",
            systems::KINDS[s.guided.choice.min(3)]
        ),
    );
    id
}

/// Saves the system as prepared under its name and makes it the active system. Switching
/// is always allowed (running work stays with the system that started it), so this returns
/// true unless the system could not be found.
pub(crate) fn mark_prepared(s: &mut State) -> bool {
    let id = ensure_target(s);
    let name = s.guided.name.text.trim().to_string();
    let Some(sys) = s.systems.get_mut(&id) else {
        return false;
    };
    let first = !sys.prepared;
    sys.prepared = true;
    if systems::clean_name(&name).is_ok() {
        sys.name = name;
    }
    let label = sys.label().to_string();
    let active = s.select_system(&id);
    s.guided.opened_for = s.systems.active_id().to_string();
    if let Err(e) = s.systems.save(s.headless) {
        s.notice = format!("The system is prepared, but it could not be saved: {e}");
    }
    if first {
        activity::record(s, format!("Prepared {label}"));
    }
    active
}

/// Saves the system and returns to the management workspace once it is the active system.
pub(crate) fn open_workspace(s: &mut State) {
    if mark_prepared(s) {
        s.navigate(ui::Page::System);
    }
}

/// Per-frame work: sequences the computer check (host scan → tool detection) and notices a
/// workflow queue started or resumed outside this view.
pub(crate) fn poll(s: &mut State) -> bool {
    let mut changed = false;
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
    s.guided.chapter = 2;
    if s.guided.name.text.trim().is_empty() {
        s.guided.name = TextEdit::new("My first system");
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
            for (i, id) in setup::workflow::ids(s).iter().enumerate() {
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
