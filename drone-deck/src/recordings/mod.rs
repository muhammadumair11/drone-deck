mod bridge;
#[cfg(test)]
mod tests;
mod view;
use crate::*;
use serde::Deserialize;
use serde_json::{Value, json};
pub(crate) use view::page;

#[derive(Clone, Default, Deserialize)]
pub(crate) struct Topic {
    pub name: String,
    #[serde(rename = "type", default)]
    pub message_type: String,
    #[serde(default)]
    pub count: u64,
}
#[derive(Clone, Default, Deserialize)]
pub(crate) struct Session {
    pub id: String,
    pub label: String,
    pub status: String,
    pub path: String,
    #[serde(default)]
    pub messages: u64,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub topics: Vec<Topic>,
    #[serde(default)]
    pub clock: String,
    #[serde(default)]
    pub source: String,
}
pub(crate) struct Recorder {
    pub name: TextEdit,
    pub source: TextEdit,
    pub topics: TextEdit,
    pub capture_domain: TextEdit,
    pub replay_domain: TextEdit,
    pub rate: TextEdit,
    pub sim_time: bool,
    pub ready: bool,
    pub pending: bool,
    pub recording: bool,
    pub playing: bool,
    pub message: String,
    pub root: String,
    pub discovered: Vec<Topic>,
    pub sessions: Vec<Session>,
    pub selected: Option<String>,
    bridge: Option<bridge::Bridge>,
}
impl Default for Recorder {
    fn default() -> Self {
        Self {
            name: TextEdit::new("flight"),
            source: TextEdit::new("Simulation"),
            topics: TextEdit::new(""),
            capture_domain: TextEdit::new("0"),
            replay_domain: TextEdit::new("77"),
            rate: TextEdit::new("1.0"),
            sim_time: false,
            ready: false,
            pending: false,
            recording: false,
            playing: false,
            message: "Check dependencies, then discover or enter ROS topics.".into(),
            root: "~/.local/share/drone-deck/recordings (inside selected Ubuntu)".into(),
            discovered: Vec::new(),
            sessions: Vec::new(),
            selected: None,
            bridge: None,
        }
    }
}
fn parse_domain(text: &str) -> Result<u16, String> {
    text.trim()
        .parse::<u16>()
        .ok()
        .filter(|n| *n <= 232)
        .ok_or_else(|| "ROS domain must be 0–232.".into())
}
fn topic_names(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
fn payload(s: &State, action: &str) -> Result<Value, String> {
    let r = &s.recorder;
    let capture = if ["record", "topics", "replay"].contains(&action) {
        parse_domain(&r.capture_domain.text)?
    } else {
        0
    };
    let mut value = json!({"action":action,"domain":capture});
    if action == "record" {
        let selected = topic_names(&r.topics.text);
        if selected.is_empty() {
            return Err("Discover and select topics, or enter absolute topic names.".into());
        }
        value = json!({"action":action,"domain":capture,"topics":selected,
            "name":r.name.text.trim(),"source":r.source.text.trim(),"sim_time":r.sim_time});
    } else if action == "replay" {
        let domain = parse_domain(&r.replay_domain.text)?;
        if domain == capture {
            return Err("Use a replay domain different from the capture domain.".into());
        }
        let rate = r
            .rate
            .text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && (0.1..=4.).contains(n))
            .ok_or("Replay speed must be 0.1–4.0.")?;
        let session = r.selected.as_ref().ok_or("Select a recording first.")?;
        value = json!({"action":action,"domain":domain,"capture_domain":capture,"rate":rate,"session":session});
    }
    Ok(value)
}
pub(crate) fn send(s: &mut State, action: &str) {
    if ["record", "replay"].contains(&action)
        && (s.setup.batch.is_some() || s.setup.jobs.iter().any(|j| j.alive))
    {
        s.recorder.message = "Finish active setup before starting recording or replay.".into();
        return;
    }
    if s.headless {
        s.notice = "Preview: recording processes disabled.".into();
        return;
    }
    let request = match payload(s, action) {
        Ok(v) => v,
        Err(e) => {
            s.recorder.message = e;
            return;
        }
    };
    let r = &mut s.recorder;
    if action != "stop"
        && r.bridge
            .as_ref()
            .is_some_and(|b| b.distro != s.config.distro)
    {
        if r.recording || r.playing {
            r.message = "Stop recording/replay before changing the WSL distribution.".into();
            return;
        }
        r.bridge = None;
        r.ready = false;
        r.sessions.clear();
        r.selected = None;
        r.discovered.clear();
    }
    if r.bridge.is_none() {
        match bridge::Bridge::spawn(&s.config.distro) {
            Ok(b) => r.bridge = Some(b),
            Err(e) => {
                r.message = e;
                return;
            }
        }
    }
    match r.bridge.as_mut().unwrap().send(request) {
        Ok(()) => {
            r.pending = true;
            r.message = format!("{} requested…", action);
        }
        Err(e) => {
            r.message = e;
            r.bridge = None;
            r.ready = false;
            r.pending = false;
        }
    }
}
pub(crate) fn toggle_topic(s: &mut State, name: &str) {
    let mut topics = topic_names(&s.recorder.topics.text);
    if topics.iter().any(|t| t == name) {
        topics.retain(|t| t != name);
    } else {
        topics.push(name.into());
    }
    s.recorder.topics = TextEdit::new(&topics.join(" "));
}
pub(crate) fn poll(s: &mut State) -> bool {
    let events: Vec<_> = s
        .recorder
        .bridge
        .as_ref()
        .map(|b| b.events.try_iter().take(150).collect())
        .unwrap_or_default();
    let changed = !events.is_empty();
    for event in events {
        apply_event(s, event);
    }
    changed
}
fn apply_event(s: &mut State, event: Value) {
    let r = &mut s.recorder;
    match event["kind"].as_str().unwrap_or("") {
        "closed" => {
            r.bridge = None;
            r.ready = false;
            r.pending = false;
            r.recording = false;
            r.playing = false;
            r.message = "Recording worker closed. Check dependencies to reconnect.".into();
        }
        "log" => {
            s.log(format!(
                "[recordings] {}",
                event["message"].as_str().unwrap_or("")
            ));
        }
        "activity" => {
            r.recording = event["recording"].as_bool().unwrap_or(false);
            r.playing = event["playing"].as_bool().unwrap_or(false);
            r.message = event["message"].as_str().unwrap_or("").into();
        }
        "result" => {
            r.pending = false;
            let action = event["action"].as_str().unwrap_or("");
            if !event["ok"].as_bool().unwrap_or(false) {
                r.message = event["error"]
                    .as_str()
                    .unwrap_or("Recording request failed")
                    .into();
                if action == "check" {
                    r.ready = false;
                }
                s.log(format!("[recordings] {}", s.recorder.message));
                return;
            }
            let data = &event["data"];
            match action {
                "check" => {
                    r.ready = data["ready"].as_bool().unwrap_or(false);
                    r.root = data["root"].as_str().unwrap_or("").into();
                    r.message="ROS 2 Jazzy + rosbag2 + MCAP ready. Discover topics on the capture domain.".into();
                }
                "topics" => {
                    r.discovered = serde_json::from_value(data.clone()).unwrap_or_default();
                    r.message = format!(
                        "Found {} topics. Select streams to capture.",
                        r.discovered.len()
                    );
                }
                "list" => {
                    r.sessions = serde_json::from_value(data.clone()).unwrap_or_default();
                    if r.selected
                        .as_ref()
                        .is_some_and(|id| !r.sessions.iter().any(|row| &row.id == id))
                    {
                        r.selected = None;
                    }
                }
                "record" => {
                    r.selected = data["id"].as_str().map(str::to_owned);
                }
                "replay" => {
                    r.message = format!(
                        "Replaying on local-only ROS domain {}. Configure analysis nodes to use that domain.",
                        data["domain"]
                    );
                }
                _ => {}
            }
        }
        _ => {}
    }
}
pub(crate) fn open_selected(s: &mut State) {
    if let Some(row) = s
        .recorder
        .sessions
        .iter()
        .find(|r| Some(&r.id) == s.recorder.selected.as_ref())
    {
        s.open(format!(
            "\\\\wsl.localhost\\{}{}",
            s.config.distro,
            row.path.replace('/', "\\")
        ));
    }
}
pub(crate) fn shutdown(s: &mut State) {
    s.recorder.bridge = None;
}
pub(crate) fn preview(s: &mut State) {
    s.navigate(7);
    s.recorder.ready = true;
    s.recorder.message = "Headless preview · example recording · no external processes".into();
    s.recorder.discovered = vec![
        Topic {
            name: "/camera/image_raw".into(),
            message_type: "sensor_msgs/msg/Image".into(),
            count: 0,
        },
        Topic {
            name: "/fmu/out/sensor_combined".into(),
            message_type: "px4_msgs/msg/SensorCombined".into(),
            count: 0,
        },
    ];
    s.recorder.topics = TextEdit::new("/camera/image_raw /fmu/out/sensor_combined");
    s.recorder.sessions.push(Session {
        id: "preview-flight".into(),
        label: "Example sensor capture".into(),
        status: "Finalized".into(),
        messages: 12500,
        bytes: 73400320,
        duration: 62.5,
        clock: "system time".into(),
        source: "Simulation preview".into(),
        ..Default::default()
    });
    s.recorder.selected = Some("preview-flight".into());
}
