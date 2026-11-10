//! Start requests a close can cut short. The setup and Studio PowerShell helpers run in a
//! kill-on-close job, and a start request can spend tens of seconds in its helper before the
//! helper detaches the install or download worker. Closing Drone Deck in that window stops the
//! helper, so the work may never start. The window cannot refuse to close, so the close records
//! what was cut short in the activity history and app.log, and the next start says so once.
use crate::*;

const CLOSED: &str = "Closing Drone Deck stopped the helper still preparing ";
const STARTED: &str = "At the last close, Drone Deck stopped the helper still preparing ";

/// One warning per start request still with its helper.
pub(crate) fn close_warnings(s: &State) -> Vec<String> {
    let setup = s.setup.starting.as_ref().map(|what| {
        format!("{CLOSED}{what}; it may not have started. Check Activity → Installations and start it again if it is not listed.")
    });
    let studio = s.studio.starting.as_ref().map(|what| {
        format!("{CLOSED}{what}; it may not have started. Check Data → Models → Model downloads and start it again if it is not listed.")
    });
    setup.into_iter().chain(studio).collect()
}

/// Called from `on_close`: records each start request the close cuts short.
pub(crate) fn warn_on_close(s: &mut State) {
    for text in close_warnings(s) {
        diagnostics::log("warn", &text);
        activity::record(s, text);
    }
}

/// The notice for warnings recorded at the last close and not yet shown: those newer than the
/// newest notice already recorded (the history is newest first).
pub(crate) fn pending_notice(entries: &VecDeque<activity::Entry>) -> Option<String> {
    let mut found: Vec<String> = entries
        .iter()
        .take_while(|e| !e.text.starts_with(STARTED))
        .filter_map(|e| e.text.strip_prefix(CLOSED))
        .map(|rest| format!("{STARTED}{rest}"))
        .collect();
    found.reverse();
    (!found.is_empty()).then(|| found.join(" "))
}

/// Runs once at start: shows the notice and records it, so the next start does not repeat it.
/// Returns whether it showed one.
pub(crate) fn notice_at_start(s: &mut State) -> bool {
    let Some(text) = pending_notice(&s.activity.entries) else {
        return false;
    };
    s.notice = if s.notice.is_empty() {
        text.clone()
    } else {
        format!("{text} {}", s.notice)
    };
    activity::record(s, text);
    true
}

#[cfg(test)]
#[path = "close_tests.rs"]
mod tests;
