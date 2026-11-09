#![windows_subsystem = "windows"]
mod backend;
mod hardware;
mod process_output;
mod profile_editor;
mod qgc_launch;
mod recordings;
mod session;
mod setup;
mod studio;
mod ui;
mod window;
use backend::{Backend, Config, Event, Request};
use rune::tok::TextStyle;
use rune::{AppSpec, Color, Cx, El, ScrollSpec, TextEdit, Tx, col, el, row, text, text_input};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use ui::*;

struct State {
    hardware: hardware::Hardware,
    studio: studio::Studio,
    setup: setup::Setup,
    recorder: recordings::Recorder,
    ui: ui::UiState,
    config: Config,
    backend: Option<Backend>,
    page: usize,
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
    log_filter: bool,
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
            config,
            backend: None,
            page: 0,
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
            log_filter: false,
        };
        ui::restore_preferences(&mut state);
        state.setup.distro = TextEdit::new(&state.config.distro);
        state
    }
    fn send(&self, r: Request) {
        if let Some(b) = &self.backend {
            let _ = b.tx.send(r);
        }
    }
    fn navigate(&mut self, page: usize) {
        self.ui.reset_scroll = true;
        self.page = page;
        self.form = None;
        self.editing = false;
        let category = match page {
            1 => "Simulation",
            2 => "Flight operations",
            3 => "Development",
            4 => "AI & data",
            _ => return,
        };
        if let Some(i) = self
            .config
            .profiles
            .iter()
            .position(|p| p.category == category)
        {
            self.selected = i;
        }
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
            self.navigate(8);
            self.notice =
                "Select a working WSL 2 environment in Setup before starting Linux tools.".into();
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
    fn bundle(&mut self) {
        if !self.headless && !self.wsl_ready() {
            self.navigate(8);
            self.notice = "Complete environment setup before starting the SIH workspace.".into();
            return;
        }
        self.session = Some(session::Launch::default());
        self.notice =
            "Starting SIH first; the visualizer will follow after live state reaches the relay."
                .into();
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
    let mut changed =
        reset | recordings::poll(s) | setup::poll(s) | studio::poll(s) | hardware::poll(s);
    let events: Vec<_> = s
        .backend
        .as_ref()
        .map(|b| b.rx.try_iter().take(150).collect())
        .unwrap_or_default();
    for e in events {
        changed = true;
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
            "Segoe UI Variable Text, Segoe UI",
            "Cascadia Code, Consolas",
        ),
        poll: Some(poll),
        on_key: Some(ui::zoom_shortcuts),
        scenes: &[
            ("simulators", |s| s.navigate(1)),
            ("flight", |s| s.navigate(2)),
            ("development", |s| s.navigate(3)),
            ("ai", |s| s.navigate(4)),
            ("datasets", |s| {
                s.navigate(4);
                s.studio.tab = 1;
            }),
            ("projects", |s| {
                s.navigate(4);
                s.studio.tab = 2;
            }),
            ("physical", |s| {
                s.navigate(2);
                s.hardware.mode = 1;
            }),
            ("integrations", |s| s.navigate(5)),
            ("settings", |s| s.navigate(6)),
            ("recordings", |s| s.navigate(7)),
            ("recordings-demo", recordings::preview),
            ("setup", |s| s.navigate(8)),
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
        ],
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
