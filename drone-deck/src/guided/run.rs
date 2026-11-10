//! Chapter 4, First run: three real steps per system. Each step requests a real action and
//! can be confirmed only once evidence of its result is observed.
use super::view::{Frame, frame, next};
use super::*;
use crate::ui::kit::*;
use crate::ui::theme::*;

/// Starts a profile. Returns whether a launch was requested (Starting) or it already runs;
/// only then can a later status of this process count as the step's evidence.
fn start_profile(s: &mut State, id: &str) -> bool {
    let Some(i) = s.config.profiles.iter().position(|p| p.id == id) else {
        s.notice = format!("{id} is not configured. Configure it in Tools & config.");
        return false;
    };
    let name = s.config.profiles[i].name.clone();
    let was = status(s, id);
    s.start(i);
    let now = status(s, id);
    if now == "Starting" && was != "Starting" {
        activity::record(s, format!("Started {name} from guided setup"));
    }
    let live = live(&now);
    if live {
        s.guided.launched = Some(id.to_string());
    }
    live
}

fn live(status: &str) -> bool {
    status == "Starting" || status == "Running"
}

fn status(s: &State, id: &str) -> String {
    s.status
        .get(id)
        .cloned()
        .unwrap_or_else(|| "Not started".into())
}

/// `id` was seen Starting or Running after this step requested it, and now has `wanted`.
fn launched_with(s: &State, id: &str, wanted: &[&str]) -> Option<String> {
    let st = status(s, id);
    (s.guided.launched.as_deref() == Some(id) && wanted.contains(&st.as_str())).then_some(st)
}

/// ROS 2 infrastructure topics every graph has; they are not evidence of a publisher.
fn infrastructure(name: &str) -> bool {
    const TOPICS: [&str; 5] = [
        "/rosout",
        "/rosout_agg",
        "/parameter_events",
        "/clock",
        "/tf_static",
    ];
    TOPICS.contains(&name) || name.split('/').any(|part| part.starts_with('_'))
}

fn app_topics(s: &State) -> Vec<&str> {
    s.recorder
        .discovered
        .iter()
        .map(|t| t.name.as_str())
        .filter(|n| !infrastructure(n))
        .collect()
}

/// The active system's simulated vehicle is up: PX4 SIH running and ready, or Gazebo /
/// ArduPilot running.
fn vehicle_up(s: &State) -> bool {
    match systems::vehicle_profile(s.systems.current()) {
        Some(id) => {
            status(s, id) == "Running"
                && (id != "sih" || s.readiness.get(id).map(String::as_str) == Some("Ready"))
        }
        None => false,
    }
}

/// The vehicle is up and the DDS agent runs.
fn ros_up(s: &State) -> bool {
    vehicle_up(s) && status(s, "dds") == "Running"
}

/// The selected recording, once listed.
fn selected_recording(s: &State) -> Option<&crate::recordings::Session> {
    let id = s.recorder.selected.as_ref()?;
    s.recorder.sessions.iter().find(|r| &r.id == id)
}

/// First run acts only on the active system.
fn on_active(s: &State) -> bool {
    s.guided.existing.as_deref() == Some(s.systems.active_id())
}

fn activate(s: &mut State) {
    let id = ensure_target(s);
    if s.select_system(&id) {
        s.guided.opened_for = id;
        s.guided.prep_error.clear();
    }
}

/// The name of the system first run acts on.
fn target_name(s: &State) -> String {
    target(s).map_or_else(
        || s.guided.name.text.trim().to_string(),
        |t| t.label().to_string(),
    )
}

/// Saves the endpoint entered in first run as the physical system's source detail.
fn remember_endpoint(s: &mut State) {
    let endpoint = s.hardware.endpoint.text.trim().to_string();
    let id = s.systems.active_id().to_string();
    let Some(sys) = s.systems.get_mut(&id) else {
        return;
    };
    if let systems::Source::Physical { endpoint: saved } = &mut sys.source {
        if *saved != endpoint {
            *saved = endpoint;
            if let Err(e) = s.systems.save(s.headless) {
                s.notice = format!("The endpoint could not be saved with the system: {e}");
            }
        }
    }
}

fn project_status(s: &State, wanted: &[&str]) -> Option<(String, String)> {
    s.status
        .iter()
        .filter(|(k, _)| k.starts_with("project-"))
        .find(|(_, v)| wanted.contains(&v.as_str()))
        .map(|(k, v)| (k.trim_start_matches("project-").to_string(), v.clone()))
}

/// The project seen running during First run, once it has exited 0.
fn launched_project(s: &State) -> Option<String> {
    let id = s.guided.launched.as_deref()?;
    let name = id.strip_prefix("project-")?;
    (status(s, id) == "Exited 0").then(|| name.to_string())
}

