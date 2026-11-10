//! 04 Activity: requested actions, owned process output and installation jobs.
use super::*;
use crate::activity;

const TABS: [&str; 3] = ["History", "Process output", "Installations"];
/// Process output lines shown at once; `State::logs` keeps the last 600.
const VISIBLE_LINES: usize = 400;

/// The mast eyebrow shared by management pages: active system and whether it is prepared.
pub(super) fn eyebrow(s: &State) -> String {
    let kind = s.systems.active;
    let state = if s.systems.prepared(kind) {
        "Configured"
    } else {
        "Preparation required"
    };
    format!("{} / {state}", s.systems.name(kind))
}

pub(super) fn page(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    let tab = s.activity.tab.min(TABS.len() - 1);
    let export = row()
        .gap(12.)
        .flex_wrap()
        .child(pill(
            "export-history",
            "Export history",
            !s.activity.entries.is_empty(),
            export_history,
        ))
        .child(pill(
            "export-diagnostics",
            "Export diagnostics",
            true,
            export_diagnostics,
        ));
    let export = if compact { export.self_start() } else { export };
    let body = match tab {
        1 => output(s, vp),
        2 => installations(s, compact),
        _ => history(s, compact),
    };
    col()
        .key("activity-page")
        .w_full()
        .min_w(0.)
        .child(mast(
            &eyebrow(s),
            "A traceable workspace.",
            "Review what was requested across sessions and systems, the output of processes Drone Deck owns, and installation jobs. Nothing here is vehicle telemetry.",
            Some(export),
            compact,
        ))
        .children(crash_notice(compact))
        .child(tabs("activity-tab", &TABS, tab, |s, i| s.activity.tab = i))
        .child(body)
        .child(disclosure(
            s,
            "activity-ownership",
            "Process and installation ownership",
            || {
                col()
                    .w_full()
                    .min_w(0.)
                    .gap(8.)
                    .child(p(
                        "Installation jobs run outside Drone Deck and continue if the application closes; reopen it to follow them here. Simulators, bridges, recordings and replay started by Drone Deck stop when it closes normally.",
                        12.,
                    ).max_w(760.))
                    .child(p(
                        "Stop session is not a vehicle land or disarm command, and it does not cancel installation jobs.",
                        12.,
                    ).max_w(760.))
            },
        )
        .mt(24.))
}

fn export_history(s: &mut State) {
    if s.headless {
        s.notice = "Headless preview: history export is disabled.".into();
        return;
    }
    s.notice = match activity::export(s) {
        Ok(path) => format!("Activity history exported to {}", path.display()),
        Err(e) => format!("Could not export activity history: {e}"),
    };
}

fn export_diagnostics(s: &mut State) {
    if s.headless {
        s.notice = "Headless preview: diagnostics export is disabled.".into();
        return;
    }
    match crate::diagnostics::export(s) {
        Ok(path) => {
            s.notice = format!(
                "Diagnostics saved to {}. User names and home folders are replaced; review the files before sharing them.",
                path.display()
            );
            activity::record(s, "Exported diagnostics");
            if let Some(dir) = path.parent() {
                s.open(dir.display().to_string());
            }
        }
        Err(e) => s.notice = format!("Could not export diagnostics: {e}"),
    }
}

