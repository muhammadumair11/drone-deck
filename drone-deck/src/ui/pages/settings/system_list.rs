//! Settings → Systems: every saved system with rename, duplicate, delete (confirmed inline)
//! and its source details and consumers.
use super::*;
use crate::systems::{self, Draft, System};

fn save(s: &mut State) -> Result<(), String> {
    s.systems.save(s.headless)
}

fn open_editor(s: &mut State, id: &str) {
    s.systems.deleting = None;
    s.systems.editor = s.systems.get(id).map(Draft::of);
}

fn draft(s: &mut State) -> &mut Draft {
    s.systems.editor.get_or_insert_with(Draft::default)
}

pub(super) fn save_editor(s: &mut State) {
    let Some(d) = s.systems.editor.as_ref() else {
        return;
    };
    let Some(base) = s.systems.get(&d.id).cloned() else {
        s.systems.editor = None;
        s.notice = "That system no longer exists.".into();
        return;
    };
    let next = match d.apply(&base) {
        Ok(next) => next,
        Err(e) => {
            draft(s).error = e;
            return;
        }
    };
    // Shown in the editor and repeated on save; it never blocks saving.
    let caution = d.caution();
    if next.source != base.source && systems::owns_work(s, &base.id) {
        draft(s).error = format!(
            "{} has running work. Stop its session before changing the source; name and consumers can change now.",
            base.label()
        );
        return;
    }
    if next == base {
        s.systems.editor = None;
        s.notice = "No changes to save.".into();
        return;
    }
    let renamed = next.name != base.name;
    let label = next.label().to_string();
    if let Some(sys) = s.systems.get_mut(&base.id) {
        *sys = next;
    }
    s.systems.changed();
    s.systems.editor = None;
    if base.id == s.systems.active_id() {
        systems::follow_active(s);
    }
    match save(s) {
        Ok(()) => {
            s.notice = match caution {
                Some(c) => format!("Saved {label}. {c}"),
                None => format!("Saved {label}."),
            };
            crate::activity::record(
                s,
                if renamed {
                    format!("Renamed system {} to {label} and saved it", base.label())
                } else {
                    format!("Saved system {label}")
                },
            );
        }
        Err(e) => s.notice = format!("{label} changed, but it could not be saved: {e}"),
    }
}

fn duplicate(s: &mut State, id: &str) {
    let Some(new) = s.systems.duplicate(id) else {
        return;
    };
    let label = s
        .systems
        .get(&new)
        .map(|x| x.label().to_string())
        .unwrap_or_default();
    match save(s) {
        Ok(()) => {
            s.notice = format!("Created {label}. Edit its source and consumers below.");
            crate::activity::record(s, format!("Duplicated a system as {label}"));
        }
        Err(e) => s.notice = format!("{label} was created, but it could not be saved: {e}"),
    }
    open_editor(s, &new);
}

fn confirm_delete(s: &mut State) {
    let Some(id) = s.systems.deleting.take() else {
        return;
    };
    if systems::owns_work(s, &id) {
        s.notice = "Stop the session of this system before deleting it.".into();
        return;
    }
    let was_active = id == s.systems.active_id();
    match s.systems.remove(&id) {
        Ok(gone) => {
            crate::hardware::forget(s, &id);
            if was_active {
                s.ui.tools_filter = 0;
                s.ui.expanded = None;
                systems::follow_active(s);
            }
            // A guided preparation of the deleted system would otherwise recreate it from
            // its template under the old name.
            let g = &s.guided;
            if [&g.base, &g.existing]
                .iter()
                .any(|x| x.as_deref() == Some(id.as_str()))
                || g.opened_for == id
            {
                s.guided = crate::guided::Guided::default();
            }
            match save(s) {
                Ok(()) => s.notice = format!("Deleted {}.", gone.label()),
                Err(e) => {
                    s.notice = format!("Deleted {}, but it could not be saved: {e}", gone.label())
                }
            }
            crate::activity::record(s, format!("Deleted system {}", gone.label()));
        }
        Err(e) => s.notice = e,
    }
}

fn status(s: &State, sys: &System) -> (String, rune::Color) {
    let active = sys.id == s.systems.active_id();
    let running = systems::owns_work(s, &sys.id);
    let prepared = if sys.prepared {
        "Prepared"
    } else {
        "Preparation required"
    };
    let text = match (active, running) {
        (true, true) => format!("Active / running / {prepared}"),
        (true, false) => format!("Active / {prepared}"),
        (false, true) => format!("Running / {prepared}"),
        (false, false) => prepared.to_string(),
    };
    let color = if running {
        success()
    } else if sys.prepared {
        fg()
    } else {
        muted()
    };
    (text, color)
}

