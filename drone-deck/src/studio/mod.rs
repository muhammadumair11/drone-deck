mod bridge;
pub(crate) mod foxglove;
pub(crate) mod pipelines;
mod projects;
#[cfg(test)]
mod tests;
mod view;
use crate::*;
use serde_json::{Value, json};
use std::sync::mpsc::{self, Receiver, Sender};
pub(crate) use view::{page, settings};
pub(crate) struct Studio {
    pub token: TextEdit,
    pub stored: bool,
    pub repo: TextEdit,
    pub revision: TextEdit,
    pub patterns: TextEdit,
    pub project_name: TextEdit,
    pub kind: usize,
    pub model_file: TextEdit,
    pub inputs: TextEdit,
    pub model_path: String,
    pub preview: Option<Value>,
    pub models: Vec<Value>,
    pub library: Value,
    pub jobs: Vec<Value>,
    pub message: String,
    /// The last message of a Hugging Face token action, shown on Settings → Model access (the
    /// shared `message` there would show unrelated Data prompts).
    pub credential_note: String,
    pub pending: bool,
    pub selected_job: Option<String>,
    /// Lazy Data & models loads already requested (`models`, `credential_status`, `data_library`,
    /// `pipe_list`).
    pub loaded: Vec<&'static str>,
    /// Data → Pipelines (extraction, inference on replay, runs).
    pub pipes: pipelines::Pipes,
    tx: Sender<Value>,
    rx: Receiver<Value>,
    last_refresh: Instant,
}
impl Default for Studio {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            token: TextEdit::new("").secure(),
            stored: false,
            repo: TextEdit::new(""),
            revision: TextEdit::new("main"),
            patterns: TextEdit::new("*.json,*.safetensors,*.onnx,*.gguf,*.txt,tokenizer.*"),
            project_name: TextEdit::new("drone-project"),
            kind: 0,
            model_file: TextEdit::new(""),
            inputs: TextEdit::new(""),
            model_path: String::new(),
            preview: None,
            models: Vec::new(),
            library: json!({}),
            jobs: Vec::new(),
            message: "Choose a model or project workflow.".into(),
            credential_note: String::new(),
            pending: false,
            selected_job: None,
            loaded: Vec::new(),
            pipes: pipelines::Pipes::default(),
            tx,
            rx,
            last_refresh: Instant::now(),
        }
    }
}
/// Hugging Face token actions; their messages also go to `Studio::credential_note`.
const CREDENTIAL_ACTIONS: [&str; 4] = [
    "credential_status",
    "save_token",
    "forget_token",
    "validate_token",
];
pub(crate) fn request(s: &mut State, action: &str) {
    let credential = CREDENTIAL_ACTIONS.contains(&action);
    if s.headless {
        s.studio.message = "Preview: external Studio actions disabled.".into();
        if credential {
            s.studio.credential_note = s.studio.message.clone();
        }
        return;
    }
    if s.studio.pending {
        return;
    }
    let linux = ["data_library", "project_create"].contains(&action);
    if linux && !s.wsl_ready() {
        s.notice = "Select a working WSL 2 environment in Settings → Environment first.".into();
        s.navigate(crate::ui::Page::Settings);
        return;
    }
    let r = &mut s.studio;
    let mut value = json!({"action":action});
    match action {
        "save_token" => {
            let token = std::mem::replace(&mut r.token, TextEdit::new("").secure());
            value["token"] = json!(token.text);
        }
        "inspect_model" => {
            r.preview = None;
            value["repo"] = json!(r.repo.text.trim());
            value["revision"] = json!(r.revision.text.trim());
            value["patterns"] = json!(r.patterns.text.trim());
        }
        "start_model" => {
            let Some(preview) = &r.preview else { return };
            value["review"] = preview["review"].clone();
        }
        "retry_model" | "cancel_model" => {
            value["id"] = json!(r.selected_job);
        }
        "verify_model" => {
            let Some(model) = r
                .models
                .iter()
                .find(|m| m["path"].as_str() == Some(&r.model_path))
            else {
                return;
            };
            value["repo"] = model["repo"].clone();
            value["sha"] = model["sha"].clone();
        }
        "project_create" => {
            value["name"] = json!(r.project_name.text.trim());
            value["kind"] = json!(["onnx", "ros2", "mavsdk"][r.kind.min(2)]);
            let mount = s.agent.hello.as_ref().map(|h| h.automount_root.as_str());
            match linux_model_path(&r.model_path, &s.config.distro, mount) {
                Ok(model) => value["model"] = json!(model),
                Err(e) => {
                    r.message = e;
                    return;
                }
            }
        }
        _ => {}
    }
    r.pending = true;
    r.message = format!("{}…", action.replace('_', " "));
    if credential {
        r.credential_note = r.message.clone();
    }
    bridge::request(value, s.config.distro.clone(), linux, r.tx.clone());
}
/// The Linux path of a model path typed or browsed on Windows, inside `distro`: a drive path goes
/// through the distro's drive mount (`automount_root` from the agent's `hello`, `/mnt/` when
/// unknown), and a path on Explorer's `\\wsl.localhost\<distro>\…` or `\\wsl$\<distro>\…` share
/// maps to the same file inside the distro. Anything else is taken as a Linux path already.
pub(crate) fn linux_model_path(
    path: &str,
    distro: &str,
    automount_root: Option<&str>,
) -> Result<String, String> {
    let slashed = path.replace('\\', "/");
    let lower = slashed.to_ascii_lowercase();
    for share in ["//wsl.localhost/", "//wsl$/"] {
        if lower.starts_with(share) {
            let rest = &slashed[share.len()..];
            let (name, inner) = rest.split_once('/').unwrap_or((rest, ""));
            if !name.eq_ignore_ascii_case(distro) {
                return Err(format!(
                    "The model is in the Linux distribution {name}, but Drone Deck uses {distro}. Copy the model into {distro} or choose another file."
                ));
            }
            return Ok(format!("/{inner}"));
        }
    }
    Ok(
        crate::agent::linux_path(std::path::Path::new(path), automount_root.unwrap_or(""))
            .unwrap_or_else(|| path.to_string()),
    )
}
pub(crate) fn poll(s: &mut State) -> bool {
    let r = &mut s.studio;
    let mut changed = false;
    for event in r.rx.try_iter() {
        changed = true;
        if let Some(progress) = event["progress"].as_str() {
            r.message = progress.into();
            continue;
        }
        r.pending = false;
        let credential = event["action"]
            .as_str()
            .is_some_and(|a| CREDENTIAL_ACTIONS.contains(&a));
        if event["ok"].as_bool() != Some(true) {
            r.message = event["error"]
                .as_str()
                .unwrap_or("Studio request failed")
                .into();
            if credential {
                r.credential_note = r.message.clone();
            }
            continue;
        }
        let data = event["data"].clone();
        r.message = data["message"].as_str().unwrap_or("Complete").into();
        if credential {
            r.credential_note = r.message.clone();
        }
        match event["action"].as_str().unwrap_or("") {
            "credential_status" | "save_token" | "forget_token" | "validate_token" => {
                r.stored = data["stored"].as_bool().unwrap_or(r.stored)
            }
            "inspect_model" => r.preview = Some(data),
            "models" => r.models = data["models"].as_array().cloned().unwrap_or_default(),
            "data_library" => r.library = data,
            "ports" => r.library["ports"] = data["ports"].clone(),
            "start_model" | "retry_model" => {
                r.selected_job = data["id"].as_str().map(str::to_owned)
            }
            "project_create" => {
                if !r.library["projects"].is_array() {
                    r.library["projects"] = json!([]);
                }
                r.library["projects"]
                    .as_array_mut()
                    .unwrap()
                    .push(data["project"].clone());
            }
            _ => {}
        }
    }
    if r.last_refresh.elapsed() > Duration::from_secs(1) {
        r.last_refresh = Instant::now();
        let root = backend::root().join("studio/jobs");
        let mut paths: Vec<_> = std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|d| d.path())
            .collect();
        paths.sort();
        paths.reverse();
        let jobs: Vec<Value> = paths
            .into_iter()
            .take(50)
            .filter_map(|path| {
                let mut state: Value =
                    serde_json::from_slice(&std::fs::read(path.join("state.json")).ok()?).ok()?;
                if let Ok(bytes) = std::fs::read(path.join("progress.json")) {
                    if let Ok(progress) = serde_json::from_slice::<Value>(&bytes) {
                        state["progress"] = progress;
                    }
                }
                let alive = crate::setup::job_alive(
                    state["pid"].as_u64().unwrap_or(0) as u32,
                    state["process_ticks"].as_u64().unwrap_or(0),
                );
                if !alive
                    && ["Downloading", "Queued"].contains(&state["status"].as_str().unwrap_or(""))
                    && path
                        .join("state.json")
                        .metadata()
                        .ok()?
                        .modified()
                        .ok()?
                        .elapsed()
                        .ok()?
                        .as_secs()
                        > 20
                {
                    state["status"] = json!("Interrupted");
                }
                state["alive"] = json!(alive);
                Some(state)
            })
            .collect();
        if jobs != r.jobs {
            r.jobs = jobs;
            changed = true;
        }
    }
    changed | pipelines::poll(s) | view::lazy_load(s)
}
