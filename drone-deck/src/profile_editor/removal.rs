//! Removing user-created launch profiles never uninstalls their tools or stops processes.
use super::*;

fn custom(s: &State) -> Option<&backend::Profile> {
    let form = s.form.as_ref()?;
    let p = s.config.profiles.get(s.selected)?;
    (!form.creating && p.id.starts_with("custom-")).then_some(p)
}

fn blocked(s: &State, id: &str) -> bool {
    systems::running(s, id) || systems::staged(s, id)
}

fn request(s: &mut State) {
    let Some(p) = custom(s) else { return };
    if blocked(s, &p.id) {
        return refuse(s, "Stop this tool before removing its launch profile.");
    }
    if let Some(form) = s.form.as_mut() {
        form.confirm_remove = true;
        form.error.clear();
    }
}

fn remove(s: &mut State) {
    let headless = s.headless;
    remove_with(s, |config| {
        if headless {
            Ok(())
        } else {
            backend::save(config)
        }
    });
}

/// Commit the new configuration before changing the UI, so write failures lose nothing.
fn remove_with(s: &mut State, persist: impl FnOnce(&Config) -> Result<(), String>) {
    let Some(p) = custom(s) else { return };
    if !s.form.as_ref().is_some_and(|f| f.confirm_remove) {
        return;
    }
    if blocked(s, &p.id) {
        return refuse(s, "Stop this tool before removing its launch profile.");
    }
    let (id, name) = (p.id.clone(), p.name.clone());
    let mut next = s.config.clone();
    next.profiles.remove(s.selected);
    if let Err(error) = persist(&next) {
        return refuse(s, &format!("The tool could not be removed: {error}"));
    }
    s.config = next;
    s.selected = s.selected.min(s.config.profiles.len().saturating_sub(1));
    s.detected.remove(&id);
    s.status.remove(&id);
    s.readiness.remove(&id);
    systems::poll(s);
    s.form = None;
    s.ui.expanded = None;
    activity::record(s, format!("Removed tool profile {name}"));
    s.notice = format!("{name} removed from Drone Deck. Installed files and saved data were kept.");
}

pub(super) fn button(s: &State) -> El {
    let Some(p) = custom(s) else { return el() };
    ghost("remove-tool", "Remove tool", !blocked(s, &p.id), request)
        .help("Remove this custom launch profile from Drone Deck; keep installed files and data")
}

pub(super) fn view(s: &State) -> El {
    let Some(p) = custom(s) else { return el() };
    let locked = blocked(s, &p.id);
    if !s.form.as_ref().is_some_and(|f| f.confirm_remove) {
        return el();
    }
    col()
        .key("tool-remove-confirm")
        .role("alertdialog")
        .label("Confirm tool removal")
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(body(
            format!("Remove {} from Drone Deck? This removes its saved launch profile only. Installed software, projects, recordings and logs are kept.", p.name),
            12., fg(),
        ).wrap())
        .child(row().gaps(8., 16.).center_y().flex_wrap()
            .child(pill("confirm-remove-tool", "Remove tool", !locked, remove))
            .child(ghost("cancel-remove-tool", "Keep tool", true, |s| {
                if let Some(form) = s.form.as_mut() {
                    form.confirm_remove = false;
                    form.error.clear();
                }
            })))
}

#[cfg(test)]
#[path = "removal_tests.rs"]
mod tests;
