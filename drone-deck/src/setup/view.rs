//! Setup building blocks shared by guided setup, Settings, Tools & config and Activity:
//! the computer and Linux environment, a single-recipe installation review and the
//! persistent installation jobs.
use super::*;
use crate::ui::kit::*;
use crate::ui::theme::*;
use rune::Color;

/// A label/value pair in a ruled list.
fn fact(name: &str, value: impl Into<String>, compact: bool) -> El {
    let label = meta(name).wrap();
    let value = body(value, 12., fg());
    let r = ruled(false).py(12.);
    if compact {
        r.child(col().w_full().min_w(0.).gap(6.).child(label).child(value))
    } else {
        r.gap(24.)
            .items_start()
            .child(label.w(180.))
            .child(col().grow(1.).min_w(0.).child(value))
    }
}

/// A selectable row with a radio marker, as in the design's `.choice` list.
pub(crate) fn option_row(
    key: impl Into<String>,
    title: &str,
    detail: &str,
    selected: bool,
    enabled: bool,
    on: impl Fn(&mut State) + 'static,
) -> El {
    let marker = el()
        .size(16., 16.)
        .center()
        .radius(8.)
        .border(1., if selected { fg() } else { rule() })
        .a11y_hidden()
        .child(
            el().size(8., 8.)
                .radius(4.)
                .fill(if selected { fg() } else { Color::TRANSPARENT }),
        );
    let r = ruled(false)
        .key(key)
        .role("radio")
        .label(title)
        .checked(selected)
        .min_h(52.)
        .px(12.)
        .py(10.)
        .gap(16.)
        .center_y()
        .fill(if selected { soft() } else { Color::TRANSPARENT })
        .child(marker)
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .gap(4.)
                .child(caps(title, 12., 500., 0.045, fg()).wrap())
                .child(meta(detail).wrap()),
        );
    if !enabled {
        return r.disabled(true).dim(0.4);
    }
    r.focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(soft())
        .focus_ring(2., 2., fg())
        .on_press(on)
}

/// Opens the review of recipe `id` and scrolls it into view.
fn review_button(key: &str, name: &str, enabled: bool, id: &'static str) -> El {
    let b = ghost(key, name, enabled, |_| {});
    if !enabled {
        return b;
    }
    b.on_press_tx(move |s: &mut State, tx| {
        if let Some(i) = s.setup.recipe_index(id) {
            choose(s, i);
            review(s);
            tx.scroll_into_view_in("main-scroll", "installation-review");
        }
    })
}

fn readiness(host: &Host) -> (&'static str, Color) {
    if host.wsl_ready {
        (
            "A WSL 2 distribution is installed. Select the environment to use below.",
            success(),
        )
    } else if host.wsl_engine {
        (
            "WSL is installed but no WSL 2 distribution is ready. Install or select one below; Windows features may still need attention.",
            warn(),
        )
    } else {
        (
            "WSL is not detected. Review the WSL installation first, then a Linux distribution.",
            warn(),
        )
    }
}

fn host_checks(s: &State, vp: &ui::theme::Viewport) -> El {
    let r = &s.setup;
    let compact = vp.compact();
    let scan = pill(
        "scan-host",
        if r.host.is_some() {
            "Check again"
        } else {
            "Check this computer"
        },
        !r.pending,
        |s| request(s, "scan"),
    );
    let mut block = col()
        .key("host-checks")
        .w_full()
        .min_w(0.)
        .child(section_head(
            "Computer readiness",
            meta("Windows / WSL 2"),
            compact,
        ));
    match &r.host {
        Some(host) => {
            let (text, tone) = readiness(host);
            block = block
                .child(fact(
                    "Windows",
                    format!(
                        "{} / build {} / {}",
                        host.windows, host.build, host.architecture
                    ),
                    compact,
                ))
                .child(fact(
                    "Guided WSL setup",
                    if host.eligible {
                        "Supported on this Windows build"
                    } else {
                        "Not supported automatically; review compatibility manually"
                    },
                    compact,
                ))
                .child(fact("Virtualization", host.virtualization.clone(), compact))
                .child(fact(
                    "Memory / graphics",
                    format!("{:.1} GiB / {}", host.ram_gib, host.gpu),
                    compact,
                ))
                .child(fact(
                    "Free space",
                    format!(
                        "{:.1} GiB on the user drive. WSL data may live on another drive.",
                        host.disk_gib
                    ),
                    compact,
                ))
                .child(
                    ruled(false)
                        .py(12.)
                        .gap(10.)
                        .center_y()
                        .child(dot(tone))
                        .child(body(text, 12., fg()).grow(1.)),
                );
        }
        None => {
            block = block.child(p(
                "Not checked yet. Drone Deck works before Linux is installed; checking reads Windows and WSL without changing them.",
                12.,
            ));
        }
    }
    block
        .child(
            meta_sized(r.message.clone(), 10., fg())
                .wrap()
                .mt(14.)
                .role("status"),
        )
        .child(row().mt(10.).child(scan))
}

