//! Data → Pipelines: frame extraction and inference on replay, run by the WSL agent's `pipe.*`
//! methods (`scripts/agent/pipe.py`). One job runs at a time; its progress and result arrive on
//! a channel that `poll` drains. Outputs stay next to the recording inside WSL.
use crate::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};

/// `pipe.list` reads manifests only.
const LIST_TIMEOUT: Duration = Duration::from_secs(60);
/// Extraction hashes and decodes whole recordings.
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(6 * 3600);
/// An inference run lasts as long as the replay (slowed down to 0.1x at worst).
const INFER_TIMEOUT: Duration = Duration::from_secs(12 * 3600);
/// A cancelled inference run can take the recorder's 20 s grace plus the others' to stop.
const CANCEL_WAIT: Duration = Duration::from_secs(60);
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
const CANCELLED: &str = "Cancelled";
const CANCEL_SLOW: &str = "Cancelled (still stopping)";
pub(crate) const FORMATS: [&str; 2] = ["PNG", "NPZ"];
const FORMAT_IDS: [&str; 2] = ["png", "npz"];
pub(crate) const IMAGE: &str = "sensor_msgs/msg/Image";
pub(crate) const COMPRESSED: &str = "sensor_msgs/msg/CompressedImage";

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct PipeTopic {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}
impl PipeTopic {
    pub(crate) fn image(&self) -> bool {
        self.kind == IMAGE || self.kind == COMPRESSED
    }
}

/// A finalized recording as `pipe.list` reports it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct PipeRecording {
    pub id: String,
    pub label: String,
    pub path: String,
    pub domain: i64,
    pub topics: Vec<PipeTopic>,
    pub mcap: Vec<String>,
}

/// An extraction (`manifest.json`) or inference run (`run.json`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Run {
    pub kind: String,
    pub recording: String,
    pub name: String,
    pub path: String,
    pub status: String,
    pub created: Option<String>,
    pub finished: Option<String>,
    pub count: Option<u64>,
    pub format: Option<String>,
    pub topics: Vec<String>,
    pub model: Option<String>,
    pub topic: Option<String>,
    pub detections: Option<u64>,
    pub mcap: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum JobKind {
    Extract,
    Infer,
}
impl JobKind {
    fn method(self) -> &'static str {
        match self {
            JobKind::Extract => "pipe.extract",
            JobKind::Infer => "pipe.infer",
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            JobKind::Extract => "Frame extraction",
            JobKind::Infer => "Inference on replay",
        }
    }
}

/// The job in flight.
pub(crate) struct Job {
    pub kind: JobKind,
    pub label: String,
    pub progress: String,
    pub started: Instant,
    cancel: Arc<AtomicBool>,
    /// Elapsed whole seconds last drawn in the status line.
    shown_secs: u64,
}
impl Job {
    /// A job in flight with no agent behind it.
    #[cfg(test)]
    pub(crate) fn fake(kind: JobKind) -> Self {
        Self {
            kind,
            label: "camera".into(),
            progress: "Requested…".into(),
            started: Instant::now(),
            cancel: Arc::new(AtomicBool::new(false)),
            shown_secs: 0,
        }
    }
    /// Cancel was asked; the job ends once the agent has stopped it.
    pub(crate) fn cancelling(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

pub(super) enum Msg {
    Listed(Result<Value, String>),
    Progress(String),
    Done(JobKind, Result<Value, String>),
    Notice(String),
}

pub(crate) struct Pipes {
    pub recordings: Vec<PipeRecording>,
    pub runs: Vec<Run>,
    /// A `pipe.list` reply arrived (the lists are real, possibly empty).
    pub listed: bool,
    pub listing: bool,
    pub selected: Option<String>,
    pub topics: TextEdit,
    pub start: TextEdit,
    pub end: TextEdit,
    pub rate: TextEdit,
    pub format: usize,
    pub name: TextEdit,
    pub model: TextEdit,
    pub image_topic: TextEdit,
    pub domain: TextEdit,
    pub speed: TextEdit,
    pub project: TextEdit,
    pub job: Option<Job>,
    pub message: String,
    /// "Live in Foxglove" waits for the bridge to run and its port to accept a connection.
    pub live_pending: Option<super::foxglove::LiveWait>,
    pub(super) tx: Sender<Msg>,
    rx: Receiver<Msg>,
}
impl Default for Pipes {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            recordings: Vec::new(),
            runs: Vec::new(),
            listed: false,
            listing: false,
            selected: None,
            topics: TextEdit::new(""),
            start: TextEdit::new(""),
            end: TextEdit::new(""),
            rate: TextEdit::new(""),
            format: 0,
            name: TextEdit::new(""),
            model: TextEdit::new(""),
            image_topic: TextEdit::new(""),
            domain: TextEdit::new("78"),
            speed: TextEdit::new("1.0"),
            project: TextEdit::new(""),
            job: None,
            message: "Select a finalized recording to extract frames or run a model on its replay."
                .into(),
            live_pending: None,
            tx,
            rx,
        }
    }
}
impl Pipes {
    /// Listed inference runs, newest first (`pipe.list` sorts runs by creation time).
    pub(crate) fn inference_runs(&self) -> impl Iterator<Item = &Run> {
        self.runs.iter().filter(|r| r.kind == "inference")
    }
    pub(crate) fn recording(&self) -> Option<&PipeRecording> {
        let id = self.selected.as_deref()?;
        self.recordings.iter().find(|r| r.id == id)
    }
}

