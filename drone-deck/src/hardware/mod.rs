use crate::*;
use serde_json::{Value, json};
#[cfg(test)]
mod tests;
mod view;
pub(crate) use view::{monitor, toggle};
pub(crate) struct Hardware {
    pub mode: usize,
    pub endpoint: TextEdit,
    pub vehicles: HashMap<String, Value>,
    pub seen: HashMap<String, Instant>,
    pub selected: Option<String>,
    pub message: String,
    pub path: String,
    last_refresh: Instant,
}
impl Default for Hardware {
    fn default() -> Self {
        Self {
            mode: 0,
            endpoint: TextEdit::new("udp:127.0.0.1:14555"),
            vehicles: HashMap::new(),
            seen: HashMap::new(),
            selected: None,
            message: "Enter a dedicated endpoint, then start read-only monitoring.".into(),
            path: String::new(),
            last_refresh: Instant::now(),
        }
    }
}
pub(crate) fn poll(s: &mut State) -> bool {
    if s.status.get("hardware").is_some_and(|v| v == "Running")
        && s.hardware.last_refresh.elapsed() > Duration::from_secs(1)
    {
        s.hardware.last_refresh = Instant::now();
        true
    } else {
        false
    }
}
pub(crate) fn event(s: &mut State, value: Value) {
    let h = &mut s.hardware;
    if let Some(message) = value["message"].as_str() {
        h.message = message.into();
    }
    if let Some(path) = value["path"].as_str() {
        h.path = path.into();
    }
    if value["kind"] == "packet" {
        let key = format!("{}:{}", value["system"], value["component"]);
        if value["message_id"] == 0
            && value["fields"]["type"] != 6
            && value["fields"]["autopilot"] != 8
        {
            if h.vehicles.len() < 32 || h.vehicles.contains_key(&key) {
                h.seen.insert(key.clone(), Instant::now());
                h.vehicles
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        if let Some(vehicle) = h.vehicles.get_mut(&key) {
            if let Some(fields) = value["fields"].as_object() {
                for (name, data) in fields {
                    vehicle["fields"][name] = data.clone();
                }
            }
        }
    }
}
pub(crate) fn start(s: &mut State) {
    if s.headless {
        s.hardware.message = "Preview: native connections disabled.".into();
        return;
    }
    if s.hardware.mode != 1 || s.setup.batch.is_some() || s.setup.jobs.iter().any(|j| j.alive) {
        s.hardware.message = "Finish active installations before monitoring starts.".into();
        return;
    }
    let endpoint = s.hardware.endpoint.text.trim().to_string();
    if endpoint.len() > 160 || endpoint.contains(['\n', '\r']) {
        s.hardware.message = "Invalid endpoint.".into();
        return;
    }
    let program = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())
        + "\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
    let script = crate::backend::scripts().join("studio/hardware.ps1");
    let p = backend::Profile {
        id: "hardware".into(),
        name: "Native vehicle monitor".into(),
        category: "Flight operations".into(),
        description: "Read-only native serial/UDP/TCP".into(),
        program,
        args: vec![
            "-NoProfile".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            script.to_string_lossy().into_owned(),
            "-Endpoint".into(),
            endpoint,
            "-Root".into(),
            backend::root().to_string_lossy().into_owned(),
        ],
        cwd: String::new(),
        probe: String::new(),
        docs: "https://mavlink.io/en/guide/serialization.html".into(),
        download_url: String::new(),
        exclusive: false,
    };
    s.hardware.vehicles.clear();
    s.hardware.seen.clear();
    s.hardware.selected = None;
    s.status.insert("hardware".into(), "Starting".into());
    s.send(Request::Start(p, s.config.distro.clone()));
}
pub(crate) fn parameters(s: &mut State) {
    let Some(key) = s.hardware.selected.as_ref() else {
        return;
    };
    if !s
        .hardware
        .seen
        .get(key)
        .is_some_and(|t| t.elapsed() < Duration::from_secs(10))
    {
        s.hardware.message = "Select a vehicle with a fresh heartbeat.".into();
        return;
    }
    let vehicle = &s.hardware.vehicles[key];
    s.send(Request::Input(
        "hardware".into(),
        json!({"action":"parameters","system":vehicle["system"],"component":vehicle["component"]})
            .to_string(),
    ));
}
