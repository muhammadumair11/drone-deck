use crate::*;
use serde_json::{Value, json};
#[cfg(test)]
mod tests;
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
            message: "Choose Physical vehicle to monitor a native Windows connection.".into(),
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
        s.hardware.message =
            "Select Physical vehicle and finish active installations first.".into();
        return;
    }
    let endpoint = s.hardware.endpoint.text.trim().to_string();
    if endpoint.len() > 160 || endpoint.contains(['\n', '\r']) {
        s.hardware.message = "Invalid endpoint.".into();
        return;
    }
    let program = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())
        + "\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
    let script = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("runtime/studio/hardware.ps1");
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
pub(crate) fn page(s: &State) -> El {
    use crate::ui::*;
    let h = &s.hardware;
    let running = s
        .status
        .get("hardware")
        .is_some_and(|v| v == "Running" || v == "Starting");
    let mut modes = row().gap(8.).flex_wrap();
    for (index, name) in ["Simulation", "Physical vehicle", "Replay"]
        .iter()
        .enumerate()
    {
        modes = modes.child(
            button(
                format!("vehicle-mode-{index}"),
                *name,
                h.mode == index,
                move |s| s.hardware.mode = index,
            )
            .disabled(running),
        );
    }
    let mut card = card()
        .key("hardware-workspace")
        .child(heading("Vehicle source", "WINDOWS SERIAL / UDP / TCP"))
        .child(modes);
    if h.mode != 1 {
        return card.child(label(if h.mode==0{"Simulation tools and MAVSDK telemetry are listed below. Physical monitoring uses a separately selected native endpoint."}else{"Replay is available in Recordings on its isolated ROS domain. It does not connect to physical vehicle control."},13.,muted()).wrap());
    }
    card=card.child(label("Listen without sending flight commands. Use a dedicated telemetry endpoint; another app may already own the serial port. A signed packet's signature is not authenticated by this monitor.",13.,muted()).wrap())
 .child(field(&h.endpoint,"hardware-endpoint").on_edit(|s:&mut State|&mut s.hardware.endpoint))
 .child(label("serial:COM4:115200 · udp:127.0.0.1:14555 · tcp:localhost:5760",12.,muted()).wrap())
 .child(row().gap(8.).flex_wrap().child(button("scan-serial","Find Windows serial devices",false,|s|crate::studio::request(s,"ports"))).child(button("monitor-start",if running{"Stop monitor"}else{"Start monitoring"},true,move|s|{if running{s.send(Request::Stop("hardware".into()));}else{start(s);}})))
 .child(label(&h.message,13.,ink()).wrap());
    if let Some(ports) = s.studio.library["ports"].as_array() {
        for port in ports {
            let id = port["DeviceID"].as_str().unwrap_or("").to_string();
            card = card.child(
                button(
                    format!("port-{id}"),
                    &format!("{} · {}", id, port["Name"].as_str().unwrap_or("")),
                    false,
                    move |s| s.hardware.endpoint = TextEdit::new(format!("serial:{id}:115200")),
                )
                .disabled(running),
            );
        }
    }
    for (key, vehicle) in &h.vehicles {
        let selected = key.clone();
        let age = h
            .seen
            .get(key)
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(999);
        card = card
            .child(button(
                format!("vehicle-{key}"),
                &format!(
                    "System {key} · {} · heartbeat {age}s ago",
                    match vehicle["fields"]["autopilot"].as_u64() {
                        Some(12) => "PX4",
                        Some(3) => "ArduPilot",
                        _ => "Other autopilot",
                    }
                ),
                h.selected.as_ref() == Some(key),
                move |s| s.hardware.selected = Some(selected.clone()),
            ))
            .child(
                mono(
                    serde_json::to_string_pretty(&vehicle["fields"]).unwrap_or_default(),
                    12.,
                    ink(),
                )
                .wrap_anywhere(),
            );
    }
    let fresh = h.selected.as_ref().is_some_and(|key| {
        h.seen
            .get(key)
            .is_some_and(|t| t.elapsed() < Duration::from_secs(10))
    });
    card.child(button("read-parameters","Read selected vehicle parameters",true,parameters).disabled(!running||!fresh))
 .child(button("open-hardware-capture","Open local capture / parameter backups",false,|s|s.open(s.hardware.path.clone())).disabled(h.path.is_empty()))
 .child(label("Captures use native MAVLink .tlog files. Parameter backups preserve raw wire values/types; use QGroundControl or Mission Planner for parameter edits and onboard flight-log retrieval.",12.,muted()).wrap())
}
