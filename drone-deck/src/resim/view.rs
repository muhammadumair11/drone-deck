//! Data & models → Re-simulation: re-simulable flights, the run's settings, a running run and
//! earlier runs.
use super::run::{self, Phase};
use super::{DOMAIN, Flight, VEHICLE_DOMAIN};
use crate::State;
use crate::ui::theme::Viewport;
use crate::ui::{Page, kit::*, theme::*};
use rune::{El, col, row};

fn summary(f: &Flight) -> String {
    let origin = if f.recipe["origin"] == "real" {
        "Real flight / "
    } else {
        ""
    };
    let world = f.adapter().map_or_else(
        || {
            f.recipe["adapter"]
                .as_str()
                .unwrap_or("unknown")
                .to_string()
        },
        |a| a.name.to_string(),
    );
    let commands = f.recipe["commands"].as_u64().unwrap_or(0);
    let mut line = format!(
        "{origin}{world} / {commands} command topic{}",
        if commands == 1 { "" } else { "s" }
    );
    if !f.recipe["params"].as_bool().unwrap_or(false) {
        line.push_str(" / parameters not kept");
    }
    line
}

fn flights(s: &State, vp: &Viewport) -> El {
    let r = &s.resim;
    let mut list = col().key("resim-flights").w_full().min_w(0.);
    for f in &r.flights {
        let selected = r.selected.as_deref() == Some(f.id.as_str());
        let id = f.id.clone();
        let main = col()
            .w_full()
            .min_w(0.)
            .gap(4.)
            .child(h3(f.label.clone(), 15.))
            .child(body(summary(f), 11., muted()).wrap());
        let runs = f.runs.len();
        let lead = meta_sized(
            if runs == 0 {
                "Not run".to_string()
            } else {
                format!("{runs} run{}", if runs == 1 { "" } else { "s" })
            },
            10.,
            if runs == 0 { muted() } else { fg() },
        )
        .wrap();
        let pull = meta_sized(
            if selected { "Selected" } else { "Select" },
            10.,
            if selected { fg() } else { muted() },
        );
        list = list.child(
            log_row(
                format!("resim-flight-{}", f.id),
                lead,
                main,
                pull,
                vp.compact(),
            )
            .role("button")
            .label(f.label.clone())
            .focus_selected(selected)
            .focusable(true)
            .cursor(rune::Cursor::Pointer)
            .fill(if selected {
                soft()
            } else {
                rune::Color::TRANSPARENT
            })
            .hover(soft())
            .focus_ring(2., 0., fg())
            .on_press(move |s: &mut State| super::select(s, &id)),
        );
    }
    if r.flights.is_empty() {
        let (title, text) = if !s.wsl_ready() {
            (
                "Linux environment not ready",
                "Flights are recorded inside the selected Ubuntu. Select a working WSL 2 distribution in Settings first.",
            )
        } else if r.listed {
            (
                "No re-simulable flights yet",
                "Record a flight of a simulated system with ROS 2 in Data & models → Recordings. Its recording keeps a recipe to fly it again.",
            )
        } else {
            (
                "Flights not loaded",
                "Reload to list the recordings that keep a recipe.",
            )
        };
        list = list.child(empty("resim-empty", title, text));
    }
    col()
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(section_head(
            "Re-simulable flights",
            row()
                .gaps(4., 16.)
                .flex_wrap()
                .center_y()
                .child(meta(if r.importing {
                    "Importing…"
                } else {
                    "Import a real flight"
                }))
                .child(browse(
                    "resim-import",
                    "Import a PX4 or ArduPilot flight log or the drone's ROS 2 recording",
                    &[
                        ("PX4 flight log", &["ulg"]),
                        ("ArduPilot flight log", &["bin"]),
                        ("ROS 2 recording", &["mcap"]),
                    ],
                    !r.importing && s.wsl_ready(),
                    super::import::start,
                ))
                .child(ghost(
                    "resim-reload",
                    "Reload",
                    !r.listing && s.wsl_ready(),
                    crate::resim::refresh,
                )),
            vp.compact(),
        ))
        .child(list)
}

