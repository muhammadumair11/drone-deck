#![windows_subsystem = "windows"]
mod activity;
mod backend;
mod guided;
mod hardware;
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
mod window;
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
    metrics: HashMap<String, String>,
    logs: VecDeque<String>,
    notice: String,
    editor: TextEdit,
    editing: bool,
    form: Option<profile_editor::Form>,
    headless: bool,
    confirm: Option<String>,
    last_metric: Option<Instant>,
    readiness: HashMap<String, String>,
    session: Option<session::Launch>,
    qgc: qgc_launch::Watch,
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
            metrics: HashMap::new(),
            logs: VecDeque::new(),
            notice,
            editor: TextEdit::new(""),
            editing: false,
            form: None,
            headless,
            confirm: None,
            last_metric: None,
            readiness: HashMap::new(),
            session: None,
            qgc: Default::default(),
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
    /// Switches the active system. Refused while owned processes, captures or installs run,
    /// so a session never outlives the context it was started in. Returns whether `kind` is
    /// the active system afterwards.
    fn select_system(&mut self, kind: usize) -> bool {
        self.ui.system_menu = false;
        if kind == self.systems.active {
            return true;
        }
        if systems::active_work(self) || self.setup.batch.is_some() {
            self.notice = "Stop the active session before changing systems.".into();
            return false;
        }
        self.systems.active = kind.min(3);
        self.ui.tools_filter = 0;
        self.ui.expanded = None;
        if kind == systems::PHYSICAL {
            self.hardware.mode = 1;
        } else if self.hardware.mode == 1 {
            self.hardware.mode = 0;
        }
        if let Err(e) = self.systems.save(self.headless) {
            self.notice = format!("System changed, but it could not be saved: {e}");
        } else {
            self.notice = format!("Active system: {}.", self.systems.name(kind));
        }
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
        self.status.insert(p.id.clone(), "Starting".into());
        self.readiness.remove(&p.id);
        self.log(format!("[session] Starting {}", p.name));
        self.send(Request::Start(p, self.config.distro.clone()));
    }
    /// Starts a staged workspace session (e.g. `systems::SESSIONS[k]`).
    fn bundle(&mut self, stages: &'static [&'static str]) {
        if stages.is_empty() {
            return;
        }
        if !self.headless && !self.wsl_ready() {
            self.navigate(Page::Settings);
            self.notice =
                "Complete environment setup (Settings → Environment) before starting the workspace."
                    .into();
            return;
        }
        self.session = Some(session::Launch::new(stages));
        self.notice = format!(
            "Starting {} first; each later tool waits for the previous one's readiness.",
            stages[0]
        );
    }
    fn stop_all(&mut self) {
        self.session = None;
        if self.recorder.recording || self.recorder.playing {
            recordings::send(self, "stop");
        }
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
                        "Stop active tools, installations, recording and replay before changing the WSL distribution.".into();
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
        | hardware::poll(s)
        | guided::poll(s);
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
                if id == "telemetry" && status != "Running" {
                    s.metrics.clear();
                    s.last_metric = None;
                }
                if status != "Running" && status != "Starting" {
                    s.readiness.remove(&id);
                }
                s.status.insert(id, status);
            }
            Event::Detected(id, found) => {
                s.detected.insert(id, found);
            }
            Event::Metric(k, v) => {
                s.metrics.insert(k, v);
                s.last_metric = Some(Instant::now());
            }
            Event::Readiness(id, value) => {
                s.readiness.insert(id, value);
            }
            Event::Hardware(value) => hardware::event(s, value),
        }
    }
    if s.last_metric
        .is_some_and(|t| t.elapsed() > Duration::from_secs(5))
        && s.metrics.get("link").is_some_and(|v| v != "Stale")
    {
        s.metrics.insert("link".into(), "Stale".into());
        changed = true;
    }
    changed | session::poll(s)
}
fn main() {
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
        handle.update(|s, tx| {
            if !s.headless && s.backend.is_none() {
                s.backend = Some(backend::spawn());
                setup::request(s, "scan");
                tx.wake_after(Duration::from_millis(300));
            }
        });
    })
    .on_close(|s, _| {
        setup::save_workflow(s);
        recordings::shutdown(s);
        if let Some(b) = s.backend.take() {
            b.shutdown();
        }
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
    ("datasets", |s| {
        s.ui.data_tab = 2;
        s.navigate(Page::Data)
    }),
    ("projects", |s| {
        s.ui.data_tab = 3;
        s.navigate(Page::Data)
    }),
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
            "[relay] Forwarding 127.0.0.1:14560 -> 127.0.0.1:14550",
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
        for id in ["sih", "relay", "jmavsim", "telemetry"] {
            s.status.insert(id.into(), "Running".into());
        }
        s.readiness.insert("sih".into(), "Ready".into());
        s.readiness.insert("relay".into(), "Forwarding".into());
        for (k, v) in [
            ("link", "Connected"),
            ("altitude", "12.4 m"),
            ("mode", "Hold"),
        ] {
            s.metrics.insert(k.into(), v.into());
        }
    }),
    ("physical-live-demo", |s| {
        s.select_system(systems::PHYSICAL);
        s.status.insert("hardware".into(), "Running".into());
        hardware::event(
            s,
            serde_json::json!({"kind":"packet","system":1,"component":1,"message_id":0,
                "path":"workspace/hardware/capture","fields":{"type":2,"autopilot":12,"base_mode":81}}),
        );
        s.hardware.selected = Some("1:1".into());
    }),
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
    ("setup-receipt", |s| {
        guided_at(s, 3);
        s.guided.done = true;
        s.guided.evidence = vec!["Headless preview fixture: no evidence was collected.".into()];
    }),
];