/// Selects a recording and fills the topic fields with its image topics.
pub(crate) fn select(s: &mut State, id: &str) {
    let p = &mut s.studio.pipes;
    p.selected = Some(id.to_string());
    let Some(recording) = p.recording() else {
        return;
    };
    let images: Vec<String> = recording
        .topics
        .iter()
        .filter(|t| t.image())
        .map(|t| t.name.clone())
        .collect();
    p.topics = TextEdit::new(&images.join(" "));
    p.image_topic = TextEdit::new(images.first().map_or("", String::as_str));
}

/// Adds or removes a topic from the extract selection.
pub(crate) fn toggle_topic(s: &mut State, name: &str) {
    let mut topics = topic_list(&s.studio.pipes.topics.text);
    if topics.iter().any(|t| t == name) {
        topics.retain(|t| t != name);
    } else {
        topics.push(name.to_string());
    }
    s.studio.pipes.topics = TextEdit::new(&topics.join(" "));
}

pub(crate) fn topic_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in text.split(|c: char| c.is_whitespace() || c == ',') {
        if !t.is_empty() && !out.iter().any(|o| o == t) {
            out.push(t.to_string());
        }
    }
    out
}

fn optional_number(text: &str, label: &str) -> Result<Option<f64>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    t.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.)
        .map(Some)
        .ok_or_else(|| format!("{label} must be a number of zero or more."))
}

fn valid_name(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 64
        && t.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `pipe.extract` parameters from the form, or the reason they are not usable.
pub(crate) fn extract_params(p: &Pipes) -> Result<Value, String> {
    let recording = p.recording().ok_or("Select a recording first.")?;
    let topics = topic_list(&p.topics.text);
    if topics.is_empty() {
        return Err("Choose at least one topic to extract.".into());
    }
    if let Some(bad) = topics.iter().find(|t| !t.starts_with('/')) {
        return Err(format!(
            "Use absolute topic names: {bad} should start with /."
        ));
    }
    let start = optional_number(&p.start.text, "Start")?;
    let end = optional_number(&p.end.text, "End")?;
    if let (Some(a), Some(b)) = (start, end)
        && b <= a
    {
        return Err("End must be after start.".into());
    }
    let rate = optional_number(&p.rate.text, "Rate")?.filter(|r| *r > 0.);
    let name = p.name.text.trim();
    if !name.is_empty() && !valid_name(name) {
        return Err("Name: use 1–64 letters, numbers, hyphens or underscores.".into());
    }
    let mut value = json!({
        "recording": recording.id,
        "topics": topics,
        "format": FORMAT_IDS[p.format.min(1)],
        "start": start,
        "end": end,
        "rate": rate,
    });
    if !name.is_empty() {
        value["name"] = json!(name);
    }
    Ok(value)
}

/// `pipe.infer` parameters from the form, or the reason they are not usable. A model path from
/// Windows maps into `distro` (see `studio::linux_model_path`).
pub(crate) fn infer_params(
    p: &Pipes,
    distro: &str,
    automount_root: Option<&str>,
) -> Result<Value, String> {
    let recording = p.recording().ok_or("Select a recording first.")?;
    let model = super::linux_model_path(p.model.text.trim(), distro, automount_root)?;
    if !model.to_ascii_lowercase().ends_with(".onnx") {
        return Err("Enter the path of an .onnx model file.".into());
    }
    let topic = p.image_topic.text.trim();
    let Some(found) = recording.topics.iter().find(|t| t.name == topic) else {
        return Err("Choose an image topic of this recording.".into());
    };
    if !found.image() {
        return Err(format!(
            "{topic} is {}; inference needs an Image or CompressedImage topic.",
            found.kind
        ));
    }
    let domain = p
        .domain
        .text
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|d| (0..=232).contains(d))
        .ok_or("Replay domain must be 0–232.")?;
    if domain == recording.domain {
        return Err(format!(
            "Use a replay domain other than {domain}, the domain this recording was captured on."
        ));
    }
    let speed = p
        .speed
        .text
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|r| r.is_finite() && (0.1..=4.).contains(r))
        .ok_or("Replay speed must be 0.1–4.0.")?;
    let project = p.project.text.trim();
    if !project.is_empty() && !valid_name(project) {
        return Err("Project: use the project's name from Projects.".into());
    }
    let mut value = json!({
        "recording": recording.id,
        "model": model,
        "topic": topic,
        "domain": domain,
        "rate": speed,
    });
    if !project.is_empty() {
        value["project"] = json!(project);
    }
    Ok(value)
}