fn endpoint_ok(s: &State) -> bool {
    let e = s.hardware.endpoint.text.trim();
    !e.is_empty() && e.len() <= 160 && !e.contains(['\n', '\r'])
}

pub(super) fn act(s: &mut State) {
    let (choice, task) = (s.guided.choice, s.guided.task);
    if !on_active(s) {
        s.guided.prep_error = inactive_note(s);
        return;
    }
    s.guided.queries = 0;
    s.guided.last_query = None;
    s.guided.prep_error.clear();
    let requested = match (choice, task) {
        (systems::SIMULATION, 0) => {
            let started = s.bundle(systems::session_stages(s.systems.current()));
            if started {
                activity::record(s, "Requested the simulation workspace from guided setup");
            }
            // A launch already under way counts only when this system started it.
            let ours =
                s.session.is_some() && s.systems.owner("session") == Some(s.systems.active_id());
            started || ours || s.vehicle.owned_by(s.systems.active_id())
        }
        (systems::SIMULATION, 1) => {
            s.guided.launched = None;
            start_profile(s, "qgc")
        }
        (systems::SIMULATION, 2) => {
            s.guided.launched = None;
            let ours = s.vehicle.owned_by(s.systems.active_id());
            if !ours {
                s.notice =
                    "Start the virtual vehicle first; its workspace opens the vehicle link.".into();
            }
            ours
        }
        (systems::ROS, 0) => {
            let vehicle = systems::vehicle_profile(s.systems.current()).unwrap_or("sih");
            if !systems::running(s, vehicle) || !systems::running(s, "dds") {
                s.bundle(systems::session_stages(s.systems.current()));
            }
            s.recorder.discovered.clear();
            let action = if s.recorder.ready { "topics" } else { "check" };
            crate::recordings::send(s, action);
            s.guided.last_query = Some(Instant::now());
            if s.recorder.pending {
                activity::record(s, "Checked recording tools and ROS publishers");
            }
            true
        }
        (systems::ROS, 1) => {
            if s.recorder.topics.text.trim().is_empty() {
                let names = app_topics(s).join(" ");
                s.recorder.topics = TextEdit::new(names);
            }
            crate::recordings::send(s, "record");
            if s.recorder.pending {
                activity::record(s, "Started a ROS recording from guided setup");
            }
            true
        }
        (systems::ROS, _) => {
            let action = if s.recorder.recording { "stop" } else { "list" };
            crate::recordings::send(s, action);
            s.guided.last_query = Some(Instant::now());
            if action == "stop" && s.recorder.pending {
                activity::record(s, "Stopped the ROS recording to finalize it");
            }
            true
        }
        (systems::AI, 2) => {
            s.ui.data_tab = 2;
            s.navigate(ui::Page::Data);
            crate::studio::pipelines::refresh(s);
            s.notice =
                "Data & models → Pipelines. Return with Guided setup ↗ to confirm the step.".into();
            true
        }
        (systems::AI, _) => {
            s.ui.data_tab = 3;
            s.navigate(ui::Page::Data);
            s.notice =
                "Data & models → Projects. Return with Guided setup ↗ to confirm the step.".into();
            true
        }
        (_, 0) => {
            if !endpoint_ok(s) {
                s.guided.prep_error = "Enter a serial, UDP or TCP endpoint first.".into();
                return;
            }
            remember_endpoint(s);
            true
        }
        (_, 1) => {
            if !crate::hardware::connected(s) {
                s.hardware.mode = 1;
                crate::hardware::start(s);
            }
            let started = crate::hardware::connected(s);
            if !started {
                s.guided.prep_error = s.hardware.message.clone();
            }
            started
        }
        _ => {
            if s.vehicle.tlog().is_some() {
                s.open(crate::vehicle::tlog_dir().display().to_string());
            }
            true
        }
    };
    s.guided.awaiting = requested;
    if !requested && s.guided.prep_error.is_empty() {
        s.guided.prep_error = format!("Nothing was started. {}", s.notice);
    }
}

fn inactive_note(s: &State) -> String {
    format!(
        "{} is not the active system. First run acts only on the active system; make it active to try these steps.",
        target_name(s)
    )
}