/// Shown for the session after a launch that found a crash report from the previous one.
fn crash_notice(compact: bool) -> Option<El> {
    let report = crate::diagnostics::pending_crash()?;
    let name = report
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let actions = row()
        .gap(16.)
        .flex_wrap()
        .center_y()
        .child(ghost("crash-open", "Open logs folder ↗", true, |s| {
            s.open(crate::diagnostics::logs_dir().display().to_string())
        }))
        .child(ghost("crash-dismiss", "Dismiss", true, |s| {
            crate::diagnostics::dismiss_crash();
            s.notice =
                "Crash report dismissed. It stays in the logs folder and in diagnostics exports."
                    .into();
        }));
    Some(helpstrip(
        "crash-notice",
        Some("The last session hit an internal error"),
        &format!(
            "Drone Deck saved a crash report as {name}. Export diagnostics to attach it to a bug report; it includes this report, recent app logs and redacted profiles."
        ),
        actions,
        compact,
    ))
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// A coarse category for an entry, from the wording `activity::record` callers use.
fn kind(text: &str) -> &'static str {
    let t = text
        .to_ascii_lowercase()
        .replace("from guided setup", "")
        .replace("(not a vehicle command)", "");
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&[
        "install",
        "setup",
        "prepar",
        "detect",
        "scan",
        "first run",
        "this computer",
    ]) {
        "Setup"
    } else if has(&["vehicle", "parameter", "onboard log"]) {
        "Vehicle"
    } else if has(&[
        "record",
        "replay",
        "extraction",
        "inference",
        "foxglove",
        "model",
        "project",
    ]) {
        "Data"
    } else if has(&[
        "profile",
        "config",
        "environment",
        "distribution",
        "endpoint",
        "token",
    ]) {
        "Configuration"
    } else if has(&["start", "stop", "session", "monitor", "launch", "workspace"]) {
        "Process"
    } else {
        "Action"
    }
}

/// `YYYY-MM-DD HH:MM:SS` as time over date; compact rows keep the time only.
fn stamp(time: &str, compact: bool) -> El {
    let (date, clock) = time.split_once(' ').unwrap_or(("", time));
    let size = if compact { 9. } else { 10. };
    let mut lead = col()
        .gap(4.)
        .child(meta_sized(clock, size, muted()).wrap_anywhere());
    if !compact && !date.is_empty() {
        lead = lead.child(meta_sized(date, 9., muted()));
    }
    lead
}

fn history(s: &State, compact: bool) -> El {
    let n = s.activity.entries.len();
    let mut list = col()
        .key("activity-history")
        .w_full()
        .min_w(0.)
        .child(section_head(
            "Activity history",
            meta(format!("{} / newest first", plural(n, "event"))),
            compact,
        ));
    if n == 0 {
        return list.child(empty(
            "activity-empty",
            "No activity yet",
            "Start a session, save a tool profile or prepare a system and it is recorded here: what was requested, when, and for which system. History stays on this computer.",
        ));
    }
    for (i, e) in s.activity.entries.iter().enumerate() {
        list = list.child(log_row(
            format!("event-{i}"),
            stamp(&e.time, compact),
            row_text(e.text.clone(), e.system.clone()),
            meta(kind(&e.text)),
            compact,
        ));
    }
    list
}