fn setup(s: &State, vp: &Viewport) -> El {
    let r = &s.resim;
    let Some(f) = r.flight() else {
        return col();
    };
    let refusal = run::refusal(s);
    let mut left = col()
        .key("resim-setup")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(h2(f.label.clone(), 22.))
        .child(p(
            "Starts a fresh autopilot and simulator, gives it the flight's parameters and replays the recorded ROS 2 commands at their original times. Nothing here reaches a real vehicle.",
            12.,
        ))
        .child(simulator(s, f))
        .child(code_choice(s))
        .child(tolerance(s))
        .child(labeled(
            "AI topics to compare (optional)",
            field(&s.resim.ai_topics, "resim-ai-topics", "/drone_deck/detections")
                .on_edit(|s: &mut State| &mut s.resim.ai_topics),
        ));
    let starting = r.run.is_none();
    left = left.child(
        row()
            .gaps(8., 20.)
            .flex_wrap()
            .center_y()
            .child(if starting {
                pill("resim-start", "Re-simulate", refusal.is_none(), run::start)
            } else {
                pill("resim-stop", "Stop re-simulation", true, run::stop)
            }),
    );
    if let (true, Some(why)) = (starting, refusal) {
        left = left.child(body(why, 11., warn()).wrap());
    }
    let right = col()
        .w_full()
        .min_w(0.)
        .gap(10.)
        .child(meta("Isolation"))
        .child(p(isolation(f), 11.))
        .child(meta("Saved with the recording"))
        .child(p(
            "resim/<run>/: run.json (settings, parameters left out, exit states), commands.jsonl (each command replayed) and fresh/ (an MCAP of the fresh vehicle's reports).",
            11.,
        ));
    asset(left, right, vp.compact())
}

/// How the fresh vehicle is kept apart from your code, by autopilot.
fn isolation(f: &Flight) -> String {
    if f.recipe["autopilot"] == "ardupilot" {
        format!(
            "The fresh ArduPilot runs on its own ROS domain {VEHICLE_DOMAIN}, kept on this computer, and Drone Deck's command player passes its /ap/* reports on to domain {DOMAIN}, where your code runs. Arm, mode and takeoff are services a recording cannot keep: the player calls them where the recorded /ap/status changed."
        )
    } else {
        format!(
            "The fresh PX4 publishes under /resim on ROS domain {DOMAIN}, kept on this computer. Only Drone Deck's command player writes to it; it passes the fresh vehicle's reports on to /fmu/out/*."
        )
    }
}

/// The world to fly in (the recorded one, or another with the same autopilot) and the
/// conditions it can change.
fn simulator(s: &State, f: &Flight) -> El {
    let r = &s.resim;
    let target = r
        .target
        .clone()
        .or_else(|| f.adapter().map(|a| a.id.to_string()));
    let mut c = col()
        .key("resim-simulator")
        .w_full()
        .min_w(0.)
        .gap(4.)
        .child(meta("Simulator"));
    let recorded = f.adapter().map(|a| a.id);
    let mut chosen = None;
    for (a, ok) in super::targets(f) {
        let selected = target.as_deref() == Some(a.id);
        if selected {
            chosen = Some(a);
        }
        let mut title = a.name.to_string();
        if Some(a.id) == recorded {
            title.push_str(" (as recorded)");
        }
        let detail = match &ok {
            Err(why) => why.clone(),
            Ok(()) if Some(a.id) == recorded => {
                "The world the flight was recorded in, with its details.".into()
            }
            Ok(()) => {
                "Different physics: expect differences. The world's own parameters are not copied."
                    .into()
            }
        };
        let id = a.id;
        let is_recorded = Some(a.id) == recorded;
        let line = row()
            .key(format!("resim-target-{id}"))
            .role("radio")
            .label(title.clone())
            .checked(selected)
            .w_full()
            .min_w(0.)
            .gap(12.)
            .py(6.)
            .items_start()
            .child(dot(if selected { fg() } else { rule() }))
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(2.)
                    .child(body(title, 12., if ok.is_ok() { fg() } else { muted() }))
                    .child(body(detail, 10., muted()).wrap()),
            );
        c = c.child(if ok.is_ok() {
            line.focusable(true)
                .cursor(rune::Cursor::Pointer)
                .hover(soft())
                .focus_ring(2., 0., fg())
                .on_press(move |s: &mut State| {
                    s.resim.target = (!is_recorded).then(|| id.to_string());
                    s.resim.conditions.clear();
                })
        } else {
            line
        });
    }
    let Some(a) = chosen else {
        return c;
    };
    c = c.child(meta("Conditions").pt(8.));
    for (key, label, offered) in super::conditions(a) {
        let on = r.conditions.iter().any(|k| k == key);
        c = c.child(match offered {
            Ok(desc) => col()
                .w_full()
                .min_w(0.)
                .child(checkbox(
                    &format!("resim-condition-{key}"),
                    label,
                    on,
                    move |s| {
                        if let Some(i) = s.resim.conditions.iter().position(|k| k == key) {
                            s.resim.conditions.remove(i);
                        } else {
                            s.resim.conditions.push(key.to_string());
                        }
                    },
                ))
                .child(body(desc, 10., muted()).wrap()),
            Err(why) => row()
                .key(format!("resim-condition-{key}"))
                .w_full()
                .min_w(0.)
                .gap(12.)
                .py(6.)
                .child(body(format!("{label}: not offered"), 11., muted()))
                .child(body(why, 10., muted()).wrap().grow(1.)),
        });
    }
    let home = if a.home.is_some() {
        "Starts at the recorded home position when the recording kept one."
    } else {
        "This world does not take a home position: the vehicle starts where the world puts it."
    };
    c.child(body(home, 10., muted()).wrap())
}