/// Real evidence for the current step, if it has been observed.
pub(super) fn evidence(s: &State) -> Option<String> {
    let (choice, task) = (s.guided.choice, s.guided.task);
    match (choice, task) {
        (systems::SIMULATION, 0) => {
            if systems::session_stages(s.systems.current()).contains(&"link") {
                let snap = s.vehicle.shown_for(s.systems.active_id())?;
                let v = snap.vehicle.as_ref().filter(|_| snap.fresh)?;
                Some(format!(
                    "Vehicle link up: fresh {} heartbeat, mode {}",
                    v.autopilot.name(),
                    v.mode
                ))
            } else {
                let id = systems::vehicle_profile(s.systems.current())?;
                vehicle_up(s).then(|| format!("{id} {}", systems::profile_state(s, id).0))
            }
        }
        (systems::SIMULATION, 1) => {
            let st = launched_with(s, "qgc", &["Running", "Exited 0"])?;
            s.guided.awaiting.then(|| {
                format!("QGroundControl launched ({st}); vehicle confirmed by you in its window")
            })
        }
        (systems::SIMULATION, _) => {
            if !s.guided.awaiting {
                return None;
            }
            let snap = s.vehicle.shown_for(s.systems.active_id())?;
            let v = snap.vehicle.as_ref().filter(|_| snap.fresh)?;
            Some(format!(
                "Live vehicle: mode {}, altitude {}, battery {}",
                v.mode,
                v.relative_alt
                    .map_or("not reported".into(), |a| format!("{a:.1} m")),
                v.battery_voltage
                    .map_or("not reported".into(), |b| format!("{b:.1} V")),
            ))
        }
        (systems::ROS, 0) => {
            let topics = app_topics(s);
            (ros_up(s) && !topics.is_empty()).then(|| {
                format!(
                    "PX4 and the DDS agent running; {} ROS topics published ({}{})",
                    topics.len(),
                    topics[..topics.len().min(3)].join(", "),
                    if topics.len() > 3 { ", …" } else { "" }
                )
            })
        }
        (systems::ROS, 1) => s
            .recorder
            .recording
            .then(|| format!("Recording topics: {}", s.recorder.topics.text.trim())),
        (systems::ROS, _) => {
            if s.recorder.recording {
                return None;
            }
            let rec =
                selected_recording(s).filter(|r| r.status == "Finalized" && r.messages > 0)?;
            Some(format!(
                "Recording {}: {} messages, {}",
                if rec.label.is_empty() {
                    &rec.id
                } else {
                    &rec.label
                },
                rec.messages,
                rec.status
            ))
        }
        (systems::AI, 0) => {
            let (m, i) = (s.studio.model_file.text.trim(), s.studio.inputs.text.trim());
            (!m.is_empty() && !i.is_empty()).then(|| format!("Model {m} with inputs {i}"))
        }
        (systems::AI, 1) => {
            launched_project(s).map(|name| format!("Project {name} finished (exit 0)"))
        }
        (systems::AI, _) => {
            if !s.guided.awaiting || s.studio.pipes.job.is_some() {
                return None;
            }
            let mut completed = s
                .studio
                .pipes
                .inference_runs()
                .filter(|r| r.status == "Completed");
            let latest = completed.next()?;
            let detections = latest.detections.map_or_else(
                || "no detection count recorded".to_string(),
                |n| format!("{n} detection message(s) recorded"),
            );
            Some(format!(
                "{} completed inference run(s) listed; latest {} on {}: {detections}",
                completed.count() + 1,
                latest.name,
                latest.recording
            ))
        }
        (_, 0) => (s.guided.awaiting && endpoint_ok(s))
            .then(|| format!("Endpoint {}", s.hardware.endpoint.text.trim())),
        (_, 1) => s
            .vehicle
            .shown_for(s.systems.active_id())
            .filter(|snap| snap.fresh)
            .and_then(|snap| snap.vehicle.as_ref())
            .map(|v| {
                format!(
                    "Fresh heartbeat from vehicle {}:{}",
                    v.system_id, v.component_id
                )
            }),
        _ => s
            .vehicle
            .tlog()
            .map(|p| format!("Telemetry log {}", p.display())),
    }
}

