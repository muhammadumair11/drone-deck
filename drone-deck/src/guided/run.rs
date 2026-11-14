//! First run executes only tasks generated from the saved system.
use super::tasks::{Task, tasks};
use super::view::{Frame, frame, next};
use super::*;
use crate::ui::kit::*;
use crate::ui::theme::*;
mod actions;
use actions::*;

fn current(s: &State) -> Option<Task> {
    tasks(s).get(s.guided.task).copied()
}
pub(super) fn on_active(s: &State) -> bool {
    s.guided.existing.as_deref() == Some(s.systems.active_id()) && saved(s)
}
fn target_name(s: &State) -> String {
    target(s).map_or_else(|| s.guided.draft.name.text.clone(), |t| t.label().into())
}
fn activate(s: &mut State) {
    if !saved(s) {
        return;
    }
    let id = ensure_target(s);
    if s.select_system(&id) {
        s.guided.opened_for = id;
        s.guided.prep_error.clear();
    }
}
fn inactive_note(s: &State) -> String {
    format!(
        "{} must be saved and active before First run can act on it.",
        target_name(s)
    )
}

pub(super) fn act(s: &mut State) {
    if !on_active(s) {
        s.guided.prep_error = inactive_note(s);
        return;
    }
    let Some(task) = current(s) else {
        return;
    };
    s.guided.prep_error.clear();
    s.guided.queries = 0;
    s.guided.last_query = None;
    s.guided.launched = None;
    s.guided.playback_seen = false;
    s.guided.prior_runs = if task == Task::Infer {
        s.studio
            .pipes
            .inference_runs()
            .filter(|r| r.status == "Completed")
            .map(|r| r.name.clone())
            .collect()
    } else {
        s.status
            .iter()
            .filter(|(id, st)| {
                id.starts_with("project-") && ["Starting", "Running"].contains(&st.as_str())
            })
            .map(|(id, _)| id.clone())
            .collect()
    };
    s.guided.awaiting = request(s, task);
    if !s.guided.awaiting && s.guided.prep_error.is_empty() {
        s.guided.prep_error = format!("No action started. {}", s.notice);
    }
}
pub(super) fn evidence(s: &State) -> Option<String> {
    if !on_active(s) || !s.guided.awaiting {
        return None;
    }
    observe(s, current(s)?)
}
pub(super) fn observed(s: &State) -> String {
    match current(s) {
        Some(Task::Ros | Task::Record | Task::Finalize | Task::Recording | Task::Playback) => {
            s.recorder.message.clone()
        }
        _ => s.notice.clone(),
    }
}
pub(super) fn poll(s: &mut State) -> bool {
    if s.guided.chapter != RUN || !s.guided.awaiting || s.guided.done || !on_active(s) {
        return false;
    }
    if current(s) == Some(Task::Project) {
        if let Some((id, _)) = s.status.iter().find(|(id, st)| {
            id.starts_with("project-")
                && !s.guided.prior_runs.contains(id)
                && ["Starting", "Running"].contains(&st.as_str())
        }) {
            if s.guided.launched.as_ref() != Some(id) {
                s.guided.launched = Some(id.clone());
                return true;
            }
        }
    }
    if current(s) == Some(Task::Playback) && s.recorder.playing && !s.guided.playback_seen {
        s.guided.playback_seen = true;
        return true;
    }
    if current(s) == Some(Task::Record) && s.recorder.recording {
        let id = s
            .recorder
            .selected
            .as_ref()
            .map(|id| format!("recording:{id}"));
        if id != s.guided.launched {
            s.guided.launched = id;
            return true;
        }
    }
    if s.recorder.pending
        || s.guided.queries >= 5
        || s.guided
            .last_query
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
        || evidence(s).is_some()
    {
        return false;
    }
    let action = match current(s) {
        Some(Task::Ros) if s.recorder.ready => "topics",
        Some(Task::Finalize | Task::Recording) if !s.recorder.recording => "list",
        _ => return false,
    };
    s.guided.queries += 1;
    s.guided.last_query = Some(Instant::now());
    crate::recordings::send(s, action);
    true
}
fn confirm(s: &mut State) {
    let Some(found) = evidence(s) else {
        return;
    };
    let Some(task) = current(s) else {
        return;
    };
    if task == Task::Record {
        s.guided.run_recording = s.recorder.selected.clone();
    }
    s.guided
        .evidence
        .push(format!("{}: {found}", task.words().0));
    activity::record(s, format!("Observed in first run: {found}"));
    s.guided.awaiting = false;
    s.guided.queries = 0;
    s.guided.last_query = None;
    if s.guided.task + 1 >= tasks(s).len() {
        s.guided.done = true;
    } else {
        s.guided.task += 1;
    }
}
fn task_row(s: &State, f: &Frame, i: usize, task: Task) -> El {
    let g = &s.guided;
    let (title, what, note) = task.words();
    let mut main = col()
        .grow(1.)
        .w(0.)
        .min_w(0.)
        .gap(10.)
        .child(h3(title, 20.).wrap().key(format!("run-title-{i}")));
    if i == g.task {
        main = main.child(p(what, 12.)).child(p(note, 12.));
        if !on_active(s) {
            main = main.child(notice(inactive_note(s))).child(ghost(
                "run-activate",
                "Make system active",
                saved(s),
                activate,
            ));
        }
        if g.awaiting {
            main = main.child(
                body(
                    evidence(s)
                        .map(|e| format!("Observed: {e}"))
                        .unwrap_or_else(|| format!("Waiting: {}", observed(s))),
                    11.,
                    muted(),
                )
                .key("task-status")
                .role("status"),
            );
            main = main.child(ghost(
                "task-retry",
                "Run again",
                !s.headless && on_active(s),
                act,
            ));
        }
        if matches!(
            task,
            Task::Ros | Task::Record | Task::Recording | Task::Playback
        ) {
            main = main.child(ghost(
                "task-topics",
                "Recordings and topics ↗",
                true,
                |s| {
                    s.ui.data_tab = 0;
                    s.navigate(ui::Page::Data);
                },
            ));
        }
    } else {
        main = main.child(p(
            if i < g.task {
                "Observation confirmed."
            } else {
                "Available after the previous observation."
            },
            12.,
        ));
    }
    ruled(true)
        .key(format!("run-task-{i}"))
        .py(25.)
        .gap(if f.narrow() { 12. } else { 20. })
        .items_start()
        .child(
            meta(if i < g.task {
                "Done".into()
            } else {
                format!("{:02}", i + 1)
            })
            .w(36.)
            .pt(6.),
        )
        .child(main)
}
pub(super) fn view(s: &State, f: &Frame) -> El {
    let g = &s.guided;
    let list = tasks(s);
    let content = if g.done {
        let mut lines = col().w_full().gap(12.).key("system-receipt");
        for e in &g.evidence {
            lines = lines.child(p(e, 12.));
        }
        lines
    } else {
        let mut lines = col().w_full().min_w(0.).child(meta(format!(
            "Step {} of {}",
            (g.task + 1).min(list.len()),
            list.len()
        )));
        for (i, task) in list.iter().enumerate() {
            lines = lines.child(task_row(s, f, i, *task));
        }
        lines
            .child(
                body(g.prep_error.clone(), 11., danger())
                    .key("run-error")
                    .role("status"),
            )
            .child(ghost(
                "skip-first-run",
                "Skip for now",
                true,
                open_workspace,
            ))
    };
    let action = if g.done || list.is_empty() {
        next("Open system workspace →", true, open_workspace)
    } else if g.awaiting {
        next("Confirm observation →", evidence(s).is_some(), confirm)
    } else {
        next("Try this step", !s.headless && on_active(s), act)
    };
    frame(
        s,
        f,
        "05 / First run",
        "Check your system.",
        &target_name(s),
        content,
        if s.headless {
            "Headless preview / no external process starts."
        } else {
            "Confirm observed results for the tools you selected."
        },
        action,
    )
}