fn distributions(s: &State, vp: &ui::theme::Viewport) -> El {
    let r = &s.setup;
    let compact = vp.compact();
    let active = r.active();
    let mut block = col()
        .key("distribution-setup")
        .w_full()
        .min_w(0.)
        .child(section_head(
            "Linux environment",
            meta("Existing or new"),
            compact,
        ));
    let installed = r.host.as_ref().map(|h| h.distros.as_slice()).unwrap_or(&[]);
    if r.host.is_some() && installed.is_empty() {
        block = block.child(p("No Linux distribution is installed yet.", 12.).pb(8.));
    }
    for distro in installed {
        let name = distro.name.clone();
        let in_use = distro.name == s.config.distro;
        block = block.child(option_row(
            format!("installed-distro-{}", distro.name),
            &distro.name,
            &format!(
                "WSL {} / {}{}",
                distro.version,
                if distro.state.is_empty() {
                    "Installed"
                } else {
                    &distro.state
                },
                if in_use { " / in use" } else { "" }
            ),
            r.distro.text.trim().eq_ignore_ascii_case(&distro.name),
            !active,
            move |s| select_distro(s, &name),
        ));
    }
    block = block.child(
        labeled(
            "Distribution identifier",
            field(&r.distro, "setup-distro", "Ubuntu-24.04")
                .on_edit(|s: &mut State| &mut s.setup.distro),
        )
        .mt(16.)
        .max_w(440.),
    );
    block = block.child(
        row()
            .mt(12.)
            .gaps(8., 16.)
            .center_y()
            .flex_wrap()
            .child(pill(
                "use-installed-distro",
                "Use this environment",
                !r.pending && !active && r.can_use_distro(),
                use_distro,
            ))
            .child(ghost(
                "initialize-distro",
                "Initialize Linux user ↗",
                !r.pending && r.can_initialize(),
                |s| request(s, "initialize"),
            ))
            .child(ghost(
                "load-distro-catalog",
                "Available distributions",
                !r.pending,
                |s| request(s, "online"),
            )),
    );
    if !r.online.is_empty() {
        let mut list = col().w_full().min_w(0.);
        for (index, distro) in r.online.iter().enumerate() {
            let name = distro.name.clone();
            list = list.child(option_row(
                format!("online-distro-{index}"),
                &format!("{} / {}", distro.name, distro.label),
                &distro.support,
                r.distro.text.trim() == distro.name,
                !active,
                move |s| select_distro(s, &name),
            ));
        }
        block = block.child(
            col()
                .key("online-distros")
                .mt(12.)
                .w_full()
                .min_w(0.)
                .max_h(300.)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list),
        );
    }
    let wsl_installable = r.host.as_ref().is_some_and(|h| h.eligible && !h.wsl_ready);
    block = block
        .child(
            row()
                .mt(12.)
                .gaps(8., 16.)
                .flex_wrap()
                .child(review_button(
                    "review-wsl",
                    "Review WSL installation →",
                    !r.pending && !active && wsl_installable,
                    "wsl",
                ))
                .child(review_button(
                    "review-distro",
                    "Review distribution installation →",
                    !r.pending && !active,
                    "distro",
                )),
        )
        .child(notice(
            "Installing a distribution may need Windows approval or a restart, and its first launch asks for a Linux user. Existing distributions are kept.",
        ));
    let recipe = &r.recipes[r.selected].id;
    if r.review && (recipe == "wsl" || recipe == "distro") {
        block = block.child(install_review(s).mt(16.));
    }
    block
}

