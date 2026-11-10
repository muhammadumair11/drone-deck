//! "Running (previous session)": processes an earlier session left running, with Adopt/Stop.
use super::Leftover;
use crate::State;
use crate::ui::{
    kit::{ghost, helpstrip, log_row, pill, row_text},
    theme::{Viewport, meta, meta_sized, muted},
};
use rune::{El, col, row};
use std::time::Instant;

fn age(started: Option<Instant>) -> String {
    let Some(started) = started else {
        return "Started earlier".into();
    };
    let minutes = started.elapsed().as_secs() / 60;
    match minutes {
        0 => "Started < 1 min ago".into(),
        m if m < 120 => format!("Started {m} min ago"),
        m => format!("Started {} h ago", m / 60),
    }
}

fn entry(s: &State, row_data: &Leftover, compact: bool) -> El {
    let name = s
        .config
        .profiles
        .iter()
        .find(|p| p.id == row_data.key)
        .map_or_else(|| row_data.key.clone(), |p| p.name.clone());
    let detail = if row_data.command.is_empty() {
        format!("pid {} · started outside this session", row_data.pid)
    } else {
        format!("pid {} · {}", row_data.pid, row_data.command)
    };
    let adopt_key = row_data.key.clone();
    let stop_key = row_data.key.clone();
    let actions = row()
        .gap(12.)
        .center_y()
        .flex_wrap()
        .child(pill(
            format!("leftover-adopt-{}", row_data.key),
            "Adopt",
            !row_data.busy,
            move |s| super::adopt(s, &adopt_key),
        ))
        .child(ghost(
            format!("leftover-stop-{}", row_data.key),
            "Stop",
            !row_data.busy,
            move |s| super::stop_leftover(s, &stop_key),
        ));
    let mut text = col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(row_text(name, detail));
    if !row_data.output {
        text = text.child(meta_sized(
            "Output from before adopting is not available",
            9.,
            muted(),
        ));
    }
    log_row(
        format!("leftover-{}", row_data.key),
        meta(age(row_data.started)),
        text.child(actions),
        meta("Running (previous session)"),
        compact,
    )
}

/// The panel above management pages while leftovers exist; `None` otherwise.
pub(crate) fn leftovers(s: &State, vp: &Viewport) -> Option<El> {
    if s.agent.leftovers.is_empty() {
        return None;
    }
    let compact = vp.compact();
    let mut block = col()
        .key("agent-leftovers")
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(helpstrip(
            "agent-leftovers-help",
            Some("Running (previous session)"),
            &format!(
                "These processes were started by Drone Deck earlier and still run in {}. Adopt to follow their output and stop them with the session, or stop them now. Stopping is a process stop, not a vehicle command.",
                s.config.distro
            ),
            row(),
            compact,
        ));
    for leftover in &s.agent.leftovers {
        block = block.child(entry(s, leftover, compact));
    }
    Some(block)
}