/// What the app currently observes for the step, shown while waiting.
pub(super) fn observed(s: &State) -> String {
    let (choice, task) = (s.guided.choice, s.guided.task);
    match (choice, task) {
        (systems::SIMULATION, 0) => systems::session_stages(s.systems.current())
            .iter()
            .map(|id| {
                let label = systems::component(id)
                    .map(|c| systems::state(s, c).0)
                    .unwrap_or_else(|| systems::profile_state(s, id).0);
                format!("{id} {label}")
            })
            .collect::<Vec<_>>()
            .join(" / "),
        (systems::SIMULATION, 1) => format!("QGroundControl: {}", status(s, "qgc")),
        (systems::SIMULATION, _) => systems::component("link")
            .map(|c| format!("Vehicle link: {}", systems::state(s, c).0))
            .unwrap_or_default(),
        (systems::ROS, 0) => format!(
            "Vehicle {} ({}) / DDS agent {} / {} application topics / {}",
            status(
                s,
                systems::vehicle_profile(s.systems.current()).unwrap_or("sih")
            ),
            if vehicle_up(s) { "ready" } else { "not ready" },
            status(s, "dds"),
            app_topics(s).len(),
            s.recorder.message
        ),
        (systems::ROS, 2) if !s.recorder.recording => match selected_recording(s) {
            Some(r) => format!("Recording status: {}, {} messages", r.status, r.messages),
            None => s.recorder.message.clone(),
        },
        (systems::ROS, _) => s.recorder.message.clone(),
        (systems::AI, 2) => {
            let state = systems::component("pipelines")
                .map(|c| systems::state(s, c).0)
                .unwrap_or_default();
            format!("Pipelines: {state} / {}", s.studio.pipes.message)
        }
        (systems::AI, _) => match project_status(s, &["Running", "Starting"]) {
            Some((name, st)) => format!("Project {name}: {st}"),
            None => s.studio.message.clone(),
        },
        (_, 1) => systems::component("link")
            .map(|c| systems::state(s, c).0)
            .unwrap_or_default(),
        (_, 2) if s.vehicle.tlog().is_none() => {
            "No telemetry log yet; connecting creates it.".into()
        }
        _ => s.hardware.message.clone(),
    }
}

/// Follow-up requests that complete a step without another click.
/// How often a waiting ROS step asks again, and how many answers it waits for once the
/// step's sources are up (then "Run again" asks anew).
const REQUERY: Duration = Duration::from_secs(3);
const REQUERIES: u8 = 5;

/// Follow-up requests while a step waits: notes the project a run started, re-discovers ROS
/// topics until PX4 and the DDS agent publish, and re-lists recordings until one finalizes.
pub(super) fn poll(s: &mut State) -> bool {
    let g = &s.guided;
    if g.chapter != 3 || !g.awaiting || g.done || !on_active(s) {
        return false;
    }
    if g.choice == systems::AI {
        let Some((name, _)) = project_status(s, &["Starting", "Running"]) else {
            return false;
        };
        let id = format!("project-{name}");
        if g.launched.as_deref() == Some(id.as_str()) {
            return false;
        }
        s.guided.launched = Some(id);
        return true;
    }
    if s.recorder.pending
        || g.queries >= REQUERIES
        || g.last_query.is_some_and(|t| t.elapsed() < REQUERY)
        || evidence(s).is_some()
    {
        return false;
    }
    let next = match (g.choice, g.task) {
        (systems::ROS, 0) if s.recorder.ready => "topics",
        (systems::ROS, 2) if !s.recorder.recording => "list",
        _ => return false,
    };
    if g.task != 0 || ros_up(s) {
        s.guided.queries += 1;
    }
    s.guided.last_query = Some(Instant::now());
    crate::recordings::send(s, next);
    true
}

fn confirm(s: &mut State) {
    let Some(found) = evidence(s).filter(|_| on_active(s)) else {
        return;
    };
    let title = STORIES[s.guided.choice].tasks[s.guided.task].0;
    s.guided.evidence.push(format!("{title}: {found}"));
    activity::record(s, format!("Observed in first run: {found}"));
    s.guided.awaiting = false;
    s.guided.queries = 0;
    s.guided.last_query = None;
    if s.guided.task >= 2 {
        s.guided.done = true;
    } else {
        s.guided.task += 1;
    }
}