/// Computer readiness and the Linux environment choice (keys `host-checks`,
/// `distribution-setup`). Shows the installation review for WSL or a distribution inline.
pub(crate) fn environment(s: &State, vp: &ui::theme::Viewport) -> El {
    col()
        .key("environment-setup")
        .w_full()
        .min_w(0.)
        .gap(40.)
        .child(host_checks(s, vp))
        .child(distributions(s, vp))
}

/// The review of one managed installation (`installation-review`). Empty unless a review
/// was requested with `setup::review`.
pub(crate) fn install_review(s: &State) -> El {
    let r = &s.setup;
    if !r.review {
        return el();
    }
    let recipe = &r.recipes[r.selected];
    let mut facts = col()
        .w_full()
        .min_w(0.)
        .child(fact("Version", recipe.version.clone(), true))
        .child(fact(
            "Target",
            if recipe.target == "WSL" {
                format!("WSL / {}", r.distro.text.trim())
            } else {
                recipe.target.clone()
            },
            true,
        ));
    if !recipe.destination.is_empty() {
        facts = facts.child(fact("Destination", recipe.destination.clone(), true));
    }
    if !recipe.privileges.is_empty() {
        facts = facts.child(fact("Privileges", recipe.privileges.clone(), true));
    }
    if !recipe.packages.is_empty() {
        facts = facts.child(fact(
            "Packages / components",
            recipe.packages.join(", "),
            true,
        ));
    }
    if !recipe.verification.is_empty() {
        facts = facts.child(fact("Verification", recipe.verification.clone(), true));
    }
    let mut steps = col().w_full().min_w(0.).gap(6.);
    for (i, step) in recipe.steps.iter().enumerate() {
        steps = steps.child(
            row()
                .w_full()
                .min_w(0.)
                .gap(14.)
                .items_start()
                .child(meta(format!("{:02}", i + 1)).w(24.).pt(3.))
                .child(body(step, 11., fg()).grow(1.)),
        );
    }
    let can_install = recipe.automatic && !s.busy() && !r.pending;
    col()
        .key("installation-review")
        .w_full()
        .min_w(0.)
        .gap(12.)
        .pt(18.)
        .border_sides([1., 0., 0., 0.], fg())
        .child(meta("Review installation / nothing installs until you confirm"))
        .child(h3(&recipe.name, 19.))
        .child(p(&recipe.summary, 12.).max_w(760.))
        .child(facts)
        .child(meta("Steps").mt(6.))
        .child(steps)
        .child(checkbox(
            "configure-after-setup",
            "Apply the verified launch configuration after success",
            r.configure_after,
            |s| s.setup.configure_after = !s.setup.configure_after,
        ))
        .child(notice(
            "Compatible existing tools are offered for reuse instead. Shared Linux prerequisites may be installed. Source builds have no reliable size or duration estimate. Jobs keep their logs and continue if you close Drone Deck.",
        ))
        .child(if s.headless {
            notice("Headless preview: installations are disabled.")
        } else if s.busy() {
            notice("Stop the session (owned tools, vehicle link, recordings, pipeline jobs) and installations before installing.")
        } else {
            el()
        })
        .child(
            row()
                .gaps(8., 16.)
                .center_y()
                .flex_wrap()
                .child(pill(
                    "confirm-managed-install",
                    "Install reviewed tool",
                    can_install,
                    |s| {
                        let name = s.setup.recipes[s.setup.selected].name.clone();
                        request(s, "start");
                        if s.setup.pending {
                            activity::record(s, format!("Started reviewed installation: {name}"));
                        }
                    },
                ))
                .child(ghost("cancel-install-review", "Back", true, |s| {
                    s.setup.review = false
                })),
        )
}

fn status_tone(job: &Job) -> Color {
    match job.status.as_str() {
        "Ready" | "Existing installation" => success(),
        "Failed" | "Interrupted" => danger(),
        "Cancelled" => muted(),
        "Needs input" | "Needs restart" | "Needs attention" => warn(),
        _ => fg(),
    }
}

