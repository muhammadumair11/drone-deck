use super::*;
use crate::ui::*;

fn item(
    key: impl Into<String>,
    name: &str,
    detail: &str,
    selected: bool,
    action: impl Fn(&mut State) + 'static,
) -> El {
    col()
        .key(key)
        .role("button")
        .label(name)
        .focusable(true)
        .w_full()
        .min_w(0.)
        .padding(12.)
        .gap(5.)
        .radius(6.)
        .border(1., line())
        .fill(if selected {
            c(232, 244, 255)
        } else {
            surface()
        })
        .focus_ring(2., 2., accent())
        .on_press(action)
        .child(label(name, 14., ink()).w_full().wrap())
        .child(label(detail, 12., muted()).w_full().wrap())
}
pub(crate) fn page(s: &State, width: f32) -> El {
    let r = &s.setup;
    let recipe = &r.recipes[r.selected];
    let active = r.batch.is_some() || r.jobs.iter().any(|j| j.alive);
    let mut machine = card()
        .key("host-checks")
        .child(heading("Computer readiness", "WINDOWS / WSL 2"));
    if let Some(host) = &r.host {
        machine=machine.child(label(format!("{} · build {} · {}",host.windows,host.build,host.architecture),14.,ink()).wrap())
            .child(badge(if host.eligible{"Windows supports guided WSL setup"}else{"Windows requires manual compatibility review"},host.eligible))
            .child(label(format!("Virtualization: {}",host.virtualization),13.,muted()).wrap())
            .child(label(format!("RAM: {:.1} GiB · Graphics: {}",host.ram_gib,host.gpu),13.,muted()).wrap())
            .child(label(format!("Windows user-drive free: {:.1} GiB. WSL data may be located on another drive.",host.disk_gib),12.,muted()).wrap())
            .child(label(if host.wsl_ready{"A WSL 2 distribution is installed. Select the environment to use below."}else if host.wsl_engine{"WSL is installed. Install/select a Linux distribution below; Windows feature setup may still require attention."}else{"WSL is not detected. Install WSL first, then select a Linux distribution."},13.,muted()).wrap());
    } else {
        machine = machine.child(
            label(
                "The desktop app works before Ubuntu is installed. Scan Windows to begin.",
                14.,
                muted(),
            )
            .wrap(),
        );
    }
    let mut distributions = card()
        .key("distribution-setup")
        .child(heading("Linux environment", "EXISTING OR NEW"));
    if let Some(host) = &r.host {
        for distro in &host.distros {
            let name = distro.name.clone();
            distributions = distributions.child(item(
                format!("installed-distro-{}", distro.name),
                &distro.name,
                &format!("WSL {} · {}", distro.version, distro.state),
                r.distro.text == name,
                move |s| select_distro(s, &name),
            ));
        }
    }
    distributions = distributions
        .child(label("Selected distribution identifier", 12., muted()))
        .child(field(&r.distro, "setup-distro").on_edit(|s: &mut State| &mut s.setup.distro))
        .child(
            row()
                .gap(8.)
                .flex_wrap()
                .child(
                    button(
                        "use-installed-distro",
                        "Use installed environment",
                        true,
                        use_distro,
                    )
                    .disabled(
                        r.pending
                            || active
                            || !r.host.as_ref().is_some_and(|h| {
                                h.distros.iter().any(|d| {
                                    d.name.eq_ignore_ascii_case(r.distro.text.trim())
                                        && d.version == 2
                                })
                            }),
                    ),
                )
                .child(
                    button("initialize-distro", "Initialize Linux user", false, |s| {
                        request(s, "initialize")
                    })
                    .disabled(
                        r.pending
                            || !r.host.as_ref().is_some_and(|h| {
                                h.distros
                                    .iter()
                                    .any(|d| d.name.eq_ignore_ascii_case(r.distro.text.trim()))
                            }),
                    ),
                )
                .child(
                    button(
                        "load-distro-catalog",
                        "Available distributions",
                        false,
                        |s| request(s, "online"),
                    )
                    .disabled(r.pending),
                ),
        );
    let mut online = col().w_full().min_w(0.).gap(6.);
    for (index, distro) in r.online.iter().enumerate() {
        let name = distro.name.clone();
        online = online.child(item(
            format!("online-distro-{index}"),
            &format!("{} · {}", distro.name, distro.label),
            &distro.support,
            r.distro.text == name,
            move |s| select_distro(s, &name),
        ));
    }
    if !r.online.is_empty() {
        distributions = distributions.child(
            col()
                .w_full()
                .min_w(0.)
                .max_h(300.)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(online),
        );
    }
    distributions=distributions.child(row().gap(8.).flex_wrap()
        .child(button("review-wsl","Install WSL",false,|s|{choose(s,0);review(s);} ).on_press_tx(|s,tx|{choose(s,0);review(s);tx.scroll_into_view_in("main-scroll","installation-review");}).disabled(r.pending||active||r.host.as_ref().is_some_and(|h|!h.eligible||h.wsl_ready)))
        .child(button("review-distro","Install selected distribution",false,|s|{choose(s,1);review(s);} ).on_press_tx(|s,tx|{choose(s,1);review(s);tx.scroll_into_view_in("main-scroll","installation-review");}).disabled(r.pending||active)))
        .child(label("Distribution installation may need Windows approval or a restart. First launch may ask you to create a Linux user. Existing distributions are retained.",12.,muted()).wrap());
    let mut tools = card().key("setup-tool-catalog").child(heading(
        "Tools and integration",
        "DISCOVER / INSTALL / MANUAL",
    ));
    let mut choices = col().w_full().min_w(0.).gap(6.);
    for (i, entry) in r.recipes.iter().enumerate().skip(2) {
        choices = choices.child(item(
            format!("setup-tool-{}", entry.id),
            &entry.name,
            &format!(
                "{} · {} · {}",
                entry.version,
                entry.target,
                if entry.automatic {
                    "Managed recipe"
                } else {
                    "Detection + guided setup"
                }
            ),
            r.selected == i,
            move |s| choose(s, i),
        ));
    }
    tools=tools.child(col().w_full().min_w(0.).max_h(300.).scroll(ScrollSpec{stick_bottom:false}).child(choices))
        .child(title(&recipe.name,21.).wrap()).child(label(&recipe.summary,13.,muted()).wrap())
        .child(label("Optional existing location: Linux path for WSL tools; full Windows executable/JAR path for Windows tools",12.,muted()).wrap())
        .child(field(&r.location,"setup-existing-location").on_edit(|s:&mut State|&mut s.setup.location))
        .child(button("browse-existing-tool","Browse for existing tool",false,|s|request(s,"browse")).disabled(r.pending))
        .child(row().gap(8.).flex_wrap()
            .child(button("detect-selected-tool","Detect existing tool",false,|s|request(s,"detect")).disabled(r.pending||recipe.id=="wsl"||recipe.id=="distro"))
            .child(button("review-installation","Review automatic setup",true,review).on_press_tx(|s,tx|{review(s);tx.scroll_into_view_in("main-scroll","installation-review");}).disabled(!recipe.automatic||r.pending||active))
            .child(button("setup-configure-manually","Configure manually",false,manual))
            .child(button("setup-official-docs","Official setup / download",false,|s|s.open(s.setup.recipes[s.setup.selected].docs.clone()))));
    if let Some(candidate) = &r.candidate {
        tools = tools
            .child(
                label(
                    format!(
                        "{} · {} · {}",
                        if candidate.found {
                            "Detected"
                        } else {
                            "Not found"
                        },
                        candidate.version,
                        candidate.reason
                    ),
                    13.,
                    ink(),
                )
                .wrap(),
            )
            .child(mono(&candidate.path, 12., muted()).wrap())
            .child(
                label(
                    if candidate.managed {
                        "App-managed location"
                    } else {
                        "Existing user/system installation; not owned by this app"
                    },
                    12.,
                    muted(),
                )
                .wrap(),
            )
            .child(
                button(
                    "adopt-tool",
                    "Apply detected launch configuration",
                    true,
                    profiles::adopt,
                )
                .disabled(!candidate.compatible || !candidate.found || active),
            );
    }
    if r.review {
        let mut review_card = card()
            .key("installation-review")
            .fill(c(242, 249, 255))
            .child(heading("Review installation", "EXPLICIT INSTALL ACTION"))
            .child(title(&recipe.name, 20.).wrap())
            .child(
                label(
                    format!(
                        "Version: {} · Target: {} · Distribution: {}",
                        recipe.version, recipe.target, r.distro.text
                    ),
                    13.,
                    ink(),
                )
                .wrap(),
            )
            .child(label(&recipe.summary, 13., muted()).wrap());
        review_card = review_card.child(
            label(
                format!(
                    "Destination: {}\nPrivileges: {}\nPackages/components: {}",
                    recipe.destination,
                    recipe.privileges,
                    recipe.packages.join(", ")
                ),
                13.,
                ink(),
            )
            .wrap(),
        );
        if !recipe.verification.is_empty() {
            review_card = review_card.child(label(&recipe.verification, 13., muted()).wrap());
        }
        review_card = review_card.child(button(
            "configure-after-setup",
            if r.configure_after {
                "Configure this tool after success: On"
            } else {
                "Configure after success: Off · apply manually"
            },
            r.configure_after,
            |s| s.setup.configure_after = !s.setup.configure_after,
        ));
        for (index, step) in recipe.steps.iter().enumerate() {
            review_card =
                review_card.child(label(format!("{}. {step}", index + 1), 13., ink()).wrap());
        }
        review_card=review_card.child(label("Review the source and target above. Existing compatible tools are offered for reuse. Shared Linux prerequisites may be installed. Space/download sizes are shown when known; source builds have no reliable fixed estimate. Jobs keep their logs and continue across app closure.",12.,muted()).wrap())
            .child(row().gap(8.).flex_wrap()
                .child(button("confirm-managed-install","Install reviewed tool",true,|s|request(s,"start")).disabled(!recipe.automatic||s.busy()||r.pending))
                .child(button("cancel-install-review","Back",false,|s|s.setup.review=false)));
        tools = tools.child(review_card);
    }
    let mut jobs = card()
        .key("setup-jobs")
        .child(heading("Installation jobs", "PERSISTENT STATUS / LOGS"));
    if r.jobs.is_empty() {
        jobs = jobs.child(
            label(
                "No managed installations started. Detection does not install or update anything.",
                13.,
                muted(),
            )
            .wrap(),
        );
    }
    for (index, job) in r.jobs.iter().enumerate() {
        let id = job.id.clone();
        jobs = jobs.child(item(
            format!("setup-job-{index}"),
            &format!("{} · {}", job.tool, job.status),
            &format!("Step {}/{} · {}", job.step, job.total, job.message),
            r.selected_job.as_ref() == Some(&id),
            move |s| s.setup.selected_job = Some(id.clone()),
        ));
    }
    if let Some(job) = r
        .jobs
        .iter()
        .find(|j| Some(&j.id) == r.selected_job.as_ref())
    {
        jobs=jobs.child(row().gap(8.).flex_wrap()
            .child(button("cancel-setup-job","Cancel after current step",false,|s|request(s,"cancel")).disabled(!job.alive||r.pending))
            .child(button("retry-setup-job","Review retry",false,retry).disabled(job.alive||r.pending))
            .child(button("apply-installed-tool","Apply verified configuration",true,profiles::use_job_result).disabled(job.result.is_none()||job.alive)))
            .child(label("Cancellation lets active package managers/build steps finish. Retry rechecks completed work; installed/shared dependencies are preserved.",12.,muted()).wrap())
            .child(col().w_full().min_w(0.).max_h(260.).padding(12.).radius(6.).fill(code_bg()).scroll(ScrollSpec{stick_bottom:true})
                .child(mono(&job.log_tail,12.,c(245,245,244)).w_full().wrap()));
    }
    let body = if width >= 1000. {
        row()
            .w_full()
            .min_w(0.)
            .gap(16.)
            .items_start()
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(16.)
                    .child(machine)
                    .child(distributions),
            )
            .child(col().grow(1.).min_w(0.).gap(16.).child(tools))
    } else {
        col()
            .w_full()
            .min_w(0.)
            .gap(16.)
            .child(machine)
            .child(distributions)
            .child(tools)
    };
    col()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(
            card()
                .child(heading(
                    "Set up this computer",
                    "WINDOWS DESKTOP / LINUX TOOLS IN WSL",
                ))
                .child(label(&r.message, 14., ink()).wrap())
                .child(
                    button("scan-host", "Scan Windows and WSL", true, |s| {
                        request(s, "scan")
                    })
                    .disabled(r.pending),
                ),
        )
        .child(workflow::view(s, width))
        .child(body)
        .child(jobs)
}
