//! 03 Data & models: recordings, models, pipelines, projects, flight logs.
use super::*;
mod logs;
mod plot;

const TABS: [&str; 5] = [
    "Recordings",
    "Models",
    "Pipelines",
    "Projects",
    "Flight logs",
];

pub(super) fn page(s: &State, vp: &Viewport) -> El {
    let kind = s.systems.active;
    let eyebrow = format!(
        "{} / {}",
        s.systems.name(kind),
        if s.systems.prepared(kind) {
            "Configured"
        } else {
            "Preparation required"
        }
    );
    let tab = s.ui.data_tab.min(TABS.len() - 1);
    col()
        .key("data-page")
        .w_full()
        .min_w(0.)
        .child(mast(
            &eyebrow,
            "Data stays with the system.",
            "Recordings, models and run outputs have different prerequisites. Preserve source files and their provenance.",
            None,
            vp.compact(),
        ))
        .child(tabs("data-tab", &TABS, tab, |s: &mut State, i: usize| {
            s.ui.data_tab = i
        }))
        .child(if tab == crate::ulog::LOGS_TAB {
            logs::tab(s, vp)
        } else if tab == 0 {
            crate::recordings::page(s, vp)
        } else {
            crate::studio::page(s, vp)
        })
}
