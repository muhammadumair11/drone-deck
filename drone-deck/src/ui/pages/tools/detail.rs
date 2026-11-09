//! The expanded tool row: guidance, command preview, the launch-profile form and the
//! installation actions, in place of the old inspector and editor page.
use super::*;

/// Setup recipe id for a profile id.
fn recipe_id(id: &str) -> &str {
    match id {
        "sih" => "px4",
        "telemetry" => "mavsdk",
        other => other,
    }
}

fn command(p: &crate::backend::Profile) -> String {
    if p.args.is_empty() {
        p.program.clone()
    } else {
        format!("{} {}", p.program, p.args.join(" "))
    }
}

fn working_directory(s: &State, p: &crate::backend::Profile) -> String {
    if !p.cwd.is_empty() {
        p.cwd.clone()
    } else if p.program == "wsl" {
        format!("Set by the command inside {}", s.config.distro)
    } else {
        "Program default".into()
    }
}

fn copy_command(s: &mut State, tx: &mut rune::Tx, id: &str) {
    let Some(p) = s.config.profiles.iter().find(|p| p.id == id) else {
        return;
    };
    tx.copy(command(p));
    s.notice = format!("{} command copied.", p.name);
}

fn profile_block(s: &State, p: &crate::backend::Profile, vp: &Viewport) -> El {
    let detected = match s.detected.get(&p.id) {
        Some(true) => "Probe found the installation",
        Some(false) => "Probe did not find the installation",
        None => "Installation not checked",
    };
    let copy_id = p.id.clone();
    let left = col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(caps("Command preview", 10., 400., 0.08, muted()))
        .child(
            code(command(p), 12., fg())
                .key("command-preview")
                .wrap_anywhere()
                .selectable(true),
        )
        .child(
            ghost("copy-command", "Copy command", true, |_| {})
                .on_press_tx(move |s: &mut State, tx| copy_command(s, tx, &copy_id))
                .self_start(),
        );
    let right = col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(caps("Working directory", 10., 400., 0.08, muted()))
        .child(
            code(working_directory(s, p), 12., fg())
                .wrap_anywhere()
                .selectable(true),
        )
        .child(
            meta_sized(
                format!("{} / {}", target(&p.program), detected),
                9.,
                muted(),
            )
            .wrap(),
        );
    let editing = s.form.as_ref().is_some_and(|f| {
        !f.creating
            && s.config
                .profiles
                .get(s.selected)
                .is_some_and(|x| x.id == p.id)
    });
    let form = if editing {
        crate::profile_editor::view(s, vp.compact())
    } else {
        let id = p.id.clone();
        row().gap(20.).center_y().flex_wrap().child(ghost(
            "edit-profile",
            "Edit launch profile",
            true,
            move |s| {
                if let Some(i) = s.config.profiles.iter().position(|x| x.id == id) {
                    s.selected = i;
                    crate::profile_editor::open(s);
                }
            },
        ))
    };
    col()
        .w_full()
        .min_w(0.)
        .child(asset(left, right, vp.compact()))
        .child(form)
}

/// QGroundControl's opt-in GPU-safe mode and the hint left by a likely graphics failure.
fn graphics(s: &State, profile: &crate::backend::Profile) -> El {
    let on = crate::qgc_launch::gpu_safe(profile);
    let mut block = col()
        .key("qgc-graphics")
        .w_full()
        .min_w(0.)
        .gap(8.)
        .mt(28.)
        .pt(20.)
        .border_sides([1., 0., 0., 0.], soft())
        .child(caps("Graphics", 10., 400., 0.08, muted()))
        .child(
            checkbox(
                "qgc-gpu-safe",
                "GPU-safe mode (software rendering)",
                on,
                move |s| crate::qgc_launch::set_gpu_safe(s, !on),
            )
            .self_start(),
        )
        .child(
            p(
                "Off by default: QGroundControl draws with the graphics card. Turn this on if it closes right after starting or opens a blank or black window, which usually means its OpenGL / Direct3D driver failed to start. GPU-safe mode adds -swrast and draws on the CPU: maps and video are slower, but it starts on almost any machine. The change is saved to this launch profile.",
                11.,
            )
            .max_w(760.),
        );
    if systems::running(s, "qgc") {
        block = block.child(
            meta_sized(
                "Stop QGroundControl to change this: a running process keeps the renderer it started with.",
                9.,
                muted(),
            )
            .wrap(),
        );
    }
    if let Some(hint) = &s.qgc.hint {
        block = block.child(
            row()
                .key("qgc-graphics-hint")
                .gap(8.)
                .center_y()
                .min_w(0.)
                .child(dot(warn()))
                .child(meta_sized(hint, 9., fg()).wrap().grow(1.)),
        );
    }
    block
}