fn task_row(s: &State, f: &Frame, i: usize) -> El {
    let g = &s.guided;
    let (title, what, note) = STORIES[g.choice].tasks[i];
    let lead = if i < g.task {
        "Done".to_string()
    } else {
        format!("0{}", i + 1)
    };
    let mut main = col().grow(1.).min_w(0.).gap(10.).child(h3(title, 20.));
    if i == g.task {
        main = main
            .child(p(what, 12.).max_w(760.))
            .child(p(note, 12.).max_w(760.));
        if g.choice == systems::PHYSICAL && i == 0 {
            main = main.child(
                labeled(
                    "Vehicle endpoint",
                    field(&s.hardware.endpoint, "run-endpoint", "serial:COM4:115200")
                        .on_edit(|s: &mut State| &mut s.hardware.endpoint),
                )
                .max_w(440.),
            );
        }
        if s.headless {
            main = main.child(notice(
                "Headless preview: this step's actions are disabled, so no evidence can be collected here.",
            ));
        } else if !on_active(s) {
            main = main
                .child(notice(inactive_note(s)).key("run-inactive"))
                .child(row().child(ghost(
                    "run-activate",
                    &format!("Make {} active", target_name(s)),
                    true,
                    activate,
                )));
        } else if g.awaiting {
            let line = match evidence(s) {
                Some(e) => body(format!("Observed: {e}"), 11., success()),
                None => body(
                    format!("Waiting for evidence. Now: {}", observed(s)),
                    11.,
                    muted(),
                ),
            };
            main = main.child(line.key("task-status").role("status").w_full());
        }
        let mut links = row().gaps(4., 16.).center_y().flex_wrap();
        if g.awaiting && !s.headless {
            links = links.child(ghost("task-retry", "Run again", on_active(s), act));
        }
        match g.choice {
            systems::AI if i == 2 => {
                links = links.child(ghost("task-pipelines", "Pipelines ↗", true, |s| {
                    s.ui.data_tab = 2;
                    s.navigate(ui::Page::Data)
                }))
            }
            systems::AI => {
                links = links.child(ghost("task-projects", "Projects ↗", true, |s| {
                    s.ui.data_tab = 3;
                    s.navigate(ui::Page::Data)
                }))
            }
            systems::ROS if i == 1 => {
                links = links.child(ghost("task-topics", "Choose topics ↗", true, |s| {
                    s.ui.data_tab = 0;
                    s.navigate(ui::Page::Data)
                }))
            }
            _ => {}
        }
        main = main.child(links);
    } else {
        main = main.child(p(
            if i < g.task {
                "Observation confirmed."
            } else {
                "Available after the previous observation."
            },
            12.,
        ));
    }
    ruled(true)
        .key(format!("run-task-{i}"))
        .py(25.)
        .gap(if f.narrow() { 12. } else { 20. })
        .items_start()
        .child(meta(lead).w(if f.narrow() { 24. } else { 36. }).pt(6.))
        .child(main)
}

fn receipt(s: &State) -> El {
    let g = &s.guided;
    let item = |label: &str, value: El| {
        col()
            .w_full()
            .min_w(0.)
            .gap(6.)
            .pb(18.)
            .child(meta(label))
            .child(value)
    };
    let mut evidence = col().w_full().min_w(0.).gap(6.);
    for e in &g.evidence {
        evidence = evidence.child(body(e, 12., fg()));
    }
    col()
        .key("system-receipt")
        .w_full()
        .min_w(0.)
        .py(24.)
        .child(item("System", body(g.name.text.trim(), 12., fg())))
        .child(item(
            "What you walked through",
            body(STORIES[g.choice].result, 12., fg()),
        ))
        .child(item("Evidence", evidence))
        .child(item(
            "Continue in the system workspace",
            body(
                "Run, configure and record from the workspace. The configuration and files stay together.",
                12.,
                fg(),
            ),
        ))
}

pub(super) fn view(s: &State, f: &Frame) -> El {
    let g = &s.guided;
    let story = &STORIES[g.choice];
    let content = if g.done {
        receipt(s)
    } else {
        let head = if f.narrow() {
            col().gap(8.)
        } else {
            row().gap(24.).center_y().justify_between()
        };
        let mut list = col().w_full().min_w(0.).child(
            head.w_full()
                .min_w(0.)
                .mt(4.)
                .mb(24.)
                .child(meta(format!("Step {} of 3", g.task + 1)))
                .child(p("Do one thing, observe the result, then continue.", 12.)),
        );
        for i in 0..3 {
            list = list.child(task_row(s, f, i));
        }
        list.child(
            body(g.prep_error.clone(), 11., danger())
                .key("run-error")
                .role("status")
                .w_full(),
        )
        .child(row().mt(16.).child(small_link(
            "skip-first-run",
            "Skip for now",
            open_workspace,
        )))
    };
    let note = if g.done {
        "No result metrics are claimed."
    } else if s.headless {
        "Headless preview / no process starts here."
    } else if g.choice == systems::SIMULATION {
        "Flight actions reach only this simulation / missions stay in QGroundControl."
    } else {
        "Each confirmation needs observed output."
    };
    let action = if g.done {
        next("Open system workspace →", true, open_workspace)
    } else if g.awaiting {
        next(
            "Confirm observation →",
            on_active(s) && evidence(s).is_some(),
            confirm,
        )
    } else {
        next("Try this step", !s.headless && on_active(s), act)
    };
    frame(
        s,
        f,
        "04 / Learn by doing",
        if g.done {
            "Your first system, documented."
        } else {
            "A first result. One step at a time."
        },
        &format!("{} / {}", g.name.text.trim(), story.name),
        content,
        note,
        action,
    )
}