fn delete_confirmation(s: &State, sys: &System) -> El {
    let busy = systems::owns_work(s, &sys.id);
    let last = s.systems.list.len() <= 1;
    let text = if last {
        "This is the only system. Create another before deleting it.".to_string()
    } else if busy {
        format!(
            "{} has running work. Stop its session first; deleting never stops processes.",
            sys.label()
        )
    } else {
        format!(
            "Delete {}? Only the saved setup is removed. Installed tools, launch profiles, recordings, models and logs stay where they are.",
            sys.label()
        )
    };
    col()
        .key("system-delete-confirm")
        .role("alertdialog")
        .label("Confirm system deletion")
        .w_full()
        .min_w(0.)
        .gap(12.)
        .pb(20.)
        .child(body(text, 12., if busy || last { warn() } else { fg() }).wrap())
        .child(
            row()
                .gaps(8., 16.)
                .center_y()
                .flex_wrap()
                .child(pill(
                    "confirm-delete-system",
                    "Delete system",
                    !busy && !last,
                    confirm_delete,
                ))
                .child(ghost("cancel-delete-system", "Cancel", true, |s| {
                    s.systems.deleting = None
                })),
        )
}

fn system_row(s: &State, i: usize, sys: &System, compact: bool) -> El {
    let id = sys.id.clone();
    let active = id == s.systems.active_id();
    let (state, color) = status(s, sys);
    let words = col()
        .grow(1.)
        .min_w(0.)
        .gap(6.)
        .child(h3(sys.label(), 15.).wrap())
        .child(meta_sized(sys.source.summary(), 9., muted()).wrap())
        .child(p(sys.consumers_summary(), 11.).max_w(620.))
        .child(
            row()
                .gap(8.)
                .center_y()
                .child(dot(color))
                .child(meta_sized(state, 9., fg()).wrap()),
        );
    let editing = s.systems.editor.as_ref().is_some_and(|d| d.id == id);
    let (a, b, c, d) = (id.clone(), id.clone(), id.clone(), id.clone());
    let actions = row()
        .gaps(0., 14.)
        .center_y()
        .flex_wrap()
        .child(ghost(
            format!("system-activate-{i}"),
            if active { "Active" } else { "Make active" },
            !active,
            move |s| {
                s.select_system(&a);
            },
        ))
        .child(ghost(
            format!("system-edit-{i}"),
            if editing { "Close ▴" } else { "Edit ▾" },
            true,
            move |s| {
                if s.systems.editor.as_ref().is_some_and(|d| d.id == b) {
                    s.systems.editor = None;
                } else {
                    open_editor(s, &b);
                }
            },
        ))
        .child(ghost(
            format!("system-duplicate-{i}"),
            "Duplicate",
            true,
            move |s| duplicate(s, &c),
        ))
        .child(ghost(
            format!("system-delete-{i}"),
            "Delete",
            true,
            move |s| {
                s.systems.editor = None;
                s.systems.deleting = Some(d.clone());
            },
        ));
    let head = if compact {
        col().w_full().min_w(0.).gap(8.).child(words).child(actions)
    } else {
        row()
            .w_full()
            .min_w(0.)
            .gap(24.)
            .center_y()
            .child(words)
            .child(actions)
    };
    let mut block = col()
        .w_full()
        .min_w(0.)
        .child(head.py(if compact { 18. } else { 20. }));
    if s.systems.deleting.as_deref() == Some(id.as_str()) {
        block = block.child(delete_confirmation(s, sys));
    }
    if editing {
        block = block.child(super::system_editor::view(s, sys, compact));
    }
    ruled(false).key(format!("system-row-{i}")).child(block)
}

pub(super) fn section(s: &State, compact: bool) -> El {
    let n = s.systems.list.len();
    let right = row()
        .gaps(4., 16.)
        .center_y()
        .flex_wrap()
        .child(meta(format!("{n} system{}", if n == 1 { "" } else { "s" })))
        .child(ghost("systems-new", "New system ↗", true, |s| {
            crate::guided::new_system(s)
        }));
    let mut list = col().key("system-rows").w_full().min_w(0.);
    for (i, sys) in s.systems.list.iter().enumerate() {
        list = list.child(system_row(s, i, sys, compact));
    }
    col()
        .key("settings-systems")
        .w_full()
        .min_w(0.)
        .pt(if compact { 8. } else { 16. })
        .child(section_head("Systems", right, compact))
        .child(notice(
            "A system is one vehicle source and what it feeds. Switching systems never stops anything: running work stays with the system that started it, and Stop session stops all of it.",
        ).max_w(760.).pb(8.))
        .child(list)
}
