pub(crate) mod batch;
mod bridge;
pub(crate) mod close;
pub(crate) mod profiles;
#[cfg(test)]
mod tests;
mod view;
pub(crate) mod workflow;
use crate::*;
pub(crate) use bridge::process_alive as job_alive;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};
pub(crate) use view::{environment, install_review, job_log, jobs_panel, option_row};

#[derive(Clone, Deserialize)]
pub(crate) struct Recipe {
    pub id: String,
    pub name: String,
    pub version: String,
    pub target: String,
    pub docs: String,
    pub summary: String,
    pub steps: Vec<String>,
    pub automatic: bool,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub privileges: String,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub verification: String,
}
#[derive(Clone, Default, Deserialize)]
pub(crate) struct Distro {
    pub name: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub version: u16,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub support: String,
}
#[derive(Default, Deserialize)]
pub(crate) struct Host {
    pub build: u32,
    pub windows: String,
    pub architecture: String,
    pub eligible: bool,
    pub virtualization: String,
    pub ram_gib: f64,
    pub gpu: String,
    pub distros: Vec<Distro>,
    pub wsl_ready: bool,
    #[serde(default)]
    pub disk_gib: f64,
    #[serde(default)]
    pub wsl_engine: bool,
}
#[derive(Clone, Default, Deserialize)]
pub(crate) struct Candidate {
    pub tool: String,
    pub found: bool,
    pub compatible: bool,
    pub path: String,
    pub executable: String,
    pub version: String,
    pub reason: String,
    pub managed: bool,
    #[serde(default)]
    pub build_target: String,
    #[serde(default)]
    pub environment: String,
}
#[derive(Clone, Default, Deserialize)]
pub(crate) struct Job {
    pub id: String,
    pub tool: String,
    pub status: String,
    pub message: String,
    pub step: u32,
    pub total: u32,
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub process_ticks: u64,
    #[serde(default)]
    pub result: Option<Candidate>,
    #[serde(default)]
    pub configure_after: bool,
    #[serde(skip)]
    pub alive: bool,
    #[serde(skip)]
    pub log_tail: String,
}
pub(crate) struct Setup {
    pub host: Option<Host>,
    pub online: Vec<Distro>,
    pub recipes: Vec<Recipe>,
    pub selected: usize,
    pub distro: TextEdit,
    pub location: TextEdit,
    pub candidate: Option<Candidate>,
    pub jobs: Vec<Job>,
    pub selected_job: Option<String>,
    pub pending: bool,
    /// What the pending request starts (`start`), while it is still with its helper: a close
    /// then stops the helper and the job may not start (`close`).
    pub starting: Option<String>,
    pub review: bool,
    pub message: String,
    pub tx: Sender<Result<Value, String>>,
    pub rx: Receiver<Result<Value, String>>,
    pub last_refresh: Instant,
    pub workflow: usize,
    pub checks: HashMap<String, Candidate>,
    pub queue: Vec<usize>,
    pub configure_after: bool,
    pub applied_jobs: std::collections::HashSet<String>,
    pub last_applied: Option<String>,
    pub batch: Option<batch::Batch>,
    pub batch_message: String,
    pub checkpoint_loaded: bool,
    pub checkpoint_cache: String,
    /// Existing tool locations given in guided setup, by recipe id; workflow checks use them.
    pub overrides: HashMap<String, String>,
}
impl Setup {
    /// A workflow queue or a live installation job owns the environment.
    pub(crate) fn active(&self) -> bool {
        self.batch.is_some() || self.jobs.iter().any(|j| j.alive)
    }
    pub(crate) fn recipe_index(&self, id: &str) -> Option<usize> {
        self.recipes.iter().position(|r| r.id == id)
    }
    fn typed_distro(&self) -> Option<&Distro> {
        let name = self.distro.text.trim();
        self.host
            .as_ref()?
            .distros
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
    }
    /// The typed distribution is installed as WSL 2.
    pub(crate) fn can_use_distro(&self) -> bool {
        self.typed_distro().is_some_and(|d| d.version == 2)
    }
    /// The typed distribution is installed and can open its first-run terminal.
    pub(crate) fn can_initialize(&self) -> bool {
        self.typed_distro().is_some()
    }
}
impl Default for Setup {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            host: None,
            online: Vec::new(),
            recipes: serde_json::from_str(include_str!("../../scripts/setup/catalog.json"))
                .unwrap(),
            selected: 0,
            distro: TextEdit::new("Ubuntu-24.04"),
            location: TextEdit::new(""),
            candidate: None,
            jobs: Vec::new(),
            selected_job: None,
            pending: false,
            starting: None,
            review: false,
            message: "Scan this computer to choose or install its tool environment.".into(),
            tx,
            rx,
            last_refresh: Instant::now() - Duration::from_secs(5),
            workflow: 0,
            checks: HashMap::new(),
            queue: Vec::new(),
            configure_after: true,
            applied_jobs: Default::default(),
            last_applied: None,
            batch: None,
            batch_message: String::new(),
            checkpoint_loaded: false,
            checkpoint_cache: String::new(),
            overrides: HashMap::new(),
        }
    }
}
pub(crate) fn root() -> PathBuf {
    backend::root().join("setup/jobs")
}
pub(crate) fn save_workflow(s: &mut State) {
    let _ = batch::persist(s);
}
pub(crate) fn choose(s: &mut State, index: usize) {
    s.setup.selected = index;
    s.setup.candidate = None;
    s.setup.review = false;
    s.setup.location = TextEdit::new("");
}
pub(crate) fn request(s: &mut State, action: &str) {
    if s.headless {
        s.notice = "Preview: setup actions are disabled.".into();
        return;
    }
    if s.setup.pending {
        return;
    }
    if s.setup.batch.is_some() && !["start", "cancel"].contains(&action) {
        s.setup.message = "Finish or cancel workflow setup before another setup operation.".into();
        return;
    }
    if action == "start" && s.busy() {
        s.setup.message =
            "Stop the session (owned tools, vehicle link, recordings, pipeline jobs) before installing shared dependencies.".into();
        return;
    }
    let r = &mut s.setup;
    let tool = &r.recipes[r.selected].id;
    let value = json!({"action":action,"tool":tool,"distro":r.distro.text.trim(),"location":r.location.text.trim(),"id":r.selected_job,"configure_after":r.configure_after});
    r.starting = starting(r, action);
    r.pending = true;
    r.review = false;
    r.message = match action {
        "scan" => "Checking Windows and WSL…",
        "online" => "Loading Microsoft's current Linux distribution catalog…",
        "detect" => "Inspecting the selected tool without changing it…",
        "start" => "Starting the reviewed setup job; complete any Windows approval prompt…",
        "cancel" => "Requesting cancellation at a safe step boundary…",
        _ => "Opening Linux initialization…",
    }
    .into();
    bridge::request(value, r.tx.clone());
}
/// What a request starts, named for the close warning: only `start` hands work to a worker.
pub(crate) fn starting(r: &Setup, action: &str) -> Option<String> {
    (action == "start").then(|| format!("the setup job for {}", r.recipes[r.selected].name))
}
pub(crate) fn poll(s: &mut State) -> bool {
    let mut changed = false;
    if !s.setup.checkpoint_loaded {
        s.setup.checkpoint_loaded = true;
        batch::restore(s);
        changed = close::notice_at_start(s);
    }
    let events: Vec<_> = s.setup.rx.try_iter().collect();
    for event in events {
        changed = true;
        s.setup.pending = false;
        s.setup.starting = None;
        match event {
            Err(e) => s.setup.message = e,
            Ok(value) => apply_event(s, value),
        }
    }
    if s.setup.last_refresh.elapsed() > Duration::from_secs(1) {
        s.setup.last_refresh = Instant::now();
        let jobs = bridge::read_jobs(&root(), s.setup.selected_job.as_deref());
        let fingerprint = |rows: &Vec<Job>| {
            rows.iter()
                .map(|j| format!("{}{}{}{}{}", j.id, j.status, j.message, j.alive, j.log_tail))
                .collect::<String>()
        };
        if fingerprint(&jobs) != fingerprint(&s.setup.jobs) {
            // A toast for each known job whose status changed (not for jobs seen for the
            // first time, such as those restored at start).
            for job in &jobs {
                let before = s.setup.jobs.iter().find(|j| j.id == job.id);
                if before.is_some_and(|b| b.status != job.status) {
                    let name = s
                        .setup
                        .recipes
                        .iter()
                        .find(|r| r.id == job.tool)
                        .map_or(job.tool.as_str(), |r| r.name.as_str());
                    crate::notify::job(&job.id, name, &job.status, &job.message);
                }
            }
            s.setup.jobs = jobs;
            changed = true;
        }
    }
    if !s.busy() {
        if let Some(job) = s
            .setup
            .jobs
            .iter()
            .find(|j| {
                j.configure_after
                    && matches!(j.status.as_str(), "Ready" | "Existing installation")
                    && j.result.as_ref().is_some_and(|c| {
                        c.compatible
                            && (c.environment.is_empty() || c.environment == s.config.distro)
                    })
                    && !s.setup.applied_jobs.contains(&j.id)
            })
            .cloned()
        {
            s.setup.selected_job = Some(job.id.clone());
            profiles::use_job_result(s);
            if s.setup.last_applied.is_some() {
                s.setup.applied_jobs.insert(job.id);
            } else if s.setup.batch.is_some() {
                s.setup.batch = None;
                s.setup.batch_message="Workflow paused: the verified tool could not be configured. Review the setup message before continuing.".into();
            }
            changed = true;
        }
    }
    let changed = changed | workflow::poll(s) | batch::poll(s);
    changed | batch::persist(s)
}
fn apply_event(s: &mut State, value: Value) {
    if value["ok"].as_bool() != Some(true) {
        if value["action"] == "start" && s.setup.batch.is_some() {
            s.setup.batch = None;
            s.setup.batch_message="Workflow paused because a setup job could not start. Review the error and recheck the workflow.".into();
        }
        s.setup.message = value["error"]
            .as_str()
            .unwrap_or("Setup request failed")
            .into();
        return;
    }
    let data = value["data"].clone();
    match value["action"].as_str().unwrap_or("") {
        "scan" => match serde_json::from_value::<Host>(data) {
            Ok(host) => {
                let configured = host
                    .distros
                    .iter()
                    .any(|d| d.name == s.config.distro && d.version == 2);
                s.setup.message = format!(
                    "Windows checked. {} WSL distributions detected.",
                    host.distros.len()
                );
                s.setup.host = Some(host);
                if configured {
                    s.send(Request::Detect(s.config.clone()));
                } else if s.page == crate::ui::Page::System && !s.systems.any_prepared() {
                    crate::guided::open(s);
                } else if s.page != crate::ui::Page::Setup {
                    s.notice = "No usable WSL 2 environment yet. Choose or install one in Settings → Environment; Windows-native tools remain available.".into();
                }
            }
            Err(e) => s.setup.message = format!("Could not read host discovery: {e}"),
        },
        "online" => {
            s.setup.online = serde_json::from_value(data).unwrap_or_default();
            s.setup.message="Choose a distribution from the live Microsoft catalog. Ubuntu 24.04 is the tested recipe baseline.".into();
        }
        "detect" => match serde_json::from_value::<Candidate>(data) {
            Ok(candidate) => {
                s.setup
                    .checks
                    .insert(candidate.tool.clone(), candidate.clone());
                if candidate.tool != s.setup.recipes[s.setup.selected].id {
                    s.setup.message="Discovery finished for the previous selection. Detect the current tool again.".into();
                } else {
                    s.setup.message = candidate.reason.clone();
                    s.setup.candidate = Some(candidate);
                }
            }
            Err(e) => s.setup.message = format!("Invalid discovery result: {e}"),
        },
        "start" => {
            s.setup.selected_job = data["id"].as_str().map(str::to_owned);
            if let Some(id) = data["id"].as_str() {
                batch::started(s, id);
            }
            s.setup.message="Setup started. Jobs and logs are retained across app restarts. Installation continues if you close Drone Deck.".into();
        }
        "browse" => {
            if data["tool"].as_str() == Some(s.setup.recipes[s.setup.selected].id.as_str()) {
                if let Some(location) = data["location"].as_str().filter(|s| !s.is_empty()) {
                    s.setup.location = TextEdit::new(location);
                    s.setup.candidate = None;
                }
            }
        }
        _ => {
            s.setup.message = data["message"]
                .as_str()
                .unwrap_or("Request complete")
                .into()
        }
    }
}
pub(crate) fn select_distro(s: &mut State, name: &str) {
    s.setup.distro = TextEdit::new(name);
    s.setup.candidate = None;
    s.setup.review = false;
}
pub(crate) fn use_distro(s: &mut State) {
    let Some(host) = &s.setup.host else {
        s.setup.message = "Scan the computer first.".into();
        return;
    };
    let Some(distro) = host
        .distros
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(s.setup.distro.text.trim()) && d.version == 2)
    else {
        s.setup.message = "Select an installed WSL 2 distribution.".into();
        return;
    };
    let name = distro.name.clone();
    if !profiles::can_reconfigure(s) {
        return;
    }
    let mut next = s.config.clone();
    next.distro = name.clone();
    s.setup.distro = TextEdit::new(name);
    if !s.headless {
        if let Err(e) = backend::save(&next) {
            s.setup.message = e;
            return;
        }
    }
    s.config = next;
    s.detected.clear();
    s.setup.checks.clear();
    s.setup.message =
        "Workspace distribution saved. Detect/reuse tools to configure their actual paths.".into();
    let distro = s.config.distro.clone();
    activity::record(s, format!("Selected WSL environment {distro}"));
}
pub(crate) fn review(s: &mut State) {
    s.setup.review = true;
}
pub(crate) fn retry(s: &mut State) {
    let Some(id) = &s.setup.selected_job else {
        return;
    };
    if let Ok(bytes) = std::fs::read(root().join(id).join("plan.json")) {
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            restore_retry(s, &value);
        }
    }
}
fn restore_retry(s: &mut State, value: &Value) {
    if let Some(index) = s
        .setup
        .recipes
        .iter()
        .position(|r| Some(r.id.as_str()) == value["tool"].as_str())
    {
        choose(s, index);
        s.setup.distro = TextEdit::new(value["distro"].as_str().unwrap_or(""));
        s.setup.location = TextEdit::new(value["location"].as_str().unwrap_or(""));
        s.setup.review = true;
    }
}
pub(crate) fn preview(s: &mut State) {
    s.navigate(crate::ui::Page::Settings);
    s.setup.host = Some(Host {
        build: 26100,
        windows: "Windows 11 · preview fixture".into(),
        architecture: "64-bit".into(),
        eligible: true,
        virtualization: "Available".into(),
        ram_gib: 16.,
        gpu: "Example GPU".into(),
        wsl_ready: true,
        wsl_engine: true,
        disk_gib: 120.,
        distros: vec![Distro {
            name: "Ubuntu-24.04".into(),
            state: "Running".into(),
            version: 2,
            ..Default::default()
        }],
    });
    s.setup.selected = 2;
    s.setup.message = "Preview fixture · installation and system changes disabled".into();
}
