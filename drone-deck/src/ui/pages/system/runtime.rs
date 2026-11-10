//! Runtime: one row per component of the active system with its observed process state.
use super::*;
use systems::{Backing, Component};

/// Starts or stops a launch profile and records the request.
pub(super) fn toggle_profile(s: &mut State, id: &str) {
    let Some(index) = s.config.profiles.iter().position(|p| p.id == id) else {
        s.notice = format!("No {id} profile is configured. Add it in Tools & config.");
        return;
    };
    let name = s.config.profiles[index].name.clone();
    if systems::running(s, id) {
        s.send(Request::Stop(id.into()));
        s.notice = format!("Stopping {name}. This is a process stop, not a vehicle command.");
        crate::activity::record(s, format!("Stopped {name}"));
    } else {
        s.start(index);
        if systems::running(s, id) {
            crate::activity::record(s, format!("Started {name}"));
        }
    }
}

fn to_data(tab: usize) -> impl Fn(&mut State) + 'static {
    move |s| {
        s.ui.data_tab = tab;
        s.navigate(Page::Data)
    }
}

fn action(s: &State, c: &Component) -> El {
    let key = format!("toggle-{}", c.id);
    match c.backing {
        Backing::Profile(id) => {
            if !s.config.profiles.iter().any(|p| p.id == id) {
                return ghost(key, "Configure ↗", true, |s| s.navigate(Page::Tools));
            }
            let on = systems::running(s, id);
            ghost(key, if on { "Stop" } else { "Start" }, true, move |s| {
                toggle_profile(s, id)
            })
        }
        // Started and stopped with the workspace session.
        Backing::Router | Backing::Scripts => el(),
        Backing::Link if s.systems.current().source.simulated() => el(),
        Backing::Link => ghost(
            key,
            if hardware::connected(s) {
                "Disconnect"
            } else {
                "Connect"
            },
            true,
            hardware::toggle,
        ),
        Backing::Endpoint => ghost(key, "Edit ↗", true, |_| {})
            .on_press_tx(|_: &mut State, tx| tx.scroll_center("hardware-endpoint")),
        Backing::Recorder => ghost(key, "Recordings ↗", true, to_data(0)),
        Backing::Pipelines => ghost(key, "Pipelines ↗", true, to_data(2)),
        Backing::Projects => ghost(key, "Projects ↗", true, to_data(3)),
        Backing::Tlog => {
            let dir = vehicle::tlog_dir().display().to_string();
            ghost(key, "Open ↗", s.vehicle.tlog().is_some(), move |s| {
                s.open(dir.clone())
            })
        }
    }
}

/// Opens Activity → Process output filtered to tool `id`.
pub(crate) fn show_output(s: &mut State, id: &str) {
    s.activity.tab = 1;
    s.activity.filter = Some(id.to_string());
    s.navigate(Page::Activity);
}

fn component_row(s: &State, c: &Component, vp: &Viewport) -> El {
    let (label, tone) = systems::state(s, c);
    // A tool that wrote output gets a direct way to it.
    let output = match c.backing {
        Backing::Profile(id) if s.logs.iter().any(|l| l.starts_with(&format!("[{id}]"))) => {
            Some(small_link(format!("output-{}", c.id), "Output ↗", move |s| {
                show_output(s, id)
            }))
        }
        _ => None,
    };
    let name = col()
        .min_w(0.)
        .gap(4.)
        .child(caps(c.name, 12., 700., 0.045, fg()).wrap())
        .child(meta_sized(c.role, 9., muted()).wrap());
    let target = meta_sized(c.target, 9., muted()).wrap();
    let state = row()
        .min_w(0.)
        .gap(8.)
        .center_y()
        .child(dot(systems::tone_color(tone)))
        .child(meta_sized(label, 9., fg()).wrap().grow(1.));
    let state = match output {
        Some(link) => col().min_w(0.).gap(2.).child(state).child(row().child(link)),
        None => state,
    };
    let action = action(s, c);
    let key = format!("runtime-{}", c.id);
    if vp.compact() {
        let pair = |a: El, b: El| {
            row()
                .w_full()
                .min_w(0.)
                .gap(12.)
                .center_y()
                .child(a.grow(1.))
                .child(b.grow(1.))
        };
        return ruled(false).key(key).py(20.).child(
            col()
                .w_full()
                .min_w(0.)
                .gap(12.)
                .child(pair(name, target))
                .child(pair(state, row().child(action))),
        );
    }
    ruled(false)
        .key(key)
        .py(if vp.wide() { 20. } else { 10. })
        .gap(if vp.medium() { 12. } else { 20. })
        .center_y()
        .child(name.grow(1.2).min_w(130.))
        .child(target.grow(1.).min_w(110.))
        .child(state.grow(1.).min_w(100.))
        .child(
            row()
                .w(if vp.medium() { 88. } else { 110. })
                .justify_end()
                .child(action),
        )
}

fn label_row(vp: &Viewport) -> El {
    let cell = |t: &str| meta_sized(t, 9., muted()).wrap();
    row()
        .key("runtime-labels")
        .w_full()
        .min_w(0.)
        .py(10.)
        .gap(if vp.medium() { 12. } else { 20. })
        .border_sides([1., 0., 0., 0.], soft())
        .a11y_hidden()
        .child(cell("Component / role").grow(1.2).min_w(130.))
        .child(cell("Execution target").grow(1.).min_w(110.))
        .child(cell("Process state").grow(1.).min_w(100.))
        .child(
            row()
                .w(if vp.medium() { 88. } else { 110. })
                .justify_end()
                .child(cell("Action")),
        )
}

pub(super) fn section(s: &State, vp: &Viewport) -> El {
    let mut list = col()
        .key("runtime-list")
        .w_full()
        .min_w(0.)
        .mt(18.)
        .child(section_head(
            "Runtime",
            ghost("open-tools", "Configuration & paths ↗", true, |s| {
                s.navigate(Page::Tools)
            }),
            vp.compact(),
        ));
    if !vp.compact() {
        list = list.child(label_row(vp));
    }
    for c in systems::components(s.systems.current()) {
        list = list.child(component_row(s, c, vp));
    }
    list
}