fn ready(s: &mut State) -> bool {
    if s.headless {
        s.studio.pipes.message = "Headless preview · pipelines run nothing.".into();
        return false;
    }
    if !s.wsl_ready() {
        s.studio.pipes.message =
            "Select a working WSL 2 distribution in Settings → Environment first.".into();
        return false;
    }
    true
}

/// Requests `pipe.list`.
pub(crate) fn refresh(s: &mut State) {
    if s.studio.pipes.listing || !ready(s) {
        return;
    }
    let p = &mut s.studio.pipes;
    p.listing = true;
    let (tx, distro) = (p.tx.clone(), s.config.distro.clone());
    std::thread::spawn(move || {
        let reply = match crate::agent::shared(&distro) {
            Some(client) => client
                .call("pipe.list", json!({}), LIST_TIMEOUT)
                .map_err(|e| e.to_string()),
            None => Err("The Linux agent is not available in this mode.".into()),
        };
        let _ = tx.send(Msg::Listed(reply));
    });
}

/// Starts extraction or inference with the form's parameters.
pub(crate) fn start(s: &mut State, kind: JobKind) {
    if s.studio.pipes.job.is_some() {
        s.studio.pipes.message = "A pipeline job is already running. Cancel it first.".into();
        return;
    }
    let params = match kind {
        JobKind::Extract => extract_params(&s.studio.pipes),
        JobKind::Infer => infer_params(
            &s.studio.pipes,
            &s.config.distro,
            s.agent.hello.as_ref().map(|h| h.automount_root.as_str()),
        ),
    };
    let params = match params {
        Ok(v) => v,
        Err(e) => {
            s.studio.pipes.message = e;
            return;
        }
    };
    if !ready(s) {
        return;
    }
    let label = s
        .studio
        .pipes
        .recording()
        .map_or_else(String::new, |r| r.label.clone());
    let cancel = Arc::new(AtomicBool::new(false));
    let p = &mut s.studio.pipes;
    p.job = Some(Job {
        kind,
        label: label.clone(),
        progress: "Requested…".into(),
        started: Instant::now(),
        cancel: cancel.clone(),
        shown_secs: 0,
    });
    p.message = format!("{} started for {label}.", kind.name());
    let (tx, distro) = (p.tx.clone(), s.config.distro.clone());
    std::thread::spawn(move || run_job(kind, params, distro, cancel, tx));
    activity::record(s, format!("Started {} for recording {label}", kind.name()));
}