/// Point setup at this row's recipe. Choosing another recipe clears the location typed for the
/// previous one; the location typed for this recipe is kept.
fn select_recipe(s: &mut State, i: usize) {
    if s.setup.selected != i {
        crate::setup::choose(s, i);
    }
}

fn find_existing(s: &mut State, i: usize, action: &str) {
    select_recipe(s, i);
    crate::setup::request(s, action);
    if action == "detect" && s.setup.pending {
        let name = s.setup.recipes[i].name.clone();
        s.notice = format!("Inspecting {name} at the given location without changing it…");
        crate::activity::record(s, format!("Requested detection of an existing {name}"));
    }
}

fn adopt_existing(s: &mut State, i: usize) {
    if s.setup.selected != i {
        return;
    }
    crate::setup::profiles::adopt(s);
    let name = s.setup.recipes[i].name.clone();
    if s.setup.last_applied.as_deref() == Some(s.setup.recipes[i].id.as_str()) {
        s.notice =
            format!("{name}: detected configuration applied. Nothing was installed or started.");
        crate::activity::record(s, format!("Applied detected configuration for {name}"));
    } else {
        s.notice = s.setup.message.clone();
    }
}

/// Detect an installation at a chosen path and adopt it as the launch configuration.
fn existing(s: &State, i: usize) -> El {
    let r = &s.setup.recipes[i];
    let selected = s.setup.selected == i;
    let idle = !s.setup.pending && !s.setup.active();
    let hint = match (r.target.as_str(), r.id.as_str()) {
        ("Windows", _) => "Full Windows path to the .exe or .jar",
        (_, "jmavsim") => {
            "Linux path for WSL tools; full Windows .exe or .jar path for Windows tools"
        }
        _ => "Linux path inside the WSL environment",
    };
    let mut block = col()
        .key("tool-existing")
        .w_full()
        .min_w(0.)
        .gap(10.)
        .mt(12.)
        .child(caps("Existing installation", 10., 400., 0.08, muted()));
    if s.setup.active() && !selected {
        block = block.child(notice(
            "A setup job or guided workflow is running. Detect existing installations when it finishes.",
        ));
    } else {
        // The field shows this recipe's location; typing into it selects the recipe first.
        let blank = TextEdit::new("");
        let edit = if selected { &s.setup.location } else { &blank };
        block = block.child(labeled(
            "Location (optional)",
            field(edit, "setup-existing-location", hint)
                .label("Existing installation location")
                .on_edit(move |s: &mut State| {
                    if s.setup.selected != i && !s.setup.active() {
                        crate::setup::choose(s, i);
                    }
                    &mut s.setup.location
                }),
        ));
    }
    block = block.child(
        row()
            .gaps(0., 24.)
            .center_y()
            .flex_wrap()
            .child(ghost(
                "browse-existing-tool",
                "Browse for existing tool…",
                idle && !s.headless,
                move |s| find_existing(s, i, "browse"),
            ))
            .child(ghost(
                "detect-selected-tool",
                "Detect existing tool",
                idle,
                move |s| find_existing(s, i, "detect"),
            )),
    );
    if selected && (s.setup.pending || s.setup.last_applied.as_deref() == Some(r.id.as_str())) {
        block = block.child(meta_sized(&s.setup.message, 9., muted()).wrap());
    }
    let Some(c) = s.setup.candidate.as_ref().filter(|c| c.tool == r.id) else {
        return block;
    };
    let (label, tone) = match (c.found, c.compatible) {
        (true, true) => ("Found / compatible", success()),
        (true, false) => ("Found / not compatible", warn()),
        _ => ("Not found", danger()),
    };
    let version = if c.version.is_empty() {
        String::new()
    } else {
        format!(" / {}", c.version)
    };
    let mut result = col()
        .key("tool-candidate")
        .w_full()
        .min_w(0.)
        .gap(8.)
        .pt(12.)
        .border_sides([1., 0., 0., 0.], soft())
        .child(
            row().gap(8.).center_y().min_w(0.).child(dot(tone)).child(
                meta_sized(format!("{label}{version}"), 9., fg())
                    .wrap()
                    .grow(1.),
            ),
        );
    if !c.path.is_empty() {
        result = result.child(code(&c.path, 12., fg()).wrap_anywhere().selectable(true));
    }
    if !c.reason.is_empty() {
        result = result.child(p(&c.reason, 11.).max_w(760.));
    }
    if c.found {
        result = result.child(meta_sized(
            if c.managed {
                "App-managed location"
            } else {
                "Existing installation / not owned by this app"
            },
            9.,
            muted(),
        ));
    }
    let can_adopt = c.found && c.compatible && !s.setup.active() && !s.busy();
    result = result.child(
        pill(
            "adopt-tool",
            "Apply detected configuration",
            can_adopt,
            move |s| adopt_existing(s, i),
        )
        .self_start(),
    );
    if c.found && c.compatible && s.busy() {
        result = result.child(meta_sized(
            "Stop active tools and recordings before changing a launch configuration.",
            9.,
            muted(),
        ));
    }
    block.child(result)
}

