//! Settings: the Linux environment, display scale, model access and the profile registry.
//! They belong to this computer and apply to every system.
use super::activity::eyebrow;
use super::*;

pub(super) fn page(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    col()
        .key("settings-page")
        .w_full()
        .min_w(0.)
        .child(mast(
            &eyebrow(s),
            "Settings.",
            "Preferences for this computer: the Linux environment tools run in, display scale, model access and the launch profile registry. They apply to every system.",
            None,
            compact,
        ))
        .child(
            col()
                .key("settings-environment")
                .w_full()
                .min_w(0.)
                .pt(if compact { 8. } else { 16. })
                .child(crate::setup::environment(s, vp)),
        )
        .child(display(s, compact))
        .child(section(
            "settings-huggingface",
            "Model access",
            meta("Hugging Face").wrap(),
            crate::studio::settings(s),
            compact,
        ))
        .child(registry(s, compact))
}

fn section(key: &str, title: &str, right: El, content: El, compact: bool) -> El {
    col()
        .key(key)
        .w_full()
        .min_w(0.)
        .pt(if compact { 24. } else { 36. })
        .child(section_head(title, right, compact))
        .child(content)
}

/// A ruled settings row: name and explanation on the left, controls on the right.
fn setting(key: &str, title: &str, desc: &str, controls: El, compact: bool) -> El {
    let words = col()
        .grow(1.)
        .min_w(0.)
        .gap(6.)
        .child(h3(title, 15.))
        .child(p(desc, 11.).max_w(620.));
    let base = ruled(false).key(key).py(if compact { 18. } else { 20. });
    if compact {
        return base.child(
            col()
                .w_full()
                .min_w(0.)
                .gap(12.)
                .child(words)
                .child(controls),
        );
    }
    base.gap(32.).center_y().child(words).child(controls)
}

fn display(s: &State, compact: bool) -> El {
    let zoom = s.ui.zoom_percent;
    let controls = row()
        .gaps(4., 16.)
        .center_y()
        .flex_wrap()
        .child(ghost("settings-zoom-out", "Smaller −", zoom > 50, |s| {
            set_zoom(s, s.ui.zoom_percent as i32 - 5)
        }))
        .child(
            meta_sized(format!("{zoom}%"), 12., fg())
                .key("settings-zoom-value")
                .role("status")
                .label("Current zoom"),
        )
        .child(ghost("settings-zoom-in", "Larger +", zoom < 150, |s| {
            set_zoom(s, s.ui.zoom_percent as i32 + 5)
        }))
        .child(ghost(
            "settings-zoom-reset",
            "Reset to 100%",
            zoom != 100,
            |s| set_zoom(s, 100),
        ));
    let mut keys = col().gap(6.);
    for (combo, effect) in [
        ("Ctrl + −", "Smaller"),
        ("Ctrl + +", "Larger"),
        ("Ctrl + 0", "Reset to 100%"),
        ("Ctrl + ,", "Open Settings"),
    ] {
        keys = keys.child(
            row()
                .gap(16.)
                .center_y()
                .child(meta_sized(combo, 10., fg()).w(72.))
                .child(meta(effect)),
        );
    }
    section(
        "display-settings",
        "Display",
        meta(format!("Zoom {zoom}%")),
        col()
            .w_full()
            .min_w(0.)
            .child(setting(
                "settings-zoom",
                "Interface zoom",
                "Scales every page between 50% and 150%. Saved on this computer and restored at launch.",
                controls,
                compact,
            ))
            .child(setting(
                "settings-shortcuts",
                "Keyboard shortcuts",
                "Available on every page, including guided setup.",
                keys,
                compact,
            )),
        compact,
    )
}

fn open_workspace(s: &mut State) {
    if s.headless {
        s.notice = "Headless preview: opening folders is disabled.".into();
        return;
    }
    let path = crate::backend::root();
    if let Err(e) = std::fs::create_dir_all(&path) {
        s.notice = format!("Could not create the workspace folder: {e}");
        return;
    }
    s.open(path.to_string_lossy().into_owned());
}

fn save_registry(s: &mut State) {
    s.apply();
    if !s.editing {
        crate::activity::record(s, "Saved launch profile registry");
    }
}

/// The registry JSON in a fixed-height scrolling field; `kit::area` does not clip long text.
fn json_editor(s: &State) -> El {
    let field = rune::text_input(&s.editor, rune::TextStyle::new(12., 400.).family(MONO))
        .key("profile-editor")
        .label("Profile registry JSON")
        .on_edit(|s: &mut State| &mut s.editor)
        .w_full()
        .min_w(0.)
        .padding(12.)
        .color(fg())
        .caret(fg(), fg().alpha(0.25));
    rune::auto_grow(field, 12, 30)
        .key("profile-editor-scroll")
        .radius(4.)
        .fill(bg())
        .border(1., rule())
        .focus_within_ring(2., 0., fg())
}

fn registry(s: &State, compact: bool) -> El {
    let profiles = s.config.profiles.len();
    let summary = format!(
        "{profiles} launch profile{} / WSL distribution {}",
        if profiles == 1 { "" } else { "s" },
        s.config.distro
    );
    let profile_actions = row()
        .gaps(4., 16.)
        .center_y()
        .flex_wrap()
        .child(ghost("edit-profiles", "Edit JSON", !s.editing, |s| {
            s.edit();
            // The editor opens in place below; keep the scroll position.
            s.ui.reset_scroll = false;
        }))
        .child(ghost("registry-tools", "Tools & config ↗", true, |s| {
            s.navigate(Page::Tools)
        }));
    let workspace = crate::backend::root().to_string_lossy().into_owned();
    let folder = if compact {
        col().w_full()
    } else {
        col().w(360.)
    };
    let folder = folder
        .min_w(0.)
        .gap(4.)
        .child(code(workspace, 11., fg()).wrap_anywhere().selectable(true))
        .child(ghost(
            "open-workspace",
            "Open folder ↗",
            true,
            open_workspace,
        ));
    let mut body = col()
        .w_full()
        .min_w(0.)
        .child(setting(
            "settings-profiles",
            "Launch profiles",
            "Each profile binds a tool's executable, arguments, working directory, readiness probe and WSL distribution. Use Tools & config for single tools; edit the JSON only for changes the form cannot express.",
            profile_actions,
            compact,
        ))
        .child(setting(
            "settings-workspace",
            "Workspace folder",
            "Profiles, activity history, setup jobs, logs and preferences are stored here.",
            folder,
            compact,
        ));
    if s.editing {
        body = body.child(
            col()
                .key("profile-registry-editor")
                .w_full()
                .min_w(0.)
                .gap(12.)
                .pt(20.)
                .border_sides([1., 0., 0., 0.], rule())
                .child(notice(
                    "Saving validates the registry: profile IDs must be unique letters, digits, - or _. A running profile cannot be removed, and the distribution cannot change while tools, installations, recording or replay run.",
                ))
                .child(json_editor(s))
                .child(
                    row()
                        .gaps(8., 16.)
                        .center_y()
                        .flex_wrap()
                        .child(pill("save-config", "Validate & save", true, save_registry))
                        .child(ghost("cancel-edit", "Cancel", true, |s| {
                            s.editing = false;
                            s.notice = "Registry edits discarded.".into();
                        })),
                ),
        );
    }
    section(
        "profile-registry",
        "Profile registry",
        meta(summary).wrap(),
        body,
        compact,
    )
}
