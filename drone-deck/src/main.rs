#![windows_subsystem = "windows"]
mod activity;
mod agent;
mod backend;
mod diagnostics;
mod guided;
mod hardware;
mod mavlink;
mod process_output;
mod profile_editor;
mod qgc_launch;
mod recordings;
mod session;
mod setup;
mod store;
mod studio;
mod systems;
mod ui;
mod ulog;
mod vehicle;
mod window;
mod winjob;
use backend::{Backend, Config, Event, Request};
use rune::{AppSpec, Color, Cx, El, ScrollSpec, TextEdit, Tx, col, el, row, spacer};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use ui::{Page, view};

struct State {
    hardware: hardware::Hardware,
    studio: studio::Studio,
    setup: setup::Setup,
    recorder: recordings::Recorder,
    ui: ui::UiState,
    systems: systems::Systems,
    activity: activity::Activity,
    guided: guided::Guided,
    config: Config,
    backend: Option<Backend>,
    page: Page,
    selected: usize,
    status: HashMap<String, String>,
    detected: HashMap<String, bool>,
    logs: VecDeque<String>,
    notice: String,
    editor: TextEdit,
    editing: bool,
    form: Option<profile_editor::Form>,
    headless: bool,
    confirm: Option<String>,
    readiness: HashMap<String, String>,
    session: Option<session::Launch>,
    qgc: qgc_launch::Watch,
    flight_logs: ulog::FlightLogs,
    agent: agent::AgentState,
    vehicle: vehicle::Vehicle,
}
impl State {
    fn new() -> Self {
        let headless = cfg!(test)
            || std::env::args().any(|a| {
                [
                    "--snap",
                    "--tree",
                    "--script",
                    "--list-scenes",
                    "--frames",
                    "--trace",
                ]
                .contains(&a.as_str())
            });
        let (config, notice) = if headless {
            (
                backend::defaults(),
                "Headless preview · external processes disabled".into(),
            )
        } else {
            backend::prepare_root();
            match backend::load() {
                Ok(c) => (
                    c,
                    "Ready. Detect your tools or launch the SIH workspace.".into(),
                ),
                Err(e) => (
                    backend::defaults(),
                    format!(
                        "Configuration error: {e}. Defaults loaded; save explicitly to replace."
                    ),
                ),
            }
        };
        let mut state = Self {
            hardware: hardware::Hardware::default(),
            studio: studio::Studio::default(),
            setup: setup::Setup::default(),
            recorder: recordings::Recorder::default(),
            ui: ui::UiState::default(),
            systems: systems::Systems::load(headless),
            activity: activity::Activity::load(headless),
            guided: guided::Guided::default(),
            config,
            backend: None,
            page: Page::System,
            selected: 0,
            status: HashMap::new(),
            detected: HashMap::new(),
            logs: VecDeque::new(),
            notice,
            editor: TextEdit::new(""),
            editing: false,
            form: None,
            headless,
            confirm: None,
            readiness: HashMap::new(),
            session: None,
            qgc: Default::default(),
            flight_logs: Default::default(),
            agent: Default::default(),
            vehicle: Default::default(),
        };
        ui::restore_preferences(&mut state);
        let notices = if headless {
            Vec::new()
        } else {
            store::take_notices()
        };
        if !notices.is_empty() {
            state.notice = format!("{} {}", notices.join(" "), state.notice);
            for notice in notices {
                activity::record(&mut state, notice);
            }
        }
        diagnostics::startup(&mut state);
        state.setup.distro = TextEdit::new(&state.config.distro);
        state
    }
    fn send(&self, r: Request) {
        if let Some(b) = &self.backend {
            let _ = b.tx.send(r);
        }
    }
    fn navigate(&mut self, page: Page) {
        self.ui.reset_scroll = true;
        self.ui.system_menu = false;
        if page != self.page {
            self.form = None;
            self.editing = false;
        }
        self.page = page;
    }
    /// Switches the active system, by id or by kind (`systems::Pick`). Allowed while work
    /// runs: processes stay with the system that started them and nothing is stopped.
    /// Returns whether the named system is active afterwards (false when none exists).
    fn select_system<'a>(&mut self, pick: impl Into<systems::Pick<'a>>) -> bool {
        self.ui.system_menu = false;
        let Some(id) = self.systems.resolve(pick.into()) else {
            self.notice = "No such system. Create one with New system… in the system list.".into();
            return false;
        };
        if id == self.systems.active_id() {
            return true;
        }
        self.systems.set_active(&id);
        self.ui.tools_filter = 0;
        self.ui.expanded = None;
        systems::follow_active(self);
        let name = self.systems.current().label().to_string();
        self.notice = if let Err(e) = self.systems.save(self.headless) {
            format!("System changed, but it could not be saved: {e}")
        } else if systems::active_work(self) {
            format!("Active system: {name}. Running work stays with the system that started it.")
        } else {
            format!("Active system: {name}.")
        };
        true
    }
    fn log(&mut self, line: String) {
        self.logs.push_back(line);
        while self.logs.len() > 600 {
            self.logs.pop_front();
        }
    }
    fn start(&mut self, index: usize) {
        if self.headless {
            self.notice = "Preview: process execution disabled".into();
            return;
        }
        if self.setup.batch.is_some() || self.setup.jobs.iter().any(|j| j.alive) {
            self.notice =
                "Wait for the active installation to finish before starting dependent tools."
                    .into();
            return;
        }
        let p = self.config.profiles[index].clone();
        if p.program == "wsl" && !self.wsl_ready() {
            self.navigate(Page::Settings);
            self.notice =
                "Select a working WSL 2 environment in Settings → Environment before starting Linux tools.".into();
            return;
        }
        if self
            .status
            .get(&p.id)
            .is_some_and(|v| v == "Running" || v == "Starting")
        {
            return;
        }
        // A stage of the staged launch keeps the owner `bundle` recorded; anything else
        // belongs to the system active now.
        if !systems::staged(self, &p.id) || self.systems.owner(&p.id).is_none() {
            self.systems.claim(&p.id);
        }
        let owner = self
            .systems
            .owner(&p.id)
            .unwrap_or(self.systems.active_id())
            .to_string();
        if let Some(conflict) = vehicle::start_conflict(self, &p.id, &owner) {
            self.notice = conflict;
            return;
        }
        let env = self
            .systems
            .get(&owner)
            .map(|sys| backend::launch_env(&p.id, sys))
            .unwrap_or_default();
        self.status.insert(p.id.clone(), "Starting".into());
        self.readiness.remove(&p.id);
        self.log(format!("[session] Starting {}", p.name));
        self.send(Request::StartEnv(p, self.config.distro.clone(), env));
    }
    /// Starts a staged workspace session (e.g. `systems::session_stages(sys)`). Returns whether
    /// a new launch began. A launch already in progress, for this or another system, is kept:
    /// it is never replaced half-way, which would orphan its later stages. A simulator that
    /// conflicts with one already running (`vehicle::simulator_conflict`) is refused.
    fn bundle(&mut self, stages: impl Into<Vec<&'static str>>) -> bool {
        let stages: Vec<&'static str> = stages.into();
        if stages.is_empty() {
            return false;
        }
        if let Some(launch) = &self.session {
            let by = self
                .systems
                .owner("session")
                .and_then(|id| self.systems.get(id))
                .map_or_else(|| "another system".to_string(), |x| x.label().to_string());
            self.notice = format!(
                "A workspace launch for {by} is still starting {}. Wait for it, or stop the session first.",
                launch.current()
            );
            return false;
        }
        if !self.headless && !self.wsl_ready() {
            self.navigate(Page::Settings);
            self.notice =
                "Complete environment setup (Settings → Environment) before starting the workspace."
                    .into();
            return false;
        }
        let active = self.systems.active_id().to_string();
        if let Some(conflict) = stages
            .iter()
            .find_map(|id| vehicle::simulator_conflict(self, id, &active))
        {
            self.notice = conflict;
            return false;
        }
        if self.vehicle.connected() && !self.vehicle.owned_by(&active) && stages.contains(&"link") {
            self.notice = format!(
                "The live link belongs to {}. Stop that session before starting this one.",
                self.vehicle
                    .owner
                    .as_deref()
                    .and_then(|o| self.systems.get(o))
                    .map_or("another system", |x| x.label())
            );
            return false;
        }
        self.session = Some(session::Launch::new(stages.clone()));
        // Later stages start on later frames; they belong to the system that asked now.
        for key in stages.iter().copied().chain(["session"]) {
            if !systems::running(self, key) {
                self.systems.claim(key);
            }
        }
        self.notice = format!(
            "Starting {} first; each later tool waits for the previous one's readiness.",
            stages[0]
        );
        true
    }
    fn stop_all(&mut self) {
        self.session = None;
        vehicle::stop(self);
        if self.recorder.recording || self.recorder.playing {
            recordings::send(self, "stop");
        }
        studio::pipelines::cancel(self);
        for id in self.status.keys() {
            self.send(Request::Stop(id.clone()));
        }
        self.notice =
            "Stopping owned processes. This is a process stop, not a flight land command.".into();
        activity::record(self, "Stopped owned processes (not a vehicle command)");
    }
    fn wsl_ready(&self) -> bool {
        self.setup.host.as_ref().is_some_and(|h| {
            h.distros
                .iter()
                .any(|d| d.name == self.config.distro && d.version == 2)
        })
    }
    fn open(&self, p: String) {
        if !self.headless {
            self.send(Request::Open(p));
        }
    }
    fn edit(&mut self) {
        self.ui.reset_scroll = true;
        self.editor = TextEdit::new(&serde_json::to_string_pretty(&self.config).unwrap());
        self.editing = true;
    }
    fn apply(&mut self) {
        if self.setup.batch.is_some() {
            self.notice = "Finish or cancel workflow setup before editing launch profiles.".into();
            return;
        }
        match serde_json::from_str::<Config>(&self.editor.text) {
            Ok(c) => {
                if c.distro != self.config.distro && self.busy() {
                    self.notice =
                        "Stop the session (owned tools, vehicle link, recordings, pipeline jobs) and installations before changing the WSL distribution.".into();
                    return;
                }
                if self.status.iter().any(|(id, status)| {
                    (status == "Running" || status == "Starting")
                        && !c.profiles.iter().any(|p| &p.id == id)
                }) {
                    self.notice =
                        "Stop a running profile before removing it from the registry.".into();
                    return;
                }
                let mut ids = std::collections::HashSet::new();
                if c.distro.is_empty()
                    || c.profiles.is_empty()
                    || c.profiles.iter().any(|p| {
                        p.id.is_empty()
                            || !p
                                .id
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                            || !ids.insert(p.id.clone())
                    })
                {
                    self.notice="Invalid config: use unique alphanumeric profile IDs and a WSL distribution.".into();
                    return;
                }
                if !self.headless {
                    if let Err(e) = backend::save(&c) {
                        self.notice = e;
                        return;
                    }
                }
                self.config = c;
                self.selected = 0;
                self.editing = false;
                self.notice = "Profiles saved. Detect tools again after changing paths.".into();
            }
            Err(e) => self.notice = format!("JSON error: {e}"),
        }
    }
    fn busy(&self) -> bool {
        self.recorder.recording
            || self.recorder.playing
            || self.setup.jobs.iter().any(|j| j.alive)
            || vehicle::active(self)
            || self.studio.pipes.job.is_some()
            || self
                .status
                .values()
                .any(|v| matches!(v.as_str(), "Running" | "Starting"))
    }
}
fn poll(s: &mut State, _: f64, tx: &mut Tx) -> bool {
    let reset = std::mem::take(&mut s.ui.reset_scroll);
    if reset {
        tx.scroll_to_instant("main-scroll", 0., 0.);
    }
    if s.headless {
        return reset;
    }
    tx.wake_after(Duration::from_millis(300));
    let mut changed = reset
        | recordings::poll(s)
        | setup::poll(s)
        | studio::poll(s)
        | guided::poll(s)
        | ulog::poll(s)
        | agent::poll(s)
        | vehicle::poll(s)
        | hardware::poll(s)
        | diagnostics::poll(s);
    let events: Vec<_> = s
        .backend
        .as_ref()
        .map(|b| b.rx.try_iter().take(150).collect())
        .unwrap_or_default();
    for e in events {
        changed = true;
        qgc_launch::observe(s, &e);
        match e {
            Event::Log(id, line) => s.log(format!("[{id}] {line}")),
            Event::Status(id, status) => {
                s.log(format!("[{id}] {status}"));
                if status != "Running" && status != "Starting" {
                    s.readiness.remove(&id);
                }
                s.status.insert(id, status);
            }
            Event::Detected(id, found) => {
                s.detected.insert(id, found);
            }
            Event::Readiness(id, value) => {
                s.readiness.insert(id, value);
            }
        }
    }
    changed | session::poll(s) | systems::poll(s)
}
fn main() {
    diagnostics::install_panic_hook();
    if std::env::args().len() == 1 && !rune::single_instance("local.drone-deck") {
        return;
    }
    if let Err(error) = rune::set_app_icon(include_bytes!("../assets/drone-deck.png").as_slice()) {
        eprintln!("Drone Deck icon: {error}");
    }
    let app = rune::App::new(AppSpec {
        min_size: Some((360., 320.)),
        families: (
            "Bahnschrift, Segoe UI Variable Text, Segoe UI",
            "Cascadia Mono, Consolas",
        ),
        poll: Some(poll),
        on_key: Some(ui::zoom_shortcuts),
        scenes: SCENES,
        ..AppSpec::new(
            "Drone Deck · Autonomy Workspace",
            window::startup_size(),
            State::new,
            view,
        )
    });
    let handle = app.handle();
    app.on_ready(move |native| {
        window::fit_native_window(native);
        let handle = handle.clone();
        handle.clone().update(move |s, tx| {
            if !s.headless && s.backend.is_none() {
                agent::enable();
                let wake = handle.clone();
                s.vehicle.waker = Some(std::sync::Arc::new(move || wake.wake()));
                s.backend = Some(backend::spawn());
                setup::request(s, "scan");
                tx.wake_after(Duration::from_millis(300));
            }
        });
    })
    .on_close(|s, _| {
        setup::save_workflow(s);
        // A setup or model start still with its helper dies with it: say what may not start.
        setup::close::warn_on_close(s);
        recordings::shutdown(s);
        // Like Stop session: a running pipeline job is cancelled. If the cancel reaches the agent
        // after its shutdown began, the shutdown stops the job's processes instead.
        studio::pipelines::cancel(s);
        // The window cannot refuse to close, so a bridged physical link is at least recorded:
        // QGroundControl and scripts lose the vehicle with it.
        let bridged = s.vehicle.link.is_some()
            && !s.vehicle.sim
            && s.vehicle
                .owner
                .as_deref()
                .and_then(|id| s.systems.get(id))
                .is_some_and(systems::physical_bridge);
        if bridged {
            let text = "Closing Drone Deck closed a physical vehicle link that QGroundControl or scripts were bridged through; they lost the vehicle";
            diagnostics::log("warn", text);
            activity::record(s, text);
        }
        vehicle::stop(s);
        if let Some(b) = s.backend.take() {
            b.shutdown();
        }
        agent::shutdown_all();
    })
    .run();
}

