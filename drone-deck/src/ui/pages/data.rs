//! 03 Data & models: recordings, models, pipelines, projects, flight logs.
use super::*;
mod logs;
pub(crate) mod plot;

const TABS: [&str; 6] = [
    "Recordings",
    "Models",
    "Pipelines",
    "Projects",
    "Flight logs",
    "Re-simulation",
];

pub(super) fn page(s: &State, vp: &Viewport) -> El {
    let eyebrow = super::activity::eyebrow(s);
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
        .child(if tab == crate::resim::TAB {
            crate::resim::view::tab(s, vp)
        } else if tab == crate::ulog::LOGS_TAB {
            logs::tab(s, vp)
        } else if tab == 0 {
            crate::recordings::page(s, vp)
        } else {
            crate::studio::page(s, vp)
        })
}