fn installation(s: &State, it: &Item) -> El {
    let rid = recipe_id(&it.id).to_string();
    let recipe = s.setup.recipes.iter().position(|r| r.id == rid);
    let profile = it.profile(s);
    let docs = profile
        .map(|p| p.docs.clone())
        .filter(|d| !d.is_empty())
        .or_else(|| recipe.map(|i| s.setup.recipes[i].docs.clone()))
        .filter(|d| !d.is_empty());
    let folder = profile.map(|p| p.cwd.clone()).unwrap_or_default();
    let mut actions = row()
        .key("tool-installation")
        .gaps(0., 24.)
        .center_y()
        .flex_wrap()
        .child(ghost(
            "detect-installation",
            "Detect installation",
            profile.is_some(),
            detect,
        ))
        .child(ghost(
            format!("setup-review-{}", it.id),
            "Review installation",
            recipe.is_some() && !s.setup.pending,
            move |s| {
                if let Some(i) = s.setup.recipes.iter().position(|r| r.id == rid) {
                    crate::setup::choose(s, i);
                    crate::setup::review(s);
                }
            },
        ));
    actions = actions
        .child(ghost(
            "docs",
            "Official docs ↗",
            docs.is_some(),
            move |s| {
                if let Some(d) = &docs {
                    s.open(d.clone())
                }
            },
        ))
        .child(ghost(
            "open-project",
            if folder.is_empty() {
                "Open workspace folder ↗"
            } else {
                "Open folder ↗"
            },
            !s.headless,
            move |s| {
                s.open(if folder.is_empty() {
                    crate::backend::root().to_string_lossy().into_owned()
                } else {
                    folder.clone()
                })
            },
        ));
    let mut block = col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .mt(28.)
        .pt(20.)
        .border_sides([1., 0., 0., 0.], soft())
        .child(caps("Installation", 10., 400., 0.08, muted()))
        .child(actions);
    if let Some(job) = recipe.and_then(|i| {
        let r = &s.setup.recipes[i];
        s.setup
            .jobs
            .iter()
            .find(|j| j.tool == r.name || j.tool == r.id)
    }) {
        let progress = if job.alive && job.total > 0 {
            format!(" / step {} of {}", job.step, job.total)
        } else {
            String::new()
        };
        block = block.child(
            row()
                .key("tool-install-job")
                .gaps(0., 12.)
                .center_y()
                .flex_wrap()
                .child(dot(match job.status.as_str() {
                    "Ready" | "Existing installation" => success(),
                    "Failed" | "Interrupted" => danger(),
                    "Needs input" | "Needs restart" | "Needs attention" => warn(),
                    _ => muted(),
                }))
                .child(
                    meta_sized(
                        format!("Latest installation: {}{progress}", job.status),
                        9.,
                        fg(),
                    )
                    .wrap(),
                )
                .child(ghost(
                    "open-installations",
                    "Installations ↗",
                    true,
                    |s| {
                        s.activity.tab = 2;
                        s.navigate(Page::Activity)
                    },
                )),
        );
    }
    if let Some(i) = recipe {
        block = block.child(existing(s, i));
    }
    match recipe {
        None => {
            block = block.child(notice(
                "No managed recipe for this tool. Configure its command and follow the publisher's installation instructions.",
            ))
        }
        Some(i) if s.setup.review && s.setup.selected == i => {
            block = block.child(crate::setup::install_review(s))
        }
        Some(i) => {
            let r = &s.setup.recipes[i];
            block = block.child(notice(format!(
                "Managed recipe: {} {} on {}. Review shows every step before anything is installed.",
                r.name, r.version, r.target
            )))
        }
    }
    block
}

pub(super) fn view(s: &State, it: &Item, vp: &Viewport) -> El {
    let guidance = match (it.comp, it.profile(s)) {
        (Some(c), _) => c.guidance.to_string(),
        (None, Some(p)) if !p.description.is_empty() => p.description.clone(),
        _ => "Discovery and compatibility checks are required before adopting a path.".into(),
    };
    let mut block = col()
        .key(format!("tool-detail-{}", it.id))
        .w_full()
        .min_w(0.)
        .pb(28.)
        .child(p(guidance, 12.).max_w(760.));
    match it.profile(s) {
        Some(p) if p.id == "qgc" && p.program != "wsl" => {
            block = block.child(profile_block(s, p, vp)).child(graphics(s, p))
        }
        Some(p) => block = block.child(profile_block(s, p, vp)),
        None => {
            block = block.child(
                notice(format!(
                    "{} has no launch profile yet. Review its installation: a verified installation registers the profile.",
                    it.name(s)
                ))
                .mt(12.),
            )
        }
    }
    block.child(installation(s, it))
}