#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod tests;

fn guided_at(s: &mut State, chapter: usize) {
    guided::open(s);
    s.guided.chapter = chapter;
    if s.guided.name.text.is_empty() {
        s.guided.name = TextEdit::new("My first system");
    }
}
/// Headless scenes: `drone-deck --scene <name> --snap out.png`.
type Scene = (&'static str, fn(&mut State));
const SCENES: &[Scene] = &[
    ("tools", |s| s.navigate(Page::Tools)),
    ("data", |s| s.navigate(Page::Data)),
    ("recordings", |s| {
        s.ui.data_tab = 0;
        s.navigate(Page::Data)
    }),
    ("models", |s| {
        s.ui.data_tab = 1;
        s.navigate(Page::Data)
    }),
    ("pipelines", |s| {
        s.ui.data_tab = 2;
        s.navigate(Page::Data)
    }),
    ("pipelines-demo", studio::pipelines::preview),
    ("projects", |s| {
        s.ui.data_tab = 3;
        s.navigate(Page::Data)
    }),
    ("flight-logs", ulog::preview),
    ("activity", |s| s.navigate(Page::Activity)),
    ("process-output", |s| {
        s.activity.tab = 1;
        s.navigate(Page::Activity)
    }),
    ("installations", |s| {
        s.activity.tab = 2;
        s.navigate(Page::Activity)
    }),
    ("settings", |s| s.navigate(Page::Settings)),
    ("ros", |s| {
        s.select_system(systems::ROS);
    }),
    ("ai", |s| {
        s.select_system(systems::AI);
    }),
    ("physical", |s| {
        s.select_system(systems::PHYSICAL);
    }),
    ("system-menu", |s| {
        s.systems.claim("sih");
        s.status.insert("sih".into(), "Running".into());
        s.ui.system_menu = true;
    }),
    ("systems-settings", |s| {
        s.navigate(Page::Settings);
        let id = s.systems.create(systems::ROS, "Gazebo camera lab");
        s.systems.editor = s.systems.get(&id).map(systems::Draft::of);
    }),
    ("system-new", guided::new_system),
    ("setup", |s| guided_at(s, 0)),
    ("setup-system", |s| guided_at(s, 1)),
    ("setup-prep", |s| guided_at(s, 2)),
    ("setup-run", |s| guided_at(s, 3)),
    ("recordings-demo", recordings::preview),
    ("setup-demo", setup::preview),
    ("setup-no-wsl", |s| {
        setup::preview(s);
        let host = s.setup.host.as_mut().unwrap();
        host.distros.clear();
        host.wsl_ready = false;
        host.wsl_engine = false;
        s.setup.selected = 0;
        s.setup.message =
            "Preview: no Linux environment installed. Setup actions are disabled.".into();
    }),
    ("tool-settings", profile_editor::open),
    ("tools-all", |s| {
        s.ui.tools_all = true;
        s.navigate(Page::Tools)
    }),
    ("tool-new", |s| profile_editor::create(s, "Development")),
    ("tools-ai", |s| {
        s.select_system(systems::AI);
        s.navigate(Page::Tools);
        s.ui.expanded = Some("onnx".into());
    }),
    ("tool-review", |s| {
        profile_editor::open(s);
        let px4 = s
            .setup
            .recipes
            .iter()
            .position(|r| r.id == "px4")
            .unwrap_or(0);
        setup::choose(s, px4);
        setup::review(s);
    }),
    ("activity-demo", |s| {
        for text in [
            "Started PX4 SIH",
            "Saved launch profile registry",
            "Started recording",
            "Stopped owned processes (not a vehicle command)",
        ] {
            activity::record(s, text);
        }
        s.navigate(Page::Activity)
    }),
    ("process-output-demo", |s| {
        for line in [
            "[session] Starting PX4 SIH",
            "[sih] INFO  [px4] startup script: /bin/sh etc/init.d-posix/rcS 0",
            "[sih] WARN  [health_and_arming_checks] Preflight: GPS fix too low",
            "[link] Connected to tcp:127.0.0.1:5760",
            "[sih] ERROR [simulator_sih] lockstep timeout",
        ] {
            s.log(line.into());
        }
        s.activity.tab = 1;
        s.navigate(Page::Activity)
    }),
    ("settings-registry", |s| {
        s.navigate(Page::Settings);
        s.edit();
    }),
    ("installations-demo", |s| {
        for (id, tool, status, step, alive, message) in [
            (
                "job-2",
                "mavsdk",
                "Running",
                3,
                true,
                "Building MAVSDK Python wheels",
            ),
            (
                "job-1",
                "px4",
                "Failed",
                5,
                false,
                "make px4_sitl_default exited with status 2",
            ),
        ] {
            s.setup.jobs.push(setup::Job {
                id: id.into(),
                tool: tool.into(),
                status: status.into(),
                message: message.into(),
                step,
                total: 7,
                alive,
                log_tail: "[5/7] make px4_sitl_default\nninja: build stopped: subcommand failed."
                    .into(),
                ..Default::default()
            });
        }
        s.setup.selected_job = Some("job-1".into());
        s.activity.tab = 2;
        s.navigate(Page::Activity)
    }),
    ("setup-env", |s| guided::preview(s, "env")),
    ("setup-review", |s| guided::preview(s, "review")),
    ("setup-applying", |s| guided::preview(s, "applying")),
    ("setup-attention", |s| guided::preview(s, "attention")),
    ("system-launch-demo", |s| {
        s.session = Some(session::Launch::new(systems::SESSIONS[systems::SIMULATION]));
        s.status.insert("sih".into(), "Starting".into());
    }),
    ("system-live-demo", |s| {
        for id in ["sih", "qgc"] {
            s.systems.claim(id);
            s.status.insert(id.into(), "Running".into());
        }
        s.readiness.insert("sih".into(), "Ready".into());
        let id = s.systems.active_id().to_string();
        vehicle::preview(s, &id, true, true);
    }),
    ("physical-live-demo", |s| {
        s.select_system(systems::PHYSICAL);
        let id = s.systems.active_id().to_string();
        vehicle::preview(s, &id, true, false);
    }),
    ("vehicle-live", vehicle::view::scene_live),
    ("vehicle-stale", vehicle::view::scene_stale),
    ("vehicle-actions", vehicle::view::scene_actions),
    ("vehicle-health-checks", vehicle::view::scene_health_checks),
    ("vehicle-console", vehicle::view::scene_console),
    ("vehicle-gated", vehicle::view::scene_gated),
    ("vehicle-readonly", vehicle::view::scene_readonly),
    ("vehicle-launching", vehicle::view::scene_launching),
    (
        "vehicle-ardupilot-console",
        vehicle::view::scene_ardupilot_console,
    ),
    ("vehicle-link-retrying", vehicle::view::scene_link_retrying),
    ("physical-params", hardware::preview_params),
    ("physical-logs", hardware::preview_logs),
    ("setup-recipe", |s| {
        setup::preview(s);
        guided_at(s, 1);
        s.ui.open.insert("technical-recipe".into());
        s.guided.path = TextEdit::new("tools/px4");
        s.guided.path_error = true;
    }),
    ("setup-run-physical", |s| {
        s.select_system(systems::PHYSICAL);
        guided_at(s, 3);
    }),
    ("setup-run-ai", |s| {
        studio::pipelines::preview(s);
        s.select_system(systems::AI);
        guided_at(s, 3);
        s.guided.task = 2;
    }),
    ("setup-receipt", |s| {
        guided_at(s, 3);
        s.guided.done = true;
        s.guided.evidence = vec!["Headless preview fixture: no evidence was collected.".into()];
    }),
];
