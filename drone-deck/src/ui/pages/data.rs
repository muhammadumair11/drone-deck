//! 03 Data & models: recordings, models, datasets & evaluation, projects.
use super::*;

const TABS: [&str; 4] = ["Recordings", "Models", "Datasets & evaluation", "Projects"];

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
    let tab = s.ui.data_tab.min(3);
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
        .child(if tab == 0 {
            crate::recordings::page(s, vp)
        } else {
            crate::studio::page(s, vp)
        })
}