fn run_job(kind: JobKind, params: Value, distro: String, cancel: Arc<AtomicBool>, tx: Sender<Msg>) {
    let Some(client) = crate::agent::shared(&distro) else {
        let _ = tx.send(Msg::Done(
            kind,
            Err("The Linux agent is not available in this mode.".into()),
        ));
        return;
    };
    let timeout = match kind {
        JobKind::Extract => EXTRACT_TIMEOUT,
        JobKind::Infer => INFER_TIMEOUT,
    };
    let (progress, messages) = mpsc::channel();
    let mut ticket = client.request_with_progress(kind.method(), params, timeout, Some(progress));
    loop {
        if let Ok(message) = messages.recv_timeout(Duration::from_millis(100)) {
            let _ = tx.send(Msg::Progress(message));
        }
        if cancel.load(Ordering::SeqCst) {
            // The agent stops the work (and the run's processes) and keeps partial output. The
            // job stays visible until the agent says it is no longer writing, so the runs list
            // refreshed after it never shows a stopping run as Running.
            ticket.cancel();
            let _ = tx.send(Msg::Progress(
                "Cancelling: waiting for the agent to stop the run's processes…".into(),
            ));
            let settled = wait_settled(&client, CANCEL_WAIT);
            let text = if settled { CANCELLED } else { CANCEL_SLOW };
            let _ = tx.send(Msg::Done(kind, Err(text.into())));
            return;
        }
        if let Some(reply) = ticket.try_take() {
            for message in messages.try_iter() {
                let _ = tx.send(Msg::Progress(message));
            }
            let _ = tx.send(Msg::Done(kind, reply.map_err(|e| e.to_string())));
            return;
        }
    }
}

/// Polls `pipe.status` until the agent writes no extract or inference folder, up to `limit`.
/// False when it still does, or the agent cannot say (gone, or too old for `pipe.status`).
fn wait_settled(client: &crate::agent::AgentClient, limit: Duration) -> bool {
    let until = Instant::now() + limit;
    loop {
        match client.call("pipe.status", json!({}), STATUS_TIMEOUT) {
            Ok(v) if v["active"].as_array().is_some_and(Vec::is_empty) => return true,
            Ok(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(500)),
            _ => return false,
        }
    }
}

/// Asks the running job to stop.
pub(crate) fn cancel(s: &mut State) {
    let Some(job) = &s.studio.pipes.job else {
        return;
    };
    if job.cancel.swap(true, Ordering::SeqCst) {
        return;
    }
    let name = job.kind.name();
    s.studio.pipes.message = format!("Cancelling {name}…");
    activity::record(s, format!("Cancelled {name}"));
}

fn summary(kind: JobKind, v: &Value) -> String {
    let status = v["status"].as_str().unwrap_or("Finished");
    match kind {
        JobKind::Extract => format!(
            "Extraction {}: {} file(s) from {} message(s) read into {}.",
            status.to_lowercase(),
            v["count"].as_u64().unwrap_or(0),
            v["messages_read"].as_u64().unwrap_or(0),
            v["path"]
                .as_str()
                .unwrap_or("the recording's extract folder")
        ),
        JobKind::Infer => match v["detections"].as_u64() {
            Some(n) => format!(
                "Inference run {}: {n} detection message(s) recorded.",
                status.to_lowercase()
            ),
            None => format!(
                "Inference run {}: no detection count in the run record; inspect its bag.",
                status.to_lowercase()
            ),
        },
    }
}