/// Your code: none, or one of the distribution's ROS 2 projects, started on the run's domain.
fn code_choice(s: &State) -> El {
    let r = &s.resim;
    let projects = super::projects(s);
    let mut names = vec!["None".to_string()];
    names.extend(
        projects
            .iter()
            .filter_map(|p| p["name"].as_str().map(str::to_string)),
    );
    let selected = r
        .code
        .as_ref()
        .and_then(|c| names.iter().position(|n| n == c))
        .unwrap_or(0);
    let items: Vec<&str> = names.iter().map(String::as_str).collect();
    let chosen = names.clone();
    let mut c = col()
        .key("resim-code")
        .w_full()
        .min_w(0.)
        .gap(6.)
        .child(meta("Your code"))
        .child(tabs(
            "resim-code",
            &items,
            selected,
            move |s: &mut State, i: usize| {
                s.resim.code = (i > 0).then(|| chosen[i].clone());
            },
        ));
    let hint = if !s.studio.library["projects"].is_array() {
        "Loading your projects…"
    } else if projects.is_empty() {
        "No ROS 2 projects yet. Create one in Data & models → Projects; it runs here on the run's own ROS domain, reading the fresh vehicle on /fmu/out/* as usual."
    } else if r.code.is_none() {
        "None: a pure replay of the recording."
    } else if !r.takeover {
        "Your code runs alongside but only watches: the recording steers the whole flight."
    } else {
        "Your code runs in shadow; it steers once it differs from the recording beyond the tolerance."
    };
    c = c.child(body(hint, 11., muted()).wrap());
    c
}

/// When your code takes over: off, or a tolerance, floor and hold time.
fn tolerance(s: &State) -> El {
    let r = &s.resim;
    let mut c = col()
        .key("resim-tolerance")
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(meta("Takeover"))
        .child(tabs(
            "resim-takeover",
            &["Off", "Tolerance"],
            usize::from(r.takeover),
            |s: &mut State, i: usize| s.resim.takeover = i == 1,
        ));
    if r.takeover {
        c = c
            .child(
                row()
                    .gaps(8., 16.)
                    .flex_wrap()
                    .child(labeled(
                        "Tolerance (%)",
                        field(&r.tolerance, "resim-tolerance-percent", super::DEFAULT_TOLERANCE)
                            .on_edit(|s: &mut State| &mut s.resim.tolerance),
                    ))
                    .child(labeled(
                        "Floor (field units)",
                        field(&r.floor, "resim-floor", "0.2").on_edit(|s: &mut State| &mut s.resim.floor),
                    ))
                    .child(labeled(
                        "Hold (s)",
                        field(&r.hold, "resim-hold", "0.5").on_edit(|s: &mut State| &mut s.resim.hold),
                    )),
            )
            .child(body(
                "A command differs when |recorded − yours| is more than the tolerance of the recorded value and more than the floor. Once a difference lasts the hold time your code steers for the rest of the run; the recording never steers again. 0% hands over at the first command.",
                11.,
                muted(),
            ).wrap());
    } else {
        c = c.child(body("Off: the recording steers the whole flight.", 11., muted()).wrap());
    }
    c
}

fn live(s: &State) -> El {
    let Some(active) = &s.resim.run else {
        return col();
    };
    let steps = [
        Phase::Preparing,
        Phase::Launching,
        Phase::ReadingParams,
        Phase::WritingParams,
        Phase::StartingCode,
        Phase::Replaying,
        Phase::Stopping,
    ];
    let at = steps.iter().position(|p| *p == active.phase).unwrap_or(0);
    let mut detail = format!(
        "{} / {} s",
        active.phase.label(),
        active.since.elapsed().as_secs()
    );
    if !active.progress.is_empty() {
        detail = format!("{detail} / {}", active.progress);
    }
    let mut c = col()
        .key("resim-live")
        .role("status")
        .label(detail.clone())
        .w_full()
        .min_w(0.)
        .gap(6.)
        .py(12.)
        .child(meta(format!(
            "Re-simulating {} · step {} of {}",
            active.label,
            at + 1,
            steps.len()
        )))
        .child(body(detail, 12., fg()).wrap());
    if active.phase == Phase::Replaying {
        let line = if active.progress.starts_with("Your code steering") {
            "Your code steers the vehicle. The recording no longer does."
        } else {
            "The recording steers the vehicle."
        };
        c = c.child(body(line, 11., muted()).key("resim-steering"));
    }
    if matches!(
        active.phase,
        Phase::Launching | Phase::ReadingParams | Phase::WritingParams | Phase::Replaying
    ) {
        c = c.child(ghost("resim-system", "Watch it on System ↗", true, |s| {
            s.navigate(Page::System)
        }));
    }
    c
}

