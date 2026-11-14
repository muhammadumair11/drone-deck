//! The inline editor of one system: name, vehicle (an autopilot in a world, a physical
//! vehicle or a recording) with its details, and consumers. The controls are
//! `ui::vehicle_form`'s, shared with guided setup.
use super::*;
use crate::systems::{Draft, System};
use crate::ui::vehicle_form::{self as form, text_field};

fn draft(s: &mut State) -> &mut Draft {
    s.systems.editor.get_or_insert_with(Draft::default)
}

fn vehicle_details(s: &State, d: &Draft, base: &System, compact: bool) -> El {
    match d.vehicle {
        1 => form::physical(s, d, draft),
        2 => form::replay(d, draft),
        _ => col()
            .w_full()
            .min_w(0.)
            .gap(12.)
            .child(caps("Autopilot", 10., 400., 0.08, muted()))
            .child(form::autopilot_tabs(d, draft))
            .child(caps("World", 10., 400., 0.08, muted()))
            .child(form::worlds(d, Some(base), draft, compact)),
    }
}

pub(super) fn view(s: &State, sys: &System, compact: bool) -> El {
    let Some(d) = s.systems.editor.as_ref() else {
        return el();
    };
    col()
        .key("system-editor")
        .w_full()
        .min_w(0.)
        .gap(16.)
        .pb(24.)
        .child(
            text_field(&d.name, "system-name", "Name", "My system", |s| &mut draft(s).name)
                .max_w(440.),
        )
        .child(
            col()
                .w_full()
                .min_w(0.)
                .child(caps("Vehicle", 10., 400., 0.08, muted()))
                .child(form::vehicle_tabs(d, draft))
                .child(vehicle_details(s, d, sys, compact)),
        )
        .child(
            col()
                .w_full()
                .min_w(0.)
                .gap(8.)
                .child(caps("Consumers", 10., 400., 0.08, muted()))
                .child(form::consumers(d, draft, compact)),
        )
        .child(
            body(d.error.clone(), 11., danger())
                .key("system-error")
                .role("status")
                .w_full(),
        )
        .child(notice(format!(
            "{}. Components, connections and the workspace launch follow the vehicle and listeners; nothing is installed or started by saving.",
            sys.suggested_name()
        )))
        .child(
            row()
                .gaps(8., 16.)
                .center_y()
                .flex_wrap()
                .child(pill("system-save", "Save system", true, super::system_list::save_editor))
                .child(ghost("system-cancel", "Cancel", true, |s| s.systems.editor = None)),
        )
}