/// The rows of a `pipe.list` array that parse; one malformed manifest drops only itself.
fn rows<T: serde::de::DeserializeOwned>(value: &Value) -> Vec<T> {
    value
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| T::deserialize(row).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the job's seconds counter is drawn: only the Data → Pipelines status line shows it.
pub(crate) fn counter_visible(s: &State) -> bool {
    s.page == crate::ui::Page::Data && s.ui.data_tab == 2
}

/// Applies replies and progress; true when visible state changed.
pub(crate) fn poll(s: &mut State) -> bool {
    let messages: Vec<Msg> = s.studio.pipes.rx.try_iter().take(200).collect();
    let mut changed = !messages.is_empty();
    for message in messages {
        let p = &mut s.studio.pipes;
        match message {
            Msg::Listed(Ok(value)) => {
                p.listing = false;
                p.listed = true;
                p.recordings = rows(&value["recordings"]);
                p.runs = rows(&value["runs"]);
                if p.selected.is_some() && p.recording().is_none() {
                    p.selected = None;
                }
            }
            Msg::Listed(Err(e)) => {
                p.listing = false;
                p.message = format!("Could not list recordings: {e}");
            }
            Msg::Progress(text) => {
                if let Some(job) = &mut p.job {
                    job.progress = text;
                }
            }
            Msg::Done(kind, result) => {
                p.job = None;
                p.message = match &result {
                    Ok(v) => summary(kind, v),
                    Err(e) if e == CANCELLED => format!(
                        "{} cancelled. Its processes have stopped; partial output is kept and marked Cancelled.",
                        kind.name()
                    ),
                    Err(e) if e == CANCEL_SLOW => format!(
                        "{} cancelled, but the agent had not finished stopping it after {}s. Partial output is kept; Reload recordings to see its final status.",
                        kind.name(),
                        CANCEL_WAIT.as_secs()
                    ),
                    Err(e) => format!("{} failed: {e}", kind.name()),
                };
                let text = p.message.clone();
                s.log(format!("[pipelines] {text}"));
                if s.wsl_ready() && !s.headless {
                    refresh(s);
                }
            }
            Msg::Notice(text) => {
                p.message = text.clone();
                s.log(format!("[foxglove] {text}"));
                s.notice = text;
            }
        }
    }
    // The status line counts a running job's seconds: one redraw per second, only while it runs
    // and its tab is on screen. Navigating there rebuilds the view, so the count is current.
    let visible = counter_visible(s);
    if let Some(job) = &mut s.studio.pipes.job {
        let secs = job.started.elapsed().as_secs();
        if visible && secs != job.shown_secs {
            job.shown_secs = secs;
            changed = true;
        }
    }
    changed |= super::foxglove::poll(s);
    changed
}

/// Headless preview: an example recording with one extraction and one inference run.
pub(crate) fn preview(s: &mut State) {
    s.ui.data_tab = 2;
    s.navigate(crate::ui::Page::Data);
    let root = "/home/pilot/.local/share/drone-deck/recordings/20261110T091500Z_camera_4f1a2b3c";
    let p = &mut s.studio.pipes;
    p.listed = true;
    p.recordings = vec![PipeRecording {
        id: "20261110T091500Z_camera_4f1a2b3c".into(),
        label: "camera".into(),
        path: root.into(),
        domain: 0,
        topics: vec![
            PipeTopic {
                name: "/camera".into(),
                kind: IMAGE.into(),
            },
            PipeTopic {
                name: "/camera_info".into(),
                kind: "sensor_msgs/msg/CameraInfo".into(),
            },
            PipeTopic {
                name: "/fmu/out/vehicle_odometry".into(),
                kind: "px4_msgs/msg/VehicleOdometry".into(),
            },
        ],
        mcap: vec![format!("{root}/bag/bag_0.mcap")],
    }];
    p.runs = vec![
        Run {
            kind: "inference".into(),
            recording: p.recordings[0].id.clone(),
            name: "20261110T093002Z_9c0d1e2f".into(),
            path: format!("{root}/inference/20261110T093002Z_9c0d1e2f"),
            status: "Completed".into(),
            created: Some("2026-11-10T09:30:02+00:00".into()),
            model: Some("/home/pilot/models/detector.onnx".into()),
            topic: Some("/camera".into()),
            detections: Some(612),
            mcap: vec![format!(
                "{root}/inference/20261110T093002Z_9c0d1e2f/bag/bag_0.mcap"
            )],
            ..Default::default()
        },
        Run {
            kind: "extract".into(),
            recording: p.recordings[0].id.clone(),
            name: "frames-2hz".into(),
            path: format!("{root}/extract/frames-2hz"),
            status: "Completed".into(),
            created: Some("2026-11-10T09:21:40+00:00".into()),
            count: Some(124),
            format: Some("png".into()),
            topics: vec!["/camera".into()],
            ..Default::default()
        },
    ];
    select(s, "20261110T091500Z_camera_4f1a2b3c");
    let p = &mut s.studio.pipes;
    p.rate = TextEdit::new("2");
    p.model = TextEdit::new("/home/pilot/models/detector.onnx");
    p.message = "Headless preview · example recording · no external processes".into();
}

#[cfg(test)]
#[path = "pipelines_tests.rs"]
mod tests;