/// Tool ids present in the process output (`[id] line`), in first-seen order.
fn log_ids(s: &State) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for line in &s.logs {
        if let Some(id) = line
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
            .map(|(id, _)| id)
        {
            let valid = !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if valid && !ids.iter().any(|known| known == id) {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

fn line_color(line: &str) -> rune::Color {
    let has = |words: &[&str]| words.iter().any(|w| line.contains(w));
    if has(&[
        "ERROR", "Error", "error:", "Failed", "FAILED", "FATAL", "panic",
    ]) {
        danger()
    } else if has(&["WARN", "Warning", "warning:"]) {
        warn()
    } else {
        fg()
    }
}

fn export_logs(s: &mut State) {
    if s.headless {
        s.notice = "Headless preview: process output export is disabled.".into();
        return;
    }
    let path = crate::backend::root().join("session.log");
    let body = s.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    s.notice = match std::fs::create_dir_all(crate::backend::root())
        .and_then(|_| std::fs::write(&path, body))
    {
        Ok(()) => format!("Process output exported to {}", path.display()),
        Err(e) => format!("Could not export process output: {e}"),
    };
}

fn output(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    let ids = log_ids(s);
    let filter = s
        .activity
        .filter
        .as_ref()
        .filter(|f| ids.len() > 1 && ids.iter().any(|id| id == *f));
    let selected = filter
        .and_then(|f| ids.iter().position(|id| id == f))
        .map_or(0, |i| i + 1);
    let names: Vec<String> = std::iter::once("All".to_string())
        .chain(ids.iter().map(|id| {
            s.config
                .profiles
                .iter()
                .find(|p| &p.id == id)
                .map_or_else(|| id.clone(), |p| p.name.clone())
        }))
        .collect();
    let labels: Vec<&str> = names.iter().map(String::as_str).collect();
    let choices = ids.clone();
    let lines: Vec<&String> = match filter {
        Some(id) => {
            let prefix = format!("[{id}]");
            s.logs.iter().filter(|l| l.starts_with(&prefix)).collect()
        }
        None => s.logs.iter().collect(),
    };
    let shown = lines.len().min(VISIBLE_LINES);
    let actions = row()
        .gap(16.)
        .center_y()
        .flex_wrap()
        .child(meta(if lines.len() > shown {
            format!("Last {shown} of {} lines", lines.len())
        } else {
            plural(lines.len(), "line")
        }))
        .child(ghost(
            "export-logs",
            "Export log",
            !s.logs.is_empty(),
            export_logs,
        ))
        .child(ghost("clear-logs", "Clear", !s.logs.is_empty(), |s| {
            s.logs.clear();
            s.activity.filter = None;
            s.notice = "Process output cleared. Running processes keep writing new lines.".into();
        }));
    let mut block = col()
        .key("process-output")
        .w_full()
        .min_w(0.)
        .child(section_head("Process output", actions, compact));
    if ids.len() > 1 {
        block = block.child(
            tabs("log-filter", &labels, selected, move |s, i| {
                s.activity.filter = i.checked_sub(1).and_then(|i| choices.get(i).cloned());
            })
            .mt(0.),
        );
    }
    if lines.is_empty() {
        let (title, text) = match filter {
            Some(id) => (
                "No output from this tool yet".to_string(),
                format!("{id} has not written anything since output was last cleared."),
            ),
            None => (
                "No process output yet".to_string(),
                "Start a session on System or a tool in Tools & config; its output streams here. Output is kept for this session only, up to the last 600 lines.".to_string(),
            ),
        };
        return block.child(empty("log-empty", &title, &text));
    }
    let mut view = col().w_full().min_w(0.).gap(4.).py(14.);
    for line in &lines[lines.len() - shown..] {
        view = view.child(
            code(line.as_str(), 11., line_color(line))
                .wrap_anywhere()
                .w_full(),
        );
    }
    block
        .child(
            col()
                .key("log-scroll")
                .role("log")
                .label("Process output")
                .w_full()
                .min_w(0.)
                .max_h((vp.h - 360.).clamp(220., 640.))
                .border_sides([1., 0., 1., 0.], rule())
                .scroll(ScrollSpec { stick_bottom: true })
                .child(view),
        )
        .child(
            meta_sized(
                "Errors in red, warnings in amber. A running process is not proof of readiness.",
                9.,
                muted(),
            )
            .wrap()
            .mt(10.),
        )
}

fn installations(s: &State, compact: bool) -> El {
    let jobs = s.setup.jobs.len();
    let running = s.setup.jobs.iter().filter(|j| j.alive).count();
    let summary = if running > 0 {
        format!("{} / {running} running", plural(jobs, "job"))
    } else {
        plural(jobs, "job")
    };
    let mut block = col()
        .key("activity-installations")
        .w_full()
        .min_w(0.)
        .child(section_head("Installation jobs", meta(summary), compact))
        .child(crate::setup::jobs_panel(s));
    let environment_review = s
        .setup
        .recipes
        .get(s.setup.selected)
        .is_some_and(|r| r.id == "wsl" || r.id == "distro");
    if s.setup.review && environment_review {
        block = block.child(
            col()
                .w_full()
                .min_w(0.)
                .mt(16.)
                .items_start()
                .child(notice(
                    "The Linux environment review continues in Settings.",
                ))
                .child(ghost(
                    "review-environment",
                    "Settings / Environment ↗",
                    true,
                    |s| s.navigate(Page::Settings),
                )),
        );
    }
    block
}