fn job_row(s: &State, index: usize, job: &Job) -> El {
    let id = job.id.clone();
    let selected = s.setup.selected_job.as_deref() == Some(job.id.as_str());
    let status = row()
        .gap(8.)
        .center_y()
        .child(dot(status_tone(job)))
        .child(meta_sized(job.status.clone(), 10., fg()).wrap());
    let step = meta(if job.total > 0 {
        format!("Step {}/{}", job.step, job.total)
    } else {
        "Not started".into()
    });
    let main = col()
        .w_full()
        .min_w(0.)
        .gap(6.)
        .child(
            row()
                .w_full()
                .min_w(0.)
                .gaps(4., 24.)
                .center_y()
                .flex_wrap()
                .child(h3(&job.tool, 15.).grow(1.))
                .child(status)
                .child(step),
        )
        .child(p(&job.message, 11.));
    ruled(true)
        .key(format!("setup-job-{index}"))
        .px(8.)
        .py(16.)
        .child(main)
        .role("button")
        .label(format!("{} / {}", job.tool, job.status))
        .focusable(true)
        .focus_selected(selected)
        .cursor(rune::Cursor::Pointer)
        .fill(if selected { soft() } else { Color::TRANSPARENT })
        .hover(soft())
        .focus_ring(2., 2., fg())
        .on_press(move |s: &mut State| s.setup.selected_job = Some(id.clone()))
}

/// Log tail of a job, newest at the bottom.
pub(crate) fn job_log(job: &Job, height: f32) -> El {
    let text = if job.log_tail.trim().is_empty() {
        "No output yet.".to_string()
    } else {
        job.log_tail.clone()
    };
    col()
        .key("setup-job-log")
        .w_full()
        .min_w(0.)
        .max_h(height)
        .pl(16.)
        .py(4.)
        .border_sides([0., 0., 0., 1.], rule())
        .scroll(ScrollSpec { stick_bottom: true })
        .child(
            code(text, 11., muted())
                .w_full()
                .wrap_anywhere()
                .selectable(true),
        )
}

/// Persistent installation jobs with the selected job's controls and log (`setup-jobs`).
pub(crate) fn jobs_panel(s: &State) -> El {
    let r = &s.setup;
    let mut panel = col().key("setup-jobs").w_full().min_w(0.);
    if r.jobs.is_empty() {
        return panel.child(empty(
            "setup-jobs-empty",
            "No installations yet",
            "Checking tools never installs or updates anything. Installations you review and start appear here with their logs, and they survive app restarts.",
        ));
    }
    for (index, job) in r.jobs.iter().enumerate() {
        panel = panel.child(job_row(s, index, job));
    }
    let Some(job) = r
        .jobs
        .iter()
        .find(|j| Some(&j.id) == r.selected_job.as_ref())
    else {
        return panel.child(notice("Select a job to see its controls and log."));
    };
    panel
        .child(
            row()
                .mt(12.)
                .gaps(8., 16.)
                .center_y()
                .flex_wrap()
                .child(ghost(
                    "cancel-setup-job",
                    "Cancel after current step",
                    job.alive && !r.pending,
                    |s| {
                        request(s, "cancel");
                        activity::record(s, "Requested installation cancellation");
                    },
                ))
                .child({
                    let enabled = !job.alive && !r.pending;
                    let b = ghost("retry-setup-job", "Review retry →", enabled, retry);
                    if enabled {
                        b.on_press_tx(|s: &mut State, tx| {
                            retry(s);
                            tx.scroll_into_view_in("main-scroll", "installation-review");
                        })
                    } else {
                        b
                    }
                })
                .child(pill(
                    "apply-installed-tool",
                    "Apply verified configuration",
                    job.result.is_some() && !job.alive,
                    |s| {
                        profiles::use_job_result(s);
                        if let Some(tool) = s.setup.last_applied.clone() {
                            activity::record(s, format!("Applied verified configuration for {tool}"));
                        }
                    },
                )),
        )
        .child(notice(
            "Cancelling lets the active package manager or build step finish. Retry rechecks completed work; installed shared dependencies are kept.",
        ))
        .child(install_review_for_retry(s))
        .child(job_log(job, 260.).mt(8.))
}

/// "Review retry" reopens the job's recipe review; show it next to the jobs.
fn install_review_for_retry(s: &State) -> El {
    let id = &s.setup.recipes[s.setup.selected].id;
    if s.setup.review && id != "wsl" && id != "distro" {
        install_review(s)
    } else {
        el()
    }
}