fn runs(s: &State, vp: &Viewport) -> El {
    let Some(f) = s.resim.flight() else {
        return col();
    };
    let mut list = col().key("resim-runs").w_full().min_w(0.);
    for run in &f.runs {
        let tone = match run.status.as_str() {
            "Completed" => success(),
            "Failed" | "Interrupted" => danger(),
            _ => muted(),
        };
        let mut main = col()
            .w_full()
            .min_w(0.)
            .gap(4.)
            .child(h3(run.created.clone(), 13.))
            .child(body(
                format!("{} of {} commands replayed", run.sent, run.commands),
                11.,
                fg(),
            ))
            .child(
                code(run.path.clone(), 11., muted())
                    .wrap_anywhere()
                    .selectable(true),
            );
        let flown = crate::systems::adapter::ALL
            .iter()
            .find(|a| a.id == run.target)
            .map_or(run.target.as_str(), |a| a.name);
        if !flown.is_empty() {
            let mut line = format!("Flown in {flown}");
            if Some(run.target.as_str()) != f.adapter().map(|a| a.id) {
                line.push_str(" (not the recorded simulator)");
            }
            if !run.conditions.is_empty() {
                line = format!("{line} · {}", run.conditions.join(", "));
            }
            main = main.child(body(line, 11., muted()).wrap());
        }
        if !run.code.is_empty() {
            let line = if run.takeover.is_empty() {
                format!(
                    "Your code {} ran alongside; the recording steered throughout.",
                    run.code
                )
            } else {
                format!("Your code {} took over at {}", run.code, run.takeover)
            };
            main = main.child(body(line, 11., fg()).wrap());
        }
        if !run.error.is_empty() {
            main = main.child(body(run.error.clone(), 11., danger()).wrap());
        }
        let verdict = match (run.verdict.as_str(), run.split_t) {
            ("same", _) => Some(("Same course".to_string(), success())),
            ("changed", Some(t)) => Some((format!("Changed at t = {t:.1} s"), warn())),
            ("changed", None) => Some(("Changed".to_string(), warn())),
            ("no data", _) => Some((
                "No verdict: the flights could not be put side by side".to_string(),
                muted(),
            )),
            _ if !run.compare_error.is_empty() => {
                Some((format!("Not compared: {}", run.compare_error), muted()))
            }
            _ => None,
        };
        if let Some((text, color)) = verdict {
            main = main.child(
                body(text, 11., color)
                    .key(format!("resim-run-verdict-{}", run.name))
                    .wrap(),
            );
        }
        let name = run.name.clone();
        let shown = s.resim.report.as_ref().is_some_and(|(n, _)| *n == run.name);
        list = list.child(log_row(
            format!("resim-run-{}", run.name),
            meta_sized(run.status.clone(), 10., tone).wrap(),
            main,
            ghost(
                format!("resim-run-compare-{}", run.name),
                if shown {
                    "Shown below"
                } else {
                    "Comparison ↓"
                },
                !shown && run.status != "Running",
                move |s| super::open_report(s, &name),
            ),
            vp.compact(),
        ));
    }
    if f.runs.is_empty() {
        list = list.child(empty(
            "resim-runs-empty",
            "No runs yet",
            "Re-simulate the flight to compare it with a fresh one.",
        ));
    }
    col()
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(section_head(
            "Runs of this flight",
            meta("Newest first"),
            vp.compact(),
        ))
        .child(list)
}

pub(crate) fn tab(s: &State, vp: &Viewport) -> El {
    let r = &s.resim;
    let mut page = col().key("resim-tab").w_full().min_w(0.);
    if !r.message.is_empty() {
        page = page.child(
            row()
                .key("resim-status")
                .role("status")
                .label(r.message.clone())
                .w_full()
                .min_w(0.)
                .gap(14.)
                .py(10.)
                .items_start()
                .child(meta_sized("Status", 9., muted()).w(64.).pt(4.))
                .child(body(r.message.clone(), 11., fg()).grow(1.)),
        );
    }
    page.child(live(s))
        .child(setup(s, vp))
        .child(runs(s, vp))
        .child(super::report::view(s, vp))
        .child(flights(s, vp))
}
