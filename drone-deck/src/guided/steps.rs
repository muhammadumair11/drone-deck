//! One configuration flow. These chapters edit a draft; Review is the only save point.
use super::view::{Frame, frame, next};
use super::*;
use crate::ui::kit::*;
use crate::ui::theme::*;
use crate::ui::vehicle_form as form;

fn at(s: &mut State) -> &mut systems::Draft {
    &mut s.guided.draft
}
fn label(t: &str) -> El {
    caps(t, 10., 400., 0.08, muted())
}
fn controls(s: &State, content: El) -> El {
    if s.setup.active() {
        content.disabled(true).inert().dim(0.6)
    } else {
        content
    }
}

pub(super) fn vehicle(s: &State, f: &Frame) -> El {
    let d = &s.guided.draft;
    let details = match d.vehicle {
        1 => form::physical(s, d, at),
        2 => col().gap(12.).child(form::replay(d, at))
            .child(p("Play recorded topics for analysis. To repeat commands in a fresh simulator, open Re-simulation.", 12.))
            .child(ghost("wizard-resimulation", "Re-simulation ↗", !s.setup.active(), |s| {
                s.ui.data_tab = 5;
                s.navigate(ui::Page::Data);
            })),
        _ => col().gap(8.).child(label("Autopilot")).child(form::autopilot_tabs(d, at)),
    };
    let content = controls(
        s,
        col()
            .w_full()
            .min_w(0.)
            .gap(18.)
            .child(
                labeled(
                    "System name",
                    field(&d.name, "name", "My system")
                        .on_edit(|s: &mut State| &mut s.guided.draft.name),
                )
                .max_w(440.),
            )
            .child(form::vehicle_tabs(d, at))
            .child(details),
    );
    let content = col().w_full().gap(12.).child(content).child(
        body(d.error.clone(), 11., danger())
            .key("nameerror")
            .role("status"),
    );
    frame(
        s,
        f,
        "01 / Vehicle",
        "What are you working with?",
        "Choose a simulated drone, a physical drone, or a recording. The following steps use these choices.",
        content,
        if s.setup.active() {
            "Configuration is locked while installation is active."
        } else {
            "Nothing installs or starts while choosing."
        },
        next(
            if d.vehicle == 0 {
                "Choose the world →"
            } else {
                "Choose listeners →"
            },
            !s.setup.active(),
            |s| match systems::clean_name(&s.guided.draft.name.text) {
                Ok(_) => {
                    s.guided.draft.error.clear();
                    go(s, if s.guided.draft.vehicle == 0 { 1 } else { 2 });
                }
                Err(e) => s.guided.draft.error = e,
            },
        ),
    )
}

pub(super) fn world(s: &State, f: &Frame) -> El {
    let d = &s.guided.draft;
    let valid = systems::adapter::world_for(d.autopilot, &d.world);
    let content = controls(s, form::worlds(d, target(s), at, f.narrow()));
    let content = col().w_full().gap(12.).child(content).child(
        body(
            valid.as_ref().err().cloned().unwrap_or_default(),
            11.,
            danger(),
        )
        .key("world-error")
        .role("status"),
    );
    frame(
        s,
        f,
        "02 / World",
        "Where does it fly?",
        "Choose a simulator compatible with your autopilot, then configure its world and vehicle.",
        content,
        "A disabled option explains why it cannot be used.",
        next(
            "Choose listeners →",
            valid.is_ok() && !s.setup.active(),
            |s| {
                if systems::adapter::world_for(s.guided.draft.autopilot, &s.guided.draft.world)
                    .is_ok()
                {
                    go(s, 2);
                }
            },
        ),
    )
}

pub(super) fn listeners(s: &State, f: &Frame) -> El {
    let summary = candidate(s).map(|c| c.summary()).unwrap_or_else(|e| e);
    let content = col()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(
            meta(format!("Your system / {summary}"))
                .key("draft-summary")
                .wrap(),
        )
        .child(controls(
            s,
            form::consumers(&s.guided.draft, at, f.narrow()),
        ))
        .child(
            body(s.guided.draft.error.clone(), 11., danger())
                .key("draft-error")
                .role("status"),
        );
    frame(
        s,
        f,
        "03 / Listeners",
        "Which tools will use it?",
        "Choose the tools and integrations for this system. ROS 2 topics can be recorded; ground stations and scripts use the vehicle connection.",
        content,
        "Review the complete configuration before saving.",
        next(
            "Review configuration →",
            !s.setup.active(),
            |s| match candidate(s) {
                Ok(_) => {
                    s.guided.draft.error.clear();
                    use_preset(s);
                    go(s, REVIEW);
                }
                Err(e) => s.guided.draft.error = e,
            },
        ),
    )
}
